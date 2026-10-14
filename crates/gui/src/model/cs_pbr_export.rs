//! The Community Shaders runtime JSON (design §5): `PBRTextureSets\<EDID>.json` and `PBRMaterialObjects\<EDID>.json`,
//! one file per EDID per block, written after a clean export. No egui types.
use super::cs_pbr::{field, floats, Kind, GLINT_FIELDS};
use serde::Serialize;
use serde_json::{json, Map, Value as Json};

/// The material-object `specularLevel` written when the block has none: the material reset value. CS's own default
/// (1.0) is 25× that (research §2), so the key is always written (plan ruling R9).
pub const MATO_SPECULAR: f64 = 0.04;

/// `<folder>/<EDID>.json`, the path under the export root (and the hash record's key).
pub fn rel_path(kind: Kind, edid: &str) -> String {
    format!("{}/{edid}.json", kind.folder())
}

/// One file's text: the block's known fields with every number a float (CS drops int colour arrays), 4-space
/// indent, keys sorted as CS's own saver (nlohmann::json) writes them, `\n` line ends, no final newline (ruling R3).
/// serde_json's map is sorted (no `preserve_order` in Cargo.lock); the golden tests pin it.
pub fn to_text(kind: Kind, values: &Map<String, Json>) -> String {
    let mut obj = Map::new();
    for (k, v) in values {
        if field(kind, k).is_none() {
            continue;
        }
        let v = match v.as_object() {
            Some(g) => Json::Object(
                g.iter()
                    .filter(|(gk, _)| {
                        *gk == "enabled" || GLINT_FIELDS.iter().any(|f| f.name == gk.as_str())
                    })
                    .map(|(gk, gv)| (gk.clone(), gv.clone()))
                    .collect(),
            ),
            None => v.clone(),
        };
        obj.insert(k.clone(), floats(&v));
    }
    if kind == Kind::Mato {
        obj.entry("specularLevel").or_insert(json!(MATO_SPECULAR));
    }
    let mut out = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(
        &mut out,
        serde_json::ser::PrettyFormatter::with_indent(b"    "),
    );
    Json::Object(obj)
        .serialize(&mut ser)
        .expect("JSON values serialise");
    String::from_utf8(out).expect("serde_json writes UTF-8")
}
