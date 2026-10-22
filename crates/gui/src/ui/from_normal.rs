//! v2 Track B (Note CO): the "Generate from normal map" dialog. The state and jobs are in `model::from_normal`.
use super::dialogs::{dialog_hover, short_name, Role, HALF_SIZE_LIST_SHARE};
use crate::model::from_normal::{
    map_label, GenMap, GenResult, GenSize, BENT_NOTE, COMPLEX_USE_P, CURVATURE_HOVER,
};
use crate::model::AppModel;
use eframe::egui;
use engine::from_normal::{AoOpts, Gray, Tiling};
use std::sync::Arc;

/// The footer: Cancel, then Generate (enabled with a tick and no job running). Neither is the Enter default:
/// Regenerate overwrites (ui-rules 5.2).
pub fn gen_buttons(ticked: bool, running: bool) -> [(&'static str, Role, bool); 2] {
    [
        ("Cancel", Role::Plain, true),
        ("Generate", Role::Plain, ticked && !running),
    ]
}

/// The maps the dialog lists.
const MAPS: [GenMap; 3] = [GenMap::Height, GenMap::Ao, GenMap::Curvature];

/// The AO group's controls, inside the engine's `AoOpts` guards (CO84). Radius is shown as a % of the width.
pub const AO_RADIUS_PCT: std::ops::RangeInclusive<f32> = 1.5625..=25.0;
/// Quality: slices; steps stay at 16 (Q17).
pub const AO_SLICES: [u32; 3] = [8, 16, 32];
pub const AO_DEPTH: std::ops::RangeInclusive<f32> = 0.25..=4.0;
pub const AO_INTENSITY: std::ops::RangeInclusive<f32> = 0.5..=3.0;
pub const AO_INTENSITY_HOVER: &str =
    "the file holds raw visibility; CS applies AO to indirect light itself. 0.7–1.0 is a good range";

/// The preview tile's side in points.
const TILE: f32 = 160.0;

pub fn draw(ctx: &egui::Context, model: &mut AppModel) {
    if model.gen.is_none() {
        model.dialog = None;
        return;
    }
    // One set lookup per frame (Task 6 review M6).
    let facts = model.gen_facts();
    model.gen_frame_at(facts.normal.clone());
    let ao_summary = model.gen_ao_summary();
    let progress = model.gen_progress();
    let Some(g) = model.gen.as_ref() else {
        model.dialog = None;
        return;
    };
    let ticked = g.ticked.clone();
    let mut toggled = vec![];
    let mut opts = g.opts.clone();
    let m_alpha = g.m_alpha;
    let (preview, error, running) = (g.preview.clone(), g.error.clone(), g.running);
    let complex = m_alpha;
    let ao_width = facts
        .paths
        .iter()
        .find(|(m, _, _)| *m == GenMap::Ao)
        .map(|(_, _, (w, _))| *w);
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
                        // 0. The bent-layer note, on the first screen (Q20, spec-M8).
                        // Review M5: only where AO can be generated.
                        let ao_ok = facts
                            .offers
                            .iter()
                            .any(|(m, r)| *m == GenMap::Ao && r.is_ok());
                        if facts.bent && ao_ok {
                            ui.colored_label(super::status::AMBER, BENT_NOTE);
                        }
                        // 1. The maps, each off with its reason.
                        for (map, offer) in &facts.offers {
                            if !MAPS.contains(map) {
                                continue;
                            }
                            let was = ticked.contains(map);
                            let mut on = was;
                            let r = ui.add_enabled(
                                offer.is_ok(),
                                egui::Checkbox::new(&mut on, map_label(*map)),
                            );
                            if let Err(why) = offer {
                                r.on_disabled_hover_text(why);
                            } else {
                                if *map == GenMap::Curvature {
                                    r.on_hover_text(CURVATURE_HOVER);
                                }
                                if on != was {
                                    toggled.push((*map, on));
                                }
                            }
                        }
                        egui::Grid::new("gen-opts").num_columns(2).show(ui, |ui| {
                            // 2. Size: one choice with per-map caps; a map its recipe fixes is read-only.
                            ui.label("Size");
                            ui.vertical(|ui| {
                                match &facts.size {
                                    Ok(GenSize::Free { sizes, .. }) => {
                                        egui::ComboBox::from_id_salt("gen-size")
                                            .selected_text(format!("{} px", facts.shown_size))
                                            .show_ui(ui, |ui| {
                                                for s in sizes {
                                                    // Highlighted by the size it writes at.
                                                    if ui
                                                        .selectable_label(
                                                            facts.shown_size == *s,
                                                            format!("{s} px"),
                                                        )
                                                        .clicked()
                                                    {
                                                        opts.size = *s;
                                                    }
                                                }
                                            });
                                    }
                                    Ok(GenSize::Locked(_)) => {}
                                    Err(why) => {
                                        ui.weak(why);
                                    }
                                }
                                for (m, s) in &facts.locked {
                                    ui.label(format!(
                                        "{}: {s} px (matches this set's maps)",
                                        map_label(*m)
                                    ));
                                }
                            });
                            ui.end_row();
                            // 3. Strength.
                            // Review M4: Strength only while Height is ticked.
                            if let (Some(h), true) =
                                (opts.height.as_mut(), ticked.contains(&GenMap::Height))
                            {
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
                            // 7. The AO group, while AO is ticked.
                            if let Some(a) = opts.ao.as_mut() {
                                dragging |= ao_group(ui, a, ao_width);
                            }
                        });
                        // 8. The normal's format, read-only.
                        ui.label(&facts.format_line);
                        // 9. The size warning.
                        if let Some(w) = facts.warning {
                            ui.colored_label(super::status::AMBER, w);
                        }
                        // 10. The output paths, shortened, the full path on hover; each at its own size.
                        for (_, p, (w, h)) in &facts.paths {
                            let name = p.file_name().unwrap_or_default().to_string_lossy();
                            let full = p.display().to_string();
                            ui.label(format!(
                                "Writes {} ({w}×{h}, 16-bit grey PNG)",
                                short_name(&name)
                            ))
                            .on_hover_text(full);
                        }
                        // 11. The preview tiles; 12. the fit and AO lines.
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
                            if let Some(s) = &ao_summary {
                                ui.weak(s);
                            }
                        }
                    });
            });
            // 13. Progress, and a refusal, below the scrolled body: always on screen beside the footer (CO61).
            if matches!(running, Some((_, false))) {
                ui.label(format!("Generating… {} %", progress / 10));
            }
            if let Some(e) = &error {
                ui.colored_label(super::status::RED, e);
            }
        },
    );
    if let Some(g) = model.gen.as_mut() {
        g.opts = opts;
        g.dragging = dragging;
    }
    for (map, on) in toggled {
        model.gen_tick(map, on);
    }
    if clicked == Some(0) || k.cancel {
        model.gen_cancel();
    } else if clicked == Some(1) {
        model.gen_generate();
    }
}

/// The AO rows of the options grid (Q17 defaults; CO84 guards). Returns whether a slider is being dragged.
fn ao_group(ui: &mut egui::Ui, a: &mut AoOpts, width: Option<u32>) -> bool {
    let mut dragging = false;
    ui.label("Radius");
    ui.horizontal(|ui| {
        let mut pct = a.radius * 100.0;
        let r = ui.add(
            egui::Slider::new(&mut pct, AO_RADIUS_PCT)
                .suffix(" %")
                .max_decimals(2),
        );
        dragging |= r.dragged();
        if r.changed() {
            a.radius = pct / 100.0;
        }
        if let Some(w) = width {
            ui.weak(format!("{:.0} texels", a.radius * w as f32));
        }
    });
    ui.end_row();
    ui.label("Quality");
    ui.horizontal(|ui| {
        for s in AO_SLICES {
            ui.radio_value(&mut a.slices, s, format!("{s} slices"));
        }
    });
    ui.end_row();
    ui.label("Depth");
    dragging |= ui.add(egui::Slider::new(&mut a.depth, AO_DEPTH)).dragged();
    ui.end_row();
    ui.label("Intensity").on_hover_text(AO_INTENSITY_HOVER);
    dragging |= ui
        .add(egui::Slider::new(&mut a.intensity, AO_INTENSITY))
        .on_hover_text(AO_INTENSITY_HOVER)
        .dragged();
    ui.end_row();
    dragging
}

/// The preview's maps as grey tiles, each texture loaded once per result.
fn tiles(ui: &mut egui::Ui, p: &Arc<GenResult>) {
    ui.horizontal(|ui| {
        for (g, label) in [
            (&p.maps.height, "Height preview"),
            (&p.maps.ao, "AO preview"),
            (&p.maps.curvature, "Curvature preview"),
        ] {
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
