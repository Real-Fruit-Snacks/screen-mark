//! Updating screen-mark from its own releases.
//!
//! Nothing here runs unless it is asked for: the tray's "Check for
//! updates", the `update` command, or `[updates] auto_check = true`.
//!
//! A release carries `latest.json` (its version, and the name, size and
//! SHA-256 of each download) and `latest.json.sig`, a signature of that
//! file. The keys it has to be signed with are compiled in (`keys`). So a
//! release is trusted for who signed it, not for where it was fetched: a
//! mirror, a proxy or a damaged download can make an update fail, but can't
//! make screen-mark install something else. An older release is never
//! installed over a newer one, so a replayed manifest does nothing either.
//!
//! - `check` reads and verifies the manifest and says whether it is newer.
//! - `install` (in `apply`) downloads the right file, checks it against the
//!   manifest, makes sure the new program runs, and swaps it in.

pub mod apply;
pub mod archive;
pub mod keys;
pub mod manifest;
pub mod proxy;

use std::fmt;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

pub use apply::{install, install_with, kind_of, remove_leftovers, running_copy, Kind};
pub use manifest::{File, Manifest};

/// The two files of a release the updater reads first.
pub const MANIFEST: &str = "latest.json";
pub const SIGNATURE: &str = "latest.json.sig";

/// Set for a screen-mark that an update started, to the version it
/// replaced. It waits for that one to exit instead of giving up because
/// "screen-mark is already running".
pub const UPDATED_FROM: &str = "SCREEN_MARK_UPDATED_FROM";

const MAX_MANIFEST: u64 = 64 * 1024;
const MAX_SIGNATURE: u64 = 4 * 1024;
/// A download or an unpacked program larger than this is refused. Releases
/// are around 10 MB; this only keeps a mistake from filling memory.
pub const MAX_DOWNLOAD: u64 = 1024 * 1024 * 1024;

/// The releases this build can install, as the end of their file names.
pub const TARGET: Option<&str> = if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
    Some("windows-x64")
} else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
    Some("linux-x64")
} else {
    None
};

/// A release version: three numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// `2.3.0`, with or without a `v` in front. Anything else is refused,
    /// pre-release tags included: nobody is updated to one.
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.strip_prefix('v').unwrap_or(text).split('.');
        let mut number = || -> Option<u32> {
            let part = parts.next()?;
            let plain =
                !part.is_empty() && part.len() <= 9 && part.bytes().all(|b| b.is_ascii_digit());
            plain.then(|| part.parse().ok()).flatten()
        };
        let version = Self::new(number()?, number()?, number()?);
        parts.next().is_none().then_some(version)
    }

    /// The version of this build.
    pub fn running() -> Self {
        let text = env!("CARGO_PKG_VERSION");
        let numbers = text.split(['-', '+']).next().unwrap_or(text);
        Self::parse(numbers).unwrap_or(Self::new(0, 0, 0))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Why an update didn't happen. The text finishes a sentence such as
/// "Couldn't check for updates: ...".
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// A file couldn't be fetched.
    #[error("{0}")]
    Fetch(String),
    /// There is no `latest.json` where releases are looked for.
    #[error("there is no update information at {0}")]
    NoManifest(String),
    /// `latest.json` is not signed with a key this build trusts.
    #[error("the update information is not signed by screen-mark's release key")]
    Signature,
    #[error("the update information can't be used: {0}")]
    Manifest(String),
    /// The release has no file for this system.
    #[error("the release has no download for this system ({0})")]
    NoDownload(String),
    /// A download isn't the file the manifest describes.
    #[error("{name} is not the file the release describes: {why}")]
    Mismatch { name: String, why: String },
    #[error("{0}")]
    Archive(String),
    /// The new version couldn't be put in place.
    #[error("{0}")]
    Install(String),
}

/// `[updates] proxy`, for every fetch this process makes. See `proxy`.
static PROXY_SETTING: Mutex<String> = Mutex::new(String::new());

/// Use `setting` (`[updates] proxy`) from now on: empty to follow the
/// environment and Windows' settings, `none` for no proxy, or a proxy.
pub fn set_proxy(setting: &str) {
    if let Ok(mut current) = PROXY_SETTING.lock() {
        *current = setting.trim().to_string();
    }
}

/// The proxy a fetch of `url` goes through, if any.
pub fn proxy_for(url: &str) -> Option<String> {
    let setting = PROXY_SETTING.lock().map(|s| s.clone()).unwrap_or_default();
    let system = proxy::system();
    proxy::choose(
        url,
        &setting,
        &|name| std::env::var(name).ok(),
        system.as_ref(),
    )
}

/// Where releases are fetched from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The project's GitHub releases.
    GitHub,
    /// A web folder that holds `latest.json`, its signature and the
    /// downloads side by side.
    Web(String),
    /// The same on a disk or a network share.
    Folder(PathBuf),
}

impl Source {
    /// From `[updates] url`: empty is GitHub, `http(s)://...` a web folder,
    /// anything else a folder path.
    pub fn from_setting(setting: &str) -> Self {
        let setting = setting.trim();
        if setting.is_empty() {
            Self::GitHub
        } else if setting.starts_with("http://") || setting.starts_with("https://") {
            Self::Web(setting.trim_end_matches('/').to_string())
        } else {
            Self::Folder(PathBuf::from(setting))
        }
    }

    /// Where `name` is. `release` is the version it belongs to, or `None`
    /// for the newest release (the manifest and its signature).
    fn place(&self, name: &str, release: Option<Version>) -> String {
        match (self, release) {
            (Self::GitHub, None) => format!(
                "{}/releases/latest/download/{name}",
                env!("CARGO_PKG_REPOSITORY")
            ),
            (Self::GitHub, Some(version)) => format!(
                "{}/releases/download/v{version}/{name}",
                env!("CARGO_PKG_REPOSITORY")
            ),
            (Self::Web(base), _) => format!("{base}/{name}"),
            (Self::Folder(folder), _) => folder.join(name).display().to_string(),
        }
    }

    /// Read `name` whole, refusing more than `max` bytes. `Ok(None)` when
    /// it isn't there.
    fn read(
        &self,
        name: &str,
        release: Option<Version>,
        max: u64,
    ) -> Result<Option<Vec<u8>>, Error> {
        let place = self.place(name, release);
        let proxy = match self {
            Self::Folder(_) => None,
            _ => proxy_for(&place),
        };
        let via = proxy
            .as_deref()
            .map(|p| format!(" through the proxy {}", proxy::shown(p)))
            .unwrap_or_default();
        let failed = |why: String| Error::Fetch(format!("couldn't read {place}{via}: {why}"));
        let reader: Box<dyn Read> = match self {
            Self::Folder(folder) => match std::fs::File::open(folder.join(name)) {
                Ok(file) => Box::new(file),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(failed(e.to_string())),
            },
            _ => {
                let mut agent = ureq::AgentBuilder::new()
                    .timeout_connect(Duration::from_secs(15))
                    .timeout_read(Duration::from_secs(30))
                    .user_agent(concat!("screen-mark/", env!("CARGO_PKG_VERSION")));
                if let Some(proxy) = &proxy {
                    let parsed = ureq::Proxy::new(proxy).map_err(|e| {
                        Error::Fetch(format!(
                            "the proxy {} can't be used: {e}",
                            proxy::shown(proxy)
                        ))
                    })?;
                    agent = agent.proxy(parsed);
                }
                let agent = agent.build();
                match agent.get(&place).call() {
                    Ok(response) => Box::new(response.into_reader()),
                    Err(ureq::Error::Status(404, _)) => return Ok(None),
                    Err(ureq::Error::Status(code, _)) => {
                        return Err(failed(format!("the server answered {code}")))
                    }
                    Err(ureq::Error::Transport(e)) => {
                        // Its own text starts with the URL, which is in ours.
                        let mut why = e.kind().to_string();
                        if let Some(detail) = e.message() {
                            why = format!("{why}: {detail}");
                        }
                        if let Some(cause) = std::error::Error::source(&e) {
                            why = format!("{why}: {cause}");
                        }
                        return Err(failed(why));
                    }
                }
            }
        };
        let mut bytes = Vec::new();
        reader
            .take(max.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|e| failed(e.to_string()))?;
        if bytes.len() as u64 > max {
            return Err(failed(format!("it is larger than {max} bytes")));
        }
        Ok(Some(bytes))
    }
}

/// A release newer than the running one, verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    version: Version,
    files: Vec<File>,
    source: Source,
}

impl Release {
    pub fn version(&self) -> Version {
        self.version
    }

    /// Where a person can read about this release and download it.
    pub fn page(&self) -> String {
        match &self.source {
            Source::GitHub => format!(
                "{}/releases/tag/v{}",
                env!("CARGO_PKG_REPOSITORY"),
                self.version
            ),
            Source::Web(base) => base.clone(),
            Source::Folder(folder) => folder.display().to_string(),
        }
    }

    /// The download whose name ends in `suffix`, e.g. `windows-x64.zip`.
    fn file(&self, suffix: &str) -> Result<&File, Error> {
        let name = format!("screen-mark-v{}-{suffix}", self.version);
        self.files
            .iter()
            .find(|file| file.name == name)
            .ok_or(Error::NoDownload(name))
    }

    /// Download `file` and check it is the one the manifest describes.
    fn fetch(&self, file: &File) -> Result<Vec<u8>, Error> {
        let mismatch = |why: String| Error::Mismatch {
            name: file.name.clone(),
            why,
        };
        if file.size > MAX_DOWNLOAD {
            return Err(mismatch(format!("{} bytes is too large", file.size)));
        }
        let bytes = self
            .source
            .read(&file.name, Some(self.version), file.size)?
            .ok_or_else(|| {
                Error::Fetch(format!(
                    "{} is missing",
                    self.source.place(&file.name, Some(self.version))
                ))
            })?;
        if bytes.len() as u64 != file.size {
            return Err(mismatch(format!(
                "it is {} bytes, not {}",
                bytes.len(),
                file.size
            )));
        }
        let digest = ring::digest::digest(&ring::digest::SHA256, &bytes);
        if digest.as_ref() != file.sha256 {
            return Err(mismatch(format!(
                "its SHA-256 is {}",
                manifest::to_hex(digest.as_ref())
            )));
        }
        Ok(bytes)
    }
}

/// What `check` found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    /// The newest release is `latest`, and it is not newer than this one.
    UpToDate {
        latest: Version,
    },
    Available(Release),
}

/// Look for a release newer than `running`, signed with one of `keys`.
/// The app passes `Version::running()` and `keys::trusted()`.
pub fn check(source: &Source, running: Version, keys: &[[u8; 32]]) -> Result<Check, Error> {
    let manifest = source
        .read(MANIFEST, None, MAX_MANIFEST)?
        .ok_or_else(|| Error::NoManifest(source.place(MANIFEST, None)))?;
    // A manifest with no signature beside it is as good as unsigned.
    let signature = source
        .read(SIGNATURE, None, MAX_SIGNATURE)?
        .ok_or(Error::Signature)?;
    manifest::verify(&manifest, &signature, keys)?;

    let Manifest { version, files } = Manifest::parse(&manifest)?;
    if version <= running {
        return Ok(Check::UpToDate { latest: version });
    }
    Ok(Check::Available(Release {
        version,
        files,
        source: source.clone(),
    }))
}
