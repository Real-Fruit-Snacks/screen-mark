//! `latest.json`: what a release holds, and the check that it is genuine.
//!
//! ```json
//! {
//!   "name": "screen-mark",
//!   "version": "2.3.0",
//!   "files": [
//!     { "name": "screen-mark-v2.3.0-windows-x64.zip", "size": 7340032, "sha256": "..." }
//!   ]
//! }
//! ```
//!
//! `latest.json.sig` holds Ed25519 signatures of those exact bytes, one per
//! line, as hex. The file is checked against them before it is parsed.

use ring::signature::{UnparsedPublicKey, ED25519};
use serde::Deserialize;

use super::{Error, Version};

/// The `name` every manifest carries, so a signature made for another
/// product with the same key is no use here.
pub const PRODUCT: &str = "screen-mark";

/// One download of a release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    pub name: String,
    pub size: u64,
    pub sha256: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub version: Version,
    pub files: Vec<File>,
}

#[derive(Deserialize)]
struct Raw {
    name: String,
    version: String,
    files: Vec<RawFile>,
}

#[derive(Deserialize)]
struct RawFile {
    name: String,
    size: u64,
    sha256: String,
}

/// Whether one of `signatures` (the text of `latest.json.sig`) is a
/// signature of `manifest` by one of `keys`.
pub fn verify(manifest: &[u8], signatures: &[u8], keys: &[[u8; 32]]) -> Result<(), Error> {
    let text = std::str::from_utf8(signatures).map_err(|_| Error::Signature)?;
    for line in text.lines() {
        let Some(signature) = from_hex::<64>(line.trim()) else {
            continue;
        };
        for key in keys {
            if UnparsedPublicKey::new(&ED25519, key)
                .verify(manifest, &signature)
                .is_ok()
            {
                return Ok(());
            }
        }
    }
    Err(Error::Signature)
}

impl Manifest {
    /// Read a manifest. Call `verify` first: this only checks that it makes
    /// sense, not where it came from.
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        let raw: Raw = serde_json::from_slice(bytes).map_err(|e| Error::Manifest(e.to_string()))?;
        if raw.name != PRODUCT {
            return Err(Error::Manifest(format!(
                "it is for \"{}\", not {PRODUCT}",
                raw.name
            )));
        }
        let version = Version::parse(&raw.version)
            .ok_or_else(|| Error::Manifest(format!("\"{}\" is not a version", raw.version)))?;
        let mut files = Vec::with_capacity(raw.files.len());
        for file in raw.files {
            // A name is joined to a URL or a folder, so it has to be just a name.
            if !plain_name(&file.name) {
                return Err(Error::Manifest(format!(
                    "\"{}\" is not a plain file name",
                    file.name
                )));
            }
            let sha256 = from_hex(&file.sha256).ok_or_else(|| {
                Error::Manifest(format!("the SHA-256 of {} is not 64 hex digits", file.name))
            })?;
            files.push(File {
                name: file.name,
                size: file.size,
                sha256,
            });
        }
        Ok(Self { version, files })
    }
}

fn plain_name(name: &str) -> bool {
    (1..=128).contains(&name.len())
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// `N` bytes from `2 * N` hex digits.
pub fn from_hex<const N: usize>(text: &str) -> Option<[u8; N]> {
    let (pairs, rest) = text.as_bytes().as_chunks::<2>();
    if pairs.len() != N || !rest.is_empty() {
        return None;
    }
    let mut out = [0u8; N];
    for (byte, [high, low]) in out.iter_mut().zip(pairs) {
        let high = (*high as char).to_digit(16)?;
        let low = (*low as char).to_digit(16)?;
        *byte = (high * 16 + low) as u8;
    }
    Some(out)
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
