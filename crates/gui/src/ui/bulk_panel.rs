//! The centre panel for two or more textures or sets (M6 Track F3a, design §3-4). Drawing only: every edit is a
//! model call, made after drawing.
use super::columns::AMBER;
use super::settings_panel::{editor, RowEdit, GROUPS};
use crate::model::bulk::{bulk_fields, kind_chips, BulkField, BulkOp};
use crate::model::fields::{group_of, FieldView};
use crate::model::sets::SlotKind;
use crate::model::{AppModel, Selection};
use eframe::egui;
use engine::config::Target;
use engine::policy::ResolvedJob;
use serde_json::Value;

enum Act {
    Kind(Option<SlotKind>),
    Edit(&'static str, RowEdit),
}

pub fn draw(ui: &mut egui::Ui, model: &mut AppModel) {
    let mut acts = vec![];
    let sets = model.selected_sets();
    if sets.len() >= 2 {
        ui.heading(format!("{} sets selected", sets.len()));
        let chips = kind_chips(model, &sets);
        ui.horizontal_wrapped(|ui| {
            for c in &chips {
                let on = model.bulk.kind == c.kind;
                if ui
                    .selectable_label(on, format!("{} ({})", c.label, c.members.len()))
                    .clicked()
                {
                    acts.push(Act::Kind(c.kind));
                }
            }
        });
        if let Some(c) = chips
            .iter()
            .find(|c| c.kind == model.bulk.kind && c.sets_with < sets.len())
        {
            ui.weak(format!("applies to {} of {} sets", c.sets_with, sets.len()));
        }
    }
    let keys = model.bulk_targets();
    ui.heading(format!("{} textures selected", keys.len()));
    if let Some(r) = &model.bulk.report {
        ui.weak(r);
    }
    // Options come from the active texture's job, else the first target's (design §3.2).
    let active = match &model.selection {
        Selection::Item(k) => model.item(k).and_then(|i| i.job.clone()),
        _ => None,
    };
    let job = active.or_else(|| {
        keys.iter()
            .find_map(|k| model.item(k).and_then(|i| i.job.clone()))
    });
    match job {
        None => {
            ui.weak("None of the selected textures has settings of its own.");
        }
        Some(job) => {
            let read_only = model.toml_read_only_reason();
            if let Some(why) = &read_only {
                ui.colored_label(AMBER, why);
            }
            let (target, busy) = (model.target, model.busy());
            let rows = bulk_fields(model, &keys);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_enabled_ui(read_only.is_none(), |ui| {
                        for g in GROUPS {
                            egui::CollapsingHeader::new(g)
                                .id_salt(("bulk", g))
                                .default_open(true)
                                .show(ui, |ui| {
                                    egui::Grid::new(("bulk-grid", g)).num_columns(4).show(
                                        ui,
                                        |ui| {
                                            for r in rows.iter().filter(|r| group_of(r.field) == g)
                                            {
                                                if let Some(e) = row(ui, &job, target, busy, r) {
                                                    acts.push(Act::Edit(r.field, e));
                                                }
                                            }
                                        },
                                    );
                                });
                        }
                    });
                });
        }
    }
    for a in acts {
        match a {
            Act::Kind(k) => model.bulk.kind = k,
            Act::Edit(f, RowEdit::Set(v)) => model.apply_bulk_edit(&keys, f, BulkOp::Set(v)),
            Act::Edit(f, RowEdit::Clear) => model.apply_bulk_edit(&keys, f, BulkOp::Clear),
        }
    }
}

/// One bulk row: the label; the editor (its mixed state, disabled when the field applies to none); the note, with
/// the skip reasons on hover; and a reset when some texture sets the field itself.
fn row(
    ui: &mut egui::Ui,
    job: &ResolvedJob,
    target: Target,
    busy: bool,
    r: &BulkField,
) -> Option<RowEdit> {
    let mut edit = None;
    ui.label(r.label);
    let view = FieldView {
        field: r.field,
        label: r.label,
        value: r.value.clone().unwrap_or(Value::Null),
        provenance: String::new(),
        edited: r.edited > 0,
        lock: None,
    };
    let (set, _) = ui
        .add_enabled_ui(r.applies > 0, |ui| {
            editor(ui, "bulk", target, job, busy, &view, r.value.is_none())
        })
        .inner;
    if let Some(v) = set {
        edit = Some(RowEdit::Set(v));
    }
    let note = ui.label(egui::RichText::new(r.note()).small().weak());
    if !r.skipped.is_empty() {
        note.on_hover_text(r.skipped_text());
    }
    if r.edited > 0
        && ui
            .small_button("↺")
            .on_hover_text("Use the inherited value on every selected texture")
            .clicked()
    {
        edit = Some(RowEdit::Clear);
    }
    ui.end_row();
    edit
}
