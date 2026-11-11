//! Marking up an image that is already there (a file, the clipboard, a
//! capture in History) instead of a new capture.
//!
//! The image goes in the middle of a plain backdrop the size of the screen,
//! and the overlay opens on that with the image selected (see
//! `OverlayState::canvas`). Everything else is the capture overlay as it
//! is: the same tools, keys and toolbar, and copy, save, pin and the rest
//! use the selection, so cropping is shrinking the selection.

use std::path::Path;

use crate::history::Editable;
use crate::overlay::selection::Selection;
use crate::tools::Annotation;

/// An image laid out for the overlay.
#[derive(Debug, Clone)]
pub struct Layout {
    /// The backdrop with the image on it, straight RGBA8, `width` x `height`.
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Where the image is.
    pub canvas: Selection,
    /// The image's annotations, moved (and scaled) to where it is.
    pub annotations: Vec<Annotation>,
    /// What the image was scaled by to fit: 1.0 unless it was bigger than
    /// the screen.
    pub scale: f32,
}

/// Lay `image` out on a `screen_w` x `screen_h` backdrop of colour `rgb`.
/// An image bigger than the screen is scaled down to fit, and so are its
/// annotations; it is then copied and saved at that size.
pub fn lay_out(image: &Editable, screen_w: u32, screen_h: u32, rgb: (u8, u8, u8)) -> Layout {
    let (sw, sh) = (screen_w.max(1), screen_h.max(1));
    let (iw, ih) = (image.width.max(1), image.height.max(1));
    let scale = (sw as f32 / iw as f32).min(sh as f32 / ih as f32).min(1.0);

    let (pixels, w, h) = if scale < 1.0 {
        let w = ((iw as f32 * scale).floor() as u32).clamp(1, sw);
        let h = ((ih as f32 * scale).floor() as u32).clamp(1, sh);
        let resized = image::RgbaImage::from_raw(image.width, image.height, image.pixels.clone())
            .map(|img| image::imageops::resize(&img, w, h, image::imageops::FilterType::Triangle));
        match resized {
            Some(img) => (img.into_raw(), w, h),
            None => (vec![0; (w * h * 4) as usize], w, h),
        }
    } else {
        (image.pixels.clone(), image.width, image.height)
    };

    let x0 = (sw - w) / 2;
    let y0 = (sh - h) / 2;
    let mut out = Vec::with_capacity(sw as usize * sh as usize * 4);
    for _ in 0..sw as usize * sh as usize {
        out.extend_from_slice(&[rgb.0, rgb.1, rgb.2, 255]);
    }
    for row in 0..h as usize {
        let src = row * w as usize * 4;
        let Some(line) = pixels.get(src..src + w as usize * 4) else {
            break;
        };
        let dst = ((y0 as usize + row) * sw as usize + x0 as usize) * 4;
        // Transparent parts of the image show the backdrop through them,
        // as they would on screen.
        let (dst_px, _) = out[dst..dst + w as usize * 4].as_chunks_mut::<4>();
        let (src_px, _) = line.as_chunks::<4>();
        for (d, s) in dst_px.iter_mut().zip(src_px) {
            let a = s[3] as u32;
            for c in 0..3 {
                d[c] = ((s[c] as u32 * a + d[c] as u32 * (255 - a)) / 255) as u8;
            }
            d[3] = 255;
        }
    }

    let annotations = image
        .annotations
        .iter()
        .map(|a| {
            let mut a = a.clone();
            crate::tools::scale_annotation(&mut a, scale);
            crate::tools::move_annotation(&mut a, x0 as f32, y0 as f32);
            a
        })
        .collect();
    Layout {
        pixels: out,
        width: sw,
        height: sh,
        canvas: Selection {
            x: x0 as f32,
            y: y0 as f32,
            width: w as f32,
            height: h as f32,
        },
        annotations,
        scale,
    }
}

/// The largest image screen-mark opens, in pixels (about 16k x 16k).
const MAX_PIXELS: u64 = 268_435_456;

/// Read an image file to mark up: PNG, JPEG, WebP or BMP.
pub fn open_file(path: &Path) -> Result<Editable, String> {
    let reader = image::ImageReader::open(path)
        .map_err(|e| format!("Couldn't open {}: {}", path.display(), short(&e)))?
        .with_guessed_format()
        .map_err(|e| format!("Couldn't read {}: {}", path.display(), short(&e)))?;
    if reader.format().is_none() {
        return Err(format!(
            "{} isn't an image screen-mark can open ({})",
            path.display(),
            crate::export::SAVE_FORMATS
        ));
    }
    if let Ok((w, h)) = reader.into_dimensions() {
        if w as u64 * h as u64 > MAX_PIXELS {
            return Err(format!(
                "{} is too big to mark up ({w} \u{00d7} {h})",
                path.display()
            ));
        }
    }
    let image = image::open(path)
        .map_err(|e| format!("Couldn't read {}: {e}", path.display()))?
        .to_rgba8();
    let (width, height) = image.dimensions();
    Ok(Editable {
        pixels: image.into_raw(),
        width,
        height,
        annotations: Vec::new(),
    })
}

/// The image on the clipboard, to mark up.
pub fn from_clipboard() -> Result<Editable, String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| format!("Couldn't open the clipboard: {e}"))?;
    let image = clipboard.get_image().map_err(|e| match e {
        arboard::Error::ContentNotAvailable => "There's no image on the clipboard".to_string(),
        e => format!("Couldn't read the clipboard: {e}"),
    })?;
    let (width, height) = (image.width as u32, image.height as u32);
    if width == 0 || height == 0 {
        return Err("The image on the clipboard is empty".into());
    }
    Ok(Editable {
        pixels: image.bytes.into_owned(),
        width,
        height,
        annotations: Vec::new(),
    })
}

/// What the save dialog suggests for a marked-up copy of `path`:
/// `photo.png` becomes `photo (marked up)`.
pub fn suggested_name(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_string_lossy();
    Some(format!("{stem} (marked up)"))
}

fn short(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "no such file".into(),
        std::io::ErrorKind::PermissionDenied => "permission denied".into(),
        _ => e.to_string(),
    }
}
