//! Fonts.
//!
//! The embedded font draws the whole UI and any Latin text. A character it
//! has no glyph for (Chinese, Japanese, Korean, emoji, ...) is looked up in
//! the fonts installed on the system the first time it is needed, so text
//! annotations aren't limited to what a 500 KB font holds.
//!
//! Nothing is read until such a character turns up. A system font is then
//! used one glyph at a time, straight from its outlines: parsing a whole
//! CJK font up front (as the embedded font is) takes seconds. What was
//! loaded is freed by `release_fallbacks` once the overlay closes.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex};

use fontdue::Metrics;

pub static FONT: LazyLock<fontdue::Font> = LazyLock::new(|| {
    static FONT_DATA: &[u8] = include_bytes!("../assets/font.ttf");
    fontdue::Font::from_bytes(FONT_DATA as &[u8], fontdue::FontSettings::default())
        .expect("Failed to load embedded font")
});

/// True if the embedded font draws `ch` (or `ch` draws nothing at all).
fn embedded(ch: char) -> bool {
    ch.is_ascii() || FONT.has_glyph(ch) || ch.is_control() || ch.is_whitespace()
}

/// True if some font, embedded or installed, has a glyph for `ch`.
pub fn has_glyph(ch: char) -> bool {
    embedded(ch) || lock().face_for(ch).is_some()
}

/// Advance width and bitmap placement of `ch` at `size`.
pub fn metrics(ch: char, size: f32) -> Metrics {
    if embedded(ch) {
        return FONT.metrics(ch, size);
    }
    let mut fb = lock();
    match fb.face_for(ch) {
        Some(face) => face.metrics(ch, size).unwrap_or_default(),
        // No font has it: the embedded font's "missing glyph" box.
        None => FONT.metrics(ch, size),
    }
}

/// Coverage bitmap of `ch` at `size` (one byte per pixel, row by row),
/// with the metrics that say where it goes.
pub fn rasterize(ch: char, size: f32) -> (Metrics, Vec<u8>) {
    if embedded(ch) {
        return FONT.rasterize(ch, size);
    }
    let mut fb = lock();
    let Some(face) = fb.face_for(ch) else {
        return FONT.rasterize(ch, size);
    };
    let key = (ch, size.to_bits());
    if let Some(hit) = fb.glyphs.get(&key) {
        return hit.clone();
    }
    let glyph = face.rasterize(ch, size).unwrap_or_default();
    if fb.glyphs.len() >= MAX_CACHED_GLYPHS {
        fb.glyphs.clear();
    }
    fb.glyphs.insert(key, glyph.clone());
    glyph
}

/// Height of the embedded font above the baseline at `size`.
pub fn ascent(size: f32) -> f32 {
    FONT.horizontal_line_metrics(size)
        .map(|m| m.ascent)
        .unwrap_or(size * 0.8)
}

/// Ascent plus descent: the height of one line of text at `size`.
pub fn text_height(size: f32) -> f32 {
    FONT.horizontal_line_metrics(size)
        .map(|m| m.ascent + m.descent.abs())
        .unwrap_or(size)
}

/// Distance from the top of one line to the top of the next.
pub fn line_height(size: f32) -> f32 {
    FONT.horizontal_line_metrics(size)
        .map(|m| m.new_line_size)
        .filter(|h| *h > 0.0)
        .unwrap_or(size * 1.2)
        .max(text_height(size))
}

/// Drop the system fonts read so far (a CJK font file is tens of
/// megabytes). They are read again on demand.
pub fn release_fallbacks() {
    let mut fb = lock();
    if !fb.faces.is_empty() {
        tracing::debug!("Releasing {} system font(s)", fb.faces.len());
    }
    fb.faces.clear();
    fb.by_char.clear();
    fb.glyphs.clear();
}

// ───────────────────────── system fonts ─────────────────────────

/// Rendered glyphs kept between frames; the overlay redraws on every
/// mouse move.
const MAX_CACHED_GLYPHS: usize = 2048;

#[derive(Default)]
struct Fallback {
    /// The system's fonts: names and paths only, read on the first miss.
    db: Option<fontdb::Database>,
    /// Faces in use, in the order they were first needed.
    faces: Vec<Arc<SystemFace>>,
    /// Faces that couldn't be read; never tried again.
    broken: HashSet<fontdb::ID>,
    /// What each character resolved to (`None`: no installed font has it).
    by_char: HashMap<char, Option<Arc<SystemFace>>>,
    glyphs: HashMap<(char, u32), (Metrics, Vec<u8>)>,
}

static FALLBACK: LazyLock<Mutex<Fallback>> = LazyLock::new(Mutex::default);

fn lock() -> std::sync::MutexGuard<'static, Fallback> {
    FALLBACK.lock().unwrap_or_else(|p| p.into_inner())
}

impl Fallback {
    /// The system face to draw `ch` with, finding one if need be.
    fn face_for(&mut self, ch: char) -> Option<Arc<SystemFace>> {
        if let Some(known) = self.by_char.get(&ch) {
            return known.clone();
        }
        // A face already in use for an earlier character usually has this
        // one too, and keeps a run of text in one typeface.
        let in_use = self.faces.iter().find(|f| f.has_glyph(ch)).cloned();
        let found = in_use.or_else(|| self.find_face(ch));
        self.by_char.insert(ch, found.clone());
        found
    }

    /// Look through the installed fonts, best candidates first.
    fn find_face(&mut self, ch: char) -> Option<Arc<SystemFace>> {
        let db = self.db.get_or_insert_with(|| {
            let started = std::time::Instant::now();
            let mut db = fontdb::Database::new();
            db.load_system_fonts();
            tracing::info!(
                "Indexed {} system font faces in {:?} (first character outside the built-in font)",
                db.len(),
                started.elapsed()
            );
            db
        });
        let cjk = preferred_cjk();
        let mut candidates: Vec<(u32, fontdb::ID)> = db
            .faces()
            .filter(|f| !self.broken.contains(&f.id))
            .filter(|f| !self.faces.iter().any(|used| used.id == f.id))
            .map(|f| (rank(f, cjk), f.id))
            .collect();
        candidates.sort();
        for (_, id) in candidates {
            // A look at the face's character map, without copying the file.
            let covers = db.with_face_data(id, |data, index| {
                ttf_parser::Face::parse(data, index)
                    .ok()
                    .is_some_and(|face| face.glyph_index(ch).is_some())
            });
            match covers {
                Some(true) => {}
                Some(false) => continue,
                None => {
                    self.broken.insert(id);
                    continue;
                }
            }
            let Some((data, index)) = db.with_face_data(id, |data, index| (data.to_vec(), index))
            else {
                self.broken.insert(id);
                continue;
            };
            let family = db
                .face(id)
                .and_then(|f| f.families.first().map(|(name, _)| name.clone()))
                .unwrap_or_default();
            tracing::info!("Using the system font \"{family}\" for {ch:?}");
            let face = Arc::new(SystemFace { id, data, index });
            self.faces.push(Arc::clone(&face));
            return Some(face);
        }
        None
    }
}

/// One installed font face, kept as the bytes of its file.
struct SystemFace {
    id: fontdb::ID,
    data: Vec<u8>,
    index: u32,
}

impl SystemFace {
    fn parse(&self) -> Option<ttf_parser::Face<'_>> {
        ttf_parser::Face::parse(&self.data, self.index).ok()
    }

    fn has_glyph(&self, ch: char) -> bool {
        self.parse().is_some_and(|f| f.glyph_index(ch).is_some())
    }

    fn metrics(&self, ch: char, size: f32) -> Option<Metrics> {
        let face = self.parse()?;
        let glyph = face.glyph_index(ch)?;
        Some(placement(&face, glyph, size))
    }

    /// Fill the glyph's outline at `size`.
    fn rasterize(&self, ch: char, size: f32) -> Option<(Metrics, Vec<u8>)> {
        let face = self.parse()?;
        let glyph = face.glyph_index(ch)?;
        let metrics = placement(&face, glyph, size);
        let (w, h) = (metrics.width as u32, metrics.height as u32);
        if w == 0 || h == 0 {
            return Some((metrics, Vec::new()));
        }
        let mut outline = Outline(tiny_skia::PathBuilder::new());
        face.outline_glyph(glyph, &mut outline)?;
        let blank = || {
            (
                Metrics {
                    width: 0,
                    height: 0,
                    ..metrics
                },
                Vec::new(),
            )
        };
        let Some(path) = outline.0.finish() else {
            return Some(blank());
        };
        let Some(mut mask) = tiny_skia::Mask::new(w, h) else {
            return Some(blank());
        };
        // Font units (y up, origin on the baseline) to bitmap pixels (y
        // down, origin at the bitmap's top-left).
        let scale = size / face.units_per_em() as f32;
        let top = (metrics.ymin + metrics.height as i32) as f32;
        let to_pixels =
            tiny_skia::Transform::from_row(scale, 0.0, 0.0, -scale, -metrics.xmin as f32, top);
        mask.fill_path(&path, tiny_skia::FillRule::Winding, true, to_pixels);
        Some((metrics, mask.data().to_vec()))
    }
}

/// Where a glyph's bitmap sits relative to the pen position on the
/// baseline, in the same terms fontdue uses for the embedded font.
fn placement(face: &ttf_parser::Face<'_>, glyph: ttf_parser::GlyphId, size: f32) -> Metrics {
    let scale = size / face.units_per_em() as f32;
    let mut metrics = Metrics {
        advance_width: face.glyph_hor_advance(glyph).unwrap_or(0) as f32 * scale,
        ..Metrics::default()
    };
    if let Some(bbox) = face.glyph_bounding_box(glyph) {
        let xmin = (bbox.x_min as f32 * scale).floor() as i32;
        let ymin = (bbox.y_min as f32 * scale).floor() as i32;
        let xmax = (bbox.x_max as f32 * scale).ceil() as i32;
        let ymax = (bbox.y_max as f32 * scale).ceil() as i32;
        // A sane glyph is a few hundred pixels at most; anything else is a
        // broken font, drawn as nothing.
        if (1..4096).contains(&(xmax - xmin)) && (1..4096).contains(&(ymax - ymin)) {
            metrics.xmin = xmin;
            metrics.ymin = ymin;
            metrics.width = (xmax - xmin) as usize;
            metrics.height = (ymax - ymin) as usize;
        }
    }
    metrics
}

struct Outline(tiny_skia::PathBuilder);

impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.0.quad_to(x1, y1, x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.0.cubic_to(x1, y1, x2, y2, x, y);
    }
    fn close(&mut self) {
        self.0.close();
    }
}

/// Which East Asian typeface to prefer for characters several of them share.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Cjk {
    Japanese,
    Korean,
    TraditionalChinese,
    SimplifiedChinese,
}

/// Font families worth trying first, per script. Earlier is better; a face
/// not named here still gets tried, after these.
const PREFERRED: &[(&str, Option<Cjk>)] = &[
    ("Segoe UI", None),
    ("Noto Sans", None),
    ("DejaVu Sans", None),
    ("Yu Gothic UI", Some(Cjk::Japanese)),
    ("Meiryo UI", Some(Cjk::Japanese)),
    ("Noto Sans CJK JP", Some(Cjk::Japanese)),
    ("Malgun Gothic", Some(Cjk::Korean)),
    ("Noto Sans CJK KR", Some(Cjk::Korean)),
    ("Microsoft JhengHei UI", Some(Cjk::TraditionalChinese)),
    ("Noto Sans CJK TC", Some(Cjk::TraditionalChinese)),
    ("Microsoft YaHei UI", Some(Cjk::SimplifiedChinese)),
    ("Noto Sans CJK SC", Some(Cjk::SimplifiedChinese)),
    ("Segoe UI Symbol", None),
    ("Segoe UI Emoji", None),
    ("Noto Sans Symbols", None),
    ("Noto Emoji", None),
];

fn rank(face: &fontdb::FaceInfo, user_cjk: Option<Cjk>) -> u32 {
    let family = PREFERRED.iter().position(|(name, _)| {
        face.families
            .iter()
            .any(|(n, _)| n.eq_ignore_ascii_case(name))
    });
    let mut score = match family {
        // The user's own East Asian typeface goes ahead of the other three.
        Some(i) if PREFERRED[i].1.is_some() && PREFERRED[i].1 == user_cjk => 2,
        Some(i) => 10 + i as u32,
        None => 1000,
    };
    // Regular, upright, proportional faces before bold, italic and mono.
    if face.weight != fontdb::Weight::NORMAL {
        score += 200;
    }
    if face.style != fontdb::Style::Normal {
        score += 200;
    }
    if face.stretch != fontdb::Stretch::Normal {
        score += 200;
    }
    if face.monospaced {
        score += 100;
    }
    score
}

/// The user's East Asian language, from the UI language, if it is one.
fn preferred_cjk() -> Option<Cjk> {
    #[cfg(target_os = "windows")]
    {
        // LANGID: the low 10 bits are the primary language, the rest the
        // sublanguage (script or region).
        let langid = unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() };
        match (langid & 0x3ff, langid >> 10) {
            (0x11, _) => Some(Cjk::Japanese),
            (0x12, _) => Some(Cjk::Korean),
            // Taiwan, Hong Kong and Macao write Traditional Chinese.
            (0x04, 0x01 | 0x03 | 0x05) => Some(Cjk::TraditionalChinese),
            (0x04, _) => Some(Cjk::SimplifiedChinese),
            _ => None,
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let lang = ["LC_ALL", "LC_CTYPE", "LANG"]
            .iter()
            .find_map(|v| std::env::var(v).ok().filter(|s| !s.is_empty()))?
            .to_ascii_lowercase();
        if lang.starts_with("ja") {
            Some(Cjk::Japanese)
        } else if lang.starts_with("ko") {
            Some(Cjk::Korean)
        } else if lang.starts_with("zh_tw") || lang.starts_with("zh_hk") {
            Some(Cjk::TraditionalChinese)
        } else if lang.starts_with("zh") {
            Some(Cjk::SimplifiedChinese)
        } else {
            None
        }
    }
}
