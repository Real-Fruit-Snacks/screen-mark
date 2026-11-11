//! The History window, driven without a display: mouse and keyboard.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use screen_mark::access::{Request, Role};
use screen_mark::history_ui::{Focus, HistoryView, Outcome, COLS, WIN_H, WIN_W};
use screen_mark::keys::{Key, KeyPress, Mods, Named};

static LOCK: Mutex<()> = Mutex::new(());

/// A temp `SCREEN_MARK_HOME` holding `count` history files, oldest first.
struct Home {
    root: PathBuf,
    before: Option<std::ffi::OsString>,
    _guard: MutexGuard<'static, ()>,
}

impl Home {
    fn with_captures(tag: &str, count: usize) -> Self {
        let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let root = std::env::temp_dir().join(format!("sm-history-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("history");
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..count {
            // Named by time, like real ones; each a different shade.
            let shade = 40 + (i as u8) * 10;
            let image =
                image::RgbaImage::from_pixel(60, 40, image::Rgba([shade, shade, shade, 255]));
            image
                .save(dir.join(format!("{}.png", 1_800_000_000_000u64 + i as u64 * 60_000)))
                .unwrap();
        }
        let before = std::env::var_os("SCREEN_MARK_HOME");
        std::env::set_var("SCREEN_MARK_HOME", &root);
        Self {
            root,
            before,
            _guard: guard,
        }
    }

    /// The path of capture `i` as the view numbers them: newest first.
    fn capture(&self, newest_first: usize, count: usize) -> PathBuf {
        let i = count - 1 - newest_first;
        self.root
            .join("history")
            .join(format!("{}.png", 1_800_000_000_000u64 + i as u64 * 60_000))
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        match self.before.take() {
            Some(old) => std::env::set_var("SCREEN_MARK_HOME", old),
            None => std::env::remove_var("SCREEN_MARK_HOME"),
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn press(v: &mut HistoryView, mods: Mods, key: Named) -> Outcome {
    v.key(KeyPress {
        mods,
        key: Some(Key::Named(key)),
        pressed: true,
    })
}

#[test]
fn clicking_a_capture_copies_it() {
    let home = Home::with_captures("click", 6);
    let mut v = HistoryView::new();
    assert_eq!(v.len(), 6);
    let (x, y, w, h) = v.thumb_rect(1);
    assert!(v.hover(x + w / 2.0, y + h / 2.0), "the border lights up");
    assert_eq!(
        v.click(x + w / 2.0, y + h / 2.0),
        Outcome::Copy(home.capture(1, 6))
    );
    assert!(!v.focus_visible(), "the mouse doesn't show the focus ring");
    // Between captures: nothing.
    assert_eq!(v.click(x + w + 4.0, y + h / 2.0), Outcome::Nothing);
}

#[test]
fn arrows_move_over_the_grid_and_enter_copies() {
    let home = Home::with_captures("arrows", 6);
    let mut v = HistoryView::new();
    assert_eq!(v.focus(), None);
    // The first arrow lands on the newest capture.
    assert_eq!(press(&mut v, Mods::NONE, Named::Right), Outcome::Redraw);
    assert_eq!(v.focus(), Some(Focus::Capture(0)));
    assert!(v.focus_visible());
    press(&mut v, Mods::NONE, Named::Right);
    press(&mut v, Mods::NONE, Named::Down);
    assert_eq!(v.focus(), Some(Focus::Capture(1 + COLS)));
    // No row below, no capture further right: the focus stays.
    assert_eq!(press(&mut v, Mods::NONE, Named::Down), Outcome::Nothing);
    press(&mut v, Mods::NONE, Named::Right);
    assert_eq!(v.focus(), Some(Focus::Capture(5)));
    assert_eq!(
        press(&mut v, Mods::NONE, Named::Enter),
        Outcome::Copy(home.capture(5, 6))
    );
    assert_eq!(
        press(&mut v, Mods::NONE, Named::Space),
        Outcome::Copy(home.capture(5, 6))
    );
    // Up from the top row is the Clear All button; down comes back.
    press(&mut v, Mods::NONE, Named::Home);
    press(&mut v, Mods::NONE, Named::Up);
    assert_eq!(v.focus(), Some(Focus::ClearAll));
    press(&mut v, Mods::NONE, Named::Down);
    assert_eq!(v.focus(), Some(Focus::Capture(0)));
}

#[test]
fn tab_walks_the_button_then_every_capture() {
    let _home = Home::with_captures("tab", 3);
    let mut v = HistoryView::new();
    let mut seen = Vec::new();
    for _ in 0..5 {
        press(&mut v, Mods::NONE, Named::Tab);
        seen.push(v.focus().unwrap());
    }
    assert_eq!(
        seen,
        [
            Focus::Capture(0),
            Focus::Capture(1),
            Focus::Capture(2),
            Focus::ClearAll,
            Focus::Capture(0)
        ]
    );
    press(&mut v, Mods::SHIFT, Named::Tab);
    assert_eq!(v.focus(), Some(Focus::ClearAll));
    press(&mut v, Mods::SHIFT, Named::Tab);
    assert_eq!(v.focus(), Some(Focus::Capture(2)));
}

#[test]
fn focusing_a_capture_below_the_fold_scrolls_to_it() {
    let _home = Home::with_captures("scroll", 20);
    let mut v = HistoryView::new();
    assert_eq!(v.len(), 20);
    let below = v.thumb_rect(19);
    assert!(below.1 > WIN_H as f32, "the last row starts out of view");
    press(&mut v, Mods::NONE, Named::End);
    let (_, y, _, h) = v.thumb_rect(19);
    assert!(y >= 44.0 && y + h <= WIN_H as f32, "now in view: y = {y}");
    press(&mut v, Mods::NONE, Named::Home);
    assert_eq!(v.thumb_rect(0).1, 60.0, "and back to the top");
}

#[test]
fn clear_all_from_the_keyboard_empties_the_window() {
    let home = Home::with_captures("clear", 4);
    let mut v = HistoryView::new();
    press(&mut v, Mods::NONE, Named::Tab);
    press(&mut v, Mods::SHIFT, Named::Tab);
    assert_eq!(v.focus(), Some(Focus::ClearAll));
    assert_eq!(press(&mut v, Mods::NONE, Named::Enter), Outcome::Redraw);
    assert!(v.is_empty());
    assert_eq!(v.focus(), None);
    assert!(!home.capture(0, 4).exists(), "the files are gone");
    // Nothing left to move between; Esc still closes.
    assert_eq!(press(&mut v, Mods::NONE, Named::Tab), Outcome::Nothing);
    assert_eq!(press(&mut v, Mods::NONE, Named::Escape), Outcome::Close);
    assert_eq!(v.snapshot().status.as_deref(), Some("No captures yet."));
}

#[test]
fn every_capture_is_described_for_a_screen_reader() {
    let home = Home::with_captures("describe", 6);
    let mut v = HistoryView::new();
    let snap = v.snapshot();
    assert_eq!(snap.title, "screen-mark History");
    assert_eq!(snap.controls.len(), 7, "the button and six captures");
    assert_eq!(snap.controls[0].label, "Clear all captures from History");
    assert!(snap.controls.iter().all(|c| c.role == Role::Button));
    assert!(
        snap.controls[1].label.starts_with("Capture 1 of 6, taken "),
        "{}",
        snap.controls[1].label
    );
    assert_eq!(snap.focus, None);
    for c in &snap.controls {
        let (x, y, w, h) = c.rect.expect("six captures fit without scrolling");
        assert!(x >= 0.0 && y >= 0.0 && x + w <= WIN_W as f32 && y + h <= WIN_H as f32);
    }

    // A screen reader can focus and press them by position in the list.
    assert_eq!(v.request(Request::Focus(3)), Outcome::Redraw);
    assert_eq!(v.focus(), Some(Focus::Capture(2)));
    assert_eq!(v.snapshot().focus, Some(3));
    assert_eq!(
        v.request(Request::Press(1)),
        Outcome::Copy(home.capture(0, 6))
    );
    assert_eq!(v.request(Request::Press(99)), Outcome::Nothing);
}

#[test]
fn the_focus_ring_is_drawn_around_the_focused_capture() {
    let _home = Home::with_captures("ring", 6);
    let mut v = HistoryView::new();
    let accent = screen_mark::theme::accent();
    let border = |v: &HistoryView, i: usize| {
        let p = v.render();
        let (x, y, _, _) = v.thumb_rect(i);
        // Three pixels outside the thumbnail: only the focus ring is that wide.
        let at = (((y - 3.0) as u32 * WIN_W + (x + 10.0) as u32) * 4) as usize;
        let d = p.data();
        (d[at], d[at + 1], d[at + 2]) == accent
    };
    assert!(!border(&v, 2));
    v.set_focus(Focus::Capture(2));
    assert!(border(&v, 2));
    assert!(!border(&v, 1));
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
    let _ = v.render().save_png(dir.join("history-focus-ring.png"));
}
