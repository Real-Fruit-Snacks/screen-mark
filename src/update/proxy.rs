//! Which proxy, if any, an update download goes through.
//!
//! In order: `[updates] proxy` when it is set (`none` for no proxy), then
//! the `HTTPS_PROXY` / `HTTP_PROXY` / `ALL_PROXY` environment variables
//! with `NO_PROXY`, then on Windows the proxy set in Windows' own settings
//! (Settings > Network & internet > Proxy > Manual), with its exceptions.
//! A proxy set by a script (PAC) or found automatically (WPAD) isn't read;
//! put that proxy in `[updates] proxy`.
//!
//! A proxy only carries the bytes. What is installed is still checked
//! against the signed manifest, so a proxy can stop an update but not
//! change it.

/// The proxy that Windows' settings name, as read from the registry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SystemProxy {
    /// `ProxyServer`: `host:port`, or `http=host:port;https=host:port`.
    pub server: String,
    /// `ProxyOverride`: `;`-separated hosts that go direct. `*` is a
    /// wildcard and `<local>` means any name without a dot.
    pub bypass: String,
}

/// The proxy for `url`, as `http://host:port`, or `None` to connect
/// directly. `env` looks up an environment variable.
pub fn choose(
    url: &str,
    setting: &str,
    env: &dyn Fn(&str) -> Option<String>,
    system: Option<&SystemProxy>,
) -> Option<String> {
    let setting = setting.trim();
    if setting.eq_ignore_ascii_case("none") || setting.eq_ignore_ascii_case("direct") {
        return None;
    }
    if !setting.is_empty() {
        return Some(with_scheme(setting));
    }
    let (https, host) = split_url(url)?;

    let var = |names: &[&str]| {
        names
            .iter()
            .find_map(|name| env(name).filter(|v| !v.trim().is_empty()))
    };
    let from_env = if https {
        var(&["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"])
    } else {
        var(&["HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"])
    };
    if let Some(proxy) = from_env {
        let skip = var(&["NO_PROXY", "no_proxy"]).unwrap_or_default();
        if bypassed_by_no_proxy(&host, &skip) {
            return None;
        }
        return Some(with_scheme(proxy.trim()));
    }

    let system = system?;
    let server = system_server(&system.server, https)?;
    if bypassed_by_windows(&host, &system.bypass) {
        return None;
    }
    Some(with_scheme(&server))
}

/// `proxy` with any password replaced, for messages and the log.
pub fn shown(proxy: &str) -> String {
    let (scheme, rest) = match proxy.split_once("://") {
        Some((scheme, rest)) => (format!("{scheme}://"), rest),
        None => (String::new(), proxy),
    };
    match rest.rsplit_once('@') {
        Some((user, host)) => {
            let user = user.split(':').next().unwrap_or("");
            format!("{scheme}{user}:***@{host}")
        }
        None => proxy.to_string(),
    }
}

/// Whether `url` is https, and its host in lower case, without the port.
fn split_url(url: &str) -> Option<(bool, String)> {
    let (https, rest) = if let Some(rest) = url.strip_prefix("https://") {
        (true, rest)
    } else {
        (false, url.strip_prefix("http://")?)
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let host = if let Some(v6) = authority.strip_prefix('[') {
        v6.split(']').next().unwrap_or("")
    } else {
        authority.split(':').next().unwrap_or("")
    };
    (!host.is_empty()).then(|| (https, host.to_ascii_lowercase()))
}

/// `proxy:8080` as `http://proxy:8080`; anything with a scheme as it is.
fn with_scheme(proxy: &str) -> String {
    let proxy = proxy.trim().trim_end_matches('/');
    if proxy.contains("://") {
        proxy.to_string()
    } else {
        format!("http://{proxy}")
    }
}

/// `NO_PROXY`: comma-separated names. `example.com` and `.example.com` both
/// cover the name and everything under it; `*` covers everything. A port on
/// an entry is ignored.
fn bypassed_by_no_proxy(host: &str, list: &str) -> bool {
    list.split([',', ' ']).any(|entry| {
        let entry = entry.trim().to_ascii_lowercase();
        if entry == "*" {
            return true;
        }
        let entry = entry
            .trim_start_matches("*.")
            .trim_start_matches('.')
            .to_string();
        let entry = match entry.strip_prefix('[') {
            Some(v6) => v6.split(']').next().unwrap_or("").to_string(),
            None if entry.matches(':').count() == 1 => {
                entry.split(':').next().unwrap_or("").to_string()
            }
            None => entry,
        };
        !entry.is_empty() && (host == entry || host.ends_with(&format!(".{entry}")))
    })
}

/// The server Windows uses for this kind of URL. A plain `host:port` is
/// for every protocol; `http=a:1;https=b:2` names one for each. A SOCKS
/// proxy is skipped: updates can't use one.
fn system_server(server: &str, https: bool) -> Option<String> {
    let server = server.trim();
    if server.is_empty() {
        return None;
    }
    if !server.contains('=') {
        return Some(server.to_string());
    }
    let wanted = if https { "https" } else { "http" };
    let mut named = server.split(';').filter_map(|part| {
        let (scheme, address) = part.split_once('=')?;
        let address = address.trim();
        (!address.is_empty()).then(|| (scheme.trim().to_ascii_lowercase(), address.to_string()))
    });
    named
        .find(|(scheme, _)| scheme == wanted)
        .map(|(_, address)| address)
}

/// `ProxyOverride`, as Windows reads it.
fn bypassed_by_windows(host: &str, list: &str) -> bool {
    list.split(';').any(|entry| {
        let entry = entry.trim().to_ascii_lowercase();
        if entry == "<local>" {
            return !host.contains('.') && !host.contains(':');
        }
        !entry.is_empty() && wildcard(&entry, host)
    })
}

/// `*` matches any run of characters, including none.
fn wildcard(pattern: &str, text: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or("");
    let Some(mut rest) = text.strip_prefix(first) else {
        return false;
    };
    let parts: Vec<&str> = parts.collect();
    let Some((last, middle)) = parts.split_last() else {
        return rest.is_empty();
    };
    for part in middle {
        match rest.find(part) {
            Some(i) => rest = &rest[i + part.len()..],
            None => return false,
        }
    }
    rest.ends_with(last)
}

/// The proxy in Windows' settings, if one is switched on.
#[cfg(target_os = "windows")]
pub fn system() -> Option<SystemProxy> {
    use windows::core::{w, PCWSTR};
    use windows::Win32::System::Registry::{
        RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
    };

    const KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings");
    let text = |name: PCWSTR| -> Option<String> {
        let mut buf = vec![0u16; 2048];
        let mut size = (buf.len() * 2) as u32;
        unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                KEY,
                name,
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr().cast()),
                Some(&mut size),
            )
            .ok()
            .ok()?;
        }
        let len = (size as usize / 2).min(buf.len());
        let s = String::from_utf16_lossy(&buf[..len]);
        Some(s.trim_end_matches('\0').to_string())
    };
    let mut enabled: u32 = 0;
    let mut size = 4u32;
    let read = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            KEY,
            w!("ProxyEnable"),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut enabled as *mut u32).cast()),
            Some(&mut size),
        )
    };
    if read.is_err() || enabled == 0 {
        return None;
    }
    Some(SystemProxy {
        server: text(w!("ProxyServer"))?,
        bypass: text(w!("ProxyOverride")).unwrap_or_default(),
    })
}

#[cfg(not(target_os = "windows"))]
pub fn system() -> Option<SystemProxy> {
    None
}
