//! The NIF edit panel (spec §7, plan A17/A19): what is edited and from where, Solo, Undo / Redo, the pending edits,
//! Export NIF with its guard dialog, then the Shader, Alpha, Controllers and Extra data tabs.
use super::edit_fields::{self, Act};
use super::{edit_ctrl, edit_extra};
use crate::model::meshes::edit_doc::OpStatus;
use crate::model::meshes::export::{GuardChoice, Placement};
use crate::model::meshes::nif_edit::NifEdit;
use crate::model::meshes::MeshesModel;
use crate::ui::dialogs::{dialog, Role};
use crate::ui::header::elide_middle;
use crate::ui::status::{AMBER, GREEN, RED};
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
    // One scope: the rows and lines that come and go here must not shift the tabs' auto ids (a focused field would
    // lose its focus, and a typed value its commit, when the first edit adds a row).
    ui.push_id("meshes-edit-pending", |ui| {
        // The pending list (spec §7): every op, the ones that did not apply in red with their way out.
        egui::ScrollArea::vertical()
            .id_salt("meshes-edit-ops")
            .max_height(OPS_MAX_H)
            .show(ui, |ui| {
                for (i, d) in e.doc.ops.iter().enumerate() {
                    let failed = d.status == OpStatus::Conflict;
                    op_row(ui, e, i, &op_label(&d.op), failed, act);
                }
            });
        export_block(ui, e, act);
    });
    guard_dialog(ui.ctx(), e, act);
    ui.separator();
}

const OPS_MAX_H: f32 = 120.0;

fn op_row(ui: &mut egui::Ui, e: &NifEdit, i: usize, label: &str, failed: bool, act: &mut Act) {
    let (text, conflict) = match (failed, e.outcomes.get(i)) {
        (false, _) => (label.to_string(), false),
        (true, Some(Outcome::Conflict(s))) => (format!("{label}: {s}"), true),
        (true, Some(Outcome::Refused(s))) => (format!("{label}: {s}"), false),
        (true, _) => (format!("{label}: not applied"), false),
    };
    egui::Sides::new().shrink_left().truncate().show(
        ui,
        |ui| {
            let t = egui::RichText::new(&text);
            ui.add(egui::Label::new(if failed { t.color(RED) } else { t }).truncate());
        },
        |ui| {
            if conflict {
                if ui.small_button("Take source").clicked() {
                    act.take_source = Some(i);
                }
                if ui.small_button("Keep mine").clicked() {
                    act.keep_mine = Some(i);
                }
            } else {
                let r = ui.small_button("Revert");
                // Several rows have a Revert: each is named after its edit.
                r.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        true,
                        format!("Revert {}: {label}", i + 1),
                    )
                });
                if r.clicked() {
                    act.revert = Some(i);
                }
            }
        },
    );
}

/// "Export NIF" with its destination, its blocked reason or last result, and the placement line (spec §8).
fn export_block(ui: &mut egui::Ui, e: &NifEdit, act: &mut Act) {
    if e.edit_mod.as_os_str().is_empty() {
        return; // read-only: no export folder and no MO2 instance, nothing to export to
    }
    // The real destination (Note CI29), cut in the middle like the header's Export to; the full path on hover and in
    // the accessible name.
    // MO2 joins `mods` with `/`: shown with `\` throughout.
    let full = e.export_path.display().to_string().replace('/', "\\");
    let font = egui::TextStyle::Body.resolve(ui.style());
    let glyph = ui.fonts_mut(|f| f.glyph_width(&font, '0'));
    let chars = ((ui.available_width() / glyph.max(1.0)) as usize).saturating_sub(4);
    let to = format!("To: {full}");
    let r = ui.add(egui::Label::new(format!("To: {}", elide_middle(&full, chars))).truncate());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &to));
    r.on_hover_text(&to);
    ui.weak("Change it with Export to ▾ in the header.");
    let r = ui.add_enabled(e.blocked.is_none(), egui::Button::new("Export NIF"));
    if r.clicked() {
        act.export = Some(None);
    }
    if let Some(why) = &e.blocked {
        r.on_disabled_hover_text(why);
        ui.add(egui::Label::new(egui::RichText::new(why).color(RED)).wrap());
    }
    if let Some((note, err)) = e
        .export_note
        .as_ref()
        .filter(|n| Some(&n.0) != e.blocked.as_ref())
    {
        let t = egui::RichText::new(note);
        ui.add(egui::Label::new(if *err { t.color(RED) } else { t }).wrap());
    }
    if let Some(p) = &e.placement {
        let color = if *p == Placement::Ok { GREEN } else { AMBER };
        let mod_name = e.edit_mod.file_name().unwrap_or_default().to_string_lossy();
        ui.add(egui::Label::new(egui::RichText::new(p.text(&mod_name)).color(color)).wrap());
    }
}

/// The hand-edit guard (spec §8.3): Enter keeps the file on disk; Esc closes without writing.
fn guard_dialog(ctx: &egui::Context, e: &NifEdit, act: &mut Act) {
    let Some(g) = &e.guard else { return };
    let buttons = [
        ("Overwrite", Role::Destructive, true),
        ("Diff", Role::Plain, true),
        ("Keep theirs", Role::Default, true),
    ];
    let title = match g.never_exported {
        true => "A file sdc did not write is already at this path.",
        false => "This file changed since sdc exported it.",
    };
    let (k, clicked) = dialog(
        ctx,
        "nif-export-guard",
        true,
        420.0,
        title,
        &buttons,
        |ui| {
            ui.add(egui::Label::new(e.export_path.display().to_string()).truncate());
            if g.show_diff {
                egui::ScrollArea::vertical()
                    .id_salt("nif-export-guard-diff")
                    .max_height(240.0)
                    .show(ui, |ui| {
                        for l in &g.diff {
                            ui.add(egui::Label::new(l).wrap());
                        }
                    });
            }
        },
    );
    match clicked {
        Some(0) => act.export = Some(Some(GuardChoice::Overwrite)),
        Some(1) => act.guard_diff = true,
        Some(_) => act.export = Some(Some(GuardChoice::KeepTheirs)),
        None if k.enter => act.export = Some(Some(GuardChoice::KeepTheirs)),
        None if k.cancel => act.guard_close = true,
        None => {}
    }
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
        Op::SetKeys { at, .. } => ("Edit keys".into(), at),
        Op::AddController { at, .. } => ("Add controller".into(), at),
        Op::RemoveController { at } => ("Remove controller".into(), at),
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
    // Last: a field that lost its focus to the Export click committed its push (and flush) above this frame (rule 7.5).
    if let Some(choice) = act.export {
        m.export_edit(choice);
    }
    if let (true, Some(e)) = (act.guard_close, &mut m.edit) {
        e.guard = None;
    }
    if let (true, Some(g)) = (
        act.guard_diff,
        m.edit.as_mut().and_then(|e| e.guard.as_mut()),
    ) {
        g.show_diff = !g.show_diff;
    }
}
