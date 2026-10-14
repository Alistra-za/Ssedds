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
    /// Uncompressed size as the central directory claims it.
    size: u64,
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
                size: u32_at(&cd, i + 24).into(),
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

/// Most bytes a whole `mergedimage.png` may inflate to (DECISIONS AF2): a 16384 px RGBA8 image stored raw
/// (`MAX_DIM`² × 4 = 1 GiB) plus 64 MiB for PNG filter bytes, chunk framing and ancillary chunks.
pub(crate) const MERGED_CAP: u64 =
    crate::load::MAX_DIM as u64 * crate::load::MAX_DIM as u64 * 4 + (64 << 20);

/// Room for ancillary chunks (an ICC profile, text) on top of a merged image's pixel data.
const MERGED_SLACK: u64 = 4 << 20;

/// The most a `w` × `h` `mergedimage.png` can need (DECISIONS AF2): its pixel data stored raw at 16 bits per
/// channel RGBA with a filter byte per row (`h·(1 + 8w)`), 1/64 for zlib and chunk framing, `MERGED_SLACK`; never
/// above `MERGED_CAP`.
pub(crate) fn merged_cap(w: u32, h: u32) -> u64 {
    let raw = u64::from(h) * (1 + 8 * u64::from(w));
    (raw + raw / 64 + MERGED_SLACK).min(MERGED_CAP)
}

/// The first `limit` bytes of `mergedimage.png` in the archive at `path` (the IHDR read).
pub(crate) fn merged_png(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let mut out = vec![];
    entry_reader(path)?
        .1
        .take(limit)
        .read_to_end(&mut out)
        .map_err(|e| format!("corrupt mergedimage.png: {e}"))?;
    Ok(out)
}

/// The whole `mergedimage.png`, refused past `cap` bytes (a zip bomb): a central-directory claim above it before
/// anything is inflated, and an entry that lies after at most `cap` bytes. The buffer grows from the claim and never
/// past `cap`.
pub(crate) fn merged_png_capped(path: &Path, cap: u64) -> Result<Vec<u8>, String> {
    let too_big = || {
        format!("mergedimage.png is larger than its size in the PNG header allows ({cap} bytes; a corrupt or hostile archive)")
    };
    let (claim, mut r) = entry_reader(path)?;
    if claim > cap {
        return Err(too_big());
    }
    let cap = cap as usize;
    let mut out = Vec::with_capacity(claim as usize);
    let mut buf = vec![0u8; 64 << 10];
    loop {
        let n = r
            .read(&mut buf)
            .map_err(|e| format!("corrupt mergedimage.png: {e}"))?;
        if n == 0 {
            return Ok(out);
        }
        if out.len() + n > cap {
            return Err(too_big());
        }
        if out.capacity() < out.len() + n {
            let want = (out.capacity() * 2).max(out.len() + n).min(cap);
            out.reserve_exact(want - out.len());
        }
        out.extend_from_slice(&buf[..n]);
    }
}

/// The entry's claimed (uncompressed) size and a reader of its inflated bytes.
fn entry_reader(path: &Path) -> Result<(u64, Box<dyn Read>), String> {
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
    let r: Box<dyn Read> = match e.method {
        0 => Box::new(raw),
        8 => Box::new(flate2::read::DeflateDecoder::new(raw)),
        m => return Err(format!("mergedimage.png uses unsupported ZIP method {m}")),
    };
    Ok((e.size, r))
}

/// Width and height of `mergedimage.png` from its IHDR, inflating only the first bytes of the entry.
pub(crate) fn size(path: &Path) -> Result<(u32, u32), String> {
    let b = merged_png(path, 24)?;
    if b.len() < 24 || b[..8] != *b"\x89PNG\r\n\x1a\n" || b[12..16] != *b"IHDR" {
        return Err("mergedimage.png is not a PNG".into());
    }
    let be = |i: usize| u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    Ok((be(16), be(20)))
}

#[cfg(test)]
mod tests {
    /// One stored `mergedimage.png` whose central directory claims `claim` bytes; the data is `data`.
    fn stored(data: &[u8], claim: u32) -> Vec<u8> {
        let name = super::MERGED;
        let mut z = b"PK\x03\x04".to_vec();
        z.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]); // version, flags, method 0, time, date, crc
        z.extend_from_slice(&(data.len() as u32).to_le_bytes());
        z.extend_from_slice(&claim.to_le_bytes());
        z.extend_from_slice(&(name.len() as u16).to_le_bytes());
        z.extend_from_slice(&[0, 0]);
        z.extend_from_slice(name);
        z.extend_from_slice(data);
        let cd_off = z.len() as u32;
        z.extend_from_slice(b"PK\x01\x02");
        z.extend_from_slice(&[20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]); // made by .. crc
        z.extend_from_slice(&(data.len() as u32).to_le_bytes());
        z.extend_from_slice(&claim.to_le_bytes());
        z.extend_from_slice(&(name.len() as u16).to_le_bytes());
        z.extend_from_slice(&[0; 12]);
        z.extend_from_slice(&0u32.to_le_bytes());
        z.extend_from_slice(name);
        let cd_size = z.len() as u32 - cd_off;
        z.extend_from_slice(b"PK\x05\x06\0\0\0\0\x01\0\x01\0");
        z.extend_from_slice(&cd_size.to_le_bytes());
        z.extend_from_slice(&cd_off.to_le_bytes());
        z.extend_from_slice(&[0, 0]);
        z
    }

    /// An entry that lies about its size is still cut off at the cap (the real one comes from the IHDR; 100 bytes here).
    #[test]
    fn inflation_past_the_cap_fails_even_when_the_entry_claims_less() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("liar.kra");
        std::fs::write(&p, stored(&[7; 1000], 10)).unwrap();
        let e = super::merged_png_capped(&p, 100).unwrap_err();
        assert!(e.contains("larger than"), "{e}");
        // Within the cap it reads.
        assert_eq!(super::merged_png_capped(&p, 1000).unwrap().len(), 1000);
        let v = super::merged_png_capped(&p, 4096).unwrap();
        assert!(
            v.capacity() <= 4096,
            "grows from the claim, never past the cap: {}",
            v.capacity()
        );
        assert_eq!(super::merged_png(&p, 24).unwrap().len(), 24);
    }
}
