//! The walk (spec §2.1): TES4, then top-level groups; a group whose label is not a wanted type is skipped by its
//! size, and so is every nested group inside a wanted one.
use crate::{records, PluginFacts, Rec, FLAG_COMPRESSED};
use std::collections::BTreeSet;
use std::io::Read;

/// Record and group headers are both 24 bytes.
pub(crate) const HEADER: usize = 24;

/// Nested groups entered under a CELL/WRLD top group; valid files use at most 5.
const MAX_DEPTH: usize = 8;

/// Largest decompressed record accepted (the size field comes from the file).
const MAX_INFLATED: usize = 64 << 20;

pub(crate) fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

pub(crate) fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

pub(crate) fn sig_at(b: &[u8], o: usize) -> [u8; 4] {
    [b[o], b[o + 1], b[o + 2], b[o + 3]]
}

fn sig_str(s: &[u8; 4]) -> String {
    String::from_utf8_lossy(s).into_owned()
}

/// Calls `f` for every subrecord of a record body (6-byte header; `XXXX` sets the next subrecord's u32 size).
/// `Err(offset)`: the subrecord at that offset (in `data`) runs past the end; the ones before it were delivered.
pub(crate) fn each_sub<'a>(
    data: &'a [u8],
    mut f: impl FnMut([u8; 4], &'a [u8]),
) -> Result<(), usize> {
    let mut p = 0;
    let mut big: Option<usize> = None;
    let mut xxxx_at = 0;
    while p + 6 <= data.len() {
        let sig = sig_at(data, p);
        let n = big.take().unwrap_or(u16_at(data, p + 4) as usize);
        let start = p + 6;
        if start + n > data.len() {
            return Err(p);
        }
        if &sig == b"XXXX" {
            if n < 4 {
                return Err(p);
            }
            big = Some(u32_at(data, start) as usize);
            xxxx_at = p;
        } else {
            f(sig, &data[start..start + n]);
        }
        p = start + n;
    }
    if big.is_some() {
        return Err(xxxx_at); // XXXX with no subrecord after it
    }
    if p != data.len() {
        return Err(p);
    }
    Ok(())
}

/// One plugin's facts. Never panics on bad input; faults go to `errors` with their file offset.
pub fn parse(name: &str, b: &[u8]) -> PluginFacts {
    let mut f = PluginFacts {
        name: name.to_string(),
        ..Default::default()
    };
    if b.len() < HEADER || &b[0..4] != b"TES4" {
        f.errors
            .push("offset 0x0: not a plugin (no TES4 header)".into());
        return f;
    }
    let size = u32_at(b, 4) as usize;
    f.flags = u32_at(b, 8);
    let end = HEADER + size;
    if end > b.len() {
        f.errors.push(format!(
            "offset 0x0: TES4 header size {size} runs past the end of the file"
        ));
        return f;
    }
    let mut masters = vec![];
    if let Err(at) = each_sub(&b[HEADER..end], |sig, d| {
        if &sig == b"MAST" {
            if let Some(m) = records::zstring(d) {
                masters.push(m);
            }
        }
    }) {
        f.errors.push(format!(
            "offset {:#x}: TES4 subrecord runs past the header",
            HEADER + at
        ));
    }
    f.masters = masters;
    let mut bases = BTreeSet::new();
    let mut off = end;
    while off < b.len() {
        if off + HEADER > b.len() {
            f.errors
                .push(format!("offset {off:#x}: {} trailing bytes", b.len() - off));
            break;
        }
        let sig = sig_at(b, off);
        let size = u32_at(b, off + 4) as usize;
        if &sig != b"GRUP" {
            f.errors.push(format!(
                "offset {off:#x}: {} record outside a group",
                sig_str(&sig)
            ));
            off = off.saturating_add(HEADER + size);
            continue;
        }
        if size < HEADER {
            f.errors.push(format!(
                "offset {off:#x}: group size {size} is smaller than its header"
            ));
            break;
        }
        let label = sig_at(b, off + 8);
        let wanted = u32_at(b, off + 12) == 0 && records::wanted(&label);
        let placed = u32_at(b, off + 12) == 0 && (&label == b"CELL" || &label == b"WRLD");
        let gend = off + size;
        if gend > b.len() {
            f.errors.push(format!(
                "offset {off:#x}: group {} runs past the end of the file",
                sig_str(&label)
            ));
            if wanted {
                walk_group(b, off + HEADER, b.len(), &mut f);
            } else if placed {
                walk_placed(b, off + HEADER, b.len(), 0, &mut bases, &mut f);
            }
            break;
        }
        if wanted {
            walk_group(b, off + HEADER, gend, &mut f);
        } else if placed {
            walk_placed(b, off + HEADER, gend, 0, &mut bases, &mut f);
        }
        off = gend;
    }
    f.placed = bases.into_iter().collect();
    f
}

/// The records of one wanted top-level group, `start..end`. A fault ends this group, never the plugin.
fn walk_group(b: &[u8], start: usize, end: usize, f: &mut PluginFacts) {
    let mut p = start;
    while p < end {
        if p + HEADER > end {
            f.errors.push(format!(
                "offset {p:#x}: truncated header runs past its group"
            ));
            return;
        }
        let sig = sig_at(b, p);
        let size = u32_at(b, p + 4) as usize;
        if &sig == b"GRUP" {
            // CELL/WRLD children and any other nested group are not wanted (spec §2.1).
            if size < HEADER || p + size > end {
                f.errors.push(format!(
                    "offset {p:#x}: nested group size {size} runs past its group"
                ));
                return;
            }
            p += size;
            continue;
        }
        let rend = p + HEADER + size;
        if rend > end {
            f.errors.push(format!(
                "offset {p:#x}: {} record size {size} runs past its group",
                sig_str(&sig)
            ));
            return;
        }
        read_record(b, p, rend, f);
        p = rend;
    }
}

/// One record at `p..rend` of a group: inflated if needed, read into `f.records`; a fault is an error, never a panic.
fn read_record(b: &[u8], p: usize, rend: usize, f: &mut PluginFacts) {
    let sig = sig_at(b, p);
    let flags = u32_at(b, p + 8);
    let formid = u32_at(b, p + 12);
    let raw = &b[p + HEADER..rend];
    let inflated;
    let data: &[u8] = if flags & FLAG_COMPRESSED != 0 {
        match inflate(raw) {
            Ok(v) => {
                inflated = v;
                &inflated
            }
            Err(e) => {
                f.errors.push(format!(
                    "offset {p:#x}: {} {formid:08X}: {e}",
                    sig_str(&sig)
                ));
                return;
            }
        }
    } else {
        raw
    };
    let mut rec = Rec {
        local_formid: formid,
        sig,
        edid: None,
        payload: Default::default(),
    };
    if let Err(at) = records::read(&mut rec, data) {
        f.errors.push(format!(
            "offset {p:#x}: {} {formid:08X}: subrecord at +{at:#x} runs past the record",
            sig_str(&sig)
        ));
    }
    f.records.push(rec);
}

/// A CELL/WRLD group, `start..end` (filtering spec §3): every nested group is entered; of the records only REFR and
/// ACHR are read, for their base FormID (NAME); every other record is skipped by its size. A fault ends this group.
fn walk_placed(
    b: &[u8],
    start: usize,
    end: usize,
    depth: usize,
    bases: &mut BTreeSet<u32>,
    f: &mut PluginFacts,
) {
    let mut p = start;
    while p < end {
        if p + HEADER > end {
            f.errors.push(format!(
                "offset {p:#x}: truncated header runs past its group"
            ));
            return;
        }
        let sig = sig_at(b, p);
        let size = u32_at(b, p + 4) as usize;
        if &sig == b"GRUP" {
            if size < HEADER || p + size > end {
                f.errors.push(format!(
                    "offset {p:#x}: nested group size {size} runs past its group"
                ));
                return;
            }
            if depth >= MAX_DEPTH {
                f.errors.push(format!(
                    "offset {p:#x}: groups nested deeper than {MAX_DEPTH}"
                ));
                return;
            }
            walk_placed(b, p + HEADER, p + size, depth + 1, bases, f);
            p += size;
            continue;
        }
        let rend = p + HEADER + size;
        if rend > end {
            f.errors.push(format!(
                "offset {p:#x}: {} record size {size} runs past its group",
                sig_str(&sig)
            ));
            return;
        }
        if &sig == b"WRLD" {
            // The worldspace record itself carries a cloud model (xEdit `wbRecord(WRLD…)`).
            read_record(b, p, rend, f);
        } else if &sig == b"REFR" || &sig == b"ACHR" {
            let formid = u32_at(b, p + 12);
            let raw = &b[p + HEADER..rend];
            let inflated;
            let data: &[u8] = if u32_at(b, p + 8) & FLAG_COMPRESSED != 0 {
                match inflate(raw) {
                    Ok(v) => {
                        inflated = v;
                        &inflated
                    }
                    Err(e) => {
                        f.errors.push(format!(
                            "offset {p:#x}: {} {formid:08X}: {e}",
                            sig_str(&sig)
                        ));
                        p = rend;
                        continue;
                    }
                }
            } else {
                raw
            };
            let mut base = None;
            if let Err(at) = each_sub(data, |s, d| {
                if &s == b"NAME" && d.len() >= 4 && base.is_none() {
                    base = Some(u32_at(d, 0));
                }
            }) {
                f.errors.push(format!(
                    "offset {p:#x}: {} {formid:08X}: subrecord at +{at:#x} runs past the record",
                    sig_str(&sig)
                ));
            }
            if let Some(id) = base.filter(|&id| id != 0) {
                bases.insert(id);
            }
        }
        p = rend;
    }
}

/// u32 decompressed size + zlib stream → the record data.
pub(crate) fn inflate(raw: &[u8]) -> Result<Vec<u8>, String> {
    if raw.len() < 4 {
        return Err("compressed record shorter than its size field".into());
    }
    let want = u32_at(raw, 0) as usize;
    if want > MAX_INFLATED {
        return Err(format!(
            "bad zlib stream: declared size {want} is over the {MAX_INFLATED} cap"
        ));
    }
    // Capacity follows the compressed size, not the header: a lying header cannot make a big allocation up front.
    let mut out = Vec::with_capacity(want.min(raw.len().saturating_mul(8)));
    // take(want + 1): a stream that inflates past its declared size stops there and is an error, not an OOM.
    flate2::read::ZlibDecoder::new(&raw[4..])
        .take(want as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|e| format!("bad zlib stream: {e}"))?;
    if out.len() != want {
        // Longer than declared: the read stopped at want + 1, so the true size is unknown.
        let or_more = if out.len() > want { " or more" } else { "" };
        return Err(format!(
            "bad zlib stream: inflates to {} bytes{or_more}, header says {want}",
            out.len()
        ));
    }
    Ok(out)
}
