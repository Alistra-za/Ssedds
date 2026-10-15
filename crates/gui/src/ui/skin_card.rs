//! The Advanced skin card (Task 10b, DECISIONS AG79-AG86): Community Shaders `SkinProfile` values of a skin set.
//! Drawing only; edits go through `AppModel::set_skin_value`.
use crate::model::sets::SetView;
use crate::model::skin::{Kind, FIELDS, GROUPS};
use crate::model::AppModel;
use eframe::egui;
use engine::config::Subtype;
use serde_json::{json, Value};

pub const TITLE: &str = "Advanced skin (Community Shaders)";
/// The disabled Export JSON placeholder's hover (owner, 2026-10-14).
pub const EXPORT_HOVER: &str = "Community Shaders Advanced Skin override export - spec pending";
const NOTE: &str =
    "Saved in the project; not written to Community Shaders yet (export spec pending)";

/// The set's skin members (`_rfaos`, `_sk`, `_wet`): CS binds them through its JSON, so they are listed, not edited.
fn members(model: &AppModel, v: &SetView) -> Vec<String> {
    model
        .items
        .iter()
        .filter(|i| v.members.contains(&i.key))
        .filter(|i| {
            matches!(
                i.job.as_ref().map(|j| j.profile.subtype),
                Some(Subtype::SkinRfaos | Subtype::SkinSk | Subtype::SkinWet)
            )
        })
        .map(|i| i.name.clone())
        .collect()
}

pub fn card(ui: &mut egui::Ui, model: &mut AppModel, v: &SetView) {
    let own = model.skin_values(&v.id);
    let editable = model.sets_doc.as_ref().is_some_and(|d| !d.read_only());
    let mut edits: Vec<(&str, Option<Value>)> = vec![];
    ui.add_space(6.0);
    egui::CollapsingHeader::new(egui::RichText::new(TITLE).strong())
        .id_salt(("skin-card", &v.id))
        .default_open(true)
        .show(ui, |ui| {
            ui.weak(NOTE);
            let m = members(model, v);
            ui.weak(if m.is_empty() {
                "Skin textures: none yet (_rfaos, _sk, _wet)".to_string()
            } else {
                format!("Skin textures: {}", m.join(", "))
            });
            ui.add_enabled_ui(editable, |ui| {
                for g in GROUPS {
                    ui.label(egui::RichText::new(g).strong());
                    for f in FIELDS.iter().filter(|f| f.group == g) {
                        ui.horizontal(|ui| {
                            ui.label(f.key);
                            let cur = own.get(f.key);
                            match (f.kind, cur) {
                                (Kind::Num { default, lo, hi }, Some(c)) => {
                                    let mut x = c.as_f64().unwrap_or(default);
                                    let r = ui.add(
                                        egui::DragValue::new(&mut x)
                                            .range(lo..=hi)
                                            .speed(0.01)
                                            .max_decimals(4),
                                    );
                                    if r.changed() && c.as_f64() != Some(x) {
                                        edits.push((f.key, Some(json!(x))));
                                    }
                                }
                                (Kind::Flag { default }, Some(c)) => {
                                    let mut on = c.as_bool().unwrap_or(default);
                                    if ui.checkbox(&mut on, "").changed() {
                                        edits.push((f.key, Some(json!(on))));
                                    }
                                }
                                (kind, None) => {
                                    let (value, hint) = match kind {
                                        Kind::Num { default, .. } => {
                                            (json!(default), format!("default {default:?}"))
                                        }
                                        Kind::Flag { default } => {
                                            (json!(default), format!("default {default}"))
                                        }
                                    };
                                    if ui.small_button("set").clicked() {
                                        edits.push((f.key, Some(value)));
                                    }
                                    ui.weak(hint);
                                }
                            }
                            if cur.is_some()
                                && ui
                                    .small_button("↺")
                                    .on_hover_text("Back to not set")
                                    .clicked()
                            {
                                edits.push((f.key, None));
                            }
                        });
                    }
                }
            });
            // AG90: the skin set gets no PGPatcher JSON; its own export is still to be specified.
            ui.add_space(4.0);
            ui.label(egui::RichText::new("Export JSON").strong());
            ui.add_enabled(false, egui::Button::new("Export JSON…"))
                .on_disabled_hover_text(EXPORT_HOVER);
        });
    for (k, v2) in edits {
        model.set_skin_value(&v.id, k, v2);
    }
}
