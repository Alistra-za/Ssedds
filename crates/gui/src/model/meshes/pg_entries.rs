//! v2 B8: the PGPatcher JSON an MO2 instance's loose mod folders hold, and pgpsim over it for one shape (01 §1.1-§1.5).
use super::{MeshesModel, ScanTarget};
use crate::model::set_preview::canon;
use modscan::mo2::{loose_origins, Instance};
use nif::extra::ExtraValue;
use nif::material::MaterialState;
use nif::model::{NifModel, ShapeModel};
use pgpsim::{Badge, SimResult};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

/// The line under the toggle in In game (simulated).
pub const IN_GAME_SCOPE: &str =
    "Simulated from loose PGPatcher JSON, in MO2 mod order. JSON inside BSAs is not read.";
/// The same place for a folder scan (no MO2 instance).
pub const NO_INSTANCE: &str = "In game needs an MO2 instance: no PGPatcher JSON was read.";
pub const PG_IGNORED: &str = "In game: PGPatcher skips this shape (PG_IGNORE).";
pub const NO_MATCH: &str = "In game: no PGPatcher JSON matches this shape.";

/// One PGPatcher config file: its data-relative path as the winning copy spells it, and its entries already
/// prepared with the file's `default` (so they go to pgpsim with `Value::Null`).
#[derive(Debug, Clone, PartialEq)]
pub struct PgFile {
    pub file: String,
    pub entries: Vec<Value>,
}

/// Every loose `PBRNifPatcher\**\*.json` of Data, the enabled mods and overwrite (the VFS winning copy of each
/// path), highest MO2 priority first, then path A→Z. Files PGPatcher would skip (parse error, an object without
/// `default` and `entries`) are left out.
pub fn pg_entries(inst: &Instance) -> Vec<PgFile> {
    let Ok((origins, _)) = loose_origins(inst) else {
        return vec![];
    };
    let mut found: HashMap<String, (i32, String, PathBuf)> = HashMap::new();
    for o in &origins {
        let Ok(rd) = std::fs::read_dir(&o.root) else {
            continue;
        };
        let mut dirs: Vec<(PathBuf, String)> = rd
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case("PBRNifPatcher")
            })
            .map(|e| (e.path(), e.file_name().to_string_lossy().into_owned()))
            .collect();
        while let Some((d, rel)) = dirs.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else {
                continue;
            };
            // `file_type` does not follow links: a link or junction (a loop, or a path out of the mod) is skipped.
            for (t, e) in rd.flatten().filter_map(|e| Some((e.file_type().ok()?, e))) {
                let r = format!("{rel}\\{}", e.file_name().to_string_lossy());
                if t.is_dir() {
                    dirs.push((e.path(), r));
                } else if t.is_file()
                    && e.path()
                        .extension()
                        .is_some_and(|x| x.eq_ignore_ascii_case("json"))
                {
                    // origins come lowest priority first: a later one is the winning copy
                    found.insert(r.to_ascii_lowercase(), (o.priority, r, e.path()));
                }
            }
        }
    }
    let mut files: Vec<(String, (i32, String, PathBuf))> = found.into_iter().collect();
    // shortcut: MO2 priority stands in for PGPatcher's own mod priority (modrules.json, 01 §1.5); read that file if the owner's orders differ.
    files.sort_by(|(ka, a), (kb, b)| b.0.cmp(&a.0).then_with(|| ka.cmp(kb)));
    // shortcut: loose files only; JSON packed in a BSA is not read (the UI says so).
    files
        .into_iter()
        .filter_map(|(_, (_, file, path))| {
            let text = String::from_utf8(std::fs::read(path).ok()?).ok()?;
            let v: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
            let (entries, default) = match v {
                Value::Array(a) => (a, Value::Null),
                Value::Object(mut o) => match (o.remove("entries"), o.remove("default")) {
                    (Some(Value::Array(a)), Some(d)) => (a, d),
                    _ => return None,
                },
                _ => return None,
            };
            let entries = entries
                .iter()
                .map(|e| pgpsim::prepare(e, &default))
                .collect();
            Some(PgFile { file, entries })
        })
        .collect()
}

/// PGPatcher's PG_IGNORE (01 §1.2 item 3): an `NiBooleanExtraData` "PG_IGNORE" with a true value on the shape or
/// on its lighting shader.
pub fn pg_ignore(m: &NifModel, s: &ShapeModel) -> bool {
    let shader = s
        .shader
        .and_then(|b| m.lighting.get(&b))
        .map(|l| &l.prop.net);
    std::iter::once(&s.net)
        .chain(shader)
        .flat_map(|net| m.extras_of(net))
        .any(|x| {
            x.kind == "NiBooleanExtraData"
                && x.name.as_deref() == Some(b"PG_IGNORE")
                && matches!(x.value, Some(ExtraValue::Bool(v)) if v != 0)
        })
}

/// One shape through PGPatcher's files (01 §1.4-§1.5): the first file (in `files`' order) that patches it wins; a
/// file that would skip it (a missing texture, a wrong-typed key) is rejected for this shape and the next may win.
/// Returns the material and its lines.
pub fn simulate_shape(
    files: &[PgFile],
    nif_path: &str,
    source: &MaterialState,
    exists: &dyn Fn(&str) -> bool,
) -> (MaterialState, Vec<String>) {
    let mut skip = None;
    for f in files {
        if !f
            .entries
            .iter()
            .any(|e| pgpsim::shape_matches(e, nif_path, &source.slots))
        {
            continue;
        }
        match pgpsim::simulate_file(&f.entries, &Value::Null, nif_path, source, exists) {
            SimResult::Patched {
                material, badges, ..
            } => {
                let mut notes = vec![format!("In game: PGPatcher applies {}.", f.file)];
                if !badges.is_empty() {
                    let keys: Vec<&str> = badges
                        .iter()
                        .map(|b| match b {
                            Badge::NotPreviewed(k) => *k,
                        })
                        .collect();
                    notes.push(format!("Not previewed: {}.", keys.join(", ")));
                }
                return (material, notes);
            }
            SimResult::Skip(why) => {
                skip.get_or_insert_with(|| format!("In game: {} is not applied ({why}).", f.file));
            }
            SimResult::NoMatch => {}
        }
    }
    (
        source.clone(),
        vec![skip.unwrap_or_else(|| NO_MATCH.to_string())],
    )
}

impl MeshesModel {
    /// v2 B8: `edit.in_game` for the side shown: computed while In game shows (the JSON read once per switch and per
    /// scan target), None on Edited source. Per change, never per frame: the clock is paused In game (F21).
    pub(super) fn refresh_in_game(&mut self) {
        let show = !self.view_source;
        let target = self.target.clone();
        let store = &self.store;
        let Some(e) = self.edit.as_mut() else { return };
        if e.le_mesh || !show {
            e.in_game = None;
            return;
        }
        if e.pg.as_ref().is_none_or(|(t, _)| *t != target) {
            let files = match &target {
                Some(ScanTarget::Mo2 { root, profile }) => Instance::load(root, Some(profile))
                    .map(|i| pg_entries(&i))
                    .unwrap_or_default(),
                _ => vec![],
            };
            e.pg = Some((target, files));
        }
        // Unknown (a store error) counts as existing: an error must never show a false skip (as set_preview).
        let exists = |p: &str| {
            store
                .as_ref()
                .is_none_or(|s| !matches!(s.texture_location(&canon(p)), Ok(None)))
        };
        let rows = e.in_game_materials(&exists);
        e.in_game = Some(rows);
    }
}
