//! JSON accessors with nlohmann's semantics for well-typed values. `simulate` rejects wrong types first
//! (`check.rs`, Task 4), so these never see one there.
use serde_json::{Map, Value};

/// PGPatcher's `flag(json, key)`: present and `true`.
pub(crate) fn flag(e: &Map<String, Value>, k: &str) -> bool {
    e.get(k).and_then(Value::as_bool) == Some(true)
}
