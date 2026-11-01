//! The bottom panel (layout v2 A1, A9): one strip of tab buttons, a body below while open. Full window width, drawn
//! before the columns (R7). Tabs: Problems, Review (while open), Messages and Files.
use super::{drawer, messages, problems, review};
use crate::model::bottom::BottomTab;
use crate::model::AppModel;
use eframe::egui;

/// The tabs that have a body, in their fixed order. Review is listed only while it is open (R7).
fn tabs(model: &AppModel) -> Vec<(BottomTab, String)> {
    let n = crate::model::bottom::problems(model).len();
    let problems = match n {
        0 => "Problems".to_string(),
        n => format!("Problems ({n})"),
    };
    let mut t = vec![(BottomTab::Problems, problems)];
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
        let mut r = ui.selectable_label(on, text);
        if tab == BottomTab::Messages {
            r = r.on_hover_text(format!(
                "Notes and results of this session ({})",
                super::actions::keys_text("messages")
            ));
        }
        if r.clicked() {
            clicked = Some(tab);
        }
    }
    if let Some(tab) = clicked {
        model.toggle_tab(tab);
    }
}

/// `nif_preview`: the Meshes view with a NIF selected, whose 3D preview needs the room above the panel (Note F?18).
pub fn draw(
    ui: &mut egui::Ui,
    model: &mut AppModel,
    drawer: &mut drawer::DrawerState,
    nif_preview: bool,
) {
    // Another folder or a new export: the cached file checks belong to another run.
    if drawer.epoch != model.drawer_epoch {
        drawer.stats.clear();
        drawer.epoch = model.drawer_epoch;
    }
    if !model.bottom.open || model.bottom.tab != BottomTab::Problems {
        problems::forget_focus(ui.ctx());
    }
    if !model.bottom.open {
        // Its own id, so the strip's height never becomes the open panel's.
        egui::Panel::bottom("bottom-strip").show(ui, |ui| {
            ui.horizontal(|ui| tab_buttons(ui, model));
        });
        return;
    }
    // Note FB10: Review has its own id, so it opens at 40 % of the window and a user resize sticks to it.
    let review = model.bottom.tab == BottomTab::Review && model.review.is_some();
    let (id, height) = if review {
        ("bottom-review", ui.ctx().content_rect().height() * 0.4)
    } else {
        ("bottom", 240.0)
    };
    // Note F?18 (GH6/N6 hand-off, the NIF team's 310): room for the 3D preview; Review's 40 % default obeys it too.
    let cap = nif_preview.then(|| (ui.available_height() - 310.0).max(120.0));
    let mut panel = egui::Panel::bottom(id)
        .resizable(true)
        .default_size(cap.map_or(height, |c| height.min(c)));
    if let Some(c) = cap {
        panel = panel.max_size(c);
    }
    let panel = panel.show(ui, |ui| {
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
            BottomTab::Problems => problems::tab(ui, model),
            BottomTab::Review if model.review.is_some() => review::tab(ui, model),
            // A closed Review shows Files.
            BottomTab::Review | BottomTab::Files => drawer::files_tab(ui, model, drawer),
        }
    });
    let (pass, top) = (ui.ctx().cumulative_pass_nr(), panel.response.rect.top());
    ui.ctx()
        .data_mut(|d| d.insert_temp(open_top_id(), (pass, top)));
}

fn open_top_id() -> egui::Id {
    egui::Id::new("bottom-open-top")
}

/// Note FC8: the top of the open bottom panel, when it was drawn this pass (the toasts sit above it).
pub fn open_top(ctx: &egui::Context) -> Option<f32> {
    ctx.data(|d| d.get_temp::<(u64, f32)>(open_top_id()))
        .filter(|(pass, _)| *pass == ctx.cumulative_pass_nr())
        .map(|(_, top)| top)
}
