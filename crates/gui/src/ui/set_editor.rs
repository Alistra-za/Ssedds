//! The set editor, layout A (M3b spec §4): slot cards on the left, PGPatcher settings on the right (Task 15).
//! Drawing only: every change is a model call, made after drawing.
use super::columns::{border_of, status_text, AMBER, RED};
use super::rnm_section::RNM_OUTLINE;
use super::slot_commands::SlotCmdState;
use super::thumbs::ThumbCache;
use crate::model::fields::{channel_text, encoding_options, field_lock, format_label};
use crate::model::items::{ItemKind, TextureItem};
use crate::model::pack_fills::{fill_rows, FillRow};
use crate::model::rnm::{rnm_active, rnm_rows};
use crate::model::sets::{
    card_title, set_views, shows_json_panel, slot_of, slot_row, SetView, SlotCell, SlotFill,
    SlotKind,
};
use crate::model::AppModel;
use crate::pgp::prefix::{active_matches, match_toggle, slot_match_path};
use eframe::egui;
use engine::config::{ColorTag, Format, PackRecipe, RawMap, Target};
use engine::events::RecipeReport;
use engine::pack::RecipeKind;
use serde_json::Value;

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
    Recipe(String, PackRecipe),
    Fill(String, RawMap, Option<u8>),
    /// A PGPatcher setting of a set: (set id, key, value; `None` removes it).
    Json(String, String, Option<Value>),
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
        // The output prefix and its error are PGPatcher JSON facts: only a JSON set shows them.
        if shows_json_panel(&v, model.target) {
            match (&v.prefix, &v.prefix_error) {
                (Some(p), _) => ui.weak(format!("→ {p}")),
                (_, Some(e)) => ui.colored_label(RED, e),
                _ => ui.weak(""),
            };
        }
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
                unslotted_packed(ui, model, &v, &mut acts);
                ui.add_space(6.0);
                ui.add_enabled_ui(editable, |ui| {
                    if ui.button("Slot commands…").clicked() {
                        cmds.open = Some(v.id.clone());
                    }
                })
                .response
                .on_disabled_hover_text(&why);
                under_grid(ui, model, &v);
                super::plugin_refs::set_section(ui, model, &v);
                if shows_json_panel(&v, model.target) {
                    super::pgp_fields::panel(&mut cols[1], model, &v.id);
                }
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
            Act::Recipe(k, r) => {
                model.set_packing_recipe(&k, r);
            }
            Act::Fill(k, role, v) => model.set_pack_fill(&k, role, v),
            Act::Json(set, k, v) => model.set_json_value(&set, &k, v),
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
    for w in model.rnm_notes.get(&v.key).into_iter().flatten() {
        ui.colored_label(AMBER, format!("{}: {}", w.code.as_str(), w.message));
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
            for (i, cell) in slot_row(v.shader.as_ref(), &v.slots).iter().enumerate() {
                card(ui, model, v, cell, thumbs, acts);
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
    cell: &SlotCell,
    thumbs: &mut ThumbCache,
    acts: &mut Vec<Act>,
) {
    let slot = cell.kind;
    let fill = cell.fill.clone();
    let frame = egui::Frame::group(ui.style());
    // A filled slot the shader cannot use stays at its place, outlined, and says why on hover (F1 spec §5).
    let frame = if cell.usable {
        frame
    } else {
        frame.stroke(egui::Stroke::new(2.0, AMBER))
    };
    // The blue outline: this normal blends RNM layers (F2 spec §5.2). The amber one wins.
    let rnm_layers: Vec<&str> = match (&cell.fill, cell.kind) {
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
    let frame = if cell.usable && !rnm_layers.is_empty() {
        frame.stroke(egui::Stroke::new(2.0, RNM_OUTLINE))
    } else {
        frame
    };
    frame.show(ui, |ui| {
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.set_width(CARD);
            ui.set_min_width(CARD);
            let title = match &fill {
                SlotFill::Own { item } | SlotFill::Borrowed { item, .. } => {
                    card_title(slot, item, v.shader.as_ref().map(|p| p.shader))
                }
                SlotFill::Empty => slot.label(),
            };
            let t = ui.label(egui::RichText::new(title).strong());
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
            if let Some(n) = match slot {
                _ if !shows_json_panel(v, model.target) => None,
                SlotKind::Diffuse => Some(1),
                SlotKind::Normal => Some(2),
                _ => None,
            } {
                // F1 spec §3.2: the card's switch is the Matching header's slot 1/2 row.
                let derived = v.derived_match.as_deref().unwrap_or("");
                let on = active_matches(&v.settings, derived)
                    .iter()
                    .any(|(m, _)| *m == n);
                let mut want = on;
                let editable = model.sets_doc.as_ref().is_some_and(|d| !d.read_only());
                if ui
                    .add_enabled(editable, egui::Checkbox::new(&mut want, "Match on this"))
                    .on_hover_text(
                        "PGPatcher applies the entry to meshes with this exact texture in this slot",
                    )
                    .changed()
                {
                    let (k, val) = match_toggle(n, want, &slot_match_path(derived, slot.suffix()));
                    acts.push(Act::Json(v.id.clone(), k, val));
                }
            }
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
                .width(CARD - 24.0)
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

/// Packed outputs that fill no slot card (the cpm `_m`) get their fill rows here, under the cards.
fn unslotted_packed(ui: &mut egui::Ui, model: &AppModel, v: &SetView, acts: &mut Vec<Act>) {
    let why = super::settings_panel::toml_read_only_reason(model);
    for item in v
        .members
        .iter()
        .filter_map(|m| model.item(m))
        .filter(|i| i.kind == ItemKind::Packed && slot_of(i).is_none())
    {
        if fill_rows(model, &item.key).is_empty() {
            continue;
        }
        ui.add_space(6.0);
        ui.label(egui::RichText::new(&item.name).strong());
        ui.add_enabled_ui(why.is_none(), |ui| {
            fill_controls(ui, model, &item.key, acts)
        })
        .response
        .on_disabled_hover_text(why.clone().unwrap_or_default());
    }
}
