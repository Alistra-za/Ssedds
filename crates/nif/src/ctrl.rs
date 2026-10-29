//! Read-only view of time controllers, interpolators, NiControllerManager and NiControllerSequence
//! (layouts: nif-edit-v2 01-format sections 3-4). Every entry point validates the block index and sizes first;
//! hostile input is an error, never a panic or an unbounded allocation.
use crate::{header::Header, NifError};
use std::sync::Arc;

/// next i32, flags u16, frequency, phase, start, stop f32, target i32.
pub const BASE_LEN: usize = 26;
pub(crate) const CB_LEN: usize = 29;

pub(crate) fn geo(block: usize, msg: &str) -> NifError {
    NifError::Geometry {
        block: block as u32,
        msg: msg.into(),
    }
}

pub(crate) fn body<'a>(h: &Header, b: &'a [u8], block: usize) -> Result<&'a [u8], NifError> {
    if block >= h.sizes.len() {
        return Err(geo(block, "no such block"));
    }
    let s = h.offsets()[block];
    let at = s.saturating_add(h.sizes[block] as usize);
    b.get(s..at).ok_or(NifError::Truncated {
        at: s,
        need: h.sizes[block] as usize,
    })
}

pub(crate) fn exact(h: &Header, block: usize, want: usize, got: usize) -> Result<(), NifError> {
    if want == got {
        return Ok(());
    }
    Err(NifError::BlockSize {
        block: block as u32,
        kind: crate::text::label(&String::from_utf8_lossy(h.kind(block))).into_owned(),
        declared: got as u32,
        consumed: want as u32,
    })
}

fn u32_at(d: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(d[at..at + 4].try_into().unwrap())
}
fn f32_at(d: &[u8], at: usize) -> f32 {
    f32::from_bits(u32_at(d, at))
}

/// Total block length for the controller classes whose layout is fully known; None otherwise.
pub(crate) fn known_len(kind: &[u8]) -> Option<usize> {
    match kind {
        b"BSLightingShaderPropertyFloatController"
        | b"BSLightingShaderPropertyColorController"
        | b"BSLightingShaderPropertyUShortController"
        | b"BSEffectShaderPropertyFloatController"
        | b"BSEffectShaderPropertyColorController" => Some(BASE_LEN + 8),
        b"BSNiAlphaPropertyTestRefController" | b"NiAlphaController" => Some(BASE_LEN + 4),
        _ => None,
    }
}

/// True for classes with an NiTimeController base.
pub(crate) fn is_ctrl(kind: &[u8]) -> bool {
    kind.ends_with(b"Controller") || kind == b"NiControllerManager"
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimeCtrl {
    pub block: u32,
    pub class: String,
    pub next: i32,
    pub flags: u16,
    pub frequency: f32,
    pub phase: f32,
    pub start: f32,
    pub stop: f32,
    pub target: i32,
    pub interp: Option<i32>,
    pub var: Option<u32>,
    pub known: bool,
}

pub fn read_ctrl(h: &Header, b: &[u8], block: usize) -> Result<TimeCtrl, NifError> {
    let d = body(h, b, block)?;
    let kind = h.kind(block);
    if !is_ctrl(kind) {
        return Err(geo(block, "not a time controller"));
    }
    if d.len() < BASE_LEN {
        return Err(geo(block, "controller shorter than NiTimeController"));
    }
    let known = known_len(kind);
    if let Some(n) = known {
        exact(h, block, n, d.len())?;
    }
    let has_var = known == Some(BASE_LEN + 8);
    Ok(TimeCtrl {
        block: block as u32,
        class: String::from_utf8_lossy(kind).into(),
        next: u32_at(d, 0) as i32,
        flags: u16::from_le_bytes([d[4], d[5]]),
        frequency: f32_at(d, 6),
        phase: f32_at(d, 10),
        start: f32_at(d, 14),
        stop: f32_at(d, 18),
        target: u32_at(d, 22) as i32,
        interp: known.map(|_| u32_at(d, BASE_LEN) as i32),
        var: has_var.then(|| u32_at(d, BASE_LEN + 4)),
        known: known.is_some(),
    })
}

/// Block ids of the controller chain starting at `first`, following `next`.
pub fn chain(h: &Header, b: &[u8], first: i32, problems: &mut Vec<String>) -> Vec<u32> {
    let mut ids: Vec<u32> = vec![];
    let mut at = first;
    while at != -1 {
        if ids.contains(&(at as u32)) {
            problems.push(format!("controller chain loops at block {at}"));
            break;
        }
        if ids.len() >= 256 {
            problems.push("controller chain longer than 256".into());
            break;
        }
        match read_ctrl(h, b, at as usize) {
            Ok(c) => {
                ids.push(at as u32);
                at = c.next;
            }
            Err(e) => {
                problems.push(format!("controller chain stops at block {at}: {e}"));
                break;
            }
        }
    }
    ids
}

#[derive(Debug, Clone, PartialEq)]
pub struct Interp {
    pub block: u32,
    pub class: String,
    pub value: Vec<f32>,
    pub data: i32,
    pub known: bool,
}

pub fn read_interp(h: &Header, b: &[u8], block: usize) -> Result<Interp, NifError> {
    let d = body(h, b, block)?;
    let kind = h.kind(block);
    let n = interp_floats(kind);
    let known = n > 0;
    if known {
        exact(h, block, 4 * n + 4, d.len())?;
    }
    Ok(Interp {
        block: block as u32,
        class: String::from_utf8_lossy(kind).into(),
        value: (0..n).map(|i| f32_at(d, 4 * i)).collect(),
        data: if known { u32_at(d, 4 * n) as i32 } else { -1 },
        known,
    })
}

/// Number of leading pose floats on the interpolator classes with a known layout, else 0.
pub(crate) fn interp_floats(kind: &[u8]) -> usize {
    match kind {
        b"NiFloatInterpolator" => 1,
        b"NiPoint3Interpolator" => 3,
        _ => 0,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BlendInterp {
    pub block: u32,
    pub class: String,
    pub flags: u8,
    pub array_size: u8,
    pub weight_threshold: f32,
    /// Raw bytes from Interp Count through the last item (20 + 17 * array_size) when flags bit 0 is clear; None when set.
    pub items: Option<Vec<u8>>,
    /// 1 value (Float) or 3 (Point3), bit patterns as stored.
    pub value: Vec<f32>,
    /// Block-relative offset of Value.
    pub value_at: usize,
}

/// NiBlendFloatInterpolator or NiBlendPoint3Interpolator; any other class is an Err. Exact size.
/// Layout: flags u8, array size u8, threshold f32, then (bit 0 clear) 20 counter bytes and 17-byte items, then Value.
pub fn read_blend_interp(h: &Header, b: &[u8], block: usize) -> Result<BlendInterp, NifError> {
    let d = body(h, b, block)?;
    let kind = h.kind(block);
    let n = match kind {
        b"NiBlendFloatInterpolator" => 1,
        b"NiBlendPoint3Interpolator" => 3,
        _ => return Err(geo(block, "not a blend interpolator")),
    };
    if d.len() < 6 {
        return Err(geo(
            block,
            "blend interpolator shorter than its fixed fields",
        ));
    }
    let (flags, array_size) = (d[0], d[1]);
    let managed = flags & 1 != 0;
    let value_at = if managed {
        6
    } else {
        26 + 17 * array_size as usize
    };
    // also guards the slices below
    exact(h, block, value_at + 4 * n, d.len())?;
    Ok(BlendInterp {
        block: block as u32,
        class: String::from_utf8_lossy(kind).into(),
        flags,
        array_size,
        weight_threshold: f32_at(d, 2),
        items: (!managed).then(|| d[6..value_at].to_vec()),
        value: (0..n).map(|i| f32_at(d, value_at + 4 * i)).collect(),
        value_at,
    })
}

/// String fields are raw string-table entries: None = index -1, Some(empty) = the empty string. Each is shared with
/// every other reference to the same entry ([`Header::string_arc`]), so many references cannot amplify one string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CtrlBlock {
    pub interp: i32,
    pub controller: i32,
    pub node: Option<Arc<[u8]>>,
    pub property: Option<Arc<[u8]>>,
    pub ctrl_type: Option<Arc<[u8]>>,
    pub ctrl_id: Option<Arc<[u8]>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sequence {
    pub block: u32,
    pub name: Option<Arc<str>>,
    pub blocks: Vec<CtrlBlock>,
    pub manager: i32,
    /// CycleType: 0 loop, 1 reverse, 2 clamp (nif.xml 1022-1027), as stored.
    pub cycle: u32,
    pub frequency: f32,
    pub start: f32,
    pub stop: f32,
    /// The NiTextKeyExtraData the sequence names: (time, raw string or None for -1); empty when there is none or it
    /// does not read.
    pub text_keys: Vec<(f32, Option<Arc<[u8]>>)>,
}

impl Sequence {
    /// `value at time s, …` over the text keys, `(none)` for a null value; at most [`crate::text::DISPLAY_CAP`]
    /// bytes, since many keys can share one large string.
    pub fn text_keys_line(&self) -> String {
        let pieces = self.text_keys.iter().enumerate().flat_map(|(i, (t, v))| {
            let sep: &[u8] = if i == 0 { b"" } else { b", " };
            [
                std::borrow::Cow::Borrowed(sep),
                std::borrow::Cow::Borrowed(v.as_deref().unwrap_or(b"(none)")),
                std::borrow::Cow::Owned(format!(" at {t} s").into_bytes()),
            ]
        });
        crate::text::lossy_capped(pieces, crate::text::DISPLAY_CAP)
    }
}

/// NiTextKeyExtraData `r` (name i32, count u32, then time f32 + value string index i32 per key, nif.xml 5180).
/// The strings are shared ([`Header::string_arc`]), so one large string named by many keys cannot amplify.
fn text_keys(h: &Header, b: &[u8], r: i32) -> Vec<(f32, Option<Arc<[u8]>>)> {
    // shortcut: a malformed text-key block reads as none; the sequence itself still reads.
    let Ok(r) = usize::try_from(r) else {
        return vec![];
    };
    if r >= h.sizes.len() || h.kind(r) != b"NiTextKeyExtraData" {
        return vec![];
    }
    let Ok(t) = body(h, b, r) else { return vec![] };
    if t.len() < 8 {
        return vec![];
    }
    let n = u32_at(t, 4) as usize;
    // the length check bounds n by the block before anything is allocated
    if n.checked_mul(8).and_then(|x| x.checked_add(8)) != Some(t.len()) {
        return vec![];
    }
    (0..n)
        .map(|k| {
            let s = h.string_arc(u32_at(t, 12 + 8 * k) as i32);
            (f32_at(t, 8 + 8 * k), s)
        })
        .collect()
}

/// One string-table entry, shared ([`Header::string_arc`]); -1 is None, any other index outside the table an error.
fn string(h: &Header, block: usize, idx: i32, what: &str) -> Result<Option<Arc<[u8]>>, NifError> {
    if idx == -1 {
        return Ok(None);
    }
    h.string_arc(idx).map(Some).ok_or_else(|| {
        geo(
            block,
            &format!("{what}: string index outside the string table"),
        )
    })
}

pub fn read_sequence(h: &Header, b: &[u8], block: usize) -> Result<Sequence, NifError> {
    let d = body(h, b, block)?;
    if h.kind(block) != b"NiControllerSequence" {
        return Err(geo(block, "not a NiControllerSequence"));
    }
    // name, count, grow-by (12), CB[], then weight, text keys, cycle, freq, start, stop (24),
    // manager, accum root name (8) and the note-array count u16
    if d.len() < 12 + 34 {
        return Err(geo(block, "sequence shorter than its fixed fields"));
    }
    let n = u32_at(d, 4) as usize;
    let tail = n
        .checked_mul(CB_LEN)
        .and_then(|c| c.checked_add(12))
        .filter(|&t| t + 34 <= d.len())
        .ok_or_else(|| geo(block, "controlled-block count runs past the block"))?;
    let notes = u16::from_le_bytes([d[tail + 32], d[tail + 33]]) as usize;
    exact(h, block, tail + 34 + 4 * notes, d.len())?;
    let mut blocks = Vec::with_capacity(n);
    for i in 0..n {
        let c = &d[12 + i * CB_LEN..];
        let s = |k: usize| {
            let what = format!("controlled block {i}");
            string(h, block, u32_at(c, 9 + 4 * k) as i32, &what)
        };
        blocks.push(CtrlBlock {
            interp: u32_at(c, 0) as i32,
            controller: u32_at(c, 4) as i32,
            node: s(0)?,
            property: s(1)?,
            ctrl_type: s(2)?,
            ctrl_id: s(3)?,
        });
    }
    // fix round 1 (Note CU): Windows-1252, as the editor shows and writes it; one byte, one char, so it is an identity
    let name = match u32_at(d, 0) as i32 {
        -1 => None,
        i => Some(
            h.name_arc(i)
                .ok_or_else(|| geo(block, "name: string index outside the string table"))?,
        ),
    };
    Ok(Sequence {
        block: block as u32,
        name,
        blocks,
        manager: u32_at(d, tail + 24) as i32,
        cycle: u32_at(d, tail + 8),
        frequency: f32_at(d, tail + 12),
        start: f32_at(d, tail + 16),
        stop: f32_at(d, tail + 20),
        text_keys: text_keys(h, b, u32_at(d, tail + 4) as i32),
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct Manager {
    pub ctrl: TimeCtrl,
    /// Cumulative as the raw byte, so a 2 round-trips.
    pub cumulative: u8,
    pub sequences: Vec<i32>,
    pub palette: i32,
}

pub fn read_manager(h: &Header, b: &[u8], block: usize) -> Result<Manager, NifError> {
    let d = body(h, b, block)?;
    if h.kind(block) != b"NiControllerManager" {
        return Err(geo(block, "not a NiControllerManager"));
    }
    let ctrl = read_ctrl(h, b, block)?;
    let at = BASE_LEN + 1; // cumulative flag
    if d.len() < at + 8 {
        return Err(geo(block, "manager shorter than its fixed fields"));
    }
    let n = u32_at(d, at) as usize;
    let end = n
        .checked_mul(4)
        .and_then(|c| c.checked_add(at + 8))
        .ok_or_else(|| geo(block, "sequence count overflows"))?;
    exact(h, block, end, d.len())?;
    Ok(Manager {
        ctrl,
        cumulative: d[BASE_LEN],
        sequences: (0..n).map(|i| u32_at(d, at + 4 + 4 * i) as i32).collect(),
        palette: u32_at(d, end - 4) as i32,
    })
}
