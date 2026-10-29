//! Global hotkeys: the two bindings that work in any app.
//!
//! `GlobalHotkeys` owns the OS registrations and reports, per hotkey, whether
//! it is live, unbound, invalid, or taken by another program. It can be
//! suspended while Settings is waiting for a new key combination (otherwise
//! the OS delivers the combo to us as a hotkey and the Settings window never
//! sees it), and it turns key-repeat into a single press.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use crate::keys::{validate, Binding, KeyId, Scope};

/// Parse a binding string like "Ctrl+Shift+S" into modifiers and a key code.
/// Kept for callers and tests from 2.0; new code uses `keys::Binding`.
pub fn parse_binding(binding: &str) -> Result<(Modifiers, Code), String> {
    match Binding::parse(binding)? {
        // A bare typed character (2.0 tool keys) has no key code.
        Some(b) if matches!(b.key, crate::keys::Key::Typed(_)) => {
            Err(format!("'{binding}': not a key a hotkey can use"))
        }
        Some(b) => {
            let hk = b.to_hotkey();
            Ok((hk.mods, hk.key))
        }
        None => Err("Empty hotkey binding".into()),
    }
}

/// What happened to one global hotkey.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Nothing bound.
    Unbound,
    /// Registered; pressing it works.
    Active,
    /// Doesn't parse or breaks the rules for global hotkeys.
    Invalid(String),
    /// Another program (or Windows) already owns this combination.
    Taken,
}

impl Status {
    pub fn describe(&self, binding: &str) -> Option<String> {
        match self {
            Status::Unbound | Status::Active => None,
            Status::Invalid(msg) => Some(msg.clone()),
            Status::Taken => Some(format!("{binding} is taken by another app")),
        }
    }
}

/// A press of one of our hotkeys arrives as (KeyId) once per physical press.
pub struct GlobalHotkeys {
    manager: Option<GlobalHotKeyManager>,
    registered: Vec<(KeyId, HotKey, Binding)>,
    /// Combinations we released recently. On X11 our own ungrab reaches the
    /// server up to ~50 ms later (the hotkey thread flushes on its next
    /// poll), so checking "is it taken?" right away would find our own grab.
    released: Vec<(Binding, Instant)>,
    statuses: HashMap<KeyId, Status>,
    /// Bindings to (re)register on resume.
    wanted: Vec<(KeyId, String)>,
    suspended: bool,
    /// Hotkey ids whose key is still down, so auto-repeat doesn't capture
    /// ten screenshots. Cleared on release, or after a timeout in case the
    /// release never arrives (focus change mid-press).
    held: HashMap<u32, Instant>,
}

const HELD_TIMEOUT: Duration = Duration::from_millis(1500);
const RELEASE_GRACE: Duration = Duration::from_secs(2);

impl Default for GlobalHotkeys {
    fn default() -> Self {
        Self::new()
    }
}

impl GlobalHotkeys {
    pub fn new() -> Self {
        Self {
            manager: None,
            registered: Vec::new(),
            released: Vec::new(),
            statuses: HashMap::new(),
            wanted: Vec::new(),
            suspended: false,
            held: HashMap::new(),
        }
    }

    /// Register `bindings` (id, config string), replacing whatever was
    /// registered before. Statuses are updated for every id given.
    pub fn apply(&mut self, bindings: &[(KeyId, String)]) {
        self.wanted = bindings.to_vec();
        if self.suspended {
            return;
        }
        self.register_wanted();
    }

    fn register_wanted(&mut self) {
        let wanted = self.wanted.clone();
        // Parse and validate first, so we know what each id should hold.
        let mut seen: Vec<(Binding, KeyId)> = Vec::new();
        let mut plan: Vec<(KeyId, Result<Option<Binding>, Status>)> = Vec::new();
        for (id, raw) in &wanted {
            let entry = match Binding::parse(raw) {
                Ok(None) => Ok(None),
                Err(e) => Err(Status::Invalid(e)),
                Ok(Some(b)) => {
                    if let Err(e) = validate(b, Scope::Global) {
                        Err(Status::Invalid(e))
                    } else if let Some((_, other)) = seen.iter().find(|(sb, _)| *sb == b) {
                        Err(Status::Invalid(format!("{b} is also {}", other.label())))
                    } else {
                        seen.push((b, *id));
                        Ok(Some(b))
                    }
                }
            };
            plan.push((*id, entry));
        }
        // Keep registrations that don't change; release the rest.
        let keep: Vec<KeyId> = self
            .registered
            .iter()
            .filter(|(id, _, b)| plan.iter().any(|(pid, e)| pid == id && *e == Ok(Some(*b))))
            .map(|(id, _, _)| *id)
            .collect();
        let drop: Vec<(KeyId, HotKey, Binding)> = self
            .registered
            .iter()
            .filter(|(id, _, _)| !keep.contains(id))
            .cloned()
            .collect();
        self.release(&drop);
        self.statuses.retain(|id, _| keep.contains(id));
        for (id, entry) in plan {
            if keep.contains(&id) {
                self.statuses.insert(id, Status::Active);
                continue;
            }
            let status = match entry {
                Ok(None) => Status::Unbound,
                Err(st) => st,
                Ok(Some(b)) => self.register_one(id, b),
            };
            let raw = wanted
                .iter()
                .find(|(i, _)| *i == id)
                .map(|(_, r)| r.as_str())
                .unwrap_or("");
            if status != Status::Active {
                tracing::warn!("Global hotkey {:?} '{}': {:?}", id, raw, status);
            } else {
                tracing::info!("Global hotkey {:?} registered: {}", id, raw);
            }
            self.statuses.insert(id, status);
        }
    }

    fn release(&mut self, list: &[(KeyId, HotKey, Binding)]) {
        let now = Instant::now();
        self.released
            .retain(|(_, t)| now.duration_since(*t) < RELEASE_GRACE);
        if let Some(m) = &self.manager {
            for (id, hk, b) in list {
                let _ = m.unregister(*hk);
                self.registered.retain(|(rid, _, _)| rid != id);
                self.released.push((*b, now));
            }
        }
        self.registered
            .retain(|(id, _, _)| !list.iter().any(|(lid, _, _)| lid == id));
        for (_, hk, _) in list {
            self.held.remove(&hk.id());
        }
    }

    fn register_one(&mut self, id: KeyId, b: Binding) -> Status {
        // On X11 the hotkey crate can't see "already grabbed" (the error is
        // asynchronous), so ask the X server directly first, unless we held
        // this combination a moment ago (then the grab it finds is ours).
        let ours = self
            .released
            .iter()
            .any(|(rb, t)| *rb == b && t.elapsed() < RELEASE_GRACE);
        if !ours && probe_taken(b) {
            return Status::Taken;
        }
        if self.manager.is_none() {
            match GlobalHotKeyManager::new() {
                Ok(m) => self.manager = Some(m),
                Err(e) => return Status::Invalid(format!("hotkeys unavailable: {e}")),
            }
        }
        let hk = b.to_hotkey();
        let manager = self.manager.as_ref().expect("manager created above");
        match manager.register(hk) {
            Ok(()) => {
                self.registered.push((id, hk, b));
                Status::Active
            }
            Err(global_hotkey::Error::AlreadyRegistered(_)) => Status::Taken,
            Err(e) => {
                // RegisterHotKey reports a combo owned by another program as
                // a generic OS error on some Windows builds.
                let msg = e.to_string();
                if msg.contains("1409") || msg.to_ascii_lowercase().contains("already") {
                    Status::Taken
                } else {
                    Status::Invalid(msg)
                }
            }
        }
    }

    fn unregister_all(&mut self) {
        let all = self.registered.clone();
        self.release(&all);
        self.held.clear();
    }

    /// Release every registration (Settings is capturing a key combo).
    pub fn suspend(&mut self) {
        if !self.suspended {
            self.suspended = true;
            self.unregister_all();
            tracing::info!("Global hotkeys suspended");
        }
    }

    /// Register again after `suspend`.
    pub fn resume(&mut self) {
        if self.suspended {
            self.suspended = false;
            self.register_wanted();
            tracing::info!("Global hotkeys resumed");
        }
    }

    pub fn is_suspended(&self) -> bool {
        self.suspended
    }

    pub fn status(&self, id: KeyId) -> Status {
        self.statuses.get(&id).cloned().unwrap_or(Status::Unbound)
    }

    /// The configured string for `id` (as last applied).
    pub fn binding_text(&self, id: KeyId) -> String {
        self.wanted
            .iter()
            .find(|(i, _)| *i == id)
            .map(|(_, s)| s.clone())
            .unwrap_or_default()
    }

    /// Feed one event from `GlobalHotKeyEvent::receiver()`. Returns the
    /// hotkey that was pressed, once per physical press.
    pub fn handle(&mut self, event: &GlobalHotKeyEvent, now: Instant) -> Option<KeyId> {
        let id = self
            .registered
            .iter()
            .find(|(_, hk, _)| hk.id() == event.id)
            .map(|(id, _, _)| *id)?;
        repeat_filter(
            &mut self.held,
            event.id,
            event.state == HotKeyState::Pressed,
            now,
        )
        .then_some(id)
    }
}

/// True if this event is a new press (not auto-repeat, not a release).
/// `held` remembers which hotkeys are down; a release clears it, and a
/// press more than 1.5 s after the last one counts as new even without a
/// release, because a release can be lost when focus changes mid-press.
pub fn repeat_filter(
    held: &mut HashMap<u32, Instant>,
    id: u32,
    pressed: bool,
    now: Instant,
) -> bool {
    if !pressed {
        held.remove(&id);
        return false;
    }
    if let Some(since) = held.get(&id) {
        if now.duration_since(*since) < HELD_TIMEOUT {
            return false;
        }
    }
    held.insert(id, now);
    true
}

/// Would registering `b` fail because someone else holds it? Checked before
/// registering, and by Settings right after a new combo is captured.
pub fn probe_taken(b: Binding) -> bool {
    #[cfg(target_os = "linux")]
    {
        x11_probe::taken(b).unwrap_or(false)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = b;
        false
    }
}

/// Check whether a combination can be registered right now, without
/// keeping it. Used by Settings to warn as soon as a combo is captured.
pub fn check_available(b: Binding) -> Status {
    if let Err(e) = validate(b, Scope::Global) {
        return Status::Invalid(e);
    }
    if probe_taken(b) {
        return Status::Taken;
    }
    #[cfg(target_os = "windows")]
    {
        let Ok(manager) = GlobalHotKeyManager::new() else {
            return Status::Active;
        };
        let hk = b.to_hotkey();
        match manager.register(hk) {
            Ok(()) => {
                let _ = manager.unregister(hk);
                Status::Active
            }
            Err(_) => Status::Taken,
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        Status::Active
    }
}

#[cfg(target_os = "linux")]
mod x11_probe {
    //! Ask the X server whether another client already grabbed a key combo.
    use crate::keys::Binding;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt, GrabMode, ModMask};

    pub fn taken(b: Binding) -> Option<bool> {
        let (conn, screen_num) = x11rb::connect(None).ok()?;
        let setup = conn.setup();
        let root = setup.roots.get(screen_num)?.root;
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let count = max - min + 1;
        let mapping = conn.get_keyboard_mapping(min, count).ok()?.reply().ok()?;
        let per = mapping.keysyms_per_keycode as usize;
        let want = b.key.keysym();
        // Letters: the keysym table lists lowercase at level 0.
        let keycode = (0..count as usize).find_map(|i| {
            let syms = &mapping.keysyms[i * per..(i + 1) * per];
            syms.contains(&want).then_some(min + i as u8)
        })?;
        let mut mods = ModMask::from(0u16);
        if b.mods.shift() {
            mods |= ModMask::SHIFT;
        }
        if b.mods.ctrl() {
            mods |= ModMask::CONTROL;
        }
        if b.mods.alt() {
            mods |= ModMask::M1;
        }
        if b.mods.super_key() {
            mods |= ModMask::M4;
        }
        let res = conn
            .grab_key(false, root, mods, keycode, GrabMode::ASYNC, GrabMode::ASYNC)
            .ok()?
            .check();
        match res {
            Ok(()) => {
                let _ = conn.ungrab_key(keycode, root, mods);
                // Round-trip so the server has processed the ungrab before
                // the hotkey thread (another connection) grabs the same keys.
                let _ = conn.get_input_focus().map(|c| c.reply());
                Some(false)
            }
            Err(_) => Some(true),
        }
    }
}
