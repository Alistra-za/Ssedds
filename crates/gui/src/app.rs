//! The eframe application: owns the model, draws it each frame.
use crate::model::preview::{PreviewOut, SourceState};
use crate::model::view::{self, Channel};
use crate::model::AppModel;
use crate::render::{PreviewRenderer, Slot};
use eframe::{egui, egui_wgpu};
use engine::session::SourcePreview;
use std::sync::Arc;

/// The model values currently on the GPU. A texture is uploaded again only when its `Arc` (or the mip and channel
/// of a heatmap) changes.
#[derive(Default)]
struct Uploaded {
    source: Option<Arc<SourcePreview>>,
    encoded: Option<Arc<PreviewOut>>,
    heatmap: Option<(Arc<PreviewOut>, usize, Channel)>,
}

fn same<T>(a: &Option<Arc<T>>, b: &Option<Arc<T>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

pub struct SdcApp {
    pub model: AppModel,
    pub state: crate::ui::State,
    /// The GPU state, `None` when eframe found no usable adapter (the preview is then off).
    pub render_state: Option<egui_wgpu::RenderState>,
    uploaded: Uploaded,
    was_focused: bool,
}

impl SdcApp {
    pub fn new(model: AppModel) -> SdcApp {
        SdcApp {
            model,
            state: crate::ui::State::default(),
            render_state: None,
            uploaded: Uploaded::default(),
            was_focused: true,
        }
    }

    /// Creates the preview renderer in egui_wgpu's callback resources. Without a render state (no usable adapter) the
    /// app runs without a preview.
    pub fn with_render_state(mut self, rs: Option<&egui_wgpu::RenderState>) -> SdcApp {
        if let Some(rs) = rs {
            let renderer = PreviewRenderer::new(rs);
            self.model.gpu_max_dim = renderer.max_dim();
            rs.renderer.write().callback_resources.insert(renderer);
            self.state.gpu.renderer = true;
            self.render_state = Some(rs.clone());
        } else {
            self.model.gpu_available = false;
        }
        self
    }

    /// Puts what the model shows on the GPU, when it changed since the last frame.
    fn sync_renderer(&mut self) {
        let Some(rs) = self.render_state.clone() else {
            return;
        };
        let expanded = self.model.expanded.is_some();
        let want_source = match &self.model.preview.source {
            SourceState::Ready(s) if expanded => Some(s.clone()),
            _ => None,
        };
        let want_encoded = self
            .model
            .preview
            .encoded
            .shown()
            .filter(|_| expanded)
            .cloned();
        let (mip, channel) = (self.model.view.mip, self.model.view.channel);
        let want_heatmap = want_encoded
            .clone()
            .filter(|_| self.model.view.heatmap)
            .map(|o| (o, mip, channel));

        let mut resources = rs.renderer.write();
        let Some(renderer) = resources.callback_resources.get_mut::<PreviewRenderer>() else {
            return;
        };
        let gpu = &mut self.state.gpu;
        if !same(&self.uploaded.source, &want_source) {
            gpu.source = match &want_source {
                Some(s) => renderer.set_source(&rs, s),
                None => {
                    renderer.clear(Slot::Source);
                    false
                }
            };
            self.uploaded.source = want_source;
        }
        if !same(&self.uploaded.encoded, &want_encoded) {
            gpu.encoded = match &want_encoded {
                Some(o) => Some(renderer.set_encoded(&rs, o)),
                None => {
                    renderer.clear(Slot::Encoded);
                    None
                }
            };
            self.uploaded.encoded = want_encoded;
        }
        let heatmap_unchanged = match (&self.uploaded.heatmap, &want_heatmap) {
            (Some((a, m, c)), Some((b, n, d))) => Arc::ptr_eq(a, b) && m == n && c == d,
            (None, None) => true,
            _ => false,
        };
        if !heatmap_unchanged {
            gpu.heatmap = match &want_heatmap {
                Some((o, mip, channel)) => match view::heatmap_of(o, *mip, *channel) {
                    Some((w, h, rgba)) => renderer.set_heatmap(&rs, w, h, &rgba),
                    None => {
                        renderer.clear(Slot::Heatmap);
                        false
                    }
                },
                None => {
                    renderer.clear(Slot::Heatmap);
                    false
                }
            };
            self.uploaded.heatmap = want_heatmap;
        }
    }

    /// The window regained focus: the place to look for source files that changed meanwhile.
    fn focus_regained(&mut self) {
        self.model.check_sources();
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
        // The export that was in the way of a close has ended; close again (unsaved edits still ask).
        if self.model.take_close_ready() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // Folders (or files, which stand for their folder) dropped on the window open like Open folder (spec §5.1).
        let dropped: Vec<std::path::PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if !dropped.is_empty() {
            self.model.dropped(&dropped);
        }
        let focused = ctx.input(|i| i.focused);
        if focused && !self.was_focused {
            self.focus_regained();
        }
        self.was_focused = focused;

        // Upload before drawing: the paint callbacks run after this frame's UI and read what is on the GPU.
        self.sync_renderer();
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
