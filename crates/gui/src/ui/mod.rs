//! egui drawing of the model (M3a spec §2). No logic beyond mapping input to model calls.
pub mod actions;
pub mod advanced;
pub mod bulk_panel;
pub mod color;
pub mod columns;
pub mod dialogs;
pub mod drawer;
pub mod header;
pub mod input_name;
pub mod json_editor;
pub mod meshes;
pub mod path_picker;
pub mod pgp_fields;
pub mod plugin_refs;
pub mod preview;
pub mod rename_dialog;
pub mod rnm_section;
pub mod set_editor;
pub mod set_preview;
pub mod sets_column;
pub mod settings_panel;
pub mod skin_card;
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
    draw_with_scan(ui, model, state, None);
}

/// [`draw_with`] with the mod scan's row under the header (the app passes its Meshes model; tests pass `None`).
pub fn draw_with_scan(
    ui: &mut egui::Ui,
    model: &mut AppModel,
    state: &mut State,
    scan: Option<&mut crate::model::meshes::MeshesModel>,
) {
    state.thumbs.sweep(model.items_rev());
    // First, before any widget sees the keys (items 5 and 19).
    actions::keys(ui.ctx(), model, state.drops.pending.is_some());
    // The column sets it again if it draws; a hidden or collapsed column leaves Up and Down off.
    model.nav.cols = 0;
    header::draw(ui, model);
    if let Some(m) = scan {
        egui::Panel::top("textures-scan").show(ui, |ui| meshes::scan_row(ui, m));
    }
    advanced::draw(ui, model, &mut state.advanced);
    let (thumbs, drops, cmds) = (&mut state.thumbs, &mut state.drops, &mut state.slot_cmds);
    // Each column stops at its minimum and leaves the columns after it theirs and the centre CENTRE_MIN.
    let textures_max = ui.available_width() - columns::SETS_MIN - set_preview::CENTRE_MIN;
    column_panel(
        ui,
        Column::Textures,
        380.0,
        columns::TEXTURES_MIN..=textures_max,
        model,
        |ui, model| {
            columns::textures(ui, model, thumbs);
        },
    );
    column_panel(
        ui,
        Column::Sets,
        260.0,
        columns::SETS_MIN..=ui.available_width() - set_preview::CENTRE_MIN,
        model,
        |ui, model| {
            sets_column::draw(ui, model, thumbs, drops, cmds);
        },
    );
    drawer::draw(ui, model, &mut state.drawer);
    // The 2D | 3D switch (AB4, AG78): in 3D the set's 3D view takes the centre in place of the set editor's body.
    let three_d = state.set3d.enabled && set_preview::wanted(model);
    egui::CentralPanel::default().show(ui, |ui| centre(ui, model, state, three_d));
    // Here, not in the set editor: a pill member's slot menu opens it while a texture is selected (F3a Task 8).
    slot_commands::draw(ui.ctx(), model, &mut state.slot_cmds);
    dialogs::draw(ui.ctx(), model);
    json_editor::draw(ui.ctx(), model);
}

/// The centre: the preview of an expanded texture, else the settings of the selected texture or set, else a hint.
fn centre(ui: &mut egui::Ui, model: &mut AppModel, state: &mut State, three_d: bool) {
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
            let view = set_editor::View {
                switch: state.set3d.enabled,
                three_d,
            };
            set_editor::draw(
                ui,
                model,
                &key,
                &mut state.thumbs,
                &mut state.slot_cmds,
                view,
            );
            if three_d {
                set_preview::draw(ui, model, &mut state.set3d);
            }
        }
        Selection::None => {
            ui.centered_and_justified(|ui| ui.label("Select a texture or a set."));
        }
    }
}

/// The two resizable columns of the Textures view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    Textures,
    Sets,
}

impl Column {
    fn ids(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Column::Textures => ("textures", "textures-strip", "Textures"),
            Column::Sets => ("sets", "sets-strip", "Sets"),
        }
    }

    /// (hidden by the header checkbox, collapsed to a strip by its arrow).
    pub fn flags(self, s: &mut crate::settings::GuiSettings) -> (&mut bool, &mut bool) {
        match self {
            Column::Textures => (&mut s.textures_hidden, &mut s.textures_collapsed),
            Column::Sets => (&mut s.sets_hidden, &mut s.sets_collapsed),
        }
    }
}

/// A resizable column, a thin named strip when it is collapsed, or nothing when the header hides it. The strip has
/// its own id: egui keeps a panel's size per id, and the strip's 24 px must not become the column's width.
fn column_panel(
    ui: &mut egui::Ui,
    column: Column,
    width: f32,
    range: std::ops::RangeInclusive<f32>,
    model: &mut AppModel,
    content: impl FnOnce(&mut egui::Ui, &mut AppModel),
) {
    let (id, strip_id, name) = column.ids();
    let (hidden, collapsed) = {
        let (h, c) = column.flags(&mut model.settings);
        (*h, *c)
    };
    if hidden {
        return;
    }
    // Too narrow a window: the minimum wins, and egui caps the column at the width left (never below zero).
    let range = *range.start()..=range.end().max(*range.start());
    if collapsed {
        let expand = egui::Panel::left(strip_id)
            .resizable(false)
            .exact_size(columns::STRIP_WIDTH)
            .frame(egui::Frame::new())
            .show(ui, |ui| {
                columns::strip_button(ui, name).on_hover_text(format!("Expand the {name} column"))
            })
            .inner
            .clicked();
        if expand {
            *column.flags(&mut model.settings).1 = false;
            model.save_settings();
        }
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
