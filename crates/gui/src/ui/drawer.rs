//! The Files tab of the bottom panel (M3a spec 2.5, layout v2 A1): progress and the report of the last export, by file.
//! Open export folder, Show in Explorer, the progress line with pace, and a virtualised Files list with filters.
//! DECISIONS AG34-AG38. A file's warnings show on hover of its row (R9).
use crate::model::export::{cli_command, warning_line, FileFilter, FilesIndex, RowState};
use crate::model::picker::{self, PickerKind, PickerPurpose, PickerSpec};
use crate::model::shell::StatCache;
use crate::model::AppModel;
use eframe::egui;
use std::path::Path;
use std::time::{Duration, Instant};

/// The drawer's own drawing state.
#[derive(Default)]
pub struct DrawerState {
    /// The Files tab's filters; both on means both must hold.
    pub filter: FileFilter,
    /// The filtered rows, cached by (filter, export revision).
    pub index: FilesIndex,
    /// Cached "is the export folder / this file on disk" answers (AG39).
    pub stats: StatCache,
    /// The `model.drawer_epoch` `stats` belongs to.
    pub epoch: u64,
}

/// The Files tab: the toolbar, the two filters and the list of the last export's files.
pub fn files_tab(ui: &mut egui::Ui, model: &mut AppModel, state: &mut DrawerState) {
    toolbar(ui, model, &mut state.stats);
    ui.separator();
    ui.horizontal(|ui| {
        ui.toggle_value(&mut state.filter.failed_only, "Failed only")
            .on_hover_text("Only files that failed or failed validation");
        ui.toggle_value(&mut state.filter.has_warnings, "Has warnings")
            .on_hover_text("Only files with warnings");
    });
    let (mut picked, mut reveal) = (None, None);
    files(ui, model, state, &mut picked, &mut reveal);
    if let Some(key) = picked {
        model.select_job(&key);
    }
    if let Some(p) = reveal {
        let _ = model.reveal(&p);
    }
}

/// The header line: counts, then Cancel / Open export folder / Close / Save report / Copy CLI command.
fn toolbar(ui: &mut egui::Ui, model: &mut AppModel, stats: &mut StatCache) {
    let count =
        |f: &dyn Fn(&RowState) -> bool| model.export.rows.iter().filter(|r| f(&r.state)).count();
    let converted = count(&|s| matches!(s, RowState::Done { .. }));
    let up_to_date = count(&|s| matches!(s, RowState::UpToDate));
    let skipped = count(&|s| matches!(s, RowState::Skipped(..)));
    let failed = count(&|s| matches!(s, RowState::Failed(_)));
    let validation_failed = count(&|s| matches!(s, RowState::Done { passed: false }));
    let (finished, _) = model.export.progress();
    let running = model.export.running.is_some();
    let (mut cancel, mut close, mut save, mut copy, mut open) = (false, false, false, false, false);
    ui.horizontal_wrapped(|ui| {
        if running {
            let total = model.export.rows.len().max(1);
            ui.add(
                egui::ProgressBar::new(super::header::shown_progress(
                    ui.ctx(),
                    model.drawer_epoch,
                    finished as f32 / total as f32,
                ))
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
        // Note F?14 (rule 1.7): no counts of nothing before an export.
        if !model.export.rows.is_empty() {
            ui.label(format!(
                "{converted} converted, {up_to_date} up to date, {skipped} skipped, {failed} failed, \
                 {validation_failed} validation failed"
            ));
        }
        if let Some(e) = &model.export.error {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
    });
    // Audit 08c-4: notes can run to hundreds of lines ("not in JSON: …"), so they scroll inside a capped area.
    if !model.export.notes.is_empty() {
        egui::CollapsingHeader::new(format!("Notes ({})", model.export.notes.len()))
            .id_salt("export_notes")
            .default_open(true)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("export_notes_scroll")
                    .max_height(96.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for n in &model.export.notes {
                            ui.label(egui::RichText::new(n).small());
                        }
                    });
            });
    }
    let exists = stats.is_dir(&model.export_root(), Instant::now());
    let folder = model.export_folder_check(exists);
    ui.horizontal(|ui| {
        if running {
            cancel = ui.button("Cancel").clicked();
        }
        let busy = model.picker.busy();
        let r = ui.add_enabled(!running && !busy, egui::Button::new("Save report…"));
        save = (if busy {
            r.on_disabled_hover_text(picker::BUSY_HOVER)
        } else {
            r
        })
        .clicked();
        copy = ui
            .button("Copy CLI command")
            .on_hover_text(
                "The command that reproduces this export from the saved sdc.toml. Set edits in sdc-project.toml and mesh evidence are not carried",
            )
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
        model.collapse_bottom();
    }
    if open {
        let _ = model.open_export_folder();
    }
    if copy {
        if let Some(root) = &model.root {
            let cmd = cli_command(
                root,
                &model.cli_output(),
                model.session_bc1(),
                model.settings.jobs,
            );
            ui.ctx().copy_text(cmd);
        }
    }
    if save {
        let kind = PickerKind::Save {
            file_name: "sdc-report.jsonl",
            filters: &[("JSON Lines", &["jsonl"]), ("Markdown", &["md"])],
            dir: None,
        };
        model.pick(
            ui.ctx(),
            PickerPurpose::SaveReport,
            PickerSpec {
                title: None,
                kind,
                start: None,
            },
        );
    }
}

/// The report text for `path`: `.md` is the Markdown report, anything else the JSON Lines event stream.
pub fn report_text(model: &AppModel, path: &Path) -> String {
    let markdown = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("md"));
    if markdown {
        model.export.to_markdown()
    } else {
        model.export.to_jsonl()
    }
}

/// Write the report on the calling thread (tests; the app writes it on a worker, `AppModel::save_report`).
pub fn write_report(model: &AppModel, path: &Path) -> std::io::Result<()> {
    std::fs::write(path, report_text(model, path))
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
            "Not exported yet."
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
                        let lines: Vec<String> = r.warnings.iter().map(warning_line).collect();
                        ui.label(format!("{} warnings", r.warnings.len()))
                            .on_hover_text(lines.join("\n"));
                    }
                    // The written output once it exists, otherwise the source.
                    let now = Instant::now();
                    let target = model.row_reveal_with(r, &mut |p| state.stats.is_file(p, now));
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
