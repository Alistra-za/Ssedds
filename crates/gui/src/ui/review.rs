//! The Review tab (layout v2 B2, C2, C5; R1–R4, R11): blockers, undecided textures and pitfalls side by side, what
//! the export writes, and one Export button. Not a dialog: Enter does nothing here (N3).
use super::settings_panel::{alpha_options, offered_groups, subtype_label};
use super::status::{AMBER, RED};
use crate::model::bottom::{problems, Level};
use crate::model::items::Decision;
use crate::model::{pbr_export, Answer, AppModel};
use eframe::egui;
use engine::config::{AlphaPolicy, Subtype};

/// What a click on an undecided row asks for.
pub enum RowPick {
    Select(String),
    Decide(String, Subtype),
    Answer(String, Answer),
}

pub fn apply(model: &mut AppModel, pick: RowPick) {
    match pick {
        RowPick::Select(k) => model.select_item(&k),
        RowPick::Decide(k, st) => model.decide_undecided_row(&k, st),
        RowPick::Answer(k, a) => model.answer_undecided_row(&k, a),
    }
}

/// One undecided texture's controls: name link, sub-type combo + ✓ when it asks its sub-type, the alpha buttons,
/// "Acknowledge for this texture". The controls are disabled with `off` as hover; the name link always selects.
pub fn undecided_row(
    ui: &mut egui::Ui,
    model: &AppModel,
    key: &str,
    off: Option<&str>,
) -> Option<RowPick> {
    let item = model.item(key)?;
    let fallback = item.job.as_ref().map(|j| j.profile.subtype);
    let question = model.subtype_question(key);
    let why = off.unwrap_or_default();
    let mut pick = None;
    ui.horizontal(|ui| {
        if ui.link(&item.name).clicked() {
            pick = Some(RowPick::Select(key.to_string()));
        }
        let Some(suggested) = question else {
            return;
        };
        let Some(default) = suggested.or(fallback) else {
            return;
        };
        ui.add_enabled_ui(off.is_none(), |ui| {
            let id = egui::Id::new(("undecided-row", key));
            let mut chosen: Subtype = ui.data(|d| d.get_temp(id)).unwrap_or(default);
            egui::ComboBox::from_id_salt(id)
                .selected_text(subtype_label(chosen))
                .show_ui(ui, |ui| {
                    for (heading, members) in offered_groups(model.target) {
                        ui.label(egui::RichText::new(heading).weak());
                        for st in members {
                            ui.selectable_value(&mut chosen, st, subtype_label(st));
                        }
                    }
                })
                .response
                .on_disabled_hover_text(why);
            ui.data_mut(|d| d.insert_temp(id, chosen));
            if ui
                .button("\u{2713}")
                .on_hover_text("Decide this texture")
                .on_disabled_hover_text(why)
                .clicked()
            {
                pick = Some(RowPick::Decide(key.to_string(), chosen));
            }
        });
    });
    // The questions that are not a sub-type: a small fixed set of choices each, answered inline.
    ui.add_enabled_ui(off.is_none(), |ui| {
        for d in &item.decisions {
            ui.horizontal_wrapped(|ui| match d {
                Decision::AlphaMeaning => {
                    ui.label("Alpha means:");
                    for (v, label) in alpha_options() {
                        let Ok(policy) = serde_json::from_value::<AlphaPolicy>(v) else {
                            continue;
                        };
                        if ui.button(label).on_disabled_hover_text(why).clicked() {
                            pick = Some(RowPick::Answer(key.to_string(), Answer::Alpha(policy)));
                        }
                    }
                }
                Decision::Acknowledge(code) => {
                    ui.label(code.as_str());
                    if ui
                        .button("Acknowledge for this texture")
                        .on_disabled_hover_text(why)
                        .clicked()
                    {
                        pick = Some(RowPick::Answer(key.to_string(), Answer::Acknowledge(*code)));
                    }
                }
                Decision::Subtype { .. } => {}
            });
        }
    });
    pick
}

enum Act {
    GoTo(String),
    Row(RowPick),
    AcceptAll,
    Tick(String, bool),
    AckAll,
    Others,
    Export,
    Close,
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// A column: its heading, then `body` in a scroll area of its own.
fn column(ui: &mut egui::Ui, heading: &str, body: impl FnOnce(&mut egui::Ui)) {
    ui.strong(heading);
    egui::ScrollArea::vertical()
        .id_salt(("review", heading))
        .auto_shrink([false, true])
        .show(ui, body);
}

pub fn tab(ui: &mut egui::Ui, model: &mut AppModel) {
    let Some(review) = &model.review else {
        return;
    };
    let m = &*model;
    // R-b: the asks and the blocks are cached per change, so the gate below reads them again for free.
    let blocks = pbr_export::export_blocks(m);
    let asks = pbr_export::export_asks(m);
    // A4: the warnings Problems lists that Review does not ask about.
    let others = problems(m)
        .iter()
        .filter(|p| p.level == Level::Warning)
        .count()
        .saturating_sub(asks.len());
    let undecided = m.undecided_keys();
    let off = m.fix_off();
    let any_suggestion = undecided
        .iter()
        .any(|k| matches!(m.subtype_question(k), Some(Some(_))));
    let gate = m.review_gate();
    let mut act = None;
    // The footer first, pinned to the bottom, so the columns scroll above it and it is never clipped.
    egui::Panel::bottom("review-footer")
        .resizable(false)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong("What will be written");
                let files = m.export_files();
                ui.label(format!("{files} DDS file{}", plural(files)));
            });
            ui.horizontal(|ui| {
                if ui.button("Close").clicked() {
                    act = Some(Act::Close);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let button = egui::Button::new(m.review_button_text());
                    match &gate {
                        Ok(_) => {
                            let fill = ui.visuals().selection.bg_fill;
                            if ui.add(button.fill(fill)).clicked() {
                                act = Some(Act::Export);
                            }
                        }
                        Err(why) => {
                            ui.add_enabled(false, button).on_disabled_hover_text(why);
                            ui.colored_label(RED, why);
                        }
                    }
                });
            });
        });
    ui.columns(3, |cols| {
        column(&mut cols[0], "Blockers", |ui| {
            if blocks.is_empty() {
                ui.weak("None");
            }
            for b in &blocks {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(RED, &b.text);
                    // A project-wide issue has no set to go to.
                    if !b.set_key.is_empty() && ui.button("Go to set").clicked() {
                        act = Some(Act::GoTo(b.set_key.clone()));
                    }
                });
            }
        });
        column(&mut cols[1], "Undecided", |ui| {
            if undecided.is_empty() {
                ui.weak("None");
            }
            for k in &undecided {
                if let Some(p) = undecided_row(ui, m, k, off.as_deref()) {
                    act = Some(Act::Row(p));
                }
            }
            let accept = ui
                .add_enabled(
                    any_suggestion && off.is_none(),
                    egui::Button::new("Accept all suggestions"),
                )
                .on_hover_text("Decide every texture that has a suggestion; the others stay listed")
                .on_disabled_hover_text(
                    off.as_deref()
                        .unwrap_or("No undecided texture has a suggestion"),
                );
            if accept.clicked() {
                act = Some(Act::AcceptAll);
            }
        });
        column(&mut cols[2], "Warnings", |ui| {
            if asks.is_empty() {
                ui.weak("None");
            }
            let mut set = None;
            for a in &asks {
                // Grouped by set: `export_asks` lists each set's pitfalls together.
                if set != Some(&a.set_id) {
                    set = Some(&a.set_id);
                    ui.colored_label(AMBER, &a.set_id);
                }
                let mut on = review.acknowledged.contains(&a.text);
                if ui.checkbox(&mut on, &a.text).changed() {
                    act = Some(Act::Tick(a.text.clone(), on));
                }
            }
            if !asks.is_empty() && ui.button("Acknowledge all").clicked() {
                act = Some(Act::AckAll);
            }
            if others > 0
                && ui
                    .link(format!(
                        "{others} other warning{} — see Problems",
                        plural(others)
                    ))
                    .clicked()
            {
                act = Some(Act::Others);
            }
        });
    });
    match act {
        Some(Act::GoTo(k)) => {
            model.select_set(&k);
            super::header::set_meshes_view(ui.ctx(), false);
        }
        Some(Act::Row(p)) => apply(model, p),
        Some(Act::AcceptAll) => model.accept_all_suggestions(),
        Some(Act::Tick(t, on)) => model.acknowledge_ask(&t, on),
        Some(Act::AckAll) => model.acknowledge_all_asks(),
        Some(Act::Others) => model.open_problems(None),
        Some(Act::Export) => model.export_from_review(),
        Some(Act::Close) => model.close_review(),
        None => {}
    }
}
