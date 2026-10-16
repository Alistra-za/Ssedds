//! NIF material → CS Lighting.hlsl permutation defines (plan 04 §1.2, corrected by real captures:
//! tools/rdc-out/p04/answers.md Q4, controller ruling RB7).

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
    // TruePBR: CS enables glint from its remapped Glint flag (Fit_Slope without MLP/Soft/Back; 01 §3.1, P1 spec
    // §2.2); SLSF2.21 is cleared at load. Vanilla: Anisotropic_Lighting (SLSF2.21) → ANISO_LIGHTING.
    if pbr && crate::material::remapped_glint(m.slsf2) {
        out.push("GLINT");
    } else if ps && !pbr && bit(m.slsf2, 21) {
        out.push("ANISO_LIGHTING");
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
