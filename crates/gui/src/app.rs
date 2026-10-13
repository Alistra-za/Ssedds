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
    /// The Meshes view (plan A Task 12).
    pub meshes: crate::model::meshes::MeshesModel,
    pub state: crate::ui::State,
    /// The GPU state, `None` when eframe found no usable adapter (the preview is then off).
    pub render_state: Option<egui_wgpu::RenderState>,
    uploaded: Uploaded,
    was_focused: bool,
}

impl SdcApp {
    pub fn new(model: AppModel) -> SdcApp {
        let mut meshes = crate::model::meshes::MeshesModel::new(
            crate::model::meshes::default_store_path(),
            model.settings.meshes.clone(),
        );
        meshes.v3 =
            crate::model::viewport3d::Viewport3d::new(model.settings.render3d.shaders.clone());
        let set3d = crate::model::set_preview::SetPreview::from_settings(&model.settings);
        SdcApp {
            model,
            meshes,
            state: crate::ui::State {
                set3d,
                ..crate::ui::State::default()
            },
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
        self.meshes.relocate();
        self.state.set3d.relocate();
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
        // Undo and redo leave a text field's own undo alone and wait while a dialog is open.
        if self.model.dialog.is_none()
            && !ctx.egui_wants_keyboard_input()
            && !crate::ui::header::meshes_view_active(&ctx)
        {
            let redo = ctx.input_mut(|i| {
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::Y)
                    || i.consume_key(
                        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                        egui::Key::Z,
                    )
            });
            if redo {
                self.model.redo();
            } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z)) {
                self.model.undo();
            }
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
        let scanning = self.meshes.scanning();
        self.meshes.poll();
        // S-shader-10: a finished scan may change what the meshes report.
        if scanning && !self.meshes.scanning() && self.model.refresh_mesh_votes() {
            self.model.replan();
        }
        self.state.set3d.poll(&self.model);
        if crate::ui::header::meshes_view_active(&ctx) {
            self.meshes.activate();
            if let Some(req) = self.meshes.take_request() {
                self.meshes.start(req);
            }
            crate::ui::meshes::page(ui, &mut self.meshes);
            if let Some(s) = self.meshes.take_settings() {
                self.model.settings.meshes = s;
                self.model.save_settings();
            }
            if let Some(fp) = self.meshes.v3.take_shaders_changed() {
                self.model.settings.render3d.shaders = Some(fp);
                self.model.save_settings();
            }
            // The Textures model's dialogs (close with unsaved edits, an export error) must show in this view too.
            crate::ui::dialogs::draw(&ctx, &mut self.model);
        } else {
            crate::ui::draw_with(ui, &mut self.model, &mut self.state);
        }
        if let Some(fp) = self.state.set3d.v3.take_shaders_changed() {
            self.model.settings.render3d.shaders = Some(fp);
            self.model.save_settings();
        }

        // The 3D gap watches expect a next frame exactly when their panel asks for one (after drawing).
        self.meshes.v3.arm_gap(self.meshes.v3.busy());
        self.state.set3d.v3.arm_gap(self.state.set3d.busy());

        // Worker results arrive on a channel, not as input events: keep frames coming while one is pending.
        if self.model.busy()
            || self.meshes.scanning()
            || self.meshes.v3.busy()
            || self.state.set3d.busy()
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        if self.model.preview.scheduled() {
            ctx.request_repaint_after(crate::model::preview::DEBOUNCE);
        }
    }
}
