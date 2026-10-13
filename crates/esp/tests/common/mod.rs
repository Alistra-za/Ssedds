//! Plugin byte builder for tests (esp spec §6: fixtures are built in code, never binary blobs). The modscan tests
//! include this file with `#[path]`.
#![allow(dead_code)]

/// A subrecord. Data over 0xFFFF bytes is preceded by an `XXXX` subrecord carrying its u32 size, and its own u16
/// size is 0.
pub fn sub(sig: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut v = vec![];
    if data.len() > 0xFFFF {
        v.extend_from_slice(b"XXXX");
        v.extend_from_slice(&4u16.to_le_bytes());
        v.extend_from_slice(&(data.len() as u32).to_le_bytes());
        v.extend_from_slice(sig);
        v.extend_from_slice(&0u16.to_le_bytes());
    } else {
        v.extend_from_slice(sig);
        v.extend_from_slice(&(data.len() as u16).to_le_bytes());
    }
    v.extend_from_slice(data);
    v
}

/// A NUL-terminated string.
pub fn zstr(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    v
}

pub fn edid(s: &str) -> Vec<u8> {
    sub(b"EDID", &zstr(s))
}

/// A FormID subrecord.
pub fn form(sig: &[u8; 4], id: u32) -> Vec<u8> {
    sub(sig, &id.to_le_bytes())
}

/// Alternate-texture data (`MODS`, `MO2S`…): u32 count, then per entry a u32 name length, the name (no terminator),
/// the TXST FormID (u32) and the 3D index (s32).
pub fn alt(entries: &[(&str, u32, i32)]) -> Vec<u8> {
    let mut v = (entries.len() as u32).to_le_bytes().to_vec();
    for (name, txst, index) in entries {
        v.extend_from_slice(&(name.len() as u32).to_le_bytes());
        v.extend_from_slice(name.as_bytes());
        v.extend_from_slice(&txst.to_le_bytes());
        v.extend_from_slice(&index.to_le_bytes());
    }
    v
}

/// A record with `body` as stored (the caller compresses it when `flags` has 0x40000).
pub fn rec_raw(sig: &[u8; 4], formid: u32, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut v = sig.to_vec();
    v.extend_from_slice(&(body.len() as u32).to_le_bytes());
    v.extend_from_slice(&flags.to_le_bytes());
    v.extend_from_slice(&formid.to_le_bytes());
    v.extend_from_slice(&[0u8; 4]); // version control info
    v.extend_from_slice(&44u16.to_le_bytes()); // form version (SSE)
    v.extend_from_slice(&[0u8; 2]);
    v.extend_from_slice(body);
    v
}

pub fn rec(sig: &[u8; 4], formid: u32, subs: &[Vec<u8>]) -> Vec<u8> {
    rec_raw(sig, formid, 0, &subs.concat())
}

/// A group: 24-byte header (size includes the header), then `children` as given.
pub fn grup(label: &[u8; 4], ty: u32, children: &[Vec<u8>]) -> Vec<u8> {
    let body = children.concat();
    let mut v = b"GRUP".to_vec();
    v.extend_from_slice(&((24 + body.len()) as u32).to_le_bytes());
    v.extend_from_slice(label);
    v.extend_from_slice(&ty.to_le_bytes());
    v.extend_from_slice(&[0u8; 8]);
    v.extend_from_slice(&body);
    v
}

/// A top-level group of record type `label`.
pub fn top(label: &[u8; 4], records: &[Vec<u8>]) -> Vec<u8> {
    grup(label, 0, records)
}

/// TES4 header (`flags`, one `MAST` + `DATA` per master), then `groups`.
pub fn plugin(flags: u32, masters: &[&str], groups: &[Vec<u8>]) -> Vec<u8> {
    let mut subs = vec![sub(b"HEDR", &[0u8; 12])];
    for m in masters {
        subs.push(sub(b"MAST", &zstr(m)));
        subs.push(sub(b"DATA", &[0u8; 8]));
    }
    let mut v = rec_raw(b"TES4", 0, flags, &subs.concat());
    for g in groups {
        v.extend_from_slice(g);
    }
    v
}
