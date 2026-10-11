//! The editable Sets column (M3b spec §3.1): set pills in project order with badges, drag and drop
//! of textures onto sets (and onto an empty area for a new set), drag-to-reorder by the ≡ handle, and a context
//! menu. Drawing only: every change is a model call, made after drawing.
use super::columns::{member, status_text, AMBER, RED};
use super::thumbs::ThumbCache;
use crate::model::pbr_export::export_blocks;
use crate::model::sets::{set_views, DropChoice, DropOutcome, SetView, SlotFill};
use crate::model::{AppModel, Selection};
use eframe::egui;
use engine::config::Target;

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
}

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
}

fn badge(ui: &mut egui::Ui, text: &str, color: Option<egui::Color32>) {
    let t = egui::RichText::new(text).small();
    ui.label(match color {
        Some(c) => t.color(c),
        None => t.weak(),
    });
}

/// Whether the sets document can be changed (P8), and the banner's reason when it cannot.
struct Editing<'a> {
    on: bool,
    why: &'a str,
}

pub fn draw(ui: &mut egui::Ui, model: &mut AppModel, thumbs: &mut ThumbCache, st: &mut DropState) {
    ui.heading("Sets");
    ui.separator();
    let editable = model.sets_doc.as_ref().is_some_and(|d| !d.read_only());
    let why = model.banner.clone().unwrap_or_default();
    let editing = Editing {
        on: editable,
        why: &why,
    };
    let views = set_views(model);
    if views.is_empty() && model.root.is_some() && !model.busy() {
        ui.weak("No sets.");
    }
    let blocked: Vec<String> = export_blocks(model).into_iter().map(|i| i.set_id).collect();
    let mut acts = vec![];
    let (mut released, mut hovered) = (None, None);
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (index, v) in views.iter().enumerate() {
                let is_blocked = blocked.contains(&v.id);
                let out = pill(ui, model, thumbs, v, index, &editing, is_blocked, &mut acts);
                if out.handle_released {
                    released = Some(v.id.clone());
                }
                if out.hovered_by_set {
                    hovered = Some(index);
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
            Act::Reorder(id, to) => model.reorder_set(&id, to),
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
    acts: &mut Vec<Act>,
) -> PillOut {
    let mut handle_released = false;
    let selected = matches!(&model.selection, Selection::Set(k) if !k.is_empty() && *k == v.key);
    let stroke = if selected {
        ui.visuals().selection.stroke
    } else {
        ui.visuals().widgets.noninteractive.bg_stroke
    };
    let scoped = ui.scope_builder(egui::UiBuilder::new().sense(egui::Sense::click()), |ui| {
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
                    if v.pbr && model.target == Target::Cs {
                        badge(ui, "PBR → JSON", None);
                    } else {
                        badge(ui, "vanilla set (no JSON)", None);
                    }
                    let warnings: usize = v
                        .members
                        .iter()
                        .filter_map(|k| model.item(k))
                        .map(|i| i.warnings.len())
                        .sum();
                    if warnings > 0 {
                        badge(ui, &format!("⚠ {warnings}"), Some(AMBER));
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
                        let sel = matches!(&model.selection, Selection::Item(k) if k == mk);
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
        acts.push(Act::Select(v.key.clone()));
    }
    if let Some(p) = dropped {
        if editing.on {
            acts.push(Act::Drop(p.0.clone(), v.id.clone()));
        }
    }
    // Not consuming: the handle's release (in `draw`) applies the move.
    let hovered_by_set = editing.on && resp.dnd_hover_payload::<DragSet>().is_some();
    resp.context_menu(|ui| menu(ui, v, index, editing, model, acts));
    PillOut {
        handle_released,
        hovered_by_set,
    }
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
