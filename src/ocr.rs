//! OCR text extraction with the engine built into Windows 10 and later
//! (`Windows.Media.Ocr`).
//!
//! The engine is called in-process. Earlier versions wrote the image to a
//! temp file and ran a PowerShell script on it with `-ExecutionPolicy
//! Bypass`: a quarter of a second just to start PowerShell, a copy of the
//! capture on disk, and nothing at all on a machine where PowerShell is
//! locked down.

/// Read the text in an image. `pixels` is straight RGBA8, `width` x `height`.
/// Lines of text come back separated by `\n`.
#[cfg(target_os = "windows")]
pub fn extract_text(pixels: &[u8], width: u32, height: u32) -> Result<String, String> {
    win::recognize(pixels, width, height)
}

#[cfg(not(target_os = "windows"))]
pub fn extract_text(_pixels: &[u8], _width: u32, _height: u32) -> Result<String, String> {
    Err("OCR is only available on Windows 10+".to_string())
}

/// `pixels` as BGRA (what the engine takes), scaled down if either side is
/// longer than `max_side`. Returns the bytes and the size they describe.
/// `None` if the buffer is too short for the size.
pub fn to_bgra_within(
    pixels: &[u8],
    width: u32,
    height: u32,
    max_side: u32,
) -> Option<(Vec<u8>, u32, u32)> {
    let expected = width as usize * height as usize * 4;
    if width == 0 || height == 0 || pixels.len() < expected {
        return None;
    }
    let max_side = max_side.max(1);
    let (mut out, w, h) = if width > max_side || height > max_side {
        let scale = max_side as f64 / width.max(height) as f64;
        let w = ((width as f64 * scale) as u32).clamp(1, max_side);
        let h = ((height as f64 * scale) as u32).clamp(1, max_side);
        let image = image::RgbaImage::from_raw(width, height, pixels[..expected].to_vec())?;
        let small = image::imageops::resize(&image, w, h, image::imageops::FilterType::Triangle);
        (small.into_raw(), w, h)
    } else {
        (pixels[..expected].to_vec(), width, height)
    };
    for px in out.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
    Some((out, w, h))
}

#[cfg(target_os = "windows")]
mod win {
    use windows::core::HRESULT;
    use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::DataWriter;

    /// A null object from a WinRT call that "tries" to make one.
    const E_POINTER: HRESULT = HRESULT(0x80004003_u32 as i32);

    pub fn recognize(pixels: &[u8], width: u32, height: u32) -> Result<String, String> {
        let engine = OcrEngine::TryCreateFromUserProfileLanguages().map_err(|e| {
            if e.code().is_ok() || e.code() == E_POINTER {
                "Windows has no text recognition for your languages. Add one under \
                 Settings > Time & language > Language & region (a language with \
                 \"Optical character recognition\")."
                    .to_string()
            } else {
                format!("Couldn't start Windows OCR: {}", e.message())
            }
        })?;

        // The engine refuses images over a few thousand pixels a side; a
        // capture across several monitors is scaled to fit.
        let max_side = OcrEngine::MaxImageDimension().unwrap_or(4096);
        let (bgra, w, h) =
            super::to_bgra_within(pixels, width, height, max_side).ok_or("Invalid image data")?;

        let read = || -> windows::core::Result<String> {
            let writer = DataWriter::new()?;
            writer.WriteBytes(&bgra)?;
            let buffer = writer.DetachBuffer()?;
            let bitmap = SoftwareBitmap::CreateCopyFromBuffer(
                &buffer,
                BitmapPixelFormat::Bgra8,
                w as i32,
                h as i32,
            )?;
            // This is a worker thread, so waiting here blocks nothing.
            let result = engine.RecognizeAsync(&bitmap)?.get()?;
            let mut lines = Vec::new();
            for line in result.Lines()? {
                lines.push(line.Text()?.to_string());
            }
            Ok(lines.join("\n"))
        };
        let text = read().map_err(|e| format!("OCR failed: {}", e.message()))?;
        if text.trim().is_empty() {
            Err("No text found in the selected region".to_string())
        } else {
            Ok(text)
        }
    }
}
