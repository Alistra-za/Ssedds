//! The Problems tab (layout v2 A1, A12): one row per problem with its fix in place; the list is rebuilt once per
//! change and only the rows on screen are drawn.
use super::review::{apply, fix_buttons, RowPick};
use super::status::{dot, Status};
use crate::model::bottom::{filtered, flagged, problems, At, Level, Problem, ProblemsFilter};
use crate::model::sets::Missing;
use crate::model::AppModel;
use eframe::egui;
use engine::codes::Code;
use serde_json::json;
use std::sync::Arc;

/// The fix button on a CHAN-NORMAL-LENGTH row (Note DP).
pub const REBUILD_Z: &str = "Rebuild Z";

pub fn status_of(l: Level) -> Status {
    match l {
        Level::Blocks => Status::Blocks,
        // A check row's own dot is the list's severity marker, not a "ready" dot (owner: "amber is fine").
        Level::Decide | Level::Warning | Level::Check => Status::Warning,
    }
}

/// A row's focus id (Note FB9): by what the row is, not where it is, so focus stays on a row while the rows above it
/// leave the list.
pub fn row_id(p: &Problem) -> egui::Id {
    egui::Id::new(("problem-row", p.level, &p.at, &p.text))
}

/// A row's accessible name: where, then what (Note FB9).
fn row_name(p: &Problem) -> String {
    let at = match &p.at {
        At::Texture(k) | At::File(k) => k.as_str(),
        At::Set { id, .. } => id.as_str(),
        At::Project => "Project",
    };
    format!("{at}: {}", p.shown())
}

type Rows = Arc<Vec<Problem>>;

/// `filtered` at most once per change: keyed on the (cached) problems build and the filter. Also the count the
/// "Checks {n}" button shows: the set's check rows while a Set filter is on, else the project's (K6, review M2), and
/// the project's rows with a suggestion, which "Accept all suggestions" decides (Note FB9).
fn rows(ui: &egui::Ui, model: &AppModel) -> (Rows, usize, usize) {
    let all = problems(model);
    let f = model.bottom.filter.clone();
    let id = egui::Id::new("problems-rows");
    if let Some((a, ff, out, n, s)) =
        ui.data(|d| d.get_temp::<(Rows, Option<ProblemsFilter>, Rows, usize, usize)>(id))
    {
        if Arc::ptr_eq(&a, &all) && ff == f {
            return (out, n, s);
        }
    }
    let out: Rows = filtered(model, f.as_ref());
    let n = match &f {
        Some(ProblemsFilter::Set(_)) => out.iter().filter(|p| p.level == Level::Check).count(),
        _ => flagged(model).check_rows,
    };
    let s = all.iter().filter(|p| p.suggested.is_some()).count();
    ui.data_mut(|d| d.insert_temp(id, (all, f, out.clone(), n, s)));
    (out, n, s)
}

/// The row that had keyboard focus and its index: when it leaves the list (its question was answered), focus goes to
/// the row now at that index, or the last one (Review Focus 5).
const ROW_FOCUS: &str = "problems-row-focus";

/// The Problems tab is not drawn: forget the focused row, so coming back never moves focus into the list.
pub fn forget_focus(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<(egui::Id, usize)>(egui::Id::new(ROW_FOCUS)));
}

enum Act {
    Texture(String),
    Set(String),
    File(String),
    Fix(RowPick),
    RebuildZ(String),
    RemoveMissing(String, Missing),
    Filter(Option<ProblemsFilter>),
    AcceptAll,
}

pub fn tab(ui: &mut egui::Ui, model: &mut AppModel) {
    // The count comes from the same cached build as the list (K6), not a count per frame.
    let (list, checks, suggestions) = rows(ui, model);
    let off = model.fix_off();
    let sets_off = model.sets_read_only_reason();
    let mut act = None;
    let label = |k: &String| {
        crate::model::sets::views(model)
            .iter()
            .find(|v| v.key == *k)
            .map_or(k.clone(), |v| v.id.clone())
    };
    if model.bottom.filter.is_some() || checks > 0 || suggestions > 0 {
        ui.horizontal(|ui| {
            // Note FB9 (owner amendment A): the project's suggestions, one undo step; written by Save.
            if suggestions > 0 {
                let b = ui
                    .add_enabled(
                        off.is_none(),
                        egui::Button::new(format!("Accept all suggestions ({suggestions})")),
                    )
                    .on_hover_text(if model.bottom.filter.is_some() {
                        "Set every texture in the whole project that has a suggested type to it, not only the \
                         listed ones; the others stay listed"
                    } else {
                        "Set every texture that has a suggested type to it; the others stay listed"
                    })
                    .on_disabled_hover_text(off.as_deref().unwrap_or_default());
                if b.clicked() {
                    act = Some(Act::AcceptAll);
                }
            }
            // The chip and what its × leaves; "Checks {n}" and what it sets (a Set filter keeps its set, M2).
            let (chip, press) = match &model.bottom.filter {
                None => (None, Some(ProblemsFilter::Checks)),
                Some(ProblemsFilter::Undecided) => (
                    Some(("Undecided only ×".to_string(), None)),
                    Some(ProblemsFilter::Checks),
                ),
                Some(ProblemsFilter::Checks) => (Some(("Checks only ×".to_string(), None)), None),
                Some(ProblemsFilter::Set(k)) => (
                    Some((format!("Set: {} ×", label(k)), None)),
                    Some(ProblemsFilter::SetChecks(k.clone())),
                ),
                Some(ProblemsFilter::SetChecks(k)) => (
                    Some((
                        format!("Set: {}, checks only ×", label(k)),
                        Some(ProblemsFilter::Set(k.clone())),
                    )),
                    None,
                ),
            };
            if let Some((text, back)) = chip {
                if ui.button(text).clicked() {
                    act = Some(Act::Filter(back));
                }
            }
            if let Some(f) = press.filter(|_| checks > 0) {
                if ui.button(format!("Checks {checks}")).clicked() {
                    act = Some(Act::Filter(Some(f)));
                }
            }
        });
    }
    if list.is_empty() {
        ui.weak("No problems");
    }
    let row_h = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
    let m = &*model;
    // Review Focus 5: the focused row left the list (its question was answered) and egui dropped the focus, so the
    // row now at its index takes it, or the last row.
    let focus_key = egui::Id::new(ROW_FOCUS);
    let last: Option<(egui::Id, usize)> = ui.data(|d| d.get_temp(focus_key));
    let focused = ui.memory(|mem| mem.focused());
    if let (None, Some((rid, i)), Some(end)) = (focused, last, list.len().checked_sub(1)) {
        if !list.iter().any(|p| row_id(p) == rid) {
            let to = row_id(&list[i.min(end)]);
            ui.memory_mut(|mem| mem.request_focus(to));
        }
    }
    let mut focus_now = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, row_h, list.len(), |ui, range| {
            for (i, p) in list[range.clone()].iter().enumerate() {
                let i = range.start + i;
                // Note FB9: the row is focusable (registered first, it takes no click from its cells); Enter or
                // Space accepts its suggestion and moves to the next row. A row without one ignores both.
                let rect = egui::Rect::from_min_size(
                    ui.cursor().min,
                    egui::vec2(ui.available_width(), ui.spacing().interact_size.y),
                );
                let rid = row_id(p);
                let row = ui.interact(rect, rid, egui::Sense::focusable_noninteractive());
                row.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Other, true, row_name(p))
                });
                if row.has_focus() {
                    // Once a frame: the next row takes focus in this same frame and sees the same key press.
                    let first = focus_now.replace((rid, i)).is_none();
                    if row.gained_focus() {
                        row.scroll_to_me(None);
                    }
                    let stroke = ui.visuals().selection.stroke;
                    ui.painter()
                        .rect_stroke(rect, 2.0, stroke, egui::StrokeKind::Inside);
                    // Consumed only when the row acts, so the Grid's Enter (open the preview) never sees it too.
                    let acting = matches!((first, &off, &p.at, p.suggested), (true, None, At::Texture(_), Some(_)));
                    let pressed = acting
                        && ui.input_mut(|k| {
                            k.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                                || k.consume_key(egui::Modifiers::NONE, egui::Key::Space)
                        });
                    if let (true, At::Texture(k), Some(st)) = (pressed, &p.at, p.suggested) {
                        act = Some(Act::Fix(RowPick::Decide(k.clone(), st)));
                        // The last row keeps focus: no wrap to the top.
                        if let Some(next) = list.get(i + 1) {
                            ui.memory_mut(|mem| mem.request_focus(row_id(next)));
                        }
                    }
                }
                // The fix buttons are laid out first, so the sentence is cut to the room left (I2).
                let (l, r) = egui::Sides::new().shrink_left().truncate().show(
                    ui,
                    |ui| {
                        let mut a = None;
                        dot(ui, status_of(p.level));
                        match &p.at {
                            At::Texture(k) => {
                                if ui.link(k).clicked() {
                                    a = Some(Act::Texture(k.clone()));
                                }
                            }
                            At::Set { key, id } => {
                                if ui.link(id).clicked() {
                                    a = Some(Act::Set(key.clone()));
                                }
                            }
                            At::File(k) => {
                                if ui.link(k).clicked() {
                                    a = Some(Act::File(k.clone()));
                                }
                            }
                            At::Project => {
                                ui.label("Project");
                            }
                        }
                        // Note FB8: a sentence that names the action; the engine's text and the code are the hover.
                        ui.add(egui::Label::new(&*p.shown()).truncate())
                            .on_hover_ui(|ui| {
                                ui.label(&p.text);
                                if let Some(c) = p.codes_text() {
                                    ui.weak(c);
                                }
                            });
                        a
                    },
                    |ui| {
                        let mut a = None;
                        // Right to left: the last widget added is the leftmost.
                        match &p.at {
                            At::Texture(k) if p.code == Some(Code::ChanNormalLength) => {
                                let b = ui.add_enabled(off.is_none(), egui::Button::new(REBUILD_Z));
                                let tip = "Rebuild the blue (Z) channel from red and green for this file \
                                           (channels.reconstruct_z). Only for normal maps: it replaces the \
                                           file's blue channel.";
                                let b = match &off {
                                    Some(why) => b.on_disabled_hover_text(why.as_str()),
                                    None => b.on_hover_text(tip),
                                };
                                if b.clicked() {
                                    a = Some(Act::RebuildZ(k.clone()));
                                }
                            }
                            At::Texture(k) if p.level == Level::Decide => {
                                if let Some(r) = fix_buttons(ui, m, k, off.as_deref()) {
                                    a = Some(Act::Fix(r));
                                }
                            }
                            // Note DU3: a set entry that matches no file is removed in place.
                            At::Set { id, .. } if p.missing.is_some() => {
                                let miss = p.missing.as_ref().expect("guarded");
                                let (label, name) = match miss {
                                    Missing::NoFilesLeft => {
                                        ("Delete set", format!("Delete set {id}"))
                                    }
                                    _ => (
                                        "Remove from set",
                                        format!("Remove {} from {id}", miss.text()),
                                    ),
                                };
                                let b = ui.add_enabled(sets_off.is_none(), egui::Button::new(label));
                                b.widget_info(|| {
                                    egui::WidgetInfo::labeled(
                                        egui::WidgetType::Button,
                                        sets_off.is_none(),
                                        &name,
                                    )
                                });
                                let b = match &sets_off {
                                    Some(why) => b.on_disabled_hover_text(why.as_str()),
                                    None => b.on_hover_text("Remove this line from sdc-project.toml"),
                                };
                                if b.clicked() {
                                    a = Some(Act::RemoveMissing(id.clone(), miss.clone()));
                                }
                            }
                            At::Set { key, .. } if ui.button("Go to set").clicked() => {
                                a = Some(Act::Set(key.clone()));
                            }
                            _ => {}
                        }
                        a
                    },
                );
                if let Some(a) = l.or(r) {
                    act = Some(a);
                }
            }
        });
    // Kept while a row has focus, and over the frame its row leaves the list (egui drops the focus at that frame's
    // end; the next frame moves it, so ask for that frame).
    let keep = focus_now.or(last.filter(|(rid, _)| focused == Some(*rid)));
    if keep.is_some() && focus_now.is_none() {
        ui.ctx().request_repaint();
    }
    ui.data_mut(|d| {
        if let Some(v) = keep {
            d.insert_temp(focus_key, v);
        } else {
            d.remove::<(egui::Id, usize)>(focus_key);
        }
    });
    // Every click-through lands in the Textures view.
    let to_textures = |ui: &egui::Ui| super::header::set_meshes_view(ui.ctx(), false);
    match act {
        Some(Act::Texture(k)) => {
            // Note FB12: a helper in a collapsed Helpers group opens it, as the palette does. Local to this click:
            // `select_item` also runs for restores, which must not reopen a group the user collapsed.
            if model
                .item(&k)
                .is_some_and(|i| i.kind == crate::model::items::ItemKind::Helper)
            {
                model.filter.hide_helpers = false;
                model.nav.reveal_item = Some(k.clone());
            }
            model.select_item(&k);
            to_textures(ui);
        }
        Some(Act::Set(k)) => {
            model.select_set(&k);
            to_textures(ui);
        }
        // Re-check A5: a Vanilla or CM row of the last Also export names no texture; its file is shown in Explorer.
        Some(Act::File(k)) => match model.also_file(&k) {
            Some(f) => {
                if let Err(e) = model.reveal(&f) {
                    model.status_line = e;
                }
            }
            None => {
                model.select_job(&k);
                to_textures(ui);
            }
        },
        Some(Act::Fix(r)) => apply(model, r),
        Some(Act::RebuildZ(k)) => model.set_field(&k, "channels.reconstruct_z", json!(true)),
        Some(Act::RemoveMissing(id, miss)) => {
            if let Err(why) = model.remove_missing(&id, &miss) {
                model.status_line = why;
            }
        }
        Some(Act::Filter(f)) => model.bottom.filter = f,
        Some(Act::AcceptAll) => model.accept_all_suggestions(),
        None => {}
    }
}
