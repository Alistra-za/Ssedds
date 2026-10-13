//! Modal dialogs for `model.dialog` (M3a spec §2.1).
use super::settings_panel::alpha_options;
use crate::model::items::Decision;
use crate::model::{AfterExport, Answer, AppModel, Dialog, DocKind};
use eframe::egui;
use engine::config::{AlphaPolicy, GreenConvention};

pub fn draw(ctx: &egui::Context, model: &mut AppModel) {
    let Some(dialog) = model.dialog.clone() else {
        return;
    };
    match dialog {
        Dialog::CloseUnsaved => close_unsaved(ctx, model),
        Dialog::OpenUnsaved(root) => open_unsaved(ctx, model, root),
        Dialog::ExportRunning(action) => export_running(ctx, model, action),
        Dialog::ExternalChange(which) => external_change(ctx, model, which),
        Dialog::OverwriteJson(path) => overwrite_json(ctx, model, &path),
        Dialog::Error(msg) => error(ctx, model, &msg),
        Dialog::ProceedAnyway(keys) => proceed_anyway(ctx, model, &keys),
        Dialog::ExportBlocked(issues) => export_issues(ctx, model, &issues, false),
        Dialog::ExportWarnings(issues) => export_issues(ctx, model, &issues, true),
        Dialog::CopySettings { from, to, fields } => copy_settings(ctx, model, &from, &to, fields),
        Dialog::ConfirmResetAll(keys) => confirm_reset_all(ctx, model, &keys),
    }
}

fn export_issues(
    ctx: &egui::Context,
    model: &mut AppModel,
    issues: &[crate::model::pbr_export::ExportIssue],
    can_proceed: bool,
) {
    let (mut proceed, mut close, mut select) = (false, false, None);
    let id = if can_proceed {
        "export-warnings"
    } else {
        "export-blocked"
    };
    modal(ctx, id, |ui| {
        ui.heading(if can_proceed {
            "Check before exporting"
        } else {
            "Export is blocked"
        });
        ui.label(if can_proceed {
            "These may not do what you expect. PGPatcher's rules are applied as written; nothing is changed for you."
        } else {
            "Fix these first. Click one to select its set."
        });
        egui::ScrollArea::vertical()
            .max_height(260.0)
            .show(ui, |ui| {
                for i in issues {
                    if i.set_key.is_empty() {
                        ui.label(&i.text);
                    } else if ui.link(&i.text).clicked() {
                        select = Some(i.set_key.clone());
                    }
                }
            });
        ui.horizontal(|ui| {
            if can_proceed {
                proceed = ui.button("Proceed anyway").clicked();
                close = ui.button("Cancel").clicked();
            } else {
                close = ui.button("Close").clicked();
            }
        });
    });
    if let Some(k) = select {
        model.dialog = None;
        model.select_set(&k);
    } else if proceed {
        model.proceed_with_warnings();
    } else if close {
        model.dialog = None;
    }
}

/// Rows past which the list scrolls.
const VISIBLE_ROWS: f32 = 12.0;

fn proceed_anyway(ctx: &egui::Context, model: &mut AppModel, keys: &[String]) {
    use super::settings_panel::{offered_groups, subtype_label};
    let (mut proceed, mut cancel, mut accept_all) = (false, false, false);
    let mut select: Option<String> = None;
    let mut decide: Option<(String, engine::config::Subtype)> = None;
    let mut answer: Option<(String, Answer)> = None;
    let any_suggestion = keys
        .iter()
        .any(|k| matches!(model.subtype_question(k), Some(Some(_))));
    let target = model.target;
    modal(ctx, "proceed-anyway", |ui| {
        ui.heading("Undecided textures");
        ui.label(format!("{} textures still need a decision.", keys.len()));
        ui.label(
            "Decide them here, or proceed anyway to export the decided textures and skip these.",
        );
        let row_h = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
        egui::ScrollArea::vertical()
            .max_height(row_h * VISIBLE_ROWS)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for key in keys {
                    let Some(item) = model.item(key) else {
                        continue;
                    };
                    let name = item.name.clone();
                    let fallback = item.job.as_ref().map(|j| j.profile.subtype);
                    let question = model.subtype_question(key);
                    let decisions = item.decisions.clone();
                    ui.horizontal(|ui| {
                        if ui.link(&name).clicked() {
                            select = Some(key.clone());
                        }
                        let Some(suggested) = question else {
                            return;
                        };
                        let Some(default) = suggested.or(fallback) else {
                            return;
                        };
                        let id = egui::Id::new(("undecided-row", key));
                        let mut chosen: engine::config::Subtype =
                            ui.data(|d| d.get_temp(id)).unwrap_or(default);
                        egui::ComboBox::from_id_salt(id)
                            .selected_text(subtype_label(chosen))
                            .show_ui(ui, |ui| {
                                for (heading, members) in offered_groups(target) {
                                    ui.label(egui::RichText::new(heading).weak());
                                    for st in members {
                                        ui.selectable_value(&mut chosen, st, subtype_label(st));
                                    }
                                }
                            });
                        ui.data_mut(|d| d.insert_temp(id, chosen));
                        if ui
                            .button("\u{2713}")
                            .on_hover_text("Decide this texture")
                            .clicked()
                        {
                            decide = Some((key.clone(), chosen));
                        }
                    });
                    // The questions that are not a sub-type: a small fixed set of choices each, answered inline.
                    for d in &decisions {
                        ui.horizontal_wrapped(|ui| match d {
                            Decision::AlphaMeaning => {
                                ui.label("Alpha means:");
                                for (v, label) in alpha_options() {
                                    let Ok(policy) = serde_json::from_value::<AlphaPolicy>(v)
                                    else {
                                        continue;
                                    };
                                    if ui.button(label).clicked() {
                                        answer = Some((key.clone(), Answer::Alpha(policy)));
                                    }
                                }
                            }
                            Decision::GreenConvention => {
                                ui.label("Green channel:");
                                if ui.button("OpenGL").clicked() {
                                    answer =
                                        Some((key.clone(), Answer::Green(GreenConvention::Opengl)));
                                }
                                if ui.button("DirectX").clicked() {
                                    answer = Some((
                                        key.clone(),
                                        Answer::Green(GreenConvention::Directx),
                                    ));
                                }
                            }
                            Decision::Acknowledge(code) => {
                                ui.label(code.as_str());
                                if ui.button("Acknowledge for this texture").clicked() {
                                    answer = Some((key.clone(), Answer::Acknowledge(*code)));
                                }
                            }
                            Decision::Subtype { .. } => {}
                        });
                    }
                }
            });
        ui.horizontal(|ui| {
            accept_all = ui
                .add_enabled(any_suggestion, egui::Button::new("Accept all suggestions"))
                .on_hover_text("Decide every texture that has a suggestion; the others stay listed")
                .clicked();
            proceed = ui.button("Proceed anyway").clicked();
            cancel = ui.button("Cancel").clicked();
        });
    });
    if let Some(k) = select {
        model.select_item(&k);
    }
    if let Some((k, a)) = answer {
        model.answer_undecided_row(&k, a);
    }
    if let Some((k, st)) = decide {
        model.decide_undecided_row(&k, st);
    } else if accept_all {
        model.accept_all_suggestions();
    } else if proceed {
        model.proceed_anyway();
    } else if cancel {
        // The undecided textures stay highlighted in the Textures column.
        model.dialog = None;
    }
}

fn modal(ctx: &egui::Context, id: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::Modal::new(egui::Id::new(id)).show(ctx, |ui| {
        ui.set_max_width(420.0);
        add(ui);
    });
}

/// F3a D4: the copy checklist, every field ticked to start. The ticks live in the dialog itself.
fn copy_settings(
    ctx: &egui::Context,
    model: &mut AppModel,
    from: &str,
    to: &[String],
    mut fields: Vec<(String, bool)>,
) {
    let (mut apply, mut close) = (false, false);
    let name = model
        .item(from)
        .map_or(from.to_string(), |i| i.name.clone());
    modal(ctx, "copy-settings", |ui| {
        ui.heading(format!(
            "Copy settings from {name} to {} selected",
            to.len()
        ));
        ui.horizontal(|ui| {
            if ui.button("Check all").clicked() {
                fields.iter_mut().for_each(|f| f.1 = true);
            }
            if ui.button("Check none").clicked() {
                fields.iter_mut().for_each(|f| f.1 = false);
            }
        });
        for (f, on) in fields.iter_mut() {
            let value = model
                .project
                .as_ref()
                .and_then(|p| p.file_value(from, f))
                .map_or(String::new(), |v| v.to_string());
            ui.checkbox(
                on,
                format!("{}: {value}", crate::model::bulk::field_label(f)),
            );
        }
        ui.horizontal(|ui| {
            apply = ui
                .add_enabled(fields.iter().any(|f| f.1), egui::Button::new("Apply"))
                .clicked();
            close = ui.button("Cancel").clicked();
        });
    });
    if apply {
        let ticked: Vec<String> = fields.iter().filter(|f| f.1).map(|f| f.0.clone()).collect();
        model.apply_copy_settings(from, to, &ticked);
    } else if close {
        model.dialog = None;
    } else if let Some(Dialog::CopySettings { fields: kept, .. }) = model.dialog.as_mut() {
        *kept = fields;
    }
}

/// F3a D4: Reset all asks first; it names how many textures lose their settings.
fn confirm_reset_all(ctx: &egui::Context, model: &mut AppModel, keys: &[String]) {
    let (mut reset, mut close) = (false, false);
    modal(ctx, "confirm-reset-all", |ui| {
        ui.heading(format!("Reset all settings of {} textures?", keys.len()));
        ui.label(
            "Their entries in sdc.toml are removed; acknowledged warnings are kept. Settings in \
             sdc-project.toml are not touched. Undo brings everything back.",
        );
        ui.horizontal(|ui| {
            reset = ui.button("Reset").clicked();
            close = ui.button("Cancel").clicked();
        });
    });
    if reset {
        model.reset_all_settings(keys);
    } else if close {
        model.dialog = None;
    }
}

fn close_unsaved(ctx: &egui::Context, model: &mut AppModel) {
    let (mut save, mut discard, mut cancel) = (false, false, false);
    modal(ctx, "close-unsaved", |ui| {
        ui.heading("Unsaved changes");
        ui.label("sdc.toml or sdc-project.toml has changes that are not saved.");
        ui.horizontal(|ui| {
            save = ui.button("Save").clicked();
            discard = ui.button("Discard").clicked();
            cancel = ui.button("Cancel").clicked();
        });
    });
    if save {
        model.dialog = None;
        // A failed save raises its own dialog (changed on disk, or an error) and the window stays open.
        if model.save().is_ok() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    } else if discard {
        model.dialog = None;
        // Dropping the edits makes the close request pass `request_close`.
        if let Some(p) = model.project.as_mut() {
            p.reload();
        }
        if let Some(s) = model.sets_doc.as_mut() {
            s.reload();
        }
        model.history.clear();
        model.refresh_banner();
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    } else if cancel {
        model.dialog = None;
    }
}

fn external_change(ctx: &egui::Context, model: &mut AppModel, which: DocKind) {
    let (mut reload, mut overwrite, mut cancel) = (false, false, false);
    modal(ctx, "external-change", |ui| {
        ui.heading(format!("{} changed on disk", which.file()));
        ui.label("Another program changed the file after it was opened.");
        ui.horizontal(|ui| {
            reload = ui.button("Reload").clicked();
            overwrite = ui.button("Overwrite").clicked();
            cancel = ui.button("Cancel").clicked();
        });
    });
    if reload {
        model.resolve_external_change(which, false);
    } else if overwrite {
        model.resolve_external_change(which, true);
    } else if cancel {
        model.dialog = None;
    }
}

fn error(ctx: &egui::Context, model: &mut AppModel, msg: &str) {
    let mut ok = false;
    modal(ctx, "error", |ui| {
        ui.heading("Error");
        ui.label(msg);
        ok = ui.button("OK").clicked();
    });
    if ok {
        model.dialog = None;
    }
}

fn open_unsaved(ctx: &egui::Context, model: &mut AppModel, root: std::path::PathBuf) {
    let (mut save, mut discard, mut cancel) = (false, false, false);
    modal(ctx, "open-unsaved", |ui| {
        ui.heading("Unsaved changes");
        ui.label(
            "sdc.toml or sdc-project.toml has changes that are not saved. Opening another folder would lose them.",
        );
        ui.horizontal(|ui| {
            save = ui.button("Save").clicked();
            discard = ui.button("Discard").clicked();
            cancel = ui.button("Cancel").clicked();
        });
    });
    if save {
        model.resolve_open_unsaved(root, true);
    } else if discard {
        model.resolve_open_unsaved(root, false);
    } else if cancel {
        model.dialog = None;
    }
}

fn export_running(ctx: &egui::Context, model: &mut AppModel, action: AfterExport) {
    let (mut cancel_export, mut keep) = (false, false);
    modal(ctx, "export-running", |ui| {
        ui.heading("An export is running");
        ui.label(match &action {
            AfterExport::Close => "Cancel it and close?",
            AfterExport::Open(_) => "Cancel it and open the other folder?",
        });
        ui.horizontal(|ui| {
            cancel_export = ui.button("Cancel export and continue").clicked();
            keep = ui.button("Keep running").clicked();
        });
    });
    if cancel_export {
        model.resolve_export_running(action, true);
    } else if keep {
        model.resolve_export_running(action, false);
    }
}

fn overwrite_json(ctx: &egui::Context, model: &mut AppModel, path: &std::path::Path) {
    let (mut overwrite, mut keep) = (false, false);
    modal(ctx, "overwrite-json", |ui| {
        ui.heading("The PGPatcher JSON was edited");
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        ui.label(format!(
            "{name} was edited outside sdc since the last export."
        ));
        ui.horizontal(|ui| {
            overwrite = ui.button("Overwrite edited JSON").clicked();
            keep = ui.button("Keep it").clicked();
        });
    });
    if overwrite {
        model.resolve_overwrite_json(true);
    } else if keep {
        model.resolve_overwrite_json(false);
    }
}
