//! Putting a new version in place.
//!
//! How depends on how this copy got here (`Kind`):
//!
//! - A copy screen-mark put there itself, or one unpacked from the zip or
//!   tar.gz, is replaced file for file. The new program is written beside
//!   the old one, run once to see that it starts and is the version the
//!   release says, and then the two change places. The old one stays, as
//!   `screen-mark.old.exe`, until the new one has started (`remove_leftovers`).
//! - A copy the MSI installed is updated by the new MSI, so Windows'
//!   record of what is installed stays right.
//! - A copy a package manager owns, or one in a folder screen-mark can't
//!   write to, is left alone.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::{archive, Error, Release, Version, TARGET};

/// How a copy of screen-mark was installed, which decides how it updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Installed by the MSI.
    Msi,
    /// A file screen-mark can replace: self-installed, or unpacked by hand.
    Replace,
    /// Owned by a package manager, which is named.
    Managed(&'static str),
    /// In a folder screen-mark can't write to.
    ReadOnly,
    /// A debug build.
    Development,
}

impl Kind {
    /// Whether `install` can update this copy.
    pub fn installs(self) -> bool {
        matches!(self, Self::Msi | Self::Replace)
    }

    /// Why not, for the copies `install` leaves alone.
    pub fn reason(self) -> Option<String> {
        match self {
            Self::Msi | Self::Replace => None,
            Self::Managed(by) => Some(format!("this copy was installed by {by}; update it there")),
            Self::ReadOnly => {
                Some("screen-mark can't write to the folder it is installed in".into())
            }
            Self::Development => Some("this is a development build".into()),
        }
    }
}

/// The running program and how it was installed.
pub fn running_copy() -> Result<(PathBuf, Kind), Error> {
    let exe = std::env::current_exe().map_err(|e| {
        Error::Install(format!("can't tell where screen-mark is running from: {e}"))
    })?;
    let kind = if cfg!(debug_assertions) {
        Kind::Development
    } else {
        kind_of(&exe)
    };
    Ok((exe, kind))
}

/// How the copy at `exe` was installed.
pub fn kind_of(exe: &Path) -> Kind {
    if let Some(by) = package_manager(exe) {
        Kind::Managed(by)
    } else if installed_by_msi(exe) {
        Kind::Msi
    } else if can_write_beside(exe) {
        Kind::Replace
    } else {
        Kind::ReadOnly
    }
}

/// Scoop, winget and Chocolatey keep what they install in folders of their
/// own; a copy in one of those is theirs to update.
fn package_manager(exe: &Path) -> Option<&'static str> {
    let parts: Vec<String> = exe
        .components()
        .map(|part| part.as_os_str().to_string_lossy().to_lowercase())
        .collect();
    let under = |outer: &str, inner: &str| {
        parts
            .windows(2)
            .any(|pair| pair[0] == outer && pair[1] == inner)
    };
    if under("scoop", "apps") {
        Some("Scoop")
    } else if under("winget", "packages") || under("winget", "links") {
        Some("winget")
    } else if under("chocolatey", "lib") || under("chocolatey", "bin") {
        Some("Chocolatey")
    } else {
        None
    }
}

fn can_write_beside(exe: &Path) -> bool {
    let Some(folder) = exe.parent() else {
        return false;
    };
    let probe = folder.join(format!(".screen-mark-update-{}", std::process::id()));
    let made = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .is_ok();
    let _ = fs::remove_file(&probe);
    made
}

/// The MSI installs to `%LocalAppData%\screen-mark` and leaves a registry
/// value with the exe (wix/main.wxs). Both have to hold: the self-installer
/// uses the same folder.
#[cfg(target_os = "windows")]
fn installed_by_msi(exe: &Path) -> bool {
    use windows::core::w;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
    };

    let Some(installed) = dirs::data_local_dir().map(|d| d.join("screen-mark/screen-mark.exe"))
    else {
        return false;
    };
    let same = match (fs::canonicalize(exe), fs::canonicalize(installed)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    if !same {
        return false;
    }
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\screen-mark"),
            0,
            KEY_READ,
            &mut key,
        )
        .is_err()
        {
            return false;
        }
        let found = RegQueryValueExW(key, w!("exe_installed"), None, None, None, None).is_ok();
        let _ = RegCloseKey(key);
        found
    }
}

#[cfg(not(target_os = "windows"))]
fn installed_by_msi(_exe: &Path) -> bool {
    false
}

/// Update the copy at `exe` to `release`.
///
/// On success the new version is at `exe`; a screen-mark that is running
/// keeps running the old one until it is started again.
pub fn install(release: &Release, exe: &Path, kind: Kind) -> Result<(), Error> {
    install_with(release, exe, kind, &|msi| run_msi(msi, exe))
}

/// `install`, with the step that runs the MSI passed in. It gets the path
/// of the downloaded MSI and has to leave the new version at `exe`.
pub fn install_with(
    release: &Release,
    exe: &Path,
    kind: Kind,
    run_installer: &dyn Fn(&Path) -> Result<(), String>,
) -> Result<(), Error> {
    if let Some(reason) = kind.reason() {
        return Err(Error::Install(reason));
    }
    let target = TARGET.ok_or_else(|| {
        Error::NoDownload(format!(
            "{} {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ))
    })?;
    let version = release.version();

    if kind == Kind::Msi {
        let file = release.file(&format!("{target}.msi"))?;
        let scratch = Scratch::new()?;
        let msi = scratch.0.join(&file.name);
        fs::write(&msi, release.fetch(file)?)
            .map_err(|e| failed(&format!("write {}", msi.display()), e))?;

        // The installer can't replace a program that is running, but a
        // running program can be renamed out of its way.
        let old = beside(exe, "old");
        clear(&old)?;
        fs::rename(exe, &old).map_err(|e| failed("move the running version aside", e))?;
        let outcome = run_installer(&msi)
            .map_err(Error::Install)
            .and_then(|()| reports_version(exe, version));
        if outcome.is_err() && !exe.exists() {
            let _ = fs::rename(&old, exe);
        }
        return outcome;
    }

    let program = if cfg!(target_os = "windows") {
        let file = release.file(&format!("{target}.zip"))?;
        archive::from_zip(&release.fetch(file)?, "screen-mark.exe")?
    } else {
        let file = release.file(&format!("{target}.tar.gz"))?;
        archive::from_tar_gz(&release.fetch(file)?, "screen-mark")?
    };

    let new = beside(exe, "new");
    let outcome = (|| {
        fs::write(&new, &program).map_err(|e| failed(&format!("write {}", new.display()), e))?;
        // Unix: the new file has to be executable, like the one it replaces.
        if cfg!(unix) {
            if let Ok(current) = fs::metadata(exe) {
                let _ = fs::set_permissions(&new, current.permissions());
            }
        }
        reports_version(&new, version)?;

        let old = beside(exe, "old");
        clear(&old)?;
        fs::rename(exe, &old).map_err(|e| failed("move the running version aside", e))?;
        fs::rename(&new, exe).map_err(|e| {
            let _ = fs::rename(&old, exe);
            failed("put the new version in place", e)
        })
    })();
    if outcome.is_err() {
        let _ = fs::remove_file(&new);
    }
    outcome
}

/// Delete the version an update replaced, which it left beside `exe`.
/// Called when screen-mark starts, so the old version is kept until the
/// new one has run.
pub fn remove_leftovers(exe: &Path) {
    let old = beside(exe, "old");
    if old.exists() {
        match fs::remove_file(&old) {
            Ok(()) => tracing::info!("Removed {}", old.display()),
            // Still running (an update from the command line while the
            // tray app was up): the next start gets it.
            Err(e) => tracing::debug!("Couldn't remove {}: {e}", old.display()),
        }
    }
}

fn failed(what: &str, e: std::io::Error) -> Error {
    Error::Install(format!("couldn't {what}: {e}"))
}

/// `screen-mark.old.exe` for `screen-mark.exe` and "old"; `screen-mark.old`
/// where programs have no extension.
fn beside(exe: &Path, tag: &str) -> PathBuf {
    let stem = exe.file_stem().unwrap_or_default().to_string_lossy();
    let name = match exe.extension() {
        Some(extension) => format!("{stem}.{tag}.{}", extension.to_string_lossy()),
        None => format!("{stem}.{tag}"),
    };
    exe.with_file_name(name)
}

/// Make room for the old version. One that is still there and can't be
/// deleted is still running: an earlier update that hasn't been restarted
/// into.
fn clear(old: &Path) -> Result<(), Error> {
    if old.exists() {
        fs::remove_file(old).map_err(|_| {
            Error::Install(
                "an earlier update is waiting for screen-mark to restart; restart it first".into(),
            )
        })?;
    }
    Ok(())
}

/// Run `program --version` and check it answers with `version`: it starts
/// on this machine, and it is the release it claims to be.
fn reports_version(program: &Path, version: Version) -> Result<(), Error> {
    let mut command = Command::new(program);
    command
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    // A file that was only just written can be held for a moment: by a
    // virus scanner on Windows (a sharing violation, error 32), or on Unix
    // by a process another thread is starting ("text file busy").
    let mut tries = 0;
    let mut child = loop {
        match command.spawn() {
            Ok(child) => break child,
            Err(e)
                if tries < 20
                    && (e.kind() == std::io::ErrorKind::ExecutableFileBusy
                        || (cfg!(target_os = "windows") && e.raw_os_error() == Some(32))) =>
            {
                tries += 1;
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                return Err(Error::Install(format!(
                    "the new version doesn't start: {e}"
                )))
            }
        }
    };

    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(25));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::Install(
                    "the new version didn't answer when it was started".into(),
                ));
            }
        }
    };
    // A few lines at most, so reading after it has exited can't block it.
    let mut said = String::new();
    if let Some(mut out) = child.stdout.take() {
        let _ = out.read_to_string(&mut said);
    }
    let expected = format!("screen-mark {version}");
    if status.success() && said.lines().any(|line| line.trim() == expected) {
        Ok(())
    } else {
        let said = said.lines().last().unwrap_or("nothing").trim().to_string();
        Err(Error::Install(format!(
            "the new version doesn't say it is {version} (it says: {said})"
        )))
    }
}

/// A folder for a download, deleted when this is dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self, Error> {
        static COUNT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("screen-mark-update-{}-{n}", std::process::id()));
        fs::create_dir_all(&path).map_err(|e| failed(&format!("create {}", path.display()), e))?;
        Ok(Self(path))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Run the MSI and wait for it. `/passive` shows its progress bar and asks
/// nothing; the install is per-user, so there is no elevation prompt.
#[cfg(target_os = "windows")]
fn run_msi(msi: &Path, exe: &Path) -> Result<(), String> {
    // The MSI clears the start-on-login entry as it installs
    // (RemoveRegistryValue in wix/main.wxs), so it is put back afterwards.
    let autostart = crate::autostart::is_enabled();
    let msiexec = std::env::var_os("SystemRoot")
        .map(|root| PathBuf::from(root).join("System32").join("msiexec.exe"))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("msiexec"));
    let status = Command::new(msiexec)
        .arg("/i")
        .arg(msi)
        .args(["/passive", "/norestart"])
        .status()
        .map_err(|e| format!("couldn't start the installer: {e}"));
    if autostart {
        if let Err(e) = crate::autostart::set_enabled_for(true, Some(exe)) {
            tracing::warn!("Couldn't restore start on login: {e}");
        }
    }
    match status?.code() {
        // 3010: installed, and Windows wants a restart at some point.
        Some(0 | 3010) => Ok(()),
        Some(1602) => Err("the installer was cancelled".into()),
        Some(1618) => Err("another installation is in progress; try again when it is done".into()),
        Some(code) => Err(format!("the installer stopped with code {code}")),
        None => Err("the installer was stopped".into()),
    }
}

#[cfg(not(target_os = "windows"))]
fn run_msi(_msi: &Path, _exe: &Path) -> Result<(), String> {
    Err("MSI installs exist only on Windows".into())
}
