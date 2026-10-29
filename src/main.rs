#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::num::NonZeroU32;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::Parser;
use winit::event::{ElementState, MouseButton as WinitMouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Window, WindowAttributes, WindowId, WindowLevel};

use winit::application::ApplicationHandler;

use screen_mark::capture;
use screen_mark::cli::{Cli, Commands};
use screen_mark::config::Config;
use screen_mark::countdown::Countdown;
use screen_mark::display::{self, Monitor};
use screen_mark::export;
use screen_mark::geometry::{Color, Point};
use screen_mark::history_ui::HistoryWindow;
use screen_mark::hotkey::{GlobalHotkeys, Status};
use screen_mark::keys::{self, KeyId, KeyMap, KeyPress};
use screen_mark::overlay::selection::{HitZone, Selection};
use screen_mark::overlay::toolbar::{ButtonAction, Toolbar, BUTTONS};
use screen_mark::pin::PinnedWindow;
use screen_mark::renderer::render_overlay;
use screen_mark::settings_ui::{Outcome, SettingsView, SettingsWindow, Tab};
use screen_mark::state::{AppState, OverlayState};
use screen_mark::toast::{self, Click, Kind, Spec, Toast};
use screen_mark::tools::{
    annotation_bounding_box, apply_redo, apply_undo, hit_test_annotation, move_annotation,
    recolor_annotation, record_undo, resize_annotation, Annotation, AnnotationTool, ResizeHandle,
    ToolKind, UndoAction,
};
use screen_mark::tray::{self, TrayEvent, TrayHandle};
use screen_mark::window_detect;

struct App {
    config: Config,
    state: AppState,
    tray: Option<TrayHandle>,
    tray_tx: Sender<TrayEvent>,
    tray_rx: Receiver<TrayEvent>,
    hotkey_rx: Receiver<global_hotkey::GlobalHotKeyEvent>,
    hotkey_wake: Arc<std::sync::OnceLock<EventLoopProxy<()>>>,
    overlay_window: Option<Arc<Window>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    pixmap: Option<tiny_skia::Pixmap>,
    modifiers: ModifiersState,
    /// Modifier state tracked separately for the settings window (used while
    /// capturing a new global-hotkey combination).
    settings_modifiers: ModifiersState,
    /// When the Settings window lost focus (see `SETTINGS_BLUR_GRACE`).
    settings_blur_at: Option<Instant>,
    needs_redraw: bool,
    last_render: Instant,
    /// A capture waiting for something to leave the screen first (the
    /// countdown window, or a popup that would end up in the picture).
    capture_at: std::collections::VecDeque<(Instant, CaptureKind)>,
    hotkeys: GlobalHotkeys,
    keymap: KeyMap,
    toast: Option<Toast>,
    /// Results from capture worker threads, with the monitor to show them on.
    jobs_tx: Sender<(Option<Spec>, Option<Monitor>)>,
    jobs_rx: Receiver<(Option<Spec>, Option<Monitor>)>,
    jobs_pending: usize,
    /// Wakes the event loop when a worker finishes.
    proxy: Option<EventLoopProxy<()>>,
    pinned_windows: Vec<PinnedWindow>,
    immediate_capture: bool,
    cli_only: bool,
    startup_notified: bool,
    countdown: Option<Countdown>,
    window_capture_mode: bool,
    window_rects: Vec<window_detect::WinRect>,
    settings_window: Option<SettingsWindow>,
    history_window: Option<HistoryWindow>,
}

const FRAME_INTERVAL: Duration = Duration::from_millis(16); // ~60fps cap

/// How long to wait after closing a popup before grabbing the screen, so
/// the compositor has removed it.
const TOAST_CLEAR_DELAY: Duration = Duration::from_millis(180);
/// How long the Settings window may be out of focus before it stops
/// listening for a combination.
const SETTINGS_BLUR_GRACE: Duration = Duration::from_millis(1500);
/// How long the tray menu takes to leave the screen after a click.
const TRAY_MENU_CLOSE_DELAY: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureKind {
    /// Open the overlay to drag a region (or pick a window).
    Region,
    /// One press: grab, save, copy.
    Screenshot,
}

/// Tool index (`ShortcutsConfig::entries()` order) to tool.
const TOOL_KINDS: [ToolKind; 14] = [
    ToolKind::Select,
    ToolKind::Arrow,
    ToolKind::Rectangle,
    ToolKind::Circle,
    ToolKind::RoundedRect,
    ToolKind::Line,
    ToolKind::Pencil,
    ToolKind::Highlight,
    ToolKind::Spotlight,
    ToolKind::Text,
    ToolKind::Pixelate,
    ToolKind::StepMarker,
    ToolKind::Eyedropper,
    ToolKind::Measurement,
];

impl App {
    fn new(config: Config) -> Self {
        screen_mark::theme::set_mode(config.theme_mode());
        let keymap = KeyMap::from_report(&config.key_report());
        let (jobs_tx, jobs_rx) = channel();
        let (tray_tx, tray_rx) = channel();
        let (hotkey_tx, hotkey_rx) = channel();
        // Hotkey presses arrive on the hotkey crate's own thread on Linux;
        // forward them and wake the event loop (on Windows they also wake it
        // as window messages, but one path for both is simpler). Set once
        // per process; `proxy` is filled in before the loop starts.
        let hotkey_wake: Arc<std::sync::OnceLock<EventLoopProxy<()>>> = Arc::default();
        {
            let wake = Arc::clone(&hotkey_wake);
            global_hotkey::GlobalHotKeyEvent::set_event_handler(Some(
                move |e: global_hotkey::GlobalHotKeyEvent| {
                    let _ = hotkey_tx.send(e);
                    if let Some(p) = wake.get() {
                        let _ = p.send_event(());
                    }
                },
            ));
        }
        Self {
            config,
            state: AppState::Idle,
            tray: None,
            tray_tx,
            tray_rx,
            hotkey_rx,
            hotkey_wake,
            overlay_window: None,
            surface: None,
            pixmap: None,
            modifiers: ModifiersState::empty(),
            settings_modifiers: ModifiersState::empty(),
            settings_blur_at: None,
            needs_redraw: false,
            last_render: Instant::now(),
            capture_at: std::collections::VecDeque::new(),
            hotkeys: GlobalHotkeys::new(),
            keymap,
            toast: None,
            jobs_tx,
            jobs_rx,
            jobs_pending: 0,
            proxy: None,
            pinned_windows: Vec::new(),
            immediate_capture: false,
            cli_only: false,
            startup_notified: false,
            countdown: None,
            window_capture_mode: false,
            window_rects: Vec::new(),
            settings_window: None,
            history_window: None,
        }
    }

    /// Register both global hotkeys from config. A hotkey that can't be
    /// registered is reported (popup, Settings shows why); it no longer
    /// silently falls back to a default.
    fn apply_hotkeys(&mut self, event_loop: &ActiveEventLoop, announce: bool) {
        let wanted = vec![
            (KeyId::Region, self.config.hotkey.region.clone()),
            (KeyId::Screenshot, self.config.hotkey.screenshot.clone()),
        ];
        self.hotkeys.apply(&wanted);
        let problems: Vec<(KeyId, String)> = wanted
            .iter()
            .filter_map(|(id, raw)| {
                self.hotkeys
                    .status(*id)
                    .describe(&screen_mark::settings_ui::display_binding(raw))
                    .map(|why| (*id, why))
            })
            .collect();
        if !problems.is_empty() {
            let title = match problems.as_slice() {
                [(KeyId::Region, _)] => "Region hotkey isn't working",
                [(_, _)] => "Screenshot hotkey isn't working",
                _ => "Hotkeys aren't working",
            };
            let detail = problems
                .iter()
                .map(|(_, why)| why.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            let spec = Spec::new(Kind::Error, title, detail)
                .hint("Click to choose another key")
                .click(Click::OpenKeySettings);
            self.feedback(event_loop, spec, None);
        } else if announce {
            let mut parts = Vec::new();
            for (id, raw) in &wanted {
                if self.hotkeys.status(*id) == Status::Active {
                    let what = if *id == KeyId::Region {
                        "region"
                    } else {
                        "save + copy"
                    };
                    parts.push(format!(
                        "{} {what}",
                        screen_mark::settings_ui::display_binding(raw)
                    ));
                }
            }
            let detail = if parts.is_empty() {
                "No hotkeys set; use the tray icon".to_string()
            } else {
                parts.join(" \u{00b7} ")
            };
            let spec = Spec::new(Kind::Info, "screen-mark is running", detail)
                .hint("Change keys in Settings > Keys");
            self.feedback(event_loop, spec, None);
        }
        self.update_tray_labels();
    }

    fn set_proxy(&mut self, proxy: EventLoopProxy<()>) {
        let _ = self.hotkey_wake.set(proxy.clone());
        self.proxy = Some(proxy);
    }

    fn update_tray_labels(&self) {
        if let Some(ref tray) = self.tray {
            tray.set_bindings(&self.config.hotkey.region, &self.config.hotkey.screenshot);
        }
    }

    /// Show a result: popup, system notification, or nothing, per the
    /// `notifications` setting. Errors always get a popup.
    fn feedback(&mut self, event_loop: &ActiveEventLoop, spec: Spec, monitor: Option<Monitor>) {
        let mode = self.config.notification_mode();
        let is_error = spec.kind == Kind::Error;
        if mode == screen_mark::config::NotificationMode::Off && !is_error {
            return;
        }
        if mode == screen_mark::config::NotificationMode::System && !is_error {
            let hint = spec
                .hint
                .strip_suffix(toast::CLICK_HINT)
                .unwrap_or(&spec.hint);
            let body = if hint.is_empty() {
                spec.detail.clone()
            } else {
                format!("{}\n{}", spec.detail, hint)
            };
            // Off the UI thread: on Windows this is a synchronous WinRT call.
            let title = spec.title;
            std::thread::spawn(move || screen_mark::notify::system(&title, &body));
            return;
        }
        let monitor = monitor.or_else(pointer_monitor);
        // Replace any popup already up.
        self.toast = None;
        self.toast = Toast::show(event_loop, spec, monitor);
        if self.toast.is_none() {
            tracing::warn!("Could not create the popup window");
        }
    }

    /// Run `job` on a worker thread; its popup is shown when it finishes.
    fn spawn_job(
        &mut self,
        monitor: Option<Monitor>,
        job: impl FnOnce() -> Option<Spec> + Send + 'static,
    ) {
        let tx = self.jobs_tx.clone();
        let proxy = self.proxy.clone();
        self.jobs_pending += 1;
        std::thread::spawn(move || {
            let spec = job();
            let _ = tx.send((spec, monitor));
            if let Some(p) = proxy {
                let _ = p.send_event(());
            }
        });
    }

    /// Start a capture, first clearing a popup that would end up in it.
    fn request_capture(&mut self, event_loop: &ActiveEventLoop, kind: CaptureKind) {
        self.request_capture_in(event_loop, kind, Duration::ZERO);
    }

    /// Start a capture after `delay` (or as soon as the screen is clear of
    /// screen-mark's own windows).
    fn request_capture_in(
        &mut self,
        event_loop: &ActiveEventLoop,
        kind: CaptureKind,
        delay: Duration,
    ) {
        // The overlay or a delayed-capture countdown is on screen: a second
        // capture now would either be lost behind it or contain it.
        if self.overlay_window.is_some() || self.countdown.is_some() {
            tracing::info!("A capture is already in progress, ignoring {kind:?}");
            return;
        }
        let toast_up = self.toast.take().is_some();
        if delay.is_zero() && !toast_up && self.capture_at.is_empty() {
            match kind {
                CaptureKind::Region => self.trigger_capture(event_loop),
                CaptureKind::Screenshot => self.do_screenshot(event_loop),
            }
            return;
        }
        // Queue behind captures already waiting, so a quick second press
        // is never lost, and give a popup time to leave the screen.
        let now = Instant::now();
        let mut when = now + delay;
        if toast_up {
            when = when.max(now + TOAST_CLEAR_DELAY);
        }
        if let Some((t, _)) = self.capture_at.back() {
            when = when.max(*t + Duration::from_millis(60));
        }
        self.capture_at.push_back((when, kind));
    }

    /// One press: capture the monitor under the mouse (or all monitors),
    /// save it to the save folder and copy it.
    fn do_screenshot(&mut self, event_loop: &ActiveEventLoop) {
        let shot = capture::create_capturer()
            .map_err(|e| e.to_string())
            .and_then(|c| c.capture_all_screens().map_err(|e| e.to_string()))
            .and_then(|s| s.into_iter().next().ok_or_else(|| "no screen".to_string()));
        let full = match shot {
            Ok(f) => f,
            Err(e) => {
                tracing::error!("Screenshot failed: {e}");
                let spec = Spec::new(Kind::Error, "Couldn't capture the screen", e);
                self.feedback(event_loop, spec, None);
                return;
            }
        };
        let monitor = pointer_monitor();
        let shot = if self.config.screenshot_all_monitors() {
            full
        } else {
            monitor
                .and_then(|m| display::crop(&full, m.area))
                .unwrap_or(full)
        };
        tracing::info!(
            "Screenshot {}x{} at ({}, {})",
            shot.width,
            shot.height,
            shot.x_offset,
            shot.y_offset
        );
        let folder = self.config.save_folder();
        let history = self.config.general.history_enabled;
        self.spawn_job(monitor, move || {
            Some(screen_mark::actions::finish_capture(
                &shot.pixels,
                shot.width,
                shot.height,
                Some(&folder),
                true,
                history,
            ))
        });
    }

    fn trigger_capture(&mut self, event_loop: &ActiveEventLoop) {
        if self.overlay_window.is_some() {
            tracing::info!("Overlay already open, ignoring capture trigger");
            return;
        }

        let capturer = match capture::create_capturer() {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("Failed to create capturer: {e}");
                return;
            }
        };

        let screens = match capturer.capture_all_screens() {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("Screen capture failed: {e}");
                return;
            }
        };

        if screens.is_empty() {
            tracing::error!("No screens captured");
            return;
        }

        // Use the first (primary) screen
        let screenshot = screens.into_iter().next().unwrap();
        tracing::info!(
            "Captured screen: {}x{} at ({}, {})",
            screenshot.width,
            screenshot.height,
            screenshot.x_offset,
            screenshot.y_offset
        );

        let attrs = WindowAttributes::default()
            .with_position(winit::dpi::PhysicalPosition::new(
                screenshot.x_offset,
                screenshot.y_offset,
            ))
            .with_inner_size(winit::dpi::PhysicalSize::new(
                screenshot.width,
                screenshot.height,
            ))
            .with_decorations(false)
            .with_visible(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_title("screen-mark");

        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                tracing::error!("Failed to create overlay window: {e}");
                return;
            }
        };

        let context = match softbuffer::Context::new(Arc::clone(&window)) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("Failed to create softbuffer context: {e}");
                return;
            }
        };

        let surface = match softbuffer::Surface::new(&context, Arc::clone(&window)) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("Failed to create softbuffer surface: {e}");
                return;
            }
        };

        let pixmap = tiny_skia::Pixmap::new(screenshot.width, screenshot.height);
        if pixmap.is_none() {
            tracing::error!(
                "Failed to create pixmap ({}x{})",
                screenshot.width,
                screenshot.height
            );
            return;
        }

        self.state = AppState::Capturing(Box::new(OverlayState::new(screenshot, &self.config)));
        self.surface = Some(surface);
        self.pixmap = pixmap;
        self.overlay_window = Some(window);
        self.needs_redraw = true;

        if let Some(w) = &self.overlay_window {
            w.request_redraw();
        }
    }

    fn open_settings(&mut self, event_loop: &ActiveEventLoop, tab: Option<Tab>) {
        if let Some(ref mut sw) = self.settings_window {
            if let Some(t) = tab {
                sw.view.open_tab(t);
            }
            sw.window.focus_window();
            sw.window.request_redraw();
            return;
        }

        let config = Config::load();

        // Load window icon from embedded PNG
        let win_icon = {
            let icon_bytes = include_bytes!("../assets/icon.png");
            let img = image::load_from_memory(icon_bytes)
                .ok()
                .map(|i| i.to_rgba8());
            img.and_then(|i| {
                let (w, h) = i.dimensions();
                winit::window::Icon::from_rgba(i.into_raw(), w, h).ok()
            })
        };

        let mut attrs = WindowAttributes::default()
            .with_title("screen-mark Settings")
            .with_inner_size(winit::dpi::LogicalSize::new(
                screen_mark::settings_ui::WIN_W,
                screen_mark::settings_ui::WIN_H,
            ))
            .with_resizable(false)
            .with_decorations(true);

        if let Some(icon) = win_icon {
            attrs = attrs.with_window_icon(Some(icon));
        }

        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                tracing::error!("Failed to create settings window: {e}");
                return;
            }
        };

        let context = match softbuffer::Context::new(Arc::clone(&window)) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("Failed to create settings context: {e}");
                return;
            }
        };

        let surface = match softbuffer::Surface::new(&context, Arc::clone(&window)) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("Failed to create settings surface: {e}");
                return;
            }
        };

        let statuses = [KeyId::Region, KeyId::Screenshot]
            .into_iter()
            .map(|id| (id, self.hotkeys.status(id)))
            .collect();
        let mut view = SettingsView::new(config, statuses);
        if let Some(t) = tab {
            view.open_tab(t);
        }
        let mut sw = SettingsWindow::new(window, surface, view);
        sw.render();
        tracing::info!("Settings window opened");
        self.settings_window = Some(sw);
    }

    fn close_settings(&mut self, event_loop: &ActiveEventLoop, saved: bool) {
        self.settings_window = None;
        if saved {
            self.config = Config::load();
            self.keymap = KeyMap::from_report(&self.config.key_report());
            screen_mark::theme::set_mode(self.config.theme_mode());
            if !self.cli_only {
                // Resume first (no-op if not suspended), then apply the new keys.
                self.hotkeys.resume();
                self.apply_hotkeys(event_loop, false);
            }
        } else {
            // Discard any live theme preview from the Settings toggle.
            screen_mark::theme::set_mode(self.config.theme_mode());
            self.hotkeys.resume();
        }
        // Settings can toggle autostart — keep the tray checkbox in sync.
        if let Some(ref tray) = self.tray {
            tray.set_autostart(screen_mark::autostart::is_enabled());
        }
        tracing::info!("Settings window closed (saved: {saved})");
    }

    fn open_history(&mut self, event_loop: &ActiveEventLoop) {
        if self.history_window.is_some() {
            tracing::info!("History window already open");
            return;
        }

        // Load window icon from embedded PNG
        let win_icon = {
            let icon_bytes = include_bytes!("../assets/icon.png");
            let img = image::load_from_memory(icon_bytes)
                .ok()
                .map(|i| i.to_rgba8());
            img.and_then(|i| {
                let (w, h) = i.dimensions();
                winit::window::Icon::from_rgba(i.into_raw(), w, h).ok()
            })
        };

        let mut attrs = WindowAttributes::default()
            .with_title("screen-mark \u{2014} History")
            .with_inner_size(winit::dpi::LogicalSize::new(
                screen_mark::history_ui::WIN_W,
                screen_mark::history_ui::WIN_H,
            ))
            .with_resizable(false)
            .with_decorations(true);

        if let Some(icon) = win_icon {
            attrs = attrs.with_window_icon(Some(icon));
        }

        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                tracing::error!("Failed to create history window: {e}");
                return;
            }
        };

        let context = match softbuffer::Context::new(Arc::clone(&window)) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("Failed to create history context: {e}");
                return;
            }
        };

        let surface = match softbuffer::Surface::new(&context, Arc::clone(&window)) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("Failed to create history surface: {e}");
                return;
            }
        };

        let mut hw = HistoryWindow::new(window, surface);
        hw.render();
        tracing::info!("History window opened");
        self.history_window = Some(hw);
    }

    fn close_history(&mut self) {
        self.history_window = None;
        tracing::info!("History window closed");
    }

    fn close_overlay(&mut self) {
        self.surface = None;
        self.pixmap = None;
        self.overlay_window = None;
        self.state = AppState::Idle;
        self.modifiers = ModifiersState::empty();
        self.window_capture_mode = false;
        self.window_rects.clear();
        tracing::info!("Overlay closed");
    }

    fn do_copy(&mut self) {
        let copy_data = if let AppState::Capturing(ref overlay) = self.state {
            overlay.flattened_selection()
        } else {
            None
        };
        if let Some((pixels, w, h)) = copy_data {
            let history = self.config.general.history_enabled;
            let save_to = self
                .config
                .general
                .copy_also_saves
                .then(|| self.config.save_folder());
            self.close_overlay();
            self.spawn_job(None, move || {
                Some(screen_mark::actions::finish_capture(
                    &pixels,
                    w,
                    h,
                    save_to.as_deref(),
                    true,
                    history,
                ))
            });
        }
    }

    fn do_save(&mut self) {
        let save_data = if let AppState::Capturing(ref overlay) = self.state {
            overlay.flattened_selection()
        } else {
            None
        };
        let Some((pixels, w, h)) = save_data else {
            return;
        };
        let history = self.config.general.history_enabled;
        let ask = self.config.ask_where_to_save();
        let folder = self.config.save_folder();
        self.close_overlay();
        if !ask {
            self.spawn_job(None, move || {
                Some(screen_mark::actions::finish_capture(
                    &pixels,
                    w,
                    h,
                    Some(&folder),
                    false,
                    history,
                ))
            });
            return;
        }
        self.spawn_job(None, move || {
            let thumb = toast::make_thumb(&pixels, w, h, 192, 120);
            match export::save_to_file(pixels, w, h, None) {
                Ok(Some(path)) => {
                    let path = std::path::PathBuf::from(path);
                    if history {
                        let _ = screen_mark::history::save_to_history_from_file(&path);
                    }
                    Some(screen_mark::actions::describe(Some(Ok(path)), None, w, h).thumb(thumb))
                }
                Ok(None) => None, // dialog cancelled
                Err(e) => Some(screen_mark::actions::describe(Some(Err(e)), None, w, h)),
            }
        });
    }

    fn do_upload(&mut self) {
        // Check if Imgur is configured before entering the confirmation flow
        let has_client_id = !self.config.general.imgur_client_id.is_empty()
            || std::env::var("SCREEN_MARK_IMGUR_CLIENT_ID")
                .map(|v| !v.is_empty())
                .unwrap_or(false);

        if !has_client_id {
            if let AppState::Capturing(ref mut o) = self.state {
                o.show_toast(
                    "Set imgur_client_id in config.toml to enable uploads".into(),
                    4000,
                );
            }
            self.needs_redraw = true;
            return;
        }

        // First click: show confirmation toast
        // Second click: actually upload
        let confirmed = if let AppState::Capturing(ref overlay) = self.state {
            overlay.upload_confirmed
        } else {
            false
        };

        if !confirmed {
            // First click — ask for confirmation
            if let AppState::Capturing(ref mut o) = self.state {
                o.upload_confirmed = true;
                o.show_toast("Click Upload again to share to Imgur (public)".into(), 4000);
            }
            self.needs_redraw = true;
            return;
        }

        // Second click — confirmed, proceed with upload
        let upload_data = if let AppState::Capturing(ref mut o) = self.state {
            o.upload_confirmed = false; // reset
            o.flattened_selection()
        } else {
            None
        };

        if let Some((pixels, w, h)) = upload_data {
            if self.config.general.history_enabled {
                let _ = screen_mark::history::save_to_history(&pixels, w, h);
            }

            // Close overlay immediately; PNG encoding and the upload both run
            // on a background thread so the UI never stalls on large captures.
            let imgur_id = self.config.general.imgur_client_id.clone();
            self.close_overlay();

            self.spawn_job(None, move || {
                let png_bytes = match export::encode_png(&pixels, w, h) {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        tracing::error!("PNG encode failed: {e}");
                        return Some(Spec::new(Kind::Error, "Upload failed", e));
                    }
                };
                Some(
                    match screen_mark::upload::upload_to_imgur(&png_bytes, &imgur_id) {
                        Ok(url) => {
                            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                let _ = clipboard.set_text(&url);
                            }
                            tracing::info!("Uploaded to Imgur: {}", url);
                            Spec::new(Kind::Success, "Uploaded; link copied", url)
                        }
                        Err(e) => {
                            tracing::error!("Imgur upload failed: {}", e);
                            Spec::new(Kind::Error, "Upload failed", e)
                        }
                    },
                )
            });
        }
    }

    fn do_ocr(&mut self) {
        // OCR reads the raw screenshot pixels (annotations would confuse it)
        let ocr_data = if let AppState::Capturing(ref overlay) = self.state {
            overlay.raw_selection()
        } else {
            None
        };

        if let Some((cropped, w, h)) = ocr_data {
            self.close_overlay();
            self.spawn_job(None, move || {
                Some(match screen_mark::ocr::extract_text(&cropped, w, h) {
                    Ok(text) => {
                        if let Ok(mut clipboard) = arboard::Clipboard::new() {
                            let _ = clipboard.set_text(&text);
                        }
                        let char_count = text.chars().count();
                        tracing::info!("OCR extracted {} chars", char_count);
                        let preview: String = text.chars().take(80).collect();
                        Spec::new(
                            Kind::Success,
                            format!("Copied {char_count} characters of text"),
                            preview.replace(['\r', '\n'], " "),
                        )
                    }
                    Err(e) => {
                        tracing::error!("OCR failed: {e}");
                        Spec::new(Kind::Error, "Couldn't read text", e)
                    }
                })
            });
        }
    }

    fn do_pin(&mut self, event_loop: &ActiveEventLoop) {
        if let AppState::Capturing(ref overlay) = self.state {
            if let Some((pixels, w, h)) = overlay.flattened_selection() {
                if self.config.general.history_enabled {
                    let _ = screen_mark::history::save_to_history(&pixels, w, h);
                }

                // Position the pin where the selection is on screen, accounting
                // for the virtual-desktop offset of the captured area (the
                // selection is in screenshot coordinates, which can start at a
                // negative virtual-desktop position on multi-monitor setups).
                if let Some(ref sel) = overlay.selection {
                    let screen_x = sel.x as i32 + overlay.screenshot.x_offset;
                    let screen_y = sel.y as i32 + overlay.screenshot.y_offset;
                    if let Some(pin) =
                        PinnedWindow::create(event_loop, &pixels, w, h, screen_x, screen_y)
                    {
                        self.pinned_windows.push(pin);
                    }
                }
            }
        }
        self.close_overlay();
    }

    fn process_tray_events(&mut self, event_loop: &ActiveEventLoop) {
        while let Ok(event) = self.tray_rx.try_recv() {
            let item = match event {
                TrayEvent::IconClicked => {
                    tracing::info!("Tray icon clicked — triggering capture");
                    self.request_capture(event_loop, CaptureKind::Region);
                    continue;
                }
                TrayEvent::Menu(item) => item,
            };
            use screen_mark::tray::Item;
            // Let the menu fade out first, or it ends up in the capture.
            let menu_gone = TRAY_MENU_CLOSE_DELAY;
            match item {
                Item::Region => self.request_capture_in(event_loop, CaptureKind::Region, menu_gone),
                Item::Screenshot => {
                    self.request_capture_in(event_loop, CaptureKind::Screenshot, menu_gone)
                }
                Item::Window => {
                    self.window_capture_mode = true;
                    self.window_rects = window_detect::enumerate_window_rects();
                    tracing::info!("Enumerated {} windows", self.window_rects.len());
                    self.request_capture_in(event_loop, CaptureKind::Region, menu_gone);
                }
                Item::Delay(secs) => {
                    tracing::info!("Capturing in {secs} seconds...");
                    self.toast = None;
                    // The countdown reaching 0 schedules the capture.
                    self.countdown = Countdown::start(event_loop, secs);
                }
                Item::OpenFolder => {
                    let folder = self.config.save_folder();
                    if let Err(e) = std::fs::create_dir_all(&folder) {
                        let spec =
                            Spec::new(Kind::Error, "Can't open the save folder", e.to_string());
                        self.feedback(event_loop, spec, None);
                    } else {
                        toast::show_in_folder(&folder);
                    }
                }
                Item::History => self.open_history(event_loop),
                Item::Autostart => {
                    let new_state = !screen_mark::autostart::is_enabled();
                    match screen_mark::autostart::set_enabled(new_state) {
                        Ok(()) => {
                            if let Some(ref tray) = self.tray {
                                tray.set_autostart(new_state);
                            }
                        }
                        Err(e) => tracing::error!("Auto-start toggle failed: {}", e),
                    }
                }
                Item::Settings => self.open_settings(event_loop, None),
                Item::About => {
                    let version = env!("CARGO_PKG_VERSION");
                    let spec = Spec::new(
                        Kind::Info,
                        format!("screen-mark {version}"),
                        "github.com/Real-Fruit-Snacks/screen-mark",
                    );
                    self.feedback(event_loop, spec, None);
                    open_url("https://github.com/Real-Fruit-Snacks/screen-mark");
                }
                Item::Quit => {
                    tracing::info!("Quit requested");
                    event_loop.exit();
                }
            }
        }
    }

    fn render(&mut self) {
        if !self.needs_redraw {
            return;
        }

        let overlay = match &mut self.state {
            AppState::Capturing(o) => o,
            AppState::Idle => return,
        };

        let pixmap = match self.pixmap.as_mut() {
            Some(p) => p,
            None => return,
        };

        let surface = match self.surface.as_mut() {
            Some(s) => s,
            None => return,
        };

        let window = match self.overlay_window.as_ref() {
            Some(w) => w,
            None => return,
        };

        render_overlay(overlay, pixmap);

        let size = window.inner_size();
        let w = size.width;
        let h = size.height;

        if w == 0 || h == 0 {
            return;
        }

        if let (Some(nz_w), Some(nz_h)) = (NonZeroU32::new(w), NonZeroU32::new(h)) {
            if let Err(e) = surface.resize(nz_w, nz_h) {
                tracing::error!("Surface resize failed: {e}");
                return;
            }
        }

        let mut buffer = match surface.buffer_mut() {
            Ok(b) => b,
            Err(e) => {
                tracing::error!("Failed to get surface buffer: {e}");
                return;
            }
        };

        // Fast pixmap → softbuffer copy using raw bytes.
        // Screenshot pixels are fully opaque (alpha=255), and tiny-skia composites
        // onto an opaque background, so all pixels remain opaque after rendering.
        // For opaque pixels, premultiplied == straight — skip demultiply entirely.
        // Pixel format: tiny-skia RGBA bytes → softbuffer 0x00RRGGBB u32
        let src_data = pixmap.data(); // &[u8], RGBA order
        let pixel_count = (pixmap.width() * pixmap.height()) as usize;
        let buf_len = buffer.len();
        let copy_count = pixel_count.min(buf_len);

        for (i, chunk) in src_data.chunks_exact(4).take(copy_count).enumerate() {
            buffer[i] = ((chunk[0] as u32) << 16) | ((chunk[1] as u32) << 8) | (chunk[2] as u32);
        }

        if let Err(e) = buffer.present() {
            tracing::error!("Buffer present failed: {e}");
        }

        // Show window after first frame is rendered (avoids white flash)
        if let Some(ref window) = self.overlay_window {
            if !window.is_visible().unwrap_or(true) {
                window.set_visible(true);
            }
        }

        self.needs_redraw = false;
    }
}

/// Determine the appropriate cursor icon based on the current overlay state and mouse position.
fn determine_cursor(overlay: &OverlayState, pos: Point) -> CursorIcon {
    // Text input active — always show text cursor
    if overlay.text_input_active {
        return CursorIcon::Text;
    }

    // Check toolbar hover (only if selection exists)
    if let Some(ref sel) = overlay.selection {
        let visible_count = overlay.visible_buttons.len();
        let toolbar =
            Toolbar::position_for_dynamic(sel, overlay.screenshot.height as f32, visible_count);
        if toolbar.hit_test_dynamic(pos, visible_count).is_some() {
            return CursorIcon::Pointer;
        }
    }

    // Currently dragging a resize handle on an annotation
    if let Some(handle) = overlay.resize_handle {
        return match handle {
            ResizeHandle::TopLeft | ResizeHandle::BottomRight => CursorIcon::NwseResize,
            ResizeHandle::TopRight | ResizeHandle::BottomLeft => CursorIcon::NeswResize,
        };
    }

    // Hovering over a resize handle on a selected annotation
    if overlay.active_tool == ToolKind::Select {
        if let Some(idx) = overlay.selected_index {
            if let Some(ann) = overlay.annotations.get(idx) {
                if let Some((bx, by, bw, bh)) = annotation_bounding_box(ann) {
                    let handles = [
                        (Point::new(bx, by), ResizeHandle::TopLeft),
                        (Point::new(bx + bw, by), ResizeHandle::TopRight),
                        (Point::new(bx, by + bh), ResizeHandle::BottomLeft),
                        (Point::new(bx + bw, by + bh), ResizeHandle::BottomRight),
                    ];
                    for (hp, handle) in &handles {
                        if (pos.x - hp.x).abs() < 8.0 && (pos.y - hp.y).abs() < 8.0 {
                            return match handle {
                                ResizeHandle::TopLeft | ResizeHandle::BottomRight => {
                                    CursorIcon::NwseResize
                                }
                                ResizeHandle::TopRight | ResizeHandle::BottomLeft => {
                                    CursorIcon::NeswResize
                                }
                            };
                        }
                    }
                }
            }
        }
    }

    // Currently dragging to create selection
    if overlay.is_selecting {
        return CursorIcon::Crosshair;
    }

    // Currently dragging a resize/move zone
    if let Some(zone) = overlay.drag_zone {
        return hitzone_to_cursor(zone, overlay);
    }

    // Selection exists — hit-test it
    if let Some(ref sel) = overlay.selection {
        if let Some(zone) = sel.hit_test(pos, 8.0) {
            return hitzone_to_cursor(zone, overlay);
        }
    }

    // No selection yet (idle) — crosshair
    CursorIcon::Crosshair
}

/// Map a HitZone to the appropriate cursor icon.
fn hitzone_to_cursor(zone: HitZone, overlay: &OverlayState) -> CursorIcon {
    match zone {
        HitZone::TopLeft | HitZone::BottomRight => CursorIcon::NwseResize,
        HitZone::TopRight | HitZone::BottomLeft => CursorIcon::NeswResize,
        HitZone::Top | HitZone::Bottom => CursorIcon::NsResize,
        HitZone::Left | HitZone::Right => CursorIcon::EwResize,
        HitZone::Inside => {
            // Inside selection — cursor depends on active tool
            match overlay.active_tool {
                ToolKind::Select => CursorIcon::Default,
                ToolKind::Text => CursorIcon::Text,
                ToolKind::StepMarker => CursorIcon::Cell,
                ToolKind::Eyedropper => CursorIcon::Crosshair,
                ToolKind::Arrow | ToolKind::Line | ToolKind::Pencil | ToolKind::Measurement => {
                    CursorIcon::Crosshair
                }
                ToolKind::Rectangle
                | ToolKind::RoundedRect
                | ToolKind::Circle
                | ToolKind::Highlight
                | ToolKind::Pixelate
                | ToolKind::Spotlight => CursorIcon::Crosshair,
            }
        }
    }
}

/// The monitor under the mouse pointer, if both can be read.
fn pointer_monitor() -> Option<Monitor> {
    let (x, y) = display::pointer()?;
    display::monitor_at(&display::monitors(), x, y)
}

fn open_url(url: &str) {
    #[cfg(target_os = "windows")]
    {
        // CREATE_NO_WINDOW — cmd.exe would otherwise flash a console
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        tracing::info!("Application resumed");
        event_loop.set_control_flow(ControlFlow::Wait);

        if !self.cli_only && !self.startup_notified {
            self.startup_notified = true;
            if self.tray.is_none() {
                let proxy = self.proxy.clone();
                let wake = move || {
                    if let Some(ref p) = proxy {
                        let _ = p.send_event(());
                    }
                };
                match tray::start(self.tray_tx.clone(), wake) {
                    Ok(t) => {
                        tracing::info!("Tray icon created");
                        self.tray = Some(t);
                    }
                    Err(e) => {
                        tracing::error!("Failed to create tray icon: {e}");
                    }
                }
            }
            // Register hotkeys and say which ones are live (or which failed).
            self.apply_hotkeys(event_loop, true);
        }

        if self.immediate_capture {
            self.immediate_capture = false;
            self.trigger_capture(event_loop);
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: ()) {
        // A worker finished; about_to_wait drains the results.
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        // Check if event is for a pinned window
        if let Some(pin_idx) = self
            .pinned_windows
            .iter()
            .position(|p| p.window_id() == _window_id)
        {
            if self.pinned_windows[pin_idx].handle_event(&event) {
                // PinnedWindow::drop removes the backing temp file.
                self.pinned_windows.remove(pin_idx);
                tracing::info!("Pinned window closed");
            }
            return;
        }

        // Check if event is for the history window
        if let Some(ref hw) = self.history_window {
            if hw.window.id() == _window_id {
                match event {
                    WindowEvent::CloseRequested => {
                        self.close_history();
                        return;
                    }
                    WindowEvent::RedrawRequested => {
                        if let Some(ref mut hw) = self.history_window {
                            hw.render();
                        }
                        return;
                    }
                    WindowEvent::CursorMoved { position, .. } => {
                        if let Some(ref mut hw) = self.history_window {
                            if hw.on_cursor_moved(position.x as f32, position.y as f32) {
                                hw.needs_redraw = true;
                                hw.window.request_redraw();
                            }
                        }
                        return;
                    }
                    WindowEvent::MouseInput {
                        state: ElementState::Pressed,
                        button: WinitMouseButton::Left,
                        ..
                    } => {
                        if let Some(ref mut hw) = self.history_window {
                            if hw.on_click() {
                                hw.render();
                            }
                        }
                        return;
                    }
                    WindowEvent::MouseWheel { delta, .. } => {
                        let scroll = match delta {
                            MouseScrollDelta::LineDelta(_, y) => y,
                            MouseScrollDelta::PixelDelta(pos) => pos.y as f32 / 20.0,
                        };
                        if let Some(ref mut hw) = self.history_window {
                            if hw.on_scroll(scroll) {
                                hw.window.request_redraw();
                            }
                        }
                        return;
                    }
                    WindowEvent::KeyboardInput { ref event, .. }
                        if event.state == ElementState::Pressed =>
                    {
                        if let Key::Named(NamedKey::Escape) = &event.logical_key {
                            self.close_history();
                            return;
                        }
                    }
                    _ => {}
                }
                return;
            }
        }

        // Check if event is for the popup
        if let Some(ref mut t) = self.toast {
            if t.window_id() == _window_id {
                match event {
                    WindowEvent::RedrawRequested | WindowEvent::Resized(_) => t.render(),
                    WindowEvent::ScaleFactorChanged {
                        mut inner_size_writer,
                        ..
                    } => t.keep_place(&mut inner_size_writer),
                    WindowEvent::CursorMoved { .. } => t.hold(),
                    WindowEvent::MouseInput {
                        state: ElementState::Released,
                        button,
                        ..
                    } => {
                        let click = t.spec.click.clone();
                        self.toast = None;
                        if button == WinitMouseButton::Left {
                            match click {
                                Click::ShowFile(path) => toast::show_in_folder(&path),
                                Click::OpenKeySettings => {
                                    self.open_settings(_event_loop, Some(Tab::Keys))
                                }
                                Click::Nothing => {}
                            }
                        }
                    }
                    WindowEvent::CloseRequested => self.toast = None,
                    _ => {}
                }
                return;
            }
        }

        // Check if event is for the settings window
        if let Some(ref sw) = self.settings_window {
            if sw.window.id() == _window_id {
                let outcome = match event {
                    WindowEvent::CloseRequested => Outcome::Close { saved: false },
                    WindowEvent::RedrawRequested => {
                        if let Some(ref mut sw) = self.settings_window {
                            sw.render();
                        }
                        Outcome::Nothing
                    }
                    WindowEvent::CursorMoved { position, .. } => {
                        let moved = self.settings_window.as_mut().is_some_and(|sw| {
                            sw.on_cursor_moved(position.x as f32, position.y as f32)
                        });
                        if moved {
                            Outcome::Redraw
                        } else {
                            Outcome::Nothing
                        }
                    }
                    WindowEvent::MouseInput {
                        state: ElementState::Pressed,
                        button: WinitMouseButton::Left,
                        ..
                    } => match self.settings_window {
                        Some(ref mut sw) => sw.on_click(),
                        None => Outcome::Nothing,
                    },
                    WindowEvent::MouseWheel { delta, .. } => {
                        let lines = match delta {
                            MouseScrollDelta::LineDelta(_, y) => y,
                            MouseScrollDelta::PixelDelta(pos) => pos.y as f32 / 26.0,
                        };
                        let scrolled = self
                            .settings_window
                            .as_mut()
                            .is_some_and(|sw| sw.view.scroll_by(lines));
                        if scrolled {
                            Outcome::Redraw
                        } else {
                            Outcome::Nothing
                        }
                    }
                    WindowEvent::ModifiersChanged(mods) => {
                        self.settings_modifiers = mods.state();
                        let press = KeyPress {
                            mods: keys::mods_from_winit(self.settings_modifiers),
                            key: None,
                            pressed: true,
                        };
                        match self.settings_window {
                            Some(ref mut sw) => sw.view.key(press),
                            None => Outcome::Nothing,
                        }
                    }
                    WindowEvent::Focused(false) => {
                        // Switched away while listening: the keys now go to
                        // another app while the global hotkeys stay
                        // suspended, so listening stops (in about_to_wait,
                        // after a grace period: on X11, pressing a combo
                        // another app has grabbed also blurs the window
                        // for as long as it's held).
                        self.settings_modifiers = ModifiersState::empty();
                        self.settings_blur_at = Some(Instant::now());
                        Outcome::Nothing
                    }
                    WindowEvent::Focused(true) => {
                        self.settings_blur_at = None;
                        Outcome::Nothing
                    }
                    WindowEvent::KeyboardInput { ref event, .. } => {
                        let key = if keys::is_modifier_key(&event.logical_key) {
                            None
                        } else {
                            keys::key_from_winit(&event.logical_key, &event.physical_key)
                        };
                        // Skip auto-repeat and keys that can't be bound (media, IME, ...).
                        let unbindable =
                            key.is_none() && !keys::is_modifier_key(&event.logical_key);
                        if unbindable || event.repeat {
                            Outcome::Nothing
                        } else {
                            let press = KeyPress {
                                mods: keys::mods_from_winit(self.settings_modifiers),
                                key,
                                pressed: event.state == ElementState::Pressed,
                            };
                            match self.settings_window {
                                Some(ref mut sw) => sw.view.key(press),
                                None => Outcome::Nothing,
                            }
                        }
                    }
                    _ => Outcome::Nothing,
                };
                match outcome {
                    Outcome::Close { saved } => self.close_settings(_event_loop, saved),
                    Outcome::Redraw => {
                        if let Some(ref sw) = self.settings_window {
                            sw.window.request_redraw();
                        }
                    }
                    Outcome::Nothing => {}
                }
                return;
            }
        }

        // Check if event is for the countdown window
        if let Some(ref mut cd) = self.countdown {
            if cd.window_id() == _window_id {
                match event {
                    WindowEvent::CloseRequested => {
                        self.countdown = None;
                        self.capture_at.clear(); // cancel the delayed capture
                        return;
                    }
                    WindowEvent::RedrawRequested => {
                        cd.render();
                        return;
                    }
                    _ => {}
                }
                return;
            }
        }

        // Check if event is for the overlay window
        if let Some(ref overlay_win) = self.overlay_window {
            if overlay_win.id() != _window_id {
                return; // Unknown window, ignore
            }
        }

        // Only process events when we're capturing
        let overlay = match &mut self.state {
            AppState::Capturing(o) => o,
            AppState::Idle => return,
        };

        match event {
            WindowEvent::CloseRequested => {
                self.close_overlay();
                return;
            }

            WindowEvent::RedrawRequested => {
                self.needs_redraw = true;
                self.render();
                return;
            }

            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = mods.state();
            }

            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }

                // --- Text input guard: MUST be first ---
                if overlay.text_input_active {
                    let ctrl = self.modifiers.control_key();
                    match &event.logical_key {
                        Key::Character(ch) if ctrl && ch.as_str().eq_ignore_ascii_case("v") => {
                            // Ctrl+V: paste clipboard text into the buffer
                            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                if let Ok(text) = clipboard.get_text() {
                                    // Single-line annotation — fold newlines
                                    let text = text.replace(['\r', '\n'], " ");
                                    overlay.text_input_buffer.push_str(&text);
                                    self.needs_redraw = true;
                                }
                            }
                        }
                        Key::Character(_) if ctrl => {
                            // Swallow other Ctrl chords so they don't insert
                            // literal characters while typing.
                        }
                        Key::Character(ch) => {
                            overlay.text_input_buffer.push_str(ch.as_str());
                            self.needs_redraw = true;
                        }
                        Key::Named(NamedKey::Space) => {
                            overlay.text_input_buffer.push(' ');
                            self.needs_redraw = true;
                        }
                        Key::Named(NamedKey::Backspace) => {
                            overlay.text_input_buffer.pop();
                            self.needs_redraw = true;
                        }
                        Key::Named(NamedKey::Enter) => {
                            let buffer = overlay.text_input_buffer.clone();
                            if let Some((idx, old_ann)) = overlay.text_edit_origin.take() {
                                // Re-edit of an existing annotation: one Modify
                                // step (or a Delete when the text was cleared).
                                let idx = idx.min(overlay.annotations.len());
                                if !buffer.is_empty() {
                                    let ann = Annotation::Text {
                                        position: overlay.text_input_position,
                                        text: buffer,
                                        color: overlay.current_color,
                                        font_size: overlay.text_input_font_size,
                                    };
                                    overlay.annotations.insert(idx, ann);
                                    record_undo(
                                        &mut overlay.undo_stack,
                                        &mut overlay.redo_stack,
                                        UndoAction::Modify(idx, old_ann),
                                    );
                                } else {
                                    record_undo(
                                        &mut overlay.undo_stack,
                                        &mut overlay.redo_stack,
                                        UndoAction::Delete(idx, old_ann),
                                    );
                                }
                            } else if !buffer.is_empty() {
                                let ann = Annotation::Text {
                                    position: overlay.text_input_position,
                                    text: buffer,
                                    color: overlay.current_color,
                                    font_size: overlay.text_input_font_size,
                                };
                                overlay.annotations.push(ann);
                                record_undo(
                                    &mut overlay.undo_stack,
                                    &mut overlay.redo_stack,
                                    UndoAction::Add(overlay.annotations.len() - 1),
                                );
                            }
                            overlay.text_input_buffer.clear();
                            overlay.text_input_active = false;
                            self.needs_redraw = true;
                        }
                        Key::Named(NamedKey::Escape) => {
                            // Cancel text input; restore the original annotation
                            // if this was a re-edit.
                            if let Some((idx, old_ann)) = overlay.text_edit_origin.take() {
                                let idx = idx.min(overlay.annotations.len());
                                overlay.annotations.insert(idx, old_ann);
                            }
                            overlay.text_input_buffer.clear();
                            overlay.text_input_active = false;
                            self.needs_redraw = true;
                        }
                        _ => {
                            // Ignore all other keys while typing
                        }
                    }
                    // Return early — don't fall through to other handlers
                    if self.needs_redraw {
                        if let Some(w) = &self.overlay_window {
                            w.request_redraw();
                        }
                    }
                    return;
                }

                // The rebindable key map comes first, so a bound Ctrl+Enter or
                // Ctrl+Delete runs its action; unbound Esc / Delete / Enter
                // keep their fixed meaning below.
                let has_selection = overlay.selection.is_some();
                let mods = keys::mods_from_winit(self.modifiers);
                let key = keys::key_from_winit(&event.logical_key, &event.physical_key);
                let action = key.and_then(|k| self.keymap.editor_action(mods, k));
                let tool = key.and_then(|k| self.keymap.tool(mods, k)).or_else(|| {
                    // 2.0-style tool keys bound to a typed character.
                    keys::typed_char(&event.logical_key)
                        .and_then(|c| self.keymap.tool(mods, keys::Key::Typed(c)))
                });
                if let Some(action) = action {
                    match action {
                        KeyId::Copy => {
                            self.do_copy();
                            return;
                        }
                        KeyId::Save => {
                            self.do_save();
                            return;
                        }
                        KeyId::Pin => {
                            self.do_pin(_event_loop);
                            return;
                        }
                        KeyId::Ocr => {
                            self.do_ocr();
                            return;
                        }
                        KeyId::Upload => {
                            self.do_upload();
                            return;
                        }
                        KeyId::Undo | KeyId::Redo => {
                            let overlay = match &mut self.state {
                                AppState::Capturing(o) => o,
                                _ => return,
                            };
                            let changed = if action == KeyId::Undo {
                                apply_undo(
                                    &mut overlay.annotations,
                                    &mut overlay.undo_stack,
                                    &mut overlay.redo_stack,
                                )
                            } else {
                                apply_redo(
                                    &mut overlay.annotations,
                                    &mut overlay.undo_stack,
                                    &mut overlay.redo_stack,
                                )
                            };
                            if changed {
                                overlay.selected_index = None;
                                overlay.sync_step_counter();
                                self.needs_redraw = true;
                            }
                        }
                        KeyId::SelectAll => {
                            let overlay = match &mut self.state {
                                AppState::Capturing(o) => o,
                                _ => return,
                            };
                            overlay.selection = Some(Selection {
                                x: 0.0,
                                y: 0.0,
                                width: overlay.screenshot.width as f32,
                                height: overlay.screenshot.height as f32,
                            });
                            self.needs_redraw = true;
                        }
                        _ => {}
                    }
                } else if let Some(i) = tool {
                    if let Some(tool) = TOOL_KINDS.get(i).copied() {
                        let overlay = match &mut self.state {
                            AppState::Capturing(o) => o,
                            _ => return,
                        };
                        overlay.active_tool = tool;
                        overlay.selected_index = None;
                        self.needs_redraw = true;
                    }
                } else {
                    match &event.logical_key {
                        Key::Named(NamedKey::Escape) => {
                            let overlay = match &mut self.state {
                                AppState::Capturing(o) => o,
                                _ => return,
                            };
                            if overlay.selected_index.is_some() {
                                overlay.selected_index = None;
                                overlay.select_drag_start = None;
                                self.needs_redraw = true;
                            } else {
                                self.close_overlay();
                                return;
                            }
                        }
                        Key::Named(NamedKey::Delete) | Key::Named(NamedKey::Backspace) => {
                            let overlay = match &mut self.state {
                                AppState::Capturing(o) => o,
                                _ => return,
                            };
                            if let Some(idx) = overlay.selected_index {
                                if idx < overlay.annotations.len() {
                                    let removed = overlay.annotations.remove(idx);
                                    record_undo(
                                        &mut overlay.undo_stack,
                                        &mut overlay.redo_stack,
                                        UndoAction::Delete(idx, removed),
                                    );
                                    overlay.selected_index = None;
                                    overlay.sync_step_counter();
                                    self.needs_redraw = true;
                                }
                            }
                        }
                        Key::Named(NamedKey::Enter) if has_selection => {
                            // Quick copy — same path as Ctrl+C / the Copy button,
                            // annotations included, clipboard work off-thread.
                            self.do_copy();
                            return;
                        }
                        _ => {}
                    }
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                let pos = Point::new(position.x as f32, position.y as f32);
                let prev = overlay.last_mouse_pos;
                overlay.last_mouse_pos = pos;
                let dx = pos.x - prev.x;
                let dy = pos.y - prev.y;

                // Window capture mode: highlight the window under the cursor
                if self.window_capture_mode {
                    let x_off = overlay.screenshot.x_offset;
                    let y_off = overlay.screenshot.y_offset;
                    let screen_x = pos.x as i32 + x_off;
                    let screen_y = pos.y as i32 + y_off;

                    if let Some((wx, wy, ww, wh)) =
                        window_detect::window_at_point(&self.window_rects, screen_x, screen_y)
                    {
                        let sel_x = (wx - x_off) as f32;
                        let sel_y = (wy - y_off) as f32;
                        overlay.selection = Some(Selection {
                            x: sel_x,
                            y: sel_y,
                            width: ww as f32,
                            height: wh as f32,
                        });
                    } else {
                        overlay.selection = None;
                    }
                    self.needs_redraw = true;

                    // Update cursor
                    if let Some(ref window) = self.overlay_window {
                        window.set_cursor(CursorIcon::Crosshair);
                    }
                    return;
                }

                if overlay.is_selecting {
                    // Update selection while dragging
                    if let Some(start) = overlay.drag_start {
                        overlay.selection = Some(Selection::from_points(start, pos));
                        self.needs_redraw = true;
                    }
                } else if let Some(zone) = overlay.drag_zone {
                    // Resizing or moving selection
                    if let Some(ref mut sel) = overlay.selection {
                        if zone == HitZone::Inside {
                            sel.move_by(dx, dy);
                        } else {
                            sel.resize(zone, dx, dy);
                        }
                        self.needs_redraw = true;
                    }
                } else {
                    // Forward to active tool if drawing
                    match overlay.active_tool {
                        ToolKind::Select => {
                            // Resize drag takes priority
                            if let (Some(idx), Some(handle)) =
                                (overlay.selected_index, overlay.resize_handle)
                            {
                                if let Some(ann) = overlay.annotations.get_mut(idx) {
                                    resize_annotation(ann, handle, pos);
                                }
                                self.needs_redraw = true;
                            } else if let (Some(idx), Some(drag_start)) =
                                (overlay.selected_index, overlay.select_drag_start)
                            {
                                let dx = pos.x - drag_start.x;
                                let dy = pos.y - drag_start.y;
                                if let Some(ann) = overlay.annotations.get_mut(idx) {
                                    move_annotation(ann, dx, dy);
                                }
                                overlay.select_drag_start = Some(pos);
                                self.needs_redraw = true;
                            }
                        }
                        ToolKind::Arrow => {
                            if overlay.arrow_tool.is_drawing() {
                                overlay.arrow_tool.on_mouse_move(pos);
                                self.needs_redraw = true;
                            }
                        }
                        ToolKind::Rectangle => {
                            if overlay.rectangle_tool.is_drawing() {
                                overlay.rectangle_tool.on_mouse_move(pos);
                                self.needs_redraw = true;
                            }
                        }
                        ToolKind::RoundedRect => {
                            if overlay.rounded_rect_tool.is_drawing() {
                                overlay.rounded_rect_tool.on_mouse_move(pos);
                                self.needs_redraw = true;
                            }
                        }
                        ToolKind::Circle => {
                            if overlay.circle_tool.is_drawing() {
                                overlay.circle_tool.on_mouse_move(pos);
                                self.needs_redraw = true;
                            }
                        }
                        ToolKind::Line => {
                            if overlay.line_tool.is_drawing() {
                                overlay.line_tool.on_mouse_move(pos);
                                self.needs_redraw = true;
                            }
                        }
                        ToolKind::Measurement => {
                            if overlay.measurement_tool.is_drawing() {
                                overlay.measurement_tool.on_mouse_move(pos);
                                self.needs_redraw = true;
                            }
                        }
                        ToolKind::Pencil => {
                            if overlay.pencil_tool.is_drawing() {
                                overlay.pencil_tool.on_mouse_move(pos);
                                self.needs_redraw = true;
                            }
                        }
                        ToolKind::Highlight => {
                            if overlay.highlight_tool.is_drawing() {
                                overlay.highlight_tool.on_mouse_move(pos);
                                self.needs_redraw = true;
                            }
                        }
                        ToolKind::Spotlight => {
                            if overlay.spotlight_tool.is_drawing() {
                                overlay.spotlight_tool.on_mouse_move(pos);
                                self.needs_redraw = true;
                            }
                        }
                        ToolKind::Text => {
                            // Text tool does not use mouse move
                            overlay.text_tool.on_mouse_move(pos);
                        }
                        ToolKind::Pixelate => {
                            if overlay.pixelate_tool.is_drawing() {
                                overlay.pixelate_tool.on_mouse_move(pos);
                                self.needs_redraw = true;
                            }
                        }
                        ToolKind::StepMarker => {
                            overlay.step_marker_tool.on_mouse_move(pos);
                        }
                        ToolKind::Eyedropper => {
                            // Read color from screenshot pixels at cursor position
                            let px = pos.x as u32;
                            let py = pos.y as u32;
                            if px < overlay.screenshot.width && py < overlay.screenshot.height {
                                let idx = ((py * overlay.screenshot.width + px) * 4) as usize;
                                if idx + 3 < overlay.screenshot.pixels.len() {
                                    let r = overlay.screenshot.pixels[idx] as f32 / 255.0;
                                    let g = overlay.screenshot.pixels[idx + 1] as f32 / 255.0;
                                    let b = overlay.screenshot.pixels[idx + 2] as f32 / 255.0;
                                    overlay.eyedropper_preview = Some(Color::new(r, g, b, 1.0));
                                }
                            }
                            self.needs_redraw = true;
                        }
                    }
                }

                // Update cursor icon based on current state
                if let Some(ref window) = self.overlay_window {
                    let cursor = determine_cursor(overlay, pos);
                    window.set_cursor(cursor);
                }

                // Always redraw on mouse move for tooltips and cursor feedback
                // (60fps cap prevents this from being wasteful)
                self.needs_redraw = true;
            }

            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: WinitMouseButton::Left,
                ..
            } => {
                // Window capture mode: click confirms the highlighted window
                if self.window_capture_mode {
                    // Selection is already set to the window under cursor
                    self.window_capture_mode = false;
                    self.window_rects.clear();
                    self.needs_redraw = true;
                    tracing::info!("Window captured");
                    if overlay.selection.is_some() && self.config.general.finish_on_release {
                        self.do_copy();
                    }
                    return;
                }

                let pos = overlay.last_mouse_pos;

                // 1. Check toolbar hit first (only if selection exists)
                if let Some(ref sel) = overlay.selection {
                    let visible_count = overlay.visible_buttons.len();
                    let toolbar = Toolbar::position_for_dynamic(
                        sel,
                        overlay.screenshot.height as f32,
                        visible_count,
                    );
                    if let Some(vis_idx) = toolbar.hit_test_dynamic(pos, visible_count) {
                        let btn = overlay.visible_buttons[vis_idx];
                        let action = BUTTONS.get(btn).map(|d| d.action);
                        // Reset upload confirmation if clicking anything other than Upload
                        if action != Some(ButtonAction::Upload) {
                            overlay.upload_confirmed = false;
                        }
                        match action {
                            Some(ButtonAction::Tool(kind)) => {
                                overlay.active_tool = kind;
                                if kind != ToolKind::Select {
                                    overlay.selected_index = None;
                                }
                                self.needs_redraw = true;
                            }
                            Some(ButtonAction::Color(idx)) => {
                                let presets = Color::presets();
                                if idx < presets.len() {
                                    // If an annotation is selected, recolor it
                                    if let Some(sel_idx) = overlay.selected_index {
                                        if sel_idx < overlay.annotations.len() {
                                            let old_ann = overlay.annotations[sel_idx].clone();
                                            recolor_annotation(
                                                &mut overlay.annotations[sel_idx],
                                                presets[idx],
                                            );
                                            record_undo(
                                                &mut overlay.undo_stack,
                                                &mut overlay.redo_stack,
                                                UndoAction::Modify(sel_idx, old_ann),
                                            );
                                        }
                                    } else {
                                        overlay.set_color_all(presets[idx]);
                                    }
                                    self.needs_redraw = true;
                                }
                            }
                            Some(ButtonAction::Ocr) => {
                                self.do_ocr();
                                return;
                            }
                            Some(ButtonAction::Upload) => {
                                self.do_upload();
                                return;
                            }
                            Some(ButtonAction::Pin) => {
                                self.do_pin(_event_loop);
                                return;
                            }
                            Some(ButtonAction::Copy) => {
                                self.do_copy();
                                return;
                            }
                            Some(ButtonAction::Save) => {
                                self.do_save();
                                return;
                            }
                            None => {}
                        }
                        return;
                    }
                }

                // 2. No selection yet — start selecting
                if overlay.selection.is_none() {
                    overlay.is_selecting = true;
                    overlay.drag_start = Some(pos);
                    return;
                }

                // 3. Selection exists — hit-test it
                if let Some(ref sel) = overlay.selection {
                    match sel.hit_test(pos, 8.0) {
                        Some(HitZone::Inside) => {
                            // Start annotation with active tool
                            match overlay.active_tool {
                                ToolKind::Select => {
                                    // Check resize handles first (if an annotation is selected)
                                    if let Some(idx) = overlay.selected_index {
                                        if let Some(ann) = overlay.annotations.get(idx) {
                                            if let Some((bx, by, bw, bh)) =
                                                annotation_bounding_box(ann)
                                            {
                                                let handles = [
                                                    (Point::new(bx, by), ResizeHandle::TopLeft),
                                                    (
                                                        Point::new(bx + bw, by),
                                                        ResizeHandle::TopRight,
                                                    ),
                                                    (
                                                        Point::new(bx, by + bh),
                                                        ResizeHandle::BottomLeft,
                                                    ),
                                                    (
                                                        Point::new(bx + bw, by + bh),
                                                        ResizeHandle::BottomRight,
                                                    ),
                                                ];
                                                for (hp, handle) in &handles {
                                                    if (pos.x - hp.x).abs() < 8.0
                                                        && (pos.y - hp.y).abs() < 8.0
                                                    {
                                                        overlay.resize_handle = Some(*handle);
                                                        // Snapshot annotation before resize for undo
                                                        overlay.pre_drag_annotation = Some((
                                                            idx,
                                                            overlay.annotations[idx].clone(),
                                                        ));
                                                        self.needs_redraw = true;
                                                        return;
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    // Check annotations in REVERSE order (top-most first)
                                    let mut found = None;
                                    for (idx, ann) in overlay.annotations.iter().enumerate().rev() {
                                        if hit_test_annotation(ann, &pos, 8.0) {
                                            found = Some(idx);
                                            break;
                                        }
                                    }
                                    if let Some(idx) = found {
                                        // If clicking an already-selected Text annotation, re-enter edit mode
                                        if overlay.selected_index == Some(idx) {
                                            if let Some(Annotation::Text {
                                                position,
                                                text,
                                                color,
                                                font_size,
                                            }) = overlay.annotations.get(idx).cloned()
                                            {
                                                // Take the annotation out and remember it: committing
                                                // records ONE Modify undo step; Escape restores it.
                                                let removed = overlay.annotations.remove(idx);
                                                overlay.text_edit_origin = Some((idx, removed));
                                                overlay.selected_index = None;
                                                overlay.text_input_active = true;
                                                overlay.text_input_position = position;
                                                overlay.text_input_buffer = text;
                                                overlay.text_input_font_size = font_size;
                                                overlay.current_color = color;
                                                self.needs_redraw = true;
                                                return;
                                            }
                                        }
                                        overlay.selected_index = Some(idx);
                                        overlay.select_drag_start = Some(pos);
                                        // Snapshot annotation before move for undo
                                        overlay.pre_drag_annotation =
                                            Some((idx, overlay.annotations[idx].clone()));
                                    } else {
                                        overlay.selected_index = None;
                                    }
                                }
                                ToolKind::Arrow => overlay.arrow_tool.on_mouse_down(pos),
                                ToolKind::Rectangle => overlay.rectangle_tool.on_mouse_down(pos),
                                ToolKind::RoundedRect => {
                                    overlay.rounded_rect_tool.on_mouse_down(pos)
                                }
                                ToolKind::Circle => overlay.circle_tool.on_mouse_down(pos),
                                ToolKind::Line => overlay.line_tool.on_mouse_down(pos),
                                ToolKind::Measurement => {
                                    overlay.measurement_tool.on_mouse_down(pos)
                                }
                                ToolKind::Pencil => overlay.pencil_tool.on_mouse_down(pos),
                                ToolKind::Highlight => overlay.highlight_tool.on_mouse_down(pos),
                                ToolKind::Text => {
                                    overlay.text_tool.on_mouse_down(pos);
                                    if let Some(p) = overlay.text_tool.take_pending_position() {
                                        overlay.text_input_active = true;
                                        overlay.text_input_position = p;
                                        overlay.text_input_buffer.clear();
                                        overlay.text_input_font_size =
                                            overlay.text_tool.font_size();
                                    }
                                }
                                ToolKind::Pixelate => overlay.pixelate_tool.on_mouse_down(pos),
                                ToolKind::StepMarker => overlay.step_marker_tool.on_mouse_down(pos),
                                ToolKind::Spotlight => overlay.spotlight_tool.on_mouse_down(pos),
                                ToolKind::Eyedropper => {
                                    if let Some(color) = overlay.eyedropper_preview {
                                        overlay.set_color_all(color);
                                        overlay.active_tool = ToolKind::Arrow;
                                    }
                                }
                            }
                            self.needs_redraw = true;
                        }
                        Some(zone) => {
                            // Start resize
                            overlay.drag_zone = Some(zone);
                        }
                        None => {
                            // Outside selection — clear and start new
                            overlay.selection = None;
                            overlay.is_selecting = true;
                            overlay.drag_start = Some(pos);
                            self.needs_redraw = true;
                        }
                    }
                }
            }

            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: WinitMouseButton::Left,
                ..
            } => {
                let pos = overlay.last_mouse_pos;

                if overlay.is_selecting {
                    overlay.is_selecting = false;
                    // Finalize selection — ensure minimum size
                    let mut drawn = false;
                    if let Some(start) = overlay.drag_start.take() {
                        let sel = Selection::from_points(start, pos);
                        if sel.width > 2.0 && sel.height > 2.0 {
                            overlay.selection = Some(sel);
                            drawn = true;
                        }
                    }
                    self.needs_redraw = true;
                    if drawn && self.config.general.finish_on_release {
                        // "Copy as soon as a region is drawn": skip markup.
                        self.do_copy();
                        return;
                    }
                } else if overlay.drag_zone.is_some() {
                    overlay.drag_zone = None;
                } else {
                    // Finalize annotation
                    let annotation = match overlay.active_tool {
                        ToolKind::Select => {
                            // Record undo for move/resize if annotation changed
                            if let Some((idx, old_ann)) = overlay.pre_drag_annotation.take() {
                                if idx < overlay.annotations.len()
                                    && overlay.annotations[idx] != old_ann
                                {
                                    record_undo(
                                        &mut overlay.undo_stack,
                                        &mut overlay.redo_stack,
                                        UndoAction::Modify(idx, old_ann),
                                    );
                                }
                            }
                            overlay.select_drag_start = None;
                            overlay.resize_handle = None;
                            None
                        }
                        ToolKind::Arrow => overlay.arrow_tool.on_mouse_up(pos),
                        ToolKind::Rectangle => overlay.rectangle_tool.on_mouse_up(pos),
                        ToolKind::RoundedRect => overlay.rounded_rect_tool.on_mouse_up(pos),
                        ToolKind::Circle => overlay.circle_tool.on_mouse_up(pos),
                        ToolKind::Line => overlay.line_tool.on_mouse_up(pos),
                        ToolKind::Measurement => overlay.measurement_tool.on_mouse_up(pos),
                        ToolKind::Pencil => overlay.pencil_tool.on_mouse_up(pos),
                        ToolKind::Highlight => overlay.highlight_tool.on_mouse_up(pos),
                        ToolKind::Text => overlay.text_tool.on_mouse_up(pos),
                        ToolKind::Pixelate => overlay.pixelate_tool.on_mouse_up(pos),
                        ToolKind::StepMarker => overlay.step_marker_tool.on_mouse_up(pos),
                        ToolKind::Spotlight => overlay.spotlight_tool.on_mouse_up(pos),
                        ToolKind::Eyedropper => None,
                    };
                    if let Some(ann) = annotation {
                        overlay.annotations.push(ann);
                        record_undo(
                            &mut overlay.undo_stack,
                            &mut overlay.redo_stack,
                            UndoAction::Add(overlay.annotations.len() - 1),
                        );
                        self.needs_redraw = true;
                    }
                }
            }

            // Right-click: color picker on swatch, otherwise close overlay
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: WinitMouseButton::Right,
                ..
            } => {
                if let AppState::Capturing(ref mut overlay) = self.state {
                    if let Some(ref sel) = overlay.selection {
                        let visible_count = overlay.visible_buttons.len();
                        let toolbar = Toolbar::position_for_dynamic(
                            sel,
                            overlay.screenshot.height as f32,
                            visible_count,
                        );
                        let pos = overlay.last_mouse_pos;
                        if let Some(vis_idx) = toolbar.hit_test_dynamic(pos, visible_count) {
                            let btn = overlay.visible_buttons[vis_idx];
                            if let Some(ButtonAction::Color(swatch_idx)) =
                                BUTTONS.get(btn).map(|d| d.action)
                            {
                                let current = Color::presets()
                                    .get(swatch_idx)
                                    .copied()
                                    .unwrap_or_else(Color::red);
                                if let Some(ref w) = self.overlay_window {
                                    w.set_visible(false);
                                }
                                let picked = screen_mark::color_picker::pick_color(&current);
                                if let Some(ref w) = self.overlay_window {
                                    w.set_visible(true);
                                }
                                if let Some(new_color) = picked {
                                    overlay.set_color_all(new_color);
                                    if let Some(idx) = overlay.selected_index {
                                        if idx < overlay.annotations.len() {
                                            let old_ann = overlay.annotations[idx].clone();
                                            recolor_annotation(
                                                &mut overlay.annotations[idx],
                                                new_color,
                                            );
                                            record_undo(
                                                &mut overlay.undo_stack,
                                                &mut overlay.redo_stack,
                                                UndoAction::Modify(idx, old_ann),
                                            );
                                        }
                                    }
                                    self.needs_redraw = true;
                                }
                                return;
                            }
                        }
                    }
                }
                if let AppState::Capturing(ref overlay) = self.state {
                    if overlay.selection.is_some() {
                        return;
                    }
                }
                self.close_overlay();
                return;
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let scroll = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(pos) => pos.y as f32 / 20.0,
                };
                if overlay.text_input_active {
                    // Adjust font size while typing
                    overlay.text_input_font_size =
                        (overlay.text_input_font_size + scroll).clamp(10.0, 72.0);
                } else if overlay.active_tool == ToolKind::StepMarker {
                    let new_size = overlay.step_marker_tool.size() + scroll * 2.0;
                    overlay.step_marker_tool.set_size(new_size);
                } else if overlay.active_tool == ToolKind::RoundedRect {
                    let new_radius = overlay.rounded_rect_tool.radius() + scroll * 2.0;
                    overlay.rounded_rect_tool.set_radius(new_radius);
                } else {
                    overlay.set_thickness_all(overlay.current_thickness + scroll);
                }
                self.needs_redraw = true;
            }

            _ => {}
        }

        if self.needs_redraw {
            if let Some(w) = &self.overlay_window {
                w.request_redraw();
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.process_tray_events(event_loop);

        // Settings lost focus while listening: stop, so the hotkeys work again.
        let mut blur_deadline = None;
        if let Some(t) = self.settings_blur_at {
            let deadline = t + SETTINGS_BLUR_GRACE;
            if Instant::now() >= deadline {
                self.settings_blur_at = None;
                if let Some(sw) = self.settings_window.as_mut() {
                    if sw.view.stop_listening() {
                        sw.window.request_redraw();
                    }
                }
            } else {
                blur_deadline = Some(deadline);
            }
        }

        // While Settings is listening for a new combination, give it every
        // key: release the OS registrations, take them back afterwards.
        let listening = self
            .settings_window
            .as_ref()
            .is_some_and(|sw| sw.view.editing().is_some());
        if listening {
            self.hotkeys.suspend();
        } else if self.hotkeys.is_suspended() {
            self.hotkeys.resume();
        }

        // Drain ALL pending hotkey events; key-repeat collapses to one press.
        let now = Instant::now();
        while let Ok(event) = self.hotkey_rx.try_recv() {
            tracing::debug!("hotkey event {:?}", event);
            match self.hotkeys.handle(&event, now) {
                Some(KeyId::Region) => {
                    tracing::info!("Region hotkey pressed");
                    self.request_capture(event_loop, CaptureKind::Region);
                }
                Some(KeyId::Screenshot) => {
                    tracing::info!("Screenshot hotkey pressed");
                    self.request_capture(event_loop, CaptureKind::Screenshot);
                }
                _ => {}
            }
        }

        // Results from capture workers.
        while let Ok((spec, monitor)) = self.jobs_rx.try_recv() {
            self.jobs_pending = self.jobs_pending.saturating_sub(1);
            if let Some(spec) = spec {
                self.feedback(event_loop, spec, monitor);
            }
        }

        // Apply async folder-picker results for the settings window.
        if let Some(ref mut sw) = self.settings_window {
            if sw.view.poll_browse() {
                sw.window.request_redraw();
            }
        }

        let mut wake: Option<Instant> = None;
        let mut wake_at = |t: Instant| {
            wake = Some(wake.map_or(t, |w: Instant| w.min(t)));
        };
        if let Some(t) = blur_deadline {
            wake_at(t);
        }

        // Countdown before a delayed capture.
        if let Some(ref mut cd) = self.countdown {
            let now = Instant::now();
            if cd.tick(now) {
                // Hide before dropping so the window is gone from the screen
                // before the screenshot is taken.
                cd.hide();
                self.countdown = None;
                self.capture_at
                    .push_back((now + Duration::from_millis(300), CaptureKind::Region));
            } else {
                wake_at(cd.next_tick());
            }
        }

        if let Some(&(when, kind)) = self.capture_at.front() {
            if Instant::now() >= when {
                if self.toast.take().is_some() {
                    // A result popup appeared while this capture waited;
                    // give the compositor time to remove it too.
                    let later = Instant::now() + TOAST_CLEAR_DELAY;
                    self.capture_at[0].0 = later;
                    wake_at(later);
                } else if self.overlay_window.is_some() {
                    // The overlay from an earlier press is open: a region
                    // would be lost behind it and a screenshot would show it.
                    self.capture_at.pop_front();
                } else {
                    self.capture_at.pop_front();
                    match kind {
                        CaptureKind::Region => self.trigger_capture(event_loop),
                        CaptureKind::Screenshot => self.do_screenshot(event_loop),
                    }
                    if let Some(&(next, _)) = self.capture_at.front() {
                        wake_at(next);
                    }
                }
            } else {
                wake_at(when);
            }
        }

        // The popup closes itself.
        if let Some(until) = self.toast.as_ref().map(|t| t.until()) {
            if Instant::now() >= until {
                self.toast = None;
            } else {
                wake_at(until);
            }
        }

        // The overlay's own in-place message expires.
        if let AppState::Capturing(ref overlay) = self.state {
            if let Some(until) = overlay.toast_until {
                if Instant::now() >= until {
                    self.needs_redraw = true;
                } else {
                    wake_at(until);
                }
            }
        }

        // `screen-mark capture` (interactive): quit once the overlay is gone
        // and its copy/save has finished and been reported.
        if self.cli_only
            && !self.immediate_capture
            && self.overlay_window.is_none()
            && self.jobs_pending == 0
            && self.toast.is_none()
            && self.capture_at.is_empty()
        {
            event_loop.exit();
            return;
        }

        // Frame rate cap: only render if enough time has passed since last frame
        if self.needs_redraw {
            let now = Instant::now();
            if now.duration_since(self.last_render) >= FRAME_INTERVAL {
                self.render();
                self.last_render = now;
            } else {
                wake_at(self.last_render + FRAME_INTERVAL);
            }
        }

        // While a folder-picker dialog is open we must self-schedule wakeups —
        // the dialog produces no winit events.
        if self
            .settings_window
            .as_ref()
            .is_some_and(|sw| sw.view.browse_pending())
        {
            wake_at(Instant::now() + Duration::from_millis(100));
        }

        match wake {
            Some(t) => event_loop.set_control_flow(ControlFlow::WaitUntil(t)),
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }
}

fn run_tray_app(config: Config) {
    // Single-instance enforcement: only one tray app at a time
    #[cfg(target_os = "windows")]
    {
        use windows::core::w;
        use windows::Win32::Foundation::GetLastError;
        use windows::Win32::Foundation::ERROR_ALREADY_EXISTS;
        use windows::Win32::System::Threading::CreateMutexW;
        unsafe {
            if let Ok(m) = CreateMutexW(None, false, w!("screen-mark.SingleInstance")) {
                if GetLastError() == ERROR_ALREADY_EXISTS {
                    let _ = windows::Win32::Foundation::CloseHandle(m);
                    screen_mark::notify::system(
                        "screen-mark",
                        "screen-mark is already running in the system tray",
                    );
                    return;
                }
                // Mutex handle is Copy — kernel keeps it alive for the process lifetime
                let _ = m;
            }
        }
    }

    let event_loop = match EventLoop::new() {
        Ok(el) => el,
        Err(e) => {
            screen_mark::installer::show_error(&format!("Failed to start: {e}"));
            std::process::exit(1);
        }
    };
    screen_mark::notify::register_app_id();
    let mut app = App::new(config);
    app.set_proxy(event_loop.create_proxy());
    if let Err(e) = event_loop.run_app(&mut app) {
        screen_mark::installer::show_error(&format!("Event loop error: {e}"));
        std::process::exit(1);
    }
}

fn run_tray_app_with_immediate_capture(config: Config) {
    let event_loop = match EventLoop::new() {
        Ok(el) => el,
        Err(e) => {
            screen_mark::installer::show_error(&format!("Failed to start: {e}"));
            std::process::exit(1);
        }
    };
    let mut app = App::new(config);
    app.set_proxy(event_loop.create_proxy());
    app.immediate_capture = true;
    app.cli_only = true;
    if let Err(e) = event_loop.run_app(&mut app) {
        screen_mark::installer::show_error(&format!("Event loop error: {e}"));
        std::process::exit(1);
    }
}

fn run_cli_capture(clipboard: bool, save: Option<String>, delay: u64) {
    if delay > 0 {
        tracing::info!("Waiting {} seconds...", delay);
        std::thread::sleep(std::time::Duration::from_secs(delay));
    }

    // If neither --clipboard nor --save: open interactive overlay
    if !clipboard && save.is_none() {
        tracing::info!("Opening interactive capture...");
        let config = Config::load();
        run_tray_app_with_immediate_capture(config);
        return;
    }

    // Non-interactive: capture full screen directly
    let capturer = match capture::create_capturer() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to create capturer: {}", e);
            std::process::exit(1);
        }
    };

    let screens = match capturer.capture_all_screens() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Capture failed: {}", e);
            std::process::exit(1);
        }
    };

    if screens.is_empty() {
        eprintln!("No screens captured");
        std::process::exit(1);
    }

    let screen = &screens[0];

    // --clipboard and --save are independent; both can be given at once.
    if clipboard {
        match export::copy_to_clipboard(&screen.pixels, screen.width, screen.height) {
            Ok(_) => {
                println!(
                    "Copied {}x{} screenshot to clipboard",
                    screen.width, screen.height
                );
            }
            Err(e) => {
                eprintln!("Clipboard error: {}", e);
                std::process::exit(1);
            }
        }
    }
    if let Some(path) = save {
        let img =
            match image::RgbaImage::from_raw(screen.width, screen.height, screen.pixels.clone()) {
                Some(img) => img,
                None => {
                    eprintln!("Invalid image data ({}x{})", screen.width, screen.height);
                    std::process::exit(1);
                }
            };
        match img.save(&path) {
            Ok(_) => println!("Saved to {}", path),
            Err(e) => {
                eprintln!("Save error: {}", e);
                std::process::exit(1);
            }
        }
    }
}

fn main() {
    tracing_subscriber::fmt::init();
    tracing::info!("screen-mark starting");

    #[cfg(target_os = "windows")]
    {
        use windows::core::HSTRING;
        use windows::Win32::UI::HiDpi::{
            SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        };
        use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
        unsafe {
            // Physical pixels everywhere, including `capture --clipboard`,
            // which never creates a winit event loop (winit would set this).
            // Without it a 150%-scaled screen is captured at 2/3 size.
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            // Same id the notification registration uses (notify::APP_ID).
            let _ = SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(
                screen_mark::notify::APP_ID,
            ));
        }
    }

    let cli = Cli::parse();

    // For CLI subcommands, attach to the parent console so println!/eprintln!
    // produce visible output (windows_subsystem = "windows" detaches stdout).
    #[cfg(target_os = "windows")]
    if cli.command.is_some() {
        use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
        unsafe {
            let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }

    match cli.command {
        None => {
            // If not running from the install location, OFFER to install (the
            // installer spawns the installed copy and we exit). Declining runs
            // the app portably from its current location. needs_install() is
            // always false in debug builds and on non-Windows platforms.
            if screen_mark::installer::needs_install() && screen_mark::installer::confirm_install()
            {
                if let Err(e) = screen_mark::installer::install() {
                    screen_mark::installer::show_error(&format!("Install failed: {e}"));
                    std::process::exit(1);
                }
                return;
            }

            let config = Config::load();
            tracing::info!(
                "Config loaded: region={}, screenshot={}, color={}, thickness={}",
                config.hotkey.region,
                config.hotkey.screenshot,
                config.general.default_color,
                config.general.default_thickness
            );
            run_tray_app(config);
        }
        Some(Commands::Capture {
            clipboard,
            save,
            delay,
        }) => {
            run_cli_capture(clipboard, save, delay);
        }
        Some(Commands::Install) => {
            if let Err(e) = screen_mark::installer::install() {
                screen_mark::installer::show_error(&format!("Install failed: {e}"));
                std::process::exit(1);
            }
        }
        Some(Commands::Uninstall) => {
            if let Err(e) = screen_mark::installer::uninstall() {
                screen_mark::installer::show_error(&format!("Uninstall failed: {e}"));
                std::process::exit(1);
            }
        }
    }
}
