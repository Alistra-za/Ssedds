//! PGPatcher 1.2.0 reads keys with nlohmann `get<T>()`, which throws on a wrong JSON type inside the per-shape
//! patch (01 §2.4). The sim reports that as a `Skip` instead of guessing. Over-approximation: a wrong-typed
//! known key is rejected even on a code path PGPatcher would not reach. A non-object `glint`/`fuzz` is NOT an
//! error (the C++ only calls `contains` on it). A too-short `fuzz.color` is a Skip: PGPatcher 1.2.0 reads `fuzzColor[2]` without a length check, which is undefined behaviour.
use crate::json::{num, GLINT_KEYS};
use serde_json::{Map, Value};

const BOOL_KEYS: &[&str] = &[
    "delete",
    "vertex_colors",
    "zbuffer_write",
    "emissive",
    "parallax",
    "subsurface",
    "subsurface_foliage",
    "hair",
    "multilayer",
    "coat_diffuse",
    "coat_parallax",
    "coat_normal",
    "lock_diffuse",
    "lock_normal",
    "lock_emissive",
    "lock_parallax",
    "lock_cubemap",
    "lock_rmaos",
    "lock_cnr",
    "lock_subsurface",
    "pbr",
    "env_mapping",
];

const NUM_KEYS: &[&str] = &[
    "smooth_angle",
    "auto_uv",
    "vertex_color_lum_mult",
    "vertex_color_sat_mult",
    "specular_level",
    "roughness_scale",
    "subsurface_opacity",
    "displacement_scale",
    "env_map_scale",
    "env_map_scale_mult",
    "emissive_scale",
    "uv_scale",
    "coat_specular_level",
    "coat_roughness",
    "coat_strength",
    "inner_uv_scale",
];

const STR_KEYS: &[&str] = &[
    "texture",
    "match_normal",
    "match_diffuse",
    "rename",
    "path_contains",
    "nif_filter",
    "cubemap",
    "match1",
    "match2",
    "match3",
    "match4",
    "match5",
    "match6",
    "match7",
    "match8",
    "slot1",
    "slot2",
    "slot3",
    "slot4",
    "slot5",
    "slot6",
    "slot7",
    "slot8",
];

/// Colour keys and the length from which PGPatcher reads them.
const COLOUR_KEYS: &[(&str, usize)] = &[
    ("subsurface_color", 3),
    ("emissive_color", 4),
    ("coat_color", 3),
];

fn bad(k: &str, want: &str) -> Result<(), String> {
    Err(format!(
        "PGPatcher 1.2.0 would fail on \"{k}\": expected {want}"
    ))
}

pub(crate) fn check_types(e: &Map<String, Value>) -> Result<(), String> {
    for k in BOOL_KEYS {
        if e.get(*k).is_some_and(|v| !v.is_boolean()) {
            return bad(k, "true or false");
        }
    }
    for k in NUM_KEYS {
        if e.get(*k).is_some_and(|v| num(v).is_none()) {
            return bad(k, "a number");
        }
    }
    for k in STR_KEYS {
        if e.get(*k).is_some_and(|v| !v.is_string()) {
            return bad(k, "a string");
        }
    }
    for (k, n) in COLOUR_KEYS {
        let Some(a) = e.get(*k).and_then(Value::as_array) else {
            continue;
        };
        if a.len() >= *n && a[..*n].iter().any(|v| num(v).is_none()) {
            return bad(k, "numbers");
        }
    }
    if let Some(g) = e.get("glint").and_then(Value::as_object) {
        for k in GLINT_KEYS {
            if g.get(k).is_some_and(|v| num(v).is_none()) {
                return bad(&format!("glint.{k}"), "a number");
            }
        }
    }
    if let Some(z) = e.get("fuzz").and_then(Value::as_object) {
        if z.get("texture").is_some_and(|v| !v.is_boolean()) {
            return bad("fuzz.texture", "true or false");
        }
        if z.get("weight").is_some_and(|v| num(v).is_none()) {
            return bad("fuzz.weight", "a number");
        }
        let colour_ok = |c: &Value| {
            c.as_array()
                .is_some_and(|a| a.len() >= 3 && a.iter().all(|v| num(v).is_some()))
        };
        if z.get("color").is_some_and(|c| !colour_ok(c)) {
            return bad("fuzz.color", "3 or more numbers");
        }
    }
    Ok(())
}
