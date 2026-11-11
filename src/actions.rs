//! What happens to a finished capture: save, copy, history, and the message
//! the user sees. Runs on a worker thread; returns the popup to show.

use std::path::{Path, PathBuf};

use crate::toast::{make_thumb, Click, Kind, Spec, CLICK_HINT};

/// Save and/or copy `pixels` (RGBA8, `w`x`h`) and describe the result.
///
/// * `save_to`: folder to write a uniquely named PNG into, if any.
/// * `copy`: put the image on the clipboard.
/// * `history`: also keep it in the recent-captures history.
/// * `markup`: what keeps that history entry editable (see
///   `history::Editable`), when it has annotations.
///
/// A failure in one step never skips the other: a full disk still copies,
/// a busy clipboard still saves. The message says exactly which part failed.
pub fn finish_capture(
    pixels: &[u8],
    w: u32,
    h: u32,
    save_to: Option<&Path>,
    copy: bool,
    history: bool,
    markup: Option<&crate::history::Editable>,
) -> Spec {
    let saved: Option<Result<PathBuf, String>> =
        save_to.map(|dir| crate::export::save_capture(pixels, w, h, dir));
    let copied: Option<Result<(), String>> =
        copy.then(|| crate::export::copy_to_clipboard(pixels, w, h));
    if history {
        let kept = match &saved {
            Some(Ok(p)) => crate::history::save_to_history_from_file(p),
            _ => crate::history::save_to_history(pixels, w, h),
        };
        keep_markup(kept, markup);
    }
    describe(saved, copied, w, h).thumb(make_thumb(pixels, w, h, 192, 120))
}

/// Keep a history entry editable, if it was saved and has annotations.
pub fn keep_markup(kept: Result<PathBuf, String>, markup: Option<&crate::history::Editable>) {
    match (kept, markup) {
        (Ok(png), Some(m)) => {
            if let Err(e) = crate::history::write_markup(&png, m) {
                tracing::warn!("{e}");
            }
        }
        (Err(e), _) => tracing::warn!("Couldn't add the capture to History: {e}"),
        (Ok(_), None) => {}
    }
}

/// The popup for a save/copy outcome (no thumbnail).
pub fn describe(
    saved: Option<Result<PathBuf, String>>,
    copied: Option<Result<(), String>>,
    w: u32,
    h: u32,
) -> Spec {
    let size = format!("{w} \u{00d7} {h}");
    let file = |p: &Path| {
        p.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| p.display().to_string())
    };
    let show_hint = format!("{size}{CLICK_HINT}");
    match (saved, copied) {
        (Some(Ok(p)), Some(Ok(()))) => Spec::new(Kind::Success, "Saved and copied", file(&p))
            .hint(show_hint)
            .click(Click::ShowFile(p)),
        (Some(Ok(p)), None) => Spec::new(Kind::Success, "Saved", file(&p))
            .hint(show_hint)
            .click(Click::ShowFile(p)),
        (None, Some(Ok(()))) => {
            Spec::new(Kind::Success, "Copied", size).hint("Paste it anywhere with Ctrl+V")
        }
        (Some(Err(e)), Some(Ok(()))) => Spec::new(Kind::Error, "Copied, but not saved", e)
            .hint("Change the folder in Settings > Capture"),
        (Some(Ok(p)), Some(Err(e))) => Spec::new(Kind::Error, "Saved, but not copied", e)
            .hint(format!("{}{CLICK_HINT}", file(&p)))
            .click(Click::ShowFile(p)),
        (Some(Err(e)), Some(Err(_)) | None) => Spec::new(Kind::Error, "Couldn't save", e)
            .hint("Change the folder in Settings > Capture"),
        (None, Some(Err(e))) => Spec::new(Kind::Error, "Couldn't copy", e),
        (None, None) => Spec::new(Kind::Info, "Nothing to do", size),
    }
}
