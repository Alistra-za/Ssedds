//! The expanded preview (M3a spec §6): the toolbar, two linked viewports (source left, encoded right, or one
//! viewport cut by a divider), the notes and readouts under them, and the compact settings strip beside them.
//! All state lives in the model (`AppModel::view`, `AppModel::preview`); this file maps input to model calls.
use crate::model::items::ItemKind;
use crate::model::preview::NO_GPU;
use crate::model::preview::{EncodedState, PreviewOut, SourceState};
use crate::model::view::{readouts, skipped_note, Channel, Layout, ViewState};
use crate::model::AppModel;
use crate::render::{self, Params, Slot, UploadPath};
use crate::ui::settings_panel;
use eframe::egui::{self, Align, Color32, Layout as UiLayout, Rect, RichText, Sense, UiBuilder};
use engine::config::Format;
use std::sync::Arc;

const SETTINGS_WIDTH: f32 = 280.0;
const GAP: f32 = 6.0;
const DIVIDER_GRAB: f32 = 10.0;
const PANE_BG: Color32 = Color32::from_gray(38);
const CHANNELS: [(Channel, &str); 6] = [
    (Channel::Rgba, "RGBA"),
    (Channel::R, "R"),
    (Channel::G, "G"),
    (Channel::B, "B"),
    (Channel::A, "A"),
    (Channel::AlphaMask, "α-mask"),
];

/// What `app.rs` has put on the GPU. Without a renderer (a test, or no usable adapter) the viewports draw a
/// placeholder with the state text instead.
#[derive(Default)]
pub struct GpuStatus {
    pub renderer: bool,
    pub source: bool,
    pub encoded: Option<UploadPath>,
    pub heatmap: bool,
}

/// Drawing state that is not part of the model.
#[derive(Default)]
pub struct PreviewUi {
    /// The item the view was last fitted for: a newly expanded texture starts fitted.
    fitted_for: Option<String>,
}

/// The facts about the expanded texture's mips that the toolbar, the viewports and the notes share.
struct Info {
    dims: Vec<(u32, u32)>,
    /// First previewed mip, and one past the last.
    first: usize,
    end: usize,
    format: Option<Format>,
    /// The result on show, and the source chain (None for a DDS input).
    shown: Option<Arc<PreviewOut>>,
    has_source: bool,
}

impl Info {
    fn new(model: &AppModel, key: &str) -> Info {
        let shown = model.preview.encoded.shown().cloned();
        let source = match &model.preview.source {
            SourceState::Ready(s) => Some(s.clone()),
            _ => None,
        };
        let job_format = model
            .item(key)
            .and_then(|i| i.job.as_ref())
            .map(|j| j.profile.format);
        let has_source = source.as_ref().is_some_and(|s| !s.levels.is_empty());
        let (dims, first, end) = match (&shown, &source) {
            (Some(o), _) => {
                let s = &o.result.source;
                (
                    s.dims.clone(),
                    s.first_level,
                    s.first_level + o.result.levels.len(),
                )
            }
            (None, Some(s)) if has_source => (
                s.dims.clone(),
                s.first_level,
                s.first_level + s.levels.len(),
            ),
            _ => (vec![], 0, 0),
        };
        Info {
            dims,
            first,
            end,
            format: shown.as_ref().map(|o| o.result.format).or(job_format),
            shown,
            has_source,
        }
    }

    fn known(&self) -> bool {
        self.end > self.first && self.end <= self.dims.len()
    }

    fn size_of(&self, mip: usize) -> (u32, u32) {
        self.dims.get(mip).copied().unwrap_or((1, 1))
    }

    /// The error heatmap needs the source the result was encoded from.
    fn heatmap_possible(&self) -> bool {
        self.shown
            .as_ref()
            .is_some_and(|o| !o.result.source.levels.is_empty())
    }
}

/// What the toolbar asks the viewports to do (they know the viewport size).
#[derive(Default)]
struct Requests {
    fit: bool,
    one_to_one: bool,
}

pub fn draw(ui: &mut egui::Ui, model: &mut AppModel, state: &mut PreviewUi, gpu: &GpuStatus) {
    let Some(key) = model.expanded.clone() else {
        return;
    };
    if escape_pressed(ui, model) {
        model.collapse();
        return;
    }
    let info = Info::new(model, &key);
    if info.known() {
        model.view.clamp_mip(info.first, info.end);
    }
    egui::Panel::right("preview-settings")
        .exact_size(SETTINGS_WIDTH)
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| settings_panel::draw(ui, model, &key, true));
        });
    let requests = egui::Panel::top("preview-toolbar")
        .show(ui, |ui| toolbar(ui, model, &info))
        .inner;
    egui::Panel::bottom("preview-notes").show(ui, |ui| notes(ui, model, &info, gpu));
    let rect = ui.available_rect_before_wrap();
    if viewports(ui, model, &info, gpu, rect, requests, state, &key) {
        model.collapse();
    }
}

/// Esc collapses, unless something else wants it: a dialog, a popup, or a text field being edited.
fn escape_pressed(ui: &mut egui::Ui, model: &AppModel) -> bool {
    let ctx = ui.ctx().clone();
    model.dialog.is_none()
        && !egui::Popup::is_any_open(&ctx)
        && !ctx.egui_wants_keyboard_input()
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
}

fn toolbar(ui: &mut egui::Ui, model: &mut AppModel, info: &Info) -> Requests {
    let mut requests = Requests::default();
    ui.horizontal_wrapped(|ui| {
        for (channel, name) in CHANNELS {
            if ui
                .selectable_label(model.view.channel == channel, name)
                .clicked()
            {
                model.view.channel = channel;
            }
        }
        ui.separator();
        mip_control(ui, &mut model.view, info);
        ui.separator();
        // BC6H is tone-mapped for display: the sRGB / UNORM choice does not apply to it.
        let bc6h = info.format == Some(Format::Bc6hUf16);
        ui.add_enabled_ui(!bc6h, |ui| {
            let linear = model.view.view_linear;
            if ui.selectable_label(!linear, "sRGB").clicked() {
                model.view.view_linear = false;
            }
            if ui.selectable_label(linear, "UNORM").clicked() {
                model.view.view_linear = true;
            }
        })
        .response
        .on_disabled_hover_text("BC6H is tone-mapped for display");
        ui.separator();
        let layout = model.view.layout;
        if ui
            .selectable_label(layout == Layout::SideBySide, "Side by side")
            .clicked()
        {
            model.view.layout = Layout::SideBySide;
        }
        if ui
            .selectable_label(layout == Layout::Split, "Split")
            .clicked()
        {
            model.view.layout = Layout::Split;
        }
        let possible = info.heatmap_possible();
        ui.add_enabled_ui(possible, |ui| {
            ui.toggle_value(&mut model.view.heatmap, "Heatmap")
                .on_disabled_hover_text("Needs an encoded result with a source image");
        });
        ui.separator();
        requests.fit = ui.button("Fit").clicked();
        requests.one_to_one = ui.button("1:1").clicked();
        ui.separator();
        let mut live = model.preview.live;
        if ui.checkbox(&mut live, "Live encode").changed() {
            model.set_live(live);
        }
        if encode_button(ui, model) {
            model.encode_now();
        }
    });
    requests
}

/// The mip label (`mip k (w×h)`) and, when there is a choice, the slider.
fn mip_control(ui: &mut egui::Ui, view: &mut ViewState, info: &Info) {
    if !info.known() {
        ui.weak("no mips yet");
        return;
    }
    let (w, h) = info.size_of(view.mip);
    ui.label(format!("mip {} ({w}×{h})", view.mip));
    if info.end - info.first > 1 {
        let mut mip = view.mip;
        let r = ui.add(egui::Slider::new(&mut mip, info.first..=info.end - 1).show_value(false));
        if r.changed() {
            view.set_mip(mip, info.size_of(view.mip), info.size_of(mip));
        }
    }
}

/// True when the button was pressed. Present (disabled) even when nothing can be encoded, so the toolbar keeps its shape.
fn encode_button(ui: &mut egui::Ui, model: &AppModel) -> bool {
    ui.add_enabled(can_encode(model), egui::Button::new("Encode now"))
        .clicked()
}

fn can_encode(model: &AppModel) -> bool {
    if !model.gpu_available {
        return false;
    }
    let encoding = matches!(model.preview.encoded, EncodedState::Encoding { .. });
    let job = model
        .expanded
        .as_deref()
        .and_then(|k| model.item(k))
        .is_some_and(|i| i.kind != ItemKind::Dds && i.job.is_some());
    job && !encoding && matches!(model.preview.source, SourceState::Ready(_))
}

fn notes(ui: &mut egui::Ui, model: &AppModel, info: &Info, gpu: &GpuStatus) {
    let mut lines: Vec<String> = vec![];
    lines.extend(skipped_note(&info.dims, info.first));
    if let Some(UploadPath::CpuDecoded(why) | UploadPath::Skipped(why)) = gpu.encoded {
        lines.push(why.to_string());
    }
    if let Some(notice) = &model.preview.notice {
        lines.extend(notice.lines().map(str::to_string));
    }
    for line in lines {
        ui.label(RichText::new(line).small());
    }
    let Some(out) = &info.shown else {
        return;
    };
    ui.horizontal_wrapped(|ui| {
        for (name, value) in readouts(out, model.view.mip).lines {
            ui.label(RichText::new(name).weak());
            ui.label(value);
            ui.add_space(8.0);
        }
    });
}

/// Draws the two viewports and handles their input. True when the preview should collapse (a double-click).
#[allow(clippy::too_many_arguments)]
fn viewports(
    ui: &mut egui::Ui,
    model: &mut AppModel,
    info: &Info,
    gpu: &GpuStatus,
    rect: Rect,
    requests: Requests,
    state: &mut PreviewUi,
    key: &str,
) -> bool {
    let split = model.view.layout == Layout::Split;
    let half = ((rect.width() - GAP) / 2.0).max(1.0);
    let (left, right) = if split {
        (rect, rect)
    } else {
        (
            Rect::from_min_size(rect.min, egui::vec2(half, rect.height())),
            Rect::from_min_size(
                egui::pos2(rect.min.x + half + GAP, rect.min.y),
                egui::vec2(half, rect.height()),
            ),
        )
    };
    let divider_x = rect.min.x + rect.width() * model.view.split;
    let responses = if split {
        vec![ui.allocate_rect(rect, Sense::click_and_drag())]
    } else {
        vec![
            ui.allocate_rect(left, Sense::click_and_drag()),
            ui.allocate_rect(right, Sense::click_and_drag()),
        ]
    };
    // The divider is added after the viewport so that it wins the pointer where they overlap.
    let divider = split.then(|| {
        let grab = Rect::from_center_size(
            egui::pos2(divider_x, rect.center().y),
            egui::vec2(DIVIDER_GRAB, rect.height()),
        );
        ui.interact(grab, ui.id().with("preview-divider"), Sense::drag())
    });

    let size = [left.width(), left.height()];
    let mip = model.view.mip;
    let mip_size = info.size_of(mip);
    if info.known() {
        let first_time = state.fitted_for.as_deref() != Some(key);
        if first_time || requests.fit {
            model.view.fit(size, mip_size);
            state.fitted_for = Some(key.to_string());
        } else if requests.one_to_one {
            model.view.one_to_one();
        }
        interact(ui, &mut model.view, &responses, size, mip_size);
    }
    if let Some(d) = &divider {
        if d.dragged() {
            let dx = d.drag_delta().x / rect.width().max(1.0);
            model.view.split = (model.view.split + dx).clamp(0.02, 0.98);
        }
        if d.hovered() || d.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
    }

    let divider_x = rect.min.x + rect.width() * model.view.split;
    let (left_vis, right_vis) = if split {
        (
            Rect::from_min_max(rect.min, egui::pos2(divider_x, rect.max.y)),
            Rect::from_min_max(egui::pos2(divider_x, rect.min.y), rect.max),
        )
    } else {
        (left, right)
    };
    let mut encode = false;
    draw_source_pane(ui, model, info, gpu, left, left_vis, size);
    draw_encoded_pane(ui, model, info, gpu, right, right_vis, size, &mut encode);
    if split {
        ui.painter().vline(
            divider_x,
            rect.y_range(),
            egui::Stroke::new(1.5, Color32::from_gray(200)),
        );
    }
    if encode {
        model.encode_now();
    }
    responses.iter().any(|r| r.double_clicked())
}

/// Scroll or pinch zooms about the pointer; dragging pans. Both viewports share the one view.
fn interact(
    ui: &egui::Ui,
    view: &mut ViewState,
    responses: &[egui::Response],
    size: [f32; 2],
    mip_size: (u32, u32),
) {
    for r in responses {
        if r.dragged_by(egui::PointerButton::Primary) {
            let d = r.drag_delta();
            view.pan_pixels([d.x, d.y], size, mip_size);
        }
        if !r.hovered() {
            continue;
        }
        let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta().y, i.zoom_delta()));
        let factor = pinch * (scroll * 0.002).exp();
        let Some(p) = r.hover_pos().filter(|_| (factor - 1.0).abs() > 1e-4) else {
            continue;
        };
        let (offset, scale) = view.uv_rect(size, mip_size);
        let s = [
            (p.x - r.rect.min.x) / r.rect.width().max(1.0),
            (p.y - r.rect.min.y) / r.rect.height().max(1.0),
        ];
        let cursor_uv = [offset[0] + s[0] * scale[0], offset[1] + s[1] * scale[1]];
        view.zoom_at(factor, cursor_uv);
    }
}

fn params_for(
    view: &ViewState,
    size: [f32; 2],
    mip_size: (u32, u32),
    mip_in_texture: usize,
    tonemap: bool,
) -> Params {
    let (uv_offset, uv_scale) = view.uv_rect(size, mip_size);
    Params {
        uv_offset,
        uv_scale,
        mip: mip_in_texture as f32,
        channel: view.channel.shader_id(),
        // The tone map already ends in sRGB; the two are never applied together.
        view_linear: (view.view_linear && !tonemap) as u32,
        tonemap: tonemap as u32,
    }
}

/// Draws a slot into `viewport`, showing only `visible` of it.
fn paint_clipped(ui: &mut egui::Ui, viewport: Rect, visible: Rect, slot: Slot, params: Params) {
    ui.scope(|ui| {
        ui.set_clip_rect(visible.intersect(ui.clip_rect()));
        render::paint(ui, viewport, slot, params);
    });
}

fn draw_source_pane(
    ui: &mut egui::Ui,
    model: &AppModel,
    info: &Info,
    gpu: &GpuStatus,
    viewport: Rect,
    visible: Rect,
    size: [f32; 2],
) {
    ui.painter().rect_filled(visible, 0.0, PANE_BG);
    let text = match &model.preview.source {
        SourceState::Loading => Some("Loading source…".to_string()),
        SourceState::Failed(e) => Some(e.clone()),
        SourceState::Ready(_) if !info.has_source => Some("No source image".to_string()),
        SourceState::Ready(_) if !gpu.renderer => Some("No GPU preview".to_string()),
        SourceState::Ready(_) if !gpu.source => {
            Some("This source is too large for this GPU".to_string())
        }
        SourceState::Ready(_) => None,
    };
    if let (None, SourceState::Ready(s)) = (&text, &model.preview.source) {
        let mip = model.view.mip;
        let tonemap = info.format == Some(Format::Bc6hUf16);
        let params = params_for(
            &model.view,
            size,
            info.size_of(mip),
            mip.saturating_sub(s.first_level),
            tonemap,
        );
        paint_clipped(ui, viewport, visible, Slot::Source, params);
    }
    pane_overlay(ui, visible, "Source", None, text.as_deref(), false);
}

#[allow(clippy::too_many_arguments)]
fn draw_encoded_pane(
    ui: &mut egui::Ui,
    model: &AppModel,
    info: &Info,
    gpu: &GpuStatus,
    viewport: Rect,
    visible: Rect,
    size: [f32; 2],
    encode: &mut bool,
) {
    ui.painter().rect_filled(visible, 0.0, PANE_BG);
    let heat = model.view.heatmap && gpu.heatmap;
    let (badge, text, offer_encode) = match &model.preview.encoded {
        EncodedState::NotEncoded => (None, Some("Not encoded".to_string()), true),
        EncodedState::Encoding { previous } => match previous {
            Some(_) => (Some("encoding…"), None, false),
            None => (None, Some("encoding…".to_string()), false),
        },
        EncodedState::Stale(_) => (Some("stale"), None, false),
        EncodedState::Done(_) => (None, None, false),
        EncodedState::Failed(e) => (None, Some(e.clone()), false),
    };
    let uploaded = matches!(
        gpu.encoded,
        Some(UploadPath::Bc | UploadPath::CpuDecoded(_))
    );
    let mut text = text;
    let mut offer_encode = offer_encode;
    if !model.gpu_available {
        text = Some(NO_GPU.to_string());
        offer_encode = false;
    }
    if text.is_none() && info.shown.is_some() {
        if !gpu.renderer {
            text = Some("No GPU preview".to_string());
        } else if !uploaded && !heat {
            text = Some("No preview of this result".to_string());
        }
    }
    let mut drawn = false;
    if gpu.renderer && info.shown.is_some() {
        let mip = model.view.mip;
        if heat {
            let mut params = params_for(&model.view, size, info.size_of(mip), 0, false);
            params.channel = 0;
            params.view_linear = 0;
            paint_clipped(ui, viewport, visible, Slot::Heatmap, params);
            drawn = true;
        } else if let (true, Some(out)) = (uploaded, &info.shown) {
            let tonemap = out.result.format == Format::Bc6hUf16;
            let first = out.result.source.first_level;
            let params = params_for(
                &model.view,
                size,
                info.size_of(mip),
                mip.saturating_sub(first),
                tonemap,
            );
            paint_clipped(ui, viewport, visible, Slot::Encoded, params);
            drawn = true;
        }
    }
    if drawn && matches!(model.preview.encoded, EncodedState::Encoding { .. }) {
        ui.painter()
            .rect_filled(visible, 0.0, Color32::from_black_alpha(128));
    }
    let title = if heat { "Difference" } else { "Encoded" };
    let button = offer_encode && can_encode(model);
    if pane_overlay(ui, visible, title, badge, text.as_deref(), button) {
        *encode = true;
    }
}

/// The pane's title and badge in its corner and, when there is nothing to draw, a centred message (with an
/// **Encode now** button when asked). True when that button was pressed.
fn pane_overlay(
    ui: &mut egui::Ui,
    visible: Rect,
    title: &str,
    badge: Option<&str>,
    message: Option<&str>,
    button: bool,
) -> bool {
    let corner = Rect::from_min_size(
        visible.min + egui::vec2(6.0, 4.0),
        egui::vec2(visible.width(), 20.0),
    );
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(corner)
            .layout(UiLayout::left_to_right(Align::Center)),
        |ui| {
            ui.label(RichText::new(title).small().strong());
            if let Some(b) = badge {
                ui.label(
                    RichText::new(b)
                        .small()
                        .color(super::columns::AMBER)
                        .background_color(Color32::from_black_alpha(120)),
                );
            }
        },
    );
    let mut pressed = false;
    if message.is_some() || button {
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(visible.shrink(12.0))
                .layout(UiLayout::top_down(Align::Center)),
            |ui| {
                ui.add_space(visible.height() * 0.35);
                if let Some(m) = message {
                    ui.label(m);
                }
                if button {
                    pressed = ui.button("Encode now").clicked();
                }
            },
        );
    }
    pressed
}
