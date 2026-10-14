//! egui drawing of the model (M3a spec §2). No logic beyond mapping input to model calls.
pub mod advanced;
pub mod bulk_panel;
pub mod columns;
pub mod dialogs;
pub mod drawer;
pub mod header;
pub mod meshes;
pub mod pgp_fields;
pub mod plugin_refs;
pub mod preview;
pub mod rename_dialog;
pub mod rnm_section;
pub mod set_editor;
pub mod set_preview;
pub mod sets_column;
pub mod settings_panel;
pub mod slot_commands;
pub mod thumbs;
pub mod viewport3d;

use crate::model::{AppModel, Selection};
use eframe::egui;
use std::cell::RefCell;

/// The badge rule lives in the model (the "Has warnings" filter shares it).
pub use crate::model::counts_toward_badge;

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
    /// The set preview (P1-A). `Default` is disabled and does no IO; `SdcApp::new` builds the enabled one.
    pub set3d: crate::model::set_preview::SetPreview,
}

thread_local! {
    static TEST_STATE: RefCell<State> = RefCell::new(State::default());
}

/// Draw with a thread-local [`State`]: for tests, which have no app to own one.
pub fn draw(ui: &mut egui::Ui, model: &mut AppModel) {
    TEST_STATE.with(|s| draw_with(ui, model, &mut s.borrow_mut()));
}

pub fn draw_with(ui: &mut egui::Ui, model: &mut AppModel, state: &mut State) {
    state.thumbs.sweep(model.items_rev());
    let keys_free = selection_keys_free(ui, model);
    header::draw(ui, model);
    advanced::draw(ui, model, &mut state.advanced);
    let (thumbs, drops, cmds) = (&mut state.thumbs, &mut state.drops, &mut state.slot_cmds);
    // Each column stops at its minimum and leaves the columns after it theirs and the centre CENTRE_MIN.
    let textures_max = ui.available_width() - columns::SETS_MIN - set_preview::CENTRE_MIN;
    column_panel(
        ui,
        "textures",
        "textures-strip",
        "Textures",
        380.0,
        columns::TEXTURES_MIN..=textures_max,
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
        columns::SETS_MIN..=ui.available_width() - set_preview::CENTRE_MIN,
        model,
        |ui, model| {
            sets_column::draw(ui, model, thumbs, drops, cmds);
        },
    );
    drawer::draw(ui, model, &mut state.drawer);
    // The panel leaves the central columns their minimum width; too narrow a window for both hides the panel
    // (narrowing a column brings it back).
    let room = ui.available_width() - set_preview::CENTRE_MIN;
    if state.set3d.enabled && set_preview::wanted(model) && room >= set_preview::PANEL_MIN {
        egui::Panel::right("set-3d")
            .default_size(380.0)
            .min_size(set_preview::PANEL_MIN)
            .max_size(room)
            .show(ui, |ui| set_preview::draw(ui, model, &mut state.set3d));
    }
    egui::CentralPanel::default().show(ui, |ui| centre(ui, model, state));
    // Here, not in the set editor: a pill member's slot menu opens it while a texture is selected (F3a Task 8).
    slot_commands::draw(ui.ctx(), model, &mut state.slot_cmds);
    dialogs::draw(ui.ctx(), model);
    // Last, so every modal, menu and the preview has had its chance to consume the key first.
    if keys_free {
        selection_keys(ui, model);
    }
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
    // F3a D2/D3: two or more textures or sets.
    if model.in_bulk() {
        bulk_panel::draw(ui, model);
        return;
    }
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
#[allow(clippy::too_many_arguments)]
fn column_panel(
    ui: &mut egui::Ui,
    id: &'static str,
    strip_id: &'static str,
    name: &str,
    width: f32,
    range: std::ops::RangeInclusive<f32>,
    model: &mut AppModel,
    content: impl FnOnce(&mut egui::Ui, &mut AppModel),
) {
    // Too narrow a window: the minimum wins, and egui caps the column at the width left (never below zero).
    let range = *range.start()..=range.end().max(*range.start());
    if model.settings.columns_collapsed {
        egui::Panel::left(strip_id)
            .resizable(false)
            .exact_size(columns::STRIP_WIDTH)
            .frame(egui::Frame::new())
            .show(ui, |ui| columns::strip(ui, name));
    } else {
        // egui stores the width it drew, clamped to the window; keep the width the user dragged to apart, so a
        // window narrowed and widened again gets it back.
        use egui::containers::panel::PanelState;
        let (panel, chosen) = (egui::Id::new(id), egui::Id::new(id).with("chosen"));
        let resize = ui.ctx().read_response(panel.with("__resize"));
        let released = resize.as_ref().is_some_and(|r| r.drag_stopped());
        if !released && !resize.is_some_and(|r| r.dragged()) {
            let w = ui
                .data_mut(|d| d.get_persisted::<f32>(chosen))
                .unwrap_or(width);
            let outer_rect = egui::Rect::from_min_size(
                ui.available_rect_before_wrap().min,
                egui::vec2(w, ui.available_height()),
            );
            ui.data_mut(|d| d.insert_persisted(panel, PanelState { outer_rect }));
        }
        egui::Panel::left(id)
            .default_size(width)
            .size_range(range)
            .show(ui, |ui| {
                // Text that would not fit truncates: otherwise it holds the column wider than the drag left it.
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                content(ui, model)
            });
        // egui stores the released width during `show`.
        if let Some(s) = PanelState::load(ui.ctx(), panel).filter(|_| released) {
            ui.data_mut(|d| d.insert_persisted(chosen, s.outer_rect.width()));
        }
    }
}

/// F3a D1: whether Ctrl+A / Esc may act this frame: not while a widget has focus (the search box keeps its own
/// Ctrl+A), a dialog, popup or any modal layer is open, or a texture is expanded (the preview owns Esc). Read at the
/// start of the frame, before anything draws or consumes.
fn selection_keys_free(ui: &egui::Ui, model: &AppModel) -> bool {
    !(model.dialog.is_some()
        || model.expanded.is_some()
        || egui::Popup::is_any_open(ui.ctx())
        || ui
            .ctx()
            .memory(|m| m.focused().is_some() || m.top_modal_layer().is_some()))
}

/// Ctrl+A selects everything visible and Esc clears. Runs after the whole UI has drawn, so a key a modal or menu
/// consumed this frame (Esc closing it) is gone and does not also clear the selection.
fn selection_keys(ui: &egui::Ui, model: &mut AppModel) {
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::A)) {
        model.select_all_visible();
    }
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        model.clear_selection();
    }
    // F3b D5: F2 renames the outputs of selected sets, or the selected textures' inputs.
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F2)) {
        use crate::model::rename::RenameMode;
        let sets = !model.selected_sets().is_empty();
        model.open_rename(if sets {
            RenameMode::Outputs
        } else {
            RenameMode::Inputs
        });
    }
}
