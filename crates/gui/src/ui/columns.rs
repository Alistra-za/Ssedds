//! The Textures and Sets columns (M3a spec §2.2–2.3). Drawing only: every click is a call on the model.
use super::thumbs::ThumbCache;
use crate::model::items::{ItemStatus, TextureItem};
use crate::model::{AppModel, Selection};
use eframe::egui;
use engine::events::RecipeStatus;
use std::collections::HashMap;

pub(super) const AMBER: egui::Color32 = egui::Color32::from_rgb(230, 160, 30);
pub(super) const RED: egui::Color32 = egui::Color32::from_rgb(210, 60, 60);
/// Width of a collapsed column.
pub const STRIP_WIDTH: f32 = 24.0;
const TILE: f32 = 96.0;
const MEMBER: f32 = 32.0;
const TILE_MARGIN: f32 = 4.0;
const TILE_STROKE: f32 = 2.0;
/// Scroll bar and margin allowance when fitting tiles to a row.
const SCROLL_ALLOWANCE: f32 = 18.0;

/// A collapsed column: a thin strip with the column name drawn vertically.
pub fn strip(ui: &mut egui::Ui, name: &str) {
    strip_sensing(ui, name, egui::Sense::hover());
}

/// A strip that can be clicked (the collapsed Advanced drawer opens with it).
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
    let color = if response.hovered() && sense.senses_click() {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().text_color()
    };
    let galley =
        ui.painter()
            .layout_no_wrap(name.to_string(), egui::FontId::proportional(14.0), color);
    // Rotated a quarter turn counter-clockwise: the text runs upward, its top edge on the left.
    let pos = egui::pos2(
        rect.center().x - galley.size().y / 2.0,
        rect.top() + 12.0 + galley.size().x,
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
fn tile_height(ui: &egui::Ui) -> f32 {
    TILE + 2.0 * (small_line_height(ui) + ui.spacing().item_spacing.y)
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

fn tile(ui: &mut egui::Ui, model: &mut AppModel, thumbs: &mut ThumbCache, idx: usize) {
    let inner_h = tile_height(ui) - 2.0 * (TILE_MARGIN + TILE_STROKE);
    let item = &model.items[idx];
    let selected = matches!(&model.selection, Selection::Item(k) if *k == item.key);
    let fill = if selected {
        ui.visuals().selection.bg_fill.linear_multiply(0.4)
    } else {
        egui::Color32::TRANSPARENT
    };
    let key = item.key.clone();
    let resp = egui::Frame::default()
        .stroke(egui::Stroke::new(TILE_STROKE, border_of(ui, &item.status)))
        .fill(fill)
        .inner_margin(TILE_MARGIN)
        .show(ui, |ui| {
            ui.style_mut().interaction.selectable_labels = false;
            ui.set_width(TILE);
            ui.set_height(inner_h);
            ui.vertical_centered(|ui| {
                match thumbs.get(ui.ctx(), item) {
                    Some(t) => {
                        ui.add(egui::Image::new(&t).fit_to_exact_size(egui::vec2(TILE, TILE)));
                    }
                    None => {
                        ui.allocate_space(egui::vec2(TILE, TILE));
                    }
                }
                ui.add(egui::Label::new(egui::RichText::new(&item.name).small()).truncate());
                let mut badges = vec![];
                if !item.warnings.is_empty() {
                    badges.push("⚠".to_string());
                }
                if let ItemStatus::PackedInto(j) = &item.status {
                    badges.push(format!("packed into {j}"));
                }
                if item.changed_on_disk {
                    badges.push("changed on disk".to_string());
                }
                ui.add(
                    egui::Label::new(egui::RichText::new(badges.join("  ")).small().weak())
                        .truncate(),
                );
            });
        })
        .response
        .interact(egui::Sense::click())
        .on_hover_text(tile_tooltip(item));
    if resp.double_clicked() {
        model.expand(&key);
    } else if resp.clicked() {
        model.select_item(&key);
    }
}

pub fn textures(ui: &mut egui::Ui, model: &mut AppModel, thumbs: &mut ThumbCache) {
    ui.horizontal(|ui| {
        ui.heading("Textures");
        let n = model.undecided();
        if n > 0 {
            ui.colored_label(AMBER, format!("{n} need a decision"));
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.toggle_value(&mut model.filter.needs_decision, "Needs decision");
        ui.toggle_value(&mut model.filter.warnings, "Has warnings");
        ui.toggle_value(&mut model.filter.failed, "Failed");
        ui.toggle_value(&mut model.filter.not_in_set, "Not in a set");
    });
    ui.add(
        egui::TextEdit::singleline(&mut model.search)
            .hint_text("Search")
            .desired_width(f32::INFINITY),
    );
    ui.separator();
    let visible = model.visible_items();
    if visible.is_empty() {
        if model.root.is_some() && !model.busy() {
            ui.weak("No textures to show.");
        }
        return;
    }
    let tile_h = tile_height(ui);
    let tile_w = TILE + 2.0 * (TILE_MARGIN + TILE_STROKE);
    let gap = ui.spacing().item_spacing.x;
    let cols = (((ui.available_width() - SCROLL_ALLOWANCE + gap) / (tile_w + gap)) as usize).max(1);
    let rows = visible.len().div_ceil(cols);
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, tile_h, rows, |ui, range| {
            for r in range {
                let row = &visible[r * cols..((r + 1) * cols).min(visible.len())];
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), tile_h),
                    egui::Layout::left_to_right(egui::Align::Min),
                    |ui| {
                        for &idx in row {
                            tile(ui, model, thumbs, idx);
                        }
                    },
                );
            }
        });
}

/// What a click in the Sets column asks of the model; applied after drawing, which only reads the model.
enum Click {
    Set(String),
    Item(String),
    Expand(String),
}

pub(super) fn status_text(s: RecipeStatus) -> (&'static str, Option<egui::Color32>) {
    match s {
        RecipeStatus::Run => ("run", None),
        RecipeStatus::Conflict => ("conflict", Some(AMBER)),
        RecipeStatus::Failed => ("failed", Some(RED)),
        RecipeStatus::Disabled => ("disabled", None),
    }
}

pub(super) fn member(
    ui: &mut egui::Ui,
    item: &TextureItem,
    selected: bool,
    thumbs: &mut ThumbCache,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(MEMBER, MEMBER), egui::Sense::click());
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
            border_of(ui, &item.status)
        };
        ui.painter().rect_stroke(
            rect,
            2.0,
            egui::Stroke::new(if selected { 2.0 } else { 1.0 }, color),
            egui::StrokeKind::Inside,
        );
    }
    response.on_hover_text(&item.name)
}

/// The export folder as a prefix of set keys: `<export folder>/`.
pub fn export_prefix(model: &AppModel) -> String {
    format!(
        "{}/",
        model
            .export_root()
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

pub fn sets(ui: &mut egui::Ui, model: &mut AppModel, thumbs: &mut ThumbCache) {
    ui.heading("Sets");
    ui.separator();
    if model.sets.is_empty() {
        if model.root.is_some() && !model.busy() {
            ui.weak("No sets.");
        }
        return;
    }
    let export_root = export_prefix(model);
    let mut click = None;
    {
        let by_key: HashMap<&str, &TextureItem> =
            model.items.iter().map(|i| (i.key.as_str(), i)).collect();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for set in &model.sets {
                    let set_selected =
                        matches!(&model.selection, Selection::Set(k) if *k == set.key);
                    let stroke = if set_selected {
                        ui.visuals().selection.stroke
                    } else {
                        ui.visuals().widgets.noninteractive.bg_stroke
                    };
                    let pill = ui.scope_builder(
                        egui::UiBuilder::new().sense(egui::Sense::click()),
                        |ui| {
                            ui.style_mut().interaction.selectable_labels = false;
                            egui::Frame::group(ui.style())
                                .corner_radius(12.0)
                                .stroke(stroke)
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.label(
                                        egui::RichText::new(set_label(&export_root, &set.key))
                                            .strong(),
                                    );
                                    ui.horizontal_wrapped(|ui| {
                                        for mk in &set.members {
                                            let Some(item) = by_key.get(mk.as_str()) else {
                                                continue;
                                            };
                                            let selected = matches!(
                                                &model.selection,
                                                Selection::Item(k) if k == mk
                                            );
                                            let r = member(ui, item, selected, thumbs);
                                            if r.double_clicked() {
                                                click = Some(Click::Expand(mk.clone()));
                                            } else if r.clicked() {
                                                click = Some(Click::Item(mk.clone()));
                                            }
                                        }
                                    });
                                    for recipe in &set.recipes {
                                        let (word, color) = status_text(recipe.status);
                                        let line = format!("{}: {word}", recipe.subtype.id());
                                        let text = egui::RichText::new(line).small();
                                        let label = match color {
                                            Some(c) => text.color(c),
                                            None => text.weak(),
                                        };
                                        let r = ui.label(label);
                                        if let Some(reason) = &recipe.reason {
                                            r.on_hover_text(reason);
                                        }
                                    }
                                });
                        },
                    );
                    // A member's own click is taken first; this one is the pill's background.
                    if pill.response.clicked() && click.is_none() {
                        click = Some(Click::Set(set.key.clone()));
                    }
                }
            });
    }
    match click {
        Some(Click::Set(k)) => model.select_set(&k),
        Some(Click::Item(k)) => model.select_item(&k),
        Some(Click::Expand(k)) => model.expand(&k),
        None => {}
    }
}
