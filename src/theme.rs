//! RFS Cyclorama Dawn v2.1.0 palette (LX 263 dark / LX 512 light, accent Beacon Orange) and the
//! process-global active mode. Source of truth: Real-Fruit-Snacks/rfs-brand.
//! Every rendered surface reads its colors through these accessors so a
//! single `set_mode` call re-themes the whole app.

use std::sync::atomic::{AtomicU8, Ordering};

type Rgb = (u8, u8, u8);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThemeMode {
    Dark,
    Light,
}

static ACTIVE: AtomicU8 = AtomicU8::new(0); // 0 = Dark, 1 = Light

pub fn set_mode(mode: ThemeMode) {
    let v = match mode {
        ThemeMode::Dark => 0,
        ThemeMode::Light => 1,
    };
    ACTIVE.store(v, Ordering::Relaxed);
}

pub fn mode() -> ThemeMode {
    if ACTIVE.load(Ordering::Relaxed) == 1 {
        ThemeMode::Light
    } else {
        ThemeMode::Dark
    }
}

struct Palette {
    bg_0: Rgb,
    bg_1: Rgb,
    bg_2: Rgb,
    bg_3: Rgb,
    bg_4: Rgb,
    border: Rgb,
    border_strong: Rgb,
    text_normal: Rgb,
    text_soft: Rgb,
    text_muted: Rgb,
    text_faint: Rgb,
    text_on_accent: Rgb,
    accent: Rgb,
    accent_alt: Rgb,
    warm: Rgb,
    red: Rgb,
    orange: Rgb,
    violet: Rgb,
}

// Cyclorama Dawn LX 263 Dark (default). bg_3/bg_4 are one-step derivations
// of the rfs-brand v2.1.0 surface ramp; everything else is verbatim.
const DARK: Palette = Palette {
    bg_0: (0x05, 0x05, 0x07),
    bg_1: (0x0d, 0x0e, 0x12),
    bg_2: (0x16, 0x18, 0x20),
    bg_3: (0x1e, 0x21, 0x2c),
    bg_4: (0x28, 0x2c, 0x3a),
    border: (0x24, 0x28, 0x34),
    border_strong: (0x5d, 0x64, 0x76),
    text_normal: (0xf2, 0xf4, 0xf8),
    text_soft: (0xc8, 0xcc, 0xd8),
    text_muted: (0x8f, 0x95, 0xa6),
    text_faint: (0x5f, 0x65, 0x75),
    text_on_accent: (0x05, 0x05, 0x07),
    accent: (0xff, 0x92, 0x54),
    accent_alt: (0xff, 0xc2, 0x47),
    warm: (0xff, 0xc2, 0x47),
    red: (0xff, 0x5c, 0x6c),
    orange: (0xff, 0xc2, 0x47),
    violet: (0xb7, 0x8c, 0xff),
};

// Cyclorama Dawn LX 512 Light.
const LIGHT: Palette = Palette {
    bg_0: (0xf5, 0xf7, 0xf9),
    bg_1: (0xff, 0xff, 0xff),
    bg_2: (0xe9, 0xee, 0xf3),
    bg_3: (0xdf, 0xe6, 0xed),
    bg_4: (0xd4, 0xdc, 0xe5),
    border: (0xcb, 0xd3, 0xdd),
    border_strong: (0x76, 0x80, 0x8f),
    text_normal: (0x11, 0x14, 0x1b),
    text_soft: (0x33, 0x39, 0x46),
    text_muted: (0x53, 0x5b, 0x69),
    text_faint: (0x8c, 0x94, 0xa2),
    text_on_accent: (0xff, 0xff, 0xff),
    accent: (0xad, 0x3d, 0x0a),
    accent_alt: (0x88, 0x58, 0x00),
    warm: (0x88, 0x58, 0x00),
    red: (0xb9, 0x1c, 0x2e),
    orange: (0x88, 0x58, 0x00),
    violet: (0x73, 0x57, 0xb8),
};

fn active() -> &'static Palette {
    match mode() {
        ThemeMode::Dark => &DARK,
        ThemeMode::Light => &LIGHT,
    }
}

macro_rules! token {
    ($name:ident) => {
        pub fn $name() -> Rgb {
            active().$name
        }
    };
}
token!(bg_0);
token!(bg_1);
token!(bg_2);
token!(bg_3);
token!(bg_4);
token!(border);
token!(border_strong);
token!(text_normal);
token!(text_soft);
token!(text_muted);
token!(text_faint);
token!(text_on_accent);
token!(accent);
token!(accent_alt);
token!(warm);
token!(red);
token!(orange);
token!(violet);

/// Convert a token to a `tiny_skia::Color` at the given alpha (0.0..=1.0).
pub fn skia(rgb: Rgb, alpha: f32) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba(
        rgb.0 as f32 / 255.0,
        rgb.1 as f32 / 255.0,
        rgb.2 as f32 / 255.0,
        alpha,
    )
    .unwrap_or(tiny_skia::Color::WHITE)
}

/// Convert a token to the crate's own `geometry::Color` at the given alpha.
pub fn gcolor(rgb: Rgb, alpha: f32) -> crate::geometry::Color {
    crate::geometry::Color::new(
        rgb.0 as f32 / 255.0,
        rgb.1 as f32 / 255.0,
        rgb.2 as f32 / 255.0,
        alpha,
    )
}

/// Lowercase `#rrggbb` string (for the SVG icon renderer).
pub fn hex(rgb: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb.0, rgb.1, rgb.2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_and_light_differ_and_match_spec() {
        set_mode(ThemeMode::Dark);
        assert_eq!(accent(), (0xff, 0x92, 0x54));
        assert_eq!(bg_0(), (0x05, 0x05, 0x07));
        assert_eq!(text_normal(), (0xf2, 0xf4, 0xf8));

        set_mode(ThemeMode::Light);
        assert_eq!(accent(), (0xad, 0x3d, 0x0a));
        assert_eq!(bg_0(), (0xf5, 0xf7, 0xf9));
        assert_eq!(text_normal(), (0x11, 0x14, 0x1b));

        set_mode(ThemeMode::Dark);
    }

    #[test]
    fn hex_formats_lowercase_six_digits() {
        assert_eq!(hex((0xff, 0x92, 0x54)), "#ff9254");
    }

    #[test]
    fn skia_applies_alpha() {
        let c = skia((0xff, 0x00, 0x00), 0.5);
        assert!((c.red() - 1.0).abs() < 0.01);
        assert!((c.alpha() - 0.5).abs() < 0.01);
    }
}
