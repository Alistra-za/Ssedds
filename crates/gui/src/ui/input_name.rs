//! GUI tweaks G1, Note AO: the set header's input name box. A text box with gray Tab completion, a path lock rail
//! and a ▾ list of known paths. Drawing only; the change is `AppModel::set_input_name`.
use super::columns::RED;
use super::json_editor::SLOT_NAMES;
use super::path_picker;
use crate::model::input_name::{
    bare_name, completion, in_form_of, live_rows, lock_cut, separator_stops, tab_part,
};
use crate::model::known_paths::strip_dds;
use crate::model::sets::is_json_set;
use crate::model::sets::SetView;
use crate::model::AppModel;
use crate::pgp::prefix::stored_set_match;
use eframe::egui::{self, text::CCursor, text::CCursorRange, Key, Modifiers};
use std::sync::Arc;

/// The box's placeholder (and how tests find it).
pub const HINT: &str = "Input name";
/// The button that opens the list of known paths.
pub const DROPDOWN: &str = "▾";
/// The list's heading.
pub const HEADING: &str = "Read this set's textures as";
/// Note DO: the list's "show every slot" toggle (TruePBR JSON sets only).
pub const ALL_SLOTS: &str = "All slots";
/// The free (editable) part of the box is never narrower than this.
const MIN_FREE: f32 = 56.0;
/// What the box needs besides its text: its margins, the ▾ and the spacing between them.
const CHROME: f32 = 44.0;

/// The path lock handle's accessible name (Note BH2).
pub const LOCK_LABEL: &str = "Path lock";
/// The lock icon's hit box.
const LOCK_W: f32 = 12.0;
const LOCK_H: f32 = 14.0;
/// BH13: the box's bottom padding, the track the rail and padlock sit on (below the text).
const LOCK_TRACK: f32 = 4.0;

/// A padlock centred on `c`: a filled body under a shackle.
pub(super) fn paint_lock(painter: &egui::Painter, c: egui::Pos2, color: egui::Color32) {
    let body = egui::Rect::from_center_size(c + egui::vec2(0.0, 1.5), egui::vec2(9.0, 6.0));
    let (l, r, top) = (body.left() + 2.0, body.right() - 2.0, body.top() - 3.0);
    let stroke = egui::Stroke::new(1.6, color);
    painter.line_segment(
        [egui::pos2(l, body.top()), egui::pos2(l, top + 1.0)],
        stroke,
    );
    painter.line_segment(
        [egui::pos2(r, body.top()), egui::pos2(r, top + 1.0)],
        stroke,
    );
    painter.line_segment([egui::pos2(l, top + 1.0), egui::pos2(l + 1.0, top)], stroke);
    painter.line_segment([egui::pos2(l + 1.0, top), egui::pos2(r - 1.0, top)], stroke);
    painter.line_segment([egui::pos2(r - 1.0, top), egui::pos2(r, top + 1.0)], stroke);
    painter.rect_filled(body, 1.5, color);
}

/// Rows in the live list under the box.
const LIVE_ROWS: usize = 12;

/// Note AO4: the live list's state, per set, in egui memory.
#[derive(Clone, Default)]
struct Live {
    open: bool,
    /// The highlighted row (Up/Down); `None` until an arrow is pressed, so Enter takes the typed text.
    sel: Option<usize>,
    /// Last frame's rows and where they were drawn, for a press on one.
    rows: Vec<(String, egui::Rect)>,
    /// The row a mouse press landed on, until its release.
    pressed: Option<(String, egui::Rect)>,
    /// A row pressed and released on: the focus loss that click causes commits it, not the typed text.
    clicked: Option<String>,
}

/// The same input name: case, `\` and a `.dds` ending aside.
fn same(a: &str, b: &str) -> bool {
    let n = |s: &str| strip_dds(&s.trim().replace('\\', "/").to_lowercase()).to_string();
    n(a) == n(b)
}

/// Why `text` cannot be the set's input name (`None` when it can, or is the current name). `input_name_plan` is
/// cached per (text, items_rev): the box asks every frame while it holds a changed name.
fn plan_error(
    ui: &egui::Ui,
    model: &AppModel,
    v: &SetView,
    id: egui::Id,
    text: &str,
    current: &str,
) -> Option<String> {
    if same(text, current) {
        return None;
    }
    let key = id.with("plan");
    let rev = model.items_rev();
    type Cached = (String, u64, Option<String>);
    if let Some((t, r, e)) = ui.data(|d| d.get_temp::<Cached>(key)) {
        if t == text && r == rev {
            return e;
        }
    }
    let e = model.input_name_plan(&v.id, text).err();
    ui.data_mut(|d| d.insert_temp::<Cached>(key, (text.to_string(), rev, e.clone())));
    e
}

/// The set's input name box, which also names the set (Note BH1); a set without an own diffuse shows its path.
/// `matching`: draw the Note DO match selector (a JSON set's 2D view; the 3D view's header row has no room).
pub fn picker(ui: &mut egui::Ui, model: &mut AppModel, v: &SetView, matching: bool) {
    let Some(current) = model.input_name_of(&v.id) else {
        ui.heading(&v.id);
        return;
    };
    let why = model
        .project
        .as_ref()
        .and_then(|p| p.error())
        .map(|e| format!("sdc.toml is read-only: {e}"))
        .or_else(|| {
            let e = model.sets_doc.as_ref()?.error()?;
            Some(format!("sdc-project.toml is read-only: {e}"))
        });
    let id = egui::Id::new(("input-name", &v.id));
    let err_id = id.with("error");
    let edit_id = id.with("edit");
    let buf = id.with("buf");
    let lock_id = id.with("lock");
    let key = (Arc::as_ptr(&model.known) as usize, model.items_rev());
    let salt = if bare_name(&current) {
        "input-name-bare"
    } else {
        "input-name"
    };
    let paths = path_picker::cached(ui, salt, key, || {
        in_form_of(model.input_name_candidates(), &current)
    });
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
    // Note BH1: the box alone names the set (no heading beside it). With too little room left in the row it starts
    // the next one (decided here: the enabled scope below is one block). AO5.
    let locked_w = width(&text[..lock_cut(&text, lock)]);
    let box_w = (160.0 - locked_w).max(MIN_FREE) + locked_w + CHROME;
    if ui.available_size_before_wrap().x < box_w {
        ui.end_row();
    }
    ui.add_enabled_ui(why.is_none(), |ui| {
        let focused = ui.memory(|m| m.has_focus(edit_id));
        let live_id = id.with("live");
        let mut live = ui
            .data(|d| d.get_temp::<Live>(live_id))
            .unwrap_or_default();
        let shown = focused && live.open && !live.rows.is_empty();
        if focused {
            if ui.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::ArrowLeft)) {
                lock = lock.saturating_sub(1);
            }
            if ui.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::ArrowRight)) {
                lock = (lock + 1).min(stops);
            }
        }
        if shown {
            // Before the box sees the click (and loses focus to it): the row pressed, then released on the same row.
            let press = ui.input(|i| i.pointer.any_pressed().then(|| i.pointer.interact_pos()).flatten());
            if let Some(p) = press {
                live.pressed = live.rows.iter().find(|(_, r)| r.contains(p)).cloned();
            }
        }
        if live.pressed.is_some() && ui.input(|i| i.pointer.any_released()) {
            let at = ui.input(|i| i.pointer.interact_pos());
            live.clicked = live
                .pressed
                .take()
                .filter(|(_, r)| at.is_some_and(|p| r.contains(p)))
                .map(|(s, _)| s);
        }
        if shown {
            let n = live.rows.len();
            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowDown)) {
                live.sel = Some(live.sel.map_or(0, |s| (s + 1).min(n - 1)));
            }
            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowUp)) {
                live.sel = live.sel.and_then(|s| s.checked_sub(1));
            }
            // The first Esc closes the list; the box keeps its text and its focus.
            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                live.open = false;
                live.sel = None;
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
            // Trace #8: egui matches Tab with an extra Shift, so Shift+Tab is taken before plain Tab.
            let back = ui.input_mut(|i| i.consume_key(Modifiers::SHIFT, Key::Tab));
            if back {
                ui.memory_mut(|m| m.move_focus(egui::FocusDirection::Previous));
            }
            let took = if back {
                None
            } else if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Tab)) {
                tab_part(&paths, &text)
            } else if at_end && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowRight)) {
                ghost.clone()
            } else {
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
        let err = plan_error(ui, model, v, id, &text, &current);
        let locked_w = width(&locked);
        // The box is at least 160 pt (its free part at least MIN_FREE) and grows with its text and ghost, up to the room left
        // in the row and 480 pt (AO5). `room` leaves the margins and the ▾.
        let min_free = (160.0 - locked_w).max(MIN_FREE);
        let room = |ui: &egui::Ui| ui.available_size_before_wrap().x - CHROME - locked_w;
        let want = width(&free) + ghost.as_deref().map_or(0.0, width) + 24.0;
        let free_w = want.min(room(ui)).clamp(min_free, 480.0);
        let mut commit = None;
        let inner = egui::Frame::new()
            .stroke(ui.visuals().widgets.inactive.bg_stroke)
            .fill(ui.visuals().extreme_bg_color)
            .corner_radius(2.0)
            // BH13: the text sits at the top; the text edit's bottom margin is the track for the rail and padlock
            // (the box is as tall as before).
            .inner_margin(egui::Margin::symmetric(4, 0))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.horizontal_top(|ui| {
                    if !locked.is_empty() {
                        ui.add(egui::Label::new(egui::RichText::new(&locked).weak()).selectable(false))
                            .on_hover_text("Locked: drag the rail under the box, or Ctrl+Left/Right, to move the lock");
                    }
                    let out = egui::TextEdit::singleline(&mut free)
                        .id(edit_id)
                        .hint_text(HINT)
                        .frame(egui::Frame::NONE)
                        .margin(egui::Margin { left: 4, right: 4, top: 0, bottom: LOCK_TRACK as i8 })
                        // Tab stays in the box while a ghost shows; Esc while the live list shows (AO2, AO4).
                        .event_filter(egui::EventFilter {
                            tab: ghost.is_some(),
                            escape: shown && live.open,
                            horizontal_arrows: true,
                            vertical_arrows: true,
                        })
                        .desired_width(free_w)
                        .show(ui);
                    ui.ctx().accesskit_node_builder(edit_id, |b| {
                        b.set_label(HINT);
                        if let Some(e) = &err {
                            b.set_description(e.as_str());
                        }
                    });
                    (out, ui.min_rect().left())
                })
                .inner
            });
        let (out, left) = inner.inner;
        let r = &out.response.response;
        let box_rect = inner.response.rect;
        // A pasted full path under the lock: drop its locked part rather than doubling it.
        let mut stripped = false;
        if r.changed() && !locked.is_empty() {
            let n = locked.len();
            let norm = |s: &str| s.replace('\\', "/");
            if free
                .get(..n)
                .is_some_and(|h| norm(h).eq_ignore_ascii_case(&norm(&locked)))
            {
                free = free[n..].to_string();
                stripped = true;
                let mut state = egui::TextEdit::load_state(ui.ctx(), edit_id).unwrap_or_default();
                let end = CCursor::new(free.chars().count());
                state.cursor.set_char_range(Some(CCursorRange::one(end)));
                state.store(ui.ctx(), edit_id);
            }
        }
        text = format!("{locked}{free}");
        // The ghost follows the text as edited this frame (review fix): drawn after the edit, not from last frame's.
        ghost = r.has_focus().then(|| completion(&paths, &text)).flatten();
        if let Some(g) = ghost.as_ref().filter(|_| !stripped) {
            let at = out.galley_pos + egui::vec2(out.galley.rect.width(), 0.0);
            ui.painter().with_clip_rect(out.text_clip_rect).text(
                at,
                egui::Align2::LEFT_TOP,
                g,
                font.clone(),
                ui.visuals().weak_text_color(),
            );
        }
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
        // AO4: the live list follows the text while the box has focus.
        let rows = if r.has_focus() || r.lost_focus() {
            live_rows(&paths, &text, cut, LIVE_ROWS)
        } else {
            vec![]
        };
        if r.changed() {
            live.open = true;
            live.sel = None;
            // The next frame's Tab and Esc hold must match the new text, not the one the box was shown with.
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    edit_id,
                    egui::EventFilter {
                        tab: ghost.is_some(),
                        escape: !rows.is_empty(),
                        horizontal_arrows: true,
                        vertical_arrows: true,
                    },
                )
            });
            ui.ctx().request_repaint();
        }
        if r.lost_focus() {
            ui.data_mut(|d| d.remove::<(String, u64)>(buf));
            // A pressed row, else the highlighted row on Enter, else the typed text (AO4). Esc reverts; an invalid or
            // unchanged value reverts; a valid change commits (one commit path, AO1).
            let cancelled = ui.input(|i| i.key_pressed(Key::Escape));
            let row = live.clicked.take().or_else(|| {
                live.sel
                    .filter(|_| live.open && ui.input(|i| i.key_pressed(Key::Enter)))
                    .and_then(|s| rows.get(s).cloned())
            });
            if let Some(p) = row {
                commit = (!same(&p, &current)).then_some(p);
            } else if !cancelled && err.is_none() && !same(&text, &current) {
                commit = Some(text.clone());
            }
            live = Live::default();
        }
        let list_rect = egui::Rect::from_min_size(
            box_rect.left_bottom() + egui::vec2(0.0, 2.0),
            egui::vec2(box_rect.width().max(320.0), 0.0),
        );
        live.rows.clear();
        if r.has_focus() && live.open && !rows.is_empty() {
            egui::Area::new(live_id.with("area"))
                .fade_in(false)
                .order(egui::Order::Foreground)
                .fixed_pos(list_rect.min)
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(list_rect.width());
                        for (i, p) in rows.iter().enumerate() {
                            // The top row is the best match, the ghost's (AO4).
                            let text = egui::RichText::new(p);
                            let text = if i == 0 { text.strong() } else { text };
                            let rr = ui.add(egui::Button::selectable(live.sel == Some(i), text));
                            live.rows.push((p.clone(), rr.rect));
                        }
                    });
                });
        }
        ui.data_mut(|d| d.insert_temp(live_id, live.clone()));
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
                egui::pos2(box_rect.left(), box_rect.bottom() - LOCK_TRACK),
                box_rect.right_bottom(),
            );
            let rr = ui
                .interact(rail, id.with("rail"), egui::Sense::click_and_drag())
                .on_hover_text("Path lock: drag to a / to keep the path up to it (Ctrl+Left/Right in the box)");
            // Note BH2: the handle is a blue lock icon on the rail, dragged like the rail; BH13: centred on the box's
            // lower edge, so it sits on the track below the text.
            let lock_y = box_rect.bottom() + 0.5;
            let hx = xs[lock.min(xs.len() - 1)];
            let handle_rect =
                egui::Rect::from_center_size(egui::pos2(hx, lock_y), egui::vec2(LOCK_W, LOCK_H));
            let hr = ui
                .interact(handle_rect, id.with("lock-handle"), egui::Sense::click_and_drag())
                .on_hover_text("Path lock: drag to a / to keep the path up to it (Ctrl+Left/Right in the box)");
            hr.widget_info(|| egui::WidgetInfo::slider(true, lock as f64, LOCK_LABEL));
            let cur = lock;
            let snap = |p: egui::Pos2| {
                (0..xs.len())
                    .min_by(|a, b| (xs[*a] - p.x).abs().total_cmp(&(xs[*b] - p.x).abs()))
                    .unwrap_or(cur)
            };
            for r in [&rr, &hr] {
                if let Some(p) = r.interact_pointer_pos().filter(|_| r.clicked() || r.dragged()) {
                    lock = snap(p);
                }
            }
            let painter = ui.painter();
            let weak = ui.visuals().weak_text_color();
            for x in &xs {
                painter.vline(*x, rail.y_range(), egui::Stroke::new(1.0, weak));
            }
            // Note FC9: the path lock is a control, not a status, so it takes the one accent (teal), not a blue.
            let accent = ui.visuals().selection.stroke.color;
            let blue = if hr.dragged() || hr.is_pointer_button_down_on() {
                accent.lerp_to_gamma(egui::Color32::BLACK, 0.3)
            } else if hr.hovered() || rr.hovered() {
                accent.lerp_to_gamma(egui::Color32::WHITE, 0.3)
            } else {
                accent
            };
            let hx = xs[lock.min(xs.len() - 1)];
            painter.line_segment(
                [egui::pos2(rail.left(), rail.bottom()), egui::pos2(hx, rail.bottom())],
                egui::Stroke::new(2.0, blue),
            );
            paint_lock(painter, egui::pos2(hx, lock_y), blue);
        }
        // Note DO: on a TruePBR JSON set, the slot the ▾ list picks for. Slot 1 names the set (the rename below);
        // slots 2–8 set that slot's match. Note FC12: it starts from the user's choice or the set's lowest unused
        // match, and only the user moves it (a pick from All slots goes to it).
        let json = matching && is_json_set(v, model.target);
        let slot_id = id.with("match-slot");
        let all_id = id.with("all-slots");
        let mut slot: u8 = if json {
            ui.data(|d| d.get_temp(slot_id))
                .unwrap_or_else(|| model.match_slot_start(v))
        } else {
            1
        };
        let chosen = slot;
        let mut all = json && ui.data(|d| d.get_temp(all_id)).unwrap_or(false);
        let name = |n: u8| format!("Match {n} · {}", SLOT_NAMES[n as usize - 1]);
        if json {
            egui::ComboBox::from_id_salt(slot_id)
                // Short in the header row, so it does not wrap a narrow 3D view's header; the list names the slots.
                .selected_text(format!("Match {slot}"))
                .width(64.0)
                .show_ui(ui, |ui| {
                    for n in 1..=8 {
                        ui.selectable_value(&mut slot, n, name(n));
                    }
                })
                .response
                .on_hover_text(
                    "The slot the ▾ list picks for. Slot 1 renames this set's textures; slots 2–8 set that slot's \
                     match in the PGPatcher JSON.",
                );
        }
        let derived = v.derived_match.clone().unwrap_or_default();
        let menu = path_picker::menu(ui, DROPDOWN, |ui| {
            if json {
                let r = ui
                    .checkbox(&mut all, ALL_SLOTS)
                    .on_hover_text("List textures used in any slot. A pick goes to the match selected beside the box.");
                if r.changed() {
                    ui.data_mut(|d| d.insert_temp(all_id, all));
                }
            }
            if slot == 1 && !all {
                ui.label(egui::RichText::new(HEADING).strong());
                ui.weak(format!("now: {current}"));
                // Only paths under the lock.
                let pre = text[..cut].replace('\\', "/");
                let under_key = (Arc::as_ptr(&paths) as usize, egui::Id::new(&pre).value());
                let under = path_picker::cached(ui, "input-name-under", under_key, || {
                    paths
                        .iter()
                        .filter(|p| p.get(..cut).is_some_and(|h| h.eq_ignore_ascii_case(&pre)))
                        .cloned()
                        .collect()
                });
                if under.is_empty() && !paths.is_empty() {
                    // Note DO: the scan knows paths, just none under the lock.
                    ui.weak(format!(
                        "No known paths under {pre}: move the path lock left to see more."
                    ));
                } else if let Some(p) = path_picker::list(ui, ("input-name", &v.id), &under, false) {
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
            } else {
                let what = if all { "any slot".to_string() } else { format!("slot {slot}") };
                ui.label(egui::RichText::new(format!("Textures used in {what}")).strong());
                let list_key = (Arc::as_ptr(&model.known) as usize, egui::Id::new((model.items_rev(), slot, all)).value());
                let list = path_picker::cached(ui, "input-name-slot", list_key, || {
                    let mut l = model.known_textures((!all).then_some(slot - 1));
                    // AO8: PGPatcher's own outputs are never a match.
                    l.retain(|p| !p.get(..13).is_some_and(|h| h.eq_ignore_ascii_case("textures/pbr/")));
                    l
                });
                if let Some(p) = path_picker::list(ui, ("input-name-slot", &v.id, slot, all), &list, false) {
                    let done = if slot == 1 {
                        let p = in_form_of(vec![p], &current).remove(0);
                        model.set_input_name(&v.id, &p)
                    } else {
                        let value = stored_set_match(slot, &p.replace('/', "\\"), &derived);
                        model.set_json_value(&v.id, &format!("match{slot}"), value);
                        Ok(())
                    };
                    match done {
                        Ok(()) => {
                            ui.data_mut(|d| d.remove::<String>(err_id));
                            ui.close();
                        }
                        Err(e) => {
                            ui.data_mut(|d| d.insert_temp(err_id, e));
                        }
                    }
                }
            }
            if let Some(e) = ui.data(|d| d.get_temp::<String>(err_id)) {
                ui.colored_label(RED, e);
            }
        });
        ui.data_mut(|d| d.insert_temp(slot_id, slot));
        if slot != chosen {
            model.set_match_slot(&v.id, slot);
        }
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
