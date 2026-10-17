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
            .iter()
            .find(|e| same_id(&e.id, set))?
            .cm
            .clone()
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
/// and packed complex `_m` follow). The output is not `item.job_key`, which is case-folded. Members of exporting sets
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

use super::history::{recorded, sha256_of};
use engine::dds::may_replace;

use std::path::PathBuf;

/// What an export about to start writes once it has ended (computed when it starts, like the CS PBR files).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PendingCm {
    pub root: PathBuf,
    /// (path under the root, `/`-separated; text).
    pub files: Vec<(String, String)>,
    /// Refused files and left-out keys, for the export report.
    pub notes: Vec<String>,
}

/// Design §6: one file per complex `_m` of every set whose export is ticked, on every target (D2). `Some` even when
/// nothing is ticked: the untick/orphan report needs it.
pub fn prepare(m: &AppModel) -> Option<PendingCm> {
    let entries = m.sets_doc.as_ref()?.entries();
    let ts = targets(m);
    let mut p = PendingCm {
        root: m.export_root(),
        ..Default::default()
    };
    for (i, e) in entries.iter().enumerate() {
        // The first entry with an id decides, as in `exports`.
        if entries[..i].iter().any(|f| same_id(&f.id, &e.id)) {
            continue;
        }
        let Some(b) = e.cm.as_ref().filter(|b| b.export) else {
            continue;
        };
        for k in &b.bad {
            p.notes.push(format!(
                "{}: cm.{k} has the wrong type and was left out",
                e.id
            ));
        }
        let mine: Vec<&CmTarget> = ts.iter().filter(|t| same_id(&t.set, &e.id)).collect();
        if mine.is_empty() {
            p.notes.push(format!(
                "set {}: no complex _m; complex material JSON not written",
                e.id
            ));
            continue;
        }
        let text = to_text(&b.values);
        let doc: Json = serde_json::from_str(&text).expect("to_text writes JSON");
        // A safety net that should never fire: `to_text` writes only schema keys with their schema types.
        if let Err(errs) = crate::pgp::validate::validate_cm(&doc) {
            p.notes.push(format!(
                "set {}: complex material JSON not written: {}",
                e.id,
                errs.join("; ")
            ));
            continue;
        }
        for t in mine {
            match &t.rel {
                Ok(r) => p.files.push((r.clone(), text.clone())),
                Err(why) => p.notes.push(format!(
                    "{}: complex material JSON not written: {why}",
                    t.name
                )),
            }
        }
    }
    Some(p)
}

/// After a clean export (design §6): write each file whose `_m` is on disk, unless it was edited outside Tessalith,
/// then report the recorded files this run did not write (D7: never deleted). `shas` is the record (path → hash of
/// what was last written there); returns the new record.
pub fn finish(
    p: &PendingCm,
    shas: &BTreeMap<String, String>,
    notes: &mut Vec<String>,
) -> BTreeMap<String, String> {
    notes.extend(p.notes.iter().cloned());
    let mut out = BTreeMap::new();
    let mut written = 0;
    for (rel, text) in &p.files {
        let path = p.root.join(rel);
        // O6 (as in cs_pbr_export): a case-only rename drops sdc's own unchanged file under the old name; a hand
        // edit stays, and so does its record (the orphan pass below skips a case-insensitive match).
        let renamed = shas
            .iter()
            .find(|(r, _)| r.as_str() != rel.as_str() && r.eq_ignore_ascii_case(rel));
        let dds = path.with_extension("dds");
        if let Some((old, old_sha)) = renamed.filter(|_| dds.is_file()) {
            if sha256_of(&p.root.join(old)).as_deref() == Some(old_sha.as_str()) {
                let _ = std::fs::remove_file(p.root.join(old));
            }
        }
        let last = shas.get(rel).map(String::as_str);
        let skip = if !dds.is_file() {
            Some(format!(
                "{}: {} was not exported; JSON not written",
                path.display(),
                dds.file_name().unwrap_or_default().to_string_lossy()
            ))
        } else if let Err(k) = may_replace(&path, text.as_bytes(), recorded(last)) {
            Some(k.note(&path))
        } else {
            match engine::dds::write_atomic(&path, text.as_bytes()) {
                Ok(()) => {
                    written += 1;
                    if let Some(s) = sha256_of(&path) {
                        out.insert(rel.clone(), s);
                    }
                    None
                }
                Err(e) => Some(format!("{} not written: {e}", path.display())),
            }
        };
        if let Some(n) = skip {
            notes.push(n);
            if let Some(s) = last {
                out.insert(rel.clone(), s.to_string());
            } else if let Some((old, s)) = renamed.filter(|(o, _)| p.root.join(o).is_file()) {
                out.insert(old.clone(), s.clone());
            }
        }
    }
    if written > 0 {
        notes.push(format!(
            "Complex material JSON written: {written} file{} under {}",
            if written == 1 { "" } else { "s" },
            p.root.display()
        ));
    }
    for (rel, sha) in shas {
        if p.files.iter().any(|(r, _)| r.eq_ignore_ascii_case(rel)) {
            continue;
        }
        let path = p.root.join(rel);
        if path.is_file() {
            notes.push(format!(
                "{}: complex material export is off for its set (or the _m left the set); the file was left in \
                 place. Delete it to drop the overrides.",
                path.display()
            ));
            out.insert(rel.clone(), sha.clone());
        }
    }
    out
}

impl AppModel {
    /// The export ended. After a clean export, write the files of `p` (taken from `pending_cm` by the caller) and
    /// record their hashes (saved, like the CS PBR record). Otherwise only add a note.
    #[doc(hidden)]
    pub fn finish_cm_json(&mut self, p: Option<PendingCm>) {
        let Some(p) = p else {
            return;
        };
        let ok = self.export.error.is_none()
            && self
                .export
                .summary
                .as_ref()
                .is_some_and(|s| s.cancelled.is_empty() && s.failed == 0);
        if !ok {
            if !p.files.is_empty() {
                self.export
                    .notes
                    .push("Complex material JSON not written: the export did not finish".into());
            }
            return;
        }
        let recorded = match self.sets_doc.as_ref() {
            Some(d) => d.cm_json_shas(),
            None => return,
        };
        let shas = finish(&p, &recorded, &mut self.export.notes);
        if shas == recorded {
            return;
        }
        let Some(d) = self.sets_doc.as_mut() else {
            return;
        };
        if d.read_only() {
            self.export.notes.push(
                "the complex material JSON hashes were not recorded: sdc-project.toml is read-only"
                    .into(),
            );
            return;
        }
        let was_dirty = d.is_dirty();
        d.set_cm_json_shas(&shas);
        self.save_hash_record(was_dirty);
    }
}

use engine::analyse::CS_COMPLEX_GATE_CODE;
use engine::codes::Code;
use engine::config::AlphaPolicy;

/// The smallest-mip alpha of `key`'s latest export, from its `VAL-M-COMPLEX-MARGIN` data, rounded (an averaged
/// non-square smallest mip can be fractional). `None` before an export, or when the engine raised no margin warning
/// (alpha below `validation.complex_m_warn_alpha`, 247 by default) (D17).
pub fn smallest_alpha(m: &AppModel, key: &str) -> Option<u8> {
    let jk = &m.item(key)?.job_key;
    let a = m
        .export
        .rows
        .iter()
        .find(|r| r.job_key.eq_ignore_ascii_case(jk))?
        .warnings
        .iter()
        .find(|w| w.code == Code::ValMComplexMargin)?
        .data
        .get("smallest_alpha")?
        .as_f64()?;
    Some(a.round().clamp(0.0, 255.0) as u8)
}

/// Written without alpha (an `envmask`, or an explicit `alpha.policy = remove`): PGPatcher reads it as an env mask.
fn env_written(m: &AppModel, key: &str) -> bool {
    m.item(key)
        .and_then(|i| i.job.as_ref())
        .is_some_and(|j| j.profile.alpha.policy == AlphaPolicy::Remove)
}

/// The card's amber lines (design §5), in order: wrong-typed keys, then per member its env-mask or alpha case and
/// its refusal. `ts` is `targets(m)`, computed once by the caller (ruling C5).
pub fn card_warnings(m: &AppModel, v: &SetView, ts: &[CmTarget]) -> Vec<String> {
    let mut out: Vec<String> = m
        .cm_block(&v.id)
        .map(|b| {
            b.bad
                .iter()
                .map(|k| format!("{k}: wrong type in sdc-project.toml; not exported"))
                .collect()
        })
        .unwrap_or_default();
    for t in ts.iter().filter(|t| same_id(&t.set, &v.id)) {
        let n = &t.name;
        if env_written(m, &t.key) {
            out.push(format!(
                "{n} is written as an env mask without alpha, so PGPatcher ignores the JSON. Set its sub-type to \
                 complex material."
            ));
        } else {
            match smallest_alpha(m, &t.key) {
                Some(a @ CS_COMPLEX_GATE_CODE..=254) => out.push(format!(
                    "{n}: smallest-mip alpha {a}. PGPatcher treats it as complex material and applies {n}.json, but \
                     Community Shaders renders it as a plain env mask (alpha ≥ 251)."
                )),
                Some(255) => out.push(format!(
                    "PGPatcher likely reads {n} as an env mask; the JSON will not be applied."
                )),
                _ => {}
            }
        }
        if let Err(why) = &t.rel {
            out.push(format!("{n}: complex material JSON not written: {why}"));
        }
    }
    out
}
