//! The header (M3a spec §2.1): open, recent, mode, collapse, export folder, save, export; then the status line and banner.
use crate::model::AppModel;
use eframe::egui;
use engine::config::Target;
use std::path::PathBuf;

const WARNING: egui::Color32 = egui::Color32::from_rgb(230, 160, 30);

fn target_name(t: Target) -> &'static str {
    match t {
        Target::Cs => "cs",
        Target::Vanilla => "vanilla",
        Target::Cpm => "cpm",
    }
}

/// `sdc — <folder>`, plus ` ●` when there are unsaved edits.
fn window_title(model: &AppModel) -> String {
    let mut t = match &model.root {
        Some(r) => format!("sdc — {}", r.display()),
        None => "sdc".to_string(),
    };
    if model.is_dirty() {
        t.push_str(" ●");
    }
    t
}

/// Send the title only when it changes.
fn set_title(ctx: &egui::Context, title: String) {
    let id = egui::Id::new("sdc-window-title");
    let same = ctx.data(|d| d.get_temp::<String>(id).is_some_and(|t| t == title));
    if !same {
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
        ctx.data_mut(|d| d.insert_temp(id, title));
    }
}

fn pick_folder() -> Option<PathBuf> {
    rfd::FileDialog::new().pick_folder()
}

pub fn draw(ui: &mut egui::Ui, model: &mut AppModel) {
    set_title(ui.ctx(), window_title(model));
    egui::Panel::top("header").show(ui, |ui| {
        ui.horizontal(|ui| {
            if ui.button("Open folder…").clicked() {
                if let Some(p) = pick_folder() {
                    model.request_open(p);
                }
            }
            recent_menu(ui, model);
            mode_combo(ui, model);
            if ui
                .toggle_value(&mut model.settings.columns_collapsed, "Collapse columns")
                .changed()
            {
                model.save_settings();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Not while `sdc.toml` is ignored (M3a spec §7.4): an export would not be the project's settings.
                let read_only = model.project.as_ref().is_some_and(|p| p.read_only());
                let can_export =
                    model.root.is_some() && model.export.running.is_none() && !read_only;
                if ui
                    .add_enabled(can_export, egui::Button::new("Export"))
                    .clicked()
                {
                    model.export_clicked();
                }
                if ui
                    .add_enabled(model.is_dirty(), egui::Button::new("Save"))
                    .clicked()
                {
                    // A failed save raises its own dialog.
                    let _ = model.save();
                }
                export_folder_button(ui, model);
                json_name_field(ui, model);
            });
        });
    });
    status(ui, model);
}

fn recent_menu(ui: &mut egui::Ui, model: &mut AppModel) {
    let mut open = None;
    ui.add_enabled_ui(!model.settings.recent.is_empty(), |ui| {
        ui.menu_button("Recent", |ui| {
            for p in &model.settings.recent {
                if ui.button(p.display().to_string()).clicked() {
                    open = Some(p.clone());
                    ui.close();
                }
            }
        });
    });
    if let Some(p) = open {
        model.request_open(p);
    }
}

fn mode_combo(ui: &mut egui::Ui, model: &mut AppModel) {
    ui.label("Mode");
    let mut picked = model.target;
    ui.add_enabled_ui(model.project.is_some(), |ui| {
        egui::ComboBox::from_id_salt("mode")
            .selected_text(target_name(model.target))
            .show_ui(ui, |ui| {
                for t in [Target::Cs, Target::Vanilla, Target::Cpm] {
                    ui.selectable_value(&mut picked, t, target_name(t));
                }
            });
    });
    if picked != model.target {
        model.set_target(picked);
    }
}

fn export_folder_button(ui: &mut egui::Ui, model: &mut AppModel) {
    // The folder an export would write to: the setting, else `<root>/sdc-out`.
    let shown = if model.settings.export_folder.is_some() || model.root.is_some() {
        model.export_root().display().to_string()
    } else {
        "not set".to_string()
    };
    if ui
        .button(format!("Export folder: {shown}"))
        .on_hover_text("Choose where converted textures are written")
        .clicked()
    {
        if let Some(p) = pick_folder() {
            model.settings.export_folder = Some(p);
            model.save_settings();
            // Existing outputs decide which textures are up to date.
            model.replan();
        }
    }
}

/// The PGPatcher JSON name (CS only, with a folder open). The default shows as hint text; empty restores it.
fn json_name_field(ui: &mut egui::Ui, model: &mut AppModel) {
    if model.target != Target::Cs || model.root.is_none() || model.sets_doc.is_none() {
        return;
    }
    // Right-to-left layout: the field is added first so the label ends up on its left.
    let id = egui::Id::new("json-name");
    let mut t = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| {
        model
            .sets_doc
            .as_ref()
            .and_then(|d| d.json_name())
            .unwrap_or_default()
    });
    let editable = model.sets_doc.as_ref().is_some_and(|d| !d.read_only());
    let hover = format!(
        "The PGPatcher JSON is written to {}\\PBRNIFPatcher\\<name>.json",
        model.export_root().display()
    );
    let r = ui
        .add_enabled(
            editable,
            egui::TextEdit::singleline(&mut t)
                .hint_text(crate::model::pbr_export::json_name(model))
                .desired_width(120.0),
        )
        .on_hover_text(hover);
    ui.label("JSON:");
    if r.changed() {
        ui.data_mut(|d| d.insert_temp(id, t.clone()));
    }
    if r.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(id));
        model.set_json_name(&t);
    }
}

fn status(ui: &mut egui::Ui, model: &mut AppModel) {
    if model.status_line.is_empty() && model.banner.is_none() {
        return;
    }
    egui::Panel::top("status").show(ui, |ui| {
        ui.label(&model.status_line);
        if let Some(b) = model.banner.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(WARNING, format!("⚠ {b}"));
                if model.project.is_some() && ui.button("Reload").clicked() {
                    model.reload_documents();
                }
            });
        }
    });
}
