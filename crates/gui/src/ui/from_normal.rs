//! v2 Track B (Note CO): the "Generate from normal map" dialog. The state and jobs are in `model::from_normal`.
use super::dialogs::{dialog_hover, short_name, Role, HALF_SIZE_LIST_SHARE};
use crate::model::from_normal::{GenMap, GenResult, GenSize, COMPLEX_USE_P};
use crate::model::AppModel;
use eframe::egui;
use engine::from_normal::{Gray, Tiling};
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// The footer: Cancel, then Generate (enabled with a tick and no job running). Neither is the Enter default:
/// Regenerate overwrites (ui-rules 5.2).
pub fn gen_buttons(ticked: bool, running: bool) -> [(&'static str, Role, bool); 2] {
    [
        ("Cancel", Role::Plain, true),
        ("Generate", Role::Plain, ticked && !running),
    ]
}

/// The maps the dialog lists; Tasks 10 and 12 add AO and curvature.
const MAPS: [(GenMap, &str); 1] = [(GenMap::Height, "Height")];

/// The preview tile's side in points.
const TILE: f32 = 160.0;

pub fn draw(ctx: &egui::Context, model: &mut AppModel) {
    model.gen_frame();
    let Some(g) = model.gen.as_ref() else {
        model.dialog = None;
        return;
    };
    let set = g.set.clone();
    let mut ticked = g.ticked.clone();
    let mut opts = g.opts.clone();
    let m_alpha = g.m_alpha;
    let (preview, error, running) = (g.preview.clone(), g.error.clone(), g.running);
    let progress = g.progress.load(Ordering::Relaxed);
    let offers: Vec<_> = MAPS
        .iter()
        .map(|&(m, label)| (m, label, model.gen_offer(&set, m)))
        .collect();
    let size_rule = model.gen_size(&set, GenMap::Height);
    let complex = m_alpha;
    let format_line = model.gen_format_line();
    let warning = model.gen_size_warning();
    let paths = model.gen_paths();
    let buttons = gen_buttons(!ticked.is_empty(), running.is_some());
    let hover = model.gen_generate_hover();
    let max_h = ctx.content_rect().height() * HALF_SIZE_LIST_SHARE;
    let mut dragging = false;
    let (k, clicked) = dialog_hover(
        ctx,
        "generate-from-normal",
        false,
        640.0,
        "Generate from normal map",
        &buttons,
        &[None, Some(hover)],
        |ui| {
            ui.scope(|ui| {
                ui.style_mut().spacing.scroll = egui::style::ScrollStyle::thin();
                egui::ScrollArea::vertical()
                    .max_height(max_h)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        // 1. The maps, each off with its reason.
                        for (map, label, offer) in &offers {
                            let mut on = ticked.contains(map);
                            let r =
                                ui.add_enabled(offer.is_ok(), egui::Checkbox::new(&mut on, *label));
                            if let Err(why) = offer {
                                r.on_disabled_hover_text(why);
                            } else if r.changed() {
                                if on {
                                    ticked.insert(*map);
                                } else {
                                    ticked.remove(map);
                                }
                            }
                        }
                        egui::Grid::new("gen-opts").num_columns(2).show(ui, |ui| {
                            // 2. Size: a fixed one read-only, else a choice.
                            ui.label("Size");
                            match &size_rule {
                                Ok(GenSize::Locked(s)) => {
                                    ui.label(format!("{s} px (matches this set's maps)"));
                                }
                                Ok(GenSize::Free { sizes, .. }) => {
                                    egui::ComboBox::from_id_salt("gen-size")
                                        .selected_text(format!("{} px", opts.size))
                                        .show_ui(ui, |ui| {
                                            for s in sizes {
                                                ui.selectable_value(
                                                    &mut opts.size,
                                                    *s,
                                                    format!("{s} px"),
                                                );
                                            }
                                        });
                                }
                                Err(why) => {
                                    ui.weak(why);
                                }
                            }
                            ui.end_row();
                            // 3. Strength.
                            if let Some(h) = opts.height.as_mut() {
                                ui.label("Strength");
                                dragging |= ui
                                    .add(egui::Slider::new(&mut h.strength, 0.1..=1.0))
                                    .dragged();
                                ui.end_row();
                            }
                            // 4. Tiling.
                            ui.label("Tiling");
                            ui.horizontal(|ui| {
                                for (t, l) in [
                                    (Tiling::Auto, "Auto"),
                                    (Tiling::Wrap, "Wrap"),
                                    (Tiling::Mirror, "Mirror"),
                                ] {
                                    ui.radio_value(&mut opts.tiling, t, l);
                                }
                                if opts.tiling == Tiling::Auto {
                                    if let Some(r) = preview.as_ref().map(|p| &p.maps.report) {
                                        let what = if r.wrap { "tiles" } else { "does not tile" };
                                        let unsure = if r.ambiguous { " (unsure)" } else { "" };
                                        ui.weak(format!("detected: {what}{unsure}"));
                                    }
                                }
                            });
                            ui.end_row();
                            // 5. Relief balance.
                            ui.label("Relief balance");
                            dragging |= ui
                                .add(egui::Slider::new(&mut opts.relief, 0.0..=1.0))
                                .dragged();
                            ui.end_row();
                            // 6. Use as, on complex-material sets only (D5).
                            if complex {
                                ui.label("Use as");
                                ui.horizontal(|ui| {
                                    ui.add_enabled(
                                        false,
                                        egui::RadioButton::new(false, "Parallax _p"),
                                    )
                                    .on_disabled_hover_text(COMPLEX_USE_P);
                                    let _ = ui.radio(true, "Complex material _m alpha");
                                });
                                ui.end_row();
                            }
                        });
                        // 7. The normal's format, read-only.
                        ui.label(&format_line);
                        // 8. The size warning.
                        if let Some(w) = warning {
                            ui.colored_label(super::status::AMBER, w);
                        }
                        // 9. The output paths, shortened, the full path on hover.
                        for (_, p, (w, h)) in &paths {
                            let name = p.file_name().unwrap_or_default().to_string_lossy();
                            let full = p.display().to_string();
                            ui.label(format!(
                                "Writes {} ({w}×{h}, 16-bit grey PNG)",
                                short_name(&name)
                            ))
                            .on_hover_text(full);
                        }
                        // 10. The preview tiles; 11. the fit line.
                        if let Some(p) = &preview {
                            tiles(ui, p);
                            let r = &p.maps.report;
                            ui.weak(format!(
                                "Fit: mean {:.1}°, p95 {:.1}°, {:.1} % clamped; normal tilt {:.1}°",
                                r.fit_mean_deg,
                                r.fit_p95_deg,
                                r.clamped_share * 100.0,
                                r.tilt_deg
                            ));
                        }
                    });
            });
            // 12. Progress, and a refusal, below the scrolled body: always on screen beside the footer (CO61).
            if matches!(running, Some((_, false))) {
                ui.label(format!("Generating… {} %", progress / 10));
            }
            if let Some(e) = &error {
                ui.colored_label(super::status::RED, e);
            }
        },
    );
    if let Some(g) = model.gen.as_mut() {
        g.ticked = ticked;
        g.opts = opts;
        g.dragging = dragging;
    }
    if clicked == Some(0) || k.cancel {
        model.gen_cancel();
    } else if clicked == Some(1) {
        model.gen_generate();
    }
}

/// The preview's maps as grey tiles, each texture loaded once per result.
fn tiles(ui: &mut egui::Ui, p: &Arc<GenResult>) {
    ui.horizontal(|ui| {
        for (g, label) in [(&p.maps.height, "Height preview")] {
            let Some(g) = g else { continue };
            let tex = texture(ui.ctx(), label, p, g);
            let size = egui::vec2(TILE, TILE * g.height as f32 / g.width.max(1) as f32);
            let r = ui.add(egui::Image::new((tex.id(), size)));
            r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, label));
        }
    });
}

fn texture(ctx: &egui::Context, label: &str, p: &Arc<GenResult>, g: &Gray) -> egui::TextureHandle {
    let id = egui::Id::new(("gen-preview-tex", label));
    type Slot = (Arc<GenResult>, egui::TextureHandle);
    if let Some((old, h)) = ctx.data(|d| d.get_temp::<Slot>(id)) {
        if Arc::ptr_eq(&old, p) {
            return h;
        }
    }
    let bytes: Vec<u8> =
        g.px.iter()
            .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
            .collect();
    let img = egui::ColorImage::from_gray([g.width as usize, g.height as usize], &bytes);
    let h = ctx.load_texture(label, img, Default::default());
    ctx.data_mut(|d| d.insert_temp(id, (p.clone(), h.clone())));
    h
}
