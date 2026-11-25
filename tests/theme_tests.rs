//! The palette is Cyclorama Dawn v2.1.0 (Real-Fruit-Snacks/rfs-brand). These tests pin the
//! values that are verbatim from the brand tokens and check its one hard rule:
//! every accent passes WCAG AA (>= 4.5:1) as text on the page background.

use screen_mark::theme::{self, ThemeMode};
use std::sync::Mutex;

/// The theme mode is one setting for the whole process, and tests run in
/// parallel: a test that switches it holds this, or another test can switch
/// it between two reads (CI once measured Dark amber on the Light page).
static MODE: Mutex<()> = Mutex::new(());

fn lum((r, g, b): (u8, u8, u8)) -> f64 {
    fn ch(c: u8) -> f64 {
        let c = c as f64 / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * ch(r) + 0.7152 * ch(g) + 0.0722 * ch(b)
}

fn contrast(a: (u8, u8, u8), b: (u8, u8, u8)) -> f64 {
    let (x, y) = (lum(a), lum(b));
    let (hi, lo) = if x > y { (x, y) } else { (y, x) };
    (hi + 0.05) / (lo + 0.05)
}

#[test]
fn dark_matches_cyclorama_dawn() {
    let _mode = MODE.lock().unwrap_or_else(|e| e.into_inner());
    theme::set_mode(ThemeMode::Dark);
    assert_eq!(theme::bg_0(), (0x05, 0x05, 0x07), "bg");
    assert_eq!(theme::bg_1(), (0x0d, 0x0e, 0x12), "surface");
    assert_eq!(theme::bg_2(), (0x16, 0x18, 0x20), "raised");
    assert_eq!(theme::border(), (0x24, 0x28, 0x34), "line");
    assert_eq!(theme::text_normal(), (0xf2, 0xf4, 0xf8), "text");
    assert_eq!(theme::text_muted(), (0x8f, 0x95, 0xa6), "mute");
    assert_eq!(theme::accent(), (0xff, 0x92, 0x54), "Beacon Orange");
    assert_eq!(theme::accent_alt(), (0xff, 0xc2, 0x47), "Amber");
    assert_eq!(theme::red(), (0xff, 0x5c, 0x6c), "Signal Crimson");
}

#[test]
fn light_matches_cyclorama_dawn() {
    let _mode = MODE.lock().unwrap_or_else(|e| e.into_inner());
    theme::set_mode(ThemeMode::Light);
    assert_eq!(theme::bg_0(), (0xf5, 0xf7, 0xf9), "bg");
    assert_eq!(theme::bg_1(), (0xff, 0xff, 0xff), "surface");
    assert_eq!(theme::bg_2(), (0xe9, 0xee, 0xf3), "raised");
    assert_eq!(theme::border(), (0xcb, 0xd3, 0xdd), "line");
    assert_eq!(theme::text_normal(), (0x11, 0x14, 0x1b), "text");
    assert_eq!(theme::text_muted(), (0x53, 0x5b, 0x69), "mute");
    assert_eq!(theme::accent(), (0xad, 0x3d, 0x0a), "Beacon Orange");
    assert_eq!(theme::accent_alt(), (0x88, 0x58, 0x00), "Amber");
    assert_eq!(theme::red(), (0xb9, 0x1c, 0x2e), "Signal Crimson");
    theme::set_mode(ThemeMode::Dark);
}

#[test]
fn accents_pass_aa_on_every_surface_in_both_modes() {
    let _mode = MODE.lock().unwrap_or_else(|e| e.into_inner());
    for mode in [ThemeMode::Dark, ThemeMode::Light] {
        theme::set_mode(mode);
        for (name, fg) in [
            ("accent", theme::accent()),
            ("accent_alt", theme::accent_alt()),
            ("red", theme::red()),
            ("text_normal", theme::text_normal()),
            ("text_muted", theme::text_muted()),
        ] {
            // Every foreground token passes strict WCAG AA (>= 4.5:1) on both page and card.
            for (surf, bg) in [("bg_0", theme::bg_0()), ("bg_1", theme::bg_1())] {
                let c = contrast(fg, bg);
                assert!(
                    c >= 4.5,
                    "{mode:?}: {name} on {surf} is {c:.2}:1, needs 4.5:1"
                );
            }
        }
        // Text on an accent-filled control.
        let c = contrast(theme::text_on_accent(), theme::accent());
        assert!(c >= 4.5, "{mode:?}: text_on_accent on accent is {c:.2}:1");

        // Primary CTA white ink on both Steel -> Beacon Fill gradient stops.
        let c_steel = contrast(theme::CTA_INK, theme::steel());
        let c_beacon = contrast(theme::CTA_INK, theme::beacon_fill());
        assert!(
            c_steel >= 4.5 && c_beacon >= 4.5,
            "{mode:?}: CTA_INK contrast steel={c_steel:.2}:1 beacon_fill={c_beacon:.2}:1"
        );
    }
    theme::set_mode(ThemeMode::Dark);
}

#[test]
fn horizon_rule_runs_steel_to_beacon_in_both_modes() {
    let _mode = MODE.lock().unwrap_or_else(|e| e.into_inner());
    for mode in [ThemeMode::Dark, ThemeMode::Light] {
        theme::set_mode(mode);
        let mut p = tiny_skia::Pixmap::new(100, 4).unwrap();
        theme::draw_horizon_rule(&mut p, 0.0, 0.0, 100.0, 2.0);
        let left = p.pixel(0, 0).unwrap();
        let right = p.pixel(99, 0).unwrap();
        assert_eq!((left.red(), left.green(), left.blue()), theme::steel());
        let b = theme::beacon();
        assert!(
            (right.red() as i16 - b.0 as i16).abs() <= 4
                && (right.green() as i16 - b.1 as i16).abs() <= 4
                && (right.blue() as i16 - b.2 as i16).abs() <= 4,
            "{mode:?}: right edge ({},{},{}) did not reach beacon {:?}",
            right.red(),
            right.green(),
            right.blue(),
            b
        );
    }
    theme::set_mode(ThemeMode::Dark);
}

#[test]
fn annotation_presets_are_five_and_distinct() {
    use screen_mark::geometry::Color;
    let p = Color::presets();
    assert_eq!(p.len(), 5);
    for i in 0..p.len() {
        for j in (i + 1)..p.len() {
            assert_ne!(p[i], p[j], "preset {i} and {j} are the same color");
        }
    }
    assert_eq!(p[2], Color::green(), "green preset is Mint");
    assert_eq!(p[3], Color::yellow(), "yellow preset is Amber");
}
