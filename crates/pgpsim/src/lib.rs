//! PGPatcher 1.2.0 TruePBR patch, simulated in memory for the 3D preview (P1 spec §2.1), plus the 2.2.0 `alphaprop_*` keys.
//! Rules: the PGPatcher 1.2.0 TruePBR rule notes ("01", NIF team plan p1). Pure: no GPU, no egui, no IO.
//! It stops at "the NIF as PGPatcher leaves it", a `MaterialState`; CS's load-time remap belongs to render3d.
mod check;
mod index;
mod json;
mod matching;
mod patch;
mod prep;
mod slots;
mod text;

pub use check::range_warnings;
pub use index::{MatchIndex, ShapeKeys};
pub use matching::{matched_path, shape_matches};
pub use nif::material::MaterialState;
pub use prep::prepare;
use serde_json::{Map, Value};

/// What In game (simulated) models, said once (Note GD7): the edit panel's hover and first line read it.
pub const SIMULATED_AS: &str = "Simulated as PGPatcher 1.2.0 with 2.2's alphaprop keys: loose JSON only, no modrules priority, read at the last scan";

pub struct SimInput<'a> {
    /// One PGPatcher entry exactly as sdc writes it (`pgp::writer::entry_node(..).to_value()`).
    pub entry: &'a Value,
    /// The file's `default` object (`pgp::writer::document(&[]).to_value()["default"]`), or `Value::Null`.
    pub default: &'a Value,
    /// Mesh path relative to Data, PGPatcher form (`meshes\…`, any case).
    pub nif_path: &'a str,
    pub source: &'a MaterialState,
    /// Does a texture exist in the VFS / will sdc produce it? Called with lower-cased PGPatcher-form paths for
    /// EVERY non-empty slot after the slot pass (01 §1.4, TP:348-375): the new `textures\pbr\…` paths, `slotN`
    /// overrides, the cubemap, and untouched source slots (which may live in BSAs).
    pub exists: &'a dyn Fn(&str) -> bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Badge {
    /// A key the preview does not show (geometry ops); the value is the key name.
    NotPreviewed(&'static str),
}

#[derive(Debug, Clone, PartialEq)]
pub enum SimResult {
    NoMatch,
    /// `material.hidden` is set by `delete`.
    Patched {
        material: MaterialState,
        matched_path: String,
        badges: Vec<Badge>,
    },
    /// PGPatcher would skip this shape for this file; the shape keeps `source`.
    Skip(String),
}

/// One entry (sdc's case): prepare, match (with `nif_filter`), then patch.
pub fn simulate(i: &SimInput) -> SimResult {
    let prepared = prepare(i.entry, i.default);
    if !shape_matches(&prepared, i.nif_path, &i.source.slots) {
        return SimResult::NoMatch;
    }
    match (
        prepared.as_object(),
        matched_path(&prepared, &i.source.slots),
    ) {
        (Some(e), Some(mp)) => patch::run(&[(e, mp)], i.source, i.exists),
        _ => SimResult::NoMatch,
    }
}

/// A whole PGPatcher file (01 §1.4-§1.5): every entry is prepared with `default` (`Value::Null` for a
/// bare-array file); all entries matching this shape apply in index order, so the last one wins per field.
/// The §1.4 texture check runs over the slot pass of all of them. `matched_path` = the last applied entry's.
pub fn simulate_file(
    entries: &[Value],
    default: &Value,
    nif_path: &str,
    source: &MaterialState,
    exists: &dyn Fn(&str) -> bool,
) -> SimResult {
    let prepared: Vec<Value> = entries.iter().map(|e| prepare(e, default)).collect();
    let matches: Vec<(&Map<String, Value>, String)> = prepared
        .iter()
        .filter(|p| shape_matches(p, nif_path, &source.slots))
        .filter_map(|p| Some((p.as_object()?, matched_path(p, &source.slots)?)))
        .collect();
    patch::run(&matches, source, exists)
}
