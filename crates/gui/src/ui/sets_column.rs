//! The editable Sets column (M3b spec §3.1): set pills in project order with badges, drag and drop
//! of textures onto sets (and onto an empty area for a new set), drag-to-reorder by the ≡ handle, and a context
//! menu. Drawing only: every change is a model call, made after drawing.
use super::columns::{bulk_menu, member, status_text, AMBER, RED};
use super::slot_commands::SlotCmdState;
use super::thumbs::ThumbCache;
use crate::model::bulk::MenuAction;
use crate::model::pbr_export::export_blocks;
use crate::model::sets::{
    badge_text, fill_proposals, kind_text, shader_options, slot_row, DropChoice, DropOutcome,
    FillProposal, SetView, SlotFill, SlotKind,
};
use crate::model::AppModel;
use eframe::egui;
use engine::codes::Warning;
use engine::shader::{Shader, ShaderOrigin};
use std::collections::HashSet;

/// Drag payload: a texture, by item key.
#[derive(Debug, Clone)]
pub struct DragItem(pub String);
/// Drag payload: a set, by id (reordering).
#[derive(Debug, Clone)]
pub struct DragSet(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pending {
    Choice {
        item: String,
        set: String,
        from: String,
    },
    /// `choice` is what the replace answers: `drop_replace(item, set, choice)`.
    Replace {
        item: String,
        set: String,
        existing: String,
        choice: DropChoice,
    },
    Refused(String),
    /// Delete set asks first: the set and how many members it has (item 4).
    ConfirmDelete {
        id: String,
        members: usize,
    },
    /// The rename dialog: the set, the name being typed, and why the last try was refused.
    Rename {
        id: String,
        text: String,
        error: Option<String>,
    },
    /// Item 18: Fill from names, one tick per proposal (all ticked to start; a blocked row stays unticked).
    FillNames {
        set: String,
        rows: Vec<FillProposal>,
        ticks: Vec<bool>,
    },
}

/// Item 17: files dropped from Explorer this frame, and where (`None` when the position is unknown).
#[derive(Debug, Clone)]
pub struct FileDrop {
    pub paths: Vec<std::path::PathBuf>,
    pub pos: Option<egui::Pos2>,
}

/// Item 17: the line the set editor shows under the slot cards after a file drop: set id, text, is an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotNote {
    pub set: String,
    pub text: String,
    pub error: bool,
}

#[derive(Debug, Default)]
pub struct DropState {
    pub pending: Option<Pending>,
    /// The set that was just reordered and when (egui time): its border flashes for `FLASH_SECS`.
    pub flash: Option<(String, f64)>,
    /// Item 17: a file drop not yet taken by a slot card this frame (`ui::draw_with_scan` opens its folder after).
    pub file: Option<FileDrop>,
    pub slot_note: Option<SlotNote>,
}

/// How long the moved pill flashes after a reorder.
const FLASH_SECS: f64 = 1.0;
/// The warning tooltip shows this many lines, then "… and K more".
const TIP_LINES: usize = 10;

enum Act {
    Select(String),
    /// The warning badge: open Problems filtered to this set key.
    Problems(String),
    SelectItem(String),
    Expand(String),
    Drop(String, String),
    NewSet(String),
    Reorder(String, usize),
    OpenRename(String),
    Delete(String, usize),
    /// Item 18: open Fill from names on a set.
    FillNames(String),
    RemoveMember(String, String),
    /// The Shader dropdown: `None` is Auto.
    Shader(String, Option<Shader>),
    /// A pill click with its modifiers (F3a D1).
    Click(String, bool, bool),
    RightClick(String),
    Bulk(MenuAction),
    /// F3a Task 8: a member's slot menu pick.
    Slot(MenuAction),
}

/// Below this inner card width the pill and the ⚠ button go under the name, so the header row never holds the
/// Sets column open past `SETS_MIN`.
const WIDE_CARD: f32 = 150.0;
/// The size of a member thumbnail (`columns::member`), which an empty chip matches.
const CHIP: f32 = 32.0;

/// A chip's letter (A3): the slot's suffix in capitals, "D" for the diffuse.
fn chip_letter(k: SlotKind) -> String {
    match k.suffix() {
        "" => "D".to_string(),
        s => s.to_uppercase(),
    }
}

/// An empty slot (or a missing entry, outlined): a dim box with the letter. K11: named by the letter.
fn letter_box(ui: &mut egui::Ui, letter: &str, outline: Option<egui::Color32>) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(egui::Vec2::splat(CHIP), egui::Sense::hover());
    let vis = ui.visuals();
    let stroke = match outline {
        Some(c) => egui::Stroke::new(2.0, c),
        None => vis.widgets.noninteractive.bg_stroke,
    };
    let p = ui.painter();
    p.rect_filled(rect, 2.0, vis.extreme_bg_color);
    p.rect_stroke(rect, 2.0, stroke, egui::StrokeKind::Inside);
    p.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        letter,
        egui::TextStyle::Small.resolve(ui.style()),
        vis.weak_text_color(),
    );
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, letter));
    r
}

/// The letter in a filled chip's bottom-left corner, on a dark backing so it reads over any thumbnail.
fn paint_letter(ui: &egui::Ui, rect: egui::Rect, letter: &str) {
    let font = egui::TextStyle::Small.resolve(ui.style());
    let g = ui
        .painter()
        .layout_no_wrap(letter.to_string(), font, egui::Color32::WHITE);
    let at = rect.left_bottom() + egui::vec2(1.0, -1.0 - g.size().y);
    ui.painter().rect_filled(
        egui::Rect::from_min_size(at, g.size()).expand(1.0),
        2.0,
        egui::Color32::from_black_alpha(170),
    );
    ui.painter().galley(at, g, egui::Color32::WHITE);
}

/// The pill's hover (R15): the long badge, then one line each for blocked, owns, missing entries and recipes.
fn pill_hover(model: &AppModel, v: &SetView, blocked: bool) -> String {
    let mut lines = vec![badge_text(model, v).to_string()];
    if blocked {
        lines.push("blocked".to_string());
    }
    lines.extend(v.owns.iter().map(|s| format!("owns {}", s.label())));
    lines.extend(v.missing.iter().map(|m| format!("no file matches {m}")));
    for r in &v.recipes {
        let mut l = format!("{}: {}", r.subtype.id(), status_text(r.status).0);
        if let Some(reason) = &r.reason {
            l.push_str(&format!(" ({reason})"));
        }
        lines.push(l);
    }
    lines.join("\n")
}

/// Task 1.6's "⚠ n" button: the set's and its members' warnings on hover; a click opens Problems on the set.
fn warn_button(ui: &mut egui::Ui, v: &SetView, rows: &[(&str, &Warning)], acts: &mut Vec<Act>) {
    if rows.is_empty() {
        return;
    }
    let warn = ui
        .add(egui::Button::new(
            egui::RichText::new(format!("⚠ {}", rows.len()))
                .small()
                .color(AMBER),
        ))
        .on_hover_text(warning_tooltip(rows));
    if warn.clicked() {
        acts.push(Act::Problems(v.key.clone()));
    }
}

/// The status pill: the status dot and the short kind (cut on a narrow card), in a rounded frame, the long text on
/// hover.
fn status_pill(ui: &mut egui::Ui, model: &AppModel, v: &SetView, blocked: bool) {
    use super::status::{dot, Status};
    let s = if blocked {
        Status::Blocks
    } else if !crate::model::bottom::set_ready(model, v) {
        Status::Warning
    } else {
        Status::Ready
    };
    egui::Frame::new()
        .corner_radius(8.0)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .inner_margin(egui::Margin::symmetric(6, 1))
        .show(ui, |ui| {
            let kind =
                egui::Label::new(egui::RichText::new(kind_text(model, v)).small()).truncate();
            // In the card row's right-to-left layout the first widget lands rightmost: the dot stays left.
            if ui.layout().prefer_right_to_left() {
                ui.add(kind);
                dot(ui, s);
            } else {
                dot(ui, s);
                ui.add(kind);
            }
        })
        .response
        .on_hover_text(pill_hover(model, v, blocked));
}

/// A member's thumbnail on the card with today's behaviours: click selects, double-click expands, a slot member has
/// the slot menu, and `drag` offers it for dragging; `note` is a line added to its hover. `None` when the item is
/// not in the model.
#[allow(clippy::too_many_arguments)]
fn member_chip(
    ui: &mut egui::Ui,
    model: &AppModel,
    thumbs: &mut ThumbCache,
    v: &SetView,
    mk: &str,
    kind: Option<SlotKind>,
    drag: bool,
    note: Option<&str>,
    sel_items: &HashSet<String>,
    acts: &mut Vec<Act>,
    member_menu: &mut bool,
) -> Option<egui::Response> {
    let item = model.item(mk)?;
    let locked = kind.is_some_and(|k| v.locks.contains(&k.slot_number()));
    let pgp = kind.is_some_and(|k| crate::pgp::prefix::pgp_locked(&v.settings, k.slot_number()));
    let r = member(ui, item, sel_items.contains(mk), locked, pgp, thumbs, note);
    if let Some(k) = kind {
        *member_menu |= r.secondary_clicked();
        let open = r.context_menu(|ui| {
            let title = format!("Slot {}: {}", k.slot_number(), k.label());
            if let Some(a) = bulk_menu(ui, &title, &model.slot_menu(&v.id, k)) {
                acts.push(Act::Slot(a));
            }
        });
        *member_menu |= open.is_some();
    }
    if drag {
        r.dnd_set_drag_payload(DragItem(mk.to_string()));
    }
    if r.double_clicked() {
        acts.push(Act::Expand(mk.to_string()));
    } else if r.clicked() {
        acts.push(Act::SelectItem(mk.to_string()));
    }
    Some(r)
}

/// The warning tooltip: `<file>: <code> <message>` per warning, at most `TIP_LINES`, then "… and K more".
fn warning_tooltip(rows: &[(&str, &Warning)]) -> String {
    let mut lines: Vec<String> = rows
        .iter()
        .take(TIP_LINES)
        .map(|(file, w)| format!("{file}: {} {}", w.code.as_str(), w.message))
        .collect();
    if rows.len() > TIP_LINES {
        lines.push(format!("… and {} more", rows.len() - TIP_LINES));
    }
    lines.join("\n")
}

/// Whether the sets document can be changed (P8), and the banner's reason when it cannot.
struct Editing<'a> {
    on: bool,
    why: &'a str,
    /// Editable and no filter on: sets can be reordered (R17).
    reorder: bool,
}

/// Why the ≡ handle and Move up / Move down are off while the Sets filter is on.
const CLEAR_TO_REORDER: &str = "Clear the filter to reorder sets";

/// R17: the search box, the pills with their counts and the shader popup, under the heading.
fn filter_rows(ui: &mut egui::Ui, model: &mut AppModel) {
    ui.add(
        egui::TextEdit::singleline(&mut model.sets_filter.search)
            .hint_text("Search sets")
            .desired_width(f32::INFINITY),
    );
    let c = model.set_counts();
    ui.horizontal_wrapped(|ui| {
        let f = &mut model.sets_filter;
        for (on, text, hover) in [
            (
                &mut f.attention,
                format!("Needs attention {}", c.attention),
                "Only sets that are not ready: a problem in the set or one of its textures",
            ),
            (
                &mut f.ready,
                format!("Ready {}", c.ready),
                "Only sets with no problem",
            ),
            (
                &mut f.no_json,
                format!("No JSON {}", c.no_json),
                "Only sets that write no JSON",
            ),
        ] {
            if ui
                .add(egui::Button::selectable(*on, text).corner_radius(10))
                .on_hover_text(hover)
                .clicked()
            {
                *on = !*on;
            }
        }
        let text = if f.shader.is_none() {
            "Filter ▾"
        } else {
            "Filter (1) ▾"
        };
        // A9: a saved shader the Mode does not offer is listed too, so it is never invisible.
        let mut shaders = shader_options(model.target);
        if let Some(s) = f.shader.filter(|s| !shaders.contains(s)) {
            shaders.push(s);
        }
        crate::ui::path_picker::menu(ui, text, |ui| {
            if ui.selectable_label(f.shader.is_none(), "Any").clicked() {
                f.shader = None;
                ui.close();
            }
            for s in shaders {
                if ui
                    .selectable_label(f.shader == Some(s), s.label())
                    .clicked()
                {
                    f.shader = Some(s);
                    ui.close();
                }
            }
        })
        .response
        .on_hover_text("Only sets with this shader");
    });
}

pub fn draw(
    ui: &mut egui::Ui,
    model: &mut AppModel,
    thumbs: &mut ThumbCache,
    st: &mut DropState,
    cmds: &mut SlotCmdState,
) {
    ui.horizontal_wrapped(|ui| {
        super::columns::collapse_arrow(ui, model, crate::ui::Column::Sets);
        ui.heading("Sets");
        if let Some(s) = model.selection_summary(true) {
            ui.label(egui::RichText::new(s).strong());
        }
    });
    filter_rows(ui, model);
    ui.separator();
    // R17: while a filter is on, the handle, Move up / Move down and the insertion line are off (K19).
    let filtering = model.sets_filter_active();
    // One set per frame keeps the per-pill and per-member tests O(1) however many are selected.
    let sel_sets: HashSet<String> = model.selected_sets().into_iter().collect();
    let sel_items: HashSet<String> = model.selected_items().into_iter().collect();
    let editable = model.sets_doc.as_ref().is_some_and(|d| !d.read_only());
    let why = model.banner.clone().unwrap_or_default();
    let editing = Editing {
        on: editable,
        why: &why,
        reorder: editable && !filtering,
    };
    let views = crate::model::sets::views(model);
    let visible = model.visible_sets();
    if views.is_empty() && model.root.is_some() && !model.busy() {
        ui.weak("No sets.");
    } else if visible.is_empty() && filtering && !views.is_empty() {
        ui.label(format!("No sets match the filter ({} hidden)", views.len()));
        if ui.button("Clear filter").clicked() {
            model.clear_sets_filter();
        }
    }
    let blocked: std::collections::HashSet<String> =
        export_blocks(model).into_iter().map(|i| i.set_id).collect();
    let mut acts = vec![];
    let (mut released, mut hovered) = (None, None);
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    // The set being dragged (a peek: the payload stays for the reorder).
    let dragged: Option<String> = egui::DragAndDrop::payload::<DragSet>(&ctx).map(|p| p.0.clone());
    let dragged_index = dragged
        .as_deref()
        .and_then(|d| views.iter().position(|v| v.id == d));
    if st
        .flash
        .as_ref()
        .is_some_and(|(_, t)| now - t >= FLASH_SECS)
    {
        st.flash = None;
    }
    if st.flash.is_some() {
        ctx.request_repaint();
    }
    let flash = st
        .flash
        .as_ref()
        .map(|(id, t)| (id.as_str(), (now - t) / FLASH_SECS));
    let mut rects = vec![];
    // A set the palette picked scrolls into view (item 19).
    let reveal = model.nav.reveal_set.take();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // `index` is the set's place among all sets: the reorder and Move up / Move down use it (K19).
            for &index in visible.iter() {
                let v = &views[index];
                let is_blocked = blocked.contains(&v.id);
                let look = PillLook {
                    dragging: dragged.as_deref() == Some(v.id.as_str()),
                    flash: flash.filter(|(id, _)| *id == v.id).map(|(_, f)| f),
                    selected: !v.key.is_empty() && sel_sets.contains(&v.key),
                    bulk: sel_sets.len() >= 2,
                };
                let out = pill(
                    ui, model, thumbs, v, index, &editing, is_blocked, look, &sel_items, &mut acts,
                );
                if reveal
                    .as_deref()
                    .is_some_and(|k| !v.key.is_empty() && k == v.key)
                {
                    ui.scroll_to_rect(out.rect, None);
                }
                rects.push(out.rect);
                if out.handle_released {
                    released = Some(v.id.clone());
                }
                if out.hovered_by_set {
                    hovered = Some(index);
                }
            }
            if let (Some(from), Some(to)) = (dragged_index, hovered) {
                if let Some((r, y)) = rects
                    .get(to)
                    .and_then(|r| insertion_y(from, to, *r).map(|y| (*r, y)))
                {
                    let accent = ui.visuals().selection.stroke.color;
                    ui.painter().line_segment(
                        [egui::pos2(r.left(), y), egui::pos2(r.right(), y)],
                        egui::Stroke::new(3.0, accent),
                    );
                }
            }
            if model.root.is_some() {
                new_set_area(ui, &editing, &mut acts);
            }
        });
    let ids: Vec<&str> = views.iter().map(|v| v.id.as_str()).collect();
    if editing.reorder {
        if let Some((id, to)) = reorder_target(released.as_deref(), hovered, &ids) {
            acts.push(Act::Reorder(id, to));
        }
    }
    for a in acts {
        match a {
            Act::Select(k) => model.select_set(&k),
            Act::SelectItem(k) => model.select_item(&k),
            Act::Click(k, ctrl, shift) => model.click_set(&k, ctrl, shift),
            Act::RightClick(k) => model.right_click_set(&k),
            Act::Bulk(a) => model.menu_picked(a),
            Act::Problems(k) => {
                model.open_problems(Some(crate::model::bottom::ProblemsFilter::Set(k)))
            }
            Act::Slot(a) => super::slot_commands::run(model, cmds, a),
            Act::Expand(k) => model.expand(&k),
            Act::Drop(item, set) => match model.drop_on_set(&item, &set) {
                DropOutcome::Joined => {}
                DropOutcome::AskChoice { from } => {
                    st.pending = Some(Pending::Choice { item, set, from })
                }
                DropOutcome::AskReplace { existing } => {
                    st.pending = Some(Pending::Replace {
                        item,
                        set,
                        existing,
                        choice: DropChoice::Move,
                    })
                }
                DropOutcome::Refused(why) => st.pending = Some(Pending::Refused(why)),
            },
            Act::NewSet(item) => {
                // Nothing was done (a packed map, a locked slot): say why.
                if let Err(why) = model.drop_new_set(&item) {
                    st.pending = Some(Pending::Refused(why));
                }
            }
            Act::Reorder(id, to) => {
                if model.reorder_set(&id, to) {
                    st.flash = Some((id, now));
                    ctx.request_repaint();
                }
            }
            Act::OpenRename(id) => {
                st.pending = Some(Pending::Rename {
                    text: id.clone(),
                    id,
                    error: None,
                })
            }
            Act::Delete(id, members) => {
                st.pending = Some(Pending::ConfirmDelete { id, members });
            }
            Act::FillNames(set) => {
                let rows = fill_proposals(model, &set);
                st.pending = Some(if rows.is_empty() {
                    Pending::Refused(format!(
                        "No other file in {set}'s folder has its base name and a suffix Tessalith knows."
                    ))
                } else {
                    Pending::FillNames {
                        set,
                        ticks: rows.iter().map(|r| r.blocked.is_none()).collect(),
                        rows,
                    }
                });
            }
            Act::RemoveMember(id, item) => {
                if let Err(why) = model.remove_member(&id, &item) {
                    st.pending = Some(Pending::Refused(why));
                }
            }
            Act::Shader(id, shader) => model.set_shader(&id, shader),
        }
    }
}

/// The dashed area at the bottom: a texture dropped here starts a new set.
fn new_set_area(ui: &mut egui::Ui, editing: &Editing, acts: &mut Vec<Act>) {
    let (zone, payload) = item_zone(
        ui,
        editing.on,
        egui::Frame::group(ui.style()).corner_radius(12.0),
        |ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new("Drop here to start a new set").color(AMBER));
        },
    );
    if !editing.on {
        zone.on_hover_text(editing.why);
    }
    if let Some(p) = payload {
        if editing.on {
            acts.push(Act::NewSet(p.0.clone()));
        }
    }
}

/// A frame that takes dropped textures. `egui::Ui::dnd_drop_zone` takes whatever payload is in flight when it
/// is released (even one of another type), so while a set is dragged for reordering, or when editing is off, a
/// plain frame is drawn instead and the set payload survives for the reorder.
fn item_zone(
    ui: &mut egui::Ui,
    editing: bool,
    frame: egui::Frame,
    add: impl FnOnce(&mut egui::Ui),
) -> (egui::Response, Option<std::sync::Arc<DragItem>>) {
    if editing && !egui::DragAndDrop::has_payload_of_type::<DragSet>(ui.ctx()) {
        let (zone, p) = ui.dnd_drop_zone::<DragItem, ()>(frame, add);
        (zone.response, p)
    } else {
        (frame.show(ui, add).response, None)
    }
}

/// Where a dragged set goes: `released` is the set whose handle was let go this frame, `hovered` the index of
/// the pill under the pointer. Nothing happens when there is no target or the set is dropped on itself.
fn reorder_target(
    released: Option<&str>,
    hovered: Option<usize>,
    ids: &[&str],
) -> Option<(String, usize)> {
    let (id, to) = (released?, hovered?);
    (*ids.get(to)? != id).then(|| (id.to_string(), to))
}

/// What a pill reports back: its handle was let go, and a dragged set is over it.
struct PillOut {
    handle_released: bool,
    hovered_by_set: bool,
    rect: egui::Rect,
}

/// How a pill is drawn this frame: held by the pointer, or just moved (progress 0..1 through the flash).
#[derive(Clone, Copy)]
struct PillLook {
    dragging: bool,
    flash: Option<f64>,
    selected: bool,
    /// Two or more sets are selected: the pills of selected ones show the bulk menu.
    bulk: bool,
}

/// The y of the insertion line (None over the dragged pill itself, where nothing moves) when set `from` is dragged over the pill at `to` (whose rect is `r`): moving
/// down lands after that pill, moving up before it (the same rule as `reorder_set`).
fn insertion_y(from: usize, to: usize, r: egui::Rect) -> Option<f32> {
    match to.cmp(&from) {
        std::cmp::Ordering::Greater => Some(r.bottom()),
        std::cmp::Ordering::Less => Some(r.top()),
        std::cmp::Ordering::Equal => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn pill(
    ui: &mut egui::Ui,
    model: &AppModel,
    thumbs: &mut ThumbCache,
    v: &SetView,
    index: usize,
    editing: &Editing,
    blocked: bool,
    look: PillLook,
    sel_items: &HashSet<String>,
    acts: &mut Vec<Act>,
) -> PillOut {
    let mut handle_released = false;
    // A member's slot menu, not the pill's, answers a right-click on that member.
    let mut member_menu = false;
    let selected = look.selected;
    let accent = ui.visuals().selection.stroke.color;
    let stroke = if look.dragging {
        egui::Stroke::new(2.0, accent)
    } else if let Some(f) = look.flash {
        egui::Stroke::new(2.0, accent.gamma_multiply((1.0 - f as f32).clamp(0.0, 1.0)))
    } else if selected {
        ui.visuals().selection.stroke
    } else {
        ui.visuals().widgets.noninteractive.bg_stroke
    };
    // A5: the open set and the selected sets are filled with the selection colour (`selected_sets` holds the open one).
    let fill = if selected {
        ui.visuals().selection.bg_fill.gamma_multiply(0.35)
    } else {
        egui::Color32::TRANSPARENT
    };
    // The drop zone repaints its own frame's fill and stroke, so the card's look is an inner frame.
    let card = egui::Frame::group(ui.style())
        .corner_radius(12.0)
        .stroke(stroke)
        .fill(fill);
    let scoped = ui.scope_builder(egui::UiBuilder::new().sense(egui::Sense::click()), |ui| {
        if look.dragging {
            ui.set_opacity(0.5);
        }
        let (_zone, dropped) = item_zone(
            ui,
            editing.on,
            egui::Frame::NONE.corner_radius(12.0),
            |ui| {
                card.show(ui, |ui| {
                    ui.style_mut().interaction.selectable_labels = false;
                    ui.set_width(ui.available_width());
                    let rows: Vec<(&str, &Warning)> = v
                        .members
                        .iter()
                        .filter_map(|k| model.item(k))
                        .flat_map(|i| {
                            i.warnings
                                .iter()
                                .filter(|w| super::counts_toward_badge(w))
                                .map(move |w| (i.name.as_str(), w))
                        })
                        .chain(
                            v.set_warnings
                                .iter()
                                .filter(|w| super::counts_toward_badge(w))
                                .map(|w| (v.label.as_str(), w)),
                        )
                        .collect();
                    let wide = ui.available_width() >= WIDE_CARD;
                    let name = |ui: &mut egui::Ui| {
                        ui.add(egui::Label::new(egui::RichText::new(&v.label).strong()).truncate());
                    };
                    ui.horizontal(|ui| {
                        let sense = if editing.reorder {
                            egui::Sense::drag()
                        } else {
                            egui::Sense::hover()
                        };
                        let handle = ui.add(egui::Label::new("≡").sense(sense));
                        if editing.reorder {
                            handle.dnd_set_drag_payload(DragSet(v.id.clone()));
                            handle_released = handle.drag_stopped();
                        } else if editing.on {
                            handle.on_hover_text(CLEAR_TO_REORDER);
                        }
                        if wide {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    warn_button(ui, v, &rows, acts);
                                    status_pill(ui, model, v, blocked);
                                    ui.with_layout(
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        name,
                                    );
                                },
                            );
                        } else {
                            name(ui);
                        }
                    });
                    if !wide {
                        ui.horizontal_wrapped(|ui| {
                            status_pill(ui, model, v, blocked);
                            warn_button(ui, v, &rows, acts);
                        });
                    }
                    if !v.key.is_empty() {
                        shader_combo(ui, model, v, editing, acts);
                    }
                    if v.key.is_empty() && v.created && v.missing.is_empty() {
                        ui.weak("Drop a texture here to start");
                    }
                    ui.horizontal_wrapped(|ui| {
                        let row = slot_row(v.shader.as_ref(), &v.slots);
                        let mut chipped: HashSet<&str> = HashSet::new();
                        for cell in &row {
                            let k = cell.kind;
                            let letter = chip_letter(k);
                            let (mk, from) = match &cell.fill {
                                SlotFill::Own { item } => (item, None),
                                SlotFill::Borrowed { from, item } => (item, Some(from)),
                                SlotFill::Empty => {
                                    letter_box(ui, &letter, None)
                                        .on_hover_text(format!("{} (empty)", k.label()));
                                    continue;
                                }
                            };
                            chipped.insert(mk);
                            let note = from.map(|f| format!("borrowed from {f}"));
                            // K13: a borrowed item belongs to another set: no drag payload from here.
                            let Some(r) = member_chip(
                                ui,
                                model,
                                thumbs,
                                v,
                                mk,
                                Some(k),
                                from.is_none(),
                                note.as_deref(),
                                sel_items,
                                acts,
                                &mut member_menu,
                            ) else {
                                letter_box(ui, &letter, None);
                                continue;
                            };
                            paint_letter(ui, r.rect, &letter);
                            if from.is_some() {
                                ui.painter().rect_stroke(
                                    r.rect,
                                    2.0,
                                    egui::Stroke::new(2.0, AMBER),
                                    egui::StrokeKind::Inside,
                                );
                            }
                            r.widget_info(|| {
                                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &letter)
                            });
                        }
                        // K12: an add/remove entry that matches no file.
                        for m in &v.missing {
                            letter_box(ui, "?", Some(RED))
                                .on_hover_text(format!("no file matches {m}"));
                        }
                        // Members in no chip (an env mask `_m`, a clashing second kind) follow as plain thumbnails.
                        for mk in &v.members {
                            if chipped.contains(mk.as_str()) {
                                continue;
                            }
                            // F3a Task 8: the slot the member fills in this set, if any, has a slot menu.
                            let kind = v
                                .slots
                                .iter()
                                .find(|(_, f)| matches!(f, SlotFill::Own { item } if item == mk))
                                .map(|(k, _)| *k);
                            member_chip(
                                ui,
                                model,
                                thumbs,
                                v,
                                mk,
                                kind,
                                true,
                                None,
                                sel_items,
                                acts,
                                &mut member_menu,
                            );
                        }
                    });
                });
            },
        );
        dropped
    });
    let dropped = scoped.inner;
    let resp = scoped.response;
    if !editing.on {
        resp.clone().on_hover_text(editing.why);
    }
    if resp.clicked() && !v.key.is_empty() && acts.is_empty() {
        let m = ui.input(|i| i.modifiers);
        acts.push(Act::Click(v.key.clone(), m.command, m.shift));
    }
    if let Some(p) = dropped {
        if editing.on {
            acts.push(Act::Drop(p.0.clone(), v.id.clone()));
        }
    }
    // Not consuming: the handle's release (in `draw`) applies the move.
    let hovered_by_set = editing.reorder && resp.dnd_hover_payload::<DragSet>().is_some();
    // F3a D4: a right-click on an unselected pill selects it alone first; among two or more selected sets the pill
    // shows the bulk menu, otherwise its own.
    if resp.secondary_clicked() && !v.key.is_empty() && !member_menu {
        acts.push(Act::RightClick(v.key.clone()));
    }
    let bulk = look.selected && look.bulk;
    if !member_menu {
        resp.context_menu(|ui| {
            if bulk {
                if let Some(a) = bulk_menu(ui, &model.menu_title(true), &model.set_menu()) {
                    acts.push(Act::Bulk(a));
                }
            } else {
                menu(ui, v, index, editing, model, acts)
            }
        });
    }
    PillOut {
        handle_released,
        hovered_by_set,
        rect: resp.rect,
    }
}

/// The set's Shader dropdown (M6 spec §7): the engine's pick with its provenance, then "Auto" and the mode's
/// shaders. Disabled when sets are not editable.
fn shader_combo(
    ui: &mut egui::Ui,
    model: &AppModel,
    v: &SetView,
    editing: &Editing,
    acts: &mut Vec<Act>,
) {
    let text = v.shader.as_ref().map_or(String::new(), |p| {
        let tag = match p.provenance {
            ShaderOrigin::Set => "",
            ShaderOrigin::Auto(_) => " (auto)",
            ShaderOrigin::Fallback => " (fallback)",
        };
        format!("{}{tag}", p.shader.label())
    });
    // A5: its own row, the card's width (a ComboBox never wraps, so it is never wider than the row).
    ui.add_enabled_ui(editing.on, |ui| {
        egui::ComboBox::from_id_salt(("shader", &v.id))
            .width(ui.available_width())
            .truncate()
            .selected_text(text)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(v.shader_choice.is_none(), "Auto")
                    .clicked()
                {
                    acts.push(Act::Shader(v.id.clone(), None));
                }
                for s in shader_options(model.target) {
                    if ui
                        .selectable_label(v.shader_choice == Some(s), s.label())
                        .clicked()
                    {
                        acts.push(Act::Shader(v.id.clone(), Some(s)));
                    }
                }
            });
    });
}

/// The Rename submenu of a set's menu: label, one-line hover text, and what it does. F3b D5 for the last two.
fn rename_entries(set: &str) -> [(String, &'static str, Act); 3] {
    [
        (
            "Rename…".to_string(),
            "Change the set's name only",
            Act::OpenRename(set.to_string()),
        ),
        (
            format!("Rename outputs…  {}", super::actions::keys_text("rename")),
            "Change the names of the files this set writes",
            Act::Bulk(MenuAction::RenameOutputs),
        ),
        (
            "Rename inputs…".to_string(),
            "Read the source files under other names; the files on disk keep theirs",
            Act::Bulk(MenuAction::RenameInputs),
        ),
    ]
}

fn menu(
    ui: &mut egui::Ui,
    v: &SetView,
    index: usize,
    editing: &Editing,
    model: &AppModel,
    acts: &mut Vec<Act>,
) {
    // First: it only reads, so it works while sets are not editable (the disable below cannot be undone).
    ui.menu_button("Show in Explorer", |ui| {
        for mk in &v.members {
            let name = model.item(mk).map_or(mk.as_str(), |i| i.name.as_str());
            let off = model.reveal_problem(mk);
            let r = ui.add_enabled(off.is_none(), egui::Button::new(name));
            let r = match &off {
                Some(why) => r.on_disabled_hover_text(why),
                None => r,
            };
            if r.clicked() {
                acts.push(Act::Bulk(MenuAction::ShowInExplorer(mk.clone())));
                ui.close();
            }
        }
    });
    if !editing.on {
        ui.disable();
    }
    let tip = |r: egui::Response| {
        if editing.on {
            r
        } else {
            r.on_disabled_hover_text(editing.why)
        }
    };
    // Item 23: one "Rename" submenu; each entry says what it renames. F2 stays on Rename outputs.
    let rename = ui.menu_button("Rename", |ui| {
        for (label, hover, act) in rename_entries(&v.id) {
            if tip(ui.button(label).on_hover_text(hover)).clicked() {
                acts.push(act);
                ui.close();
            }
        }
    });
    tip(rename.response);
    if tip(ui.button("Edit match…")).clicked() && !v.key.is_empty() {
        acts.push(Act::Select(v.key.clone()));
        ui.close();
    }
    let remove = ui.menu_button("Remove member", |ui| {
        for mk in &v.members {
            let name = model.item(mk).map_or(mk.as_str(), |i| i.name.as_str());
            if ui.button(name).clicked() {
                acts.push(Act::RemoveMember(v.id.clone(), mk.clone()));
                ui.close();
            }
        }
    });
    tip(remove.response);
    let del = ui.add_enabled(
        v.created || v.key.is_empty(),
        egui::Button::new("Delete set"),
    );
    let del = del.on_disabled_hover_text(if editing.on {
        "Only sets you created can be deleted; an automatic set comes back from the file names"
    } else {
        editing.why
    });
    if del.clicked() {
        acts.push(Act::Delete(v.id.clone(), v.members.len()));
        ui.close();
    }
    // K19: `index` is among all sets; while a filter is on neither moves.
    let move_button = |ui: &mut egui::Ui, label: &str| {
        let r = ui.add_enabled(editing.reorder || !editing.on, egui::Button::new(label));
        if editing.on && !editing.reorder {
            r.on_disabled_hover_text(CLEAR_TO_REORDER)
        } else {
            tip(r)
        }
    };
    if move_button(ui, "Move up").clicked() && index > 0 {
        acts.push(Act::Reorder(v.id.clone(), index - 1));
        ui.close();
    }
    if move_button(ui, "Move down").clicked() {
        acts.push(Act::Reorder(v.id.clone(), index + 1));
        ui.close();
    }
    let fill = ui.button("Fill from names…").on_hover_text(
        "Move in the files of this folder named like the set with a known suffix (_n, _p, _rmaos, …)",
    );
    if tip(fill).clicked() {
        acts.push(Act::FillNames(v.id.clone()));
        ui.close();
    }
}

/// What a button in the drop modal asked for; applied after the modal is drawn (the modal only reads the model).
enum ModalAnswer {
    Choice(DropChoice),
    Replace,
    Delete,
    Close,
}

/// The drop, delete and rename dialogs. Drawn by `ui::draw_with_scan` whether or not this column shows: a file
/// dropped on a slot card asks its Replace here too (item 17).
pub fn pending_modal(ctx: &egui::Context, model: &mut AppModel, st: &mut DropState) {
    if matches!(st.pending, Some(Pending::Rename { .. })) {
        rename_modal(ctx, model, st);
        return;
    }
    if matches!(st.pending, Some(Pending::FillNames { .. })) {
        fill_modal(ctx, model, st);
        return;
    }
    let Some(p) = st.pending.clone() else { return };
    let mut answer = None;
    let keys = super::dialogs::modal_keys(ctx, "set-drop", true, |ui| match &p {
        Pending::Choice { item, from, .. } => {
            let name = model.item(item).map_or(item.clone(), |i| i.name.clone());
            ui.heading(format!("{name} is in {from}"));
            if ui.button("Move it here").clicked() {
                answer = Some(ModalAnswer::Choice(DropChoice::Move));
            }
            if ui.button("Share (borrow its finished output)").clicked() {
                answer = Some(ModalAnswer::Choice(DropChoice::Share));
            }
            if ui.button("Duplicate the source for this set").clicked() {
                answer = Some(ModalAnswer::Choice(DropChoice::Duplicate));
            }
            if ui.button("Cancel").clicked() {
                answer = Some(ModalAnswer::Close);
            }
        }
        Pending::Replace { set, existing, .. } => {
            let name = model
                .item(existing)
                .map_or(existing.clone(), |i| i.name.clone());
            ui.heading("Replace?");
            ui.label(format!("{set} already has {name} in that slot."));
            ui.horizontal(|ui| {
                if ui.button(format!("Replace {name}")).clicked() {
                    answer = Some(ModalAnswer::Replace);
                }
                if ui.button("Cancel").clicked() {
                    answer = Some(ModalAnswer::Close);
                }
            });
        }
        Pending::Refused(why) => {
            ui.heading("Can't do that");
            ui.label(why);
            if ui.button("OK").clicked() {
                answer = Some(ModalAnswer::Close);
            }
        }
        Pending::ConfirmDelete { id, members } => {
            ui.heading(format!(
                "Delete set '{id}' ({members} member{})?",
                if *members == 1 { "" } else { "s" }
            ));
            ui.label("Its textures stay; only the set is removed.");
            ui.horizontal(|ui| {
                if ui.button("Delete").clicked() {
                    answer = Some(ModalAnswer::Delete);
                }
                if ui.button("Cancel").clicked() {
                    answer = Some(ModalAnswer::Close);
                }
            });
        }
        Pending::Rename { .. } | Pending::FillNames { .. } => {}
    });
    // Esc and a click outside cancel; Enter takes the safe default of a confirm (Cancel) and presses the OK of a refusal.
    let confirm = matches!(p, Pending::ConfirmDelete { .. } | Pending::Refused(_));
    if answer.is_none() && (keys.cancel || (keys.enter && confirm)) {
        answer = Some(ModalAnswer::Close);
    }
    let Some(answer) = answer else { return };
    st.pending = match (answer, p) {
        (ModalAnswer::Choice(c), Pending::Choice { item, set, .. }) => {
            match model.drop_choice(&item, &set, c) {
                DropOutcome::AskReplace { existing } => Some(Pending::Replace {
                    item,
                    set,
                    existing,
                    choice: c,
                }),
                DropOutcome::Refused(why) => Some(Pending::Refused(why)),
                _ => None,
            }
        }
        // The replace answers the choice that led to it (Move, Share or Duplicate); a refusal shows its reason.
        (
            ModalAnswer::Replace,
            Pending::Replace {
                item, set, choice, ..
            },
        ) => model
            .drop_replace(&item, &set, choice)
            .err()
            .map(Pending::Refused),
        (ModalAnswer::Delete, Pending::ConfirmDelete { id, .. }) => {
            model.delete_set(&id);
            None
        }
        _ => None,
    };
}

/// Item 18: Fill from names. A row per file with its slot; a filled slot says what it replaces and a blocked row
/// (a locked slot) cannot be ticked. Esc or Cancel closes, Enter or Fill applies the ticked rows as one step. Ticks
/// are typed state, so a click outside does nothing (AG11).
fn fill_modal(ctx: &egui::Context, model: &mut AppModel, st: &mut DropState) {
    let Some(Pending::FillNames { set, rows, ticks }) = &mut st.pending else {
        return;
    };
    let (mut apply, mut cancel, mut error) = (false, false, None);
    let keys = super::dialogs::modal_keys(ctx, "fill-names", false, |ui| {
        ui.heading(format!("Fill {set} from names"));
        ui.label("Each ticked file moves into the set's slot; nothing is copied.");
        egui::Grid::new("fill-names-grid")
            .num_columns(2)
            .show(ui, |ui| {
                for (r, tick) in rows.iter().zip(ticks.iter_mut()) {
                    let mut on = *tick && r.blocked.is_none();
                    let resp =
                        ui.add_enabled(r.blocked.is_none(), egui::Checkbox::new(&mut on, &r.name));
                    if let Some(why) = &r.blocked {
                        resp.on_disabled_hover_text(why);
                    }
                    *tick = on;
                    let mut what = format!("→ {}", r.slot.label());
                    match (&r.blocked, &r.replaces) {
                        (Some(why), _) => what.push_str(&format!(" · {why}")),
                        (None, Some(cur)) => what.push_str(&format!(" · replaces {cur}")),
                        _ => {}
                    }
                    ui.label(what);
                    ui.end_row();
                }
            });
        ui.horizontal(|ui| {
            if ui.button("Fill").clicked() {
                apply = true;
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
        });
    });
    if keys.cancel || cancel {
        st.pending = None;
        return;
    }
    if apply || keys.enter {
        let picked: Vec<String> = rows
            .iter()
            .zip(ticks.iter())
            .filter(|(_, t)| **t)
            .map(|(r, _)| r.item.clone())
            .collect();
        if let Err(e) = model.fill_from_names(set, &picked) {
            error = Some(e);
        }
        st.pending = error.map(Pending::Refused);
    }
}

/// The rename dialog: Enter or OK renames, a refusal (a name already taken) is shown under the field, and
/// Cancel or Esc closes it.
fn rename_modal(ctx: &egui::Context, model: &mut AppModel, st: &mut DropState) {
    let Some(Pending::Rename { id, text, error }) = &mut st.pending else {
        return;
    };
    let (mut submit, mut cancel) = (false, false);
    // Only Esc cancels: a click outside could lose the typed name (AG11).
    let keys = super::dialogs::modal_keys(ctx, "set-rename", false, |ui| {
        ui.heading(format!("Rename {id}"));
        let field = ui.text_edit_singleline(text);
        if !field.has_focus() && !field.lost_focus() {
            field.request_focus();
        }
        if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            submit = true;
        }
        if let Some(e) = error.as_ref() {
            ui.colored_label(RED, e);
        }
        ui.horizontal(|ui| {
            if ui.button("OK").clicked() {
                submit = true;
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
        });
    });
    if keys.cancel {
        cancel = true;
    }
    if cancel {
        st.pending = None;
    } else if submit {
        match model.rename_set(id, text) {
            Ok(()) => st.pending = None,
            Err(e) => *error = Some(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::codes::Code;

    fn warn(msg: &str) -> Warning {
        Warning::new(Code::LossAlphaData, msg)
    }

    #[test]
    fn the_warning_tooltip_lists_file_code_message_and_caps_at_ten() {
        let w = warn("alpha is lost");
        let few = [("a.png", &w), ("b.png", &w)];
        let t = warning_tooltip(&few);
        assert_eq!(t.lines().count(), 2);
        assert!(t.starts_with("a.png: LOSS-ALPHA-DATA alpha is lost"), "{t}");
        let many: Vec<(&str, &Warning)> = (0..13).map(|_| ("x.png", &w)).collect();
        let t = warning_tooltip(&many);
        assert_eq!(t.lines().count(), 11);
        assert_eq!(t.lines().last(), Some("… and 3 more"));
        assert!(!warning_tooltip(&many[..10]).contains("more"));
    }

    #[test]
    fn the_insertion_line_is_after_the_target_when_moving_down_and_before_it_when_moving_up() {
        let r = egui::Rect::from_min_max(egui::pos2(0.0, 10.0), egui::pos2(5.0, 30.0));
        assert_eq!(insertion_y(0, 2, r), Some(30.0));
        assert_eq!(insertion_y(2, 0, r), Some(10.0));
        assert_eq!(
            insertion_y(1, 1, r),
            None,
            "hovering the dragged pill itself shows no line"
        );
    }

    #[test]
    fn a_released_handle_over_another_pill_reorders_to_its_index() {
        let v = ["a", "b", "c"];
        assert_eq!(
            reorder_target(Some("a"), Some(2), &v),
            Some(("a".to_string(), 2))
        );
        assert_eq!(
            reorder_target(Some("c"), Some(0), &v),
            Some(("c".to_string(), 0))
        );
    }

    #[test]
    fn no_release_no_target_or_a_drop_on_itself_does_nothing() {
        let v = ["a", "b"];
        assert_eq!(reorder_target(None, Some(1), &v), None);
        assert_eq!(reorder_target(Some("a"), None, &v), None);
        assert_eq!(reorder_target(Some("a"), Some(0), &v), None);
        assert_eq!(reorder_target(Some("a"), Some(9), &v), None);
    }
}
