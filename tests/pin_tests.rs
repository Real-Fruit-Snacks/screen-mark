//! A pin's frame is how it looks on screen; it is not part of the capture.

use screen_mark::pin::frame;

#[test]
fn the_frame_goes_around_the_capture_without_changing_it() {
    // A 4 x 3 capture where every pixel is different.
    let (w, h) = (4u32, 3u32);
    let image: Vec<u8> = (0..w * h)
        .flat_map(|i| [i as u8 * 10, 100, 200 - i as u8, 255])
        .collect();
    let accent = (255, 106, 61);
    let (framed, fw, fh) = frame(&image, w, h, accent);

    // Three pixels of border all round, two of shadow below and right.
    assert_eq!((fw, fh), (w + 8, h + 8));
    assert_eq!(framed.len(), (fw * fh * 4) as usize);
    let at = |x: u32, y: u32| {
        let i = ((y * fw + x) * 4) as usize;
        [framed[i], framed[i + 1], framed[i + 2], framed[i + 3]]
    };
    assert_eq!(at(0, 0), [255, 106, 61, 255], "border");
    assert_eq!(at(w + 5, h + 5), [255, 106, 61, 255], "border, far corner");
    assert_eq!(at(fw - 1, fh - 1), [0, 0, 0, 100], "shadow");
    assert_eq!(at(fw - 1, 0), [0, 0, 0, 0], "nothing above the shadow");

    // The capture sits inside, pixel for pixel.
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            assert_eq!(at(x + 3, y + 3), image[i..i + 4], "pixel ({x}, {y})");
        }
    }
}
