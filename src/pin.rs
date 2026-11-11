//! Pinned always-on-top capture windows.
//!
//! A pin shows a captured selection in a borderless floating window framed by
//! a themed border. Drag to move, middle-click to copy, right-click to
//! reveal the backing temp file, Escape or close to dismiss.
//!
//! The border and shadow are only how the pin looks on screen. What a
//! middle-click copies, and the temp file a right-click reveals, is the
//! capture itself.

use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;

use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::window::{CursorIcon, Window, WindowAttributes, WindowId, WindowLevel};

/// Border thickness for pinned windows (themed frame)
const PIN_BORDER: u32 = 3;
/// Shadow offset for pinned windows
const PIN_SHADOW: u32 = 2;

/// What the app should do after a pin handled an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinEvent {
    Nothing,
    /// Escape or the window's close button.
    Close,
    /// Middle-click: copy `PinnedWindow::image` and report it like any
    /// other copy, so the `notifications` setting applies.
    Copy,
}

/// `image` (RGBA8, `w` x `h`) with the pin's border and shadow drawn around
/// it. Returns the framed pixels and their size.
pub fn frame(image: &[u8], w: u32, h: u32, accent: (u8, u8, u8)) -> (Vec<u8>, u32, u32) {
    let border = PIN_BORDER;
    let shadow = PIN_SHADOW;
    let total_w = w + border * 2 + shadow;
    let total_h = h + border * 2 + shadow;
    let mut out = vec![0u8; total_w as usize * total_h as usize * 4];
    let stride = total_w as usize * 4;

    // Shadow (dark in both themes, offset bottom-right), then the border.
    for y in shadow..total_h {
        for x in shadow..total_w {
            let i = y as usize * stride + x as usize * 4;
            out[i + 3] = 100;
        }
    }
    let (r, g, b) = accent;
    for y in 0..total_h - shadow {
        for x in 0..total_w - shadow {
            let i = y as usize * stride + x as usize * 4;
            out[i..i + 4].copy_from_slice(&[r, g, b, 255]);
        }
    }
    // The capture inside the border.
    let row = w as usize * 4;
    for y in 0..h as usize {
        let src = y * row;
        let dst = (y + border as usize) * stride + border as usize * 4;
        if let Some(line) = image.get(src..src + row) {
            out[dst..dst + row].copy_from_slice(line);
        }
    }
    (out, total_w, total_h)
}

pub struct PinnedWindow {
    window: Arc<Window>,
    surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
    /// What the window shows: the capture inside its border and shadow.
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    /// The capture itself (RGBA8), as copied and as written to `temp_path`.
    image: Vec<u8>,
    image_w: u32,
    image_h: u32,
    dragging: bool,
    drag_start: Option<winit::dpi::PhysicalPosition<f64>>,
    temp_path: Option<PathBuf>,
}

impl Drop for PinnedWindow {
    fn drop(&mut self) {
        // Clean the drag-and-drop temp file up on close AND on app exit.
        if let Some(ref path) = self.temp_path {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl PinnedWindow {
    /// Create a pin for `pixels` (RGBA, `img_w` x `img_h`), positioned so the
    /// image content sits at screen position (`screen_x`, `screen_y`).
    pub fn create(
        event_loop: &ActiveEventLoop,
        pixels: &[u8],
        img_w: u32,
        img_h: u32,
        screen_x: i32,
        screen_y: i32,
    ) -> Option<Self> {
        let expected = img_w as usize * img_h as usize * 4;
        if img_w == 0 || img_h == 0 || pixels.len() < expected {
            tracing::error!("Nothing to pin ({img_w}x{img_h}, {} bytes)", pixels.len());
            return None;
        }
        let image = pixels[..expected].to_vec();
        let (framed, total_w, total_h) = frame(&image, img_w, img_h, crate::theme::accent());

        let pin_x = screen_x - PIN_BORDER as i32;
        let pin_y = screen_y - PIN_BORDER as i32;

        let attrs = WindowAttributes::default()
            .with_title("screen-mark Pin")
            .with_inner_size(winit::dpi::PhysicalSize::new(total_w, total_h))
            .with_position(winit::dpi::PhysicalPosition::new(pin_x, pin_y))
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_decorations(false);

        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                tracing::error!("Failed to create pin window: {e}");
                return None;
            }
        };

        let context = match softbuffer::Context::new(Arc::clone(&window)) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("Failed to create pin softbuffer context: {e}");
                return None;
            }
        };

        let surface = match softbuffer::Surface::new(&context, Arc::clone(&window)) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("Failed to create pin softbuffer surface: {e}");
                return None;
            }
        };

        // Save the capture to a temp file for drag-and-drop / reveal support
        let temp_path = {
            let temp_dir = std::env::temp_dir();
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let path = temp_dir.join(format!("screen-mark_pin_{ts}.png"));
            image::RgbaImage::from_raw(img_w, img_h, image.clone()).and_then(|img| {
                match img.save(&path) {
                    Ok(()) => {
                        tracing::info!("Pin temp file saved to {}", path.display());
                        Some(path)
                    }
                    Err(e) => {
                        tracing::warn!("Failed to save pin temp file: {e}");
                        None
                    }
                }
            })
        };

        let mut pin = Self {
            window,
            surface,
            pixels: framed,
            width: total_w,
            height: total_h,
            image,
            image_w: img_w,
            image_h: img_h,
            dragging: false,
            drag_start: None,
            temp_path,
        };
        pin.render();
        pin.window.set_cursor(CursorIcon::Grab);
        tracing::info!("Pinned {}x{} capture to screen", total_w, total_h);
        Some(pin)
    }

    pub fn window_id(&self) -> WindowId {
        self.window.id()
    }

    /// The pinned capture (RGBA8) and its size, without the border and
    /// shadow the window draws around it.
    pub fn image(&self) -> (Vec<u8>, u32, u32) {
        (self.image.clone(), self.image_w, self.image_h)
    }

    pub fn render(&mut self) {
        let (w, h) = (self.width, self.height);
        if let (Some(nz_w), Some(nz_h)) = (NonZeroU32::new(w), NonZeroU32::new(h)) {
            if self.surface.resize(nz_w, nz_h).is_err() {
                return;
            }
            if let Ok(mut buffer) = self.surface.buffer_mut() {
                let pixel_count = (w * h) as usize;
                for (i, chunk) in self
                    .pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .take(pixel_count)
                    .enumerate()
                {
                    buffer[i] =
                        ((chunk[0] as u32) << 16) | ((chunk[1] as u32) << 8) | (chunk[2] as u32);
                }
                let _ = buffer.present();
            }
        }
    }

    /// Handle an event for this pin's window.
    pub fn handle_event(&mut self, event: &WindowEvent) -> PinEvent {
        match event {
            WindowEvent::CloseRequested => return PinEvent::Close,

            WindowEvent::RedrawRequested => self.render(),

            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                if let Key::Named(NamedKey::Escape) = &event.logical_key {
                    return PinEvent::Close;
                }
            }

            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => match state {
                ElementState::Pressed => {
                    self.dragging = true;
                    self.drag_start = None;
                    self.window.set_cursor(CursorIcon::Grabbing);
                }
                ElementState::Released => {
                    self.dragging = false;
                    self.drag_start = None;
                    self.window.set_cursor(CursorIcon::Grab);
                }
            },

            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Right,
                ..
            } => {
                if let Some(ref path) = self.temp_path {
                    reveal_in_file_manager(path);
                }
            }

            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Middle,
                ..
            } => return PinEvent::Copy,

            WindowEvent::CursorMoved { position, .. } if self.dragging => {
                if let Some(start) = self.drag_start {
                    let dx = position.x - start.x;
                    let dy = position.y - start.y;
                    if let Ok(current_pos) = self.window.outer_position() {
                        let new_x = current_pos.x + dx as i32;
                        let new_y = current_pos.y + dy as i32;
                        self.window
                            .set_outer_position(winit::dpi::PhysicalPosition::new(new_x, new_y));
                    }
                    // Don't update drag_start — cursor position is window-relative,
                    // so it stays constant while the window tracks the cursor.
                } else {
                    self.drag_start = Some(*position);
                }
            }

            _ => {}
        }
        PinEvent::Nothing
    }
}

#[cfg(target_os = "windows")]
fn reveal_in_file_manager(path: &std::path::Path) {
    let _ = std::process::Command::new("explorer")
        .arg("/select,")
        .arg(path)
        .spawn();
}

#[cfg(target_os = "linux")]
fn reveal_in_file_manager(path: &std::path::Path) {
    let _ = std::process::Command::new("xdg-open")
        .arg(path.parent().unwrap_or(path))
        .spawn();
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn reveal_in_file_manager(_path: &std::path::Path) {}
