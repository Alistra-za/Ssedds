//! The bottom drawer (M3a spec §2.5): progress and the report of the last export, by file and grouped by code.
use crate::model::export::{cli_command, RowState};
use crate::model::{AppModel, Dialog};
use eframe::egui;
use std::path::Path;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Files,
    Warnings,
}

/// The drawer's own drawing state.
#[derive(Default)]
pub struct DrawerState {
    pub tab: Tab,
}

/// Draw the drawer while `model.export.open`. Call before the central panel.
pub fn draw(ui: &mut egui::Ui, model: &mut AppModel, state: &mut DrawerState) {
    if !model.export.open {
        return;
    }
    egui::Panel::bottom("report")
        .resizable(true)
        .default_size(220.0)
        .show(ui, |ui| {
            toolbar(ui, model);
            ui.separator();
            ui.horizontal(|ui| {
                ui.selectable_value(&mut state.tab, Tab::Files, "Files");
                ui.selectable_value(&mut state.tab, Tab::Warnings, "Warnings");
            });
            let mut picked = None;
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| match state.tab {
                    Tab::Files => files(ui, model, &mut picked),
                    Tab::Warnings => warnings(ui, model, &mut picked),
                });
            if let Some(key) = picked {
                model.select_job(&key);
            }
        });
}

/// The header line: counts, then Cancel / Close / Save report / Copy CLI command.
fn toolbar(ui: &mut egui::Ui, model: &mut AppModel) {
    let count =
        |f: &dyn Fn(&RowState) -> bool| model.export.rows.iter().filter(|r| f(&r.state)).count();
    let converted = count(&|s| matches!(s, RowState::Done { .. }));
    let up_to_date = count(&|s| matches!(s, RowState::UpToDate));
    let skipped = count(&|s| matches!(s, RowState::Skipped(..)));
    let failed = count(&|s| matches!(s, RowState::Failed(_)));
    let validation_failed = count(&|s| matches!(s, RowState::Done { passed: false }));
    let finished = model
        .export
        .rows
        .iter()
        .filter(|r| !matches!(r.state, RowState::Queued | RowState::Running))
        .count();
    let running = model.export.running.is_some();
    let (mut cancel, mut close, mut save, mut copy) = (false, false, false, false);
    ui.horizontal_wrapped(|ui| {
        if running {
            let total = model.export.rows.len().max(1);
            ui.add(
                egui::ProgressBar::new(finished as f32 / total as f32)
                    .desired_width(120.0)
                    .animate(false),
            );
            ui.label(format!(
                "Exporting: {finished} of {} done.",
                model.export.rows.len()
            ));
        }
        ui.label(format!(
            "{converted} converted, {up_to_date} up to date, {skipped} skipped, {failed} failed, \
             {validation_failed} validation failed"
        ));
        if let Some(e) = &model.export.error {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
    });
    ui.horizontal(|ui| {
        if running {
            cancel = ui.button("Cancel").clicked();
        }
        save = ui
            .add_enabled(!running, egui::Button::new("Save report…"))
            .clicked();
        copy = ui
            .button("Copy CLI command")
            .on_hover_text("The command that reproduces this export from the saved sdc.toml")
            .clicked();
        close = ui.button("Close").clicked();
    });
    if cancel {
        model.cancel_export();
    }
    if close {
        model.export.open = false;
    }
    if copy {
        if let Some(root) = &model.root {
            let cmd = cli_command(
                root,
                &model.export_root(),
                model.settings.bc1_decoder,
                model.settings.jobs,
            );
            ui.ctx().copy_text(cmd);
        }
    }
    if save {
        save_report(model);
    }
}

fn save_report(model: &mut AppModel) {
    let Some(path) = rfd::FileDialog::new()
        .set_file_name("sdc-report.jsonl")
        .add_filter("JSON Lines", &["jsonl"])
        .add_filter("Markdown", &["md"])
        .save_file()
    else {
        return;
    };
    if let Err(e) = write_report(model, &path) {
        model.dialog = Some(Dialog::Error(format!(
            "Could not write the report to {}: {e}",
            path.display()
        )));
    }
}

/// `.md` writes the Markdown report; anything else the JSON Lines event stream.
pub fn write_report(model: &AppModel, path: &Path) -> std::io::Result<()> {
    let markdown = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("md"));
    let text = if markdown {
        model.export.to_markdown()
    } else {
        model.export.to_jsonl()
    };
    std::fs::write(path, text)
}

fn files(ui: &mut egui::Ui, model: &AppModel, picked: &mut Option<String>) {
    for r in &model.export.rows {
        ui.horizontal(|ui| {
            if ui.selectable_label(false, &r.job_key).clicked() {
                *picked = Some(r.job_key.clone());
            }
            ui.label(r.state.label());
            if !r.warnings.is_empty() {
                ui.label(format!("{} warnings", r.warnings.len()));
            }
        });
    }
}

fn warnings(ui: &mut egui::Ui, model: &AppModel, picked: &mut Option<String>) {
    let by_code = model.export.by_code();
    if by_code.is_empty() {
        ui.label("No warnings.");
        return;
    }
    for (code, keys) in by_code {
        ui.horizontal_wrapped(|ui| {
            ui.strong(code.as_str());
            ui.label(format!("{:?}", code.severity()).to_lowercase());
            ui.label(format!("{}", keys.len()));
            for k in &keys {
                if ui.link(k).clicked() {
                    *picked = Some(k.clone());
                }
            }
        });
    }
}
