//! egui drawing of the model (M3a spec §2). No logic beyond mapping input to model calls.
pub mod actions;
pub mod advanced;
pub mod bottom;
pub mod bulk_panel;
pub mod cm_card;
pub mod color;
pub mod columns;
pub mod cs_pbr;
pub mod cube_section;
pub mod dialogs;
pub mod drawer;
pub mod from_normal;
pub mod group;
pub mod header;
pub mod input_name;
pub mod json_editor;
pub mod marker_dialog;
pub mod meshes;
pub mod messages;
pub mod motion;
pub mod path_picker;
pub mod pgp_fields;
pub mod plugin_refs;
pub mod presets;
pub mod preview;
pub mod problems;
pub mod rename_dialog;
pub mod review;
pub mod rnm_section;
pub mod set_editor;
pub mod set_preview;
pub mod sets_column;
pub mod settings_panel;
pub mod skin_card;
pub mod slot_commands;
pub mod status;
pub mod thumbs;
pub mod viewport3d;

use crate::model::picker::{PickerPurpose, PickerSpec};
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
    /// The `model.folder_epoch` `drops` and `slot_cmds` belong to; `None` until the first frame adopts it.
    pub folder_epoch: Option<u64>,
}

thread_local! {
    static TEST_STATE: RefCell<State> = RefCell::new(State::default());
}

/// Draw with a thread-local [`State`]: for tests, which have no app to own one.
pub fn draw(ui: &mut egui::Ui, model: &mut AppModel) {
    TEST_STATE.with(|s| draw_with(ui, model, &mut s.borrow_mut()));
}

fn below_header_id() -> egui::Id {
    egui::Id::new("below-header")
}

fn centre_id() -> egui::Id {
    egui::Id::new("centre-rect")
}

/// A9: the area under the header (and the scan row), as of the last frame.
pub fn below_header(ctx: &egui::Context) -> Option<egui::Rect> {
    ctx.data(|d| d.get_temp(below_header_id()))
}

/// A9: the CentralPanel's rect as of the last frame (the Meshes view has none: K7, it is `below_header`).
pub fn centre_rect(ctx: &egui::Context) -> Option<egui::Rect> {
    ctx.data(|d| d.get_temp(centre_id()))
}

/// Call just before `bottom::draw`. K7: the Meshes view has no CentralPanel of ours, so it writes both keys alike.
pub fn record_below_header(ui: &egui::Ui) {
    let r = ui.available_rect_before_wrap();
    ui.ctx().data_mut(|d| {
        d.insert_temp(below_header_id(), r);
        d.insert_temp(centre_id(), r);
    });
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
    motion::set_reduce_motion(ui.ctx(), model.settings.reduce_motion);
    state.thumbs.sweep(model.items_rev());
    // Trace-debug #52: a modal or Slot commands left open would act on the new folder's set of the same id.
    if state
        .folder_epoch
        .replace(model.folder_epoch)
        .is_some_and(|e| e != model.folder_epoch)
    {
        state.drops = Default::default();
        state.slot_cmds = Default::default();
    }
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
    // A9: what is left under the header and the scan row, for windows that must not cover them.
    record_below_header(ui);
    // Under the header and the scan row, before the columns (R7).
    bottom::draw(ui, model, &mut state.drawer);
    // The 2D | 3D switch (AB4, AG78): in 3D the set's 3D view takes the centre in place of the set editor's body.
    let three_d = state.set3d.enabled && set_preview::wanted(model);
    // C6: a set editor in 2D has Advanced as its panel's last tab; everything else keeps the window-edge drawer.
    // A11: the start screen (no folder) has no drawer.
    let folder = model.root.is_some();
    if folder && !three_d && set_editor_shown(model, three_d) {
        // The drawer is one panel: keep the columns' widget ids (a set pill's open menu) as if it were drawn, as egui
        // does for a collapsed panel.
        ui.skip_ahead_auto_ids(1);
    } else if folder {
        advanced::draw(ui, model, &mut state.advanced);
    }
    // CF16: the columns and centre fade in when a folder opens; closing snaps. Called every frame, so a folder
    // restored at launch starts at 1. Not `ui.scope` (an auto id). K1: the start screen is never faded.
    let open = model.root.is_some();
    let t = ui.ctx().animate_bool_with_time_and_easing(
        egui::Id::new("folder-in"),
        open,
        if open {
            motion::fade_dur(ui.ctx(), 0.25)
        } else {
            0.0
        },
        egui::emath::easing::quadratic_out,
    );
    let opacity = ui.opacity();
    ui.multiply_opacity(if open { t } else { 1.0 });
    // A11: no columns until a folder is open.
    if model.root.is_some() {
        let (thumbs, drops, cmds) = (&mut state.thumbs, &mut state.drops, &mut state.slot_cmds);
        // Each column stops at its minimum and leaves the columns after it theirs and the centre its floor.
        let floor = centre_floor(model);
        let textures_max = ui.available_width() - columns::SETS_MIN - floor;
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
            columns::SETS_MIN..=ui.available_width() - floor,
            model,
            |ui, model| {
                sets_column::draw(ui, model, thumbs, drops, cmds);
            },
        );
    }
    let c = egui::CentralPanel::default().show(ui, |ui| centre(ui, model, state, three_d));
    ui.set_opacity(opacity);
    // Review M2: with the set editor not drawn, its next frame only records the slots, so a change made meanwhile
    // does not flash (CF14).
    if !set_editor_shown(model, three_d) {
        state.drops.slot_seen = None;
    }
    ui.ctx()
        .data_mut(|d| d.insert_temp(centre_id(), c.response.rect));
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
    presets::window(ui.ctx(), model);
    // AG132: only while the centre shows the 3D view (not over an expanded preview or the bulk panel).
    set_preview::settings_window(ui.ctx(), model, three_d && set_editor_shown(model, three_d));
    // T7 K10: last, so it is over everything and survives a virtualised source.
    columns::drag_ghost(ui.ctx(), model, &mut state.thumbs);
}

/// The narrowest the centre gets: `CENTRE_MIN`, and while a texture is expanded enough for `PREVIEW_MIN` beside the
/// collapsed settings strip (K8). The 16 is `Frame::central_panel`'s 8 px margin a side.
pub(crate) fn centre_floor(model: &AppModel) -> f32 {
    if model.expanded.is_some() {
        set_preview::CENTRE_MIN.max(preview::PREVIEW_MIN + columns::STRIP_WIDTH + 16.0)
    } else {
        set_preview::CENTRE_MIN
    }
}

/// Mirrors `centre`'s and `set_editor::draw`'s routing: true when the centre shows a set's editor (2D: a set still in
/// the plan; 3D: the followed set's 3D view, also for a member texture, AG87) rather than the preview, the bulk panel
/// or a hint. Callers add the 2D/3D side (`!three_d` = the set panel, `three_d` = the 3D settings window). Keep it in
/// step with those two; `set_panel.rs::drawer_and_panel_never_both_or_neither` pins it.
fn set_editor_shown(model: &AppModel, three_d: bool) -> bool {
    model.root.is_some()
        && model.expanded.is_none()
        && !model.in_bulk()
        && (three_d
            || matches!(&model.selection, Selection::Set(key)
                if crate::model::sets::set_views(model).iter().any(|v| v.key == *key)))
}

/// A11: with no folder, one big Open folder…, the recent folders and the drop hint.
fn start_screen(ui: &mut egui::Ui, model: &mut AppModel) {
    let mut open = None;
    let mut pick = false;
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() / 3.0).max(0.0));
        ui.label(egui::RichText::new(model.empty_hint()).heading());
        ui.add_space(12.0);
        let b = egui::Button::new(egui::RichText::new("Open folder…").size(18.0))
            .min_size(egui::vec2(240.0, 44.0))
            .fill(ui.visuals().selection.bg_fill);
        pick = crate::model::picker::button(ui, &model.picker, b);
        if !model.settings.recent.is_empty() {
            ui.add_space(16.0);
            ui.weak("Recent");
            for p in &model.settings.recent {
                if ui.link(p.display().to_string()).clicked() {
                    open = Some(p.clone());
                }
            }
        }
        ui.add_space(16.0);
        ui.weak("Or drag a folder onto the window.");
    });
    if pick {
        model.pick(ui.ctx(), PickerPurpose::OpenFolder, PickerSpec::folder());
    }
    if let Some(p) = open {
        model.request_open(p);
    }
}

/// The centre: the preview of an expanded texture, else the settings of the selected texture or set, else a hint.
fn centre(ui: &mut egui::Ui, model: &mut AppModel, state: &mut State, three_d: bool) {
    if model.root.is_none() {
        start_screen(ui, model);
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
                &mut state.advanced,
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
        let panel = egui::Panel::left(id).default_size(width).size_range(range);
        chosen_width_panel(ui, id, width, panel, |ui| {
            // Text that would not fit truncates: otherwise it holds the column wider than the drag left it.
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
            // Trace-debug #11: blur text fields on the press, while the old set is still drawn, so its
            // commit-on-focus-loss runs before the release changes the selection (AO1).
            // shortcut: a press and release in one frame still loses the edit; defer the selection a frame if seen.
            if ui.input(|i| i.pointer.primary_pressed()) && ui.rect_contains_pointer(ui.max_rect())
            {
                ui.memory_mut(|m| m.stop_text_input());
            }
            content(ui, model)
        });
    }
}

/// Shows `panel` (whose id is `id`) at the width the user dragged to, `<id>/chosen`, or `width` before any drag.
/// egui stores the width it drew, clamped to `size_range` and the window; keeping the chosen width apart gives it back
/// once the clamp lifts (a window widened again, a preview closed), and it is what gui.toml saves (`layout.rs`).
pub(crate) fn chosen_width_panel(
    ui: &mut egui::Ui,
    id: &str,
    width: f32,
    panel: egui::Panel,
    add: impl FnOnce(&mut egui::Ui),
) {
    use egui::containers::panel::PanelState;
    let (panel_id, chosen) = (egui::Id::new(id), egui::Id::new(id).with("chosen"));
    let resize = ui.ctx().read_response(panel_id.with("__resize"));
    let released = resize.as_ref().is_some_and(|r| r.drag_stopped());
    if !released && !resize.is_some_and(|r| r.dragged()) {
        let w = ui
            .data_mut(|d| d.get_persisted::<f32>(chosen))
            .unwrap_or(width);
        let outer_rect = egui::Rect::from_min_size(
            ui.available_rect_before_wrap().min,
            egui::vec2(w, ui.available_height()),
        );
        ui.data_mut(|d| d.insert_persisted(panel_id, PanelState { outer_rect }));
    }
    panel.show(ui, add);
    // egui stores the released width during `show`.
    if let Some(s) = PanelState::load(ui.ctx(), panel_id).filter(|_| released) {
        ui.data_mut(|d| d.insert_persisted(chosen, s.outer_rect.width()));
    }
}
