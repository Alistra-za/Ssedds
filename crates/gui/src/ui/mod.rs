//! egui drawing of the model (M3a spec §2). No logic beyond mapping input to model calls.
use crate::model::AppModel;
use eframe::egui;

pub fn draw(ui: &mut egui::Ui, model: &mut AppModel) {
    egui::Panel::top("header").show(ui, |ui| {
        ui.horizontal(|ui| {
            let _ = ui.button("Open folder…");
            ui.label("Mode");
            ui.separator();
            ui.checkbox(&mut model.settings.columns_collapsed, "Collapse columns");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_enabled(false, egui::Button::new("Export"));
                ui.label("Export folder: not set");
            });
        });
    });
    egui::Panel::right("advanced")
        .resizable(false)
        .show(ui, |ui| {
            ui.toggle_value(&mut model.settings.advanced_open, "Advanced");
        });
    egui::Panel::left("textures").show(ui, |ui| {
        ui.heading("Textures");
    });
    egui::Panel::left("sets").show(ui, |ui| {
        ui.heading("Sets");
    });
    egui::CentralPanel::default().show(ui, |ui| {
        ui.centered_and_justified(|ui| ui.label("Open a folder of source textures to begin."));
    });
}
