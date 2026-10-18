//! The eframe application: owns the model, draws it each frame.
use crate::model::heatmap::HeatImage;
use crate::model::preview::{PreviewOut, SourceState};
use crate::model::viewport3d::Look;
use crate::model::AppModel;
use crate::render::{PreviewRenderer, Slot};
use eframe::{egui, egui_wgpu};
use engine::session::SourcePreview;
use std::sync::Arc;

/// The model values currently on the GPU. A texture is uploaded again only when its `Arc` changes.
#[derive(Default)]
struct Uploaded {
    source: Option<Arc<SourcePreview>>,
    encoded: Option<Arc<PreviewOut>>,
    heatmap: Option<Arc<HeatImage>>,
    pinned: Option<Arc<PreviewOut>>,
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
    /// Waits out 2 s after the layout changed before `gui.toml` is written (AG19).
    layout_save: crate::layout::SaveDebounce,
    /// What the layout looked like last frame; `None` before the first.
    layout_probe: Option<crate::layout::Probe>,
}

impl SdcApp {
    pub fn new(mut model: AppModel) -> SdcApp {
        model.refresh_known_paths();
        let mut meshes = crate::model::meshes::MeshesModel::new(
            model.scan_db.clone(),
            model.settings.meshes.clone(),
        );
        meshes.v3 =
            crate::model::viewport3d::Viewport3d::new(model.settings.render3d.shaders.clone());
        let mut set3d = crate::model::set_preview::SetPreview::from_settings(
            &model.settings,
            model.scan_db.clone(),
        );
        // The saved arrangement: filters, preview channel and layout, and both 3D looks (AG17).
        let layout = model.settings.layout.clone();
        crate::layout::restore_model(
            &layout,
            &mut model.filter,
            &mut model.prefs,
            &mut model.view,
        );
        meshes.v3.look = Look::from_settings(layout.look.as_ref());
        set3d.v3.look = Look::from_settings(layout.set_look.as_ref());
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
            layout_save: Default::default(),
            layout_probe: None,
        }
    }

    /// Puts the saved panel widths and the Textures | Meshes view into egui, before the first frame.
    pub fn restore_ctx(&self, ctx: &egui::Context) {
        crate::layout::restore_ctx(ctx, &self.model.settings.layout);
    }

    /// Takes the window and arrangement as they are now into the settings. `gui.toml` is written once the change has
    /// waited 2 s from its first change (AG19), and at exit.
    pub fn sync_layout_at(&mut self, ctx: &egui::Context, now: std::time::Instant) {
        let looks = (&self.meshes.v3.look, &self.state.set3d.v3.look);
        let probe = crate::layout::probe(
            ctx,
            &self.model.filter,
            &self.model.prefs,
            &self.model.view,
            looks,
        );
        if self.layout_probe.as_ref() != Some(&probe) {
            let cur = crate::layout::capture(
                ctx,
                &self.model.settings.layout,
                &self.model.filter,
                &self.model.prefs,
                &self.model.view,
                looks,
            );
            // The first frame is the starting point: what it reads that the file did not have is no change to write.
            if cur != self.model.settings.layout {
                self.model.settings.layout = cur;
                if self.layout_probe.is_some() {
                    self.layout_save.changed(now);
                }
            }
            self.layout_probe = Some(probe);
        }
        if self.layout_save.due(now) {
            self.model.save_settings();
        } else if self.layout_save.pending() {
            ctx.request_repaint_after(crate::layout::SaveDebounce::DELAY);
        }
    }

    /// Both 3D panels' links to the Textures view: which missing texture paths are items of the open folder, and a
    /// "Open in Textures" click (selects the item and switches to the Textures view).
    pub fn route_3d(&mut self, ctx: &egui::Context) {
        let model = &mut self.model;
        for v in [&mut self.meshes.v3, &mut self.state.set3d.v3] {
            let missing = v
                .report
                .as_ref()
                .map_or(&[][..], |r| &r.missing_textures[..]);
            v.missing_items = missing
                .iter()
                .take(crate::ui::viewport3d::MAX_LISTED)
                .map(|p| model.item_for_game_path(p))
                .collect();
            if let Some(k) = v.show_in_textures.take() {
                crate::ui::header::set_meshes_view(ctx, false);
                model.palette_pick(&crate::model::nav::Pick::Texture(k));
            }
        }
        // Item 9: the Meshes view's texture paths, looked up again only when they or the items change.
        let m = &mut self.meshes;
        let listed = (m.listed_textures(), model.items_rev());
        if listed != m.folder_items_for {
            m.folder_items = listed
                .0
                .iter()
                .filter_map(|p| Some((p.clone(), model.item_for_game_path(p)?)))
                .collect();
            m.folder_items_for = listed;
        }
        if let Some(k) = m.open_in_textures.take() {
            crate::ui::header::set_meshes_view(ctx, false);
            model.palette_pick(&crate::model::nav::Pick::Texture(k));
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
        // Made off the UI thread (`heatmap_frame`); the one on show stays until the next lands.
        let want_heatmap = self
            .model
            .heatmap_image()
            .filter(|_| expanded && self.model.view.heatmap);

        // The pinned encode only goes to the GPU while it is compared; unpinning releases the texture.
        let want_pinned = self
            .model
            .compared_pin()
            .filter(|_| expanded)
            .map(|p| p.out.clone());

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
        if !same(&self.uploaded.pinned, &want_pinned) {
            gpu.pinned = match &want_pinned {
                Some(o) => Some(renderer.set_pinned(&rs, o)),
                None => {
                    renderer.clear(Slot::Pinned);
                    None
                }
            };
            self.uploaded.pinned = want_pinned;
        }
        if !same(&self.uploaded.heatmap, &want_heatmap) {
            gpu.heatmap = match &want_heatmap {
                Some(i) => renderer.set_heatmap(&rs, i.width, i.height, &i.rgba),
                None => {
                    renderer.clear(Slot::Heatmap);
                    false
                }
            };
            self.uploaded.heatmap = want_heatmap;
        }
    }

    /// A scan ended: a finished one toasts "Scan finished: <summary>"; a paused or cancelled one repeats its note;
    /// a failed one has its dialog.
    fn scan_ended(&mut self) {
        use crate::model::messages::Severity;
        let m = &self.meshes;
        if m.error.is_some() || m.status.is_empty() {
            return;
        }
        let (sev, text) = if m.status.starts_with("Scan ") {
            (Severity::Warning, m.status.clone())
        } else {
            (Severity::Info, format!("Scan finished: {}", m.status))
        };
        self.model.messages.notify(sev, text);
    }

    /// The window regained focus: the place to look for source files that changed meanwhile.
    fn focus_regained(&mut self) {
        self.model.check_sources();
        self.model.refresh_nvtt_if_changed();
        self.meshes.relocate();
        self.state.set3d.relocate();
    }
}

impl eframe::App for SdcApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.model.poll();
        let now = std::time::Instant::now();
        self.model.tick(now);
        if let Some(left) = self.model.undo_note_left(now) {
            ctx.request_repaint_after(left);
        }

        if ctx.input(|i| i.viewport().close_requested()) && !self.model.request_close() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        // The export that was in the way of a close has ended; close again (unsaved edits still ask).
        if self.model.take_close_ready() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // Folders (or files, which stand for their folder) dropped on the Meshes view open like Open folder (spec
        // §5.1). The Textures view handles its drops in `ui::draw_with_scan` (a slot card may take them, item 17).
        let dropped: Vec<std::path::PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if !dropped.is_empty() && crate::ui::header::meshes_view_active(&ctx) {
            self.model.dropped(&dropped);
        }
        let focused = ctx.input(|i| i.focused);
        if focused && !self.was_focused {
            self.focus_regained();
        }
        self.was_focused = focused;

        // Upload before drawing: the paint callbacks run after this frame's UI and read what is on the GPU.
        self.model.heatmap_frame();
        self.sync_renderer();
        let scanning = self.meshes.scanning();
        self.meshes.poll();
        // S-shader-10: a finished scan may change what the meshes report (re-plans when the evidence changed).
        // Trace-debug #41: after a cancel or failure the store reads empty; keep the evidence on screen.
        if scanning && !self.meshes.scanning() {
            self.scan_ended();
            if self.meshes.scan_completed {
                self.model.refresh_mesh_votes(false);
                self.model.refresh_known_paths();
            }
        }
        self.state.set3d.poll(&self.model);
        // Both views offer the scan (the Textures view has its row under the header).
        self.meshes.activate();
        if let Some(req) = self.meshes.take_request() {
            self.meshes.start(req);
        }
        self.route_3d(&ctx);
        if crate::ui::header::meshes_view_active(&ctx) {
            // The Textures view reads its keys in `ui::draw_with_scan`.
            crate::ui::actions::keys(&ctx, &mut self.model, self.state.drops.pending.is_some());
            crate::ui::meshes::page(ui, &mut self.meshes);
            if let Some(fp) = self.meshes.v3.take_shaders_changed() {
                self.model.settings.render3d.shaders = Some(fp);
                self.model.save_settings();
            }
            // The Textures model's dialogs (close with unsaved edits, an export error), the shortcut sheet and the palette
            // must show in this view too.
            crate::ui::dialogs::draw(&ctx, &mut self.model);
            crate::ui::presets::window(&ctx, &mut self.model);
        } else {
            crate::ui::draw_with_scan(ui, &mut self.model, &mut self.state, Some(&mut self.meshes));
        }
        // A "Open in Textures" click of this frame.
        self.route_3d(&ctx);
        for v3 in [&mut self.meshes.v3, &mut self.state.set3d.v3] {
            for (ok, text) in v3.take_notices() {
                let sev = if ok {
                    crate::model::messages::Severity::Info
                } else {
                    crate::model::messages::Severity::Warning
                };
                self.model.messages.notify(sev, text);
            }
        }
        crate::ui::messages::draw(&ctx, &mut self.model);
        // Explorer requests the drawer and the menus queued this frame (Task 5).
        for req in self.model.take_shell() {
            crate::model::shell::spawn(&req);
        }
        if let Some(s) = self.meshes.take_settings() {
            self.model.settings.meshes = s;
            self.model.save_settings();
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
            || self.model.heatmap_pending()
            || self.model.rnm_preview_pending()
            || self.model.dims_pending()
            || self.model.used_by_pending()
            || self.model.report_saving
            || self.model.picker.busy()
            || self.meshes.scanning()
            || self.meshes.v3.busy()
            || self.state.set3d.busy()
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        if self.model.preview.scheduled() {
            ctx.request_repaint_after(crate::model::preview::DEBOUNCE);
        }
        self.sync_layout_at(&ctx, std::time::Instant::now());
        // Last, so the panel paint that follows (where a lost device panics) has this frame's edits (Note AR).
        if let Some(files) = self.model.recovery_changed() {
            crate::crash_log::set_recovery(files);
        }
    }

    /// AG124: the panel fill, so a strip no panel paints never shows as a black bar (eframe's default is near black).
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }

    /// The last write: a layout change still inside its 2 s wait is not lost.
    fn on_exit(&mut self) {
        if self.layout_save.pending() {
            self.model.save_settings();
        }
    }
}
