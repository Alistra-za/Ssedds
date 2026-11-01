//! The NIF edit panel (spec §7, plan A17/A19): what is edited and from where, Solo, Undo / Redo, the pending edits,
//! the export status with Review… (Note GD2: NIF edits export from Review), then the Shader, Alpha, Controllers and
//! Extra data tabs.
use super::edit_fields::{self, Act};
use super::{edit_ctrl, edit_extra};
use crate::model::meshes::edit_doc::OpStatus;
use crate::model::meshes::nif_edit::{NifEdit, Selection, NO_EDITS, NO_EXPORT_FOLDER};
use crate::model::meshes::MeshesModel;
use crate::ui::header::elide_middle;
use crate::ui::status::{self, Status, AMBER, GREEN, RED};
use eframe::egui;
use nif::edit::Outcome;
use nif::fields::Field;

const TABS: [&str; 4] = ["Shader", "Alpha", "Controllers", "Extra data"];

/// Note GH5: the footer's line for an edit with no export folder; Review's footer says the same.
const SKIPPED_NO_FOLDER: &str = "Skipped until an export folder is chosen";

pub fn panel(ui: &mut egui::Ui, m: &mut MeshesModel) {
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
    let mut act = Act::default();
    // Note GD1: a worklist fix opens its tab; its field takes focus when its widget is drawn.
    if let Some((tab, field)) = m.nif_tab.take() {
        ui.data_mut(|d| {
            d.insert_temp(egui::Id::new("meshes-edit-tab"), tab.index());
            d.remove::<Field>(edit_fields::focus_id());
            if let Some(f) = field {
                d.insert_temp(edit_fields::focus_id(), f);
            }
        });
    }
    if let Some(e) = &m.edit {
        top(ui, e, &mut act);
        // Note GD18: a foreign file at the export path has a Diff…; the rows read the disk only when its stat changed.
        let foreign = m
            .nif_review_rows()
            .first()
            .is_some_and(|r| r.foreign.is_some());
        let diff = m
            .nif_diff_view
            .as_ref()
            .filter(|(p, _)| p.eq_ignore_ascii_case(&e.data_path))
            .map(|(_, l)| l.as_slice());
        // Meshes UI T3: a footer drawn before the tabs (rule 2.1), so a long tab never pushes it away.
        egui::Panel::bottom("meshes-edit-export")
            .resizable(false)
            .show(ui, |ui| export_block(ui, e, foreign, diff, &mut act));
        let tab_id = egui::Id::new("meshes-edit-tab");
        let mut tab: usize = ui.data(|d| d.get_temp(tab_id)).unwrap_or(0);
        ui.horizontal(|ui| {
            for (i, t) in TABS.iter().enumerate() {
                let r = ui.selectable_label(tab == i, *t);
                // Meshes UI T7 (rule 8.1): "Alpha tab", not the same name as the fields and headings it opens.
                let (on, name) = (tab == i, format!("{t} tab"));
                r.widget_info(|| {
                    egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, on, &name)
                });
                if r.clicked() {
                    tab = i;
                }
            }
        });
        ui.data_mut(|d| d.insert_temp(tab_id, tab));
        // Meshes UI T6 (Note CZ21): what the tabs act on, always in sight.
        ui.add(egui::Label::new(editing_line(e)).truncate());
        ui.separator();
        // Both ways: a wide row scrolls instead of widening the panel; thin bars stay visible without hover.
        ui.spacing_mut().scroll = egui::style::ScrollStyle::thin();
        egui::ScrollArea::both()
            .id_salt("meshes-edit-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| match tab {
                0 => edit_fields::shader_tab(ui, m, e, &mut act),
                1 => edit_fields::alpha_tab(ui, e, &mut act),
                2 => edit_ctrl::controllers_tab(ui, m, e, &mut act),
                _ => edit_extra::extra_tab(ui, e, &mut act),
            });
    }
    apply(m, act);
}

/// "Editing: BodyMesh (BSTriShape)", or "Editing: Root (NiNode)" while the root is selected.
fn editing_line(e: &NifEdit) -> String {
    match e.selection {
        Selection::Root if e.model.root_kind.is_empty() => "Editing: Root".into(),
        Selection::Root => format!("Editing: Root ({})", e.model.root_kind),
        Selection::Shape(b) => match e.model.shape(b) {
            Some(s) => format!(
                "Editing: {} ({:?})",
                s.name.as_deref().unwrap_or("(unnamed)"),
                s.kind
            ),
            None => format!("Editing: block {b} (not in this NIF)"),
        },
    }
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
        // The pending list (spec §7): every op, the ones that did not apply in red with their way out. Meshes UI T3:
        // collapsible, closed until a row turns red or the footer's Show asks for it.
        let red = e
            .doc
            .ops
            .iter()
            .filter(|d| d.status == OpStatus::Conflict)
            .count();
        let seen = egui::Id::new("meshes-edit-pending-red");
        let before: usize = ui.data(|d| d.get_temp(seen)).unwrap_or(0);
        ui.data_mut(|d| d.insert_temp(seen, red));
        let show = ui.data_mut(|d| d.remove_temp::<bool>(show_pending_id()));
        if e.doc.ops.is_empty() {
            return;
        }
        egui::CollapsingHeader::new(format!("Pending edits ({})", e.doc.ops.len()))
            .id_salt("meshes-edit-pending-list")
            .open((show.is_some() || red > before).then_some(true))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("meshes-edit-ops")
                    .max_height(OPS_MAX_H)
                    .show(ui, |ui| {
                        for (i, d) in e.doc.ops.iter().enumerate() {
                            let failed = d.status == OpStatus::Conflict;
                            let mut label = e.op_label_at(i);
                            // Note GD8: the row says that its Revert takes the clip's timing too
                            if !e.timing_dependents(i).is_empty() {
                                label.push_str(" (Revert also removes its clip timing)");
                            }
                            op_row(ui, e, i, &label, failed, act);
                        }
                    });
            });
    });
    ui.separator();
}

const OPS_MAX_H: f32 = 120.0;

/// The footer's Show asks the pending list to open (it is drawn before the footer, so on the next frame).
fn show_pending_id() -> egui::Id {
    egui::Id::new("meshes-edit-show-pending")
}

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
                // Meshes UI T7 (rule 8.4): each row's buttons are named after its edit, as Revert is.
                for (text, take) in [("Take source", true), ("Keep mine", false)] {
                    let r = ui.small_button(text);
                    let name = format!("{text} {}: {label}", i + 1);
                    r.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name)
                    });
                    if r.clicked() {
                        match take {
                            true => act.take_source = Some(i),
                            false => act.keep_mine = Some(i),
                        }
                    }
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

/// The export footer (spec §8, Meshes UI T3): a status line (Ready, or Blocked and why), what is written where, then
/// Review… (Note GD2). A read-only session shows its reason here too. `foreign`: a file Tessalith did not write is at
/// the export path (Diff… shows it); `diff`: the open Diff view's lines (Note GD18).
fn export_block(
    ui: &mut egui::Ui,
    e: &NifEdit,
    foreign: bool,
    diff: Option<&[String]>,
    act: &mut Act,
) {
    ui.add_space(4.0);
    let red = e.doc.has_conflicts();
    egui::Sides::new().shrink_left().truncate().show(
        ui,
        |ui| match &e.blocked {
            None => {
                status::dot(ui, Status::Ready);
                ui.colored_label(GREEN, "Ready");
            }
            // Review Minor 5: nothing edited yet is an idle state, not a block: a weak dot and weak text. Note GH5: so is
            // an edit with no export folder, as Review's footer lists it (`Skipped until an export folder is chosen`).
            // shortcut: a hand-drawn copy of `status::dot`; add a `Status::Idle` there (main-team file) if another view needs it.
            Some(why) if why == NO_EDITS || why == NO_EXPORT_FOLDER => {
                let (rect, r) =
                    ui.allocate_exact_size(egui::Vec2::splat(8.0), egui::Sense::hover());
                let c = ui.visuals().weak_text_color();
                ui.painter().circle_filled(rect.center(), 4.0, c);
                r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "idle"));
                ui.weak(if why == NO_EDITS {
                    NO_EDITS
                } else {
                    SKIPPED_NO_FOLDER
                });
            }
            Some(why) => {
                status::dot(ui, Status::Blocks);
                ui.add(
                    egui::Label::new(egui::RichText::new(format!("Blocked: {why}")).color(RED))
                        .truncate(),
                );
            }
        },
        |ui| {
            if red {
                let r = ui.small_button("Show");
                r.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Show pending edits")
                });
                if r.clicked() {
                    ui.data_mut(|d| d.insert_temp(show_pending_id(), true));
                    ui.ctx().request_repaint();
                }
            }
            if foreign
                && ui
                    .small_button("Diff…")
                    .on_hover_text(
                        "A file Tessalith did not write is at the export path: its blocks against what the \
                         export writes.",
                    )
                    .clicked()
            {
                act.diff = Some(true);
            }
        },
    );
    if let Some(lines) = diff {
        ui.horizontal(|ui| {
            ui.strong("The file on disk against the export");
            let r = ui.small_button("Close");
            r.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Close the diff")
            });
            if r.clicked() {
                act.diff = Some(false);
            }
        });
        // shortcut: the lines are read at the click and go stale after a later edit; Diff… again reads them anew.
        egui::ScrollArea::vertical()
            .id_salt("meshes-edit-diff")
            .max_height(160.0)
            .show(ui, |ui| {
                for l in lines {
                    let r = ui.add(egui::Label::new(egui::RichText::new(l).monospace()).truncate());
                    r.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Label,
                            true,
                            format!("Diff line: {l}"),
                        )
                    });
                }
            });
    }
    if !e.edit_mod.as_os_str().is_empty() {
        // The real destination (Note CI29), cut in the middle like the header's Export to; the full path on hover and
        // in the accessible name. MO2 joins `mods` with `/`: shown with `\` throughout.
        let full = e.export_path.display().to_string().replace('/', "\\");
        let data = e.data_path.replace('/', "\\");
        let font = egui::TextStyle::Body.resolve(ui.style());
        let glyph = ui.fonts_mut(|f| f.glyph_width(&font, '0'));
        let chars = ((ui.available_width() / glyph.max(1.0)) as usize)
            .saturating_sub(data.chars().count() + 14);
        let line = format!("Writes {data} to {full}");
        let r = ui.add(
            egui::Label::new(format!("Writes {data} to {}", elide_middle(&full, chars))).truncate(),
        );
        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &line));
        r.on_hover_text(&line);
        ui.add(
            egui::Label::new(
                egui::RichText::new(
                    "The source file is never changed. Change the folder with Export to ▾ in the header.",
                )
                .weak(),
            )
            .wrap(),
        );
    }
    // Note GD2: NIF edits export from Review, with the textures; its NIF row carries the result, the placement, the
    // Keep / Overwrite answer and the cleanup count. The button only opens Review, so it never writes.
    let r = ui
        .button("Review…")
        .on_hover_text("NIF edits export from Review, with the textures.");
    if r.clicked() {
        act.review = true;
    }
}

fn apply(m: &mut MeshesModel, act: Act) {
    if let Some((ops, merge)) = act.push {
        m.edit_push(ops, merge);
        if let Some(e) = &mut m.edit {
            e.notice = act.notice;
        }
    }
    if let (true, Some(e)) = (act.flush, &mut m.edit) {
        e.flush();
    }
    // v2 B6: a curve click jumps the clock (B3) and pauses it; tick_anim later this frame sends the materials.
    if let (Some(t), Some(c)) = (act.scrub, m.v3.clock.as_mut()) {
        c.scrub(t);
    }
    // v2 B7: edit_with sends nothing for an unchanged static material; the next tick_anim sends the animated ones.
    if let Some(p) = act.playing {
        m.edit_with(|e| e.set_playing(p));
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
    // A field that lost its focus to the Review… click committed its push (and flush) above this frame (rule 7.5).
    m.open_review_request |= act.review;
    match act.diff {
        Some(true) => {
            if let Some(p) = m.edit.as_ref().map(|e| e.data_path.clone()) {
                m.show_nif_diff(&p);
            }
        }
        Some(false) => m.nif_diff_view = None,
        None => {}
    }
}
