use crate::capture::CapturedScreen;
use crate::geometry::{Color, Point};
use crate::icons::IconCache;
use crate::overlay::selection::{HitZone, Selection};
use crate::tools::arrow::ArrowTool;
use crate::tools::circle::CircleTool;
use crate::tools::highlight::HighlightTool;
use crate::tools::line::LineTool;
use crate::tools::measurement::MeasurementTool;
use crate::tools::pencil::PencilTool;
use crate::tools::pixelate::PixelateTool;
use crate::tools::rectangle::RectangleTool;
use crate::tools::redact::RedactTool;
use crate::tools::rounded_rect::RoundedRectTool;
use crate::tools::spotlight::SpotlightTool;
use crate::tools::step_marker::StepMarkerTool;
use crate::tools::text::TextTool;
use crate::tools::{Annotation, AnnotationTool, ResizeHandle, ToolKind, UndoAction};
use crate::window_detect::WinRect;

pub enum AppState {
    Idle,
    Capturing(Box<OverlayState>),
}

pub struct OverlayState {
    /// The capture. It is the only copy the overlay keeps: the renderer
    /// reads it for the background (dimming as it goes), the tools for the
    /// pixels they work on, and the export for the result.
    pub screenshot: CapturedScreen,
    pub selection: Option<Selection>,
    pub annotations: Vec<Annotation>,
    pub undo_stack: Vec<UndoAction>,
    pub redo_stack: Vec<UndoAction>,
    /// Snapshot of an annotation before a drag/resize starts, for undo recording.
    pub pre_drag_annotation: Option<(usize, Annotation)>,
    pub active_tool: ToolKind,
    pub arrow_tool: ArrowTool,
    pub rectangle_tool: RectangleTool,
    pub rounded_rect_tool: RoundedRectTool,
    pub circle_tool: CircleTool,
    pub line_tool: LineTool,
    pub pencil_tool: PencilTool,
    pub highlight_tool: HighlightTool,
    pub text_tool: TextTool,
    pub pixelate_tool: PixelateTool,
    pub step_marker_tool: StepMarkerTool,
    pub spotlight_tool: SpotlightTool,
    pub measurement_tool: MeasurementTool,
    pub redact_tool: RedactTool,
    pub text_input_active: bool,
    pub text_input_buffer: String,
    /// The caret, as a byte offset into `text_input_buffer`.
    pub text_caret: usize,
    /// What an input method (IME) is composing: drawn at the caret, not yet
    /// part of the text.
    pub text_preedit: String,
    pub text_input_position: Point,
    pub text_input_font_size: f32,
    /// When re-editing an existing Text annotation: its original index and
    /// content, so committing records a single Modify undo step (and Escape
    /// restores it) instead of a Delete + Add pair.
    pub text_edit_origin: Option<(usize, Annotation)>,
    pub current_color: Color,
    pub current_thickness: f32,
    pub is_selecting: bool,
    pub drag_start: Option<Point>,
    pub drag_zone: Option<HitZone>,
    pub last_mouse_pos: Point,
    pub selected_index: Option<usize>,
    pub select_drag_start: Option<Point>,
    pub resize_handle: Option<ResizeHandle>,
    pub icon_cache: IconCache,
    pub eyedropper_preview: Option<Color>,
    /// In-overlay toast notification (visible on top of the overlay)
    pub toast_message: Option<String>,
    pub toast_until: Option<std::time::Instant>,
    /// Upload confirmation — true means "user confirmed, proceed"
    pub upload_confirmed: bool,
    /// Visible toolbar button indices (into `toolbar::BUTTONS`), computed from ToolbarConfig.
    pub visible_buttons: Vec<usize>,
    /// Window-capture mode: the windows on screen, front to back. While this
    /// is `Some`, moving the mouse selects the window under it and a click
    /// confirms it.
    pub window_pick: Option<Vec<WinRect>>,
    /// "Copy as soon as a region is drawn" (`finish_on_release` in config).
    pub finish_on_release: bool,
    /// Show the magnifier while a region is being placed (`magnifier` in
    /// config).
    pub magnifier: bool,
    /// The modifier keys held now. Shift constrains a drag.
    pub mods: crate::keys::Mods,
    /// Where the drag that is drawing an annotation started.
    pub draw_anchor: Option<Point>,
    /// A run of arrow-key nudges in progress: the annotation, and the
    /// length of the undo stack once its one undo step was recorded.
    pub nudging: Option<(usize, usize)>,
}

impl OverlayState {
    pub fn new(screenshot: CapturedScreen, config: &crate::config::Config) -> Self {
        let color = config.default_color();
        let thickness = config.clamped_thickness();

        let expected = (screenshot.width as usize)
            .checked_mul(screenshot.height as usize)
            .and_then(|wh| wh.checked_mul(4))
            .unwrap_or(0);
        if screenshot.pixels.len() < expected {
            // The renderer draws the rows that are there and leaves the rest.
            tracing::warn!(
                "Screenshot pixel buffer too small: expected {} bytes, got {}",
                expected,
                screenshot.pixels.len()
            );
        }

        Self {
            screenshot,
            selection: None,
            annotations: Vec::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            pre_drag_annotation: None,
            active_tool: ToolKind::Arrow,
            arrow_tool: ArrowTool::new(color, thickness),
            rectangle_tool: RectangleTool::new(color, thickness),
            rounded_rect_tool: RoundedRectTool::new(color, thickness),
            circle_tool: CircleTool::new(color, thickness),
            line_tool: LineTool::new(color, thickness),
            pencil_tool: PencilTool::new(color, thickness),
            highlight_tool: HighlightTool::new(color),
            text_tool: TextTool::new(color, 20.0),
            pixelate_tool: PixelateTool::new(10),
            step_marker_tool: StepMarkerTool::new(color, 28.0),
            spotlight_tool: SpotlightTool::new(),
            measurement_tool: MeasurementTool::new(color),
            redact_tool: RedactTool::new(),
            text_input_active: false,
            text_input_buffer: String::new(),
            text_caret: 0,
            text_preedit: String::new(),
            text_input_position: Point::new(0.0, 0.0),
            text_input_font_size: 20.0,
            text_edit_origin: None,
            current_color: color,
            current_thickness: thickness,
            is_selecting: false,
            drag_start: None,
            drag_zone: None,
            last_mouse_pos: Point::new(0.0, 0.0),
            selected_index: None,
            select_drag_start: None,
            resize_handle: None,
            icon_cache: IconCache::new(),
            eyedropper_preview: None,
            toast_message: None,
            toast_until: None,
            upload_confirmed: false,
            visible_buttons: config.toolbar.visible_button_indices(),
            window_pick: None,
            finish_on_release: config.general.finish_on_release,
            magnifier: config.general.magnifier,
            mods: crate::keys::Mods::NONE,
            draw_anchor: None,
            nudging: None,
        }
    }

    /// The tool that draws `kind`'s annotations. `None` for Select and the
    /// Eyedropper, which draw nothing. Everything that forwards mouse input
    /// to "the active tool" goes through here, so a new tool is one arm in
    /// this function and in `tool_mut`.
    pub fn tool(&self, kind: ToolKind) -> Option<&dyn AnnotationTool> {
        Some(match kind {
            ToolKind::Select | ToolKind::Eyedropper => return None,
            ToolKind::Arrow => &self.arrow_tool,
            ToolKind::Rectangle => &self.rectangle_tool,
            ToolKind::RoundedRect => &self.rounded_rect_tool,
            ToolKind::Circle => &self.circle_tool,
            ToolKind::Line => &self.line_tool,
            ToolKind::Pencil => &self.pencil_tool,
            ToolKind::Highlight => &self.highlight_tool,
            ToolKind::Text => &self.text_tool,
            ToolKind::Pixelate => &self.pixelate_tool,
            ToolKind::StepMarker => &self.step_marker_tool,
            ToolKind::Spotlight => &self.spotlight_tool,
            ToolKind::Measurement => &self.measurement_tool,
            ToolKind::Redact => &self.redact_tool,
        })
    }

    /// See [`OverlayState::tool`].
    pub fn tool_mut(&mut self, kind: ToolKind) -> Option<&mut dyn AnnotationTool> {
        Some(match kind {
            ToolKind::Select | ToolKind::Eyedropper => return None,
            ToolKind::Arrow => &mut self.arrow_tool,
            ToolKind::Rectangle => &mut self.rectangle_tool,
            ToolKind::RoundedRect => &mut self.rounded_rect_tool,
            ToolKind::Circle => &mut self.circle_tool,
            ToolKind::Line => &mut self.line_tool,
            ToolKind::Pencil => &mut self.pencil_tool,
            ToolKind::Highlight => &mut self.highlight_tool,
            ToolKind::Text => &mut self.text_tool,
            ToolKind::Pixelate => &mut self.pixelate_tool,
            ToolKind::StepMarker => &mut self.step_marker_tool,
            ToolKind::Spotlight => &mut self.spotlight_tool,
            ToolKind::Measurement => &mut self.measurement_tool,
            ToolKind::Redact => &mut self.redact_tool,
        })
    }

    /// Set the drawing color on every tool that has one, plus `current_color`.
    pub fn set_color_all(&mut self, color: Color) {
        self.current_color = color;
        self.arrow_tool.set_color(color);
        self.rectangle_tool.set_color(color);
        self.rounded_rect_tool.set_color(color);
        self.circle_tool.set_color(color);
        self.line_tool.set_color(color);
        self.pencil_tool.set_color(color);
        self.highlight_tool.set_color(color);
        self.text_tool.set_color(color);
        self.step_marker_tool.set_color(color);
        self.measurement_tool.set_color(color);
    }

    /// Set the stroke thickness on every tool that has one, plus `current_thickness`.
    pub fn set_thickness_all(&mut self, thickness: f32) {
        let t = thickness.clamp(1.0, 20.0);
        self.current_thickness = t;
        self.arrow_tool.set_thickness(t);
        self.rectangle_tool.set_thickness(t);
        self.rounded_rect_tool.set_thickness(t);
        self.circle_tool.set_thickness(t);
        self.line_tool.set_thickness(t);
        self.pencil_tool.set_thickness(t);
    }

    /// Resync the step-marker counter to one past the highest number present,
    /// so undo/redo/delete don't leave gaps in the numbering.
    pub fn sync_step_counter(&mut self) {
        let max = self
            .annotations
            .iter()
            .filter_map(|a| match a {
                Annotation::StepMarker { number, .. } => Some(*number),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        self.step_marker_tool.set_next_number(max.saturating_add(1));
    }

    /// Crop the current selection and flatten all annotations into it.
    /// Returns `(pixels, width, height)`, or `None` if there is no selection
    /// or it has zero area.
    pub fn flattened_selection(&self) -> Option<(Vec<u8>, u32, u32)> {
        let sel = self.selection.as_ref()?;
        let r = sel.clamped(self.screenshot.width, self.screenshot.height);
        if r.w == 0 || r.h == 0 {
            return None;
        }
        let pixels = crate::export::crop_and_flatten(
            &self.screenshot.pixels,
            self.screenshot.width,
            r.x,
            r.y,
            r.w,
            r.h,
            &self.annotations,
        );
        Some((pixels, r.w, r.h))
    }

    /// Where the selection is on screen, in virtual-desktop pixels, clipped
    /// to the capture. `None` without a selection or with zero area.
    pub fn selection_on_screen(&self) -> Option<crate::display::Rect> {
        let sel = self.selection.as_ref()?;
        let r = sel.clamped(self.screenshot.width, self.screenshot.height);
        if r.w == 0 || r.h == 0 {
            return None;
        }
        Some(crate::display::Rect::new(
            self.screenshot.x_offset + r.x as i32,
            self.screenshot.y_offset + r.y as i32,
            r.w,
            r.h,
        ))
    }

    /// Crop the current selection WITHOUT annotations (raw screenshot pixels).
    pub fn raw_selection(&self) -> Option<(Vec<u8>, u32, u32)> {
        let sel = self.selection.as_ref()?;
        let r = sel.clamped(self.screenshot.width, self.screenshot.height);
        if r.w == 0 || r.h == 0 {
            return None;
        }
        let mut cropped = vec![0u8; (r.w as usize) * (r.h as usize) * 4];
        let src_w = self.screenshot.width;
        for row in 0..r.h {
            let src_row = r.y + row;
            if src_row >= self.screenshot.height {
                break;
            }
            let src_offset = ((src_row * src_w + r.x) * 4) as usize;
            let dst_offset = (row * r.w * 4) as usize;
            let copy_w = (r.w.min(src_w.saturating_sub(r.x)) * 4) as usize;
            let src_end = (src_offset + copy_w).min(self.screenshot.pixels.len());
            if src_end > src_offset {
                let actual = src_end - src_offset;
                cropped[dst_offset..dst_offset + actual]
                    .copy_from_slice(&self.screenshot.pixels[src_offset..src_end]);
            }
        }
        Some((cropped, r.w, r.h))
    }

    /// Show an in-overlay toast for the given duration
    pub fn show_toast(&mut self, message: String, duration_ms: u64) {
        self.toast_message = Some(message);
        self.toast_until =
            Some(std::time::Instant::now() + std::time::Duration::from_millis(duration_ms));
    }

    /// Clear expired toast
    pub fn clear_expired_toast(&mut self) {
        if let Some(until) = self.toast_until {
            if std::time::Instant::now() >= until {
                self.toast_message = None;
                self.toast_until = None;
            }
        }
    }
}
