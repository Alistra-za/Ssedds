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
use crate::sets_doc::{SetEntry, SetsDoc};
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

use super::cs_pbr::floats;
use super::sets::{views, SetView};
use engine::config::Subtype;
use engine::shader::Shader;
use std::collections::{BTreeMap, BTreeSet};

/// The schema's `$id`, written as `$schema` (D6). PGPatcher ignores it; editors use it.
pub const SCHEMA_URL: &str =
    "https://raw.githubusercontent.com/hakasapl/PGPatcher/main/schema/complex_material.schema.json";

/// The file's text (design §6): `$schema`, then the block's well-typed known keys, every number a float. serde_json
/// pretty (2-space indent, keys sorted, so `$schema` comes first) and a final newline.
pub fn to_text(values: &Map<String, Json>) -> String {
    let mut obj = Map::new();
    obj.insert("$schema".into(), json!(SCHEMA_URL));
    for (k, v) in values {
        if key(k).is_some_and(|f| valid(f.kind, v)) {
            obj.insert(k.clone(), floats(v));
        }
    }
    let mut s = serde_json::to_string_pretty(&Json::Object(obj)).expect("JSON values serialise");
    s.push('\n');
    s
}

/// The set's complex `_m` members, as item keys (design §4): resolved `complex_material`, or `envmask` in a set whose
/// shader (the choice, else the engine's pick) is Complex Material.
pub fn cm_members(m: &AppModel, v: &SetView) -> Vec<String> {
    let shader = v.shader_choice.or(v.shader.as_ref().map(|p| p.shader));
    v.members
        .iter()
        .filter(|k| {
            m.item(k)
                .and_then(|i| i.job.as_ref())
                .is_some_and(|j| match j.profile.subtype {
                    Subtype::ComplexMaterial => true,
                    Subtype::Envmask => shader == Some(Shader::ComplexMaterial),
                    _ => false,
                })
        })
        .cloned()
        .collect()
}

/// Whether the set editor draws the card: a complex `_m` member, or a stored block (never hide the user's values).
pub fn shows_card(m: &AppModel, v: &SetView) -> bool {
    !cm_members(m, v).is_empty() || m.cm_block(&v.id).is_some()
}

/// One member's JSON (design §6).
#[derive(Debug, Clone, PartialEq)]
pub struct CmTarget {
    pub set: String,
    /// The member's item key.
    pub key: String,
    /// The output's file name without extension (`iron_m`).
    pub name: String,
    /// The JSON's path under the export root, `/`-separated (the hash record's key), or why it is not written.
    pub rel: Result<String, String>,
}

/// The paths, lower-cased, that more than one exporting member writes. Windows file names ignore case.
pub fn clashes<'a>(rels: impl Iterator<Item = &'a str>) -> BTreeSet<String> {
    let mut n: BTreeMap<String, usize> = BTreeMap::new();
    for r in rels {
        *n.entry(r.to_lowercase()).or_default() += 1;
    }
    n.into_iter()
        .filter(|(_, c)| *c > 1)
        .map(|(r, _)| r)
        .collect()
}

/// Every card set's members with their JSON path: the job's output with `.json` (so `_em`, `_envmask`, renames
/// and packed cpm `_m` follow). The output is not `item.job_key`, which is case-folded. Members of exporting sets
/// that share a path are refused.
pub fn targets(m: &AppModel) -> Vec<CmTarget> {
    let root = m.export_root();
    let mut out = vec![];
    for v in views(m).iter() {
        for key in cm_members(m, v) {
            let Some(job) = m.item(&key).and_then(|i| i.job.as_ref()) else {
                continue;
            };
            let json = job.output.with_extension("json");
            let rel = json
                .strip_prefix(&root)
                .map(|r| r.to_string_lossy().replace('\\', "/"))
                .map_err(|_| {
                    format!(
                        "{} is outside the export folder {}",
                        json.display(),
                        root.display()
                    )
                });
            out.push(CmTarget {
                set: v.id.clone(),
                name: job
                    .output
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                key,
                rel,
            });
        }
    }
    // Ruling C5: one parse of the document for every target.
    let entries = m.sets_doc.as_ref().map(|d| d.entries()).unwrap_or_default();
    refuse_clashes(&mut out, |s| exports(&entries, s));
    out
}

/// Whether `set`'s `[sets.cm]` has `export` on. The first entry with the id decides, as in `cm_block`.
pub fn exports(entries: &[SetEntry], set: &str) -> bool {
    entries
        .iter()
        .find(|e| same_id(&e.id, set))
        .is_some_and(|e| e.cm.as_ref().is_some_and(|b| b.export))
}

/// Refuses the targets of exporting sets that share a path with another exporting target. The planner never gives
/// two outputs one path, so this guards hand edits and future planner changes.
pub fn refuse_clashes(out: &mut [CmTarget], exporting: impl Fn(&str) -> bool) {
    let twice = clashes(
        out.iter()
            .filter(|t| exporting(&t.set))
            .filter_map(|t| t.rel.as_deref().ok()),
    );
    for t in out.iter_mut() {
        let clash = exporting(&t.set)
            && t.rel
                .as_ref()
                .is_ok_and(|r| twice.contains(&r.to_lowercase()));
        if clash {
            t.rel = Err(format!(
                "{}.json is written by two complex _m (names ignore case)",
                t.name
            ));
        }
    }
}
