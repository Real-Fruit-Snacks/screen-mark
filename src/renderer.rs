//! Drawing the capture overlay.
//!
//! A frame is the background (the capture, dimmed outside the selection
//! and around any spotlights) with everything else drawn over it:
//! crosshair, selection border, annotations, toolbar, magnifier. On a 4K
//! desktop the capture alone is 33 MB, and the pointer
//! moving is a new frame, so a frame is not drawn from scratch. `Frame`
//! remembers which areas the last one drew on; the next one puts the
//! background back in just those areas (and where the selection changed),
//! draws again, and reports the areas that differ, which is all the window
//! then has to be given. `render_overlay` draws a whole frame the plain way;
//! tests check that the two always agree.

use std::num::NonZeroU32;

use crate::capture::CapturedScreen;
use crate::geometry::Color;
use crate::icons::blend_pixmap;
use crate::overlay::selection::Selection;
use crate::overlay::toolbar::{ButtonAction, Toolbar, BUTTONS};
use crate::state::OverlayState;
use crate::tools::{
    annotation_bounding_box, annotation_reach, draw_annotations, measure_text_height,
    measure_text_width, spotlights, Annotation, Backdrop, ToolKind,
};
use tiny_skia::{Paint, PathBuilder, Stroke, Transform};

/// A rectangle of whole pixels: `x0 <= x < x1` and `y0 <= y < y1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Area {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Area {
    pub fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        Self { x0, y0, x1, y1 }
    }

    /// Every pixel a rectangle given in fractions of a pixel touches, grown
    /// by `margin` on each side.
    pub fn around(x: f32, y: f32, w: f32, h: f32, margin: f32) -> Self {
        let (x0, x1) = (x.min(x + w), x.max(x + w));
        let (y0, y1) = (y.min(y + h), y.max(y + h));
        Self {
            x0: (x0 - margin).floor() as i32,
            y0: (y0 - margin).floor() as i32,
            x1: (x1 + margin).ceil() as i32,
            y1: (y1 + margin).ceil() as i32,
        }
    }

    pub fn is_empty(self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }

    pub fn width(self) -> i32 {
        (self.x1 - self.x0).max(0)
    }

    pub fn height(self) -> i32 {
        (self.y1 - self.y0).max(0)
    }

    /// How many pixels it holds.
    pub fn pixels(self) -> i64 {
        self.width() as i64 * self.height() as i64
    }

    /// The part of `self` that is also in `other` (possibly empty).
    pub fn intersect(self, other: Area) -> Area {
        Area {
            x0: self.x0.max(other.x0),
            y0: self.y0.max(other.y0),
            x1: self.x1.min(other.x1),
            y1: self.y1.min(other.y1),
        }
    }

    /// True if every pixel of `other` is in `self`.
    pub fn contains(self, other: Area) -> bool {
        other.is_empty()
            || (other.x0 >= self.x0
                && other.y0 >= self.y0
                && other.x1 <= self.x1
                && other.y1 <= self.y1)
    }

    /// `self` with `hole` cut out of it: up to four areas, none empty.
    pub fn without(self, hole: Area) -> Vec<Area> {
        if self.is_empty() {
            return Vec::new();
        }
        let hole = hole.intersect(self);
        if hole.is_empty() {
            return vec![self];
        }
        [
            // Above and below the hole, full width; then left and right of it.
            Area::new(self.x0, self.y0, self.x1, hole.y0),
            Area::new(self.x0, hole.y1, self.x1, self.y1),
            Area::new(self.x0, hole.y0, hole.x0, hole.y1),
            Area::new(hole.x1, hole.y0, self.x1, hole.y1),
        ]
        .into_iter()
        .filter(|a| !a.is_empty())
        .collect()
    }
}

/// What the last frame left in the pixmap, so the next one can change only
/// what differs. A new or invalidated `Frame` draws everything.
#[derive(Debug, Default)]
pub struct Frame {
    /// False until a frame has been drawn into this pixmap.
    drawn_once: bool,
    size: (u32, u32),
    /// The selection the background was painted for.
    bright: Area,
    /// The spotlights the background was painted for.
    spots: Vec<Area>,
    /// Where the last frame drew over the background.
    drawn: Vec<Area>,
}

impl Frame {
    /// Draw the next frame from scratch.
    pub fn invalidate(&mut self) {
        self.drawn_once = false;
    }
}

/// The areas a frame draws on, collected while it is drawn.
struct Drawn {
    bounds: Area,
    areas: Vec<Area>,
}

impl Drawn {
    fn add(&mut self, area: Area) {
        let area = area.intersect(self.bounds);
        if !area.is_empty() {
            self.areas.push(area);
        }
    }
}

/// Draw a whole overlay frame: the capture, dimmed outside the selection,
/// and everything on top of it.
pub fn render_overlay(state: &mut OverlayState, pixmap: &mut tiny_skia::Pixmap) {
    render_frame(state, pixmap, &mut Frame::default());
}

/// Bring `pixmap` from the last frame (as `frame` remembers it) to the
/// current one, redrawing only what changed. Returns the areas whose pixels
/// may differ from the last frame; the first time, that is all of it.
pub fn render_frame(
    state: &mut OverlayState,
    pixmap: &mut tiny_skia::Pixmap,
    frame: &mut Frame,
) -> Vec<Area> {
    let size = (pixmap.width(), pixmap.height());
    let bounds = Area::new(0, 0, size.0 as i32, size.1 as i32);
    let bright = bright_area(state.selection.as_ref(), bounds);

    // The annotation being drawn right now, or the text being typed.
    let (typed, _) = state.text_for_display();
    let preview = if state.text_input_active {
        (!typed.is_empty()).then_some(Annotation::Text {
            position: state.text_input_position,
            text: typed,
            color: state.current_color,
            font_size: state.text_input_font_size,
        })
    } else {
        state
            .tool(state.active_tool)
            .and_then(|tool| tool.in_progress_annotation())
    };
    // Spotlights belong to the background: they change every pixel, but
    // only when one is added, moved or removed.
    let spots: Vec<Area> = spotlights(&state.annotations, preview.as_ref())
        .into_iter()
        .map(|(x, y, w, h)| {
            let px = |v: usize| v.min(i32::MAX as usize) as i32;
            Area::new(
                px(x),
                px(y),
                px(x.saturating_add(w)),
                px(y.saturating_add(h)),
            )
            .intersect(bounds)
        })
        .collect();

    // What has to go back to plain background: everything the last frame
    // drew, and the edges the selection moved over.
    let from_scratch = !frame.drawn_once || frame.size != size || frame.spots != spots;
    let mut changed = if from_scratch {
        vec![bounds]
    } else {
        let mut stale = std::mem::take(&mut frame.drawn);
        stale.extend(frame.bright.without(bright));
        stale.extend(bright.without(frame.bright));
        stale
    };
    for area in &changed {
        paint_background(pixmap, &state.screenshot, bright, &spots, *area);
    }

    // Everything else is drawn over background, old or just put back, so a
    // translucent toolbar never lands on a copy of itself.
    let mut drawn = Drawn {
        bounds,
        areas: Vec::new(),
    };
    draw_elements(state, pixmap, preview.as_ref(), &mut drawn);

    changed.extend(drawn.areas.iter().copied());
    *frame = Frame {
        drawn_once: true,
        size,
        bright,
        spots,
        drawn: drawn.areas,
    };
    tidy(changed, bounds)
}

/// The pixels the selection covers, as the background shows them bright.
fn bright_area(selection: Option<&Selection>, bounds: Area) -> Area {
    let Some(sel) = selection else {
        return Area::default();
    };
    // Truncated like a pixel index: -0.5 is pixel 0, 3.9 is pixel 3.
    let pixel = |v: f32| (v as usize).min(i32::MAX as usize) as i32;
    Area::new(
        pixel(sel.x),
        pixel(sel.y),
        pixel(sel.x + sel.width),
        pixel(sel.y + sel.height),
    )
    .intersect(bounds)
}

/// How much of a capture pixel is left outside the selection: 153/256, or
/// 60 %.
const DIM: u32 = 153;

/// Paint the background into `area`: the capture as it is inside `bright`,
/// dimmed everywhere else. With spotlights, all of that is halved again
/// except inside `spots`, where the capture shows as it is; the same sum
/// the export does (`tools::render_annotations`), so the two agree.
///
/// This is the only place the capture reaches the frame from, so no dimmed
/// copy of it has to be kept.
fn paint_background(
    pixmap: &mut tiny_skia::Pixmap,
    capture: &CapturedScreen,
    bright: Area,
    spots: &[Area],
    area: Area,
) {
    let frame_w = pixmap.width() as usize;
    let capture_w = capture.width as usize;
    let capture_h = (capture.pixels.len() / 4)
        .checked_div(capture_w)
        .unwrap_or(0)
        .min(capture.height as usize);
    let area = area.intersect(Area::new(
        0,
        0,
        frame_w.min(capture_w) as i32,
        (pixmap.height() as usize).min(capture_h) as i32,
    ));
    if area.is_empty() {
        return;
    }
    let (x0, x1) = (area.x0 as usize, area.x1 as usize);
    // The stretch of each row that is inside the selection, counted from x0.
    let lit = bright.intersect(area);
    let (lit0, lit1) = if lit.is_empty() {
        (0, 0)
    } else {
        (lit.x0 as usize - x0, lit.x1 as usize - x0)
    };
    let out = pixmap.data_mut();
    for y in area.y0 as usize..area.y1 as usize {
        let src = &capture.pixels[(y * capture_w + x0) * 4..(y * capture_w + x1) * 4];
        let dst = &mut out[(y * frame_w + x0) * 4..(y * frame_w + x1) * 4];
        let (a, b) = if (lit.y0..lit.y1).contains(&(y as i32)) {
            (lit0 * 4, lit1 * 4)
        } else {
            (0, 0)
        };
        dim_into(&mut dst[..a], &src[..a]);
        // A capture is opaque, so its bytes are already what tiny-skia
        // keeps (premultiplied) and can be copied as they are.
        dst[a..b].copy_from_slice(&src[a..b]);
        dim_into(&mut dst[b..], &src[b..]);

        if spots.is_empty() {
            continue;
        }
        for px in dst.as_chunks_mut::<4>().0 {
            let pixel = u32::from_le_bytes(*px);
            *px = (((pixel >> 1) & 0x007f_7f7f) | 0xff00_0000).to_le_bytes();
        }
        let row = Area::new(area.x0, y as i32, area.x1, y as i32 + 1);
        for spot in spots {
            let lit = spot.intersect(row);
            if !lit.is_empty() {
                let (from, to) = ((lit.x0 as usize - x0) * 4, (lit.x1 as usize - x0) * 4);
                dst[from..to].copy_from_slice(&src[from..to]);
            }
        }
    }
}

/// `src` at 60 % brightness, opaque. A whole pixel at a time (red and blue
/// in one multiplication, green in another), which the compiler turns into
/// vector code; this runs over the whole desktop for a frame from scratch.
fn dim_into(dst: &mut [u8], src: &[u8]) {
    for (d, s) in dst
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(src.as_chunks::<4>().0)
    {
        let pixel = u32::from_le_bytes(*s);
        let red_blue = (((pixel & 0x00ff_00ff) * DIM) >> 8) & 0x00ff_00ff;
        let green = (((pixel & 0x0000_ff00) * DIM) >> 8) & 0x0000_ff00;
        *d = (red_blue | green | 0xff00_0000).to_le_bytes();
    }
}

/// Clip a list of changed areas to the frame, drop the ones inside others,
/// and fall back to the whole frame when the list no longer saves anything.
fn tidy(areas: Vec<Area>, bounds: Area) -> Vec<Area> {
    let mut kept: Vec<Area> = Vec::with_capacity(areas.len());
    for area in areas {
        let area = area.intersect(bounds);
        if area.is_empty() || kept.iter().any(|k| k.contains(area)) {
            continue;
        }
        kept.retain(|k| !area.contains(*k));
        kept.push(area);
    }
    let total: i64 = kept.iter().map(|a| a.pixels()).sum();
    if total >= bounds.pixels() || kept.len() > 96 {
        return vec![bounds];
    }
    kept
}

/// Copy the changed areas of a frame into a window's buffer, which holds one
/// `0x00RRGGBB` word per pixel in rows of `stride`. Returns those areas as
/// the window wants to be told about them.
///
/// The overlay is opaque (it is drawn over an opaque capture), so the bytes
/// tiny-skia keeps are the colours themselves.
pub fn to_window_pixels(
    pixmap: &tiny_skia::Pixmap,
    out: &mut [u32],
    stride: usize,
    areas: &[Area],
) -> Vec<softbuffer::Rect> {
    let rows = out.len().checked_div(stride).unwrap_or(0);
    let frame_stride = pixmap.width() as usize * 4;
    let bounds = Area::new(
        0,
        0,
        (pixmap.width() as usize).min(stride) as i32,
        (pixmap.height() as usize).min(rows) as i32,
    );
    let src = pixmap.data();
    let mut told = Vec::with_capacity(areas.len());
    for area in areas {
        let area = area.intersect(bounds);
        let (Some(w), Some(h)) = (
            NonZeroU32::new(area.width() as u32),
            NonZeroU32::new(area.height() as u32),
        ) else {
            continue;
        };
        let (x0, x1) = (area.x0 as usize, area.x1 as usize);
        for y in area.y0 as usize..area.y1 as usize {
            let from = &src[y * frame_stride + x0 * 4..y * frame_stride + x1 * 4];
            let to = &mut out[y * stride + x0..y * stride + x1];
            for (word, rgba) in to.iter_mut().zip(from.as_chunks::<4>().0) {
                *word = u32::from_be_bytes([0, rgba[0], rgba[1], rgba[2]]);
            }
        }
        told.push(softbuffer::Rect {
            x: area.x0 as u32,
            y: area.y0 as u32,
            width: w,
            height: h,
        });
    }
    told
}

/// Draw everything that goes over the background, noting where. `preview`
/// is the annotation being drawn right now, if any.
fn draw_elements(
    state: &mut OverlayState,
    pixmap: &mut tiny_skia::Pixmap,
    preview: Option<&Annotation>,
    drawn: &mut Drawn,
) {
    let width = pixmap.width() as usize;
    let height = pixmap.height() as usize;

    // 2b. Crosshair guide lines through cursor for precise alignment
    let show_crosshair = state.selection.is_none() || state.is_selecting;
    if show_crosshair {
        let mx = state.last_mouse_pos.x;
        let my = state.last_mouse_pos.y;
        let pw = pixmap.width() as f32;
        let ph = pixmap.height() as f32;

        let mut paint = Paint::default();
        paint.set_color(crate::theme::skia(crate::theme::accent(), 0.5));
        paint.anti_alias = true;
        let stroke = Stroke {
            width: 1.0,
            ..Stroke::default()
        };

        // Vertical line
        let mut pb = PathBuilder::new();
        pb.move_to(mx, 0.0);
        pb.line_to(mx, ph);
        if let Some(path) = pb.finish() {
            pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        }

        // Horizontal line
        let mut pb = PathBuilder::new();
        pb.move_to(0.0, my);
        pb.line_to(pw, my);
        if let Some(path) = pb.finish() {
            pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        }
        drawn.add(Area::around(mx, 0.0, 0.0, ph, 2.0));
        drawn.add(Area::around(0.0, my, pw, 0.0, 2.0));
    }

    // 3. (The selection is bright because the background was painted so.)
    if let Some(sel) = &state.selection {
        // 4. Selection border
        if let Some(rect) = tiny_skia::Rect::from_xywh(sel.x, sel.y, sel.width, sel.height) {
            let path = PathBuilder::from_rect(rect);
            let mut paint = Paint::default();
            paint.set_color(crate::theme::skia(crate::theme::accent(), 0.9));
            paint.anti_alias = true;
            let stroke = Stroke {
                width: 1.5,
                ..Stroke::default()
            };
            pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
            // Its four edges, not the rectangle: nothing is drawn inside.
            let (x, y, w, h) = (sel.x, sel.y, sel.width, sel.height);
            drawn.add(Area::around(x, y, w, 0.0, 3.0));
            drawn.add(Area::around(x, y + h, w, 0.0, 3.0));
            drawn.add(Area::around(x, y, 0.0, h, 3.0));
            drawn.add(Area::around(x + w, y, 0.0, h, 3.0));
        }

        // 4b. Selection size label
        drawn.add(render_size_label(pixmap, sel, width as f32, height as f32));
    }

    // 5. Annotations, plus the one being drawn or the text being typed,
    //    with the code the export draws them with. (A spotlight draws
    //    nothing here: its dimming is already in the background.)
    draw_annotations(
        &state.annotations,
        preview,
        pixmap,
        Backdrop {
            pixels: &state.screenshot.pixels,
            width: state.screenshot.width,
        },
    );
    for annotation in state.annotations.iter().chain(preview) {
        if let Some((x, y, w, h)) = annotation_reach(annotation) {
            drawn.add(Area::around(x, y, w, h, 1.0));
        }
    }

    // 6b. Selection highlight around selected annotation
    if let Some(idx) = state.selected_index {
        if let Some(ann) = state.annotations.get(idx) {
            if let Some((bx, by, bw, bh)) = annotation_bounding_box(ann) {
                render_selection_highlight(pixmap, bx - 4.0, by - 4.0, bw + 8.0, bh + 8.0);
                // The dashed box, and the corner handles that straddle it.
                drawn.add(Area::around(bx - 4.0, by - 4.0, bw + 8.0, bh + 8.0, 7.0));
            }
        }
    }

    // 7. The caret, in the text's own colour so it shows on any background,
    //    and a line under text an input method is still composing.
    if let Some((cx, cy, ch)) = state.text_caret_rect() {
        let mut paint = Paint::default();
        paint.set_color(state.current_color.into());
        paint.anti_alias = true;
        let mut line = |x0: f32, y0: f32, x1: f32, y1: f32, width: f32| {
            let mut pb = PathBuilder::new();
            pb.move_to(x0, y0);
            pb.line_to(x1, y1);
            if let Some(path) = pb.finish() {
                let stroke = Stroke {
                    width,
                    ..Stroke::default()
                };
                pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
            }
        };
        line(cx, cy, cx, cy + ch, 1.5);
        let (mut left, mut bottom) = (cx, cy + ch);
        if !state.text_preedit.is_empty() {
            let size = state.text_input_font_size;
            let w = measure_text_width(&state.text_preedit, size);
            let y = cy + measure_text_height(size) + 1.0;
            line(cx - w, y, cx, y, 1.0);
            (left, bottom) = (cx - w, bottom.max(y));
        }
        drawn.add(Area::around(left, cy, cx - left, bottom - cy, 3.0));
    }

    // 8. Toolbar (only if there is a selection)
    if let Some(sel) = state.selection {
        render_toolbar(state, &sel, pixmap, drawn);
    }

    // 8b. Magnifier, while a region is being placed, dragged out or resized
    if state.magnifier_visible() {
        drawn.add(render_magnifier(state, pixmap));
    }

    // 9. Eyedropper preview (color swatch + hex near cursor)
    if state.active_tool == ToolKind::Eyedropper {
        if let Some(color) = state.eyedropper_preview {
            let mx = state.last_mouse_pos.x;
            let my = state.last_mouse_pos.y;

            // Small filled square showing the color
            let swatch_size = 24.0_f32;
            let swatch_x = mx + 16.0;
            let swatch_y = my + 16.0;

            if let Some(rect) =
                tiny_skia::Rect::from_xywh(swatch_x, swatch_y, swatch_size, swatch_size)
            {
                let mut paint = Paint::default();
                if let Some(c) = tiny_skia::Color::from_rgba(color.r, color.g, color.b, 1.0) {
                    paint.set_color(c);
                }
                paint.anti_alias = false;
                pixmap.fill_rect(rect, &paint, Transform::identity(), None);

                // White border around swatch
                let path = PathBuilder::from_rect(rect);
                let mut border = Paint::default();
                border.set_color(tiny_skia::Color::WHITE);
                border.anti_alias = true;
                let stroke = Stroke {
                    width: 1.5,
                    ..Stroke::default()
                };
                pixmap.stroke_path(&path, &border, &stroke, Transform::identity(), None);
            }

            // Hex code label below swatch (round, don't truncate)
            let r8 = (color.r * 255.0 + 0.5) as u8;
            let g8 = (color.g * 255.0 + 0.5) as u8;
            let b8 = (color.b * 255.0 + 0.5) as u8;
            let hex = format!("#{:02x}{:02x}{:02x}", r8, g8, b8);

            let font_size = 12.0_f32;
            let text_width = measure_text_width(&hex, font_size);

            let pad_x = 4.0_f32;
            let pad_y = 2.0_f32;
            let pill_w = text_width + pad_x * 2.0;
            let pill_h = font_size + pad_y * 2.0;
            let pill_x = swatch_x;
            let pill_y = swatch_y + swatch_size + 2.0;

            if let Some(bg) = rounded_rect_path(pill_x, pill_y, pill_w, pill_h, 3.0) {
                let mut bg_paint = Paint::default();
                bg_paint.set_color(crate::theme::skia(crate::theme::bg_1(), 0.9));
                bg_paint.anti_alias = true;
                pixmap.fill_path(
                    &bg,
                    &bg_paint,
                    tiny_skia::FillRule::Winding,
                    Transform::identity(),
                    None,
                );
            }

            use crate::tools::render_text_annotation;
            let text_pos = crate::geometry::Point::new(pill_x + pad_x, pill_y + pad_y);
            let white = crate::theme::gcolor(crate::theme::text_normal(), 1.0);
            render_text_annotation(pixmap, &text_pos, &hex, &white, font_size);
            // The swatch and the label under it.
            drawn.add(Area::around(
                swatch_x,
                swatch_y,
                swatch_size.max(pill_w),
                swatch_size + 2.0 + pill_h,
                4.0,
            ));
        }
    }

    // 10. In-overlay toast notification
    state.clear_expired_toast();
    if let Some(ref msg) = state.toast_message {
        let font_size = 14.0_f32;
        let text_width = measure_text_width(msg, font_size);

        let pad_x = 16.0_f32;
        let pad_y = 10.0_f32;
        let toast_w = text_width + pad_x * 2.0;
        let toast_h = font_size + pad_y * 2.0;
        let toast_x = (pixmap.width() as f32 - toast_w) / 2.0;
        let toast_y = pixmap.height() as f32 - 80.0;

        // Background pill
        if let Some(bg) = rounded_rect_path(toast_x, toast_y, toast_w, toast_h, 8.0) {
            let mut bg_paint = Paint::default();
            bg_paint.set_color(crate::theme::skia(crate::theme::bg_1(), 0.92));
            bg_paint.anti_alias = true;
            pixmap.fill_path(
                &bg,
                &bg_paint,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );
            // Border
            let mut border = Paint::default();
            border.set_color(crate::theme::skia(crate::theme::accent(), 0.6));
            border.anti_alias = true;
            let stroke = Stroke {
                width: 1.0,
                ..Stroke::default()
            };
            pixmap.stroke_path(&bg, &border, &stroke, Transform::identity(), None);
        }

        // Text
        use crate::tools::render_text_annotation;
        let text_pos = crate::geometry::Point::new(toast_x + pad_x, toast_y + pad_y);
        let text_color = crate::theme::gcolor(crate::theme::text_normal(), 1.0);
        render_text_annotation(pixmap, &text_pos, msg, &text_color, font_size);
        drawn.add(Area::around(toast_x, toast_y, toast_w, toast_h, 4.0));
    }
}

/// Capture pixels shown along each side of the magnifier (odd, so one is
/// in the middle), and how many times larger each is drawn.
const MAGNIFIER_CELLS: i32 = 15;
const MAGNIFIER_ZOOM: i32 = 8;

/// A magnified view of the capture around the pointer, with the pixel under
/// it outlined and its screen position underneath, so a region can start
/// and end on an exact pixel. It is part of the overlay only, never of
/// what is copied or saved. Returns the area it covers.
fn render_magnifier(state: &OverlayState, pixmap: &mut tiny_skia::Pixmap) -> Area {
    let pw = pixmap.width() as i32;
    let ph = pixmap.height() as i32;
    let size = MAGNIFIER_CELLS * MAGNIFIER_ZOOM;
    let half = MAGNIFIER_CELLS / 2;
    let label_h = 22;
    let gap = 24;
    let mx = state.last_mouse_pos.x.floor() as i32;
    let my = state.last_mouse_pos.y.floor() as i32;

    // Below and to the right of the pointer; on the other side near an edge.
    let mut x = mx + gap;
    if x + size + 2 > pw {
        x = mx - gap - size;
    }
    let mut y = my + gap;
    if y + size + label_h + 2 > ph {
        y = my - gap - size - label_h;
    }
    let x = x.clamp(1, (pw - size - 1).max(1));
    let y = y.clamp(1, (ph - size - label_h - 1).max(1));

    // The capture's own pixels (not the dimmed ones on screen), enlarged.
    let src = &state.screenshot.pixels;
    let sw = state.screenshot.width as i32;
    let sh = state.screenshot.height as i32;
    let off_capture = crate::theme::bg_1();
    let out = pixmap.data_mut();
    for row in 0..size {
        let py = y + row;
        if py < 0 || py >= ph {
            continue;
        }
        let sy = my - half + row / MAGNIFIER_ZOOM;
        for col in 0..size {
            let px = x + col;
            if px < 0 || px >= pw {
                continue;
            }
            let sx = mx - half + col / MAGNIFIER_ZOOM;
            let rgb = (sx >= 0 && sy >= 0 && sx < sw && sy < sh)
                .then(|| ((sy * sw + sx) * 4) as usize)
                .and_then(|i| src.get(i..i + 3))
                .map_or([off_capture.0, off_capture.1, off_capture.2], |p| {
                    [p[0], p[1], p[2]]
                });
            let o = ((py * pw + px) * 4) as usize;
            if let Some(dst) = out.get_mut(o..o + 4) {
                dst.copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            }
        }
    }

    // A one-pixel square outline just outside (x, y, side, side).
    let outline = |pixmap: &mut tiny_skia::Pixmap, x: i32, y: i32, side: i32, color| {
        let mut paint = Paint::default();
        paint.set_color(color);
        paint.anti_alias = false;
        let (x, y, side) = (x as f32, y as f32, side as f32);
        for (rx, ry, rw, rh) in [
            (x - 1.0, y - 1.0, side + 2.0, 1.0),
            (x - 1.0, y + side, side + 2.0, 1.0),
            (x - 1.0, y, 1.0, side),
            (x + side, y, 1.0, side),
        ] {
            if let Some(rect) = tiny_skia::Rect::from_xywh(rx, ry, rw, rh) {
                pixmap.fill_rect(rect, &paint, Transform::identity(), None);
            }
        }
    };
    // The pixel under the pointer, then the frame: accent with a dark line
    // outside it, so both show on any colour.
    let cx = x + half * MAGNIFIER_ZOOM;
    let cy = y + half * MAGNIFIER_ZOOM;
    let dark = tiny_skia::Color::from_rgba8(0, 0, 0, 200);
    let accent = crate::theme::skia(crate::theme::accent(), 1.0);
    outline(pixmap, cx - 1, cy - 1, MAGNIFIER_ZOOM + 2, dark);
    outline(pixmap, cx, cy, MAGNIFIER_ZOOM, accent);
    outline(pixmap, x - 1, y - 1, size + 2, dark);
    outline(pixmap, x, y, size, accent);

    // The pointer's position on the screen (not in the capture: with a
    // monitor to the left of the main one these differ).
    let label = format!(
        "{}, {}",
        mx + state.screenshot.x_offset,
        my + state.screenshot.y_offset
    );
    let font_size = 12.0;
    let pill_w = measure_text_width(&label, font_size) + 12.0;
    let pill_y = (y + size + 3) as f32;
    if let Some(pill) = rounded_rect_path(x as f32, pill_y, pill_w, 18.0, 4.0) {
        let mut paint = Paint::default();
        paint.set_color(crate::theme::skia(crate::theme::bg_1(), 0.9));
        paint.anti_alias = true;
        pixmap.fill_path(
            &pill,
            &paint,
            tiny_skia::FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
    crate::tools::render_text_annotation(
        pixmap,
        &crate::geometry::Point::new(x as f32 + 6.0, pill_y + 2.0),
        &label,
        &crate::theme::gcolor(crate::theme::text_normal(), 1.0),
        font_size,
    );
    // The frame's two outlines, the pixels, and the label under them.
    Area::new(
        x - 3,
        y - 3,
        x + size.max(pill_w.ceil() as i32) + 4,
        y + size + label_h + 4,
    )
}

/// Build a rounded rectangle path with given corner radius.
fn rounded_rect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let r = r.min(w / 2.0).min(h / 2.0);
    let mut pb = PathBuilder::new();
    // Top edge (starting after top-left corner)
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    // Top-right corner
    pb.quad_to(x + w, y, x + w, y + r);
    // Right edge
    pb.line_to(x + w, y + h - r);
    // Bottom-right corner
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    // Bottom edge
    pb.line_to(x + r, y + h);
    // Bottom-left corner
    pb.quad_to(x, y + h, x, y + h - r);
    // Left edge
    pb.line_to(x, y + r);
    // Top-left corner
    pb.quad_to(x, y, x + r, y);
    pb.close();
    pb.finish()
}

/// Draw a dashed selection highlight rectangle around a selected annotation,
/// plus resize handles at the four corners.
fn render_selection_highlight(pixmap: &mut tiny_skia::Pixmap, x: f32, y: f32, w: f32, h: f32) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let highlight_color = crate::theme::skia(crate::theme::accent(), 0.8);
    if let Some(rect) = tiny_skia::Rect::from_xywh(x, y, w, h) {
        let path = PathBuilder::from_rect(rect);
        let mut paint = Paint::default();
        paint.set_color(highlight_color);
        paint.anti_alias = true;
        let stroke = Stroke {
            width: 1.5,
            dash: tiny_skia::StrokeDash::new(vec![4.0, 4.0], 0.0),
            ..Stroke::default()
        };
        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }

    // Draw resize handles at the four corners
    let handles = [(x, y), (x + w, y), (x, y + h), (x + w, y + h)];
    for (hx, hy) in &handles {
        let hs = 4.0; // half-size
        if let Some(rect) = tiny_skia::Rect::from_xywh(hx - hs, hy - hs, hs * 2.0, hs * 2.0) {
            // White filled square
            let mut fill = Paint::default();
            fill.set_color(tiny_skia::Color::WHITE);
            fill.anti_alias = false;
            pixmap.fill_rect(rect, &fill, Transform::identity(), None);

            // Accent border to match the dashed selection border
            let path = PathBuilder::from_rect(rect);
            let mut border = Paint::default();
            border.set_color(highlight_color);
            border.anti_alias = true;
            let stroke = Stroke {
                width: 1.0,
                ..Stroke::default()
            };
            pixmap.stroke_path(&path, &border, &stroke, Transform::identity(), None);
        }
    }
}

fn render_toolbar(
    state: &mut OverlayState,
    selection: &Selection,
    pixmap: &mut tiny_skia::Pixmap,
    drawn: &mut Drawn,
) {
    let visible = &state.visible_buttons;
    let visible_count = visible.len();
    let toolbar = Toolbar::position_for_dynamic(selection, pixmap.height() as f32, visible_count);
    let presets = Color::presets();
    // The bar and the shadow that sits a pixel right of it and two below.
    drawn.add(Area::around(
        toolbar.x,
        toolbar.y,
        toolbar.width + 1.0,
        toolbar.height + 2.0,
        3.0,
    ));

    // --- Toolbar background: rounded rect with subtle border ---
    if let Some(bg_path) =
        rounded_rect_path(toolbar.x, toolbar.y, toolbar.width, toolbar.height, 8.0)
    {
        // Shadow (offset dark rect behind)
        if let Some(shadow_path) = rounded_rect_path(
            toolbar.x + 1.0,
            toolbar.y + 2.0,
            toolbar.width,
            toolbar.height,
            8.0,
        ) {
            let mut shadow_paint = Paint::default();
            shadow_paint.set_color(tiny_skia::Color::from_rgba(0.0, 0.0, 0.0, 0.4).unwrap());
            shadow_paint.anti_alias = true;
            pixmap.fill_path(
                &shadow_path,
                &shadow_paint,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
        // Background fill
        let mut bg_paint = Paint::default();
        bg_paint.set_color(crate::theme::skia(crate::theme::bg_1(), 0.95));
        bg_paint.anti_alias = true;
        pixmap.fill_path(
            &bg_path,
            &bg_paint,
            tiny_skia::FillRule::Winding,
            Transform::identity(),
            None,
        );
        // Subtle border
        let mut border_paint = Paint::default();
        border_paint.set_color(crate::theme::skia(crate::theme::text_normal(), 0.12));
        border_paint.anti_alias = true;
        let border_stroke = Stroke {
            width: 1.0,
            ..Stroke::default()
        };
        pixmap.stroke_path(
            &bg_path,
            &border_paint,
            &border_stroke,
            Transform::identity(),
            None,
        );
    }

    // --- Separators between button groups (tools | colours | actions) ---
    let group = |button: usize| {
        BUTTONS.get(button).map(|def| match def.action {
            ButtonAction::Tool(_) => 0,
            ButtonAction::Color(_) => 1,
            _ => 2,
        })
    };
    let sep_color = crate::theme::skia(crate::theme::text_normal(), 0.15);
    for (vis_idx, pair) in visible.windows(2).enumerate() {
        if group(pair[0]) == group(pair[1]) {
            continue;
        }
        let (bx, _, bw, _) = toolbar.button_rect(vis_idx);
        let sep_x = bx + bw + TOOLBAR_PADDING / 2.0;
        let mut pb = PathBuilder::new();
        pb.move_to(sep_x, toolbar.y + 8.0);
        pb.line_to(sep_x, toolbar.y + toolbar.height - 8.0);
        if let Some(path) = pb.finish() {
            let mut paint = Paint::default();
            paint.set_color(sep_color);
            paint.anti_alias = true;
            let stroke = Stroke {
                width: 1.0,
                ..Stroke::default()
            };
            pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        }
    }

    // --- Render each button ---
    for (vis_idx, &i) in visible.iter().enumerate() {
        let (bx, by, bw, bh) = toolbar.button_rect(vis_idx);
        let Some(def) = BUTTONS.get(i) else { continue };

        let is_active = match def.action {
            ButtonAction::Tool(kind) => state.active_tool == kind,
            ButtonAction::Color(idx) => idx < presets.len() && state.current_color == presets[idx],
            _ => false,
        };

        // Button background: rounded rect
        let btn_radius = 5.0;
        if let Some(btn_path) = rounded_rect_path(bx, by, bw, bh, btn_radius) {
            if is_active {
                // Active: filled highlight
                let mut fill = Paint::default();
                fill.set_color(crate::theme::skia(crate::theme::accent(), 0.3));
                fill.anti_alias = true;
                pixmap.fill_path(
                    &btn_path,
                    &fill,
                    tiny_skia::FillRule::Winding,
                    Transform::identity(),
                    None,
                );
                // Active border glow
                let mut glow = Paint::default();
                glow.set_color(crate::theme::skia(crate::theme::accent(), 0.7));
                glow.anti_alias = true;
                let glow_stroke = Stroke {
                    width: 1.5,
                    ..Stroke::default()
                };
                pixmap.stroke_path(&btn_path, &glow, &glow_stroke, Transform::identity(), None);
            } else {
                // Inactive: subtle hover-ready background
                let mut fill = Paint::default();
                fill.set_color(crate::theme::skia(crate::theme::text_normal(), 0.08));
                fill.anti_alias = true;
                pixmap.fill_path(
                    &btn_path,
                    &fill,
                    tiny_skia::FillRule::Winding,
                    Transform::identity(),
                    None,
                );
            }
        }

        // --- SVG icon rendering for tool buttons and action buttons ---
        if let Some(name) = def.icon {
            let icon_size = (bw - 12.0).max(12.0) as u32;
            let active_hex = crate::theme::hex(crate::theme::text_normal());
            let inactive_hex = crate::theme::hex(crate::theme::text_muted());
            let color_hex = if is_active {
                active_hex.as_str()
            } else {
                inactive_hex.as_str()
            };
            if let Some(icon_pixmap) = state.icon_cache.get_or_render(name, icon_size, color_hex) {
                let icon_x = (bx + (bw - icon_size as f32) / 2.0) as i32;
                let icon_y = (by + (bh - icon_size as f32) / 2.0) as i32;
                blend_pixmap(pixmap, icon_pixmap, icon_x, icon_y);
            }
        }

        if let ButtonAction::Color(idx) = def.action {
            // Color swatch: rounded filled rect with border
            if let Some(c) = presets.get(idx) {
                let (r, g, b) = (c.r, c.g, c.b);
                let inset = 6.0;
                if let Some(swatch_path) = rounded_rect_path(
                    bx + inset,
                    by + inset,
                    bw - inset * 2.0,
                    bh - inset * 2.0,
                    3.0,
                ) {
                    // Fill with color
                    let mut paint = Paint::default();
                    paint.set_color(
                        tiny_skia::Color::from_rgba(r, g, b, 1.0)
                            .unwrap_or(tiny_skia::Color::WHITE),
                    );
                    paint.anti_alias = true;
                    pixmap.fill_path(
                        &swatch_path,
                        &paint,
                        tiny_skia::FillRule::Winding,
                        Transform::identity(),
                        None,
                    );
                    // Border (darker for light colors, lighter for dark)
                    let border_alpha = if r + g + b > 2.0 { 0.3 } else { 0.5 };
                    let mut border = Paint::default();
                    border.set_color(
                        tiny_skia::Color::from_rgba(1.0, 1.0, 1.0, border_alpha)
                            .unwrap_or(tiny_skia::Color::WHITE),
                    );
                    border.anti_alias = true;
                    let stroke = Stroke {
                        width: 1.0,
                        ..Stroke::default()
                    };
                    pixmap.stroke_path(&swatch_path, &border, &stroke, Transform::identity(), None);
                }
            }
        }
    }

    // --- Tooltip for hovered button ---
    let mouse = crate::geometry::Point::new(state.last_mouse_pos.x, state.last_mouse_pos.y);
    if let Some(vis_idx) = toolbar.hit_test_dynamic(mouse, visible_count) {
        let btn_idx = visible[vis_idx];
        let label = BUTTONS.get(btn_idx).map(|d| d.tooltip).unwrap_or("");

        if !label.is_empty() {
            let (btn_x, _btn_y, btn_w, _btn_h) = toolbar.button_rect(vis_idx);

            let font_size = 12.0;
            let text_width = measure_text_width(label, font_size);

            let pad_x = 6.0;
            let pad_y = 4.0;
            let tip_w = text_width + pad_x * 2.0;
            let tip_h = font_size + pad_y * 2.0;

            // Center tooltip above the button
            let tip_x = (btn_x + btn_w / 2.0 - tip_w / 2.0)
                .max(toolbar.x)
                .min(toolbar.x + toolbar.width - tip_w);
            let tip_y = toolbar.y - tip_h - 6.0;

            // Background pill
            if let Some(bg) = rounded_rect_path(tip_x, tip_y, tip_w, tip_h, 4.0) {
                let mut bg_paint = Paint::default();
                bg_paint.set_color(crate::theme::skia(crate::theme::bg_1(), 0.95));
                bg_paint.anti_alias = true;
                pixmap.fill_path(
                    &bg,
                    &bg_paint,
                    tiny_skia::FillRule::Winding,
                    Transform::identity(),
                    None,
                );

                // Border
                let mut border_paint = Paint::default();
                border_paint.set_color(crate::theme::skia(crate::theme::text_normal(), 0.2));
                border_paint.anti_alias = true;
                let border_stroke = Stroke {
                    width: 0.5,
                    ..Stroke::default()
                };
                pixmap.stroke_path(
                    &bg,
                    &border_paint,
                    &border_stroke,
                    Transform::identity(),
                    None,
                );
            }

            // Text
            use crate::tools::render_text_annotation;
            let text_pos = crate::geometry::Point::new(tip_x + pad_x, tip_y + pad_y);
            let white = crate::theme::gcolor(crate::theme::text_normal(), 1.0);
            render_text_annotation(pixmap, &text_pos, label, &white, font_size);
            drawn.add(Area::around(tip_x, tip_y, tip_w, tip_h, 4.0));
        }
    }
}

use crate::overlay::toolbar::TOOLBAR_PADDING;

/// Render a small label showing the selection dimensions (e.g. "1920 × 1080")
/// near the bottom-right corner of the selection, flipping position if near screen edges.
fn render_size_label(
    pixmap: &mut tiny_skia::Pixmap,
    sel: &Selection,
    screen_w: f32,
    screen_h: f32,
) -> Area {
    let font_size = 14.0;

    let label = format!("{} \u{00D7} {}", sel.width as u32, sel.height as u32);
    let text_width = measure_text_width(&label, font_size);

    let pad_x: f32 = 8.0;
    let pad_y: f32 = 4.0;
    let pill_w = text_width + pad_x * 2.0;
    let pill_h = font_size + pad_y * 2.0;
    let margin = 4.0;

    // Default: bottom-right, offset outside selection
    let mut pill_x = sel.x + sel.width - pill_w + margin;
    let mut pill_y = sel.y + sel.height + margin;

    // Flip left if near right edge
    if pill_x + pill_w > screen_w {
        pill_x = sel.x - margin;
    }
    // Flip above if near bottom edge
    if pill_y + pill_h > screen_h {
        pill_y = sel.y - pill_h - margin;
    }

    // Clamp to screen
    if pill_x < 0.0 {
        pill_x = 0.0;
    }
    if pill_y < 0.0 {
        pill_y = 0.0;
    }

    // Draw background pill (dark semi-transparent rounded rect)
    if let Some(bg_path) = rounded_rect_path(pill_x, pill_y, pill_w, pill_h, 4.0) {
        let mut paint = Paint::default();
        paint.set_color(crate::theme::skia(crate::theme::bg_1(), 0.85));
        paint.anti_alias = true;
        pixmap.fill_path(
            &bg_path,
            &paint,
            tiny_skia::FillRule::Winding,
            Transform::identity(),
            None,
        );
    }

    // Render text on top via the shared single-line text path
    use crate::tools::render_text_annotation;
    let text_pos = crate::geometry::Point::new(pill_x + pad_x, pill_y + pad_y);
    let white = crate::theme::gcolor(crate::theme::text_normal(), 1.0);
    render_text_annotation(pixmap, &text_pos, &label, &white, font_size);
    Area::around(pill_x, pill_y, pill_w, pill_h, 4.0)
}
