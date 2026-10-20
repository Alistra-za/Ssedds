//! The Problems tab (layout v2 A1, A12): one row per problem with its fix in place; the list is rebuilt once per
//! change and only the rows on screen are drawn.
use super::review::{apply, fix_buttons, RowPick};
use super::status::{dot, Status};
use crate::model::bottom::{filtered, flagged, problems, At, Level, Problem, ProblemsFilter};
use crate::model::AppModel;
use eframe::egui;
use std::sync::Arc;

pub fn status_of(l: Level) -> Status {
    match l {
        Level::Blocks => Status::Blocks,
        // A check row's own dot is the list's severity marker, not a "ready" dot (owner: "amber is fine").
        Level::Decide | Level::Warning | Level::Check => Status::Warning,
    }
}

type Rows = Arc<Vec<Problem>>;

/// `filtered` at most once per change: keyed on the (cached) problems build and the filter.
fn rows(ui: &egui::Ui, model: &AppModel) -> Rows {
    let all = problems(model);
    let f = model.bottom.filter.clone();
    let id = egui::Id::new("problems-rows");
    if let Some((a, ff, out)) = ui.data(|d| d.get_temp::<(Rows, Option<ProblemsFilter>, Rows)>(id))
    {
        if Arc::ptr_eq(&a, &all) && ff == f {
            return out;
        }
    }
    let out: Rows = filtered(model, f.as_ref());
    ui.data_mut(|d| d.insert_temp(id, (all, f, out.clone())));
    out
}

enum Act {
    Texture(String),
    Set(String),
    File(String),
    Fix(RowPick),
    ClearFilter,
    Checks,
}

pub fn tab(ui: &mut egui::Ui, model: &mut AppModel) {
    let list = rows(ui, model);
    let off = model.fix_off();
    let mut act = None;
    // From the same cached build as the list (K6), not a count per frame.
    let checks = flagged(model).check_rows;
    let shows_checks = model.bottom.filter == Some(ProblemsFilter::Checks);
    if model.bottom.filter.is_some() || checks > 0 {
        ui.horizontal(|ui| {
            if let Some(f) = &model.bottom.filter {
                let text = match f {
                    ProblemsFilter::Undecided => "Undecided only ×".to_string(),
                    ProblemsFilter::Checks => "Checks only ×".to_string(),
                    ProblemsFilter::Set(k) => {
                        let label = crate::model::sets::views(model)
                            .iter()
                            .find(|v| v.key == *k)
                            .map_or(k.clone(), |v| v.label.clone());
                        format!("Set: {label} ×")
                    }
                };
                if ui.button(text).clicked() {
                    act = Some(Act::ClearFilter);
                }
            }
            if checks > 0 && !shows_checks && ui.button(format!("Checks {checks}")).clicked() {
                act = Some(Act::Checks);
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
        Some(Act::File(k)) => {
            model.select_job(&k);
            to_textures(ui);
        }
        Some(Act::Fix(r)) => apply(model, r),
        Some(Act::ClearFilter) => model.bottom.filter = None,
        Some(Act::Checks) => model.bottom.filter = Some(ProblemsFilter::Checks),
        None => {}
    }
}
