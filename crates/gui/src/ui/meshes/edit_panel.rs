//! The NIF edit panel (spec §7, plan A17/A19): what is edited and from where, Solo, Undo / Redo, the pending edits,
//! Export NIF with its guard dialog, then the Shader, Alpha, Controllers and Extra data tabs.
use super::edit_fields::{self, Act};
use super::{edit_ctrl, edit_extra};
use crate::model::meshes::edit_doc::OpStatus;
use crate::model::meshes::export::{GuardChoice, Placement};
use crate::model::meshes::nif_edit::{NifEdit, Selection, NO_EDITS};
use crate::model::meshes::MeshesModel;
use crate::ui::dialogs::{dialog, Role};
use crate::ui::header::elide_middle;
use crate::ui::status::{self, Status, AMBER, GREEN, RED};
use eframe::egui;
use nif::edit::Outcome;
use nif::fields::Field;

const TABS: [&str; 4] = ["Shader", "Alpha", "Controllers", "Extra data"];

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
        // Meshes UI T3: a footer drawn before the tabs (rule 2.1), so a long tab never pushes Export NIF away.
        egui::Panel::bottom("meshes-edit-export")
            .resizable(false)
            .show(ui, |ui| export_block(ui, e, &mut act));
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
    guard_dialog(ui.ctx(), e, act);
    ui.separator();
}

const OPS_MAX_H: f32 = 120.0;

/// The footer's Show asks the pending list to open (it is drawn before the footer, so on the next frame).
fn show_pending_id() -> egui::Id {
    egui::Id::new("meshes-edit-show-pending")
}

/// Bug 2: egui turns Enter or Space on a focused button into a click. A write takes a pointer or AccessKit click.
fn key_click(ui: &egui::Ui, r: &egui::Response) -> bool {
    r.has_focus()
        && ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Space))
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

/// The Export NIF footer (spec §8, Meshes UI T3): a status line (Ready, or Blocked and why), what is written where,
/// the button, then the last result and the placement line. A read-only session shows its reason here too.
fn export_block(ui: &mut egui::Ui, e: &NifEdit, act: &mut Act) {
    ui.add_space(4.0);
    let red = e.doc.has_conflicts();
    egui::Sides::new().shrink_left().truncate().show(
        ui,
        |ui| match &e.blocked {
            None => {
                status::dot(ui, Status::Ready);
                ui.colored_label(GREEN, "Ready");
            }
            // Review Minor 5: nothing edited yet is an idle state, not a block: a weak dot and weak text.
            // shortcut: a hand-drawn copy of `status::dot`; add a `Status::Idle` there (main-team file) if another view needs it.
            Some(why) if why == NO_EDITS => {
                let (rect, r) =
                    ui.allocate_exact_size(egui::Vec2::splat(8.0), egui::Sense::hover());
                let c = ui.visuals().weak_text_color();
                ui.painter().circle_filled(rect.center(), 4.0, c);
                r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "idle"));
                ui.weak(NO_EDITS);
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
        },
    );
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
    let ready = e.blocked.is_none();
    let n = e.doc.ops.len();
    let text = match (e.up_to_date, n) {
        (true, _) => "Export NIF again".to_string(),
        // Every edit undone after an export: writing the source back needs no count.
        (false, 0) => "Export NIF".to_string(),
        (false, 1) => "Export NIF · 1 edit".to_string(),
        (false, n) => format!("Export NIF · {n} edits"),
    };
    // The page's primary action: strong, two rows tall, the accent fill when it can run (as the header's Export).
    let mut b = egui::Button::new(egui::RichText::new(text).strong()).min_size(egui::vec2(
        ui.available_width(),
        2.0 * ui.spacing().interact_size.y,
    ));
    if ready {
        b = b.fill(ui.visuals().selection.bg_fill);
    }
    let r = ui.add_enabled(ready, b);
    if r.clicked() && !key_click(ui, &r) {
        act.export = Some(None);
    }
    if let Some(why) = &e.blocked {
        r.on_disabled_hover_text(why);
    }
    if let Some((note, err)) = e
        .export_note
        .as_ref()
        .filter(|n| Some(&n.0) != e.blocked.as_ref())
    {
        let t = egui::RichText::new(note);
        ui.add(egui::Label::new(if *err { t.color(RED) } else { t }).wrap());
    }
    if let Some(note) = &e.cleanup_note {
        ui.add(egui::Label::new(note).wrap());
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
    // Bug 2: Enter or Space on a focused Overwrite is egui's click; only a pointer or AccessKit click overwrites.
    // NIF-side: the shared `dialogs.rs` (main team) still clicks on keys for its other Destructive buttons.
    let key = ctx.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Space));
    match clicked {
        Some(0) if key => {}
        Some(0) => act.export = Some(Some(GuardChoice::Overwrite)),
        Some(1) => act.guard_diff = true,
        Some(_) => act.export = Some(Some(GuardChoice::KeepTheirs)),
        None if k.enter => act.export = Some(Some(GuardChoice::KeepTheirs)),
        None if k.cancel => act.guard_close = true,
        None => {}
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
