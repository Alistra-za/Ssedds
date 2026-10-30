//! JSON accessors as PGPatcher 2.x reads patch keys: `contains && is_<type>`, so a wrong type is absent
//! [Src 2.2.0 TP:52-57, 744-1235].
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

/// `is_number()`: a boolean is not a number in 2.x [Src 2.2.0 TP:868].
pub(crate) fn num(v: &Value) -> Option<f32> {
    v.as_f64().map(|x| x as f32)
}

pub(crate) fn get_num(e: &Map<String, Value>, k: &str) -> Option<f32> {
    e.get(k).and_then(num)
}

/// The first three numbers of an array with at least `min_len` (>= 3) entries whose first `min_len` are all
/// numbers (`size() >= n` and n `is_number()` checks in the C++, e.g. four for `emissive_color` [TP:920-923]).
pub(crate) fn rgb(e: &Map<String, Value>, k: &str, min_len: usize) -> Option<[f32; 3]> {
    let a = e.get(k)?.as_array()?;
    let n = min_len.max(3);
    if a.len() < n || a[..n].iter().any(|v| num(v).is_none()) {
        return None;
    }
    Some([num(&a[0])?, num(&a[1])?, num(&a[2])?])
}
