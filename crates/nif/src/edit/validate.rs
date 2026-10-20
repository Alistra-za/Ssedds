//! Validation (spec §3.3): warns, never clamps or normalises. Reads the model only.
use super::{anchor_role, Anchor, Op, Role};
use crate::extra::ExtraValue;
use crate::fields::Field;
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
const LSCF_VARS: [u32; 10] = [0, 8, 9, 10, 11, 12, 20, 21, 22, 23];

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
    }

    for (&block, a) in &m.alphas {
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

/// Errors in `edited` that `source` does not already have (R14): an untouched source error never blocks export.
pub fn new_errors(source: &[Warning], edited: &[Warning]) -> Vec<Warning> {
    edited
        .iter()
        .filter(|w| {
            w.level == Level::Error
                && !source
                    .iter()
                    .any(|s| s.target == w.target && s.text == w.text)
        })
        .cloned()
        .collect()
}
