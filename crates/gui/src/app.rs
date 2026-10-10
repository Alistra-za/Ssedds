//! The eframe application: owns the model, draws it each frame.
use crate::model::AppModel;
use eframe::egui;

pub struct SdcApp {
    pub model: AppModel,
    pub state: crate::ui::State,
    was_focused: bool,
}

impl SdcApp {
    pub fn new(model: AppModel) -> SdcApp {
        SdcApp {
            model,
            state: crate::ui::State::default(),
            was_focused: true,
        }
    }

    /// The window regained focus: the place to look for source files that changed meanwhile.
    fn focus_regained(&mut self) {
        // `AppModel::check_sources` arrives with the source-change task; nothing to do until then.
    }
}

impl eframe::App for SdcApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.model.poll();

        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S)) {
            // A failed save raises its own dialog.
            let _ = self.model.save();
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.model.request_close() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        let focused = ctx.input(|i| i.viewport().focused).unwrap_or(true);
        if focused && !self.was_focused {
            self.focus_regained();
        }
        self.was_focused = focused;

        crate::ui::draw_with(ui, &mut self.model, &mut self.state);

        // Worker results arrive on a channel, not as input events: keep frames coming while one is pending.
        if self.model.busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }
}
