//! The eframe application: owns the model, draws it each frame.
use crate::model::AppModel;
use crate::render::PreviewRenderer;
use eframe::{egui, egui_wgpu};

pub struct SdcApp {
    pub model: AppModel,
    pub state: crate::ui::State,
    /// The GPU state, `None` when eframe found no usable adapter (the preview is then off).
    pub render_state: Option<egui_wgpu::RenderState>,
    was_focused: bool,
}

impl SdcApp {
    pub fn new(model: AppModel) -> SdcApp {
        SdcApp {
            model,
            state: crate::ui::State::default(),
            render_state: None,
            was_focused: true,
        }
    }

    /// Creates the preview renderer in egui_wgpu's callback resources. Without a render state (no usable adapter) the
    /// app runs without a preview.
    pub fn with_render_state(mut self, rs: Option<&egui_wgpu::RenderState>) -> SdcApp {
        if let Some(rs) = rs {
            let renderer = PreviewRenderer::new(rs);
            rs.renderer.write().callback_resources.insert(renderer);
            self.render_state = Some(rs.clone());
        }
        self
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
        self.model.tick(std::time::Instant::now());

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
        if self.model.preview.scheduled() {
            ctx.request_repaint_after(crate::model::preview::DEBOUNCE);
        }
    }
}
