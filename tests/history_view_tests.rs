//! The History window, driven without a display: mouse and keyboard.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use screen_mark::access::{Request, Role};
use screen_mark::history_ui::{Act, Focus, HistoryView, Outcome, COLS, WIN_H, WIN_W};
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
    // No capture further right: the focus stays.
    assert_eq!(press(&mut v, Mods::NONE, Named::Right), Outcome::Nothing);
    assert_eq!(v.focus(), Some(Focus::Capture(5)));
    // No row below: down is the buttons along the bottom, and up comes
    // back to the capture they act on.
    press(&mut v, Mods::NONE, Named::Down);
    assert_eq!(v.focus(), Some(Focus::Action(Act::Copy)));
    press(&mut v, Mods::NONE, Named::Up);
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
fn tab_walks_the_captures_the_buttons_and_back() {
    let _home = Home::with_captures("tab", 3);
    let mut v = HistoryView::new();
    let mut seen = Vec::new();
    for _ in 0..10 {
        press(&mut v, Mods::NONE, Named::Tab);
        seen.push(v.focus().unwrap());
    }
    assert_eq!(
        seen,
        [
            Focus::Capture(0),
            Focus::Capture(1),
            Focus::Capture(2),
            Focus::Action(Act::Copy),
            Focus::Action(Act::Edit),
            Focus::Action(Act::SaveAs),
            Focus::Action(Act::Show),
            Focus::Action(Act::Delete),
            Focus::ClearAll,
            Focus::Capture(0)
        ]
    );
    press(&mut v, Mods::SHIFT, Named::Tab);
    assert_eq!(v.focus(), Some(Focus::ClearAll));
    press(&mut v, Mods::SHIFT, Named::Tab);
    assert_eq!(v.focus(), Some(Focus::Action(Act::Delete)));
    for _ in 0..4 {
        press(&mut v, Mods::SHIFT, Named::Tab);
    }
    assert_eq!(v.focus(), Some(Focus::Action(Act::Copy)));
    // Back from the buttons to the capture they act on.
    press(&mut v, Mods::SHIFT, Named::Tab);
    assert_eq!(v.focus(), Some(Focus::Capture(0)));
}

#[test]
fn the_buttons_act_on_the_capture_clicked_last() {
    let home = Home::with_captures("buttons", 5);
    let mut v = HistoryView::new();
    let middle = |r: (f32, f32, f32, f32)| (r.0 + r.2 / 2.0, r.1 + r.3 / 2.0);

    // Nothing picked yet: the buttons do nothing and say why.
    let (x, y) = middle(Act::Edit.rect());
    assert_eq!(v.click(x, y), Outcome::Nothing);
    let edit = v
        .snapshot()
        .controls
        .into_iter()
        .find(|c| c.label.starts_with("Mark up"))
        .unwrap();
    assert_eq!(edit.label, "Mark up (pick a capture first)");

    let (x, y) = middle(v.thumb_rect(2));
    assert_eq!(v.click(x, y), Outcome::Copy(home.capture(2, 5)));
    assert_eq!(v.current(), Some(2));
    for (act, want) in [
        (Act::Copy, Outcome::Copy(home.capture(2, 5))),
        (Act::Edit, Outcome::Edit(home.capture(2, 5))),
        (Act::SaveAs, Outcome::SaveAs(home.capture(2, 5))),
        (Act::Show, Outcome::Show(home.capture(2, 5))),
    ] {
        let (x, y) = middle(act.rect());
        assert_eq!(v.click(x, y), want, "{act:?}");
    }
    assert!(v
        .snapshot()
        .controls
        .iter()
        .any(|c| c.label == "Mark up capture 3 again"));

    // Delete removes the file, and the next capture takes its place.
    let (x, y) = middle(Act::Delete.rect());
    assert_eq!(v.click(x, y), Outcome::Redraw);
    assert_eq!(v.len(), 4);
    assert!(!home.capture(2, 5).exists());
    assert_eq!(v.current(), Some(2));
    assert_eq!(
        v.act(Act::Copy),
        Outcome::Copy(home.capture(3, 5)),
        "the one after the deleted one"
    );
}

#[test]
fn letters_and_delete_work_on_the_focused_capture() {
    let home = Home::with_captures("letters", 3);
    let mut v = HistoryView::new();
    let letter = |v: &mut HistoryView, c: char| {
        v.key(KeyPress {
            mods: Mods::NONE,
            key: Some(Key::Char(c)),
            pressed: true,
        })
    };
    assert_eq!(letter(&mut v, 'e'), Outcome::Nothing, "nothing focused yet");
    press(&mut v, Mods::NONE, Named::End);
    assert_eq!(letter(&mut v, 'e'), Outcome::Edit(home.capture(2, 3)));
    assert_eq!(letter(&mut v, 's'), Outcome::SaveAs(home.capture(2, 3)));
    assert_eq!(letter(&mut v, 'o'), Outcome::Show(home.capture(2, 3)));
    assert_eq!(letter(&mut v, 'q'), Outcome::Nothing);
    assert_eq!(press(&mut v, Mods::NONE, Named::Delete), Outcome::Redraw);
    assert_eq!(v.len(), 2);
    // The last one went: the focus moves to the one before it.
    assert_eq!(v.focus(), Some(Focus::Capture(1)));
    press(&mut v, Mods::NONE, Named::Delete);
    press(&mut v, Mods::NONE, Named::Delete);
    assert!(v.is_empty());
    assert_eq!(v.focus(), None);
    assert_eq!(press(&mut v, Mods::NONE, Named::Delete), Outcome::Nothing);
}

#[test]
fn a_capture_with_kept_markup_says_so() {
    let home = Home::with_captures("markup", 2);
    let png = home.capture(0, 2);
    let editable = screen_mark::history::Editable {
        pixels: vec![200; 60 * 40 * 4],
        width: 60,
        height: 40,
        annotations: vec![screen_mark::tools::Annotation::Rectangle {
            top_left: screen_mark::geometry::Point::new(5.0, 5.0),
            size: screen_mark::geometry::Size::new(20.0, 10.0),
            color: screen_mark::geometry::Color::red(),
            thickness: 3.0,
            style: screen_mark::tools::ShapeStyle::Dashed,
        }],
    };
    screen_mark::history::write_markup(&png, &editable).unwrap();
    let v = HistoryView::new();
    let labels: Vec<String> = v.snapshot().controls.into_iter().map(|c| c.label).collect();
    assert!(
        labels[1].ends_with("its annotations can be changed. Copy it again"),
        "{labels:?}"
    );
    assert!(!labels[2].contains("annotations"), "{labels:?}");
    // And it loads back with its annotations, the image as it was before.
    assert_eq!(screen_mark::history::load_editable(&png).unwrap(), editable);
    // Deleting it takes the markup too.
    screen_mark::history::delete_entry(&png).unwrap();
    assert!(!screen_mark::history::markup_path(&png).exists());
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
    assert_eq!(
        snap.controls.len(),
        12,
        "the button, six captures and five buttons"
    );
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
