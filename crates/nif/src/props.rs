// crates/nif/src/props.rs
//! Small property blocks, decoded fully and checked against the declared block size (plan 01 Q2, nif.xml
//! 6581-6685, 6307-6319, 3972-3978). BSVER 83 and 100 share these layouts.
use crate::cursor::Cursor;
use crate::header::Header;
use crate::path::{canon_texture, TexPath};
use crate::text::lossy;
use crate::NifError;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum LightingTrailer {
    None,
    EnvMap {
        scale: f32,
    },
    SkinTint([f32; 3]),
    HairTint([f32; 3]),
    ParallaxOcc {
        max_passes: f32,
        scale: f32,
    },
    MultiLayer {
        inner_thickness: f32,
        refraction_scale: f32,
        inner_texture_scale: [f32; 2],
        envmap_strength: f32,
    },
    SparkleSnow([f32; 4]),
    Eye {
        cubemap_scale: f32,
        left_center: [f32; 3],
        right_center: [f32; 3],
    },
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LightingParams {
    pub uv_offset: [f32; 2],
    pub uv_scale: [f32; 2],
    pub emissive_color: [f32; 3],
    pub emissive_multiple: f32,
    pub texture_clamp_mode: u32,
    pub alpha: f32,
    pub refraction_strength: f32,
    pub glossiness: f32,
    pub specular_color: [f32; 3],
    pub specular_strength: f32,
    pub lighting_effect_1: f32,
    pub lighting_effect_2: f32,
    pub trailer: LightingTrailer,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Lighting {
    pub name: Option<Arc<str>>,
    pub shader_type: u32,
    pub slsf1: u32,
    pub slsf2: u32,
    pub texture_set: i32,
    pub params: LightingParams,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EffectParams {
    pub uv_offset: [f32; 2],
    pub uv_scale: [f32; 2],
    pub texture_clamp_mode: u8,
    pub lighting_influence: u8,
    pub env_map_min_lod: u8,
    /// start angle, stop angle, start opacity, stop opacity
    pub falloff: [f32; 4],
    pub base_color: [f32; 4],
    pub base_color_scale: f32,
    pub soft_falloff_depth: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Effect {
    pub name: Option<Arc<str>>,
    pub slsf1: u32,
    pub slsf2: u32,
    pub source: TexPath,
    pub greyscale: TexPath,
    pub params: EffectParams,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextureSet {
    pub slots: Vec<TexPath>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Alpha {
    pub flags: u16,
    pub threshold: u8,
}
impl Alpha {
    pub fn blend(&self) -> bool {
        self.flags & 1 != 0
    }
    pub fn test(&self) -> bool {
        self.flags & (1 << 9) != 0
    }
}

fn block_cursor<'a>(h: &Header, b: &'a [u8], block: usize) -> (Cursor<'a>, usize, usize) {
    let start = h.offsets()[block];
    (
        Cursor::new(b, start),
        start,
        start + h.sizes[block] as usize,
    )
}

fn finish(h: &Header, block: usize, c: &Cursor, start: usize, end: usize) -> Result<(), NifError> {
    if c.pos() != end {
        return Err(NifError::BlockSize {
            block: block as u32,
            kind: crate::text::label(&lossy(h.kind(block))).into_owned(),
            declared: h.sizes[block],
            consumed: (c.pos() - start) as u32,
        });
    }
    Ok(())
}

/// NiObjectNET: Name (string index), NumExtraData + refs, Controller.
fn object_net(
    h: &Header,
    b: &[u8],
    block: usize,
    c: &mut Cursor,
) -> Result<Option<Arc<str>>, NifError> {
    let net = crate::net::read_net(h, b, block)?;
    c.skip(net.end - c.pos())?;
    Ok(h.name_arc(net.name).filter(|s| !s.is_empty()))
}

pub fn decode_lighting(h: &Header, b: &[u8], block: usize) -> Result<Lighting, NifError> {
    let (mut c, start, end) = block_cursor(h, b, block);
    let shader_type = c.u32()?; // before the name at BSVER 83..139 (plan 01 Q2)
    let name = object_net(h, b, block, &mut c)?;
    let slsf1 = c.u32()?;
    let slsf2 = c.u32()?;
    let uv_offset = c.f32s::<2>()?;
    let uv_scale = c.f32s::<2>()?;
    let texture_set = c.i32()?;
    let emissive_color = c.f32s::<3>()?;
    let emissive_multiple = c.f32()?;
    let texture_clamp_mode = c.u32()?;
    let alpha = c.f32()?;
    let refraction_strength = c.f32()?;
    let glossiness = c.f32()?;
    let specular_color = c.f32s::<3>()?;
    let specular_strength = c.f32()?;
    let lighting_effect_1 = c.f32()?;
    let lighting_effect_2 = c.f32()?;
    let trailer = match shader_type {
        1 => LightingTrailer::EnvMap { scale: c.f32()? },
        5 => LightingTrailer::SkinTint(c.f32s::<3>()?),
        6 => LightingTrailer::HairTint(c.f32s::<3>()?),
        7 => LightingTrailer::ParallaxOcc {
            max_passes: c.f32()?,
            scale: c.f32()?,
        },
        11 => LightingTrailer::MultiLayer {
            inner_thickness: c.f32()?,
            refraction_scale: c.f32()?,
            inner_texture_scale: c.f32s::<2>()?,
            envmap_strength: c.f32()?,
        },
        14 => LightingTrailer::SparkleSnow(c.f32s::<4>()?),
        16 => LightingTrailer::Eye {
            cubemap_scale: c.f32()?,
            left_center: c.f32s::<3>()?,
            right_center: c.f32s::<3>()?,
        },
        _ => LightingTrailer::None,
    };
    finish(h, block, &c, start, end)?;
    Ok(Lighting {
        name,
        shader_type,
        slsf1,
        slsf2,
        texture_set,
        params: LightingParams {
            uv_offset,
            uv_scale,
            emissive_color,
            emissive_multiple,
            texture_clamp_mode,
            alpha,
            refraction_strength,
            glossiness,
            specular_color,
            specular_strength,
            lighting_effect_1,
            lighting_effect_2,
            trailer,
        },
    })
}

pub fn decode_effect(h: &Header, b: &[u8], block: usize) -> Result<Effect, NifError> {
    let (mut c, start, end) = block_cursor(h, b, block);
    let name = object_net(h, b, block, &mut c)?;
    let slsf1 = c.u32()?;
    let slsf2 = c.u32()?;
    let uv_offset = c.f32s::<2>()?;
    let uv_scale = c.f32s::<2>()?;
    let source = canon_texture(c.sized_bytes()?);
    let texture_clamp_mode = c.u8()?;
    let lighting_influence = c.u8()?;
    let env_map_min_lod = c.u8()?;
    c.u8()?;
    let falloff = c.f32s::<4>()?;
    let base_color = c.f32s::<4>()?;
    let base_color_scale = c.f32()?;
    let soft_falloff_depth = c.f32()?;
    let greyscale = canon_texture(c.sized_bytes()?);
    finish(h, block, &c, start, end)?;
    Ok(Effect {
        name,
        slsf1,
        slsf2,
        source,
        greyscale,
        params: EffectParams {
            uv_offset,
            uv_scale,
            texture_clamp_mode,
            lighting_influence,
            env_map_min_lod,
            falloff,
            base_color,
            base_color_scale,
            soft_falloff_depth,
        },
    })
}

/// Most slots a BSShaderTextureSet may declare. Skyrim writes 9, Fallout 4 writes 10; a larger count is a
/// damaged file, refused before its slots are kept (audit 2026-10-14 finding 1).
pub const MAX_TEXTURE_SLOTS: usize = 32;

pub fn decode_texture_set(h: &Header, b: &[u8], block: usize) -> Result<TextureSet, NifError> {
    let (mut c, start, end) = block_cursor(h, b, block);
    let n = c.u32()? as usize;
    if n > MAX_TEXTURE_SLOTS {
        return Err(NifError::Geometry {
            block: block as u32,
            msg: format!("{n} texture slots (at most {MAX_TEXTURE_SLOTS})"),
        });
    }
    let slots = (0..n)
        .map(|_| c.sized_bytes().map(canon_texture))
        .collect::<Result<Vec<_>, _>>()?;
    finish(h, block, &c, start, end)?;
    Ok(TextureSet { slots })
}

/// A texture set exactly as stored: raw bytes, exact count, no normalisation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TexSetRaw {
    pub block: u32,
    pub slots: Vec<Vec<u8>>,
}

pub fn decode_texture_set_raw(h: &Header, b: &[u8], block: usize) -> Result<TexSetRaw, NifError> {
    if block >= h.sizes.len() {
        return Err(NifError::Geometry {
            block: block as u32,
            msg: "no such block".into(),
        });
    }
    let (mut c, start, end) = block_cursor(h, b, block);
    let n = c.u32()? as usize;
    if n > MAX_TEXTURE_SLOTS {
        return Err(NifError::Geometry {
            block: block as u32,
            msg: format!("{n} texture slots (at most {MAX_TEXTURE_SLOTS})"),
        });
    }
    let slots = (0..n)
        .map(|_| c.sized_bytes().map(<[u8]>::to_vec))
        .collect::<Result<Vec<_>, _>>()?;
    finish(h, block, &c, start, end)?;
    Ok(TexSetRaw {
        block: block as u32,
        slots,
    })
}

pub fn encode_texture_set(slots: &[Vec<u8>]) -> Vec<u8> {
    let mut o = (slots.len() as u32).to_le_bytes().to_vec();
    for s in slots {
        o.extend((s.len() as u32).to_le_bytes());
        o.extend(s);
    }
    o
}

pub fn decode_alpha(h: &Header, b: &[u8], block: usize) -> Result<Alpha, NifError> {
    let (mut c, start, end) = block_cursor(h, b, block);
    object_net(h, b, block, &mut c)?;
    let flags = c.u16()?;
    let threshold = c.u8()?;
    finish(h, block, &c, start, end)?;
    Ok(Alpha { flags, threshold })
}
