//! v2 B8: the PGPatcher JSON an MO2 instance's loose mod folders hold, and pgpsim over it for one shape (01 §1.1-§1.5).
use super::{MeshesModel, ScanTarget};
use crate::model::set_preview::canon;
use modscan::mo2::{loose_origins, Instance};
use nif::extra::ExtraValue;
use nif::material::MaterialState;
use nif::model::{NifModel, ShapeModel};
use nif::path::TexPath;
use pgpsim::{Badge, MatchIndex, ShapeKeys, SimResult};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, TryRecvError};

/// The line under the toggle in In game (simulated).
pub const IN_GAME_SCOPE: &str =
    "Simulated from loose PGPatcher JSON, in MO2 mod order. JSON inside BSAs is not read.";
/// The same place for a folder scan (no MO2 instance).
pub const NO_INSTANCE: &str = "In game needs an MO2 instance: no PGPatcher JSON was read.";
pub const PG_IGNORED: &str = "In game: PGPatcher skips this shape (PG_IGNORE).";
pub const NO_MATCH: &str = "In game: no PGPatcher JSON matches this shape.";
/// B-P41: a red In game line, and a Warning in the edit view, when the simulation makes a non-PBR shape with an
/// LSCF var-8 controller PBR (B9: var 8 on PBR crashed the game). Export stays open: PGPatcher's output depends on
/// the JSON at play time.
pub const PGP_ENV_SCALE: &str =
    "PGPatcher makes this shape PBR: its Env map scale controller will crash the game.";
/// 01 §1.4's pre-patched case (fix F9).
pub const ALREADY_PBR: &str = "In game: already PBR. PGPatcher leaves it as is.";
/// Under the toggle while the worker reads the JSON (fix F2).
pub const READING_JSON: &str = "Reading PGPatcher JSON…";
/// The start of the line that replaces the scope line when the instance cannot be read (fix F3).
pub const INSTANCE_UNREADABLE: &str = "In game: the MO2 instance could not be read:";
/// The middle of a skip line, "In game: {file} is not applied ({why}).".
const NOT_APPLIED: &str = " is not applied (";

/// One PGPatcher config file: its data-relative path as the winning copy spells it, and its entries already
/// prepared with the file's `default` (so they go to pgpsim with `Value::Null`).
#[derive(Debug, Clone, PartialEq)]
pub struct PgFile {
    pub file: String,
    pub entries: Vec<Value>,
    /// `entries` by the names they can match (fix round 2, B-P38): a shape tests only its candidates.
    pub index: MatchIndex,
}

/// Every loose `PBRNifPatcher\**\*.json` of Data, the enabled mods and overwrite (the VFS winning copy of each
/// path), highest MO2 priority first, then path A→Z. Files PGPatcher would skip (parse error, an object without
/// `default` and `entries`) are left out.
pub fn pg_entries(inst: &Instance) -> Result<Vec<PgFile>, String> {
    let (origins, _) = loose_origins(inst).map_err(|e| e.to_string())?;
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
    Ok(files
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
            let entries: Vec<Value> = entries
                .iter()
                .map(|e| pgpsim::prepare(e, &default))
                .collect();
            let index = MatchIndex::new(&entries);
            Some(PgFile {
                file,
                entries,
                index,
            })
        })
        .collect())
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

/// Lines drawn amber (fix F4): a file PGPatcher rejects for the shape, and PG_IGNORE.
pub fn is_warning(note: &str) -> bool {
    note == PG_IGNORED || note.contains(NOT_APPLIED)
}

/// The JSON for `target`, as the worker reads it: an MO2 instance's files, nothing for a folder scan.
pub fn read_pg(target: Option<&ScanTarget>) -> Result<Vec<PgFile>, String> {
    match target {
        Some(ScanTarget::Mo2 { root, profile }) => Instance::load(root, Some(profile))
            .map_err(|e| e.to_string())
            .and_then(|i| pg_entries(&i)),
        _ => Ok(vec![]),
    }
}

/// The files with an entry that matches the shape (by its slots and the NIF path), as indices into `files`. Only
/// these can patch it; the scan over every entry runs again only when the slots change (fix F1).
/// Each file's matching entries are listed (ascending), and only those reach `simulate_file`: it drops the others
/// itself, so the result is the same. Fix round 2: each file's `MatchIndex` names the entries worth testing, so no
/// file is scanned entry by entry. The second value is how many entries were tested.
pub fn candidates(
    files: &[PgFile],
    nif_path: &str,
    slots: &[TexPath],
) -> (Vec<(usize, Vec<usize>)>, usize) {
    let keys = ShapeKeys::of(slots);
    let mut tested = 0;
    let mut out = vec![];
    for (i, f) in files.iter().enumerate() {
        let c = f.index.candidates(&keys);
        tested += c.len();
        let hit: Vec<usize> = c
            .into_iter()
            .filter(|&j| pgpsim::shape_matches(&f.entries[j], nif_path, slots))
            .collect();
        if !hit.is_empty() {
            out.push((i, hit));
        }
    }
    (out, tested)
}

/// One shape through PGPatcher's files (01 §1.4-§1.5): the first file (in `files`' order, the `candidates`) that
/// patches it wins; a file that would skip it (a missing texture, a wrong-typed key) is rejected for this shape and
/// the next may win. Returns the material and its lines.
pub fn simulate_shape(
    files: &[PgFile],
    cands: &[(usize, Vec<usize>)],
    nif_path: &str,
    source: &MaterialState,
    exists: &dyn Fn(&str) -> bool,
) -> (MaterialState, Vec<String>) {
    let mut skip = None;
    for (i, hit) in cands {
        let f = &files[*i];
        let entries: Vec<Value> = hit.iter().map(|&j| f.entries[j].clone()).collect();
        match pgpsim::simulate_file(&entries, &Value::Null, nif_path, source, exists) {
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
                skip.get_or_insert_with(|| format!("In game: {}{NOT_APPLIED}{why}).", f.file));
            }
            SimResult::NoMatch => {}
        }
    }
    let line = skip.unwrap_or_else(|| {
        // 01 §1.4 pre-patched case: no entry matched and slot 5 is an existing `_rmaos`: PGPatcher changes nothing.
        let rmaos = source
            .slots
            .get(5)
            .map(|s| s.raw.to_ascii_lowercase())
            .filter(|s| s.ends_with("_rmaos.dds") && exists(s));
        match rmaos {
            Some(_) => ALREADY_PBR.to_string(),
            None => NO_MATCH.to_string(),
        }
    });
    (source.clone(), vec![line])
}

/// The JSON In game reads (fix F2): read on a worker once per scan target and scan, kept across switches.
pub struct PgJson {
    key: (Option<ScanTarget>, u64),
    /// Which read this is (`MeshesModel::pg_reads` when it started): part of every cached row's key.
    pub gen: u64,
    pub state: PgState,
}

pub enum PgState {
    Reading(Receiver<Result<Vec<PgFile>, String>>),
    Done(Result<Vec<PgFile>, String>),
}

impl MeshesModel {
    /// The PGPatcher JSON read is running ("Reading PGPatcher JSON…").
    pub fn pg_reading(&self) -> bool {
        matches!(
            self.pg.as_ref().map(|p| &p.state),
            Some(PgState::Reading(_))
        )
    }

    /// Why the JSON could not be read (the instance or its mod list), if it could not.
    pub fn pg_error(&self) -> Option<&str> {
        match self.pg.as_ref().map(|p| &p.state) {
            Some(PgState::Done(Err(e))) => Some(e),
            _ => None,
        }
    }

    /// Starts the read on a worker unless the JSON on hand (or being read) is for this scan target and scan.
    fn ensure_pg(&mut self) {
        let key = (self.target.clone(), self.scan_rev);
        if self.pg.as_ref().is_some_and(|p| p.key == key) {
            return;
        }
        self.pg_reads += 1;
        let (tx, rx) = channel();
        let target = key.0.clone();
        std::thread::spawn(move || {
            let _ = tx.send(read_pg(target.as_ref()));
        });
        self.pg = Some(PgJson {
            key,
            gen: self.pg_reads,
            state: PgState::Reading(rx),
        });
    }

    /// Takes a finished read; In game then shows its rows. True when one landed.
    pub(super) fn poll_pg(&mut self) -> bool {
        let Some(p) = &mut self.pg else { return false };
        let PgState::Reading(rx) = &p.state else {
            return false;
        };
        p.state = PgState::Done(match rx.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => Err("the read stopped".into()),
        });
        if let Some(e) = self.edit.as_ref().filter(|e| e.in_game.is_some()) {
            let before = e.materials();
            self.edit_changed(before);
        }
        true
    }

    /// v2 B8: `edit.in_game` for the side shown: computed while In game shows, None on Edited source. Until the JSON
    /// lands (or when it could not be read) each shape keeps its edited material with no line. Per change, never per
    /// frame (the clock is paused In game, F21); unchanged shapes reuse their cached row (fix F1).
    pub(super) fn refresh_in_game(&mut self) {
        let show = !self.view_source && self.edit.as_ref().is_some_and(|e| !e.le_mesh);
        if show {
            self.ensure_pg();
        }
        let (store, rev) = (&self.store, self.scan_rev);
        let Some(e) = self.edit.as_mut() else { return };
        if !show {
            e.in_game = None;
            return;
        }
        // Unknown (a store error) counts as existing: an error must never show a false skip (as set_preview).
        let exists = |p: &str| {
            store
                .as_ref()
                .is_none_or(|s| !matches!(s.texture_location(&canon(p)), Ok(None)))
        };
        let rows = match &self.pg {
            Some(PgJson {
                gen,
                state: PgState::Done(Ok(files)),
                ..
            }) => e.in_game_materials(files, *gen, rev, &exists),
            _ => e.plain_rows(),
        };
        e.in_game = Some(rows);
    }
}
