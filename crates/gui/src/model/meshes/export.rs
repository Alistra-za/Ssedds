//! Export of a NIF edit into `sdc NIF Edits` (spec §8, plan A19): the MO2 placement check, the guard outcomes and
//! the per-NIF badges read from the edit documents. No egui types. The export routine is `MeshesModel::export_edit`.
use super::edit_doc::{self, EditDoc, EDIT_MOD};
use modscan::store::{ModRow, PathCopy, KIND_BSA, KIND_MOD};
use std::path::Path;

/// Where the edit mod sits in MO2's priority order, relative to the NIF's other copies (spec §8.2).
#[derive(Debug, Clone, PartialEq)]
pub enum Placement {
    Ok,
    NotEnabled,
    NotScanned,
    /// A source mod with higher priority than the edit mod.
    Below(String),
    /// PGPatcher output with lower priority than the edit mod.
    AbovePg(String),
}

impl Placement {
    pub fn text(&self) -> String {
        match self {
            Placement::Ok => "Placement OK.".into(),
            Placement::NotEnabled => {
                format!("The edit mod is not enabled: enable '{EDIT_MOD}' in MO2.")
            }
            Placement::NotScanned => format!("Rescan to check where '{EDIT_MOD}' sits."),
            Placement::Below(x) => format!(
                "The edit mod is below {x}: your edit loses. Drag '{EDIT_MOD}' below {x} in MO2."
            ),
            Placement::AbovePg(x) => format!(
                "The edit mod is above {x}: PGPatcher output will not include your edit until you drag \
                 '{EDIT_MOD}' above {x} in MO2."
            ),
        }
    }
}

/// The edit mod must beat every non-derived loose copy and lose to every derived one (BSA copies always lose to
/// loose files; the edit mod's own copy is skipped). `NotEnabled` when the edit mod has no row in `mods`; the caller
/// tells it apart from `NotScanned` by the modlist.
pub fn placement(copies: &[PathCopy], mods: &[ModRow], edit_mod_name: &str) -> Placement {
    let Some(edit) = mods
        .iter()
        .find(|m| m.kind == KIND_MOD && m.name.eq_ignore_ascii_case(edit_mod_name))
    else {
        return Placement::NotEnabled;
    };
    let others = || {
        copies
            .iter()
            .filter(|c| c.kind != KIND_BSA && !c.origin.eq_ignore_ascii_case(edit_mod_name))
    };
    // MO2 lists higher priority lower down: drag below the highest source, above the lowest PGPatcher output.
    if let Some(c) = others()
        .filter(|c| !c.derived && c.priority > edit.priority)
        .max_by_key(|c| c.priority)
    {
        return Placement::Below(c.origin.clone());
    }
    if let Some(c) = others()
        .filter(|c| c.derived && c.priority < edit.priority)
        .min_by_key(|c| c.priority)
    {
        return Placement::AbovePg(c.origin.clone());
    }
    Placement::Ok
}

/// The answer to the guard dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardChoice {
    Overwrite,
    KeepTheirs,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExportOutcome {
    Written {
        rerun_pgpatcher: bool,
    },
    /// The file on disk is not the one sdc last wrote: ask (the block-level diff, on disk → ours).
    Guard {
        diff: Vec<String>,
    },
    Blocked(String),
    KeptTheirs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocBadge {
    Pending,
    Exported,
    Conflict,
}

impl DocBadge {
    pub fn of(doc: &EditDoc) -> Option<DocBadge> {
        if doc.has_conflicts() {
            Some(DocBadge::Conflict)
        } else if doc.pending() {
            Some(DocBadge::Pending)
        } else if doc.export.sha256.is_some() {
            Some(DocBadge::Exported)
        } else {
            None
        }
    }

    pub fn chip(self) -> &'static str {
        match self {
            DocBadge::Pending => "Edit pending",
            DocBadge::Exported => "Edited",
            DocBadge::Conflict => "Edit conflict",
        }
    }
}

/// Each edit document under `<edit_mod>\.sdc\nif-edits` with a badge, keyed by its source data path (the NIF list's
/// path). A document that does not load is skipped.
pub fn pending_docs(edit_mod: &Path) -> Vec<(String, DocBadge)> {
    let mut out = vec![];
    let mut dirs = vec![edit_mod.join(".sdc").join("nif-edits")];
    while let Some(d) = dirs.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        // `file_type` does not follow links: a link or junction (a loop, or a path out of the mod) is skipped.
        for (p, t) in rd
            .flatten()
            .filter_map(|e| Some((e.path(), e.file_type().ok()?)))
        {
            if t.is_dir() {
                dirs.push(p);
            } else if t.is_file() && p.to_string_lossy().ends_with(".sdc-nifedit.toml") {
                if let Ok(Some(doc)) = edit_doc::load(&p) {
                    if let Some(b) = DocBadge::of(&doc) {
                        out.push((doc.source.data_path.clone(), b));
                    }
                }
            }
        }
    }
    out
}

/// `bytes` read back as a NIF (`scan` checks the file length too).
pub fn read_back(bytes: &[u8]) -> Result<(), String> {
    nif::scan::scan(bytes)
        .map(|_| ())
        .map_err(|e| format!("The written file would not read back: {e}"))
}
