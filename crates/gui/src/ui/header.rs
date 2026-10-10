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
                    model.open_folder(p);
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
                // Not wired yet: exporting arrives with the export task.
                ui.add_enabled(false, egui::Button::new("Export"));
                if ui
                    .add_enabled(model.is_dirty(), egui::Button::new("Save"))
                    .clicked()
                {
                    // A failed save raises its own dialog.
                    let _ = model.save();
                }
                export_folder_button(ui, model);
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
        model.open_folder(p);
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
    let shown = match &model.settings.export_folder {
        Some(p) => p.display().to_string(),
        None => "not set".to_string(),
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
                    model.resolve_external_change(false);
                }
            });
        }
    });
}
