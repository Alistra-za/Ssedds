//! The PGPatcher settings column of the set editor (filled in by M3b Task 15).
use crate::model::AppModel;
use eframe::egui;

pub fn panel(ui: &mut egui::Ui, _model: &mut AppModel, _set: &str) {
    ui.label("PGPatcher settings");
}
