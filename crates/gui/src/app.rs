//! The eframe application: owns the model, draws it each frame.
use crate::model::AppModel;
use eframe::egui;

pub struct SdcApp {
    pub model: AppModel,
}

impl eframe::App for SdcApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        crate::ui::draw(ui, &mut self.model);
    }
}
