//! What the overlay draws, checked by reading pixels back. Snapshots are
//! written to target/overlay-*.png for review, like the Settings ones.

use screen_mark::capture::CapturedScreen;
use screen_mark::config::Config;
use screen_mark::geometry::{Color, Point, Size};
use screen_mark::keys::{KeyMap, Mods, Named};
use screen_mark::overlay::input::KeyInput;
use screen_mark::overlay::selection::Selection;
use screen_mark::renderer::render_overlay;
use screen_mark::state::OverlayState;
use screen_mark::tools::{Annotation, ToolKind};
use tiny_skia::Pixmap;

const W: u32 = 640;
const H: u32 = 420;
const PAPER: [u8; 3] = [236, 236, 232];

/// A plain light capture: anything drawn on it shows.
fn capture() -> CapturedScreen {
    let pixels = (0..W * H)
        .flat_map(|_| [PAPER[0], PAPER[1], PAPER[2], 255])
        .collect();
    CapturedScreen {
        pixels,
        width: W,
        height: H,
        x_offset: 0,
        y_offset: 0,
        scale_factor: 1.0,
    }
}

/// An overlay with a region selected, leaving room for the toolbar below.
fn overlay() -> OverlayState {
    let mut o = OverlayState::new(capture(), &Config::default());
    o.selection = Some(Selection {
        x: 20.0,
        y: 20.0,
        width: 600.0,
        height: 320.0,
    });
    o.last_mouse_pos = Point::new(5.0, 5.0);
    o
}

fn render(o: &mut OverlayState, name: &str) -> Pixmap {
    let mut pixmap = Pixmap::new(W, H).unwrap();
    render_overlay(o, &mut pixmap);
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
    let _ = std::fs::create_dir_all(&dir);
    let _ = pixmap.save_png(dir.join(format!("overlay-{name}.png")));
    pixmap
}

fn px(p: &Pixmap, x: u32, y: u32) -> [u8; 3] {
    let i = ((y * p.width() + x) * 4) as usize;
    let d = p.data();
    [d[i], d[i + 1], d[i + 2]]
}

/// How many pixels in the rectangle differ from the paper colour.
fn inked(p: &Pixmap, x: u32, y: u32, w: u32, h: u32) -> usize {
    let mut n = 0;
    for yy in y..y + h {
        for xx in x..x + w {
            if px(p, xx, yy) != PAPER {
                n += 1;
            }
        }
    }
    n
}

fn key(o: &mut OverlayState, input: KeyInput) {
    let map = KeyMap::from_report(&Config::default().key_report());
    o.on_key(&input, &map);
}

#[test]
fn text_is_drawn_line_by_line_with_a_caret() {
    let mut o = overlay();
    o.active_tool = ToolKind::Text;
    o.on_cursor_moved(Point::new(60.0, 60.0));
    o.on_mouse_down();
    o.on_mouse_up();
    for c in "First line".chars() {
        key(&mut o, KeyInput::typed(&c.to_string()));
    }
    key(&mut o, KeyInput::named(Named::Enter).with(Mods::SHIFT));
    for c in "second".chars() {
        key(&mut o, KeyInput::typed(&c.to_string()));
    }
    o.on_cursor_moved(Point::new(5.0, 5.0));
    let p = render(&mut o, "text-typing");

    let line = screen_mark::tools::text_line_height(20.0) as u32;
    assert!(inked(&p, 60, 60, 200, 20) > 100, "first line");
    assert!(inked(&p, 60, 60 + line, 200, 20) > 100, "second line");
    assert_eq!(
        inked(&p, 60, 60 + 2 * line + 4, 200, 20),
        0,
        "nothing below"
    );

    // The caret sits after "second", in the text's colour.
    let (cx, cy, ch) = o.text_caret_rect().unwrap();
    let caret = px(&p, cx.round() as u32, (cy + ch / 2.0) as u32);
    assert_ne!(caret, PAPER, "caret at ({cx}, {cy})");

    // Finished, the annotation looks the same as it did while typing.
    key(&mut o, KeyInput::named(Named::Enter));
    let done = render(&mut o, "text-done");
    assert_eq!(
        inked(&done, 60, 60, 200, 20),
        inked(&p, 60, 60, 200, 20),
        "first line unchanged by finishing"
    );
}

#[test]
fn characters_outside_the_built_in_font_use_a_system_font_when_there_is_one() {
    // Chinese, Japanese kana, Korean, Cyrillic.
    let sample = "\u{4f60}\u{597d} \u{3053}\u{3093}\u{306b}\u{3061}\u{306f} \u{d55c}\u{ae00} \u{041f}\u{0440}\u{0438}\u{0432}\u{0435}\u{0442}";
    let mut o = overlay();
    o.annotations.push(Annotation::Text {
        position: Point::new(60.0, 60.0),
        text: format!("Latin {sample}"),
        color: Color::red(),
        font_size: 24.0,
    });
    let p = render(&mut o, "text-fallback");
    assert!(
        inked(&p, 60, 60, 60, 24) > 100,
        "Latin text is always drawn"
    );

    // Whether this machine has fonts for the rest is not the test's
    // business; that looking for one works, and can be undone, is.
    for ch in sample.chars().filter(|c| !c.is_whitespace()) {
        if screen_mark::font::has_glyph(ch) {
            let (metrics, bitmap) = screen_mark::font::rasterize(ch, 24.0);
            assert!(metrics.advance_width > 0.0, "{ch:?} has a width");
            assert!(bitmap.iter().any(|v| *v > 0), "{ch:?} draws something");
        }
    }
    screen_mark::font::release_fallbacks();
    let again = render(&mut o, "text-fallback");
    assert_eq!(
        inked(&again, 60, 60, 520, 30),
        inked(&p, 60, 60, 520, 30),
        "the same picture after the system fonts were released and reloaded"
    );
}

#[test]
fn a_spotlight_dims_everything_outside_it_and_annotations_stay_bright() {
    let mut o = overlay();
    o.annotations = vec![
        Annotation::Rectangle {
            top_left: Point::new(300.0, 200.0),
            size: Size::new(80.0, 50.0),
            color: Color::red(),
            thickness: 4.0,
        },
        Annotation::Spotlight {
            top_left: Point::new(80.0, 80.0),
            size: Size::new(150.0, 100.0),
        },
    ];
    let p = render(&mut o, "spotlight");
    assert_eq!(px(&p, 150, 130), PAPER, "inside the spotlight");
    let outside = px(&p, 500, 300);
    assert!(outside[0] < 130, "outside is dimmed: {outside:?}");
    // The rectangle's stroke is drawn over the dimming, at full strength.
    let stroke = px(&p, 300, 225);
    assert!(stroke[0] > 200 && stroke[1] < 150, "red stroke: {stroke:?}");

    // The exported image shows the same thing.
    let (out, w, _) = o.flattened_selection().unwrap();
    let at = |x: u32, y: u32| {
        let i = (((y - 20) * w + (x - 20)) * 4) as usize;
        [out[i], out[i + 1], out[i + 2]]
    };
    assert_eq!(at(150, 130), px(&p, 150, 130));
    assert_eq!(at(500, 300), px(&p, 500, 300));
    assert_eq!(at(300, 225), px(&p, 300, 225));
}

/// A capture that is black left of x = 100 and white from there on.
fn split_capture() -> CapturedScreen {
    let mut c = capture();
    for (i, px) in c.pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let v = if (i as u32 % W) < 100 { 0 } else { 255 };
        px[..3].copy_from_slice(&[v, v, v]);
    }
    c
}

#[test]
fn the_magnifier_enlarges_the_pixels_around_the_pointer() {
    let mut o = OverlayState::new(split_capture(), &Config::default());
    // The pointer on the first white pixel, with room below and right.
    o.on_cursor_moved(Point::new(100.0, 60.0));
    let p = render(&mut o, "magnifier");

    // The magnifier is 24 px below and right of the pointer: 15 capture
    // pixels across, each 8 wide, the pointer's own in the middle column.
    let (x, y) = (124, 84);
    assert_eq!(px(&p, x + 2 * 8 + 4, y + 20), [0, 0, 0], "left of the edge");
    assert_eq!(
        px(&p, x + 6 * 8 + 4, y + 20),
        [0, 0, 0],
        "one pixel left of it"
    );
    assert_eq!(
        px(&p, x + 8 * 8 + 4, y + 20),
        [255, 255, 255],
        "one pixel right"
    );
    // The pointer's pixel is white in the capture and outlined, not dimmed.
    assert_eq!(px(&p, x + 7 * 8 + 4, y + 7 * 8 + 4), [255, 255, 255]);
    // Outside the magnifier the overlay is dimmed as usual.
    let dimmed = px(&p, 400, 300);
    assert!(dimmed[0] < 200 && dimmed[0] > 100, "{dimmed:?}");

    // Near the bottom-right corner it moves to the other side of the pointer.
    o.on_cursor_moved(Point::new(W as f32 - 10.0, H as f32 - 10.0));
    let p = render(&mut o, "magnifier-corner");
    let inside = px(&p, W - 10 - 24 - 60, H - 10 - 24 - 22 - 60);
    assert_eq!(inside, [255, 255, 255], "magnifier above and left");

    // Not in the picture that gets copied.
    o.selection = Some(Selection {
        x: 90.0,
        y: 50.0,
        width: 200.0,
        height: 150.0,
    });
    let (out, w, _) = o.flattened_selection().unwrap();
    let at = |x: u32, y: u32| {
        let i = ((y * w + x) * 4) as usize;
        [out[i], out[i + 1], out[i + 2]]
    };
    assert_eq!(at(5, 60), [0, 0, 0]);
    assert_eq!(at(60, 60), [255, 255, 255], "where the magnifier was drawn");
}

#[test]
fn a_redact_box_is_solid_on_screen_and_in_the_export() {
    let mut o = overlay();
    o.annotations.push(Annotation::Text {
        position: Point::new(60.0, 60.0),
        text: "password: hunter2".into(),
        color: Color::blue(),
        font_size: 22.0,
    });
    // The box covers the secret, not the label.
    o.annotations.push(Annotation::Redact {
        top_left: Point::new(160.0, 56.0),
        size: Size::new(120.0, 30.0),
        color: Color::new(0.0, 0.0, 0.0, 1.0),
    });
    let p = render(&mut o, "redact");
    for (x, y) in [(160, 56), (220, 70), (279, 85)] {
        assert_eq!(px(&p, x, y), [0, 0, 0], "({x}, {y}) on screen");
    }
    assert!(
        inked(&p, 60, 60, 90, 22) > 100,
        "the label is still readable"
    );

    let (out, w, _) = o.flattened_selection().unwrap();
    for (x, y) in [(160u32, 56u32), (220, 70), (279, 85)] {
        let i = (((y - 20) * w + (x - 20)) * 4) as usize;
        assert_eq!(&out[i..i + 4], &[0, 0, 0, 255], "({x}, {y}) in the export");
    }
}

#[test]
fn the_region_on_screen_is_pixel_for_pixel_what_gets_copied() {
    // The overlay paints the spotlight dimming into its background and the
    // export applies it to the cropped image: two routes to one picture.
    let mut shot = capture();
    for (i, px) in shot.pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let (x, y) = (i as u32 % W, i as u32 / W);
        px[..3].copy_from_slice(&[(x * 3 + y) as u8, (y * 5) as u8, (x ^ y) as u8]);
    }
    let mut o = OverlayState::new(shot, &Config::default());
    o.selection = Some(Selection {
        x: 20.0,
        y: 20.0,
        width: 600.0,
        height: 320.0,
    });
    o.last_mouse_pos = Point::new(5.0, 5.0);
    let presets = Color::presets();
    o.annotations = vec![
        Annotation::Spotlight {
            top_left: Point::new(60.0, 50.0),
            size: Size::new(220.0, 120.0),
        },
        Annotation::Arrow {
            start: Point::new(90.0, 220.0),
            end: Point::new(260.0, 150.0),
            color: presets[0],
            thickness: 5.0,
        },
        Annotation::Highlight {
            top_left: Point::new(300.0, 60.0),
            size: Size::new(160.0, 40.0),
            color: presets[3],
        },
        Annotation::Pixelate {
            top_left: Point::new(240.0, 130.0),
            size: Size::new(120.0, 80.0),
            block_size: 10,
        },
        Annotation::Redact {
            top_left: Point::new(400.5, 200.5),
            size: Size::new(90.0, 30.0),
            color: Color::new(0.0, 0.0, 0.0, 1.0),
        },
        Annotation::Text {
            position: Point::new(330.0, 250.0),
            text: "Two\nlines".into(),
            color: presets[1],
            font_size: 24.0,
        },
        // A second spotlight, partly off the region.
        Annotation::Spotlight {
            top_left: Point::new(520.0, 260.0),
            size: Size::new(200.0, 200.0),
        },
    ];
    let p = render(&mut o, "wysiwyg");
    let (out, w, h) = o.flattened_selection().unwrap();
    assert_eq!((w, h), (600, 320));

    // Everything inside the region's border.
    let mut differing = 0;
    for y in 4..h - 4 {
        for x in 4..w - 4 {
            let i = ((y * w + x) * 4) as usize;
            if px(&p, x + 20, y + 20) != [out[i], out[i + 1], out[i + 2]] {
                differing += 1;
            }
        }
    }
    assert_eq!(differing, 0, "pixels that differ between screen and copy");
}
