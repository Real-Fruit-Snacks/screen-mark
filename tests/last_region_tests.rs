//! The last-region hotkey without a display: where a region capture was on
//! screen, how that is remembered across restarts, and that capturing the
//! same area again gives the same pixels as the original selection.

use screen_mark::capture::CapturedScreen;
use screen_mark::config::Config;
use screen_mark::display::{crop, Rect};
use screen_mark::keys::{check, KeyId, Scope};
use screen_mark::last_region::{from_line, load_from, save_to, to_line};
use screen_mark::overlay::selection::Selection;
use screen_mark::state::OverlayState;

/// A virtual-desktop capture where each pixel encodes its own position, so
/// a crop can be checked by reading pixels back.
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

fn sel(x: f32, y: f32, width: f32, height: f32) -> Option<Selection> {
    Some(Selection {
        x,
        y,
        width,
        height,
    })
}

/// Two 400x300 monitors, the left one at negative x.
fn two_monitor_overlay() -> OverlayState {
    OverlayState::new(desktop(-400, 0, 800, 300), &Config::default())
}

fn temp_file(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("sm-last-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d.join("nested").join("last-region.txt")
}

#[test]
fn a_line_round_trips_including_negative_positions() {
    let r = Rect::new(-1500, 40, 800, 600);
    assert_eq!(to_line(r), "-1500 40 800 600");
    assert_eq!(from_line(&to_line(r)), Some(r));
    assert_eq!(from_line("  -1500 40 800 600\r\n"), Some(r));
}

#[test]
fn anything_else_reads_as_no_region() {
    for bad in [
        "",
        "1 2 3",
        "1 2 3 4 5",
        "a b c d",
        "0 0 0 10",
        "0 0 10 0",
        "0 0 -5 10",
    ] {
        assert_eq!(from_line(bad), None, "{bad:?}");
    }
}

#[test]
fn saving_creates_the_folder_and_loading_reads_it_back() {
    let path = temp_file("save");
    assert_eq!(load_from(&path), None, "nothing saved yet");
    let first = Rect::new(10, 20, 300, 200);
    save_to(&path, first).unwrap();
    assert_eq!(load_from(&path), Some(first));
    let second = Rect::new(-1920, 0, 1920, 1080);
    save_to(&path, second).unwrap();
    assert_eq!(
        load_from(&path),
        Some(second),
        "a newer capture replaces it"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
}

#[test]
fn the_selection_is_placed_in_virtual_desktop_pixels() {
    let mut overlay = two_monitor_overlay();
    assert_eq!(overlay.selection_on_screen(), None, "no selection yet");
    // Straddles the two monitors: starts 20 px left of x = 0.
    overlay.selection = sel(380.0, 50.0, 100.0, 60.0);
    assert_eq!(
        overlay.selection_on_screen(),
        Some(Rect::new(-20, 50, 100, 60))
    );
}

#[test]
fn a_selection_past_the_edge_is_clipped_and_an_empty_one_is_none() {
    let mut overlay = two_monitor_overlay();
    overlay.selection = sel(750.0, 280.0, 100.0, 60.0);
    assert_eq!(
        overlay.selection_on_screen(),
        Some(Rect::new(350, 280, 50, 20))
    );
    overlay.selection = sel(50.0, 50.0, 0.0, 30.0);
    assert_eq!(overlay.selection_on_screen(), None);
}

#[test]
fn capturing_the_area_again_gives_the_same_pixels_as_the_selection() {
    let mut overlay = two_monitor_overlay();
    overlay.selection = sel(370.0, 40.0, 70.0, 90.0);
    let (raw, w, h) = overlay.raw_selection().unwrap();
    let rect = overlay.selection_on_screen().unwrap();
    // Later: a fresh capture of the same desktop, cut at the remembered area.
    let again = crop(&desktop(-400, 0, 800, 300), rect).unwrap();
    assert_eq!((again.width, again.height), (w, h));
    assert_eq!((again.x_offset, again.y_offset), (rect.x, rect.y));
    assert_eq!(again.pixels, raw);
}

#[test]
fn an_area_on_a_monitor_that_is_gone_crops_to_nothing() {
    // Remembered on the left monitor, which has since been unplugged.
    let rect = Rect::new(-300, 100, 200, 100);
    assert!(crop(&desktop(0, 0, 400, 300), rect).is_none());
}

#[test]
fn the_hotkey_is_global_with_a_default_that_clashes_with_nothing() {
    assert_eq!(KeyId::LastRegion.scope(), Scope::Global);
    assert_eq!(KeyId::LastRegion.default_binding(), "Shift+PrintScreen");
    assert_eq!(&KeyId::all()[..3], &KeyId::GLOBALS[..]);
    let config = Config::default();
    assert_eq!(config.hotkey.last_region, "Shift+PrintScreen");
    let report = config.key_report();
    assert!(
        report.is_ok(),
        "{:?} {:?}",
        report.invalid,
        report.conflicts
    );
}

#[test]
fn it_cannot_share_a_combo_with_another_global_hotkey() {
    let report = check(|id| match id {
        KeyId::LastRegion => "Ctrl+PrintScreen".to_string(),
        _ => id.default_binding().to_string(),
    });
    let problem = report.problem(KeyId::LastRegion).expect("a conflict");
    assert!(problem.contains("Screenshot"), "{problem}");
}

#[test]
fn a_config_from_before_the_hotkey_existed_gets_the_default() {
    let cfg: Config = toml::from_str("[hotkey]\nregion = \"Ctrl+Alt+R\"\n").unwrap();
    assert_eq!(cfg.hotkey.region, "Ctrl+Alt+R");
    assert_eq!(cfg.hotkey.last_region, "Shift+PrintScreen");
    let mut cfg = cfg;
    cfg.set_binding(KeyId::LastRegion, String::new());
    assert_eq!(cfg.binding(KeyId::LastRegion), "", "can be unbound");
    let written = toml::to_string(&cfg).unwrap();
    assert!(written.contains("last_region = \"\""), "{written}");
}
