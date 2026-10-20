//! The expanded preview (M3a spec §6): the toolbar, two linked viewports (source left, encoded right, or one
//! viewport cut by a divider), the notes and readouts under them, and the compact settings strip beside them.
//! All state lives in the model (`AppModel::view`, `AppModel::preview`); this file maps input to model calls.
use crate::model::items::ItemKind;
use crate::model::overlay::OverlayKind;
use crate::model::preview::NO_GPU;
use crate::model::preview::{EncodedState, PreviewOut, SourceState};
use crate::model::view::{self, readouts, skipped_note, Channel, Layout, ViewState};
use crate::model::AppModel;
use crate::render::{self, Params, Slot, UploadPath};
use crate::ui::columns::{strip_button, STRIP_WIDTH};
use crate::ui::settings_panel;
use eframe::egui::{self, Align, Color32, Layout as UiLayout, Rect, RichText, Sense, UiBuilder};
use engine::config::Format;
use std::sync::Arc;

const SETTINGS_WIDTH: f32 = 280.0;
/// The narrowest the viewports get (A10): below it the settings collapse to a strip.
pub const PREVIEW_MIN: f32 = 480.0;
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
    /// The pinned encode, while it is compared.
    pub pinned: Option<UploadPath>,
}

/// Drawing state that is not part of the model.
#[derive(Default)]
pub struct PreviewUi {
    /// The item the view was last fitted for: a newly expanded texture starts fitted.
    fitted_for: Option<String>,
    /// The pixel readout of the texel under the cursor, shown in the notes strip (laid out before the viewports).
    readout: Option<Vec<String>>,
    /// The settings were opened from their strip: they stay open until the preview closes (A10).
    settings_open: bool,
    /// The overlay on the GPU and the image it was uploaded from (uploaded again only when that `Arc` changes).
    overlay_tex: Option<(Arc<egui::ColorImage>, egui::TextureHandle)>,
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
    /// The texture exports with NVTT; its preview uses the built-in encoder (DECISIONS Note NV9).
    nvtt: bool,
}

impl Info {
    fn new(model: &AppModel, key: &str) -> Info {
        let shown = model.preview.encoded.shown().cloned();
        let job = model.item(key).and_then(|i| i.job.as_ref());
        let job_format = job.map(|j| j.profile.format);
        let nvtt = job.is_some_and(|j| j.profile.encoder.backend == engine::config::Backend::Nvtt);
        let has_source =
            matches!(&model.preview.source, SourceState::Ready(s) if !s.levels.is_empty());
        let (dims, first, end) = model.preview_mips();
        Info {
            dims,
            first,
            end,
            format: shown.as_ref().map(|o| o.result.format).or(job_format),
            shown,
            has_source,
            nvtt,
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
    close: bool,
}

pub fn draw(ui: &mut egui::Ui, model: &mut AppModel, state: &mut PreviewUi, gpu: &GpuStatus) {
    let Some(key) = model.expanded.clone() else {
        return;
    };
    let info = Info::new(model, &key);
    if info.known() {
        model.view.clamp_mip(info.first, info.end);
    }
    // A10: the viewports keep PREVIEW_MIN; the settings collapse to a strip rather than squeeze them.
    if state.settings_open || ui.available_width() - SETTINGS_WIDTH >= PREVIEW_MIN {
        egui::Panel::right("preview-settings")
            .exact_size(SETTINGS_WIDTH)
            .show(ui, |ui| {
                // I2: both axes, so settings wider than the panel scroll inside it instead of moving its left edge
                // under the toolbar (egui cuts an over-wide right panel from the left). Thin bars show without hover.
                ui.spacing_mut().scroll = egui::style::ScrollStyle::thin();
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| settings_panel::draw(ui, model, &key, true));
            });
    } else {
        // Its own id: a strip under "preview-settings" would store 24 px as that panel's width (A6).
        egui::Panel::right("preview-settings-strip")
            .resizable(false)
            .exact_size(STRIP_WIDTH)
            .frame(egui::Frame::new().fill(ui.visuals().panel_fill))
            .show(ui, |ui| {
                if strip_button(ui, "Settings")
                    .on_hover_text("Show the texture settings")
                    .clicked()
                {
                    state.settings_open = true;
                }
            });
    }
    let toolbar_id = egui::Id::new("preview-toolbar");
    let height = |ui: &egui::Ui| {
        egui::containers::panel::PanelState::load(ui.ctx(), toolbar_id)
            .map(|s| s.outer_rect.height())
    };
    let before = height(ui);
    let requests = egui::Panel::top(toolbar_id)
        .show(ui, |ui| toolbar(ui, model, &info))
        .inner;
    // The panel clips to last frame's height: a row the toolbar just gained (first draw, a pin) shows next frame.
    if height(ui) != before {
        ui.ctx().request_repaint();
    }
    if requests.close {
        model.collapse();
        return;
    }
    egui::Panel::bottom("preview-notes").show(ui, |ui| {
        notes(ui, model, &info, gpu, state.readout.as_deref())
    });
    let rect = ui.available_rect_before_wrap();
    if viewports(ui, model, &info, gpu, rect, requests, state, &key) {
        model.collapse();
    }
}

fn channel_help(c: Channel) -> &'static str {
    match c {
        Channel::Rgba => "All channels",
        Channel::R => "Red channel as grey",
        Channel::G => "Green channel as grey",
        Channel::B => "Blue channel as grey",
        Channel::A => "Alpha channel as grey",
        Channel::AlphaMask => "Alpha as a mask over a checkerboard",
    }
}

fn toolbar(ui: &mut egui::Ui, model: &mut AppModel, info: &Info) -> Requests {
    let mut requests = Requests::default();
    ui.with_layout(UiLayout::right_to_left(Align::Min), |ui| {
        if ui
            .button("✕ Close")
            .on_hover_text(format!(
                "Close ({})",
                super::actions::keys_text("close-preview")
            ))
            .clicked()
        {
            requests.close = true;
        }
        // Top-aligned: centring the wrapped rows in last frame's height never settles once they wrap.
        ui.with_layout(UiLayout::left_to_right(Align::Min), |ui| {
            ui.horizontal_wrapped(|ui| toolbar_row(ui, model, info, &mut requests));
        });
    });
    requests
}

/// The wrapped flow: Channels, View and Encode, each after a small caption, then the sRGB / UNORM choice and the
/// mip control (K9, K10: captions and separators in one flow, so a cluster can wrap like any other widgets).
fn toolbar_row(ui: &mut egui::Ui, model: &mut AppModel, info: &Info, requests: &mut Requests) {
    // A caption or the mip label never breaks across rows: the flow wraps whole widgets.
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
    caption_label(ui, "Channels");
    for (channel, name) in CHANNELS {
        if ui
            .selectable_label(model.view.channel == channel, name)
            .on_hover_text(channel_help(channel))
            .clicked()
        {
            model.view.channel = channel;
        }
    }
    ui.separator();
    view_cluster(ui, model, info, requests);
    ui.separator();
    encode_cluster(ui, model);
    ui.separator();
    // HDR formats (BC6H, RGBA16F) are tone-mapped for display: the sRGB / UNORM choice does not apply to them.
    let hdr = info.format.is_some_and(Format::is_hdr);
    // One widget per `add_enabled`, so each wraps on its own in the flow (a nested scope's first widget never wraps).
    let linear = model.view.view_linear;
    for (text, value) in [("sRGB", false), ("UNORM", true)] {
        if ui
            .add_enabled(!hdr, egui::Button::selectable(linear == value, text))
            .on_disabled_hover_text("HDR formats are tone-mapped for display")
            .clicked()
        {
            model.view.view_linear = value;
        }
    }
    mip_control(ui, &mut model.view, info);
}

fn caption_label(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).small().weak());
}

/// Side by side, Split, the pin controls, Heatmap, Lens, Tile 3×3, Fit, 1:1 and, for HDR, the exposure.
fn view_cluster(ui: &mut egui::Ui, model: &mut AppModel, info: &Info, requests: &mut Requests) {
    caption_label(ui, "View");
    let layout = model.view.layout;
    if ui
        .selectable_label(layout == Layout::SideBySide, "Side by side")
        .clicked()
    {
        model.view.layout = Layout::SideBySide;
    }
    if ui
        .selectable_label(layout == Layout::Split, "Split")
        .on_hover_text("One viewport cut by a divider: source on the left, encoded on the right")
        .clicked()
    {
        model.view.layout = Layout::Split;
    }
    pin_controls(ui, model);
    // `add_enabled`, not `add_enabled_ui`: a nested scope's first widget never wraps, so it ran under ✕ Close.
    let heatmap = egui::Button::selectable(model.view.heatmap, "Heatmap");
    if ui
        .add_enabled(info.heatmap_possible(), heatmap)
        .on_disabled_hover_text("Needs an encoded result with a source image")
        .clicked()
    {
        model.view.heatmap = !model.view.heatmap;
    }
    ui.toggle_value(&mut model.view.lens, "Lens")
        .on_hover_text(format!(
            "A magnifier circle at 8x that follows the cursor ({})",
            super::actions::keys_text("lens")
        ));
    ui.toggle_value(&mut model.view.tile, "Tile 3×3")
        .on_hover_text(format!(
            "Repeat the picture 3×3 to check the seams ({})",
            super::actions::keys_text("tile")
        ));
    // Only for a texture with a per-texel value-range warning (owner, spec "Value-range overlay").
    if model
        .expanded
        .as_deref()
        .is_some_and(|k| model.range_overlay_offered(k))
    {
        let on = model.view.overlay == Some(OverlayKind::Range);
        // Review M3: the overlay is drawn on the source pane, which a compared pin replaces.
        let r = ui
            .add_enabled(
                !model.comparing_pin(),
                egui::Button::selectable(on, "Range"),
            )
            .on_hover_text("Paint the source texels outside the expected value range in magenta")
            .on_disabled_hover_text("Hidden while comparing");
        // ui-rules 8.4: distinct from any other "Range".
        r.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, r.enabled(), on, "Range overlay")
        });
        if r.clicked() {
            model.view.overlay = (!on).then_some(OverlayKind::Range);
        }
    }
    requests.fit = ui
        .button("Fit")
        .on_hover_text("Zoom to fit the viewport")
        .clicked();
    requests.one_to_one = ui
        .button("1:1")
        .on_hover_text("One image pixel per screen pixel")
        .clicked();
    if info.format.is_some_and(Format::is_hdr) {
        // Scoped: the mip slider after it keeps egui's width.
        ui.scope(|ui| {
            ui.spacing_mut().slider_width = 90.0;
            let (lo, hi) = view::EXPOSURE_RANGE;
            let r = ui
                .add(
                    egui::Slider::new(&mut model.view.exposure, lo..=hi)
                        .step_by(0.1)
                        .suffix(" EV"),
                )
                .on_hover_text("Exposure: scales the HDR values by 2^EV before the tone map");
            let ev = f64::from(model.view.exposure);
            r.widget_info(|| egui::WidgetInfo::slider(r.enabled(), ev, "Exposure"));
        });
    }
}

/// Live encode and Encode now.
fn encode_cluster(ui: &mut egui::Ui, model: &mut AppModel) {
    caption_label(ui, "Encode");
    let mut live = model.preview.live;
    if ui
        .checkbox(&mut live, "Live encode")
        .on_hover_text("Encode after every edit. Off: press Encode now.")
        .changed()
    {
        model.set_live(live);
    }
    if encode_button(ui, model) {
        model.encode_now();
    }
}

/// **Pin this result**, and while a result is pinned: **Compare: Source | Pinned** and **Unpin**.
fn pin_controls(ui: &mut egui::Ui, model: &mut AppModel) {
    let pin = ui
        .add_enabled(model.can_pin(), egui::Button::new("Pin this result"))
        .on_hover_text(format!(
            "Keep this encode to compare against the next one ({})",
            super::actions::keys_text("pin")
        ))
        .on_disabled_hover_text("Needs a finished encode of the current settings");
    if pin.clicked() {
        model.pin_this();
    }
    if model.preview.pin.is_none() {
        return;
    }
    let pinned = model.comparing_pin();
    ui.label("Compare:");
    if ui.selectable_label(!pinned, "Source").clicked() {
        model.set_compare_pinned(false);
    }
    if ui
        .selectable_label(pinned, "Pinned")
        .on_hover_text("Side by side and Split show the pinned encode against the current one")
        .clicked()
    {
        model.set_compare_pinned(true);
    }
    if ui.small_button("Unpin").clicked() {
        model.unpin();
    }
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
        let r = ui.add(
            egui::Slider::new(&mut mip, info.first..=info.end - 1)
                .step_by(1.0)
                .show_value(false),
        );
        paint_ticks(ui, r.rect, info.first, info.end, mip);
        if r.changed() {
            view.set_mip(mip, info.size_of(view.mip), info.size_of(mip));
        }
    }
}

/// True when the button was pressed. Present (disabled) even when nothing can be encoded, so the toolbar keeps its shape.
/// The toolbar's is the preview's one filled push button while it can encode (A9).
fn encode_button(ui: &mut egui::Ui, model: &AppModel) -> bool {
    let enabled = can_encode(model);
    let mut b = egui::Button::new("Encode now");
    if enabled {
        b = b.fill(ui.visuals().selection.bg_fill);
    }
    ui.add_enabled(enabled, b).clicked()
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

fn notes(
    ui: &mut egui::Ui,
    model: &mut AppModel,
    info: &Info,
    gpu: &GpuStatus,
    readout: Option<&[String]>,
) {
    if model.view.heatmap && info.heatmap_possible() {
        let (label, updating) = (model.heatmap_label(), model.heatmap_updating());
        legend(ui, &mut model.view.full_scale, label, updating);
    }
    // A fixed height (the five lines of a normal map), so the viewports do not move as the pointer enters them;
    // otherwise a pointer near their bottom edge would flip the strip on and off every frame.
    ui.scope(|ui| {
        let line = ui.text_style_height(&egui::TextStyle::Small) + ui.spacing().item_spacing.y;
        // Two more lines while a pin is compared (the pinned values and their difference).
        let lines = if model.comparing_pin() { 7.0 } else { 5.0 };
        ui.set_min_height(lines * line);
        match readout {
            Some(lines) => {
                for l in lines {
                    // One line each (cut, not wrapped), so the height stays put.
                    ui.add(egui::Label::new(RichText::new(l).monospace().small()).truncate());
                }
            }
            None => {
                ui.label(
                    RichText::new("Point at the picture for the texel readout")
                        .small()
                        .weak(),
                );
            }
        }
    });
    let mut lines: Vec<String> = vec![];
    lines.extend(skipped_note(&info.dims, info.first));
    if info.nvtt {
        lines.push("Preview uses the built-in encoder; the export uses NVTT.".into());
    }
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

/// The heatmap's colour bar from 0 to the full scale, with ticks, and the full-scale slider (log, 0.005 to 0.5).
fn legend(ui: &mut egui::Ui, full_scale: &mut f32, label: &str, updating: bool) {
    const STEPS: usize = 32;
    ui.horizontal(|ui| {
        if updating {
            ui.label(RichText::new(format!("{label} updating…")).small().weak());
        } else {
            ui.label(RichText::new(label).small());
        }
        let (rect, _) = ui.allocate_exact_size(egui::vec2(160.0, 22.0), Sense::hover());
        let bar = Rect::from_min_size(rect.min, egui::vec2(rect.width(), 10.0));
        let w = bar.width() / STEPS as f32;
        for i in 0..STEPS {
            let [r, g, b] = view::ramp((i as f32 + 0.5) / STEPS as f32);
            let cell = Rect::from_min_size(
                egui::pos2(bar.min.x + w * i as f32, bar.min.y),
                egui::vec2(w + 0.5, bar.height()),
            );
            ui.painter()
                .rect_filled(cell, 0.0, Color32::from_rgb(r, g, b));
        }
        let text = ui.visuals().text_color();
        for (f, align) in [
            (0.0, egui::Align2::LEFT_TOP),
            (0.5, egui::Align2::CENTER_TOP),
            (1.0, egui::Align2::RIGHT_TOP),
        ] {
            let x = bar.min.x + bar.width() * f;
            ui.painter().vline(
                x,
                bar.bottom()..=bar.bottom() + 3.0,
                egui::Stroke::new(1.0, text),
            );
            ui.painter().text(
                egui::pos2(x, bar.bottom() + 2.0),
                align,
                format!("{:.3}", *full_scale * f),
                egui::FontId::proportional(9.0),
                text,
            );
        }
        let (lo, hi) = view::FULL_SCALE_RANGE;
        ui.add(
            egui::Slider::new(full_scale, lo..=hi)
                .logarithmic(true)
                .max_decimals(3)
                .text("full scale"),
        )
        .on_hover_text("The difference drawn white; 0.1 is about 25 codes of 255");
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
    let left_draw = if model.comparing_pin() {
        draw_pinned_pane(ui, model, gpu, left, left_vis, size)
    } else {
        let overlay = range_overlay_texture(ui, model, state, key);
        draw_source_pane(ui, model, info, gpu, left, left_vis, size, overlay)
    };
    let right_draw = draw_encoded_pane(ui, model, info, gpu, right, right_vis, size, &mut encode);
    if let Some((pinned, current)) = model.pin_captions() {
        caption(ui, left_vis, &pinned, false);
        caption(ui, right_vis, &current, true);
    }
    if split {
        ui.painter().vline(
            divider_x,
            rect.y_range(),
            egui::Stroke::new(1.5, Color32::from_gray(200)),
        );
    }
    // The cursor as a fraction of the pane it is over: both panes (or both sides of Split) share it.
    let hover = responses
        .iter()
        .find_map(|r| r.hover_pos().map(|p| (r.rect.min, p)))
        .filter(|_| info.known())
        .map(|(min, p)| view::pane_fraction([p.x, p.y], [min.x, min.y], size));
    let readout = hover.and_then(|s| {
        let t = model.view.texel_at(size, mip_size, s)?;
        Some(model.preview_texel(mip, t).lines())
    });
    if readout != state.readout {
        state.readout = readout;
        ui.ctx().request_repaint();
    }
    if let (true, Some(s)) = (model.view.lens, hover) {
        let (uv_offset, uv_scale) = model.view.lens_uv(size, mip_size, s);
        for (pane, vis, drawn) in [(left, left_vis, left_draw), (right, right_vis, right_draw)] {
            let centre = pane.min + egui::vec2(s[0] * size[0], s[1] * size[1]);
            lens(ui, centre, vis, drawn, uv_offset, uv_scale);
        }
    }
    if encode {
        model.encode_now();
    }
    responses.iter().any(|r| r.double_clicked())
}

/// The magnifier in one pane: the pane's own slot drawn again at `centre` through the lens uv rect (a circle,
/// nearest sampling; its own uniform block), clipped to the pane, with a rim and a centre crosshair.
fn lens(
    ui: &mut egui::Ui,
    centre: egui::Pos2,
    visible: Rect,
    drawn: Option<(Slot, Params)>,
    uv_offset: [f32; 2],
    uv_scale: [f32; 2],
) {
    let Some((slot, params)) = drawn else {
        return;
    };
    let r = Rect::from_center_size(centre, egui::Vec2::splat(view::LENS_PX));
    let params = Params {
        uv_offset,
        uv_scale,
        lens: 1,
        ..params
    };
    let clip = visible.intersect(ui.clip_rect());
    let painter = ui.painter().with_clip_rect(clip);
    painter.circle_filled(centre, view::LENS_PX / 2.0, PANE_BG);
    paint_clipped(ui, r, visible, slot, params);
    let stroke = egui::Stroke::new(1.0, Color32::from_gray(220));
    painter.circle_stroke(
        centre,
        view::LENS_PX / 2.0,
        egui::Stroke::new(1.5, Color32::from_gray(200)),
    );
    let arm = 6.0;
    painter.hline(centre.x - arm..=centre.x + arm, centre.y, stroke);
    painter.vline(centre.x, centre.y - arm..=centre.y + arm, stroke);
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
        exposure: view::exposure_factor(view.exposure),
        tile: view.tile as u32,
        lens: 0,
    }
}

/// Draws a slot into `viewport`, showing only `visible` of it.
fn paint_clipped(ui: &mut egui::Ui, viewport: Rect, visible: Rect, slot: Slot, params: Params) {
    ui.scope(|ui| {
        ui.set_clip_rect(visible.intersect(ui.clip_rect()));
        render::paint(ui, viewport, slot, params);
    });
}

/// The range overlay's texture while the toggle is on and offered; the image is built on a worker (`overlay_for`).
fn range_overlay_texture(
    ui: &egui::Ui,
    model: &mut AppModel,
    state: &mut PreviewUi,
    key: &str,
) -> Option<egui::TextureId> {
    if model.view.overlay != Some(OverlayKind::Range) || !model.range_overlay_offered(key) {
        // Review M7: off frees the full-size image and its texture.
        state.overlay_tex = None;
        if model.overlay.key.is_some() {
            model.overlay.clear();
        }
        return None;
    }
    let SourceState::Ready(s) = &model.preview.source else {
        return None;
    };
    let mip = model.view.mip.saturating_sub(s.first_level);
    let img = model.overlay_for(key, mip, OverlayKind::Range)?;
    if !state
        .overlay_tex
        .as_ref()
        .is_some_and(|(a, _)| Arc::ptr_eq(a, &img))
    {
        // Review I1, M4: the worker built the ColorImage; the upload shares its `Arc` (no per-texel work here), and
        // Nearest both ways keeps a single bad texel visible when zoomed out.
        let options = egui::TextureOptions {
            magnification: egui::TextureFilter::Nearest,
            minification: egui::TextureFilter::Nearest,
            wrap_mode: egui::TextureWrapMode::Repeat,
            ..Default::default()
        };
        let data = egui::ImageData::Color(img.clone());
        let tex = match state.overlay_tex.take() {
            Some((_, mut tex)) => {
                tex.set(data, options);
                tex
            }
            None => ui.ctx().load_texture("overlay", data, options),
        };
        state.overlay_tex = Some((img, tex));
    }
    state.overlay_tex.as_ref().map(|(_, t)| t.id())
}

/// K8: the screen rect of the picture (uv `[0, 1]²`, or `[-1, 2)²` with Tile 3×3, as the shader shows it) and that uv
/// rect. An overlay painted there with Repeat wrap lines up with the picture and never draws past it.
pub fn picture_rects(
    view: &ViewState,
    viewport: Rect,
    size: [f32; 2],
    mip_size: (u32, u32),
) -> (Rect, Rect) {
    let (lo, hi) = if view.tile { (-1.0, 2.0) } else { (0.0, 1.0) };
    let (offset, scale) = view.uv_rect(size, mip_size);
    let at = |u: f32, i: usize| viewport.min[i] + (u - offset[i]) / scale[i] * size[i];
    (
        Rect::from_min_max(
            egui::pos2(at(lo, 0), at(lo, 1)),
            egui::pos2(at(hi, 0), at(hi, 1)),
        ),
        Rect::from_min_max(egui::pos2(lo, lo), egui::pos2(hi, hi)),
    )
}

#[allow(clippy::too_many_arguments)]
fn draw_source_pane(
    ui: &mut egui::Ui,
    model: &AppModel,
    info: &Info,
    gpu: &GpuStatus,
    viewport: Rect,
    visible: Rect,
    size: [f32; 2],
    overlay: Option<egui::TextureId>,
) -> Option<(Slot, Params)> {
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
    let mut drawn = None;
    if let (None, SourceState::Ready(s)) = (&text, &model.preview.source) {
        let mip = model.view.mip;
        let tonemap = info.format.is_some_and(Format::is_hdr);
        let params = params_for(
            &model.view,
            size,
            info.size_of(mip),
            mip.saturating_sub(s.first_level),
            tonemap,
        );
        paint_clipped(ui, viewport, visible, Slot::Source, params);
        drawn = Some((Slot::Source, params));
        if let Some(id) = overlay {
            let (pic, uv) = picture_rects(&model.view, viewport, size, info.size_of(mip));
            ui.painter()
                .with_clip_rect(visible.intersect(ui.clip_rect()))
                .image(id, pic, uv, Color32::WHITE);
        }
    }
    pane_overlay(ui, visible, "Source", None, text.as_deref(), false);
    drawn
}

/// The left pane while comparing with a pin: the pinned encode, or the dash when it lacks the mip on view.
fn draw_pinned_pane(
    ui: &mut egui::Ui,
    model: &AppModel,
    gpu: &GpuStatus,
    viewport: Rect,
    visible: Rect,
    size: [f32; 2],
) -> Option<(Slot, Params)> {
    ui.painter().rect_filled(visible, 0.0, PANE_BG);
    let mip = model.view.mip;
    let pin = model.compared_pin()?;
    let uploaded = matches!(gpu.pinned, Some(UploadPath::Bc | UploadPath::CpuDecoded(_)));
    let text = if let Some(missing) = model.pin_missing_text(mip) {
        Some(missing)
    } else if !gpu.renderer {
        Some("No GPU preview")
    } else if !uploaded {
        Some("No preview of the pinned result")
    } else {
        None
    };
    let mut drawn = None;
    if text.is_none() {
        let first = pin.out.result.source.first_level;
        let params = params_for(
            &model.view,
            size,
            (pin.out.result.source.dims.get(mip).copied()).unwrap_or((1, 1)),
            mip.saturating_sub(first),
            pin.out.result.format.is_hdr(),
        );
        paint_clipped(ui, viewport, visible, Slot::Pinned, params);
        drawn = Some((Slot::Pinned, params));
    }
    pane_overlay(ui, visible, "Pinned", None, text, false);
    drawn
}

/// A small caption under a pane's title, wrapped to the pane (the current side's right-aligned).
fn caption(ui: &egui::Ui, visible: Rect, text: &str, right: bool) {
    let colour = Color32::from_gray(230);
    let wrap = (visible.width() - 12.0).max(20.0);
    let galley = ui.painter().layout(
        text.to_string(),
        egui::FontId::proportional(11.0),
        colour,
        wrap,
    );
    let x = if right {
        visible.max.x - 6.0 - galley.size().x
    } else {
        visible.min.x + 6.0
    };
    let pos = egui::pos2(x, visible.min.y + 26.0);
    let painter = ui.painter().with_clip_rect(visible);
    painter.rect_filled(
        Rect::from_min_size(pos, galley.size()).expand(2.0),
        2.0,
        Color32::from_black_alpha(150),
    );
    painter.galley(pos, galley, colour);
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
) -> Option<(Slot, Params)> {
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
    let mut drawn = None;
    if gpu.renderer && info.shown.is_some() {
        let mip = model.view.mip;
        if heat {
            let mut params = params_for(&model.view, size, info.size_of(mip), 0, false);
            params.channel = 0;
            params.view_linear = 0;
            paint_clipped(ui, viewport, visible, Slot::Heatmap, params);
            drawn = Some((Slot::Heatmap, params));
        } else if let (true, Some(out)) = (uploaded, &info.shown) {
            let tonemap = out.result.format.is_hdr();
            let first = out.result.source.first_level;
            let params = params_for(
                &model.view,
                size,
                info.size_of(mip),
                mip.saturating_sub(first),
                tonemap,
            );
            paint_clipped(ui, viewport, visible, Slot::Encoded, params);
            drawn = Some((Slot::Encoded, params));
        }
    }
    if drawn.is_some() && matches!(model.preview.encoded, EncodedState::Encoding { .. }) {
        ui.painter()
            .rect_filled(visible, 0.0, Color32::from_black_alpha(128));
    }
    let comparing = model.comparing_pin();
    let title = match (heat, comparing) {
        (true, true) => "Current vs Pinned |Δ|",
        (true, false) => "Difference",
        (false, true) => "Current",
        (false, false) => "Encoded",
    };
    let button = offer_encode && can_encode(model);
    if pane_overlay(ui, visible, title, badge, text.as_deref(), button) {
        *encode = true;
    }
    drawn
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

/// The x of each mip's stop along a slider whose widget spans `left..=right` (the handle's travel is inset by
/// its radius, about two fifths of the slider's height; `radius`).
fn tick_xs(left: f32, right: f32, radius: f32, count: usize) -> Vec<f32> {
    let (a, b) = (left + radius, right - radius);
    match count {
        0 => vec![],
        1 => vec![(a + b) / 2.0],
        n => (0..n)
            .map(|i| a + (b - a) * i as f32 / (n - 1) as f32)
            .collect(),
    }
}

/// A short tick under the slider's track for each mip, the current one drawn longer and in the accent colour.
fn paint_ticks(ui: &egui::Ui, rect: Rect, first: usize, end: usize, current: usize) {
    let radius = rect.height() / 2.5;
    let weak = ui.visuals().weak_text_color();
    let accent = ui.visuals().selection.bg_fill;
    for (i, x) in tick_xs(rect.left(), rect.right(), radius, end - first)
        .into_iter()
        .enumerate()
    {
        let is_current = first + i == current;
        let (len, stroke) = if is_current {
            (5.0, egui::Stroke::new(2.0, accent))
        } else {
            (3.0, egui::Stroke::new(1.0, weak))
        };
        let y = rect.bottom();
        ui.painter()
            .line_segment([egui::pos2(x, y - len), egui::pos2(x, y)], stroke);
    }
}

#[cfg(test)]
mod tests {
    use super::tick_xs;

    #[test]
    fn ticks_are_evenly_spaced_between_the_handle_ends() {
        let xs = tick_xs(0.0, 100.0, 10.0, 5);
        assert_eq!(xs, vec![10.0, 30.0, 50.0, 70.0, 90.0]);
        assert_eq!(tick_xs(0.0, 100.0, 10.0, 2), vec![10.0, 90.0]);
        assert_eq!(tick_xs(0.0, 100.0, 10.0, 1), vec![50.0]);
        assert!(tick_xs(0.0, 100.0, 10.0, 0).is_empty());
    }
}
