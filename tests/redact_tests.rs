//! The Redact tool: a solid box that keeps nothing of what is under it.

use screen_mark::config::Config;
use screen_mark::export::{crop_and_flatten, flatten_annotations};
use screen_mark::geometry::{Color, Point, Size};
use screen_mark::keys::{KeyId, TOOL_LABELS};
use screen_mark::overlay::toolbar::{ButtonAction, BUTTONS};
use screen_mark::tools::redact::RedactTool;
use screen_mark::tools::{
    annotation_bounding_box, hit_test_annotation, recolor_annotation, Annotation, AnnotationTool,
    ToolKind, TOOL_ORDER,
};

const W: u32 = 64;
const H: u32 = 48;

/// A capture where every pixel is different, seeded so two captures can
/// differ in a chosen area only.
fn noise(seed: u32) -> Vec<u8> {
    let mut state = seed.wrapping_mul(2654435761).max(1);
    let mut out = Vec::with_capacity((W * H * 4) as usize);
    for _ in 0..W * H {
        // xorshift32
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        out.extend_from_slice(&[state as u8, (state >> 8) as u8, (state >> 16) as u8, 255]);
    }
    out
}

/// `base` with the pixels inside (x0..x1, y0..y1) taken from `secret`.
fn with_secret(base: &[u8], secret: &[u8], x0: u32, y0: u32, x1: u32, y1: u32) -> Vec<u8> {
    let mut out = base.to_vec();
    for y in y0..y1 {
        for x in x0..x1 {
            let i = ((y * W + x) * 4) as usize;
            out[i..i + 4].copy_from_slice(&secret[i..i + 4]);
        }
    }
    out
}

fn redact(x: f32, y: f32, w: f32, h: f32) -> Annotation {
    Annotation::Redact {
        top_left: Point::new(x, y),
        size: Size::new(w, h),
        color: Color::new(0.0, 0.0, 0.0, 1.0),
    }
}

fn pixel(image: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * W + x) * 4) as usize;
    [image[i], image[i + 1], image[i + 2], image[i + 3]]
}

#[test]
fn dragging_makes_a_black_box() {
    let mut tool = RedactTool::new();
    tool.on_mouse_down(Point::new(40.0, 30.0));
    tool.on_mouse_move(Point::new(10.0, 10.0));
    assert!(tool.is_drawing());
    assert!(tool.in_progress_annotation().is_some());
    let Some(Annotation::Redact {
        top_left,
        size,
        color,
    }) = tool.on_mouse_up(Point::new(10.0, 10.0))
    else {
        panic!("expected a Redact annotation");
    };
    assert_eq!(
        (top_left, size),
        (Point::new(10.0, 10.0), Size::new(30.0, 20.0))
    );
    assert_eq!(color, Color::new(0.0, 0.0, 0.0, 1.0));

    // A click is not a box.
    tool.on_mouse_down(Point::new(5.0, 5.0));
    assert!(tool.on_mouse_up(Point::new(6.0, 5.0)).is_none());
}

#[test]
fn every_pixel_the_box_touches_is_replaced() {
    let source = noise(1);
    // Edges that fall inside pixels: the box covers 10.4..30.6 by 8.5..20.2,
    // so pixels 10..=30 by 8..=20 must all go.
    let out = flatten_annotations(&source, W, H, &[redact(10.4, 8.5, 20.2, 11.7)]);
    for y in 0..H {
        for x in 0..W {
            let inside = (10..=30).contains(&x) && (8..=20).contains(&y);
            let expected = if inside {
                [0, 0, 0, 255]
            } else {
                pixel(&source, x, y)
            };
            assert_eq!(pixel(&out, x, y), expected, "pixel ({x}, {y})");
        }
    }
}

#[test]
fn nothing_under_the_box_reaches_the_export_whatever_is_drawn_with_it() {
    // Two captures that differ only inside the box. If any export differs,
    // something of the hidden area leaked into it.
    let base = noise(7);
    let a = with_secret(&base, &noise(100), 20, 12, 44, 30);
    let b = with_secret(&base, &noise(200), 20, 12, 44, 30);
    assert_ne!(a, b);

    let the_box = redact(20.0, 12.0, 24.0, 18.0);
    // A Pixelate that straddles the box: partly over it, partly beside it.
    let pixelate = Annotation::Pixelate {
        top_left: Point::new(8.0, 6.0),
        size: Size::new(30.0, 30.0),
        block_size: 7,
    };
    let spotlight = Annotation::Spotlight {
        top_left: Point::new(16.0, 8.0),
        size: Size::new(20.0, 20.0),
    };
    let highlight = Annotation::Highlight {
        top_left: Point::new(0.0, 0.0),
        size: Size::new(W as f32, H as f32),
        color: Color::yellow(),
    };
    let lists: [Vec<Annotation>; 5] = [
        vec![the_box.clone()],
        vec![the_box.clone(), pixelate.clone()],
        vec![pixelate.clone(), the_box.clone()],
        vec![spotlight.clone(), the_box.clone(), pixelate.clone()],
        vec![the_box.clone(), highlight, pixelate, spotlight],
    ];
    for (i, list) in lists.iter().enumerate() {
        let out_a = flatten_annotations(&a, W, H, list);
        let out_b = flatten_annotations(&b, W, H, list);
        assert!(
            out_a == out_b,
            "annotation list {i} leaks the redacted area"
        );
        // And a crop that cuts through the box.
        let crop_a = crop_and_flatten(&a, W, 14, 10, 24, 16, list);
        let crop_b = crop_and_flatten(&b, W, 14, 10, 24, 16, list);
        assert!(crop_a == crop_b, "annotation list {i} leaks when cropped");
    }
}

#[test]
fn without_the_box_pixelate_does_show_what_is_under_it() {
    // The control for the test above: the leak is real without a Redact.
    let base = noise(7);
    let a = with_secret(&base, &noise(100), 20, 12, 44, 30);
    let b = with_secret(&base, &noise(200), 20, 12, 44, 30);
    let pixelate = [Annotation::Pixelate {
        top_left: Point::new(8.0, 6.0),
        size: Size::new(30.0, 30.0),
        block_size: 7,
    }];
    assert_ne!(
        flatten_annotations(&a, W, H, &pixelate),
        flatten_annotations(&b, W, H, &pixelate)
    );
}

#[test]
fn a_box_is_picked_by_its_inside_and_can_be_recoloured() {
    let mut the_box = redact(10.0, 10.0, 30.0, 20.0);
    assert!(hit_test_annotation(&the_box, &Point::new(25.0, 20.0), 8.0));
    assert!(!hit_test_annotation(&the_box, &Point::new(60.0, 40.0), 8.0));
    assert_eq!(
        annotation_bounding_box(&the_box),
        Some((10.0, 10.0, 30.0, 20.0))
    );
    recolor_annotation(&mut the_box, Color::red());
    let out = flatten_annotations(&noise(3), W, H, &[the_box]);
    let red = Color::red();
    let to_u8 = |c: f32| (c * 255.0 + 0.5) as u8;
    assert_eq!(
        pixel(&out, 20, 20),
        [to_u8(red.r), to_u8(red.g), to_u8(red.b), 255]
    );
}

#[test]
fn the_tool_lists_agree_with_each_other() {
    // The toolbar, the key bindings, the config and the tools are indexed
    // alike; a new tool has to be added to all of them.
    for (i, kind) in TOOL_ORDER.iter().enumerate() {
        assert_eq!(
            BUTTONS[i].action,
            ButtonAction::Tool(*kind),
            "toolbar button {i}"
        );
    }
    let tool_buttons = BUTTONS
        .iter()
        .filter(|b| matches!(b.action, ButtonAction::Tool(_)))
        .count();
    assert_eq!(tool_buttons, TOOL_ORDER.len());
    assert_eq!(TOOL_LABELS.len(), TOOL_ORDER.len());

    let config = Config::default();
    assert_eq!(config.shortcuts.entries().len(), TOOL_ORDER.len());
    assert_eq!(config.toolbar.entries().len(), TOOL_ORDER.len());
    assert_eq!(config.toolbar.visible_button_indices().len(), BUTTONS.len());

    // Every tool has a default key and none of them collide.
    let report = config.key_report();
    assert!(
        report.is_ok(),
        "{:?} {:?}",
        report.invalid,
        report.conflicts
    );
    for i in 0..TOOL_ORDER.len() {
        assert!(
            report.binding(KeyId::Tool(i)).is_some(),
            "tool {i} has no key"
        );
    }
    assert_eq!(TOOL_ORDER[14], ToolKind::Redact);
    assert_eq!(config.binding(KeyId::Tool(14)), "x");
}

#[test]
fn hiding_redact_removes_only_its_button() {
    let mut config = Config::default();
    config.toolbar.redact = false;
    let visible = config.toolbar.visible_button_indices();
    assert_eq!(visible.len(), BUTTONS.len() - 1);
    assert!(!visible.contains(&14));
    // A config written before Redact existed shows it and binds X.
    let old: Config =
        toml::from_str("[toolbar]\nmeasurement = false\n[shortcuts]\nselect = \"v\"\n")
            .expect("an old config still parses");
    assert!(old.toolbar.redact);
    assert_eq!(old.shortcuts.redact, "x");
}
