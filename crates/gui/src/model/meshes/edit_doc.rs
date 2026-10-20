//! The NIF edit document (spec §5, plan A15): the op list that rides with one source NIF, stored as TOML next to the
//! exported file, plus the in-memory undo stack the editor uses. No egui types.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Name of the mod that receives exported edits.
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
    #[serde(default)]
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
        toml::from_str(t).map_err(|e| e.to_string())
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
        let t = toml::to_string_pretty(&Ops { ops: &self.ops }).expect("ops serialise");
        sha256_hex(t.as_bytes())
    }

    pub fn pending(&self) -> bool {
        !self.ops.is_empty() && self.export.ops_hash.as_deref() != Some(self.ops_hash().as_str())
    }

    pub fn has_conflicts(&self) -> bool {
        self.ops.iter().any(|d| d.status == OpStatus::Conflict)
    }
}

fn win(data_path: &str) -> String {
    data_path.replace('/', "\\")
}

pub fn doc_path(edit_mod: &Path, data_path: &str) -> PathBuf {
    edit_mod
        .join(".sdc")
        .join("nif-edits")
        .join(format!("{}.sdc-nifedit.toml", win(data_path)))
}

pub fn export_path(edit_mod: &Path, data_path: &str) -> PathBuf {
    edit_mod.join(win(data_path))
}

pub fn save(path: &Path, doc: &EditDoc) -> Result<(), String> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    engine::dds::write_atomic(path, doc.to_text().as_bytes())
        .map_err(|e| format!("{}: {e}", path.display()))
}

pub fn load(path: &Path) -> Result<Option<EditDoc>, String> {
    match std::fs::read_to_string(path) {
        Ok(t) => EditDoc::from_text(&t).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

pub fn sha256_hex(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}

/// Undo/redo over whole-document snapshots (TOML text). A record with the same merge key as the previous one
/// (one drag) keeps only the first snapshot.
#[derive(Debug, Default)]
pub struct NifHistory {
    undo: Vec<String>,
    redo: Vec<String>,
    merge: Option<String>,
}

impl NifHistory {
    pub const LIMIT: usize = 200;

    pub fn record(&mut self, before: String, merge: Option<String>) {
        self.redo.clear();
        if merge.is_some() && merge == self.merge {
            return;
        }
        self.merge = merge;
        self.undo.push(before);
        if self.undo.len() > Self::LIMIT {
            self.undo.remove(0);
        }
    }

    pub fn break_merge(&mut self) {
        self.merge = None;
    }

    pub fn undo(&mut self, current: String) -> Option<String> {
        let s = self.undo.pop()?;
        self.redo.push(current);
        self.merge = None;
        Some(s)
    }

    pub fn redo(&mut self, current: String) -> Option<String> {
        let s = self.redo.pop()?;
        self.undo.push(current);
        self.merge = None;
        Some(s)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}
