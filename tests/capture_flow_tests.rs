//! The one-press screenshot path without a display: which monitor, the crop,
//! the unique file name, what the popup says, and key-repeat handling.

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use screen_mark::actions::{describe, finish_capture};
use screen_mark::capture::CapturedScreen;
use screen_mark::display::{crop, monitor_at, Monitor, Rect};
use screen_mark::export::{capture_file_stem, save_capture, write_unique};
use screen_mark::hotkey::repeat_filter;
use screen_mark::toast::{self, Click, Kind};

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("sm-flow-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

/// Two 1920x1080 monitors, the left one at negative x (a common layout
/// when the second screen sits to the left of the primary).
fn two_monitors() -> Vec<Monitor> {
    let left = Rect::new(-1920, 0, 1920, 1080);
    let right = Rect::new(0, 0, 1920, 1080);
    vec![
        Monitor {
            area: right,
            work: Rect::new(0, 0, 1920, 1040),
        },
        Monitor {
            area: left,
            work: Rect::new(-1920, 0, 1920, 1040),
        },
    ]
}

/// A virtual-desktop capture where each pixel encodes its own x position,
/// so a crop can be checked by reading a pixel back.
fn desktop(x0: i32, y0: i32, w: u32, h: u32) -> CapturedScreen {
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let vx = x as i32 + x0;
            pixels[i] = (vx & 0xff) as u8;
            pixels[i + 1] = ((vx >> 8) & 0xff) as u8;
            pixels[i + 2] = y as u8;
            pixels[i + 3] = 255;
        }
    }
    CapturedScreen {
        pixels,
        width: w,
        height: h,
        x_offset: x0,
        y_offset: y0,
        scale_factor: 1.0,
    }
}

fn vx_at(s: &CapturedScreen, x: u32, y: u32) -> i32 {
    let i = ((y * s.width + x) * 4) as usize;
    let v = s.pixels[i] as i32 | (s.pixels[i + 1] as i32) << 8;
    if v >= 0x8000 {
        v - 0x10000
    } else {
        v
    }
}

// ───────────── monitors ─────────────

#[test]
fn picks_the_monitor_under_the_pointer() {
    let m = two_monitors();
    assert_eq!(monitor_at(&m, 100, 100).unwrap().area.x, 0);
    assert_eq!(monitor_at(&m, -100, 500).unwrap().area.x, -1920);
    // Edges: x = 0 belongs to the right monitor, -1 to the left.
    assert_eq!(monitor_at(&m, 0, 0).unwrap().area.x, 0);
    assert_eq!(monitor_at(&m, -1, 0).unwrap().area.x, -1920);
}

#[test]
fn a_pointer_outside_every_monitor_uses_the_nearest() {
    let m = two_monitors();
    // Below the right monitor (in the gap a taller neighbour would leave).
    assert_eq!(monitor_at(&m, 300, 1500).unwrap().area.x, 0);
    assert_eq!(monitor_at(&m, -5000, 10).unwrap().area.x, -1920);
    assert!(monitor_at(&[], 0, 0).is_none());
}

#[test]
fn an_empty_monitor_rectangle_does_not_panic() {
    // A disconnected output can report 0x0.
    let empty = Monitor {
        area: Rect::new(100, 100, 0, 0),
        work: Rect::new(100, 100, 0, 0),
    };
    assert_eq!(monitor_at(&[empty], 5000, 5000), Some(empty));
}

#[test]
fn system_notifications_drop_the_click_hint() {
    let p = Path::new("/tmp/x/screen-mark_1.png").to_path_buf();
    let spec = describe(Some(Ok(p)), Some(Ok(())), 640, 480);
    assert_eq!(
        spec.hint.strip_suffix(toast::CLICK_HINT),
        Some("640 \u{d7} 480")
    );
}

#[test]
fn crops_one_monitor_out_of_the_virtual_desktop() {
    let full = desktop(-1920, 0, 3840, 1080);
    let right = crop(&full, Rect::new(0, 0, 1920, 1080)).unwrap();
    assert_eq!((right.width, right.height, right.x_offset), (1920, 1080, 0));
    assert_eq!(vx_at(&right, 0, 0), 0);
    assert_eq!(vx_at(&right, 1919, 5), 1919);
    let left = crop(&full, Rect::new(-1920, 0, 1920, 1080)).unwrap();
    assert_eq!(vx_at(&left, 0, 0), -1920);
    assert_eq!(left.pixels.len(), 1920 * 1080 * 4);
}

#[test]
fn crop_clips_and_handles_no_overlap() {
    let full = desktop(0, 0, 100, 50);
    let part = crop(&full, Rect::new(90, 40, 30, 30)).unwrap();
    assert_eq!((part.width, part.height), (10, 10));
    assert_eq!(vx_at(&part, 0, 0), 90);
    assert!(crop(&full, Rect::new(200, 0, 10, 10)).is_none());
    let same = crop(&full, Rect::new(0, 0, 100, 50)).unwrap();
    assert_eq!(same.pixels, full.pixels);
}

// ───────────── saving ─────────────

#[test]
fn file_names_are_timestamped() {
    use chrono::TimeZone;
    let when = chrono::Local
        .with_ymd_and_hms(2026, 10, 28, 14, 30, 12)
        .unwrap();
    assert_eq!(capture_file_stem(when), "screen-mark_2026-10-28_143012");
}

#[test]
fn never_overwrites_a_capture_from_the_same_second() {
    let dir = temp_dir("unique");
    let a = write_unique(&dir, "shot", b"first").unwrap();
    let b = write_unique(&dir, "shot", b"second").unwrap();
    let c = write_unique(&dir, "shot", b"third").unwrap();
    assert_eq!(a.file_name().unwrap(), "shot.png");
    assert_eq!(b.file_name().unwrap(), "shot_2.png");
    assert_eq!(c.file_name().unwrap(), "shot_3.png");
    assert_eq!(std::fs::read(&a).unwrap(), b"first");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn parallel_saves_get_distinct_files() {
    let dir = temp_dir("parallel");
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let dir = dir.clone();
            std::thread::spawn(move || {
                write_unique(&dir, "burst", format!("{i}").as_bytes()).unwrap()
            })
        })
        .collect();
    let mut names: Vec<String> = handles
        .into_iter()
        .map(|h| {
            h.join()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string()
        })
        .collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), 8);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn creates_the_folder_and_writes_a_real_png() {
    let dir = temp_dir("png").join("nested").join("deeper");
    let shot = desktop(0, 0, 40, 30);
    let path = save_capture(&shot.pixels, 40, 30, &dir).unwrap();
    let img = image::open(&path).unwrap().to_rgba8();
    assert_eq!(img.dimensions(), (40, 30));
    assert_eq!(img.get_pixel(7, 3).0[0], 7);
    let _ = std::fs::remove_dir_all(temp_dir("png"));
}

#[test]
fn a_folder_that_is_a_file_gives_a_readable_error() {
    let dir = temp_dir("isfile");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("not-a-folder");
    std::fs::write(&file, b"x").unwrap();
    let err = write_unique(&file, "shot", b"png").unwrap_err();
    assert!(err.contains("not-a-folder"), "{err}");
    // Same words on every OS (Windows says "already exists" / "not found"),
    // for the folder itself and for one further down.
    assert!(err.contains("a file is in the way"), "{err}");
    let err = write_unique(&file.join("deeper"), "shot", b"png").unwrap_err();
    assert!(err.contains("a file is in the way"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

// ───────────── what the popup says ─────────────

#[test]
fn describes_every_save_and_copy_outcome() {
    let p = Path::new("/tmp/x/screen-mark_1.png").to_path_buf();
    let both = describe(Some(Ok(p.clone())), Some(Ok(())), 1920, 1080);
    assert_eq!(
        (both.kind, both.title.as_str()),
        (Kind::Success, "Saved and copied")
    );
    assert_eq!(both.detail, "screen-mark_1.png");
    assert!(both.hint.contains("1920 \u{d7} 1080"));
    assert_eq!(both.click, Click::ShowFile(p.clone()));

    let saved = describe(Some(Ok(p.clone())), None, 10, 10);
    assert_eq!(saved.title, "Saved");

    let copied = describe(None, Some(Ok(())), 10, 20);
    assert_eq!(
        (copied.title.as_str(), copied.detail.as_str()),
        ("Copied", "10 \u{d7} 20")
    );
    assert_eq!(copied.click, Click::Nothing);

    let disk = describe(
        Some(Err("Can't write in D:\\: permission denied".into())),
        Some(Ok(())),
        1,
        1,
    );
    assert_eq!(
        (disk.kind, disk.title.as_str()),
        (Kind::Error, "Copied, but not saved")
    );
    assert!(disk.detail.contains("permission denied"));

    let clip = describe(Some(Ok(p.clone())), Some(Err("busy".into())), 1, 1);
    assert_eq!(
        (clip.kind, clip.title.as_str()),
        (Kind::Error, "Saved, but not copied")
    );
    assert_eq!(clip.click, Click::ShowFile(p));

    let neither = describe(Some(Err("disk".into())), Some(Err("clip".into())), 1, 1);
    assert_eq!(neither.title, "Couldn't save");
    let nocopy = describe(None, Some(Err("clip".into())), 1, 1);
    assert_eq!(nocopy.title, "Couldn't copy");
}

#[test]
fn finish_capture_saves_and_attaches_a_thumbnail() {
    let dir = temp_dir("finish");
    let shot = desktop(0, 0, 640, 360);
    let spec = finish_capture(&shot.pixels, 640, 360, Some(&dir), false, false);
    assert_eq!(spec.title, "Saved");
    let Click::ShowFile(path) = spec.click.clone() else {
        panic!("expected a file to show: {spec:?}");
    };
    assert!(path.starts_with(&dir));
    assert!(path.exists());
    let th = spec.thumb.expect("thumbnail");
    assert!(
        th.w <= 192 && th.h <= 120 && th.w >= 190,
        "{}x{}",
        th.w,
        th.h
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn finish_capture_reports_an_unwritable_folder() {
    let dir = temp_dir("blocked");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("file");
    std::fs::write(&file, b"x").unwrap();
    let shot = desktop(0, 0, 8, 8);
    let spec = finish_capture(&shot.pixels, 8, 8, Some(&file), false, false);
    assert_eq!(
        (spec.kind, spec.title.as_str()),
        (Kind::Error, "Couldn't save")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ───────────── popup drawing ─────────────

#[test]
fn thumbnails_keep_aspect_and_never_upscale() {
    let wide = desktop(0, 0, 4000, 10);
    let t = toast::make_thumb(&wide.pixels, 4000, 10, 192, 120).unwrap();
    assert_eq!((t.w, t.h), (192, 1));
    let small = desktop(0, 0, 20, 10);
    let t = toast::make_thumb(&small.pixels, 20, 10, 192, 120).unwrap();
    assert_eq!((t.w, t.h), (20, 10));
    assert!(toast::make_thumb(&[], 0, 0, 10, 10).is_none());
    assert!(toast::make_thumb(&[0; 3], 1, 1, 10, 10).is_none());
}

#[test]
fn long_names_are_shortened_in_the_middle() {
    let name = "screen-mark_2026-10-28_143012_with_a_very_long_suffix_17.png";
    let short = toast::fit_middle(name, 12.0, 150.0);
    assert!(short.contains('\u{2026}'));
    assert!(short.ends_with(".png"), "{short}");
    assert!(screen_mark::tools::measure_text_width(&short, 12.0) <= 150.0);
    assert_eq!(toast::fit_middle("short.png", 12.0, 300.0), "short.png");
}

#[test]
fn the_card_scales_and_shows_its_kind() {
    for scale in [1.0f32, 1.25, 1.5, 2.0] {
        let p = toast::render(
            &toast::Spec::new(Kind::Success, "Saved and copied", "x.png"),
            scale,
        );
        assert_eq!(p.width(), (toast::W * scale).round() as u32);
        assert_eq!(p.height(), (toast::H * scale).round() as u32);
    }
    let bar = |kind| {
        let p = toast::render(&toast::Spec::new(kind, "t", "d"), 1.0);
        let px = p.pixel(2, 40).unwrap();
        (px.red(), px.green(), px.blue())
    };
    assert_eq!(bar(Kind::Success), screen_mark::theme::accent());
    assert_eq!(bar(Kind::Error), screen_mark::theme::red());
    assert_ne!(bar(Kind::Info), bar(Kind::Success));
}

#[test]
fn the_card_sits_bottom_right_of_the_work_area() {
    let m = Monitor {
        area: Rect::new(0, 0, 1920, 1080),
        work: Rect::new(0, 0, 1920, 1040),
    };
    let (x, y) = toast::position(Some(&m), 1.0);
    assert_eq!(x, 1920 - 380 - 16);
    assert_eq!(y, 1040 - 84 - 16);
    let left = Monitor {
        area: Rect::new(-1920, 0, 1920, 1080),
        work: Rect::new(-1920, 0, 1920, 1040),
    };
    let (x, _) = toast::position(Some(&left), 2.0);
    assert_eq!(x, -760 - 32);
}

// ───────────── key repeat ─────────────

#[test]
fn holding_a_hotkey_captures_once() {
    let mut held = HashMap::new();
    let t0 = Instant::now();
    assert!(repeat_filter(&mut held, 7, true, t0));
    // Auto-repeat while held.
    assert!(!repeat_filter(
        &mut held,
        7,
        true,
        t0 + Duration::from_millis(30)
    ));
    assert!(!repeat_filter(
        &mut held,
        7,
        true,
        t0 + Duration::from_millis(600)
    ));
    // Release, press again: a new capture.
    assert!(!repeat_filter(
        &mut held,
        7,
        false,
        t0 + Duration::from_millis(700)
    ));
    assert!(repeat_filter(
        &mut held,
        7,
        true,
        t0 + Duration::from_millis(710)
    ));
    // Another hotkey is independent.
    assert!(repeat_filter(
        &mut held,
        8,
        true,
        t0 + Duration::from_millis(720)
    ));
}

#[test]
fn a_lost_release_does_not_block_the_hotkey_forever() {
    let mut held = HashMap::new();
    let t0 = Instant::now();
    assert!(repeat_filter(&mut held, 1, true, t0));
    assert!(repeat_filter(
        &mut held,
        1,
        true,
        t0 + Duration::from_secs(2)
    ));
}
