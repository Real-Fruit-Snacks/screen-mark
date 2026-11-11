use crate::tools::{render_annotations, Annotation, Backdrop};
use tiny_skia::{Pixmap, PremultipliedColorU8};

/// Flatten annotations onto a pixel buffer.
///
/// Input and output are straight (non-premultiplied) RGBA8 pixels.
/// Internally converts to premultiplied alpha for tiny-skia rendering,
/// then converts back to straight alpha for the output.
pub fn flatten_annotations(
    pixels: &[u8],
    width: u32,
    height: u32,
    annotations: &[Annotation],
) -> Vec<u8> {
    let mut pixmap = match Pixmap::new(width, height) {
        Some(p) => p,
        None => return pixels.to_vec(), // zero-dimension: return input unchanged
    };

    // Copy source pixels into pixmap WITH premultiplication
    let expected_len = (width as usize) * (height as usize) * 4;
    if pixels.len() < expected_len {
        return pixels.to_vec(); // undersized buffer: return input unchanged
    }

    // Fast path: screenshots are fully opaque (a=255), so premultiplication is a
    // no-op — we can copy components directly instead of multiplying per pixel.
    let all_opaque = pixels.as_chunks::<4>().0.iter().all(|px| px[3] == 255);
    let pm_pixels = pixmap.pixels_mut();
    if all_opaque {
        for (i, chunk) in pixels.as_chunks::<4>().0.iter().enumerate() {
            pm_pixels[i] = PremultipliedColorU8::from_rgba(chunk[0], chunk[1], chunk[2], 255)
                .unwrap_or(PremultipliedColorU8::TRANSPARENT);
        }
    } else {
        // Straight RGBA in — premultiply each component. The multiply guarantees
        // component <= alpha, so from_rgba cannot fail here.
        for (i, chunk) in pixels.as_chunks::<4>().0.iter().enumerate() {
            let a = chunk[3] as u16;
            let pm = |c: u8| ((c as u16 * a + 127) / 255) as u8;
            pm_pixels[i] =
                PremultipliedColorU8::from_rgba(pm(chunk[0]), pm(chunk[1]), pm(chunk[2]), chunk[3])
                    .unwrap_or(PremultipliedColorU8::TRANSPARENT);
        }
    }

    // The same call the overlay makes, so the export matches the screen.
    render_annotations(annotations, None, &mut pixmap, Backdrop { pixels, width });

    // Demultiply back to straight alpha
    let pm_pixels = pixmap.pixels();
    let mut output = vec![0u8; expected_len];
    for (i, px) in pm_pixels.iter().enumerate() {
        let a = px.alpha();
        let base = i * 4;
        if a == 0 {
            // output is already zeroed
        } else if a == 255 {
            // Fully opaque: no division needed
            output[base] = px.red();
            output[base + 1] = px.green();
            output[base + 2] = px.blue();
            output[base + 3] = 255;
        } else {
            output[base] = (px.red() as u16 * 255 / a as u16) as u8;
            output[base + 1] = (px.green() as u16 * 255 / a as u16) as u8;
            output[base + 2] = (px.blue() as u16 * 255 / a as u16) as u8;
            output[base + 3] = a;
        }
    }

    output
}

/// Crop a screenshot to a selection region, offset annotations, and flatten.
///
/// `screenshot_pixels` is straight RGBA8 for the full screenshot.
/// Annotations are in screenshot-absolute coordinates and will be offset
/// by (-sel_x, -sel_y) to become selection-relative.
pub fn crop_and_flatten(
    screenshot_pixels: &[u8],
    screenshot_width: u32,
    sel_x: u32,
    sel_y: u32,
    sel_w: u32,
    sel_h: u32,
    annotations: &[Annotation],
) -> Vec<u8> {
    // Clamp selection to screenshot bounds to prevent out-of-bounds access
    let screenshot_height = screenshot_pixels.len() as u32 / (screenshot_width * 4).max(1);
    let clamped_w = sel_w.min(screenshot_width.saturating_sub(sel_x));
    let clamped_h = sel_h.min(screenshot_height.saturating_sub(sel_y));

    // Crop the pixel buffer
    let mut cropped = Vec::with_capacity((clamped_w as usize) * (clamped_h as usize) * 4);
    for row in sel_y..(sel_y + clamped_h) {
        let start = ((row * screenshot_width + sel_x) * 4) as usize;
        let end = start + (clamped_w * 4) as usize;
        if end <= screenshot_pixels.len() {
            cropped.extend_from_slice(&screenshot_pixels[start..end]);
        }
    }

    // Offset annotations to selection-relative coordinates
    let offset_annotations: Vec<Annotation> = annotations
        .iter()
        .map(|a| offset_annotation(a, sel_x as f32, sel_y as f32))
        .collect();

    flatten_annotations(&cropped, clamped_w, clamped_h, &offset_annotations)
}

/// Offset an annotation's coordinates by (-dx, -dy).
fn offset_annotation(annotation: &Annotation, dx: f32, dy: f32) -> Annotation {
    let mut moved = annotation.clone();
    crate::tools::move_annotation(&mut moved, -dx, -dy);
    moved
}

/// The process keeps one clipboard handle for its whole life. On X11 and
/// Wayland the copying program has to stay around to hand the image to
/// whoever pastes it; arboard does that from a background thread for as long
/// as a `Clipboard` exists. A handle dropped right after `set_image` (what
/// 2.0 did, on a worker thread) meant the image was gone before anyone
/// could paste it, unless a clipboard manager grabbed it first.
static CLIPBOARD: std::sync::Mutex<Option<arboard::Clipboard>> = std::sync::Mutex::new(None);

/// Copy text to the system clipboard, through the same long-lived handle as
/// images (see `CLIPBOARD`), so on Linux it is still there to paste.
pub fn copy_text(text: &str) -> Result<(), String> {
    let mut guard = CLIPBOARD.lock().unwrap_or_else(|p| p.into_inner());
    let mut last_err = String::new();
    for _ in 0..2 {
        if guard.is_none() {
            match arboard::Clipboard::new() {
                Ok(c) => *guard = Some(c),
                Err(e) => {
                    last_err = format!("Failed to open clipboard: {e}");
                    std::thread::sleep(std::time::Duration::from_millis(60));
                    continue;
                }
            }
        }
        match guard.as_mut().expect("set above").set_text(text) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last_err = format!("Failed to copy text to clipboard: {e}");
                *guard = None;
                std::thread::sleep(std::time::Duration::from_millis(60));
            }
        }
    }
    Err(last_err)
}

/// Copy pixel data to the system clipboard.
///
/// `pixels` is straight RGBA8 data.
pub fn copy_to_clipboard(pixels: &[u8], width: u32, height: u32) -> Result<(), String> {
    use arboard::{Clipboard, ImageData};

    let expected = width as usize * height as usize * 4;
    if width == 0 || height == 0 || pixels.len() < expected {
        return Err(format!(
            "Nothing to copy ({width}x{height}, {} bytes)",
            pixels.len()
        ));
    }
    let mut guard = CLIPBOARD.lock().unwrap_or_else(|p| p.into_inner());
    // One retry with a fresh handle: on Windows another program can hold
    // the clipboard open for a moment, and a stale X11 connection fails once.
    let mut last_err = String::new();
    for _ in 0..2 {
        if guard.is_none() {
            match Clipboard::new() {
                Ok(c) => *guard = Some(c),
                Err(e) => {
                    last_err = format!("Failed to open clipboard: {e}");
                    std::thread::sleep(std::time::Duration::from_millis(60));
                    continue;
                }
            }
        }
        let img = ImageData {
            width: width as usize,
            height: height as usize,
            bytes: std::borrow::Cow::Borrowed(&pixels[..expected]),
        };
        match guard.as_mut().expect("set above").set_image(img) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last_err = format!("Failed to copy image to clipboard: {e}");
                *guard = None;
                std::thread::sleep(std::time::Duration::from_millis(60));
            }
        }
    }
    Err(last_err)
}

/// Encode RGBA8 pixels as PNG bytes.
pub fn encode_png(pixels: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    let expected = width as usize * height as usize * 4;
    if pixels.len() < expected {
        return Err("invalid image data".into());
    }
    let img = image::RgbaImage::from_raw(width, height, pixels[..expected].to_vec())
        .ok_or("invalid image data")?;
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// The name captures are saved under unless `file_name` in config.toml
/// says otherwise.
pub const DEFAULT_FILE_NAME: &str = "screen-mark_{date}_{time}";

/// The `file_name` pattern in use (see `set_file_name`).
static FILE_NAME: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// Use `pattern` for the captures saved from now on (`file_name` in
/// config.toml). Empty means the default.
pub fn set_file_name(pattern: &str) {
    let mut name = FILE_NAME.lock().unwrap_or_else(|e| e.into_inner());
    *name = pattern.to_string();
}

/// The `file_name` pattern set by `set_file_name`, or the default.
pub fn file_name_pattern() -> String {
    let name = FILE_NAME.lock().unwrap_or_else(|e| e.into_inner());
    match name.trim() {
        "" => DEFAULT_FILE_NAME.to_string(),
        n => n.to_string(),
    }
}

/// The file name screen-mark gives a capture taken at `when`, before any
/// `_2`, `_3` suffix: `screen-mark_2026-10-28_143012.png`.
pub fn capture_file_stem(when: chrono::DateTime<chrono::Local>) -> String {
    file_stem(DEFAULT_FILE_NAME, when, 0, 0)
}

/// A file name (without ".png") from a `file_name` pattern. Placeholders:
///
/// | | |
/// |---|---|
/// | `{date}` | 2026-10-28 |
/// | `{time}` | 143012 |
/// | `{year}` `{month}` `{day}` | 2026, 10, 28 |
/// | `{hour}` `{minute}` `{second}` | 14, 30, 12 |
/// | `{width}` `{height}` | the capture's size in pixels |
///
/// Anything else is kept as written. Characters a file name can't hold
/// (`\ / : * ? " < > |`) become `-`, so a pattern never names a folder.
/// A pattern that comes out empty gives the default name.
pub fn file_stem(
    pattern: &str,
    when: chrono::DateTime<chrono::Local>,
    width: u32,
    height: u32,
) -> String {
    let pattern = pattern.trim();
    let pattern = pattern
        .strip_suffix(".png")
        .or_else(|| pattern.strip_suffix(".PNG"))
        .unwrap_or(pattern);
    let mut out = pattern.to_string();
    for (token, value) in [
        ("{date}", when.format("%Y-%m-%d").to_string()),
        ("{time}", when.format("%H%M%S").to_string()),
        ("{year}", when.format("%Y").to_string()),
        ("{month}", when.format("%m").to_string()),
        ("{day}", when.format("%d").to_string()),
        ("{hour}", when.format("%H").to_string()),
        ("{minute}", when.format("%M").to_string()),
        ("{second}", when.format("%S").to_string()),
        ("{width}", width.to_string()),
        ("{height}", height.to_string()),
    ] {
        out = out.replace(token, &value);
    }
    let cleaned: String = out
        .chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .collect();
    // Windows drops trailing dots and spaces from a name.
    let cleaned = cleaned.trim().trim_end_matches(['.', ' ']).to_string();
    if cleaned.is_empty() || cleaned.chars().all(|c| c == '-') {
        return file_stem(DEFAULT_FILE_NAME, when, width, height);
    }
    cleaned
}

/// Write `png` into `dir` under `<stem>.png`, or `<stem>_2.png`,
/// `<stem>_3.png`, ... if that name is taken. Never overwrites: the file is
/// created with `create_new`, so two captures in the same second (or two
/// screen-mark processes) can't clobber each other. Creates `dir`.
pub fn write_unique(
    dir: &std::path::Path,
    stem: &str,
    png: &[u8],
) -> Result<std::path::PathBuf, String> {
    use std::io::Write;
    std::fs::create_dir_all(dir).map_err(|e| {
        // Windows reports a file sitting where a folder should be as
        // "already exists" or "not found", not "not a directory".
        let reason = if file_in_the_way(dir) {
            "a file is in the way".to_string()
        } else {
            short_io_error(&e)
        };
        format!("Can't create the folder ({reason}): {}", dir.display())
    })?;
    for n in 1..=9999u32 {
        let name = if n == 1 {
            format!("{stem}.png")
        } else {
            format!("{stem}_{n}.png")
        };
        let path = dir.join(name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut f) => {
                if let Err(e) = f.write_all(png).and_then(|_| f.flush()) {
                    drop(f);
                    let _ = std::fs::remove_file(&path);
                    return Err(format!(
                        "Can't write the file ({}): {}",
                        short_io_error(&e),
                        path.display()
                    ));
                }
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(format!(
                    "Can't write in the folder ({}): {}",
                    short_io_error(&e),
                    dir.display()
                ));
            }
        }
    }
    Err(format!(
        "{} already has 9999 captures named {stem}",
        dir.display()
    ))
}

/// Encode and save a capture into `dir` with a timestamped, unique name.
pub fn save_capture(
    pixels: &[u8],
    width: u32,
    height: u32,
    dir: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    let png = encode_png(pixels, width, height)?;
    let stem = file_stem(&file_name_pattern(), chrono::Local::now(), width, height);
    write_unique(dir, &stem, &png)
}

/// True if `dir` or one of its parents exists as something other than a
/// folder.
fn file_in_the_way(dir: &std::path::Path) -> bool {
    dir.ancestors()
        .any(|p| !p.as_os_str().is_empty() && p.exists() && !p.is_dir())
}

fn short_io_error(e: &std::io::Error) -> String {
    use std::io::ErrorKind::*;
    match e.kind() {
        PermissionDenied => "permission denied".into(),
        NotFound => "not found".into(),
        NotADirectory => "a file is in the way".into(),
        StorageFull => "disk full".into(),
        ReadOnlyFilesystem => "read-only drive".into(),
        _ => e.to_string(),
    }
}

/// Save pixel data to an image file (accepts owned Vec to avoid redundant copy).
///
/// When `default_dir` is `Some`, auto-saves to that directory with a
/// timestamped PNG filename (no dialog). When `None`, opens a native save
/// dialog offering PNG, JPEG, and WebP formats, with `suggested` (a name
/// without its extension) filled in, or else the `file_name` pattern's.
///
/// Returns `Ok(Some(path))` on success, `Ok(None)` if the user cancelled,
/// or `Err` on failure.
pub fn save_to_file(
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    default_dir: Option<&std::path::Path>,
    suggested: Option<&str>,
) -> Result<Option<String>, String> {
    let stem = match suggested {
        Some(name) => name.to_string(),
        None => file_stem(&file_name_pattern(), chrono::Local::now(), width, height),
    };
    let default_name = format!("{stem}.png");

    if let Some(dir) = default_dir {
        // Auto-save (no dialog), always PNG, never overwriting.
        let path = save_capture(&pixels, width, height, dir)?;
        return Ok(Some(path.to_string_lossy().into_owned()));
    }

    // Show dialog with format options
    let path = rfd::FileDialog::new()
        .set_file_name(&default_name)
        .add_filter("PNG Image", &["png"])
        .add_filter("JPEG Image", &["jpg", "jpeg"])
        .add_filter("WebP Image", &["webp"])
        .save_file();

    match path {
        Some(p) => {
            write_image(&p, pixels, width, height)?;
            Ok(Some(p.to_string_lossy().into_owned()))
        }
        None => Ok(None),
    }
}

/// The formats `write_image` writes, as an error message names them.
pub const SAVE_FORMATS: &str = "PNG, JPEG, WebP or BMP";

/// Write RGBA8 pixels to `path`. The extension picks the format; a name
/// with no extension is written as PNG. Used by the save dialog and by
/// `screen-mark capture --save`.
pub fn write_image(
    path: &std::path::Path,
    pixels: Vec<u8>,
    width: u32,
    height: u32,
) -> Result<(), String> {
    use image::ImageFormat;

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_ascii_lowercase();
    let format = match ext.as_str() {
        "png" => ImageFormat::Png,
        "jpg" | "jpeg" => ImageFormat::Jpeg,
        "webp" => ImageFormat::WebP,
        "bmp" => ImageFormat::Bmp,
        other => {
            return Err(format!(
                "Can't save as .{other}: screen-mark writes {SAVE_FORMATS}"
            ))
        }
    };
    let img = image::RgbaImage::from_raw(width, height, pixels)
        .ok_or_else(|| "Failed to create image from pixel data".to_string())?;
    let result = if format == ImageFormat::Jpeg {
        // JPEG has no alpha channel.
        image::DynamicImage::ImageRgba8(img)
            .to_rgb8()
            .save_with_format(path, format)
    } else {
        img.save_with_format(path, format)
    };
    result.map_err(|e| format!("Failed to save image: {e}"))
}
