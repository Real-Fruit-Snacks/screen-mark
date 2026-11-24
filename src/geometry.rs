#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    pub fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self {
            r: r.clamp(0.0, 1.0),
            g: g.clamp(0.0, 1.0),
            b: b.clamp(0.0, 1.0),
            a: a.clamp(0.0, 1.0),
        }
    }

    // Annotation ink. Named presets keep their hue so existing config files
    // keep meaning the same thing; red is Signal Crimson, blue is Glacial Steel,
    // green is Mint, and yellow is Amber from Cyclorama Dawn v2.1.0.
    pub fn red() -> Self {
        Self::new(1.0, 0.361, 0.424, 1.0) // #ff5c6c (Signal Crimson)
    }

    pub fn blue() -> Self {
        Self::new(0.455, 0.722, 0.910, 1.0) // #74b8e8 (Glacial Steel)
    }

    pub fn green() -> Self {
        Self::new(0.239, 0.863, 0.592, 1.0) // #3ddc97 (Mint)
    }

    pub fn yellow() -> Self {
        Self::new(1.0, 0.761, 0.278, 1.0) // #ffc247 (Amber)
    }

    pub fn white() -> Self {
        Self::new(0.949, 0.957, 0.973, 1.0) // #f2f4f8 (text)
    }

    pub fn mauve() -> Self {
        Self::new(0.718, 0.549, 1.0, 1.0) // #b78cff (violet)
    }

    pub fn peach() -> Self {
        Self::new(1.0, 0.573, 0.329, 1.0) // #ff9254 (Beacon Orange)
    }

    pub fn teal() -> Self {
        Self::new(0.455, 0.722, 0.910, 1.0) // #74b8e8
    }

    pub fn sky() -> Self {
        Self::new(0.455, 0.722, 0.910, 1.0) // #74b8e8
    }

    pub fn lavender() -> Self {
        Self::new(0.239, 0.863, 0.592, 1.0) // #3ddc97 (Mint)
    }

    pub fn presets() -> &'static [Self] {
        static PRESETS: &[Color] = &[
            Color {
                r: 1.0,
                g: 0.361,
                b: 0.424,
                a: 1.0,
            }, // red #ff5c6c
            Color {
                r: 0.455,
                g: 0.722,
                b: 0.910,
                a: 1.0,
            }, // blue #74b8e8
            Color {
                r: 0.239,
                g: 0.863,
                b: 0.592,
                a: 1.0,
            }, // green #3ddc97
            Color {
                r: 1.0,
                g: 0.761,
                b: 0.278,
                a: 1.0,
            }, // yellow #ffc247
            Color {
                r: 0.718,
                g: 0.549,
                b: 1.0,
                a: 1.0,
            }, // mauve #b78cff
        ];
        PRESETS
    }
}

impl From<Color> for tiny_skia::Color {
    fn from(c: Color) -> Self {
        tiny_skia::Color::from_rgba(c.r, c.g, c.b, c.a).unwrap_or(tiny_skia::Color::BLACK)
    }
}
