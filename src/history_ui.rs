//! The History window: thumbnails of the last captures; click one to copy
//! it again.
//!
//! `HistoryView` holds the state and draws into a `Pixmap`; it takes clicks
//! (logical pixels) and key presses, and needs no window, so tests can drive
//! it. `HistoryWindow` wraps it with a winit window and a softbuffer surface.
//!
//! Keyboard: the arrow keys move between captures (Tab and Shift+Tab too),
//! Enter or Space copies the focused one, Esc closes.

use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;

use tiny_skia::{Color as SkiaColor, Paint, Pixmap, Rect, Transform};
use winit::window::Window;

use crate::access::{Control, Request, Role, Snapshot};
use crate::geometry::{Color, Point};
use crate::keys::{Key, KeyPress, Named};
use crate::tools::{measure_text_width, render_text_annotation};

/// Width / height of the history window in LOGICAL pixels (the window is
/// created with LogicalSize and the UI is scaled to the physical surface).
pub const WIN_W: u32 = 600;
pub const WIN_H: u32 = 400;

/// Thumbnail grid layout
pub const COLS: usize = 4;
const THUMB_W: u32 = 120;
const THUMB_H: u32 = 80;
const PADDING: f32 = 16.0;
const GAP: f32 = 12.0;
const HEADER_H: f32 = 44.0;

/// The "Clear All" button in the header: (x, y, w, h).
const CLEAR_RECT: (f32, f32, f32, f32) = (WIN_W as f32 - PADDING - 80.0, 10.0, 80.0, 24.0);

/// A loaded thumbnail: path, RGBA pixels, width, height.
struct Thumbnail {
    path: PathBuf,
    pixels: Vec<u8>,
    width: u32,
    height: u32,
}

/// What has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    ClearAll,
    /// A capture, newest first.
    Capture(usize),
}

/// What the caller should do after an input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Nothing,
    Redraw,
    Close,
    /// Copy this capture to the clipboard and say so.
    Copy(PathBuf),
}

/// All History state, no window.
pub struct HistoryView {
    thumbnails: Vec<Thumbnail>,
    hovered: Option<usize>,
    hover_clear: bool,
    cursor: (f32, f32),
    /// Vertical scroll offset in logical pixels (0 = top).
    scroll_offset: f32,
    focus: Option<Focus>,
    /// Draw the focus ring. Set by the keyboard, cleared by the mouse.
    focus_ring: bool,
}

impl Default for HistoryView {
    fn default() -> Self {
        Self::new()
    }
}

impl HistoryView {
    /// The captures in the history folder, newest first.
    pub fn new() -> Self {
        Self {
            thumbnails: load_thumbnails(),
            hovered: None,
            hover_clear: false,
            cursor: (-1.0, -1.0),
            scroll_offset: 0.0,
            focus: None,
            focus_ring: false,
        }
    }

    /// How many captures are shown.
    pub fn len(&self) -> usize {
        self.thumbnails.len()
    }

    pub fn is_empty(&self) -> bool {
        self.thumbnails.is_empty()
    }

    pub fn focus(&self) -> Option<Focus> {
        self.focus
    }

    /// Whether the focus ring is drawn (the keyboard was used last).
    pub fn focus_visible(&self) -> bool {
        self.focus_ring && self.focus.is_some()
    }

    /// Returns (x, y, w, h) of the thumbnail at the given index, in logical
    /// coordinates with the current scroll applied.
    pub fn thumb_rect(&self, idx: usize) -> (f32, f32, f32, f32) {
        let col = idx % COLS;
        let row = idx / COLS;
        let cell_w = THUMB_W as f32 + GAP;
        let cell_h = THUMB_H as f32 + GAP;
        let x = PADDING + col as f32 * cell_w;
        let y = HEADER_H + PADDING + row as f32 * cell_h - self.scroll_offset;
        if let Some(thumb) = self.thumbnails.get(idx) {
            (x, y, thumb.width as f32, thumb.height as f32)
        } else {
            (x, y, THUMB_W as f32, THUMB_H as f32)
        }
    }

    /// The "Clear All" button, shown while there is something to clear.
    pub fn clear_rect(&self) -> Option<(f32, f32, f32, f32)> {
        (!self.thumbnails.is_empty()).then_some(CLEAR_RECT)
    }

    /// Total height of the thumbnail grid content (without scroll).
    fn content_height(&self) -> f32 {
        if self.thumbnails.is_empty() {
            return 0.0;
        }
        let rows = self.thumbnails.len().div_ceil(COLS);
        PADDING + rows as f32 * (THUMB_H as f32 + GAP)
    }

    fn max_scroll(&self) -> f32 {
        (self.content_height() - (WIN_H as f32 - HEADER_H)).max(0.0)
    }

    // ───────────── input ─────────────

    /// Scroll by a wheel delta (positive = scroll up). Returns true if moved.
    pub fn scroll(&mut self, delta: f32) -> bool {
        let old = self.scroll_offset;
        self.scroll_offset = (self.scroll_offset - delta * 40.0).clamp(0.0, self.max_scroll());
        (self.scroll_offset - old).abs() > 0.01
    }

    /// The pointer moved (logical pixels). Returns true if what is
    /// highlighted changed.
    pub fn hover(&mut self, x: f32, y: f32) -> bool {
        self.cursor = (x, y);
        let before = (self.hovered, self.hover_clear);
        self.hover_clear = self.clear_rect().is_some_and(|r| within(r, x, y));
        self.hovered = self.capture_at(x, y);
        before != (self.hovered, self.hover_clear)
    }

    fn capture_at(&self, x: f32, y: f32) -> Option<usize> {
        if y < HEADER_H {
            return None;
        }
        (0..self.thumbnails.len()).find(|i| within(self.thumb_rect(*i), x, y))
    }

    /// A left click at (x, y), in logical pixels.
    pub fn click(&mut self, x: f32, y: f32) -> Outcome {
        self.cursor = (x, y);
        // The mouse takes focus with it, without the ring.
        self.focus_ring = false;
        if self.clear_rect().is_some_and(|r| within(r, x, y)) {
            self.focus = Some(Focus::ClearAll);
            return self.press(Focus::ClearAll);
        }
        match self.capture_at(x, y) {
            Some(i) => {
                self.focus = Some(Focus::Capture(i));
                self.press(Focus::Capture(i))
            }
            None => Outcome::Nothing,
        }
    }

    /// Do what a control does.
    fn press(&mut self, what: Focus) -> Outcome {
        match what {
            Focus::ClearAll => {
                if self.thumbnails.is_empty() {
                    return Outcome::Nothing;
                }
                let removed = crate::history::clear_history();
                tracing::info!("Cleared {} history entries", removed);
                self.thumbnails.clear();
                self.scroll_offset = 0.0;
                self.hovered = None;
                self.hover_clear = false;
                self.focus = None;
                Outcome::Redraw
            }
            Focus::Capture(i) => match self.thumbnails.get(i) {
                Some(thumb) => Outcome::Copy(thumb.path.clone()),
                None => Outcome::Nothing,
            },
        }
    }

    /// A key went down.
    pub fn key(&mut self, press: KeyPress) -> Outcome {
        if !press.pressed {
            return Outcome::Nothing;
        }
        let Some(Key::Named(key)) = press.key else {
            return Outcome::Nothing;
        };
        let count = self.thumbnails.len();
        let mods = press.mods;
        if mods.has_command() {
            return Outcome::Nothing;
        }
        if key == Named::Escape {
            return Outcome::Close;
        }
        if count == 0 {
            return Outcome::Nothing;
        }
        let next = match (key, self.focus) {
            (Named::Enter | Named::Space, Some(focus)) => {
                self.focus_ring = true;
                return self.press(focus);
            }
            // Nothing focused yet: any movement key starts at the newest
            // capture (Shift+Tab at the oldest).
            (Named::Tab, None) if mods.shift() => Focus::Capture(count - 1),
            (
                Named::Tab | Named::Right | Named::Left | Named::Down | Named::Up | Named::Home,
                None,
            ) => Focus::Capture(0),
            (Named::End, _) => Focus::Capture(count - 1),
            (Named::Home, _) => Focus::Capture(0),

            // Tab walks everything in reading order, the button first.
            (Named::Tab, Some(Focus::ClearAll)) if mods.shift() => Focus::Capture(count - 1),
            (Named::Tab, Some(Focus::ClearAll)) => Focus::Capture(0),
            (Named::Tab, Some(Focus::Capture(0))) if mods.shift() => Focus::ClearAll,
            (Named::Tab, Some(Focus::Capture(i))) if mods.shift() => Focus::Capture(i - 1),
            (Named::Tab, Some(Focus::Capture(i))) if i + 1 == count => Focus::ClearAll,
            (Named::Tab, Some(Focus::Capture(i))) => Focus::Capture(i + 1),

            // Arrows move on the grid; up from the top row is the button.
            (Named::Right, Some(Focus::Capture(i))) => Focus::Capture((i + 1).min(count - 1)),
            (Named::Left, Some(Focus::Capture(i))) => Focus::Capture(i.saturating_sub(1)),
            (Named::Down, Some(Focus::Capture(i))) if i + COLS < count => Focus::Capture(i + COLS),
            (Named::Up, Some(Focus::Capture(i))) if i >= COLS => Focus::Capture(i - COLS),
            (Named::Up, Some(Focus::Capture(_))) => Focus::ClearAll,
            (Named::Down | Named::Right, Some(Focus::ClearAll)) => Focus::Capture(0),
            _ => return Outcome::Nothing,
        };
        self.set_focus(next);
        Outcome::Redraw
    }

    /// Focus a control with the ring showing, scrolling a capture into view.
    pub fn set_focus(&mut self, focus: Focus) {
        self.focus = Some(focus);
        self.focus_ring = true;
        if let Focus::Capture(i) = focus {
            let (_, y, _, h) = self.thumb_rect(i);
            let top = HEADER_H + PADDING;
            let bottom = WIN_H as f32 - PADDING;
            if y < top {
                self.scroll_offset -= top - y;
            } else if y + h > bottom {
                self.scroll_offset += y + h - bottom;
            }
            self.scroll_offset = self.scroll_offset.clamp(0.0, self.max_scroll());
        }
    }

    // ───────────── for assistive technology ─────────────

    /// Every control, in reading order.
    fn order(&self) -> Vec<Focus> {
        let mut order = Vec::new();
        if !self.thumbnails.is_empty() {
            order.push(Focus::ClearAll);
        }
        order.extend((0..self.thumbnails.len()).map(Focus::Capture));
        order
    }

    /// The window as a screen reader should present it.
    pub fn snapshot(&self) -> Snapshot {
        let count = self.thumbnails.len();
        let order = self.order();
        let controls = order
            .iter()
            .map(|focus| match *focus {
                Focus::ClearAll => Control {
                    role: Role::Button,
                    label: "Clear all captures from History".to_string(),
                    checked: None,
                    value: None,
                    rect: Some(CLEAR_RECT),
                },
                Focus::Capture(i) => {
                    let rect = self.thumb_rect(i);
                    let in_view = rect.1 >= HEADER_H && rect.1 + rect.3 <= WIN_H as f32;
                    Control {
                        role: Role::Button,
                        label: format!(
                            "Capture {} of {count}{}. Copy it again",
                            i + 1,
                            capture_time(&self.thumbnails[i].path)
                                .map(|t| format!(", taken {t}"))
                                .unwrap_or_default()
                        ),
                        checked: None,
                        value: None,
                        rect: in_view.then_some(rect),
                    }
                }
            })
            .collect();
        Snapshot {
            title: "screen-mark History".to_string(),
            controls,
            focus: self.focus.and_then(|f| order.iter().position(|o| *o == f)),
            status: self
                .thumbnails
                .is_empty()
                .then(|| "No captures yet.".to_string()),
        }
    }

    /// Carry out a screen reader's request on control `index` of `snapshot`.
    pub fn request(&mut self, request: Request) -> Outcome {
        let order = self.order();
        match request {
            Request::Press(i) => match order.get(i) {
                Some(focus) => {
                    self.set_focus(*focus);
                    self.press(*focus)
                }
                None => Outcome::Nothing,
            },
            Request::Focus(i) => match order.get(i) {
                Some(focus) => {
                    self.set_focus(*focus);
                    Outcome::Redraw
                }
                None => Outcome::Nothing,
            },
        }
    }

    // ───────────── drawing ─────────────

    /// Draw the whole window at logical size.
    pub fn render(&self) -> Pixmap {
        let mut pixmap = Pixmap::new(WIN_W, WIN_H).expect("history size is non-zero");
        fill_rect_rgb(
            &mut pixmap,
            0.0,
            0.0,
            WIN_W as f32,
            WIN_H as f32,
            crate::theme::bg_1(),
        );
        let ring = |focus: Focus| self.focus_ring && self.focus == Some(focus);

        if self.thumbnails.is_empty() {
            draw_label(
                &mut pixmap,
                PADDING,
                HEADER_H + PADDING + 20.0,
                "No captures yet.",
                14.0,
                crate::theme::text_muted(),
            );
        } else {
            for (i, thumb) in self.thumbnails.iter().enumerate() {
                let (x, y, tw, th) = self.thumb_rect(i);

                // Skip thumbnails fully outside the viewport
                if y + th < HEADER_H || y > WIN_H as f32 {
                    continue;
                }

                // Border: accent when hovered or focused, wider for focus.
                let focused = ring(Focus::Capture(i));
                let border_color = if focused || self.hovered == Some(i) {
                    crate::theme::accent()
                } else {
                    crate::theme::bg_3()
                };
                let b = if focused { 4.0 } else { 2.0 };
                fill_rect_rgb(
                    &mut pixmap,
                    x - b,
                    y - b,
                    tw + b * 2.0,
                    th + b * 2.0,
                    border_color,
                );

                // Render thumbnail pixels, clipped against the header line
                blit_rgba_clipped(
                    &mut pixmap,
                    x as i32,
                    y as i32,
                    &thumb.pixels,
                    thumb.width,
                    thumb.height,
                    HEADER_H as i32,
                );
            }

            // Scrollbar indicator when the content overflows
            let max_scroll = self.max_scroll();
            if max_scroll > 0.0 {
                let viewport_h = WIN_H as f32 - HEADER_H;
                let track_h = viewport_h - 8.0;
                let knob_h = (viewport_h / self.content_height() * track_h).max(24.0);
                let knob_y =
                    HEADER_H + 4.0 + (self.scroll_offset / max_scroll) * (track_h - knob_h);
                fill_rect_rgb(
                    &mut pixmap,
                    WIN_W as f32 - 6.0,
                    knob_y,
                    3.0,
                    knob_h,
                    crate::theme::bg_4(),
                );
            }
        }

        // Header drawn last so scrolled thumbnails never overlap it
        fill_rect_rgb(
            &mut pixmap,
            0.0,
            0.0,
            WIN_W as f32,
            HEADER_H - 3.0,
            crate::theme::bg_1(),
        );
        draw_label(
            &mut pixmap,
            PADDING,
            14.0,
            "Recent Captures",
            16.0,
            crate::theme::text_normal(),
        );
        // Separator
        fill_rect_rgb(
            &mut pixmap,
            PADDING,
            HEADER_H - 4.0,
            WIN_W as f32 - PADDING * 2.0,
            1.0,
            crate::theme::bg_3(),
        );
        // Clear All button
        if let Some((cx, cy, cw, ch)) = self.clear_rect() {
            if ring(Focus::ClearAll) {
                fill_rect_rgb(
                    &mut pixmap,
                    cx - 3.0,
                    cy - 3.0,
                    cw + 6.0,
                    ch + 6.0,
                    crate::theme::accent(),
                );
                fill_rect_rgb(
                    &mut pixmap,
                    cx - 1.0,
                    cy - 1.0,
                    cw + 2.0,
                    ch + 2.0,
                    crate::theme::bg_1(),
                );
            }
            let bg = if self.hover_clear {
                crate::theme::bg_4()
            } else {
                crate::theme::bg_3()
            };
            fill_rect_rgb(&mut pixmap, cx, cy, cw, ch, bg);
            let label = "Clear All";
            let tw = measure_text_width(label, 12.0);
            draw_label(
                &mut pixmap,
                cx + (cw - tw) / 2.0,
                cy + 5.0,
                label,
                12.0,
                crate::theme::text_normal(),
            );
        }
        pixmap
    }
}

/// When a history file was saved, from its name (milliseconds since 1970),
/// as "5 Nov 14:30".
fn capture_time(path: &std::path::Path) -> Option<String> {
    use chrono::TimeZone;
    let millis: i64 = path.file_stem()?.to_str()?.parse().ok()?;
    let when = chrono::Local.timestamp_millis_opt(millis).single()?;
    Some(when.format("%-d %b %H:%M").to_string())
}

fn within((x, y, w, h): (f32, f32, f32, f32), px: f32, py: f32) -> bool {
    px >= x && px <= x + w && py >= y && py <= y + h
}

// ───────────── the window ─────────────

/// `HistoryView` in a real window.
pub struct HistoryWindow {
    pub window: Arc<Window>,
    pub surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
    pub view: HistoryView,
}

impl HistoryWindow {
    pub fn new(
        window: Arc<Window>,
        surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
    ) -> Self {
        Self {
            window,
            surface,
            view: HistoryView::new(),
        }
    }

    fn scale(&self) -> f32 {
        self.window.scale_factor() as f32
    }

    /// Cursor moved (physical pixels). Returns true to redraw.
    pub fn on_cursor_moved(&mut self, x: f32, y: f32) -> bool {
        let s = self.scale();
        self.view.hover(x / s, y / s)
    }

    /// Left click at the last cursor position.
    pub fn on_click(&mut self) -> Outcome {
        let (x, y) = self.view.cursor;
        self.view.click(x, y)
    }

    /// Draw the view, scaling the logical pixmap to the physical surface
    /// (nearest neighbour) so the window is usable on high-DPI displays.
    pub fn render(&mut self) {
        let pixmap = self.view.render();
        let phys = self.window.inner_size();
        let pw = phys.width.max(1);
        let ph = phys.height.max(1);
        if let (Some(nz_w), Some(nz_h)) = (NonZeroU32::new(pw), NonZeroU32::new(ph)) {
            if let Err(e) = self.surface.resize(nz_w, nz_h) {
                tracing::error!("History surface resize failed: {e}");
                return;
            }
        }

        if let Ok(mut buffer) = self.surface.buffer_mut() {
            let src = pixmap.data();
            let src_w = WIN_W as usize;
            let src_h = WIN_H as usize;
            for y in 0..ph as usize {
                let sy = (y * src_h / ph as usize).min(src_h - 1);
                for x in 0..pw as usize {
                    let sx = (x * src_w / pw as usize).min(src_w - 1);
                    let si = (sy * src_w + sx) * 4;
                    buffer[y * pw as usize + x] = ((src[si] as u32) << 16)
                        | ((src[si + 1] as u32) << 8)
                        | (src[si + 2] as u32);
                }
            }
            let _ = buffer.present();
        }
    }
}

/// Decode a history file and put it on the clipboard. Returns the popup to
/// show, like any other copy.
pub fn copy_capture(path: &std::path::Path) -> crate::toast::Spec {
    use crate::toast::{Kind, Spec};
    let image = match image::open(path) {
        Ok(image) => image.to_rgba8(),
        Err(e) => {
            tracing::error!("Failed to open history image {}: {e}", path.display());
            return Spec::new(
                Kind::Error,
                "Couldn't copy",
                "That capture is gone from History",
            );
        }
    };
    let (w, h) = image.dimensions();
    let copied = crate::export::copy_to_clipboard(&image.into_raw(), w, h);
    crate::actions::describe(None, Some(copied), w, h)
}

fn load_thumbnails() -> Vec<Thumbnail> {
    crate::history::list_history()
        .into_iter()
        .filter_map(|path| {
            let img = image::open(&path).ok()?;
            let thumb = img.thumbnail(THUMB_W, THUMB_H);
            let rgba = thumb.to_rgba8();
            let (w, h) = rgba.dimensions();
            Some(Thumbnail {
                path,
                pixels: rgba.into_raw(),
                width: w,
                height: h,
            })
        })
        .collect()
}

// ── Drawing helpers ──

fn fill_rect_rgb(pixmap: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, (r, g, b): (u8, u8, u8)) {
    let rect = match Rect::from_xywh(x, y, w, h) {
        Some(r) => r,
        None => return,
    };
    let mut paint = Paint::default();
    paint.set_color(SkiaColor::from_rgba8(r, g, b, 255));
    paint.anti_alias = true;
    pixmap.fill_rect(rect, &paint, Transform::identity(), None);
}

fn draw_label(
    pixmap: &mut Pixmap,
    x: f32,
    y: f32,
    text: &str,
    font_size: f32,
    (r, g, b): (u8, u8, u8),
) {
    let color = Color::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0);
    let pos = Point::new(x, y);
    render_text_annotation(pixmap, &pos, text, &color, font_size);
}

/// Blit RGBA pixels onto a pixmap, skipping rows above `min_y` (header clip).
fn blit_rgba_clipped(
    pixmap: &mut Pixmap,
    dst_x: i32,
    dst_y: i32,
    pixels: &[u8],
    w: u32,
    h: u32,
    min_y: i32,
) {
    let pm_w = pixmap.width() as i32;
    let pm_h = pixmap.height() as i32;
    let data = pixmap.data_mut();
    for y in 0..h as i32 {
        let py = dst_y + y;
        if py < min_y || py >= pm_h {
            continue;
        }
        for x in 0..w as i32 {
            let px = dst_x + x;
            if px < 0 || px >= pm_w {
                continue;
            }
            let src_i = ((y * w as i32 + x) * 4) as usize;
            let dst_i = ((py * pm_w + px) * 4) as usize;
            if src_i + 3 < pixels.len() && dst_i + 3 < data.len() {
                // Premultiply alpha for tiny-skia
                let a = pixels[src_i + 3] as u32;
                data[dst_i] = ((pixels[src_i] as u32 * a) / 255) as u8;
                data[dst_i + 1] = ((pixels[src_i + 1] as u32 * a) / 255) as u8;
                data[dst_i + 2] = ((pixels[src_i + 2] as u32 * a) / 255) as u8;
                data[dst_i + 3] = pixels[src_i + 3];
            }
        }
    }
}
