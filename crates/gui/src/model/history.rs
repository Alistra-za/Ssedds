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
}

impl History {
    pub const LIMIT: usize = 200;

    pub fn push(&mut self, c: Command) {
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
        self.redo.push(c.clone());
        Some(c)
    }

    /// The command to redo (moved back to the undo list).
    pub fn redo_step(&mut self) -> Option<Command> {
        let c = self.redo.pop()?;
        self.undo.push(c.clone());
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
