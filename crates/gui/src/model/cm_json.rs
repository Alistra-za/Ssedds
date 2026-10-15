//! PGPatcher complex material JSON (complex material design 2026-10-14): the `[sets.cm]` block, which members get
//! `<texture>.json`, its text, the export hooks and the card's warnings. No egui types.
use super::cs_pbr::{valid_value, FieldKind};
use serde_json::{json, Map, Value as Json};
use toml_edit::TableLike;

/// The type PGPatcher reads for a key (`complex_material.schema.json`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmKind {
    Bool,
    Num,
    Rgb,
}

/// One schema key: its type and the card's hint while it is unset (design §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CmKey {
    pub name: &'static str,
    pub kind: CmKind,
    pub hint: &'static str,
}

const fn k(name: &'static str, kind: CmKind, hint: &'static str) -> CmKey {
    CmKey { name, kind, hint }
}

/// The schema's keys in schema order. `$schema` is written by the writer, never stored.
pub const KEYS: [CmKey; 6] = [
    k(
        "specular_enabled",
        CmKind::Bool,
        "PGPatcher: on if the _m has gloss (G), else the mesh's",
    ),
    k("specular_color", CmKind::Rgb, "PGPatcher: 1, 1, 1"),
    k("specular_strength", CmKind::Num, "PGPatcher: 1"),
    k(
        "glossiness",
        CmKind::Num,
        "PGPatcher: keeps the mesh's value",
    ),
    k("environment_map_scale", CmKind::Num, "PGPatcher: 1"),
    k(
        "dynamic_cubemap",
        CmKind::Bool,
        "PGPatcher: on (dynamic cubemap)",
    ),
];

pub fn key(name: &str) -> Option<&'static CmKey> {
    KEYS.iter().find(|k| k.name == name)
}

/// Whether `v` has the type PGPatcher reads. A colour is exactly three numbers: 1.2.0 crashes on anything else.
pub fn valid(kind: CmKind, v: &Json) -> bool {
    match kind {
        CmKind::Bool => v.is_boolean(),
        CmKind::Num => valid_value(FieldKind::Num, v),
        CmKind::Rgb => valid_value(FieldKind::Rgb, v),
    }
}

/// The value a key takes when its box is ticked (design §5). Glossiness 80 is the schema example, never a mesh's (D10).
pub fn first_value(k: &CmKey) -> Json {
    match k.kind {
        CmKind::Bool => json!(true),
        CmKind::Rgb => json!([1.0, 1.0, 1.0]),
        CmKind::Num if k.name == "glossiness" => json!(80.0),
        CmKind::Num => json!(1.0),
    }
}

/// `[sets.cm]` as read (design §3).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CmBlock {
    /// Write the JSON on export. Absent, `false` or not a bool is off.
    pub export: bool,
    /// Set keys with a usable value, as read.
    pub values: Map<String, Json>,
    /// Known keys (`export` included) with the wrong type: kept in the file, warned about, not exported.
    pub bad: Vec<String>,
}

/// Reads a `[sets.cm]` table. Unknown keys stay in the file and are never exported (the schema forbids them).
pub(crate) fn block_of(t: &dyn TableLike) -> CmBlock {
    let mut b = CmBlock::default();
    for (name, item) in t.iter() {
        let v = crate::project::toml_to_json(item);
        if name == "export" {
            match v {
                Some(Json::Bool(on)) => b.export = on,
                _ => b.bad.push(name.to_string()),
            }
        } else if let Some(k) = key(name) {
            match v.filter(|v| valid(k.kind, v)) {
                Some(v) => {
                    b.values.insert(name.to_string(), v);
                }
                None => b.bad.push(name.to_string()),
            }
        }
    }
    b
}

use super::AppModel;
use crate::sets_doc::SetsDoc;
use engine::sets::same_id;

impl AppModel {
    /// Set `set`'s `[sets.cm]` block, as the document holds it.
    pub fn cm_block(&self, set: &str) -> Option<CmBlock> {
        self.sets_doc
            .as_ref()?
            .entries()
            .into_iter()
            .find(|e| same_id(&e.id, set))?
            .cm
    }

    /// One undoable edit of `[sets.cm]`. Nothing is planned: the block never changes a job.
    fn cm_edit(
        &mut self,
        set: &str,
        label: &str,
        merge: Option<String>,
        f: impl FnOnce(&mut SetsDoc, &str),
    ) {
        if !self.sets_editable() {
            return;
        }
        let s = set.to_string();
        self.edit_docs(label, merge, false, move |_, d| f(d, &s));
    }

    /// Set (or with `None` remove) key `key` of `[sets.cm]`. Drags of one key merge into one undo step.
    pub fn set_cm_value(&mut self, set: &str, key: &str, v: Option<Json>) {
        let merge = Some(format!("{set}:cm:{key}"));
        let k = key.to_string();
        self.cm_edit(set, &format!("Change {key}"), merge, move |d, s| {
            d.set_cm_value(s, &k, v)
        });
    }

    pub fn set_cm_export(&mut self, set: &str, on: bool) {
        let label = if on {
            "Export complex material JSON"
        } else {
            "Stop exporting complex material JSON"
        };
        self.cm_edit(set, label, None, move |d, s| d.set_cm_export(s, on));
    }
}
