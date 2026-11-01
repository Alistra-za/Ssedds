//! The centre panel for two or more textures or sets (M6 Track F3a, design §3-4). Drawing only: every edit is a
//! model call, made after drawing.
use super::columns::AMBER;
use super::settings_panel::{combo, editor, Options, RowEdit, GROUPS};
use crate::model::bulk::{BulkField, BulkOp};
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
    Reencode(Vec<String>),
}

/// Note FI3: re-encode the selection's skipped DDS files (the S6 `reencode` selector, a plan-time field, so not a
/// `FIELDS` row). Files another input already writes are left out, with their names on hover.
fn reencode_row(ui: &mut egui::Ui, model: &AppModel, keys: &[String], acts: &mut Vec<Act>) {
    let (ok, blocked) = model.reencode_split(keys);
    let (k, j) = (ok.len(), blocked.len());
    if k == 0 && j == 0 {
        return;
    }
    let read_only = model.toml_read_only_reason();
    let left_out = format!("{j} left out: another file already writes them");
    ui.horizontal(|ui| {
        let (text, off) = if k == 0 {
            ("Re-encode DDS files".to_string(), Some(left_out.clone()))
        } else {
            let s = if k == 1 { "" } else { "s" };
            (format!("Re-encode {k} DDS file{s}"), read_only)
        };
        let r = ui
            .add_enabled(off.is_none(), egui::Button::new(text))
            .on_hover_text(
                "Decode them and encode them again with this project's settings. Block-compressed files lose some quality.",
            );
        let r = match &off {
            Some(why) => r.on_disabled_hover_text(why),
            None => r,
        };
        if r.clicked() {
            acts.push(Act::Reencode(ok.clone()));
        }
        if k > 0 && j > 0 {
            ui.weak(format!("({left_out})"))
                .on_hover_text(blocked.join("\n"));
        }
    });
}

pub fn draw(ui: &mut egui::Ui, model: &mut AppModel, to_top: bool) {
    let mut acts = vec![];
    // Chips and rows come from one cached view (F3a: nothing recomputed per frame).
    let view = model.bulk_view();
    let sets = model.selected_sets();
    if sets.len() >= 2 {
        ui.heading(format!("{} sets selected", sets.len()));
        let chips = &view.chips;
        ui.horizontal_wrapped(|ui| {
            for c in chips {
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
    let keys = &view.keys;
    ui.heading(format!("{} textures selected", keys.len()));
    if let Some(r) = &model.bulk.report {
        ui.weak(r);
    }
    reencode_row(ui, model, keys, &mut acts);
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
            let rows = &view.rows;
            // The Format row offers the pairs every selected texture allows (`bulk_encodings`).
            let formats: Options = view
                .encodings
                .iter()
                .map(|(e, l)| (serde_json::json!(e), l.clone()))
                .collect();
            // Slider state is per selection, not shared by every bulk selection.
            let id = format!(
                "bulk-{}-{}",
                keys.len(),
                keys.first().map_or("", String::as_str)
            );
            let mut area = egui::ScrollArea::vertical()
                .id_salt("centre-bulk")
                .auto_shrink([false, false]);
            if to_top {
                area = area.vertical_scroll_offset(0.0);
            }
            area.show(ui, |ui| {
                ui.add_enabled_ui(read_only.is_none(), |ui| {
                    for g in GROUPS {
                        egui::CollapsingHeader::new(g)
                            .id_salt(("bulk", g))
                            .default_open(true)
                            .show(ui, |ui| {
                                egui::Grid::new(("bulk-grid", g))
                                    .num_columns(4)
                                    .show(ui, |ui| {
                                        for r in rows.iter().filter(|r| group_of(r.field) == g) {
                                            if let Some(e) =
                                                row(ui, &id, &job, target, busy, r, &formats)
                                            {
                                                acts.push(Act::Edit(r.field, e));
                                            }
                                        }
                                    });
                            });
                    }
                });
            });
        }
    }
    for a in acts {
        match a {
            Act::Kind(k) => model.bulk.kind = k,
            Act::Reencode(ks) => model.set_reencode(&ks, true),
            Act::Edit(f, RowEdit::Set(v)) => model.apply_bulk_edit(keys, f, BulkOp::Set(v)),
            Act::Edit(f, RowEdit::Clear) => model.apply_bulk_edit(keys, f, BulkOp::Clear),
        }
    }
}

/// One bulk row: the label; the editor (its mixed state, disabled when the field applies to none); the note, with
/// the skip reasons on hover; and a reset when some texture sets the field itself.
fn row(
    ui: &mut egui::Ui,
    id: &str,
    job: &ResolvedJob,
    target: Target,
    busy: bool,
    r: &BulkField,
    formats: &Options,
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
            let mixed = r.value.is_none() && r.applies > 0;
            if r.field == "format" {
                return combo(
                    ui,
                    egui::Id::new((id, r.field)),
                    &view.value,
                    formats,
                    mixed,
                );
            }
            editor(ui, id, target, job, busy, &view, mixed)
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
