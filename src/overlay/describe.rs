//! What the capture overlay tells assistive technology about itself (see
//! `access`): the markup toolbar's buttons, which tool and colour are on,
//! and a status line that says where things stand and which keys do what.
//!
//! The status only changes when something settles (a region is placed, a
//! tool picked), never while the mouse is dragging, so a screen reader
//! isn't made to read out every size the selection passes through.

use crate::access::{Control, Request, Role, Snapshot};
use crate::geometry::Color;
use crate::overlay::input::Outcome;
use crate::overlay::toolbar::{ButtonAction, BUTTONS};
use crate::state::OverlayState;
use crate::tools::{ToolKind, TOOL_ORDER};

impl OverlayState {
    /// The overlay as a screen reader should present it.
    pub fn snapshot(&self) -> Snapshot {
        let toolbar = self.toolbar();
        let presets = Color::presets();
        let controls = match toolbar {
            None => Vec::new(),
            Some(bar) => self
                .visible_buttons
                .iter()
                .enumerate()
                .filter_map(|(slot, &i)| {
                    let def = BUTTONS.get(i)?;
                    let plain = def
                        .tooltip
                        .split(" (right-click")
                        .next()
                        .unwrap_or(def.tooltip);
                    let (role, label, checked, value) = match def.action {
                        ButtonAction::Tool(kind) => (
                            Role::Radio,
                            format!("{plain} tool"),
                            Some(self.active_tool == kind),
                            None,
                        ),
                        ButtonAction::Color(c) => (
                            Role::Radio,
                            format!("Colour {plain}"),
                            Some(presets.get(c) == Some(&self.current_color)),
                            None,
                        ),
                        ButtonAction::Style => (
                            Role::Button,
                            "Shape style (D): outline, dashed or filled".to_string(),
                            None,
                            Some(self.shape_style.label().to_string()),
                        ),
                        _ => (Role::Button, plain.to_string(), None, None),
                    };
                    Some(Control {
                        role,
                        label,
                        checked,
                        value,
                        rect: Some(bar.button_rect(slot)),
                    })
                })
                .collect(),
        };
        // Focus sits on the tool that is on, so switching tools is read out.
        let focus = toolbar.and_then(|_| {
            self.visible_buttons.iter().position(|&i| {
                BUTTONS.get(i).map(|d| d.action) == Some(ButtonAction::Tool(self.active_tool))
            })
        });
        Snapshot {
            title: match self.canvas {
                Some(_) => "screen-mark: marking up an image".to_string(),
                None => "screen-mark capture".to_string(),
            },
            controls,
            focus,
            status: Some(self.status()),
        }
    }

    /// Where things stand, and the keys that move them on.
    fn status(&self) -> String {
        let mut s = if self.window_pick.is_some() {
            "Point at a window and click to capture it. Escape cancels.".to_string()
        } else if self.is_selecting || self.drag_zone.is_some() {
            "Selecting.".to_string()
        } else {
            match self.selection {
                None => "Drag to select a region, or press Ctrl+A for the whole screen. \
                         The arrow keys move the pointer a pixel at a time. Escape cancels."
                    .to_string(),
                Some(sel) => {
                    let tool = TOOL_ORDER
                        .iter()
                        .position(|k| *k == self.active_tool)
                        .and_then(|i| crate::keys::TOOL_LABELS.get(i))
                        .copied()
                        .unwrap_or("Select");
                    let n = self.annotations.len();
                    let marks = match n {
                        0 => "no annotations".to_string(),
                        1 => "1 annotation".to_string(),
                        n => format!("{n} annotations"),
                    };
                    let mut line = format!(
                        "Selected {} by {} pixels. Tool: {tool}",
                        sel.width.round() as i64,
                        sel.height.round() as i64,
                    );
                    if matches!(
                        self.active_tool,
                        ToolKind::Rectangle
                            | ToolKind::Circle
                            | ToolKind::RoundedRect
                            | ToolKind::Line
                            | ToolKind::Arrow
                    ) {
                        line.push_str(&format!(", {}", self.shape_style.label().to_lowercase()));
                    }
                    line.push_str(&format!(
                        ", {marks}. Enter copies, Ctrl+S saves, Escape cancels."
                    ));
                    line
                }
            }
        };
        if let Some(msg) = self.toast_message.as_deref().filter(|_| {
            self.toast_until
                .is_some_and(|t| t > std::time::Instant::now())
        }) {
            s = format!("{msg}. {s}");
        }
        s
    }

    /// Carry out a screen reader's request on control `index` of
    /// `snapshot`: pressing a button does what clicking it does.
    pub fn request(&mut self, request: Request) -> Outcome {
        match request {
            Request::Press(index) => match self.visible_buttons.get(index) {
                Some(&button) if self.toolbar().is_some() => self.press_button(button),
                _ => Outcome::Nothing,
            },
            Request::Focus(_) => Outcome::Nothing,
        }
    }
}
