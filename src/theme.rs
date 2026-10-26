//! RFS palette (Nightshift dark / Dayshift light, accent Ember) and the
//! process-global active mode. Source of truth: Real-Fruit-Snacks/rfs-brand/BRAND.md.
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

// Nightshift (default). bg_3/bg_4 and border_strong are one-step derivations
// of the BRAND.md ramp; everything else is verbatim.
const DARK: Palette = Palette {
    bg_0: (0x0c, 0x0d, 0x10),
    bg_1: (0x13, 0x15, 0x1a),
    bg_2: (0x1a, 0x1d, 0x24),
    bg_3: (0x21, 0x25, 0x2d),
    bg_4: (0x2a, 0x2f, 0x38),
    border: (0x26, 0x2a, 0x33),
    border_strong: (0x35, 0x3a, 0x45),
    text_normal: (0xe8, 0xe6, 0xe1),
    text_soft: (0xc2, 0xc0, 0xba),
    text_muted: (0x8a, 0x8f, 0x99),
    text_faint: (0x5e, 0x63, 0x6c),
    text_on_accent: (0x0c, 0x0d, 0x10),
    accent: (0xff, 0x6a, 0x3d),
    accent_alt: (0xff, 0xc2, 0x47),
    warm: (0xff, 0xc2, 0x47),
    red: (0xff, 0x6e, 0x7a),
    orange: (0xff, 0xc2, 0x47),
    violet: (0xb7, 0x8c, 0xff),
};

// Dayshift.
const LIGHT: Palette = Palette {
    bg_0: (0xf6, 0xf5, 0xf2),
    bg_1: (0xff, 0xff, 0xff),
    bg_2: (0xec, 0xea, 0xe4),
    bg_3: (0xe6, 0xe3, 0xdc),
    bg_4: (0xdd, 0xd9, 0xd1),
    border: (0xd6, 0xd3, 0xcb),
    border_strong: (0xbd, 0xb9, 0xb0),
    text_normal: (0x1a, 0x1b, 0x1e),
    text_soft: (0x3a, 0x3c, 0x42),
    text_muted: (0x6b, 0x6f, 0x78),
    text_faint: (0x9a, 0x9e, 0xa6),
    text_on_accent: (0xff, 0xff, 0xff),
    accent: (0xc4, 0x3f, 0x11),
    accent_alt: (0x9a, 0x67, 0x00),
    warm: (0x9a, 0x67, 0x00),
    red: (0xc8, 0x32, 0x4c),
    orange: (0x9a, 0x67, 0x00),
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
        assert_eq!(accent(), (0xff, 0x6a, 0x3d));
        assert_eq!(bg_0(), (0x0c, 0x0d, 0x10));
        assert_eq!(text_normal(), (0xe8, 0xe6, 0xe1));

        set_mode(ThemeMode::Light);
        assert_eq!(accent(), (0xc4, 0x3f, 0x11));
        assert_eq!(bg_0(), (0xf6, 0xf5, 0xf2));
        assert_eq!(text_normal(), (0x1a, 0x1b, 0x1e));

        set_mode(ThemeMode::Dark);
    }

    #[test]
    fn hex_formats_lowercase_six_digits() {
        assert_eq!(hex((0xff, 0x6a, 0x3d)), "#ff6a3d");
    }

    #[test]
    fn skia_applies_alpha() {
        let c = skia((0xff, 0x00, 0x00), 0.5);
        assert!((c.red() - 1.0).abs() < 0.01);
        assert!((c.alpha() - 0.5).abs() < 0.01);
    }
}
