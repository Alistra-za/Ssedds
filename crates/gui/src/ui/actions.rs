//! Items 5 and 19: one action list. The F1 shortcut sheet, the Ctrl+K command palette and the key handling all read
//! [`actions`]; a menu that shows a shortcut takes its text from here ([`keys_text`]). DECISIONS AG22-AG26.
use crate::model::nav::{Palette, Pick};
use crate::model::picker::{PickerPurpose, PickerSpec};
use crate::model::AppModel;
use eframe::egui::{self, Key, Modifiers};

/// Where an action works; the sheet groups by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Area {
    /// Both views, whatever is shown.
    Global,
    /// The Textures view with no texture expanded.
    Grid,
    /// The expanded preview.
    Preview,
    /// The Textures view with no texture expanded (the selection's keys that act on sets).
    Sets,
    /// An open dialog. Its keys are its own (`dialogs::modal_keys`); they are only listed here.
    Dialogs,
    /// The 3D panel (Meshes view, set preview). `ui::viewport3d` handles its keys (the viewports are not in
    /// `AppModel`); they are only listed here.
    View3d,
    /// The PGPatcher JSON editor window (`ui::json_editor` handles its keys; it is a window of its own).
    JsonEditor,
}

impl Area {
    pub const ALL: [Area; 7] = [
        Area::Global,
        Area::Grid,
        Area::Preview,
        Area::Sets,
        Area::View3d,
        Area::JsonEditor,
        Area::Dialogs,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Area::Global => "Global",
            Area::Grid => "Textures grid",
            Area::Preview => "Preview",
            Area::Sets => "Sets",
            Area::Dialogs => "Dialogs",
            Area::View3d => "3D preview",
            Area::JsonEditor => "JSON editor",
        }
    }

    /// Two areas that are never live at once may share a key (Esc clears the selection in the grid and closes the
    /// preview; Enter opens the preview from the grid and presses a dialog's default).
    pub fn exclusive_with(self, other: Area) -> bool {
        use Area::*;
        let both = |a, b| (self == a && other == b) || (self == b && other == a);
        // The set preview's 3D panel shows beside the grid: its keys never share with any other area.
        self != other
            && self != Global
            && other != Global
            && self != View3d
            && other != View3d
            && self != JsonEditor
            && other != JsonEditor
            && !both(Grid, Sets)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    pub mods: Modifiers,
    pub key: Key,
}

const fn key(key: Key) -> Shortcut {
    Shortcut {
        mods: Modifiers::NONE,
        key,
    }
}

const fn ctrl(key: Key) -> Shortcut {
    Shortcut {
        mods: Modifiers::COMMAND,
        key,
    }
}

const fn shift(key: Key) -> Shortcut {
    Shortcut {
        mods: Modifiers::SHIFT,
        key,
    }
}

const fn ctrl_shift(key: Key) -> Shortcut {
    Shortcut {
        mods: Modifiers {
            shift: true,
            ..Modifiers::COMMAND
        },
        key,
    }
}

impl Shortcut {
    /// "Ctrl+Shift+Z", "Shift+Left", "Esc", "?".
    pub fn text(self) -> String {
        let mut s = String::new();
        if self.mods.command || self.mods.ctrl {
            s.push_str("Ctrl+");
        }
        if self.mods.alt {
            s.push_str("Alt+");
        }
        if self.mods.shift {
            s.push_str("Shift+");
        }
        s.push_str(match self.key {
            Key::Escape => "Esc",
            Key::ArrowLeft => "Left",
            Key::ArrowRight => "Right",
            Key::ArrowUp => "Up",
            Key::ArrowDown => "Down",
            k => k.symbol_or_name(),
        });
        s
    }

    /// egui ignores an extra Shift or Alt when it matches a key, so the keys with more modifiers are tried first.
    fn weight(self) -> u8 {
        u8::from(self.mods.command || self.mods.ctrl)
            + u8::from(self.mods.shift)
            + u8::from(self.mods.alt)
    }
}

pub struct Action {
    pub id: &'static str,
    pub label: &'static str,
    pub area: Area,
    /// Empty: the palette only.
    pub keys: &'static [Shortcut],
    /// Offered in the palette (the arrow-key moves and the channel keys are not).
    pub palette: bool,
    pub enabled: fn(&AppModel) -> bool,
    /// `None`: the key is handled where it belongs (a dialog's Enter and Esc) and only listed.
    pub run: Option<fn(&mut AppModel, &egui::Context)>,
}

const fn always(_: &AppModel) -> bool {
    true
}

const fn a(
    id: &'static str,
    label: &'static str,
    area: Area,
    keys: &'static [Shortcut],
    run: fn(&mut AppModel, &egui::Context),
) -> Action {
    Action {
        id,
        label,
        area,
        keys,
        palette: true,
        enabled: always,
        run: Some(run),
    }
}

/// Left out of the palette.
const fn unlisted(x: Action) -> Action {
    Action {
        palette: false,
        ..x
    }
}

/// Enabled only when `enabled` says so.
const fn when(x: Action, enabled: fn(&AppModel) -> bool) -> Action {
    Action { enabled, ..x }
}

/// A key handled where it belongs, only listed.
const fn listed(
    id: &'static str,
    label: &'static str,
    area: Area,
    keys: &'static [Shortcut],
) -> Action {
    Action {
        id,
        label,
        area,
        keys,
        palette: false,
        enabled: always,
        run: None,
    }
}

/// Every action, in the sheet's order within each area.
pub fn actions() -> &'static [Action] {
    ACTIONS
}

use Area::{Dialogs, Global, Grid, JsonEditor, Preview, Sets, View3d};

pub static ACTIONS: &[Action] = &[
    // Global
    unlisted(a(
        PALETTE,
        "Command palette",
        Global,
        &[ctrl(Key::K)],
        |m, _| {
            m.nav.palette = Some(Palette {
                focus: true,
                ..Palette::default()
            });
        },
    )),
    a(
        "sheet",
        "Keyboard shortcuts",
        Global,
        &[key(Key::F1), key(Key::Questionmark)],
        |m, _| m.nav.sheet = true,
    ),
    a("messages", "Messages", Global, &[ctrl(Key::M)], |m, _| {
        m.toggle_tab(crate::model::bottom::BottomTab::Messages);
    }),
    when(
        a("open", "Open folder…", Global, &[], |m, ctx| {
            m.pick(ctx, PickerPurpose::OpenFolder, PickerSpec::folder());
        }),
        |m| !m.picker.busy(),
    ),
    when(
        a("save", "Save", Global, &[ctrl(Key::S)], |m, _| {
            // A failed save raises its own dialog.
            let _ = m.save();
        }),
        AppModel::is_dirty,
    ),
    when(
        a("export", "Export", Global, &[], |m, _| m.export_clicked()),
        AppModel::can_export,
    ),
    when(
        a("undo", "Undo", Global, &[ctrl(Key::Z)], |m, ctx| {
            if !super::meshes::request_undo(ctx, true) {
                m.undo();
            }
        }),
        AppModel::can_undo,
    ),
    when(
        a(
            "redo",
            "Redo",
            Global,
            &[ctrl(Key::Y), ctrl_shift(Key::Z)],
            |m, ctx| {
                if !super::meshes::request_undo(ctx, false) {
                    m.redo();
                }
            },
        ),
        AppModel::can_redo,
    ),
    a(
        "view",
        "Switch between the Textures and Meshes views",
        Global,
        &[],
        |_, ctx| {
            let meshes = super::header::meshes_view_active(ctx);
            super::header::set_meshes_view(ctx, !meshes);
        },
    ),
    a(
        "set-3d",
        "Switch the set view between 2D and 3D",
        Global,
        &[],
        |m, _| {
            m.settings.set_view_3d = !m.settings.set_view_3d;
            m.save_settings();
        },
    ),
    a("live", "Turn Live encode on or off", Global, &[], |m, _| {
        let on = !m.preview.live;
        m.set_live(on);
    }),
    when(
        a(
            "json-editor",
            "Open the PGPatcher JSON editor",
            Global,
            &[],
            |m, _| m.json_editor_open = true,
        ),
        |m| m.target == engine::config::Target::Cs && m.root.is_some() && m.sets_doc.is_some(),
    ),
    a("presets", "Open the preset creator", Global, &[], |m, _| {
        m.open_presets()
    }),
    // Textures grid
    a(
        "select-all",
        "Select every visible texture (every set when a set is active)",
        Grid,
        &[ctrl(Key::A)],
        |m, _| m.select_all_visible(),
    ),
    a(
        "clear",
        "Clear the selection",
        Grid,
        &[key(Key::Escape)],
        |m, _| m.clear_selection(),
    ),
    unlisted(a(
        "left",
        "Select the texture to the left",
        Grid,
        &[key(Key::ArrowLeft)],
        |m, _| m.grid_move(-1, false),
    )),
    unlisted(a(
        "right",
        "Select the texture to the right",
        Grid,
        &[key(Key::ArrowRight)],
        |m, _| m.grid_move(1, false),
    )),
    when(
        unlisted(a(
            "up",
            "Select the texture above",
            Grid,
            &[key(Key::ArrowUp)],
            |m, _| m.grid_move(-m.grid_row_step(), false),
        )),
        AppModel::can_step_rows,
    ),
    when(
        unlisted(a(
            "down",
            "Select the texture below",
            Grid,
            &[key(Key::ArrowDown)],
            |m, _| m.grid_move(m.grid_row_step(), false),
        )),
        AppModel::can_step_rows,
    ),
    unlisted(a(
        "extend-left",
        "Extend the selection to the left",
        Grid,
        &[shift(Key::ArrowLeft)],
        |m, _| m.grid_move(-1, true),
    )),
    unlisted(a(
        "extend-right",
        "Extend the selection to the right",
        Grid,
        &[shift(Key::ArrowRight)],
        |m, _| m.grid_move(1, true),
    )),
    when(
        unlisted(a(
            "extend-up",
            "Extend the selection up a row",
            Grid,
            &[shift(Key::ArrowUp)],
            |m, _| m.grid_move(-m.grid_row_step(), true),
        )),
        AppModel::can_step_rows,
    ),
    when(
        unlisted(a(
            "extend-down",
            "Extend the selection down a row",
            Grid,
            &[shift(Key::ArrowDown)],
            |m, _| m.grid_move(m.grid_row_step(), true),
        )),
        AppModel::can_step_rows,
    ),
    when(
        a(
            "open-preview",
            "Open the expanded preview of the active texture",
            Grid,
            &[key(Key::Enter), key(Key::Space)],
            |m, _| m.open_active(),
        ),
        |m| matches!(m.selection, crate::model::Selection::Item(_)),
    ),
    // Preview
    a(
        "close-preview",
        "Close the preview",
        Preview,
        &[key(Key::Escape)],
        |m, _| m.collapse(),
    ),
    a(
        "prev-texture",
        "Previous texture",
        Preview,
        &[key(Key::OpenBracket)],
        |m, _| m.expand_step(-1),
    ),
    a(
        "next-texture",
        "Next texture",
        Preview,
        &[key(Key::CloseBracket)],
        |m, _| m.expand_step(1),
    ),
    a(
        "lens",
        "Magnifier lens on or off",
        Preview,
        &[key(Key::L)],
        |m, _| m.view.lens = !m.view.lens,
    ),
    a(
        "tile",
        "Tile the picture 3×3 on or off",
        Preview,
        &[key(Key::T)],
        |m, _| m.view.tile = !m.view.tile,
    ),
    when(
        a("pin", "Pin this result", Preview, &[key(Key::P)], |m, _| {
            m.pin_this()
        }),
        AppModel::can_pin,
    ),
    unlisted(a(
        "channel-rgba",
        "Show RGBA",
        Preview,
        &[key(Key::Num1)],
        |m, _| m.pick_channel(0),
    )),
    unlisted(a(
        "channel-r",
        "Show R",
        Preview,
        &[key(Key::Num2)],
        |m, _| m.pick_channel(1),
    )),
    unlisted(a(
        "channel-g",
        "Show G",
        Preview,
        &[key(Key::Num3)],
        |m, _| m.pick_channel(2),
    )),
    unlisted(a(
        "channel-b",
        "Show B",
        Preview,
        &[key(Key::Num4)],
        |m, _| m.pick_channel(3),
    )),
    unlisted(a(
        "channel-a",
        "Show A",
        Preview,
        &[key(Key::Num5)],
        |m, _| m.pick_channel(4),
    )),
    unlisted(a(
        "channel-mask",
        "Show the α-mask",
        Preview,
        &[key(Key::Num6)],
        |m, _| m.pick_channel(5),
    )),
    unlisted(a(
        "mip-up",
        "Larger mip",
        Preview,
        &[key(Key::PageUp)],
        |m, _| m.step_mip(-1),
    )),
    unlisted(a(
        "mip-down",
        "Smaller mip",
        Preview,
        &[key(Key::PageDown)],
        |m, _| m.step_mip(1),
    )),
    // Sets
    a(
        "rename",
        "Rename the outputs of the selected sets, or the inputs of the selected textures",
        Sets,
        &[key(Key::F2)],
        |m, _| {
            use crate::model::rename::RenameMode;
            let sets = !m.selected_sets().is_empty();
            m.open_rename(if sets {
                RenameMode::Outputs
            } else {
                RenameMode::Inputs
            });
        },
    ),
    // 3D preview (ui::viewport3d handles them)
    listed(
        "3d-reset",
        "Recentre the 3D view (as a double-click)",
        View3d,
        &[key(Key::Home)],
    ),
    listed(
        "3d-debug",
        "Next 3D debug view (Lit, UV checker)",
        View3d,
        &[key(Key::V)],
    ),
    listed(
        "3d-copy",
        "Copy the 3D view as an image",
        View3d,
        &[ctrl_shift(Key::C)],
    ),
    // The JSON editor (its window handles them)
    listed(
        "json-next-error",
        "Next error in the PGPatcher JSON editor",
        JsonEditor,
        &[key(Key::F8)],
    ),
    listed(
        "json-prev-error",
        "Previous error in the PGPatcher JSON editor",
        JsonEditor,
        &[shift(Key::F8)],
    ),
    // Dialogs
    listed(
        "dialog-enter",
        "Press the dialog's safe default",
        Dialogs,
        &[key(Key::Enter)],
    ),
    listed(
        "dialog-esc",
        "Cancel the dialog",
        Dialogs,
        &[key(Key::Escape)],
    ),
];

/// The palette's own id: the one key that works from a text field.
pub const PALETTE: &str = "palette";

/// An action by id.
pub fn action(id: &str) -> Option<&'static Action> {
    actions().iter().find(|a| a.id == id)
}

/// The shortcut text of action `id`: "Ctrl+Y or Ctrl+Shift+Z"; empty without keys.
pub fn keys_text(id: &str) -> String {
    action(id)
        .map(|a| {
            a.keys
                .iter()
                .map(|s| s.text())
                .collect::<Vec<_>>()
                .join(" or ")
        })
        .unwrap_or_default()
}

/// Whether action `a` can run now. A20: Undo / Redo follow the NIF history while a NIF edit is open.
fn enabled(a: &Action, model: &AppModel, ctx: &egui::Context) -> bool {
    match (a.id, super::meshes::undo_target(ctx)) {
        ("undo", Some((u, _))) => u,
        ("redo", Some((_, r))) => r,
        _ => (a.enabled)(model),
    }
}

/// Whether an action's area applies now.
fn live(area: Area, model: &AppModel, meshes_view: bool) -> bool {
    match area {
        Area::Global => true,
        Area::Grid | Area::Sets => !meshes_view && model.expanded.is_none(),
        Area::Preview => !meshes_view && model.expanded.is_some(),
        Area::Dialogs | Area::View3d | Area::JsonEditor => false,
    }
}

/// Run action `id` (the palette's pick).
pub fn run(id: &str, model: &mut AppModel, ctx: &egui::Context) {
    if let Some(f) = action(id).and_then(|a| a.run) {
        f(model, ctx);
    }
}

/// The keys of this frame. Called first thing in the frame, before any widget has seen the input: Ctrl+K must not
/// reach a focused text field (egui's Ctrl+K deletes to the end of the line), and a key that runs an action is
/// consumed so no widget acts on it too. Nothing fires while a dialog, a modal, a popup or a sets-column box
/// (`set_modal`) is open, and only Ctrl+K while a widget has the keyboard focus (Task 2's guard). Returns the id of
/// the action that ran.
pub fn keys(ctx: &egui::Context, model: &mut AppModel, set_modal: bool) -> Option<&'static str> {
    // egui drops the focus on Esc before any widget runs, so a field focused at the last frame's start still owns
    // this frame's Esc (it only leaves the field).
    let seen = egui::Id::new("sdc-keys-focus");
    let focused = ctx.egui_wants_keyboard_input();
    let before = ctx.data(|d| d.get_temp::<bool>(seen)).unwrap_or(false);
    ctx.data_mut(|d| d.insert_temp(seen, focused));
    let text = focused || (before && ctx.input(|i| i.key_pressed(Key::Escape)));
    let blocked = set_modal
        || model.dialog.is_some()
        || egui::Popup::is_any_open(ctx)
        || super::columns::combo_menu_open(ctx)
        || ctx.memory(|m| m.top_modal_layer().is_some());
    if blocked {
        return None;
    }
    // AG23: Ctrl+S waits for the field; say so instead of doing nothing.
    if text && model.is_dirty() {
        let save = action("save").map_or(&[][..], |a| a.keys);
        let pressed = ctx.input(|i| {
            save.iter()
                .any(|s| i.key_pressed(s.key) && i.modifiers.matches_logically(s.mods))
        });
        if pressed {
            model.status_line = "Ctrl+S: leave the text field first".into();
            // Once per press: a held key repeats, and the log should not fill with the same line.
            let fresh = ctx.input(|i| {
                i.events.iter().any(|e| {
                    matches!(e, egui::Event::Key { repeat: false, pressed: true, key, .. }
                        if save.iter().any(|s| s.key == *key))
                })
            });
            if fresh {
                model.messages.log(
                    crate::model::messages::Severity::Info,
                    model.status_line.clone(),
                );
            }
        }
    }
    let meshes = super::header::meshes_view_active(ctx);
    // Trace #7: a disabled action keeps its keys in the match when an enabled action shares the key (egui ignores
    // an extra Shift, so Ctrl+Shift+Z with nothing to redo would otherwise be Undo). It eats the key, runs nothing.
    let mut pairs: Vec<(&'static Action, Shortcut, bool)> = actions()
        .iter()
        .filter(|a| a.run.is_some() && live(a.area, model, meshes) && (!text || a.id == PALETTE))
        .flat_map(|a| {
            let on = enabled(a, model, ctx);
            a.keys.iter().map(move |s| (a, *s, on))
        })
        .collect();
    keep_disabled_shadows(&mut pairs);
    pairs.sort_by_key(|(_, s, _)| std::cmp::Reverse(s.weight()));
    let (a, _, on) = pairs
        .into_iter()
        .find(|(_, s, _)| ctx.input_mut(|i| i.consume_key(s.mods, s.key)))?;
    if !on {
        return None;
    }
    (a.run?)(model, ctx);
    Some(a.id)
}

/// Keeps every enabled pair, and a disabled pair only when an enabled one shares its key (it eats the key, trace #7)
/// but not its exact (key, modifiers): an enabled twin of the same shortcut must win.
fn keep_disabled_shadows<T>(pairs: &mut Vec<(T, Shortcut, bool)>) {
    let live: Vec<Shortcut> = pairs.iter().filter(|p| p.2).map(|p| p.1).collect();
    pairs.retain(|p| p.2 || (live.iter().any(|s| s.key == p.1.key) && !live.contains(&p.1)));
}

/// The sheet and the palette, while open. Drawn with the dialogs, so both views have them.
pub fn draw(ctx: &egui::Context, model: &mut AppModel) {
    if model.nav.sheet {
        sheet(ctx, model);
    }
    if model.nav.palette.is_some() {
        palette(ctx, model);
    }
}

/// The F1 sheet: every action with a key, by area.
fn sheet(ctx: &egui::Context, model: &mut AppModel) {
    let mut close = false;
    let k = super::dialogs::modal_keys(ctx, "keys-sheet", true, false, |ui| {
        ui.heading("Keyboard shortcuts");
        egui::ScrollArea::vertical()
            .max_height(520.0)
            .show(ui, |ui| {
                for area in Area::ALL {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(area.title()).strong());
                    egui::Grid::new(("keys-sheet", area.title()))
                        .num_columns(2)
                        .striped(true)
                        .show(ui, |ui| {
                            for a in actions()
                                .iter()
                                .filter(|a| a.area == area && !a.keys.is_empty())
                            {
                                ui.monospace(keys_text(a.id));
                                ui.label(a.label);
                                ui.end_row();
                            }
                        });
                }
            });
        close = ui.button("Close").clicked();
    });
    if close || k.cancel || k.enter {
        model.nav.sheet = false;
    }
}

/// The actions the palette offers now: enabled, in an area that applies, not the palette itself.
fn palette_actions(
    model: &AppModel,
    meshes_view: bool,
    ctx: &egui::Context,
) -> Vec<(&'static str, &'static str, String)> {
    actions()
        .iter()
        .filter(|a| {
            a.palette
                && a.run.is_some()
                && live(a.area, model, meshes_view)
                && enabled(a, model, ctx)
        })
        .map(|a| (a.id, a.label, keys_text(a.id)))
        .collect()
}

/// The Ctrl+K palette: a search field over the actions, the textures and the sets.
fn palette(ctx: &egui::Context, model: &mut AppModel) {
    let Some(mut p) = model.nav.palette.clone() else {
        return;
    };
    let meshes = super::header::meshes_view_active(ctx);
    let offered = palette_actions(model, meshes, ctx);
    let mut chosen = None;
    let k = super::dialogs::modal_keys(ctx, "palette", true, false, |ui| {
        ui.set_min_width(420.0);
        // Before the field, so it does not move its cursor with them; a second Ctrl+K must not reach it either (egui's
        // Ctrl+K deletes to the end of the line).
        let (up, down) = ui.input_mut(|i| {
            i.consume_key(Modifiers::COMMAND, Key::K);
            (
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
            )
        });
        let r = ui.add(
            egui::TextEdit::singleline(&mut p.query)
                .hint_text("Search actions, textures and sets")
                .desired_width(f32::INFINITY),
        );
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        // The field keeps the focus while the palette is open: typing always searches.
        if p.focus || !r.has_focus() {
            r.request_focus();
            p.focus = false;
        }
        // The arrows move the highlight: egui must not move the focus to a result row with them.
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                r.id,
                egui::EventFilter {
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    ..Default::default()
                },
            )
        });
        if r.changed() {
            p.sel = 0;
        }
        let hits = model.palette_hits(&p.query, &offered);
        let last = hits.len().saturating_sub(1);
        if up {
            p.sel = p.sel.saturating_sub(1);
        }
        if down {
            p.sel = (p.sel + 1).min(last);
        }
        p.sel = p.sel.min(last);
        if hits.is_empty() {
            ui.weak("Nothing matches.");
        }
        let row = ui.spacing().interact_size.y;
        egui::ScrollArea::vertical()
            .max_height(360.0)
            .auto_shrink([false, true])
            .show_rows(ui, row, hits.len(), |ui, range| {
                for i in range {
                    let h = &hits[i];
                    let text = format!("{}   {}", h.label, h.detail);
                    let r = ui.selectable_label(i == p.sel, text);
                    if i == p.sel && (up || down) {
                        r.scroll_to_me(None);
                    }
                    if r.clicked() {
                        chosen = Some(h.pick.clone());
                    }
                }
            });
        if enter {
            chosen = chosen
                .take()
                .or_else(|| hits.get(p.sel).map(|h| h.pick.clone()));
        }
    });
    if k.cancel {
        model.nav.palette = None;
        return;
    }
    let Some(pick) = chosen else {
        model.nav.palette = Some(p);
        return;
    };
    model.nav.palette = None;
    match &pick {
        Pick::Action(id) => run(id, model, ctx),
        _ => {
            // A texture or a set is picked in the Textures view.
            super::header::set_meshes_view(ctx, false);
            model.palette_pick(&pick);
        }
    }
}

/// The "Keyboard" table of docs/gui.md, from [`actions`]: one row per action with a key.
pub fn markdown_table() -> String {
    let mut s = String::from("| Area | Keys | Action |\n|---|---|---|\n");
    for area in Area::ALL {
        for a in actions()
            .iter()
            .filter(|a| a.area == area && !a.keys.is_empty())
        {
            s.push_str(&format!(
                "| {} | {} | {} |\n",
                area.title(),
                keys_text(a.id),
                a.label
            ));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sc(mods: Modifiers) -> Shortcut {
        Shortcut { mods, key: Key::Z }
    }

    #[test]
    fn a_disabled_pair_yields_to_an_enabled_twin_but_shadows_a_different_modifier() {
        let mut pairs = vec![
            (1, sc(Modifiers::COMMAND), true),
            (2, sc(Modifiers::COMMAND), false), // same (key, mods) as an enabled pair: dropped
            (3, sc(Modifiers::COMMAND | Modifiers::SHIFT), false), // same key only: kept (trace #7)
            (
                4,
                Shortcut {
                    mods: Modifiers::NONE,
                    key: Key::Enter,
                },
                false,
            ), // key nothing enabled uses: dropped
        ];
        keep_disabled_shadows(&mut pairs);
        let ids: Vec<i32> = pairs.iter().map(|p| p.0).collect();
        assert_eq!(ids, vec![1, 3]);
    }
}
