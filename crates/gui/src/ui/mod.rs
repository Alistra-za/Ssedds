//! egui drawing of the model (M3a spec §2). No logic beyond mapping input to model calls.
pub mod advanced;
pub mod columns;
pub mod dialogs;
pub mod drawer;
pub mod header;
pub mod pgp_fields;
pub mod preview;
pub mod set_editor;
pub mod sets_column;
pub mod settings_panel;
pub mod slot_commands;
pub mod thumbs;

use crate::model::{AppModel, Selection};
use eframe::egui;
use std::cell::RefCell;

/// Per-window drawing state that is not part of the model.
#[derive(Default)]
pub struct State {
    pub thumbs: thumbs::ThumbCache,
    pub drawer: drawer::DrawerState,
    pub advanced: advanced::AdvancedState,
    pub preview: preview::PreviewUi,
    pub drops: sets_column::DropState,
    pub slot_cmds: slot_commands::SlotCmdState,
    /// What is on the GPU; set by the app, empty in tests.
    pub gpu: preview::GpuStatus,
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
    advanced::draw(ui, model, &mut state.advanced);
    let (thumbs, drops) = (&mut state.thumbs, &mut state.drops);
    column_panel(
        ui,
        "textures",
        "textures-strip",
        "Textures",
        380.0,
        model,
        |ui, model| {
            columns::textures(ui, model, thumbs);
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
            sets_column::draw(ui, model, thumbs, drops);
        },
    );
    drawer::draw(ui, model, &mut state.drawer);
    egui::CentralPanel::default().show(ui, |ui| centre(ui, model, state));
    dialogs::draw(ui.ctx(), model);
}

/// The centre: the preview of an expanded texture, else the settings of the selected texture or set, else a hint.
fn centre(ui: &mut egui::Ui, model: &mut AppModel, state: &mut State) {
    if model.root.is_none() {
        ui.centered_and_justified(|ui| ui.label("Open a folder of source textures to begin."));
        return;
    }
    if model.expanded.is_some() {
        preview::draw(ui, model, &mut state.preview, &state.gpu);
        return;
    }
    // A texture expanded again starts fitted.
    state.preview = preview::PreviewUi::default();
    let selection = model.selection.clone();
    match selection {
        Selection::Item(key) => {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| settings_panel::draw(ui, model, &key, false));
        }
        Selection::Set(key) => {
            set_editor::draw(ui, model, &key, &mut state.thumbs, &mut state.slot_cmds);
        }
        Selection::None => {
            ui.centered_and_justified(|ui| ui.label("Select a texture or a set."));
        }
    }
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
