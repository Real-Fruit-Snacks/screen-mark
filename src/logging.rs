//! Where the log goes, and what happens on a panic.
//!
//! The tray app has no console on Windows (`windows_subsystem = "windows"`),
//! so what `tracing` writes to stdout is lost exactly when it is needed: a
//! hotkey that does nothing, a capture that fails. Everything is therefore
//! also appended to `screen-mark.log`, next to the history. The log holds
//! messages and file paths, never image data or recognised text.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tracing_subscriber::filter::Targets;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// The log starts afresh once it passes this size; the previous one is kept
/// as `screen-mark.log.old`.
pub const MAX_LOG_BYTES: u64 = 1024 * 1024;

/// `screen-mark.log` in the local data folder (`%LocalAppData%\screen-mark`
/// on Windows), or in `$SCREEN_MARK_HOME` when that is set.
pub fn log_path() -> Option<PathBuf> {
    if let Some(home) = crate::config::home_override() {
        return Some(home.join("screen-mark.log"));
    }
    dirs::data_local_dir().map(|d| d.join("screen-mark").join("screen-mark.log"))
}

/// Open the log for appending, creating its folder. A log over
/// `MAX_LOG_BYTES` is first moved to `<name>.old`, replacing the one before.
pub fn open_log(path: &Path) -> std::io::Result<File> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let too_big = std::fs::metadata(path)
        .map(|m| m.len() > MAX_LOG_BYTES)
        .unwrap_or(false);
    if too_big {
        let mut old = path.as_os_str().to_owned();
        old.push(".old");
        let _ = std::fs::rename(path, PathBuf::from(old));
    }
    OpenOptions::new().create(true).append(true).open(path)
}

/// Start logging to stdout (as before) and to the log file. `RUST_LOG`
/// picks the level (`debug`, `screen_mark=trace`, ...); the default is info.
/// A log file that can't be opened is skipped, never fatal.
pub fn init() {
    let filter = std::env::var("RUST_LOG")
        .ok()
        .and_then(|v| v.parse::<Targets>().ok())
        .unwrap_or_else(|| Targets::new().with_default(tracing::Level::INFO));
    let file = log_path().and_then(|p| open_log(&p).ok());
    let to_file = file.map(|f| {
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(Mutex::new(f))
    });
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .with(to_file)
        .try_init();
}

static PANIC_DIALOG: AtomicBool = AtomicBool::new(false);

/// Whether a panic on the main thread also shows an error dialog. On for
/// the tray app and the interactive overlay, where nobody reads stderr; off
/// for `capture --clipboard/--save`, which scripts run unattended.
pub fn show_dialog_on_panic(on: bool) {
    PANIC_DIALOG.store(on, Ordering::Relaxed);
}

/// Write panics to the log (with a backtrace) before the default handler
/// prints them to stderr. Without this, a panic in the tray app ended the
/// process with no trace anywhere.
pub fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        tracing::error!("panic: {info}\n{backtrace}");
        default(info);
        let on_main = std::thread::current().name() == Some("main");
        if on_main && PANIC_DIALOG.load(Ordering::Relaxed) {
            crate::installer::show_error(&panic_message());
        }
    }));
}

/// What the error dialog says after a panic.
pub fn panic_message() -> String {
    match log_path() {
        Some(p) => format!(
            "screen-mark hit an internal error and has to close.\n\nDetails are in {}",
            p.display()
        ),
        None => "screen-mark hit an internal error and has to close.".to_string(),
    }
}
