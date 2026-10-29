//! Monitors and the mouse pointer, in virtual-desktop pixels (the same
//! coordinates as `CapturedScreen::x_offset/y_offset`).

use crate::capture::CapturedScreen;

/// A rectangle in virtual-desktop pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }

    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x
            && py >= self.y
            && (px as i64) < self.x as i64 + self.w as i64
            && (py as i64) < self.y as i64 + self.h as i64
    }

    /// Squared distance from a point to the rectangle (0 inside).
    fn distance2(&self, px: i32, py: i32) -> i64 {
        // max(1): a zero-size rectangle is a point, not a panicking clamp.
        let cx = (px as i64).clamp(self.x as i64, self.x as i64 + self.w.max(1) as i64 - 1);
        let cy = (py as i64).clamp(self.y as i64, self.y as i64 + self.h.max(1) as i64 - 1);
        (px as i64 - cx).pow(2) + (py as i64 - cy).pow(2)
    }

    /// The overlap of two rectangles, if any.
    pub fn intersect(&self, o: &Rect) -> Option<Rect> {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = (self.x as i64 + self.w as i64).min(o.x as i64 + o.w as i64);
        let y1 = (self.y as i64 + self.h as i64).min(o.y as i64 + o.h as i64);
        if x1 <= x0 as i64 || y1 <= y0 as i64 {
            return None;
        }
        Some(Rect::new(
            x0,
            y0,
            (x1 - x0 as i64) as u32,
            (y1 - y0 as i64) as u32,
        ))
    }
}

/// One monitor: its full area and its work area (minus taskbars/docks).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Monitor {
    pub area: Rect,
    pub work: Rect,
}

/// The monitor containing the point, or the nearest one if the point is in
/// a gap between monitors. `None` only when the list is empty.
pub fn monitor_at(monitors: &[Monitor], x: i32, y: i32) -> Option<Monitor> {
    monitors
        .iter()
        .find(|m| m.area.contains(x, y))
        .or_else(|| monitors.iter().min_by_key(|m| m.area.distance2(x, y)))
        .copied()
}

/// Cut `rect` (virtual-desktop pixels) out of a capture. The rectangle is
/// clipped to the capture; `None` if they don't overlap.
pub fn crop(screen: &CapturedScreen, rect: Rect) -> Option<CapturedScreen> {
    let full = Rect::new(
        screen.x_offset,
        screen.y_offset,
        screen.width,
        screen.height,
    );
    let r = full.intersect(&rect)?;
    if r == full {
        return Some(screen.clone());
    }
    let sx = (r.x - screen.x_offset) as usize;
    let sy = (r.y - screen.y_offset) as usize;
    let stride = screen.width as usize * 4;
    let mut pixels = Vec::with_capacity(r.w as usize * r.h as usize * 4);
    for row in sy..sy + r.h as usize {
        let start = row * stride + sx * 4;
        let end = start + r.w as usize * 4;
        pixels.extend_from_slice(screen.pixels.get(start..end)?);
    }
    Some(CapturedScreen {
        pixels,
        width: r.w,
        height: r.h,
        x_offset: r.x,
        y_offset: r.y,
        scale_factor: screen.scale_factor,
    })
}

/// Every monitor, as the OS reports it. Empty if it can't be determined.
pub fn monitors() -> Vec<Monitor> {
    #[cfg(target_os = "windows")]
    let list = win::monitors();
    #[cfg(target_os = "linux")]
    let list = x11::monitors().unwrap_or_default();
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    let list: Vec<Monitor> = Vec::new();
    // A disconnected or mirrored output can report an empty rectangle.
    list.into_iter()
        .filter(|m| m.area.w > 0 && m.area.h > 0)
        .collect()
}

/// The mouse pointer, if it can be read.
pub fn pointer() -> Option<(i32, i32)> {
    #[cfg(target_os = "windows")]
    {
        win::pointer()
    }
    #[cfg(target_os = "linux")]
    {
        x11::pointer()
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        None
    }
}

#[cfg(target_os = "windows")]
mod win {
    use super::{Monitor, Rect};
    use windows::Win32::Foundation::{BOOL, LPARAM, POINT, RECT, TRUE};
    use windows::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

    fn rect(r: RECT) -> Rect {
        Rect::new(
            r.left,
            r.top,
            (r.right - r.left).max(0) as u32,
            (r.bottom - r.top).max(0) as u32,
        )
    }

    unsafe extern "system" fn collect(
        hmon: HMONITOR,
        _hdc: HDC,
        _clip: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        let list = unsafe { &mut *(data.0 as *mut Vec<Monitor>) };
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if unsafe { GetMonitorInfoW(hmon, &mut info) }.as_bool() {
            list.push(Monitor {
                area: rect(info.rcMonitor),
                work: rect(info.rcWork),
            });
        }
        TRUE
    }

    pub fn monitors() -> Vec<Monitor> {
        let mut list: Vec<Monitor> = Vec::new();
        unsafe {
            let _ = EnumDisplayMonitors(
                None,
                None,
                Some(collect),
                LPARAM(&mut list as *mut Vec<Monitor> as isize),
            );
        }
        list
    }

    pub fn pointer() -> Option<(i32, i32)> {
        let mut p = POINT::default();
        unsafe { GetCursorPos(&mut p) }.ok()?;
        Some((p.x, p.y))
    }
}

#[cfg(target_os = "linux")]
mod x11 {
    use super::{Monitor, Rect};
    use x11rb::connection::Connection;
    use x11rb::protocol::randr::ConnectionExt as _;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};

    pub fn monitors() -> Option<Vec<Monitor>> {
        let (conn, screen_num) = x11rb::connect(None).ok()?;
        let screen = conn.setup().roots.get(screen_num)?.clone();
        let whole = Rect::new(
            0,
            0,
            screen.width_in_pixels as u32,
            screen.height_in_pixels as u32,
        );
        let work = work_area(&conn, screen.root).unwrap_or(whole);
        let list = conn
            .randr_get_monitors(screen.root, true)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| {
                r.monitors
                    .iter()
                    .map(|m| {
                        let area =
                            Rect::new(m.x as i32, m.y as i32, m.width as u32, m.height as u32);
                        Monitor {
                            area,
                            work: area.intersect(&work).unwrap_or(area),
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if list.is_empty() {
            Some(vec![Monitor { area: whole, work }])
        } else {
            Some(list)
        }
    }

    /// `_NET_WORKAREA` of desktop 0, if a window manager publishes it.
    fn work_area(conn: &impl Connection, root: u32) -> Option<Rect> {
        let atom = conn
            .intern_atom(true, b"_NET_WORKAREA")
            .ok()?
            .reply()
            .ok()?
            .atom;
        if atom == 0 {
            return None;
        }
        let reply = conn
            .get_property(false, root, atom, AtomEnum::CARDINAL, 0, 4)
            .ok()?
            .reply()
            .ok()?;
        let v: Vec<u32> = reply.value32()?.collect();
        if v.len() < 4 {
            return None;
        }
        Some(Rect::new(v[0] as i32, v[1] as i32, v[2], v[3]))
    }

    pub fn pointer() -> Option<(i32, i32)> {
        let (conn, screen_num) = x11rb::connect(None).ok()?;
        let root = conn.setup().roots.get(screen_num)?.root;
        let r = conn.query_pointer(root).ok()?.reply().ok()?;
        Some((r.root_x as i32, r.root_y as i32))
    }
}
