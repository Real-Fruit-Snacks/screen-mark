//! OCR. The image preparation is tested everywhere; reading text needs
//! Windows with an OCR language installed, so that test is opt-in:
//!
//!     cargo test --test ocr_tests -- --ignored

use screen_mark::ocr::to_bgra_within;

#[test]
fn pixels_go_to_the_engine_as_bgra() {
    // One red, one green, one blue, one translucent white pixel.
    let rgba = [
        255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 128,
    ];
    let (bgra, w, h) = to_bgra_within(&rgba, 2, 2, 4096).unwrap();
    assert_eq!((w, h), (2, 2));
    assert_eq!(
        bgra,
        [0, 0, 255, 255, 0, 255, 0, 255, 255, 0, 0, 255, 255, 255, 255, 128]
    );
}

#[test]
fn an_image_too_large_for_the_engine_is_scaled_to_fit() {
    let (w, h) = (400u32, 100u32);
    let rgba = vec![200u8; (w * h * 4) as usize];
    let (bgra, sw, sh) = to_bgra_within(&rgba, w, h, 100).unwrap();
    assert_eq!((sw, sh), (100, 25), "same shape, longest side at the limit");
    assert_eq!(bgra.len(), (sw * sh * 4) as usize);
    assert!(bgra.iter().all(|v| *v == 200));
    // Already small enough: untouched apart from the channel order.
    let (_, sw, sh) = to_bgra_within(&rgba, w, h, 400).unwrap();
    assert_eq!((sw, sh), (400, 100));
}

#[test]
fn a_buffer_that_does_not_match_its_size_is_refused() {
    assert!(to_bgra_within(&[0u8; 12], 2, 2, 4096).is_none());
    assert!(to_bgra_within(&[], 0, 0, 4096).is_none());
}

#[cfg(not(target_os = "windows"))]
#[test]
fn other_platforms_say_ocr_is_windows_only() {
    let err = screen_mark::ocr::extract_text(&[0, 0, 0, 255], 1, 1).unwrap_err();
    assert!(err.contains("Windows"), "{err}");
}

/// Black text on white, drawn with screen-mark's own font.
#[cfg(target_os = "windows")]
fn page(lines: &[&str], font_size: f32) -> (Vec<u8>, u32, u32) {
    use screen_mark::geometry::{Color, Point};
    use screen_mark::tools::{render_text_annotation, text_line_height};
    let (w, h) = (
        640u32,
        60 + lines.len() as u32 * text_line_height(font_size) as u32,
    );
    let mut pixmap = tiny_skia::Pixmap::new(w, h).unwrap();
    pixmap.fill(tiny_skia::Color::WHITE);
    render_text_annotation(
        &mut pixmap,
        &Point::new(30.0, 30.0),
        &lines.join("\n"),
        &Color::new(0.0, 0.0, 0.0, 1.0),
        font_size,
    );
    (pixmap.data().to_vec(), w, h)
}

#[cfg(target_os = "windows")]
#[test]
#[ignore = "needs a Windows OCR language (see the module comment)"]
fn windows_reads_text_line_by_line() {
    let (pixels, w, h) = page(&["Invoice 2048 is overdue", "Second line here"], 28.0);
    let text = screen_mark::ocr::extract_text(&pixels, w, h).expect("OCR runs");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "{text:?}");
    assert!(
        lines[0].contains("Invoice") && lines[0].contains("2048"),
        "{text:?}"
    );
    assert!(lines[1].contains("Second"), "{text:?}");
}

#[cfg(target_os = "windows")]
#[test]
#[ignore = "needs a Windows OCR language (see the module comment)"]
fn an_image_with_no_text_says_so() {
    let pixels = vec![255u8; 200 * 100 * 4];
    let err = screen_mark::ocr::extract_text(&pixels, 200, 100).unwrap_err();
    assert_eq!(err, "No text found in the selected region");
}

#[cfg(target_os = "windows")]
#[test]
#[ignore = "needs a Windows OCR language (see the module comment)"]
fn a_capture_wider_than_the_engine_allows_is_still_read() {
    // Wider than three 4K monitors side by side, and more than the
    // 10000 pixels the engine takes: it is scaled down first.
    let (small, w, h) = page(&["Wide capture"], 64.0);
    let (wide_w, wide_h) = (12_000u32, 1_400u32);
    let mut wide = vec![255u8; (wide_w * wide_h * 4) as usize];
    for y in 0..h as usize {
        let src = y * w as usize * 4;
        let dst = y * wide_w as usize * 4;
        wide[dst..dst + w as usize * 4].copy_from_slice(&small[src..src + w as usize * 4]);
    }
    let text = screen_mark::ocr::extract_text(&wide, wide_w, wide_h).expect("OCR runs");
    assert!(text.contains("Wide"), "{text:?}");
}
