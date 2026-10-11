//! The right-hand Advanced drawer (M3a spec §2.5): the preset inspector, batch, legacy project-wide fills and preview settings,
//! the rarely changed texture fields, and the `[files]` entries that match nothing. Drawing only: every edit is a
//! call on the model, applied once the drawing is over.
use super::columns::{strip_button, AMBER, STRIP_WIDTH};
use super::settings_panel::{field_row, toml_read_only_reason, Ctx, RowEdit};
use crate::model::fields::{advanced_field_views, levels_view, provenance_text};
use crate::model::inspector::{inspector_rows, InspectorRow};
use crate::model::{AppModel, Selection};
use crate::settings::{Bc1Choice, PREVIEW_SIZES};
use eframe::egui;
use engine::policy::ResolvedJob;
use serde_json::Value;

#[derive(Debug, Default)]
pub struct AdvancedState {
    pub only_differences: bool,
}

/// What a control asks of the model.
enum Action {
    SetField(String, &'static str, Value),
    ClearField(String, &'static str),
    ClearLegacyFills,
    RemoveUnmatched(String),
    PreviewMax(u32),
    Allow16k(bool),
    Live(bool),
    Bc1(Bc1Choice),
}

/// The drawer: a 24 px "Advanced" strip while closed, the panel while `settings.advanced_open`.
pub fn draw(ui: &mut egui::Ui, model: &mut AppModel, state: &mut AdvancedState) {
    if !model.settings.advanced_open {
        egui::Panel::right("advanced-strip")
            .resizable(false)
            .exact_size(STRIP_WIDTH)
            .frame(egui::Frame::new())
            .show(ui, |ui| {
                if strip_button(ui, "Advanced")
                    .on_hover_text("Open the Advanced drawer")
                    .clicked()
                {
                    model.settings.advanced_open = true;
                    model.save_settings();
                }
            });
        return;
    }
    let mut actions = vec![];
    let mut close = false;
    let set_id = selected_set_id(model);
    egui::Panel::right("advanced")
        .default_size(380.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Advanced");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close = ui.button("Close").clicked();
                });
            });
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    inspector(ui, model, state);
                    if let Some(id) = &set_id {
                        egui::CollapsingHeader::new(
                            egui::RichText::new("PGPatcher (advanced)").strong(),
                        )
                        .default_open(true)
                        .show(ui, |ui| super::pgp_fields::advanced(ui, model, id));
                    }
                    batch(ui, model, &mut actions);
                    legacy_fills(ui, model, &mut actions);
                    preview_settings(ui, model, &mut actions);
                    mipmaps(ui, model, &mut actions);
                    texture_fields(ui, model, &mut actions);
                    unmatched(ui, model, &mut actions);
                });
        });
    if close {
        model.settings.advanced_open = false;
        model.save_settings();
    }
    for a in actions {
        match a {
            Action::SetField(k, f, v) => model.set_field(&k, f, v),
            Action::ClearField(k, f) => model.clear_field(&k, f),
            Action::ClearLegacyFills => model.clear_legacy_fills(),
            Action::RemoveUnmatched(k) => model.remove_unmatched(&k),
            Action::PreviewMax(n) => model.set_preview_max(n),
            Action::Allow16k(on) => model.set_allow_16k_preview(on),
            Action::Live(on) => model.set_live(on),
            Action::Bc1(c) => model.set_bc1_choice(c),
        }
    }
}

/// The id of the selected set, when a set is selected.
fn selected_set_id(model: &AppModel) -> Option<String> {
    let Selection::Set(key) = &model.selection else {
        return None;
    };
    crate::model::sets::set_views(model)
        .into_iter()
        .find(|v| &v.key == key)
        .map(|v| v.id)
}

/// The selected texture's key and resolved job, when it has one.
fn selected_job(model: &AppModel) -> Option<(String, ResolvedJob)> {
    let Selection::Item(key) = &model.selection else {
        return None;
    };
    let job = model.item(key)?.job.clone()?;
    Some((key.clone(), job))
}

fn value_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn section(ui: &mut egui::Ui, name: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::CollapsingHeader::new(egui::RichText::new(name).strong())
        .default_open(true)
        .show(ui, body);
}

fn inspector(ui: &mut egui::Ui, model: &AppModel, state: &mut AdvancedState) {
    section(ui, "Preset inspector", |ui| {
        ui.label(format!("Batch preset: {}", model.batch_preset));
        ui.label(
            egui::RichText::new(
                "Built-in presets are read-only here. Change a texture in the centre panel.",
            )
            .small()
            .weak(),
        );
        let (Some((_, job)), Some(session)) = (selected_job(model), model.session.as_ref()) else {
            ui.weak("Select a texture to see how its settings were resolved.");
            return;
        };
        ui.checkbox(&mut state.only_differences, "Show only differences");
        let rows: Vec<InspectorRow> = inspector_rows(&job, session.engine().registry())
            .into_iter()
            .filter(|r| !state.only_differences || r.differs_from_base)
            .collect();
        if rows.is_empty() {
            ui.weak("Nothing differs from the base preset.");
            return;
        }
        egui::Grid::new("inspector")
            .num_columns(3)
            .striped(true)
            .show(ui, |ui| {
                for r in rows {
                    let text = |s: String| {
                        if r.differs_from_base {
                            egui::RichText::new(s).strong()
                        } else {
                            egui::RichText::new(s)
                        }
                    };
                    ui.label(text(r.field.clone()));
                    ui.label(text(value_text(&r.value)));
                    ui.label(
                        egui::RichText::new(provenance_text(&r.provenance))
                            .small()
                            .weak(),
                    );
                    ui.end_row();
                }
            });
    });
}

fn bc1_label(c: Bc1Choice) -> &'static str {
    match c {
        Bc1Choice::Ideal => "Ideal",
        Bc1Choice::IdealRound4 => "Ideal (round 4)",
        Bc1Choice::Nvidia => "NVIDIA",
        Bc1Choice::Amd => "AMD",
    }
}

/// A number box; true once its value is settled (a drag ended, or a typed value was accepted).
pub(super) fn settled_drag<T: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    value: &mut T,
    speed: f64,
    range: std::ops::RangeInclusive<T>,
) -> bool {
    let r = ui.add(egui::DragValue::new(value).speed(speed).range(range));
    r.drag_stopped() || (r.changed() && !r.dragged())
}

fn batch(ui: &mut egui::Ui, model: &mut AppModel, actions: &mut Vec<Action>) {
    section(ui, "Batch", |ui| {
        let mut settled = false;
        egui::Grid::new("batch").num_columns(2).show(ui, |ui| {
            ui.label("Worker threads");
            settled |= settled_drag(ui, &mut model.settings.jobs, 0.2, 0..=256);
            ui.end_row();
            ui.label("Memory budget (MiB)");
            settled |= settled_drag(ui, &mut model.settings.mem_budget_mb, 16.0, 0..=1_048_576);
            ui.end_row();
            ui.label("BC1 decoder");
            let current = model.settings.bc1_decoder;
            egui::ComboBox::from_id_salt("bc1-decoder")
                .selected_text(bc1_label(current))
                .show_ui(ui, |ui| {
                    for c in Bc1Choice::ALL {
                        if ui.selectable_label(c == current, bc1_label(c)).clicked() && c != current
                        {
                            actions.push(Action::Bc1(c));
                        }
                    }
                });
            ui.end_row();
        });
        ui.label(
            egui::RichText::new("0 threads = one per core; 0 MiB = half of the physical memory.")
                .small()
                .weak(),
        );
        if model.restart_needed {
            ui.colored_label(AMBER, "restart sdc-gui to apply");
        }
        if settled {
            model.save_settings();
        }
    });
}

/// The project-wide fills an older `sdc.toml` may still hold (`defaults.packing.fill.*`). They still apply as a
/// fallback under each packed output's own fills; new fills are set on the packed output's card.
fn legacy_fills(ui: &mut egui::Ui, model: &AppModel, actions: &mut Vec<Action>) {
    let fills = model.legacy_fills();
    if fills.is_empty() {
        return;
    }
    let list: Vec<String> = fills
        .iter()
        .map(|(role, v)| format!("{} {v}", role.id()))
        .collect();
    ui.horizontal_wrapped(|ui| {
        ui.label(format!("Project-wide fills (legacy): {}", list.join(", ")));
        let why = toml_read_only_reason(model);
        let editable = model.project.as_ref().is_some_and(|p| !p.read_only());
        if ui
            .add_enabled(editable, egui::Button::new("Clear"))
            .on_hover_text(
                "Remove the project-wide fills; set fills on each packed output's card instead",
            )
            .on_disabled_hover_text(why.unwrap_or_default())
            .clicked()
        {
            actions.push(Action::ClearLegacyFills);
        }
    });
}

fn preview_settings(ui: &mut egui::Ui, model: &AppModel, actions: &mut Vec<Action>) {
    section(ui, "Preview", |ui| {
        let allow = model.settings.allow_16k_preview;
        let mut size = model.settings.preview_max;
        ui.horizontal(|ui| {
            ui.label("Largest previewed side");
            egui::ComboBox::from_id_salt("preview-size")
                .selected_text(format!("{} px", model.settings.preview_max))
                .show_ui(ui, |ui| {
                    for n in PREVIEW_SIZES.into_iter().filter(|n| *n < 16384 || allow) {
                        ui.selectable_value(&mut size, n, format!("{n} px"));
                    }
                });
        });
        if size != model.settings.preview_max {
            actions.push(Action::PreviewMax(size));
        }
        let mut allow_now = allow;
        if ui
            .checkbox(&mut allow_now, "Allow 16384 previews")
            .on_hover_text(
                "A 16384 px mip needs a very large GPU texture, so it is off unless you ask",
            )
            .changed()
        {
            actions.push(Action::Allow16k(allow_now));
        }
        let mut live = model.settings.live_encode;
        if ui
            .checkbox(&mut live, "Live encode")
            .on_hover_text(
                "Encode the expanded preview after every edit (otherwise use Encode now)",
            )
            .changed()
        {
            actions.push(Action::Live(live));
        }
    });
}

/// The mip level count (Full chain, None or 2..N levels); the basic panel only has the on/off tick box.
fn mipmaps(ui: &mut egui::Ui, model: &AppModel, actions: &mut Vec<Action>) {
    section(ui, "Mipmaps", |ui| {
        let Some((key, job)) = selected_job(model) else {
            ui.weak("Select a texture to edit its mip level count.");
            return;
        };
        let read_only = model.project.as_ref().is_none_or(|p| p.read_only());
        let why = toml_read_only_reason(model);
        let f = levels_view(model.target, &job);
        ui.add_enabled_ui(!read_only, |ui| {
            egui::Grid::new(("advanced-mips", &key))
                .num_columns(4)
                .show(ui, |ui| {
                    let cx = Ctx {
                        key: &key,
                        job: &job,
                        target: model.target,
                        busy: model.busy(),
                        compact: false,
                        toml_read_only: why.clone(),
                        warnings: &[],
                    };
                    match field_row(ui, &cx, &f) {
                        Some(RowEdit::Set(v)) => {
                            actions.push(Action::SetField(key.clone(), f.field, v))
                        }
                        Some(RowEdit::Clear) => {
                            actions.push(Action::ClearField(key.clone(), f.field))
                        }
                        None => {}
                    }
                });
        });
    });
}

fn texture_fields(ui: &mut egui::Ui, model: &AppModel, actions: &mut Vec<Action>) {
    section(ui, "Advanced texture fields", |ui| {
        let Some((key, job)) = selected_job(model) else {
            ui.weak("Select a texture to edit its encoder and header fields.");
            return;
        };
        let read_only = model.project.as_ref().is_none_or(|p| p.read_only());
        let busy = model.busy();
        let why = toml_read_only_reason(model);
        ui.add_enabled_ui(!read_only, |ui| {
            egui::Grid::new(("advanced-fields", &key))
                .num_columns(4)
                .show(ui, |ui| {
                    let cx = Ctx {
                        key: &key,
                        job: &job,
                        target: model.target,
                        busy,
                        compact: false,
                        toml_read_only: why.clone(),
                        warnings: &[],
                    };
                    for f in advanced_field_views(model.target, &job) {
                        match field_row(ui, &cx, &f) {
                            Some(RowEdit::Set(v)) => {
                                actions.push(Action::SetField(key.clone(), f.field, v))
                            }
                            Some(RowEdit::Clear) => {
                                actions.push(Action::ClearField(key.clone(), f.field))
                            }
                            None => {}
                        }
                    }
                });
        });
    });
}

fn unmatched(ui: &mut egui::Ui, model: &AppModel, actions: &mut Vec<Action>) {
    section(ui, "Unmatched [files] entries", |ui| {
        if model.unmatched_keys.is_empty() {
            ui.weak("Every [files] entry in sdc.toml matches a file.");
            return;
        }
        ui.label(
            egui::RichText::new(
                "These entries name no file under the current target and have no effect.",
            )
            .small()
            .weak(),
        );
        let editable = model.project.as_ref().is_some_and(|p| !p.read_only());
        for k in &model.unmatched_keys {
            ui.horizontal(|ui| {
                ui.label(k);
                if ui
                    .add_enabled(editable, egui::Button::new("Remove"))
                    .clicked()
                {
                    actions.push(Action::RemoveUnmatched(k.clone()));
                }
            });
        }
    });
}
