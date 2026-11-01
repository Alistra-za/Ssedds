//! Validation (spec §3.3): warns, never clamps or normalises. Reads the model only.
use super::{anchor_role, Anchor, Op, Role};
use crate::ctrl::TimeCtrl;
use crate::extra::{Extra, ExtraValue};
use crate::fields::Field;
use crate::keys::{KeyData, KeyGroup};
use crate::material::{SLSF1_SPECULAR, SLSF2_TRUE_PBR};
use crate::model::NifModel;
use crate::path::canon_texture;
use crate::props::LightingTrailer;
use std::collections::BTreeSet;

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Warn,
    Error,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Field { block: u32, field: Field },
    Slot { set: u32, slot: u32 },
    Extra { block: u32 },
    Block { block: u32 },
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct Warning {
    pub level: Level,
    pub target: Target,
    pub text: String,
    pub fix: Option<Op>,
}

const SLSF1_OWN_EMIT: u32 = 1 << 22;
const SLSF1_EXTERNAL_EMITTANCE: u32 = 1 << 29;
const BSX_EXTERNAL_EMIT: u32 = 1 << 9;
pub(crate) const LSCF_VARS: [u32; 10] = [0, 8, 9, 10, 11, 12, 20, 21, 22, 23];
pub(crate) const LSCF: &str = "BSLightingShaderPropertyFloatController";
pub(crate) const LSCC: &str = "BSLightingShaderPropertyColorController";
pub(crate) const TESTREF: &str = "BSNiAlphaPropertyTestRefController";
/// SLSF2 bit 24 (TwoLayer) and bit 26 (Subsurface) decide what a PBR material's specular colour does.
const SLSF2_TWO_LAYER: u32 = 1 << 24;
const SLSF2_SUBSURFACE: u32 = 1 << 26;

/// B9 I11-P08: LSCF var 8 on a PBR shader crashed the game. An Error in every file, never only a new one (B-P39).
pub const ENV_SCALE_PBR: &str = "Env map scale crashes the game on PBR materials.";
/// B9 I6-R10Z: the specular permutation is fixed at load.
pub const SPECULAR_OFF: &str =
    "This controller has no effect: Specular is off when the mesh loads.";
/// B9 I6-R10Z (B-P40): a static strength of 0 drops the specular permutation at load too.
pub const SPECULAR_ZERO: &str =
    "This controller has no effect: specular strength is 0 when the mesh loads.";
/// B9 RTRB: the ref is unused without the test bit.
pub const ALPHA_TEST_OFF: &str = "This controller has no effect: alpha test is off.";
/// B9 R00 / I9-P00: the cbuffer follows but nothing shows on screen.
pub const REFRACTION_NO_EFFECT: &str = "Refraction strength has no visible effect in game.";

/// Note CS (owner): SLSF1 bit 21 FaceGen_RGB_Tint off shader type 5 (Skin Tint) crashes the game.
pub const SKIN_TINT_TYPE: &str =
    "Skin tint flag needs the Skin Tint shader type: the game crashes otherwise.";
/// Note CS (owner): SLSF1 bit 10 Facegen_Detail_Map off shader types 4 (Face Tint) and 5 (Skin Tint, CS3) renders black.
pub const FACEGEN_DETAIL_TYPE: &str =
    "FaceGen detail flag needs the Face Tint or Skin Tint shader type: the mesh renders black otherwise.";
/// Note CS (owner): type 4 without bit 10 is wrong too (every vanilla Face Tint shape has it).
pub const FACE_TINT_FLAG: &str =
    "Face Tint shader type needs the FaceGen detail flag: the face renders wrong otherwise.";
/// Note CU8 (C6E): Skyrim crashes in Load3D on a NiControllerManager that lists no sequences.
pub const EMPTY_MANAGER: &str =
    "This file has a NiControllerManager with no clips: the game crashes on it.";
/// Heel data (HH_OFFSET or SDTA) on the root or a property: RaceMenu reads it only on a shape (HH-results).
pub const HEEL_NOT_ON_SHAPE: &str = "The game ignores heel height here. Put it on a shape.";
/// Heel data on a shape after the first shape that has some: the first shape wins (HH-results).
pub const HEEL_EARLIER_WINS: &str = "An earlier shape's heel height wins.";
/// Note DJ (cubemap-up-axis A/B, 2026-10-27): SLSF2 bit 24 without PBR on an Environment Map shader crashes the game.
/// Note DK (owner): on every type but 11.
pub const MLP_TYPE: &str =
    "Multi-layer parallax flag (SLSF2 24) needs shader type 11: the game crashes otherwise.";
/// Note DK (owner): SLSF1 bit 18 Hair_Soft_Lighting off shader type 6 (Hair Tint) draws nothing.
pub const HAIR_SOFT_TYPE: &str =
    "Hair soft lighting flag needs the Hair Tint shader type: the shape is invisible otherwise.";
/// Note DK (owner): the Eye shader has no glow map, so SLSF2 bit 6 there does nothing.
pub const GLOW_ON_EYE: &str = "Glow map has no effect on an Eye shader.";
/// Note DK (owner): SLSF1 bit 17 Eye_Environment_Mapping belongs to the Eye shader; often set by mistake on type 1.
pub const EYE_ENV_TYPE: &str =
    "Eye environment mapping is for the Eye shader type. Use Environment_Mapping here.";
/// CM27 / Note GD6: on a PBR material CS's default white RMAOS sets metallic to 1, so an empty NIF slot 5 (index; the
/// 1-based slot 6 of the set editor, Note GH7) is fully metallic.
pub const EMPTY_RMAOS: &str = "Slot 6 (RMAOS) is empty: in game the mesh renders fully metallic";
/// Errors that block export even when the source already has them: each one crashes or blacks out in game.
const ALWAYS_BLOCK: [&str; 6] = [
    ENV_SCALE_PBR,
    SKIN_TINT_TYPE,
    FACEGEN_DETAIL_TYPE,
    FACE_TINT_FLAG,
    MLP_TYPE,
    HAIR_SOFT_TYPE,
];

/// The SLSF1 bits `shader_type` gets wrong (Note CS), as (mask, value it needs, error, notice for a type change).
pub fn type_flag_errors(
    shader_type: u32,
    slsf1: u32,
) -> Vec<(u32, u32, &'static str, &'static str)> {
    let (skin, detail, hair) = (1 << 21, 1 << 10, 1 << 18);
    let mut out = vec![];
    if slsf1 & hair != 0 && shader_type != 6 {
        let n = "Hair soft lighting flag cleared: it needs the Hair Tint shader type.";
        out.push((hair, 0, HAIR_SOFT_TYPE, n));
    }
    if slsf1 & skin != 0 && shader_type != 5 {
        let n = "Skin tint flag cleared: it needs the Skin Tint shader type.";
        out.push((skin, 0, SKIN_TINT_TYPE, n));
    }
    match (slsf1 & detail != 0, shader_type) {
        (true, t) if t != 4 && t != 5 => {
            let n = "FaceGen detail flag cleared: it needs the Face Tint or Skin Tint shader type.";
            out.push((detail, 0, FACEGEN_DETAIL_TYPE, n));
        }
        (false, 4) => {
            let n = "FaceGen detail flag set: the Face Tint shader type needs it.";
            out.push((detail, detail, FACE_TINT_FLAG, n));
        }
        _ => {}
    }
    out
}

/// Note DJ: a type change off 11 clears SLSF2 bit 24 unless PBR (bit 23) makes it the two-layer coat,
/// as (mask, value, notice).
pub fn type_slsf2_fix(shader_type: u32, slsf2: u32) -> Option<(u32, u32, &'static str)> {
    let stale = shader_type != 11 && slsf2 & (1 << 24) != 0 && slsf2 & SLSF2_TRUE_PBR == 0;
    stale.then_some((
        1 << 24,
        0,
        "Multi-layer parallax flag cleared: it needs shader type 11.",
    ))
}

/// Why an SLSF1 checkbox is greyed on `shader_type` (Note CS): the bit cannot apply to it.
pub fn type_flag_lock(shader_type: u32, bit: u32) -> Option<&'static str> {
    match bit {
        21 if shader_type != 5 => Some("Needs the Skin Tint shader type."),
        18 if shader_type != 6 => Some("Needs the Hair Tint shader type."),
        10 if shader_type != 4 && shader_type != 5 => {
            Some("Needs the Face Tint or Skin Tint shader type.")
        }
        _ => None,
    }
}

/// A variable's label. Under CS TruePBR (SLSF2 bit 23) three read differently, as on the Shader tab (B9 design §1).
pub fn var_label(class: &str, v: u32, slsf2: u32) -> String {
    let pbr = slsf2 & SLSF2_TRUE_PBR != 0;
    let known = match (class, v) {
        (LSCF, 9) if pbr => "Specular level",
        (LSCF, 10) if pbr => "Roughness scale",
        (LSCC, 0) if pbr && slsf2 & SLSF2_TWO_LAYER != 0 => "Coat colour",
        (LSCC, 0) if pbr && slsf2 & SLSF2_SUBSURFACE != 0 => "Subsurface colour",
        (LSCC, 0) if pbr => "Specular colour (no effect on PBR)",
        (LSCF, 0) => "Refraction strength",
        (LSCF, 8) => "Env map scale",
        (LSCF, 9) => "Glossiness",
        (LSCF, 10) => "Specular strength",
        (LSCF, 11) => "Emissive multiple",
        (LSCF, 12) => "Alpha",
        (LSCF, 20) => "U offset",
        (LSCF, 21) => "U scale",
        (LSCF, 22) => "V offset",
        (LSCF, 23) => "V scale",
        (LSCC, 0) => "Specular colour",
        (LSCC, 1) => "Emissive colour",
        _ => return format!("Variable {v}"),
    };
    known.to_string()
}

/// A controller's title: its variable's label, "Alpha test threshold" for TestRef, the class when unknown.
pub fn ctrl_title(c: &TimeCtrl, slsf2: u32) -> String {
    match (c.known, c.var) {
        (false, _) => c.class.to_string(),
        (true, Some(v)) => var_label(&c.class, v, slsf2),
        (true, None) => "Alpha test threshold".to_string(),
    }
}

pub fn validate(m: &NifModel, exists: Option<&dyn Fn(&str) -> bool>) -> Vec<Warning> {
    let mut out = vec![];
    let mut push = |level, target, text: &str, fix| {
        out.push(Warning {
            level,
            target,
            text: text.to_string(),
            fix,
        })
    };
    let fld = |block, field| Target::Field { block, field };

    let bsx = root_bsx(m);
    for (&block, l) in &m.lighting {
        let (lt, p) = (&l.lighting, &l.lighting.params);
        let mut range = |label: &str, v: f32, lo: f32, hi: f32, field| {
            // NaN is outside every range.
            if !(lo..=hi).contains(&v) {
                let t = format!("{label} {v} is outside {lo}-{hi}");
                push(Level::Warn, fld(block, field), &t, None);
            }
        };
        range(
            "Emissive Multiple",
            p.emissive_multiple,
            0.0,
            100.0,
            Field::EmissiveMultiple,
        );
        range(
            "Specular Strength",
            p.specular_strength,
            0.0,
            10.0,
            Field::SpecularStrength,
        );
        range("Glossiness", p.glossiness, 0.0, 999.0, Field::Glossiness);
        range("Alpha", p.alpha, 0.0, 128.0, Field::Alpha);
        range(
            "Refraction",
            p.refraction_strength,
            0.0,
            1.0,
            Field::Refraction,
        );
        match p.trailer {
            LightingTrailer::EnvMap { scale } => {
                range("Env scale", scale, 0.0, 10.0, Field::Trailer(0))
            }
            LightingTrailer::MultiLayer {
                inner_thickness,
                refraction_scale,
                ..
            } => {
                range(
                    "MLP thickness",
                    inner_thickness,
                    5.0,
                    500.0,
                    Field::Trailer(0),
                );
                range(
                    "MLP refraction scale",
                    refraction_scale,
                    0.0,
                    1.0,
                    Field::Trailer(1),
                );
            }
            LightingTrailer::ParallaxOcc { max_passes, .. } => {
                range("POM passes", max_passes, 1.0, 320.0, Field::Trailer(0))
            }
            _ => {}
        }
        if p.texture_clamp_mode > 3 {
            let t = format!("Clamp mode {} is not 0-3", p.texture_clamp_mode);
            push(Level::Warn, fld(block, Field::ClampMode), &t, None);
        }
        if lt.shader_type == 20 {
            push(
                Level::Error,
                fld(block, Field::ShaderType),
                "Shader type 20 is FO4 only",
                None,
            );
        }
        if p.emissive_color.iter().any(|&c| c != 0.0) && lt.slsf1 & SLSF1_OWN_EMIT == 0 {
            let t = "No effect in game: Own_Emit is off";
            push(Level::Warn, fld(block, Field::EmissiveColor(0)), t, None);
        }
        let pbr = lt.slsf2 & (1 << 23) != 0;
        let mlp = lt.slsf2 & (1 << 24) != 0;
        let slsf2 = fld(block, Field::Slsf2);
        if lt.shader_type == 11 && !pbr && !mlp {
            let t = "Shader type 11 needs the Multi_Layer_Parallax flag (SLSF2 24) without PBR.";
            push(Level::Warn, slsf2.clone(), t, None);
        }
        let shader_at = l
            .prop
            .users
            .first()
            .and_then(|&s| anchor_role(m, Some(s), Role::Shader));
        if mlp && !pbr && lt.shader_type != 11 && !l.prop.users.is_empty() {
            let fix = shader_at.clone().map(|at| Op::SetBits {
                at,
                field: Field::Slsf2,
                mask: 1 << 24,
                value: 0,
                base: lt.slsf2,
            });
            push(Level::Error, slsf2.clone(), MLP_TYPE, fix);
        } else if mlp && lt.shader_type != 11 {
            let t = "Multi_Layer_Parallax (SLSF2 24) is set but the shader type is not 11.";
            push(Level::Warn, slsf2.clone(), t, None);
        }
        if lt.shader_type == 16 && lt.slsf2 & (1 << 6) != 0 {
            let fix = shader_at.clone().map(|at| Op::SetBits {
                at,
                field: Field::Slsf2,
                mask: 1 << 6,
                value: 0,
                base: lt.slsf2,
            });
            push(Level::Warn, slsf2.clone(), GLOW_ON_EYE, fix);
        }
        if lt.shader_type != 16 && lt.slsf1 & (1 << 17) != 0 {
            let fix = shader_at.clone().map(|at| Op::SetBits {
                at,
                field: Field::Slsf1,
                mask: 1 << 17,
                value: 0,
                base: lt.slsf1,
            });
            push(Level::Warn, fld(block, Field::Slsf1), EYE_ENV_TYPE, fix);
        }
        if !pbr && lt.slsf2 & (1 << 12) != 0 {
            let t = "Fit_Slope (SLSF2 12) does nothing without the PBR flag.";
            push(Level::Warn, slsf2, t, None);
        }
        // S-P2: a shader no shape uses is never drawn, so its flags cannot crash or render black.
        let errs = match l.prop.users.is_empty() {
            true => vec![],
            false => type_flag_errors(lt.shader_type, lt.slsf1),
        };
        for (mask, value, text, _) in errs {
            let fix = shader_at.clone().map(|at| Op::SetBits {
                at,
                field: Field::Slsf1,
                mask,
                value,
                base: lt.slsf1,
            });
            push(Level::Error, fld(block, Field::Slsf1), text, fix);
        }
        let ext = lt.slsf1 & SLSF1_EXTERNAL_EMITTANCE != 0;
        let have = bsx.as_ref().map_or(0, |b| b.1);
        if ext != (have & BSX_EXTERNAL_EMIT != 0) {
            let fix = bsx.as_ref().map(|(at, base)| Op::EditExtra {
                at: at.clone(),
                value: ExtraValue::Bsx(if ext {
                    base | BSX_EXTERNAL_EMIT
                } else {
                    base & !BSX_EXTERNAL_EMIT
                }),
                base: ExtraValue::Bsx(*base),
            });
            let t = "External_Emittance does not match BSX ExternalEmit";
            push(Level::Warn, fld(block, Field::Slsf1), t, fix);
        }
        // B-P39: what the B9 in-game check found per controller of this shader. Source files only get Warnings,
        // except the var-8 crash.
        let specular = pbr || lt.slsf1 & SLSF1_SPECULAR != 0;
        let chain: Vec<_> = l.prop.ctrls.iter().filter_map(|c| m.ctrls.get(c)).collect();
        for (i, c) in chain.iter().enumerate() {
            let tgt = Target::Block { block: c.block };
            match (&*c.class, c.var) {
                (LSCF, Some(8)) if pbr => push(Level::Error, tgt.clone(), ENV_SCALE_PBR, None),
                // B-P41: glossiness (9) too, per B9 I6-R10Z (CM22).
                (LSCF, Some(9 | 10)) | (LSCC, Some(0)) if !specular => {
                    push(Level::Warn, tgt.clone(), SPECULAR_OFF, None)
                }
                (LSCF, Some(9 | 10)) | (LSCC, Some(0)) if !pbr && p.specular_strength == 0.0 => {
                    push(Level::Warn, tgt.clone(), SPECULAR_ZERO, None)
                }
                (LSCF, Some(0)) => push(Level::Warn, tgt.clone(), REFRACTION_NO_EFFECT, None),
                _ => {}
            }
            // B9 I2-RDUP: the last controller in the chain wins; warn on each one that does not play.
            if c.known
                && chain[i + 1..]
                    .iter()
                    .any(|d| d.class == c.class && d.var == c.var)
            {
                let t = format!(
                    "Two controllers drive {}: only the last one plays.",
                    ctrl_title(c, lt.slsf2)
                );
                push(Level::Warn, tgt, &t, None);
            }
        }
    }

    for (&block, a) in &m.alphas {
        for &c in &a.prop.ctrls {
            if m.ctrls.get(&c).is_some_and(|c| &*c.class == TESTREF) && !a.alpha.test() {
                push(
                    Level::Warn,
                    Target::Block { block: c },
                    ALPHA_TEST_OFF,
                    None,
                );
            }
        }
        for mode in [(a.alpha.flags >> 1) & 15, (a.alpha.flags >> 5) & 15] {
            if mode >= 11 {
                let t = format!("Blend mode {mode} reads as ONE (invalid)");
                push(Level::Warn, fld(block, Field::AlphaFlags), &t, None);
            }
        }
    }

    for (&block, c) in &m.ctrls {
        if (c.flags >> 1) & 3 == 3 {
            push(
                Level::Warn,
                fld(block, Field::CtrlFlags),
                "Cycle type 3 is not 0-2",
                None,
            );
        }
        if c.frequency.is_nan() || c.frequency <= 0.0 {
            let t = "Frequency ≤ 0 freezes the controller";
            push(Level::Warn, fld(block, Field::Frequency), t, None);
        }
        if c.start > c.stop {
            push(
                Level::Warn,
                fld(block, Field::StartTime),
                "Start is after stop",
                None,
            );
        }
        if let (true, Some(v)) = (
            &*c.class == "BSLightingShaderPropertyFloatController",
            c.var,
        ) {
            if !LSCF_VARS.contains(&v) {
                let t = format!("Variable {v} is not a known Skyrim value (kept)");
                push(Level::Warn, fld(block, Field::Variable), &t, None);
            }
        }
    }

    // Note CU8: a manager the root's chain reaches that lists nothing (an unlinked one is never loaded)
    let mut at = m.root_net.as_ref().map_or(-1, |n| n.controller);
    for _ in 0..=m.ctrls.len() {
        let Some(c) = u32::try_from(at).ok().and_then(|b| m.ctrls.get(&b)) else {
            break;
        };
        if m.managers.get(&c.block) == Some(&0) {
            let tgt = Target::Block { block: c.block };
            push(Level::Error, tgt, EMPTY_MANAGER, None);
        }
        at = c.next;
    }

    for (&block, e) in &m.extras {
        if &*e.kind == "BSXFlags" && e.name.as_deref() != Some(b"BSX") {
            push(
                Level::Error,
                Target::Extra { block },
                "BSXFlags must be named BSX",
                None,
            );
        }
    }

    // Meshes UI T5b (HH-results, 2026-10-22): RaceMenu reads heel data only on a shape, and the first shape wins.
    let heel = |x: &&Extra| {
        x.name.as_deref().is_some_and(|n| {
            n.eq_ignore_ascii_case(b"HH_OFFSET") || n.eq_ignore_ascii_case(b"SDTA")
        })
    };
    let on_shape: BTreeSet<u32> = m
        .shapes
        .iter()
        .flat_map(|s| s.net.extra.iter().filter_map(|&r| u32::try_from(r).ok()))
        .collect();
    // T5b review Minor 7: any block but a shape (root, other nodes, any shader or property, nothing at all).
    for x in m
        .extras
        .values()
        .filter(heel)
        .filter(|x| !on_shape.contains(&x.block))
    {
        push(
            Level::Warn,
            Target::Extra { block: x.block },
            HEEL_NOT_ON_SHAPE,
            None,
        );
    }
    // Controller ruling (T5b fix round 1): only shapes the root reaches count, in block order.
    // shortcut: block order is unverified against scene-graph order (HH-results cannot tell them apart); settle it
    // in game with two heeled shapes whose block order and child order disagree (swap them under the root).
    let mut first = true;
    for s in m.shapes.iter().filter(|s| m.index3d.contains_key(&s.block)) {
        let mine: Vec<u32> = m
            .extras_of(&s.net)
            .into_iter()
            .filter(heel)
            .map(|x| x.block)
            .collect();
        if !mine.is_empty() && !std::mem::take(&mut first) {
            for b in mine {
                push(
                    Level::Warn,
                    Target::Extra { block: b },
                    HEEL_EARLIER_WINS,
                    None,
                );
            }
        }
    }

    for (&set, s) in m.texsets.iter().filter(|(_, s)| !s.users.is_empty()) {
        // Note GD6: a slot the set does not have reads as empty too.
        let pbr = s.users.iter().any(|u| {
            m.lighting
                .get(u)
                .is_some_and(|l| l.lighting.slsf2 & SLSF2_TRUE_PBR != 0)
        });
        if pbr && s.raw.slots.get(5).is_none_or(|r| r.is_empty()) {
            push(
                Level::Warn,
                Target::Slot { set, slot: 5 },
                EMPTY_RMAOS,
                None,
            );
        }
        let shape = s
            .users
            .iter()
            .find_map(|u| m.shapes.iter().find(|sh| sh.shader == Some(*u)))
            .map(|sh| sh.block);
        for (i, raw) in s
            .raw
            .slots
            .iter()
            .enumerate()
            .filter(|(_, r)| !r.is_empty())
        {
            let slot = i as u32;
            let tgt = Target::Slot { set, slot };
            // Windows-1252, as apply compares and writes it (trace 2026-10-28 #44)
            let text = crate::text::lossy(raw);
            let mut warn = |t: &str, fix| push(Level::Warn, tgt.clone(), t, fix);
            if text.contains('/') {
                warn("Uses / instead of \\", None);
            }
            let b = text.as_bytes();
            if matches!(b[0], b'\\' | b'/')
                || (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':')
            {
                warn("Absolute path", None);
            }
            let norm = text.replace('/', "\\").to_ascii_lowercase();
            if !norm.starts_with("textures\\") {
                let fix = fixed_path(&text, &norm).zip(shape).and_then(|(path, sh)| {
                    Some(Op::SetTexturePath {
                        at: anchor_role(m, Some(sh), Role::Shape)?,
                        slot,
                        path,
                        base: text.clone(),
                    })
                });
                warn("Not relative to Data", fix);
            }
            if !norm.ends_with(".dds") {
                warn("Not a .dds file", None);
            }
            if exists.is_some_and(|f| !f(&canon_texture(raw).norm)) {
                warn("Not found in the scanned mods or BSAs", None);
            }
        }
    }
    out
}

/// Why a clip timing cannot be written (Note GD8), the file loader's checks plus the cycle's range: None when it can.
pub fn timing_problem(cycle: u32, freq: f32, start: f32, stop: f32) -> Option<&'static str> {
    if ![freq, start, stop].iter().all(|v| v.is_finite()) {
        Some("times must be finite numbers")
    } else if cycle > 2 {
        Some("the cycle must be Loop, Reverse or Clamp")
    } else if freq == 0.0 {
        Some("frequency 0 freezes the clip")
    } else if start > stop {
        Some("start is after stop")
    } else {
        None
    }
}

/// Errors in the keys the ops author (spec §3.3): times, values and tangents must be finite (B-P29); times must not
/// decrease; only LINEAR (1) and QUADRATIC (2) keys are authored. A source curve that is not edited is never checked (R14).
/// Only the keys that are exported are checked: a SetKeys that a later SetKeys on the same (controller, clip)
/// replaced is skipped (Note CM33), or a fixed curve would keep its old error.
pub fn validate_ops(ops: &[Op]) -> Vec<Warning> {
    /// (key type, times, every time, value and tangent finite); `floats` lists a value's f32s.
    fn times<T>(g: &KeyGroup<T>, floats: fn(&T) -> Vec<f32>) -> (Option<u32>, Vec<f32>, bool) {
        let finite = g.keys.iter().all(|k| {
            let vals = [Some(&k.value), k.forward.as_ref(), k.backward.as_ref()];
            k.time.is_finite()
                && vals
                    .into_iter()
                    .flatten()
                    .flat_map(floats)
                    .all(f32::is_finite)
        });
        (g.key_type, g.keys.iter().map(|k| k.time).collect(), finite)
    }
    let mut out = vec![];
    let target = |op: &Op| match op {
        Op::SetKeys { at, clip, .. } => Some((at.block, *clip)),
        _ => None,
    };
    for (i, op) in ops.iter().enumerate() {
        // Note GD8: bad clip timing holds back that clip's timing only (a Warn); a later timing of the clip replaces it
        if let Op::SetSequenceTiming {
            at,
            name,
            nth,
            cycle,
            freq,
            start,
            stop,
        } = op
        {
            let later = ops[i + 1..].iter().any(|o| {
                matches!(o, Op::SetSequenceTiming { name: n, nth: k, .. } if n == name && k == nth)
            });
            if let Some(p) = timing_problem(*cycle, *freq, *start, *stop).filter(|_| !later) {
                out.push(Warning {
                    level: Level::Warn,
                    target: Target::Block { block: at.block },
                    text: format!("Clip {name} timing: {p}; the clip keeps the timing it had"),
                    fix: None,
                });
            }
            continue;
        }
        // (anchor, keys) pairs: one per SetKeys / AddController, one per AddSequence track (F10)
        let pairs: Vec<(&Anchor, &KeyData)> = match op {
            Op::SetKeys { at, keys, .. } | Op::AddController { at, keys, .. } => vec![(at, keys)],
            Op::AddSequence { tracks, .. } => tracks.iter().map(|t| (&t.at, &t.keys)).collect(),
            _ => continue,
        };
        if target(op).is_some() && ops[i + 1..].iter().any(|o| target(o) == target(op)) {
            continue;
        }
        for (at, keys) in pairs {
            let (key_type, t, finite) = match keys {
                KeyData::Float(g) => times(g, |v| vec![*v]),
                KeyData::Pos(g) => times(g, |v| v.to_vec()),
                KeyData::Bool(g) => times(g, |_| vec![]),
            };
            let mut err = |text: &str| {
                out.push(Warning {
                    level: Level::Error,
                    target: Target::Block { block: at.block },
                    text: text.into(),
                    fix: None,
                })
            };
            // NaN slips past the ordering check below ([0, NaN, -1]) and inf times give inf/inf when sampled
            if !finite {
                err("Key values must be finite numbers");
            }
            // B-P41: the UI writes 1/0/1; a hand-edited document could carry anything.
            if let Op::AddController {
                freq, start, stop, ..
            } = op
            {
                if ![freq, start, stop].iter().all(|v| v.is_finite()) {
                    err("Controller times must be finite numbers");
                }
            }
            if t.windows(2).any(|k| k[1] < k[0]) {
                err("Key times must not decrease");
            }
            if key_type.is_some_and(|t| t != 1 && t != 2) {
                err("Only linear and quadratic keys can be authored");
            }
        }
    }
    out
}

/// The root's first BSXFlags: the anchor a fix edits and its current value.
fn root_bsx(m: &NifModel) -> Option<(Anchor, u32)> {
    let net = m.root_net.as_ref()?;
    let (e, v) = m.extras_of(net).into_iter().find_map(|e| match e.value {
        Some(ExtraValue::Bsx(v)) if &*e.kind == "BSXFlags" => Some((e, v)),
        _ => None,
    })?;
    let role = Role::Extra {
        on_root: true,
        name: crate::text::lossy(e.name.as_deref().unwrap_or(&[])),
        kind: e.kind.to_string(),
        nth: 0,
    };
    Some((anchor_role(m, None, role)?, v))
}

/// The path cut at its last `textures\` component, or without a leading `data\`, when that gives `textures\…`.
/// `norm` is `text` with `/` as `\` and ASCII lower case (same byte offsets).
fn fixed_path(text: &str, norm: &str) -> Option<String> {
    let cut = norm
        .rmatch_indices("textures\\")
        .map(|(i, _)| i)
        .find(|&i| i == 0 || norm.as_bytes()[i - 1] == b'\\')?;
    Some(text[cut..].to_string())
}

/// Note CU8: an [`EMPTY_MANAGER`] Error the source already has is shown as a Warning (the edits did not make it).
pub fn demote_source(source: &[Warning], edited: &mut [Warning]) {
    for w in edited.iter_mut().filter(|w| w.text == EMPTY_MANAGER) {
        if source
            .iter()
            .any(|s| s.target == w.target && s.text == w.text)
        {
            w.level = Level::Warn;
        }
    }
}

/// Errors in `edited` that `source` does not already have (R14): an untouched source error never blocks export,
/// except [`ALWAYS_BLOCK`], which crash or black out in game (B-P39, Note CS).
pub fn new_errors(source: &[Warning], edited: &[Warning]) -> Vec<Warning> {
    edited
        .iter()
        .filter(|w| {
            w.level == Level::Error
                && (ALWAYS_BLOCK.contains(&w.text.as_str())
                    || !source
                        .iter()
                        .any(|s| s.target == w.target && s.text == w.text))
        })
        .cloned()
        .collect()
}
