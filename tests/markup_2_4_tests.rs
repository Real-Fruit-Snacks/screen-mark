//! Shape styles (outline, dashed, filled), marking up an image that is
//! already there, and what the overlay tells a screen reader. Driven like
//! the other overlay tests: no window or display.

use screen_mark::access::{Request, Role};
use screen_mark::capture::CapturedScreen;
use screen_mark::config::Config;
use screen_mark::editor;
use screen_mark::export::flatten_annotations;
use screen_mark::geometry::{Color, Point, Size};
use screen_mark::history::Editable;
use screen_mark::keys::{KeyMap, Mods, Named};
use screen_mark::overlay::input::{KeyInput, Outcome};
use screen_mark::overlay::selection::Selection;
use screen_mark::overlay::toolbar::{ButtonAction, BUTTONS};
use screen_mark::state::OverlayState;
use screen_mark::tools::{hit_test_annotation, scale_annotation, Annotation, ShapeStyle, ToolKind};

const W: u32 = 800;
const H: u32 = 600;

fn grey(w: u32, h: u32, v: u8) -> Vec<u8> {
    let mut p = Vec::with_capacity((w * h * 4) as usize);
    for _ in 0..w * h {
        p.extend_from_slice(&[v, v, v, 255]);
    }
    p
}

fn overlay_with(config: &Config) -> OverlayState {
    OverlayState::new(
        CapturedScreen {
            pixels: grey(W, H, 128),
            width: W,
            height: H,
            x_offset: 0,
            y_offset: 0,
            scale_factor: 1.0,
        },
        config,
    )
}

fn keymap() -> KeyMap {
    KeyMap::from_report(&Config::default().key_report())
}

fn drag(o: &mut OverlayState, from: (f32, f32), to: (f32, f32)) -> Outcome {
    o.on_cursor_moved(Point::new(from.0, from.1));
    o.on_mouse_down();
    o.on_cursor_moved(Point::new(to.0, to.1));
    o.on_mouse_up()
}

fn with_region() -> OverlayState {
    let mut o = overlay_with(&Config::default());
    drag(&mut o, (100.0, 100.0), (500.0, 400.0));
    o
}

fn press_key(o: &mut OverlayState, s: &str) -> Outcome {
    o.on_key(&KeyInput::typed(s), &keymap())
}

fn rect(style: ShapeStyle) -> Annotation {
    Annotation::Rectangle {
        top_left: Point::new(10.0, 10.0),
        size: Size::new(40.0, 30.0),
        color: Color::red(),
        thickness: 2.0,
        style,
    }
}

fn pixel(p: &[u8], w: u32, x: u32, y: u32) -> [u8; 3] {
    let i = ((y * w + x) * 4) as usize;
    [p[i], p[i + 1], p[i + 2]]
}

// ───────────── shape styles ─────────────

#[test]
fn a_filled_shape_covers_its_inside_and_a_dashed_one_has_gaps() {
    let base = grey(60, 50, 128);
    let outline = flatten_annotations(&base, 60, 50, &[rect(ShapeStyle::Outline)]);
    let filled = flatten_annotations(&base, 60, 50, &[rect(ShapeStyle::Filled)]);
    let dashed = flatten_annotations(&base, 60, 50, &[rect(ShapeStyle::Dashed)]);
    assert_eq!(
        pixel(&outline, 60, 30, 25),
        [128, 128, 128],
        "outline: inside untouched"
    );
    assert_ne!(
        pixel(&filled, 60, 30, 25),
        [128, 128, 128],
        "filled: inside painted"
    );
    // Along the top edge, the outline is solid; the dashed one has gaps.
    let along = |p: &[u8]| {
        (12..48)
            .filter(|x| pixel(p, 60, *x, 10) != [128, 128, 128])
            .count()
    };
    assert_eq!(along(&outline), 36);
    let dashes = along(&dashed);
    assert!(
        dashes > 10 && dashes < 32,
        "dashed edge covers {dashes} of 36 pixels"
    );
}

#[test]
fn a_filled_shape_can_be_picked_up_from_its_middle() {
    let middle = Point::new(30.0, 25.0);
    assert!(!hit_test_annotation(
        &rect(ShapeStyle::Outline),
        &middle,
        4.0
    ));
    assert!(hit_test_annotation(&rect(ShapeStyle::Filled), &middle, 4.0));
    let ellipse = |style| Annotation::Ellipse {
        center: Point::new(50.0, 50.0),
        radius_x: 30.0,
        radius_y: 20.0,
        color: Color::red(),
        thickness: 2.0,
        style,
    };
    assert!(!hit_test_annotation(
        &ellipse(ShapeStyle::Dashed),
        &Point::new(50.0, 50.0),
        4.0
    ));
    assert!(hit_test_annotation(
        &ellipse(ShapeStyle::Filled),
        &Point::new(50.0, 50.0),
        4.0
    ));
}

#[test]
fn d_switches_the_style_new_shapes_are_drawn_with() {
    let mut o = with_region();
    o.active_tool = ToolKind::Rectangle;
    assert_eq!(o.shape_style, ShapeStyle::Outline);
    assert_eq!(press_key(&mut o, "d"), Outcome::Redraw);
    assert_eq!(o.shape_style, ShapeStyle::Dashed);
    press_key(&mut o, "d");
    assert_eq!(o.shape_style, ShapeStyle::Filled);
    drag(&mut o, (150.0, 150.0), (250.0, 220.0));
    assert!(matches!(
        o.annotations.last(),
        Some(Annotation::Rectangle {
            style: ShapeStyle::Filled,
            ..
        })
    ));
    // A line can't be filled: it is drawn solid, and from dashed the
    // cycle goes back to outline.
    o.active_tool = ToolKind::Line;
    drag(&mut o, (150.0, 300.0), (300.0, 300.0));
    assert!(matches!(
        o.annotations.last(),
        Some(Annotation::Line {
            style: ShapeStyle::Outline,
            ..
        })
    ));
    o.shape_style = ShapeStyle::Dashed;
    press_key(&mut o, "d");
    assert_eq!(o.shape_style, ShapeStyle::Outline);
}

#[test]
fn the_style_button_restyles_the_selected_shape_as_one_undo_step() {
    let mut o = with_region();
    o.active_tool = ToolKind::Circle;
    drag(&mut o, (150.0, 150.0), (250.0, 250.0));
    o.active_tool = ToolKind::Select;
    o.selected_index = Some(0);
    let style = BUTTONS
        .iter()
        .position(|b| b.action == ButtonAction::Style)
        .unwrap();
    assert_eq!(o.press_button(style), Outcome::Redraw);
    assert!(matches!(
        o.annotations[0],
        Annotation::Ellipse {
            style: ShapeStyle::Dashed,
            ..
        }
    ));
    assert_eq!(o.shape_style, ShapeStyle::Outline, "new shapes keep theirs");
    o.on_key(&KeyInput::typed("z").with(Mods::CTRL), &keymap());
    assert!(matches!(
        o.annotations[0],
        Annotation::Ellipse {
            style: ShapeStyle::Outline,
            ..
        }
    ));
}

#[test]
fn shape_style_comes_from_the_config() {
    let mut config = Config::default();
    config.general.shape_style = "filled".into();
    assert_eq!(overlay_with(&config).shape_style, ShapeStyle::Filled);
    config.general.shape_style = "nonsense".into();
    assert_eq!(overlay_with(&config).shape_style, ShapeStyle::Outline);
}

#[test]
fn annotations_saved_before_styles_existed_still_load() {
    let json = r#"{"kind":"rectangle","top_left":{"x":1.0,"y":2.0},"size":{"width":3.0,"height":4.0},"color":{"r":1.0,"g":0.0,"b":0.0,"a":1.0},"thickness":2.0}"#;
    let a: Annotation = serde_json::from_str(json).unwrap();
    assert!(matches!(
        a,
        Annotation::Rectangle {
            style: ShapeStyle::Outline,
            ..
        }
    ));
}

#[test]
fn the_send_button_shows_only_with_a_command() {
    let send = BUTTONS
        .iter()
        .position(|b| b.action == ButtonAction::Send)
        .unwrap();
    assert!(!overlay_with(&Config::default())
        .visible_buttons
        .contains(&send));
    let mut config = Config::default();
    config.send.command = "upload.sh".into();
    let mut o = overlay_with(&config);
    assert!(o.visible_buttons.contains(&send));
    drag(&mut o, (100.0, 100.0), (500.0, 400.0));
    assert_eq!(o.press_button(send), Outcome::Send);
}

// ───────────── marking up an image ─────────────

fn image(w: u32, h: u32) -> Editable {
    Editable {
        pixels: grey(w, h, 30),
        width: w,
        height: h,
        annotations: vec![Annotation::Arrow {
            start: Point::new(10.0, 10.0),
            end: Point::new(50.0, 30.0),
            color: Color::red(),
            thickness: 4.0,
            style: ShapeStyle::Dashed,
        }],
    }
}

#[test]
fn an_image_goes_in_the_middle_of_the_screen_at_its_own_size() {
    let layout = editor::lay_out(&image(200, 100), W, H, (90, 90, 90));
    assert_eq!((layout.width, layout.height), (W, H));
    assert_eq!(layout.scale, 1.0);
    assert_eq!(
        layout.canvas,
        Selection {
            x: 300.0,
            y: 250.0,
            width: 200.0,
            height: 100.0
        }
    );
    assert_eq!(pixel(&layout.pixels, W, 0, 0), [90, 90, 90], "backdrop");
    assert_eq!(pixel(&layout.pixels, W, 300, 250), [30, 30, 30], "image");
    assert!(matches!(
        layout.annotations[0],
        Annotation::Arrow { start, .. } if start == Point::new(310.0, 260.0)
    ));
}

#[test]
fn an_image_bigger_than_the_screen_is_scaled_to_fit_with_its_annotations() {
    let layout = editor::lay_out(&image(1600, 600), W, H, (90, 90, 90));
    assert_eq!(layout.scale, 0.5);
    assert_eq!(layout.canvas.width, 800.0);
    assert_eq!(layout.canvas.height, 300.0);
    match &layout.annotations[0] {
        Annotation::Arrow {
            start, thickness, ..
        } => {
            assert_eq!(*start, Point::new(5.0, 155.0));
            assert_eq!(*thickness, 2.0);
        }
        a => panic!("{a:?}"),
    }
}

#[test]
fn a_transparent_image_shows_the_backdrop_through_it() {
    let mut img = image(10, 10);
    img.pixels = [255, 0, 0, 0].repeat(100);
    let layout = editor::lay_out(&img, 20, 20, (90, 90, 90));
    assert_eq!(pixel(&layout.pixels, 20, 10, 10), [90, 90, 90]);
}

fn editing() -> OverlayState {
    let layout = editor::lay_out(&image(200, 100), W, H, (90, 90, 90));
    let mut o = OverlayState::new(
        CapturedScreen {
            pixels: layout.pixels,
            width: W,
            height: H,
            x_offset: 0,
            y_offset: 0,
            scale_factor: 1.0,
        },
        &Config::default(),
    );
    o.canvas = Some(layout.canvas);
    o.selection = Some(layout.canvas);
    o.annotations = layout.annotations;
    o
}

#[test]
fn the_selection_stays_on_the_image_so_shrinking_it_crops() {
    let mut o = editing();
    // A new region dragged partly off the image is cut to it.
    drag(&mut o, (250.0, 200.0), (400.0, 300.0));
    assert_eq!(
        o.selection,
        Some(Selection {
            x: 300.0,
            y: 250.0,
            width: 100.0,
            height: 50.0
        })
    );
    // A click beside the image selects all of it again.
    o.on_cursor_moved(Point::new(50.0, 50.0));
    o.on_mouse_down();
    o.on_mouse_up();
    assert_eq!(o.selection, o.canvas);
    // So does Ctrl+A, rather than the backdrop around it.
    o.on_key(&KeyInput::typed("a").with(Mods::CTRL), &keymap());
    assert_eq!(o.selection, o.canvas);
    // Moving the region with the arrow keys can't take it off the image.
    o.selection = Some(Selection {
        x: 320.0,
        y: 270.0,
        width: 40.0,
        height: 30.0,
    });
    for _ in 0..200 {
        o.on_key(&KeyInput::named(Named::Right), &keymap());
    }
    let sel = o.selection.unwrap();
    assert!(sel.x + sel.width <= 500.0, "{sel:?}");
}

#[test]
fn history_keeps_the_clean_image_and_annotations_relative_to_it() {
    let o = editing();
    let kept = o.editable_selection().expect("there are annotations");
    assert_eq!((kept.width, kept.height), (200, 100));
    assert_eq!(
        kept.pixels,
        grey(200, 100, 30),
        "without the arrow drawn in"
    );
    assert_eq!(kept.annotations, image(200, 100).annotations);
    // Nothing drawn: nothing to keep.
    let mut plain = editing();
    plain.annotations.clear();
    assert!(plain.editable_selection().is_none());
}

#[test]
fn scaling_an_annotation_scales_its_size_and_line_together() {
    let mut a = Annotation::Text {
        position: Point::new(100.0, 40.0),
        text: "hi".into(),
        color: Color::red(),
        font_size: 20.0,
    };
    scale_annotation(&mut a, 0.5);
    assert!(matches!(a, Annotation::Text { position, font_size, .. }
        if position == Point::new(50.0, 20.0) && font_size == 10.0));
}

#[test]
fn files_that_arent_images_are_refused_in_words() {
    let dir = std::env::temp_dir().join(format!("sm-editor-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let notes = dir.join("notes.png");
    std::fs::write(&notes, b"not an image at all").unwrap();
    let e = editor::open_file(&notes).unwrap_err();
    assert!(e.contains("notes.png"), "{e}");
    let e = editor::open_file(&dir.join("missing.png")).unwrap_err();
    assert!(e.contains("no such file"), "{e}");
    let ok = dir.join("ok.png");
    image::RgbaImage::from_pixel(7, 5, image::Rgba([1, 2, 3, 255]))
        .save(&ok)
        .unwrap();
    let opened = editor::open_file(&ok).unwrap();
    assert_eq!((opened.width, opened.height), (7, 5));
    assert_eq!(
        editor::suggested_name(&ok).as_deref(),
        Some("ok (marked up)")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ───────────── the overlay and a screen reader ─────────────

#[test]
fn before_a_region_the_overlay_says_how_to_start() {
    let o = overlay_with(&Config::default());
    let snap = o.snapshot();
    assert_eq!(snap.title, "screen-mark capture");
    assert!(snap.controls.is_empty(), "no toolbar yet");
    assert!(snap.status.unwrap().starts_with("Drag to select a region"));
}

#[test]
fn with_a_region_the_toolbar_and_the_state_are_described() {
    let mut o = with_region();
    let snap = o.snapshot();
    assert_eq!(snap.controls.len(), o.visible_buttons.len());
    let arrow = snap
        .controls
        .iter()
        .position(|c| c.label == "Arrow (A) tool")
        .expect("the arrow tool");
    assert_eq!(snap.controls[arrow].role, Role::Radio);
    assert_eq!(snap.controls[arrow].checked, Some(true));
    assert_eq!(snap.focus, Some(arrow), "focus sits on the tool that is on");
    assert!(snap
        .controls
        .iter()
        .any(|c| c.label == "Colour Red #ff6e7a"));
    let style = snap
        .controls
        .iter()
        .find(|c| c.label.starts_with("Shape style"))
        .unwrap();
    assert_eq!(style.value.as_deref(), Some("Outline"));
    let status = snap.status.unwrap();
    assert!(
        status.starts_with("Selected 400 by 300 pixels. Tool: Arrow, outline, no annotations."),
        "{status}"
    );

    // Pressing a control does what clicking it does.
    let rect_tool = snap
        .controls
        .iter()
        .position(|c| c.label == "Rectangle (R) tool")
        .unwrap();
    assert_eq!(o.request(Request::Press(rect_tool)), Outcome::Redraw);
    assert_eq!(o.active_tool, ToolKind::Rectangle);
    let copy = snap
        .controls
        .iter()
        .position(|c| c.label.starts_with("Copy"))
        .unwrap();
    assert_eq!(o.request(Request::Press(copy)), Outcome::Copy);
    assert_eq!(o.request(Request::Press(999)), Outcome::Nothing);
}

#[test]
fn the_status_holds_still_while_the_mouse_drags() {
    let mut o = with_region();
    o.on_cursor_moved(Point::new(100.0, 100.0));
    o.on_mouse_down();
    o.on_cursor_moved(Point::new(90.0, 95.0));
    assert_eq!(o.snapshot().status.as_deref(), Some("Selecting."));
}

#[test]
fn marking_up_an_image_is_named_as_such() {
    assert_eq!(
        editing().snapshot().title,
        "screen-mark: marking up an image"
    );
}
