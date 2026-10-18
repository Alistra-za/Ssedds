//! The set editor, layout A (M3b spec §4): slot rows (layout v2 A2) on the left, PGPatcher settings on the right (Task 15).
//! Drawing only: every change is a model call, made after drawing.
use super::columns::{border_of, bulk_menu, status_text, AMBER, RED};
use super::rnm_section::RNM_OUTLINE;
use super::sets_column::{DropState, Pending, SlotNote};
use super::slot_commands::SlotCmdState;
use super::status::{dot, Status};
use super::thumbs::ThumbCache;
use crate::model::bottom::tile_ready;
use crate::model::bulk::MenuAction;
use crate::model::fields::{
    channel_text, encoding_options, field_hidden, field_lock, field_view, format_label,
    PARALLAX_HOVER,
};
use crate::model::items::{ItemKind, ItemStatus, TextureItem};
use crate::model::out_size::{human, SizeEst};
use crate::model::pack_fills::{fill_rows, FillRow};
use crate::model::rnm::{rnm_active, rnm_rows};
use crate::model::sets::{
    card_title, is_json_set, set_views, shows_json_panel, slot_clash_text, slot_label, slot_of,
    slot_row, SetView, SlotCell, SlotFill, SlotKind,
};
use crate::model::AppModel;
use crate::pgp::prefix::{
    active_matches, extra_match_refusal, lock_key, match_toggle, pgp_locked, slot_match_path,
};
use eframe::egui;
use engine::config::{ColorTag, Format, PackRecipe, RawMap, Target};
use engine::events::RecipeReport;
use engine::pack::RecipeKind;
use serde_json::Value;
use std::collections::HashMap;

/// Note AN: the caption across a PGPatcher-locked slot card.
/// Review M2: a lock key on a set that writes no JSON.
pub const PGP_LOCK_UNWRITTEN: &str = "lock stored, not written (no PGPatcher entry)";

const THUMB: f32 = 56.0;
/// The format and recipe combos (A5).
const COMBO_W: f32 = 96.0;
/// The File cell is the stretchy one (it truncates); a Grid column sizes itself from its content, so it gets a width
/// between these: what the row's other cells leave of the column.
const FILE_MIN: f32 = 60.0;
const FILE_MAX: f32 = 240.0;
/// Between the row cells.
const SPACING: f32 = 4.0;
/// Seconds a single click waits for its second click (a little over egui's double-click delay).
const DOUBLE_CLICK_WAIT: f64 = 0.35;

enum Act {
    Select(String),
    Expand(String),
    Format(String, Format, ColorTag),
    Opaque(String, bool),
    /// "Enable parallax" on a complex `_m` (DECISIONS AH2).
    Parallax(String, bool),
    /// Clear the `_m`'s own `alpha.parallax` (the reset next to a locked box).
    ParallaxReset(String),
    Recipe(String, PackRecipe),
    Fill(String, RawMap, Option<u8>),
    /// A PGPatcher setting of a set: (set id, key, value; `None` removes it).
    Json(String, String, Option<Value>),
    /// Note AN: read a locked slot's original textures (set id, slot) on a worker.
    AskOriginal(String, u8),
    /// F3a Task 8: a slot menu pick.
    Slot(MenuAction),
    /// Item 17: files from Explorer dropped on this set's slot card.
    FileDrop(String, SlotKind),
    /// Note AO7: take a member out of the set (set id, item), from a clashing slot card.
    RemoveMember(String, String),
}

/// How the set editor shares the view with the 3D preview panel.
#[derive(Debug, Clone, Copy, Default)]
pub struct View {
    /// The 2D | 3D switch is offered (the app has a 3D preview).
    pub switch: bool,
    /// The 3D panel shows this set: the slot rows are left out, the TruePBR settings stay.
    pub three_d: bool,
}

pub fn draw(
    ui: &mut egui::Ui,
    model: &mut AppModel,
    key: &str,
    thumbs: &mut ThumbCache,
    cmds: &mut SlotCmdState,
    drops: &mut DropState,
    view: View,
) {
    let Some(v) = set_views(model).into_iter().find(|v| v.key == key) else {
        ui.weak("This set is no longer in the plan.");
        return;
    };
    ui.horizontal_wrapped(|ui| {
        if view.switch {
            super::set_preview::view_switch(ui, model);
        }
        // The set's path and its input name box share the row where the width allows (AO5).
        super::input_name::picker(ui, model, &v);
        if view.three_d {
            super::set_preview::settings_toggle(ui, model);
        }
        // The output prefix and its error are PGPatcher JSON facts: only a JSON set shows them.
        if shows_json_panel(&v, model.target) {
            match (&v.prefix, &v.prefix_error) {
                (Some(p), _) => ui.weak(format!("→ {p}")),
                (_, Some(e)) => ui.colored_label(RED, e),
                _ => ui.weak(""),
            };
        }
    });
    // F3b: the set's output name, or why it is ignored.
    let mut clear_name = false;
    if let Some(n) = model.output_name_of(&v.id) {
        ui.horizontal_wrapped(|ui| {
            if v.pbr && model.target == Target::Cs {
                ui.colored_label(
                    AMBER,
                    format!("Output name {n:?} is ignored on a PBR set; use Rename"),
                );
            } else {
                ui.weak(format!("Outputs named {n}"));
            }
            clear_name = ui.small_button("Clear").clicked();
        });
    }
    if clear_name {
        model.clear_output_name(&v.id);
    }
    // G6: a delete section that matches this set's textures deletes the shapes the set is for.
    for h in crate::model::sections::delete_hits(model)
        .iter()
        .filter(|h| engine::sets::same_id(&h.set_id, &v.id))
    {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(
                AMBER,
                format!(
                    "⚠ Delete section {} deletes this set's shapes: {}",
                    h.section + 1,
                    h.why
                ),
            );
            if ui.small_button("Open the JSON editor").clicked() {
                model.json_editor_open = true;
            }
        });
    }
    ui.separator();
    let mut acts = vec![];
    // Item 17: where a file from Explorer was dropped this frame, if one waits for a slot card.
    let drop_at = drops.file.as_ref().and_then(|f| f.pos);
    let editable = model.sets_doc.as_ref().is_some_and(|d| !d.read_only());
    let why = model.banner.clone().unwrap_or_default();
    // `size_of` needs the model mutably, which the drawing below does not have.
    let sizes: HashMap<String, SizeEst> = v
        .members
        .iter()
        .filter_map(|k| model.size_of(k).map(|e| (k.clone(), e)))
        .collect();
    if view.three_d {
        // 3D (AG132): the set's 3D view takes the rest of this pane (`ui::centre`); the TruePBR settings and the skin
        // card are in the TruePBR settings… window, as are the CS PBR section and the complex material card, so an edit
        // still shows live in 3D.
        return;
    }
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.columns(2, |cols| {
                let ui = &mut cols[0];
                rows(ui, model, &v, thumbs, &mut acts, drop_at, &sizes);
                slot_note(ui, drops, &v.id);
                unslotted_packed(ui, model, &v, &mut acts);
                ui.add_space(6.0);
                // A2: a normal button, not stretched across the justified column.
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(editable, |ui| {
                        if ui.button("Slot commands…").clicked() {
                            cmds.open = Some(v.id.clone());
                        }
                    })
                    .response
                    .on_disabled_hover_text(&why);
                });
                under_grid(ui, model, &v);
                super::plugin_refs::set_section(ui, model, &v);
                super::cs_pbr::section(ui, model, &v);
                super::cs_pbr::card(&mut cols[1], model, &v);
                if shows_json_panel(&v, model.target) {
                    super::cs_pbr::pgp_panel(&mut cols[1], model, &v);
                }
                if v.skin {
                    super::skin_card::card(&mut cols[1], model, &v);
                }
                super::cm_card::section(&mut cols[1], model, &v);
            });
        });
    let (mut select, mut expand) = (None, None);
    for a in acts {
        match a {
            Act::Select(k) => select = Some(k),
            Act::Expand(k) => expand = Some(k),
            Act::Format(k, f, t) => model.set_format(&k, f, t),
            Act::Opaque(k, on) => model.set_opaque(&k, on),
            Act::Parallax(k, on) => model.set_field(&k, "alpha.parallax", serde_json::json!(on)),
            Act::ParallaxReset(k) => model.clear_field(&k, "alpha.parallax"),
            Act::Recipe(k, r) => {
                model.set_packing_recipe(&k, r);
            }
            Act::Fill(k, role, v) => model.set_pack_fill(&k, role, v),
            Act::Json(set, k, v) => model.set_json_value(&set, &k, v),
            Act::AskOriginal(set, n) => model.ask_original_textures(&set, n),
            Act::Slot(a) => super::slot_commands::run(model, cmds, a),
            Act::FileDrop(set, slot) => file_drop(model, drops, &set, slot),
            Act::RemoveMember(set, item) => {
                if let Err(why) = model.remove_member(&set, &item) {
                    drops.slot_note = Some(SlotNote {
                        set,
                        text: why,
                        error: true,
                    });
                }
            }
        }
    }
    select_or_expand(ui, model, &v.id, select, expand);
}

/// Item 17: a file from Explorer dropped on slot `slot` of `set`. The drop is taken (its folder is not opened); a
/// filled slot asks Replace / Cancel as a texture dragged onto the set does, anything refused is the error line.
fn file_drop(model: &mut AppModel, drops: &mut DropState, set: &str, slot: SlotKind) {
    let Some(f) = drops.file.take() else { return };
    let r = model.drop_file_on_slot(set, slot, &f.paths);
    let line = |text: String, error: bool| SlotNote {
        set: set.to_string(),
        text,
        error,
    };
    drops.slot_note = match r.outcome {
        crate::model::sets::DropOutcome::Refused(why) => Some(line(why, true)),
        crate::model::sets::DropOutcome::AskReplace { existing } => {
            if let Some(item) = r.item {
                drops.pending = Some(Pending::Replace {
                    item,
                    set: set.to_string(),
                    existing,
                    choice: crate::model::sets::DropChoice::Move,
                });
            }
            r.note.map(|n| line(n.into(), false))
        }
        // `drop_choice` with Move never asks the Share / Duplicate choice.
        _ => r.note.map(|n| line(n.into(), false)),
    };
}

/// Item 17: the last file drop's error or note, under the slot cards of the set it was for.
fn slot_note(ui: &mut egui::Ui, drops: &DropState, set: &str) {
    if let Some(n) = drops.slot_note.as_ref().filter(|n| n.set == set) {
        if n.error {
            ui.colored_label(RED, &n.text);
        } else {
            ui.weak(&n.text);
        }
    }
}

/// The recipe lines, the set's warnings (each member's, each once) and what matches nothing.
fn under_grid(ui: &mut egui::Ui, model: &mut AppModel, v: &SetView) {
    let Some(set) = model.sets.iter().find(|s| s.key == v.key) else {
        return;
    };
    for r in &set.recipes {
        ui.add_space(4.0);
        recipe(ui, r);
    }
    for w in model.rnm_notes.get(&v.key).into_iter().flatten() {
        ui.colored_label(AMBER, format!("{}: {}", w.code.as_str(), w.message));
    }
    // R10: the codes live in Problems; one line points there.
    let n = crate::model::bottom::set_problem_count(model, &v.key);
    if n > 0 {
        ui.add_space(6.0);
        let text = format!(
            "{n} problem{} in this set — Show in Problems",
            if n == 1 { "" } else { "s" }
        );
        if ui.link(text).clicked() {
            model.open_problems(Some(crate::model::bottom::ProblemsFilter::Set(
                v.key.clone(),
            )));
        }
    }
    for m in &v.missing {
        ui.colored_label(AMBER, format!("no file matches {m}"));
    }
}

fn recipe(ui: &mut egui::Ui, r: &RecipeReport) {
    let (word, color) = status_text(r.status);
    let file = r.output.rsplit('/').next().unwrap_or(&r.output);
    let line = egui::RichText::new(format!("{file}: {word}"));
    ui.label(match color {
        Some(c) => line.color(c),
        None => line,
    });
    if let Some(reason) = &r.reason {
        ui.label(egui::RichText::new(reason).small().weak());
    }
    for (name, p) in ["R", "G", "B", "A"].iter().zip(&r.channels) {
        if !p.is_empty() {
            ui.label(egui::RichText::new(format!("{name}: {}", channel_text(p))).small());
        }
    }
}

/// Layout v2 A2: one row per slot, in PGPatcher slot order: Slot · thumbnail · File · Format · Flags · Status.
fn rows(
    ui: &mut egui::Ui,
    model: &AppModel,
    v: &SetView,
    thumbs: &mut ThumbCache,
    acts: &mut Vec<Act>,
    drop_at: Option<egui::Pos2>,
    sizes: &HashMap<String, SizeEst>,
) {
    // The File cell takes what the other columns leave: their widest content last frame, plus the spacing.
    let others_id = egui::Id::new(("slot-rows-others", &v.id));
    let others = ui.data(|d| d.get_temp::<[f32; 5]>(others_id));
    // A few pixels short of the column: rows exactly as wide as it would flip the sideways scroll bar on and off.
    let file_w = others.map_or(FILE_MIN, |w| {
        (ui.available_width() - w.iter().sum::<f32>() - 5.0 * SPACING - 8.0)
            .clamp(FILE_MIN, FILE_MAX)
    });
    let mut widths = [0.0f32; 5];
    // A ScrollArea takes every scroll target set inside it, the vertical one too, so a row widget asked to scroll into
    // view (ScrollIntoView) would not move the set editor's vertical scroll: its row is asked again after.
    let asks = |ui: &egui::Ui| {
        ui.input(|i| {
            i.events
                .iter()
                .filter(|e| {
                    matches!(e, egui::Event::AccessKitActionRequest(r)
                        if r.action == egui::accesskit::Action::ScrollIntoView)
                })
                .count()
        })
    };
    let mut reveal = None;
    // Never drawn under the panel beside it: a column narrower than the rows scrolls them sideways.
    egui::ScrollArea::horizontal()
        .id_salt(("slot-rows-scroll", &v.id))
        .show(ui, |ui| {
            egui::Grid::new(("slot-rows", &v.id))
                .num_columns(6)
                .striped(true)
                .spacing([SPACING, 4.0])
                .min_col_width(0.0)
                .show(ui, |ui| {
                    // The row takes the right-click; its labels are not selectable, so they do not take it.
                    ui.style_mut().interaction.selectable_labels = false;
                    for cell in slot_row(v.shader.as_ref(), &v.slots) {
                        let size = match &cell.fill {
                            SlotFill::Own { item } | SlotFill::Borrowed { item, .. } => {
                                sizes.get(item)
                            }
                            SlotFill::Empty => None,
                        };
                        let before = asks(ui);
                        let (w, rect) =
                            row(ui, model, v, &cell, thumbs, acts, drop_at, size, file_w);
                        if asks(ui) < before {
                            reveal = Some(rect);
                        }
                        for (a, b) in widths.iter_mut().zip(w) {
                            *a = a.max(b);
                        }
                    }
                });
        });
    ui.data_mut(|d| d.insert_temp(others_id, widths));
    if let Some(r) = reveal {
        ui.scroll_to_rect(r, Some(egui::Align::Center));
    }
}

/// A Flags icon toggle: its accessible name is the full `label`, which also leads its hover. True when clicked.
fn flag(
    ui: &mut egui::Ui,
    enabled: bool,
    on: bool,
    icon: &str,
    label: &str,
    hover: &str,
    why: &str,
) -> bool {
    let r = ui
        .add_enabled(enabled, egui::Button::selectable(on, icon))
        .on_hover_text(format!("{label}. {hover}"))
        .on_disabled_hover_text(why);
    ui.ctx()
        .accesskit_node_builder(r.id, |b| b.set_label(label));
    r.clicked()
}

#[allow(clippy::too_many_arguments)]
fn row(
    ui: &mut egui::Ui,
    model: &AppModel,
    v: &SetView,
    cell: &SlotCell,
    thumbs: &mut ThumbCache,
    acts: &mut Vec<Act>,
    drop_at: Option<egui::Pos2>,
    size: Option<&SizeEst>,
    file_w: f32,
) -> ([f32; 5], egui::Rect) {
    let slot = cell.kind;
    let fill = &cell.fill;
    // K1: a Grid cell cannot hold the whole row, so last frame's row rect takes the right-click (F3a Task 8) and the
    // Explorer drop (item 17). It is registered before the cells, so they keep their own clicks.
    let row_id = egui::Id::new(("slot-row", &v.id, slot.slot_number()));
    if let Some(r) = ui.data(|d| d.get_temp::<egui::Rect>(row_id)) {
        let resp = ui.interact(r, row_id, egui::Sense::click());
        slot_menu(&resp, model, v, slot, acts);
        // The clipped rect: a row scrolled out of view takes no drop (trace-debug #51).
        if drop_at.is_some_and(|p| resp.interact_rect.contains(p)) {
            acts.push(Act::FileDrop(v.id.clone(), slot));
        }
    }
    let top = ui.cursor().min;
    // The blue outline: this normal blends RNM layers (F2 spec §5.2). The amber one wins.
    let rnm_layers: Vec<&str> = match (fill, slot) {
        (
            SlotFill::Own { item } | SlotFill::Borrowed { item, .. },
            SlotKind::Normal | SlotKind::Coat,
        ) => model
            .item(item)
            .and_then(|i| i.job.as_ref())
            .filter(|j| rnm_active(j))
            .map(|j| {
                rnm_rows(j, &model.rnm_keys, &|_| None)
                    .iter()
                    .map(|r| r.label)
                    .collect()
            })
            .unwrap_or_default(),
        _ => vec![],
    };
    let locked = v.locks.contains(&slot.slot_number());
    let title = match fill {
        SlotFill::Own { item } | SlotFill::Borrowed { item, .. } => {
            card_title(slot, item, v.shader.as_ref().map(|p| p.shader))
        }
        SlotFill::Empty => slot_label(slot, ""),
    };
    let (item_key, borrowed) = match fill {
        SlotFill::Own { item } => (Some(item.clone()), None),
        SlotFill::Borrowed { from, item } => (Some(item.clone()), Some(from.clone())),
        SlotFill::Empty => (None, None),
    };
    let item = item_key.as_deref().and_then(|k| model.item(k));

    // Slot: the title, dim when empty.
    let text = if locked {
        format!("🔒 {title}")
    } else {
        title.to_string()
    };
    let text = egui::RichText::new(text);
    let t = ui.label(if item.is_some() {
        text.strong()
    } else {
        text.weak()
    });
    let slot_w = t.rect.width();
    let t = if locked {
        t.on_hover_text("Slot pinned (sdc): right-click to unpin")
    } else {
        t
    };
    if cell.usable && !rnm_layers.is_empty() {
        t.clone()
            .on_hover_text(format!("RNM: {}", rnm_layers.join(" → ")));
    }
    if let (false, Some(p)) = (cell.usable, &v.shader) {
        t.on_hover_text(format!(
            "{title}: the {} shader does not use slot {}",
            p.shader.label(),
            slot.slot_number()
        ));
    }

    // Thumbnail.
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(THUMB, THUMB), egui::Sense::click());
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Image, true, format!("{title} thumbnail"))
    });
    slot_menu(&resp, model, v, slot, acts);
    // A PGPatcher-locked slot has one hover, the lock's, in place of the thumbnail's path.
    let lock_cross = pgp_locked(&v.settings, slot.slot_number()) && is_json_set(v, model.target);
    let resp = if lock_cross {
        pgp_lock_hover(resp, model, v, slot, acts)
    } else {
        resp
    };
    if ui.is_rect_visible(rect) {
        match item.and_then(|i| thumbs.get(ui.ctx(), i)) {
            Some(t) => egui::Image::new(&t)
                .fit_to_exact_size(rect.size())
                .paint_at(ui, rect),
            None => {
                ui.painter().rect_filled(
                    rect,
                    2.0,
                    ui.visuals().widgets.noninteractive.weak_bg_fill,
                );
            }
        }
        if let Some(i) = item {
            ui.painter().rect_stroke(
                rect,
                2.0,
                egui::Stroke::new(1.0, border_of(ui, &i.status)),
                egui::StrokeKind::Inside,
            );
        }
        if lock_cross {
            pgp_lock_cross(ui, rect);
        }
    }
    if let Some(i) = item.filter(|_| borrowed.is_none()) {
        let resp = if lock_cross {
            resp
        } else {
            resp.on_hover_text(&i.key)
        };
        if resp.double_clicked() {
            acts.push(Act::Expand(i.key.clone()));
        } else if resp.clicked() {
            acts.push(Act::Select(i.key.clone()));
        }
    }

    // File: the name (truncated; full name and size on hover) and a weak size line, or "empty"; a clashing slot lists
    // both members under it.
    ui.vertical(|ui| {
        ui.set_width(file_w);
        match item {
            Some(i) => {
                let size =
                    size.map(|e| format!("{}×{} · {}", e.width, e.height, human(e.file_bytes())));
                let hover = match &size {
                    Some(s) => format!("{}\n{s}", i.name),
                    None => i.name.clone(),
                };
                ui.add(egui::Label::new(egui::RichText::new(&i.name).small()).truncate())
                    .on_hover_text(hover);
                if let Some(s) = size {
                    ui.add(egui::Label::new(egui::RichText::new(s).small().weak()).truncate());
                }
            }
            None => {
                ui.weak("empty");
            }
        }
        // Note AO7: both kinds of a shared slot (slot 7: `_cnr` and `_f`) are a conflict, not a silent pick: the row
        // names and shows both, and either can be taken out of the set here.
        if slot_clash_text(cell).is_some() {
            clash_rows(ui, model, v, cell, thumbs, acts);
        }
    });

    // Format, or Assign… on an empty slot (R13: the slot's own menu); a packed output's recipe and fills under it.
    let toml_read_only = model
        .project
        .as_ref()
        .and_then(|p| p.error())
        .map(|e| format!("sdc.toml is read-only: {e}"));
    let own = item.filter(|_| borrowed.is_none());
    let own_job = own.and_then(|i| i.job.as_ref().map(|j| (i, j)));
    let format_w = ui
        .vertical(|ui| {
            if item.is_none() {
                let menu = ui.menu_button("Assign…", |ui| {
                    let title = format!("Slot {}: {}", slot.slot_number(), slot_label(slot, ""));
                    bulk_menu(ui, &title, &model.slot_menu(&v.id, slot))
                });
                if let Some(Some(a)) = menu.inner {
                    acts.push(Act::Slot(a));
                }
            }
            let Some((i, job)) = own_job else { return };
            let (cur_f, cur_t) = (job.profile.format, job.profile.color.tag);
            // Format and Opaque write `sdc.toml`: a read-only one would drop the edit silently.
            let lock = toml_read_only
                .clone()
                .or(field_lock(model.target, &job.profile, "format").map(str::to_string));
            let mut choice = (cur_f, cur_t);
            ui.add_enabled_ui(lock.is_none(), |ui| {
                egui::ComboBox::from_id_salt(("slot-format", &v.id, slot.label()))
                    .selected_text(format_label(cur_f, cur_t))
                    .width(COMBO_W)
                    .show_ui(ui, |ui| {
                        for (e, label) in encoding_options(model.target, &job.profile) {
                            ui.selectable_value(&mut choice, e, label);
                        }
                    });
            })
            .response
            .on_disabled_hover_text(lock.unwrap_or_default());
            if choice != (cur_f, cur_t) {
                acts.push(Act::Format(i.key.clone(), choice.0, choice.1));
            }
            if i.kind == ItemKind::Packed {
                packed_controls(ui, model, i, toml_read_only.as_deref(), acts);
            }
        })
        .response
        .rect
        .width();

    // Flags: small icon toggles, Match on this (slots 1/2 of a JSON set) and Opaque (the diffuse). Each keeps its
    // full label as its accessible name and leads its hover with it.
    let flags_w = ui
        .vertical(|ui| {
            if let (Some(n), Some(_)) = (
                match slot {
                    _ if !shows_json_panel(v, model.target) => None,
                    SlotKind::Diffuse => Some(1),
                    SlotKind::Normal => Some(2),
                    _ => None,
                },
                item,
            ) {
                // F1 spec §3.2: the row's switch is the Matching header's slot 1/2 row.
                let derived = v.derived_match.as_deref().unwrap_or("");
                let on = active_matches(&v.settings, derived)
                    .iter()
                    .any(|(m, _)| *m == n);
                let editable = model.sets_doc.as_ref().is_some_and(|d| !d.read_only());
                let path = slot_match_path(derived, slot.suffix());
                // AN2: a renamed set takes no second match.
                let (k, val) = match_toggle(n, true, &path);
                let refused = (!on)
                    .then(|| extra_match_refusal(&v.settings, derived, &k, val.as_ref()))
                    .flatten();
                if flag(
                    ui,
                    editable && refused.is_none(),
                    on,
                    "🔗",
                    "Match on this",
                    "PGPatcher applies the entry to meshes with this exact texture in this slot",
                    refused.unwrap_or_default(),
                ) {
                    let (k, val) = match_toggle(n, !on, &path);
                    acts.push(Act::Json(v.id.clone(), k, val));
                }
            }
            let Some(i) = own.filter(|i| i.job.is_some() && slot == SlotKind::Diffuse) else {
                return;
            };
            let on = model.is_opaque(&i.key);
            if flag(
                ui,
                toml_read_only.is_none(),
                on,
                "◼",
                "Opaque",
                "Strips alpha: the diffuse is fully opaque",
                toml_read_only.as_deref().unwrap_or_default(),
            ) {
                acts.push(Act::Opaque(i.key.clone(), !on));
            }
        })
        .response
        .rect
        .width();

    // Status: the texture's dot (K3), with the short lines (K4: the title's hovers keep the long forms) on its hover.
    let status_w = ui
        .horizontal(|ui| {
            let mut lines = vec![];
            if let Some(from) = &borrowed {
                lines.push(format!("borrowed from {from}"));
            }
            if let (false, Some(p)) = (cell.usable, &v.shader) {
                lines.push(format!("not used by {}", p.shader.label()));
            } else if !rnm_layers.is_empty() {
                lines.push(format!("RNM, {} layers", rnm_layers.len()));
            }
            if let Some(i) = item {
                let s = match i.status {
                    ItemStatus::Failed(_) | ItemStatus::Error(_) => Status::Blocks,
                    _ if tile_ready(i) => Status::Ready,
                    _ => Status::Warning,
                };
                let r = dot(ui, s);
                if !lines.is_empty() {
                    r.on_hover_text(lines.join("\n"));
                }
            }
            // A4: only a set that writes JSON gives PGPatcher the lock (review M2); otherwise a note.
            if pgp_locked(&v.settings, slot.slot_number()) && !lock_cross {
                let r = ui
                    .add(egui::Label::new(egui::RichText::new("ⓘ").weak()))
                    .on_hover_text(PGP_LOCK_UNWRITTEN);
                ui.ctx()
                    .accesskit_node_builder(r.id, |b| b.set_value(PGP_LOCK_UNWRITTEN));
            }
        })
        .response
        .rect
        .width();
    let rect = egui::Rect::from_min_max(top, ui.min_rect().max);
    ui.end_row();
    ui.data_mut(|d| d.insert_temp(row_id, rect));
    // A filled slot the shader cannot use stays at its place, outlined, and says why (F1 spec §5); a normal blending RNM
    // layers is outlined blue.
    let outline = if !cell.usable {
        Some(AMBER)
    } else if !rnm_layers.is_empty() {
        Some(RNM_OUTLINE)
    } else {
        None
    };
    if let Some(c) = outline {
        ui.painter().rect_stroke(
            rect,
            2.0,
            egui::Stroke::new(2.0, c),
            egui::StrokeKind::Outside,
        );
    }
    ([slot_w, THUMB, format_w, flags_w, status_w], rect)
}

/// Note AN: a PGPatcher-locked slot: a big red ✕ over the thumbnail.
fn pgp_lock_cross(ui: &egui::Ui, rect: egui::Rect) {
    let stroke = egui::Stroke::new(6.0, RED);
    let r = rect.shrink(6.0);
    ui.painter()
        .line_segment([r.left_top(), r.right_bottom()], stroke);
    ui.painter()
        .line_segment([r.right_top(), r.left_bottom()], stroke);
}

/// Note AN: the locked card's one hover: the lock, then (with a scan) the textures the matched meshes have in that slot,
/// read on a worker ("…" until it lands).
fn pgp_lock_hover(
    resp: egui::Response,
    model: &AppModel,
    v: &SetView,
    slot: SlotKind,
    acts: &mut Vec<Act>,
) -> egui::Response {
    let n = slot.slot_number();
    resp.on_hover_ui(|ui| {
        ui.label(format!(
            "PGPatcher lock ({}): the original texture in this spot is used",
            lock_key(n).unwrap_or_default()
        ));
        match model.original_textures(&v.id, n) {
            Some(Some(paths)) if !paths.is_empty() => {
                for p in paths.iter() {
                    ui.monospace(p);
                }
            }
            Some(Some(_)) => {
                ui.weak("The scan has no mesh with this set's diffuse");
            }
            // The read runs on a worker (AN6); it lands on a later frame.
            Some(None) => {
                ui.weak("…");
                acts.push(Act::AskOriginal(v.id.clone(), n));
            }
            None => {}
        }
    })
}

/// F3a Task 8: the slot menu of card `slot` (the model's list, drawn as the bulk menus are).
fn slot_menu(
    resp: &egui::Response,
    model: &AppModel,
    v: &SetView,
    slot: SlotKind,
    acts: &mut Vec<Act>,
) {
    resp.context_menu(|ui| {
        let title = format!("Slot {}: {}", slot.slot_number(), slot_label(slot, ""));
        if let Some(a) = bulk_menu(ui, &title, &model.slot_menu(&v.id, slot)) {
            acts.push(Act::Slot(a));
        }
    });
}

/// A click selects a texture, which replaces this view by its settings, so the second click of a double click
/// would land on something else. The selection therefore waits out the double-click delay; a double click
/// meanwhile expands the texture instead. The pending click is set `set`'s and lives only while its view is drawn
/// every pass: another set's view never sees it, and a pass without this view drops it (trace-debug #53).
fn select_or_expand(
    ui: &egui::Ui,
    model: &mut AppModel,
    set: &str,
    clicked: Option<String>,
    expand: Option<String>,
) {
    type Pending = (String, f64, u64);
    let id = egui::Id::new(("set-editor-pending-select", set));
    let (now, pass) = (ui.input(|i| i.time), ui.ctx().cumulative_pass_nr());
    if let Some(k) = expand {
        ui.data_mut(|d| d.remove::<Pending>(id));
        model.expand(&k);
        return;
    }
    if let Some(k) = clicked {
        ui.data_mut(|d| d.insert_temp(id, (k, now, pass)));
    }
    let Some((k, at, seen)) = ui.data(|d| d.get_temp::<Pending>(id)) else {
        return;
    };
    let left = DOUBLE_CLICK_WAIT - (now - at);
    // A click left pending while this view was not drawn (another selection took over) is stale: drop it.
    if left <= 0.0 || seen + 1 < pass {
        ui.data_mut(|d| d.remove::<Pending>(id));
        if seen + 1 >= pass {
            model.select_item(&k);
        }
    } else {
        ui.data_mut(|d| d.insert_temp(id, (k, at, pass)));
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(left));
    }
}

fn role_label(role: RawMap) -> String {
    match role {
        RawMap::Ao => "AO".into(),
        other => other.id(),
    }
}

/// A packed output's recipe choice (cs `_rmaos`/`_rfaos`) and its fill rows. Both write `sdc.toml`, so both are
/// disabled, with the reason on hover, while it is read-only.
fn packed_controls(
    ui: &mut egui::Ui,
    model: &AppModel,
    item: &TextureItem,
    why: Option<&str>,
    acts: &mut Vec<Act>,
) {
    let Some(pk) = item.job.as_ref().and_then(|j| j.pack.as_ref()) else {
        return;
    };
    ui.add_enabled_ui(why.is_none(), |ui| {
        if model.target == Target::Cs && matches!(pk.kind, RecipeKind::Rmaos | RecipeKind::Rfaos) {
            let label = |k: RecipeKind| match k {
                RecipeKind::Rfaos => "RFAOS (skin)",
                _ => "RMAOS",
            };
            let explicit = model.explicit_recipe(&item.key);
            let auto = explicit == PackRecipe::Auto;
            let recipe = |k: RecipeKind| {
                if k == RecipeKind::Rfaos {
                    PackRecipe::Rfaos
                } else {
                    PackRecipe::Rmaos
                }
            };
            let unbuildable = model.rfaos_unbuildable(&item.key);
            egui::ComboBox::from_id_salt(("pack-recipe", &item.key))
                .selected_text(format!(
                    "{}{}",
                    label(pk.kind),
                    if auto { " (auto)" } else { "" }
                ))
                .width(COMBO_W)
                .show_ui(ui, |ui| {
                    for k in [RecipeKind::Rmaos, RecipeKind::Rfaos] {
                        // Final review I2: RFAOS is offered only where the set can build it.
                        let why = unbuildable.filter(|_| k == RecipeKind::Rfaos);
                        let r = ui
                            .add_enabled(
                                why.is_none(),
                                egui::Button::selectable(pk.kind == k, label(k)),
                            )
                            .on_disabled_hover_text(why.unwrap_or_default());
                        // Picking the built recipe again still writes it when an explicit value differs: a stale
                        // hand-written `rfaos` the engine could not follow is cleared by picking RMAOS. An `auto`
                        // choice is never pinned by a re-click.
                        if r.clicked()
                            && (k != pk.kind
                                || (explicit != PackRecipe::Auto && explicit != recipe(k)))
                        {
                            acts.push(Act::Recipe(item.key.clone(), recipe(k)));
                        }
                    }
                });
        }
        fill_controls(ui, model, &item.key, acts);
        if let Some(note) = model.dropped_fills.get(&item.key) {
            ui.add(egui::Label::new(egui::RichText::new(note).small().weak()).wrap());
        }
    })
    .response
    .on_disabled_hover_text(why.unwrap_or_default());
}

/// One row per channel that a fill can stand in for: a value box with a reset while the channel has no source,
/// read-only text naming the file when it has one.
fn fill_controls(ui: &mut egui::Ui, model: &AppModel, key: &str, acts: &mut Vec<Act>) {
    for row in fill_rows(model, key) {
        match row {
            FillRow::FromFile { role, file } => {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(format!("{}: from {file}", role_label(role))).small(),
                    )
                    .truncate(),
                );
            }
            FillRow::Value {
                role,
                shown,
                own_key,
            } => {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(role_label(role)).small());
                    let mut v = shown;
                    if super::advanced::settled_drag(ui, &mut v, 1.0, 0..=255) {
                        acts.push(Act::Fill(key.to_string(), role, Some(v)));
                    }
                    if own_key
                        && ui
                            .small_button("Reset")
                            .on_hover_text("Remove this fill")
                            .clicked()
                    {
                        acts.push(Act::Fill(key.to_string(), role, None));
                    }
                });
            }
        }
    }
}

/// Outputs that fill no slot card get their controls here, under the cards: the complex `_m` (packed or loose)
/// its "Enable parallax" box (DECISIONS AH2), a packed output its fill rows.
fn unslotted_packed(ui: &mut egui::Ui, model: &AppModel, v: &SetView, acts: &mut Vec<Act>) {
    let why = super::settings_panel::toml_read_only_reason(model);
    for item in v
        .members
        .iter()
        .filter_map(|m| model.item(m))
        .filter(|i| slot_of(i).is_none())
    {
        let parallax = item
            .job
            .as_ref()
            .filter(|j| !field_hidden(&j.profile, "alpha.parallax"))
            .and_then(|j| field_view(model.target, j, "alpha.parallax"));
        let fills = item.kind == ItemKind::Packed && !fill_rows(model, &item.key).is_empty();
        if parallax.is_none() && !fills {
            continue;
        }
        ui.add_space(6.0);
        ui.label(egui::RichText::new(&item.name).strong());
        ui.add_enabled_ui(why.is_none(), |ui| {
            if let Some(f) = &parallax {
                ui.horizontal(|ui| {
                    let mut on = f.value.as_bool().unwrap_or(false);
                    let r = ui
                        .add_enabled(
                            f.lock.is_none(),
                            egui::Checkbox::new(&mut on, "Enable parallax"),
                        )
                        .on_hover_text(PARALLAX_HOVER)
                        .on_disabled_hover_text(f.lock.unwrap_or_default());
                    if r.changed() {
                        acts.push(Act::Parallax(item.key.clone(), on));
                    }
                    // A locked box the texture's own entry ticked (no source alpha) cannot be unticked: reset it.
                    if f.edited
                        && ui
                            .small_button("↺")
                            .on_hover_text("Use the inherited value")
                            .clicked()
                    {
                        acts.push(Act::ParallaxReset(item.key.clone()));
                    }
                });
            }
            if fills {
                fill_controls(ui, model, &item.key, acts)
            }
        })
        .response
        .on_disabled_hover_text(why.clone().unwrap_or_default());
    }
}

/// Note AO7: a row whose PGPatcher slot holds two members: a red line naming both files, then a line per member with
/// ✕, which takes it out of the set, its thumbnail and its file name (click selects, double-click expands).
fn clash_rows(
    ui: &mut egui::Ui,
    model: &AppModel,
    v: &SetView,
    cell: &SlotCell,
    thumbs: &mut ThumbCache,
    acts: &mut Vec<Act>,
) {
    let item_of = |fill: Option<&SlotFill>| match fill {
        Some(SlotFill::Own { item } | SlotFill::Borrowed { item, .. }) => model.item(item),
        _ => None,
    };
    let both: Vec<&TextureItem> = [Some(&cell.fill), cell.clash.and_then(|k| v.slots.get(&k))]
        .into_iter()
        .filter_map(item_of)
        .collect();
    let names: Vec<&str> = both.iter().map(|i| i.name.as_str()).collect();
    ui.colored_label(
        RED,
        format!(
            "{} both use slot {}: remove one",
            names.join(" and "),
            cell.kind.slot_number()
        ),
    );
    for i in both {
        // The remove button first: the File cell is narrow, and the name truncates after it.
        ui.horizontal(|ui| {
            let x = ui
                .small_button("✕")
                .on_hover_text("Take it out of this set");
            let label = format!("Remove {}", i.name);
            ui.ctx()
                .accesskit_node_builder(x.id, |b| b.set_label(label));
            if x.clicked() {
                acts.push(Act::RemoveMember(v.id.clone(), i.key.clone()));
            }
            if let Some(t) = thumbs.get(ui.ctx(), i) {
                ui.add(egui::Image::new(&t).fit_to_exact_size(egui::vec2(20.0, 20.0)));
            }
            let r = ui
                .add(
                    egui::Label::new(egui::RichText::new(&i.name).small())
                        .truncate()
                        .sense(egui::Sense::click()),
                )
                .on_hover_text(&i.key);
            if r.double_clicked() {
                acts.push(Act::Expand(i.key.clone()));
            } else if r.clicked() {
                acts.push(Act::Select(i.key.clone()));
            }
        });
    }
}
