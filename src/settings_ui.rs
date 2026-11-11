//! The Settings window.
//!
//! `SettingsView` holds all the state and draws into a `Pixmap`; it takes
//! clicks (logical pixels) and key presses (`keys::KeyPress`) and needs no
//! window, so tests can drive it end to end. `SettingsWindow` wraps it with a
//! winit window and a softbuffer surface.
//!
//! Rebinding: click a key button, press the new combination. Esc cancels,
//! Backspace or Delete leaves the action unbound. A combination that breaks
//! the rules for that kind of key, or (for the two global hotkeys) is
//! already owned by another program, is refused on the spot with the
//! reason, and the button keeps listening. Conflicts between two bindings
//! are shown in red on both rows and Save refuses until one is changed.
//! While a button is listening the app releases its global hotkeys, so any
//! combination, including the current ones, reaches this window.
//!
//! Keyboard: Tab and Shift+Tab (or the arrow keys) move between controls,
//! Enter or Space presses the focused one, Ctrl+Tab changes tab, Ctrl+Enter
//! saves, Esc closes. Every control is also described in `widgets()` (what
//! it is, what it is called, whether it is on), which is what a screen
//! reader is told.

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tiny_skia::{
    Color as SkiaColor, Paint, PathBuilder, Pixmap, PixmapPaint, Rect, Stroke, Transform,
};
use winit::window::Window;

pub use crate::access::Role;
use crate::access::{Control, Request, Snapshot};
use crate::config::{default_save_folder, Config, NotificationMode};
use crate::geometry::{Color, Point};
use crate::hotkey::Status;
use crate::keys::{caution, validate, Binding, Key, KeyId, KeyPress, Mods, Named, Scope};
use crate::tools::{measure_text_width, render_text_annotation};

/// Logical size of the window.
pub const WIN_W: u32 = 440;
pub const WIN_H: u32 = 560;

const LEFT: f32 = 24.0;
const CONTENT_TOP: f32 = 98.0;
const FOOTER_TOP: f32 = WIN_H as f32 - 92.0;

/// Color choices available in settings (name, R, G, B).
const COLOR_CHOICES: &[(&str, u8, u8, u8)] = &[
    ("red", 0xff, 0x6e, 0x7a),
    ("blue", 0x5c, 0xb8, 0xff),
    ("green", 0x3d, 0xdc, 0x97),
    ("yellow", 0xff, 0xc2, 0x47),
    ("mauve", 0xb7, 0x8c, 0xff),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    General,
    Capture,
    Keys,
    Toolbar,
}

const TABS: [(Tab, &str); 4] = [
    (Tab::General, "General"),
    (Tab::Capture, "Capture"),
    (Tab::Keys, "Keys"),
    (Tab::Toolbar, "Toolbar"),
];

/// Something clickable.
/// The settings a folder picker can fill in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Picker {
    SaveFolder,
    UpdateFolder,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    SwitchTab(Tab),
    SelectColor(usize),
    ThicknessDown,
    ThicknessUp,
    ToggleTheme,
    ToggleHistory,
    ToggleAutostart,
    ToggleUpdateCheck,
    ScreenshotAll(bool),
    ToggleFinishOnRelease,
    ToggleMagnifier,
    ToggleCopyAlsoSaves,
    ToggleAskWhere,
    BrowseDir,
    DefaultDir,
    BrowseUpdateDir,
    UpdatesFromGitHub,
    Notifications(NotificationMode),
    EditKey(KeyId),
    ResetKey(KeyId),
    ResetAllKeys,
    ToggleToolbar(usize),
    Save,
    Cancel,
}

/// What the caller should do after an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Nothing,
    Redraw,
    /// Close the window; `saved` = config.toml was written.
    Close {
        saved: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsgKind {
    Info,
    Error,
}

struct HitRect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    action: Action,
}

/// One control, as the last `render` drew it: what keyboard focus moves
/// between, and what a screen reader announces.
#[derive(Debug, Clone, PartialEq)]
pub struct Widget {
    pub action: Action,
    pub role: Role,
    /// What it is called.
    pub label: String,
    /// On or off, for a check box; chosen or not, for a tab or radio button.
    pub checked: Option<bool>,
    /// What it currently holds (a key button's binding).
    pub value: Option<String>,
    /// Where it is in the window, in logical pixels: (x, y, w, h). `None`
    /// while it is scrolled out of view.
    pub rect: Option<(f32, f32, f32, f32)>,
}

impl HitRect {
    fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }
}

/// All Settings state, no window.
pub struct SettingsView {
    pub config: Config,
    pub tab: Tab,
    /// The key button that is listening for a new combination.
    editing: Option<KeyId>,
    /// Modifiers held while listening (shown live on the button).
    held: Mods,
    /// Every modifier held since the last one went down, kept for a moment
    /// after they're all released: Windows reports PrintScreen only on
    /// key-up, and people often let go of Ctrl first.
    peak: Mods,
    peak_until: Option<Instant>,
    /// The bindings as they were when Settings opened. A problem with one
    /// the user hasn't touched (say, from a hand-edited config) is shown
    /// but doesn't block saving other changes.
    loaded: HashMap<KeyId, String>,
    hits: Vec<HitRect>,
    /// Every control of the current tab, in reading order (see `Widget`).
    widgets: Vec<Widget>,
    /// The control with keyboard focus: its action, and which of the
    /// widgets with that action it is (the two theme buttons share one).
    focus: Option<(Action, usize)>,
    /// Draw the focus ring. Set by the keyboard, cleared by the mouse.
    focus_ring: bool,
    hovered: Option<usize>,
    cursor: (f32, f32),
    scroll: f32,
    max_scroll: f32,
    message: Option<(String, MsgKind)>,
    /// Global hotkey statuses (from the running app, updated on capture).
    statuses: HashMap<KeyId, Status>,
    browse_rx: Option<Receiver<Option<String>>>,
    /// Which setting the open folder picker is for.
    browse_for: Picker,
    autostart: bool,
    probe: fn(Binding) -> Status,
}

impl SettingsView {
    pub fn new(config: Config, statuses: HashMap<KeyId, Status>) -> Self {
        let loaded = KeyId::all()
            .into_iter()
            .map(|id| (id, config.binding(id)))
            .collect();
        Self {
            config,
            tab: Tab::General,
            editing: None,
            held: Mods::NONE,
            peak: Mods::NONE,
            peak_until: None,
            loaded,
            hits: Vec::new(),
            widgets: Vec::new(),
            focus: None,
            focus_ring: false,
            hovered: None,
            cursor: (-1.0, -1.0),
            scroll: 0.0,
            max_scroll: 0.0,
            message: None,
            statuses,
            browse_rx: None,
            browse_for: Picker::SaveFolder,
            autostart: crate::autostart::is_enabled(),
            probe: crate::hotkey::check_available,
        }
    }

    /// Replace the "is this combo free?" check (tests).
    pub fn with_probe(mut self, probe: fn(Binding) -> Status) -> Self {
        self.probe = probe;
        self
    }

    /// The key button that is listening, if any. While this is `Some` the
    /// app must suspend its global hotkeys.
    pub fn editing(&self) -> Option<KeyId> {
        self.editing
    }

    /// Stop listening for a combination (the window lost focus, so the
    /// keys would go elsewhere). Returns true if it was listening.
    pub fn stop_listening(&mut self) -> bool {
        if self.editing.take().is_none() {
            return false;
        }
        self.held = Mods::NONE;
        self.peak = Mods::NONE;
        self.peak_until = None;
        self.message = Some((
            "Stopped listening. Click the key again to change it.".into(),
            MsgKind::Info,
        ));
        true
    }

    pub fn message(&self) -> Option<(&str, MsgKind)> {
        self.message.as_ref().map(|(s, k)| (s.as_str(), *k))
    }

    pub fn open_tab(&mut self, tab: Tab) {
        self.tab = tab;
        self.editing = None;
        self.scroll = 0.0;
    }

    /// The screen rectangle of an action's button after the last render.
    pub fn hit_rect(&self, action: Action) -> Option<(f32, f32, f32, f32)> {
        self.hits
            .iter()
            .find(|h| h.action == action)
            .map(|h| (h.x, h.y, h.w, h.h))
    }

    // ───────────── input ─────────────

    pub fn hover(&mut self, x: f32, y: f32) -> bool {
        self.cursor = (x, y);
        let old = self.hovered;
        self.hovered = self.hits.iter().position(|h| h.contains(x, y));
        old != self.hovered
    }

    /// Every control of the current tab, in reading order, as of the last
    /// `render`.
    pub fn widgets(&self) -> &[Widget] {
        &self.widgets
    }

    /// The control with keyboard focus, if any.
    pub fn focused(&self) -> Option<&Widget> {
        self.focus_index().map(|i| &self.widgets[i])
    }

    /// Whether the focus ring is drawn (the keyboard was used last).
    pub fn focus_visible(&self) -> bool {
        self.focus_ring && self.focus_index().is_some()
    }

    fn focus_index(&self) -> Option<usize> {
        let (action, nth) = self.focus?;
        self.widgets
            .iter()
            .enumerate()
            .filter(|(_, w)| w.action == action)
            .map(|(i, _)| i)
            .nth(nth)
    }

    fn set_focus(&mut self, index: usize) {
        let Some(widget) = self.widgets.get(index) else {
            self.focus = None;
            return;
        };
        let action = widget.action;
        let nth = self.widgets[..index]
            .iter()
            .filter(|w| w.action == action)
            .count();
        self.focus = Some((action, nth));
        // A key row scrolled out of view comes into it.
        if let Action::EditKey(id) | Action::ResetKey(id) = action {
            self.reveal(id);
        }
    }

    /// Move focus to a screen reader's choice, or a test's.
    pub fn focus_action(&mut self, action: Action) -> bool {
        self.layout();
        match self.widgets.iter().position(|w| w.action == action) {
            Some(i) => {
                self.set_focus(i);
                self.focus_ring = true;
                true
            }
            None => false,
        }
    }

    /// The window as a screen reader should present it, as of the last
    /// `render`.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            title: "screen-mark Settings".to_string(),
            controls: self
                .widgets
                .iter()
                .map(|w| Control {
                    role: w.role,
                    label: w.label.clone(),
                    checked: w.checked,
                    value: w.value.clone(),
                    rect: w.rect,
                })
                .collect(),
            focus: self.focus_index(),
            status: self.message.as_ref().map(|(text, _)| text.clone()),
        }
    }

    /// Carry out a screen reader's request on control `index` of `snapshot`.
    pub fn request(&mut self, request: Request) -> Outcome {
        self.layout();
        let (Request::Press(index) | Request::Focus(index)) = request;
        let Some(action) = self.widgets.get(index).map(|w| w.action) else {
            return Outcome::Nothing;
        };
        self.set_focus(index);
        self.focus_ring = true;
        match request {
            Request::Press(_) => self.act(action),
            Request::Focus(_) => Outcome::Redraw,
        }
    }

    /// Bring `widgets` up to date with the state (a key press can arrive
    /// between two draws).
    fn layout(&mut self) {
        let _ = self.render();
    }

    /// A key press while no key button is listening: move between controls
    /// and press them.
    fn navigate(&mut self, press: KeyPress) -> Outcome {
        let Some(Key::Named(key)) = press.key else {
            return Outcome::Nothing;
        };
        let mods = press.mods;
        let plain = mods.is_empty();
        match key {
            Named::Escape if plain => Outcome::Close { saved: false },
            Named::Tab if mods.ctrl() => self.step_tab(if mods.shift() { -1 } else { 1 }),
            Named::Tab if !mods.has_command() => self.step_focus(if mods.shift() { -1 } else { 1 }),
            Named::Down | Named::Right if plain => self.arrow(1),
            Named::Up | Named::Left if plain => self.arrow(-1),
            Named::Home if plain => self.focus_edge(false),
            Named::End if plain => self.focus_edge(true),
            Named::PageDown | Named::PageUp if plain => {
                let page = (FOOTER_TOP - CONTENT_TOP) / 26.0 - 1.0;
                let lines = if key == Named::PageDown { -page } else { page };
                if self.scroll_by(lines) {
                    Outcome::Redraw
                } else {
                    Outcome::Nothing
                }
            }
            Named::Enter if mods == Mods::CTRL => {
                self.focus_ring = true;
                self.act(Action::Save)
            }
            Named::Enter | Named::Space if plain => {
                self.layout();
                match self.focused().map(|w| w.action) {
                    Some(action) => {
                        self.focus_ring = true;
                        self.act(action)
                    }
                    None => Outcome::Nothing,
                }
            }
            _ => Outcome::Nothing,
        }
    }

    /// Focus the next (`1`) or previous (`-1`) control, wrapping round.
    fn step_focus(&mut self, by: i32) -> Outcome {
        self.layout();
        let count = self.widgets.len() as i32;
        if count == 0 {
            return Outcome::Nothing;
        }
        let next = match self.focus_index() {
            Some(i) => (i as i32 + by).rem_euclid(count),
            // Nothing focused yet: Tab starts at the top, Shift+Tab at the end.
            None if by > 0 => 0,
            None => count - 1,
        };
        self.set_focus(next as usize);
        self.focus_ring = true;
        Outcome::Redraw
    }

    /// An arrow key: on the tab strip it changes tab, anywhere else it
    /// moves focus like Tab.
    fn arrow(&mut self, by: i32) -> Outcome {
        self.layout();
        match self.focused().map(|w| w.role) {
            Some(Role::Tab) => self.step_tab(by),
            _ => self.step_focus(by),
        }
    }

    /// Switch to the next or previous tab and focus its tab button.
    fn step_tab(&mut self, by: i32) -> Outcome {
        let current = TABS.iter().position(|(t, _)| *t == self.tab).unwrap_or(0);
        let next = (current as i32 + by).rem_euclid(TABS.len() as i32) as usize;
        self.open_tab(TABS[next].0);
        self.message = None;
        self.focus = Some((Action::SwitchTab(TABS[next].0), 0));
        self.focus_ring = true;
        Outcome::Redraw
    }

    fn focus_edge(&mut self, last: bool) -> Outcome {
        self.layout();
        if self.widgets.is_empty() {
            return Outcome::Nothing;
        }
        let index = if last { self.widgets.len() - 1 } else { 0 };
        self.set_focus(index);
        self.focus_ring = true;
        Outcome::Redraw
    }

    pub fn click(&mut self, x: f32, y: f32) -> Outcome {
        self.cursor = (x, y);
        let action = self
            .hits
            .iter()
            .find(|h| h.contains(x, y))
            .map(|h| h.action);
        // The mouse takes focus with it, without the ring.
        self.focus_ring = false;
        if let Some(i) = self
            .widgets
            .iter()
            .position(|w| w.rect.is_some_and(|r| within(r, x, y)))
        {
            self.set_focus(i);
        }
        match action {
            Some(a) => self.act(a),
            None => {
                // Clicking elsewhere stops listening.
                if self.editing.take().is_some() {
                    self.message = None;
                    Outcome::Redraw
                } else {
                    Outcome::Nothing
                }
            }
        }
    }

    pub fn scroll_by(&mut self, lines: f32) -> bool {
        if self.tab != Tab::Keys {
            return false;
        }
        self.max_scroll = keys_max_scroll();
        let new = (self.scroll - lines * 26.0).clamp(0.0, self.max_scroll);
        let changed = (new - self.scroll).abs() > f32::EPSILON;
        self.scroll = new;
        changed
    }

    pub fn act(&mut self, action: Action) -> Outcome {
        // Any click other than the listening button itself stops listening.
        if !matches!(action, Action::EditKey(_)) {
            self.editing = None;
        }
        let g = &mut self.config.general;
        match action {
            Action::SwitchTab(t) => {
                self.open_tab(t);
                self.message = None;
            }
            Action::SelectColor(i) => {
                if let Some(&(name, ..)) = COLOR_CHOICES.get(i) {
                    g.default_color = name.to_string();
                }
            }
            Action::ThicknessDown => g.default_thickness = (g.default_thickness - 0.5).max(1.0),
            Action::ThicknessUp => g.default_thickness = (g.default_thickness + 0.5).min(20.0),
            Action::ToggleTheme => {
                let light = !g.theme.eq_ignore_ascii_case("light");
                g.theme = if light { "light" } else { "dark" }.to_string();
                crate::theme::set_mode(self.config.theme_mode());
            }
            Action::ToggleHistory => g.history_enabled = !g.history_enabled,
            Action::ToggleAutostart => {
                let new_state = !self.autostart;
                match crate::autostart::set_enabled(new_state) {
                    Ok(()) => self.autostart = new_state,
                    Err(e) => {
                        self.message = Some((format!("Auto-start: {e}"), MsgKind::Error));
                    }
                }
            }
            Action::ScreenshotAll(all) => {
                g.screenshot_area = if all { "all" } else { "monitor" }.to_string();
            }
            Action::ToggleFinishOnRelease => g.finish_on_release = !g.finish_on_release,
            Action::ToggleMagnifier => g.magnifier = !g.magnifier,
            Action::ToggleUpdateCheck => {
                self.config.updates.auto_check = !self.config.updates.auto_check;
            }
            Action::ToggleCopyAlsoSaves => g.copy_also_saves = !g.copy_also_saves,
            Action::ToggleAskWhere => {
                let now = self.config.ask_where_to_save();
                self.config.general.ask_where_to_save = Some(!now);
            }
            Action::BrowseDir => {
                let start = self.config.save_folder();
                self.pick_folder(start, Picker::SaveFolder);
            }
            Action::BrowseUpdateDir => {
                let start = std::path::PathBuf::from(self.config.updates.url.trim());
                self.pick_folder(start, Picker::UpdateFolder);
            }
            Action::UpdatesFromGitHub => self.config.updates.url.clear(),
            Action::DefaultDir => {
                // Keep Ctrl+S behaving as it did with the old folder.
                let ask = self.config.ask_where_to_save();
                self.config.general.save_directory.clear();
                self.config.general.ask_where_to_save = Some(ask);
            }
            Action::Notifications(mode) => {
                g.notifications = match mode {
                    NotificationMode::Toast => "toast",
                    NotificationMode::System => "system",
                    NotificationMode::Off => "off",
                }
                .to_string();
            }
            Action::EditKey(id) => {
                if self.editing == Some(id) {
                    self.editing = None;
                    self.message = None;
                } else {
                    self.editing = Some(id);
                    self.held = Mods::NONE;
                    self.peak = Mods::NONE;
                    self.peak_until = None;
                    self.message = Some((
                        "Press the new keys. Esc cancels, Backspace leaves it unbound.".into(),
                        MsgKind::Info,
                    ));
                }
            }
            Action::ResetKey(id) => {
                self.config
                    .set_binding(id, id.default_binding().to_string());
                self.statuses.remove(&id);
                self.message = Some((
                    format!(
                        "{}: back to {}",
                        id.label(),
                        display_binding(id.default_binding())
                    ),
                    MsgKind::Info,
                ));
            }
            Action::ResetAllKeys => {
                self.config.reset_bindings();
                self.statuses.clear();
                self.message = Some(("Every key is back to its default.".into(), MsgKind::Info));
            }
            Action::ToggleToolbar(i) => self.config.toolbar.toggle_by_index(i),
            Action::Save => return self.try_save(),
            Action::Cancel => return Outcome::Close { saved: false },
        }
        Outcome::Redraw
    }

    fn try_save(&mut self) -> Outcome {
        let report = self.config.key_report();
        // Only problems with bindings changed here block saving (a conflict
        // always involves the changed one, so it's caught too).
        let blocking = KeyId::all()
            .into_iter()
            .filter(|id| Some(&self.config.binding(*id)) != self.loaded.get(id))
            .find_map(|id| report.problem(id).map(|p| (id, p)));
        if let Some((id, problem)) = blocking {
            self.open_tab(Tab::Keys);
            self.reveal(id);
            self.message = Some((format!("Can't save yet: {problem}"), MsgKind::Error));
            return Outcome::Redraw;
        }
        match self.config.save() {
            Ok(()) => Outcome::Close { saved: true },
            Err(e) => {
                self.message = Some((format!("Couldn't save: {e}"), MsgKind::Error));
                Outcome::Redraw
            }
        }
    }

    /// Scroll the Keys list so `id`'s row is visible.
    fn reveal(&mut self, id: KeyId) {
        if let Some(row_y) = keys_layout().iter().find(|r| r.id == Some(id)).map(|r| r.y) {
            let view_h = FOOTER_TOP - CONTENT_TOP - 8.0;
            if row_y < self.scroll {
                self.scroll = row_y;
            } else if row_y + 26.0 > self.scroll + view_h {
                self.scroll = row_y + 26.0 - view_h;
            }
            self.scroll = self.scroll.max(0.0);
        }
    }

    /// A key event from the window. `press.key == None` means a bare
    /// modifier changed.
    pub fn key(&mut self, press: KeyPress) -> Outcome {
        let Some(id) = self.editing else {
            return if press.pressed {
                self.navigate(press)
            } else {
                Outcome::Nothing
            };
        };

        let now = Instant::now();
        self.note_mods(press.mods, now);
        let Some(key) = press.key else {
            // Show held modifiers live ("Ctrl+Shift+...").
            if self.held != press.mods {
                self.held = press.mods;
                return Outcome::Redraw;
            }
            return Outcome::Nothing;
        };

        // Windows never delivers PrintScreen's key-down to a window, only
        // its key-up, so accept that one key on release.
        if !press.pressed && key != Key::Named(Named::PrintScreen) {
            return Outcome::Nothing;
        }
        let mut press = press;
        if !press.pressed && self.peak_until.is_none_or(|t| now <= t) {
            // PrintScreen's key-up: count modifiers released just before it.
            press.mods = press.mods | self.peak;
        }

        if press.mods.is_empty() {
            match key {
                Key::Named(Named::Escape) => {
                    self.editing = None;
                    self.message = None;
                    return Outcome::Redraw;
                }
                Key::Named(Named::Backspace) | Key::Named(Named::Delete) => {
                    self.config.set_binding(id, String::new());
                    self.statuses.remove(&id);
                    self.editing = None;
                    self.message = Some((format!("{} is now unbound.", id.label()), MsgKind::Info));
                    return Outcome::Redraw;
                }
                _ => {}
            }
        }

        let mut binding = Binding::new(press.mods, key);
        if id.scope() == Scope::Tool {
            // Tool keys are single keys; Shift (or Caps Lock) is ignored.
            binding.mods = binding.mods.without_shift();
        }
        if let Err(why) = validate(binding, id.scope()) {
            self.message = Some((why, MsgKind::Error));
            return Outcome::Redraw; // keep listening
        }
        if id.scope() == Scope::Global {
            // A combination screen-mark itself already uses for another
            // hotkey is a conflict, reported below, not "taken".
            let ours = KeyId::GLOBALS.iter().any(|o| {
                *o != id && Binding::parse(&self.config.binding(*o)).ok().flatten() == Some(binding)
            });
            if !ours {
                let status = (self.probe)(binding);
                if status == Status::Taken {
                    self.message = Some((
                        format!("{binding} is taken by another app. Try another combination."),
                        MsgKind::Error,
                    ));
                    return Outcome::Redraw; // keep listening
                }
                self.statuses.insert(id, status);
            }
        }
        self.config.set_binding(id, binding.to_string());
        self.editing = None;
        self.held = Mods::NONE;
        self.peak = Mods::NONE;
        self.peak_until = None;
        let report = self.config.key_report();
        self.message = Some(match report.problem(id) {
            Some(p) => (
                format!("{p}. Change one of them before saving."),
                MsgKind::Error,
            ),
            None => match caution(binding, id.scope()) {
                Some(c) => (format!("{}: {binding}. {c}", id.label()), MsgKind::Info),
                None => (format!("{}: {binding}", id.label()), MsgKind::Info),
            },
        });
        Outcome::Redraw
    }

    /// Track `peak` (see the field).
    fn note_mods(&mut self, mods: Mods, now: Instant) {
        const GRACE: Duration = Duration::from_millis(1000);
        if !mods.is_empty() {
            if self.peak_until.is_some_and(|t| now > t) {
                self.peak = Mods::NONE;
            }
            self.peak = self.peak | mods;
            self.peak_until = None;
        } else if !self.peak.is_empty() && self.peak_until.is_none() {
            self.peak_until = Some(now + GRACE);
        }
    }

    /// Apply a finished folder-picker result. Returns true to redraw.
    pub fn poll_browse(&mut self) -> bool {
        let Some(rx) = &self.browse_rx else {
            return false;
        };
        match rx.try_recv() {
            Ok(Some(dir)) => {
                self.picked_folder(self.browse_for, dir);
                self.browse_rx = None;
                true
            }
            Ok(None) | Err(TryRecvError::Disconnected) => {
                self.browse_rx = None;
                false
            }
            Err(TryRecvError::Empty) => false,
        }
    }

    pub fn browse_pending(&self) -> bool {
        self.browse_rx.is_some()
    }

    /// Open a folder picker on a worker thread, for `target`. One at a time.
    fn pick_folder(&mut self, start: std::path::PathBuf, target: Picker) {
        if self.browse_rx.is_some() {
            return;
        }
        let (tx, rx) = channel();
        self.browse_rx = Some(rx);
        self.browse_for = target;
        std::thread::spawn(move || {
            let mut dlg = rfd::FileDialog::new();
            if start.is_dir() {
                dlg = dlg.set_directory(&start);
            }
            let _ = tx.send(dlg.pick_folder().map(|p| p.to_string_lossy().to_string()));
        });
    }

    /// Use `dir`, which the folder picker returned, for `target`. Public so
    /// tests can stand in for the picker.
    pub fn picked_folder(&mut self, target: Picker, dir: String) {
        match target {
            Picker::SaveFolder => {
                let ask = self.config.ask_where_to_save();
                self.config.general.save_directory = dir;
                // Picking a folder used to also mean "stop asking"; keep
                // whatever the checkbox showed instead.
                self.config.general.ask_where_to_save = Some(ask);
            }
            Picker::UpdateFolder => self.config.updates.url = dir,
        }
    }

    // ───────────── drawing ─────────────

    /// Draw the whole window at logical size.
    pub fn render(&mut self) -> Pixmap {
        let mut p = Pixmap::new(WIN_W, WIN_H).expect("settings size is non-zero");
        fill(
            &mut p,
            0.0,
            0.0,
            WIN_W as f32,
            WIN_H as f32,
            crate::theme::bg_1(),
        );
        self.hits.clear();
        self.widgets.clear();

        label(
            &mut p,
            LEFT,
            20.0,
            "screen-mark settings",
            16.0,
            crate::theme::text_normal(),
        );

        // Tabs
        let tab_y = 50.0;
        let tab_w = (WIN_W as f32 - LEFT * 2.0) / TABS.len() as f32;
        for (i, (tab, name)) in TABS.iter().enumerate() {
            let tx = LEFT + i as f32 * tab_w;
            let active = *tab == self.tab;
            if active {
                fill(&mut p, tx, tab_y, tab_w, 32.0, crate::theme::bg_3());
            } else if self.is_hovered(tx, tab_y, tab_w, 32.0) {
                fill(&mut p, tx, tab_y, tab_w, 32.0, crate::theme::bg_2());
            }
            let c = if active {
                crate::theme::accent()
            } else {
                crate::theme::text_muted()
            };
            let w = measure_text_width(name, 14.0);
            label(&mut p, tx + (tab_w - w) / 2.0, tab_y + 8.0, name, 14.0, c);
            self.widget(
                (tx, tab_y, tab_w, 32.0),
                Action::SwitchTab(*tab),
                Role::Tab,
                name,
                Some(active),
            );
        }
        fill(
            &mut p,
            LEFT,
            tab_y + 34.0,
            WIN_W as f32 - LEFT * 2.0,
            1.0,
            crate::theme::bg_3(),
        );

        match self.tab {
            Tab::General => self.render_general(&mut p),
            Tab::Capture => self.render_capture(&mut p),
            Tab::Keys => self.render_keys(&mut p),
            Tab::Toolbar => self.render_toolbar(&mut p),
        }

        // Footer: message line + buttons.
        fill(
            &mut p,
            0.0,
            FOOTER_TOP,
            WIN_W as f32,
            WIN_H as f32 - FOOTER_TOP,
            crate::theme::bg_1(),
        );
        fill(
            &mut p,
            LEFT,
            FOOTER_TOP,
            WIN_W as f32 - LEFT * 2.0,
            1.0,
            crate::theme::bg_3(),
        );
        if let Some((msg, kind)) = &self.message {
            let c = match kind {
                MsgKind::Error => crate::theme::red(),
                MsgKind::Info => crate::theme::text_soft(),
            };
            let max_w = WIN_W as f32 - LEFT * 2.0;
            for (i, line) in wrap(msg, 12.0, max_w).iter().take(2).enumerate() {
                label(
                    &mut p,
                    LEFT,
                    FOOTER_TOP + 8.0 + i as f32 * 16.0,
                    line,
                    12.0,
                    c,
                );
            }
        }
        let by = WIN_H as f32 - 50.0;
        if self.tab == Tab::Keys {
            self.button(
                &mut p,
                LEFT,
                by,
                124.0,
                32.0,
                "Reset all keys",
                Action::ResetAllKeys,
            );
        }
        let save_w = 128.0;
        let save_x = WIN_W as f32 - LEFT - save_w;
        self.button(
            &mut p,
            save_x - 96.0,
            by,
            88.0,
            32.0,
            "Cancel",
            Action::Cancel,
        );
        self.primary_button(
            &mut p,
            save_x,
            by,
            save_w,
            32.0,
            "Save & Close",
            Action::Save,
        );

        // A Reset button that just did its job is gone; its key button
        // takes the focus over.
        if self.focus_index().is_none() {
            if let Some((Action::ResetKey(id), _)) = self.focus {
                self.focus = Some((Action::EditKey(id), 0));
            }
        }
        if self.focus_ring {
            if let Some((x, y, w, h)) = self.focused().and_then(|w| w.rect) {
                // Two pixels wide, two pixels clear of the control, on whole
                // pixels (text-sized controls have fractional widths).
                let ring = crate::theme::accent();
                let (x0, y0) = (x.floor() - 2.0, y.floor() - 2.0);
                let (x1, y1) = ((x + w).ceil() + 2.0, (y + h).ceil() + 2.0);
                outline(
                    &mut p,
                    x0 - 1.0,
                    y0 - 1.0,
                    x1 - x0 + 2.0,
                    y1 - y0 + 2.0,
                    ring,
                );
                outline(&mut p, x0, y0, x1 - x0, y1 - y0, ring);
            }
        }
        p
    }

    fn render_general(&mut self, p: &mut Pixmap) {
        let mut y = CONTENT_TOP;
        section(p, y, "Default colour");
        y += 22.0;
        for (i, &(name, r, g, b)) in COLOR_CHOICES.iter().enumerate() {
            let sx = LEFT + i as f32 * 38.0;
            if self.config.general.default_color == name {
                fill(p, sx - 3.0, y - 3.0, 34.0, 34.0, crate::theme::accent());
            } else if self.is_hovered(sx, y, 28.0, 28.0) {
                fill(p, sx - 2.0, y - 2.0, 32.0, 32.0, crate::theme::bg_4());
            }
            fill(p, sx, y, 28.0, 28.0, (r, g, b));
            let chosen = self.config.general.default_color == name;
            self.widget(
                (sx, y, 28.0, 28.0),
                Action::SelectColor(i),
                Role::Radio,
                &format!("Default colour: {name}"),
                Some(chosen),
            );
        }
        y += 44.0;

        section(p, y, "Default thickness");
        y += 22.0;
        self.button(p, LEFT, y, 34.0, 26.0, "-", Action::ThicknessDown);
        label(
            p,
            LEFT + 46.0,
            y + 5.0,
            &format!("{:.1}", self.config.general.default_thickness),
            13.0,
            crate::theme::text_normal(),
        );
        self.button(p, LEFT + 88.0, y, 34.0, 26.0, "+", Action::ThicknessUp);
        y += 42.0;

        section(p, y, "Appearance");
        y += 24.0;
        let light = self.config.general.theme.eq_ignore_ascii_case("light");
        self.segmented(
            p,
            y,
            &[
                ("Dark", !light, Action::ToggleTheme),
                ("Light", light, Action::ToggleTheme),
            ],
        );
        y += 42.0;

        section(p, y, "System");
        y += 22.0;
        let hist = self.config.general.history_enabled;
        self.checkbox(
            p,
            y,
            "Keep the last 20 captures in History",
            hist,
            Action::ToggleHistory,
        );
        y += 28.0;
        let auto = self.autostart;
        self.checkbox(
            p,
            y,
            "Start screen-mark when I sign in",
            auto,
            Action::ToggleAutostart,
        );
        y += 28.0;
        let updates = self.config.updates.auto_check;
        self.checkbox(
            p,
            y,
            "Check for a new version once a day",
            updates,
            Action::ToggleUpdateCheck,
        );
        y += 30.0;

        // Where updates come from. A web address is typed into config.toml;
        // a folder (a share at work, say) can be picked here.
        let from = self.config.updates.url.trim().to_string();
        let shown = if from.is_empty() { "GitHub" } else { &from };
        label(p, LEFT, y + 6.0, "From", 12.0, crate::theme::text_muted());
        let btn_x = WIN_W as f32 - LEFT - 70.0 - 6.0 - 66.0;
        let path_w = btn_x - (LEFT + 52.0) - 8.0;
        let path = crate::toast::fit_middle(shown, 12.0, path_w);
        label(
            p,
            LEFT + 52.0,
            y + 6.0,
            &path,
            12.0,
            crate::theme::text_normal(),
        );
        let picking = self.browse_rx.is_some() && self.browse_for == Picker::UpdateFolder;
        let folder = if picking { "..." } else { "Folder" };
        self.button(p, btn_x, y, 70.0, 26.0, folder, Action::BrowseUpdateDir);
        if !from.is_empty() {
            self.button(
                p,
                btn_x + 76.0,
                y,
                66.0,
                26.0,
                "GitHub",
                Action::UpdatesFromGitHub,
            );
        }
        y += 40.0;

        let imgur = if self.config.general.imgur_client_id.is_empty() {
            "Imgur upload: not set up (imgur_client_id in config.toml)"
        } else {
            "Imgur upload: set up (client ID in config.toml)"
        };
        label(p, LEFT, y, imgur, 12.0, crate::theme::text_soft());
    }

    fn render_capture(&mut self, p: &mut Pixmap) {
        let mut y = CONTENT_TOP;
        let shot = display_binding(&self.config.hotkey.screenshot);
        section(p, y, &format!("Screenshot hotkey ({shot})"));
        y += 20.0;
        label(
            p,
            LEFT,
            y,
            "One press saves a PNG and copies it.",
            12.0,
            crate::theme::text_soft(),
        );
        y += 24.0;
        let all = self.config.screenshot_all_monitors();
        self.segmented(
            p,
            y,
            &[
                (
                    "Monitor under the mouse",
                    !all,
                    Action::ScreenshotAll(false),
                ),
                ("All monitors", all, Action::ScreenshotAll(true)),
            ],
        );
        y += 40.0;

        let region = display_binding(&self.config.hotkey.region);
        section(p, y, &format!("Region hotkey ({region})"));
        y += 22.0;
        let fin = self.config.general.finish_on_release;
        self.checkbox(
            p,
            y,
            "Copy as soon as a region is drawn (skip markup)",
            fin,
            Action::ToggleFinishOnRelease,
        );
        y += 26.0;
        let magnifier = self.config.general.magnifier;
        self.checkbox(
            p,
            y,
            "Show a magnifier while selecting a region",
            magnifier,
            Action::ToggleMagnifier,
        );
        y += 38.0;

        section(p, y, "Saving");
        y += 22.0;
        let also = self.config.general.copy_also_saves;
        self.checkbox(
            p,
            y,
            "Save a file every time I copy",
            also,
            Action::ToggleCopyAlsoSaves,
        );
        y += 30.0;
        let save_key = display_binding(&self.config.keys.save);
        let ask = self.config.ask_where_to_save();
        self.checkbox(
            p,
            y,
            &format!("Save ({save_key}) asks where to save"),
            ask,
            Action::ToggleAskWhere,
        );
        y += 32.0;
        let custom = !self.config.general.save_directory.trim().is_empty();
        let folder = self.config.save_folder().to_string_lossy().to_string();
        let shown = if custom {
            folder
        } else {
            format!("{folder} (default)")
        };
        label(p, LEFT, y + 6.0, "Folder", 12.0, crate::theme::text_muted());
        let btn_x = WIN_W as f32 - LEFT - 70.0 - 6.0 - 66.0;
        let path_w = btn_x - (LEFT + 52.0) - 8.0;
        let path = crate::toast::fit_middle(&shown, 12.0, path_w);
        label(
            p,
            LEFT + 52.0,
            y + 6.0,
            &path,
            12.0,
            crate::theme::text_normal(),
        );
        let browse = if self.browse_rx.is_some() && self.browse_for == Picker::SaveFolder {
            "..."
        } else {
            "Browse"
        };
        self.button(p, btn_x, y, 70.0, 26.0, browse, Action::BrowseDir);
        if custom {
            self.button(
                p,
                btn_x + 76.0,
                y,
                66.0,
                26.0,
                "Default",
                Action::DefaultDir,
            );
        }
        y += 40.0;

        section(p, y, "After a capture");
        y += 24.0;
        let mode = self.config.notification_mode();
        let system = if cfg!(target_os = "windows") {
            "Windows"
        } else {
            "Desktop"
        };
        self.segmented(
            p,
            y,
            &[
                (
                    "Popup",
                    mode == NotificationMode::Toast,
                    Action::Notifications(NotificationMode::Toast),
                ),
                (
                    system,
                    mode == NotificationMode::System,
                    Action::Notifications(NotificationMode::System),
                ),
                (
                    "Off",
                    mode == NotificationMode::Off,
                    Action::Notifications(NotificationMode::Off),
                ),
            ],
        );
        y += 32.0;
        label(
            p,
            LEFT,
            y,
            "Errors always show a popup.",
            12.0,
            crate::theme::text_muted(),
        );
    }

    fn render_keys(&mut self, p: &mut Pixmap) {
        let view_top = CONTENT_TOP - 8.0;
        let view_h = FOOTER_TOP - view_top;
        let layout = keys_layout();
        let total = keys_total_height();
        self.max_scroll = keys_max_scroll();
        self.scroll = self.scroll.clamp(0.0, self.max_scroll);

        let mut list = Pixmap::new(WIN_W, view_h as u32).expect("non-zero");
        fill(
            &mut list,
            0.0,
            0.0,
            WIN_W as f32,
            view_h,
            crate::theme::bg_1(),
        );
        let report = self.config.key_report();
        let mut hits: Vec<HitRect> = Vec::new();
        let mut widgets: Vec<Widget> = Vec::new();
        let bind_w = 156.0;
        let reset_w = 52.0;
        let bind_x = WIN_W as f32 - LEFT - reset_w - 6.0 - bind_w;
        let reset_x = WIN_W as f32 - LEFT - reset_w;
        for row in &layout {
            let ly = row.y - self.scroll;
            let screen_y = view_top + ly;
            // Fully inside the list: drawn, clickable, and has a place for
            // the focus ring. Rows outside it are still listed in `widgets`,
            // so focus can move to them and scroll them in.
            let in_view = screen_y >= view_top && screen_y + 24.0 <= FOOTER_TOP;
            if let Some(id) = row.id {
                let raw = self.config.binding(id);
                let shown = if self.editing == Some(id) {
                    "listening for keys".to_string()
                } else {
                    display_binding(&raw)
                };
                widgets.push(Widget {
                    action: Action::EditKey(id),
                    role: Role::Button,
                    label: id.label().to_string(),
                    checked: None,
                    value: Some(shown),
                    rect: in_view.then_some((bind_x, screen_y - 1.0, bind_w, 24.0)),
                });
                let is_default = Binding::parse(&raw).ok().flatten()
                    == Binding::parse(id.default_binding()).ok().flatten();
                if !is_default {
                    widgets.push(Widget {
                        action: Action::ResetKey(id),
                        role: Role::Button,
                        label: format!("Reset {} to its default", id.label()),
                        checked: None,
                        value: None,
                        rect: in_view.then_some((reset_x, screen_y - 1.0, reset_w, 24.0)),
                    });
                }
            }
            if ly + 30.0 < 0.0 || ly > view_h {
                continue;
            }
            let Some(id) = row.id else {
                section(&mut list, ly + 6.0, row.title);
                continue;
            };
            let editing = self.editing == Some(id);
            if editing {
                fill(
                    &mut list,
                    LEFT - 6.0,
                    ly - 3.0,
                    WIN_W as f32 - (LEFT - 6.0) * 2.0,
                    28.0,
                    crate::theme::bg_2(),
                );
            }
            label(
                &mut list,
                LEFT,
                ly + 3.0,
                id.label(),
                13.0,
                crate::theme::text_normal(),
            );

            let raw = self.config.binding(id);
            let problem = report.problem(id).or_else(|| {
                self.statuses
                    .get(&id)
                    .and_then(|s| s.describe(&display_binding(&raw)))
            });
            let (text, color) = if editing {
                let held = if self.held.is_empty() {
                    "Press keys...".to_string()
                } else {
                    format!("{}...", mods_prefix(self.held))
                };
                (held, crate::theme::accent())
            } else if raw.trim().is_empty() {
                ("Not set".to_string(), crate::theme::text_faint())
            } else if problem.is_some() {
                (display_binding(&raw), crate::theme::red())
            } else {
                (display_binding(&raw), crate::theme::text_normal())
            };
            let hovered = self.is_hovered(bind_x, screen_y - 1.0, bind_w, 24.0);
            let bg = if editing || hovered {
                crate::theme::bg_4()
            } else {
                crate::theme::bg_3()
            };
            fill(&mut list, bind_x, ly - 1.0, bind_w, 24.0, bg);
            if editing {
                outline(
                    &mut list,
                    bind_x,
                    ly - 1.0,
                    bind_w,
                    24.0,
                    crate::theme::accent(),
                );
            } else if problem.is_some() {
                outline(
                    &mut list,
                    bind_x,
                    ly - 1.0,
                    bind_w,
                    24.0,
                    crate::theme::red(),
                );
            }
            let tw = measure_text_width(&text, 12.0);
            label(
                &mut list,
                bind_x + (bind_w - tw) / 2.0,
                ly + 4.0,
                &text,
                12.0,
                color,
            );
            if screen_y >= view_top && screen_y + 24.0 <= FOOTER_TOP {
                hits.push(HitRect {
                    x: bind_x,
                    y: screen_y - 1.0,
                    w: bind_w,
                    h: 24.0,
                    action: Action::EditKey(id),
                });
            }

            let is_default = Binding::parse(&raw).ok().flatten()
                == Binding::parse(id.default_binding()).ok().flatten();
            if !is_default {
                let hov = self.is_hovered(reset_x, screen_y - 1.0, reset_w, 24.0);
                fill(
                    &mut list,
                    reset_x,
                    ly - 1.0,
                    reset_w,
                    24.0,
                    if hov {
                        crate::theme::bg_4()
                    } else {
                        crate::theme::bg_2()
                    },
                );
                let rw = measure_text_width("Reset", 11.0);
                label(
                    &mut list,
                    reset_x + (reset_w - rw) / 2.0,
                    ly + 5.0,
                    "Reset",
                    11.0,
                    crate::theme::text_muted(),
                );
                if screen_y >= view_top && screen_y + 24.0 <= FOOTER_TOP {
                    hits.push(HitRect {
                        x: reset_x,
                        y: screen_y - 1.0,
                        w: reset_w,
                        h: 24.0,
                        action: Action::ResetKey(id),
                    });
                }
            }
        }
        // Scrollbar
        if self.max_scroll > 0.0 {
            let track_x = WIN_W as f32 - 10.0;
            let thumb_h = (view_h * view_h / total).max(24.0);
            let thumb_y = (view_h - thumb_h) * (self.scroll / self.max_scroll);
            fill(
                &mut list,
                track_x,
                thumb_y,
                4.0,
                thumb_h,
                crate::theme::bg_4(),
            );
        }
        p.draw_pixmap(
            0,
            view_top as i32,
            list.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
        self.hits.extend(hits);
        self.widgets.extend(widgets);

        // While listening, a problem with the currently typed attempt is in
        // the footer; otherwise show the hovered row's problem, if any.
        if self.editing.is_none() && self.message.is_none() {
            let status_problem = KeyId::GLOBALS.iter().find_map(|id| {
                self.statuses
                    .get(id)
                    .and_then(|st| st.describe(&display_binding(&self.config.binding(*id))))
                    .map(|why| format!("{}: {why}. Pick another combination.", id.label()))
            });
            if let Some((_, problem)) = report.first_problem() {
                self.message = Some((problem, MsgKind::Error));
            } else if let Some(problem) = status_problem {
                self.message = Some((problem, MsgKind::Error));
            }
        }
    }

    fn render_toolbar(&mut self, p: &mut Pixmap) {
        let mut y = CONTENT_TOP;
        label(
            p,
            LEFT,
            y,
            "Tools shown on the markup toolbar",
            12.0,
            crate::theme::text_soft(),
        );
        y += 24.0;
        // 22 apart: fifteen tools have to fit above the footer.
        let entries = self.config.toolbar.entries();
        for (i, (_, name, on)) in entries.iter().enumerate() {
            self.checkbox(p, y, name, *on, Action::ToggleToolbar(i));
            y += 22.0;
        }
    }

    // ───────────── widgets ─────────────

    /// A control that is on screen: where a click lands, and its entry in
    /// `widgets`.
    fn widget(
        &mut self,
        (x, y, w, h): (f32, f32, f32, f32),
        action: Action,
        role: Role,
        label: &str,
        checked: Option<bool>,
    ) {
        self.hits.push(HitRect { x, y, w, h, action });
        self.widgets.push(Widget {
            action,
            role,
            label: spoken_label(action, label),
            checked,
            value: None,
            rect: Some((x, y, w, h)),
        });
    }

    fn is_hovered(&self, x: f32, y: f32, w: f32, h: f32) -> bool {
        let (cx, cy) = self.cursor;
        cx >= x && cx <= x + w && cy >= y && cy <= y + h
    }

    #[allow(clippy::too_many_arguments)]
    fn button(
        &mut self,
        p: &mut Pixmap,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        text: &str,
        action: Action,
    ) {
        let bg = if self.is_hovered(x, y, w, h) {
            crate::theme::bg_4()
        } else {
            crate::theme::bg_3()
        };
        fill(p, x, y, w, h, bg);
        let tw = measure_text_width(text, 12.0);
        label(
            p,
            x + (w - tw) / 2.0,
            y + (h - 12.0) / 2.0 - 1.0,
            text,
            12.0,
            crate::theme::text_normal(),
        );
        self.widget((x, y, w, h), action, Role::Button, text, None);
    }

    #[allow(clippy::too_many_arguments)]
    fn primary_button(
        &mut self,
        p: &mut Pixmap,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        text: &str,
        action: Action,
    ) {
        fill(p, x, y, w, h, crate::theme::accent());
        if self.is_hovered(x, y, w, h) {
            outline(p, x, y, w, h, crate::theme::text_normal());
        }
        let tw = measure_text_width(text, 12.0);
        label(
            p,
            x + (w - tw) / 2.0,
            y + (h - 12.0) / 2.0 - 1.0,
            text,
            12.0,
            crate::theme::text_on_accent(),
        );
        self.widget((x, y, w, h), action, Role::Button, text, None);
    }

    fn checkbox(&mut self, p: &mut Pixmap, y: f32, text: &str, on: bool, action: Action) {
        let size = 16.0;
        let hovered = self.is_hovered(LEFT, y - 2.0, WIN_W as f32 - LEFT * 2.0, 22.0);
        fill(
            p,
            LEFT,
            y + 1.0,
            size,
            size,
            if on {
                crate::theme::accent()
            } else if hovered {
                crate::theme::bg_4()
            } else {
                crate::theme::bg_3()
            },
        );
        if on {
            // A drawn tick (the UI font has no check mark).
            let mut pb = PathBuilder::new();
            pb.move_to(LEFT + 3.5, y + 9.0);
            pb.line_to(LEFT + 7.0, y + 12.5);
            pb.line_to(LEFT + 12.5, y + 5.0);
            if let Some(path) = pb.finish() {
                let mut paint = Paint::default();
                paint.set_color(sk(crate::theme::text_on_accent()));
                paint.anti_alias = true;
                let stroke = Stroke {
                    width: 2.0,
                    ..Stroke::default()
                };
                p.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
            }
        }
        label(
            p,
            LEFT + size + 10.0,
            y + 1.0,
            text,
            13.0,
            crate::theme::text_normal(),
        );
        self.widget(
            (LEFT, y - 2.0, WIN_W as f32 - LEFT * 2.0, 22.0),
            action,
            Role::CheckBox,
            text,
            Some(on),
        );
    }

    fn segmented(&mut self, p: &mut Pixmap, y: f32, items: &[(&str, bool, Action)]) {
        let mut x = LEFT;
        for (text, on, action) in items {
            let w = measure_text_width(text, 12.0) + 24.0;
            let hovered = self.is_hovered(x, y, w, 26.0);
            let bg = if *on {
                crate::theme::bg_4()
            } else if hovered {
                crate::theme::bg_3()
            } else {
                crate::theme::bg_2()
            };
            fill(p, x, y, w, 26.0, bg);
            if *on {
                fill(p, x, y + 24.0, w, 2.0, crate::theme::accent());
            }
            let c = if *on {
                crate::theme::text_normal()
            } else {
                crate::theme::text_muted()
            };
            label(p, x + 12.0, y + 6.0, text, 12.0, c);
            self.widget((x, y, w, 26.0), *action, Role::Radio, text, Some(*on));
            x += w + 4.0;
        }
    }
}

fn within((x, y, w, h): (f32, f32, f32, f32), px: f32, py: f32) -> bool {
    px >= x && px <= x + w && py >= y && py <= y + h
}

/// What a control is called when its drawn text doesn't say (a bare "+").
fn spoken_label(action: Action, drawn: &str) -> String {
    match action {
        Action::ThicknessDown => "Thinner default line".to_string(),
        Action::ThicknessUp => "Thicker default line".to_string(),
        Action::BrowseDir => "Browse for the save folder".to_string(),
        Action::DefaultDir => "Use the default save folder".to_string(),
        Action::BrowseUpdateDir => "Update from a folder".to_string(),
        Action::UpdatesFromGitHub => "Update from GitHub".to_string(),
        Action::ToggleToolbar(_) => format!("Show {drawn} on the toolbar"),
        _ => drawn.to_string(),
    }
}

/// One line of the Keys tab: a section title or a binding row.
struct KeyRow {
    y: f32,
    id: Option<KeyId>,
    title: &'static str,
}

fn keys_layout() -> Vec<KeyRow> {
    let mut rows = Vec::new();
    let mut y = 8.0;
    let mut last_scope: Option<Scope> = None;
    for id in KeyId::all() {
        let scope = id.scope();
        if last_scope != Some(scope) {
            if last_scope.is_some() {
                y += 8.0;
            }
            let title = match scope {
                Scope::Global => "Global hotkeys (work in any app)",
                Scope::Editor => "While marking up",
                Scope::Tool => "Tools (one key each)",
            };
            rows.push(KeyRow { y, id: None, title });
            y += 26.0;
            last_scope = Some(scope);
        }
        rows.push(KeyRow {
            y,
            id: Some(id),
            title: "",
        });
        y += 28.0;
    }
    rows
}

fn keys_total_height() -> f32 {
    keys_layout().last().map(|r| r.y + 30.0).unwrap_or(0.0)
}

fn keys_max_scroll() -> f32 {
    let view_h = FOOTER_TOP - (CONTENT_TOP - 8.0);
    (keys_total_height() - view_h).max(0.0)
}

/// "ctrl+shift+s" → "Ctrl+Shift+S"; unparseable text is shown as typed.
pub fn display_binding(raw: &str) -> String {
    match Binding::parse(raw) {
        Ok(Some(b)) => b.to_string(),
        Ok(None) => "not set".into(),
        Err(_) => raw.to_string(),
    }
}

fn mods_prefix(m: Mods) -> String {
    let mut s = String::new();
    if m.ctrl() {
        s.push_str("Ctrl+");
    }
    if m.shift() {
        s.push_str("Shift+");
    }
    if m.alt() {
        s.push_str("Alt+");
    }
    if m.super_key() {
        s.push_str(if cfg!(target_os = "windows") {
            "Win+"
        } else {
            "Super+"
        });
    }
    s
}

/// Greedy word wrap by measured width.
fn wrap(text: &str, size: f32, max_w: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in text.split(' ') {
        let candidate = if cur.is_empty() {
            word.to_string()
        } else {
            format!("{cur} {word}")
        };
        if measure_text_width(&candidate, size) > max_w && !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
            cur = word.to_string();
        } else {
            cur = candidate;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

fn section(p: &mut Pixmap, y: f32, title: &str) {
    label(p, LEFT, y, title, 12.0, crate::theme::text_muted());
}

// ───────────── the window ─────────────

/// `SettingsView` in a real window.
pub struct SettingsWindow {
    pub window: Arc<Window>,
    pub surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
    pub view: SettingsView,
    pub needs_redraw: bool,
}

impl SettingsWindow {
    pub fn new(
        window: Arc<Window>,
        surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
        view: SettingsView,
    ) -> Self {
        Self {
            window,
            surface,
            view,
            needs_redraw: true,
        }
    }

    fn scale(&self) -> f32 {
        self.window.scale_factor() as f32
    }

    /// Cursor moved (physical pixels). Returns true to redraw.
    pub fn on_cursor_moved(&mut self, x: f32, y: f32) -> bool {
        let s = self.scale();
        self.view.hover(x / s, y / s)
    }

    /// Left click at the last cursor position.
    pub fn on_click(&mut self) -> Outcome {
        let (x, y) = self.view.cursor;
        self.view.click(x, y)
    }

    pub fn render(&mut self) {
        let pixmap = self.view.render();
        let phys = self.window.inner_size();
        let pw = phys.width.max(1);
        let ph = phys.height.max(1);
        if let (Some(nz_w), Some(nz_h)) = (NonZeroU32::new(pw), NonZeroU32::new(ph)) {
            if let Err(e) = self.surface.resize(nz_w, nz_h) {
                tracing::error!("Settings surface resize failed: {e}");
                return;
            }
        }
        if let Ok(mut buffer) = self.surface.buffer_mut() {
            let src = pixmap.data();
            let src_w = WIN_W as usize;
            let src_h = WIN_H as usize;
            for y in 0..ph as usize {
                let sy = (y * src_h / ph as usize).min(src_h - 1);
                for x in 0..pw as usize {
                    let sx = (x * src_w / pw as usize).min(src_w - 1);
                    let si = (sy * src_w + sx) * 4;
                    buffer[y * pw as usize + x] = ((src[si] as u32) << 16)
                        | ((src[si + 1] as u32) << 8)
                        | (src[si + 2] as u32);
                }
            }
            let _ = buffer.present();
        }
        self.needs_redraw = false;
    }
}

// ───────────── drawing helpers ─────────────

fn sk((r, g, b): (u8, u8, u8)) -> SkiaColor {
    SkiaColor::from_rgba8(r, g, b, 255)
}

fn fill(pixmap: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, rgb: (u8, u8, u8)) {
    let Some(rect) = Rect::from_xywh(x, y, w, h) else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color(sk(rgb));
    pixmap.fill_rect(rect, &paint, Transform::identity(), None);
}

fn outline(pixmap: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, rgb: (u8, u8, u8)) {
    fill(pixmap, x, y, w, 1.0, rgb);
    fill(pixmap, x, y + h - 1.0, w, 1.0, rgb);
    fill(pixmap, x, y, 1.0, h, rgb);
    fill(pixmap, x + w - 1.0, y, 1.0, h, rgb);
}

fn label(pixmap: &mut Pixmap, x: f32, y: f32, text: &str, size: f32, (r, g, b): (u8, u8, u8)) {
    let color = Color::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0);
    render_text_annotation(pixmap, &Point::new(x, y), text, &color, size);
}

#[allow(dead_code)]
fn default_folder_text() -> String {
    default_save_folder().to_string_lossy().to_string()
}
