//! The NIF edit document (spec §5, plan A15): the op list that rides with one source NIF, stored as TOML next to the
//! exported file, plus the in-memory undo stack the editor uses. No egui types.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// The mod NIF edits went to without an export folder until Note GD5 (CI3's default); nothing uses it as a default now.
pub const EDIT_MOD: &str = "sdc NIF Edits";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceId {
    pub origin: String,
    pub data_path: String,
    pub archive: Option<String>,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ExportRec {
    pub sha256: Option<String>,
    pub at: Option<u64>,
    pub ops_hash: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpStatus {
    Applied,
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocOp {
    pub op: nif::edit::Op,
    pub status: OpStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditDoc {
    pub version: u32,
    pub source: SourceId,
    #[serde(default)]
    pub export: ExportRec,
    pub ops: Vec<DocOp>,
}

impl EditDoc {
    pub fn new(source: SourceId) -> EditDoc {
        EditDoc {
            version: 1,
            source,
            export: ExportRec::default(),
            ops: Vec::new(),
        }
    }

    pub fn to_text(&self) -> String {
        toml::to_string_pretty(self).expect("edit doc serialises")
    }

    pub fn from_text(t: &str) -> Result<EditDoc, String> {
        let d: EditDoc = toml::from_str(t).map_err(|e| e.to_string())?;
        if d.version > 1 {
            return Err(format!(
                "This edit file was written by a newer sdc (version {}).",
                d.version
            ));
        }
        Ok(d)
    }

    pub fn ops(&self) -> Vec<nif::edit::Op> {
        self.ops.iter().map(|d| d.op.clone()).collect()
    }

    /// SHA-256 hex of the TOML of the ops alone (a top-level table needs a wrapper key).
    pub fn ops_hash(&self) -> String {
        #[derive(Serialize)]
        struct Ops<'a> {
            ops: &'a [DocOp],
        }
        HASHES.with(|n| n.set(n.get() + 1));
        let t = toml::to_string_pretty(&Ops { ops: &self.ops }).expect("ops serialise");
        sha256_hex(t.as_bytes())
    }

    /// The ops differ from the last export; a document never exported is pending once it has ops (an undo back to
    /// no ops after an export is pending: the exported file still has the edits).
    pub fn pending(&self) -> bool {
        match &self.export.ops_hash {
            None => !self.ops.is_empty(),
            Some(h) => *h != self.ops_hash(),
        }
    }

    pub fn has_conflicts(&self) -> bool {
        self.ops.iter().any(|d| d.status == OpStatus::Conflict)
    }
}

/// `data_path` with `\` separators, or Err unless every component is a plain name: a crafted BSA folder such as
/// `..\x` or `C:\x` must not lead a write out of the edit mod (Windows drops trailing dots and spaces, so `. .` too).
pub fn win(data_path: &str) -> Result<String, String> {
    let w = data_path.replace('/', "\\");
    let bad = |s: &str| s.trim_end_matches(['.', ' ']).is_empty() || s.contains(':');
    match w.split('\\').any(bad) {
        true => Err(format!(
            "This NIF's path cannot be used for an edit: {data_path}"
        )),
        false => Ok(w),
    }
}

pub fn doc_path(edit_mod: &Path, data_path: &str) -> Result<PathBuf, String> {
    Ok(edit_mod
        .join(".sdc")
        .join("nif-edits")
        .join(format!("{}.sdc-nifedit.toml", win(data_path)?)))
}

/// Where an edit without an export folder autosaves (Note GD5 (c)): `<dir>\nif-edits\<SHA-256 of the lowercase data
/// path>.toml`; `dir` is the scan store's folder, `%LOCALAPPDATA%\sdc` in the app.
pub fn autosave_path(dir: &Path, data_path: &str) -> Result<PathBuf, String> {
    let key = win(data_path)?.to_ascii_lowercase();
    Ok(dir
        .join("nif-edits")
        .join(format!("{}.toml", sha256_hex(key.as_bytes()))))
}

pub fn export_path(edit_mod: &Path, data_path: &str) -> Result<PathBuf, String> {
    Ok(edit_mod.join(win(data_path)?))
}

pub fn save(path: &Path, doc: &EditDoc) -> Result<(), String> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    engine::dds::write_atomic(path, doc.to_text().as_bytes())
        .map_err(|e| format!("{}: {e}", path.display()))
}

thread_local! {
    static LOADS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static HASHES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many `EditDoc::ops_hash` this thread computed (Note GD23's test: idle frames hash nothing).
pub fn ops_hash_count() -> usize {
    HASHES.with(std::cell::Cell::get)
}

/// How many `load` calls this thread made (Note GD20's test: a failed folder change is not read each frame).
pub fn load_count() -> usize {
    LOADS.with(std::cell::Cell::get)
}

pub fn load(path: &Path) -> Result<Option<EditDoc>, String> {
    LOADS.with(|n| n.set(n.get() + 1));
    match std::fs::read_to_string(path) {
        Ok(t) => EditDoc::from_text(&t).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

pub fn sha256_hex(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}

/// Undo/redo over whole-document snapshots (TOML text), each with the short name of the step it undoes (Note GD4).
/// A record with the same merge key as the previous one (one drag) keeps only the first snapshot and its name.
#[derive(Debug, Default)]
pub struct NifHistory {
    undo: Vec<(String, String)>,
    redo: Vec<(String, String)>,
    merge: Option<String>,
}

impl NifHistory {
    pub const LIMIT: usize = 200;

    /// `before` is the document before the step named `label` ("Set shader type").
    pub fn record(&mut self, before: String, label: &str, merge: Option<String>) {
        self.redo.clear();
        if merge.is_some() && merge == self.merge {
            return;
        }
        self.merge = merge;
        self.undo.push((before, label.to_string()));
        if self.undo.len() > Self::LIMIT {
            self.undo.remove(0);
        }
    }

    pub fn break_merge(&mut self) {
        self.merge = None;
    }

    pub fn undo(&mut self, current: String) -> Option<String> {
        let (s, label) = self.undo.pop()?;
        self.redo.push((current, label));
        self.merge = None;
        Some(s)
    }

    pub fn redo(&mut self, current: String) -> Option<String> {
        let (s, label) = self.redo.pop()?;
        self.undo.push((current, label));
        self.merge = None;
        Some(s)
    }

    /// The name of the step Undo would undo.
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|(_, l)| l.as_str())
    }

    /// The name of the step Redo would redo.
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|(_, l)| l.as_str())
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}
