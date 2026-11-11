//! How long one overlay frame takes, at the sizes where it matters: one 4K
//! monitor, and two side by side. No window is opened; what is timed is the
//! work screen-mark does for a frame (drawing it, and turning it into the
//! pixels the window takes), not the system's copy to the screen, which
//! shrinks with the changed area in the same way.
//!
//! Run: cargo run --release --example overlay_bench

use std::time::{Duration, Instant};

use screen_mark::capture::CapturedScreen;
use screen_mark::config::Config;
use screen_mark::geometry::Point;
use screen_mark::renderer::{render_frame, to_window_pixels, Area, Frame};
use screen_mark::state::OverlayState;
use screen_mark::tools::ToolKind;

const FRAMES: usize = 90;

/// A desktop-like capture: not one flat colour.
fn capture(w: u32, h: u32) -> CapturedScreen {
    let mut pixels = vec![0u8; w as usize * h as usize * 4];
    for (i, px) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let (x, y) = (i as u32 % w, i as u32 / w);
        px.copy_from_slice(&[(x / 7) as u8, (y / 5) as u8, ((x + y) / 11) as u8, 255]);
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

fn median(mut times: Vec<Duration>) -> f64 {
    times.sort();
    times[times.len() / 2].as_secs_f64() * 1000.0
}

/// What the app keeps between frames.
struct Screen {
    pixmap: tiny_skia::Pixmap,
    frame: Frame,
    window: Vec<u32>,
}

impl Screen {
    fn new(w: u32, h: u32) -> Self {
        Self {
            pixmap: tiny_skia::Pixmap::new(w, h).unwrap(),
            frame: Frame::default(),
            window: vec![0u32; w as usize * h as usize],
        }
    }

    /// One frame: draw what changed, convert what changed. Returns the two
    /// times and the share of the screen that changed.
    fn draw(&mut self, state: &mut OverlayState) -> (Duration, Duration, f64) {
        let w = self.pixmap.width() as usize;
        let t = Instant::now();
        let changed = render_frame(state, &mut self.pixmap, &mut self.frame);
        let draw = t.elapsed();
        let t = Instant::now();
        to_window_pixels(&self.pixmap, &mut self.window, w, &changed);
        let convert = t.elapsed();
        let share = changed.iter().map(|a: &Area| a.pixels()).sum::<i64>() as f64
            / (w as f64 * self.pixmap.height() as f64);
        (draw, convert, share)
    }
}

/// Run `step` for every frame (it moves the mouse), then draw the frame.
fn scenario(
    name: &str,
    state: &mut OverlayState,
    screen: &mut Screen,
    mut step: impl FnMut(&mut OverlayState, usize),
) {
    let (mut draw, mut convert, mut share) = (Vec::new(), Vec::new(), 0.0);
    for frame in 0..FRAMES {
        step(state, frame);
        let (d, c, s) = screen.draw(state);
        draw.push(d);
        convert.push(c);
        share += s / FRAMES as f64;
    }
    let (d, c) = (median(draw), median(convert));
    println!(
        "  {name:<34} draw {d:6.2} ms  + to window {c:6.2} ms  = {:6.2} ms/frame   ({:4.1}% of the screen)",
        d + c,
        share * 100.0
    );
}

fn run(label: &str, w: u32, h: u32) {
    println!("{label}: {w} x {h}");
    let config = Config::default();
    let megabytes = w as f64 * h as f64 * 4.0 / 1e6;
    println!(
        "  kept while the overlay is open         {:6.0} MB (the capture, the frame, the window's buffer)",
        megabytes * 3.0
    );

    let shot = capture(w, h);
    let t = Instant::now();
    let mut state = OverlayState::new(shot, &config);
    let mut screen = Screen::new(w, h);
    state.on_cursor_moved(Point::new(300.0, 200.0));
    let (d, c, _) = screen.draw(&mut state);
    println!(
        "  opening it, with the first frame       {:6.2} ms",
        t.elapsed().as_secs_f64() * 1000.0
    );
    let _ = (d, c);
    // After a click or a key the frame is drawn from scratch.
    let mut whole = Vec::new();
    for _ in 0..15 {
        screen.frame.invalidate();
        let (d, c, _) = screen.draw(&mut state);
        whole.push((d, c));
    }
    let d = median(whole.iter().map(|t| t.0).collect());
    let c = median(whole.iter().map(|t| t.1).collect());
    println!(
        "  {:<34} draw {d:6.2} ms  + to window {c:6.2} ms  = {:6.2} ms/frame   (100.0% of the screen)",
        "a frame from scratch (click, key)",
        d + c
    );

    // The pointer wanders before anything is selected: crosshair, magnifier.
    scenario("placing the region", &mut state, &mut screen, |s, f| {
        s.on_cursor_moved(Point::new(300.0 + f as f32 * 9.0, 200.0 + f as f32 * 5.0));
    });

    // Dragging the region out, corner to corner.
    state.on_cursor_moved(Point::new(200.0, 150.0));
    state.on_mouse_down();
    scenario(
        "dragging the region out",
        &mut state,
        &mut screen,
        |s, f| {
            s.on_cursor_moved(Point::new(400.0 + f as f32 * 14.0, 300.0 + f as f32 * 8.0));
        },
    );
    state.on_cursor_moved(Point::new(1800.0, 1050.0));
    state.on_mouse_up();

    // Drawing an arrow inside it.
    state.active_tool = ToolKind::Arrow;
    state.on_cursor_moved(Point::new(400.0, 300.0));
    state.on_mouse_down();
    scenario("drawing an arrow", &mut state, &mut screen, |s, f| {
        s.on_cursor_moved(Point::new(450.0 + f as f32 * 10.0, 320.0 + f as f32 * 6.0));
    });
    state.on_mouse_up();

    // A few more annotations, then just moving the pointer about.
    for i in 0..6 {
        let at = 300.0 + i as f32 * 180.0;
        state.active_tool = if i % 2 == 0 {
            ToolKind::Rectangle
        } else {
            ToolKind::Circle
        };
        state.on_cursor_moved(Point::new(at, 500.0));
        state.on_mouse_down();
        state.on_cursor_moved(Point::new(at + 140.0, 620.0));
        state.on_mouse_up();
    }
    scenario(
        "moving the pointer, 7 annotations",
        &mut state,
        &mut screen,
        |s, f| {
            s.on_cursor_moved(Point::new(500.0 + f as f32 * 8.0, 800.0));
        },
    );

    // The one thing that still repaints everything: a spotlight.
    state.active_tool = ToolKind::Spotlight;
    state.on_cursor_moved(Point::new(600.0, 300.0));
    state.on_mouse_down();
    state.on_cursor_moved(Point::new(1000.0, 450.0));
    state.on_mouse_up();
    scenario(
        "moving the pointer, with a spotlight",
        &mut state,
        &mut screen,
        |s, f| {
            s.on_cursor_moved(Point::new(500.0 + f as f32 * 8.0, 800.0));
        },
    );
    println!();
}

fn main() {
    run("One 4K monitor", 3840, 2160);
    run("Two 4K monitors", 7680, 2160);
}
