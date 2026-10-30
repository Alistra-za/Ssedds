//! The Textures and Sets columns (M3a spec §2.2–2.3). Drawing only: every click is a call on the model.
use super::thumbs::ThumbCache;
use crate::model::bulk::{MenuAction, MenuEntry};
use crate::model::items::{ItemStatus, TextureItem};
use crate::model::texture_view::{encoder_label, ColumnPrefs, FilterOptions, SetFilter, SortKey};
use crate::model::AppModel;
use eframe::egui;
use engine::events::RecipeStatus;
use std::collections::HashSet;
use std::sync::Arc;

pub(super) use super::status::{AMBER, RED};
/// Width of a collapsed column.
pub const STRIP_WIDTH: f32 = 24.0;
/// The tile size a fresh install starts at (the slider runs 64 to 192); the narrowest column is sized for it.
const TILE: f32 = crate::model::texture_view::ColumnPrefs::TILE_DEFAULT;
const MEMBER: f32 = 32.0;
const TILE_MARGIN: f32 = 4.0;
const TILE_STROKE: f32 = 2.0;
/// Scroll bar and margin allowance when fitting tiles to a row.
const SCROLL_ALLOWANCE: f32 = 18.0;
/// A side panel's own chrome: `Frame::side_top_panel`'s 8 px margins and the 1 px separator.
const PANEL_CHROME: f32 = 2.0 * 8.0 + 1.0;
/// A set pill's chrome (`Frame::group`: 6 px margin and 1 px stroke a side) and egui's 8 px item spacing.
const PILL_CHROME: f32 = 2.0 * (6.0 + 1.0);
const ITEM_SPACING: f32 = 8.0;
/// The Textures column's narrowest width: its heading over one tile.
pub const TEXTURES_MIN: f32 =
    TILE + 2.0 * (TILE_MARGIN + TILE_STROKE) + SCROLL_ALLOWANCE + PANEL_CHROME;
/// The Sets column's narrowest width: its heading over a pill with two members in a row (one alone is under
/// egui's 96 px default).
pub const SETS_MIN: f32 =
    2.0 * MEMBER + ITEM_SPACING + PILL_CHROME + SCROLL_ALLOWANCE + PANEL_CHROME;

/// The arrow beside a column heading that collapses the column to its strip (the strip's own arrow expands it).
pub fn collapse_arrow(ui: &mut egui::Ui, model: &mut AppModel, column: crate::ui::Column) {
    let name = match column {
        crate::ui::Column::Textures => "Textures",
        crate::ui::Column::Sets => "Sets",
    };
    if ui
        .small_button("◀")
        .on_hover_text(format!("Collapse the {name} column"))
        .clicked()
    {
        *column.flags(&mut model.settings).1 = true;
        model.save_settings();
    }
}

/// A strip that can be clicked (a collapsed column, and the collapsed Advanced drawer, open with it).
pub fn strip_button(ui: &mut egui::Ui, name: &str) -> egui::Response {
    strip_sensing(ui, name, egui::Sense::click())
}

fn strip_sensing(ui: &mut egui::Ui, name: &str, sense: egui::Sense) -> egui::Response {
    let widget = if sense.senses_click() {
        egui::WidgetType::Button
    } else {
        egui::WidgetType::Label
    };
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(STRIP_WIDTH, ui.available_height()), sense);
    response.widget_info(|| egui::WidgetInfo::labeled(widget, ui.is_enabled(), name));
    // T7: an instant press look, under the text.
    if response.is_pointer_button_down_on() {
        ui.painter()
            .rect_filled(rect, 0.0, ui.visuals().widgets.active.weak_bg_fill);
    }
    let color = if response.hovered() && sense.senses_click() {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().text_color()
    };
    let galley =
        ui.painter()
            .layout_no_wrap(name.to_string(), egui::FontId::proportional(14.0), color);
    // A clickable strip shows the arrow that opens it, above the name.
    let top = if sense.senses_click() {
        ui.painter().text(
            egui::pos2(rect.center().x, rect.top() + 4.0),
            egui::Align2::CENTER_TOP,
            "▶",
            egui::FontId::proportional(12.0),
            color,
        );
        20.0
    } else {
        0.0
    };
    // Rotated a quarter turn counter-clockwise: the text runs upward, its top edge on the left.
    let pos = egui::pos2(
        rect.center().x - galley.size().y / 2.0,
        rect.top() + 12.0 + top + galley.size().x,
    );
    ui.painter().add(
        egui::epaint::TextShape::new(pos, galley, color).with_angle(-std::f32::consts::FRAC_PI_2),
    );
    response
}

fn small_line_height(ui: &egui::Ui) -> f32 {
    ui.text_style_height(&egui::TextStyle::Small)
}

/// A tile's outer height: the thumbnail, a name line and a badge line, plus margin and stroke. Fixed, so the
/// grid can draw only the rows in view.
fn tile_height(ui: &egui::Ui, tile: f32) -> f32 {
    tile + 2.0 * (small_line_height(ui) + ui.spacing().item_spacing.y)
        + 2.0 * (TILE_MARGIN + TILE_STROKE)
}

pub(super) fn border_of(ui: &egui::Ui, status: &ItemStatus) -> egui::Color32 {
    match status {
        ItemStatus::NeedsDecision => AMBER,
        ItemStatus::Failed(_) | ItemStatus::Error(_) => RED,
        _ => ui.visuals().widgets.noninteractive.bg_stroke.color,
    }
}

/// The hover text of a tile: what the status means, then the warnings.
fn tile_tooltip(item: &TextureItem) -> String {
    let mut lines = vec![item.key.clone()];
    match &item.status {
        ItemStatus::Failed(e) => lines.push(format!("Failed: {e}")),
        ItemStatus::Error(codes) => lines.push(format!(
            "Error: {}",
            codes
                .iter()
                .map(|c| c.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
        ItemStatus::Skipped(r) => lines.push(r.clone()),
        ItemStatus::NeedsDecision => lines.push("Needs a decision".to_string()),
        ItemStatus::PackedInto(j) => lines.push(format!("Packed into {j}")),
        ItemStatus::Ready => {}
    }
    lines.extend(item.warnings.iter().map(|w| w.message.clone()));
    lines.join("\n")
}

fn tile(
    ui: &mut egui::Ui,
    model: &mut AppModel,
    thumbs: &mut ThumbCache,
    idx: usize,
    chosen: &HashSet<String>,
    size: f32,
) {
    let inner_h = tile_height(ui, size) - 2.0 * (TILE_MARGIN + TILE_STROKE);
    let item = &model.items[idx];
    let selected = chosen.contains(&item.key);
    let fill = if selected {
        ui.visuals().selection.bg_fill.linear_multiply(0.4)
    } else {
        egui::Color32::TRANSPARENT
    };
    let key = item.key.clone();
    // T7: the tile being dragged is dimmed; the ghost follows the pointer (`drag_ghost`).
    let dim = egui::DragAndDrop::payload::<crate::ui::sets_column::DragItem>(ui.ctx())
        .is_some_and(|p| p.0 == key);
    let opacity = ui.opacity();
    if dim {
        ui.set_opacity(opacity * 0.5);
    }
    // F3b: an `[inputs]` alias shows, so it never looks like the file was renamed on disk.
    let read_as = model
        .read_as_of(&key)
        .map(|a| a.rsplit(['/', '\\']).next().unwrap_or_default().to_string());
    let resp = egui::Frame::default()
        .stroke(egui::Stroke::new(TILE_STROKE, border_of(ui, &item.status)))
        .fill(fill)
        .inner_margin(TILE_MARGIN)
        .show(ui, |ui| {
            ui.style_mut().interaction.selectable_labels = false;
            ui.set_width(size);
            ui.set_height(inner_h);
            ui.vertical_centered(|ui| {
                match thumbs.get(ui.ctx(), item) {
                    Some(t) => {
                        ui.add(egui::Image::new(&t).fit_to_exact_size(egui::vec2(size, size)));
                    }
                    None => {
                        ui.allocate_space(egui::vec2(size, size));
                    }
                }
                ui.add(egui::Label::new(egui::RichText::new(&item.name).small()).truncate());
                let mut badges = vec![];
                if item.warnings.iter().any(super::counts_toward_badge) {
                    badges.push("⚠".to_string());
                }
                if let ItemStatus::PackedInto(j) = &item.status {
                    badges.push(format!("packed into {j}"));
                }
                if item.changed_on_disk {
                    badges.push("changed on disk".to_string());
                }
                if let Some(a) = &read_as {
                    badges.push(format!("read as {a}"));
                }
                let ready = crate::model::bottom::tile_ready(item);
                ui.horizontal(|ui| {
                    if ready {
                        super::status::dot(ui, super::status::Status::Ready);
                    }
                    ui.add(
                        egui::Label::new(egui::RichText::new(badges.join("  ")).small().weak())
                            .truncate(),
                    );
                });
            });
        })
        .response
        .interact(egui::Sense::click_and_drag())
        .on_hover_text(tile_tooltip(item));
    ui.set_opacity(opacity);
    // T7: hover and press show instantly; the frame's stroke was chosen before `interact`, so paint over it.
    // Not on the dimmed drag source: a full-opacity stroke would make it look lit (review I1).
    let base = border_of(ui, &item.status);
    let stroke = interact_stroke(ui, &resp, base);
    if stroke != base && !dim {
        ui.painter().rect_stroke(
            resp.rect,
            ui.visuals().widgets.noninteractive.corner_radius,
            egui::Stroke::new(TILE_STROKE, stroke),
            egui::StrokeKind::Inside,
        );
    }
    resp.dnd_set_drag_payload(crate::ui::sets_column::DragItem(key.clone()));
    // F3a D4: a right-click on an unselected tile selects it alone first.
    if resp.secondary_clicked() {
        model.right_click_item(&key);
    }
    let mut picked = None;
    resp.context_menu(|ui| {
        picked = bulk_menu(ui, &model.menu_title(false), &model.item_menu());
    });
    if let Some(a) = picked {
        model.menu_picked(a);
    } else if resp.double_clicked() {
        super::after_commit(ui.ctx(), model, move |m| m.expand(&key));
    } else if resp.clicked() {
        // Note FC8 (BN3): a new selection waits a frame for a focused field to commit.
        let mods = ui.input(|i| i.modifiers);
        super::after_commit(ui.ctx(), model, move |m| {
            m.click_item(&key, mods.command, mods.shift)
        });
    }
}

/// What the Textures column derives from the selection, the filters and the items; rebuilt only when those change.
pub struct ColumnStats {
    pub summary: Option<String>,
    pub chosen: HashSet<String>,
}

#[derive(Clone, PartialEq)]
struct StatsKey {
    items: u64,
    filter: crate::model::Filter,
    search: String,
    sort: SortKey,
    selection: crate::model::Selection,
    selection_rev: u64,
}

/// `changed on disk` and `need a decision` stay uncounted here: they are one cheap pass and move without the items revision.
pub fn column_stats(ctx: &egui::Context, model: &AppModel) -> Arc<ColumnStats> {
    let key = StatsKey {
        items: model.items_rev(),
        filter: model.filter.clone(),
        search: model.search.clone(),
        sort: model.prefs.sort,
        selection: model.selection.clone(),
        selection_rev: model.multi.selection_rev,
    };
    let id = egui::Id::new("column-stats");
    if let Some((k, s)) = ctx.data(|d| d.get_temp::<(StatsKey, Arc<ColumnStats>)>(id)) {
        if k == key {
            return s;
        }
    }
    let s = Arc::new(ColumnStats {
        summary: model.selection_summary(false),
        chosen: model.selected_items().into_iter().collect(),
    });
    ctx.data_mut(|d| d.insert_temp(id, (key, s.clone())));
    s
}

pub fn textures(ui: &mut egui::Ui, model: &mut AppModel, thumbs: &mut ThumbCache) {
    // Note FB13 (C23): one line however many are selected. The "need a decision" link keeps its place on the right;
    // the selection count truncates, with the full text on hover.
    let stats = column_stats(ui.ctx(), model);
    let n = model.undecided();
    let mut open_undecided = false;
    ui.horizontal(|ui| {
        collapse_arrow(ui, model, crate::ui::Column::Textures);
        egui::Sides::new().shrink_left().truncate().show(
            ui,
            |ui| {
                ui.heading("Textures");
                if let Some(s) = &stats.summary {
                    ui.add(egui::Label::new(egui::RichText::new(s).strong()).truncate());
                }
            },
            |ui| {
                let link = egui::RichText::new(format!("{n} need a decision")).color(AMBER);
                open_undecided = n > 0 && ui.link(link).clicked();
            },
        );
    });
    if open_undecided {
        model.open_problems(Some(crate::model::bottom::ProblemsFilter::Undecided));
    }
    let changed = model.items.iter().filter(|i| i.changed_on_disk).count();
    if changed > 0 {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(AMBER, format!("{changed} changed on disk"));
            if ui.button("Reload").clicked() {
                model.reload_changed();
            }
        });
    }
    filter_row(ui, model);
    ui.add(
        egui::TextEdit::singleline(&mut model.search)
            .hint_text("Search")
            .desired_width(f32::INFINITY),
    );
    ui.separator();
    let visible = model.visible_shared();
    let helpers = model.visible_helpers_shared();
    if visible.is_empty() {
        if model.root.is_some() && !model.busy() {
            if model.filters_active() && !model.items.is_empty() {
                ui.label(format!(
                    "No textures match the filters ({} hidden)",
                    model.hidden_by_filters()
                ));
                if ui.button("Clear filters").clicked() {
                    model.clear_filters();
                }
            } else {
                ui.weak("No textures to show.");
            }
        }
        if helpers.is_empty() {
            return;
        }
    }
    // One set per frame keeps the per-tile test O(1) however many are selected.
    let stats = column_stats(ui.ctx(), model);
    let chosen = &stats.chosen;
    // A tile never runs wider than the column, so a narrow column still shows one.
    let chrome = 2.0 * (TILE_MARGIN + TILE_STROKE);
    let size = model
        .prefs
        .tile
        .min(ui.available_width() - SCROLL_ALLOWANCE - chrome)
        .max(ColumnPrefs::TILE_RANGE.0);
    let tile_h = tile_height(ui, size);
    let tile_w = size + chrome;
    let gap = ui.spacing().item_spacing.x;
    let cols = (((ui.available_width() - SCROLL_ALLOWANCE + gap) / (tile_w + gap)) as usize).max(1);
    let rows = visible.len().div_ceil(cols);
    // Item 5: Up and Down move by this row's width, and a texture the keys or the palette moved to scrolls into view.
    model.nav.cols = cols;
    let row_step = tile_h + ui.spacing().item_spacing.y;

    // Note DY16: the helper group under the tiles: a header line, then fixed-height rows.
    let helper_h = helper_row_height(ui);
    let header_h = if helpers.is_empty() {
        0.0
    } else {
        // The header is a Button: its height is the larger of the minimum and one line plus its padding.
        let line =
            ui.text_style_height(&egui::TextStyle::Body) + 2.0 * ui.spacing().button_padding.y;
        line.max(ui.spacing().interact_size.y) + ui.spacing().item_spacing.y
    };
    let tiles_h = rows as f32 * row_step;
    // Item 5 and Note FB12: a texture the keys or the palette moved to scrolls into view (a tile row, or a helper row,
    // which the palette opened the group for).
    let helper_top = tiles_h + header_h;
    let reveal = model.nav.reveal_item.take().and_then(|k| {
        let tile = visible.iter().position(|&i| model.items[i].key == k);
        match tile {
            Some(n) => Some(((n / cols) as f32 * row_step, tile_h)),
            None => helpers
                .iter()
                .position(|&i| model.items[i].key == k)
                .map(|n| (helper_top + n as f32 * helper_h, helper_h)),
        }
    });
    // Note FB12: the group is open unless the user collapsed it; collapsed, only the header line is laid out.
    let open = !model.filter.hide_helpers;
    let shown = if open { helpers.len() } else { 0 };
    let total = tiles_h + header_h + shown as f32 * helper_h;
    let mut toggle = false;
    let mut drawn = 0usize;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_viewport(ui, |ui, viewport| {
            ui.set_height(total);
            let origin = ui.max_rect().min;
            let width = ui.max_rect().width();
            // Only what intersects the viewport is laid out (rule 9.2); the reveal's rectangle is computed.
            if let Some((y, h)) = reveal {
                let rect =
                    egui::Rect::from_min_size(origin + egui::vec2(0.0, y), egui::vec2(width, h));
                ui.scroll_to_rect(rect, None);
            }
            let first = (viewport.min.y / row_step).floor().max(0.0) as usize;
            let last = ((viewport.max.y / row_step).ceil().max(0.0) as usize).min(rows);
            for r in first..last {
                let row = &visible[r * cols..((r + 1) * cols).min(visible.len())];
                let rect = egui::Rect::from_min_size(
                    origin + egui::vec2(0.0, r as f32 * row_step),
                    egui::vec2(width, tile_h),
                );
                ui.push_id(("tile-row", r), |ui| {
                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .max_rect(rect)
                            .layout(egui::Layout::left_to_right(egui::Align::Min)),
                        |ui| {
                            for &idx in row {
                                tile(ui, model, thumbs, idx, chosen, size);
                            }
                        },
                    );
                });
            }
            if helpers.is_empty() {
                return;
            }
            let header = egui::Rect::from_min_size(
                origin + egui::vec2(0.0, tiles_h),
                egui::vec2(width, header_h),
            );
            if viewport.translate(origin.to_vec2()).intersects(header) {
                // An explicit id: opening or closing never shifts other widgets' auto ids.
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .id_salt("helpers-header")
                        .max_rect(header),
                    |ui| {
                        let name = format!("Helpers ({})", helpers.len());
                        let arrow = if open { "▾" } else { "▸" };
                        let r = ui.add(
                            egui::Button::new(
                                egui::RichText::new(format!("{arrow} {name}")).strong(),
                            )
                            .frame(false),
                        );
                        r.widget_info(|| {
                            egui::WidgetInfo::selected(egui::WidgetType::Button, true, open, &name)
                        });
                        let hover = if open {
                            "Collapse the helper files (AO, masks, layers…)"
                        } else {
                            "Show the helper files (AO, masks, layers…)"
                        };
                        toggle = r.on_hover_text(hover).clicked();
                    },
                );
            }
            let top = tiles_h + header_h;
            let first = ((viewport.min.y - top) / helper_h).floor().max(0.0) as usize;
            let last = (((viewport.max.y - top) / helper_h).ceil().max(0.0) as usize).min(shown);
            for n in first..last {
                let idx = helpers[n];
                let rect = egui::Rect::from_min_size(
                    origin + egui::vec2(0.0, top + n as f32 * helper_h),
                    egui::vec2(width, helper_h),
                );
                helper_row(ui, model, thumbs, idx, chosen, rect);
                drawn += 1;
            }
        });
    if toggle {
        model.filter.hide_helpers = open;
    }
    ui.ctx()
        .data_mut(|d| d.insert_temp(egui::Id::new(HELPER_ROWS_DRAWN), drawn));
}

/// Where the helper rows laid out in the last frame are counted (tests: only the rows in view).
const HELPER_ROWS_DRAWN: &str = "helper-rows-drawn";

#[doc(hidden)]
pub fn helper_rows_drawn(ctx: &egui::Context) -> usize {
    ctx.data(|d| d.get_temp(egui::Id::new(HELPER_ROWS_DRAWN)))
        .unwrap_or(0)
}

/// A helper row's fixed height: 24 px, or one line of text plus 6, whichever is larger.
fn helper_row_height(ui: &egui::Ui) -> f32 {
    HELPER_ROW_H.max(ui.text_style_height(&egui::TextStyle::Body) + 6.0)
}

const HELPER_ROW_H: f32 = 24.0;
const HELPER_THUMB: f32 = 20.0;

/// Note DY16: one helper row in `rect`: a 20 px thumbnail and the file name on the left; where it goes ("→ targets"
/// or "not used") and the role chip on the right. A click selects it as a tile click does.
fn helper_row(
    ui: &mut egui::Ui,
    model: &mut AppModel,
    thumbs: &mut ThumbCache,
    idx: usize,
    chosen: &HashSet<String>,
    rect: egui::Rect,
) {
    let item = &model.items[idx];
    let Some(h) = item.helper.clone() else {
        return;
    };
    let key = item.key.clone();
    let target = crate::model::items::helper_target_text(&h);
    let role = h.role.label();
    // Registered before the cells, so the truncated labels keep their hover (the topmost widget wins the click).
    let resp = ui.interact(
        rect,
        ui.id().with(("helper-row", &key)),
        egui::Sense::click(),
    );
    let by_you = if h.role_hand_set { " (set by you)" } else { "" };
    let name = format!("Helper {}: {role}{by_you}, {target}", item.name);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
    if chosen.contains(&key) {
        ui.painter().rect_filled(
            rect,
            2.0,
            ui.visuals().selection.bg_fill.linear_multiply(0.4),
        );
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, 2.0, ui.visuals().widgets.hovered.weak_bg_fill);
    }
    let width = rect.width();
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.style_mut().interaction.selectable_labels = false;
        egui::Sides::new()
            .height(rect.height())
            .shrink_left()
            .truncate()
            .show(
                ui,
                |ui| {
                    let (r, _) = ui
                        .allocate_exact_size(egui::Vec2::splat(HELPER_THUMB), egui::Sense::hover());
                    match thumbs.get(ui.ctx(), item) {
                        Some(t) => egui::Image::new(&t)
                            .fit_to_exact_size(r.size())
                            .paint_at(ui, r),
                        None => {
                            ui.painter().rect_filled(
                                r,
                                2.0,
                                ui.visuals().widgets.noninteractive.weak_bg_fill,
                            );
                        }
                    }
                    ui.add(egui::Label::new(&item.name).truncate());
                },
                |ui| {
                    // The name keeps at least half the row; right to left: the chip, then the target.
                    ui.set_max_width(width * 0.5);
                    role_chip(ui, &role, h.role_hand_set);
                    let shown = if h.fills.is_empty() {
                        "not used".to_string()
                    } else {
                        format!("→ {}", target.trim_start_matches("fills to "))
                    };
                    ui.add(egui::Label::new(egui::RichText::new(shown).weak()).truncate())
                        .on_hover_text(&target);
                },
            );
    });
    if resp.clicked() {
        let mods = ui.input(|i| i.modifiers);
        super::after_commit(ui.ctx(), model, move |m| {
            m.click_item(&key, mods.command, mods.shift)
        });
    }
}

/// A helper's role as a small framed chip; a role the user set reads "✎ <role>" in amber.
fn role_chip(ui: &mut egui::Ui, role: &str, hand_set: bool) {
    let text = if hand_set {
        egui::RichText::new(format!("✎ {role}")).color(AMBER)
    } else {
        egui::RichText::new(role)
    };
    egui::Frame::new()
        .fill(ui.visuals().widgets.noninteractive.weak_bg_fill)
        .corner_radius(8)
        .inner_margin(egui::Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.add(egui::Label::new(text.small()).truncate());
        });
}

/// A compact combo: `name` and the current choice in one box (a label beside it would hold the column wider), the
/// hover text says what it filters.
fn pick<T: PartialEq + Copy>(
    ui: &mut egui::Ui,
    name: &str,
    hover: &str,
    current: &mut T,
    options: &[(T, String)],
) {
    let text = options
        .iter()
        .find(|o| o.0 == *current)
        .map_or("?", |o| o.1.as_str());
    egui::ComboBox::from_id_salt(name)
        .width(ui.available_width())
        .selected_text(format!("{name}: {text}"))
        .show_ui(ui, |ui| {
            for (v, label) in options {
                ui.selectable_value(current, *v, label);
            }
        })
        .response
        .on_hover_text(hover);
}

/// "Any or a value" over a filter's `Option<String>`. The current value is listed even if the folder has none of it,
/// so a saved filter is never invisible.
fn value_combo(
    ui: &mut egui::Ui,
    name: &str,
    hover: &str,
    current: &mut Option<String>,
    present: &[String],
    show: impl Fn(&str) -> String,
) {
    let mut values: Vec<&String> = present.iter().collect();
    if let Some(c) = current.as_ref().filter(|c| !present.contains(c)) {
        values.push(c);
    }
    let mut options = vec![(None, "Any".to_string())];
    options.extend(values.into_iter().map(|v| (Some(v.as_str()), show(v))));
    let mut now = current.as_deref();
    pick(ui, name, hover, &mut now, &options);
    if now != current.as_deref() {
        *current = now.map(str::to_string);
    }
}

/// The combos of the filter row, by position.
const COMBOS: usize = 5;

fn combo_at(ui: &mut egui::Ui, model: &mut AppModel, o: &FilterOptions, n: usize) {
    match n {
        0 => value_combo(
            ui,
            "Sub-type",
            "Only textures of this sub-type (the sub-types in this folder)",
            &mut model.filter.subtype,
            &o.subtypes,
            str::to_string,
        ),
        1 => value_combo(
            ui,
            "Output format",
            "Only textures that will be written in this DDS format",
            &mut model.filter.format,
            &o.formats,
            str::to_uppercase,
        ),
        2 => value_combo(
            ui,
            "Encoder",
            "Only textures that this encoder will compress",
            &mut model.filter.encoder,
            &o.encoders,
            |s| encoder_label(s).to_string(),
        ),
        3 => pick(
            ui,
            "Set membership",
            "Only textures in a set of more than one texture, or only those that are not",
            &mut model.filter.set,
            &SetFilter::ALL.map(|s| (s, s.label().to_string())),
        ),
        _ => pick(
            ui,
            "Sort",
            "Order of the tiles. Source size lists the largest first; Status lists errors, warnings, undecided, \
             then the rest. Ties go by name.",
            &mut model.prefs.sort,
            &SortKey::ALL.map(|s| (s, s.label().to_string())),
        ),
    }
}

/// The filter pills with their counts, the Filter ▾ popup and the tile buttons, in one wrapped row (A6, R16).
fn filter_row(ui: &mut egui::Ui, model: &mut AppModel) {
    let c = model.filter_counts();
    ui.horizontal_wrapped(|ui| {
        let f = &mut model.filter;
        for (on, text, hover) in [
            (
                &mut f.needs_decision,
                format!("Needs decision {}", c.needs_decision),
                "Only textures that still need a decision from you",
            ),
            (
                &mut f.warnings,
                format!("Warnings {}", c.warnings),
                "Only textures with warnings",
            ),
            (
                &mut f.failed,
                format!("Failed {}", c.failed),
                "Only textures that failed to plan or export",
            ),
            (
                &mut f.not_in_set,
                format!("Not in a set {}", c.not_in_set),
                "Only textures that are not a member of any set",
            ),
        ] {
            if ui
                .add(
                    egui::Button::selectable(*on, text)
                        .corner_radius(10)
                        .frame_when_inactive(true),
                )
                .on_hover_text(hover)
                .clicked()
            {
                *on = !*on;
            }
        }
        // A7: Sort is not a filter, so it stays out of the count.
        let k = [
            f.subtype.is_some(),
            f.format.is_some(),
            f.encoder.is_some(),
            f.set != SetFilter::Any,
        ]
        .iter()
        .filter(|on| **on)
        .count();
        let text = if k == 0 {
            "Filter ▾".to_string()
        } else {
            format!("Filter ({k}) ▾")
        };
        combo_menu(ui, &text, |ui| filter_popup(ui, model));
        let (lo, hi) = ColumnPrefs::TILE_RANGE;
        let tile = model.prefs.tile;
        // R16: a saved size off the 16 grid loads as it is and steps from there.
        for (glyph, name, hover, to, enabled) in [
            (
                "−",
                "Smaller tiles",
                "Smaller thumbnails",
                (tile - TILE_STEP).max(lo),
                tile > lo,
            ),
            (
                "+",
                "Larger tiles",
                "Larger thumbnails",
                (tile + TILE_STEP).min(hi),
                tile < hi,
            ),
        ] {
            let r = ui.add_enabled(enabled, egui::Button::new(glyph));
            r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, name));
            if r.on_hover_text(format!("{hover} ({lo} to {hi} points)"))
                .clicked()
            {
                model.prefs.tile = to;
            }
        }
    });
}

/// Where [`combo_menu`] keeps the pass it was last shown open in. shortcut: one id, so one combo menu in the app;
/// key it by the button id if a second one is added.
const COMBO_MENU: &str = "sdc-combo-menu";

/// Whether the combo menu was open at the last pass. [`super::actions::keys`] holds the shortcuts then, as it does for
/// egui's own popups, so Esc reaches the menu. A menu not drawn at the last pass (another view) is closed.
pub(crate) fn combo_menu_open(ctx: &egui::Context) -> bool {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|d| d.get_temp::<u64>(egui::Id::new(COMBO_MENU)))
        .is_some_and(|p| p + 1 == pass)
}

/// A menu that holds combos (final review I2). A combo's popup takes egui's one open-popup slot, which would close a
/// menu kept there, so this menu keeps its own open state; and a click while one of its combos is open (a pick, or a
/// click off it) only closes that combo. A click outside, Esc or the button closes the menu.
fn combo_menu(ui: &mut egui::Ui, label: &str, content: impl FnOnce(&mut egui::Ui)) {
    let button = ui.button(label);
    let mut open = combo_menu_open(ui.ctx());
    let child_open = egui::Popup::is_any_open(ui.ctx());
    if button.clicked() {
        open = !open;
    }
    let shown = egui::Popup::menu(&button)
        .open_bool(&mut open)
        .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
        .show(content);
    if shown.is_some_and(|r| !child_open && !button.clicked() && r.response.clicked_elsewhere()) {
        open = false;
    }
    let (id, pass) = (egui::Id::new(COMBO_MENU), ui.ctx().cumulative_pass_nr());
    ui.data_mut(|d| {
        if open {
            d.insert_temp(id, pass);
        } else {
            d.remove::<u64>(id);
        }
    });
}

/// How far a tile button moves the size.
const TILE_STEP: f32 = 16.0;
/// Every combo in the popup is this wide.
const POPUP_COMBO_WIDTH: f32 = 220.0;

/// The sub-type, format, encoder and set-membership combos and the sort. A combo is a nested layout, so each gets an
/// exact-width slot (AG124: `ComboBox::width` is only a minimum and a truncated combo takes all the room left).
fn filter_popup(ui: &mut egui::Ui, model: &mut AppModel) {
    let o = model.filter_options();
    let height = ui.spacing().interact_size.y;
    for n in 0..COMBOS {
        ui.allocate_ui(egui::vec2(POPUP_COMBO_WIDTH, height), |ui| {
            combo_at(ui, model, &o, n)
        });
    }
}

pub(super) fn status_text(s: RecipeStatus) -> (&'static str, Option<egui::Color32>) {
    match s {
        RecipeStatus::Run => ("run", None),
        RecipeStatus::Conflict => ("conflict", Some(AMBER)),
        RecipeStatus::Failed => ("failed", Some(RED)),
        RecipeStatus::Disabled => ("disabled", None),
    }
}

/// T7: the stroke of a tile or chip: `base` for a status colour (AMBER, RED), else the widget visuals' press or hover
/// stroke. Instant, no animation.
pub(super) fn interact_stroke(
    ui: &egui::Ui,
    resp: &egui::Response,
    base: egui::Color32,
) -> egui::Color32 {
    let w = &ui.visuals().widgets;
    if base == AMBER || base == RED {
        base
    } else if resp.is_pointer_button_down_on() {
        w.active.bg_stroke.color
    } else if resp.hovered() {
        w.hovered.bg_stroke.color
    } else {
        base
    }
}

/// T7 K10: the dragged texture's ghost, a 40 x 40 thumbnail beside the pointer. Painted once per frame from the
/// payload, not from the source widget, so it survives a source that scrolled out of a virtualised list.
pub(super) fn drag_ghost(ctx: &egui::Context, model: &AppModel, thumbs: &mut ThumbCache) {
    let Some(p) = egui::DragAndDrop::payload::<crate::ui::sets_column::DragItem>(ctx) else {
        return;
    };
    // No repaint request: the ghost moves only with pointer events, and egui repaints after the release frame's
    // events, which takes the ghost away (final review D1).
    let (Some(pos), Some(item)) = (ctx.pointer_interact_pos(), model.item(&p.0)) else {
        return;
    };
    let rect = egui::Rect::from_min_size(pos + egui::vec2(12.0, 12.0), egui::Vec2::splat(40.0));
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::new("drag-ghost"),
    ));
    match thumbs.get(ctx, item) {
        Some(t) => painter.image(
            t.id(),
            // Fitted, not stretched (review M2): a non-square thumbnail keeps its aspect in the 40 x 40 box.
            egui::Rect::from_center_size(
                rect.center(),
                t.size_vec2() * (rect.width() / t.size_vec2().max_elem()),
            ),
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE.gamma_multiply(0.75),
        ),
        None => painter.rect_filled(
            rect,
            2.0,
            ctx.global_style()
                .visuals
                .widgets
                .noninteractive
                .weak_bg_fill,
        ),
    };
}

pub(super) fn member(
    ui: &mut egui::Ui,
    item: &TextureItem,
    selected: bool,
    locked: bool,
    pgp_locked: bool,
    thumbs: &mut ThumbCache,
    // A line added to the one hover (a set card's "borrowed from …").
    note: Option<&str>,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(MEMBER, MEMBER), egui::Sense::click_and_drag());
    if ui.is_rect_visible(rect) {
        match thumbs.get(ui.ctx(), item) {
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
        let color = if selected {
            ui.visuals().selection.stroke.color
        } else {
            interact_stroke(ui, &response, border_of(ui, &item.status))
        };
        ui.painter().rect_stroke(
            rect,
            2.0,
            egui::Stroke::new(if selected { 2.0 } else { 1.0 }, color),
            egui::StrokeKind::Inside,
        );
        // F3a Task 8: the member fills a locked slot.
        if locked {
            ui.painter().text(
                rect.right_top() + egui::vec2(-1.0, 1.0),
                egui::Align2::RIGHT_TOP,
                "🔒",
                egui::FontId::proportional(10.0),
                ui.visuals().strong_text_color(),
            );
        }
        // Note AN: PGPatcher keeps the mesh's own texture in this member's slot.
        if pgp_locked {
            ui.painter().text(
                rect.left_top() + egui::vec2(1.0, 0.0),
                egui::Align2::LEFT_TOP,
                "✕",
                egui::FontId::proportional(13.0),
                RED,
            );
        }
    }
    let mut hover = item.name.clone();
    if locked {
        hover.push_str(" (slot pinned)");
    }
    if pgp_locked {
        hover.push_str(" (PGPatcher lock: the mesh's original texture is used)");
    }
    if let Some(n) = note {
        hover.push('\n');
        hover.push_str(n);
    }
    response.on_hover_text(hover)
}

/// The export folder as a prefix of set keys: `<export folder>/`.
pub fn export_prefix(model: &AppModel) -> String {
    format!(
        "{}/",
        model
            .sets_root()
            .to_string_lossy()
            .replace('\\', "/")
            .trim_end_matches('/')
    )
}

/// A set key is the output path without extension; the export folder in front of it is noise.
pub fn set_label(export_root: &str, key: &str) -> String {
    let k = key.replace('\\', "/");
    if k.len() > export_root.len()
        && k.is_char_boundary(export_root.len())
        && k[..export_root.len()].eq_ignore_ascii_case(export_root)
    {
        k[export_root.len()..].to_string()
    } else {
        k
    }
}

/// F3a D4: a right-click menu drawn from `entries` under its count title; the members-of-kind entries go in a
/// "Select members of kind" submenu, the slot menu's links in "From another set". The menus are lists so F3b can
/// append. Returns the clicked action.
pub(super) fn bulk_menu(
    ui: &mut egui::Ui,
    title: &str,
    entries: &[MenuEntry],
) -> Option<MenuAction> {
    let mut picked = None;
    ui.label(egui::RichText::new(title).strong());
    ui.separator();
    let (kinds, rest): (Vec<&MenuEntry>, Vec<&MenuEntry>) = entries
        .iter()
        .partition(|e| matches!(e.action, MenuAction::SelectMembersOfKind(_)));
    // F3a Task 8: the `SlotLink` entries go in the "From another set" submenu, which their header entry draws.
    let (links, rest): (Vec<&MenuEntry>, Vec<&MenuEntry>) = rest
        .into_iter()
        .partition(|e| matches!(e.action, MenuAction::SlotLink { .. }));
    for e in rest {
        if e.action == MenuAction::FromAnotherSet {
            let r = ui
                .add_enabled_ui(e.disabled.is_none(), |ui| {
                    ui.menu_button(&e.label, |ui| {
                        for l in &links {
                            let r =
                                ui.add_enabled(l.disabled.is_none(), egui::Button::new(&l.label));
                            let r = match &l.disabled {
                                Some(why) => r.on_disabled_hover_text(why),
                                None => r,
                            };
                            if r.clicked() {
                                picked = Some(l.action.clone());
                                ui.close();
                            }
                        }
                    })
                })
                .response;
            if let Some(why) = &e.disabled {
                r.on_disabled_hover_text(why);
            }
            continue;
        }
        let r = ui.add_enabled(e.disabled.is_none(), egui::Button::new(&e.label));
        let r = match &e.disabled {
            Some(why) => r.on_disabled_hover_text(why),
            None => r,
        };
        if r.clicked() {
            picked = Some(e.action.clone());
            ui.close();
        }
    }
    if !kinds.is_empty() {
        ui.menu_button("Select members of kind", |ui| {
            for e in kinds {
                if ui.button(&e.label).clicked() {
                    picked = Some(e.action.clone());
                    ui.close();
                }
            }
        });
    }
    picked
}
