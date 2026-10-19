//! M6 Track F3b (spec §5): the rename dialog. Drawing only: the rows come from `rename::plan_rename`, and Apply is
//! one model call.
use super::columns::{AMBER, RED};
use super::dialogs::{dialog, Role};
use crate::model::rename::{Case, RenameDialog, RenameMode};
use crate::model::{AppModel, Dialog};
use eframe::egui;

pub fn draw(ctx: &egui::Context, model: &mut AppModel, mut d: RenameDialog) {
    let plan = crate::model::rename::plan_rename(model, &d);
    let mut mode = d.mode;
    let n = d.keys.len();
    let title = match d.mode {
        RenameMode::Inputs => format!("Rename {n} input{}", if n == 1 { "" } else { "s" }),
        RenameMode::Outputs => {
            format!("Rename outputs of {n} set{}", if n == 1 { "" } else { "s" })
        }
    };
    let can = plan.can_apply();
    let buttons = [("Cancel", Role::Plain, true), ("Apply", Role::Default, can)];
    // K4: the shared keys. Enter applies when Apply is enabled, not on the opening frame and not while a widget
    // keeps the focus (a single-line field gives it up on that Enter); a click outside does nothing (typed input).
    let (k, clicked) = dialog(ctx, "rename", false, 860.0, &title, &buttons, |ui| {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut mode, RenameMode::Inputs, "Inputs")
                .on_hover_text("Read the files under new names (sdc.toml [inputs]); the files on disk are not touched.");
            ui.selectable_value(&mut mode, RenameMode::Outputs, "Outputs")
                .on_hover_text("Give each set's outputs a new base name; every output keeps its folder and suffix.");
        });
        ui.label(
            egui::RichText::new(match d.mode {
                RenameMode::Inputs if d.recipe.path => "The folders and the base name change; the map suffix (_n, _normal…) and the extension stay.",
                RenameMode::Inputs => "Only the base name changes (the part before _n, _normal…). Tick Whole path to change the folders too.",
                RenameMode::Outputs => "A TruePBR set on Community Shaders renames through its PGPatcher rename.",
            })
            .small()
            .weak(),
        );
        ui.add_space(4.0);
        egui::Grid::new("rename-recipe")
            .num_columns(4)
            .show(ui, |ui| {
                ui.label("Find");
                ui.text_edit_singleline(&mut d.recipe.find);
                ui.label("Replace");
                ui.text_edit_singleline(&mut d.recipe.replace);
                ui.end_row();
                ui.label("Prefix");
                ui.text_edit_singleline(&mut d.recipe.prefix);
                ui.label("Suffix");
                ui.text_edit_singleline(&mut d.recipe.suffix);
                ui.end_row();
                ui.checkbox(&mut d.recipe.regex, "Regex")
                    .on_hover_text("$1 or ${name} in Replace for a capture group");
                if d.mode == RenameMode::Inputs {
                    ui.checkbox(&mut d.recipe.path, "Whole path").on_hover_text(
                        "Find, Replace, Prefix and Suffix work on the folders and the base name",
                    );
                }
                egui::ComboBox::from_id_salt("rename-case")
                    .selected_text(case_label(d.recipe.case))
                    .show_ui(ui, |ui| {
                        for c in [Case::Keep, Case::Lower, Case::Upper] {
                            ui.selectable_value(&mut d.recipe.case, c, case_label(c));
                        }
                    });
                ui.end_row();
            });
        match &plan.error {
            Some(e) => {
                ui.colored_label(RED, e);
            }
            None if !d.recipe.find.is_empty() => {
                let (k, m) = plan.matched();
                ui.weak(format!("{k} of {m} matched"));
            }
            None => {}
        }
        ui.separator();
        egui::ScrollArea::vertical()
            .max_height(360.0)
            .show(ui, |ui| {
                egui::Grid::new("rename-rows")
                    .striped(true)
                    .num_columns(5)
                    .show(ui, |ui| {
                        for h in ["", "Old", "Kind", "Output", ""] {
                            ui.label(egui::RichText::new(h).strong());
                        }
                        ui.end_row();
                        for r in &plan.rows {
                            let mut on = r.included;
                            let tick = ui.add_enabled(
                                r.skipped.is_none(),
                                egui::Checkbox::without_text(&mut on),
                            );
                            if tick.changed() {
                                if on {
                                    d.exclude.remove(&r.key);
                                } else {
                                    d.exclude.insert(r.key.clone());
                                }
                            }
                            let weak = !r.included || r.skipped.is_some();
                            let text = |s: &str| {
                                let t = egui::RichText::new(s);
                                if weak {
                                    t.weak()
                                } else {
                                    t
                                }
                            };
                            ui.label(text(&r.old));
                            ui.label(text(&r.kind.as_ref().map_or(String::new(), |(a, b)| {
                                if a == b {
                                    a.clone()
                                } else {
                                    format!("{a} → {b}")
                                }
                            })));
                            ui.label(text(&r.output));
                            match (&r.skipped, &r.problem, &r.warning) {
                                (Some(s), _, _) => ui.weak(s),
                                (_, Some(p), _) if r.included => ui.colored_label(RED, p),
                                (_, _, Some(w)) if r.included => ui.colored_label(AMBER, w),
                                _ => ui.label(""),
                            };
                            ui.end_row();
                        }
                    });
            });
    });
    let apply = clicked == Some(1) || (k.enter && can);
    if clicked == Some(0) || k.cancel {
        model.dialog = None;
        return;
    }
    // The edits made this frame are kept before anything acts on them.
    if let Some(Dialog::Rename(kept)) = model.dialog.as_mut() {
        *kept = d;
    }
    if mode != model_mode(model) {
        model.set_rename_mode(mode);
    }
    if apply {
        model.apply_rename();
    }
}

fn model_mode(model: &AppModel) -> RenameMode {
    match &model.dialog {
        Some(Dialog::Rename(d)) => d.mode,
        _ => RenameMode::Inputs,
    }
}

fn case_label(c: Case) -> &'static str {
    match c {
        Case::Keep => "Keep case",
        Case::Lower => "lower case",
        Case::Upper => "UPPER CASE",
    }
}
