//! The Problems tab (layout v2 A1, A12): one row per problem with its fix in place; the list is rebuilt once per
//! change and only the rows on screen are drawn.
use super::review::{apply, fix_buttons, RowPick};
use super::status::{dot, Status};
use crate::model::bottom::{filtered, flagged, problems, At, Level, Problem, ProblemsFilter};
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

type Rows = Arc<Vec<Problem>>;

/// `filtered` at most once per change: keyed on the (cached) problems build and the filter. Also the count the
/// "Checks {n}" button shows: the set's check rows while a Set filter is on, else the project's (K6, review M2).
fn rows(ui: &egui::Ui, model: &AppModel) -> (Rows, usize) {
    let all = problems(model);
    let f = model.bottom.filter.clone();
    let id = egui::Id::new("problems-rows");
    if let Some((a, ff, out, n)) =
        ui.data(|d| d.get_temp::<(Rows, Option<ProblemsFilter>, Rows, usize)>(id))
    {
        if Arc::ptr_eq(&a, &all) && ff == f {
            return (out, n);
        }
    }
    let out: Rows = filtered(model, f.as_ref());
    let n = match &f {
        Some(ProblemsFilter::Set(_)) => out.iter().filter(|p| p.level == Level::Check).count(),
        _ => flagged(model).check_rows,
    };
    ui.data_mut(|d| d.insert_temp(id, (all, f, out.clone(), n)));
    (out, n)
}

enum Act {
    Texture(String),
    Set(String),
    File(String),
    Fix(RowPick),
    RebuildZ(String),
    Filter(Option<ProblemsFilter>),
}

pub fn tab(ui: &mut egui::Ui, model: &mut AppModel) {
    // The count comes from the same cached build as the list (K6), not a count per frame.
    let (list, checks) = rows(ui, model);
    let off = model.fix_off();
    let mut act = None;
    let label = |k: &String| {
        crate::model::sets::views(model)
            .iter()
            .find(|v| v.key == *k)
            .map_or(k.clone(), |v| v.label.clone())
    };
    if model.bottom.filter.is_some() || checks > 0 {
        ui.horizontal(|ui| {
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
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, row_h, list.len(), |ui, range| {
            for p in &list[range] {
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
                        ui.add(egui::Label::new(&p.text).truncate())
                            .on_hover_text(&p.text);
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
                            At::Set { key, .. } if ui.button("Go to set").clicked() => {
                                a = Some(Act::Set(key.clone()));
                            }
                            _ => {}
                        }
                        if let Some(c) = p.code {
                            ui.label(egui::RichText::new(c.as_str()).small().weak());
                        }
                        a
                    },
                );
                if let Some(a) = l.or(r) {
                    act = Some(a);
                }
            }
        });
    // Every click-through lands in the Textures view.
    let to_textures = |ui: &egui::Ui| super::header::set_meshes_view(ui.ctx(), false);
    match act {
        Some(Act::Texture(k)) => {
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
        Some(Act::Filter(f)) => model.bottom.filter = f,
        None => {}
    }
}
