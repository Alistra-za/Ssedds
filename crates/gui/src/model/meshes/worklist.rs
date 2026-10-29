//! The open NIF edit's entries in the one worklist (Note GD1): every warning and blocker of the edit, one row each,
//! with the tab and field where its fix lives. The app hands them to the Problems list; no egui types.
use super::nif_edit::{op_label, NifEdit, NO_EDITS, NO_EXPORT_FOLDER};
use nif::edit::validate::{new_errors, Level, Target, Warning};
use nif::edit::{Op, Outcome};
use nif::fields::Field;
use nif::model::NifModel;

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
/// no-folder reasons (the edit's own state, said elsewhere). Each warning once, at its highest level.
pub(super) fn problems(e: &NifEdit) -> Vec<NifProblem> {
    let mut out: Vec<NifProblem> = vec![];
    let mut push = |level, text: String, (tab, field): (NifTab, Option<Field>)| {
        if !out
            .iter()
            .any(|p| p.text == text && p.tab == tab && p.field == field)
        {
            out.push(NifProblem {
                level,
                path: e.data_path.clone(),
                text,
                tab,
                field,
            });
        }
    };
    let m: &NifModel = &e.model;
    for (d, o) in e.doc.ops.iter().zip(&e.outcomes) {
        if let Outcome::Conflict(why) | Outcome::Refused(why) = o {
            let text = format!("{}: {why}", op_label(&d.op, m));
            push(NifLevel::Blocks, text, op_tab(&d.op));
        }
    }
    let blocking = new_errors(&e.source_warnings, &e.warnings);
    let mut warn = |w: &Warning, blocks: bool| {
        let level = if blocks {
            NifLevel::Blocks
        } else {
            NifLevel::Warning
        };
        push(level, w.text.clone(), target_tab(&w.target));
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
    // A reason `export_block` found that no row above covers (bytes that do not read back).
    if let Some(why) = e
        .blocked
        .as_ref()
        .filter(|b| *b != NO_EDITS && *b != NO_EXPORT_FOLDER)
    {
        if !out.iter().any(|p| p.level == NifLevel::Blocks) {
            out.insert(
                0,
                NifProblem {
                    level: NifLevel::Blocks,
                    path: e.data_path.clone(),
                    text: why.clone(),
                    tab: NifTab::Shader,
                    field: None,
                },
            );
        }
    }
    out
}
