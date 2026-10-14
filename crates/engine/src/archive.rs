//! Krita `.kra` and OpenRaster `.ora` input: both are ZIP archives that store the flattened document as
//! `mergedimage.png` next to the layers. sdc reads that PNG and never the layers, like a PSD's composite (`psd`).
//!
//! A minimal ZIP reader: the end-of-central-directory record, the central directory, then the one entry, stored or
//! deflated (flate2). ZIP64, encrypted entries and other methods are refused with the reason.
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const MERGED: &[u8] = b"mergedimage.png";

/// True when `path` starts with a ZIP local file header. `false` when it cannot be read.
pub(crate) fn sniff(path: &Path) -> bool {
    let mut b = [0u8; 4];
    File::open(path)
        .and_then(|mut f| f.read_exact(&mut b))
        .is_ok_and(|()| &b == b"PK\x03\x04")
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

struct Entry {
    method: u16,
    compressed: u64,
    /// Offset of the entry's local file header.
    local: u64,
}

/// The central directory entry for `mergedimage.png`.
fn find(f: &mut File) -> Result<Entry, String> {
    let len = f.metadata().map_err(|e| e.to_string())?.len();
    // The end record is 22 bytes plus a comment of at most 65535.
    let tail = len.min(22 + 65535);
    f.seek(SeekFrom::Start(len - tail))
        .map_err(|e| e.to_string())?;
    let mut t = vec![0u8; tail as usize];
    f.read_exact(&mut t).map_err(|e| e.to_string())?;
    let eocd = (0..t.len().saturating_sub(21))
        .rev()
        .find(|&i| t[i..i + 4] == *b"PK\x05\x06")
        .ok_or("not a readable ZIP archive (no end record)")?;
    let (count, cd_size, cd_off) = (
        u16_at(&t, eocd + 10),
        u32_at(&t, eocd + 12),
        u32_at(&t, eocd + 16),
    );
    if count == 0xFFFF || cd_size == u32::MAX || cd_off == u32::MAX {
        return Err("ZIP64 archives are not supported".into());
    }
    if u64::from(cd_off) + u64::from(cd_size) > len {
        return Err("corrupt ZIP archive (central directory out of range)".into());
    }
    f.seek(SeekFrom::Start(cd_off.into()))
        .map_err(|e| e.to_string())?;
    let mut cd = vec![0u8; cd_size as usize];
    f.read_exact(&mut cd).map_err(|e| e.to_string())?;
    let mut i = 0;
    while i + 46 <= cd.len() && cd[i..i + 4] == *b"PK\x01\x02" {
        let n = usize::from(u16_at(&cd, i + 28));
        let skip = n + usize::from(u16_at(&cd, i + 30)) + usize::from(u16_at(&cd, i + 32));
        if i + 46 + n > cd.len() {
            break;
        }
        if &cd[i + 46..i + 46 + n] == MERGED {
            if u16_at(&cd, i + 8) & 1 != 0 {
                return Err("mergedimage.png is encrypted".into());
            }
            let (compressed, local) = (u32_at(&cd, i + 20), u32_at(&cd, i + 42));
            if compressed == u32::MAX || local == u32::MAX {
                return Err("ZIP64 archives are not supported".into());
            }
            return Ok(Entry {
                method: u16_at(&cd, i + 10),
                compressed: compressed.into(),
                local: local.into(),
            });
        }
        i += 46 + skip;
    }
    Err(
        "no mergedimage.png (re-save it in Krita, or in an OpenRaster editor that writes one)"
            .into(),
    )
}

/// The bytes of `mergedimage.png` in the archive at `path`. With `limit`, only its first `limit` bytes.
pub(crate) fn merged_png(path: &Path, limit: Option<u64>) -> Result<Vec<u8>, String> {
    let mut f = File::open(path).map_err(|e| e.to_string())?;
    let e = find(&mut f)?;
    let mut h = [0u8; 30];
    f.seek(SeekFrom::Start(e.local))
        .map_err(|e| e.to_string())?;
    f.read_exact(&mut h).map_err(|e| e.to_string())?;
    if h[..4] != *b"PK\x03\x04" {
        return Err("corrupt ZIP archive (bad local header)".into());
    }
    let data = e.local + 30 + u64::from(u16_at(&h, 26)) + u64::from(u16_at(&h, 28));
    f.seek(SeekFrom::Start(data)).map_err(|e| e.to_string())?;
    let raw = f.take(e.compressed);
    let mut out = Vec::new();
    let read = match e.method {
        0 => raw.take(limit.unwrap_or(u64::MAX)).read_to_end(&mut out),
        8 => flate2::read::DeflateDecoder::new(raw)
            .take(limit.unwrap_or(u64::MAX))
            .read_to_end(&mut out),
        m => return Err(format!("mergedimage.png uses unsupported ZIP method {m}")),
    };
    read.map_err(|e| format!("corrupt mergedimage.png: {e}"))?;
    Ok(out)
}

/// Width and height of `mergedimage.png` from its IHDR, inflating only the first bytes of the entry.
pub(crate) fn size(path: &Path) -> Result<(u32, u32), String> {
    let b = merged_png(path, Some(24))?;
    if b.len() < 24 || b[..8] != *b"\x89PNG\r\n\x1a\n" || b[12..16] != *b"IHDR" {
        return Err("mergedimage.png is not a PNG".into());
    }
    let be = |i: usize| u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    Ok((be(16), be(20)))
}
