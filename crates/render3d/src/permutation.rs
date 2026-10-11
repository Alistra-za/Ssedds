//! NIF material → CS Lighting.hlsl permutation defines (plan 04 §1.2, corrected by real captures:
//! tools/rdc-out/p04/answers.md Q4, controller ruling RB7).
use nif::props::{Alpha, Lighting};
use nif::shape_head::ShapeHead;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Material {
    pub shader_type: u32,
    pub slsf1: u32,
    pub slsf2: u32,
    pub skinned: bool,
    /// The shape has an NiAlphaProperty (blend or test): CS sets DoAlphaTest for both (answers.md Q4.2).
    pub alpha_test: bool,
    pub has_vertex_colors: bool,
    /// NIF specular strength; SPECULAR is dropped when it is 0 (answers.md Q4.2, Arch_Mouth).
    pub specular_strength: f32,
}

/// BSVertexDesc: the VertexAttribute flags sit at bits 44..56, `Vertex_Colors` is attribute bit 5
/// (nif.xml `VertexAttribute` / `BSVertexDesc`).
pub fn desc_has_vertex_colors(vertex_desc: u64) -> bool {
    (vertex_desc >> 44) & (1 << 5) != 0
}

impl Material {
    pub fn from_shape(head: &ShapeHead, lighting: &Lighting, alpha: Option<Alpha>) -> Material {
        Material {
            shader_type: lighting.shader_type,
            slsf1: lighting.slsf1,
            slsf2: lighting.slsf2,
            skinned: head.skinned,
            alpha_test: alpha.is_some(),
            has_vertex_colors: head.vertex_desc.is_some_and(desc_has_vertex_colors),
            specular_strength: lighting.params.specular_strength,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Stage {
    Vertex,
    Pixel,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Defines(pub Vec<(String, String)>);

const fn bit(v: u32, b: u32) -> bool {
    v & (1 << b) != 0
}

fn technique(shader_type: u32) -> &'static [&'static str] {
    match shader_type {
        1 => &["ENVMAP"],
        3 => &["PARALLAX"],
        4 => &["FACEGEN"],
        5 => &["FACEGEN_RGB_TINT"],
        6 => &["HAIR"],
        11 => &["MULTI_LAYER_PARALLAX", "ENVMAP"],
        16 => &["EYE"],
        // 2 = Glowmap: runtime technique (cb4 PixelShaderDescriptor >> 24, Lighting.hlsl:2598).
        _ => &[],
    }
}

pub fn defines(m: &Material, stage: Stage, features: &[&str], deferred: bool) -> Defines {
    let ps = stage == Stage::Pixel;
    let pbr = ps && bit(m.slsf2, 23);
    let mut out: Vec<&str> = vec![if ps { "PSHADER" } else { "VSHADER" }];
    let vs_cleared = matches!(m.shader_type, 2 | 3 | 4 | 5 | 6 | 13 | 14 | 15);
    if ps || !vs_cleared {
        // CS drops the EnvMap/Parallax/MLP technique for TruePBR on both stages (captured Arch_Eye_Moisture:
        // type 11 + SLSF2.23 → PS key 100009 and VS desc 2, technique 0). Keeping them collides with the PBR
        // samplers (Lighting.hlsl:360-371: s4/s5 overlap, SampParallaxSampler redefinition).
        let pbr_material = bit(m.slsf2, 23);
        out.extend(technique(m.shader_type).iter().filter(|t| {
            !(pbr_material && matches!(**t, "ENVMAP" | "PARALLAX" | "MULTI_LAYER_PARALLAX"))
        }));
    }
    if m.skinned {
        out.push("SKINNED");
    }
    if bit(m.slsf1, 12) {
        out.push("MODELSPACENORMALS");
    }
    if bit(m.slsf1, 23) {
        out.push("PROJECTED_UV");
    }
    if !ps && bit(m.slsf2, 5) && m.has_vertex_colors {
        out.push("VC");
    }
    if pbr {
        out.push("TRUE_PBR");
    }
    // PS only: the VS never reads it, and captured VS descriptors lack bit 20 (answers.md Q4.2).
    if ps && m.alpha_test {
        out.push("DO_ALPHA_TEST");
    }
    // HAIR + Decal/Dynamic_Decal (SLSF1.26/27) → DepthWriteDecals (answers.md Q4.2, Arch_Scalp/Lashes).
    // PS only: read at Lighting.hlsl:884, 892, 3036 (all in the PSHADER section).
    if ps && m.shader_type == 6 && (bit(m.slsf1, 26) || bit(m.slsf1, 27)) {
        out.push("DEPTH_WRITE_DECALS");
    }
    if ps && bit(m.slsf2, 21) {
        out.push(if pbr { "GLINT" } else { "ANISO_LIGHTING" });
    }
    // Compile-time in installed CS 1.9, PS only, and not under TruePBR (answers.md Q4.1/Q4.2).
    if ps && !pbr {
        if bit(m.slsf1, 0) && m.specular_strength > 0.0 {
            out.push("SPECULAR");
        }
        if bit(m.slsf2, 25) {
            out.push("SOFT_LIGHTING");
        }
        if bit(m.slsf2, 26) {
            out.push("RIM_LIGHTING");
        }
        if bit(m.slsf2, 27) {
            out.push("BACK_LIGHTING");
        }
    }
    if deferred {
        out.push("DEFERRED");
    }
    let mut d: Vec<(String, String)> = vec![];
    for n in out.into_iter().chain(features.iter().copied()) {
        if !d.iter().any(|(k, _)| k == n) {
            d.push((n.to_string(), "1".to_string()));
        }
    }
    Defines(d)
}

/// The bits cb4 `PixelShaderDescriptor` still carries for this material. In installed CS 1.9 cb4 is
/// `engine PS desc & ~CS compile key` (answers.md Q4.1): Specular/Soft/Rim/Back are compile-time defines and
/// CharacterLight/DefShadow stay 0 in the preview. Only the Glowmap technique is read at run time
/// (`(PixelShaderDescriptor >> 24) & 0x3F == Glowmap`, Lighting.hlsl:2598), so type 2 → `2 << 24`, else 0.
pub fn runtime_bits(m: &Material) -> u32 {
    if m.shader_type == 2 {
        2 << 24
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertex_colour_bit_from_desc() {
        // VF_Vertex | VF_UVs | VF_Normals | VF_Tangents | VF_Vertex_Colors = 0x3B at bit 44.
        assert!(desc_has_vertex_colors(0x3Bu64 << 44 | 0x408));
        assert!(!desc_has_vertex_colors(0x1Bu64 << 44 | 0x408));
    }
}
