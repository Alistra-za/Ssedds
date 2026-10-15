//! Task 10b: the Advanced skin card's fields (Community Shaders `SkinProfile`, CS README section 4). Per set,
//! non-inheritable: a field is either unset (nothing stored, the CS default shows as a hint) or an explicit value in
//! the set's `skin` table of `sdc-project.toml`, under the exact CS key. Nothing is exported yet (DECISIONS AG79).
use super::AppModel;
use crate::pgp::settings::Settings;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    /// A float with the UI-only clamp `lo..=hi` (AG83).
    Num {
        default: f64,
        lo: f64,
        hi: f64,
    },
    Flag {
        default: bool,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct SkinField {
    pub key: &'static str,
    pub group: &'static str,
    pub kind: Kind,
}

const fn num(key: &'static str, group: &'static str, default: f64, lo: f64, hi: f64) -> SkinField {
    SkinField {
        key,
        group,
        kind: Kind::Num { default, lo, hi },
    }
}
const fn flag(key: &'static str, group: &'static str, default: bool) -> SkinField {
    SkinField {
        key,
        group,
        kind: Kind::Flag { default },
    }
}

pub const GROUPS: [&str; 5] = ["Specular", "Physical", "Edge & detail", "SSS", "Fuzz"];

/// The 20 per-material `SkinProfile` fields in README order. `WetResponse` is wetness, which CS keeps global: left out.
pub const FIELDS: [SkinField; 20] = [
    num("SkinMainRoughness", "Specular", 0.7, 0.0, 1.0),
    num("SkinSecondRoughness", "Specular", 0.35, 0.0, 1.0),
    num("SkinSpecularTexMultiplier", "Specular", 1.0, 0.0, 4.0),
    num("SecondarySpecularStrength", "Specular", 0.15, 0.0, 1.0),
    num("F0", "Specular", 0.0278, 0.0, 1.0),
    num("BaseColorMultiplier", "Physical", 1.0, 0.0, 4.0),
    num("PhysicalMainRoughnessMultiplier", "Physical", 1.3, 0.0, 4.0),
    num(
        "PhysicalSecondRoughnessMultiplier",
        "Physical",
        0.75,
        0.0,
        4.0,
    ),
    num("PhysicalSpecularStrength", "Physical", 1.0, 0.0, 4.0),
    num("ExtraEdgeRoughness", "Edge & detail", 0.25, 0.0, 1.0),
    flag("EnableSkinDetail", "Edge & detail", true),
    num("SkinDetailStrength", "Edge & detail", 0.25, 0.0, 1.0),
    num("SkinDetailTiling", "Edge & detail", 10.0, 0.1, 100.0),
    num("BodyTilingMultiplier", "Edge & detail", 2.0, 0.0, 4.0),
    num("Translucency", "SSS", 0.1, 0.0, 1.0),
    num("sssWidth", "SSS", 0.2, 0.0, 1.0),
    flag("UseSSS", "SSS", true),
    num("FuzzStrength", "Fuzz", 1.0, 0.0, 1.0),
    num("FuzzRoughness", "Fuzz", 0.35, 0.0, 1.0),
    num("FuzzF0", "Fuzz", 0.045, 0.0, 1.0),
];

impl AppModel {
    /// Set `set`'s explicit skin values (every key under `skin`, unknown ones included).
    pub fn skin_values(&self, set: &str) -> Settings {
        self.sets_doc
            .as_ref()
            .and_then(|d| {
                d.entries()
                    .into_iter()
                    .find(|e| engine::sets::same_id(&e.id, set))
            })
            .map(|e| e.skin)
            .unwrap_or_default()
    }

    /// Set (or with `None` clear) skin field `key` of set `set`. One undo step; a drag on the same field coalesces.
    /// No plan: nothing in the skin table reaches the engine.
    pub fn set_skin_value(&mut self, set: &str, key: &str, v: Option<Value>) {
        if !self.sets_editable() {
            return;
        }
        let (s, path) = (set.to_string(), format!("skin.{key}"));
        let merge = Some(format!("{set}:{path}"));
        self.edit_docs(&format!("Change {path}"), merge, false, move |_, d| {
            d.set_value(&s, &path, v)
        });
    }
}
