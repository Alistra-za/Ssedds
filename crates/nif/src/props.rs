// crates/nif/src/props.rs
//! Small property blocks, decoded fully and checked against the declared block size (plan 01 Q2, nif.xml
//! 6581-6685, 6307-6319, 3972-3978). BSVER 83 and 100 share these layouts.
use crate::cursor::Cursor;
use crate::header::Header;
use crate::path::{canon_texture, TexPath};
use crate::text::lossy;
use crate::NifError;

#[derive(Debug, Clone, PartialEq)]
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

/// Absolute file offsets of editable fields, for a later in-place patcher (plan 01 Q8).
#[derive(Debug, Clone, PartialEq)]
pub struct FieldOffsets {
    pub block_offset: usize,
    pub slsf1: usize,
    pub uv_offset: usize,
    pub emissive_color: usize,
    pub glossiness: usize,
    pub trailer: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Lighting {
    pub name: Option<String>,
    pub shader_type: u32,
    pub slsf1: u32,
    pub slsf2: u32,
    pub texture_set: i32,
    pub params: LightingParams,
    pub field_offsets: FieldOffsets,
}

#[derive(Debug, Clone, PartialEq)]
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
    pub name: Option<String>,
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
            kind: lossy(h.kind(block)),
            declared: h.sizes[block],
            consumed: (c.pos() - start) as u32,
        });
    }
    Ok(())
}

/// NiObjectNET: Name (string index), NumExtraData + refs, Controller.
fn object_net(h: &Header, c: &mut Cursor) -> Result<Option<String>, NifError> {
    let name = h.string(c.i32()?).map(lossy).filter(|s| !s.is_empty());
    let n = c.u32()? as usize;
    c.skip(4 * n)?;
    c.i32()?;
    Ok(name)
}

pub fn decode_lighting(h: &Header, b: &[u8], block: usize) -> Result<Lighting, NifError> {
    let (mut c, start, end) = block_cursor(h, b, block);
    let shader_type = c.u32()?; // before the name at BSVER 83..139 (plan 01 Q2)
    let name = object_net(h, &mut c)?;
    let slsf1_off = c.pos();
    let slsf1 = c.u32()?;
    let slsf2 = c.u32()?;
    let uv_off = c.pos();
    let uv_offset = c.f32s::<2>()?;
    let uv_scale = c.f32s::<2>()?;
    let texture_set = c.i32()?;
    let em_off = c.pos();
    let emissive_color = c.f32s::<3>()?;
    let emissive_multiple = c.f32()?;
    let texture_clamp_mode = c.u32()?;
    let alpha = c.f32()?;
    let refraction_strength = c.f32()?;
    let gloss_off = c.pos();
    let glossiness = c.f32()?;
    let specular_color = c.f32s::<3>()?;
    let specular_strength = c.f32()?;
    let lighting_effect_1 = c.f32()?;
    let lighting_effect_2 = c.f32()?;
    let trailer_off = c.pos();
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
        field_offsets: FieldOffsets {
            block_offset: start,
            slsf1: slsf1_off,
            uv_offset: uv_off,
            emissive_color: em_off,
            glossiness: gloss_off,
            trailer: trailer_off,
        },
    })
}

pub fn decode_effect(h: &Header, b: &[u8], block: usize) -> Result<Effect, NifError> {
    let (mut c, start, end) = block_cursor(h, b, block);
    let name = object_net(h, &mut c)?;
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

pub fn decode_texture_set(h: &Header, b: &[u8], block: usize) -> Result<TextureSet, NifError> {
    let (mut c, start, end) = block_cursor(h, b, block);
    let n = c.u32()? as usize;
    let slots = (0..n)
        .map(|_| c.sized_bytes().map(canon_texture))
        .collect::<Result<Vec<_>, _>>()?;
    finish(h, block, &c, start, end)?;
    Ok(TextureSet { slots })
}

pub fn decode_alpha(h: &Header, b: &[u8], block: usize) -> Result<Alpha, NifError> {
    let (mut c, start, end) = block_cursor(h, b, block);
    object_net(h, &mut c)?;
    let flags = c.u16()?;
    let threshold = c.u8()?;
    finish(h, block, &c, start, end)?;
    Ok(Alpha { flags, threshold })
}
