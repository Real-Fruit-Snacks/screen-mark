//! The keys a release has to be signed with.
//!
//! Ed25519 public keys, as hex. `latest.json` is trusted when its signature
//! was made with either one; nothing else about a download is trusted.
//!
//! - The first is the release key. The release workflow signs with it, from
//!   the `UPDATE_SIGNING_KEY` secret.
//! - The second is kept offline. If the release key is lost or leaks, a
//!   release signed with this one can still reach every installed copy, and
//!   can carry a new release key.
//!
//! `.github/scripts/update-manifest.sh keygen` makes a key and prints the
//! line to put here. Changing a key here only reaches copies that update
//! with the old keys first, so keys are added before they are removed.

use super::manifest::from_hex;

pub const TRUSTED: [&str; 2] = [
    "18add3888902eae35459c0f2149ac3b046da60475d1b03fa5f032b3cf6502212",
    "78cd540df2009487d668d98dc8e2171f950463439f336be8d3cf3af3bc71228e",
];

/// The trusted keys as bytes.
pub fn trusted() -> Vec<[u8; 32]> {
    TRUSTED.iter().filter_map(|key| from_hex(key)).collect()
}
