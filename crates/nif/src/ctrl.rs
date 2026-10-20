//! Read-only view of time controllers, interpolators, NiControllerManager and NiControllerSequence
//! (layouts: nif-edit-v2 01-format sections 3-4). Every entry point validates the block index and sizes first;
//! hostile input is an error, never a panic or an unbounded allocation.
use crate::{header::Header, NifError};

/// next i32, flags u16, frequency, phase, start, stop f32, target i32.
pub const BASE_LEN: usize = 26;
const CB_LEN: usize = 29;

fn geo(block: usize, msg: &str) -> NifError {
    NifError::Geometry {
        block: block as u32,
        msg: msg.into(),
    }
}

fn body<'a>(h: &Header, b: &'a [u8], block: usize) -> Result<&'a [u8], NifError> {
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

fn exact(h: &Header, block: usize, want: usize, got: usize) -> Result<(), NifError> {
    if want == got {
        return Ok(());
    }
    Err(NifError::BlockSize {
        block: block as u32,
        kind: String::from_utf8_lossy(h.kind(block)).into(),
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

/// String fields are raw string-table entries: None = index -1, Some(empty) = the empty string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CtrlBlock {
    pub interp: i32,
    pub controller: i32,
    pub node: Option<Vec<u8>>,
    pub property: Option<Vec<u8>>,
    pub ctrl_type: Option<Vec<u8>>,
    pub ctrl_id: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sequence {
    pub block: u32,
    pub name: Option<String>,
    pub blocks: Vec<CtrlBlock>,
    pub manager: i32,
}

/// Copies one string-table entry. `budget` is the bytes still allowed to be copied for this sequence.
fn string(
    h: &Header,
    block: usize,
    idx: i32,
    what: &str,
    budget: &mut usize,
) -> Result<Option<Vec<u8>>, NifError> {
    if idx == -1 {
        return Ok(None);
    }
    let s = h.string(idx).ok_or_else(|| {
        geo(
            block,
            &format!("{what}: string index outside the string table"),
        )
    })?;
    // shortcut: bounded by the file length, not by a tighter per-field limit
    *budget = budget.checked_sub(s.len()).ok_or_else(|| {
        geo(
            block,
            "strings referenced by the sequence exceed the file length",
        )
    })?;
    Ok(Some(s.to_vec()))
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
    let mut budget = b.len();
    let mut blocks = Vec::with_capacity(n);
    for i in 0..n {
        let c = &d[12 + i * CB_LEN..];
        let mut s = |k: usize| {
            let what = format!("controlled block {i}");
            string(h, block, u32_at(c, 9 + 4 * k) as i32, &what, &mut budget)
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
    let name = string(h, block, u32_at(d, 0) as i32, "name", &mut budget)?
        .map(|s| String::from_utf8_lossy(&s).into());
    Ok(Sequence {
        block: block as u32,
        name,
        blocks,
        manager: u32_at(d, tail + 24) as i32,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct Manager {
    pub ctrl: TimeCtrl,
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
        sequences: (0..n).map(|i| u32_at(d, at + 4 + 4 * i) as i32).collect(),
        palette: u32_at(d, end - 4) as i32,
    })
}
