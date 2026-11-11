//! Frames are patched, not redrawn: only the areas that changed since the
//! last frame are painted again and given to the window. These tests hold
//! the patched frame to the same picture as one drawn from scratch, through
//! long random sessions of everything a user can do in the overlay.

use screen_mark::capture::CapturedScreen;
use screen_mark::config::Config;
use screen_mark::geometry::Point;
use screen_mark::keys::{KeyMap, Mods, Named};
use screen_mark::overlay::input::KeyInput;
use screen_mark::overlay::selection::Selection;
use screen_mark::renderer::{render_frame, render_overlay, to_window_pixels, Area, Frame};
use screen_mark::state::OverlayState;
use screen_mark::tools::{ToolKind, TOOL_ORDER};
use tiny_skia::Pixmap;

const W: u32 = 480;
const H: u32 = 360;

/// The theme is one setting for the whole process, and one test here
/// switches it; tests that draw take turns.
static DRAWING: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn my_turn() -> std::sync::MutexGuard<'static, ()> {
    DRAWING.lock().unwrap_or_else(|e| e.into_inner())
}

/// A capture with no two neighbouring pixels alike, so a patch that is a
/// pixel off shows.
fn capture(w: u32, h: u32) -> CapturedScreen {
    let mut pixels = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            pixels.extend_from_slice(&[
                (x * 7 + y * 3) as u8,
                (x * 2 + y * 11) as u8,
                (x ^ y) as u8,
                255,
            ]);
        }
    }
    CapturedScreen {
        pixels,
        width: w,
        height: h,
        x_offset: 0,
        y_offset: 0,
        scale_factor: 1.0,
    }
}

fn whole(w: u32, h: u32) -> Area {
    Area::new(0, 0, w as i32, h as i32)
}

/// xorshift64*: the same session every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn between(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (self.below(10_000) as f32 / 10_000.0) * (hi - lo)
    }
}

/// One thing a user might do next. Returns what it was, for the failure
/// message.
fn act(o: &mut OverlayState, rng: &mut Rng, keymap: &KeyMap) -> String {
    let key = |o: &mut OverlayState, input: KeyInput| {
        o.on_key(&input, keymap);
    };
    match rng.below(100) {
        // Mostly the pointer moves, sometimes just off the capture's edge.
        0..=49 => {
            let p = Point::new(
                rng.between(-12.0, W as f32 + 12.0),
                rng.between(-12.0, H as f32 + 12.0),
            );
            o.on_cursor_moved(p);
            format!("move to {:.1}, {:.1}", p.x, p.y)
        }
        // A short move, as in a drag.
        50..=59 => {
            let p = Point::new(
                o.last_mouse_pos.x + rng.between(-9.0, 9.0),
                o.last_mouse_pos.y + rng.between(-9.0, 9.0),
            );
            o.on_cursor_moved(p);
            format!("nudge to {:.1}, {:.1}", p.x, p.y)
        }
        60..=67 => {
            o.on_mouse_down();
            "press".into()
        }
        68..=75 => {
            o.on_mouse_up();
            "release".into()
        }
        76..=80 => {
            let tool = TOOL_ORDER[rng.below(TOOL_ORDER.len() as u64) as usize];
            // Not while typing: the letter would go into the text.
            if !o.text_input_active {
                o.active_tool = tool;
                o.selected_index = None;
            }
            format!("tool {tool:?}")
        }
        // Pick an annotation with the select tool, as a click on it would:
        // its dashed box and corner handles show.
        81..=82 => {
            if o.text_input_active || o.annotations.is_empty() {
                return "nothing to select".into();
            }
            let i = rng.below(o.annotations.len() as u64) as usize;
            o.active_tool = ToolKind::Select;
            o.selected_index = Some(i);
            format!("select annotation {i}")
        }
        83..=86 => {
            let named = [
                Named::Left,
                Named::Right,
                Named::Up,
                Named::Down,
                Named::Enter,
                Named::Backspace,
                Named::Delete,
                Named::Home,
                Named::End,
                Named::Space,
            ][rng.below(10) as usize];
            let mods = [Mods::NONE, Mods::SHIFT, Mods::CTRL][rng.below(3) as usize];
            key(o, KeyInput::named(named).with(mods));
            format!("key {named:?} with {mods:?}")
        }
        87..=89 => {
            // Text, when the text tool is typing; a tool key otherwise.
            let c = ["a", "W", "q", "j", "g", "y", "|", "\u{4f60}"][rng.below(8) as usize];
            key(o, KeyInput::typed(c));
            format!("type {c}")
        }
        90..=91 => {
            let undo = rng.below(2) == 0;
            let mods = if undo {
                Mods::CTRL
            } else {
                Mods::CTRL | Mods::SHIFT
            };
            key(o, KeyInput::typed("z").with(mods));
            format!("undo={undo}")
        }
        92..=93 => {
            let lines = rng.between(-3.0, 3.0).round();
            o.on_scroll(lines);
            format!("wheel {lines}")
        }
        94..=95 => {
            let shift = rng.below(2) == 0;
            o.set_modifiers(if shift { Mods::SHIFT } else { Mods::NONE });
            format!("shift={shift}")
        }
        96 => {
            o.on_right_click();
            "right click".into()
        }
        97 => {
            if rng.below(2) == 0 {
                o.show_toast("Click Upload again to share".into(), 600_000);
                "message shown".into()
            } else {
                o.toast_message = None;
                o.toast_until = None;
                "message gone".into()
            }
        }
        98 => {
            // A fresh region, so the session doesn't stay stuck without one.
            o.selection = Some(Selection::from_points(
                Point::new(rng.between(0.0, 200.0), rng.between(0.0, 150.0)),
                Point::new(rng.between(220.0, W as f32), rng.between(170.0, H as f32)),
            ));
            o.is_selecting = false;
            "new region".into()
        }
        _ => {
            key(o, KeyInput::typed("a").with(Mods::CTRL));
            "select all".into()
        }
    }
}

fn first_difference(a: &[u8], b: &[u8], width: u32) -> Option<(u32, u32)> {
    a.as_chunks::<4>()
        .0
        .iter()
        .zip(b.as_chunks::<4>().0)
        .position(|(x, y)| x != y)
        .map(|i| (i as u32 % width, i as u32 / width))
}

/// Run a random session. After every action the patched frame must equal a
/// frame drawn from scratch, and the window's buffer, given only the
/// changed areas, must equal that frame too. Returns how much of what
/// whole frames would have sent to the window was sent (1.0 = no saving).
fn session(seed: u64, steps: usize, config: &Config, setup: impl Fn(&mut OverlayState)) -> f64 {
    let _turn = my_turn();
    let keymap = KeyMap::from_report(&config.key_report());
    let mut rng = Rng(seed);
    let mut o = OverlayState::new(capture(W, H), config);
    setup(&mut o);

    let mut patched = Pixmap::new(W, H).unwrap();
    let mut frame = Frame::default();
    let mut window = vec![0u32; (W * H) as usize];
    let mut scratch = Pixmap::new(W, H).unwrap();
    let mut expected = vec![0u32; (W * H) as usize];
    let mut log: Vec<String> = Vec::new();
    let mut patched_pixels = 0i64;

    for step in 0..steps {
        if step > 0 {
            log.push(act(&mut o, &mut rng, &keymap));
        }
        let changed = render_frame(&mut o, &mut patched, &mut frame);
        to_window_pixels(&patched, &mut window, W as usize, &changed);
        patched_pixels += changed.iter().map(|a| a.pixels()).sum::<i64>();

        scratch.data_mut().fill(0);
        render_overlay(&mut o, &mut scratch);
        to_window_pixels(&scratch, &mut expected, W as usize, &[whole(W, H)]);

        let recent = || {
            log.iter()
                .rev()
                .take(12)
                .rev()
                .cloned()
                .collect::<Vec<_>>()
                .join("; ")
        };
        if let Some((x, y)) = first_difference(patched.data(), scratch.data(), W) {
            panic!(
                "seed {seed}, step {step}: the patched frame differs from a fresh one at ({x}, {y}).\n\
                 Last actions: {}",
                recent()
            );
        }
        if let Some(i) = window.iter().zip(&expected).position(|(a, b)| a != b) {
            panic!(
                "seed {seed}, step {step}: the window was not given the change at ({}, {}).\n\
                 Last actions: {}",
                i as u32 % W,
                i as u32 / W,
                recent()
            );
        }
    }
    patched_pixels as f64 / (steps as i64 * whole(W, H).pixels()) as f64
}

#[test]
fn a_patched_frame_always_equals_a_fresh_one() {
    // How much patching saves is measured where it means something, on a
    // screen-sized frame, in `moving_the_pointer_changes_a_small_part_of_the_frame`.
    // Here the frame is small and the session may draw a spotlight.
    for seed in [1, 2, 3, 4] {
        session(seed, 700, &Config::default(), |_| {});
    }
}

#[test]
fn also_when_a_window_is_being_picked() {
    session(11, 300, &Config::default(), |o| {
        o.window_pick = Some(vec![
            (40, 30, 200, 150),
            (10, 10, 400, 300),
            (300, 200, 170, 150),
        ]);
    });
}

#[test]
fn also_without_the_magnifier_and_in_the_light_theme() {
    let mut config = Config::default();
    config.general.magnifier = false;
    config.general.finish_on_release = true;
    // A second lock around the session's own would deadlock, so the theme
    // is switched inside the session's setup, which runs under its lock.
    session(21, 400, &config, |_| {
        screen_mark::theme::set_mode(screen_mark::theme::ThemeMode::Light);
    });
    let _turn = my_turn();
    screen_mark::theme::set_mode(screen_mark::theme::ThemeMode::Dark);
}

#[test]
fn moving_the_pointer_changes_a_small_part_of_the_frame() {
    let _turn = my_turn();
    let (w, h) = (1920u32, 1080u32);
    let config = Config::default();
    let mut o = OverlayState::new(capture(w, h), &config);
    let mut pixmap = Pixmap::new(w, h).unwrap();
    let mut frame = Frame::default();
    let frame_pixels = whole(w, h).pixels();
    let share = |changed: &[Area]| {
        changed.iter().map(|a| a.pixels()).sum::<i64>() as f64 / frame_pixels as f64
    };

    // The first frame is all of it.
    o.on_cursor_moved(Point::new(400.0, 300.0));
    assert_eq!(render_frame(&mut o, &mut pixmap, &mut frame), [whole(w, h)]);

    // Placing the region: two crosshair lines and the magnifier, old and new.
    o.on_cursor_moved(Point::new(420.0, 310.0));
    let placing = share(&render_frame(&mut o, &mut pixmap, &mut frame));
    assert!(
        placing < 0.04,
        "placing the region repaints {:.1}%",
        placing * 100.0
    );

    // Dragging it out: the strips the region grew by and its border, the
    // toolbar and the magnifier in their old and new places. Not the
    // region itself, which is most of the screen here.
    o.on_mouse_down();
    o.on_cursor_moved(Point::new(1400.0, 800.0));
    render_frame(&mut o, &mut pixmap, &mut frame);
    o.on_cursor_moved(Point::new(1410.0, 806.0));
    let dragging = share(&render_frame(&mut o, &mut pixmap, &mut frame));
    assert!(
        dragging < 0.12,
        "dragging repaints {:.1}%",
        dragging * 100.0
    );
    o.on_mouse_up();
    render_frame(&mut o, &mut pixmap, &mut frame);

    // Marking up: the pointer moving inside the region redraws the toolbar.
    o.on_cursor_moved(Point::new(700.0, 500.0));
    render_frame(&mut o, &mut pixmap, &mut frame);
    o.on_cursor_moved(Point::new(710.0, 505.0));
    let idle = share(&render_frame(&mut o, &mut pixmap, &mut frame));
    assert!(
        idle < 0.04,
        "moving the pointer repaints {:.1}%",
        idle * 100.0
    );

    // Drawing an arrow: the arrow, old and new, plus the toolbar.
    o.active_tool = ToolKind::Arrow;
    o.on_mouse_down();
    o.on_cursor_moved(Point::new(900.0, 600.0));
    render_frame(&mut o, &mut pixmap, &mut frame);
    o.on_cursor_moved(Point::new(905.0, 604.0));
    let drawing = share(&render_frame(&mut o, &mut pixmap, &mut frame));
    assert!(drawing < 0.08, "drawing repaints {:.1}%", drawing * 100.0);
    o.on_mouse_up();

    // A frame with nothing new still reports what it redrew, and no more.
    render_frame(&mut o, &mut pixmap, &mut frame);
    let again = share(&render_frame(&mut o, &mut pixmap, &mut frame));
    assert!(
        again < 0.08,
        "an unchanged frame repaints {:.1}%",
        again * 100.0
    );

    // A spotlight changes every pixel when it appears, and none after:
    // the pointer moving under one costs what it cost before.
    o.active_tool = ToolKind::Spotlight;
    o.on_cursor_moved(Point::new(600.0, 400.0));
    o.on_mouse_down();
    o.on_cursor_moved(Point::new(1000.0, 550.0));
    o.on_mouse_up();
    assert!(matches!(
        o.annotations.last(),
        Some(screen_mark::tools::Annotation::Spotlight { .. })
    ));
    assert_eq!(render_frame(&mut o, &mut pixmap, &mut frame), [whole(w, h)]);
    o.on_cursor_moved(Point::new(720.0, 510.0));
    let lit = share(&render_frame(&mut o, &mut pixmap, &mut frame));
    assert!(
        lit < 0.08,
        "with a spotlight, a move repaints {:.1}%",
        lit * 100.0
    );

    // An invalidated frame is drawn whole again.
    frame.invalidate();
    assert_eq!(render_frame(&mut o, &mut pixmap, &mut frame), [whole(w, h)]);
}

#[test]
fn the_background_is_the_capture_dimmed_outside_the_region() {
    let _turn = my_turn();
    let (w, h) = (400u32, 300u32);
    let shot = capture(w, h);
    let mut o = OverlayState::new(shot.clone(), &Config::default());
    // A region near the top; the toolbar sits in a band just under it.
    o.selection = Some(Selection {
        x: 40.0,
        y: 30.0,
        width: 100.0,
        height: 60.0,
    });
    let mut pixmap = Pixmap::new(w, h).unwrap();
    render_overlay(&mut o, &mut pixmap);
    let at = |data: &[u8], x: u32, y: u32| {
        let i = ((y * w + x) * 4) as usize;
        [data[i], data[i + 1], data[i + 2], data[i + 3]]
    };
    // Well inside the region: the capture itself.
    assert_eq!(at(pixmap.data(), 60, 50), at(&shot.pixels, 60, 50));
    // Outside it, clear of the toolbar: each channel at about 60 %, opaque.
    let (out, src) = (at(pixmap.data(), 300, 250), at(&shot.pixels, 300, 250));
    for c in 0..3 {
        let want = src[c] as f32 * 0.6;
        assert!((out[c] as f32 - want).abs() <= 1.5, "{out:?} from {src:?}");
    }
    assert_eq!(out[3], 255);
}

#[test]
fn only_the_named_areas_reach_the_window() {
    let (w, h) = (16u32, 8u32);
    let mut pixmap = Pixmap::new(w, h).unwrap();
    for (i, px) in pixmap
        .data_mut()
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .enumerate()
    {
        px.copy_from_slice(&[i as u8, 100, 200, 255]);
    }
    let mut window = vec![0xdead_beefu32; (w * h) as usize];
    let told = to_window_pixels(
        &pixmap,
        &mut window,
        w as usize,
        // One inside, one hanging off the edge, one entirely outside.
        &[
            Area::new(2, 1, 5, 3),
            Area::new(14, 6, 40, 40),
            Area::new(50, 50, 60, 60),
        ],
    );
    assert_eq!(told.len(), 2);
    assert_eq!((told[1].x, told[1].y), (14, 6));
    assert_eq!((told[1].width.get(), told[1].height.get()), (2, 2));
    for y in 0..h {
        for x in 0..w {
            let inside = ((2..5).contains(&x) && (1..3).contains(&y)) || (x >= 14 && y >= 6);
            let i = (y * w + x) as usize;
            let want = if inside {
                ((i as u32 & 0xff) << 16) | (100 << 8) | 200
            } else {
                0xdead_beef
            };
            assert_eq!(window[i], want, "({x}, {y})");
        }
    }
}

#[test]
fn areas_cut_and_clip_as_rectangles_do() {
    let a = Area::new(0, 0, 10, 10);
    assert_eq!(a.pixels(), 100);
    assert_eq!(
        a.intersect(Area::new(5, 5, 20, 20)),
        Area::new(5, 5, 10, 10)
    );
    assert!(a.intersect(Area::new(10, 0, 20, 10)).is_empty());
    assert!(a.contains(Area::new(2, 2, 4, 4)) && !a.contains(Area::new(2, 2, 11, 4)));

    // A hole in the middle leaves four pieces that tile the rest exactly.
    let pieces = a.without(Area::new(3, 4, 6, 8));
    assert_eq!(pieces.len(), 4);
    assert_eq!(pieces.iter().map(|p| p.pixels()).sum::<i64>(), 100 - 12);
    for (i, p) in pieces.iter().enumerate() {
        assert!(p.intersect(Area::new(3, 4, 6, 8)).is_empty());
        for q in &pieces[i + 1..] {
            assert!(p.intersect(*q).is_empty(), "{p:?} overlaps {q:?}");
        }
    }
    // A hole elsewhere, or covering everything.
    assert_eq!(a.without(Area::new(20, 20, 30, 30)), [a]);
    assert!(a.without(Area::new(-5, -5, 50, 50)).is_empty());
    // A region that grew by a pixel at the bottom right: two thin strips.
    let grown = Area::new(0, 0, 11, 11).without(a);
    assert_eq!(grown.iter().map(|p| p.pixels()).sum::<i64>(), 21);

    // Fractions of a pixel count as the whole pixel.
    assert_eq!(Area::around(1.5, 2.5, 3.0, 0.0, 1.0), Area::new(0, 1, 6, 4));
}

/// One annotation of every kind, with the awkward cases: a thick arrow, a
/// pencil stroke that doubles back on itself, text on several lines with
/// characters from a system font, a measurement whose label sticks out.
fn every_kind() -> Vec<screen_mark::tools::Annotation> {
    use screen_mark::geometry::{Color, Size};
    use screen_mark::tools::Annotation;
    let p = Point::new;
    let presets = Color::presets();
    vec![
        Annotation::Arrow {
            start: p(60.0, 60.0),
            end: p(150.0, 95.0),
            color: presets[0],
            thickness: 9.0,
        },
        Annotation::Rectangle {
            top_left: p(170.5, 40.5),
            size: Size::new(70.0, 44.0),
            color: presets[1],
            thickness: 6.0,
        },
        Annotation::RoundedRect {
            top_left: p(260.0, 44.0),
            size: Size::new(60.0, 40.0),
            color: presets[2],
            thickness: 3.0,
            radius: 12.0,
        },
        Annotation::Ellipse {
            center: p(370.0, 70.0),
            radius_x: 34.0,
            radius_y: 22.0,
            color: presets[3],
            thickness: 5.0,
        },
        Annotation::Line {
            start: p(60.0, 130.0),
            end: p(140.0, 118.0),
            color: presets[4],
            thickness: 7.0,
        },
        Annotation::Pencil {
            points: vec![
                p(170.0, 120.0),
                p(230.0, 150.0),
                p(172.0, 124.0),
                p(240.0, 122.0),
            ],
            color: presets[0],
            thickness: 6.0,
        },
        Annotation::Highlight {
            top_left: p(260.0, 110.0),
            size: Size::new(80.0, 26.0),
            color: presets[3],
        },
        Annotation::Text {
            position: p(60.0, 170.0),
            text: "gjpqy Wide\n\u{4f60}\u{597d} \u{00c5}ngstr\u{00f6}m".into(),
            color: presets[1],
            font_size: 26.0,
        },
        Annotation::Pixelate {
            top_left: p(250.0, 160.0),
            size: Size::new(70.0, 50.0),
            block_size: 9,
        },
        Annotation::StepMarker {
            position: p(360.0, 180.0),
            number: 12,
            color: presets[2],
            size: 40.0,
        },
        Annotation::Measurement {
            start: p(70.0, 262.0),
            end: p(92.0, 268.0),
            color: presets[4],
        },
        Annotation::Redact {
            top_left: p(150.3, 250.7),
            size: Size::new(60.0, 22.0),
            color: Color::new(0.0, 0.0, 0.0, 1.0),
        },
    ]
}

#[test]
fn also_with_every_kind_of_annotation_being_moved_about() {
    for seed in [31, 32] {
        session(seed, 400, &Config::default(), |o| {
            o.selection = Some(Selection {
                x: 30.0,
                y: 20.0,
                width: 420.0,
                height: 290.0,
            });
            o.annotations = every_kind();
            // The select tool: clicks pick annotations, drags and arrow keys
            // move them, the corners resize them.
            o.active_tool = ToolKind::Select;
            o.selected_index = Some(7);
        });
    }
}

#[test]
fn also_with_spotlights_which_dim_everything_around_them() {
    session(41, 300, &Config::default(), |o| {
        o.selection = Some(Selection {
            x: 30.0,
            y: 20.0,
            width: 420.0,
            height: 290.0,
        });
        o.annotations = every_kind();
        o.annotations
            .push(screen_mark::tools::Annotation::Spotlight {
                top_left: Point::new(100.0, 60.0),
                size: screen_mark::geometry::Size::new(200.0, 120.0),
            });
        // A second one, hanging off the top-left corner of the capture.
        o.annotations
            .push(screen_mark::tools::Annotation::Spotlight {
                top_left: Point::new(-20.0, -10.0),
                size: screen_mark::geometry::Size::new(90.0, 70.0),
            });
    });
}

#[test]
fn also_while_text_is_being_typed() {
    session(51, 400, &Config::default(), |o| {
        o.selection = Some(Selection {
            x: 30.0,
            y: 20.0,
            width: 420.0,
            height: 290.0,
        });
        o.active_tool = ToolKind::Text;
        o.on_cursor_moved(Point::new(80.0, 70.0));
        o.on_mouse_down();
        o.on_mouse_up();
        o.insert_text("Typed so far\nand a second line");
        o.set_preedit("ni hao");
    });
}
