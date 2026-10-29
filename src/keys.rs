//! One key-binding model for everything you can rebind: the two global
//! hotkeys (work in any app), the editor actions (Copy, Save, Undo, ...) and
//! the tool keys.
//!
//! A binding is written in config.toml as `Ctrl+Shift+S`: modifiers in any
//! order, then one key. An empty string means "not bound".
//!
//! Letters are matched by the character the keyboard layout produces (so an
//! AZERTY user who presses the key labelled A gets A, on both Windows, where
//! RegisterHotKey works on layout-mapped virtual keys, and X11, where the
//! keysym is looked up in the current layout). Digits, punctuation and named
//! keys are matched by position, because their characters move with Shift
//! and with the layout. Capture (Settings) and matching (overlay) go through
//! the same `key_from_winit`, so a captured binding always matches the key
//! that was pressed.

use std::fmt;

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use winit::keyboard::{Key as WKey, KeyCode, ModifiersState, NamedKey as WNamed, PhysicalKey};

// ───────────────────────── modifiers ─────────────────────────

/// Modifier set. Bit layout is private; use the constants.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Mods(u8);

impl Mods {
    pub const NONE: Mods = Mods(0);
    pub const CTRL: Mods = Mods(1);
    pub const SHIFT: Mods = Mods(2);
    pub const ALT: Mods = Mods(4);
    pub const SUPER: Mods = Mods(8);

    pub fn ctrl(self) -> bool {
        self.0 & 1 != 0
    }
    pub fn shift(self) -> bool {
        self.0 & 2 != 0
    }
    pub fn alt(self) -> bool {
        self.0 & 4 != 0
    }
    pub fn super_key(self) -> bool {
        self.0 & 8 != 0
    }
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
    /// Ctrl, Alt or Super: modifiers that turn a key into a command.
    pub fn has_command(self) -> bool {
        self.0 & (1 | 4 | 8) != 0
    }
    pub fn count(self) -> u32 {
        self.0.count_ones()
    }
    pub fn without_shift(self) -> Mods {
        Mods(self.0 & !2)
    }
    pub fn with(self, other: Mods) -> Mods {
        Mods(self.0 | other.0)
    }
}

impl std::ops::BitOr for Mods {
    type Output = Mods;
    fn bitor(self, rhs: Mods) -> Mods {
        self.with(rhs)
    }
}

pub fn mods_from_winit(m: ModifiersState) -> Mods {
    let mut out = Mods::NONE;
    if m.control_key() {
        out = out | Mods::CTRL;
    }
    if m.shift_key() {
        out = out | Mods::SHIFT;
    }
    if m.alt_key() {
        out = out | Mods::ALT;
    }
    if m.super_key() {
        out = out | Mods::SUPER;
    }
    out
}

// ───────────────────────── keys ─────────────────────────

/// A non-modifier key.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Key {
    /// `a`..=`z` (always lowercase) or `0`..=`9`.
    Char(char),
    Named(Named),
    /// Whatever key types this character (lowercase). Only from configs
    /// written by 2.0, whose tool keys were any typed character (`#`, `ö`,
    /// `<`); Settings never records one. Tool keys only.
    Typed(char),
}

macro_rules! named_keys {
    ($( $v:ident => $name:literal, [$($alias:literal),*], $kc:ident, $keysym:literal; )*) => {
        /// Keys that aren't letters or digits.
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
        pub enum Named { $($v,)* }

        impl Named {
            pub const ALL: &'static [Named] = &[$(Named::$v,)*];

            /// The name written in config.toml and shown in Settings.
            pub fn name(self) -> &'static str {
                match self { $(Named::$v => $name,)* }
            }
            fn aliases(self) -> &'static [&'static str] {
                match self { $(Named::$v => &[$($alias),*],)* }
            }
            fn from_key_code(kc: KeyCode) -> Option<Named> {
                match kc { $(KeyCode::$kc => Some(Named::$v),)* _ => None }
            }
            fn hotkey_code(self) -> Code {
                match self { $(Named::$v => Code::$kc,)* }
            }
            /// X11 keysym, for checking whether another program holds the key.
            pub fn keysym(self) -> u32 {
                match self { $(Named::$v => $keysym,)* }
            }
        }
    };
}

named_keys! {
    F1 => "F1", [], F1, 0xffbe;
    F2 => "F2", [], F2, 0xffbf;
    F3 => "F3", [], F3, 0xffc0;
    F4 => "F4", [], F4, 0xffc1;
    F5 => "F5", [], F5, 0xffc2;
    F6 => "F6", [], F6, 0xffc3;
    F7 => "F7", [], F7, 0xffc4;
    F8 => "F8", [], F8, 0xffc5;
    F9 => "F9", [], F9, 0xffc6;
    F10 => "F10", [], F10, 0xffc7;
    F11 => "F11", [], F11, 0xffc8;
    F12 => "F12", [], F12, 0xffc9;
    F13 => "F13", [], F13, 0xffca;
    F14 => "F14", [], F14, 0xffcb;
    F15 => "F15", [], F15, 0xffcc;
    F16 => "F16", [], F16, 0xffcd;
    F17 => "F17", [], F17, 0xffce;
    F18 => "F18", [], F18, 0xffcf;
    F19 => "F19", [], F19, 0xffd0;
    F20 => "F20", [], F20, 0xffd1;
    F21 => "F21", [], F21, 0xffd2;
    F22 => "F22", [], F22, 0xffd3;
    F23 => "F23", [], F23, 0xffd4;
    F24 => "F24", [], F24, 0xffd5;
    PrintScreen => "PrintScreen", ["print", "prtsc", "prtscn", "prtscr", "printscr", "snapshot"], PrintScreen, 0xff61;
    Pause => "Pause", ["break"], Pause, 0xff13;
    ScrollLock => "ScrollLock", ["scroll", "scrlk"], ScrollLock, 0xff14;
    Insert => "Insert", ["ins"], Insert, 0xff63;
    Delete => "Delete", ["del"], Delete, 0xffff;
    Home => "Home", [], Home, 0xff50;
    End => "End", [], End, 0xff57;
    PageUp => "PageUp", ["pgup", "prior"], PageUp, 0xff55;
    PageDown => "PageDown", ["pgdn", "pgdown", "next"], PageDown, 0xff56;
    Up => "Up", ["arrowup"], ArrowUp, 0xff52;
    Down => "Down", ["arrowdown"], ArrowDown, 0xff54;
    Left => "Left", ["arrowleft"], ArrowLeft, 0xff51;
    Right => "Right", ["arrowright"], ArrowRight, 0xff53;
    Space => "Space", ["spacebar"], Space, 0x0020;
    Tab => "Tab", [], Tab, 0xff09;
    Enter => "Enter", ["return"], Enter, 0xff0d;
    Escape => "Esc", ["escape"], Escape, 0xff1b;
    Backspace => "Backspace", ["back"], Backspace, 0xff08;
    Minus => "Minus", ["-"], Minus, 0x002d;
    Equal => "Equal", ["=", "equals"], Equal, 0x003d;
    BracketLeft => "BracketLeft", ["["], BracketLeft, 0x005b;
    BracketRight => "BracketRight", ["]"], BracketRight, 0x005d;
    Backslash => "Backslash", ["\\"], Backslash, 0x005c;
    Semicolon => "Semicolon", [";"], Semicolon, 0x003b;
    Quote => "Quote", ["'", "apostrophe"], Quote, 0x0027;
    Comma => "Comma", [","], Comma, 0x002c;
    Period => "Period", [".", "dot"], Period, 0x002e;
    Slash => "Slash", ["/"], Slash, 0x002f;
    Backquote => "Backquote", ["`", "grave"], Backquote, 0x0060;
    Num0 => "Num0", ["numpad0"], Numpad0, 0xffb0;
    Num1 => "Num1", ["numpad1"], Numpad1, 0xffb1;
    Num2 => "Num2", ["numpad2"], Numpad2, 0xffb2;
    Num3 => "Num3", ["numpad3"], Numpad3, 0xffb3;
    Num4 => "Num4", ["numpad4"], Numpad4, 0xffb4;
    Num5 => "Num5", ["numpad5"], Numpad5, 0xffb5;
    Num6 => "Num6", ["numpad6"], Numpad6, 0xffb6;
    Num7 => "Num7", ["numpad7"], Numpad7, 0xffb7;
    Num8 => "Num8", ["numpad8"], Numpad8, 0xffb8;
    Num9 => "Num9", ["numpad9"], Numpad9, 0xffb9;
    NumAdd => "NumAdd", ["numpadadd", "numplus"], NumpadAdd, 0xffab;
    NumSubtract => "NumSubtract", ["numpadsubtract", "numminus"], NumpadSubtract, 0xffad;
    NumMultiply => "NumMultiply", ["numpadmultiply"], NumpadMultiply, 0xffaa;
    NumDivide => "NumDivide", ["numpaddivide"], NumpadDivide, 0xffaf;
    NumDecimal => "NumDecimal", ["numpaddecimal"], NumpadDecimal, 0xffae;
}

impl Named {
    fn is_function_key(self) -> bool {
        matches!(
            self,
            Named::F1
                | Named::F2
                | Named::F3
                | Named::F4
                | Named::F5
                | Named::F6
                | Named::F7
                | Named::F8
                | Named::F9
                | Named::F10
                | Named::F11
                | Named::F12
        )
    }
    /// Keys no ordinary app uses on their own, so a bare global hotkey on
    /// them doesn't take anything away from the user.
    fn safe_alone(self) -> bool {
        matches!(
            self,
            Named::F13
                | Named::F14
                | Named::F15
                | Named::F16
                | Named::F17
                | Named::F18
                | Named::F19
                | Named::F20
                | Named::F21
                | Named::F22
                | Named::F23
                | Named::F24
                | Named::PrintScreen
                | Named::Pause
                | Named::ScrollLock
        )
    }
    fn is_punctuation(self) -> bool {
        matches!(
            self,
            Named::Minus
                | Named::Equal
                | Named::BracketLeft
                | Named::BracketRight
                | Named::Backslash
                | Named::Semicolon
                | Named::Quote
                | Named::Comma
                | Named::Period
                | Named::Slash
                | Named::Backquote
        )
    }
    /// Keys the editor keeps for itself: Enter copies, Esc cancels,
    /// Delete/Backspace remove the selected annotation, Space types a space.
    fn reserved_in_editor(self) -> bool {
        matches!(
            self,
            Named::Enter
                | Named::Escape
                | Named::Delete
                | Named::Backspace
                | Named::Space
                | Named::Tab
        )
    }
    fn from_logical(n: &WNamed) -> Option<Named> {
        Some(match n {
            WNamed::F1 => Named::F1,
            WNamed::F2 => Named::F2,
            WNamed::F3 => Named::F3,
            WNamed::F4 => Named::F4,
            WNamed::F5 => Named::F5,
            WNamed::F6 => Named::F6,
            WNamed::F7 => Named::F7,
            WNamed::F8 => Named::F8,
            WNamed::F9 => Named::F9,
            WNamed::F10 => Named::F10,
            WNamed::F11 => Named::F11,
            WNamed::F12 => Named::F12,
            WNamed::F13 => Named::F13,
            WNamed::F14 => Named::F14,
            WNamed::F15 => Named::F15,
            WNamed::F16 => Named::F16,
            WNamed::F17 => Named::F17,
            WNamed::F18 => Named::F18,
            WNamed::F19 => Named::F19,
            WNamed::F20 => Named::F20,
            WNamed::F21 => Named::F21,
            WNamed::F22 => Named::F22,
            WNamed::F23 => Named::F23,
            WNamed::F24 => Named::F24,
            WNamed::PrintScreen => Named::PrintScreen,
            WNamed::Pause => Named::Pause,
            WNamed::ScrollLock => Named::ScrollLock,
            WNamed::Insert => Named::Insert,
            WNamed::Delete => Named::Delete,
            WNamed::Home => Named::Home,
            WNamed::End => Named::End,
            WNamed::PageUp => Named::PageUp,
            WNamed::PageDown => Named::PageDown,
            WNamed::ArrowUp => Named::Up,
            WNamed::ArrowDown => Named::Down,
            WNamed::ArrowLeft => Named::Left,
            WNamed::ArrowRight => Named::Right,
            WNamed::Space => Named::Space,
            WNamed::Tab => Named::Tab,
            WNamed::Enter => Named::Enter,
            WNamed::Escape => Named::Escape,
            WNamed::Backspace => Named::Backspace,
            _ => return None,
        })
    }
}

impl Key {
    pub fn name(self) -> String {
        match self {
            Key::Char(c) => c.to_ascii_uppercase().to_string(),
            Key::Named(n) => n.name().to_string(),
            Key::Typed(c) => c.to_string(),
        }
    }

    fn hotkey_code(self) -> Code {
        match self {
            Key::Char(c) => match c {
                'a' => Code::KeyA,
                'b' => Code::KeyB,
                'c' => Code::KeyC,
                'd' => Code::KeyD,
                'e' => Code::KeyE,
                'f' => Code::KeyF,
                'g' => Code::KeyG,
                'h' => Code::KeyH,
                'i' => Code::KeyI,
                'j' => Code::KeyJ,
                'k' => Code::KeyK,
                'l' => Code::KeyL,
                'm' => Code::KeyM,
                'n' => Code::KeyN,
                'o' => Code::KeyO,
                'p' => Code::KeyP,
                'q' => Code::KeyQ,
                'r' => Code::KeyR,
                's' => Code::KeyS,
                't' => Code::KeyT,
                'u' => Code::KeyU,
                'v' => Code::KeyV,
                'w' => Code::KeyW,
                'x' => Code::KeyX,
                'y' => Code::KeyY,
                'z' => Code::KeyZ,
                '0' => Code::Digit0,
                '1' => Code::Digit1,
                '2' => Code::Digit2,
                '3' => Code::Digit3,
                '4' => Code::Digit4,
                '5' => Code::Digit5,
                '6' => Code::Digit6,
                '7' => Code::Digit7,
                '8' => Code::Digit8,
                _ => Code::Digit9,
            },
            Key::Named(n) => n.hotkey_code(),
            // Refused by `validate` for global hotkeys; registering this fails.
            Key::Typed(_) => Code::Unidentified,
        }
    }

    /// X11 keysym (letters and digits are their ASCII code).
    pub fn keysym(self) -> u32 {
        match self {
            Key::Char(c) => c as u32,
            Key::Named(n) => n.keysym(),
            Key::Typed(c) if (c as u32) < 0x100 => c as u32,
            Key::Typed(c) => 0x0100_0000 + c as u32,
        }
    }

    fn parse(s: &str) -> Option<Key> {
        let t = s.trim();
        if t.chars().count() == 1 {
            let c = t.chars().next()?;
            if c.is_ascii_alphanumeric() {
                return Some(Key::Char(c.to_ascii_lowercase()));
            }
        }
        let lower = t.to_ascii_lowercase();
        let named = Named::ALL
            .iter()
            .find(|n| n.name().eq_ignore_ascii_case(t) || n.aliases().contains(&lower.as_str()))
            .map(|n| Key::Named(*n));
        // Any other single character: a 2.0 tool key such as `#` or `ö`.
        named.or_else(|| {
            let mut it = t.chars();
            match (it.next(), it.next()) {
                (Some(c), None) if !c.is_whitespace() && !c.is_control() => {
                    Some(Key::Typed(c.to_lowercase().next().unwrap_or(c)))
                }
                _ => None,
            }
        })
    }
}

/// Turn a winit key event into our `Key`. Returns `None` for modifier keys
/// and keys that can't be bound (dead keys, IME, media keys, ...).
pub fn key_from_winit(logical: &WKey, physical: &PhysicalKey) -> Option<Key> {
    // 1. A letter, as the layout writes it (handles AZERTY/QWERTZ/Dvorak).
    if let WKey::Character(s) = logical {
        let mut it = s.chars();
        if let (Some(c), None) = (it.next(), it.next()) {
            if c.is_ascii_alphabetic() {
                return Some(Key::Char(c.to_ascii_lowercase()));
            }
        }
    }
    // 2. Position: digits, punctuation, named keys, and letters on layouts
    //    whose letters aren't ASCII (Cyrillic, Greek, ...).
    if let PhysicalKey::Code(kc) = physical {
        if let Some(k) = key_from_code(*kc) {
            return Some(k);
        }
    }
    // 3. Logical named key (e.g. a remapped PrintScreen without a scancode).
    if let WKey::Named(n) = logical {
        return Named::from_logical(n).map(Key::Named);
    }
    None
}

fn key_from_code(kc: KeyCode) -> Option<Key> {
    let letter = |c: char| Some(Key::Char(c));
    match kc {
        KeyCode::KeyA => letter('a'),
        KeyCode::KeyB => letter('b'),
        KeyCode::KeyC => letter('c'),
        KeyCode::KeyD => letter('d'),
        KeyCode::KeyE => letter('e'),
        KeyCode::KeyF => letter('f'),
        KeyCode::KeyG => letter('g'),
        KeyCode::KeyH => letter('h'),
        KeyCode::KeyI => letter('i'),
        KeyCode::KeyJ => letter('j'),
        KeyCode::KeyK => letter('k'),
        KeyCode::KeyL => letter('l'),
        KeyCode::KeyM => letter('m'),
        KeyCode::KeyN => letter('n'),
        KeyCode::KeyO => letter('o'),
        KeyCode::KeyP => letter('p'),
        KeyCode::KeyQ => letter('q'),
        KeyCode::KeyR => letter('r'),
        KeyCode::KeyS => letter('s'),
        KeyCode::KeyT => letter('t'),
        KeyCode::KeyU => letter('u'),
        KeyCode::KeyV => letter('v'),
        KeyCode::KeyW => letter('w'),
        KeyCode::KeyX => letter('x'),
        KeyCode::KeyY => letter('y'),
        KeyCode::KeyZ => letter('z'),
        KeyCode::Digit0 => letter('0'),
        KeyCode::Digit1 => letter('1'),
        KeyCode::Digit2 => letter('2'),
        KeyCode::Digit3 => letter('3'),
        KeyCode::Digit4 => letter('4'),
        KeyCode::Digit5 => letter('5'),
        KeyCode::Digit6 => letter('6'),
        KeyCode::Digit7 => letter('7'),
        KeyCode::Digit8 => letter('8'),
        KeyCode::Digit9 => letter('9'),
        other => Named::from_key_code(other).map(Key::Named),
    }
}

/// The character a key press types (lowercase), for matching 2.0-style
/// `Key::Typed` tool keys.
pub fn typed_char(logical: &WKey) -> Option<char> {
    match logical {
        WKey::Character(s) => {
            let mut it = s.chars();
            match (it.next(), it.next()) {
                (Some(c), None) => Some(c.to_lowercase().next().unwrap_or(c)),
                _ => None,
            }
        }
        _ => None,
    }
}

/// True for keys that are only modifiers (pressing them alone never binds).
pub fn is_modifier_key(logical: &WKey) -> bool {
    matches!(
        logical,
        WKey::Named(
            WNamed::Control
                | WNamed::Shift
                | WNamed::Alt
                | WNamed::AltGraph
                | WNamed::Super
                | WNamed::Meta
                | WNamed::Hyper
                | WNamed::CapsLock
                | WNamed::NumLock
                | WNamed::Fn
                | WNamed::FnLock
        )
    )
}

// ───────────────────────── bindings ─────────────────────────

/// A modifier set plus one key, e.g. `Ctrl+Shift+S`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Binding {
    pub mods: Mods,
    pub key: Key,
}

impl Binding {
    pub fn new(mods: Mods, key: Key) -> Self {
        Self { mods, key }
    }

    /// Parse `Ctrl+Shift+S`. Empty or whitespace means unbound (`Ok(None)`).
    pub fn parse(s: &str) -> Result<Option<Binding>, String> {
        let s = s.trim();
        if s.is_empty() {
            return Ok(None);
        }
        if s == "+" {
            // A 2.0 tool key; `+` can't be split like the rest.
            return Ok(Some(Binding {
                mods: Mods::NONE,
                key: Key::Typed('+'),
            }));
        }
        let mut mods = Mods::NONE;
        let mut key: Option<Key> = None;
        for raw in s.split('+') {
            let part = raw.trim();
            if part.is_empty() {
                return Err(format!(
                    "'{s}': empty part (use Equal or NumAdd for the + key)"
                ));
            }
            let m = match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "ctl" => Some(Mods::CTRL),
                "shift" => Some(Mods::SHIFT),
                "alt" | "option" | "opt" => Some(Mods::ALT),
                "super" | "win" | "windows" | "meta" | "cmd" | "command" => Some(Mods::SUPER),
                _ => None,
            };
            if let Some(m) = m {
                if key.is_some() {
                    return Err(format!("'{s}': modifiers must come before the key"));
                }
                mods = mods | m;
                continue;
            }
            if key.is_some() {
                return Err(format!("'{s}': only one key can follow the modifiers"));
            }
            key = Some(Key::parse(part).ok_or_else(|| format!("'{s}': unknown key '{part}'"))?);
        }
        match key {
            Some(key) => Ok(Some(Binding { mods, key })),
            None => Err(format!("'{s}': no key, only modifiers")),
        }
    }

    /// The global-hotkey crate's representation.
    pub fn to_hotkey(self) -> HotKey {
        let mut m = Modifiers::empty();
        if self.mods.ctrl() {
            m |= Modifiers::CONTROL;
        }
        if self.mods.shift() {
            m |= Modifiers::SHIFT;
        }
        if self.mods.alt() {
            m |= Modifiers::ALT;
        }
        if self.mods.super_key() {
            m |= Modifiers::SUPER;
        }
        HotKey::new(Some(m), self.key.hotkey_code())
    }

    /// Does a key press trigger this binding? Tool keys ignore Shift (so
    /// Caps Lock or a held Shift still switches tools); everything else
    /// needs the exact modifier set.
    pub fn matches(self, mods: Mods, key: Key, scope: Scope) -> bool {
        if key != self.key {
            return false;
        }
        match scope {
            Scope::Tool => mods.without_shift() == self.mods.without_shift(),
            _ => mods == self.mods,
        }
    }
}

impl fmt::Display for Binding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.mods.ctrl() {
            f.write_str("Ctrl+")?;
        }
        if self.mods.shift() {
            f.write_str("Shift+")?;
        }
        if self.mods.alt() {
            f.write_str("Alt+")?;
        }
        if self.mods.super_key() {
            f.write_str(if cfg!(target_os = "windows") {
                "Win+"
            } else {
                "Super+"
            })?;
        }
        f.write_str(&self.key.name())
    }
}

/// Where a binding is used; each has its own rules.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Scope {
    /// Registered with the OS; fires in any app.
    Global,
    /// An editor action while the capture overlay is open.
    Editor,
    /// Picks a tool while the capture overlay is open.
    Tool,
}

/// Combos the OS or every app relies on. Grabbing them globally would break
/// the machine in ways that are hard to undo from inside screen-mark.
const NEVER_GLOBAL: &[&str] = &[
    "Alt+F4",
    "Alt+Tab",
    "Alt+Shift+Tab",
    "Alt+Esc",
    "Alt+Space",
    "Ctrl+Esc",
    "Ctrl+Alt+Delete",
    "Ctrl+Shift+Esc",
    "Super+L",
    "Super+D",
    "Super+Tab",
];

/// Check a binding against the rules of its scope. Returns a sentence the
/// Settings window shows as-is.
pub fn validate(b: Binding, scope: Scope) -> Result<(), String> {
    match scope {
        Scope::Global => {
            if NEVER_GLOBAL
                .iter()
                .any(|s| Binding::parse(s).ok().flatten().is_some_and(|nb| nb == b))
            {
                return Err(format!("{b} is used by the system"));
            }
            match b.key {
                Key::Typed(_) => Err(format!(
                    "{b} can't be a global hotkey; use a letter, digit or F-key"
                )),
                // global-hotkey registers punctuation by its US character,
                // which sits on a different key on other layouts.
                Key::Named(n) if n.is_punctuation() => Err(format!(
                    "{b}: punctuation keys move between keyboard layouts, so they \
                     can't be global hotkeys; use a letter, digit or F-key"
                )),
                Key::Named(n) if n.safe_alone() || n.is_function_key() => Ok(()),
                // Letters, digits, Space, arrows, Enter, ... type or move
                // the cursor; without Ctrl/Alt/Win they'd stop working.
                _ if b.mods.has_command() => Ok(()),
                _ => Err(format!("{b} needs Ctrl, Alt or Win")),
            }
        }
        Scope::Editor => {
            if let Some(e) = not_in_overlay(b) {
                return Err(e);
            }
            if let Key::Named(n) = b.key {
                if n.reserved_in_editor() {
                    // Bare Enter/Esc/Delete/... keep their fixed meaning;
                    // with Ctrl/Alt/Win they're free.
                    return if b.mods.has_command() {
                        Ok(())
                    } else {
                        Err(format!("{} is fixed in the editor", n.name()))
                    };
                }
                if n.is_function_key() || n.safe_alone() || !n.is_punctuation() {
                    return Ok(());
                }
            }
            if b.mods.has_command() {
                Ok(())
            } else {
                Err(format!(
                    "{b} would type text; editor actions need Ctrl, Alt or Win (bare keys pick tools)"
                ))
            }
        }
        Scope::Tool => {
            if !b.mods.is_empty() {
                return Err(format!("{b}: tool keys are a single key, no modifiers"));
            }
            if b.key == Key::Named(Named::PrintScreen) {
                return Err(format!("{b} only works as a global hotkey"));
            }
            if let Key::Named(n) = b.key {
                if n.reserved_in_editor() {
                    return Err(format!("{} is fixed in the editor", n.name()));
                }
            }
            Ok(())
        }
    }
}

// ───────────────────────── the binding table ─────────────────────────

/// Keys the capture overlay can't use for an editor action.
fn not_in_overlay(b: Binding) -> Option<String> {
    match b.key {
        // Windows sends no key-down for PrintScreen to a window.
        Key::Named(Named::PrintScreen) => Some(format!("{b} only works as a global hotkey")),
        Key::Typed(_) => Some(format!("{b}: pick the key again in Settings")),
        _ => None,
    }
}

/// A binding `validate` accepts but that has a side effect worth a line in
/// Settings. Global hotkeys win over every app, so a one-modifier shortcut
/// or a bare F-key stops reaching the app in front.
pub fn caution(b: Binding, scope: Scope) -> Option<String> {
    if scope != Scope::Global {
        return None;
    }
    let common = match b.key {
        Key::Named(n) => n.is_function_key() && b.mods.without_shift().is_empty(),
        Key::Char(_) => !b.mods.super_key() && b.mods.count() == 1,
        Key::Typed(_) => false,
    };
    common.then(|| format!("Other apps won't see {b} while screen-mark runs."))
}

/// Everything that can be bound, in Settings order.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum KeyId {
    Region,
    Screenshot,
    Copy,
    Save,
    Undo,
    Redo,
    SelectAll,
    Pin,
    Ocr,
    Upload,
    /// Index into `ShortcutsConfig::entries()` (0..14).
    Tool(usize),
}

impl KeyId {
    pub fn scope(self) -> Scope {
        match self {
            KeyId::Region | KeyId::Screenshot => Scope::Global,
            KeyId::Tool(_) => Scope::Tool,
            _ => Scope::Editor,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            KeyId::Region => "Capture a region",
            KeyId::Screenshot => "Screenshot: save + copy",
            KeyId::Copy => "Copy",
            KeyId::Save => "Save",
            KeyId::Undo => "Undo",
            KeyId::Redo => "Redo",
            KeyId::SelectAll => "Select whole screen",
            KeyId::Pin => "Pin to screen",
            KeyId::Ocr => "Copy text (OCR)",
            KeyId::Upload => "Upload to Imgur",
            KeyId::Tool(i) => TOOL_LABELS.get(i).copied().unwrap_or("Tool"),
        }
    }

    /// The default binding, as written in config.toml ("" = unbound).
    pub fn default_binding(self) -> &'static str {
        match self {
            KeyId::Region => "Ctrl+Shift+S",
            KeyId::Screenshot => "Ctrl+PrintScreen",
            KeyId::Copy => "Ctrl+C",
            KeyId::Save => "Ctrl+S",
            KeyId::Undo => "Ctrl+Z",
            KeyId::Redo => "Ctrl+Shift+Z",
            KeyId::SelectAll => "Ctrl+A",
            KeyId::Pin | KeyId::Ocr | KeyId::Upload => "",
            KeyId::Tool(i) => TOOL_DEFAULTS.get(i).copied().unwrap_or(""),
        }
    }

    /// Every bindable id, globals first, then editor actions, then tools.
    pub fn all() -> Vec<KeyId> {
        let mut v = vec![
            KeyId::Region,
            KeyId::Screenshot,
            KeyId::Copy,
            KeyId::Save,
            KeyId::Undo,
            KeyId::Redo,
            KeyId::SelectAll,
            KeyId::Pin,
            KeyId::Ocr,
            KeyId::Upload,
        ];
        v.extend((0..TOOL_LABELS.len()).map(KeyId::Tool));
        v
    }
}

/// Tool labels, in `ShortcutsConfig::entries()` order.
pub const TOOL_LABELS: [&str; 14] = [
    "Select / move",
    "Arrow",
    "Rectangle",
    "Circle",
    "Rounded rectangle",
    "Line",
    "Pencil",
    "Highlight",
    "Spotlight",
    "Text",
    "Pixelate",
    "Step marker",
    "Eyedropper",
    "Measurement",
];

const TOOL_DEFAULTS: [&str; 14] = [
    "V", "A", "R", "C", "O", "L", "P", "H", "F", "T", "B", "N", "I", "M",
];

/// The result of checking every binding in a config.
#[derive(Debug, Default, Clone)]
pub struct Report {
    /// Parsed bindings that passed their scope's rules (unbound = absent).
    pub bound: Vec<(KeyId, Binding)>,
    /// Bindings that don't parse or break their scope's rules.
    pub invalid: Vec<(KeyId, String)>,
    /// Pairs bound to the same keys where both would be live at once.
    pub conflicts: Vec<(KeyId, KeyId, Binding)>,
}

impl Report {
    pub fn is_ok(&self) -> bool {
        self.invalid.is_empty() && self.conflicts.is_empty()
    }

    pub fn binding(&self, id: KeyId) -> Option<Binding> {
        self.bound.iter().find(|(i, _)| *i == id).map(|(_, b)| *b)
    }

    /// One sentence about `id`, if something is wrong with it.
    pub fn problem(&self, id: KeyId) -> Option<String> {
        if let Some((_, msg)) = self.invalid.iter().find(|(i, _)| *i == id) {
            return Some(msg.clone());
        }
        self.conflicts.iter().find_map(|(a, b, binding)| {
            if *a == id {
                Some(format!("{binding} is also {}", b.label()))
            } else if *b == id {
                Some(format!("{binding} is also {}", a.label()))
            } else {
                None
            }
        })
    }

    /// The first problem in Settings order, for a status line.
    pub fn first_problem(&self) -> Option<(KeyId, String)> {
        KeyId::all()
            .into_iter()
            .find_map(|id| self.problem(id).map(|p| (id, p)))
    }
}

/// Two bindings collide if they would ever be live at the same time and a
/// key press could match both.
fn collide(a: (KeyId, Binding), b: (KeyId, Binding)) -> bool {
    let (sa, sb) = (a.0.scope(), b.0.scope());
    match (sa, sb) {
        // Tools ignore Shift.
        (Scope::Tool, Scope::Tool) => {
            a.1.key == b.1.key && a.1.mods.without_shift() == b.1.mods.without_shift()
        }
        (Scope::Tool, _) | (_, Scope::Tool) => {
            let (t, o) = if sa == Scope::Tool {
                (a.1, b.1)
            } else {
                (b.1, a.1)
            };
            t.key == o.key && o.mods.without_shift().is_empty()
        }
        // A global hotkey is grabbed by the OS while the editor is open, so
        // an editor action on the same keys would never fire.
        _ => a.1 == b.1,
    }
}

/// Check a full set of bindings. `get` returns the configured string for an id.
pub fn check(get: impl Fn(KeyId) -> String) -> Report {
    let mut report = Report::default();
    for id in KeyId::all() {
        let raw = get(id);
        match Binding::parse(&raw) {
            Ok(None) => {}
            Ok(Some(b)) => match validate(b, id.scope()) {
                Ok(()) => report.bound.push((id, b)),
                Err(e) => report.invalid.push((id, e)),
            },
            Err(e) => report.invalid.push((id, e)),
        }
    }
    let bound = report.bound.clone();
    for (i, a) in bound.iter().enumerate() {
        for b in bound.iter().skip(i + 1) {
            if collide(*a, *b) {
                report.conflicts.push((a.0, b.0, a.1));
            }
        }
    }
    report
}

/// What the overlay should do for a key press. Built once from the config;
/// earlier entries win if a hand-edited config has a conflict.
#[derive(Debug, Clone, Default)]
pub struct KeyMap {
    editor: Vec<(KeyId, Binding)>,
    tools: Vec<(usize, Binding)>,
}

impl KeyMap {
    pub fn from_report(report: &Report) -> Self {
        let mut map = KeyMap::default();
        for (id, b) in &report.bound {
            match id.scope() {
                Scope::Editor => map.editor.push((*id, *b)),
                Scope::Tool => {
                    if let KeyId::Tool(i) = id {
                        map.tools.push((*i, *b));
                    }
                }
                Scope::Global => {}
            }
        }
        map
    }

    /// The editor action bound to this press, if any.
    pub fn editor_action(&self, mods: Mods, key: Key) -> Option<KeyId> {
        self.editor
            .iter()
            .find(|(_, b)| b.matches(mods, key, Scope::Editor))
            .map(|(id, _)| *id)
    }

    /// The tool index bound to this press, if any.
    pub fn tool(&self, mods: Mods, key: Key) -> Option<usize> {
        self.tools
            .iter()
            .find(|(_, b)| b.matches(mods, key, Scope::Tool))
            .map(|(i, _)| *i)
    }
}

/// One keyboard event, already converted (see `key_from_winit`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyPress {
    pub mods: Mods,
    /// `None` for a bare modifier key.
    pub key: Option<Key>,
    /// false for a key release.
    pub pressed: bool,
}
