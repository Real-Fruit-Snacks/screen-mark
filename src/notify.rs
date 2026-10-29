//! System notifications (Windows notification centre / freedesktop).
//!
//! On Windows a toast needs an AppUserModelID the shell knows about. Without
//! one, notify-rust sends toasts as "Windows PowerShell", which is what 2.0
//! did. An unpackaged app can make its ID known by writing
//! `HKCU\Software\Classes\AppUserModelId\<id>` with a display name and icon;
//! that is what `register_app_id` does (and `unregister_app_id` undoes on
//! uninstall).

/// The AppUserModelID screen-mark uses for its process and its toasts.
pub const APP_ID: &str = "RealFruitSnacks.screen-mark";

/// Show a system notification. Errors are logged, not returned: a missing
/// notification daemon must never break a capture.
pub fn system(title: &str, body: &str) {
    let mut n = notify_rust::Notification::new();
    n.summary(title).body(body).timeout(4000);
    #[cfg(target_os = "windows")]
    {
        if app_id_registered() {
            n.app_id(APP_ID);
        }
    }
    #[cfg(target_os = "linux")]
    {
        n.appname("screen-mark");
    }
    if let Err(e) = n.show() {
        tracing::warn!("System notification failed: {e}");
    }
}

/// Make the app ID known to Windows so toasts show "screen-mark" and its icon.
/// Safe to call on every start.
pub fn register_app_id() {
    #[cfg(target_os = "windows")]
    win::register();
}

/// Remove what `register_app_id` wrote (uninstall).
pub fn unregister_app_id() {
    #[cfg(target_os = "windows")]
    win::unregister();
}

#[cfg(target_os = "windows")]
fn app_id_registered() -> bool {
    win::is_registered()
}

#[cfg(target_os = "windows")]
mod win {
    use super::APP_ID;
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegOpenKeyExW, RegSetValueExW, HKEY,
        HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ,
    };

    fn key_path() -> String {
        format!("Software\\Classes\\AppUserModelId\\{APP_ID}")
    }

    fn set_string(key: HKEY, name: &str, value: &str) {
        let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes =
            unsafe { std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2) };
        let name = HSTRING::from(name);
        let _ = unsafe { RegSetValueExW(key, PCWSTR(name.as_ptr()), 0, REG_SZ, Some(bytes)) };
    }

    pub fn register() {
        // The icon has to be a file; write the embedded PNG next to history.
        let icon = dirs::data_local_dir().map(|d| d.join("screen-mark").join("notify-icon.png"));
        if let Some(ref path) = icon {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if !path.exists() {
                let _ = std::fs::write(path, include_bytes!("../assets/icon.png"));
            }
        }
        let sub = HSTRING::from(key_path());
        let mut key = HKEY::default();
        let ok = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(sub.as_ptr()),
                0,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                None,
                &mut key,
                None,
            )
        };
        if ok.is_err() {
            tracing::warn!("Could not register notification app id: {ok:?}");
            return;
        }
        set_string(key, "DisplayName", "screen-mark");
        if let Some(path) = icon {
            set_string(key, "IconUri", &path.to_string_lossy());
        }
        let _ = unsafe { RegCloseKey(key) };
    }

    pub fn is_registered() -> bool {
        let sub = HSTRING::from(key_path());
        let mut key = HKEY::default();
        let ok = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(sub.as_ptr()),
                0,
                KEY_READ,
                &mut key,
            )
        };
        if ok.is_ok() {
            let _ = unsafe { RegCloseKey(key) };
            true
        } else {
            false
        }
    }

    pub fn unregister() {
        let sub = HSTRING::from(key_path());
        let _ = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(sub.as_ptr())) };
        if let Some(d) = dirs::data_local_dir() {
            let _ = std::fs::remove_file(d.join("screen-mark").join("notify-icon.png"));
        }
    }
}
