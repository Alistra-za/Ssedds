//! The patch itself, `applyOnePatch` + `enableTruePBROnShape` [Src 1.2.0 TP:662-1142; 01 §2.1-§2.3], on a
//! `MaterialState`, plus the per-file checks of 01 §1.4.
use crate::check::{check_types, uint_in};
use crate::json::{flag, get_bool, get_num, rgb, GLINT_KEYS};
use crate::matching::pg_slots;
use crate::slots::{apply_slots, first_missing};
use crate::{Badge, MaterialState, SimResult};
use nif::props::{Alpha, LightingTrailer};
use serde_json::{Map, Value};

// nifly SkyrimShaderPropertyFlags1/2 bit numbers (nifly include/Shaders.hpp:50-116).
const SLSF1_ENVMAP: u32 = 7;
const SLSF1_FACEGEN_DETAIL: u32 = 10;
const SLSF1_PARALLAX: u32 = 11;
const SLSF1_EYE_ENVMAP: u32 = 17;
const SLSF1_HAIR_SOFT: u32 = 18;
const SLSF1_FACEGEN_RGB_TINT: u32 = 21;
const SLSF1_EXTERNAL_EMITTANCE: u32 = 29;
const SLSF2_ZBUFFER_WRITE: u32 = 0;
const SLSF2_VERTEX_COLORS: u32 = 5;
const SLSF2_GLOW_MAP: u32 = 6;
const SLSF2_FIT_SLOPE: u32 = 12;
/// `SLSF2_UNUSED01`, the TruePBR marker.
const SLSF2_PBR: u32 = 23;
const SLSF2_MLP: u32 = 24;
const SLSF2_SOFT: u32 = 25;
const SLSF2_RIM: u32 = 26;
const SLSF2_BACK: u32 = 27;
const SLSF2_EFFECT_LIGHTING: u32 = 30;

const TYPE_DEFAULT: u32 = 0;
const TYPE_MLP: u32 = 11;

/// Keys badged "not previewed" (spec §2.1), in display order. 2.0's removed `pbr` / `env_*` keys get none (R2).
const BADGE_KEYS: [&str; 4] = [
    "smooth_angle",
    "auto_uv",
    "vertex_color_lum_mult",
    "vertex_color_sat_mult",
];

fn set_bit(word: &mut u32, bit: u32, on: bool) {
    if on {
        *word |= 1 << bit;
    } else {
        *word &= !(1 << bit);
    }
}

/// The shape as PGPatcher holds it in memory while it applies one file's entries.
struct Sim {
    m: MaterialState,
    /// PGPatcher's lower-cased slot strings (01 §1.3).
    slots: [String; 9],
    /// nifly keeps the MLP floats (thickness, refraction, texture scale u, v) and the envmap strength in memory
    /// whatever the type; the NIF gets them only for type 11 (01 §2.3 notes). Defaults: Shaders.hpp:699-702.
    mlp: [f32; 4],
    mlp_envmap: f32,
    src_type: u32,
    src_trailer: LightingTrailer,
    /// Some entry wrote the PBR slots, so PGPatcher writes back its (lower-cased) slot set.
    pbr_written: bool,
    badges: Vec<Badge>,
}

impl Sim {
    fn new(source: &MaterialState) -> Sim {
        let (mlp, mlp_envmap) = match &source.params.trailer {
            LightingTrailer::MultiLayer {
                inner_thickness,
                refraction_scale,
                inner_texture_scale,
                envmap_strength,
            } => (
                [
                    *inner_thickness,
                    *refraction_scale,
                    inner_texture_scale[0],
                    inner_texture_scale[1],
                ],
                *envmap_strength,
            ),
            _ => ([0.0, 1.0, 1.0, 1.0], 1.0),
        };
        Sim {
            m: source.clone(),
            slots: pg_slots(&source.slots),
            mlp,
            mlp_envmap,
            src_type: source.shader_type,
            src_trailer: source.params.trailer.clone(),
            pbr_written: false,
            badges: Vec::new(),
        }
    }

    fn f1(&mut self, bit: u32, on: bool) {
        set_bit(&mut self.m.slsf1, bit, on);
    }

    fn f2(&mut self, bit: u32, on: bool) {
        set_bit(&mut self.m.slsf2, bit, on);
    }

    /// One entry, `applyOnePatch` [TP:662-863]; 2.x dropped the `env_mapping` block [Src 2.2.0 TP:729-945].
    /// Returns true when the entry deleted the shape.
    fn apply_one(&mut self, e: &Map<String, Value>, mp: &str) -> bool {
        if flag(e, "delete") {
            self.m.hidden = true;
            return true;
        }
        // Vertex-colour multipliers run only on shapes that have vertex colours (after `vertex_colors`).
        let vc_after = get_bool(e, "vertex_colors").unwrap_or(self.m.has_vertex_colors);
        for k in BADGE_KEYS {
            // 2.x skips a non-number value [Src 2.2.0 TP:756, 763, 787-788], so the preview misses nothing.
            if !e.get(k).is_some_and(Value::is_number) {
                continue;
            }
            let shown = match k {
                "vertex_color_lum_mult" | "vertex_color_sat_mult" => vc_after,
                _ => true,
            };
            let b = Badge::NotPreviewed(k);
            if shown && !self.badges.contains(&b) {
                self.badges.push(b);
            }
        }
        if let Some(vc) = get_bool(e, "vertex_colors") {
            self.m.has_vertex_colors = vc;
            self.f2(SLSF2_VERTEX_COLORS, vc);
        }
        if let Some(z) = get_bool(e, "zbuffer_write") {
            self.f2(SLSF2_ZBUFFER_WRITE, z);
        }
        // PGPatcher 2.2 `setAlphaProperty` / `deleteAlphaProperty`. Delete wins; create makes nifly's default
        // NiAlphaProperty (flags 4844, threshold 128); only values `unsignedFromJSON` accepts are written.
        // Sharing/unsharing of the property has no effect on a per-shape preview.
        if flag(e, "alphaprop_delete") {
            self.m.alpha = None;
        } else if self.m.alpha.is_some() || flag(e, "alphaprop_create") {
            let old = self.m.alpha.unwrap_or(Alpha {
                flags: 4844,
                threshold: 128,
            });
            let int = |k: &str, max: u32| e.get(k).and_then(|v| uint_in(v, max));
            self.m.alpha = Some(Alpha {
                flags: int("alphaprop_flags", 65535).map_or(old.flags, |v| v as u16),
                threshold: int("alphaprop_thresh", 255).map_or(old.threshold, |v| v as u8),
            });
        }
        if let Some(v) = get_num(e, "specular_level") {
            self.m.params.glossiness = v;
        }
        if let Some(c) = rgb(e, "subsurface_color", 3) {
            self.m.params.specular_color = c;
        }
        if let Some(v) = get_num(e, "roughness_scale") {
            self.m.params.specular_strength = v;
        }
        if let Some(v) = get_num(e, "subsurface_opacity") {
            self.m.params.lighting_effect_1 = v;
        }
        if let Some(v) = get_num(e, "displacement_scale") {
            self.m.params.lighting_effect_2 = v;
        }
        if let Some(v) = get_num(e, "emissive_scale") {
            self.m.params.emissive_multiple = v;
        }
        // Read only with >= 4 numbers; sdc writes RGBA. The NIF keeps RGB.
        if let Some(c) = rgb(e, "emissive_color", 4) {
            self.m.params.emissive_color = c;
        }
        if let Some(v) = get_num(e, "uv_scale") {
            self.m.params.uv_scale = [v, v];
        }
        // 2.x: always on [Src 2.2.0 TP:941-942].
        self.enable_pbr(e, mp);
        false
    }

    /// `enableTruePBROnShape` [TP:967-1142]. Runs after the scalars, so coat_* overrides subsurface_*.
    fn enable_pbr(&mut self, e: &Map<String, Value>, mp: &str) {
        self.pbr_written = true;
        apply_slots(&mut self.slots, e, mp);
        if let Some(v) = get_bool(e, "emissive") {
            self.f1(SLSF1_EXTERNAL_EMITTANCE, v);
        }
        self.f1(SLSF1_ENVMAP, false);
        self.f1(SLSF1_HAIR_SOFT, false);
        self.f1(SLSF1_PARALLAX, false);
        self.f2(SLSF2_GLOW_MAP, false);
        self.f1(SLSF1_FACEGEN_DETAIL, false);
        // [Src 2.2.0 TP:1080]
        self.f1(SLSF1_FACEGEN_RGB_TINT, false);
        self.f2(SLSF2_PBR, true);
        self.f1(SLSF1_EYE_ENVMAP, false);
        if let Some(v) = get_bool(e, "subsurface") {
            self.f2(SLSF2_RIM, v);
        }
        let hair = flag(e, "hair");
        if hair {
            self.f2(SLSF2_BACK, true);
        }
        let multilayer = flag(e, "multilayer");
        if multilayer {
            self.m.shader_type = TYPE_MLP;
            self.f2(SLSF2_MLP, true);
            if let Some(c) = rgb(e, "coat_color", 3) {
                self.m.params.specular_color = c;
            }
            if let Some(v) = get_num(e, "coat_specular_level") {
                self.mlp[1] = v;
            }
            if let Some(v) = get_num(e, "coat_roughness") {
                self.mlp[0] = v;
            }
            if let Some(v) = get_num(e, "coat_strength") {
                self.m.params.lighting_effect_1 = v;
            }
            if let Some(v) = get_bool(e, "coat_diffuse") {
                self.f2(SLSF2_EFFECT_LIGHTING, v);
            }
            if let Some(v) = get_bool(e, "coat_parallax") {
                self.f2(SLSF2_SOFT, v);
            }
            if let Some(v) = get_bool(e, "coat_normal") {
                self.f2(SLSF2_BACK, v);
            }
            if let Some(v) = get_num(e, "inner_uv_scale") {
                self.mlp[2] = v;
                self.mlp[3] = v;
            }
        } else if let Some(g) = e.get("glint").and_then(Value::as_object) {
            // [Src 2.2.0 TP:1162]: only an object picks the branch.
            self.m.shader_type = TYPE_MLP;
            self.f2(SLSF2_FIT_SLOPE, true);
            for (slot, k) in self.mlp.iter_mut().zip(GLINT_KEYS) {
                if let Some(v) = get_num(g, k) {
                    *slot = v;
                }
            }
        } else if let Some(z) = e.get("fuzz").and_then(Value::as_object) {
            // [Src 2.2.0 TP:1191, 1201-1206]: an object; `color` exactly three numbers, else black.
            self.m.shader_type = TYPE_MLP;
            self.f2(SLSF2_SOFT, true);
            let three = z
                .get("color")
                .and_then(Value::as_array)
                .is_some_and(|a| a.len() == 3);
            let [r, g, b] = rgb(z, "color", 3).filter(|_| three).unwrap_or([0.0; 3]);
            let w = get_num(z, "weight").unwrap_or(1.0);
            self.mlp = [r, g, b, w];
        } else {
            self.m.shader_type = TYPE_DEFAULT;
        }
        if !multilayer {
            self.f2(SLSF2_MLP, false);
            if !hair {
                self.f2(SLSF2_BACK, false);
            }
            // [Src 2.2.0 TP:1230]: absent or not an object.
            if !e.get("fuzz").is_some_and(Value::is_object) {
                self.f2(SLSF2_SOFT, false);
            }
        }
    }

    fn finish(mut self) -> (MaterialState, Vec<Badge>) {
        self.m.params.trailer = match self.m.shader_type {
            TYPE_MLP => LightingTrailer::MultiLayer {
                inner_thickness: self.mlp[0],
                refraction_scale: self.mlp[1],
                inner_texture_scale: [self.mlp[2], self.mlp[3]],
                envmap_strength: self.mlp_envmap,
            },
            t if t == self.src_type => self.src_trailer.clone(),
            _ => LightingTrailer::None,
        };
        if self.pbr_written {
            for (i, s) in self.slots.iter().enumerate() {
                self.m.set_slot_raw(i, s);
            }
        }
        (self.m, self.badges)
    }
}

/// The core of `simulate` and `simulate_file`. `matches` = every matching prepared entry of ONE PGPatcher file
/// with its `matchedPath`, in index order (01 §1.4-§1.5: all apply, the last one wins per field).
pub(crate) fn run(
    matches: &[(&Map<String, Value>, String)],
    source: &MaterialState,
    exists: &dyn Fn(&str) -> bool,
) -> SimResult {
    if matches.is_empty() {
        return SimResult::NoMatch;
    }
    // 2.x canApply is always true [Src 2.2.0 TP:302-307]: SLSF1.21 shapes are patched (and the bit cleared).
    for (e, _) in matches {
        if let Err(msg) = check_types(e) {
            return SimResult::Skip(msg);
        }
    }
    let mut sim = Sim::new(source);
    // 01 §1.4: the slot pass of all entries, then every non-empty slot must exist; `delete` skips the check.
    if !matches.iter().any(|(e, _)| flag(e, "delete")) {
        let mut slots = sim.slots.clone();
        for (e, mp) in matches {
            apply_slots(&mut slots, e, mp);
        }
        if let Some(p) = first_missing(&slots, exists) {
            return SimResult::Skip(format!("{p} missing"));
        }
    }
    let mut matched_path = String::new();
    for (e, mp) in matches {
        matched_path.clone_from(mp);
        if sim.apply_one(e, mp) {
            break;
        }
    }
    let (material, badges) = sim.finish();
    SimResult::Patched {
        material,
        matched_path,
        badges,
    }
}
