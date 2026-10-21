//! Encoders for the blocks a named clip needs, with index-level readers so decode then encode is byte-identical
//! (20.2.0.7, user 12, BS 100). Strings stay string-table indices: re-interning a resolved string would pick the
//! first of duplicate strings (spec 5.4). `ctrl::read_sequence` stays the model's view.
//! nif.xml (F:\ai\dds\nifparseref\nifxml-master\nif.xml): NiTimeController 3600-3609, NiControllerManager
//! 4193-4199, NiControllerSequence 4201-4232, ControlledBlock 1919-1950, NiDefaultAVObjectPalette 4234-4243
//! (AVObject 1913-1917, SizedString 1652-1656), NiTextKeyExtraData 5180-5184 (NiExtraData 3220-3225),
//! NiMultiTargetTransformController 3613-3621, blend interpolators 3292-3320 and 4078-4086.
use crate::ctrl::{body, exact, geo, BlendInterp, Manager, TimeCtrl, BASE_LEN, CB_LEN};
use crate::{header::Header, NifError};

fn i32_at(d: &[u8], at: usize) -> i32 {
    i32::from_le_bytes(d[at..at + 4].try_into().unwrap())
}
fn f32_at(d: &[u8], at: usize) -> f32 {
    f32::from_bits(i32_at(d, at) as u32)
}

/// A count as its field type, or Err when the format cannot hold it (never truncated).
fn count<T: TryFrom<usize>>(n: usize, what: &str) -> Result<T, NifError> {
    T::try_from(n).map_err(|_| NifError::Encode(format!("{what}: {n} does not fit its field")))
}

/// Authoring defaults measured on Meshes0.bsa (C1 census, ruling C-P26): all 1051 managers have flags 0x4c;
/// 1050 of 1051 manager-owned MTTCs have 0x6c; all 6722 blend interpolators have flags 1 and array size 2.
pub const MANAGER_FLAGS: u16 = 0x4c;
pub const MTTC_FLAGS: u16 = 0x6c;
pub const BLEND_FLAGS: u8 = 1;
pub const BLEND_ARRAY_SIZE: u8 = 2;

/// The 26 NiTimeController bytes: next, flags, frequency, phase, start, stop, target.
pub fn encode_base(c: &TimeCtrl) -> Vec<u8> {
    let mut o = Vec::with_capacity(BASE_LEN);
    o.extend(c.next.to_le_bytes());
    o.extend(c.flags.to_le_bytes());
    for f in [c.frequency, c.phase, c.start, c.stop] {
        o.extend(f.to_le_bytes());
    }
    o.extend(c.target.to_le_bytes());
    o
}

/// Base, Cumulative byte, sequence count and refs, object palette ref.
pub fn encode_manager(m: &Manager) -> Result<Vec<u8>, NifError> {
    let n: u32 = count(m.sequences.len(), "manager sequence count")?;
    let mut o = encode_base(&m.ctrl);
    o.push(m.cumulative);
    o.extend(n.to_le_bytes());
    for s in &m.sequences {
        o.extend(s.to_le_bytes());
    }
    o.extend(m.palette.to_le_bytes());
    Ok(o)
}

/// One ControlledBlock (29 bytes). String fields are string-table indices, -1 = null.
#[derive(Debug, Clone, PartialEq)]
pub struct CtrlBlockOut {
    pub interp: i32,
    pub controller: i32,
    pub priority: u8,
    pub node: i32,
    pub property: i32,
    pub ctrl_type: i32,
    pub ctrl_id: i32,
    pub interp_id: i32,
}

/// Every NiControllerSequence field present at 20.2.0.7. `notes` are the Anim Note Arrays refs, carried raw.
#[derive(Debug, Clone, PartialEq)]
pub struct SequenceOut {
    pub name: i32,
    pub blocks: Vec<CtrlBlockOut>,
    pub array_grow_by: u32,
    pub weight: f32,
    pub text_keys: i32,
    pub cycle: u32,
    pub freq: f32,
    pub start: f32,
    pub stop: f32,
    pub manager: i32,
    pub accum_root: i32,
    pub notes: Vec<i32>,
}

pub fn read_sequence_out(h: &Header, b: &[u8], block: usize) -> Result<SequenceOut, NifError> {
    let d = body(h, b, block)?;
    if h.kind(block) != b"NiControllerSequence" {
        return Err(geo(block, "not a NiControllerSequence"));
    }
    if d.len() < 12 + 34 {
        return Err(geo(block, "sequence shorter than its fixed fields"));
    }
    let n = i32_at(d, 4) as u32 as usize;
    let t = n
        .checked_mul(CB_LEN)
        .and_then(|c| c.checked_add(12))
        .filter(|&t| t + 34 <= d.len())
        .ok_or_else(|| geo(block, "controlled-block count runs past the block"))?;
    let m = u16::from_le_bytes([d[t + 32], d[t + 33]]) as usize;
    exact(h, block, t + 34 + 4 * m, d.len())?;
    let blocks = (0..n)
        .map(|i| {
            let c = &d[12 + i * CB_LEN..];
            CtrlBlockOut {
                interp: i32_at(c, 0),
                controller: i32_at(c, 4),
                priority: c[8],
                node: i32_at(c, 9),
                property: i32_at(c, 13),
                ctrl_type: i32_at(c, 17),
                ctrl_id: i32_at(c, 21),
                interp_id: i32_at(c, 25),
            }
        })
        .collect();
    Ok(SequenceOut {
        name: i32_at(d, 0),
        blocks,
        array_grow_by: i32_at(d, 8) as u32,
        weight: f32_at(d, t),
        text_keys: i32_at(d, t + 4),
        cycle: i32_at(d, t + 8) as u32,
        freq: f32_at(d, t + 12),
        start: f32_at(d, t + 16),
        stop: f32_at(d, t + 20),
        manager: i32_at(d, t + 24),
        accum_root: i32_at(d, t + 28),
        notes: (0..m).map(|k| i32_at(d, t + 34 + 4 * k)).collect(),
    })
}

pub fn encode_sequence(s: &SequenceOut) -> Result<Vec<u8>, NifError> {
    let n: u32 = count(s.blocks.len(), "controlled-block count")?;
    let m: u16 = count(s.notes.len(), "anim note array count")?;
    let mut o = Vec::with_capacity(46 + CB_LEN * s.blocks.len() + 4 * s.notes.len());
    o.extend(s.name.to_le_bytes());
    o.extend(n.to_le_bytes());
    o.extend(s.array_grow_by.to_le_bytes());
    for c in &s.blocks {
        o.extend(c.interp.to_le_bytes());
        o.extend(c.controller.to_le_bytes());
        o.push(c.priority);
        for x in [c.node, c.property, c.ctrl_type, c.ctrl_id, c.interp_id] {
            o.extend(x.to_le_bytes());
        }
    }
    o.extend(s.weight.to_le_bytes());
    o.extend(s.text_keys.to_le_bytes());
    o.extend(s.cycle.to_le_bytes());
    for f in [s.freq, s.start, s.stop] {
        o.extend(f.to_le_bytes());
    }
    o.extend(s.manager.to_le_bytes());
    o.extend(s.accum_root.to_le_bytes());
    o.extend(m.to_le_bytes());
    for r in &s.notes {
        o.extend(r.to_le_bytes());
    }
    Ok(o)
}

/// (Scene ptr, [(name bytes without the u32 length, AV Object ptr)]).
pub type Palette = (i32, Vec<(Vec<u8>, i32)>);

/// Names are SizedStrings, not string indices.
pub fn read_palette(h: &Header, b: &[u8], block: usize) -> Result<Palette, NifError> {
    let d = body(h, b, block)?;
    if h.kind(block) != b"NiDefaultAVObjectPalette" {
        return Err(geo(block, "not a NiDefaultAVObjectPalette"));
    }
    if d.len() < 8 {
        return Err(geo(block, "palette shorter than its fixed fields"));
    }
    let n = i32_at(d, 4) as u32 as usize;
    // each object takes at least 8 bytes: bound n before allocating
    if n > (d.len() - 8) / 8 {
        return Err(geo(block, "object count runs past the block"));
    }
    let mut objs = Vec::with_capacity(n);
    let mut at = 8;
    for _ in 0..n {
        let need = |x: usize| (x <= d.len()).then_some(x);
        let l = need(at + 4).map(|_| i32_at(d, at) as u32 as usize);
        let end = l
            .and_then(|l| (at + 8).checked_add(l))
            .and_then(need)
            .ok_or_else(|| geo(block, "object name runs past the block"))?;
        objs.push((d[at + 4..end - 4].to_vec(), i32_at(d, end - 4)));
        at = end;
    }
    exact(h, block, at, d.len())?;
    Ok((i32_at(d, 0), objs))
}

pub fn encode_palette(scene: i32, objs: &[(Vec<u8>, i32)]) -> Result<Vec<u8>, NifError> {
    let n: u32 = count(objs.len(), "palette object count")?;
    let mut o = scene.to_le_bytes().to_vec();
    o.extend(n.to_le_bytes());
    for (name, ptr) in objs {
        let l: u32 = count(name.len(), "palette name length")?;
        o.extend(l.to_le_bytes());
        o.extend(name);
        o.extend(ptr.to_le_bytes());
    }
    Ok(o)
}

/// (Name string index, [(time, value string index)]).
pub fn read_text_keys(
    h: &Header,
    b: &[u8],
    block: usize,
) -> Result<(i32, Vec<(f32, i32)>), NifError> {
    let d = body(h, b, block)?;
    if h.kind(block) != b"NiTextKeyExtraData" {
        return Err(geo(block, "not a NiTextKeyExtraData"));
    }
    if d.len() < 8 {
        return Err(geo(block, "text keys shorter than their fixed fields"));
    }
    let n = i32_at(d, 4) as u32 as usize;
    let want = n
        .checked_mul(8)
        .and_then(|x| x.checked_add(8))
        .ok_or_else(|| geo(block, "text-key count overflows"))?;
    exact(h, block, want, d.len())?;
    let keys = (0..n)
        .map(|k| (f32_at(d, 8 + 8 * k), i32_at(d, 12 + 8 * k)))
        .collect();
    Ok((i32_at(d, 0), keys))
}

pub fn encode_text_keys(name: i32, keys: &[(f32, i32)]) -> Result<Vec<u8>, NifError> {
    let n: u32 = count(keys.len(), "text-key count")?;
    let mut o = name.to_le_bytes().to_vec();
    o.extend(n.to_le_bytes());
    for (t, v) in keys {
        o.extend(t.to_le_bytes());
        o.extend(v.to_le_bytes());
    }
    Ok(o)
}

/// (base, Extra Targets ptrs) of a NiMultiTargetTransformController. Exact size.
pub fn read_mttc(h: &Header, b: &[u8], block: usize) -> Result<(TimeCtrl, Vec<i32>), NifError> {
    let d = body(h, b, block)?;
    if h.kind(block) != b"NiMultiTargetTransformController" {
        return Err(geo(block, "not a NiMultiTargetTransformController"));
    }
    let base = crate::ctrl::read_ctrl(h, b, block)?;
    if d.len() < BASE_LEN + 2 {
        return Err(geo(block, "MTTC shorter than its fixed fields"));
    }
    let k = u16::from_le_bytes([d[BASE_LEN], d[BASE_LEN + 1]]) as usize;
    exact(h, block, BASE_LEN + 2 + 4 * k, d.len())?;
    let at = BASE_LEN + 2;
    Ok((base, (0..k).map(|i| i32_at(d, at + 4 * i)).collect()))
}

/// Base, Num Extra Targets u16, the target ptrs. Vanilla manager-owned MTTCs always list targets; zero is allowed.
pub fn encode_mttc(base: &TimeCtrl, targets: &[i32]) -> Result<Vec<u8>, NifError> {
    let k: u16 = count(targets.len(), "MTTC extra target count")?;
    let mut o = encode_base(base);
    o.extend(k.to_le_bytes());
    for t in targets {
        o.extend(t.to_le_bytes());
    }
    Ok(o)
}

fn blend_values(class: &str) -> Option<usize> {
    match class {
        "NiBlendFloatInterpolator" => Some(1),
        "NiBlendPoint3Interpolator" => Some(3),
        _ => None,
    }
}

/// Any blend interpolator: flags, array size, threshold, the raw counter + item bytes (bit 0 clear), value.
/// Err when `items` disagrees with the flags or the array size, or the value count with the class.
pub fn encode_blend(b: &BlendInterp) -> Result<Vec<u8>, NifError> {
    let bad = |m: &str| Err(NifError::Encode(format!("{}: {m}", b.class)));
    if blend_values(&b.class) != Some(b.value.len()) {
        return bad("value count does not match the class");
    }
    let want = (b.flags & 1 == 0).then_some(20 + 17 * b.array_size as usize);
    if b.items.as_ref().map(Vec::len) != want {
        return bad("item bytes do not match the flags and array size");
    }
    let mut o = vec![b.flags, b.array_size];
    o.extend(b.weight_threshold.to_le_bytes());
    if let Some(items) = &b.items {
        o.extend(items);
    }
    for v in &b.value {
        o.extend(v.to_le_bytes());
    }
    Ok(o)
}

/// The authoring form, vanilla values: flags 1 (manager controlled), array size 2, threshold 0, then `value`
/// (vanilla stores `#INV_FLT#` / `#INV_VEC3#` there; the caller passes it).
pub fn encode_blend_interp(class: &str, value: &[f32]) -> Result<Vec<u8>, NifError> {
    encode_blend(&BlendInterp {
        block: 0,
        class: class.into(),
        flags: BLEND_FLAGS,
        array_size: BLEND_ARRAY_SIZE,
        weight_threshold: 0.0,
        items: None,
        value: value.to_vec(),
        value_at: 6,
    })
}
