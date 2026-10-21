//! Validation (spec §3.3): warns, never clamps or normalises. Reads the model only.
use super::{anchor_role, Anchor, Op, Role};
use crate::ctrl::TimeCtrl;
use crate::extra::ExtraValue;
use crate::fields::Field;
use crate::keys::{KeyData, KeyGroup};
use crate::material::{SLSF1_SPECULAR, SLSF2_TRUE_PBR};
use crate::model::NifModel;
use crate::path::canon_texture;
use crate::props::LightingTrailer;

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
/// B9 RTRB: the ref is unused without the test bit.
pub const ALPHA_TEST_OFF: &str = "This controller has no effect: alpha test is off.";
/// B9 R00 / I9-P00: the cbuffer follows but nothing shows on screen.
pub const REFRACTION_NO_EFFECT: &str = "Refraction strength has no visible effect in game.";

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
        (false, _) => c.class.clone(),
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
            10.0,
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
        if mlp && lt.shader_type != 11 {
            let t = "Multi_Layer_Parallax (SLSF2 24) is set but the shader type is not 11.";
            push(Level::Warn, slsf2.clone(), t, None);
        }
        if !pbr && lt.slsf2 & (1 << 12) != 0 {
            let t = "Fit_Slope (SLSF2 12) does nothing without the PBR flag.";
            push(Level::Warn, slsf2, t, None);
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
            match (c.class.as_str(), c.var) {
                (LSCF, Some(8)) if pbr => push(Level::Error, tgt.clone(), ENV_SCALE_PBR, None),
                (LSCF, Some(10)) | (LSCC, Some(0)) if !specular => {
                    push(Level::Warn, tgt.clone(), SPECULAR_OFF, None)
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
            if m.ctrls.get(&c).is_some_and(|c| c.class == TESTREF) && !a.alpha.test() {
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
        if let (true, Some(v)) = (c.class == "BSLightingShaderPropertyFloatController", c.var) {
            if !LSCF_VARS.contains(&v) {
                let t = format!("Variable {v} is not a known Skyrim value (kept)");
                push(Level::Warn, fld(block, Field::Variable), &t, None);
            }
        }
    }

    for (&block, e) in &m.extras {
        if e.kind == "BSXFlags" && e.name.as_deref() != Some(b"BSX") {
            push(
                Level::Error,
                Target::Extra { block },
                "BSXFlags must be named BSX",
                None,
            );
        }
    }

    for (&set, s) in m.texsets.iter().filter(|(_, s)| !s.users.is_empty()) {
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
            let text = String::from_utf8_lossy(raw).into_owned();
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

/// Errors in the keys the ops author (spec §3.3): times, values and tangents must be finite (B-P29); times must not
/// decrease; only LINEAR (1) and QUADRATIC (2) keys are authored. A source curve that is not edited is never checked (R14).
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
    for op in ops {
        let (Op::SetKeys { at, keys, .. } | Op::AddController { at, keys, .. }) = op else {
            continue;
        };
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
        if t.windows(2).any(|k| k[1] < k[0]) {
            err("Key times must not decrease");
        }
        if key_type.is_some_and(|t| t != 1 && t != 2) {
            err("Only linear and quadratic keys can be authored");
        }
    }
    out
}

/// The root's first BSXFlags: the anchor a fix edits and its current value.
fn root_bsx(m: &NifModel) -> Option<(Anchor, u32)> {
    let net = m.root_net.as_ref()?;
    let (e, v) = m.extras_of(net).into_iter().find_map(|e| match e.value {
        Some(ExtraValue::Bsx(v)) if e.kind == "BSXFlags" => Some((e, v)),
        _ => None,
    })?;
    let role = Role::Extra {
        on_root: true,
        name: String::from_utf8_lossy(e.name.as_deref().unwrap_or(&[])).into_owned(),
        kind: e.kind.clone(),
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

/// Errors in `edited` that `source` does not already have (R14): an untouched source error never blocks export,
/// except [`ENV_SCALE_PBR`], which crashes the game (B-P39).
pub fn new_errors(source: &[Warning], edited: &[Warning]) -> Vec<Warning> {
    edited
        .iter()
        .filter(|w| {
            w.level == Level::Error
                && (w.text == ENV_SCALE_PBR
                    || !source
                        .iter()
                        .any(|s| s.target == w.target && s.text == w.text))
        })
        .cloned()
        .collect()
}
