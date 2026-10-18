//! The set editor's Community Shaders PBR records (design §4, §6): the PBR Landscape card and the generic
//! "CS PBR records" panel (Note BM). Per kind a block with its template picker, fields, EDID list, warnings
//! and (texture sets) the statics line or the PGPatcher box. Drawing only: every change is a model call made after
//! drawing.
use super::columns::{AMBER, RED};
use crate::model::cs_pbr::{
    check_edid, check_text, field_label, fields, pgp_box_text, single_pass_text, statics_text,
    templates, CsBlock, EdidCheck, Field, FieldKind, Kind, Statics, StaticsStatus, GLINT_FIELDS,
    LANDSCAPE_UNREAD, NOT_CHECKED, NO_TXST_USES, PGP_BOX, PGP_BOX_UNAVAILABLE,
};
use crate::model::cs_pbr::{field_note, mato_synced, SHARED_FIELDS};
use crate::model::cs_pbr_export::{refusals, MATO_SPECULAR};
use crate::model::plugin_refs::set_key;
use crate::model::sets::{is_json_set, is_pbr_landscape, SetView};
use crate::model::AppModel;
use eframe::egui;
use serde_json::{json, Value as Json};

pub const SECTION: &str = "CS PBR records";
/// Note BM: the PBR Landscape card's title.
pub const CARD: &str = "PBR Landscape";
/// Note BM: after the count of texture-set rows the card hides.
pub const HIDDEN_ROWS: &str = "fields Community Shaders' landscape shader does not read are hidden; they show while \"Also write into the PGPatcher JSON\" is on (meshes sharing the texture read them).";

enum Act {
    Add(Kind),
    /// Note BM16: the card's add, which starts the material object on the shared values.
    AddMato,
    Remove(Kind),
    Value(Kind, String, Option<Json>),
    /// Note BM16: a Shared-section edit.
    Shared(String, Option<Json>),
    /// Note BM16: Sync (true) / Unsync (false) the material object's shared field.
    Sync(&'static str, bool),
    Edids(Kind, Vec<String>),
    Template(Kind, String),
    Statics(Statics),
    Refresh,
}

/// The generic records panel, collapsed, for any CS set but a PBR Landscape set, whose card edits the same blocks
/// (Note BM: hidden there, so the blocks are never edited in two places).
pub fn section(ui: &mut egui::Ui, model: &mut AppModel, v: &SetView) {
    if model.target == engine::config::Target::Cs && !is_pbr_landscape(v) {
        draw(ui, model, v, false);
    }
}

/// Note BM: the PBR Landscape card: the shared fields, the texture set (always shown; the first edit creates the
/// block) with the PGPatcher box in place of the statics choice, and the material object.
pub fn card(ui: &mut egui::Ui, model: &mut AppModel, v: &SetView) {
    if model.target == engine::config::Target::Cs && is_pbr_landscape(v) {
        draw(ui, model, v, true);
    }
}

/// Note BM16: the card's Shared section heading.
pub const SHARED: &str = "Shared (texture set, material object and PGPatcher JSON)";

fn draw(ui: &mut egui::Ui, model: &mut AppModel, v: &SetView, card: bool) {
    let mut acts = vec![];
    let m: &AppModel = model;
    // Layout v2 A3 (A12): the records panel counts its blocks in the header; the card has no header value.
    let n = [Kind::Txst, Kind::Mato]
        .into_iter()
        .filter(|k| m.cs_block(&v.id, *k).is_some())
        .count();
    let head = (!card).then(|| egui::RichText::new(format!("{n} records")).weak());
    let title = if card { CARD } else { SECTION };
    super::group::group(
        ui,
        ("cs-pbr", card, v.id.as_str()),
        title,
        head,
        card,
        |ui| {
            if ui.small_button("Refresh scan").clicked() {
                acts.push(Act::Refresh);
            }
            if card {
                shared(ui, m, v, &mut acts);
            }
            for kind in [Kind::Txst, Kind::Mato] {
                ui.separator();
                let always = card && kind == Kind::Txst;
                match m.cs_block(&v.id, kind) {
                    None if !always => {
                        if ui.button(format!("Add {}", kind.label())).clicked() {
                            acts.push(if card { Act::AddMato } else { Act::Add(kind) });
                        }
                    }
                    b => block(ui, m, v, kind, &b.unwrap_or_default(), card, &mut acts),
                }
            }
        },
    );
    for a in acts {
        match a {
            Act::Add(k) => model.add_cs_block(&v.id, k),
            Act::AddMato => model.add_mato_synced(&v.id),
            Act::Remove(k) => model.remove_cs_block(&v.id, k),
            Act::Value(k, key, x) => model.set_cs_value(&v.id, k, &key, x),
            Act::Shared(key, x) => model.set_shared_value(&v.id, &key, x),
            Act::Sync(f, on) => model.set_mato_synced(&v.id, f, on),
            Act::Edids(k, l) => model.set_cs_edids(&v.id, k, &l),
            Act::Template(k, name) => {
                let _ = model.apply_cs_template(&v.id, k, &name);
            }
            Act::Statics(s) => model.set_cs_statics(&v.id, s),
            Act::Refresh => model.cs_pbr.refresh(),
        }
    }
}

/// Note BM16: the shared fields, shown with the texture set's value and written to every block that follows it.
fn shared(ui: &mut egui::Ui, m: &AppModel, v: &SetView, acts: &mut Vec<Act>) {
    ui.separator();
    ui.strong(SHARED);
    let b = m.cs_block(&v.id, Kind::Txst).unwrap_or_default();
    let mut own = vec![];
    for f in fields(Kind::Txst)
        .iter()
        .filter(|f| SHARED_FIELDS.contains(&f.name))
    {
        field_row(ui, Kind::Txst, f, b.values.get(f.name), &mut own);
    }
    acts.extend(own.into_iter().map(|a| match a {
        Act::Value(_, k, x) => Act::Shared(k, x),
        a => a,
    }));
}

/// Note BM16: a material object's shared field: "shared (above)" with Unsync, or its own row with Sync.
fn mato_shared(ui: &mut egui::Ui, m: &AppModel, v: &SetView, b: &CsBlock, acts: &mut Vec<Act>) {
    let txst = m.cs_block(&v.id, Kind::Txst);
    for f in fields(Kind::Mato)
        .iter()
        .filter(|f| SHARED_FIELDS.contains(&f.name))
    {
        if mato_synced(txst.as_ref(), b, f.name) {
            ui.horizontal_wrapped(|ui| {
                ui.weak(format!("{}: shared (above)", field_label(f.name)));
                if ui
                    .small_button("Unsync")
                    .on_hover_text("Give the material object its own value")
                    .clicked()
                {
                    acts.push(Act::Sync(f.name, false));
                }
            });
        } else {
            field_row(ui, Kind::Mato, f, b.values.get(f.name), acts);
            if ui
                .small_button(format!("Sync {}", field_label(f.name)))
                .on_hover_text("Use the shared value again")
                .clicked()
            {
                acts.push(Act::Sync(f.name, true));
            }
        }
    }
}

/// `card`: drawn in the PBR Landscape card, where the texture set has no Remove and the PGPatcher box replaces the
/// statics choice.
fn block(
    ui: &mut egui::Ui,
    m: &AppModel,
    v: &SetView,
    kind: Kind,
    b: &CsBlock,
    card: bool,
    acts: &mut Vec<Act>,
) {
    let removable = !(card && kind == Kind::Txst);
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
        if removable && ui.small_button("Remove").clicked() {
            acts.push(Act::Remove(kind));
        }
    });
    // Note BM: in the card the box comes first, since it decides which texture-set rows apply.
    let landscape = card && kind == Kind::Txst;
    if landscape {
        pgp_box(ui, m, v, b, acts);
    }
    let pgp = landscape && m.pgp_written(v);
    let mut hidden = 0;
    for f in fields(kind) {
        // Note BM16: in the card the shared fields are edited in the Shared section (and below for an unsynced block).
        if card && SHARED_FIELDS.contains(&f.name) {
            continue;
        }
        if landscape && !pgp && LANDSCAPE_UNREAD.contains(&f.name) && !b.values.contains_key(f.name)
        {
            hidden += 1;
            continue;
        }
        field_row(ui, kind, f, b.values.get(f.name), acts);
    }
    if card && kind == Kind::Mato {
        mato_shared(ui, m, v, b, acts);
    }
    if hidden > 0 {
        ui.weak(format!("{hidden} {HIDDEN_ROWS}"));
    }
    for k in &b.bad {
        ui.colored_label(
            AMBER,
            format!("{k}: wrong type in sdc-project.toml; not exported"),
        );
    }
    edids(ui, m, v, kind, b, acts);
    if kind == Kind::Txst && !card {
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
        if ui
            .checkbox(&mut on, field_label(f.name))
            .on_hover_text(match field_note(kind, f.name) {
                Some(n) => format!("JSON key: {}. {n}", f.name),
                None => format!("JSON key: {}", f.name),
            })
            .changed()
        {
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
                    ui.label(field_label(g.name))
                        .on_hover_text(format!("JSON key: {}.{}", f.name, g.name));
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
    let recs = m.scan_records();
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
        Kind::Txst => match set_key(v).map(|k| m.scan_facts(&k)) {
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

/// Note BM6: ticked = the set's PGPatcher entry is written (`pgp_written`). A click stores always/never; "Back to
/// auto" clears it. A set that is not a PGPatcher JSON set (its colour map is not `pbr_albedo`) gets the box off and
/// disabled, with the reason (review I2).
fn pgp_box(ui: &mut egui::Ui, m: &AppModel, v: &SetView, b: &CsBlock, acts: &mut Vec<Act>) {
    let json = is_json_set(v, m.target);
    let st = m.statics_status(v);
    let mut on = m.pgp_written(v);
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(json, egui::Checkbox::new(&mut on, PGP_BOX))
            .changed()
        {
            acts.push(Act::Statics(if on {
                Statics::Always
            } else {
                Statics::Never
            }));
        }
        if json && b.statics != Statics::Auto && ui.small_button("Back to auto").clicked() {
            acts.push(Act::Statics(Statics::Auto));
        }
    });
    if !json {
        ui.colored_label(AMBER, PGP_BOX_UNAVAILABLE);
        return;
    }
    ui.weak(pgp_box_text(st.as_ref()));
    meshes(ui, v, st.as_ref());
}

fn meshes(ui: &mut egui::Ui, v: &SetView, st: Option<&StaticsStatus>) {
    if let Some(StaticsStatus::Used(paths)) = st {
        egui::CollapsingHeader::new("Meshes")
            .id_salt(("cs-statics", v.id.as_str()))
            .show(ui, |ui| {
                for p in paths {
                    ui.monospace(p);
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
    let st = m.statics_status(v);
    if let Some(s) = &st {
        ui.label(statics_text(s));
    }
    meshes(ui, v, st.as_ref());
}

/// Note BM (review 7): the TruePBR panel's header on a PBR Landscape set.
pub const PGP_PANEL: &str = "PGPatcher JSON settings";
/// Kept short: an Extend-wrapped title wider than the 300 px panel widens every group below it.
pub const PGP_PANEL_OFF: &str = "PGPatcher JSON settings (not written)";
pub const PGP_PANEL_OFF_NOTE: &str =
    "Not written: tick 'Also write into the PGPatcher JSON' above.";
pub const PGP_PANEL_NOTE: &str =
    "Roughness, specular, displacement, subsurface, coat and glint are the texture set's values above.";

/// The set's TruePBR (PGPatcher) panel. On a PBR Landscape set it sits under "PGPatcher JSON settings": open with a
/// note while the entry is written, collapsed and marked "not written" while it is not (never hidden: matching,
/// slots and the unmapped keys stay editable).
pub fn pgp_panel(ui: &mut egui::Ui, model: &mut AppModel, v: &SetView) {
    if model.target != engine::config::Target::Cs || !is_pbr_landscape(v) {
        return super::pgp_fields::panel(ui, model, &v.id);
    }
    let on = model.pgp_written(v);
    let title = if on { PGP_PANEL } else { PGP_PANEL_OFF };
    super::group::group(
        ui,
        ("pgp-panel", on, v.id.as_str()),
        title,
        None,
        on,
        |ui| {
            ui.weak(if on {
                PGP_PANEL_NOTE
            } else {
                PGP_PANEL_OFF_NOTE
            });
            super::pgp_fields::panel(ui, model, &v.id);
        },
    );
}
