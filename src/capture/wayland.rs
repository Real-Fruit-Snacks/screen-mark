//! Wayland screen capture, through the desktop's screenshot portal
//! (`org.freedesktop.portal.Screenshot`, from xdg-desktop-portal and the
//! GNOME, KDE or wlroots backend). Wayland lets no program read the screen
//! itself; the portal takes the picture and hands back a file.
//!
//! The first time, the desktop may ask whether screen-mark may take
//! screenshots. screen-mark's windows run under XWayland (see
//! `event_loop_builder` in main.rs), so the picture is scaled to the X
//! screen's size when the two differ: the overlay is an X window, and it
//! shows the capture one pixel to one pixel.

use super::{CaptureError, CapturedScreen, ScreenCapture};

pub struct PortalCapturer;

impl PortalCapturer {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PortalCapturer {
    fn default() -> Self {
        Self::new()
    }
}

impl ScreenCapture for PortalCapturer {
    fn capture_all_screens(&self) -> Result<Vec<CapturedScreen>, CaptureError> {
        let uri = pollster::block_on(request()).map_err(describe)?;
        let path = uri.to_file_path().map_err(|_| {
            CaptureError::PlatformError(format!("the screenshot portal gave {uri}, not a file"))
        })?;
        let image = image::open(&path)
            .map_err(|e| {
                CaptureError::PlatformError(format!(
                    "couldn't read the portal's screenshot {}: {e}",
                    path.display()
                ))
            })?
            .to_rgba8();
        forget(&path);

        let image = match x_screen_size() {
            Some((w, h)) if (w, h) != image.dimensions() && w > 0 && h > 0 => {
                tracing::info!(
                    "Scaling the portal's {}x{} screenshot to the X screen's {w}x{h}",
                    image.width(),
                    image.height()
                );
                image::imageops::resize(&image, w, h, image::imageops::FilterType::Triangle)
            }
            _ => image,
        };
        let (width, height) = image.dimensions();
        Ok(vec![CapturedScreen {
            pixels: image.into_raw(),
            width,
            height,
            x_offset: 0,
            y_offset: 0,
            scale_factor: 1.0,
        }])
    }
}

async fn request() -> ashpd::Result<url::Url> {
    let response = ashpd::desktop::screenshot::Screenshot::request()
        .interactive(false)
        .modal(false)
        .send()
        .await?
        .response()?;
    Ok(response.uri().clone())
}

/// The portal's errors, in words for the popup.
fn describe(e: ashpd::Error) -> CaptureError {
    match e {
        ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled) => {
            CaptureError::PermissionDenied
        }
        ashpd::Error::Response(ashpd::desktop::ResponseError::Other) => {
            CaptureError::PlatformError("the desktop refused to take the screenshot".into())
        }
        ashpd::Error::PortalNotFound(_) | ashpd::Error::Zbus(_) | ashpd::Error::Portal(_) => {
            CaptureError::PlatformError(format!(
                "no screenshot portal answered ({e}); install xdg-desktop-portal with the \
                 GNOME, KDE or wlr backend"
            ))
        }
        e => CaptureError::PlatformError(format!("the screenshot portal failed: {e}")),
    }
}

/// Remove the portal's file: the capture is screen-mark's to save or not.
/// Only a file made just now, so a portal that hands back some other file
/// never costs anyone it.
fn forget(path: &std::path::Path) {
    let fresh = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < std::time::Duration::from_secs(60));
    if fresh {
        if let Err(e) = std::fs::remove_file(path) {
            tracing::debug!(
                "Couldn't remove the portal's screenshot {}: {e}",
                path.display()
            );
        }
    }
}

/// The size of the X screen (XWayland's), if there is one.
fn x_screen_size() -> Option<(u32, u32)> {
    use x11rb::connection::Connection;
    std::env::var_os("DISPLAY")?;
    let (conn, screen) = x11rb::connect(None).ok()?;
    let root = conn.setup().roots.get(screen)?;
    Some((root.width_in_pixels as u32, root.height_in_pixels as u32))
}

/// Whether capture should go through the portal: on a Wayland session, or
/// when `SCREEN_MARK_CAPTURE` says so (`portal` or `x11`).
pub fn wanted() -> bool {
    match std::env::var("SCREEN_MARK_CAPTURE")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "portal" | "wayland" => true,
        "x11" | "x" => false,
        _ => {
            std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty())
                || std::env::var("XDG_SESSION_TYPE")
                    .is_ok_and(|t| t.eq_ignore_ascii_case("wayland"))
        }
    }
}
