//! Track A recipe maths: pure per-texel functions on stored values (DECISIONS Note CM5-CM10).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// `[export.legacy]`: the PBR → vanilla / complex-material recipe settings (C4, Note CM5).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct LegacyParams {
    /// Diffuse brightness k (owner answer 1: 0.9; Q1's 0.65 stays available).
    pub brightness: f64,
    /// AO strength s for objects (Q3).
    pub ao_strength: f64,
    /// AO strength s for landscape (calibration).
    pub ao_strength_landscape: f64,
    /// Opacity of the `_curves` soft light and the cavity multiply (Q2, TA4).
    pub curves_opacity: f64,
    /// Vanilla diffuse factor at full metal (Q4).
    pub metal_darken: f64,
    /// `_n` alpha scale (round 2).
    pub spec_scale: f64,
    /// `_n` alpha boost at full metal (round 2).
    pub spec_metal_boost: f64,
    /// Landscape `_n` alpha factor, applied before saturation (O5).
    pub landscape_spec: f64,
    /// CM `_m` R floor for non-metals, × gloss (Q5; 0 = off, option 0.35).
    pub cm_dielectric_env: f64,
    /// CM diffuse keeps metal brightness instead of reusing the vanilla diffuse.
    pub keep_metal_brightness: bool,
    /// How much of the metal darkening Keep metal brightness undoes.
    pub keep_metal_strength: f64,
}

impl Default for LegacyParams {
    fn default() -> Self {
        Self {
            brightness: 0.9,
            ao_strength: 0.5,
            ao_strength_landscape: 0.3,
            curves_opacity: 0.40,
            metal_darken: 0.3,
            spec_scale: 0.66,
            spec_metal_boost: 2.0,
            landscape_spec: 2.5,
            cm_dielectric_env: 0.0,
            keep_metal_brightness: false,
            keep_metal_strength: 1.0,
        }
    }
}

impl LegacyParams {
    /// Every value finite and in range; the error names the `sdc.toml` key.
    pub fn check(&self) -> Result<(), String> {
        let rows = [
            ("brightness", self.brightness, 0.0, 1.5),
            ("ao_strength", self.ao_strength, 0.0, 1.0),
            (
                "ao_strength_landscape",
                self.ao_strength_landscape,
                0.0,
                1.0,
            ),
            ("curves_opacity", self.curves_opacity, 0.0, 1.0),
            ("metal_darken", self.metal_darken, 0.0, 1.0),
            ("cm_dielectric_env", self.cm_dielectric_env, 0.0, 1.0),
            ("keep_metal_strength", self.keep_metal_strength, 0.0, 1.0),
            ("spec_scale", self.spec_scale, 0.0, 2.0),
            ("spec_metal_boost", self.spec_metal_boost, 1.0, 3.0),
            ("landscape_spec", self.landscape_spec, 0.0, 5.0),
        ];
        for (key, v, lo, hi) in rows {
            // `contains` is false for NaN, and ±inf is out of every range.
            if !(lo..=hi).contains(&v) {
                return Err(format!(
                    "export.legacy.{key} = {v}: must be between {lo} and {hi}"
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffuseFlavour {
    Vanilla,
    CmKeepMetal,
    Landscape,
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// W3C soft light, `a` the backdrop, `b` the source; `b` = 0.5 leaves `a` unchanged.
pub fn soft_light(a: f32, b: f32) -> f32 {
    if b <= 0.5 {
        a - (1.0 - 2.0 * b) * a * (1.0 - a)
    } else {
        let d = if a <= 0.25 {
            ((16.0 * a - 12.0) * a + 4.0) * a
        } else {
            a.sqrt()
        };
        a + (2.0 * b - 1.0) * (d - a)
    }
}

/// The vanilla (or CM keep-metal, or landscape) diffuse RGB from the PBR albedo.
pub fn diffuse(
    rgb: [f32; 3],
    ao: f32,
    metal: f32,
    curves: Option<f32>,
    cavity: Option<f32>,
    p: &LegacyParams,
    f: DiffuseFlavour,
) -> [f32; 3] {
    let s = match f {
        DiffuseFlavour::Landscape => p.ao_strength_landscape,
        _ => p.ao_strength,
    } as f32;
    let op = p.curves_opacity as f32;
    let vanilla_metal = lerp(1.0, p.metal_darken as f32, metal);
    let metal_factor = match f {
        DiffuseFlavour::Vanilla => vanilla_metal,
        DiffuseFlavour::CmKeepMetal => lerp(vanilla_metal, 1.0, p.keep_metal_strength as f32),
        DiffuseFlavour::Landscape => 1.0,
    };
    let base = p.brightness as f32 * lerp(1.0, ao, s);
    rgb.map(|v| {
        // Clamped before the soft light, whose domain is [0, 1] (k may exceed 1; CM10).
        let mut c = (v * base).clamp(0.0, 1.0);
        if let Some(b) = curves {
            c = lerp(c, soft_light(c, b), op);
        }
        if let Some(m) = cavity {
            c = lerp(c, c * m, op);
        }
        (c * metal_factor).clamp(0.0, 1.0)
    })
}

/// The vanilla `_n` alpha (specular mask); the CM `_n` alpha is the same (round 2).
pub fn spec_alpha(rough: f32, ao: f32, metal: f32, p: &LegacyParams, landscape: bool) -> f32 {
    let f = if landscape {
        p.landscape_spec as f32
    } else {
        1.0
    };
    // Landscape ignores metal (TA5); O5's 2.5 was fitted without a boost.
    let boost = if landscape {
        1.0
    } else {
        lerp(1.0, p.spec_metal_boost as f32, metal)
    };
    (f * p.spec_scale as f32 * (1.0 - rough) * ao * boost).clamp(0.0, 1.0)
}

/// The complex-material `_m` RGBA (TA3, TA5, TA6).
pub fn cm_m(
    rough: f32,
    metal: f32,
    height: Option<f32>,
    p: &LegacyParams,
    landscape: bool,
) -> [f32; 4] {
    let g = 1.0 - rough;
    let (r, b) = if landscape {
        (0.0, 0.0)
    } else {
        (g * metal.max(p.cm_dielectric_env as f32), metal)
    };
    // At ≤ 4/255 gloss or height the shader turns complex material or parallax off.
    let a = height.map_or(0.0, |h| h.clamp(5.0 / 255.0, 250.0 / 255.0));
    [r, g.max(5.0 / 255.0), b, a]
}

/// The vanilla env `_m` (metal × gloss).
pub fn env_m(rough: f32, metal: f32) -> f32 {
    metal * (1.0 - rough)
}

/// Suggested NIF glossiness from a set's mean roughness (report only).
pub fn nif_glossiness(mean_rough: f64) -> f64 {
    (2.0 / mean_rough.powi(4) - 2.0).clamp(2.0, 500.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close<T: Into<f64>>(a: T, b: T, eps: f64) {
        let (a, b) = (a.into(), b.into());
        assert!((a - b).abs() <= eps, "{a} vs {b} (eps {eps})");
    }

    #[test]
    fn defaults_are_the_spec_values() {
        let p = LegacyParams::default();
        assert_eq!(
            (
                p.brightness,
                p.ao_strength,
                p.ao_strength_landscape,
                p.curves_opacity
            ),
            (0.9, 0.5, 0.3, 0.40)
        );
        assert_eq!(
            (
                p.metal_darken,
                p.spec_scale,
                p.spec_metal_boost,
                p.cm_dielectric_env
            ),
            (0.3, 0.66, 2.0, 0.0)
        );
        assert_eq!(
            (
                p.keep_metal_brightness,
                p.keep_metal_strength,
                p.landscape_spec
            ),
            (false, 1.0, 2.5)
        );
    }

    #[test]
    fn brightness_ao_and_metal_follow_the_chain() {
        use DiffuseFlavour::*;
        let p = LegacyParams::default();
        let g = [200. / 255.; 3];
        // 200/255 grey, AO 0.5 at s 0.5 → ×0.75, no metal: 200·0.9·0.75 = 135 codes.
        close(
            diffuse(g, 0.5, 0.0, None, None, &p, Vanilla)[0] * 255.0,
            135.0,
            1e-3,
        );
        // Full metal, vanilla: × lerp(1, 0.3, 1) = 0.3 → 54 codes (AO 1).
        close(
            diffuse(g, 1.0, 1.0, None, None, &p, Vanilla)[0] * 255.0,
            54.0,
            1e-3,
        );
        // Half metal: × lerp(1, 0.3, 0.5) = 0.65 → 200·0.9·0.65 = 117 codes (M1).
        close(
            diffuse(g, 1.0, 0.5, None, None, &p, Vanilla)[0] * 255.0,
            117.0,
            1e-3,
        );
        // CmKeepMetal at strength 1: × 1; at strength 0: × 0.3.
        close(
            diffuse(g, 1.0, 1.0, None, None, &p, CmKeepMetal)[0] * 255.0,
            180.0,
            1e-3,
        );
        let k0 = LegacyParams {
            keep_metal_strength: 0.0,
            ..p
        };
        close(
            diffuse(g, 1.0, 1.0, None, None, &k0, CmKeepMetal)[0] * 255.0,
            54.0,
            1e-3,
        );
        // Landscape: AO 0.5 at s 0.3 → ×0.85, metal ignored: 200·0.9·0.85 = 153 codes.
        for metal in [0.0, 1.0] {
            close(
                diffuse(g, 0.5, metal, None, None, &p, Landscape)[0] * 255.0,
                153.0,
                1e-3,
            );
        }
        // Both _curves and cavity (TA4): c = 0.2 after steps 1–2, curves 0.8, cavity 0.5 →
        // 0.2 + 0.4·(0.3488 − 0.2) = 0.25952, then 0.25952·lerp(1, 0.5, 0.4) = 0.207616.
        let c = diffuse([0.2 / 0.9; 3], 1.0, 0.0, Some(0.8), Some(0.5), &p, Vanilla);
        close(c[2], 0.207616, 1e-5);
        // Clamped to [0, 1].
        let hot = LegacyParams {
            brightness: 1.5,
            ..p
        };
        assert_eq!(
            diffuse([1.0; 3], 1.0, 0.0, None, None, &hot, Vanilla),
            [1.0; 3]
        );
        // k > 1 (CM10): c is clamped before the soft light, so a darkening curve cannot brighten.
        // k 1.5, albedo 0.9 → 1.35 → 1; soft_light(1, 0.2) = 1; full metal × 0.3 = 0.3.
        let c = diffuse([0.9; 3], 1.0, 1.0, Some(0.2), None, &hot, Vanilla);
        close(c[0], 0.3, 1e-6);
    }

    #[test]
    fn soft_light_at_half_is_identity_and_follows_w3c_below_a_quarter() {
        for a in [0.0, 0.1, 0.25, 0.6, 1.0] {
            close(soft_light(a, 0.5), a, 1e-6);
        }
        // a = 0.2, b = 0.8: D(a) = ((16a−12)a+4)a = 0.448 → 0.2 + 0.6·(0.448 − 0.2) = 0.3488.
        close(soft_light(0.2, 0.8), 0.3488, 1e-5);
        // b ≤ 0.5 branch: a − (1 − 2b)·a·(1 − a); above a quarter D(a) = √a.
        close(soft_light(0.2, 0.2), 0.2 - 0.6 * 0.2 * 0.8, 1e-6);
        close(soft_light(0.64, 1.0), 0.8, 1e-6);
    }

    #[test]
    fn spec_alpha_is_k_gloss_ao_metal_boost_saturated() {
        let p = LegacyParams::default();
        close(spec_alpha(0.5, 1.0, 0.0, &p, false), 0.33, 1e-6); // 0.66·0.5
        close(spec_alpha(0.5, 1.0, 1.0, &p, false), 0.66, 1e-6); // ×2.0
        close(spec_alpha(0.5, 1.0, 0.5, &p, false), 0.495, 1e-6); // ×1.5 at half metal (M1)
                                                                  // Landscape ignores metal (TA5, CM8): metal 1 gets the metal-0 value, 2.5·0.66·0.2 = 0.33.
        close(spec_alpha(0.8, 1.0, 1.0, &p, true), 0.33, 1e-6);
        assert_eq!(
            spec_alpha(0.8, 1.0, 1.0, &p, true),
            spec_alpha(0.8, 1.0, 0.0, &p, true)
        );
        assert_eq!(spec_alpha(0.0, 1.0, 1.0, &p, false), 1.0); // saturated
        close(spec_alpha(0.9, 0.5, 0.0, &p, true), 0.0825, 1e-6); // landscape: 2.5·0.66·0.1·0.5 (O5)
        assert_eq!(spec_alpha(0.0, 1.0, 0.0, &p, true), 1.0); // the factor scales before the saturation
    }

    #[test]
    fn cm_m_is_metal_gloss_floor_metal_and_clamped_height() {
        let p = LegacyParams::default();
        let m = cm_m(0.4, 1.0, Some(0.0), &p, false);
        close(m[0], 0.6, 1e-6);
        close(m[1], 0.6, 1e-6);
        assert_eq!(m[2], 1.0);
        close(m[3], 5.0 / 255.0, 1e-7);
        assert_eq!(
            cm_m(1.0, 0.0, None, &p, false),
            [0.0, 5.0 / 255.0, 0.0, 0.0]
        ); // G floor, A 0 without _p
        close(cm_m(0.4, 0.0, Some(1.0), &p, false)[3], 250.0 / 255.0, 1e-7);
        let d = LegacyParams {
            cm_dielectric_env: 0.35,
            ..p
        };
        close(cm_m(0.4, 0.0, None, &d, false)[0], 0.21, 1e-6); // 0.35 × gloss
        close(cm_m(0.4, 0.5, None, &d, false)[0], 0.3, 1e-6); // 0.6 · max(0.5, 0.35) (M1)
        let l = cm_m(0.4, 1.0, Some(0.5), &p, true);
        assert_eq!((l[0], l[2]), (0.0, 0.0)); // landscape
        close(l[3], 0.5, 1e-7);
    }

    #[test]
    fn env_m_and_glossiness() {
        close(env_m(0.25, 0.8), 0.6, 1e-6);
        close(nif_glossiness(0.5), 30.0, 1e-9);
        assert_eq!((nif_glossiness(0.0), nif_glossiness(1.0)), (500.0, 2.0));
    }

    #[test]
    fn check_names_the_key_and_refuses_non_finite() {
        let e = LegacyParams {
            brightness: 2.0,
            ..Default::default()
        }
        .check()
        .unwrap_err();
        assert_eq!(e, "export.legacy.brightness = 2: must be between 0 and 1.5");
        assert!(LegacyParams {
            spec_metal_boost: f64::NAN,
            ..Default::default()
        }
        .check()
        .is_err());
        assert!(LegacyParams {
            landscape_spec: f64::INFINITY,
            ..Default::default()
        }
        .check()
        .is_err());
        assert!(LegacyParams {
            spec_metal_boost: 0.5,
            ..Default::default()
        }
        .check()
        .is_err());
        assert_eq!(LegacyParams::default().check(), Ok(()));
    }

    #[test]
    fn unknown_keys_are_refused_and_missing_keys_default() {
        let p: LegacyParams = toml::from_str("brightness = 0.65").unwrap();
        assert_eq!(
            p,
            LegacyParams {
                brightness: 0.65,
                ..Default::default()
            }
        );
        assert!(toml::from_str::<LegacyParams>("brightnes = 0.65").is_err());
    }
}
