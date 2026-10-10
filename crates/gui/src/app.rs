//! The eframe application: owns the model, draws it each frame.
use crate::model::AppModel;
use eframe::egui;

pub struct SdcApp {
    pub model: AppModel,
}

impl eframe::App for SdcApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.model.poll();
        crate::ui::draw(ui, &mut self.model);
        // Worker results arrive on a channel, not as input events: keep frames coming while one is pending.
        if self.model.busy() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(50));
        }
    }
}
