//! The editable Sets column (M3b spec §3.1): set pills in project order with badges, drag and drop
//! of textures onto sets (and onto an empty area for a new set), drag-to-reorder by the ≡ handle, and a context
//! menu. Drawing only: every change is a model call, made after drawing.
use super::columns::{bulk_menu, member, status_text, AMBER, RED};
use super::thumbs::ThumbCache;
use crate::model::bulk::MenuAction;
use crate::model::pbr_export::export_blocks;
use crate::model::sets::{is_json_set, shader_options, DropChoice, DropOutcome, SetView, SlotFill};
use crate::model::AppModel;
use eframe::egui;
use engine::codes::Warning;
use engine::shader::{Shader, ShaderOrigin};
use std::collections::HashSet;

/// Drag payload: a texture, by item key.
#[derive(Debug, Clone)]
pub struct DragItem(pub String);
/// Drag payload: a set, by id (reordering).
#[derive(Debug, Clone)]
pub struct DragSet(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pending {
    Choice {
        item: String,
        set: String,
        from: String,
    },
    /// `choice` is what the replace answers: `drop_replace(item, set, choice)`.
    Replace {
        item: String,
        set: String,
        existing: String,
        choice: DropChoice,
    },
    Refused(String),
    /// The rename dialog: the set, the name being typed, and why the last try was refused.
    Rename {
        id: String,
        text: String,
        error: Option<String>,
    },
}

#[derive(Debug, Default)]
pub struct DropState {
    pub pending: Option<Pending>,
    /// The set that was just reordered and when (egui time): its border flashes for `FLASH_SECS`.
    pub flash: Option<(String, f64)>,
}

/// How long the moved pill flashes after a reorder.
const FLASH_SECS: f64 = 1.0;
/// The warning tooltip shows this many lines, then "… and K more".
const TIP_LINES: usize = 10;

enum Act {
    Select(String),
    SelectItem(String),
    Expand(String),
    Drop(String, String),
    NewSet(String),
    Reorder(String, usize),
    OpenRename(String),
    Delete(String),
    RemoveMember(String, String),
    /// The Shader dropdown: `None` is Auto.
    Shader(String, Option<Shader>),
    /// A pill click with its modifiers (F3a D1).
    Click(String, bool, bool),
    RightClick(String),
    Bulk(MenuAction),
}

fn badge(ui: &mut egui::Ui, text: &str, color: Option<egui::Color32>) -> egui::Response {
    let t = egui::RichText::new(text).small();
    ui.label(match color {
        Some(c) => t.color(c),
        None => t.weak(),
    })
}

/// The warning tooltip: `<file>: <code> <message>` per warning, at most `TIP_LINES`, then "… and K more".
fn warning_tooltip(rows: &[(&str, &Warning)]) -> String {
    let mut lines: Vec<String> = rows
        .iter()
        .take(TIP_LINES)
        .map(|(file, w)| format!("{file}: {} {}", w.code.as_str(), w.message))
        .collect();
    if rows.len() > TIP_LINES {
        lines.push(format!("… and {} more", rows.len() - TIP_LINES));
    }
    lines.join("\n")
}

/// Whether the sets document can be changed (P8), and the banner's reason when it cannot.
struct Editing<'a> {
    on: bool,
    why: &'a str,
}

pub fn draw(ui: &mut egui::Ui, model: &mut AppModel, thumbs: &mut ThumbCache, st: &mut DropState) {
    ui.horizontal_wrapped(|ui| {
        ui.heading("Sets");
        if let Some(s) = model.selection_summary(true) {
            ui.label(egui::RichText::new(s).strong());
        }
    });
    ui.separator();
    // One set per frame keeps the per-pill and per-member tests O(1) however many are selected.
    let sel_sets: HashSet<String> = model.selected_sets().into_iter().collect();
    let sel_items: HashSet<String> = model.selected_items().into_iter().collect();
    let editable = model.sets_doc.as_ref().is_some_and(|d| !d.read_only());
    let why = model.banner.clone().unwrap_or_default();
    let editing = Editing {
        on: editable,
        why: &why,
    };
    let views = crate::model::sets::views(model);
    if views.is_empty() && model.root.is_some() && !model.busy() {
        ui.weak("No sets.");
    }
    let blocked: Vec<String> = export_blocks(model).into_iter().map(|i| i.set_id).collect();
    let mut acts = vec![];
    let (mut released, mut hovered) = (None, None);
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    // The set being dragged (a peek: the payload stays for the reorder).
    let dragged: Option<String> = egui::DragAndDrop::payload::<DragSet>(&ctx).map(|p| p.0.clone());
    let dragged_index = dragged
        .as_deref()
        .and_then(|d| views.iter().position(|v| v.id == d));
    if st
        .flash
        .as_ref()
        .is_some_and(|(_, t)| now - t >= FLASH_SECS)
    {
        st.flash = None;
    }
    if st.flash.is_some() {
        ctx.request_repaint();
    }
    let flash = st
        .flash
        .as_ref()
        .map(|(id, t)| (id.as_str(), (now - t) / FLASH_SECS));
    let mut rects = vec![];
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (index, v) in views.iter().enumerate() {
                let is_blocked = blocked.contains(&v.id);
                let look = PillLook {
                    dragging: dragged.as_deref() == Some(v.id.as_str()),
                    flash: flash.filter(|(id, _)| *id == v.id).map(|(_, f)| f),
                    selected: !v.key.is_empty() && sel_sets.contains(&v.key),
                    bulk: sel_sets.len() >= 2,
                };
                let out = pill(
                    ui, model, thumbs, v, index, &editing, is_blocked, look, &sel_items, &mut acts,
                );
                rects.push(out.rect);
                if out.handle_released {
                    released = Some(v.id.clone());
                }
                if out.hovered_by_set {
                    hovered = Some(index);
                }
            }
            if let (Some(from), Some(to)) = (dragged_index, hovered) {
                if let Some((r, y)) = rects
                    .get(to)
                    .and_then(|r| insertion_y(from, to, *r).map(|y| (*r, y)))
                {
                    let accent = ui.visuals().selection.stroke.color;
                    ui.painter().line_segment(
                        [egui::pos2(r.left(), y), egui::pos2(r.right(), y)],
                        egui::Stroke::new(3.0, accent),
                    );
                }
            }
            if model.root.is_some() {
                new_set_area(ui, &editing, &mut acts);
            }
        });
    let ids: Vec<&str> = views.iter().map(|v| v.id.as_str()).collect();
    if editable {
        if let Some((id, to)) = reorder_target(released.as_deref(), hovered, &ids) {
            acts.push(Act::Reorder(id, to));
        }
    }
    pending_modal(ui.ctx(), model, st);
    for a in acts {
        match a {
            Act::Select(k) => model.select_set(&k),
            Act::SelectItem(k) => model.select_item(&k),
            Act::Click(k, ctrl, shift) => model.click_set(&k, ctrl, shift),
            Act::RightClick(k) => model.right_click_set(&k),
            Act::Bulk(a) => {
                model.run_menu(a);
            }
            Act::Expand(k) => model.expand(&k),
            Act::Drop(item, set) => match model.drop_on_set(&item, &set) {
                DropOutcome::Joined => {}
                DropOutcome::AskChoice { from } => {
                    st.pending = Some(Pending::Choice { item, set, from })
                }
                DropOutcome::AskReplace { existing } => {
                    st.pending = Some(Pending::Replace {
                        item,
                        set,
                        existing,
                        choice: DropChoice::Move,
                    })
                }
                DropOutcome::Refused(why) => st.pending = Some(Pending::Refused(why)),
            },
            Act::NewSet(item) => {
                // `None`: nothing was done (read-only file, or a packed map, which follows its raw maps).
                let _ = model.drop_new_set(&item);
            }
            Act::Reorder(id, to) => {
                model.reorder_set(&id, to);
                if editable {
                    st.flash = Some((id, now));
                    ctx.request_repaint();
                }
            }
            Act::OpenRename(id) => {
                st.pending = Some(Pending::Rename {
                    text: id.clone(),
                    id,
                    error: None,
                })
            }
            Act::Delete(id) => {
                model.delete_set(&id);
            }
            Act::RemoveMember(id, item) => {
                if let Err(why) = model.remove_member(&id, &item) {
                    st.pending = Some(Pending::Refused(why));
                }
            }
            Act::Shader(id, shader) => model.set_shader(&id, shader),
        }
    }
}

/// The dashed area at the bottom: a texture dropped here starts a new set.
fn new_set_area(ui: &mut egui::Ui, editing: &Editing, acts: &mut Vec<Act>) {
    let (zone, payload) = item_zone(
        ui,
        editing.on,
        egui::Frame::group(ui.style()).corner_radius(12.0),
        |ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new("Drop here to start a new set").color(AMBER));
        },
    );
    if !editing.on {
        zone.on_hover_text(editing.why);
    }
    if let Some(p) = payload {
        if editing.on {
            acts.push(Act::NewSet(p.0.clone()));
        }
    }
}

/// A frame that takes dropped textures. `egui::Ui::dnd_drop_zone` takes whatever payload is in flight when it
/// is released (even one of another type), so while a set is dragged for reordering, or when editing is off, a
/// plain frame is drawn instead and the set payload survives for the reorder.
fn item_zone(
    ui: &mut egui::Ui,
    editing: bool,
    frame: egui::Frame,
    add: impl FnOnce(&mut egui::Ui),
) -> (egui::Response, Option<std::sync::Arc<DragItem>>) {
    if editing && !egui::DragAndDrop::has_payload_of_type::<DragSet>(ui.ctx()) {
        let (zone, p) = ui.dnd_drop_zone::<DragItem, ()>(frame, add);
        (zone.response, p)
    } else {
        (frame.show(ui, add).response, None)
    }
}

/// Where a dragged set goes: `released` is the set whose handle was let go this frame, `hovered` the index of
/// the pill under the pointer. Nothing happens when there is no target or the set is dropped on itself.
fn reorder_target(
    released: Option<&str>,
    hovered: Option<usize>,
    ids: &[&str],
) -> Option<(String, usize)> {
    let (id, to) = (released?, hovered?);
    (*ids.get(to)? != id).then(|| (id.to_string(), to))
}

/// What a pill reports back: its handle was let go, and a dragged set is over it.
struct PillOut {
    handle_released: bool,
    hovered_by_set: bool,
    rect: egui::Rect,
}

/// How a pill is drawn this frame: held by the pointer, or just moved (progress 0..1 through the flash).
#[derive(Clone, Copy)]
struct PillLook {
    dragging: bool,
    flash: Option<f64>,
    selected: bool,
    /// Two or more sets are selected: the pills of selected ones show the bulk menu.
    bulk: bool,
}

/// The y of the insertion line (None over the dragged pill itself, where nothing moves) when set `from` is dragged over the pill at `to` (whose rect is `r`): moving
/// down lands after that pill, moving up before it (the same rule as `reorder_set`).
fn insertion_y(from: usize, to: usize, r: egui::Rect) -> Option<f32> {
    match to.cmp(&from) {
        std::cmp::Ordering::Greater => Some(r.bottom()),
        std::cmp::Ordering::Less => Some(r.top()),
        std::cmp::Ordering::Equal => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn pill(
    ui: &mut egui::Ui,
    model: &AppModel,
    thumbs: &mut ThumbCache,
    v: &SetView,
    index: usize,
    editing: &Editing,
    blocked: bool,
    look: PillLook,
    sel_items: &HashSet<String>,
    acts: &mut Vec<Act>,
) -> PillOut {
    let mut handle_released = false;
    let selected = look.selected;
    let accent = ui.visuals().selection.stroke.color;
    let stroke = if look.dragging {
        egui::Stroke::new(2.0, accent)
    } else if let Some(f) = look.flash {
        egui::Stroke::new(2.0, accent.gamma_multiply((1.0 - f as f32).clamp(0.0, 1.0)))
    } else if selected {
        ui.visuals().selection.stroke
    } else {
        ui.visuals().widgets.noninteractive.bg_stroke
    };
    let scoped = ui.scope_builder(egui::UiBuilder::new().sense(egui::Sense::click()), |ui| {
        if look.dragging {
            ui.set_opacity(0.5);
        }
        let (_zone, dropped) = item_zone(
            ui,
            editing.on,
            egui::Frame::group(ui.style())
                .corner_radius(12.0)
                .stroke(stroke),
            |ui| {
                ui.style_mut().interaction.selectable_labels = false;
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let sense = if editing.on {
                        egui::Sense::drag()
                    } else {
                        egui::Sense::hover()
                    };
                    let handle = ui.add(egui::Label::new("≡").sense(sense));
                    if editing.on {
                        handle.dnd_set_drag_payload(DragSet(v.id.clone()));
                        handle_released = handle.drag_stopped();
                    }
                    ui.label(egui::RichText::new(&v.label).strong());
                });
                ui.horizontal_wrapped(|ui| {
                    if !is_json_set(v, model.target) {
                        badge(ui, "vanilla set (no JSON)", None);
                    } else if v.skin {
                        badge(ui, "skin → JSON", None);
                    } else {
                        badge(ui, "PBR → JSON", None);
                    }
                    let rows: Vec<(&str, &Warning)> = v
                        .members
                        .iter()
                        .filter_map(|k| model.item(k))
                        .flat_map(|i| {
                            i.warnings
                                .iter()
                                .filter(|w| super::counts_toward_badge(w))
                                .map(move |w| (i.name.as_str(), w))
                        })
                        .chain(
                            v.set_warnings
                                .iter()
                                .filter(|w| super::counts_toward_badge(w))
                                .map(|w| (v.label.as_str(), w)),
                        )
                        .collect();
                    if !v.key.is_empty() {
                        shader_combo(ui, model, v, editing, acts);
                    }
                    if !rows.is_empty() {
                        badge(ui, &format!("⚠ {}", rows.len()), Some(AMBER))
                            .on_hover_text(warning_tooltip(&rows));
                    }
                    if blocked {
                        badge(ui, "blocked", Some(RED));
                    }
                    for s in &v.owns {
                        badge(ui, &format!("owns {}", s.label()), None);
                    }
                    for f in v.slots.values() {
                        if let SlotFill::Borrowed { from, .. } = f {
                            badge(ui, &format!("borrowed from {from}"), Some(AMBER));
                        }
                    }
                    for m in &v.missing {
                        badge(ui, &format!("missing: {m}"), Some(RED));
                    }
                });
                if v.key.is_empty() && v.created && v.missing.is_empty() {
                    ui.weak("Drop a texture here to start");
                }
                ui.horizontal_wrapped(|ui| {
                    for mk in &v.members {
                        let Some(item) = model.item(mk) else { continue };
                        let sel = sel_items.contains(mk);
                        let r = member(ui, item, sel, thumbs);
                        r.dnd_set_drag_payload(DragItem(mk.clone()));
                        if r.double_clicked() {
                            acts.push(Act::Expand(mk.clone()));
                        } else if r.clicked() {
                            acts.push(Act::SelectItem(mk.clone()));
                        }
                    }
                });
                for set in model.sets.iter().filter(|s| s.key == v.key) {
                    for recipe in &set.recipes {
                        let (word, color) = status_text(recipe.status);
                        let t =
                            egui::RichText::new(format!("{}: {word}", recipe.subtype.id())).small();
                        let r = ui.label(match color {
                            Some(c) => t.color(c),
                            None => t.weak(),
                        });
                        if let Some(reason) = &recipe.reason {
                            r.on_hover_text(reason);
                        }
                    }
                }
            },
        );
        dropped
    });
    let dropped = scoped.inner;
    let resp = scoped.response;
    if !editing.on {
        resp.clone().on_hover_text(editing.why);
    }
    if resp.clicked() && !v.key.is_empty() && acts.is_empty() {
        let m = ui.input(|i| i.modifiers);
        acts.push(Act::Click(v.key.clone(), m.command, m.shift));
    }
    if let Some(p) = dropped {
        if editing.on {
            acts.push(Act::Drop(p.0.clone(), v.id.clone()));
        }
    }
    // Not consuming: the handle's release (in `draw`) applies the move.
    let hovered_by_set = editing.on && resp.dnd_hover_payload::<DragSet>().is_some();
    // F3a D4: a right-click on an unselected pill selects it alone first; among two or more selected sets the pill
    // shows the bulk menu, otherwise its own.
    if resp.secondary_clicked() && !v.key.is_empty() {
        acts.push(Act::RightClick(v.key.clone()));
    }
    let bulk = look.selected && look.bulk;
    resp.context_menu(|ui| {
        if bulk {
            if let Some(a) = bulk_menu(ui, &model.menu_title(true), &model.set_menu()) {
                acts.push(Act::Bulk(a));
            }
        } else {
            menu(ui, v, index, editing, model, acts)
        }
    });
    PillOut {
        handle_released,
        hovered_by_set,
        rect: resp.rect,
    }
}

/// The set's Shader dropdown (M6 spec §7): the engine's pick with its provenance, then "Auto" and the mode's
/// shaders. Disabled when sets are not editable.
fn shader_combo(
    ui: &mut egui::Ui,
    model: &AppModel,
    v: &SetView,
    editing: &Editing,
    acts: &mut Vec<Act>,
) {
    let text = v.shader.as_ref().map_or(String::new(), |p| {
        let tag = match p.provenance {
            ShaderOrigin::Set => "",
            ShaderOrigin::Auto(_) => " (auto)",
            ShaderOrigin::Fallback => " (fallback)",
        };
        format!("{}{tag}", p.shader.label())
    });
    // A ComboBox never wraps and is at least `combo_width` wide, which held the Sets column open: start a new row
    // when it would not fit, and never be wider than the row.
    let w = ui.spacing().combo_width;
    if ui.available_width() < w {
        ui.end_row();
    }
    let w = ui.available_width().min(w);
    ui.add_enabled_ui(editing.on, |ui| {
        egui::ComboBox::from_id_salt(("shader", &v.id))
            .width(w)
            .truncate()
            .selected_text(text)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(v.shader_choice.is_none(), "Auto")
                    .clicked()
                {
                    acts.push(Act::Shader(v.id.clone(), None));
                }
                for s in shader_options(model.target) {
                    if ui
                        .selectable_label(v.shader_choice == Some(s), s.label())
                        .clicked()
                    {
                        acts.push(Act::Shader(v.id.clone(), Some(s)));
                    }
                }
            });
    });
}

fn menu(
    ui: &mut egui::Ui,
    v: &SetView,
    index: usize,
    editing: &Editing,
    model: &AppModel,
    acts: &mut Vec<Act>,
) {
    if !editing.on {
        ui.disable();
    }
    let tip = |r: egui::Response| {
        if editing.on {
            r
        } else {
            r.on_disabled_hover_text(editing.why)
        }
    };
    if tip(ui.button("Rename…")).clicked() {
        acts.push(Act::OpenRename(v.id.clone()));
        ui.close();
    }
    if tip(ui.button("Edit match…")).clicked() && !v.key.is_empty() {
        acts.push(Act::Select(v.key.clone()));
        ui.close();
    }
    let remove = ui.menu_button("Remove member", |ui| {
        for mk in &v.members {
            let name = model.item(mk).map_or(mk.as_str(), |i| i.name.as_str());
            if ui.button(name).clicked() {
                acts.push(Act::RemoveMember(v.id.clone(), mk.clone()));
                ui.close();
            }
        }
    });
    tip(remove.response);
    let del = ui.add_enabled(
        v.created || v.key.is_empty(),
        egui::Button::new("Delete set"),
    );
    let del = del.on_disabled_hover_text(if editing.on {
        "Only sets you created can be deleted; an automatic set comes back from the file names"
    } else {
        editing.why
    });
    if del.clicked() {
        acts.push(Act::Delete(v.id.clone()));
        ui.close();
    }
    if tip(ui.button("Move up")).clicked() && index > 0 {
        acts.push(Act::Reorder(v.id.clone(), index - 1));
        ui.close();
    }
    if tip(ui.button("Move down")).clicked() {
        acts.push(Act::Reorder(v.id.clone(), index + 1));
        ui.close();
    }
}

/// What a button in the drop modal asked for; applied after the modal is drawn (the modal only reads the model).
enum ModalAnswer {
    Choice(DropChoice),
    Replace,
    Close,
}

fn pending_modal(ctx: &egui::Context, model: &mut AppModel, st: &mut DropState) {
    if matches!(st.pending, Some(Pending::Rename { .. })) {
        rename_modal(ctx, model, st);
        return;
    }
    let Some(p) = st.pending.clone() else { return };
    let mut answer = None;
    let shown = egui::Modal::new(egui::Id::new("set-drop")).show(ctx, |ui| {
        ui.set_max_width(420.0);
        match &p {
            Pending::Choice { item, from, .. } => {
                let name = model.item(item).map_or(item.clone(), |i| i.name.clone());
                ui.heading(format!("{name} is in {from}"));
                if ui.button("Move it here").clicked() {
                    answer = Some(ModalAnswer::Choice(DropChoice::Move));
                }
                if ui.button("Share (borrow its finished output)").clicked() {
                    answer = Some(ModalAnswer::Choice(DropChoice::Share));
                }
                if ui.button("Duplicate the source for this set").clicked() {
                    answer = Some(ModalAnswer::Choice(DropChoice::Duplicate));
                }
                if ui.button("Cancel").clicked() {
                    answer = Some(ModalAnswer::Close);
                }
            }
            Pending::Replace { set, existing, .. } => {
                let name = model
                    .item(existing)
                    .map_or(existing.clone(), |i| i.name.clone());
                ui.heading("Replace?");
                ui.label(format!("{set} already has {name} in that slot."));
                ui.horizontal(|ui| {
                    if ui.button(format!("Replace {name}")).clicked() {
                        answer = Some(ModalAnswer::Replace);
                    }
                    if ui.button("Cancel").clicked() {
                        answer = Some(ModalAnswer::Close);
                    }
                });
            }
            Pending::Refused(why) => {
                ui.heading("Can't do that");
                ui.label(why);
                if ui.button("OK").clicked() {
                    answer = Some(ModalAnswer::Close);
                }
            }
            Pending::Rename { .. } => {}
        }
    });
    if answer.is_none() && shown.should_close() {
        answer = Some(ModalAnswer::Close);
    }
    let Some(answer) = answer else { return };
    st.pending = match (answer, p) {
        (ModalAnswer::Choice(c), Pending::Choice { item, set, .. }) => {
            match model.drop_choice(&item, &set, c) {
                DropOutcome::AskReplace { existing } => Some(Pending::Replace {
                    item,
                    set,
                    existing,
                    choice: c,
                }),
                DropOutcome::Refused(why) => Some(Pending::Refused(why)),
                _ => None,
            }
        }
        // The replace answers the choice that led to it (Move, Share or Duplicate); a refusal shows its reason.
        (
            ModalAnswer::Replace,
            Pending::Replace {
                item, set, choice, ..
            },
        ) => model
            .drop_replace(&item, &set, choice)
            .err()
            .map(Pending::Refused),
        _ => None,
    };
}

/// The rename dialog: Enter or OK renames, a refusal (a name already taken) is shown under the field, and
/// Cancel or Esc closes it.
fn rename_modal(ctx: &egui::Context, model: &mut AppModel, st: &mut DropState) {
    let Some(Pending::Rename { id, text, error }) = &mut st.pending else {
        return;
    };
    let (mut submit, mut cancel) = (false, false);
    let shown = egui::Modal::new(egui::Id::new("set-rename")).show(ctx, |ui| {
        ui.set_max_width(420.0);
        ui.heading(format!("Rename {id}"));
        let field = ui.text_edit_singleline(text);
        if !field.has_focus() && !field.lost_focus() {
            field.request_focus();
        }
        if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            submit = true;
        }
        if let Some(e) = error.as_ref() {
            ui.colored_label(RED, e);
        }
        ui.horizontal(|ui| {
            if ui.button("OK").clicked() {
                submit = true;
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
        });
    });
    if shown.should_close() {
        cancel = true;
    }
    if cancel {
        st.pending = None;
    } else if submit {
        match model.rename_set(id, text) {
            Ok(()) => st.pending = None,
            Err(e) => *error = Some(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::codes::Code;

    fn warn(msg: &str) -> Warning {
        Warning::new(Code::LossAlphaData, msg)
    }

    #[test]
    fn the_warning_tooltip_lists_file_code_message_and_caps_at_ten() {
        let w = warn("alpha is lost");
        let few = [("a.png", &w), ("b.png", &w)];
        let t = warning_tooltip(&few);
        assert_eq!(t.lines().count(), 2);
        assert!(t.starts_with("a.png: LOSS-ALPHA-DATA alpha is lost"), "{t}");
        let many: Vec<(&str, &Warning)> = (0..13).map(|_| ("x.png", &w)).collect();
        let t = warning_tooltip(&many);
        assert_eq!(t.lines().count(), 11);
        assert_eq!(t.lines().last(), Some("… and 3 more"));
        assert!(!warning_tooltip(&many[..10]).contains("more"));
    }

    #[test]
    fn the_insertion_line_is_after_the_target_when_moving_down_and_before_it_when_moving_up() {
        let r = egui::Rect::from_min_max(egui::pos2(0.0, 10.0), egui::pos2(5.0, 30.0));
        assert_eq!(insertion_y(0, 2, r), Some(30.0));
        assert_eq!(insertion_y(2, 0, r), Some(10.0));
        assert_eq!(
            insertion_y(1, 1, r),
            None,
            "hovering the dragged pill itself shows no line"
        );
    }

    #[test]
    fn a_released_handle_over_another_pill_reorders_to_its_index() {
        let v = ["a", "b", "c"];
        assert_eq!(
            reorder_target(Some("a"), Some(2), &v),
            Some(("a".to_string(), 2))
        );
        assert_eq!(
            reorder_target(Some("c"), Some(0), &v),
            Some(("c".to_string(), 0))
        );
    }

    #[test]
    fn no_release_no_target_or_a_drop_on_itself_does_nothing() {
        let v = ["a", "b"];
        assert_eq!(reorder_target(None, Some(1), &v), None);
        assert_eq!(reorder_target(Some("a"), None, &v), None);
        assert_eq!(reorder_target(Some("a"), Some(0), &v), None);
        assert_eq!(reorder_target(Some("a"), Some(9), &v), None);
    }
}
