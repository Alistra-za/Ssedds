//! M6 Track F2: the RNM section of a base normal's settings (F2 spec §5.2).
use super::columns::AMBER;
use super::thumbs::ThumbCache;
use crate::model::rnm::{ao_line, order_after_move, rnm_rows, RnmRow, AO_NOTE};
use crate::model::AppModel;
use eframe::egui;
use engine::config::{BentMode, LayerId, RnmTarget};
use engine::policy::ResolvedJob;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// The outline of a normal card whose RNM layers are blended in; blue, so it never reads as the amber warning.
pub const RNM_OUTLINE: egui::Color32 = egui::Color32::from_rgb(90, 150, 230);
const THUMB: f32 = 32.0;

/// (item key, field, value); `None` clears the field. Applied after the drawing, as the settings panel does.
type Edits = Vec<(String, String, Option<Value>)>;

fn shared_thumbs(ctx: &egui::Context) -> Arc<Mutex<ThumbCache>> {
    ctx.data_mut(|d| {
        d.get_temp_mut_or_insert_with(egui::Id::new("rnm-thumbs"), || {
            Arc::new(Mutex::new(ThumbCache::default()))
        })
        .clone()
    })
}

fn thumb(ui: &mut egui::Ui, model: &AppModel, path: &Path, invert: Option<usize>) {
    let t = model
        .session
        .as_ref()
        .and_then(|s| s.cache().thumbnail_of(path));
    let Some(t) = t else {
        ui.add_space(THUMB);
        return;
    };
    let cache = shared_thumbs(ui.ctx());
    let tex = {
        let mut c = cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        c.sweep(model.items_rev());
        c.get_raw(ui.ctx(), &path.to_string_lossy(), &t, invert)
    };
    ui.add(egui::Image::new(&tex).fit_to_exact_size(egui::vec2(THUMB, THUMB)));
}

fn target_label(t: RnmTarget) -> &'static str {
    match t {
        RnmTarget::N => "Normal",
        RnmTarget::Cnr => "CNR",
        RnmTarget::Both => "Both",
    }
}

/// A checkbox that queues `field = value` only when it was toggled.
fn check(ui: &mut egui::Ui, edits: &mut Edits, key: &str, field: String, on: bool, text: &str) {
    let mut v = on;
    if ui.checkbox(&mut v, text).changed() {
        edits.push((key.into(), field, Some(json!(v))));
    }
}

fn row(
    ui: &mut egui::Ui,
    model: &AppModel,
    key: &str,
    r: &RnmRow,
    both: bool,
    job: &ResolvedJob,
    edits: &mut Edits,
) {
    let id = r.id.id();
    ui.horizontal(|ui| {
        thumb(ui, model, &r.path, None);
        ui.label(r.label);
        let sid = egui::Id::new(("rnm-strength", key, id));
        if let Some(v) = drag_slider(ui, model.busy(), sid, r.strength, 0.0..=2.0, "") {
            let v = json!(v);
            edits.push((key.into(), format!("rnm.{id}.strength"), Some(v)));
        }
        let flip = format!("rnm.{id}.flip_green");
        check(ui, edits, key, flip, r.flip_green, "flip green");
        match &r.mask {
            Some((p, used)) => {
                thumb(ui, model, p, None);
                check(ui, edits, key, format!("rnm.{id}.mask"), *used, "mask");
            }
            None => {
                ui.weak("no mask");
            }
        }
        for (arrow, up) in [("▲", true), ("▼", false)] {
            if ui.button(arrow).clicked() {
                let new = order_after_move(job, r.id, up);
                if new != job.profile.rnm.effective_order() {
                    let ids: Vec<&str> = new.iter().map(|i| i.id()).collect();
                    edits.push((key.into(), "rnm.order".into(), Some(json!(ids))));
                }
            }
        }
        if both {
            let mut t = r.target;
            egui::ComboBox::from_id_salt(("rnm-target", id))
                .selected_text(target_label(t))
                .show_ui(ui, |ui| {
                    for o in [RnmTarget::N, RnmTarget::Cnr, RnmTarget::Both] {
                        ui.selectable_value(&mut t, o, target_label(o));
                    }
                });
            if t != r.target {
                // Keyed on the layer file, not the base (Note W11).
                edits.push((r.key.clone(), "rnm.target".into(), Some(json!(t))));
            }
        }
    });
}

fn bent(ui: &mut egui::Ui, model: &AppModel, key: &str, job: &ResolvedJob, edits: &mut Edits) {
    let b = &job.profile.rnm.bent;
    ui.horizontal(|ui| {
        let mut mode = b.mode;
        egui::ComboBox::from_id_salt("rnm-bent-mode")
            .selected_text(if mode == BentMode::AoBlend {
                "AO blend"
            } else {
                "Like a detail"
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut mode, BentMode::AoBlend, "AO blend");
                ui.selectable_value(&mut mode, BentMode::Detail, "Like a detail");
            });
        if mode != b.mode {
            edits.push((key.into(), "rnm.bent.mode".into(), Some(json!(mode))));
        }
        let pid = egui::Id::new(("rnm-ao-power", key));
        if let Some(p) = drag_slider(
            ui,
            model.busy(),
            pid,
            job.profile.rnm.ao_power(),
            0.1..=8.0,
            "AO power",
        ) {
            let p = json!(p);
            edits.push((key.into(), "rnm.bent.ao_power".into(), Some(p)));
        }
    });
    if let Some((path, channel, used)) = ao_line(job) {
        ui.horizontal(|ui| {
            thumb(ui, model, &path, Some(usize::from(channel)));
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            ui.label(if channel == 2 {
                format!("{name} (B channel)")
            } else {
                name.to_string()
            });
            check(ui, edits, key, "rnm.bent.ao".into(), used, "use AO");
        });
        ui.weak(AO_NOTE);
    }
}

pub fn rnm_section(ui: &mut egui::Ui, model: &mut AppModel, key: &str) {
    let Some(item) = model.item(key) else { return };
    let Some(job) = item.job.clone().filter(|j| j.rnm.is_some()) else {
        return;
    };
    let warnings = item.warnings.clone();
    let both = item.set_key.as_ref().is_some_and(|sk| {
        let has = |t| {
            model.items.iter().any(|i| {
                i.set_key.as_ref() == Some(sk)
                    && i.job.as_ref().and_then(|j| j.profile.subtype.rnm_base()) == Some(t)
            })
        };
        has(RnmTarget::N) && has(RnmTarget::Cnr)
    });
    let targets = |k: &str| {
        model
            .project
            .as_ref()
            .and_then(|p| p.file_value(k, "rnm.target"))
            .and_then(|v| serde_json::from_value::<RnmTarget>(v).ok())
    };
    let rows = rnm_rows(&job, &model.rnm_keys, &targets);
    let mut edits: Edits = vec![];
    ui.add_space(6.0);
    egui::CollapsingHeader::new("RNM")
        .default_open(true)
        .show(ui, |ui| {
            let mut on = job.profile.rnm.enabled;
            if ui.checkbox(&mut on, "RNM").changed() {
                // Re-ticking drops the key rather than writing `true`, the default.
                edits.push((
                    key.into(),
                    "rnm.enabled".into(),
                    (!on).then_some(json!(false)),
                ));
            }
            if job.active_rnm().is_some() {
                blended_preview(ui, model, key);
            }
            for r in &rows {
                row(ui, model, key, r, both, &job, &mut edits);
                if r.id == LayerId::Bent {
                    bent(ui, model, key, &job, &mut edits);
                }
            }
            for w in warnings
                .iter()
                .filter(|w| w.code.as_str().starts_with("RNM-"))
            {
                ui.colored_label(AMBER, format!("{}: {}", w.code.as_str(), w.message));
            }
        });
    for (k, f, v) in edits {
        match v {
            Some(v) => model.set_field(&k, &f, v),
            None => model.clear_field(&k, &f),
        }
    }
}

/// A slider that returns its value once per gesture (drag release, click or key press), not every frame; the
/// value in flight lives in `ui.data` under `id`. As settings_panel::commit_slider does, a value already sent
/// (`committed`) stays on show while the re-plan runs (`busy`), so the slider does not snap back after release.
fn drag_slider(
    ui: &mut egui::Ui,
    busy: bool,
    id: egui::Id,
    current: f64,
    range: std::ops::RangeInclusive<f64>,
    text: &str,
) -> Option<f64> {
    let mut pending: Option<(f64, bool)> = ui.data(|d| d.get_temp(id));
    if pending.is_some_and(|(v, sent)| v == current || (sent && !busy)) {
        pending = None;
    }
    let mut v = pending.map_or(current, |p| p.0);
    let r = ui.add(egui::Slider::new(&mut v, range.clone()).text(text));
    if r.changed() {
        pending = Some((v, false));
    }
    let mut out = None;
    if r.drag_stopped() || (r.changed() && !r.dragged()) {
        let v = v.clamp(*range.start(), *range.end());
        if v != current {
            out = Some(v);
            pending = Some((v, true));
        }
    }
    ui.data_mut(|d| match pending {
        Some(p) => {
            d.insert_temp(id, p);
        }
        None => d.remove::<(f64, bool)>(id),
    });
    out
}

/// The blended preview's texture: one slot, holding the `Arc` it was made from (so its pointer cannot be reused by
/// another picture) and replaced when the key or the picture changes, which frees the last one (audit 3 item 12).
pub fn preview_texture(
    ctx: &egui::Context,
    key: &str,
    t: &Arc<crate::model::rnm_preview::RnmThumb>,
) -> egui::TextureHandle {
    let id = egui::Id::new("rnm-preview-tex");
    type Slot = (
        String,
        Arc<crate::model::rnm_preview::RnmThumb>,
        egui::TextureHandle,
    );
    if let Some((k, old, h)) = ctx.data(|d| d.get_temp::<Slot>(id)) {
        if k == key && Arc::ptr_eq(&old, t) {
            return h;
        }
    }
    let img =
        egui::ColorImage::from_rgba_unmultiplied([t.width as usize, t.height as usize], &t.rgba);
    let h = ctx.load_texture(format!("rnm-preview-{key}"), img, Default::default());
    ctx.data_mut(|d| d.insert_temp(id, (key.to_string(), t.clone(), h.clone())));
    h
}

/// Item 21 (DECISIONS AG115): the set's blended normal as a 128 px thumbnail, `(n + 1) / 2` as colour. The job runs
/// off the UI thread; the picture on show stays until the one for the new settings lands.
fn blended_preview(ui: &mut egui::Ui, model: &mut AppModel, key: &str) {
    model.rnm_preview_frame(key);
    ui.label("Blended normal");
    if let Some(t) = model.rnm_preview_image(key) {
        let tex = preview_texture(ui.ctx(), key, &t);
        ui.add(
            egui::Image::new(&tex).fit_to_exact_size(egui::vec2(t.width as f32, t.height as f32)),
        );
    } else if model.rnm_preview_failed(key) {
        ui.weak("Preview unavailable: a layer could not be read.");
    } else {
        ui.weak("Blending…");
    }
}
