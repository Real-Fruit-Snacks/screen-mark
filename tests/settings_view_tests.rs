//! The Settings window, driven like a user would: click the real buttons
//! (by their rendered positions), press keys, save. No display needed.
//! Snapshots of every tab are written to target/settings-*.png for review.

use std::collections::HashMap;
use std::sync::Mutex;

use screen_mark::config::{Config, NotificationMode};
use screen_mark::hotkey::Status;
use screen_mark::keys::{Binding, Key, KeyId, KeyPress, Mods, Named};
use screen_mark::settings_ui::{Action, MsgKind, Outcome, SettingsView, Tab};

static LOCK: Mutex<()> = Mutex::new(());

/// Point config (and autostart) at a temp home so Save writes there.
struct TempHome {
    root: std::path::PathBuf,
    saved: Vec<(&'static str, Option<String>)>,
}

impl TempHome {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!("sm-settings-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("config")).unwrap();
        let mut saved = Vec::new();
        for (k, v) in [
            ("XDG_CONFIG_HOME", root.join("config")),
            ("HOME", root.clone()),
            // The only one Windows honours; keeps the real config untouched.
            ("SCREEN_MARK_HOME", root.join("config/screen-mark")),
        ] {
            saved.push((k, std::env::var(k).ok()));
            std::env::set_var(k, v);
        }
        Self { root, saved }
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        for (k, v) in self.saved.drain(..) {
            match v {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn free(_: Binding) -> Status {
    Status::Active
}
fn taken(_: Binding) -> Status {
    Status::Taken
}

fn view() -> SettingsView {
    SettingsView::new(Config::default(), HashMap::new()).with_probe(free)
}

/// Render, find the button for `action`, click its centre.
fn click(v: &mut SettingsView, action: Action) -> Outcome {
    v.render();
    let (x, y, w, h) = v
        .hit_rect(action)
        .unwrap_or_else(|| panic!("no button for {action:?} on {:?}", v.tab));
    v.click(x + w / 2.0, y + h / 2.0)
}

fn press(v: &mut SettingsView, mods: Mods, key: Key) -> Outcome {
    v.key(KeyPress {
        mods,
        key: Some(key),
        pressed: true,
    })
}

fn release(v: &mut SettingsView, mods: Mods, key: Key) -> Outcome {
    v.key(KeyPress {
        mods,
        key: Some(key),
        pressed: false,
    })
}

/// A bare modifier key going down or up; `mods` is what's held after it.
fn modifier(v: &mut SettingsView, mods: Mods, pressed: bool) -> Outcome {
    v.key(KeyPress {
        mods,
        key: None,
        pressed,
    })
}

fn snapshot(v: &mut SettingsView, name: &str) {
    let p = v.render();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
    let _ = p.save_png(dir.join(format!("settings-{name}.png")));
}

#[test]
fn every_tab_renders() {
    let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut v = view();
    for (tab, name) in [
        (Tab::General, "general"),
        (Tab::Capture, "capture"),
        (Tab::Keys, "keys"),
        (Tab::Toolbar, "toolbar"),
    ] {
        click(&mut v, Action::SwitchTab(tab));
        assert_eq!(v.tab, tab);
        snapshot(&mut v, name);
    }
}

#[test]
fn rebinding_a_global_hotkey() {
    let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut v = view();
    click(&mut v, Action::SwitchTab(Tab::Keys));
    click(&mut v, Action::EditKey(KeyId::Screenshot));
    assert_eq!(v.editing(), Some(KeyId::Screenshot));
    snapshot(&mut v, "keys-listening");
    // Holding modifiers shows them; nothing is bound yet.
    assert_eq!(
        v.key(KeyPress {
            mods: Mods::CTRL | Mods::ALT,
            key: None,
            pressed: true
        }),
        Outcome::Redraw
    );
    assert_eq!(v.editing(), Some(KeyId::Screenshot));
    press(&mut v, Mods::CTRL | Mods::ALT, Key::Char('p'));
    assert_eq!(v.editing(), None);
    assert_eq!(v.config.hotkey.screenshot, "Ctrl+Alt+P");
    assert_eq!(v.message().unwrap().1, MsgKind::Info);
}

#[test]
fn printscreen_is_accepted_on_release() {
    // Windows only ever sends PrintScreen's key-up to a window.
    let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut v = view();
    click(&mut v, Action::SwitchTab(Tab::Keys));
    click(&mut v, Action::EditKey(KeyId::Region));
    release(&mut v, Mods::SHIFT, Key::Char('q')); // other releases are ignored
    assert_eq!(v.editing(), Some(KeyId::Region));
    release(
        &mut v,
        Mods::CTRL | Mods::SHIFT,
        Key::Named(Named::PrintScreen),
    );
    assert_eq!(v.config.hotkey.region, "Ctrl+Shift+PrintScreen");
}

#[test]
fn escape_cancels_and_backspace_unbinds() {
    let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut v = view();
    click(&mut v, Action::SwitchTab(Tab::Keys));
    click(&mut v, Action::EditKey(KeyId::Region));
    press(&mut v, Mods::NONE, Key::Named(Named::Escape));
    assert_eq!(v.editing(), None);
    assert_eq!(v.config.hotkey.region, "Ctrl+Shift+S");

    click(&mut v, Action::EditKey(KeyId::Region));
    press(&mut v, Mods::NONE, Key::Named(Named::Backspace));
    assert_eq!(v.config.hotkey.region, "");
    assert!(v.message().unwrap().0.contains("unbound"));

    // Delete works too, for an editor action.
    click(&mut v, Action::EditKey(KeyId::Undo));
    press(&mut v, Mods::NONE, Key::Named(Named::Delete));
    assert_eq!(v.config.keys.undo, "");
}

#[test]
fn a_refused_combination_keeps_listening_and_says_why() {
    let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut v = view();
    click(&mut v, Action::SwitchTab(Tab::Keys));
    click(&mut v, Action::EditKey(KeyId::Region));
    press(&mut v, Mods::SHIFT, Key::Char('d'));
    assert_eq!(v.editing(), Some(KeyId::Region), "still listening");
    assert_eq!(v.config.hotkey.region, "Ctrl+Shift+S", "unchanged");
    let (msg, kind) = v.message().unwrap();
    assert_eq!(kind, MsgKind::Error);
    assert!(msg.contains("Shift+D"), "{msg}");
    snapshot(&mut v, "keys-refused");
    // Then a good one.
    press(&mut v, Mods::CTRL | Mods::SHIFT, Key::Char('d'));
    assert_eq!(v.config.hotkey.region, "Ctrl+Shift+D");
}

#[test]
fn a_combination_owned_by_another_app_is_refused() {
    let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut v = SettingsView::new(Config::default(), HashMap::new()).with_probe(taken);
    click(&mut v, Action::SwitchTab(Tab::Keys));
    click(&mut v, Action::EditKey(KeyId::Screenshot));
    press(&mut v, Mods::CTRL | Mods::SHIFT, Key::Char('x'));
    assert_eq!(v.editing(), Some(KeyId::Screenshot));
    assert!(v.message().unwrap().0.contains("taken by another app"));
    assert_eq!(v.config.hotkey.screenshot, "Ctrl+PrintScreen");
}

#[test]
fn a_status_from_the_running_app_shows_on_the_row() {
    let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut statuses = HashMap::new();
    statuses.insert(KeyId::Screenshot, Status::Taken);
    let mut v = SettingsView::new(Config::default(), statuses).with_probe(free);
    click(&mut v, Action::SwitchTab(Tab::Keys));
    snapshot(&mut v, "keys-taken");
    let (msg, kind) = v.message().expect("the reason is shown");
    assert_eq!(kind, MsgKind::Error);
    assert!(
        msg.contains("Ctrl+PrintScreen is taken by another app"),
        "{msg}"
    );
    // Re-binding it clears the problem.
    click(&mut v, Action::EditKey(KeyId::Screenshot));
    press(&mut v, Mods::CTRL | Mods::ALT, Key::Char('p'));
    assert_eq!(v.config.hotkey.screenshot, "Ctrl+Alt+P");
}

#[test]
fn tool_keys_ignore_shift_and_refuse_modifiers() {
    let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut v = view();
    click(&mut v, Action::SwitchTab(Tab::Keys));
    v.scroll_by(-100.0); // to the bottom
    click(&mut v, Action::EditKey(KeyId::Tool(13)));
    press(&mut v, Mods::SHIFT, Key::Char('k'));
    assert_eq!(v.config.shortcuts.measurement, "K");
    click(&mut v, Action::EditKey(KeyId::Tool(13)));
    press(&mut v, Mods::CTRL, Key::Char('k'));
    assert_eq!(v.editing(), Some(KeyId::Tool(13)));
    assert_eq!(v.message().unwrap().1, MsgKind::Error);
}

#[test]
fn conflicts_block_save_until_fixed() {
    let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let home = TempHome::new("conflict");
    let mut v = view();
    click(&mut v, Action::SwitchTab(Tab::Keys));
    click(&mut v, Action::EditKey(KeyId::Copy));
    press(&mut v, Mods::CTRL, Key::Char('s'));
    assert_eq!(v.config.keys.copy, "Ctrl+S", "accepted, but flagged");
    let (msg, kind) = v.message().unwrap();
    assert_eq!(kind, MsgKind::Error);
    assert!(msg.contains("Save"), "{msg}");
    snapshot(&mut v, "keys-conflict");

    // Save from another tab: refused, and it jumps to the problem.
    click(&mut v, Action::SwitchTab(Tab::General));
    assert_eq!(click(&mut v, Action::Save), Outcome::Redraw);
    assert_eq!(v.tab, Tab::Keys);
    assert!(v.message().unwrap().0.starts_with("Can't save yet"));
    assert!(!home.root.join("config/screen-mark/config.toml").exists());

    // Reset that one row; now it saves.
    click(&mut v, Action::ResetKey(KeyId::Copy));
    assert_eq!(v.config.keys.copy, "Ctrl+C");
    assert_eq!(click(&mut v, Action::Save), Outcome::Close { saved: true });
    let written = Config::load();
    assert_eq!(written.keys.copy, "Ctrl+C");
}

#[test]
fn a_problem_far_down_the_list_is_scrolled_into_view() {
    let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _home = TempHome::new("reveal");
    let mut v = view();
    click(&mut v, Action::SwitchTab(Tab::Keys));
    // Eyedropper and Measurement sit at the bottom; give Eyedropper M too.
    v.scroll_by(-10_000.0); // to the bottom
    click(&mut v, Action::EditKey(KeyId::Tool(12)));
    press(&mut v, Mods::NONE, Key::Char('m'));
    assert!(v.message().unwrap().0.contains("Measurement"));
    // Another tab (which scrolls the Keys list back to the top), then Save.
    click(&mut v, Action::SwitchTab(Tab::General));
    click(&mut v, Action::Save);
    assert_eq!(v.tab, Tab::Keys);
    v.render();
    assert!(
        v.hit_rect(Action::EditKey(KeyId::Tool(12))).is_some(),
        "Eyedropper row scrolled into view"
    );
    assert!(v.message().unwrap().0.contains("Measurement"));
}

#[test]
fn a_problem_already_in_the_config_does_not_block_other_changes() {
    let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _home = TempHome::new("preexisting");
    // A hand-edited config: a duplicate tool key and a hotkey that can't work.
    let mut config = Config::default();
    config.shortcuts.eyedropper = "m".into();
    config.hotkey.screenshot = "Shift+S".into();
    let mut v = SettingsView::new(config, HashMap::new()).with_probe(free);
    click(&mut v, Action::SwitchTab(Tab::General));
    click(&mut v, Action::ToggleTheme);
    assert_eq!(click(&mut v, Action::Save), Outcome::Close { saved: true });
    let written = Config::load();
    assert_eq!(written.general.theme, "light");
    assert_eq!(written.hotkey.screenshot, "Shift+S", "left as it was");
}

#[test]
fn a_one_modifier_hotkey_is_accepted_with_a_caution() {
    let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut v = view();
    click(&mut v, Action::SwitchTab(Tab::Keys));
    click(&mut v, Action::EditKey(KeyId::Region));
    press(&mut v, Mods::ALT, Key::Char('q'));
    assert_eq!(v.editing(), None);
    assert_eq!(v.config.hotkey.region, "Alt+Q");
    let (msg, kind) = v.message().unwrap();
    assert_eq!(kind, MsgKind::Info);
    assert!(msg.contains("Other apps won't see Alt+Q"), "{msg}");
}

#[test]
fn printscreen_keeps_the_modifier_released_just_before_it() {
    let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut v = view();
    click(&mut v, Action::SwitchTab(Tab::Keys));
    click(&mut v, Action::EditKey(KeyId::Screenshot));
    // Windows: Ctrl down, (no PrintScreen down event), Ctrl up, PrintScreen up.
    modifier(&mut v, Mods::CTRL, true);
    modifier(&mut v, Mods::NONE, false);
    release(&mut v, Mods::NONE, Key::Named(Named::PrintScreen));
    assert_eq!(v.config.hotkey.screenshot, "Ctrl+PrintScreen");

    // A modifier let go well before PrintScreen doesn't count.
    click(&mut v, Action::EditKey(KeyId::Screenshot));
    modifier(&mut v, Mods::ALT, true);
    modifier(&mut v, Mods::NONE, false);
    std::thread::sleep(std::time::Duration::from_millis(1100));
    release(&mut v, Mods::NONE, Key::Named(Named::PrintScreen));
    assert_eq!(v.config.hotkey.screenshot, "PrintScreen");
}

#[test]
fn losing_focus_stops_listening() {
    let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut v = view();
    click(&mut v, Action::SwitchTab(Tab::Keys));
    assert!(!v.stop_listening(), "nothing to stop");
    click(&mut v, Action::EditKey(KeyId::Region));
    assert!(v.stop_listening());
    assert_eq!(v.editing(), None, "global hotkeys come back");
    assert_eq!(v.config.hotkey.region, "Ctrl+Shift+S");
}

#[test]
fn reset_all_keys_restores_every_default() {
    let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut config = Config::default();
    config.hotkey.region = "Ctrl+Alt+R".into();
    config.keys.copy = "".into();
    config.shortcuts.arrow = "q".into();
    let mut v = SettingsView::new(config, HashMap::new()).with_probe(free);
    click(&mut v, Action::SwitchTab(Tab::Keys));
    click(&mut v, Action::ResetAllKeys);
    assert_eq!(v.config.hotkey.region, "Ctrl+Shift+S");
    assert_eq!(v.config.keys.copy, "Ctrl+C");
    assert_eq!(v.config.shortcuts.arrow, "A");
}

#[test]
fn reset_buttons_only_show_for_changed_rows() {
    let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut v = view();
    click(&mut v, Action::SwitchTab(Tab::Keys));
    v.render();
    assert!(v.hit_rect(Action::ResetKey(KeyId::Region)).is_none());
    click(&mut v, Action::EditKey(KeyId::Region));
    press(&mut v, Mods::CTRL | Mods::ALT, Key::Char('r'));
    v.render();
    assert!(v.hit_rect(Action::ResetKey(KeyId::Region)).is_some());
}

#[test]
fn rebinding_one_global_to_the_others_keys_is_a_conflict_not_taken() {
    let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // The probe would say "taken" (by us); the view must not ask it.
    let mut v = SettingsView::new(Config::default(), HashMap::new()).with_probe(taken);
    click(&mut v, Action::SwitchTab(Tab::Keys));
    click(&mut v, Action::EditKey(KeyId::Screenshot));
    press(&mut v, Mods::CTRL | Mods::SHIFT, Key::Char('s'));
    assert_eq!(v.config.hotkey.screenshot, "Ctrl+Shift+S");
    assert!(v.message().unwrap().0.contains("Capture a region"));
}

#[test]
fn clicking_elsewhere_stops_listening() {
    let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut v = view();
    click(&mut v, Action::SwitchTab(Tab::Keys));
    click(&mut v, Action::EditKey(KeyId::Copy));
    assert!(v.editing().is_some());
    v.click(5.0, 5.0);
    assert!(v.editing().is_none());
    // Clicking the listening button again also stops.
    click(&mut v, Action::EditKey(KeyId::Copy));
    click(&mut v, Action::EditKey(KeyId::Copy));
    assert!(v.editing().is_none());
}

#[test]
fn escape_closes_without_saving_when_not_listening() {
    let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut v = view();
    assert_eq!(
        press(&mut v, Mods::NONE, Key::Named(Named::Escape)),
        Outcome::Close { saved: false }
    );
    assert_eq!(
        click(&mut v, Action::Cancel),
        Outcome::Close { saved: false }
    );
}

#[test]
fn capture_tab_options() {
    let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut v = view();
    click(&mut v, Action::SwitchTab(Tab::Capture));
    assert!(!v.config.screenshot_all_monitors());
    click(&mut v, Action::ScreenshotAll(true));
    assert!(v.config.screenshot_all_monitors());
    click(&mut v, Action::ToggleFinishOnRelease);
    assert!(v.config.general.finish_on_release);
    click(&mut v, Action::ToggleCopyAlsoSaves);
    assert!(v.config.general.copy_also_saves);
    click(&mut v, Action::Notifications(NotificationMode::System));
    assert_eq!(v.config.notification_mode(), NotificationMode::System);
    click(&mut v, Action::Notifications(NotificationMode::Off));
    assert_eq!(v.config.notification_mode(), NotificationMode::Off);
    snapshot(&mut v, "capture-changed");
}

#[test]
fn ask_where_to_save_keeps_2_0_behaviour_until_changed() {
    let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // 2.0 config without a folder: Ctrl+S asked.
    let mut v = view();
    assert!(v.config.ask_where_to_save());
    click(&mut v, Action::SwitchTab(Tab::Capture));
    click(&mut v, Action::ToggleAskWhere);
    assert_eq!(v.config.general.ask_where_to_save, Some(false));
    // 2.0 config with a folder: Ctrl+S saved silently.
    let mut c = Config::default();
    c.general.save_directory = std::env::temp_dir().to_string_lossy().into_owned();
    let mut v = SettingsView::new(c, HashMap::new());
    assert!(!v.config.ask_where_to_save());
    // ...unless that folder was missing (an unplugged drive): it asked.
    let mut gone = Config::default();
    gone.general.save_directory = std::env::temp_dir()
        .join("sm-no-such-folder")
        .to_string_lossy()
        .into_owned();
    assert!(gone.ask_where_to_save());
    // "Default" folder keeps the current answer instead of flipping it.
    click(&mut v, Action::SwitchTab(Tab::Capture));
    click(&mut v, Action::DefaultDir);
    assert!(v.config.general.save_directory.is_empty());
    assert!(!v.config.ask_where_to_save());
}

#[test]
fn keys_list_scrolls_and_clamps() {
    let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut v = view();
    click(&mut v, Action::SwitchTab(Tab::Keys));
    v.render();
    assert!(
        v.hit_rect(Action::EditKey(KeyId::Tool(13))).is_none(),
        "last tool starts below the fold"
    );
    assert!(v.scroll_by(-3.0));
    assert!(v.scroll_by(-100.0));
    assert!(!v.scroll_by(-100.0), "already at the bottom");
    v.render();
    assert!(v.hit_rect(Action::EditKey(KeyId::Tool(13))).is_some());
    snapshot(&mut v, "keys-bottom");
    assert!(v.scroll_by(100.0));
    v.render();
    assert!(v.hit_rect(Action::EditKey(KeyId::Region)).is_some());
}
