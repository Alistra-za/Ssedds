//! egui drawing of the model (M3a spec §2). No logic beyond mapping input to model calls.
pub mod columns;
pub mod dialogs;
pub mod header;
pub mod thumbs;

use crate::model::AppModel;
use eframe::egui;
use std::cell::RefCell;

/// Per-window drawing state that is not part of the model.
#[derive(Default)]
pub struct State {
    pub thumbs: thumbs::ThumbCache,
}

thread_local! {
    static TEST_STATE: RefCell<State> = RefCell::new(State::default());
}

/// Draw with a thread-local [`State`]: for tests, which have no app to own one.
pub fn draw(ui: &mut egui::Ui, model: &mut AppModel) {
    TEST_STATE.with(|s| draw_with(ui, model, &mut s.borrow_mut()));
}

pub fn draw_with(ui: &mut egui::Ui, model: &mut AppModel, state: &mut State) {
    header::draw(ui, model);
    egui::Panel::right("advanced")
        .resizable(false)
        .show(ui, |ui| {
            ui.toggle_value(&mut model.settings.advanced_open, "Advanced");
        });
    column_panel(
        ui,
        "textures",
        "textures-strip",
        "Textures",
        380.0,
        model,
        |ui, model| {
            columns::textures(ui, model, &mut state.thumbs);
        },
    );
    column_panel(
        ui,
        "sets",
        "sets-strip",
        "Sets",
        260.0,
        model,
        |ui, model| {
            columns::sets(ui, model, &mut state.thumbs);
        },
    );
    egui::CentralPanel::default().show(ui, |ui| {
        if model.root.is_none() {
            ui.centered_and_justified(|ui| ui.label("Open a folder of source textures to begin."));
        }
    });
    dialogs::draw(ui.ctx(), model);
}

/// A resizable column, or a thin named strip when the columns are collapsed. The strip has its own id: egui keeps a panel's size per id, and the strip's 24 px must not become the column's width.
fn column_panel(
    ui: &mut egui::Ui,
    id: &'static str,
    strip_id: &'static str,
    name: &str,
    width: f32,
    model: &mut AppModel,
    content: impl FnOnce(&mut egui::Ui, &mut AppModel),
) {
    if model.settings.columns_collapsed {
        egui::Panel::left(strip_id)
            .resizable(false)
            .exact_size(columns::STRIP_WIDTH)
            .frame(egui::Frame::new())
            .show(ui, |ui| columns::strip(ui, name));
    } else {
        egui::Panel::left(id)
            .default_size(width)
            .show(ui, |ui| content(ui, model));
    }
}
