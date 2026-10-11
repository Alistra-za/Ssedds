//! Modal dialogs for `model.dialog` (M3a spec §2.1).
use crate::model::{AfterExport, AppModel, Dialog, DocKind};
use eframe::egui;

pub fn draw(ctx: &egui::Context, model: &mut AppModel) {
    let Some(dialog) = model.dialog.clone() else {
        return;
    };
    match dialog {
        Dialog::CloseUnsaved => close_unsaved(ctx, model),
        Dialog::OpenUnsaved(root) => open_unsaved(ctx, model, root),
        Dialog::ExportRunning(action) => export_running(ctx, model, action),
        Dialog::ExternalChange(which) => external_change(ctx, model, which),
        Dialog::Error(msg) => error(ctx, model, &msg),
        Dialog::ProceedAnyway(n) => proceed_anyway(ctx, model, n),
    }
}

fn proceed_anyway(ctx: &egui::Context, model: &mut AppModel, n: usize) {
    let (mut proceed, mut cancel) = (false, false);
    modal(ctx, "proceed-anyway", |ui| {
        ui.heading("Undecided textures");
        ui.label(format!("{n} textures still need a decision."));
        ui.label(
            "Proceed anyway exports the decided textures and skips these; nothing is decided for you.",
        );
        ui.horizontal(|ui| {
            proceed = ui.button("Proceed anyway").clicked();
            cancel = ui.button("Cancel").clicked();
        });
    });
    if proceed {
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
