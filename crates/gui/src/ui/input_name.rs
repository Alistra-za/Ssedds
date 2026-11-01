//! GUI tweaks G1, Note AO: the set header's input name box. A text box with gray Tab completion, path segment
//! selection and lists (Note FI17) and a ▾ list of known paths. Drawing only; the change is
//! `AppModel::set_input_name`.
use super::columns::{AMBER, RED};
use super::path_picker;
use crate::model::input_name::{
    bare_name, closest_first, completion, in_form_of, is_sep, live_rows, replace_segment,
    segment_at, segment_choices, segment_index, segment_start, snap_to_segments, tab_part, Choice,
};
use crate::model::known_paths::strip_dds;
use crate::model::sets::is_json_set;
use crate::model::sets::SetView;
use crate::model::AppModel;
use crate::pgp::prefix::stored_set_match;
use eframe::egui::{self, text::CCursor, text::CCursorRange, Event, Key, Modifiers};
use std::collections::HashSet;
use std::sync::Arc;

/// The box's placeholder (and how tests find it).
pub const HINT: &str = "Input name";
/// The button that opens the list of known paths.
pub const DROPDOWN: &str = "▾";
/// The list's heading.
pub const HEADING: &str = "Read this set's textures as";
/// Note DO: the list's "show every slot" toggle (TruePBR JSON sets only).
pub const ALL_SLOTS: &str = "All slots";
/// The box is never narrower than this.
const MIN_W: f32 = 160.0;
/// What the box needs besides its text: its margins, the ▾ and the spacing between them.
const CHROME: f32 = 44.0;

/// A padlock centred on `c`: a filled body under a shackle (the PGPatcher lock on a slot card).
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

/// The box's text edit id for set `set_id` (tests read its cursor through `TextEdit::load_state`).
pub fn edit_id(set_id: &str) -> egui::Id {
    egui::Id::new(("input-name", set_id)).with("edit")
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
    /// Note FI17: the segment list (Ctrl+Up/Down): the segment's index and its rows. While it shows it takes the live
    /// list's place, `sel` and the row clicks.
    seg: Option<(usize, Vec<Choice>)>,
    /// `rows` were the segment list's: a click there names a segment, never a path to commit.
    seg_rows: bool,
}

/// Note FI17: the hint while a separator is highlighted by a first Backspace / Delete.
fn separator_hint(text: &str, r: Option<CCursorRange>) -> Option<&'static str> {
    let r = r?;
    let (p, s) = (r.primary.index.0, r.secondary.index.0);
    let one_sep = p.abs_diff(s) == 1 && text.chars().nth(p.min(s)).is_some_and(is_sep);
    // A first Backspace leaves the head before the `/`, a first Delete after it.
    one_sep.then_some(if p < s {
        "Backspace again to remove the folder separator"
    } else {
        "Delete again to remove the folder separator"
    })
}

/// A cursor range from `anchor` (secondary) to `head` (primary), char indices.
fn range(anchor: usize, head: usize) -> CCursorRange {
    CCursorRange::two(CCursor::new(anchor), CCursor::new(head))
}

/// Note FI17 (rules 4, 5): Backspace / Delete on a `/`. With no selection, a first press highlights the `/` and a second
/// (the `/` now selected) deletes it; a held key (repeat events) stops at it. Removes the events egui must not see and
/// returns the new selection, if any. `r` is the box's selection before this frame's events.
// shortcut: decided against the selection before the frame's events, so a frame holding two presses (a second one
// reaching the `/` after egui ran the first) is not stopped; key repeat is 30+ ms apart, frames are shorter. Upgrade
// if a slow frame ever deletes a `/` under a held key.
fn separator_keys(ui: &egui::Ui, text: &str, r: CCursorRange) -> Option<CCursorRange> {
    let chars: Vec<char> = text.chars().collect();
    let (p, s) = (r.primary.index.0, r.secondary.index.0);
    let (lo, hi) = (p.min(s), p.max(s));
    let mut out = None;
    ui.input_mut(|i| {
        i.events.retain(|e| {
            let Event::Key {
                key: key @ (Key::Backspace | Key::Delete),
                pressed: true,
                repeat,
                modifiers,
                ..
            } = e
            else {
                return true;
            };
            if modifiers.ctrl || modifiers.alt || modifiers.command || modifiers.shift {
                return true;
            }
            if lo == hi {
                let at = if *key == Key::Backspace {
                    lo.checked_sub(1)
                } else {
                    Some(lo)
                };
                let Some(j) = at.filter(|j| chars.get(*j).is_some_and(|c| is_sep(*c))) else {
                    return true;
                };
                if !repeat {
                    out = Some(if *key == Key::Backspace {
                        range(j + 1, j)
                    } else {
                        range(j, j + 1)
                    });
                }
                false
            } else {
                // A held key never deletes a `/`, selected or not (the first press already removed a selection).
                !(*repeat && chars[lo..hi.min(chars.len())].iter().any(|c| is_sep(*c)))
            }
        })
    });
    out
}

/// The same input name: case, `\` and a `.dds` ending aside.
fn same(a: &str, b: &str) -> bool {
    fold_path(a) == fold_path(b)
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
    let anchor_id = id.with("drag-anchor");
    let full_id = id.with("menu-full");
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
    let font = egui::TextStyle::Body.resolve(ui.style());
    let ctx = ui.ctx().clone();
    let width = |s: &str| {
        ctx.fonts_mut(|f| f.layout_no_wrap(s.to_string(), font.clone(), egui::Color32::WHITE))
            .size()
            .x
    };
    // Note BH1: the box alone names the set (no heading beside it). With too little room left in the row it starts
    // the next one (decided here: the enabled scope below is one block). AO5.
    if ui.available_size_before_wrap().x < MIN_W + CHROME {
        ui.end_row();
    }
    // Note FI17 (rule 14): the known paths, folded, for the amber "no known texture" check (built once per list).
    // Keyed by the list itself (held, so its address is never reused while cached).
    let known: Arc<HashSet<String>> = {
        let kid = egui::Id::new(("input-name-known", salt));
        type Known = (Arc<Vec<String>>, Arc<HashSet<String>>);
        match ui.data(|d| d.get_temp::<Known>(kid)) {
            Some((pk, s)) if Arc::ptr_eq(&pk, &paths) => s,
            _ => {
                let s: Arc<HashSet<String>> =
                    Arc::new(paths.iter().map(|p| fold_path(p)).collect());
                ui.data_mut(|d| d.insert_temp::<Known>(kid, (paths.clone(), s.clone())));
                s
            }
        }
    };
    let unknown =
        |t: &str| !paths.is_empty() && !t.trim().is_empty() && !known.contains(&fold_path(t));
    let mut open_menu = false;
    ui.add_enabled_ui(why.is_none(), |ui| {
        let focused = ui.memory(|m| m.has_focus(edit_id));
        let live_id = id.with("live");
        let mut live = ui
            .data(|d| d.get_temp::<Live>(live_id))
            .unwrap_or_default();
        if !focused {
            live.seg = None;
        }
        let seg_shown = focused && live.seg.is_some();
        let shown = focused && !seg_shown && live.open && !live.rows.is_empty();
        let n_chars = text.chars().count();
        let state_range = || {
            egui::TextEdit::load_state(&ctx, edit_id)
                .and_then(|s| s.cursor.char_range())
                .unwrap_or(CCursorRange::one(CCursor::new(n_chars)))
        };
        // Set the box's selection (read by the box when it is drawn).
        let set_range = |r: CCursorRange| {
            let mut st = egui::TextEdit::load_state(&ctx, edit_id).unwrap_or_default();
            st.cursor.set_char_range(Some(r));
            st.store(&ctx, edit_id);
            ctx.request_repaint();
        };
        // Note FI17: open the list for segment `k` of `t`, the current value highlighted (rule 13: when that value is
        // not there, the closest names first and nothing highlighted).
        let open_segment = |live: &mut Live, t: &str, k: usize| {
            let old = t.split(['/', '\\']).nth(k).unwrap_or_default().to_string();
            let choices = segment_choices(&paths, t, k);
            let at = choices.iter().position(|c| fold_path(&c.value) == fold_path(&old));
            let choices = if at.is_none() { closest_first(choices, &old) } else { choices };
            live.sel = at;
            live.open = false;
            live.seg = Some((k, choices));
        };
        if focused {
            // Note FI17 (rule 6): Ctrl+Left / Ctrl+Right jump by segment; Shift extends.
            for (k, left) in [(Key::ArrowLeft, true), (Key::ArrowRight, false)] {
                // Ctrl+Shift first: a Ctrl pattern also matches with Shift held.
                let shift = ui.input_mut(|i| i.consume_key(Modifiers::CTRL | Modifiers::SHIFT, k));
                if shift || ui.input_mut(|i| i.consume_key(Modifiers::CTRL, k)) {
                    let r = state_range();
                    let c = r.primary.index.0;
                    let s = segment_at(&text, c);
                    let to = if left {
                        if c > s.start { s.start } else { c.checked_sub(1).map_or(0, |b| segment_at(&text, b).start) }
                    } else if c < s.end {
                        s.end
                    } else if c < n_chars {
                        segment_at(&text, c + 1).end
                    } else {
                        c
                    };
                    set_range(if shift { range(r.secondary.index.0, to) } else { range(to, to) });
                }
            }
            // Note FI17 (rules 9, 10): Ctrl+Up / Ctrl+Down list the segment under the cursor; with all selected, the ▾ list.
            let ctrl_ud = ui.input_mut(|i| {
                i.consume_key(Modifiers::CTRL, Key::ArrowDown) | i.consume_key(Modifiers::CTRL, Key::ArrowUp)
            });
            if ctrl_ud {
                let r = state_range();
                let (p, s) = (r.primary.index.0, r.secondary.index.0);
                if n_chars > 0 && p.min(s) == 0 && p.max(s) == n_chars {
                    open_menu = true;
                    live.open = false;
                } else {
                    open_segment(&mut live, &text, segment_index(&text, p));
                }
            }
            // Note FI17 (rules 4, 5).
            if let Some(r) = separator_keys(ui, &text, state_range()) {
                set_range(r);
            }
        }
        let seg_shown = focused && live.seg.is_some();
        // Note FI17: a pick in the segment list replaces only that segment; a middle folder opens the next list.
        let pick = |live: &mut Live, text: &mut String, c: &Choice| {
            let Some((k, _)) = live.seg.take() else { return };
            let (t, r) = replace_segment(text, k, &c.value);
            *text = t;
            let last = segment_index(text, text.chars().count()) == k;
            if last {
                live.sel = None;
            } else {
                open_segment(live, text, k + 1);
            }
            let end = if last { r.end } else { segment_at(text, r.end + 1).end };
            set_range(range(end, end));
        };
        if seg_shown {
            let n = live.seg.as_ref().map_or(0, |(_, c)| c.len());
            if n > 0 && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowDown)) {
                live.sel = Some(live.sel.map_or(0, |s| (s + 1).min(n - 1)));
            }
            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowUp)) {
                live.sel = live.sel.and_then(|s| s.checked_sub(1));
            }
            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                live.seg = None;
                live.sel = None;
            }
            // Enter picks the highlighted row; with none (rule 13), it closes the list and keeps the text. Taken here, so
            // the box keeps its focus and commits nothing.
            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
                let c = live.sel.and_then(|s| live.seg.as_ref()?.1.get(s).cloned());
                match c {
                    Some(c) => pick(&mut live, &mut text, &c),
                    None => {
                        live.seg = None;
                        live.sel = None;
                    }
                }
            }
        }
        let seg_shown = focused && live.seg.is_some();
        if focused && (shown || seg_shown) {
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
        let mut ghost = focused.then(|| completion(&paths, &text)).flatten();
        if ghost.is_some() {
            let mut state = egui::TextEdit::load_state(ui.ctx(), edit_id).unwrap_or_default();
            let at_end = state
                .cursor
                .char_range()
                .is_none_or(|r| r.primary.index.0 >= text.chars().count());
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
                text.push_str(&t);
                ghost = completion(&paths, &text);
                let end = CCursor::new(text.chars().count());
                state.cursor.set_char_range(Some(CCursorRange::one(end)));
                state.store(ui.ctx(), edit_id);
            }
        }
        let err = plan_error(ui, model, v, id, &text, &current);
        // The box is at least 160 pt and grows with its text and ghost, up to the room left in the row and 480 pt (AO5).
        // `room` leaves the margins and the ▾.
        let room = ui.available_size_before_wrap().x - CHROME;
        let want = width(&text) + ghost.as_deref().map_or(0.0, width) + 24.0;
        let box_w = want.min(room).clamp(MIN_W, 480.0);
        let mut commit = None;
        // Note FI17 (rule 14): the last segment of a name that is no known texture is amber.
        let mut layouter = |ui: &egui::Ui, b: &dyn egui::TextBuffer, _wrap: f32| {
            let t = b.as_str();
            let base = ui.visuals().widgets.inactive.text_color();
            let cut = if unknown(t) { segment_start(t) } else { t.len() };
            let mut job = egui::text::LayoutJob::default();
            job.append(&t[..cut], 0.0, egui::TextFormat::simple(font.clone(), base));
            if cut < t.len() {
                job.append(&t[cut..], 0.0, egui::TextFormat::simple(font.clone(), AMBER));
            }
            job.keep_trailing_whitespace = true;
            ui.fonts_mut(|f| f.layout_job(job))
        };
        let inner = egui::Frame::new()
            .stroke(ui.visuals().widgets.inactive.bg_stroke)
            .fill(ui.visuals().extreme_bg_color)
            .corner_radius(2.0)
            .inner_margin(egui::Margin::symmetric(4, 0))
            .show(ui, |ui| {
                egui::TextEdit::singleline(&mut text)
                    .id(edit_id)
                    .hint_text(HINT)
                    .frame(egui::Frame::NONE)
                    // As tall as the box was with BH13's lock track (2 + 2 pt).
                    .margin(egui::Margin { left: 4, right: 4, top: 2, bottom: 2 })
                    .layouter(&mut layouter)
                    // Tab stays in the box while a ghost shows; Esc while a list shows (AO2, AO4, FI17).
                    .event_filter(egui::EventFilter {
                        tab: ghost.is_some(),
                        escape: shown || seg_shown,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                    })
                    .desired_width(box_w)
                    .show(ui)
            });
        let out = inner.inner;
        let r = &out.response.response;
        let box_rect = inner.response.rect;
        ui.ctx().accesskit_node_builder(edit_id, |b| {
            b.set_label(HINT);
            if let Some(e) = &err {
                b.set_description(e.as_str());
            }
        });
        // Note FI17 (rule 1): a double-click selects the segment under the pointer (egui selected its word).
        if r.double_clicked() {
            if let Some(p) = r.interact_pointer_pos() {
                let c = out.galley.cursor_from_pos(p - out.galley_pos).index.0;
                let s = segment_at(&text, c);
                set_range(range(s.start, s.end));
            }
        }
        // Note FI17 (rule 2): a drag that crosses a `/` snaps to whole segments; Alt held keeps it per character. egui
        // moves only the head while dragging, so the anchor is kept from the drag's start.
        if r.drag_started() {
            let a = out.state.cursor.char_range().map(|c| c.secondary.index.0);
            ui.data_mut(|d| d.insert_temp(anchor_id, a));
        }
        if r.dragged() {
            let anchor = ui.data(|d| d.get_temp::<Option<usize>>(anchor_id)).flatten();
            if let (Some(a), Some(c)) = (anchor, out.state.cursor.char_range()) {
                let head = c.primary.index.0;
                let alt = ui.input(|i| i.modifiers.alt);
                let (a, h) = if alt { (a, head) } else { snap_to_segments(&text, a, head) };
                set_range(range(a, h));
            }
        }
        if r.drag_stopped() {
            ui.data_mut(|d| d.remove::<Option<usize>>(anchor_id));
        }
        // The ghost follows the text as edited this frame (review fix): drawn after the edit, not from last frame's.
        ghost = r.has_focus().then(|| completion(&paths, &text)).flatten();
        if let Some(g) = ghost.as_ref() {
            let at = out.galley_pos + egui::vec2(out.galley.rect.width(), 0.0);
            ui.painter().with_clip_rect(out.text_clip_rect).text(
                at,
                egui::Align2::LEFT_TOP,
                g,
                font.clone(),
                ui.visuals().weak_text_color(),
            );
        }
        if err.is_some() {
            ui.painter().rect_stroke(
                box_rect,
                2.0,
                egui::Stroke::new(1.5, RED),
                egui::StrokeKind::Outside,
            );
        }
        // The box's hover: why the name is refused, the separator hint (FI17 rule 8) and the unknown-texture note (rule 14).
        let hint = r
            .has_focus()
            .then(|| separator_hint(&text, out.state.cursor.char_range()))
            .flatten();
        let hover: Vec<String> = [
            err.clone(),
            hint.map(str::to_string),
            unknown(&text).then(|| format!("No known texture at {}", text.trim())),
        ]
        .into_iter()
        .flatten()
        .collect();
        if !hover.is_empty() {
            // One tooltip for the frame and the text edit inside it.
            inner.response.clone().union(r.clone()).on_hover_text(hover.join("\n"));
        }
        if r.has_focus() || r.changed() {
            ui.data_mut(|d| d.insert_temp(buf, (text.clone(), frame)));
        }
        // AO4: the live list follows the text while the box has focus; FI17: under the folder before the last segment.
        let rows = if r.has_focus() || r.lost_focus() {
            live_rows(&paths, &text, segment_start(&text), LIVE_ROWS)
        } else {
            vec![]
        };
        if r.changed() {
            // Typing closes the segment list and keeps the text (FI17 rule 13).
            live.seg = None;
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
        // Note FI17: a click on a segment row picks it (as Enter does): the box takes its focus back and commits nothing.
        let mut refocus = false;
        if live.seg.is_some() {
            if let Some(label) = live.clicked.take() {
                let c = live
                    .seg
                    .as_ref()
                    .and_then(|(_, cs)| cs.iter().find(|c| c.label == label).cloned());
                if let Some(c) = c {
                    pick(&mut live, &mut text, &c);
                    ui.data_mut(|d| d.insert_temp(buf, (text.clone(), frame)));
                    ui.memory_mut(|m| m.request_focus(edit_id));
                    refocus = true;
                }
            }
        }
        if r.lost_focus() && !refocus {
            ui.data_mut(|d| d.remove::<(String, u64)>(buf));
            // A pressed row, else the highlighted row on Enter, else the typed text (AO4). Esc reverts; an invalid or
            // unchanged value reverts; a valid change commits (one commit path, AO1).
            let cancelled = ui.input(|i| i.key_pressed(Key::Escape));
            // A segment row's label is a folder or texture name, not a path (review fix): it never commits here.
            let row = live.clicked.take().filter(|_| !live.seg_rows).or_else(|| {
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
        let has_focus = r.has_focus() || refocus;
        let seg_rows = live.seg.clone().filter(|_| has_focus);
        live.seg_rows = seg_rows.is_some();
        let live_open = has_focus && seg_rows.is_none() && live.open && !rows.is_empty();
        // The separator hint (FI17 rule 4) heads the list under the box, so it never adds a header row.
        if live_open || seg_rows.is_some() || hint.is_some() {
            egui::Area::new(live_id.with("area"))
                .fade_in(false)
                .order(egui::Order::Foreground)
                .fixed_pos(list_rect.min)
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(list_rect.width());
                        if let Some(h) = hint {
                            ui.weak(h);
                        }
                        if let Some((_, choices)) = &seg_rows {
                            let tail = text.split(['/', '\\']).skip(live.seg.as_ref().map_or(0, |s| s.0) + 1);
                            let tail = tail.collect::<Vec<_>>().join("/");
                            if choices.is_empty() {
                                ui.weak("No known names here");
                            }
                            for (i, c) in choices.iter().enumerate() {
                                // Rule 12: a folder that also holds the rest of the path.
                                let label = if c.keeps_tail {
                                    format!("{} ✓", c.label)
                                } else {
                                    c.label.clone()
                                };
                                let mut rr = ui.add(egui::Button::selectable(live.sel == Some(i), label));
                                if c.keeps_tail {
                                    let name = format!("{}, has {tail}", c.label);
                                    let selected = live.sel == Some(i);
                                    rr.widget_info(|| {
                                        egui::WidgetInfo::selected(
                                            egui::WidgetType::Button,
                                            true,
                                            selected,
                                            &name,
                                        )
                                    });
                                    rr = rr.on_hover_text(format!("Has {tail}"));
                                }
                                live.rows.push((c.label.clone(), rr.rect));
                            }
                        } else if live_open {
                            for (i, p) in rows.iter().enumerate() {
                                // The top row is the best match, the ghost's (AO4).
                                let text = egui::RichText::new(p);
                                let text = if i == 0 { text.strong() } else { text };
                                let rr = ui.add(egui::Button::selectable(live.sel == Some(i), text));
                                live.rows.push((p.clone(), rr.rect));
                            }
                        }
                    });
                });
        }
        ui.data_mut(|d| d.insert_temp(live_id, live.clone()));
        // Note DO: on a TruePBR JSON set, the slot the ▾ list picks for. Slot 1 names the set (the rename below);
        // slots 2–8 set that slot's match. Note FC12: it starts from the user's choice or the set's lowest unused
        // match, and only the user moves it (a pick from All slots goes to it).
        let json = matching && is_json_set(v, model.target);
        // Per project too: another folder's set with the same id starts from its own gui.toml value.
        let slot_id = id.with(("match-slot", model.root.as_deref()));
        let all_id = id.with(("all-slots", model.root.as_deref()));
        let mut slot: u8 = if json {
            ui.data(|d| d.get_temp(slot_id))
                .unwrap_or_else(|| model.match_slot_start(v))
        } else {
            1
        };
        let chosen = slot;
        let mut all = json && ui.data(|d| d.get_temp(all_id)).unwrap_or(false);
        let name = |n: u8| format!("Match {n} · {}", crate::model::sets::slot_name(true, n));
        // (The Match menu is TruePBR-only, so the PBR names.)
        if json {
            egui::ComboBox::from_id_salt(slot_id)
                // Short in the header row, so it does not wrap a narrow 3D view's header; the list names the slots.
                .selected_text(format!("Match {slot}"))
                .width(64.0)
                .show_ui(ui, |ui| {
                    for n in 1..=8 {
                        // The PGPatcher key rides on hover.
                        ui.selectable_value(&mut slot, n, name(n))
                            .on_hover_text(format!("match{n}"));
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
                // Only paths under the folder before the last segment (FI17, was the path lock); opened by Ctrl+Up/Down
                // on the whole text, every path (rule 10, ruling R2: a whole-name selection has no folder).
                let full = ui.data(|d| d.get_temp::<bool>(full_id)).unwrap_or(false);
                let cut = if full { 0 } else { segment_start(&text) };
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
                    // Note DO: the scan knows paths, just none under the folder (FI17: no path lock to move).
                    // Review fix: advice that works (the box reverts when ▾ takes its focus, so editing it first does not).
                    ui.weak(format!(
                        "No known paths under {pre}. In the box, Ctrl+Down on a folder lists the others; Ctrl+A then \
                         Ctrl+Down lists every path."
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
        // Note FI17 (rule 10): Ctrl+Up/Down with the whole text selected opens this list unfiltered; a ▾ click, filtered.
        let popup = egui::Popup::default_response_id(&menu.response);
        if open_menu {
            egui::Popup::open_id(ui.ctx(), popup);
            ui.data_mut(|d| d.insert_temp(full_id, true));
            ui.ctx().request_repaint();
        } else if !egui::Popup::is_id_open(ui.ctx(), popup) {
            ui.data_mut(|d| d.remove::<bool>(full_id));
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
}

/// A path folded for comparison: `/`, lower case, no `.dds`.
fn fold_path(p: &str) -> String {
    strip_dds(&p.trim().replace('\\', "/").to_lowercase()).to_string()
}
