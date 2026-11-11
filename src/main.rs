#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::num::NonZeroU32;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use clap::Parser;
use winit::event::{
    ElementState, Ime, MouseButton as WinitMouseButton, MouseScrollDelta, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowAttributes, WindowId, WindowLevel};

use winit::application::ApplicationHandler;

use screen_mark::access::Bridge;
use screen_mark::capture;
use screen_mark::cli::{Cli, Commands};
use screen_mark::config::Config;
use screen_mark::countdown::Countdown;
use screen_mark::display::{self, Monitor};
use screen_mark::export;
use screen_mark::geometry::{Color, Point};
use screen_mark::history_ui::{HistoryWindow, Outcome as HistoryOutcome};
use screen_mark::hotkey::{GlobalHotkeys, Status};
use screen_mark::keys::{self, KeyId, KeyMap, KeyPress};
use screen_mark::overlay::input::{KeyInput, Outcome as OverlayOutcome};
use screen_mark::pin::{PinEvent, PinnedWindow};
use screen_mark::renderer::{render_frame, to_window_pixels, Area, Frame};
use screen_mark::settings_ui::{Outcome, SettingsView, SettingsWindow, Tab};
use screen_mark::state::{AppState, OverlayState};
use screen_mark::toast::{self, Click, Kind, Spec, Toast};
use screen_mark::tray::{self, TrayEvent, TrayHandle};
use screen_mark::update::{self, Check, Release, Source, Version};
use screen_mark::window_detect;

/// A window's software-rendered surface.
type Surface = softbuffer::Surface<Arc<Window>, Arc<Window>>;

struct App {
    config: Config,
    state: AppState,
    tray: Option<TrayHandle>,
    tray_tx: Sender<TrayEvent>,
    tray_rx: Receiver<TrayEvent>,
    hotkey_rx: Receiver<global_hotkey::GlobalHotKeyEvent>,
    hotkey_wake: Arc<std::sync::OnceLock<EventLoopProxy<()>>>,
    overlay_window: Option<Arc<Window>>,
    surface: Option<Surface>,
    pixmap: Option<tiny_skia::Pixmap>,
    modifiers: ModifiersState,
    /// What the last overlay frame drew, so the next redraws only what
    /// changed (see `renderer`).
    frame: Frame,
    /// The next frame goes to the window whole: its buffer doesn't hold
    /// the last one, or the system asked for a repaint.
    present_all: bool,
    /// The size the window's buffer was last set to.
    surface_size: (u32, u32),
    /// Whether the overlay window is taking input-method (IME) text: on
    /// only while a text annotation is being typed.
    ime_on: bool,
    /// Modifier state tracked separately for the settings window (used while
    /// capturing a new global-hotkey combination).
    settings_modifiers: ModifiersState,
    /// When the Settings window lost focus (see `SETTINGS_BLUR_GRACE`).
    settings_blur_at: Option<Instant>,
    /// Modifier state for the History window (Shift+Tab).
    history_modifiers: ModifiersState,
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
    /// What a command-line run (`capture`, `edit`) does once the event
    /// loop is up.
    startup: Option<Startup>,
    cli_only: bool,
    /// `capture --clipboard` / `--save` with a delay: what to do with the
    /// capture once the countdown is over.
    cli_grab: Option<(bool, Option<String>)>,
    /// The exit code of a command-line run.
    exit_code: i32,
    /// What the countdown starts when it reaches 0.
    countdown_kind: CaptureKind,
    /// Images read on a worker thread (a file, a History capture), to mark
    /// up, with the name the save dialog suggests.
    opened_tx: Sender<Opened>,
    opened_rx: Receiver<Opened>,
    startup_notified: bool,
    countdown: Option<Countdown>,
    /// Where the last region capture was on screen, for the last-region
    /// hotkey. Loaded at startup, saved whenever a region capture is used.
    last_region: Option<display::Rect>,
    settings_window: Option<SettingsWindow>,
    history_window: Option<HistoryWindow>,
    /// What tells a screen reader about Settings and History (see
    /// `access`). Each lives as long as its window and is dropped first.
    settings_access: Option<Bridge>,
    history_access: Option<Bridge>,
    /// The same for the capture overlay: its toolbar and where things
    /// stand (see `overlay::describe`).
    overlay_access: Option<Bridge>,
    /// Where an update stands, shared with the threads that check and
    /// install (see `update_clicked`).
    update: Arc<Mutex<UpdateStatus>>,
    /// What the tray's update item says now.
    update_label: String,
    /// When to look for a new release next (`[updates] auto_check`).
    update_check_at: Option<Instant>,
    /// The restart after an update has been done, or offered.
    restart_handled: bool,
    /// Quit was chosen once while an update was being installed.
    quit_warned: bool,
    /// The version this one replaced, when an update started it.
    updated_from: Option<String>,
}

/// Where an update stands. The worker threads write it; the event loop
/// reads it when a job finishes (`update_progress`).
#[derive(Default)]
struct UpdateStatus {
    stage: UpdateStage,
    /// A result to show, and whether the user asked for it. One they asked
    /// for is shown whatever `notifications` says.
    news: Option<(Spec, bool)>,
}

#[derive(Default, Clone)]
enum UpdateStage {
    /// Nothing is known: no check yet, or the last one found nothing.
    #[default]
    Unknown,
    Checking,
    /// A newer release, verified and ready to install.
    Available(Release),
    Installing(Version),
    /// The new version is in place at this path; a restart runs it.
    Installed(Version, std::path::PathBuf),
}

fn lock(status: &Mutex<UpdateStatus>) -> MutexGuard<'_, UpdateStatus> {
    status
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Whether this copy of screen-mark can replace itself with a new version
/// (not one a package manager owns, or a development build).
fn installs_updates() -> bool {
    update::running_copy().is_ok_and(|(_, kind)| kind.installs())
}

/// An error popup that shows the log file when clicked.
fn failure_spec(title: &str, detail: String) -> Spec {
    let spec = Spec::new(Kind::Error, title, detail);
    match screen_mark::logging::log_path().filter(|p| p.is_file()) {
        Some(log) => spec
            .hint("Click to show the log")
            .click(Click::ShowFile(log)),
        None => spec,
    }
}

/// `text` with its first letter capitalised: error texts are written to
/// follow a colon.
fn sentence(text: &str) -> String {
    let mut letters = text.chars();
    match letters.next() {
        Some(first) => first.to_uppercase().chain(letters).collect(),
        None => String::new(),
    }
}

const FRAME_INTERVAL: Duration = Duration::from_millis(16); // ~60fps cap
/// `[updates] auto_check`: the first look after starting, and how often
/// after that.
const FIRST_UPDATE_CHECK: Duration = Duration::from_secs(60);
const UPDATE_CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

/// On Windows the accessibility bridge has to be attached to a window
/// before it is first shown, so Settings and History start hidden and are
/// shown once their first frame is drawn.
const ACCESS_NEEDS_HIDDEN_START: bool = cfg!(target_os = "windows");

/// How long to wait after closing a popup before grabbing the screen, so
/// the compositor has removed it.
const TOAST_CLEAR_DELAY: Duration = Duration::from_millis(180);
/// How long the Settings window may be out of focus before it stops
/// listening for a combination.
const SETTINGS_BLUR_GRACE: Duration = Duration::from_millis(1500);
/// How long the tray menu takes to leave the screen after a click.
const TRAY_MENU_CLOSE_DELAY: Duration = Duration::from_millis(250);

/// An image read to mark up, and the name the save dialog suggests for it;
/// or why it couldn't be read.
type Opened = Result<(screen_mark::history::Editable, Option<String>), String>;

/// What a command-line run does once the event loop is up.
enum Startup {
    /// `screen-mark capture`: open the overlay, after a countdown of this
    /// many seconds.
    Overlay(u32),
    /// `screen-mark capture --clipboard` / `--save` with a delay: count
    /// down, then capture.
    Grab(u32),
    /// `screen-mark edit`: mark up this image.
    Edit(Box<screen_mark::history::Editable>, Option<String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureKind {
    /// Open the overlay to drag a region.
    Region,
    /// Open the overlay to click a window.
    Window,
    /// One press: grab, save, copy.
    Screenshot,
    /// One press: grab the area of the last region capture again, save, copy.
    LastRegion,
    /// `capture --clipboard` / `--save` after a countdown (see `cli_grab`).
    CliGrab,
}

impl App {
    fn new(config: Config) -> Self {
        screen_mark::theme::set_mode(config.theme_mode());
        export::set_file_name(&config.general.file_name);
        let keymap = KeyMap::from_report(&config.key_report());
        let (jobs_tx, jobs_rx) = channel();
        let (tray_tx, tray_rx) = channel();
        let (opened_tx, opened_rx) = channel();
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
            frame: Frame::default(),
            present_all: true,
            surface_size: (0, 0),
            ime_on: false,
            settings_modifiers: ModifiersState::empty(),
            settings_blur_at: None,
            history_modifiers: ModifiersState::empty(),
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
            startup: None,
            cli_only: false,
            cli_grab: None,
            exit_code: 0,
            countdown_kind: CaptureKind::Region,
            opened_tx,
            opened_rx,
            startup_notified: false,
            countdown: None,
            last_region: screen_mark::last_region::load(),
            settings_window: None,
            history_window: None,
            settings_access: None,
            history_access: None,
            overlay_access: None,
            update: Arc::default(),
            update_label: tray::UPDATE_LABEL.to_string(),
            update_check_at: None,
            restart_handled: false,
            quit_warned: false,
            updated_from: None,
        }
    }

    /// Register both global hotkeys from config. A hotkey that can't be
    /// registered is reported (popup, Settings shows why); it no longer
    /// silently falls back to a default.
    fn apply_hotkeys(&mut self, event_loop: &ActiveEventLoop, announce: bool) {
        let wanted: Vec<(KeyId, String)> = KeyId::GLOBALS
            .iter()
            .map(|id| (*id, self.config.binding(*id)))
            .collect();
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
                [(KeyId::Screenshot, _)] => "Screenshot hotkey isn't working",
                [(_, _)] => "Last-region hotkey isn't working",
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
            // The popup has room for two; the tray menu shows the third.
            for (id, raw) in wanted.iter().filter(|(id, _)| *id != KeyId::LastRegion) {
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
            tray.set_bindings(
                &self.config.hotkey.region,
                &self.config.hotkey.screenshot,
                &self.config.hotkey.last_region,
            );
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
        self.popup(event_loop, spec, monitor);
    }

    /// Show a popup, whatever the `notifications` setting says.
    fn popup(&mut self, event_loop: &ActiveEventLoop, spec: Spec, monitor: Option<Monitor>) {
        let monitor = monitor.or_else(pointer_monitor);
        // Replace any popup already up.
        self.toast = None;
        self.toast = Toast::show(event_loop, spec, monitor);
        if self.toast.is_none() {
            tracing::warn!("Could not create the popup window");
        }
    }

    /// The tray's update item, or a click on an update popup: whatever
    /// comes next.
    fn update_clicked(&mut self, event_loop: &ActiveEventLoop) {
        let stage = lock(&self.update).stage.clone();
        match stage {
            UpdateStage::Unknown => self.check_for_updates(true),
            UpdateStage::Checking | UpdateStage::Installing(_) => {}
            UpdateStage::Available(release) => self.install_update(event_loop, release),
            UpdateStage::Installed(_, exe) => self.restart_into(event_loop, &exe),
        }
        self.update_progress(event_loop);
    }

    /// Look for a newer release, on a worker thread. `asked`: the user
    /// clicked for it and gets an answer either way. A check nobody asked
    /// for only speaks up when there is something new.
    fn check_for_updates(&mut self, asked: bool) {
        lock(&self.update).stage = UpdateStage::Checking;
        update::set_proxy(&self.config.updates.proxy);
        let status = Arc::clone(&self.update);
        let source = Source::from_setting(&self.config.updates.url);
        self.spawn_job(None, move || {
            let running = Version::running();
            let (stage, news) = match update::check(&source, running, &update::keys::trusted()) {
                Ok(Check::Available(release)) => {
                    let version = release.version();
                    tracing::info!("screen-mark {version} is available");
                    let hint = if installs_updates() {
                        "Click to install it, or use the tray menu"
                    } else {
                        "Click to see how to get it"
                    };
                    let mut spec = Spec::new(
                        Kind::Info,
                        format!("screen-mark {version} is available"),
                        format!("You have {running}"),
                    )
                    .hint(hint)
                    .click(Click::Update);
                    spec.duration = Duration::from_secs(12);
                    (UpdateStage::Available(release), Some(spec))
                }
                Ok(Check::UpToDate { latest }) => {
                    tracing::info!("No update: the newest release is {latest}");
                    let detail = if latest == running {
                        format!("{running} is the newest version")
                    } else {
                        format!("You have {running}; the newest release is {latest}")
                    };
                    let spec = Spec::new(Kind::Success, "screen-mark is up to date", detail);
                    (UpdateStage::Unknown, asked.then_some(spec))
                }
                Err(e) => {
                    tracing::warn!("Couldn't check for updates: {e}");
                    let spec = failure_spec("Couldn't check for updates", sentence(&e.to_string()));
                    (UpdateStage::Unknown, asked.then_some(spec))
                }
            };
            let mut status = lock(&status);
            status.stage = stage;
            status.news = news.map(|spec| (spec, asked));
            None
        });
    }

    /// Download `release` and put it in place, on a worker thread.
    fn install_update(&mut self, event_loop: &ActiveEventLoop, release: Release) {
        update::set_proxy(&self.config.updates.proxy);
        let version = release.version();
        let (exe, kind) = match update::running_copy() {
            Ok(found) => found,
            Err(e) => {
                return self.report_failure(event_loop, "Couldn't update", sentence(&e.to_string()))
            }
        };
        if let Some(reason) = kind.reason() {
            // Not a copy screen-mark replaces by itself: say why, and show
            // where the release is unless a package manager brings it.
            let title = format!("screen-mark {version} is available");
            self.popup(
                event_loop,
                Spec::new(Kind::Info, title, sentence(&reason)),
                None,
            );
            if !matches!(kind, update::Kind::Managed(_)) {
                open_url(&release.page());
            }
            return;
        }

        lock(&self.update).stage = UpdateStage::Installing(version);
        self.quit_warned = false;
        let spec = Spec::new(
            Kind::Info,
            format!("Downloading screen-mark {version}"),
            "screen-mark restarts when it is installed",
        );
        self.popup(event_loop, spec, None);
        let status = Arc::clone(&self.update);
        self.spawn_job(None, move || {
            let result = update::install(&release, &exe, kind);
            let mut status = lock(&status);
            match result {
                Ok(()) => {
                    tracing::info!("screen-mark {version} is installed at {}", exe.display());
                    status.stage = UpdateStage::Installed(version, exe);
                }
                Err(e) => {
                    tracing::error!("The update to {version} failed: {e}");
                    status.stage = UpdateStage::Available(release);
                    let spec =
                        failure_spec("The update wasn't installed", sentence(&e.to_string()));
                    status.news = Some((spec, true));
                }
            }
            None
        });
    }

    /// Keep the tray's update item saying what a click on it does, show
    /// what a finished check or install has to say, and restart once an
    /// update is in place.
    fn update_progress(&mut self, event_loop: &ActiveEventLoop) {
        let (stage, news) = {
            let mut status = lock(&self.update);
            (status.stage.clone(), status.news.take())
        };
        let label = match &stage {
            UpdateStage::Unknown => tray::UPDATE_LABEL.to_string(),
            UpdateStage::Checking => "Checking for updates\u{2026}".to_string(),
            UpdateStage::Available(release) if installs_updates() => {
                format!("Install screen-mark {}", release.version())
            }
            UpdateStage::Available(release) => format!("Get screen-mark {}", release.version()),
            UpdateStage::Installing(version) => format!("Installing screen-mark {version}\u{2026}"),
            UpdateStage::Installed(version, _) => format!("Restart to use screen-mark {version}"),
        };
        if label != self.update_label {
            if let Some(ref tray) = self.tray {
                tray.set_update(&label);
            }
            self.update_label = label;
        }

        if let Some((mut spec, asked)) = news {
            if asked {
                self.popup(event_loop, spec, None);
            } else {
                // A system notification can't be clicked.
                if self.config.notification_mode() == screen_mark::config::NotificationMode::System
                {
                    spec.hint = "It is in the tray menu".to_string();
                }
                self.feedback(event_loop, spec, None);
            }
        }

        if let UpdateStage::Installed(version, exe) = stage {
            if !self.restart_handled {
                self.restart_handled = true;
                let busy = self.overlay_window.is_some()
                    || self.countdown.is_some()
                    || !self.capture_at.is_empty()
                    || !self.pinned_windows.is_empty()
                    || self.settings_window.is_some()
                    || self.history_window.is_some()
                    || self.jobs_pending > 0;
                if busy {
                    // Not in the middle of a capture, and not with pins up.
                    let spec = Spec::new(
                        Kind::Success,
                        format!("screen-mark {version} is installed"),
                        "It takes over the next time screen-mark starts",
                    )
                    .hint("Click to restart now, or use the tray menu")
                    .click(Click::Update);
                    self.popup(event_loop, spec, None);
                } else {
                    self.restart_into(event_loop, &exe);
                }
            }
        }
    }

    /// Start the version an update put at `exe`, and leave. The new one
    /// waits for this one to be gone (`update::UPDATED_FROM`).
    fn restart_into(&mut self, event_loop: &ActiveEventLoop, exe: &std::path::Path) {
        tracing::info!("Restarting into {}", exe.display());
        let started = std::process::Command::new(exe)
            .env(update::UPDATED_FROM, Version::running().to_string())
            .spawn();
        match started {
            Ok(_) => event_loop.exit(),
            Err(e) => self.report_failure(
                event_loop,
                "Couldn't restart screen-mark",
                format!("{e}. Quit and start it again to finish the update."),
            ),
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
            self.run_capture(event_loop, kind);
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

    /// Start the capture `kind` asks for, now.
    fn run_capture(&mut self, event_loop: &ActiveEventLoop, kind: CaptureKind) {
        match kind {
            CaptureKind::Region => self.trigger_capture(event_loop, false),
            CaptureKind::Window => self.trigger_capture(event_loop, true),
            CaptureKind::Screenshot => self.do_screenshot(event_loop),
            CaptureKind::LastRegion => self.do_last_region(event_loop),
            CaptureKind::CliGrab => self.do_cli_grab(event_loop),
        }
    }

    /// Something went wrong outside a capture job. It is logged and shown
    /// in a popup; the tray app has no console, so without the popup a
    /// failed hotkey press looks like nothing happened. A click on the
    /// popup shows the log file.
    fn report_failure(&mut self, event_loop: &ActiveEventLoop, title: &str, detail: String) {
        tracing::error!("{title}: {detail}");
        self.feedback(event_loop, failure_spec(title, detail), None);
    }

    /// Grab the whole virtual desktop. A failure is reported in a popup.
    fn grab_desktop(&mut self, event_loop: &ActiveEventLoop) -> Option<capture::CapturedScreen> {
        let shot = capture::create_capturer()
            .map_err(|e| e.to_string())
            .and_then(|c| c.capture_all_screens().map_err(|e| e.to_string()))
            .and_then(|s| s.into_iter().next().ok_or_else(|| "no screen".to_string()));
        match shot {
            Ok(full) => Some(full),
            Err(e) => {
                self.report_failure(event_loop, "Couldn't capture the screen", e);
                None
            }
        }
    }

    /// Save `shot` to the save folder and copy it, on a worker thread.
    fn save_and_copy(&mut self, shot: capture::CapturedScreen, monitor: Option<Monitor>) {
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
                None,
            ))
        });
    }

    /// One press: capture the monitor under the mouse (or all monitors),
    /// save it to the save folder and copy it.
    fn do_screenshot(&mut self, event_loop: &ActiveEventLoop) {
        let Some(full) = self.grab_desktop(event_loop) else {
            return;
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
        self.save_and_copy(shot, monitor);
    }

    /// One press: capture the area of the last region capture again, then
    /// save it and copy it, like the one-press screenshot. Says so if no
    /// region has been captured yet, or if that area is on no screen now.
    fn do_last_region(&mut self, event_loop: &ActiveEventLoop) {
        let Some(rect) = self.last_region else {
            let region = &self.config.hotkey.region;
            let detail = if keys::Binding::parse(region).ok().flatten().is_some() {
                format!(
                    "Capture one with {} first",
                    screen_mark::settings_ui::display_binding(region)
                )
            } else {
                "Capture one from the tray menu first".to_string()
            };
            let spec = Spec::new(Kind::Info, "No region to capture again", detail);
            self.feedback(event_loop, spec, None);
            return;
        };
        let Some(full) = self.grab_desktop(event_loop) else {
            return;
        };
        let Some(shot) = display::crop(&full, rect) else {
            let detail = format!(
                "{} \u{00d7} {} at {}, {} isn't on a screen now",
                rect.w, rect.h, rect.x, rect.y
            );
            let spec = Spec::new(Kind::Error, "The last region is off screen", detail);
            self.feedback(event_loop, spec, None);
            return;
        };
        tracing::info!(
            "Last region {}x{} at ({}, {})",
            shot.width,
            shot.height,
            shot.x_offset,
            shot.y_offset
        );
        let monitor = display::monitor_at(
            &display::monitors(),
            rect.x.saturating_add((rect.w / 2) as i32),
            rect.y.saturating_add((rect.h / 2) as i32),
        );
        self.save_and_copy(shot, monitor);
    }

    /// Remember where the current selection is on screen, for the
    /// last-region hotkey. Called when a capture is used, not cancelled.
    fn remember_region(&mut self) {
        let AppState::Capturing(ref overlay) = self.state else {
            return;
        };
        // An image being marked up isn't anywhere on screen.
        if overlay.canvas.is_some() {
            return;
        }
        let Some(rect) = overlay.selection_on_screen() else {
            return;
        };
        self.last_region = Some(rect);
        if let Err(e) = screen_mark::last_region::save(rect) {
            tracing::warn!("Couldn't remember the last region: {e}");
        }
    }

    /// Grab the desktop and open the overlay on it, in window-capture mode
    /// if `pick_window`. A failure at any step is reported in a popup.
    fn trigger_capture(&mut self, event_loop: &ActiveEventLoop, pick_window: bool) {
        if self.overlay_window.is_some() {
            tracing::info!("Overlay already open, ignoring capture trigger");
            return;
        }
        let Some(screenshot) = self.grab_desktop(event_loop) else {
            return;
        };
        tracing::info!(
            "Captured screen: {}x{} at ({}, {})",
            screenshot.width,
            screenshot.height,
            screenshot.x_offset,
            screenshot.y_offset
        );
        // The windows on screen now, front to back. The overlay itself is
        // not among them: it is created below, and is screen-mark's own.
        let windows = pick_window.then(|| {
            let rects = window_detect::enumerate_window_rects();
            tracing::info!("Enumerated {} windows", rects.len());
            rects
        });

        let mut overlay = OverlayState::new(screenshot, &self.config);
        overlay.window_pick = windows;
        self.show_overlay(event_loop, overlay);
    }

    /// Open the overlay window over the area `overlay` covers.
    fn show_overlay(&mut self, event_loop: &ActiveEventLoop, overlay: OverlayState) {
        let screenshot = &overlay.screenshot;
        let (w, h) = (screenshot.width, screenshot.height);
        let attrs = WindowAttributes::default()
            .with_position(winit::dpi::PhysicalPosition::new(
                screenshot.x_offset,
                screenshot.y_offset,
            ))
            .with_inner_size(winit::dpi::PhysicalSize::new(w, h))
            .with_decorations(false)
            .with_visible(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_title("screen-mark");
        let opened = open_window(event_loop, attrs).and_then(|(window, surface)| {
            let pixmap = tiny_skia::Pixmap::new(w, h)
                .ok_or_else(|| format!("no memory for a {w} \u{00d7} {h} image"))?;
            Ok((window, surface, pixmap))
        });
        let (window, surface, pixmap) = match opened {
            Ok(parts) => parts,
            Err(e) => {
                self.report_failure(event_loop, "Couldn't open the capture overlay", e);
                return;
            }
        };

        // Made before the window is first shown (see `access::Bridge`).
        self.overlay_access = Some(Bridge::new(event_loop, &window, self.waker()));
        self.state = AppState::Capturing(Box::new(overlay));
        self.surface = Some(surface);
        self.pixmap = Some(pixmap);
        // A new pixmap and a new window buffer: nothing to build on yet.
        self.frame = Frame::default();
        self.present_all = true;
        self.surface_size = (0, 0);
        window.request_redraw();
        self.overlay_window = Some(window);
        self.needs_redraw = true;
    }

    /// Count down `secs` seconds on screen, then start `kind`.
    fn start_countdown(&mut self, event_loop: &ActiveEventLoop, secs: u32, kind: CaptureKind) {
        if self.overlay_window.is_some() || self.countdown.is_some() {
            tracing::info!("A capture is already in progress, ignoring the countdown");
            return;
        }
        tracing::info!("Capturing in {secs} seconds...");
        self.toast = None;
        self.countdown_kind = kind;
        // The countdown reaching 0 schedules the capture.
        self.countdown = Countdown::start(event_loop, secs);
        if self.countdown.is_none() {
            // No window to count down in: wait anyway.
            let when = Instant::now() + Duration::from_secs(secs.into());
            self.capture_at.push_back((when, kind));
        }
    }

    /// `capture --clipboard` / `--save` once its countdown is over.
    fn do_cli_grab(&mut self, _event_loop: &ActiveEventLoop) {
        let Some((clipboard, save)) = self.cli_grab.take() else {
            return;
        };
        self.exit_code = match grab_screens() {
            Ok(screen) => cli_output(&screen, clipboard, save.as_deref()),
            Err(e) => {
                eprintln!("Capture failed: {e}");
                1
            }
        };
    }

    /// Ask for an image file to mark up. The dialog runs on a worker
    /// thread; the image arrives through `opened_rx`.
    fn pick_image_to_edit(&mut self) {
        let tx = self.opened_tx.clone();
        let wake = self.waker();
        std::thread::spawn(move || {
            let picked = rfd::FileDialog::new()
                .set_title("Mark up an image")
                .add_filter("Images", &["png", "jpg", "jpeg", "webp", "bmp"])
                .pick_file();
            if let Some(path) = picked {
                let opened = screen_mark::editor::open_file(&path)
                    .map(|image| (image, screen_mark::editor::suggested_name(&path)));
                let _ = tx.send(opened);
                wake();
            }
        });
    }

    /// Read a capture from History on a worker thread, to mark up again.
    fn edit_from_history(&mut self, path: std::path::PathBuf) {
        let tx = self.opened_tx.clone();
        let wake = self.waker();
        std::thread::spawn(move || {
            let _ = tx.send(screen_mark::history::load_editable(&path).map(|image| (image, None)));
            wake();
        });
    }

    /// Open the overlay on `image` to mark it up: the image in the middle
    /// of the monitor under the mouse, selected (see `editor`).
    fn open_editor(
        &mut self,
        event_loop: &ActiveEventLoop,
        image: screen_mark::history::Editable,
        name: Option<String>,
    ) {
        if self.overlay_window.is_some() || self.countdown.is_some() {
            let spec = Spec::new(
                Kind::Info,
                "Finish the capture that is open first",
                "Then mark up the image",
            );
            self.feedback(event_loop, spec, None);
            return;
        }
        self.toast = None;
        let monitor = pointer_monitor().or_else(|| display::monitors().first().copied());
        let area = monitor
            .map(|m| m.area)
            .unwrap_or(display::Rect::new(0, 0, 1280, 800));
        let layout =
            screen_mark::editor::lay_out(&image, area.w, area.h, screen_mark::theme::bg_2());
        tracing::info!(
            "Marking up a {}x{} image with {} annotations (shown at {:.0}%)",
            image.width,
            image.height,
            image.annotations.len(),
            layout.scale * 100.0
        );
        let shot = capture::CapturedScreen {
            pixels: layout.pixels,
            width: layout.width,
            height: layout.height,
            x_offset: area.x,
            y_offset: area.y,
            scale_factor: 1.0,
        };
        let mut overlay = OverlayState::new(shot, &self.config);
        overlay.canvas = Some(layout.canvas);
        overlay.selection = Some(layout.canvas);
        overlay.annotations = layout.annotations;
        overlay.sync_step_counter();
        overlay.finish_on_release = false;
        overlay.source_name = name;
        if layout.scale < 1.0 {
            overlay.show_toast(
                format!(
                    "Shown at {:.0}% to fit the screen, and copied and saved at that size",
                    layout.scale * 100.0
                ),
                5000,
            );
        }
        self.show_overlay(event_loop, overlay);
    }

    /// The selection, finished: flattened pixels, their size, and what
    /// keeps the History entry editable.
    fn take_selection(
        &self,
    ) -> Option<(Vec<u8>, u32, u32, Option<screen_mark::history::Editable>)> {
        let AppState::Capturing(ref overlay) = self.state else {
            return None;
        };
        let (pixels, w, h) = overlay.flattened_selection()?;
        Some((pixels, w, h, overlay.editable_selection()))
    }

    /// Hand the capture to the `[send] command`.
    fn do_send(&mut self) {
        let command = self.config.send.command.trim().to_string();
        if command.is_empty() {
            if let AppState::Capturing(ref mut o) = self.state {
                o.show_toast("Set [send] command in config.toml first".into(), 4000);
            }
            self.needs_redraw = true;
            return;
        }
        let Some((pixels, w, h, markup)) = self.take_selection() else {
            return;
        };
        self.remember_region();
        if self.config.general.history_enabled {
            screen_mark::actions::keep_markup(
                screen_mark::history::save_to_history(&pixels, w, h),
                markup.as_ref(),
            );
        }
        let name = screen_mark::send::name(&self.config.send);
        self.close_overlay();
        self.spawn_job(None, move || {
            Some(screen_mark::send::send_capture(
                &pixels, w, h, &command, &name,
            ))
        });
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

        let attrs = WindowAttributes::default()
            .with_title("screen-mark Settings")
            .with_inner_size(winit::dpi::LogicalSize::new(
                screen_mark::settings_ui::WIN_W,
                screen_mark::settings_ui::WIN_H,
            ))
            .with_resizable(false)
            .with_decorations(true)
            .with_visible(!ACCESS_NEEDS_HIDDEN_START)
            .with_window_icon(window_icon());
        let (window, surface) = match open_window(event_loop, attrs) {
            Ok(parts) => parts,
            Err(e) => {
                self.report_failure(event_loop, "Couldn't open Settings", e);
                return;
            }
        };
        self.settings_access = Some(Bridge::new(event_loop, &window, self.waker()));

        let statuses = KeyId::GLOBALS
            .into_iter()
            .map(|id| (id, self.hotkeys.status(id)))
            .collect();
        let mut view = SettingsView::new(Config::load(), statuses);
        if let Some(t) = tab {
            view.open_tab(t);
        }
        self.settings_window = Some(SettingsWindow::new(window, surface, view));
        self.render_settings();
        if let Some(sw) = &self.settings_window {
            sw.window.set_visible(true);
        }
        tracing::info!("Settings window opened");
    }

    /// Draw Settings and tell assistive technology what it shows now.
    fn render_settings(&mut self) {
        let Some(sw) = self.settings_window.as_mut() else {
            return;
        };
        sw.render();
        if let Some(access) = self.settings_access.as_mut() {
            access.update(sw.view.snapshot(), sw.window.scale_factor());
        }
    }

    /// Carry out what Settings asked for after an input.
    fn settings_outcome(&mut self, event_loop: &ActiveEventLoop, outcome: Outcome) {
        match outcome {
            Outcome::Close { saved } => self.close_settings(event_loop, saved),
            Outcome::Redraw => {
                if let Some(ref sw) = self.settings_window {
                    sw.window.request_redraw();
                }
            }
            Outcome::Nothing => {}
        }
    }

    /// A callback that wakes the event loop from any thread.
    fn waker(&self) -> impl Fn() + Send + 'static {
        let proxy = self.proxy.clone();
        move || {
            if let Some(proxy) = &proxy {
                let _ = proxy.send_event(());
            }
        }
    }

    fn close_settings(&mut self, event_loop: &ActiveEventLoop, saved: bool) {
        self.settings_access = None;
        self.settings_window = None;
        if saved {
            self.config = Config::load();
            self.keymap = KeyMap::from_report(&self.config.key_report());
            screen_mark::theme::set_mode(self.config.theme_mode());
            export::set_file_name(&self.config.general.file_name);
            if !self.cli_only {
                // Resume first (no-op if not suspended), then apply the new keys.
                self.hotkeys.resume();
                self.apply_hotkeys(event_loop, false);
                // The automatic update check may have been switched.
                self.update_check_at = match self.config.updates.auto_check {
                    true => self
                        .update_check_at
                        .or(Some(Instant::now() + FIRST_UPDATE_CHECK)),
                    false => None,
                };
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
        if let Some(ref hw) = self.history_window {
            hw.window.focus_window();
            return;
        }

        let attrs = WindowAttributes::default()
            .with_title("screen-mark \u{2014} History")
            .with_inner_size(winit::dpi::LogicalSize::new(
                screen_mark::history_ui::WIN_W,
                screen_mark::history_ui::WIN_H,
            ))
            .with_resizable(false)
            .with_decorations(true)
            .with_visible(!ACCESS_NEEDS_HIDDEN_START)
            .with_window_icon(window_icon());
        let (window, surface) = match open_window(event_loop, attrs) {
            Ok(parts) => parts,
            Err(e) => {
                self.report_failure(event_loop, "Couldn't open History", e);
                return;
            }
        };
        self.history_access = Some(Bridge::new(event_loop, &window, self.waker()));

        self.history_window = Some(HistoryWindow::new(window, surface));
        self.render_history();
        if let Some(hw) = &self.history_window {
            hw.window.set_visible(true);
        }
        tracing::info!("History window opened");
    }

    /// Draw History and tell assistive technology what it shows now.
    fn render_history(&mut self) {
        let Some(hw) = self.history_window.as_mut() else {
            return;
        };
        hw.render();
        if let Some(access) = self.history_access.as_mut() {
            access.update(hw.view.snapshot(), hw.window.scale_factor());
        }
    }

    /// Carry out what History asked for after an input.
    fn history_outcome(&mut self, outcome: HistoryOutcome) {
        match outcome {
            HistoryOutcome::Nothing => {}
            HistoryOutcome::Close => self.close_history(),
            _ => {
                if let Some(ref hw) = self.history_window {
                    hw.window.request_redraw();
                }
            }
        }
        match outcome {
            // Decoded and copied off the UI thread, and reported like any
            // other copy.
            HistoryOutcome::Copy(path) => self.spawn_job(None, move || {
                Some(screen_mark::history_ui::copy_capture(&path))
            }),
            HistoryOutcome::Edit(path) => self.edit_from_history(path),
            // The dialog waits on a worker thread, like Save in the editor.
            HistoryOutcome::SaveAs(path) => self.spawn_job(None, move || {
                screen_mark::history_ui::save_capture_as(&path)
            }),
            HistoryOutcome::Show(path) => toast::show_in_folder(&path),
            HistoryOutcome::Nothing | HistoryOutcome::Close | HistoryOutcome::Redraw => {}
        }
    }

    fn close_history(&mut self) {
        self.history_access = None;
        self.history_window = None;
        tracing::info!("History window closed");
    }

    fn close_overlay(&mut self) {
        self.overlay_access = None;
        self.surface = None;
        self.pixmap = None;
        self.overlay_window = None;
        self.state = AppState::Idle;
        self.modifiers = ModifiersState::empty();
        self.ime_on = false;
        // A CJK font loaded for a text annotation is not needed any more.
        screen_mark::font::release_fallbacks();
        tracing::info!("Overlay closed");
    }

    fn do_copy(&mut self) {
        if let Some((pixels, w, h, markup)) = self.take_selection() {
            self.remember_region();
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
                    markup.as_ref(),
                ))
            });
        }
    }

    fn do_save(&mut self) {
        let Some((pixels, w, h, markup)) = self.take_selection() else {
            return;
        };
        let suggested = match &self.state {
            AppState::Capturing(o) => o.source_name.clone(),
            AppState::Idle => None,
        };
        self.remember_region();
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
                    markup.as_ref(),
                ))
            });
            return;
        }
        self.spawn_job(None, move || {
            let thumb = toast::make_thumb(&pixels, w, h, 192, 120);
            match export::save_to_file(pixels, w, h, None, suggested.as_deref()) {
                Ok(Some(path)) => {
                    let path = std::path::PathBuf::from(path);
                    if history {
                        screen_mark::actions::keep_markup(
                            screen_mark::history::save_to_history_from_file(&path),
                            markup.as_ref(),
                        );
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
        if let AppState::Capturing(ref mut o) = self.state {
            o.upload_confirmed = false; // reset
        }
        if let Some((pixels, w, h, markup)) = self.take_selection() {
            self.remember_region();
            if self.config.general.history_enabled {
                screen_mark::actions::keep_markup(
                    screen_mark::history::save_to_history(&pixels, w, h),
                    markup.as_ref(),
                );
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
                            let _ = export::copy_text(&url);
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
            self.remember_region();
            self.close_overlay();
            self.spawn_job(None, move || {
                Some(match screen_mark::ocr::extract_text(&cropped, w, h) {
                    Ok(text) => {
                        let _ = export::copy_text(&text);
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
        self.remember_region();
        let markup = self.take_selection().and_then(|(_, _, _, m)| m);
        if let AppState::Capturing(ref overlay) = self.state {
            if let Some((pixels, w, h)) = overlay.flattened_selection() {
                if self.config.general.history_enabled {
                    screen_mark::actions::keep_markup(
                        screen_mark::history::save_to_history(&pixels, w, h),
                        markup.as_ref(),
                    );
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
                Item::LastRegion => {
                    self.request_capture_in(event_loop, CaptureKind::LastRegion, menu_gone)
                }
                Item::Window => self.request_capture_in(event_loop, CaptureKind::Window, menu_gone),
                Item::Delay(secs) => self.start_countdown(event_loop, secs, CaptureKind::Region),
                Item::EditFile => self.pick_image_to_edit(),
                Item::EditClipboard => match screen_mark::editor::from_clipboard() {
                    Ok(image) => self.open_editor(event_loop, image, None),
                    Err(e) => {
                        let spec = Spec::new(Kind::Info, "Nothing to mark up", e);
                        self.feedback(event_loop, spec, None);
                    }
                },
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
                        Err(e) => self.report_failure(event_loop, "Couldn't change auto-start", e),
                    }
                }
                Item::Settings => self.open_settings(event_loop, None),
                Item::Update => self.update_clicked(event_loop),
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
                    // Between moving the old version aside and the new one
                    // being in place there is no screen-mark.exe to start.
                    // Say so once; a second Quit goes through.
                    let installing = matches!(lock(&self.update).stage, UpdateStage::Installing(_));
                    if installing && !self.quit_warned {
                        self.quit_warned = true;
                        let spec = Spec::new(
                            Kind::Info,
                            "An update is being installed",
                            "Quitting now can leave it half done. Quit again to do it anyway.",
                        );
                        self.popup(event_loop, spec, None);
                        continue;
                    }
                    tracing::info!("Quit requested");
                    event_loop.exit();
                }
            }
        }
    }

    /// Draw the overlay and give the window what changed. Most frames
    /// differ from the last one in a few small areas (the pointer moved),
    /// so only those are drawn again, converted and copied to the screen;
    /// a frame costs what changed, not the size of the desktop.
    fn render(&mut self) {
        if !self.needs_redraw {
            return;
        }
        let AppState::Capturing(overlay) = &mut self.state else {
            return;
        };
        let (Some(pixmap), Some(surface), Some(window)) = (
            self.pixmap.as_mut(),
            self.surface.as_mut(),
            self.overlay_window.as_ref(),
        ) else {
            return;
        };

        let mut changed = render_frame(overlay, pixmap, &mut self.frame);
        // The frame is ahead of the window from here on. If it can't be
        // shown now, the next one that can is shown whole.
        let show_all = std::mem::replace(&mut self.present_all, true);

        let size = window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return;
        };
        if self.surface_size != (size.width, size.height) {
            if let Err(e) = surface.resize(w, h) {
                tracing::error!("Surface resize failed: {e}");
                return;
            }
            self.surface_size = (size.width, size.height);
        }
        let mut buffer = match surface.buffer_mut() {
            Ok(b) => b,
            Err(e) => {
                tracing::error!("Failed to get surface buffer: {e}");
                return;
            }
        };

        // The window's buffer holds the last frame only if that frame was
        // presented from it. The first frame goes whole, and so does any
        // frame the system asked for (the window was uncovered or shown).
        if show_all || buffer.age() != 1 {
            changed = vec![Area::new(0, 0, size.width as i32, size.height as i32)];
        }
        let told = to_window_pixels(pixmap, &mut buffer, size.width as usize, &changed);
        if !told.is_empty() {
            if let Err(e) = buffer.present_with_damage(&told) {
                tracing::error!("Buffer present failed: {e}");
                return;
            }
        }
        self.present_all = false;

        // Show window after first frame is rendered (avoids white flash)
        if !window.is_visible().unwrap_or(true) {
            window.set_visible(true);
        }

        self.needs_redraw = false;
        if let (Some(access), AppState::Capturing(overlay)) =
            (self.overlay_access.as_mut(), &self.state)
        {
            // The overlay's pixels are the window's: no scaling.
            access.update(overlay.snapshot(), 1.0);
        }
    }

    /// Carry out what the overlay asked for after an input (see
    /// `overlay::input`, which holds the logic).
    fn apply_outcome(&mut self, event_loop: &ActiveEventLoop, outcome: OverlayOutcome) {
        match outcome {
            OverlayOutcome::Nothing => {}
            OverlayOutcome::Redraw => self.needs_redraw = true,
            OverlayOutcome::Close => self.close_overlay(),
            OverlayOutcome::Copy => self.do_copy(),
            OverlayOutcome::Save => self.do_save(),
            OverlayOutcome::Pin => self.do_pin(event_loop),
            OverlayOutcome::Ocr => self.do_ocr(),
            OverlayOutcome::Upload => self.do_upload(),
            OverlayOutcome::Send => self.do_send(),
            OverlayOutcome::PickColor(preset) => self.pick_color(preset),
            OverlayOutcome::MovePointer(to) => {
                // An arrow key moved the pointer by a pixel. Tell the
                // overlay right away; the system's own move event follows.
                if let Some(window) = &self.overlay_window {
                    let at = winit::dpi::PhysicalPosition::new(to.x as f64, to.y as f64);
                    if let Err(e) = window.set_cursor_position(at) {
                        tracing::debug!("Couldn't move the pointer: {e}");
                    }
                }
                if let AppState::Capturing(overlay) = &mut self.state {
                    overlay.on_cursor_moved(to);
                    self.needs_redraw = true;
                }
            }
            OverlayOutcome::Paste => {
                let text = arboard::Clipboard::new()
                    .and_then(|mut clipboard| clipboard.get_text())
                    .unwrap_or_default();
                if let AppState::Capturing(overlay) = &mut self.state {
                    if overlay.insert_text(&text) == OverlayOutcome::Redraw {
                        self.needs_redraw = true;
                    }
                }
            }
        }
        self.sync_overlay_window();
    }

    /// Open the system colour picker, starting from a preset. The overlay
    /// is hidden while the dialog is up: it is always-on-top and would
    /// cover it.
    fn pick_color(&mut self, preset: usize) {
        let current = Color::presets()
            .get(preset)
            .copied()
            .unwrap_or_else(Color::red);
        if let Some(window) = &self.overlay_window {
            window.set_visible(false);
        }
        let picked = screen_mark::color_picker::pick_color(&current);
        if let Some(window) = &self.overlay_window {
            window.set_visible(true);
        }
        if let (Some(color), AppState::Capturing(overlay)) = (picked, &mut self.state) {
            overlay.apply_picked_color(color);
            self.needs_redraw = true;
        }
    }

    /// Keep the overlay window in step with the overlay: its cursor,
    /// whether it takes input-method text, and a redraw if one is due.
    fn sync_overlay_window(&mut self) {
        let (AppState::Capturing(overlay), Some(window)) = (&self.state, &self.overlay_window)
        else {
            return;
        };
        window.set_cursor(overlay.cursor_icon());
        // An input method composes text only while a text annotation is
        // being typed; the rest of the time keys are tool shortcuts.
        let typing = overlay.text_input_active;
        if typing != self.ime_on {
            window.set_ime_allowed(typing);
            self.ime_on = typing;
        }
        if let Some((x, y, height)) = overlay.text_caret_rect() {
            // Where the input method puts its candidate list.
            window.set_ime_cursor_area(
                winit::dpi::PhysicalPosition::new(x as i32, y as i32),
                winit::dpi::PhysicalSize::new(1u32, height.ceil().max(1.0) as u32),
            );
        }
        // No `request_redraw` here: `about_to_wait` draws the frame, at most
        // sixty times a second. A redraw request reaching the window is
        // then always the system's, and gets the whole frame.
    }
}

/// Create a window and the surface screen-mark draws it with.
fn open_window(
    event_loop: &ActiveEventLoop,
    attrs: WindowAttributes,
) -> Result<(Arc<Window>, Surface), String> {
    let window = event_loop
        .create_window(attrs)
        .map(Arc::new)
        .map_err(|e| format!("window: {e}"))?;
    let context = softbuffer::Context::new(Arc::clone(&window))
        .map_err(|e| format!("drawing context: {e}"))?;
    let surface = softbuffer::Surface::new(&context, Arc::clone(&window))
        .map_err(|e| format!("drawing surface: {e}"))?;
    Ok((window, surface))
}

/// The app icon, for a window's title bar and taskbar button.
fn window_icon() -> Option<winit::window::Icon> {
    let image = image::load_from_memory(include_bytes!("../assets/icon.png"))
        .ok()?
        .to_rgba8();
    let (w, h) = image.dimensions();
    winit::window::Icon::from_rgba(image.into_raw(), w, h).ok()
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
            // After an update, say that instead of "screen-mark is running".
            let updated_from = self.updated_from.take();
            self.apply_hotkeys(event_loop, updated_from.is_none());
            if let (Some(old), true) = (updated_from, self.toast.is_none()) {
                let spec = Spec::new(
                    Kind::Success,
                    format!("screen-mark {} is installed", Version::running()),
                    format!("Updated from {old}"),
                );
                self.feedback(event_loop, spec, None);
            }
            if self.config.updates.auto_check {
                self.update_check_at = Some(Instant::now() + FIRST_UPDATE_CHECK);
            }
        }

        match self.startup.take() {
            None => {}
            Some(Startup::Overlay(0)) => self.trigger_capture(event_loop, false),
            Some(Startup::Overlay(secs)) => {
                self.start_countdown(event_loop, secs, CaptureKind::Region)
            }
            Some(Startup::Grab(secs)) => {
                self.start_countdown(event_loop, secs, CaptureKind::CliGrab)
            }
            Some(Startup::Edit(image, name)) => self.open_editor(event_loop, *image, name),
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
            match self.pinned_windows[pin_idx].handle_event(&event) {
                PinEvent::Close => {
                    // PinnedWindow::drop removes the backing temp file.
                    self.pinned_windows.remove(pin_idx);
                    tracing::info!("Pinned window closed");
                }
                PinEvent::Copy => {
                    // Reported like any other copy, so the popup follows
                    // the `notifications` setting.
                    let (pixels, w, h) = self.pinned_windows[pin_idx].image();
                    self.spawn_job(None, move || {
                        let copied = export::copy_to_clipboard(&pixels, w, h);
                        Some(screen_mark::actions::describe(None, Some(copied), w, h))
                    });
                }
                PinEvent::Nothing => {}
            }
            return;
        }

        // Check if event is for the history window
        if let Some(hw) = self
            .history_window
            .as_mut()
            .filter(|hw| hw.window.id() == _window_id)
        {
            let outcome = match event {
                WindowEvent::CloseRequested => HistoryOutcome::Close,
                WindowEvent::RedrawRequested => {
                    hw.render();
                    if let Some(access) = self.history_access.as_mut() {
                        access.update(hw.view.snapshot(), hw.window.scale_factor());
                    }
                    HistoryOutcome::Nothing
                }
                WindowEvent::CursorMoved { position, .. } => {
                    if hw.on_cursor_moved(position.x as f32, position.y as f32) {
                        HistoryOutcome::Redraw
                    } else {
                        HistoryOutcome::Nothing
                    }
                }
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    button: WinitMouseButton::Left,
                    ..
                } => hw.on_click(),
                WindowEvent::MouseWheel { delta, .. } => {
                    let lines = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y,
                        MouseScrollDelta::PixelDelta(pos) => pos.y as f32 / 20.0,
                    };
                    if hw.view.scroll(lines) {
                        HistoryOutcome::Redraw
                    } else {
                        HistoryOutcome::Nothing
                    }
                }
                WindowEvent::ModifiersChanged(mods) => {
                    self.history_modifiers = mods.state();
                    HistoryOutcome::Nothing
                }
                WindowEvent::KeyboardInput { ref event, .. }
                    if event.state == ElementState::Pressed =>
                {
                    let key = keys::key_from_winit(&event.logical_key, &event.physical_key);
                    // A held Enter must not copy over and over.
                    let presses = matches!(
                        key,
                        Some(keys::Key::Named(keys::Named::Enter | keys::Named::Space))
                    );
                    if event.repeat && presses {
                        HistoryOutcome::Nothing
                    } else {
                        hw.view.key(KeyPress {
                            mods: keys::mods_from_winit(self.history_modifiers),
                            key,
                            pressed: true,
                        })
                    }
                }
                _ => HistoryOutcome::Nothing,
            };
            self.history_outcome(outcome);
            return;
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
                                Click::Update => self.update_clicked(_event_loop),
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
                        self.render_settings();
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
                        // Skip keys that can't be bound (media, IME, ...) and
                        // auto-repeat, except that a held Tab or arrow key
                        // keeps moving through the controls.
                        let unbindable =
                            key.is_none() && !keys::is_modifier_key(&event.logical_key);
                        let listening = self
                            .settings_window
                            .as_ref()
                            .is_some_and(|sw| sw.view.editing().is_some());
                        let moves_focus = matches!(
                            key,
                            Some(keys::Key::Named(
                                keys::Named::Tab
                                    | keys::Named::Up
                                    | keys::Named::Down
                                    | keys::Named::Left
                                    | keys::Named::Right
                                    | keys::Named::PageUp
                                    | keys::Named::PageDown
                            ))
                        );
                        if unbindable || (event.repeat && (listening || !moves_focus)) {
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
                self.settings_outcome(_event_loop, outcome);
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

        // What's left is the overlay, and only while capturing.
        let AppState::Capturing(overlay) = &mut self.state else {
            return;
        };
        // Between two pointer moves little changes, and the frame is
        // patched. After anything else (a click, a key, the wheel) it is
        // drawn from scratch, so a patch that missed something could never
        // outlive the next click.
        let pointer_moved = matches!(event, WindowEvent::CursorMoved { .. });
        let outcome = match event {
            WindowEvent::CloseRequested => OverlayOutcome::Close,
            WindowEvent::RedrawRequested => {
                // The system wants the window painted (it was shown or
                // uncovered): all of it, not just what changed.
                self.present_all = true;
                self.needs_redraw = true;
                self.render();
                return;
            }
            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = mods.state();
                // Shift constrains a drag in progress.
                overlay.set_modifiers(keys::mods_from_winit(self.modifiers))
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                let input = KeyInput::from_winit(
                    &event.logical_key,
                    &event.physical_key,
                    keys::mods_from_winit(self.modifiers),
                );
                overlay.on_key(&input, &self.keymap)
            }
            // Text from an input method (IME), while a text annotation is
            // being typed: what is being composed, then the final text.
            WindowEvent::Ime(Ime::Preedit(text, _)) => overlay.set_preedit(&text),
            WindowEvent::Ime(Ime::Commit(text)) => overlay.insert_text(&text),
            WindowEvent::CursorMoved { position, .. } => {
                overlay.on_cursor_moved(Point::new(position.x as f32, position.y as f32))
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: WinitMouseButton::Left,
                ..
            } => overlay.on_mouse_down(),
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: WinitMouseButton::Left,
                ..
            } => overlay.on_mouse_up(),
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: WinitMouseButton::Right,
                ..
            } => overlay.on_right_click(),
            WindowEvent::MouseWheel { delta, .. } => overlay.on_scroll(match delta {
                MouseScrollDelta::LineDelta(_, y) => y,
                MouseScrollDelta::PixelDelta(pos) => pos.y as f32 / 20.0,
            }),
            _ => return,
        };
        if !pointer_moved {
            self.frame.invalidate();
        }
        self.apply_outcome(_event_loop, outcome);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.process_tray_events(event_loop);

        // What a screen reader asked the overlay to do.
        let asked = self
            .overlay_access
            .as_mut()
            .map(|access| access.requests())
            .unwrap_or_default();
        for request in asked {
            let AppState::Capturing(overlay) = &mut self.state else {
                break;
            };
            let outcome = overlay.request(request);
            self.apply_outcome(event_loop, outcome);
        }

        // An image read on a worker thread, to mark up.
        while let Ok(opened) = self.opened_rx.try_recv() {
            match opened {
                Ok((image, name)) => self.open_editor(event_loop, image, name),
                Err(e) => self.report_failure(event_loop, "Couldn't open the image", e),
            }
        }

        // What a screen reader asked Settings or History to do.
        let asked = self
            .settings_access
            .as_mut()
            .map(|access| access.requests())
            .unwrap_or_default();
        for request in asked {
            let Some(sw) = self.settings_window.as_mut() else {
                break;
            };
            let outcome = sw.view.request(request);
            self.settings_outcome(event_loop, outcome);
        }
        let asked = self
            .history_access
            .as_mut()
            .map(|access| access.requests())
            .unwrap_or_default();
        for request in asked {
            let Some(hw) = self.history_window.as_mut() else {
                break;
            };
            let outcome = hw.view.request(request);
            self.history_outcome(outcome);
        }

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
                Some(KeyId::LastRegion) => {
                    tracing::info!("Last-region hotkey pressed");
                    self.request_capture(event_loop, CaptureKind::LastRegion);
                }
                _ => {}
            }
        }

        // Results from worker threads.
        let mut finished = false;
        while let Ok((spec, monitor)) = self.jobs_rx.try_recv() {
            self.jobs_pending = self.jobs_pending.saturating_sub(1);
            finished = true;
            if let Some(spec) = spec {
                self.feedback(event_loop, spec, monitor);
            }
        }
        if finished {
            // It may have been an update check or install.
            self.update_progress(event_loop);
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

        // `[updates] auto_check`: look for a new release now and then.
        if let Some(when) = self.update_check_at {
            let now = Instant::now();
            if now < when {
                wake_at(when);
            } else {
                self.update_check_at = Some(now + UPDATE_CHECK_EVERY);
                if matches!(lock(&self.update).stage, UpdateStage::Unknown) {
                    self.check_for_updates(false);
                }
            }
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
                    .push_back((now + Duration::from_millis(300), self.countdown_kind));
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
                    self.run_capture(event_loop, kind);
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
            && self.startup.is_none()
            && self.countdown.is_none()
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

        // With no window of ours left that draws text, a system font that
        // was read for one (a CJK file name in a popup) can go.
        if self.toast.is_none()
            && self.overlay_window.is_none()
            && self.settings_window.is_none()
            && self.history_window.is_none()
        {
            screen_mark::font::release_fallbacks();
        }

        match wake {
            Some(t) => event_loop.set_control_flow(ControlFlow::WaitUntil(t)),
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }
}

/// Single-instance enforcement: only one tray app at a time. False when
/// another one is running. With `wait`, the other one is the version an
/// update just replaced, on its way out: give it time to go.
#[cfg(target_os = "windows")]
fn only_instance(wait: bool) -> bool {
    use windows::core::w;
    use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;

    let give_up = Instant::now() + Duration::from_secs(20);
    loop {
        unsafe {
            let Ok(m) = CreateMutexW(None, false, w!("screen-mark.SingleInstance")) else {
                return true;
            };
            if GetLastError() != ERROR_ALREADY_EXISTS {
                // Mutex handle is Copy — kernel keeps it alive for the process lifetime
                let _ = m;
                return true;
            }
            let _ = CloseHandle(m);
        }
        if !wait || Instant::now() >= give_up {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The event loop. On a Wayland session screen-mark's windows run under
/// XWayland when it is there: Wayland lets no window place itself or stay
/// on top, and the overlay has to cover the screen it shows.
fn new_event_loop() -> Result<EventLoop<()>, winit::error::EventLoopError> {
    #[cfg(target_os = "linux")]
    if std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty()) {
        use winit::platform::x11::EventLoopBuilderExtX11;
        return EventLoop::builder().with_x11().build();
    }
    EventLoop::new()
}

fn run_tray_app(config: Config, updated_from: Option<String>) {
    // The tray app has no console: a panic shows a dialog naming the log.
    screen_mark::logging::show_dialog_on_panic(true);
    #[cfg(target_os = "windows")]
    if !only_instance(updated_from.is_some()) {
        screen_mark::notify::system(
            "screen-mark",
            "screen-mark is already running in the system tray",
        );
        return;
    }
    // This start is the proof the new version runs: the one an update
    // kept beside it can go.
    if let Ok(exe) = std::env::current_exe() {
        update::remove_leftovers(&exe);
    }

    let event_loop = match new_event_loop() {
        Ok(el) => el,
        Err(e) => {
            screen_mark::installer::show_error(&format!("Failed to start: {e}"));
            std::process::exit(1);
        }
    };
    screen_mark::notify::register_app_id();
    let mut app = App::new(config);
    app.updated_from = updated_from;
    app.set_proxy(event_loop.create_proxy());
    if let Err(e) = event_loop.run_app(&mut app) {
        screen_mark::installer::show_error(&format!("Event loop error: {e}"));
        std::process::exit(1);
    }
}

/// A command-line run that needs windows (the overlay, a countdown): an
/// event loop with no tray and no hotkeys, that ends when its job is done.
/// Returns the exit code.
fn run_cli_app(config: Config, startup: Startup, grab: Option<(bool, Option<String>)>) -> i32 {
    // Nobody reads stderr while the overlay is up; show a dialog instead.
    screen_mark::logging::show_dialog_on_panic(true);
    let event_loop = match new_event_loop() {
        Ok(el) => el,
        Err(e) => {
            screen_mark::installer::show_error(&format!("Failed to start: {e}"));
            return 1;
        }
    };
    let mut app = App::new(config);
    app.set_proxy(event_loop.create_proxy());
    app.startup = Some(startup);
    app.cli_grab = grab;
    app.cli_only = true;
    if let Err(e) = event_loop.run_app(&mut app) {
        screen_mark::installer::show_error(&format!("Event loop error: {e}"));
        return 1;
    }
    app.exit_code
}

/// Grab the whole virtual desktop as one image.
fn grab_screens() -> Result<capture::CapturedScreen, String> {
    capture::create_capturer()
        .map_err(|e| e.to_string())
        .and_then(|c| c.capture_all_screens().map_err(|e| e.to_string()))
        .and_then(|s| s.into_iter().next().ok_or_else(|| "no screen".to_string()))
}

/// What `capture --clipboard` / `--save` does with a capture. Returns the
/// exit code.
fn cli_output(screen: &capture::CapturedScreen, clipboard: bool, save: Option<&str>) -> i32 {
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
                return 1;
            }
        }
    }
    if let Some(path) = save {
        let written = export::write_image(
            std::path::Path::new(path),
            screen.pixels.clone(),
            screen.width,
            screen.height,
        );
        match written {
            Ok(()) => println!("Saved to {}", path),
            Err(e) => {
                eprintln!("Save error: {}", e);
                return 1;
            }
        }
    }
    0
}

fn run_cli_capture(clipboard: bool, save: Option<String>, delay: u64) {
    let delay = u32::try_from(delay).unwrap_or(u32::MAX);
    let config = Config::load();
    export::set_file_name(&config.general.file_name);
    if delay > 0 {
        // The countdown is on screen too: the terminal may well be behind
        // what is being captured.
        println!("Capturing in {delay} seconds...");
    }
    // Neither --clipboard nor --save: the interactive overlay.
    if !clipboard && save.is_none() {
        tracing::info!("Opening interactive capture...");
        let code = run_cli_app(config, Startup::Overlay(delay), None);
        std::process::exit(code);
    }
    if delay > 0 {
        let code = run_cli_app(config, Startup::Grab(delay), Some((clipboard, save)));
        std::process::exit(code);
    }

    // Non-interactive: capture full screen directly
    let screen = match grab_screens() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Capture failed: {}", e);
            std::process::exit(1);
        }
    };
    let code = cli_output(&screen, clipboard, save.as_deref());
    if code != 0 {
        std::process::exit(code);
    }
}

/// `screen-mark edit [FILE]`: mark up an image file, or the image on the
/// clipboard.
fn run_cli_edit(file: Option<std::path::PathBuf>) {
    let opened = match &file {
        Some(path) => screen_mark::editor::open_file(path)
            .map(|image| (image, screen_mark::editor::suggested_name(path))),
        None => screen_mark::editor::from_clipboard().map(|image| (image, None)),
    };
    let (image, name) = match opened {
        Ok(opened) => opened,
        Err(e) => {
            eprintln!("{e}");
            // Started from Explorer ("Open with"), not a terminal: nobody
            // would see the message otherwise.
            if !has_console() {
                screen_mark::installer::show_error(&e);
            }
            std::process::exit(1);
        }
    };
    let config = Config::load();
    export::set_file_name(&config.general.file_name);
    let code = run_cli_app(config, Startup::Edit(Box::new(image), name), None);
    std::process::exit(code);
}

/// Set when this process is attached to a terminal; on Windows only once
/// `AttachConsole` has worked (see `main`).
static HAS_CONSOLE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(!cfg!(target_os = "windows"));

/// Whether this process writes to a terminal (it was started from one).
fn has_console() -> bool {
    HAS_CONSOLE.load(std::sync::atomic::Ordering::Relaxed)
}

/// `screen-mark update`: look for a newer release and, unless `check_only`,
/// install it.
fn run_cli_update(check_only: bool, from: Option<String>) {
    let config = Config::load();
    update::set_proxy(&config.updates.proxy);
    let source = match from {
        Some(place) => Source::from_setting(&place),
        None => Source::from_setting(&config.updates.url),
    };
    let running = Version::running();
    let release = match update::check(&source, running, &update::keys::trusted()) {
        Ok(Check::Available(release)) => release,
        Ok(Check::UpToDate { latest }) if latest == running => {
            println!("screen-mark {running} is up to date.");
            return;
        }
        Ok(Check::UpToDate { latest }) => {
            println!("screen-mark {running} is newer than the newest release, {latest}.");
            return;
        }
        Err(e) => {
            eprintln!("Couldn't check for updates: {e}");
            std::process::exit(1);
        }
    };
    let version = release.version();
    println!("screen-mark {version} is available (this is {running}).");
    if check_only {
        return;
    }

    let installed = update::running_copy().and_then(|(exe, kind)| {
        println!("Installing it...");
        update::install(&release, &exe, kind)
    });
    match installed {
        Ok(()) => println!(
            "screen-mark {version} is installed. A screen-mark that is running \
             keeps the old version until it is restarted."
        ),
        Err(e) => {
            eprintln!("The update wasn't installed: {e}");
            eprintln!("It can be downloaded from {}", release.page());
            std::process::exit(1);
        }
    }
}

fn main() {
    screen_mark::logging::init();
    screen_mark::logging::install_panic_hook();
    tracing::info!("screen-mark {} starting", env!("CARGO_PKG_VERSION"));
    // Set by a screen-mark that updated itself and started this one.
    let updated_from = std::env::var(update::UPDATED_FROM)
        .ok()
        .filter(|old| !old.is_empty());
    std::env::remove_var(update::UPDATED_FROM);

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
        let attached = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) }.is_ok();
        HAS_CONSOLE.store(attached, std::sync::atomic::Ordering::Relaxed);
    }

    match cli.command {
        None => {
            // If not running from the install location, OFFER to install (the
            // installer spawns the installed copy and we exit). Declining runs
            // the app portably from its current location. needs_install() is
            // always false in debug builds and on non-Windows platforms.
            // A copy that was just updated where it is has been asked before.
            if updated_from.is_none()
                && screen_mark::installer::needs_install()
                && screen_mark::installer::confirm_install()
            {
                if let Err(e) = screen_mark::installer::install() {
                    screen_mark::installer::show_error(&format!("Install failed: {e}"));
                    std::process::exit(1);
                }
                return;
            }

            let config = Config::load();
            tracing::info!(
                "Config loaded: region={}, screenshot={}, last_region={}, color={}, thickness={}",
                config.hotkey.region,
                config.hotkey.screenshot,
                config.hotkey.last_region,
                config.general.default_color,
                config.general.default_thickness
            );
            run_tray_app(config, updated_from);
        }
        Some(Commands::Update { check, from }) => run_cli_update(check, from),
        Some(Commands::Capture {
            clipboard,
            save,
            delay,
        }) => {
            run_cli_capture(clipboard, save, delay);
        }
        Some(Commands::Edit { file }) => run_cli_edit(file),
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
