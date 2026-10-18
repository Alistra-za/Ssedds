//! The set editor's "PGPatcher complex material" card (complex material design 2026-10-14 §5). Drawing only: every
//! change is a model call made after drawing (one `sets_doc` edit, one undo step).
use super::columns::AMBER;
use crate::model::cm_json::{card_warnings, first_value, shows_card, targets, CmKey, CmKind, KEYS};
use crate::model::labels::readable;
use crate::model::sets::SetView;
use crate::model::AppModel;
use eframe::egui;
use engine::sets::same_id;
use serde_json::{json, Value as Json};

pub const SECTION: &str = "PGPatcher complex material";
pub const EXPORT: &str = "Export complex material JSON";
const EXPORT_HOVER: &str =
    "Writes a .json with the _m's name next to each complex _m when you export. PGPatcher \
     applies it to every shape it patches as complex material with that _m.";
/// D11.
pub const NOTE: &str = "PGPatcher sets specular colour, specular strength and environment map scale to 1 on every \
                        complex-material shape unless this file sets them. Glossiness keeps the mesh's value. Only \
                        the keys you tick are written.";
const CUBEMAP_HOVER: &str =
    "Off keeps the shape's own cubemap. Under CS an active complex material still uses the dynamic path.";

enum Act {
    Export(bool),
    Value(String, Option<Json>),
}

pub fn section(ui: &mut egui::Ui, model: &mut AppModel, v: &SetView) {
    if !shows_card(model, v) {
        return;
    }
    let mut acts = vec![];
    let m: &AppModel = model;
    let b = m.cm_block(&v.id).unwrap_or_default();
    let editable = m.sets_doc.as_ref().is_some_and(|d| !d.read_only());
    let why = m.banner.clone().unwrap_or_default();
    egui::CollapsingHeader::new(SECTION)
        .id_salt(("cm-card", v.id.as_str()))
        .show(ui, |ui| {
            // Ruling C5: the targets once per drawn card, shared by the paths and the warnings.
            let ts = targets(m);
            ui.add_enabled_ui(editable, |ui| {
                let mut on = b.export;
                if ui
                    .checkbox(&mut on, EXPORT)
                    .on_hover_text(EXPORT_HOVER)
                    .changed()
                {
                    acts.push(Act::Export(on));
                }
                for t in ts.iter().filter(|t| same_id(&t.set, &v.id)) {
                    ui.weak(match t.rel.as_deref() {
                        Ok(r) => r.replace('/', "\\"),
                        Err(_) => format!("{}.json", t.name),
                    });
                }
                ui.weak(NOTE);
                for k in &KEYS {
                    key_row(ui, k, b.values.get(k.name), &mut acts);
                }
                for w in card_warnings(m, v, &ts) {
                    ui.colored_label(AMBER, w);
                }
            })
            .response
            .on_disabled_hover_text(&why);
        });
    for a in acts {
        match a {
            Act::Export(on) => model.set_cm_export(&v.id, on),
            Act::Value(k, x) => model.set_cm_value(&v.id, &k, x),
        }
    }
}

/// `[ ] key` then its value widget once ticked; unticking removes the key (the per-key clear).
fn key_row(ui: &mut egui::Ui, k: &CmKey, cur: Option<&Json>, acts: &mut Vec<Act>) {
    ui.horizontal_wrapped(|ui| {
        let mut set = cur.is_some();
        let mut r = ui.checkbox(&mut set, readable(k.name));
        r = r.on_hover_text(if k.name == "dynamic_cubemap" {
            format!("{}. {CUBEMAP_HOVER}", k.name)
        } else {
            k.name.to_string()
        });
        if r.changed() {
            acts.push(Act::Value(k.name.into(), set.then(|| first_value(k))));
        }
        let Some(cur) = cur else {
            ui.weak(k.hint);
            return;
        };
        match k.kind {
            CmKind::Bool => {
                let mut on = cur.as_bool().unwrap_or(true);
                if ui.checkbox(&mut on, "on").changed() {
                    acts.push(Act::Value(k.name.into(), Some(json!(on))));
                }
            }
            CmKind::Num => {
                let mut x = cur.as_f64().unwrap_or(1.0);
                if drag(ui, &mut x) {
                    acts.push(Act::Value(k.name.into(), Some(json!(x))));
                }
            }
            CmKind::Rgb => {
                let mut c: Vec<f64> = cur
                    .as_array()
                    .map(|a| a.iter().map(|x| x.as_f64().unwrap_or(1.0)).collect())
                    .unwrap_or_default();
                let mut changed = false;
                for (x, l) in c.iter_mut().zip(["R", "G", "B"]) {
                    ui.label(l);
                    changed |= drag(ui, x);
                }
                if changed {
                    acts.push(Act::Value(k.name.into(), Some(json!(c))));
                }
            }
        }
    });
}

/// ≥ 0 with no upper bound (D5, D8).
fn drag(ui: &mut egui::Ui, x: &mut f64) -> bool {
    ui.add(
        egui::DragValue::new(x)
            .range(0.0..=f64::INFINITY)
            .speed(0.01),
    )
    .changed()
}
