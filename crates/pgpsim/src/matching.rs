//! Which shapes an entry applies to: 01 §1.3 [Src 1.2.0 TP:274-311, 403-547].
use crate::text::{icontains, strip_pbr, tex_base, tex_base_slot};
use nif::path::TexPath;
use serde_json::{Map, Value};

/// The 9 slots as PGPatcher reads them: the NIF string ASCII-lower-cased, nothing else [NU:363-380].
/// Missing slots are empty.
pub(crate) fn pg_slots(slots: &[TexPath]) -> [String; 9] {
    std::array::from_fn(|i| {
        slots
            .get(i)
            .map(|t| t.raw.to_ascii_lowercase())
            .unwrap_or_default()
    })
}

/// A string-valued key; any other JSON type counts as absent here (`simulate` reports wrong types).
pub(crate) fn str_of<'a>(e: &'a Map<String, Value>, k: &str) -> Option<&'a str> {
    e.get(k).and_then(Value::as_str)
}

/// The first rule that hits, in PGPatcher's insert order (match_normal, match_diffuse, path_contains,
/// match1..match8; insert-first), returned as the `texName` PGPatcher hands to `insertTruePBRData`.
/// `None`: no rule hits. `nif_filter` is not checked here.
pub(crate) fn tex_name(e: &Map<String, Value>, slots: &[String; 9]) -> Option<String> {
    let pre0 = strip_pbr(tex_base(&slots[0]));
    let pre1 = strip_pbr(tex_base_slot(&slots[1], 1));
    // match_normal / match_diffuse: ends-with on the prepared value, whose leading "\" makes it whole
    // components (TP:403-466 is a reversed-key binary search with the same result).
    if str_of(e, "match_normal").is_some_and(|v| pre1.ends_with(&tex_base(v))) {
        return Some(pre1);
    }
    if str_of(e, "match_diffuse").is_some_and(|v| pre0.ends_with(&tex_base(v))) {
        return Some(pre0);
    }
    if str_of(e, "path_contains").is_some_and(|v| icontains(&pre0, v)) {
        return Some(pre0);
    }
    // matchN: the full slot path, "textures\" prepended to the value unless it starts with exactly
    // "textures\" (case-sensitive check), then lower-cased; N = 1..8 (the code loops i < 8) [TP:204-217, 499-532].
    for (i, slot) in slots.iter().enumerate().take(8) {
        let Some(v) = str_of(e, &format!("match{}", i + 1)) else {
            continue;
        };
        let want = if v.is_empty() || v.starts_with("textures\\") {
            v.to_string()
        } else {
            format!("textures\\{v}")
        };
        if !slot.is_empty() && want.to_ascii_lowercase() == *slot {
            return Some(tex_base_slot(slot, i));
        }
    }
    None
}

/// Does the prepared entry (`prepare(entry, default)`) apply to this shape? `nif_path` is the mesh path
/// relative to Data (`meshes\…`, any case), used for `nif_filter` (case-insensitive substring, TP:544-547).
/// `slots` are the shape's texture-set paths (`MaterialState::slots`).
pub fn shape_matches(prepared: &Value, nif_path: &str, slots: &[TexPath]) -> bool {
    let Some(e) = prepared.as_object() else {
        return false;
    };
    if str_of(e, "nif_filter").is_some_and(|f| !icontains(nif_path, f)) {
        return false;
    }
    tex_name(e, &pg_slots(slots)).is_some()
}
