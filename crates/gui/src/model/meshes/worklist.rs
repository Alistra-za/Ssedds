//! The open NIF edit's entries in the one worklist (Note GD1): every warning and blocker of the edit, one row each,
//! with the tab and field where its fix lives. The app hands them to the Problems list; no egui types.
use super::nif_edit::{NifEdit, NO_CLEANUP, RERUN_PGPATCHER};
use nif::edit::validate::{new_errors, Level, Target, Warning};
use nif::edit::{Op, Outcome};
use nif::fields::Field;

/// How a NIF row weighs: `Blocks` stops that NIF's export, `Info` is a note (hover and Messages only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NifLevel {
    Blocks,
    Warning,
    Info,
}

/// The edit panel's four tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NifTab {
    Shader,
    Alpha,
    Controllers,
    Extra,
}

impl NifTab {
    /// The tab's place in the panel's strip.
    pub fn index(self) -> usize {
        self as usize
    }
}

/// One worklist row of a NIF edit. `path` is the NIF's data-relative path; `field` is None where the fix is not one
/// field (a texture slot, a whole controller, an extra data block).
#[derive(Debug, Clone, PartialEq)]
pub struct NifProblem {
    pub level: NifLevel,
    pub path: String,
    pub text: String,
    pub tab: NifTab,
    pub field: Option<Field>,
}

fn field_tab(f: Field) -> NifTab {
    match f {
        Field::AlphaFlags | Field::AlphaThreshold => NifTab::Alpha,
        Field::CtrlFlags
        | Field::Frequency
        | Field::Phase
        | Field::StartTime
        | Field::StopTime
        | Field::Variable
        | Field::InterpValue(_) => NifTab::Controllers,
        _ => NifTab::Shader,
    }
}

fn target_tab(t: &Target) -> (NifTab, Option<Field>) {
    match t {
        Target::Field { field, .. } => (field_tab(*field), Some(*field)),
        Target::Slot { .. } => (NifTab::Shader, None),
        Target::Extra { .. } => (NifTab::Extra, None),
        // controllers, managers and the clip ops' root
        Target::Block { .. } => (NifTab::Controllers, None),
    }
}

fn op_tab(op: &Op) -> (NifTab, Option<Field>) {
    match op {
        Op::SetField { field, .. } | Op::SetBits { field, .. } => (field_tab(*field), Some(*field)),
        Op::SetTexturePath { .. } => (NifTab::Shader, None),
        Op::SplitAlpha { .. } | Op::AddAlpha { .. } | Op::RemoveAlpha { .. } => {
            (NifTab::Alpha, None)
        }
        Op::AddExtra { .. } | Op::EditExtra { .. } | Op::RemoveExtra { .. } => {
            (NifTab::Extra, None)
        }
        _ => (NifTab::Controllers, None),
    }
}

/// The rows of `e`, in the order `export_block` checks them: what blocks first, then the warnings. Not the idle or
/// no-folder reasons (the edit's own state, said elsewhere). Each warning once per target, at its highest level
/// (Note GD15: the same text on two targets, two texture sets say, is two rows).
pub(super) fn problems(e: &NifEdit) -> Vec<NifProblem> {
    let mut out: Vec<NifProblem> = vec![];
    // shortcut: the rows of one text on two targets read the same (NifProblem has no target); add the shape's name
    // to the text if users cannot tell them apart.
    let mut seen: Vec<(String, NifTab, Option<Field>, Option<Target>)> = vec![];
    let mut push =
        |level, text: String, (tab, field): (NifTab, Option<Field>), target: Option<&Target>| {
            let key = (text, tab, field, target.cloned());
            if !seen.contains(&key) {
                let text = key.0.clone();
                seen.push(key);
                out.push(NifProblem {
                    level,
                    path: e.data_path.clone(),
                    text,
                    tab,
                    field,
                });
            }
        };
    for (i, (d, o)) in e.doc.ops.iter().zip(&e.outcomes).enumerate() {
        if let Outcome::Conflict(why) | Outcome::Refused(why) = o {
            let text = format!("{}: {why}", e.op_label_at(i));
            push(NifLevel::Blocks, text, op_tab(&d.op), None);
        }
    }
    let blocking = new_errors(&e.source_warnings, &e.warnings);
    let mut warn = |w: &Warning, blocks: bool| {
        let level = if blocks {
            NifLevel::Blocks
        } else {
            NifLevel::Warning
        };
        push(
            level,
            w.text.clone(),
            target_tab(&w.target),
            Some(&w.target),
        );
    };
    for w in &blocking {
        warn(w, true);
    }
    for w in &e.op_warnings {
        warn(w, w.level == Level::Error);
    }
    for w in e.warnings.iter().chain(&e.pg_warnings) {
        warn(w, false);
    }
    // Note GD2: after a write PGPatcher's output is stale (was the footer's note); the cleanup's skip in plain words
    if matches!(&e.export_note, Some((t, false)) if t == RERUN_PGPATCHER) {
        push(
            NifLevel::Warning,
            RERUN_PGPATCHER.into(),
            (NifTab::Shader, None),
            None,
        );
    }
    if e.cleanup_skip.is_some() {
        push(
            NifLevel::Info,
            NO_CLEANUP.into(),
            (NifTab::Controllers, None),
            None,
        );
    }
    // Note GD33: a failed folder change, an apply error or bytes that do not read back (no row above says them); first,
    // as `export_block` checks them first. Another blocking row does not hide it (GD30).
    if let Some(why) = e.uncovered_block() {
        out.insert(
            0,
            NifProblem {
                level: NifLevel::Blocks,
                path: e.data_path.clone(),
                text: why,
                tab: NifTab::Shader,
                field: None,
            },
        );
    }
    out
}
