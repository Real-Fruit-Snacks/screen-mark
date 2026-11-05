//! Where the last region capture was on screen, so the last-region hotkey
//! can capture the same area again. The app keeps it in memory and in a
//! one-line file, so it survives a restart.

use std::path::{Path, PathBuf};

use crate::display::Rect;

/// `<local data dir>/screen-mark/last-region.txt`, or
/// `$SCREEN_MARK_HOME/last-region.txt` when that is set.
pub fn path() -> Option<PathBuf> {
    if let Some(home) = crate::config::home_override() {
        return Some(home.join("last-region.txt"));
    }
    dirs::data_local_dir().map(|d| d.join("screen-mark").join("last-region.txt"))
}

/// One line: `x y width height`, in virtual-desktop pixels.
pub fn to_line(r: Rect) -> String {
    format!("{} {} {} {}", r.x, r.y, r.w, r.h)
}

/// Read what `to_line` wrote. `None` for anything else, including an area
/// with no width or height.
pub fn from_line(s: &str) -> Option<Rect> {
    let mut parts = s.split_whitespace();
    let x = parts.next()?.parse().ok()?;
    let y = parts.next()?.parse().ok()?;
    let w: u32 = parts.next()?.parse().ok()?;
    let h: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || w == 0 || h == 0 {
        return None;
    }
    Some(Rect::new(x, y, w, h))
}

pub fn load_from(path: &Path) -> Option<Rect> {
    from_line(&std::fs::read_to_string(path).ok()?)
}

pub fn save_to(path: &Path, r: Rect) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, to_line(r) + "\n")
}

/// The remembered area, if there is one.
pub fn load() -> Option<Rect> {
    load_from(&path()?)
}

/// Remember `r` as the last region.
pub fn save(r: Rect) -> std::io::Result<()> {
    let path = path().ok_or_else(|| std::io::Error::other("no data folder"))?;
    save_to(&path, r)
}
