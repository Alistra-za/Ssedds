//! The set editor's Community Shaders PBR section (design §4, §6): per kind a block with its template picker,
//! fields, EDID list, warnings and (texture sets) the statics line. Drawing only: every change is a model call made
//! after drawing.
use super::columns::{AMBER, RED};
use crate::model::cs_pbr::{
    check_edid, check_text, fields, single_pass_text, statics_text, templates, CsBlock, EdidCheck,
    Field, FieldKind, Kind, Statics, StaticsStatus, GLINT_FIELDS, NOT_CHECKED, NO_TXST_USES,
};
use crate::model::cs_pbr_export::{refusals, MATO_SPECULAR};
use crate::model::plugin_refs::set_key;
use crate::model::sets::SetView;
use crate::model::AppModel;
use eframe::egui;
use serde_json::{json, Value as Json};

pub const SECTION: &str = "Community Shaders PBR records";

enum Act {
    Add(Kind),
    Remove(Kind),
    Value(Kind, String, Option<Json>),
    Edids(Kind, Vec<String>),
    Template(Kind, String),
    Statics(Statics),
    Refresh,
}

pub fn section(ui: &mut egui::Ui, model: &mut AppModel, v: &SetView) {
    if model.target != engine::config::Target::Cs {
        return;
    }
    let mut acts = vec![];
    let m: &AppModel = model;
    egui::CollapsingHeader::new(SECTION)
        .id_salt(("cs-pbr", v.id.as_str()))
        .show(ui, |ui| {
            if ui.small_button("Refresh scan").clicked() {
                acts.push(Act::Refresh);
            }
            for kind in [Kind::Txst, Kind::Mato] {
                ui.separator();
                match m.cs_block(&v.id, kind) {
                    None => {
                        if ui.button(format!("Add {}", kind.label())).clicked() {
                            acts.push(Act::Add(kind));
                        }
                    }
                    Some(b) => block(ui, m, v, kind, &b, &mut acts),
                }
            }
        });
    for a in acts {
        match a {
            Act::Add(k) => model.add_cs_block(&v.id, k),
            Act::Remove(k) => model.remove_cs_block(&v.id, k),
            Act::Value(k, key, x) => model.set_cs_value(&v.id, k, &key, x),
            Act::Edids(k, l) => model.set_cs_edids(&v.id, k, &l),
            Act::Template(k, name) => {
                let _ = model.apply_cs_template(&v.id, k, &name);
            }
            Act::Statics(s) => model.set_cs_statics(&v.id, s),
            Act::Refresh => model.cs_pbr.refresh(),
        }
    }
}

fn block(
    ui: &mut egui::Ui,
    m: &AppModel,
    v: &SetView,
    kind: Kind,
    b: &CsBlock,
    acts: &mut Vec<Act>,
) {
    ui.horizontal(|ui| {
        ui.strong(kind.label());
        egui::ComboBox::from_id_salt(("cs-template", kind.key(), v.id.as_str()))
            .selected_text(b.template.clone().unwrap_or_else(|| "Template…".into()))
            .show_ui(ui, |ui| {
                for t in templates().iter().filter(|t| t.kind == kind) {
                    let sel = b.template.as_deref() == Some(t.name.as_str());
                    if ui
                        .selectable_label(sel, format!("{} ({})", t.name, t.edids.join(", ")))
                        .clicked()
                    {
                        acts.push(Act::Template(kind, t.name.clone()));
                    }
                }
            });
        if ui.small_button("Remove").clicked() {
            acts.push(Act::Remove(kind));
        }
    });
    for f in fields(kind) {
        field_row(ui, kind, f, b.values.get(f.name), acts);
    }
    for k in &b.bad {
        ui.colored_label(
            AMBER,
            format!("{k}: wrong type in sdc-project.toml; not exported"),
        );
    }
    edids(ui, m, v, kind, b, acts);
    if kind == Kind::Txst {
        statics(ui, m, v, b, acts);
    }
}

fn default_of(f: &Field) -> Json {
    match f.kind {
        FieldKind::Num => json!(f.default),
        FieldKind::Rgb => json!([f.default, f.default, f.default]),
        FieldKind::Glint => json!({"enabled": true}),
    }
}

fn default_text(kind: Kind, f: &Field) -> String {
    match f.kind {
        _ if kind == Kind::Mato && f.name == "specularLevel" => {
            format!("written as {MATO_SPECULAR} (CS default {})", f.default)
        }
        FieldKind::Glint => "CS default: off".into(),
        FieldKind::Rgb => format!("CS default: {0}, {0}, {0}", f.default),
        FieldKind::Num => format!("CS default: {}", f.default),
    }
}

fn drag(ui: &mut egui::Ui, x: &mut f64, f: &Field) -> bool {
    ui.add(
        egui::DragValue::new(x)
            .range(f.range.0..=f.range.1)
            .speed(0.01),
    )
    .changed()
}

fn field_row(ui: &mut egui::Ui, kind: Kind, f: &Field, cur: Option<&Json>, acts: &mut Vec<Act>) {
    ui.horizontal_wrapped(|ui| {
        let mut on = cur.is_some();
        if ui.checkbox(&mut on, f.name).changed() {
            acts.push(Act::Value(kind, f.name.into(), on.then(|| default_of(f))));
        }
        let Some(cur) = cur else {
            ui.weak(default_text(kind, f));
            return;
        };
        match f.kind {
            FieldKind::Num => {
                let mut x = cur.as_f64().unwrap_or(f.default);
                if drag(ui, &mut x, f) {
                    acts.push(Act::Value(kind, f.name.into(), Some(json!(x))));
                }
            }
            FieldKind::Rgb => {
                let mut c: Vec<f64> = cur
                    .as_array()
                    .map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0)).collect())
                    .unwrap_or_default();
                let mut changed = false;
                for x in c.iter_mut() {
                    changed |= drag(ui, x, f);
                }
                if changed {
                    acts.push(Act::Value(kind, f.name.into(), Some(json!(c))));
                }
            }
            FieldKind::Glint => {
                let mut en = cur.get("enabled").and_then(Json::as_bool).unwrap_or(false);
                if ui.checkbox(&mut en, "enabled").changed() {
                    acts.push(Act::Value(
                        kind,
                        format!("{}.enabled", f.name),
                        Some(json!(en)),
                    ));
                }
                for g in &GLINT_FIELDS {
                    let mut x = cur.get(g.name).and_then(Json::as_f64).unwrap_or(g.default);
                    ui.label(g.name);
                    if drag(ui, &mut x, g) {
                        acts.push(Act::Value(
                            kind,
                            format!("{}.{}", f.name, g.name),
                            Some(json!(x)),
                        ));
                    }
                }
            }
        }
    });
}

fn edids(
    ui: &mut egui::Ui,
    m: &AppModel,
    v: &SetView,
    kind: Kind,
    b: &CsBlock,
    acts: &mut Vec<Act>,
) {
    let refused = refusals(&m.sets_doc.as_ref().map(|d| d.entries()).unwrap_or_default());
    let recs = m.cs_pbr.records(m.scan_db.as_deref());
    let scanned = recs.as_ref().ok().filter(|r| !r.of(kind).is_empty());
    ui.label("EDIDs (the file names, exact case):");
    for (i, e) in b.edids.iter().enumerate() {
        ui.horizontal_wrapped(|ui| {
            ui.monospace(e);
            if ui
                .small_button("×")
                .on_hover_text("Remove this EDID")
                .clicked()
            {
                let mut l = b.edids.clone();
                l.remove(i);
                acts.push(Act::Edids(kind, l));
            }
            if let Some(why) = refused.get(&(v.id.clone(), kind, e.clone())) {
                ui.colored_label(RED, format!("not written: {why}"));
            }
            if let Some(r) = scanned {
                let c = check_edid(kind, e, r.of(kind));
                if let Some(t) = check_text(&c) {
                    ui.colored_label(AMBER, t);
                }
                if let EdidCheck::Case(exact) = &c {
                    if ui.small_button(format!("Use {exact}")).clicked() {
                        let mut l = b.edids.clone();
                        l[i] = exact.clone();
                        acts.push(Act::Edids(kind, l));
                    }
                }
            }
        });
    }
    if scanned.is_none() {
        ui.weak(NOT_CHECKED);
    }
    let id = ui.make_persistent_id(("cs-edid-text", kind.key(), v.id.as_str()));
    let mut text: String = ui.data_mut(|d| d.get_temp(id).unwrap_or_default());
    ui.horizontal(|ui| {
        ui.text_edit_singleline(&mut text);
        if ui.button("Add EDID").clicked() && !text.trim().is_empty() {
            let mut l = b.edids.clone();
            l.push(text.trim().to_string());
            acts.push(Act::Edids(kind, l));
            text.clear();
        }
    });
    let suggestions: Vec<(String, String)> = match kind {
        Kind::Txst => match set_key(v).map(|k| m.cs_pbr.facts(m.scan_db.as_deref(), &k)) {
            Some(Ok(f)) => {
                if f.txst_edids.is_empty() {
                    ui.colored_label(AMBER, NO_TXST_USES);
                }
                f.txst_edids.into_iter().map(|e| (e.clone(), e)).collect()
            }
            _ => vec![],
        },
        Kind::Mato => scanned
            .map(|r| {
                let q = text.to_lowercase();
                r.mato
                    .iter()
                    .filter(|w| w.edid.to_lowercase().contains(&q))
                    .take(50)
                    .map(|w| {
                        let label = format!(
                            "{} · {} · {}",
                            w.edid,
                            w.plugin,
                            single_pass_text(w.mato_flags)
                        );
                        (w.edid.clone(), label)
                    })
                    .collect()
            })
            .unwrap_or_default(),
    };
    ui.data_mut(|d| d.insert_temp(id, text));
    let new: Vec<_> = suggestions
        .into_iter()
        .filter(|(e, _)| !b.edids.contains(e))
        .collect();
    if !new.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.weak("From the scan:");
            for (e, label) in new {
                if ui.small_button(format!("+ {label}")).clicked() {
                    let mut l = b.edids.clone();
                    l.push(e);
                    acts.push(Act::Edids(kind, l));
                }
            }
        });
    }
}

fn statics(ui: &mut egui::Ui, m: &AppModel, v: &SetView, b: &CsBlock, acts: &mut Vec<Act>) {
    ui.horizontal(|ui| {
        ui.label("PGPatcher entry for statics:");
        for (s, name) in [
            (Statics::Auto, "auto"),
            (Statics::Always, "always"),
            (Statics::Never, "never"),
        ] {
            if ui.selectable_label(b.statics == s, name).clicked() && b.statics != s {
                acts.push(Act::Statics(s));
            }
        }
    });
    if let Some(st) = m.statics_status(v) {
        ui.label(statics_text(&st));
        if let StaticsStatus::Used(paths) = &st {
            egui::CollapsingHeader::new("Meshes")
                .id_salt(("cs-statics", v.id.as_str()))
                .show(ui, |ui| {
                    for p in paths {
                        ui.monospace(p);
                    }
                });
        }
    }
}
