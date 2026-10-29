use crate::geometry::Color;
use crate::keys::KeyId;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing;

// Per-field serde defaults so a config missing any single key (e.g. hand-edited
// or written by an older version) still parses instead of resetting everything.

fn default_true() -> bool {
    true
}
fn d_color() -> String {
    "red".into()
}
fn d_thickness() -> f32 {
    3.0
}
fn d_theme() -> String {
    "dark".into()
}
fn d_region() -> String {
    "Ctrl+Shift+S".into()
}
fn d_screenshot() -> String {
    "Ctrl+PrintScreen".into()
}
fn d_screenshot_area() -> String {
    "monitor".into()
}
fn d_notifications() -> String {
    "toast".into()
}

macro_rules! action_default {
    ($fn_name:ident, $key:expr) => {
        fn $fn_name() -> String {
            $key.into()
        }
    };
}
action_default!(a_copy, "Ctrl+C");
action_default!(a_save, "Ctrl+S");
action_default!(a_undo, "Ctrl+Z");
action_default!(a_redo, "Ctrl+Shift+Z");
action_default!(a_select_all, "Ctrl+A");

macro_rules! key_default {
    ($fn_name:ident, $key:expr) => {
        fn $fn_name() -> String {
            $key.into()
        }
    };
}
key_default!(k_select, "v");
key_default!(k_arrow, "a");
key_default!(k_rectangle, "r");
key_default!(k_circle, "c");
key_default!(k_rounded_rect, "o");
key_default!(k_line, "l");
key_default!(k_pencil, "p");
key_default!(k_highlight, "h");
key_default!(k_spotlight, "f");
key_default!(k_text, "t");
key_default!(k_pixelate, "b");
key_default!(k_step_marker, "n");
key_default!(k_eyedropper, "i");
key_default!(k_measurement, "m");

#[derive(Debug, Serialize, Deserialize)]
pub struct GeneralConfig {
    #[serde(default = "d_color")]
    pub default_color: String,
    #[serde(default = "d_thickness")]
    pub default_thickness: f32,
    #[serde(default)]
    pub save_directory: String,
    #[serde(default)]
    pub imgur_client_id: String,
    /// When false, captures are not saved to the recent-captures history.
    #[serde(default = "default_true")]
    pub history_enabled: bool,
    /// UI theme: "dark" or "light".
    #[serde(default = "d_theme")]
    pub theme: String,
    /// Copying from the editor also writes a PNG to the save folder.
    #[serde(default)]
    pub copy_also_saves: bool,
    /// Copy (and save, with `copy_also_saves`) as soon as a region is drawn
    /// or a window is picked, skipping the markup toolbar.
    #[serde(default)]
    pub finish_on_release: bool,
    /// What the one-press screenshot hotkey grabs: "monitor" (the one under
    /// the mouse) or "all" (every monitor as one image).
    #[serde(default = "d_screenshot_area")]
    pub screenshot_area: String,
    /// Feedback after a capture: "toast" (screen-mark's own popup),
    /// "system" (the OS notification centre) or "off". Errors always show.
    #[serde(default = "d_notifications")]
    pub notifications: String,
    /// Whether Save (Ctrl+S) opens a file dialog. Unset in configs written
    /// before 2.1: then it asks only when no save folder was chosen, which
    /// is what those versions did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask_where_to_save: Option<bool>,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            default_color: d_color(),
            default_thickness: d_thickness(),
            save_directory: String::new(),
            imgur_client_id: String::new(),
            history_enabled: true,
            theme: d_theme(),
            copy_also_saves: false,
            finish_on_release: false,
            screenshot_area: d_screenshot_area(),
            notifications: d_notifications(),
            ask_where_to_save: None,
        }
    }
}

/// Global hotkeys: registered with the OS, so they work in any app.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HotkeyConfig {
    /// Opens the capture overlay to drag a region. `capture` is the 2.0 name.
    #[serde(default = "d_region", alias = "capture")]
    pub region: String,
    /// One press: capture, save a PNG and copy it.
    #[serde(default = "d_screenshot")]
    pub screenshot: String,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            region: d_region(),
            screenshot: d_screenshot(),
        }
    }
}

/// Editor actions while the capture overlay is open. "" = unbound.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct KeysConfig {
    #[serde(default = "a_copy")]
    pub copy: String,
    #[serde(default = "a_save")]
    pub save: String,
    #[serde(default = "a_undo")]
    pub undo: String,
    #[serde(default = "a_redo")]
    pub redo: String,
    #[serde(default = "a_select_all")]
    pub select_all: String,
    #[serde(default)]
    pub pin: String,
    #[serde(default)]
    pub ocr: String,
    #[serde(default)]
    pub upload: String,
}

impl Default for KeysConfig {
    fn default() -> Self {
        Self {
            copy: a_copy(),
            save: a_save(),
            undo: a_undo(),
            redo: a_redo(),
            select_all: a_select_all(),
            pin: String::new(),
            ocr: String::new(),
            upload: String::new(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ShortcutsConfig {
    #[serde(default = "k_select")]
    pub select: String,
    #[serde(default = "k_arrow")]
    pub arrow: String,
    #[serde(default = "k_rectangle")]
    pub rectangle: String,
    #[serde(default = "k_circle")]
    pub circle: String,
    #[serde(default = "k_rounded_rect")]
    pub rounded_rect: String,
    #[serde(default = "k_line")]
    pub line: String,
    #[serde(default = "k_pencil")]
    pub pencil: String,
    #[serde(default = "k_highlight")]
    pub highlight: String,
    #[serde(default = "k_spotlight")]
    pub spotlight: String,
    #[serde(default = "k_text")]
    pub text: String,
    #[serde(default = "k_pixelate")]
    pub pixelate: String,
    #[serde(default = "k_step_marker")]
    pub step_marker: String,
    #[serde(default = "k_eyedropper")]
    pub eyedropper: String,
    #[serde(default = "k_measurement")]
    pub measurement: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolbarConfig {
    #[serde(default = "default_true")]
    pub select: bool,
    #[serde(default = "default_true")]
    pub arrow: bool,
    #[serde(default = "default_true")]
    pub rectangle: bool,
    #[serde(default = "default_true")]
    pub circle: bool,
    #[serde(default = "default_true")]
    pub rounded_rect: bool,
    #[serde(default = "default_true")]
    pub line: bool,
    #[serde(default = "default_true")]
    pub pencil: bool,
    #[serde(default = "default_true")]
    pub highlight: bool,
    #[serde(default = "default_true")]
    pub spotlight: bool,
    #[serde(default = "default_true")]
    pub text: bool,
    #[serde(default = "default_true")]
    pub pixelate: bool,
    #[serde(default = "default_true")]
    pub step_marker: bool,
    #[serde(default = "default_true")]
    pub eyedropper: bool,
    #[serde(default = "default_true")]
    pub measurement: bool,
}

impl Default for ToolbarConfig {
    fn default() -> Self {
        Self {
            select: true,
            arrow: true,
            rectangle: true,
            circle: true,
            rounded_rect: true,
            line: true,
            pencil: true,
            highlight: true,
            spotlight: true,
            text: true,
            pixelate: true,
            step_marker: true,
            eyedropper: true,
            measurement: true,
        }
    }
}

impl ToolbarConfig {
    /// Returns the list of visible button indices (0-23) based on which tools are enabled.
    /// Tool buttons 0-13 are conditionally shown; colors 14-18 and actions 19-23 are always shown.
    pub fn visible_button_indices(&self) -> Vec<usize> {
        let mut indices = Vec::new();

        let tool_flags = [
            self.select,
            self.arrow,
            self.rectangle,
            self.circle,
            self.rounded_rect,
            self.line,
            self.pencil,
            self.highlight,
            self.spotlight,
            self.text,
            self.pixelate,
            self.step_marker,
            self.eyedropper,
            self.measurement,
        ];

        for (i, &enabled) in tool_flags.iter().enumerate() {
            if enabled {
                indices.push(i);
            }
        }

        // Colors: always visible (indices 14-18)
        for i in 14..=18 {
            indices.push(i);
        }

        // Actions: always visible (indices 19-23)
        for i in 19..=23 {
            indices.push(i);
        }

        indices
    }

    /// Ordered list of (symbol, label, enabled) for the Settings UI.
    pub fn entries(&self) -> Vec<(&'static str, &'static str, bool)> {
        vec![
            ("->", "Select / Move", self.select),
            (">>", "Arrow", self.arrow),
            ("[]", "Rectangle", self.rectangle),
            ("()", "Circle", self.circle),
            ("[.]", "Rounded Rect", self.rounded_rect),
            ("--", "Line", self.line),
            ("~", "Pencil", self.pencil),
            ("##", "Highlight", self.highlight),
            ("**", "Spotlight", self.spotlight),
            ("Aa", "Text", self.text),
            ("::", "Pixelate", self.pixelate),
            ("1.", "Step Marker", self.step_marker),
            ("/|", "Eyedropper", self.eyedropper),
            ("|<>", "Measurement", self.measurement),
        ]
    }

    /// Toggle a tool by index (0-13).
    pub fn toggle_by_index(&mut self, index: usize) {
        match index {
            0 => self.select = !self.select,
            1 => self.arrow = !self.arrow,
            2 => self.rectangle = !self.rectangle,
            3 => self.circle = !self.circle,
            4 => self.rounded_rect = !self.rounded_rect,
            5 => self.line = !self.line,
            6 => self.pencil = !self.pencil,
            7 => self.highlight = !self.highlight,
            8 => self.spotlight = !self.spotlight,
            9 => self.text = !self.text,
            10 => self.pixelate = !self.pixelate,
            11 => self.step_marker = !self.step_marker,
            12 => self.eyedropper = !self.eyedropper,
            13 => self.measurement = !self.measurement,
            _ => {}
        }
    }
}

impl Default for ShortcutsConfig {
    fn default() -> Self {
        Self {
            select: "v".into(),
            arrow: "a".into(),
            rectangle: "r".into(),
            circle: "c".into(),
            rounded_rect: "o".into(),
            line: "l".into(),
            pencil: "p".into(),
            highlight: "h".into(),
            spotlight: "f".into(),
            text: "t".into(),
            pixelate: "b".into(),
            step_marker: "n".into(),
            eyedropper: "i".into(),
            measurement: "m".into(),
        }
    }
}

impl ShortcutsConfig {
    /// Ordered list of (symbol, label, current key) for UI display.
    pub fn entries(&self) -> Vec<(&'static str, &'static str, &str)> {
        vec![
            ("->", "Select / Move", &self.select),
            (">>", "Arrow", &self.arrow),
            ("[]", "Rectangle", &self.rectangle),
            ("()", "Circle", &self.circle),
            ("[.]", "Rounded Rect", &self.rounded_rect),
            ("--", "Line", &self.line),
            ("~", "Pencil", &self.pencil),
            ("##", "Highlight", &self.highlight),
            ("**", "Spotlight", &self.spotlight),
            ("Aa", "Text", &self.text),
            ("::", "Pixelate", &self.pixelate),
            ("1.", "Step Marker", &self.step_marker),
            ("/|", "Eyedropper", &self.eyedropper),
            ("|<>", "Measurement", &self.measurement),
        ]
    }

    /// Set shortcut by index (0-13).
    pub fn set_by_index(&mut self, index: usize, key: String) {
        match index {
            0 => self.select = key,
            1 => self.arrow = key,
            2 => self.rectangle = key,
            3 => self.circle = key,
            4 => self.rounded_rect = key,
            5 => self.line = key,
            6 => self.pencil = key,
            7 => self.highlight = key,
            8 => self.spotlight = key,
            9 => self.text = key,
            10 => self.pixelate = key,
            11 => self.step_marker = key,
            12 => self.eyedropper = key,
            13 => self.measurement = key,
            _ => {}
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub hotkey: HotkeyConfig,
    #[serde(default)]
    pub keys: KeysConfig,
    #[serde(default)]
    pub shortcuts: ShortcutsConfig,
    #[serde(default)]
    pub toolbar: ToolbarConfig,
}

impl Config {
    /// Returns the config file path: `<config_dir>/screen-mark/config.toml`,
    /// or `$SCREEN_MARK_HOME/config.toml` when that is set.
    pub fn config_path() -> Option<PathBuf> {
        if let Some(home) = home_override() {
            return Some(home.join("config.toml"));
        }
        dirs::config_dir().map(|p| p.join("screen-mark").join("config.toml"))
    }

    /// Where 1.x (`hydroshot`) kept its config.
    fn legacy_config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|p| p.join("hydroshot").join("config.toml"))
    }

    /// If there is no config yet but a 1.x one exists, copy it across (once).
    /// The old file is left in place so a 1.x install keeps working.
    fn migrate_legacy(path: &std::path::Path) {
        if path.exists() || home_override().is_some() {
            return;
        }
        let Some(old) = Self::legacy_config_path() else {
            return;
        };
        if !old.is_file() {
            return;
        }
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::copy(&old, path) {
            Ok(_) => tracing::info!("Migrated config from {}", old.display()),
            Err(e) => tracing::warn!("Could not migrate 1.x config: {e}"),
        }
    }

    /// Load config from disk; creates default if missing; falls back to defaults on parse error.
    pub fn load() -> Self {
        let path = match Self::config_path() {
            Some(p) => p,
            None => {
                tracing::warn!("Could not determine config directory, using defaults");
                return Self::default();
            }
        };
        Self::migrate_legacy(&path);

        match std::fs::read_to_string(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::info!("Config file not found, creating with defaults");
                let cfg = Self::default();
                if let Err(e) = cfg.save() {
                    tracing::warn!("Failed to write default config: {e}");
                }
                cfg
            }
            Ok(contents) => match toml::from_str::<Config>(&contents) {
                Ok(cfg) => cfg,
                Err(e) => {
                    // The file is left untouched so the user can fix it.
                    // Surface the error visibly — tracing output is invisible
                    // in a windows_subsystem = "windows" build.
                    tracing::warn!("Failed to parse config, using defaults: {e}");
                    let _ = notify_rust::Notification::new()
                        .summary("screen-mark")
                        .body(&format!("config.toml has an error and was ignored:\n{e}"))
                        .timeout(5000)
                        .show();
                    Self::default()
                }
            },
            Err(e) => {
                tracing::warn!("Failed to read config file, using defaults: {e}");
                Self::default()
            }
        }
    }

    /// Write current config to disk as TOML.
    pub fn save(&self) -> Result<(), String> {
        let path = Self::config_path().ok_or("Could not determine config directory")?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create config directory: {e}"))?;
        }

        let contents =
            toml::to_string(self).map_err(|e| format!("Failed to serialize config: {e}"))?;

        // Atomic write: write to a temp file then rename, so a crash mid-write
        // won't corrupt the config.
        let tmp_path = path.with_extension("toml.tmp");
        std::fs::write(&tmp_path, &contents)
            .map_err(|e| format!("Failed to write temp config file: {e}"))?;
        std::fs::rename(&tmp_path, &path)
            .map_err(|e| format!("Failed to rename config file: {e}"))?;

        Ok(())
    }

    /// Parse the `default_color` string into a `Color`. Accepts the named
    /// Preset names (red, blue, green, yellow, mauve, ...) or a hex value like `#ff6a3d`. Falls back to red.
    pub fn default_color(&self) -> Color {
        match self.general.default_color.as_str() {
            "red" => Color::red(),
            "blue" => Color::blue(),
            "green" => Color::green(),
            "yellow" => Color::yellow(),
            "white" => Color::white(),
            "mauve" => Color::mauve(),
            "peach" => Color::peach(),
            "teal" => Color::teal(),
            "sky" => Color::sky(),
            "lavender" => Color::lavender(),
            other => {
                if let Some(c) = parse_hex_color(other) {
                    return c;
                }
                tracing::warn!("Unknown color '{other}', falling back to red");
                Color::red()
            }
        }
    }

    /// Resolve the configured theme into a `ThemeMode` (unknown → dark).
    pub fn theme_mode(&self) -> crate::theme::ThemeMode {
        if self.general.theme.eq_ignore_ascii_case("light") {
            crate::theme::ThemeMode::Light
        } else {
            crate::theme::ThemeMode::Dark
        }
    }

    /// Clamp thickness to the range 1.0..=20.0.
    pub fn clamped_thickness(&self) -> f32 {
        self.general.default_thickness.clamp(1.0, 20.0)
    }

    /// The folder automatic saves go to: the configured one, or
    /// `<Pictures>/screen-mark`. It is created on first save.
    pub fn save_folder(&self) -> PathBuf {
        let configured = self.general.save_directory.trim();
        if !configured.is_empty() {
            return PathBuf::from(configured);
        }
        default_save_folder()
    }

    /// Whether Save (Ctrl+S) shows a file dialog. See `ask_where_to_save`.
    pub fn ask_where_to_save(&self) -> bool {
        self.general.ask_where_to_save.unwrap_or_else(|| {
            // What 2.0 did: ask unless a folder is set and is there (a
            // folder on an unplugged drive gets the dialog, not an error).
            let dir = self.general.save_directory.trim();
            dir.is_empty() || !std::path::Path::new(dir).is_dir()
        })
    }

    /// The one-press screenshot grabs every monitor (true) or the one under
    /// the mouse (false).
    pub fn screenshot_all_monitors(&self) -> bool {
        self.general.screenshot_area.eq_ignore_ascii_case("all")
    }

    pub fn notification_mode(&self) -> NotificationMode {
        match self.general.notifications.to_ascii_lowercase().as_str() {
            "system" => NotificationMode::System,
            "off" | "none" | "false" => NotificationMode::Off,
            _ => NotificationMode::Toast,
        }
    }

    /// The configured string for a bindable key.
    pub fn binding(&self, id: KeyId) -> String {
        match id {
            KeyId::Region => self.hotkey.region.clone(),
            KeyId::Screenshot => self.hotkey.screenshot.clone(),
            KeyId::Copy => self.keys.copy.clone(),
            KeyId::Save => self.keys.save.clone(),
            KeyId::Undo => self.keys.undo.clone(),
            KeyId::Redo => self.keys.redo.clone(),
            KeyId::SelectAll => self.keys.select_all.clone(),
            KeyId::Pin => self.keys.pin.clone(),
            KeyId::Ocr => self.keys.ocr.clone(),
            KeyId::Upload => self.keys.upload.clone(),
            KeyId::Tool(i) => self
                .shortcuts
                .entries()
                .get(i)
                .map(|(_, _, k)| k.to_string())
                .unwrap_or_default(),
        }
    }

    /// Set a bindable key ("" = unbound).
    pub fn set_binding(&mut self, id: KeyId, value: String) {
        match id {
            KeyId::Region => self.hotkey.region = value,
            KeyId::Screenshot => self.hotkey.screenshot = value,
            KeyId::Copy => self.keys.copy = value,
            KeyId::Save => self.keys.save = value,
            KeyId::Undo => self.keys.undo = value,
            KeyId::Redo => self.keys.redo = value,
            KeyId::SelectAll => self.keys.select_all = value,
            KeyId::Pin => self.keys.pin = value,
            KeyId::Ocr => self.keys.ocr = value,
            KeyId::Upload => self.keys.upload = value,
            KeyId::Tool(i) => self.shortcuts.set_by_index(i, value),
        }
    }

    /// Check every binding (parse, scope rules, conflicts).
    pub fn key_report(&self) -> crate::keys::Report {
        crate::keys::check(|id| self.binding(id))
    }

    /// Put every binding back to its default.
    pub fn reset_bindings(&mut self) {
        for id in KeyId::all() {
            self.set_binding(id, id.default_binding().to_string());
        }
    }
}

/// `SCREEN_MARK_HOME`: keep config.toml and history in this one folder
/// instead of the per-user locations (portable use, and the test suite, so
/// `cargo test` never touches a real config or history).
pub fn home_override() -> Option<PathBuf> {
    std::env::var_os("SCREEN_MARK_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// How screen-mark reports a finished capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationMode {
    Toast,
    System,
    Off,
}

/// `<Pictures>/screen-mark`, falling back to `<home>/Pictures/screen-mark`.
pub fn default_save_folder() -> PathBuf {
    dirs::picture_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Pictures")))
        .unwrap_or_else(std::env::temp_dir)
        .join("screen-mark")
}

/// Parse a `#rrggbb` hex string into a Color.
pub fn parse_hex_color(s: &str) -> Option<Color> {
    let hex = s.strip_prefix('#')?;
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color::new(
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        1.0,
    ))
}
