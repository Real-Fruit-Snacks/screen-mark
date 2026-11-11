//! Send: hand a capture to a command of your own (`[send] command` in
//! config.toml), for an upload script, a share, a chat tool, anything
//! screen-mark doesn't know about.
//!
//! The capture is written to a PNG in a temporary folder and the command
//! runs with its path. What the command prints is copied, so a script that
//! uploads the file and prints a link leaves the link on the clipboard.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long a command may run before screen-mark gives up on it.
pub const TIMEOUT: Duration = Duration::from_secs(120);

/// How long a sent PNG is kept in the temporary folder.
const KEEP: Duration = Duration::from_secs(24 * 60 * 60);

/// Split a command line into words the way a shell would, without running
/// one: spaces separate words, and "..." or '...' keep spaces in a word.
/// A backslash is an ordinary character, so Windows paths need no escaping.
pub fn split(command: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    for c in command.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => word.push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                in_word = true;
            }
            None if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            None => {
                word.push(c);
                in_word = true;
            }
        }
    }
    if let Some(q) = quote {
        return Err(format!("the command has a {q} that isn't closed"));
    }
    if in_word {
        words.push(word);
    }
    if words.is_empty() {
        return Err("no command is set ([send] command in config.toml)".into());
    }
    Ok(words)
}

/// The program and arguments to run for `file`: every `{file}` in a word
/// becomes the file's path, and with no `{file}` anywhere the path is
/// added as the last argument.
pub fn arguments(command: &str, file: &Path) -> Result<(String, Vec<String>), String> {
    let path = file.to_string_lossy();
    let words = split(command)?;
    let mentioned = words.iter().any(|w| w.contains("{file}"));
    let mut words: Vec<String> = words
        .into_iter()
        .map(|w| w.replace("{file}", &path))
        .collect();
    if !mentioned {
        words.push(path.into_owned());
    }
    let program = words.remove(0);
    Ok((program, words))
}

/// The folder sent PNGs are written to.
pub fn folder() -> PathBuf {
    std::env::temp_dir().join("screen-mark-send")
}

/// Write the capture to a new PNG in `folder()` and remove ones from more
/// than a day ago. Returns the new file.
pub fn write_png(pixels: &[u8], width: u32, height: u32) -> Result<PathBuf, String> {
    let dir = folder();
    forget_old(&dir);
    crate::export::save_capture(pixels, width, height, &dir)
}

fn forget_old(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > KEEP);
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Run `command` on `file` and wait for it, up to `timeout`. Returns what
/// it printed, trimmed. A command that fails or can't start is an error
/// that says why, in words for the popup.
pub fn run(command: &str, file: &Path, timeout: Duration) -> Result<String, String> {
    let (program, args) = arguments(command, file)?;
    let mut cmd = Command::new(&program);
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // No console window flashing up for a console program.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => format!("{program} wasn't found"),
        std::io::ErrorKind::PermissionDenied => {
            format!("{program} can't be run (permission denied)")
        }
        _ => format!("{program} couldn't start: {e}"),
    })?;

    // Read both pipes on their own threads, so a command that prints a lot
    // can't stall on a full pipe while it is waited for.
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut text = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut text);
            }
            String::from_utf8_lossy(&text).into_owned()
        })
    };
    let out = drain(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let err = drain(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "{program} was still running after {} seconds and was stopped",
                    timeout.as_secs()
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(format!("couldn't wait for {program}: {e}")),
        }
    };
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    if status.success() {
        return Ok(stdout.trim().to_string());
    }
    let said = stderr
        .lines()
        .chain(stdout.lines())
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .map(|l| format!(": {l}"))
        .unwrap_or_default();
    Err(match status.code() {
        Some(code) => format!("{program} failed (exit code {code}){said}"),
        None => format!("{program} was stopped{said}"),
    })
}

/// What the Send button and popups call the command: `[send] name`, or
/// "Send".
pub fn name(config: &crate::config::SendConfig) -> String {
    match config.name.trim() {
        "" => "Send".to_string(),
        n => n.to_string(),
    }
}

/// Send a capture: write it, run the command, copy what it printed.
/// Returns the popup to show. Runs on a worker thread.
pub fn send_capture(
    pixels: &[u8],
    width: u32,
    height: u32,
    command: &str,
    name: &str,
) -> crate::toast::Spec {
    use crate::toast::{Kind, Spec};
    let file = match write_png(pixels, width, height) {
        Ok(file) => file,
        Err(e) => return Spec::new(Kind::Error, format!("{name} failed"), e),
    };
    match run(command, &file, TIMEOUT) {
        Ok(output) if output.is_empty() => {
            tracing::info!("Sent {} with the [send] command", file.display());
            Spec::new(
                Kind::Success,
                format!("{name}: done"),
                format!("{width} \u{00d7} {height}"),
            )
        }
        Ok(output) => {
            tracing::info!(
                "Sent {}; the command printed {} bytes",
                file.display(),
                output.len()
            );
            let copied = crate::export::copy_text(&output).is_ok();
            let first = output.lines().next().unwrap_or_default().to_string();
            let title = if copied {
                format!("{name}: done; what it printed is copied")
            } else {
                format!("{name}: done")
            };
            Spec::new(Kind::Success, title, first)
        }
        Err(e) => {
            tracing::error!("[send] command failed: {e}");
            Spec::new(Kind::Error, format!("{name} failed"), e)
        }
    }
}
