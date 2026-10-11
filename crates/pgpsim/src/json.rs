//! JSON accessors with nlohmann's semantics for well-typed values. `simulate` rejects wrong types first
//! (`check.rs`), so these never see one there.
use serde_json::{Map, Value};

/// The glint sub-keys, in MLP-float order (thickness, refraction, texture scale u, v) [TP:1077-1096].
pub(crate) const GLINT_KEYS: [&str; 4] = [
    "screen_space_scale",
    "log_microfacet_density",
    "microfacet_roughness",
    "density_randomization",
];

/// PGPatcher's `flag(json, key)`: present and `true`.
pub(crate) fn flag(e: &Map<String, Value>, k: &str) -> bool {
    e.get(k).and_then(Value::as_bool) == Some(true)
}

pub(crate) fn get_bool(e: &Map<String, Value>, k: &str) -> Option<bool> {
    e.get(k).and_then(Value::as_bool)
}

/// nlohmann `get<float>()`: any number, and booleans as 0/1.
pub(crate) fn num(v: &Value) -> Option<f32> {
    match v {
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => v.as_f64().map(|x| x as f32),
    }
}

pub(crate) fn get_num(e: &Map<String, Value>, k: &str) -> Option<f32> {
    e.get(k).and_then(num)
}

/// The first three numbers of an array with at least `min_len` (>= 3) entries (`size() >= n` in the C++).
pub(crate) fn rgb(e: &Map<String, Value>, k: &str, min_len: usize) -> Option<[f32; 3]> {
    let a = e.get(k)?.as_array()?;
    if a.len() < min_len.max(3) {
        return None;
    }
    Some([num(&a[0])?, num(&a[1])?, num(&a[2])?])
}
