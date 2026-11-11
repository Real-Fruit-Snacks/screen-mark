//! What the capture overlay does with a key press, a mouse move or a click.
//!
//! Everything here works on `OverlayState` and plain values, with no window,
//! so tests can drive the overlay end to end (the way `settings_ui` does for
//! Settings). `main.rs` turns winit events into these calls and carries out
//! the `Outcome`.

use winit::keyboard::{Key as WKey, PhysicalKey};
use winit::window::CursorIcon;

use crate::geometry::{Color, Point};
use crate::keys::{self, Key, KeyId, KeyMap, Mods, Named};
use crate::overlay::selection::{HitZone, Selection};
use crate::overlay::text_edit;
use crate::overlay::toolbar::{ButtonAction, Toolbar, BUTTONS};
use crate::state::OverlayState;
use crate::tools::{
    annotation_bounding_box, apply_redo, apply_undo, hit_test_annotation, measure_text_block,
    measure_text_width, move_annotation, recolor_annotation, record_undo, resize_annotation,
    text_line_height, Annotation, AnnotationTool, ResizeHandle, ToolKind, UndoAction, TOOL_ORDER,
};

/// How close, in pixels, a click has to be to a selection edge, a resize
/// handle or an annotation to grab it.
pub const GRAB: f32 = 8.0;

/// What the app should do after the overlay handled an input.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Outcome {
    /// Nothing visible changed.
    Nothing,
    /// The overlay changed; draw it again.
    Redraw,
    /// Close the overlay without using the capture.
    Close,
    Copy,
    Save,
    Pin,
    Ocr,
    Upload,
    /// Open the colour picker, starting from this preset (a right-click on
    /// its swatch), then call `apply_picked_color`.
    PickColor(usize),
    /// Read the clipboard's text and pass it to `insert_text`.
    Paste,
    /// Put the mouse pointer at this overlay position (an arrow key moved
    /// it by a pixel), then report the move with `on_cursor_moved`.
    MovePointer(Point),
}

/// One key press, as the overlay sees it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct KeyInput {
    pub mods: Mods,
    /// The key as bindings name it: a letter by what the layout types,
    /// everything else by position (`keys::key_from_winit`).
    pub key: Option<Key>,
    /// The named key the layout says this is (Enter, Esc, an arrow, ...).
    /// The fixed editor keys go by this, so a remapped key still works.
    pub logical: Option<Named>,
    /// The text this press types, if it types any.
    pub text: Option<String>,
}

impl KeyInput {
    pub fn from_winit(logical: &WKey, physical: &PhysicalKey, mods: Mods) -> Self {
        let named = keys::named_from_logical(logical);
        let text = match logical {
            WKey::Character(s) => Some(s.to_string()),
            _ if named == Some(Named::Space) => Some(" ".to_string()),
            _ => None,
        };
        Self {
            mods,
            key: keys::key_from_winit(logical, physical),
            logical: named,
            text,
        }
    }

    /// A named key with no modifiers (tests).
    pub fn named(n: Named) -> Self {
        Self {
            mods: Mods::NONE,
            key: Some(Key::Named(n)),
            logical: Some(n),
            text: (n == Named::Space).then(|| " ".to_string()),
        }
    }

    /// A key that types `s`, with no modifiers (tests).
    pub fn typed(s: &str) -> Self {
        let mut chars = s.chars();
        let key = match (chars.next(), chars.next()) {
            (Some(c), None) if c.is_ascii_alphanumeric() => Some(Key::Char(c.to_ascii_lowercase())),
            _ => None,
        };
        Self {
            mods: Mods::NONE,
            key,
            logical: None,
            text: Some(s.to_string()),
        }
    }

    pub fn with(mut self, mods: Mods) -> Self {
        self.mods = mods;
        self
    }

    /// The single character this press types (lowercase), for 2.0-style
    /// tool keys bound to any typed character.
    fn typed_char(&self) -> Option<char> {
        if self.logical.is_some() {
            return None;
        }
        let mut chars = self.text.as_deref()?.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) => Some(c.to_lowercase().next().unwrap_or(c)),
            _ => None,
        }
    }
}

impl OverlayState {
    // ───────────────────────── keyboard ─────────────────────────

    /// A key went down.
    pub fn on_key(&mut self, input: &KeyInput, keymap: &KeyMap) -> Outcome {
        self.mods = input.mods;
        // Typing a text annotation takes every key.
        if self.text_input_active {
            return self.text_key(input);
        }
        // A run of arrow-key nudges is one undo step; any other key ends it.
        let arrow = arrow_step(input);
        if arrow.is_none() {
            self.nudging = None;
        }

        // The rebindable key map comes first, so a bound Ctrl+Enter or
        // Ctrl+Delete runs its action; unbound Esc / Delete / Enter keep
        // their fixed meaning below.
        let action = input.key.and_then(|k| keymap.editor_action(input.mods, k));
        if let Some(action) = action {
            return match action {
                KeyId::Copy => Outcome::Copy,
                KeyId::Save => Outcome::Save,
                KeyId::Pin => Outcome::Pin,
                KeyId::Ocr => Outcome::Ocr,
                KeyId::Upload => Outcome::Upload,
                KeyId::Undo | KeyId::Redo => {
                    let changed = if action == KeyId::Undo {
                        apply_undo(
                            &mut self.annotations,
                            &mut self.undo_stack,
                            &mut self.redo_stack,
                        )
                    } else {
                        apply_redo(
                            &mut self.annotations,
                            &mut self.undo_stack,
                            &mut self.redo_stack,
                        )
                    };
                    if changed {
                        self.selected_index = None;
                        self.sync_step_counter();
                        Outcome::Redraw
                    } else {
                        Outcome::Nothing
                    }
                }
                KeyId::SelectAll => {
                    self.selection = Some(Selection {
                        x: 0.0,
                        y: 0.0,
                        width: self.screenshot.width as f32,
                        height: self.screenshot.height as f32,
                    });
                    Outcome::Redraw
                }
                _ => Outcome::Nothing,
            };
        }

        let tool = input
            .key
            .and_then(|k| keymap.tool(input.mods, k))
            .or_else(|| {
                // 2.0-style tool keys bound to a typed character.
                input
                    .typed_char()
                    .and_then(|c| keymap.tool(input.mods, Key::Typed(c)))
            });
        if let Some(i) = tool {
            return match TOOL_ORDER.get(i) {
                Some(kind) => {
                    self.active_tool = *kind;
                    self.selected_index = None;
                    Outcome::Redraw
                }
                None => Outcome::Nothing,
            };
        }

        match input.logical {
            Some(Named::Escape) => {
                if self.selected_index.is_some() {
                    self.selected_index = None;
                    self.select_drag_start = None;
                    Outcome::Redraw
                } else {
                    Outcome::Close
                }
            }
            Some(Named::Delete | Named::Backspace) => self.delete_selected(),
            // Quick copy: the same path as the Copy key and button.
            Some(Named::Enter) if self.selection.is_some() => Outcome::Copy,
            _ => match arrow {
                Some((dx, dy)) => self.nudge(dx, dy, input.mods.ctrl()),
                None => Outcome::Nothing,
            },
        }
    }

    /// An arrow key, for pixel-exact work. In order of what there is to
    /// move: the selected annotation; the region (with Ctrl, its bottom
    /// right corner, which resizes it); and before a region exists, or
    /// while one is being dragged out, the mouse pointer itself.
    fn nudge(&mut self, dx: f32, dy: f32, resize: bool) -> Outcome {
        let selected = self.selected_index.filter(|i| *i < self.annotations.len());
        if let Some(idx) = selected {
            // The first nudge of a run records the undo step.
            if self.nudging != Some((idx, self.undo_stack.len())) {
                let before = self.annotations[idx].clone();
                record_undo(
                    &mut self.undo_stack,
                    &mut self.redo_stack,
                    UndoAction::Modify(idx, before),
                );
                self.nudging = Some((idx, self.undo_stack.len()));
            }
            move_annotation(&mut self.annotations[idx], dx, dy);
            return Outcome::Redraw;
        }

        let (max_w, max_h) = (self.screenshot.width as f32, self.screenshot.height as f32);
        // While a window is being picked the outline follows the pointer,
        // so there it is the pointer that moves.
        let placed = !self.is_selecting && self.window_pick.is_none();
        match &mut self.selection {
            Some(sel) if placed => {
                if resize {
                    sel.width = (sel.width + dx).clamp(3.0, (max_w - sel.x).max(3.0));
                    sel.height = (sel.height + dy).clamp(3.0, (max_h - sel.y).max(3.0));
                } else {
                    sel.x = (sel.x + dx).clamp(0.0, (max_w - sel.width).max(0.0));
                    sel.y = (sel.y + dy).clamp(0.0, (max_h - sel.height).max(0.0));
                }
                Outcome::Redraw
            }
            _ => Outcome::MovePointer(Point::new(
                (self.last_mouse_pos.x + dx).clamp(0.0, max_w - 1.0),
                (self.last_mouse_pos.y + dy).clamp(0.0, max_h - 1.0),
            )),
        }
    }

    /// The modifier keys changed. Shift constrains what is being dragged
    /// (see `constrained`), so a drag in progress is redone with the new
    /// state.
    pub fn set_modifiers(&mut self, mods: Mods) -> Outcome {
        let shift_changed = mods.shift() != self.mods.shift();
        self.mods = mods;
        let dragging = self.is_selecting
            || self
                .tool(self.active_tool)
                .is_some_and(|tool| tool.is_drawing());
        if shift_changed && dragging {
            self.on_cursor_moved(self.last_mouse_pos)
        } else {
            Outcome::Nothing
        }
    }

    /// Where a drag from `anchor` to `pos` ends. With Shift held, lines,
    /// arrows and measurements turn in steps of 15 degrees, and boxes,
    /// ellipses and the region itself become squares and circles.
    fn constrained(&self, anchor: Option<Point>, pos: Point, kind: Option<ToolKind>) -> Point {
        let Some(anchor) = anchor.filter(|_| self.mods.shift()) else {
            return pos;
        };
        let (dx, dy) = (pos.x - anchor.x, pos.y - anchor.y);
        match kind {
            Some(ToolKind::Arrow | ToolKind::Line | ToolKind::Measurement) => {
                let step = std::f32::consts::PI / 12.0;
                let angle = (dy.atan2(dx) / step).round() * step;
                let length = dx.hypot(dy);
                Point::new(
                    anchor.x + length * angle.cos(),
                    anchor.y + length * angle.sin(),
                )
            }
            Some(ToolKind::Pencil | ToolKind::Text | ToolKind::StepMarker) => pos,
            // Every other tool drags out a box, as does the region (`None`).
            _ => {
                let side = dx.abs().max(dy.abs());
                Point::new(anchor.x + side.copysign(dx), anchor.y + side.copysign(dy))
            }
        }
    }

    fn delete_selected(&mut self) -> Outcome {
        let Some(idx) = self.selected_index else {
            return Outcome::Nothing;
        };
        if idx >= self.annotations.len() {
            return Outcome::Nothing;
        }
        let removed = self.annotations.remove(idx);
        record_undo(
            &mut self.undo_stack,
            &mut self.redo_stack,
            UndoAction::Delete(idx, removed),
        );
        self.selected_index = None;
        self.sync_step_counter();
        Outcome::Redraw
    }

    // ───────────────────────── text ─────────────────────────

    /// A key while a text annotation is being typed.
    fn text_key(&mut self, input: &KeyInput) -> Outcome {
        let size = self.text_input_font_size;
        let width = move |s: &str| measure_text_width(s, size);
        let moved = |did: bool| {
            if did {
                Outcome::Redraw
            } else {
                Outcome::Nothing
            }
        };
        match input.logical {
            // Enter finishes; Shift+Enter starts a new line.
            Some(Named::Enter) if input.mods.shift() => self.insert_text("\n"),
            Some(Named::Enter) => {
                self.commit_text();
                Outcome::Redraw
            }
            Some(Named::Escape) => {
                self.cancel_text();
                Outcome::Redraw
            }
            Some(Named::Backspace) => moved(text_edit::backspace(
                &mut self.text_input_buffer,
                &mut self.text_caret,
            )),
            Some(Named::Delete) => moved(text_edit::delete(
                &mut self.text_input_buffer,
                &mut self.text_caret,
            )),
            Some(Named::Left) => moved(text_edit::left(
                &self.text_input_buffer,
                &mut self.text_caret,
            )),
            Some(Named::Right) => moved(text_edit::right(
                &self.text_input_buffer,
                &mut self.text_caret,
            )),
            Some(Named::Home) => moved(text_edit::home(
                &self.text_input_buffer,
                &mut self.text_caret,
            )),
            Some(Named::End) => moved(text_edit::end(
                &self.text_input_buffer,
                &mut self.text_caret,
            )),
            Some(Named::Up) => moved(text_edit::vertical(
                &self.text_input_buffer,
                &mut self.text_caret,
                true,
                &width,
            )),
            Some(Named::Down) => moved(text_edit::vertical(
                &self.text_input_buffer,
                &mut self.text_caret,
                false,
                &width,
            )),
            Some(Named::Space) => self.insert_text(" "),
            // Tab, F-keys, PageUp, ...: nothing while typing.
            Some(_) => Outcome::Nothing,
            None => match input.text.as_deref() {
                Some(t) if input.mods.ctrl() && t.eq_ignore_ascii_case("v") => Outcome::Paste,
                // Other Ctrl chords are commands, not text.
                Some(_) if input.mods.ctrl() => Outcome::Nothing,
                Some(t) => self.insert_text(t),
                None => Outcome::Nothing,
            },
        }
    }

    /// Put `text` in at the caret: typed, pasted, or committed by an input
    /// method. Does nothing unless a text annotation is being typed.
    pub fn insert_text(&mut self, text: &str) -> Outcome {
        if !self.text_input_active {
            return Outcome::Nothing;
        }
        self.text_preedit.clear();
        text_edit::insert(&mut self.text_input_buffer, &mut self.text_caret, text);
        Outcome::Redraw
    }

    /// What an input method is composing: shown at the caret, not yet part
    /// of the text. An empty string ends the composition.
    pub fn set_preedit(&mut self, text: &str) -> Outcome {
        if !self.text_input_active || self.text_preedit == text {
            return Outcome::Nothing;
        }
        self.text_preedit = text.to_string();
        Outcome::Redraw
    }

    /// Start typing a new text annotation at `at`.
    fn begin_text(&mut self, at: Point) {
        self.text_input_active = true;
        self.text_input_position = at;
        self.text_input_buffer.clear();
        self.text_caret = 0;
        self.text_preedit.clear();
        self.text_input_font_size = self.text_tool.font_size();
    }

    /// Finish typing: the text becomes an annotation (or replaces the one
    /// being re-edited). Blank text adds nothing, and removes an annotation
    /// that was being re-edited.
    pub fn commit_text(&mut self) {
        if !self.text_input_active {
            return;
        }
        self.text_input_active = false;
        self.text_preedit.clear();
        self.text_caret = 0;
        let text = std::mem::take(&mut self.text_input_buffer);
        let text = text.trim_end().to_string();
        let blank = text.trim().is_empty();
        let annotation = Annotation::Text {
            position: self.text_input_position,
            text,
            color: self.current_color,
            font_size: self.text_input_font_size,
        };
        if let Some((idx, old)) = self.text_edit_origin.take() {
            // Re-edit of an existing annotation: one Modify step (or a
            // Delete when the text was cleared).
            let idx = idx.min(self.annotations.len());
            let action = if blank {
                UndoAction::Delete(idx, old)
            } else {
                self.annotations.insert(idx, annotation);
                UndoAction::Modify(idx, old)
            };
            record_undo(&mut self.undo_stack, &mut self.redo_stack, action);
        } else if !blank {
            self.annotations.push(annotation);
            record_undo(
                &mut self.undo_stack,
                &mut self.redo_stack,
                UndoAction::Add(self.annotations.len() - 1),
            );
        }
    }

    /// Stop typing and drop the text; a re-edited annotation comes back as
    /// it was.
    fn cancel_text(&mut self) {
        if let Some((idx, old)) = self.text_edit_origin.take() {
            let idx = idx.min(self.annotations.len());
            self.annotations.insert(idx, old);
        }
        self.text_input_buffer.clear();
        self.text_preedit.clear();
        self.text_caret = 0;
        self.text_input_active = false;
    }

    /// The text being typed as it should be drawn (with any input-method
    /// composition in place), and the caret's offset in that string.
    pub fn text_for_display(&self) -> (String, usize) {
        let caret = text_edit::snap(&self.text_input_buffer, self.text_caret);
        let mut shown = self.text_input_buffer.clone();
        shown.insert_str(caret, &self.text_preedit);
        (shown, caret + self.text_preedit.len())
    }

    /// Where the caret is on the overlay, as (x, y, height), while typing.
    pub fn text_caret_rect(&self) -> Option<(f32, f32, f32)> {
        if !self.text_input_active {
            return None;
        }
        let size = self.text_input_font_size;
        let (shown, caret) = self.text_for_display();
        let (row, col) = text_edit::line_col(&shown, caret);
        let (start, _) = text_edit::line_range(&shown, caret);
        Some((
            self.text_input_position.x + measure_text_width(&shown[start..start + col], size),
            self.text_input_position.y + row as f32 * text_line_height(size),
            size,
        ))
    }

    /// While typing: is `pos` on the text itself (plus a small margin)?
    fn on_typed_text(&self, pos: Point) -> bool {
        let size = self.text_input_font_size;
        let (w, h) = measure_text_block(&self.text_input_buffer, size);
        let at = self.text_input_position;
        // An empty line still has a caret to click next to.
        let w = w.max(size * 0.5);
        pos.x >= at.x - GRAB && pos.x <= at.x + w + GRAB && pos.y >= at.y && pos.y <= at.y + h
    }

    // ───────────────────────── mouse ─────────────────────────

    /// The pointer moved to `pos` (overlay pixels).
    pub fn on_cursor_moved(&mut self, pos: Point) -> Outcome {
        let prev = self.last_mouse_pos;
        self.last_mouse_pos = pos;
        let dx = pos.x - prev.x;
        let dy = pos.y - prev.y;

        // Window capture: select the window under the pointer.
        if let Some(rects) = &self.window_pick {
            let x_off = self.screenshot.x_offset;
            let y_off = self.screenshot.y_offset;
            let hit = crate::window_detect::window_at_point(
                rects,
                pos.x as i32 + x_off,
                pos.y as i32 + y_off,
            );
            self.selection = hit.map(|(wx, wy, ww, wh)| Selection {
                x: (wx - x_off) as f32,
                y: (wy - y_off) as f32,
                width: ww as f32,
                height: wh as f32,
            });
            return Outcome::Redraw;
        }

        if self.is_selecting {
            if let Some(start) = self.drag_start {
                let end = self.constrained(Some(start), pos, None);
                self.selection = Some(Selection::from_points(start, end));
            }
        } else if let Some(zone) = self.drag_zone {
            if let Some(sel) = &mut self.selection {
                if zone == HitZone::Inside {
                    sel.move_by(dx, dy);
                } else {
                    sel.resize(zone, dx, dy);
                }
            }
        } else {
            match self.active_tool {
                ToolKind::Select => self.drag_selected(pos),
                ToolKind::Eyedropper => self.eyedropper_preview = self.color_at(pos),
                kind => {
                    let end = self.constrained(self.draw_anchor, pos, Some(kind));
                    if let Some(tool) = self.tool_mut(kind) {
                        if tool.is_drawing() {
                            tool.on_mouse_move(end);
                        }
                    }
                }
            }
        }
        // Tooltips, the crosshair and the cursor follow the pointer, so
        // every move is drawn (the app caps the frame rate).
        Outcome::Redraw
    }

    /// Select tool: carry on resizing or moving the selected annotation.
    fn drag_selected(&mut self, pos: Point) {
        let Some(idx) = self.selected_index else {
            return;
        };
        // A resize drag takes priority over a move.
        if let Some(handle) = self.resize_handle {
            if let Some(ann) = self.annotations.get_mut(idx) {
                resize_annotation(ann, handle, pos);
            }
        } else if let Some(from) = self.select_drag_start {
            if let Some(ann) = self.annotations.get_mut(idx) {
                move_annotation(ann, pos.x - from.x, pos.y - from.y);
            }
            self.select_drag_start = Some(pos);
        }
    }

    /// The capture's colour under `pos`, if `pos` is on it.
    fn color_at(&self, pos: Point) -> Option<Color> {
        if pos.x < 0.0 || pos.y < 0.0 {
            return self.eyedropper_preview;
        }
        let (px, py) = (pos.x as u32, pos.y as u32);
        if px >= self.screenshot.width || py >= self.screenshot.height {
            return self.eyedropper_preview;
        }
        let i = (py as usize * self.screenshot.width as usize + px as usize) * 4;
        match self.screenshot.pixels.get(i..i + 3) {
            Some(rgb) => Some(Color::new(
                rgb[0] as f32 / 255.0,
                rgb[1] as f32 / 255.0,
                rgb[2] as f32 / 255.0,
                1.0,
            )),
            None => self.eyedropper_preview,
        }
    }

    /// The left button went down at the last pointer position.
    pub fn on_mouse_down(&mut self) -> Outcome {
        // Window capture: the click confirms the highlighted window.
        if self.window_pick.take().is_some() {
            tracing::info!("Window captured");
            return if self.selection.is_some() && self.finish_on_release {
                Outcome::Copy
            } else {
                Outcome::Redraw
            };
        }

        let pos = self.last_mouse_pos;
        self.nudging = None;

        // Typing: a click on the text moves the caret; a click anywhere
        // else finishes the text first, then does what it would have done.
        if self.text_input_active {
            if self.toolbar_hit(pos).is_none() && self.on_typed_text(pos) {
                let size = self.text_input_font_size;
                self.text_preedit.clear();
                self.text_caret = text_edit::offset_at_point(
                    &self.text_input_buffer,
                    pos.x - self.text_input_position.x,
                    pos.y - self.text_input_position.y,
                    text_line_height(size),
                    &|s| measure_text_width(s, size),
                );
                return Outcome::Redraw;
            }
            self.commit_text();
        }

        // 1. The toolbar (only there once a selection exists).
        if let Some(button) = self.toolbar_hit(pos) {
            let action = BUTTONS.get(button).map(|d| d.action);
            // Upload asks for a second click; anything else withdraws it.
            if action != Some(ButtonAction::Upload) {
                self.upload_confirmed = false;
            }
            return match action {
                Some(ButtonAction::Tool(kind)) => {
                    self.active_tool = kind;
                    if kind != ToolKind::Select {
                        self.selected_index = None;
                    }
                    Outcome::Redraw
                }
                Some(ButtonAction::Color(i)) => match Color::presets().get(i) {
                    Some(color) => {
                        self.pick_color(*color, false);
                        Outcome::Redraw
                    }
                    None => Outcome::Nothing,
                },
                Some(ButtonAction::Ocr) => Outcome::Ocr,
                Some(ButtonAction::Upload) => Outcome::Upload,
                Some(ButtonAction::Pin) => Outcome::Pin,
                Some(ButtonAction::Copy) => Outcome::Copy,
                Some(ButtonAction::Save) => Outcome::Save,
                None => Outcome::Nothing,
            };
        }

        // 2. No selection yet: start one.
        let Some(sel) = self.selection else {
            self.is_selecting = true;
            self.drag_start = Some(pos);
            return Outcome::Redraw;
        };

        // 3. A selection exists.
        match sel.hit_test(pos, GRAB) {
            Some(HitZone::Inside) => {
                self.press_inside(pos);
                Outcome::Redraw
            }
            Some(zone) => {
                self.drag_zone = Some(zone);
                Outcome::Nothing
            }
            None => {
                // Outside it: drop it and start a new one.
                self.selection = None;
                self.is_selecting = true;
                self.drag_start = Some(pos);
                Outcome::Redraw
            }
        }
    }

    /// A press inside the selection: the active tool starts work.
    fn press_inside(&mut self, pos: Point) {
        match self.active_tool {
            ToolKind::Select => self.press_select(pos),
            ToolKind::Text => {
                self.text_tool.on_mouse_down(pos);
                if let Some(at) = self.text_tool.take_pending_position() {
                    self.begin_text(at);
                }
            }
            ToolKind::Eyedropper => {
                if let Some(color) = self.eyedropper_preview {
                    self.set_color_all(color);
                    self.active_tool = ToolKind::Arrow;
                }
            }
            kind => {
                self.draw_anchor = Some(pos);
                if let Some(tool) = self.tool_mut(kind) {
                    tool.on_mouse_down(pos);
                }
            }
        }
    }

    /// Select tool: grab a resize handle, pick an annotation, or re-edit
    /// the text that is already selected.
    fn press_select(&mut self, pos: Point) {
        // The selected annotation's corner handles come first.
        if let Some(idx) = self.selected_index {
            if let Some(handle) = self.handle_at(idx, pos) {
                self.resize_handle = Some(handle);
                self.pre_drag_annotation = Some((idx, self.annotations[idx].clone()));
                return;
            }
        }

        // Otherwise the top-most annotation under the pointer.
        let found = self
            .annotations
            .iter()
            .rposition(|a| hit_test_annotation(a, &pos, GRAB));
        let Some(idx) = found else {
            self.selected_index = None;
            return;
        };

        // A click on a text annotation that is already selected edits it.
        if self.selected_index == Some(idx) {
            if let Annotation::Text {
                position,
                text,
                color,
                font_size,
            } = self.annotations[idx].clone()
            {
                // Take the annotation out and remember it: finishing records
                // ONE Modify undo step; Escape puts it back.
                let removed = self.annotations.remove(idx);
                self.text_edit_origin = Some((idx, removed));
                self.selected_index = None;
                self.text_input_active = true;
                self.text_input_position = position;
                self.text_caret = text.len();
                self.text_input_buffer = text;
                self.text_preedit.clear();
                self.text_input_font_size = font_size;
                self.current_color = color;
                return;
            }
        }

        self.selected_index = Some(idx);
        self.select_drag_start = Some(pos);
        // Snapshot before the move, for undo.
        self.pre_drag_annotation = Some((idx, self.annotations[idx].clone()));
    }

    /// The left button came up at the last pointer position.
    pub fn on_mouse_up(&mut self) -> Outcome {
        let pos = self.last_mouse_pos;

        if self.is_selecting {
            self.is_selecting = false;
            let drawn = self
                .drag_start
                .take()
                .map(|start| {
                    let end = self.constrained(Some(start), pos, None);
                    Selection::from_points(start, end)
                })
                .filter(|sel| sel.width > 2.0 && sel.height > 2.0);
            // A click, or a drag of a pixel or two, selects nothing.
            self.selection = drawn;
            return if drawn.is_some() && self.finish_on_release {
                // "Copy as soon as a region is drawn": skip markup.
                Outcome::Copy
            } else {
                Outcome::Redraw
            };
        }

        if self.drag_zone.take().is_some() {
            return Outcome::Nothing;
        }

        let annotation = match self.active_tool {
            ToolKind::Select => {
                // One undo step for a move or resize, if anything changed.
                if let Some((idx, before)) = self.pre_drag_annotation.take() {
                    if self.annotations.get(idx).is_some_and(|now| *now != before) {
                        record_undo(
                            &mut self.undo_stack,
                            &mut self.redo_stack,
                            UndoAction::Modify(idx, before),
                        );
                    }
                }
                self.select_drag_start = None;
                self.resize_handle = None;
                None
            }
            kind => {
                let anchor = self.draw_anchor.take();
                let end = self.constrained(anchor, pos, Some(kind));
                self.tool_mut(kind).and_then(|tool| tool.on_mouse_up(end))
            }
        };
        match annotation {
            Some(annotation) => {
                self.annotations.push(annotation);
                record_undo(
                    &mut self.undo_stack,
                    &mut self.redo_stack,
                    UndoAction::Add(self.annotations.len() - 1),
                );
                Outcome::Redraw
            }
            None => Outcome::Nothing,
        }
    }

    /// The right button went down: a colour swatch opens the picker;
    /// anywhere else cancels the capture, unless a region is selected.
    pub fn on_right_click(&mut self) -> Outcome {
        if let Some(button) = self.toolbar_hit(self.last_mouse_pos) {
            if let Some(ButtonAction::Color(i)) = BUTTONS.get(button).map(|d| d.action) {
                return Outcome::PickColor(i);
            }
        }
        if self.selection.is_some() {
            Outcome::Nothing
        } else {
            Outcome::Close
        }
    }

    /// The wheel turned by `lines` (up is positive): the size of whatever
    /// the active tool draws.
    pub fn on_scroll(&mut self, lines: f32) -> Outcome {
        if self.text_input_active {
            self.text_input_font_size = (self.text_input_font_size + lines).clamp(10.0, 72.0);
        } else if self.active_tool == ToolKind::StepMarker {
            let size = self.step_marker_tool.size() + lines * 2.0;
            self.step_marker_tool.set_size(size);
        } else if self.active_tool == ToolKind::RoundedRect {
            let radius = self.rounded_rect_tool.radius() + lines * 2.0;
            self.rounded_rect_tool.set_radius(radius);
        } else {
            self.set_thickness_all(self.current_thickness + lines);
        }
        Outcome::Redraw
    }

    /// The colour picker returned `color`: it becomes the drawing colour,
    /// and the colour of the selected annotation if there is one.
    pub fn apply_picked_color(&mut self, color: Color) {
        self.pick_color(color, true);
    }

    /// A colour was chosen. With an annotation selected it is recoloured
    /// (one undo step); a swatch click then leaves the drawing colour alone,
    /// while the picker (`also_set`) changes both.
    fn pick_color(&mut self, color: Color, also_set: bool) {
        let selected = self.selected_index.filter(|i| *i < self.annotations.len());
        if also_set || self.selected_index.is_none() {
            self.set_color_all(color);
        }
        if let Some(idx) = selected {
            let before = self.annotations[idx].clone();
            recolor_annotation(&mut self.annotations[idx], color);
            record_undo(
                &mut self.undo_stack,
                &mut self.redo_stack,
                UndoAction::Modify(idx, before),
            );
        }
    }

    // ───────────────────────── geometry ─────────────────────────

    /// The markup toolbar, once there is a selection to hang it on.
    pub fn toolbar(&self) -> Option<Toolbar> {
        self.selection.as_ref().map(|sel| {
            Toolbar::position_for_dynamic(
                sel,
                self.screenshot.height as f32,
                self.visible_buttons.len(),
            )
        })
    }

    /// The toolbar button under `pos`, as an index into `BUTTONS`.
    pub fn toolbar_hit(&self, pos: Point) -> Option<usize> {
        let visible = self
            .toolbar()?
            .hit_test_dynamic(pos, self.visible_buttons.len())?;
        self.visible_buttons.get(visible).copied()
    }

    /// The corner handle of annotation `idx` under `pos`.
    fn handle_at(&self, idx: usize, pos: Point) -> Option<ResizeHandle> {
        let (x, y, w, h) = annotation_bounding_box(self.annotations.get(idx)?)?;
        [
            (x, y, ResizeHandle::TopLeft),
            (x + w, y, ResizeHandle::TopRight),
            (x, y + h, ResizeHandle::BottomLeft),
            (x + w, y + h, ResizeHandle::BottomRight),
        ]
        .into_iter()
        .find(|(hx, hy, _)| (pos.x - hx).abs() < GRAB && (pos.y - hy).abs() < GRAB)
        .map(|(_, _, handle)| handle)
    }

    /// The mouse cursor for the pointer's current position.
    pub fn cursor_icon(&self) -> CursorIcon {
        let pos = self.last_mouse_pos;
        if self.window_pick.is_some() {
            return CursorIcon::Crosshair;
        }
        if self.text_input_active {
            return CursorIcon::Text;
        }
        if self.toolbar_hit(pos).is_some() {
            return CursorIcon::Pointer;
        }
        // Dragging, or hovering over, a corner of the selected annotation.
        let handle = self.resize_handle.or_else(|| {
            (self.active_tool == ToolKind::Select)
                .then_some(self.selected_index)
                .flatten()
                .and_then(|idx| self.handle_at(idx, pos))
        });
        if let Some(handle) = handle {
            return match handle {
                ResizeHandle::TopLeft | ResizeHandle::BottomRight => CursorIcon::NwseResize,
                ResizeHandle::TopRight | ResizeHandle::BottomLeft => CursorIcon::NeswResize,
            };
        }
        if self.is_selecting {
            return CursorIcon::Crosshair;
        }
        let zone = self.drag_zone.or_else(|| {
            self.selection
                .as_ref()
                .and_then(|sel| sel.hit_test(pos, GRAB))
        });
        match zone {
            Some(HitZone::TopLeft | HitZone::BottomRight) => CursorIcon::NwseResize,
            Some(HitZone::TopRight | HitZone::BottomLeft) => CursorIcon::NeswResize,
            Some(HitZone::Top | HitZone::Bottom) => CursorIcon::NsResize,
            Some(HitZone::Left | HitZone::Right) => CursorIcon::EwResize,
            Some(HitZone::Inside) => match self.active_tool {
                ToolKind::Select => CursorIcon::Default,
                ToolKind::Text => CursorIcon::Text,
                ToolKind::StepMarker => CursorIcon::Cell,
                _ => CursorIcon::Crosshair,
            },
            // No selection yet, or the pointer is outside it.
            None => CursorIcon::Crosshair,
        }
    }

    /// Whether the magnifier is up: while a region is being placed, dragged
    /// out or resized, which is when single pixels matter.
    pub fn magnifier_visible(&self) -> bool {
        self.magnifier
            && self.window_pick.is_none()
            && !self.text_input_active
            && (self.selection.is_none() || self.is_selecting || self.drag_zone.is_some())
    }
}

/// How far an arrow key moves things: a pixel, or ten with Shift.
fn arrow_step(input: &KeyInput) -> Option<(f32, f32)> {
    let step = if input.mods.shift() { 10.0 } else { 1.0 };
    match input.logical? {
        Named::Left => Some((-step, 0.0)),
        Named::Right => Some((step, 0.0)),
        Named::Up => Some((0.0, -step)),
        Named::Down => Some((0.0, step)),
        _ => None,
    }
}
