//! GUI tweaks G1, Note AO: the set header's input name box. A text box with gray Tab completion, a path lock rail
//! and a ▾ list of known paths. Drawing only; the change is `AppModel::set_input_name`.
use super::columns::RED;
use super::path_picker;
use crate::model::input_name::{completion, lock_cut, separator_stops, tab_part};
use crate::model::known_paths::strip_dds;
use crate::model::sets::SetView;
use crate::model::AppModel;
use eframe::egui::{self, text::CCursor, text::CCursorRange, Key, Modifiers};
use std::sync::Arc;

/// The box's placeholder (and how tests find it).
pub const HINT: &str = "Input name";
/// The button that opens the list of known paths.
pub const DROPDOWN: &str = "▾";
/// The list's heading.
pub const HEADING: &str = "Read this set's textures as";
/// The free (editable) part of the box is never narrower than this.
const MIN_FREE: f32 = 56.0;

/// The same input name: case, `\` and a `.dds` ending aside.
fn same(a: &str, b: &str) -> bool {
    let n = |s: &str| strip_dds(&s.trim().replace('\\', "/").to_lowercase()).to_string();
    n(a) == n(b)
}

pub fn picker(ui: &mut egui::Ui, model: &mut AppModel, v: &SetView) {
    let Some(current) = model.input_name_of(&v.id) else {
        return;
    };
    let why = model
        .project
        .as_ref()
        .and_then(|p| p.error())
        .map(|e| format!("sdc.toml is read-only: {e}"));
    let id = egui::Id::new(("input-name", &v.id));
    let err_id = id.with("error");
    let edit_id = id.with("edit");
    let buf = id.with("buf");
    let lock_id = id.with("lock");
    let key = (Arc::as_ptr(&model.known) as usize, model.items_rev());
    let paths = path_picker::cached(ui, "input-name", key, || model.input_name_candidates());
    // The edit buffer carries the frame it was last drawn in (as the colour hex box does).
    let frame = ui.ctx().cumulative_frame_nr();
    let mut text = ui
        .data(|d| d.get_temp::<(String, u64)>(buf))
        .filter(|(_, f)| f + 1 >= frame)
        .map_or_else(|| current.clone(), |(t, _)| t);
    let stops = separator_stops(&text).len();
    // The lock defaults to the folder: the file name is free (AO).
    let mut lock = ui
        .data(|d| d.get_temp::<usize>(lock_id))
        .unwrap_or(separator_stops(&current).len())
        .min(stops);
    let font = egui::TextStyle::Body.resolve(ui.style());
    let ctx = ui.ctx().clone();
    let width = |s: &str| {
        ctx.fonts_mut(|f| f.layout_no_wrap(s.to_string(), font.clone(), egui::Color32::WHITE))
            .size()
            .x
    };
    // Too little room left in the row: the box starts the next one (decided here: the enabled scope below is one block).
    let locked_w = width(&text[..lock_cut(&text, lock)]);
    if ui.available_size_before_wrap().x - 64.0 - locked_w < (160.0 - locked_w).max(MIN_FREE) {
        ui.end_row();
    }
    ui.add_enabled_ui(why.is_none(), |ui| {
        let focused = ui.memory(|m| m.has_focus(edit_id));
        if focused {
            if ui.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::ArrowLeft)) {
                lock = lock.saturating_sub(1);
            }
            if ui.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::ArrowRight)) {
                lock = (lock + 1).min(stops);
            }
        }
        let cut = lock_cut(&text, lock);
        let locked = text[..cut].to_string();
        let mut free = text[cut..].to_string();
        let mut ghost = focused.then(|| completion(&paths, &text)).flatten();
        if ghost.is_some() {
            let mut state = egui::TextEdit::load_state(ui.ctx(), edit_id).unwrap_or_default();
            let at_end = state
                .cursor
                .char_range()
                .is_none_or(|r| r.primary.index.0 >= free.chars().count());
            let took = if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Tab)) {
                tab_part(&paths, &text)
            } else if at_end && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowRight)) {
                ghost.clone()
            } else {
                // Tab is held in the box while a ghost shows; Shift+Tab still leaves it.
                if ui.input_mut(|i| i.consume_key(Modifiers::SHIFT, Key::Tab)) {
                    ui.memory_mut(|m| m.move_focus(egui::FocusDirection::Previous));
                }
                None
            };
            if let Some(t) = took {
                free.push_str(&t);
                text = format!("{locked}{free}");
                ghost = completion(&paths, &text);
                let end = CCursor::new(free.chars().count());
                state.cursor.set_char_range(Some(CCursorRange::one(end)));
                state.store(ui.ctx(), edit_id);
            }
        }
        let err = (!same(&text, &current))
            .then(|| model.input_name_plan(&v.id, &text).err())
            .flatten();
        let locked_w = width(&locked);
        // The box is at least 160 pt (its free part at least MIN_FREE) and grows with its text and ghost, up to the room left
        // in the row and 480 pt (AO5). `room` leaves the margins and the ▾.
        let min_free = (160.0 - locked_w).max(MIN_FREE);
        let room = |ui: &egui::Ui| ui.available_size_before_wrap().x - 64.0 - locked_w;
        let want = width(&free) + ghost.as_deref().map_or(0.0, width) + 24.0;
        let free_w = want.min(room(ui)).clamp(min_free, 480.0);
        let mut commit = None;
        let inner = egui::Frame::new()
            .stroke(ui.visuals().widgets.inactive.bg_stroke)
            .fill(ui.visuals().extreme_bg_color)
            .corner_radius(2.0)
            .inner_margin(egui::Margin::symmetric(4, 0))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.horizontal(|ui| {
                    if !locked.is_empty() {
                        ui.add(egui::Label::new(egui::RichText::new(&locked).weak()).selectable(false))
                            .on_hover_text("Locked: drag the rail under the box, or Ctrl+Left/Right, to move the lock");
                    }
                    let out = egui::TextEdit::singleline(&mut free)
                        .id(edit_id)
                        .hint_text(HINT)
                        .frame(egui::Frame::NONE)
                        .lock_focus(ghost.is_some())
                        .desired_width(free_w)
                        .show(ui);
                    ui.ctx().accesskit_node_builder(edit_id, |b| {
                        b.set_label(HINT);
                        if let Some(e) = &err {
                            b.set_description(e.as_str());
                        }
                    });
                    if let Some(g) = &ghost {
                        let at = out.galley_pos + egui::vec2(out.galley.rect.width(), 0.0);
                        ui.painter().with_clip_rect(out.text_clip_rect).text(
                            at,
                            egui::Align2::LEFT_TOP,
                            g,
                            font.clone(),
                            ui.visuals().weak_text_color(),
                        );
                    }
                    (out, ui.min_rect().left())
                })
                .inner
            });
        let (out, left) = inner.inner;
        let r = &out.response.response;
        let box_rect = inner.response.rect;
        text = format!("{locked}{free}");
        if let Some(e) = &err {
            ui.painter().rect_stroke(
                box_rect,
                2.0,
                egui::Stroke::new(1.5, RED),
                egui::StrokeKind::Outside,
            );
            inner.response.clone().on_hover_text(e);
            r.clone().on_hover_text(e);
        }
        if r.has_focus() || r.changed() {
            ui.data_mut(|d| d.insert_temp(buf, (text.clone(), frame)));
        }
        if r.lost_focus() {
            ui.data_mut(|d| d.remove::<(String, u64)>(buf));
            // Esc reverts; an invalid or unchanged value reverts; a valid change commits (one commit path, AO).
            let cancelled = ui.input(|i| i.key_pressed(Key::Escape));
            if !cancelled && err.is_none() && !same(&text, &current) {
                commit = Some(text.clone());
            }
        }
        // The lock rail along the box's lower edge: a stop at the start and at each separator.
        let xs: Vec<f32> = std::iter::once(left)
            .chain(separator_stops(&text).into_iter().map(|i| {
                if i < cut {
                    left + width(&text[..=i])
                } else {
                    out.galley_pos.x + width(&text[cut..=i])
                }
            }))
            .map(|x| x.clamp(box_rect.left(), box_rect.right()))
            .collect();
        if xs.len() > 1 {
            let rail = egui::Rect::from_min_max(
                egui::pos2(box_rect.left(), box_rect.bottom() - 4.0),
                box_rect.right_bottom(),
            );
            let rr = ui
                .interact(rail, id.with("rail"), egui::Sense::click_and_drag())
                .on_hover_text("Path lock: drag to a / to keep the path up to it (Ctrl+Left/Right in the box)");
            if let Some(p) = rr.interact_pointer_pos().filter(|_| rr.clicked() || rr.dragged()) {
                lock = (0..xs.len())
                    .min_by(|a, b| (xs[*a] - p.x).abs().total_cmp(&(xs[*b] - p.x).abs()))
                    .unwrap_or(lock);
            }
            let painter = ui.painter();
            let weak = ui.visuals().weak_text_color();
            for x in &xs {
                painter.vline(*x, rail.y_range(), egui::Stroke::new(1.0, weak));
            }
            let hx = xs[lock.min(xs.len() - 1)];
            painter.line_segment(
                [egui::pos2(rail.left(), rail.bottom()), egui::pos2(hx, rail.bottom())],
                egui::Stroke::new(2.0, ui.visuals().selection.bg_fill),
            );
            painter.circle_filled(egui::pos2(hx, rail.center().y), 3.0, ui.visuals().selection.bg_fill);
        }
        let menu = path_picker::menu(ui, DROPDOWN, |ui| {
            ui.label(egui::RichText::new(HEADING).strong());
            ui.weak(format!("now: {current}"));
            // Only paths under the lock.
            let pre = text[..cut].replace('\\', "/");
            let under: Vec<String> = paths
                .iter()
                .filter(|p| p.get(..cut).is_some_and(|h| h.eq_ignore_ascii_case(&pre)))
                .cloned()
                .collect();
            if let Some(p) = path_picker::list(ui, ("input-name", &v.id), &under, false) {
                match model.set_input_name(&v.id, &p) {
                    Ok(()) => {
                        ui.data_mut(|d| d.remove::<String>(err_id));
                        ui.close();
                    }
                    Err(e) => {
                        ui.data_mut(|d| d.insert_temp(err_id, e));
                    }
                }
            }
            if let Some(e) = ui.data(|d| d.get_temp::<String>(err_id)) {
                ui.colored_label(RED, e);
            }
        });
        menu.response.on_hover_text(
            "Pick the game path this set's textures are read as. Each texture keeps its own suffix; files on \
             disk are not renamed (an [inputs] alias in sdc.toml).",
        );
        // The box was validated as typed; a refusal here (sdc.toml went read-only) shows in the list.
        if let Some(p) = commit {
            if let Err(e) = model.set_input_name(&v.id, &p) {
                ui.data_mut(|d| d.insert_temp(err_id, e));
            }
        }
    })
    .response
    .on_disabled_hover_text(why.unwrap_or_default());
    ui.data_mut(|d| d.insert_temp(lock_id, lock));
}
