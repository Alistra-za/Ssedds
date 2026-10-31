//! The walk (spec §2.1): TES4, then top-level groups; a group whose label is not a wanted type is skipped by its
//! size, and so is every nested group inside a wanted one.
use crate::{records, PluginFacts, Rec, FLAG_COMPRESSED};
use std::borrow::Cow;
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

/// One top-level group: `start..end` is its body (the end is the end of the file for a group that claims more).
pub(crate) struct TopGroup {
    pub label: [u8; 4],
    /// Group type 0 (a top-level group proper).
    pub top: bool,
    pub start: usize,
    pub end: usize,
}

/// The top-level items from `from` (the end of the TES4 record): every group, and a fault `Err("offset 0x…: message")`
/// for each thing that is not one. A group that claims to run past the file is reported, then delivered clipped to
/// the file, and ends the walk; so does a group smaller than its header. Never panics.
pub(crate) fn top_groups(b: &[u8], from: usize, mut f: impl FnMut(Result<TopGroup, String>)) {
    let mut off = from;
    while off < b.len() {
        if off + HEADER > b.len() {
            f(Err(format!(
                "offset {off:#x}: {} trailing bytes",
                b.len() - off
            )));
            break;
        }
        let sig = sig_at(b, off);
        let size = u32_at(b, off + 4) as usize;
        if &sig != b"GRUP" {
            f(Err(format!(
                "offset {off:#x}: {} record outside a group",
                sig_str(&sig)
            )));
            off = off.saturating_add(HEADER + size);
            continue;
        }
        if size < HEADER {
            f(Err(format!(
                "offset {off:#x}: group size {size} is smaller than its header"
            )));
            break;
        }
        let label = sig_at(b, off + 8);
        let top = u32_at(b, off + 12) == 0;
        let gend = off + size;
        if gend > b.len() {
            f(Err(format!(
                "offset {off:#x}: group {} runs past the end of the file",
                sig_str(&label)
            )));
            f(Ok(TopGroup {
                label,
                top,
                start: off + HEADER,
                end: b.len(),
            }));
            break;
        }
        f(Ok(TopGroup {
            label,
            top,
            start: off + HEADER,
            end: gend,
        }));
        off = gend;
    }
}

/// The records of one group body, `start..end`, as `Ok((record start, record end))` (header included). A nested group is
/// skipped by its size, or entered (up to `MAX_DEPTH` levels) when `enter_nested`. A fault is delivered as
/// `Err("offset 0x…: message")` and ends the group being walked (not its parent). Never panics.
pub(crate) fn group_records(
    b: &[u8],
    start: usize,
    end: usize,
    depth: usize,
    enter_nested: bool,
    f: &mut impl FnMut(Result<(usize, usize), String>),
) {
    let mut p = start;
    while p < end {
        if p + HEADER > end {
            f(Err(format!(
                "offset {p:#x}: truncated header runs past its group"
            )));
            return;
        }
        let sig = sig_at(b, p);
        let size = u32_at(b, p + 4) as usize;
        if &sig == b"GRUP" {
            if size < HEADER || p + size > end {
                f(Err(format!(
                    "offset {p:#x}: nested group size {size} runs past its group"
                )));
                return;
            }
            if enter_nested {
                if depth >= MAX_DEPTH {
                    f(Err(format!(
                        "offset {p:#x}: groups nested deeper than {MAX_DEPTH}"
                    )));
                    return;
                }
                group_records(b, p + HEADER, p + size, depth + 1, true, f);
            }
            p += size;
            continue;
        }
        let rend = p + HEADER + size;
        if rend > end {
            f(Err(format!(
                "offset {p:#x}: {} record size {size} runs past its group",
                sig_str(&sig)
            )));
            return;
        }
        f(Ok((p, rend)));
        p = rend;
    }
}

/// The data (subrecords) of the record at `p..rend`: inflated when flagged. The error names the record.
pub(crate) fn record_data(b: &[u8], p: usize, rend: usize) -> Result<Cow<'_, [u8]>, String> {
    let raw = &b[p + HEADER..rend];
    if u32_at(b, p + 8) & FLAG_COMPRESSED == 0 {
        return Ok(Cow::Borrowed(raw));
    }
    inflate(raw).map(Cow::Owned).map_err(|e| {
        format!(
            "offset {p:#x}: {} {:08X}: {e}",
            sig_str(&sig_at(b, p)),
            u32_at(b, p + 12)
        )
    })
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
    top_groups(b, end, |g| match g {
        Err(e) => f.errors.push(e),
        Ok(g) => {
            let wanted = g.top && records::wanted(&g.label);
            let placed = g.top && (&g.label == b"CELL" || &g.label == b"WRLD");
            if wanted {
                walk_group(b, g.start, g.end, &mut f);
            } else if placed {
                walk_placed(b, g.start, g.end, &mut bases, &mut f);
            }
        }
    });
    f.placed = bases.into_iter().collect();
    f
}

/// The records of one wanted top-level group, `start..end`. A fault ends this group, never the plugin.
fn walk_group(b: &[u8], start: usize, end: usize, f: &mut PluginFacts) {
    // CELL/WRLD children and any other nested group are not wanted (spec §2.1): skipped by size.
    group_records(b, start, end, 0, false, &mut |r| match r {
        Ok((p, rend)) => read_record(b, p, rend, f),
        Err(e) => f.errors.push(e),
    });
}

/// One record at `p..rend` of a group: inflated if needed, read into `f.records`; a fault is an error, never a panic.
fn read_record(b: &[u8], p: usize, rend: usize, f: &mut PluginFacts) {
    let sig = sig_at(b, p);
    let formid = u32_at(b, p + 12);
    let data = match record_data(b, p, rend) {
        Ok(d) => d,
        Err(e) => {
            f.errors.push(e);
            return;
        }
    };
    let mut rec = Rec {
        local_formid: formid,
        sig,
        edid: None,
        payload: Default::default(),
    };
    if let Err(at) = records::read(&mut rec, &data) {
        f.errors.push(format!(
            "offset {p:#x}: {} {formid:08X}: subrecord at +{at:#x} runs past the record",
            sig_str(&sig)
        ));
    }
    f.records.push(rec);
}

/// A CELL/WRLD group, `start..end` (filtering spec §3): every nested group is entered; of the records only REFR and
/// ACHR are read, for their base FormID (NAME); every other record is skipped by its size. A fault ends this group.
fn walk_placed(b: &[u8], start: usize, end: usize, bases: &mut BTreeSet<u32>, f: &mut PluginFacts) {
    group_records(b, start, end, 0, true, &mut |r| {
        let (p, rend) = match r {
            Ok(x) => x,
            Err(e) => return f.errors.push(e),
        };
        let sig = sig_at(b, p);
        if &sig == b"WRLD" {
            // The worldspace record itself carries a cloud model (xEdit `wbRecord(WRLD…)`).
            read_record(b, p, rend, f);
        } else if &sig == b"REFR" || &sig == b"ACHR" {
            let data = match record_data(b, p, rend) {
                Ok(d) => d,
                Err(e) => return f.errors.push(e),
            };
            let mut base = None;
            if let Err(at) = each_sub(&data, |s, d| {
                if &s == b"NAME" && d.len() >= 4 && base.is_none() {
                    base = Some(u32_at(d, 0));
                }
            }) {
                f.errors.push(format!(
                    "offset {p:#x}: {} {:08X}: subrecord at +{at:#x} runs past the record",
                    sig_str(&sig),
                    u32_at(b, p + 12)
                ));
            }
            if let Some(id) = base.filter(|&id| id != 0) {
                bases.insert(id);
            }
        }
    });
}

/// u32 decompressed size + zlib stream → the record data.
fn inflate(raw: &[u8]) -> Result<Vec<u8>, String> {
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
