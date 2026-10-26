/// Check whether screen-mark is configured to start on login.
pub fn is_enabled() -> bool {
    #[cfg(target_os = "windows")]
    {
        is_enabled_windows()
    }
    #[cfg(target_os = "linux")]
    {
        is_enabled_linux()
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        false
    }
}

/// Enable or disable auto-start on login.
///
/// Uses `current_exe()` to determine which binary to register.
/// If you need to register a *different* path (e.g. during install),
/// use [`set_enabled_for`] instead.
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let exe = if enabled {
        Some(std::env::current_exe().map_err(|e| format!("Failed to find current exe: {e}"))?)
    } else {
        None
    };
    set_enabled_for(enabled, exe.as_deref())
}

/// Enable or disable auto-start, registering a specific exe path.
///
/// When `enabled` is true, `exe_path` must be `Some`.
pub fn set_enabled_for(enabled: bool, exe_path: Option<&std::path::Path>) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        set_enabled_windows(enabled, exe_path)
    }
    #[cfg(target_os = "linux")]
    {
        set_enabled_linux(enabled, exe_path)
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = (enabled, exe_path);
        Err("Auto-start is not supported on this platform".into())
    }
}

#[cfg(target_os = "windows")]
fn is_enabled_windows() -> bool {
    use windows::core::*;
    use windows::Win32::System::Registry::*;

    unsafe {
        let mut key = HKEY::default();
        let path = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
        if RegOpenKeyExW(HKEY_CURRENT_USER, path, 0, KEY_READ, &mut key).is_ok() {
            let new = RegQueryValueExW(key, w!("screen-mark"), None, None, None, None);
            // A 1.x (`HydroShot`) entry also counts; set_enabled replaces it.
            let old = RegQueryValueExW(key, w!("HydroShot"), None, None, None, None);
            let _ = RegCloseKey(key);
            new.is_ok() || old.is_ok()
        } else {
            false
        }
    }
}

#[cfg(target_os = "windows")]
fn set_enabled_windows(enabled: bool, exe_path: Option<&std::path::Path>) -> Result<(), String> {
    use windows::core::*;
    use windows::Win32::System::Registry::*;

    unsafe {
        let mut key = HKEY::default();
        let path = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
        RegOpenKeyExW(HKEY_CURRENT_USER, path, 0, KEY_WRITE, &mut key)
            .ok()
            .map_err(|e| e.to_string())?;

        if enabled {
            let exe = exe_path.ok_or("exe_path required when enabling autostart")?;
            // Quote the path: unquoted Run values containing spaces are parsed
            // CreateProcess-style (tries "C:\Users\First.exe" first).
            let exe_str = format!("\"{}\"", exe.to_string_lossy());
            let value: Vec<u16> = exe_str.encode_utf16().chain(std::iter::once(0)).collect();
            let bytes: &[u8] =
                std::slice::from_raw_parts(value.as_ptr() as *const u8, value.len() * 2);
            RegSetValueExW(key, w!("screen-mark"), 0, REG_SZ, Some(bytes))
                .ok()
                .map_err(|e| e.to_string())?;
        } else {
            let _ = RegDeleteValueW(key, w!("screen-mark"));
        }
        // Either way, retire the 1.x entry so two copies never start.
        let _ = RegDeleteValueW(key, w!("HydroShot"));

        let _ = RegCloseKey(key);
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn is_enabled_linux() -> bool {
    autostart_desktop_path().is_some_and(|p| p.exists())
        || legacy_autostart_desktop_path().is_some_and(|p| p.exists())
}

#[cfg(target_os = "linux")]
fn set_enabled_linux(enabled: bool, exe_path: Option<&std::path::Path>) -> Result<(), String> {
    let path = autostart_desktop_path().ok_or("Could not determine config directory")?;
    // Retire the 1.x entry so two copies never start.
    if let Some(old) = legacy_autostart_desktop_path() {
        let _ = std::fs::remove_file(old);
    }

    if enabled {
        let exe = exe_path.ok_or("exe_path required when enabling autostart")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        // Quote Exec so paths containing spaces survive desktop-entry parsing.
        let content = format!(
            "[Desktop Entry]\nType=Application\nName=screen-mark\nExec=\"{}\"\nX-GNOME-Autostart-enabled=true\n",
            exe.display()
        );
        std::fs::write(&path, content).map_err(|e| e.to_string())
    } else {
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| e.to_string())
        } else {
            Ok(())
        }
    }
}

#[cfg(target_os = "linux")]
fn autostart_desktop_path() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|d| d.join("autostart").join("screen-mark.desktop"))
}

#[cfg(target_os = "linux")]
fn legacy_autostart_desktop_path() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|d| d.join("autostart").join("hydroshot.desktop"))
}
