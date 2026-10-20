//! The NIF edit panel (spec §7, plan A17): what is edited and from where, Solo, Undo / Redo, the ops that did not
//! apply, then the Shader, Alpha, Controllers and Extra data tabs.
use super::edit_fields::{self, Act};
use super::{edit_ctrl, edit_extra};
use crate::model::meshes::edit_doc::OpStatus;
use crate::model::meshes::nif_edit::NifEdit;
use crate::model::meshes::MeshesModel;
use crate::ui::status::{AMBER, RED};
use eframe::egui;
use nif::edit::{Op, Outcome};

const TABS: [&str; 4] = ["Shader", "Alpha", "Controllers", "Extra data"];

pub fn panel(ui: &mut egui::Ui, m: &mut MeshesModel) {
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
    let mut act = Act::default();
    if let Some(e) = &m.edit {
        top(ui, e, &mut act);
        let tab_id = egui::Id::new("meshes-edit-tab");
        let mut tab: usize = ui.data(|d| d.get_temp(tab_id)).unwrap_or(0);
        ui.horizontal(|ui| {
            for (i, t) in TABS.iter().enumerate() {
                if ui.selectable_label(tab == i, *t).clicked() {
                    tab = i;
                }
            }
        });
        ui.data_mut(|d| d.insert_temp(tab_id, tab));
        ui.separator();
        // Both ways: a wide row scrolls instead of widening the panel; thin bars stay visible without hover.
        ui.spacing_mut().scroll = egui::style::ScrollStyle::thin();
        egui::ScrollArea::both()
            .id_salt("meshes-edit-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| match tab {
                0 => edit_fields::shader_tab(ui, m, e, &mut act),
                1 => edit_fields::alpha_tab(ui, e, &mut act),
                2 => edit_ctrl::controllers_tab(ui, e, &mut act),
                _ => edit_extra::extra_tab(ui, e, &mut act),
            });
    }
    apply(m, act);
}

fn top(ui: &mut egui::Ui, e: &NifEdit, act: &mut Act) {
    ui.add(egui::Label::new(egui::RichText::new(&e.data_path).strong()).truncate());
    ui.label(format!("Source: {}", e.source_copy.origin));
    if let Some(g) = &e.generated {
        ui.add(egui::Label::new(egui::RichText::new(g).color(AMBER)).wrap());
    }
    if e.le_mesh {
        ui.colored_label(AMBER, "LE mesh: no 3D preview");
    }
    ui.horizontal(|ui| {
        let mut solo = e.solo;
        if ui.checkbox(&mut solo, "Show only this shape").changed() {
            act.solo = Some(solo);
        }
        // Distinct names: the shared header has its own Undo / Redo for the Textures history.
        for (text, name, enabled, undo) in [
            ("Undo", "Undo NIF edit", e.history.can_undo(), true),
            ("Redo", "Redo NIF edit", e.history.can_redo(), false),
        ] {
            let r = ui.add_enabled(enabled, egui::Button::new(text));
            r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, name));
            if r.clicked() {
                if undo {
                    act.undo = true;
                } else {
                    act.redo = true;
                }
            }
        }
    });
    if let Some((applied, conflicts)) = e.source_changed {
        ui.colored_label(
            AMBER,
            format!("Source changed: {applied} applied, {conflicts} conflicts"),
        );
    }
    for (i, d) in e.doc.ops.iter().enumerate() {
        if d.status != OpStatus::Conflict {
            continue;
        }
        let (why, conflict) = match e.outcomes.get(i) {
            Some(Outcome::Conflict(s)) => (s.as_str(), true),
            Some(Outcome::Refused(s)) => (s.as_str(), false),
            _ => ("not applied", false),
        };
        let text = format!("{}: {why}", op_label(&d.op));
        egui::Sides::new().shrink_left().truncate().show(
            ui,
            |ui| {
                ui.add(egui::Label::new(egui::RichText::new(&text).color(RED)).truncate());
            },
            |ui| {
                if conflict {
                    if ui.small_button("Take source").clicked() {
                        act.take_source = Some(i);
                    }
                    if ui.small_button("Keep mine").clicked() {
                        act.keep_mine = Some(i);
                    }
                } else if ui.small_button("Revert").clicked() {
                    act.revert = Some(i);
                }
            },
        );
    }
    ui.separator();
}

/// A short name of what an op changes, for the rows of ops that did not apply.
fn op_label(op: &Op) -> String {
    let (what, at) = match op {
        Op::SetField { at, field, .. } | Op::SetBits { at, field, .. } => {
            (format!("{field:?}"), at)
        }
        Op::SetTexturePath { at, slot, .. } => (format!("Texture slot {slot}"), at),
        Op::SplitAlpha { at } => ("Split alpha".into(), at),
        Op::AddAlpha { at, .. } => ("Add alpha".into(), at),
        Op::RemoveAlpha { at } => ("Remove alpha".into(), at),
        Op::AddExtra { at, name, .. } => (format!("Add extra data {name}"), at),
        Op::EditExtra { at, .. } => ("Edit extra data".into(), at),
        Op::RemoveExtra { at } => ("Remove extra data".into(), at),
    };
    match &at.shape {
        Some(s) => format!("{s}: {what}"),
        None => what,
    }
}

fn apply(m: &mut MeshesModel, act: Act) {
    if let Some((ops, merge)) = act.push {
        m.edit_push(ops, merge);
    }
    if let (true, Some(e)) = (act.flush, &mut m.edit) {
        e.flush();
    }
    if act.undo {
        m.undo_edit();
    }
    if act.redo {
        m.redo_edit();
    }
    if let Some(on) = act.solo {
        m.edit_with(|e| e.solo = on);
    }
    if let Some(on) = act.check {
        m.set_edit_check_exists(on);
    }
    if let Some(i) = act.keep_mine {
        m.edit_with(|e| e.keep_mine(i));
    }
    if let Some(i) = act.take_source {
        m.edit_with(|e| e.take_source(i));
    }
    if let Some(i) = act.revert {
        m.edit_with(|e| e.revert(i));
    }
    if act.open.is_some() {
        m.open_in_textures = act.open;
    }
}
