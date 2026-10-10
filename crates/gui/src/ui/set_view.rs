//! The centre panel for a selected set (M3a spec §2.4): its members, its recipes and their channel sources, and the
//! warnings of its members. Drawing only; a click selects a member and a double click expands it, through the model.
use super::columns::{border_of, export_prefix, set_label, status_text};
use super::thumbs::ThumbCache;
use crate::model::fields::channel_text;
use crate::model::AppModel;
use eframe::egui;
use engine::events::RecipeReport;

const THUMB: f32 = 64.0;
/// Seconds a single click waits for its second click (a little over egui's double-click delay).
const DOUBLE_CLICK_WAIT: f64 = 0.35;

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

pub fn draw(ui: &mut egui::Ui, model: &mut AppModel, key: &str, thumbs: &mut ThumbCache) {
    let Some(set) = model.sets.iter().find(|s| s.key == key) else {
        ui.weak("This set is no longer in the plan.");
        return;
    };
    ui.heading(set_label(&export_prefix(model), key));
    ui.add_space(4.0);
    let mut clicked = None;
    let mut expand = None;
    ui.horizontal_wrapped(|ui| {
        for mk in &set.members {
            let Some(item) = model.item(mk) else { continue };
            ui.vertical(|ui| {
                ui.set_width(THUMB);
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(THUMB, THUMB), egui::Sense::click());
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
                    ui.painter().rect_stroke(
                        rect,
                        2.0,
                        egui::Stroke::new(1.0, border_of(ui, &item.status)),
                        egui::StrokeKind::Inside,
                    );
                }
                ui.add(egui::Label::new(egui::RichText::new(&item.name).small()).truncate());
                let resp = resp.on_hover_text(&item.key);
                // A double click expands the member (M3a spec §2.4); a single click selects it.
                if resp.double_clicked() {
                    expand = Some(mk.clone());
                } else if resp.clicked() {
                    clicked = Some(mk.clone());
                }
            });
        }
    });
    ui.add_space(8.0);
    for r in &set.recipes {
        recipe(ui, r);
        ui.add_space(4.0);
    }
    // The union of the members' warnings, each once.
    let mut seen = std::collections::BTreeSet::new();
    let warnings: Vec<_> = set
        .members
        .iter()
        .filter_map(|m| model.item(m))
        .flat_map(|i| i.warnings.iter())
        .filter(|w| seen.insert((w.code, w.message.clone())))
        .collect();
    if !warnings.is_empty() {
        ui.add_space(6.0);
        ui.label(egui::RichText::new("Warnings").strong());
        for w in warnings {
            ui.label(egui::RichText::new(format!("{}: {}", w.code.as_str(), w.message)).small());
        }
    }
    select_or_expand(ui, model, clicked, expand);
}

/// A click selects a member, which replaces this view by the member's settings, so the second click of a double
/// click would land on something else. The selection therefore waits out the double-click delay; a double click
/// meanwhile expands the member instead.
fn select_or_expand(
    ui: &egui::Ui,
    model: &mut AppModel,
    clicked: Option<String>,
    expand: Option<String>,
) {
    let id = egui::Id::new("set-view-pending-select");
    let now = ui.input(|i| i.time);
    if let Some(k) = expand {
        ui.data_mut(|d| d.remove::<(String, f64)>(id));
        model.expand(&k);
        return;
    }
    if let Some(k) = clicked {
        ui.data_mut(|d| d.insert_temp(id, (k, now)));
    }
    let Some((k, at)) = ui.data(|d| d.get_temp::<(String, f64)>(id)) else {
        return;
    };
    let left = DOUBLE_CLICK_WAIT - (now - at);
    if left <= 0.0 {
        ui.data_mut(|d| d.remove::<(String, f64)>(id));
        model.select_item(&k);
    } else {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(left));
    }
}
