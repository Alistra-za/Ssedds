//! The bottom drawer (M3a spec §2.5): progress and the report of the last export, by file and grouped by code.
//! Task 5 (items 6 and 14): Open export folder, Show in Explorer, the progress line with pace, a virtualised Files
//! list with filters, and a Warnings tab that shows each message. DECISIONS AG34-AG38.
use crate::model::export::{cli_command, warning_line, FileFilter, FilesIndex, RowState};
use crate::model::{AppModel, Dialog};
use eframe::egui;
use std::path::Path;
use std::time::{Duration, Instant};

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
    /// The Files tab's filters; both on means both must hold.
    pub filter: FileFilter,
    /// The filtered rows and the warning list, cached by (filter, export revision).
    pub index: FilesIndex,
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
                if state.tab == Tab::Files {
                    ui.separator();
                    ui.toggle_value(&mut state.filter.failed_only, "Failed only")
                        .on_hover_text("Only files that failed or failed validation");
                    ui.toggle_value(&mut state.filter.has_warnings, "Has warnings")
                        .on_hover_text("Only files with warnings");
                }
            });
            let (mut picked, mut reveal) = (None, None);
            match state.tab {
                Tab::Files => files(ui, model, state, &mut picked, &mut reveal),
                Tab::Warnings => warnings(ui, model, &mut state.index, &mut picked),
            }
            if let Some(key) = picked {
                model.select_job(&key);
            }
            if let Some(p) = reveal {
                let _ = model.reveal(&p);
            }
        });
}

/// The header line: counts, then Cancel / Open export folder / Close / Save report / Copy CLI command.
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
    let (mut cancel, mut close, mut save, mut copy, mut open) = (false, false, false, false, false);
    ui.horizontal_wrapped(|ui| {
        if running {
            let total = model.export.rows.len().max(1);
            ui.add(
                egui::ProgressBar::new(finished as f32 / total as f32)
                    .desired_width(120.0)
                    .animate(false),
            );
            // `done/total · elapsed · files/s · ETA`; the clock moves between events, so ask for a repaint.
            let line = match &model.export.pace {
                Some(p) => p.line(finished, model.export.rows.len(), Instant::now()),
                None => format!("{finished}/{}", model.export.rows.len()),
            };
            ui.label(format!("Exporting: {line}"));
            ui.ctx().request_repaint_after(Duration::from_millis(500));
        }
        ui.label(format!(
            "{converted} converted, {up_to_date} up to date, {skipped} skipped, {failed} failed, \
             {validation_failed} validation failed"
        ));
        if let Some(e) = &model.export.error {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
        for n in &model.export.notes {
            ui.label(egui::RichText::new(n).small());
        }
    });
    let folder = model.export_folder_state();
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
        let r = ui.add_enabled(folder.is_ok(), egui::Button::new("Open export folder"));
        open = match &folder {
            Ok(p) => r.on_hover_text(p.display().to_string()).clicked(),
            Err(why) => {
                r.on_disabled_hover_text(why);
                false
            }
        };
        close = ui.button("Close").clicked();
    });
    if cancel {
        model.cancel_export();
    }
    if close {
        model.export.open = false;
    }
    if open {
        let _ = model.open_export_folder();
    }
    if copy {
        if let Some(root) = &model.root {
            let cmd = cli_command(
                root,
                &model.export_root(),
                model.session_bc1(),
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

/// The Files tab: only the rows on screen are laid out (`show_rows`), over the cached filtered index.
fn files(
    ui: &mut egui::Ui,
    model: &AppModel,
    state: &mut DrawerState,
    picked: &mut Option<String>,
    reveal: &mut Option<std::path::PathBuf>,
) {
    let filter = state.filter;
    let n = state.index.len(&model.export, filter);
    if n == 0 {
        ui.weak(if model.export.rows.is_empty() {
            "No files yet."
        } else {
            "No files match the filters."
        });
        return;
    }
    let row_h = ui.spacing().interact_size.y;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, row_h, n, |ui, range| {
            for i in range {
                let Some(r) = state.index.row(&model.export, filter, i) else {
                    continue;
                };
                ui.horizontal(|ui| {
                    if ui.selectable_label(false, &r.job_key).clicked() {
                        *picked = Some(r.job_key.clone());
                    }
                    ui.label(r.state.label());
                    if !r.warnings.is_empty() {
                        ui.label(format!("{} warnings", r.warnings.len()));
                    }
                    // The written output once it exists, otherwise the source.
                    let target = model.row_reveal_target(r);
                    let b = ui.add_enabled(
                        target.is_some(),
                        egui::Button::new("Show in Explorer").small(),
                    );
                    let b = match &target {
                        Some(_) => b.on_hover_text(
                            "Select the file in Explorer: the output once written, else the source",
                        ),
                        None => {
                            b.on_disabled_hover_text("Neither the output nor the source is on disk")
                        }
                    };
                    if b.clicked() {
                        *reveal = target;
                    }
                });
            }
        });
}

/// The Warnings tab: one row per warning, `CODE severity` then the message (wrapped) then the file; virtualised, with
/// a fixed row height of three lines (a longer message is cut there and shows whole on hover).
fn warnings(
    ui: &mut egui::Ui,
    model: &AppModel,
    index: &mut FilesIndex,
    picked: &mut Option<String>,
) {
    let list = index.warnings(&model.export);
    if list.is_empty() {
        ui.label("No warnings.");
        return;
    }
    let line = ui.text_style_height(&egui::TextStyle::Body);
    let row_h = 3.0 * line;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, row_h, list.len(), |ui, range| {
            for &(r, w) in &list[range] {
                let row = &model.export.rows[r];
                let warning = &row.warnings[w];
                let size = egui::vec2(ui.available_width(), row_h);
                ui.allocate_ui_with_layout(size, egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_clip_rect(ui.clip_rect().intersect(ui.max_rect()));
                    ui.horizontal_wrapped(|ui| {
                        ui.strong(warning.code.as_str());
                        ui.label(format!("{:?}", warning.severity).to_lowercase());
                        ui.label(&warning.message)
                            .on_hover_text(warning_line(warning));
                        if ui.link(&row.job_key).clicked() {
                            *picked = Some(row.job_key.clone());
                        }
                    });
                });
            }
        });
}
