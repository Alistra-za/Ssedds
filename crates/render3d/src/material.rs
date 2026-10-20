//! Community Shaders' TruePBR load remap and the material cbuffer fields, from an owned `MaterialState`
//! (03 §1.2-1.3; 01 §3.1-3.4). `write_material`, `pack_specular` and `pbr_params` moved here from `draw.rs`
//! (P0 borrowed `nif::props::Lighting`).
use crate::cbuffer::CbWriter;
use crate::permutation::{defines, runtime_bits, Material, Stage};
use crate::scene::srgb_to_linear;
use nif::material::MaterialState;
use nif::props::{LightingParams, LightingTrailer};
use std::collections::HashSet;

/// Registers bound to a real (non-default) texture in one draw. `pbr_flags` reads t4, t6, t9 and t12;
/// `EnvmapData.y` reads t5.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bound {
    pub regs: HashSet<u32>,
}

// CS 1.9 `PBR::Flags` (01 §3.4; CS BSLightingShaderMaterialPBR.h:17-31; iLH Common/PBRMath.hlsli:23-35).
pub const PBR_HAS_EMISSIVE: u32 = 1;
pub const PBR_HAS_DISPLACEMENT: u32 = 2;
pub const PBR_HAS_FEATURES0: u32 = 4;
pub const PBR_HAS_FEATURES1: u32 = 8;
pub const PBR_SUBSURFACE: u32 = 16;
pub const PBR_TWO_LAYER: u32 = 32;
pub const PBR_COLORED_COAT: u32 = 64;
pub const PBR_INTERLAYER_PARALLAX: u32 = 128;
pub const PBR_COAT_NORMAL: u32 = 256;
pub const PBR_FUZZ: u32 = 512;
pub const PBR_HAIR_MARSCHNER: u32 = 1024;
pub const PBR_GLINT: u32 = 2048;

const fn bit(v: u32, b: u32) -> bool {
    v & (1 << b) != 0
}

/// CS's Glint flag from the NIF flags (01 §3.1): TruePBR (SLSF2.23) and Fit_Slope (SLSF2.12), and none of the
/// branches CS tests first: MLP (24), Back (27), Soft (25). Drives both `PBRFlags` bit 2048 and the `GLINT`
/// define (P1 spec §2.2: not SLSF2.21, which CS clears at load).
pub fn remapped_glint(slsf2: u32) -> bool {
    bit(slsf2, 23) && bit(slsf2, 12) && !bit(slsf2, 24) && !bit(slsf2, 25) && !bit(slsf2, 27)
}

/// `PBRFlags` as CS packs it at load (01 §3.1 remap + §3.3 bound textures); 0 for non-PBR materials.
/// ```text
/// SLSF2.24 (MLP): TwoLayer; +InterlayerParallax if .25; +CoatNormal if .27; +ColoredCoat if .30
/// elif .27 (Back): HairMarschner
/// else: Subsurface if .26; Fuzz if .25, else Glint if .12
/// t6 → HasEmissive, t4 → HasDisplacement, t12 → HasFeatures0, t9 → HasFeatures1 (only when in `bound`)
/// ```
/// Capture oracle: Arch_Eye_Moisture ev6721 = 428 (answers.md Q1.3).
pub fn pbr_flags(m: &MaterialState, bound: &Bound) -> u32 {
    if !m.is_pbr() {
        return 0;
    }
    let f2 = m.slsf2;
    let mut f = 0;
    if bit(f2, 24) {
        f |= PBR_TWO_LAYER;
        if bit(f2, 25) {
            f |= PBR_INTERLAYER_PARALLAX;
        }
        if bit(f2, 27) {
            f |= PBR_COAT_NORMAL;
        }
        if bit(f2, 30) {
            f |= PBR_COLORED_COAT;
        }
    } else if bit(f2, 27) {
        f |= PBR_HAIR_MARSCHNER;
    } else {
        if bit(f2, 26) {
            f |= PBR_SUBSURFACE;
        }
        if bit(f2, 25) {
            f |= PBR_FUZZ;
        } else if bit(f2, 12) {
            f |= PBR_GLINT;
        }
    }
    for (reg, flag) in [
        (6, PBR_HAS_EMISSIVE),
        (4, PBR_HAS_DISPLACEMENT),
        (12, PBR_HAS_FEATURES0),
        (9, PBR_HAS_FEATURES1),
    ] {
        if bound.regs.contains(&reg) {
            f |= flag;
        }
    }
    f
}

/// The permutation-relevant subset of a material.
pub fn perm_material(m: &MaterialState) -> Material {
    Material {
        shader_type: m.shader_type,
        slsf1: m.slsf1,
        slsf2: m.slsf2,
        skinned: m.skinned,
        alpha_test: m.alpha.is_some(),
        has_vertex_colors: m.has_vertex_colors,
        specular_strength: m.params.specular_strength,
    }
}

/// `PerMaterial.SpecularColor` as the engine uploads it (answers.md Q5, frame11178 ev4217/5488/5536/6544):
/// `xyz = sRGB→linear(NIF specular colour) × NIF specular strength`, `w = NIF glossiness` (shininess,
/// iLH:973). The linearisation is the Linear Lighting effect (owner's CS has LL on); with LL off it is
/// `colour × strength` [Inference, Q5 open item]. Written only when SPECULAR is compiled; the PBR path
/// uses [`pbr_params`] instead.
pub fn pack_specular(p: &LightingParams, linear_lighting: bool) -> [f32; 4] {
    let c = p.specular_color.map(|c| {
        if linear_lighting {
            srgb_to_linear(c)
        } else {
            c
        }
    });
    let s = p.specular_strength;
    [c[0] * s, c[1] * s, c[2] * s, p.glossiness]
}

/// TruePBR `(PBRParams1, PBRParams2)` (answers.md Q5, Arch_Eye ev3891 = NIF exactly):
/// `PBRParams1 = (specStrength → roughness scale, lightingEffect2 → displacement scale, glossiness →
/// specular level)`, `PBRParams2 = (specular colour raw — not linearised —, lightingEffect1)`.
pub fn pbr_params(p: &LightingParams) -> ([f32; 3], [f32; 4]) {
    (
        [p.specular_strength, p.lighting_effect_2, p.glossiness],
        [
            p.specular_color[0],
            p.specular_color[1],
            p.specular_color[2],
            p.lighting_effect_1,
        ],
    )
}

/// The TruePBR block (01 §3.4): `PBRFlags` and `PBRParams1` always; `PBRParams2` only for TwoLayer or
/// Subsurface; `MultiLayerParallaxData` only for TwoLayer (coat roughness, coat specular level, 0, 0), Fuzz
/// (r, g, b, weight) or Glint (screen-space scale, 40 − log microfacet density, roughness, randomization).
/// The four MultiLayer floats are loaded only for shader type 11; CS zeroes them otherwise, so a leaked
/// Fit_Slope on type 0 packs (0, 40, 0, 0).
fn write_pbr(w: &mut CbWriter, m: &MaterialState, bound: &Bound) {
    let flags = pbr_flags(m, bound);
    let (p1, p2) = pbr_params(&m.params);
    w.set_u32("PBRFlags", &[flags]);
    w.set("PBRParams1", &p1);
    if flags & (PBR_TWO_LAYER | PBR_SUBSURFACE) != 0 {
        w.set("PBRParams2", &p2);
    }
    let f = match (m.shader_type, &m.params.trailer) {
        (
            11,
            LightingTrailer::MultiLayer {
                inner_thickness,
                refraction_scale,
                inner_texture_scale,
                ..
            },
        ) => [
            *inner_thickness,
            *refraction_scale,
            inner_texture_scale[0],
            inner_texture_scale[1],
        ],
        _ => [0.0; 4],
    };
    let mlpd = if flags & PBR_TWO_LAYER != 0 {
        Some([f[0], f[1], 0.0, 0.0])
    } else if flags & PBR_FUZZ != 0 {
        Some(f)
    } else if flags & PBR_GLINT != 0 {
        Some([f[0], 40.0 - f[1], f[2], f[3]])
    } else {
        None
    };
    if let Some(v) = mlpd {
        w.set("MultiLayerParallaxData", &v);
    }
}

/// P0's per-technique trailer fields, for non-PBR materials only (CS's PBR material has no envmap, tint,
/// parallax-occlusion or eye data; its MultiLayer floats go through [`write_pbr`]).
fn write_trailer(w: &mut CbWriter, p: &LightingParams, has_env_mask: bool) {
    let mask = if has_env_mask { 1.0 } else { 0.0 };
    // TintColor is not linearised (answers.md Q5: hair 6544 = NIF hairTint).
    match &p.trailer {
        LightingTrailer::SkinTint(c) | LightingTrailer::HairTint(c) => {
            w.set("TintColor", &[c[0], c[1], c[2], 1.0]);
        }
        LightingTrailer::EnvMap { scale } => {
            w.set("EnvmapData", &[*scale, mask, 0.0, 0.0]);
        }
        LightingTrailer::ParallaxOcc { max_passes, scale } => {
            w.set("ParallaxOccData", &[*max_passes, *scale, 0.0, 0.0]);
        }
        LightingTrailer::MultiLayer {
            inner_thickness,
            refraction_scale,
            inner_texture_scale,
            envmap_strength,
        } => {
            w.set(
                "MultiLayerParallaxData",
                &[
                    *inner_thickness,
                    *refraction_scale,
                    inner_texture_scale[0],
                    inner_texture_scale[1],
                ],
            );
            w.set("EnvmapData", &[*envmap_strength, mask, 0.0, 0.0]);
        }
        LightingTrailer::Eye {
            left_center,
            right_center,
            cubemap_scale,
        } => {
            w.set(
                "LeftEyeCenter",
                &[left_center[0], left_center[1], left_center[2], 0.0],
            );
            w.set(
                "RightEyeCenter",
                &[right_center[0], right_center[1], right_center[2], 0.0],
            );
            w.set("EnvmapData", &[*cubemap_scale, mask, 0.0, 0.0]);
        }
        LightingTrailer::SparkleSnow(s) => {
            w.set("SparkleParams", s);
        }
        LightingTrailer::None => {}
    }
}

/// CS `SkinPerGeometry.skinMaterialFlags` of one draw (02-skin-rfaos.md §0.5): x = an RFAOS map is bound at t71,
/// w = a skin shape (type 4/5) names a slot 2 `_sk` map (the installed build's `entry.thickness`); y (wetness) and z
/// (detail normal-only) stay 0. Called after `write_static`, whose FeatureData values hold a global zero.
pub fn write_skin_flags(w: &mut CbWriter, m: &MaterialState, bound: &Bound) {
    let x = bound.regs.contains(&71);
    let sk = matches!(m.shader_type, 4 | 5) && m.slots.get(2).is_some_and(|p| !p.norm.is_empty());
    w.set(
        "skinMaterialFlags",
        &[f32::from(u8::from(x)), 0.0, 0.0, f32::from(u8::from(sk))],
    );
}

/// Writes the material fields into one cbuffer of any stage. The writer must be zero (fresh from
/// `CbWriter::new` or after `CbWriter::zero`): fields a branch does not write must read 0, never a previous
/// material's value. `bound` = registers bound to a real texture in this draw.
pub fn write_material(w: &mut CbWriter, m: &MaterialState, bound: &Bound, linear_lighting: bool) {
    let p = &m.params;
    let pbr = m.is_pbr();
    w.set(
        "TexcoordOffset",
        &[p.uv_offset[0], p.uv_offset[1], p.uv_scale[0], p.uv_scale[1]],
    );
    // SPECULAR is compiled only for vanilla shapes with Specular set and a non-zero strength (permutation.rs).
    if !pbr && bit(m.slsf1, 0) && p.specular_strength > 0.0 {
        w.set("SpecularColor", &pack_specular(p, linear_lighting));
    }
    if pbr {
        write_pbr(w, m, bound);
    } else {
        write_trailer(w, p, bound.regs.contains(&5));
    }
    w.set(
        "LightingEffectParams",
        &[p.lighting_effect_1, p.lighting_effect_2, 0.0, 0.0],
    );
    w.set("MaterialObjectRGBScale", &[1.0, 1.0, 1.0]);
    // EmitColor: "sRGB by default, linear if LL on" (iLH ~2595).
    // - Vanilla (P0 rule): only Own_Emit (SLSF1.22) shapes get the NIF colour × multiple [Inference: every
    //   captured vanilla EmitColor was 0].
    // - PBR (spec D1, settled by captures C1/frame6171 with LL on): sRGB-to-linear(NIF colour) × multiple, exact
    //   to 7 digits on a controller-free mesh. Only Own_Emit PBR shapes were captured; the rest get the same rule.
    //   An animated mesh (BSLightingShaderPropertyColor/FloatController) differs per frame; the preview is static.
    let emit = if pbr || bit(m.slsf1, 22) {
        p.emissive_color.map(|c| {
            (if linear_lighting {
                srgb_to_linear(c)
            } else {
                c
            }) * p.emissive_multiple
        })
    } else {
        [0.0; 3]
    };
    w.set("EmitColor", &emit);
    w.set("emissiveMult", &[p.emissive_multiple]);
    // envmap LOD fade, specular LOD fade, alpha.
    w.set("MaterialData", &[1.0, 1.0, p.alpha, 0.0]);
    // b11 AlphaTestRefRS = threshold/255 only when the NiAlphaProperty test flag is set; b2 AlphaTestRef stays
    // 0 (frame11178: Hair ev6544 test 40 → 0.1569, Lashes ev5536 test 5 → 0.0196, blend-only Scalp ev5488
    // threshold 50 → 0; b2 AlphaTestRef = 0 on all of them). Blend-only shapes still compile DO_ALPHA_TEST.
    let tref = m
        .alpha
        .filter(|a| a.test())
        .map_or(0.0, |a| a.threshold as f32 / 255.0);
    w.set("AlphaTestRefRS", &[tref]);
    // cb4: engine PS desc & ~compile key = runtime bits only (answers.md Q4.1, B5 `runtime_bits`).
    w.set_u32("PixelShaderDescriptor", &[runtime_bits(&perm_material(m))]);
    // InWorld (bit 0); SourceAlphaBlend (bit 8) only for blended passes (answers.md Q4.2).
    w.set_u32(
        "ExtraShaderDescriptor",
        &[1 | if m.blend() { 1 << 8 } else { 0 }],
    );
    // Identity bones for all 240 rows (P0 bind pose); the camera-relative shift is BonesPivot (Scene).
    let bones: Vec<f32> = (0..80)
        .flat_map(|_| [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0])
        .collect();
    w.set("Bones", &bones);
    w.set("PreviousBones", &bones);
}

/// How a material change reaches the screen (03 §1.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    /// Nothing changed.
    Same,
    /// Only cbuffer fields (and possibly blending / `hidden`): rewrite the cbuffers, no compile.
    Cbuffer,
    /// A slot path or the slot → register mapping changed: rebind textures, then rewrite the cbuffers.
    Textures,
    /// The VS or PS define set changed: a new program and a new draw.
    Permutation,
}

/// Which texture each slot feeds: `(register, canonical path)` per slot (`textures::register_for_slot`).
fn bindings(m: &MaterialState) -> Vec<(Option<u32>, &str)> {
    let msn = bit(m.slsf1, 12);
    m.slots
        .iter()
        .enumerate()
        .map(|(i, s)| {
            (
                crate::textures::register_for_slot(m.shader_type, m.is_pbr(), msn, i),
                s.norm.as_str(),
            )
        })
        .collect()
}

/// Sorts an edit by what it costs. The permutation oracle is `permutation::defines` (03 §1.2): `Permutation`
/// iff the VS or PS define set differs. Otherwise `Textures` iff a slot's register or canonical path differs
/// (e.g. a new path, or type 0 ↔ 2 moving slot 2 between t12 and t6), else `Cbuffer`. A change of `hidden` alone
/// is `Cbuffer`.
pub fn classify(old: &MaterialState, new: &MaterialState, features: &[&str]) -> EditKind {
    if old == new {
        return EditKind::Same;
    }
    let (a, b) = (perm_material(old), perm_material(new));
    for stage in [Stage::Vertex, Stage::Pixel] {
        if defines(&a, stage, features, false) != defines(&b, stage, features, false) {
            return EditKind::Permutation;
        }
    }
    if bindings(old) != bindings(new) {
        EditKind::Textures
    } else {
        EditKind::Cbuffer
    }
}

/// One shape's blend, cull and depth state (v2 A14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pipe {
    /// The `NiAlphaProperty` (source, destination) AlphaFunction after [`engine_blend`]; `None` = blending off.
    pub blend: Option<(u8, u8)>,
    /// The pair was remapped by the engine (G3), so it differs from what the NIF says.
    pub mapped: bool,
    pub cull_back: bool,
    pub z_test: bool,
    pub z_write: bool,
}

impl Pipe {
    /// Built-in shapes keep the P0 look: alpha blend 6/7, no culling, depth test and write on.
    pub const FIXED: Pipe = Pipe {
        blend: Some((6, 7)),
        mapped: false,
        cull_back: false,
        z_test: true,
        z_write: true,
    };
}

/// What the game draws for a NIF blend pair (G3): codes 11-15 act as ONE first; (6,7) is kept, (0,0) draws as
/// (6,0) and (1,2) as (4,1); any other pair is unchanged. The bool is true when the pair was remapped.
pub fn engine_blend(src: u8, dst: u8) -> ((u8, u8), bool) {
    let fold = |c: u8| if c >= 11 { 0 } else { c };
    match (fold(src), fold(dst)) {
        (0, 0) => ((6, 0), true),
        (1, 2) => ((4, 1), true),
        p => (p, false),
    }
}

/// The three pairs G3 measured in game.
pub fn verified_blend(src: u8, dst: u8) -> bool {
    matches!((src, dst), (6, 7) | (0, 0) | (1, 2))
}

/// The draw state a material asks for: alpha bits 1-4 / 5-8 when bit 0 is set; back-face culling unless
/// Double_Sided (SLSF2 bit 4); depth test SLSF1 bit 31; depth write SLSF2 bit 0.
pub fn pipe(m: &MaterialState) -> Pipe {
    let (blend, mapped) = match m.alpha.filter(|a| a.blend()) {
        Some(a) => {
            let (p, mapped) =
                engine_blend(((a.flags >> 1) & 0xF) as u8, ((a.flags >> 5) & 0xF) as u8);
            (Some(p), mapped)
        }
        None => (None, false),
    };
    Pipe {
        blend,
        mapped,
        cull_back: m.slsf2 & (1 << 4) == 0,
        z_test: m.slsf1 & (1 << 31) != 0,
        z_write: m.slsf2 & 1 != 0,
    }
}
