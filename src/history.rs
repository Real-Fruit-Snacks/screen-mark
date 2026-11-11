use std::path::{Path, PathBuf};

use crate::tools::Annotation;

const MAX_HISTORY: usize = 20;

/// History lives in the LOCAL data dir (e.g. %LocalAppData%) — captures can be
/// sensitive and must not sync into roaming profiles.
pub fn history_dir() -> Option<PathBuf> {
    if let Some(home) = crate::config::home_override() {
        return Some(home.join("history"));
    }
    dirs::data_local_dir().map(|d| d.join("screen-mark").join("history"))
}

/// Earlier history locations, oldest first: pre-0.6 kept it in the roaming
/// config dir; 1.x (as `hydroshot`) kept it in the local data dir.
fn legacy_history_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(d) = dirs::config_dir() {
        v.push(d.join("hydroshot").join("history"));
    }
    if let Some(d) = dirs::data_local_dir() {
        v.push(d.join("hydroshot").join("history"));
    }
    v
}

/// Move any PNGs from the legacy locations into `dir` (best effort, once).
fn migrate_legacy(dir: &std::path::Path) {
    if crate::config::home_override().is_some() {
        return;
    }
    for legacy in legacy_history_dirs() {
        migrate_one(&legacy, dir);
    }
}

fn migrate_one(legacy: &std::path::Path, dir: &std::path::Path) {
    if legacy == dir || !legacy.is_dir() {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(legacy) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "png") {
                if let Some(name) = path.file_name() {
                    let _ = std::fs::rename(&path, dir.join(name));
                }
            }
        }
    }
    let _ = std::fs::remove_dir(legacy); // only removes if now empty
}

fn ensure_dir() -> Result<PathBuf, String> {
    let dir = history_dir().ok_or("No local data directory")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    migrate_legacy(&dir);
    Ok(dir)
}

/// Save a capture to history. Filename: timestamp-based (e.g., "1711234567.png")
pub fn save_to_history(pixels: &[u8], width: u32, height: u32) -> Result<PathBuf, String> {
    let dir = ensure_dir()?;

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let filename = format!("{}.png", timestamp);
    let path = dir.join(&filename);

    let img =
        image::RgbaImage::from_raw(width, height, pixels.to_vec()).ok_or("Invalid image data")?;
    img.save(&path).map_err(|e| e.to_string())?;

    // Prune old entries (keep only MAX_HISTORY most recent)
    prune_history(&dir);

    Ok(path)
}

/// Copy an already-saved file into history (avoids re-encoding).
pub fn save_to_history_from_file(source: &std::path::Path) -> Result<PathBuf, String> {
    let dir = ensure_dir()?;

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let filename = format!("{}.png", timestamp);
    let path = dir.join(&filename);

    std::fs::copy(source, &path).map_err(|e| format!("Failed to copy to history: {e}"))?;

    prune_history(&dir);
    Ok(path)
}

/// Get list of history entries sorted newest first
pub fn list_history() -> Vec<PathBuf> {
    // ensure_dir creates the directory before migrating, so a 1.x user's
    // captures show up the first time the History window opens.
    let Ok(dir) = ensure_dir() else {
        return vec![];
    };
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|ext| ext == "png").unwrap_or(false))
        .collect();
    // Sort by filename (timestamp) descending
    entries.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    entries.truncate(MAX_HISTORY);
    entries
}

/// Delete every history entry. Returns the number of captures removed.
pub fn clear_history() -> usize {
    let Some(dir) = history_dir() else {
        return 0;
    };
    let mut removed = 0;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str());
            if ext == Some("png") && std::fs::remove_file(&path).is_ok() {
                removed += 1;
            } else if ext == Some(MARKUP_EXT) {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    removed
}

/// Delete one capture from history, with its markup.
pub fn delete_entry(png: &Path) -> Result<(), String> {
    let _ = std::fs::remove_file(markup_path(png));
    match std::fs::remove_file(png) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Couldn't delete {}: {e}", png.display())),
    }
}

// ───────────── markup: captures that can be edited again ─────────────

/// The extension of the file that keeps a history capture editable.
const MARKUP_EXT: &str = "markup";

/// A capture as it was before its annotations were drawn in, and the
/// annotations, with positions relative to its top left corner.
#[derive(Debug, Clone, PartialEq)]
pub struct Editable {
    /// Straight RGBA8, row-major.
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub annotations: Vec<Annotation>,
}

/// The markup file as written: the clean capture as a base64 PNG, and the
/// annotations.
#[derive(serde::Serialize, serde::Deserialize)]
struct MarkupFile {
    version: u32,
    width: u32,
    height: u32,
    image: String,
    annotations: Vec<Annotation>,
}

/// Where the markup of the history capture `png` is kept.
pub fn markup_path(png: &Path) -> PathBuf {
    png.with_extension(MARKUP_EXT)
}

/// Keep the history capture `png` editable: write its clean image and
/// annotations beside it.
pub fn write_markup(png: &Path, editable: &Editable) -> Result<(), String> {
    use base64::Engine;
    let image = crate::export::encode_png(&editable.pixels, editable.width, editable.height)?;
    let file = MarkupFile {
        version: 1,
        width: editable.width,
        height: editable.height,
        image: base64::engine::general_purpose::STANDARD.encode(image),
        annotations: editable.annotations.clone(),
    };
    let json = serde_json::to_vec(&file).map_err(|e| e.to_string())?;
    std::fs::write(markup_path(png), json).map_err(|e| format!("Couldn't keep the markup: {e}"))
}

/// A history capture, ready to edit: its clean image and annotations when
/// its markup was kept, or else the saved picture with nothing to edit
/// on it.
pub fn load_editable(png: &Path) -> Result<Editable, String> {
    match read_markup(png) {
        Ok(Some(e)) => return Ok(e),
        Ok(None) => {}
        Err(e) => tracing::warn!("Ignoring the markup of {}: {e}", png.display()),
    }
    let image = image::open(png)
        .map_err(|e| format!("Couldn't open {}: {e}", png.display()))?
        .to_rgba8();
    let (width, height) = image.dimensions();
    Ok(Editable {
        pixels: image.into_raw(),
        width,
        height,
        annotations: Vec::new(),
    })
}

fn read_markup(png: &Path) -> Result<Option<Editable>, String> {
    use base64::Engine;
    let bytes = match std::fs::read(markup_path(png)) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let file: MarkupFile = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if file.version != 1 {
        return Err(format!(
            "markup version {} is newer than this screen-mark",
            file.version
        ));
    }
    let png_bytes = base64::engine::general_purpose::STANDARD
        .decode(file.image)
        .map_err(|e| e.to_string())?;
    let image = image::load_from_memory(&png_bytes)
        .map_err(|e| e.to_string())?
        .to_rgba8();
    let (width, height) = image.dimensions();
    if (width, height) != (file.width, file.height) {
        return Err("the markup's image is the wrong size".into());
    }
    Ok(Some(Editable {
        pixels: image.into_raw(),
        width,
        height,
        annotations: file.annotations,
    }))
}

/// Whether a history capture can be edited with its annotations.
pub fn has_markup(png: &Path) -> bool {
    markup_path(png).is_file()
}

fn prune_history(dir: &std::path::Path) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|ext| ext == "png").unwrap_or(false))
        .collect();
    entries.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    // Remove entries beyond MAX_HISTORY
    for path in entries.iter().skip(MAX_HISTORY) {
        let _ = delete_entry(path);
    }
    // Markup whose capture is gone.
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == MARKUP_EXT) && !path.with_extension("png").exists()
        {
            let _ = std::fs::remove_file(path);
        }
    }
}
