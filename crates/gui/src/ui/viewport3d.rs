//! The 3D panel of the Meshes view (plan B Task B8): draws `Viewport3d`, turns drag / scroll / double-click into
//! orbit changes and uploads a new frame into an egui texture when its id changes (P0 CPU path).
use crate::model::meshes::LE_WARNING;
use crate::model::viewport3d::{
    moved_text, off_line, Look, V3State, Viewport3d, CHOOSE_FOLDER, CUBEMAPS_FOOTER,
    FEATURES_LABEL, FRAME_LABEL, KEEP_OLD, NOT_FOUND, USE_NEW,
};
use eframe::egui;
use render3d::locate::Change;
use render3d::ShapeStatus;

const AMBER: egui::Color32 = egui::Color32::from_rgb(230, 160, 30);
const RED: egui::Color32 = egui::Color32::from_rgb(210, 60, 60);
/// Space kept under the image for the problem lists and the footer.
const BELOW: f32 = 110.0;
const MAX_LISTED: usize = 50;

fn pick_folder() -> Option<std::path::PathBuf> {
    rfd::FileDialog::new()
        .set_title("Community Shaders folder (the mod folder or its Shaders folder)")
        .pick_folder()
}

pub fn show(ui: &mut egui::Ui, v: &mut Viewport3d) {
    popup(ui.ctx(), v);
    ui.horizontal_wrapped(|ui| {
        let mut on = v.features_on;
        if ui.checkbox(&mut on, FEATURES_LABEL).changed() {
            v.set_features(on);
        }
        let used = v
            .shaders
            .as_ref()
            .map_or("none yet".into(), |f| f.root.display().to_string());
        if ui
            .small_button("Shader folder…")
            .on_hover_text(format!("Community Shaders in use: {used}"))
            .clicked()
        {
            if let Some(p) = pick_folder() {
                v.choose_folder(p);
            }
        }
        if v.show_spinner(std::time::Instant::now()) {
            ui.spinner();
        }
    });
    look_controls(ui, &mut v.look);
    if let Some(n) = v.notice.clone() {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(AMBER, n);
            if ui.small_button("Dismiss").clicked() {
                v.notice = None;
            }
        });
    }
    match v.state.clone() {
        V3State::NotFound => {
            ui.label(NOT_FOUND);
            if ui.button(CHOOSE_FOLDER).clicked() {
                if let Some(p) = pick_folder() {
                    v.choose_folder(p);
                }
            }
        }
        V3State::Error(e) => {
            ui.colored_label(RED, e);
        }
        V3State::Ready | V3State::Rendering => {}
    }
    if v.shaders.is_none() && v.state != V3State::NotFound && v.locating() {
        ui.label("Looking for Community Shaders…");
    }
    let le = v.report.as_ref().is_some_and(|r| r.le_mesh);
    if le {
        ui.colored_label(AMBER, LE_WARNING);
    } else {
        image(ui, v);
    }
    problems(ui, v);
    footer(ui, v);
    if v.busy() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(16));
    }
}

/// The "View" section: background and lights. Each change only re-renders (the model sends a new `Render`).
fn look_controls(ui: &mut egui::Ui, l: &mut Look) {
    egui::CollapsingHeader::new("View")
        .id_salt("viewport3d-look")
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Background");
                ui.radio_value(&mut l.hdri, false, "Solid");
                ui.radio_value(&mut l.hdri, true, "Studio HDRI");
                if !l.hdri {
                    // AG9: the background is one sRGB grey; swatch + hex edit it, a non-grey pick keeps its Rec.709 luma.
                    let g = super::color::unit_to_u8(l.grey);
                    let id = egui::Id::new("viewport3d-bg");
                    if let Some([r, gg, b]) = super::color::color_field(ui, id, [g; 3]) {
                        let y =
                            0.2126 * f32::from(r) + 0.7152 * f32::from(gg) + 0.0722 * f32::from(b);
                        l.grey = y.round() / 255.0;
                    }
                    let mut pct = l.grey * 100.0;
                    if ui
                        .add(
                            egui::Slider::new(&mut pct, 0.0..=100.0)
                                .suffix(" %")
                                .text("Grey"),
                        )
                        .changed()
                    {
                        l.grey = pct / 100.0;
                    }
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    egui::Slider::new(&mut l.sun, Look::SUN_RANGE.0..=Look::SUN_RANGE.1)
                        .text("Sun brightness"),
                );
                ui.add(
                    egui::Slider::new(&mut l.point, Look::POINT_RANGE.0..=Look::POINT_RANGE.1)
                        .text("Point light brightness"),
                );
                ui.add(
                    egui::Slider::new(&mut l.radius, Look::RADIUS_RANGE.0..=Look::RADIUS_RANGE.1)
                        .logarithmic(true)
                        .text("Point light radius (x model size)"),
                );
                if ui.small_button("Reset view").clicked() {
                    *l = Look::default();
                }
            });
            if l.hdri {
                ui.weak("Studio HDRI by Poly Haven (CC0): background and ambient light");
            }
        });
}

/// The frame, sized to the space left; asks the model for a frame of that size (physical pixels).
fn image(ui: &mut egui::Ui, v: &mut Viewport3d) {
    let avail = ui.available_size();
    let size = egui::vec2(avail.x.max(1.0), (avail.y - BELOW).max(64.0));
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    if resp.double_clicked() {
        v.orbit.reset();
    } else if resp.dragged() {
        let d = resp.drag_delta();
        v.orbit.drag(d.x, d.y);
    }
    if resp.hovered() {
        let s = ui.input(|i| i.smooth_scroll_delta.y);
        if s != 0.0 {
            v.orbit.zoom((-s * 0.002).exp());
        }
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, FRAME_LABEL));
    let ppp = ui.ctx().pixels_per_point();
    v.request_frame(
        (rect.width() * ppp).round() as u32,
        (rect.height() * ppp).round() as u32,
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, egui::Color32::BLACK);
    let Some(frame) = v.frame.clone() else {
        return;
    };
    let id = egui::Id::new("viewport3d-frame");
    let cached: Option<(u64, egui::TextureHandle)> = ui.ctx().data(|d| d.get_temp(id));
    let tex = match cached {
        Some((fid, t)) if fid == frame.id => t,
        old => {
            let t0 = std::time::Instant::now();
            let img = egui::ColorImage::from_rgba_unmultiplied(
                [frame.width as usize, frame.height as usize],
                &frame.rgba,
            );
            let t = match old {
                Some((_, mut t)) => {
                    t.set(img, egui::TextureOptions::LINEAR);
                    t
                }
                None => ui
                    .ctx()
                    .load_texture("viewport3d", img, egui::TextureOptions::LINEAR),
            };
            v.upload_ms = Some(t0.elapsed().as_secs_f64() * 1000.0);
            ui.ctx()
                .data_mut(|d| d.insert_temp(id, (frame.id, t.clone())));
            t
        }
    };
    painter.image(
        tex.id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
}

fn problems(ui: &mut egui::Ui, v: &Viewport3d) {
    let Some(r) = &v.report else {
        if let Some(e) = &v.render_error {
            ui.colored_label(RED, format!("Render error: {e}"));
        }
        return;
    };
    egui::ScrollArea::vertical()
        .id_salt("viewport3d-problems")
        .max_height(BELOW - 30.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            if let Some(e) = &v.render_error {
                ui.colored_label(RED, format!("Render error: {e}"));
            }
            for e in r.compile_errors.iter().take(MAX_LISTED) {
                ui.colored_label(RED, format!("Compile error: {e}"));
            }
            for e in r.errors.iter().take(MAX_LISTED) {
                ui.colored_label(RED, format!("Error: {e}"));
            }
            // A recompile after a toggle can fail on a loaded mesh; the old look stays, so say why.
            for i in v.shapes.iter().filter(|_| v.is_mesh()) {
                if let ShapeStatus::Failed { error, .. } = &i.status {
                    let kept = if matches!(
                        i.status,
                        ShapeStatus::Failed {
                            showing_previous: true,
                            ..
                        }
                    ) {
                        " (previous look kept)"
                    } else {
                        ""
                    };
                    ui.colored_label(RED, format!("{}: shader failed: {error}{kept}", i.name));
                }
            }
            for t in r.missing_textures.iter().take(MAX_LISTED) {
                ui.colored_label(AMBER, format!("Missing texture: {t}"));
            }
            if !r.warnings.is_empty() {
                egui::CollapsingHeader::new(format!("{} notes", r.warnings.len()))
                    .id_salt("viewport3d-warnings")
                    .show(ui, |ui| {
                        for w in r.warnings.iter().take(MAX_LISTED) {
                            ui.weak(w);
                        }
                    });
            }
        });
}

fn footer(ui: &mut egui::Ui, v: &Viewport3d) {
    ui.horizontal_wrapped(|ui| {
        ui.weak(off_line(&v.off_in_preview, CUBEMAPS_FOOTER));
        if let Some(c) = &v.coverage {
            ui.weak(format!("· {c}"));
        }
        if let Some(t) = v.timing {
            let up = v.upload_ms.unwrap_or(0.0);
            ui.weak(format!(
                "· frame {:.1} ms (draw {:.1} · GPU {:.1} · readback {:.1} · upload {:.1})",
                t.draw_ms + t.gpu_ms + t.readback_ms + up,
                t.draw_ms,
                t.gpu_ms,
                t.readback_ms,
                up
            ));
        }
        if let Some((name, warp)) = &v.adapter {
            if *warp {
                ui.colored_label(AMBER, format!("· {name} (software renderer)"));
            } else {
                ui.weak(format!("· {name}"));
            }
        }
    });
}

fn popup(ctx: &egui::Context, v: &mut Viewport3d) {
    let Some(Change::Moved { old, new }) = v.popup.clone() else {
        return;
    };
    let (mut use_new, mut choose, mut keep) = (false, false, false);
    egui::Modal::new(egui::Id::new("viewport3d-moved")).show(ctx, |ui| {
        ui.set_max_width(520.0);
        ui.label(moved_text(&old, &new));
        ui.horizontal(|ui| {
            use_new = ui.button(USE_NEW).clicked();
            choose = ui.button(CHOOSE_FOLDER).clicked();
            if old.root.is_dir() {
                keep = ui.button(KEEP_OLD).clicked();
            }
        });
    });
    if use_new {
        v.use_new();
    } else if keep {
        v.keep_old();
    } else if choose {
        if let Some(p) = pick_folder() {
            v.popup = None;
            v.choose_folder(p);
        }
    }
}
