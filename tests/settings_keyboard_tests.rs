//! The Settings window without a mouse: Tab, arrows, Enter, Space. Also
//! what each control says it is, which is what a screen reader reads out.

use std::collections::HashMap;
use std::sync::Mutex;

use screen_mark::config::Config;
use screen_mark::hotkey::Status;
use screen_mark::keys::{Binding, Key, KeyId, KeyPress, Mods, Named};
use screen_mark::settings_ui::{Action, Outcome, Role, SettingsView, Tab, WIN_H, WIN_W};

/// The line above the message and the Save / Cancel buttons.
const FOOTER_TOP: f32 = WIN_H as f32 - 92.0;

static LOCK: Mutex<()> = Mutex::new(());

fn free(_: Binding) -> Status {
    Status::Active
}

fn view() -> SettingsView {
    let mut v = SettingsView::new(Config::default(), HashMap::new()).with_probe(free);
    v.render();
    v
}

fn press(v: &mut SettingsView, mods: Mods, key: Named) -> Outcome {
    let out = v.key(KeyPress {
        mods,
        key: Some(Key::Named(key)),
        pressed: true,
    });
    v.render();
    out
}

fn tab(v: &mut SettingsView) -> Outcome {
    press(v, Mods::NONE, Named::Tab)
}

fn focused_action(v: &SettingsView) -> Option<Action> {
    v.focused().map(|w| w.action)
}

#[test]
fn tab_walks_every_control_in_order_and_wraps() {
    let mut v = view();
    assert_eq!(v.focused(), None, "nothing is focused until a key is used");
    let expected: Vec<Action> = v.widgets().iter().map(|w| w.action).collect();
    assert!(expected.len() > 10, "{expected:?}");
    assert_eq!(expected[0], Action::SwitchTab(Tab::General));
    assert_eq!(expected.last(), Some(&Action::Save));

    let mut seen = Vec::new();
    for _ in 0..expected.len() {
        assert_eq!(tab(&mut v), Outcome::Redraw);
        seen.push(focused_action(&v).unwrap());
    }
    assert_eq!(seen, expected);
    tab(&mut v);
    assert_eq!(focused_action(&v), Some(expected[0]), "wraps to the start");

    // Shift+Tab goes back, and from the first control round to the last.
    press(&mut v, Mods::SHIFT, Named::Tab);
    assert_eq!(focused_action(&v), Some(Action::Save));
    press(&mut v, Mods::SHIFT, Named::Tab);
    assert_eq!(focused_action(&v), Some(Action::Cancel));
}

#[test]
fn the_two_theme_buttons_are_separate_stops() {
    let mut v = view();
    let theme: Vec<&str> = v
        .widgets()
        .iter()
        .filter(|w| w.action == Action::ToggleTheme)
        .map(|w| w.label.as_str())
        .collect();
    assert_eq!(theme, ["Dark", "Light"]);
    v.focus_action(Action::ToggleTheme);
    assert_eq!(v.focused().map(|w| w.label.as_str()), Some("Dark"));
    tab(&mut v);
    assert_eq!(v.focused().map(|w| w.label.as_str()), Some("Light"));
    tab(&mut v);
    assert_ne!(focused_action(&v), Some(Action::ToggleTheme));
}

#[test]
fn arrows_change_tab_on_the_tab_strip_and_ctrl_tab_does_anywhere() {
    let mut v = view();
    tab(&mut v);
    assert_eq!(focused_action(&v), Some(Action::SwitchTab(Tab::General)));
    press(&mut v, Mods::NONE, Named::Right);
    assert_eq!(v.tab, Tab::Capture);
    assert_eq!(focused_action(&v), Some(Action::SwitchTab(Tab::Capture)));
    press(&mut v, Mods::NONE, Named::Left);
    press(&mut v, Mods::NONE, Named::Left);
    assert_eq!(v.tab, Tab::Toolbar, "wraps round");

    // Off the tab strip an arrow moves focus instead.
    tab(&mut v);
    let first = focused_action(&v);
    press(&mut v, Mods::NONE, Named::Down);
    assert_ne!(focused_action(&v), first);
    assert_eq!(v.tab, Tab::Toolbar);

    press(&mut v, Mods::CTRL, Named::Tab);
    assert_eq!(v.tab, Tab::General);
    press(&mut v, Mods::CTRL | Mods::SHIFT, Named::Tab);
    assert_eq!(v.tab, Tab::Toolbar);
}

#[test]
fn space_and_enter_press_the_focused_control() {
    let mut v = view();
    assert!(v.config.general.history_enabled);
    assert!(v.focus_action(Action::ToggleHistory));
    assert_eq!(press(&mut v, Mods::NONE, Named::Space), Outcome::Redraw);
    assert!(!v.config.general.history_enabled);
    assert_eq!(v.focused().and_then(|w| w.checked), Some(false));
    press(&mut v, Mods::NONE, Named::Enter);
    assert!(v.config.general.history_enabled);

    // With nothing focused, Enter and Space do nothing (and never save).
    let mut v = view();
    assert_eq!(press(&mut v, Mods::NONE, Named::Enter), Outcome::Nothing);
    assert_eq!(press(&mut v, Mods::NONE, Named::Space), Outcome::Nothing);

    // Cancel from the keyboard closes without saving; so does Esc.
    v.focus_action(Action::Cancel);
    assert_eq!(
        press(&mut v, Mods::NONE, Named::Enter),
        Outcome::Close { saved: false }
    );
    assert_eq!(
        press(&mut view(), Mods::NONE, Named::Escape),
        Outcome::Close { saved: false }
    );
}

#[test]
fn a_key_row_below_the_fold_scrolls_into_view_when_focused() {
    let mut v = view();
    v.open_tab(Tab::Keys);
    v.render();
    let last = Action::EditKey(KeyId::Tool(14));
    let row = v.widgets().iter().find(|w| w.action == last).unwrap();
    assert_eq!(row.rect, None, "the last tool key starts out of view");
    assert_eq!(row.label, "Redact (solid box)");
    assert_eq!(row.value.as_deref(), Some("X"));

    // End goes to the last control; Shift+Tab from the footer reaches the
    // last key row.
    press(&mut v, Mods::NONE, Named::End);
    assert_eq!(focused_action(&v), Some(Action::Save));
    while focused_action(&v) != Some(last) {
        press(&mut v, Mods::SHIFT, Named::Tab);
    }
    let rect = v.focused().unwrap().rect.expect("scrolled into view");
    assert!(rect.1 + rect.3 <= FOOTER_TOP, "{rect:?}");

    // Enter starts listening on it; the new key is taken; focus stays put.
    press(&mut v, Mods::NONE, Named::Enter);
    assert_eq!(v.editing(), Some(KeyId::Tool(14)));
    assert_eq!(
        v.focused().and_then(|w| w.value.clone()).as_deref(),
        Some("listening for keys")
    );
    v.key(KeyPress {
        mods: Mods::NONE,
        key: Some(Key::Char('k')),
        pressed: true,
    });
    v.render();
    assert_eq!(v.config.shortcuts.redact, "K");
    assert_eq!(focused_action(&v), Some(last));

    // Its Reset button appears as the next stop; pressing it puts the key
    // back, the button goes away, and focus returns to the key.
    tab(&mut v);
    assert_eq!(focused_action(&v), Some(Action::ResetKey(KeyId::Tool(14))));
    press(&mut v, Mods::NONE, Named::Enter);
    assert_eq!(v.config.shortcuts.redact, "X");
    assert_eq!(focused_action(&v), Some(last));
}

#[test]
fn the_focus_ring_shows_for_the_keyboard_only() {
    let mut v = view();
    let accent = screen_mark::theme::accent();
    let ring_at = |v: &mut SettingsView, action: Action| {
        let p = v.render();
        let (x, y, _, _) = v
            .widgets()
            .iter()
            .find(|w| w.action == action)
            .and_then(|w| w.rect)
            .unwrap();
        let i = (((y - 3.0) as u32 * WIN_W + (x + 4.0) as u32) * 4) as usize;
        let d = p.data();
        (d[i], d[i + 1], d[i + 2]) == accent
    };
    assert!(!ring_at(&mut v, Action::ToggleHistory));
    v.focus_action(Action::ToggleHistory);
    assert!(v.focus_visible());
    assert!(ring_at(&mut v, Action::ToggleHistory));
    let p = v.render();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
    let _ = p.save_png(dir.join("settings-focus-ring.png"));

    // A click moves focus to what was clicked and hides the ring.
    let (x, y, w, h) = v.hit_rect(Action::ThicknessUp).unwrap();
    v.click(x + w / 2.0, y + h / 2.0);
    assert!(!v.focus_visible());
    assert!(!ring_at(&mut v, Action::ThicknessUp));
    assert_eq!(focused_action(&v), Some(Action::ThicknessUp));
    // The next Tab carries on from there, with the ring back.
    tab(&mut v);
    assert!(v.focus_visible());
    assert_ne!(focused_action(&v), Some(Action::ThicknessUp));
}

#[test]
fn every_control_says_what_it_is() {
    let mut v = view();
    for tab in [Tab::General, Tab::Capture, Tab::Keys, Tab::Toolbar] {
        v.open_tab(tab);
        v.render();
        for w in v.widgets() {
            assert!(
                w.label.chars().any(|c| c.is_alphabetic()),
                "{tab:?}: {:?} has no spoken name ({:?})",
                w.action,
                w.label
            );
            match w.role {
                Role::CheckBox | Role::Radio | Role::Tab => {
                    assert!(w.checked.is_some(), "{tab:?}: {:?} has no state", w.label)
                }
                Role::Button => assert_eq!(w.checked, None),
            }
        }
        // Exactly one tab is the current one.
        let current: Vec<_> = v
            .widgets()
            .iter()
            .filter(|w| w.role == Role::Tab && w.checked == Some(true))
            .collect();
        assert_eq!(current.len(), 1);
        assert_eq!(current[0].action, Action::SwitchTab(tab));
    }

    v.open_tab(Tab::General);
    v.render();
    let label = |action: Action| {
        v.widgets()
            .iter()
            .find(|w| w.action == action)
            .map(|w| w.label.clone())
            .unwrap()
    };
    assert_eq!(label(Action::ThicknessUp), "Thicker default line");
    assert_eq!(label(Action::SelectColor(0)), "Default colour: red");
    assert_eq!(label(Action::Save), "Save & Close");
}

#[test]
fn every_tab_fits_above_the_footer() {
    let mut v = view();
    for tab in [Tab::General, Tab::Capture, Tab::Toolbar] {
        v.open_tab(tab);
        v.render();
        let footer = [Action::Save, Action::Cancel, Action::ResetAllKeys];
        let content: Vec<_> = v
            .widgets()
            .iter()
            .filter(|w| !footer.contains(&w.action))
            .collect();
        for w in &content {
            let (x, y, width, height) = w.rect.expect("these tabs don't scroll");
            assert!(
                y + height <= FOOTER_TOP && x + width <= WIN_W as f32,
                "{tab:?}: {:?} at {:?} runs into the footer",
                w.label,
                w.rect
            );
        }
    }
    // All fifteen tools are on the Toolbar tab.
    let tools = v
        .widgets()
        .iter()
        .filter(|w| matches!(w.action, Action::ToggleToolbar(_)))
        .count();
    assert_eq!(tools, 15);
    // The magnifier switch is on the Capture tab.
    v.open_tab(Tab::Capture);
    v.render();
    assert!(v.config.general.magnifier);
    v.focus_action(Action::ToggleMagnifier);
    press(&mut v, Mods::NONE, Named::Space);
    assert!(!v.config.general.magnifier);
}

#[test]
fn ctrl_enter_saves_from_anywhere() {
    let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let home = std::env::temp_dir().join(format!("sm-settings-kbd-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    let before = std::env::var_os("SCREEN_MARK_HOME");
    std::env::set_var("SCREEN_MARK_HOME", &home);

    let mut v = view();
    v.focus_action(Action::ThicknessUp);
    press(&mut v, Mods::NONE, Named::Enter);
    let out = press(&mut v, Mods::CTRL, Named::Enter);
    let saved = std::fs::read_to_string(home.join("config.toml"));

    match before {
        Some(old) => std::env::set_var("SCREEN_MARK_HOME", old),
        None => std::env::remove_var("SCREEN_MARK_HOME"),
    }
    let _ = std::fs::remove_dir_all(&home);

    assert_eq!(out, Outcome::Close { saved: true });
    assert!(saved.unwrap().contains("default_thickness = 3.5"));
}
