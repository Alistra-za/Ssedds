//! Shared fixtures for the pgpsim integration tests.
#![allow(dead_code)]
use nif::material::MaterialState;
use nif::props::LightingTrailer;
use pgpsim::{simulate, Badge, SimInput, SimResult};
use serde_json::{json, Value};

pub const D: &str = "textures\\landscape\\rock01_d.dds";
pub const N: &str = "textures\\landscape\\rock01_n.dds";
pub const NIF: &str = "meshes\\landscape\\rocks\\rock01.nif";
/// `matchedPath` of `{"match_diffuse": "rock01_d"}` on `rock()`.
pub const MP: &str = "textures\\pbr\\landscape\\rock01_d";

/// A shape whose slots are `slots` (raw NIF strings, slot 0 first); everything else is `builtin_default`.
pub fn shape(slots: &[&str]) -> MaterialState {
    let mut m = MaterialState::builtin_default();
    for (i, s) in slots.iter().enumerate() {
        m.set_slot_raw(i, s);
    }
    m
}

/// The usual fixture: a vanilla rock with diffuse and normal.
pub fn rock() -> MaterialState {
    shape(&[D, N])
}

/// sdc's fixed `default` block (`gui/src/pgp/writer.rs` `fixed_default_block`).
pub fn sdc_default() -> Value {
    json!({"emissive": false, "parallax": false, "subsurface": false,
           "roughness_scale": 1.0, "specular_level": 0.04, "displacement_scale": 1.0})
}

pub fn sim_with(
    entry: &Value,
    default: &Value,
    source: &MaterialState,
    exists: &dyn Fn(&str) -> bool,
) -> SimResult {
    simulate(&SimInput {
        entry,
        default,
        nif_path: NIF,
        source,
        exists,
    })
}

/// Every texture exists.
pub fn sim(entry: &Value, default: &Value, source: &MaterialState) -> SimResult {
    sim_with(entry, default, source, &|_| true)
}

pub fn patched(r: SimResult) -> (MaterialState, String, Vec<Badge>) {
    match r {
        SimResult::Patched {
            material,
            matched_path,
            badges,
        } => (material, matched_path, badges),
        other => panic!("expected Patched, got {other:?}"),
    }
}

pub fn raws(m: &MaterialState) -> Vec<String> {
    m.slots.iter().map(|t| t.raw.clone()).collect()
}

pub fn bit(word: u32, n: u32) -> bool {
    word & (1 << n) != 0
}

pub fn with_bits(word: u32, bits: &[u32]) -> u32 {
    bits.iter().fold(word, |w, b| w | (1 << b))
}

/// The JSON number as the sim stores it (`as_f64() as f32`).
pub fn f(x: f64) -> f32 {
    x as f32
}

/// A type-11 trailer with nifly's default envmap strength (1.0).
pub fn mlp(a: f64, b: f64, c: f64, d: f64) -> LightingTrailer {
    LightingTrailer::MultiLayer {
        inner_thickness: f(a),
        refraction_scale: f(b),
        inner_texture_scale: [f(c), f(d)],
        envmap_strength: 1.0,
    }
}
