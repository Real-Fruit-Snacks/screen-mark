//! Reading one file out of a release archive: the Windows zip or the Linux
//! tar.gz. Just enough of each format for that, so no archive crate is
//! needed.
//!
//! Both are only called on bytes whose SHA-256 matched a signed manifest,
//! so the archive is one the release workflow made. The checks here are
//! against a damaged or surprising file, not a hostile one.

use std::io::Read;

use super::{Error, MAX_DOWNLOAD};

fn bad(what: &str, why: &str) -> Error {
    Error::Archive(format!("the {what} can't be read: {why}"))
}

fn u16_at(bytes: &[u8], at: usize) -> Option<usize> {
    let pair = bytes.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([pair[0], pair[1]]) as usize)
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let four = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([four[0], four[1], four[2], four[3]]))
}

/// The file called `wanted` at the top of a zip.
pub fn from_zip(zip: &[u8], wanted: &str) -> Result<Vec<u8>, Error> {
    let bad = |why: &str| bad("zip", why);
    let cut = || bad("it is cut short");

    // The directory is at the end: a 22-byte record, then a comment of up
    // to 65535 bytes.
    let last = zip.len().checked_sub(22).ok_or_else(cut)?;
    let end = (0..=last)
        .rev()
        .take(65_536)
        .find(|&at| u32_at(zip, at) == Some(0x0605_4b50))
        .ok_or_else(|| bad("it has no directory"))?;
    let count = u16_at(zip, end + 10).ok_or_else(cut)?;
    let mut at = u32_at(zip, end + 16).ok_or_else(cut)? as usize;

    for _ in 0..count {
        if u32_at(zip, at) != Some(0x0201_4b50) {
            return Err(bad("its directory is damaged"));
        }
        let flags = u16_at(zip, at + 8).ok_or_else(cut)?;
        let method = u16_at(zip, at + 10).ok_or_else(cut)?;
        let crc = u32_at(zip, at + 16).ok_or_else(cut)?;
        let packed = u32_at(zip, at + 20).ok_or_else(cut)?;
        let size = u32_at(zip, at + 24).ok_or_else(cut)?;
        let name_len = u16_at(zip, at + 28).ok_or_else(cut)?;
        let extra_len = u16_at(zip, at + 30).ok_or_else(cut)?;
        let comment_len = u16_at(zip, at + 32).ok_or_else(cut)?;
        let local = u32_at(zip, at + 42).ok_or_else(cut)?;
        let name = zip.get(at + 46..at + 46 + name_len).ok_or_else(cut)?;
        at += 46 + name_len + extra_len + comment_len;
        if name != wanted.as_bytes() {
            continue;
        }

        if flags & 1 != 0 {
            return Err(bad("it is encrypted"));
        }
        // 0xffffffff means "see the zip64 record": a file over 4 GB.
        if [packed, size, local].contains(&u32::MAX) || u64::from(size) > MAX_DOWNLOAD {
            return Err(bad("it is too large"));
        }
        let (packed, size, local) = (packed as usize, size as usize, local as usize);
        // The sizes come from the directory; the copy in front of the data
        // may be blank (written before the sizes were known).
        if u32_at(zip, local) != Some(0x0403_4b50) {
            return Err(bad("its directory points at nothing"));
        }
        let start = local
            + 30
            + u16_at(zip, local + 26).ok_or_else(cut)?
            + u16_at(zip, local + 28).ok_or_else(cut)?;
        let data = zip.get(start..start + packed).ok_or_else(cut)?;

        let mut out = Vec::with_capacity(size);
        match method {
            0 => out.extend_from_slice(data),
            8 => {
                flate2::read::DeflateDecoder::new(data)
                    .take(size as u64 + 1)
                    .read_to_end(&mut out)
                    .map_err(|e| bad(&e.to_string()))?;
            }
            _ => return Err(bad("it is packed in a way screen-mark doesn't read")),
        }
        let mut sum = flate2::Crc::new();
        sum.update(&out);
        if out.len() != size || sum.sum() != crc {
            return Err(bad(&format!("{wanted} is damaged")));
        }
        return Ok(out);
    }
    Err(Error::Archive(format!("{wanted} isn't in the zip")))
}

/// The file called `wanted` at the top of a gzipped tar.
pub fn from_tar_gz(archive: &[u8], wanted: &str) -> Result<Vec<u8>, Error> {
    let bad = |why: &str| bad("tar.gz", why);

    let mut tar = Vec::new();
    flate2::read::GzDecoder::new(archive)
        .take(MAX_DOWNLOAD + 1)
        .read_to_end(&mut tar)
        .map_err(|e| bad(&e.to_string()))?;
    if tar.len() as u64 > MAX_DOWNLOAD {
        return Err(bad("it is too large"));
    }

    // A tar is 512-byte blocks: a header, then the file padded to a block.
    let text = |field: &[u8]| {
        let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
        String::from_utf8_lossy(&field[..end]).into_owned()
    };
    let mut at = 0usize;
    while let Some(header) = tar.get(at..at + 512) {
        if header.iter().all(|&b| b == 0) {
            break;
        }
        let size = usize::from_str_radix(text(&header[124..136]).trim(), 8)
            .map_err(|_| bad("a size in it isn't a number"))?;
        let data = at + 512;
        at = size
            .checked_next_multiple_of(512)
            .and_then(|padded| data.checked_add(padded))
            .ok_or_else(|| bad("it is damaged"))?;

        let (name, folder) = (text(&header[..100]), text(&header[345..500]));
        let name = if folder.is_empty() {
            name
        } else {
            format!("{folder}/{name}")
        };
        let is_file = matches!(header[156], b'0' | 0);
        if is_file && name.strip_prefix("./").unwrap_or(&name) == wanted {
            return tar
                .get(data..data + size)
                .map(<[u8]>::to_vec)
                .ok_or_else(|| bad("it is cut short"));
        }
    }
    Err(Error::Archive(format!("{wanted} isn't in the tar.gz")))
}
