//! A shape's material as PGPatcher leaves it in the NIF and Community Shaders reads it at load (P1 spec §2.1,
//! 03 §1.2). Owned plain data over `props`, so the set editor, the PGPatcher simulator (`pgpsim`) and the
//! renderer share one value without a GPU dependency.
use crate::geom::{Attr, VertexDesc};
use crate::path::{canon_texture, TexPath};
use crate::props::{Alpha, Lighting, LightingParams, LightingTrailer};
use crate::shape_head::ShapeHead;

/// PGPatcher `PreFixTextureSlotCount`: every `MaterialState` carries exactly this many slots.
pub const SLOT_COUNT: usize = 9;

/// nif.xml `SkyrimShaderPropertyFlags1` bit 0 `Specular` (nifxml-master `nif.xml:6373`).
pub const SLSF1_SPECULAR: u32 = 1 << 0;
/// nif.xml `SkyrimShaderPropertyFlags1` bit 8 `Recieve_Shadows` (sic, `nif.xml:6380`).
pub const SLSF1_RECEIVE_SHADOWS: u32 = 1 << 8;
/// nif.xml `SkyrimShaderPropertyFlags1` bit 9 `Cast_Shadows` (`nif.xml:6382`).
pub const SLSF1_CAST_SHADOWS: u32 = 1 << 9;
/// nif.xml `SkyrimShaderPropertyFlags2` bit 0 `ZBuffer_Write` (`nif.xml:6409`).
pub const SLSF2_ZBUFFER_WRITE: u32 = 1 << 0;
/// nif.xml `SkyrimShaderPropertyFlags2` bit 23 `Unused01`: the TruePBR marker PGPatcher sets and CS reads
/// (01 §2.2, §3.1).
pub const SLSF2_TRUE_PBR: u32 = 1 << 23;

/// A shape's material after any PGPatcher simulation: what CS reads from the NIF at load.
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialState {
    /// BSLightingShaderType (0 default, 1 envmap, …, 11 multilayer parallax, 16 eye).
    pub shader_type: u32,
    pub slsf1: u32,
    pub slsf2: u32,
    /// NiAlphaProperty: presence → DO_ALPHA_TEST; flags bit 0 → blending.
    pub alpha: Option<Alpha>,
    pub params: LightingParams,
    /// Exactly 9 entries (PGPatcher PreFixTextureSlotCount); missing slots are empty `TexPath`s.
    /// `raw` = the string PGPatcher sees/writes (backslashes, any case); `norm` = canonical render path
    /// (`textures/…`, lower case, `/`, from `canon_texture`).
    pub slots: Vec<TexPath>,
    pub skinned: bool,
    pub has_vertex_colors: bool,
    /// PGPatcher `delete: true`: the shape is not drawn.
    pub hidden: bool,
}

/// The empty slot: `canon_texture(b"")` (raw "", norm "", `flags.empty`).
pub fn empty_slot() -> TexPath {
    canon_texture(b"")
}

impl MaterialState {
    /// The NIF's own material. `slots` is the texture set as scanned (`nif::scan::Shader::Lighting`), padded
    /// with empty slots or cut to [`SLOT_COUNT`]. `has_vertex_colors` comes from the shape's vertex
    /// descriptor (`Vertex_Colors` attribute), not from SLSF2.5.
    pub fn from_lighting(
        head: &ShapeHead,
        l: &Lighting,
        slots: &[TexPath],
        alpha: Option<Alpha>,
    ) -> MaterialState {
        let mut s: Vec<TexPath> = slots.iter().take(SLOT_COUNT).cloned().collect();
        s.resize(SLOT_COUNT, empty_slot());
        MaterialState {
            shader_type: l.shader_type,
            slsf1: l.slsf1,
            slsf2: l.slsf2,
            alpha,
            params: l.params.clone(),
            slots: s,
            skinned: head.skinned,
            has_vertex_colors: head
                .vertex_desc
                .is_some_and(|d| VertexDesc(d).has(Attr::Colors)),
            hidden: false,
        }
    }

    /// The material of a built-in preview shape before the set editor fills it: shader 0, SLSF1 = Specular |
    /// Receive_Shadows | Cast_Shadows, SLSF2 = ZBuffer_Write, PBR off, neutral parameters (uv scale 1, alpha 1,
    /// glossiness 80, specular colour 1, strength 1, lighting effects 0.3 / 2.0, no emissive colour, emissive
    /// multiple 1, wrap clamp mode 3, no trailer), 9 empty slots.
    pub fn builtin_default() -> MaterialState {
        MaterialState {
            shader_type: 0,
            slsf1: SLSF1_SPECULAR | SLSF1_RECEIVE_SHADOWS | SLSF1_CAST_SHADOWS,
            slsf2: SLSF2_ZBUFFER_WRITE,
            alpha: None,
            params: LightingParams {
                uv_offset: [0.0; 2],
                uv_scale: [1.0; 2],
                emissive_color: [0.0; 3],
                emissive_multiple: 1.0,
                texture_clamp_mode: 3,
                alpha: 1.0,
                refraction_strength: 0.0,
                glossiness: 80.0,
                specular_color: [1.0; 3],
                specular_strength: 1.0,
                lighting_effect_1: 0.3,
                lighting_effect_2: 2.0,
                trailer: LightingTrailer::None,
            },
            slots: vec![empty_slot(); SLOT_COUNT],
            skinned: false,
            has_vertex_colors: false,
            hidden: false,
        }
    }

    /// SLSF2 bit 23, the TruePBR marker.
    pub fn is_pbr(&self) -> bool {
        self.slsf2 & SLSF2_TRUE_PBR != 0
    }

    /// The NiAlphaProperty enables blending (flags bit 0).
    pub fn blend(&self) -> bool {
        self.alpha.is_some_and(|a| a.blend())
    }

    /// Sets slot `i` from a PGPatcher-form string: `raw` is kept as given, `norm` is recomputed with
    /// `canon_texture`. Panics when `i >= SLOT_COUNT` (every `MaterialState` has 9 slots; a larger index is a
    /// programming error).
    pub fn set_slot_raw(&mut self, i: usize, raw: &str) {
        assert!(i < SLOT_COUNT, "slot {i} out of range (0..{SLOT_COUNT})");
        if self.slots.len() < SLOT_COUNT {
            self.slots.resize(SLOT_COUNT, empty_slot());
        }
        self.slots[i] = canon_texture(raw.as_bytes());
    }
}
