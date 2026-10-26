//! Headless render of the capture overlay in both themes, for eyeballing a
//! palette change without a display. Writes `theme-dark.png` and
//! `theme-light.png` to the current directory.
//!
//! Run: cargo run --example render_theme

use screen_mark::capture::CapturedScreen;
use screen_mark::config::Config;
use screen_mark::geometry::{Color, Point, Size};
use screen_mark::overlay::selection::Selection;
use screen_mark::renderer::render_overlay;
use screen_mark::state::OverlayState;
use screen_mark::theme::{self, ThemeMode};
use screen_mark::tools::Annotation;

const W: u32 = 960;
const H: u32 = 600;

/// A fake desktop: light panels on a mid-grey field, so dimming, the
/// selection cut-out, and every ink preset are all visible at once.
fn fake_screen() -> CapturedScreen {
    let mut px = vec![0u8; (W * H * 4) as usize];
    for y in 0..H {
        for x in 0..W {
            let i = ((y * W + x) * 4) as usize;
            let panel = (x > 60 && x < 900 && y > 50 && y < 110)
                || (x > 60 && x < 420 && y > 140 && y < 560);
            let (r, g, b) = if panel {
                (236, 236, 232)
            } else {
                (120, 124, 132)
            };
            px[i] = r;
            px[i + 1] = g;
            px[i + 2] = b;
            px[i + 3] = 255;
        }
    }
    CapturedScreen {
        pixels: px,
        width: W,
        height: H,
        x_offset: 0,
        y_offset: 0,
        scale_factor: 1.0,
    }
}

fn render(mode: ThemeMode, out: &str) {
    theme::set_mode(mode);
    let config = Config::default();
    let mut state = OverlayState::new(fake_screen(), &config);
    state.selection = Some(Selection::from_points(
        Point::new(460.0, 140.0),
        Point::new(900.0, 470.0),
    ));
    state.last_mouse_pos = Point::new(900.0, 470.0);
    let presets = Color::presets();
    state.annotations = vec![
        Annotation::Rectangle {
            top_left: Point::new(480.0, 160.0),
            size: Size::new(180.0, 90.0),
            color: presets[0],
            thickness: 3.0,
        },
        Annotation::Arrow {
            start: Point::new(700.0, 180.0),
            end: Point::new(860.0, 260.0),
            color: presets[1],
            thickness: 3.0,
        },
        Annotation::Text {
            position: Point::new(490.0, 300.0),
            text: "preset green".into(),
            color: presets[2],
            font_size: 22.0,
        },
        Annotation::StepMarker {
            position: Point::new(720.0, 330.0),
            number: 1,
            color: presets[3],
            size: 28.0,
        },
        Annotation::Ellipse {
            center: Point::new(800.0, 410.0),
            radius_x: 40.0,
            radius_y: 40.0,
            color: presets[4],
            thickness: 3.0,
        },
    ];
    let mut pixmap = tiny_skia::Pixmap::new(W, H).unwrap();
    render_overlay(&mut state, &mut pixmap);
    pixmap.save_png(out).unwrap();
    println!("wrote {out}");
}

fn main() {
    render(ThemeMode::Dark, "theme-dark.png");
    render(ThemeMode::Light, "theme-light.png");
}
