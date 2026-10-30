//! Undo and redo over every edit (M3b spec §7.2, DECISIONS P9): a command keeps the text of `sdc.toml` and
//! `sdc-project.toml` before and after. Opening another folder clears it; Export and Save are not steps.
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub sdc_toml: String,
    pub project: String,
}

/// A file a command created (Duplicate source, spec §3.4): its path, its source, and its SHA-256 when created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedFile {
    pub path: PathBuf,
    pub source: PathBuf,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub label: String,
    pub before: Snapshot,
    pub after: Snapshot,
    /// Consecutive commands with the same `Some` key are one step (a slider drag, typing in one field).
    pub merge: Option<String>,
    pub created_file: Option<CreatedFile>,
}

#[derive(Debug, Default)]
pub struct History {
    pub undo: Vec<Command>,
    pub redo: Vec<Command>,
    /// Note FC3: every `push` counts here, merges included: a row that offers `Undo` remembers the count its step
    /// left, and offers it only while nothing was pushed since.
    pub pushes: u64,
}

impl History {
    pub const LIMIT: usize = 200;

    pub fn push(&mut self, c: Command) {
        self.pushes += 1;
        self.redo.clear();
        if let (Some(last), Some(k)) = (self.undo.last_mut(), c.merge.as_ref()) {
            if last.merge.as_ref() == Some(k)
                && last.created_file.is_none()
                && c.created_file.is_none()
            {
                last.after = c.after;
                return;
            }
        }
        self.undo.push(c);
        if self.undo.len() > Self::LIMIT {
            self.undo.remove(0);
        }
    }

    /// The command to undo (moved to the redo list).
    pub fn undo_step(&mut self) -> Option<Command> {
        let c = self.undo.pop()?;
        // audit 10: a merge only continues an uninterrupted run.
        if let Some(top) = self.undo.last_mut() {
            top.merge = None;
        }
        self.redo.push(c.clone());
        Some(c)
    }

    /// The command to redo (moved back to the undo list).
    pub fn redo_step(&mut self) -> Option<Command> {
        let c = self.redo.pop()?;
        self.undo.push(Command {
            merge: None,
            ..c.clone()
        });
        Some(c)
    }

    /// Put back a step whose undo or redo was refused.
    pub fn refuse_undo(&mut self) {
        if let Some(c) = self.redo.pop() {
            self.undo.push(c);
        }
    }
    pub fn refuse_redo(&mut self) {
        if let Some(c) = self.undo.pop() {
            self.redo.push(c);
        }
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

pub fn sha256_of(p: &std::path::Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    Some(hex::encode(Sha256::digest(std::fs::read(p).ok()?)))
}

/// The key of a hash record for an exported file: its absolute path, `/`-separated and lower-cased.
pub fn record_key(p: &std::path::Path) -> String {
    std::path::absolute(p)
        .unwrap_or_else(|_| p.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/")
        .to_lowercase()
}

/// Audit 08c-1: a hash record map with every key a `record_key`. A key from before (a path relative to the export
/// folder) is read as a file under `root`.
pub fn records_at(
    shas: &std::collections::BTreeMap<String, String>,
    root: &std::path::Path,
) -> std::collections::BTreeMap<String, String> {
    shas.iter()
        .map(|(k, v)| {
            let p = std::path::Path::new(k);
            let k = if p.is_absolute() {
                k.clone()
            } else {
                record_key(&root.join(p))
            };
            (k, v.clone())
        })
        .collect()
}

/// The file on disk whose name equals `path`'s apart from case, when the case differs (Note O6: a case-only
/// rename). Records are lower-case (audit 08c-1), so the old spelling is read from the folder.
pub fn case_twin(path: &std::path::Path) -> Option<std::path::PathBuf> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    std::fs::read_dir(path.parent()?)
        .ok()?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .find(|n| *n != name && n.to_lowercase() == name.to_lowercase())
        .map(|n| path.with_file_name(n))
}

/// O6: a case-only rename drops sdc's own unchanged file under the old name: `path`'s case twin is removed only when
/// a record `last` exists and the twin still hashes to it. No record, or a twin that cannot be read, is not sdc's
/// file and stays (wave 2 #48).
pub fn remove_own_case_twin(path: &std::path::Path, last: Option<&str>) {
    if let (Some(old), Some(last)) = (case_twin(path), last) {
        if sha256_of(&old).as_deref() == Some(last) {
            let _ = std::fs::remove_file(old);
        }
    }
}

/// The recorded files under `root` this run did not write (`this_run`, relative to `root`) that are still on disk,
/// spelled as on disk. They are reported, never deleted, and their records stay.
pub fn orphans<'a>(
    shas: &std::collections::BTreeMap<String, String>,
    root: &std::path::Path,
    this_run: impl Iterator<Item = &'a String>,
) -> Vec<std::path::PathBuf> {
    let prefix = format!("{}/", record_key(root));
    let mine: std::collections::BTreeSet<String> =
        this_run.map(|r| record_key(&root.join(r))).collect();
    shas.keys()
        .filter(|k| !mine.contains(*k))
        .filter_map(|k| k.strip_prefix(&prefix))
        .map(|rest| root.join(rest))
        .filter(|p| p.is_file())
        .map(|p| case_twin(&p).unwrap_or(p))
        .collect()
}

/// `ours` for `engine::dds::may_replace`: the bytes on disk are the ones whose hash is recorded (`last`).
pub fn recorded(last: Option<&str>) -> impl FnOnce(&[u8]) -> bool + '_ {
    use sha2::{Digest, Sha256};
    move |b| last == Some(hex::encode(Sha256::digest(b)).as_str())
}
