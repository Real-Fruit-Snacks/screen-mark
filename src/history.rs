use std::path::PathBuf;

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

/// Delete every history entry. Returns the number of files removed.
pub fn clear_history() -> usize {
    let Some(dir) = history_dir() else {
        return 0;
    };
    let mut removed = 0;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "png")
                && std::fs::remove_file(&path).is_ok()
            {
                removed += 1;
            }
        }
    }
    removed
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
        let _ = std::fs::remove_file(path);
    }
}
