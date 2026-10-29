//! screen-mark's own notification popup: a small always-on-top card in the
//! bottom-right of the monitor you're working on, with a thumbnail of the
//! capture, what happened, and the file name. Click it to show the file in
//! its folder. It never takes keyboard focus.
//!
//! It works regardless of Windows' notification settings and Focus Assist,
//! appears instantly, and leaves nothing in the notification centre. The
//! drawing is a pure function (`render`) so tests can snapshot it.

use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tiny_skia::{Color as SkColor, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes, WindowId, WindowLevel};

use crate::display::Monitor;
use crate::geometry::{Color, Point};

/// Logical size of the card.
pub const W: f32 = 380.0;
pub const H: f32 = 84.0;
const MARGIN: f32 = 16.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Success,
    Error,
    Info,
}

/// What a click on the card does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Click {
    Nothing,
    /// Show this file selected in its folder.
    ShowFile(PathBuf),
    /// Open the Settings window on the Keys tab.
    OpenKeySettings,
}

/// A small RGBA8 image.
#[derive(Debug, Clone)]
pub struct Thumb {
    pub pixels: Vec<u8>,
    pub w: u32,
    pub h: u32,
}

/// The end of a hint that only makes sense in the popup (it can be clicked;
/// a system notification can't).
pub const CLICK_HINT: &str = " \u{00b7} click to show in folder";

/// Everything a popup shows.
#[derive(Debug, Clone)]
pub struct Spec {
    pub kind: Kind,
    pub title: String,
    pub detail: String,
    pub hint: String,
    pub thumb: Option<Thumb>,
    pub click: Click,
    pub duration: Duration,
}

impl Spec {
    pub fn new(kind: Kind, title: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            kind,
            title: title.into(),
            detail: detail.into(),
            hint: String::new(),
            thumb: None,
            click: Click::Nothing,
            duration: match kind {
                Kind::Error => Duration::from_secs(7),
                _ => Duration::from_millis(3500),
            },
        }
    }
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = hint.into();
        self
    }
    pub fn thumb(mut self, thumb: Option<Thumb>) -> Self {
        self.thumb = thumb;
        self
    }
    pub fn click(mut self, click: Click) -> Self {
        self.click = click;
        self
    }
}

/// Downscale `pixels` (RGBA8, `w`x`h`) to fit `max_w`x`max_h`, averaging
/// each source block so text-heavy screenshots stay readable.
pub fn make_thumb(pixels: &[u8], w: u32, h: u32, max_w: u32, max_h: u32) -> Option<Thumb> {
    if w == 0 || h == 0 || pixels.len() < (w as usize * h as usize * 4) {
        return None;
    }
    let scale = (max_w as f64 / w as f64)
        .min(max_h as f64 / h as f64)
        .min(1.0);
    let tw = ((w as f64 * scale).round() as u32).max(1);
    let th = ((h as f64 * scale).round() as u32).max(1);
    let mut out = vec![0u8; (tw * th * 4) as usize];
    for ty in 0..th {
        let y0 = (ty as u64 * h as u64 / th as u64) as u32;
        let y1 = (((ty + 1) as u64 * h as u64 / th as u64) as u32)
            .max(y0 + 1)
            .min(h);
        for tx in 0..tw {
            let x0 = (tx as u64 * w as u64 / tw as u64) as u32;
            let x1 = (((tx + 1) as u64 * w as u64 / tw as u64) as u32)
                .max(x0 + 1)
                .min(w);
            // Sample at most a 4x4 grid per block: fast on 4K captures.
            let sx = ((x1 - x0) / 4).max(1);
            let sy = ((y1 - y0) / 4).max(1);
            let mut acc = [0u32; 4];
            let mut n = 0u32;
            let mut y = y0;
            while y < y1 {
                let mut x = x0;
                while x < x1 {
                    let i = ((y * w + x) * 4) as usize;
                    for c in 0..4 {
                        acc[c] += pixels[i + c] as u32;
                    }
                    n += 1;
                    x += sx;
                }
                y += sy;
            }
            let o = ((ty * tw + tx) * 4) as usize;
            for c in 0..4 {
                out[o + c] = (acc[c] / n.max(1)) as u8;
            }
        }
    }
    Some(Thumb {
        pixels: out,
        w: tw,
        h: th,
    })
}

fn sk(rgb: (u8, u8, u8), a: f32) -> SkColor {
    crate::theme::skia(rgb, a)
}

fn fill(p: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, c: SkColor) {
    if let Some(r) = Rect::from_xywh(x, y, w, h) {
        let mut paint = Paint::default();
        paint.set_color(c);
        p.fill_rect(r, &paint, Transform::identity(), None);
    }
}

fn text(p: &mut Pixmap, x: f32, y: f32, s: &str, size: f32, rgb: (u8, u8, u8)) {
    let c = Color::new(
        rgb.0 as f32 / 255.0,
        rgb.1 as f32 / 255.0,
        rgb.2 as f32 / 255.0,
        1.0,
    );
    crate::tools::render_text_annotation(p, &Point::new(x, y), s, &c, size);
}

/// Shorten `s` with an ellipsis in the middle so it fits `max_w` pixels.
/// Keeps the end, because the end of a file name is the part that differs.
pub fn fit_middle(s: &str, size: f32, max_w: f32) -> String {
    if crate::tools::measure_text_width(s, size) <= max_w {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut keep = chars.len();
    while keep > 2 {
        keep -= 1;
        let head = keep / 3;
        let tail = keep - head;
        let candidate: String = chars[..head]
            .iter()
            .chain(std::iter::once(&'\u{2026}'))
            .chain(chars[chars.len() - tail..].iter())
            .collect();
        if crate::tools::measure_text_width(&candidate, size) <= max_w {
            return candidate;
        }
    }
    "\u{2026}".into()
}

/// Shorten `s` with an ellipsis at the end so it fits `max_w` pixels.
pub fn fit_end(s: &str, size: f32, max_w: f32) -> String {
    if crate::tools::measure_text_width(s, size) <= max_w {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut keep = chars.len();
    while keep > 0 {
        keep -= 1;
        let candidate: String = chars[..keep]
            .iter()
            .chain(std::iter::once(&'\u{2026}'))
            .collect();
        if crate::tools::measure_text_width(&candidate, size) <= max_w {
            return candidate;
        }
    }
    "\u{2026}".into()
}

/// Draw the card at `scale` (physical pixels per logical pixel).
pub fn render(spec: &Spec, scale: f32) -> Pixmap {
    let s = scale.max(0.5);
    let pw = (W * s).round() as u32;
    let ph = (H * s).round() as u32;
    let mut p = Pixmap::new(pw, ph).expect("toast size is non-zero");

    // Card and hairline border.
    fill(
        &mut p,
        0.0,
        0.0,
        pw as f32,
        ph as f32,
        sk(crate::theme::border(), 1.0),
    );
    fill(
        &mut p,
        1.0,
        1.0,
        pw as f32 - 2.0,
        ph as f32 - 2.0,
        sk(crate::theme::bg_1(), 1.0),
    );

    // Status bar on the left edge.
    let bar = match spec.kind {
        Kind::Success => crate::theme::accent(),
        Kind::Error => crate::theme::red(),
        Kind::Info => crate::theme::text_muted(),
    };
    fill(&mut p, 1.0, 1.0, 3.0 * s, ph as f32 - 2.0, sk(bar, 1.0));

    let pad = 14.0 * s;
    let mut tx = pad + 3.0 * s;
    if let Some(th) = &spec.thumb {
        let box_w = 96.0 * s;
        let box_h = 60.0 * s;
        let bx = tx;
        let by = (ph as f32 - box_h) / 2.0;
        fill(&mut p, bx, by, box_w, box_h, sk(crate::theme::bg_0(), 1.0));
        draw_thumb(&mut p, th, bx, by, box_w, box_h);
        // 1px frame
        let mut pb = PathBuilder::new();
        pb.push_rect(Rect::from_xywh(bx + 0.5, by + 0.5, box_w - 1.0, box_h - 1.0).unwrap());
        if let Some(path) = pb.finish() {
            let mut paint = Paint::default();
            paint.set_color(sk(crate::theme::border(), 1.0));
            p.stroke_path(
                &path,
                &paint,
                &Stroke::default(),
                Transform::identity(),
                None,
            );
        }
        tx = bx + box_w + 14.0 * s;
    } else if spec.kind == Kind::Error {
        // A plain "!" mark keeps error cards from looking empty.
        text(
            &mut p,
            tx + 2.0 * s,
            20.0 * s,
            "!",
            30.0 * s,
            crate::theme::red(),
        );
        tx += 26.0 * s;
    }

    let max_w = pw as f32 - tx - pad;
    let title = fit_end(&spec.title, 14.0 * s, max_w);
    text(
        &mut p,
        tx,
        14.0 * s,
        &title,
        14.0 * s,
        crate::theme::text_normal(),
    );
    // A file name differs at its end, so shorten it in the middle; an
    // error message says what went wrong first, so keep its start.
    let detail = if spec.kind == Kind::Success {
        fit_middle(&spec.detail, 12.0 * s, max_w)
    } else {
        fit_end(&spec.detail, 12.0 * s, max_w)
    };
    text(
        &mut p,
        tx,
        37.0 * s,
        &detail,
        12.0 * s,
        crate::theme::text_soft(),
    );
    if !spec.hint.is_empty() {
        let hint = fit_middle(&spec.hint, 11.0 * s, max_w);
        text(
            &mut p,
            tx,
            58.0 * s,
            &hint,
            11.0 * s,
            crate::theme::text_muted(),
        );
    }
    p
}

fn draw_thumb(p: &mut Pixmap, th: &Thumb, bx: f32, by: f32, bw: f32, bh: f32) {
    // Fit inside the box, centred, nearest-neighbour from the small thumb.
    let scale = (bw / th.w as f32).min(bh / th.h as f32);
    let dw = (th.w as f32 * scale).floor().max(1.0);
    let dh = (th.h as f32 * scale).floor().max(1.0);
    let ox = (bx + (bw - dw) / 2.0).round() as i64;
    let oy = (by + (bh - dh) / 2.0).round() as i64;
    let (pw, ph) = (p.width() as i64, p.height() as i64);
    let data = p.data_mut();
    for y in 0..dh as i64 {
        let sy = ((y as f32 / scale) as u32).min(th.h - 1);
        for x in 0..dw as i64 {
            let sx = ((x as f32 / scale) as u32).min(th.w - 1);
            let (px, py) = (ox + x, oy + y);
            if px < 0 || py < 0 || px >= pw || py >= ph {
                continue;
            }
            let si = ((sy * th.w + sx) * 4) as usize;
            let di = ((py * pw + px) * 4) as usize;
            // Screenshots are opaque; write straight into the premultiplied buffer.
            data[di] = th.pixels[si];
            data[di + 1] = th.pixels[si + 1];
            data[di + 2] = th.pixels[si + 2];
            data[di + 3] = 255;
        }
    }
}

/// Where the card goes: bottom-right of the monitor's work area.
pub fn position(monitor: Option<&Monitor>, scale: f32) -> (i32, i32) {
    let (w, h) = ((W * scale).round() as i32, (H * scale).round() as i32);
    let m = (MARGIN * scale).round() as i32;
    match monitor {
        Some(mon) => (
            mon.work.x + mon.work.w as i32 - w - m,
            mon.work.y + mon.work.h as i32 - h - m,
        ),
        None => (m, m),
    }
}

/// A live popup window.
pub struct Toast {
    window: Arc<Window>,
    surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
    pub spec: Spec,
    until: Instant,
    scale: f32,
    /// Where it belongs, in physical pixels: (x, y, width, height).
    place: (i32, i32, u32, u32),
}

impl Toast {
    /// Create and show the card on `monitor` (the one the user is on).
    pub fn show(
        event_loop: &ActiveEventLoop,
        spec: Spec,
        monitor: Option<Monitor>,
    ) -> Option<Self> {
        // The monitor's DPI scale: winit knows it once we look it up by position.
        let scale = monitor
            .and_then(|m| {
                event_loop.available_monitors().find(|wm| {
                    let p = wm.position();
                    p.x == m.area.x && p.y == m.area.y
                })
            })
            .map(|wm| wm.scale_factor() as f32)
            .unwrap_or(1.0);
        let (x, y) = position(monitor.as_ref(), scale);
        let pw = (W * scale).round() as u32;
        let ph = (H * scale).round() as u32;

        let mut attrs = WindowAttributes::default()
            .with_title("screen-mark notification")
            .with_inner_size(winit::dpi::PhysicalSize::new(pw, ph))
            .with_position(winit::dpi::PhysicalPosition::new(x, y))
            .with_decorations(false)
            .with_resizable(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_active(false)
            .with_visible(false);
        #[cfg(target_os = "windows")]
        {
            use winit::platform::windows::WindowAttributesExtWindows;
            attrs = attrs.with_skip_taskbar(true);
        }
        #[cfg(target_os = "linux")]
        {
            use winit::platform::x11::{WindowAttributesExtX11, WindowType};
            // Override-redirect: the window manager neither decorates,
            // moves nor focuses it, exactly like a desktop notification.
            attrs = attrs
                .with_override_redirect(true)
                .with_x11_window_type(vec![WindowType::Notification]);
        }

        let window = Arc::new(event_loop.create_window(attrs).ok()?);
        #[cfg(target_os = "windows")]
        {
            no_activate(&window);
            // winit creates the window on the primary monitor, sizes it, then
            // moves it; crossing to a monitor with another DPI rescales it.
            // Put the intended size and place back (see also `keep_place`).
            let _ = window.request_inner_size(winit::dpi::PhysicalSize::new(pw, ph));
            window.set_outer_position(winit::dpi::PhysicalPosition::new(x, y));
        }
        let context = softbuffer::Context::new(Arc::clone(&window)).ok()?;
        let surface = softbuffer::Surface::new(&context, Arc::clone(&window)).ok()?;
        let until = Instant::now() + spec.duration;
        let mut t = Self {
            window,
            surface,
            spec,
            until,
            scale,
            place: (x, y, pw, ph),
        };
        t.render();
        t.window.set_visible(true);
        Some(t)
    }

    pub fn window_id(&self) -> WindowId {
        self.window.id()
    }

    pub fn until(&self) -> Instant {
        self.until
    }

    /// The window moved to a monitor with another DPI (Windows, while it's
    /// being placed): keep the size and position computed for its monitor
    /// instead of winit's rescaled ones.
    pub fn keep_place(&self, writer: &mut winit::event::InnerSizeWriter) {
        let (x, y, w, h) = self.place;
        let _ = writer.request_inner_size(winit::dpi::PhysicalSize::new(w, h));
        self.window
            .set_outer_position(winit::dpi::PhysicalPosition::new(x, y));
    }

    /// Keep it up a bit longer while the pointer is over it.
    pub fn hold(&mut self) {
        let min = Instant::now() + Duration::from_millis(1500);
        if self.until < min {
            self.until = min;
        }
    }

    pub fn render(&mut self) {
        let pix = render(&self.spec, self.scale);
        let size = self.window.inner_size();
        let (w, h) = (size.width.max(1), size.height.max(1));
        let (Some(nw), Some(nh)) = (NonZeroU32::new(w), NonZeroU32::new(h)) else {
            return;
        };
        if self.surface.resize(nw, nh).is_err() {
            return;
        }
        if let Ok(mut buf) = self.surface.buffer_mut() {
            let src = pix.data();
            let (sw, sh) = (pix.width() as usize, pix.height() as usize);
            for y in 0..h as usize {
                let sy = (y * sh / h as usize).min(sh - 1);
                for x in 0..w as usize {
                    let sx = (x * sw / w as usize).min(sw - 1);
                    let i = (sy * sw + sx) * 4;
                    buf[y * w as usize + x] =
                        ((src[i] as u32) << 16) | ((src[i + 1] as u32) << 8) | src[i + 2] as u32;
                }
            }
            let _ = buf.present();
        }
    }
}

/// Show a file selected in the system file manager (or open its folder).
pub fn show_in_folder(path: &std::path::Path) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let mut cmd = std::process::Command::new("explorer.exe");
        if path.is_dir() {
            // A folder: open it (`/select` would open its parent instead).
            cmd.arg(path);
        } else {
            // explorer wants `/select,"C:\path with spaces\x.png"` as ONE
            // argument with the quotes inside; raw_arg passes it untouched.
            cmd.raw_arg(format!("/select,\"{}\"", path.display()));
        }
        let _ = cmd.spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let dir = if path.is_dir() {
            path.to_path_buf()
        } else {
            path.parent().map(|p| p.to_path_buf()).unwrap_or_default()
        };
        let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
    }
}

#[cfg(target_os = "windows")]
fn no_activate(window: &Window) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    };
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    if let Ok(h) = window.window_handle() {
        if let RawWindowHandle::Win32(w) = h.as_raw() {
            let hwnd = HWND(w.hwnd.get() as *mut core::ffi::c_void);
            unsafe {
                let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                SetWindowLongPtrW(
                    hwnd,
                    GWL_EXSTYLE,
                    ex | WS_EX_NOACTIVATE.0 as isize | WS_EX_TOOLWINDOW.0 as isize,
                );
            }
        }
    }
}
