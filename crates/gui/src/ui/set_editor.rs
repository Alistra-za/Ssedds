//! The set editor, layout A (M3b spec §4): slot cards on the left, PGPatcher settings on the right (Task 15).
//! Drawing only: every change is a model call, made after drawing.
use super::columns::{border_of, status_text, AMBER, RED};
use super::slot_commands::SlotCmdState;
use super::thumbs::ThumbCache;
use crate::model::fields::{channel_text, field_lock, format_label, format_options};
use crate::model::sets::{set_views, SetView, SlotFill, SlotKind};
use crate::model::AppModel;
use eframe::egui;
use engine::config::{ColorTag, Format};
use engine::events::RecipeReport;

const CARD: f32 = 150.0;
const THUMB: f32 = 72.0;
/// The frame, its margins and the grid spacing around a card.
const CARD_CHROME: f32 = 24.0;
/// Seconds a single click waits for its second click (a little over egui's double-click delay).
const DOUBLE_CLICK_WAIT: f64 = 0.35;
/// Seconds past the wait after which a pending click is stale (the view was not drawn when it came due).
const STALE_AFTER: f64 = 1.0;

enum Act {
    Select(String),
    Expand(String),
    Format(String, Format, ColorTag),
    Opaque(String, bool),
}

fn tag_options(f: Format) -> Vec<ColorTag> {
    match f {
        Format::Bc1 | Format::Bc3 | Format::Bc7 => vec![ColorTag::Srgb, ColorTag::Unorm],
        _ => vec![ColorTag::Unorm],
    }
}

pub fn draw(
    ui: &mut egui::Ui,
    model: &mut AppModel,
    key: &str,
    thumbs: &mut ThumbCache,
    cmds: &mut SlotCmdState,
) {
    let Some(v) = set_views(model).into_iter().find(|v| v.key == key) else {
        ui.weak("This set is no longer in the plan.");
        return;
    };
    ui.horizontal_wrapped(|ui| {
        ui.heading(&v.label);
        let m = v
            .settings
            .get("match_diffuse")
            .and_then(|x| x.as_str())
            .map(str::to_string);
        match (m, &v.derived_match) {
            (Some(m), _) => ui.label(format!("match_diffuse: {m}")),
            (None, Some(d)) => ui.label(format!("match_diffuse: {d} (derived)")),
            (None, None) => ui.weak("no diffuse to derive a match from"),
        };
        match (&v.prefix, &v.prefix_error) {
            (Some(p), _) => ui.weak(format!("→ {p}")),
            (_, Some(e)) => ui.colored_label(RED, e),
            _ => ui.weak(""),
        };
    });
    ui.separator();
    let mut acts = vec![];
    let editable = model.sets_doc.as_ref().is_some_and(|d| !d.read_only());
    let why = model.banner.clone().unwrap_or_default();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.columns(2, |cols| {
                let ui = &mut cols[0];
                cards(ui, model, &v, thumbs, &mut acts);
                ui.add_space(6.0);
                ui.add_enabled_ui(editable, |ui| {
                    if ui.button("Slot commands…").clicked() {
                        cmds.open = Some(v.id.clone());
                    }
                })
                .response
                .on_disabled_hover_text(&why);
                under_grid(ui, model, &v);
                super::pgp_fields::panel(&mut cols[1], model, &v.id);
            });
        });
    super::slot_commands::draw(ui.ctx(), model, cmds);
    let (mut select, mut expand) = (None, None);
    for a in acts {
        match a {
            Act::Select(k) => select = Some(k),
            Act::Expand(k) => expand = Some(k),
            Act::Format(k, f, t) => model.set_format(&k, f, t),
            Act::Opaque(k, on) => model.set_opaque(&k, on),
        }
    }
    select_or_expand(ui, model, select, expand);
}

/// The recipe lines, the set's warnings (each member's, each once) and what matches nothing.
fn under_grid(ui: &mut egui::Ui, model: &AppModel, v: &SetView) {
    let Some(set) = model.sets.iter().find(|s| s.key == v.key) else {
        return;
    };
    for r in &set.recipes {
        ui.add_space(4.0);
        recipe(ui, r);
    }
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

fn cards(
    ui: &mut egui::Ui,
    model: &AppModel,
    v: &SetView,
    thumbs: &mut ThumbCache,
    acts: &mut Vec<Act>,
) {
    // Four per row when they fit; fewer in a narrow column, so no card ends up outside the panel.
    let per_row = ((ui.available_width() / (CARD + CARD_CHROME)) as usize).clamp(1, 4);
    egui::Grid::new(("slot-cards", &v.id, per_row))
        .num_columns(per_row)
        .spacing([8.0, 8.0])
        .show(ui, |ui| {
            for (i, slot) in SlotKind::ALL.iter().enumerate() {
                card(ui, model, v, *slot, thumbs, acts);
                if i % per_row == per_row - 1 {
                    ui.end_row();
                }
            }
        });
}

fn card(
    ui: &mut egui::Ui,
    model: &AppModel,
    v: &SetView,
    slot: SlotKind,
    thumbs: &mut ThumbCache,
    acts: &mut Vec<Act>,
) {
    let fill = v.slots.get(&slot).cloned().unwrap_or(SlotFill::Empty);
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.set_width(CARD);
            ui.set_min_width(CARD);
            ui.label(egui::RichText::new(slot.label()).strong());
            let (item_key, borrowed) = match &fill {
                SlotFill::Own { item } => (Some(item.clone()), None),
                SlotFill::Borrowed { from, item } => (Some(item.clone()), Some(from.clone())),
                SlotFill::Empty => (None, None),
            };
            let item = item_key.as_deref().and_then(|k| model.item(k));
            let (rect, resp) =
                ui.allocate_exact_size(egui::vec2(THUMB, THUMB), egui::Sense::click());
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
            }
            let Some(i) = item else {
                ui.weak("empty");
                return;
            };
            ui.add(egui::Label::new(egui::RichText::new(&i.name).small()).truncate());
            if let Some(from) = &borrowed {
                ui.colored_label(AMBER, format!("borrowed from {from}"));
                return;
            }
            let resp = resp.on_hover_text(&i.key);
            if resp.double_clicked() {
                acts.push(Act::Expand(i.key.clone()));
            } else if resp.clicked() {
                acts.push(Act::Select(i.key.clone()));
            }
            let Some(job) = &i.job else { return };
            let (cur_f, cur_t) = (job.profile.format, job.profile.color.tag);
            // Format and Opaque write `sdc.toml`: a read-only one would drop the edit silently.
            let toml_read_only = model
                .project
                .as_ref()
                .and_then(|p| p.error())
                .map(|e| format!("sdc.toml is read-only: {e}"));
            let lock = toml_read_only
                .clone()
                .or(field_lock(model.target, &job.profile, "format").map(str::to_string));
            let mut choice = (cur_f, cur_t);
            ui.add_enabled_ui(lock.is_none(), |ui| {
                egui::ComboBox::from_id_salt(("slot-format", &v.id, slot.label()))
                    .selected_text(format_label(cur_f, cur_t))
                    .width(CARD - 24.0)
                    .show_ui(ui, |ui| {
                        for f in format_options(model.target) {
                            for t in tag_options(f) {
                                ui.selectable_value(&mut choice, (f, t), format_label(f, t));
                            }
                        }
                    });
            })
            .response
            .on_disabled_hover_text(lock.unwrap_or_default());
            if choice != (cur_f, cur_t) {
                acts.push(Act::Format(i.key.clone(), choice.0, choice.1));
            }
            if slot == SlotKind::Diffuse {
                let mut on = model.is_opaque(&i.key);
                if ui
                    .add_enabled(
                        toml_read_only.is_none(),
                        egui::Checkbox::new(&mut on, "Opaque"),
                    )
                    .on_hover_text("Strips alpha: the diffuse is fully opaque")
                    .on_disabled_hover_text(toml_read_only.unwrap_or_default())
                    .changed()
                {
                    acts.push(Act::Opaque(i.key.clone(), on));
                }
            }
        })
    });
}

/// A click selects a texture, which replaces this view by its settings, so the second click of a double click
/// would land on something else. The selection therefore waits out the double-click delay; a double click
/// meanwhile expands the texture instead.
fn select_or_expand(
    ui: &egui::Ui,
    model: &mut AppModel,
    clicked: Option<String>,
    expand: Option<String>,
) {
    let id = egui::Id::new("set-editor-pending-select");
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
        // A click left pending while this view was not drawn (another selection took over) is stale: drop it.
        if -left < STALE_AFTER {
            model.select_item(&k);
        }
    } else {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(left));
    }
}
