//! Entry preprocessing, 01 §1.1 [Src 1.2.0 TP:142-166].
use serde_json::{Map, Value};

/// Keys that get a leading `\` (`getTruePBRConfigFilenameFields`); the `\` enforces whole path components.
const FILENAME_FIELDS: [&str; 3] = ["match_normal", "match_diffuse", "rename"];

/// Default merge (shallow, entry keys win), then `texture` → `match_diffuse` (texture wins), then the leading
/// backslash on `match_normal` / `match_diffuse` / `rename`. `default` may be `Value::Null` (bare-array file).
/// A non-object entry comes back unchanged (it can match nothing).
pub fn prepare(entry: &Value, default: &Value) -> Value {
    let Some(e) = entry.as_object() else {
        return entry.clone();
    };
    let mut out: Map<String, Value> = e.clone();
    if let Some(d) = default.as_object() {
        for (k, v) in d {
            out.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    if let Some(t) = out.get("texture").cloned() {
        out.insert("match_diffuse".to_string(), t);
    }
    for f in FILENAME_FIELDS {
        if let Some(Value::String(s)) = out.get_mut(f) {
            if !s.starts_with('\\') {
                s.insert(0, '\\');
            }
        }
    }
    Value::Object(out)
}
