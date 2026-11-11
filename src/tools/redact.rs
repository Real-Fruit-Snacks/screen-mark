use super::{Annotation, AnnotationTool};
use crate::geometry::{Color, Point, Size};

/// Draws a solid box over something that must not be seen: a password, an
/// address, a face. Pixelate keeps a blurred average of what is under it,
/// which is sometimes enough to work the original out; this keeps nothing.
pub struct RedactTool {
    color: Color,
    start: Option<Point>,
    current: Option<Point>,
    drawing: bool,
}

impl RedactTool {
    /// Black, whatever the drawing colour is. A box can be recoloured
    /// afterwards like any other annotation.
    pub fn new() -> Self {
        Self {
            color: Color::new(0.0, 0.0, 0.0, 1.0),
            start: None,
            current: None,
            drawing: false,
        }
    }

    fn make_annotation(&self, end: Point) -> Option<Annotation> {
        let start = self.start?;
        let min_x = start.x.min(end.x);
        let min_y = start.y.min(end.y);
        let w = (start.x - end.x).abs();
        let h = (start.y - end.y).abs();
        if w < 2.0 || h < 2.0 {
            return None; // too small — likely a click, not a drag
        }
        Some(Annotation::Redact {
            top_left: Point::new(min_x, min_y),
            size: Size::new(w, h),
            color: self.color,
        })
    }
}

impl Default for RedactTool {
    fn default() -> Self {
        Self::new()
    }
}

impl AnnotationTool for RedactTool {
    fn on_mouse_down(&mut self, pos: Point) {
        self.start = Some(pos);
        self.current = Some(pos);
        self.drawing = true;
    }

    fn on_mouse_move(&mut self, pos: Point) {
        if self.drawing {
            self.current = Some(pos);
        }
    }

    fn on_mouse_up(&mut self, pos: Point) -> Option<Annotation> {
        if !self.drawing {
            return None;
        }
        self.drawing = false;
        let annotation = self.make_annotation(pos);
        self.start = None;
        self.current = None;
        annotation
    }

    fn is_drawing(&self) -> bool {
        self.drawing
    }

    fn in_progress_annotation(&self) -> Option<Annotation> {
        if !self.drawing {
            return None;
        }
        self.current.and_then(|c| self.make_annotation(c))
    }
}
