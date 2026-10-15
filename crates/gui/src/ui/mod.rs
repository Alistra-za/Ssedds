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
pub mod messages;
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
    // Item 17: files dropped from Explorer wait for a slot card to take them (see the end of this function).
    let paths: Vec<std::path::PathBuf> = ui.input(|i| {
        i.raw
            .dropped_files
            .iter()
            .map(|f| f.path().to_path_buf())
            .collect()
    });
    state.drops.file = (!paths.is_empty()).then(|| sets_column::FileDrop {
        paths,
        pos: drop_pos(ui.ctx()),
    });
    // First, before any widget sees the keys (items 5 and 19).
    actions::keys(ui.ctx(), model, state.drops.pending.is_some());
    // The column sets it again if it draws; a hidden or collapsed column leaves Up and Down off.
    model.nav.cols = 0;
    if let Some(m) = scan {
        header::draw(ui, model, Some(&mut *m));
        if meshes::scan_row_shown(m) {
            egui::Panel::top("textures-scan").show(ui, |ui| meshes::scan_row(ui, m));
        }
        meshes::scan_dialogs(ui.ctx(), m);
    } else {
        header::draw(ui, model, None);
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
    // Folders (or files, which stand for their folder) dropped anywhere but on a slot card open like Open folder
    // (spec §5.1).
    if let Some(f) = state.drops.file.take() {
        model.dropped(&f.paths);
    }
    // Here, not in the Sets column: a hidden column must still show a drop's Replace prompt (item 17).
    sets_column::pending_modal(ui.ctx(), model, &mut state.drops);
    // Here, not in the set editor: a pill member's slot menu opens it while a texture is selected (F3a Task 8).
    slot_commands::draw(ui.ctx(), model, &mut state.slot_cmds);
    dialogs::draw(ui.ctx(), model);
    json_editor::draw(ui.ctx(), model);
    // AG132: only while the centre shows the 3D view (not over an expanded preview or the bulk panel).
    let shows_3d = three_d && model.root.is_some() && model.expanded.is_none() && !model.in_bulk();
    set_preview::settings_window(ui.ctx(), model, shows_3d);
}

/// The centre: the preview of an expanded texture, else the settings of the selected texture or set, else a hint.
fn centre(ui: &mut egui::Ui, model: &mut AppModel, state: &mut State, three_d: bool) {
    if model.root.is_none() {
        ui.centered_and_justified(|ui| ui.label(model.empty_hint()));
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
        // AG87: on 3D, any member of a set opens or keeps that set's 3D view (`three_d` needs `followed_set`), its own
        // settings in the settings window (AG132).
        Selection::Item(key) if three_d => {
            let name = model.item(&key).map_or(key.clone(), |i| i.name.clone());
            ui.horizontal_wrapped(|ui| {
                set_preview::view_switch(ui, model);
                ui.heading(&name);
                // AG132: its own settings are in the settings window, with the set's.
                set_preview::settings_toggle(ui, model);
            });
            ui.separator();
            set_preview::draw(ui, model, &mut state.set3d);
        }
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
                &mut state.drops,
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

/// Item 17 (DECISIONS AG98): where a file from Explorer was dropped, in this window's points. winit 0.30 reports a
/// drop without a position and no mouse moves reach the window during the drag, so the pointer egui knows is stale:
/// on Windows the cursor is read from the OS and placed with the window's inner position. Elsewhere, and without
/// viewport information (tests), the last pointer position egui has.
fn drop_pos(ctx: &egui::Context) -> Option<egui::Pos2> {
    os_cursor(ctx).or_else(|| ctx.input(|i| i.pointer.latest_pos()))
}

#[cfg(windows)]
fn os_cursor(ctx: &egui::Context) -> Option<egui::Pos2> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let inner = ctx.input(|i| i.viewport().inner_rect)?;
    let mut p = POINT { x: 0, y: 0 };
    // SAFETY: GetCursorPos writes one POINT through a valid pointer.
    if unsafe { GetCursorPos(&mut p) } == 0 {
        return None;
    }
    // egui-winit gives `inner_rect` in the same points as the widgets (pixels / (native scale × zoom)).
    let ppp = ctx.pixels_per_point();
    Some(egui::pos2(p.x as f32 / ppp, p.y as f32 / ppp) - inner.min.to_vec2())
}

#[cfg(not(windows))]
fn os_cursor(_: &egui::Context) -> Option<egui::Pos2> {
    None
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
            .frame(egui::Frame::new().fill(ui.visuals().panel_fill))
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
