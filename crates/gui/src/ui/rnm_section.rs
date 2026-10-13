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

fn thumb(ui: &mut egui::Ui, model: &AppModel, path: &Path, invert: bool) {
    let t = model
        .session
        .as_ref()
        .and_then(|s| s.cache().thumbnail_of(path));
    let Some(t) = t else {
        ui.add_space(THUMB);
        return;
    };
    let cache = shared_thumbs(ui.ctx());
    let tex = cache
        .lock()
        .unwrap()
        .get_raw(ui.ctx(), &path.to_string_lossy(), &t, invert);
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
        thumb(ui, model, &r.path, false);
        ui.label(r.label);
        let mut v = r.strength;
        if ui.add(egui::Slider::new(&mut v, 0.0..=2.0)).changed() {
            let v = json!(v.clamp(0.0, 2.0));
            edits.push((key.into(), format!("rnm.{id}.strength"), Some(v)));
        }
        let flip = format!("rnm.{id}.flip_green");
        check(ui, edits, key, flip, r.flip_green, "flip green");
        match &r.mask {
            Some((p, used)) => {
                thumb(ui, model, p, false);
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
        let mut p = job.profile.rnm.ao_power();
        if ui
            .add(egui::Slider::new(&mut p, 0.1..=8.0).text("AO power"))
            .changed()
        {
            let p = json!(p.clamp(0.1, 8.0));
            edits.push((key.into(), "rnm.bent.ao_power".into(), Some(p)));
        }
    });
    if let Some((path, channel, used)) = ao_line(job) {
        ui.horizontal(|ui| {
            thumb(ui, model, &path, true);
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
