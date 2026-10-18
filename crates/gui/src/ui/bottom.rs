//! The bottom panel (layout v2 A1, A9): one strip of tab buttons, a body below while open. Full window width, drawn
//! before the columns (R7). Tabs with a body so far: Review (while open), Messages and Files; Problems arrives with
//! its own.
use super::{drawer, messages, review};
use crate::model::bottom::BottomTab;
use crate::model::AppModel;
use eframe::egui;

/// The tabs that have a body, in their fixed order. Review is listed only while it is open (R7).
fn tabs(model: &AppModel) -> Vec<(BottomTab, String)> {
    let mut t = vec![];
    if model.review.is_some() {
        t.push((BottomTab::Review, "Review".to_string()));
    }
    t.push((BottomTab::Messages, messages::button_text(model)));
    t.push((BottomTab::Files, "Files".to_string()));
    t
}

fn tab_buttons(ui: &mut egui::Ui, model: &mut AppModel) {
    let mut clicked = None;
    for (tab, text) in tabs(model) {
        let on = model.bottom.open && model.bottom.tab == tab;
        if ui.selectable_label(on, text).clicked() {
            clicked = Some(tab);
        }
    }
    if let Some(tab) = clicked {
        model.toggle_tab(tab);
    }
}

pub fn draw(ui: &mut egui::Ui, model: &mut AppModel, drawer: &mut drawer::DrawerState) {
    // Another folder or a new export: the cached file checks belong to another run.
    if drawer.epoch != model.drawer_epoch {
        drawer.stats.clear();
        drawer.epoch = model.drawer_epoch;
    }
    if !model.bottom.open {
        // Its own id, so the strip's height never becomes the open panel's.
        egui::Panel::bottom("bottom-strip").show(ui, |ui| {
            ui.horizontal(|ui| tab_buttons(ui, model));
        });
        return;
    }
    egui::Panel::bottom("bottom")
        .resizable(true)
        .default_size(240.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                tab_buttons(ui, model);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button("▼")
                        .on_hover_text("Collapse the panel")
                        .clicked()
                    {
                        model.collapse_bottom();
                    }
                });
            });
            ui.separator();
            match model.bottom.tab {
                BottomTab::Messages => messages::tab(ui, model),
                BottomTab::Review if model.review.is_some() => review::tab(ui, model),
                // A closed Review, and Problems until it has a body, show Files.
                BottomTab::Review | BottomTab::Problems | BottomTab::Files => {
                    drawer::files_tab(ui, model, drawer)
                }
            }
        });
}
