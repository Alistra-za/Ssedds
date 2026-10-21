//! Editable field offsets inside blocks. `locate` returns the block-relative offset and the current value.
//! Each block type has its own arm; add controller/interpolator types as further arms.
use crate::{header::Header, net::read_net, NifError};

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Field {
    ShaderType,
    Slsf1,
    Slsf2,
    UvOffset(u8),
    UvScale(u8),
    EmissiveColor(u8),
    EmissiveMultiple,
    ClampMode,
    Alpha,
    Refraction,
    Glossiness,
    SpecularColor(u8),
    SpecularStrength,
    LightingEffect1,
    LightingEffect2,
    Trailer(u8),
    AlphaFlags,
    AlphaThreshold,
    CtrlFlags,
    Frequency,
    Phase,
    StartTime,
    StopTime,
    Variable,
    InterpValue(u8),
}

/// F32 holds the bit pattern so edits are exact.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value {
    U8(u8),
    U16(u16),
    U32(u32),
    F32(u32),
}

impl Value {
    pub fn bytes(&self) -> Vec<u8> {
        match *self {
            Value::U8(v) => vec![v],
            Value::U16(v) => v.to_le_bytes().to_vec(),
            Value::U32(v) | Value::F32(v) => v.to_le_bytes().to_vec(),
        }
    }
    pub fn as_f32(&self) -> Option<f32> {
        match *self {
            Value::F32(v) => Some(f32::from_bits(v)),
            _ => None,
        }
    }
    pub fn as_u32(&self) -> Option<u32> {
        match *self {
            Value::U32(v) => Some(v),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Ty {
    U8,
    U16,
    U32,
    F32,
}

/// Count of f32 after Lighting Effect 2 for a BSLightingShaderProperty shader type.
fn trailer_len(shader_type: u32) -> u8 {
    match shader_type {
        1 => 1,
        5 | 6 => 3,
        7 => 2,
        11 => 5,
        14 => 4,
        16 => 7,
        _ => 0,
    }
}

pub fn locate(h: &Header, b: &[u8], block: usize, f: Field) -> Result<(usize, Value), NifError> {
    if block >= h.sizes.len() {
        return Err(NifError::Geometry {
            block: block as u32,
            msg: "no such block".into(),
        });
    }
    let bs = h.offsets()[block];
    let geo = |msg: String| NifError::Geometry {
        block: block as u32,
        msg,
    };
    let found = match h.kind(block) {
        b"BSLightingShaderProperty" => lsp(h, b, block, f)?,
        b"NiAlphaProperty" => alpha(h, b, block, f)?,
        k if crate::ctrl::is_ctrl(k) => ctrl(k, f),
        b"NiBlendFloatInterpolator" | b"NiBlendPoint3Interpolator" => blend(h, b, block, f)?,
        k => interp(k, f),
    };
    let Some((rel, ty)) = found else {
        return Err(geo(format!("{f:?} does not exist on this block type")));
    };
    let at = bs + rel;
    let size = match ty {
        Ty::U8 => 1,
        Ty::U16 => 2,
        Ty::U32 | Ty::F32 => 4,
    };
    if at + size > bs + h.sizes[block] as usize || at + size > b.len() {
        return Err(geo(format!("{f:?} runs past the block")));
    }
    let s = &b[at..at + size];
    let v = match ty {
        Ty::U8 => Value::U8(s[0]),
        Ty::U16 => Value::U16(u16::from_le_bytes(s.try_into().unwrap())),
        Ty::U32 => Value::U32(u32::from_le_bytes(s.try_into().unwrap())),
        Ty::F32 => Value::F32(u32::from_le_bytes(s.try_into().unwrap())),
    };
    Ok((rel, v))
}

type Found = Option<(usize, Ty)>;

/// Controller fields: NiTimeController base, then the variable on the classes that carry one.
fn ctrl(kind: &[u8], f: Field) -> Found {
    Some(match f {
        Field::CtrlFlags => (4, Ty::U16),
        Field::Frequency => (6, Ty::F32),
        Field::Phase => (10, Ty::F32),
        Field::StartTime => (14, Ty::F32),
        Field::StopTime => (18, Ty::F32),
        Field::Variable if crate::ctrl::known_len(kind) == Some(crate::ctrl::BASE_LEN + 8) => {
            (30, Ty::U32)
        }
        _ => return None,
    })
}

fn interp(kind: &[u8], f: Field) -> Found {
    match f {
        Field::InterpValue(i) if (i as usize) < crate::ctrl::interp_floats(kind) => {
            Some((4 * i as usize, Ty::F32))
        }
        _ => None,
    }
}

/// Blend Value sits after the items, so its offset depends on the flags and array size (F7).
fn blend(h: &Header, b: &[u8], block: usize, f: Field) -> Result<Found, NifError> {
    let x = crate::ctrl::read_blend_interp(h, b, block)?;
    Ok(match f {
        Field::InterpValue(i) if (i as usize) < x.value.len() => {
            Some((x.value_at + 4 * i as usize, Ty::F32))
        }
        _ => None,
    })
}

fn alpha(h: &Header, b: &[u8], block: usize, f: Field) -> Result<Found, NifError> {
    let p = read_net(h, b, block)?.end - h.offsets()[block];
    Ok(match f {
        Field::AlphaFlags => Some((p, Ty::U16)),
        Field::AlphaThreshold => Some((p + 2, Ty::U8)),
        _ => None,
    })
}

fn lsp(h: &Header, b: &[u8], block: usize, f: Field) -> Result<Found, NifError> {
    let bs = h.offsets()[block];
    let p = read_net(h, b, block)?.end - bs;
    let idx = |n: u8, max: u8| n < max;
    Ok(match f {
        Field::ShaderType => Some((0, Ty::U32)),
        Field::Slsf1 => Some((p, Ty::U32)),
        Field::Slsf2 => Some((p + 4, Ty::U32)),
        Field::UvOffset(n) if idx(n, 2) => Some((p + 8 + 4 * n as usize, Ty::F32)),
        Field::UvScale(n) if idx(n, 2) => Some((p + 16 + 4 * n as usize, Ty::F32)),
        Field::EmissiveColor(n) if idx(n, 3) => Some((p + 28 + 4 * n as usize, Ty::F32)),
        Field::EmissiveMultiple => Some((p + 40, Ty::F32)),
        Field::ClampMode => Some((p + 44, Ty::U32)),
        Field::Alpha => Some((p + 48, Ty::F32)),
        Field::Refraction => Some((p + 52, Ty::F32)),
        Field::Glossiness => Some((p + 56, Ty::F32)),
        Field::SpecularColor(n) if idx(n, 3) => Some((p + 60 + 4 * n as usize, Ty::F32)),
        Field::SpecularStrength => Some((p + 72, Ty::F32)),
        Field::LightingEffect1 => Some((p + 76, Ty::F32)),
        Field::LightingEffect2 => Some((p + 80, Ty::F32)),
        Field::Trailer(n) => {
            let t = b
                .get(bs..bs + 4)
                .ok_or(NifError::Truncated { at: bs, need: 4 })?;
            let t = u32::from_le_bytes(t.try_into().unwrap());
            idx(n, trailer_len(t)).then(|| (p + 84 + 4 * n as usize, Ty::F32))
        }
        _ => None,
    })
}
