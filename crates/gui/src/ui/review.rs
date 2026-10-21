//! The Review tab (layout v2 B2, C2, C5; R1–R4, R11): blockers, undecided textures and pitfalls side by side, what
//! the export writes, and one Export button. Not a dialog: Enter does nothing here (N3).
use super::settings_panel::{alpha_options, offered_groups, subtype_label};
use super::status::{AMBER, RED};
use crate::model::bottom::{problems, Level};
use crate::model::items::Decision;
use crate::model::picker::{self, PickerPurpose, PickerSpec};
use crate::model::review::{DiskState, JsonKind, JsonLine};
use crate::model::{also, pbr_export, sets, Answer, AppModel};
use eframe::egui;
use engine::config::{AlphaPolicy, Subtype, Target};
use engine::presets::{AlsoTarget, ExportConfig};

/// What a click on an undecided row asks for.
pub enum RowPick {
    Select(String),
    Decide(String, Subtype),
    Answer(String, Answer),
}

pub fn apply(model: &mut AppModel, pick: RowPick) {
    match pick {
        RowPick::Select(k) => model.select_item(&k),
        RowPick::Decide(k, st) => model.decide_undecided_row(&k, st),
        RowPick::Answer(k, a) => model.answer_undecided_row(&k, a),
    }
}

/// One undecided texture's controls: name link, sub-type combo + ✓ when it asks its sub-type, the alpha buttons,
/// "Acknowledge for this texture". The controls are disabled with `off` as hover; the name link always selects.
pub fn undecided_row(
    ui: &mut egui::Ui,
    model: &AppModel,
    key: &str,
    off: Option<&str>,
) -> Option<RowPick> {
    let item = model.item(key)?;
    let fallback = item.job.as_ref().map(|j| j.profile.subtype);
    let question = model.subtype_question(key);
    let why = off.unwrap_or_default();
    let mut pick = None;
    ui.horizontal(|ui| {
        if ui.link(&item.name).clicked() {
            pick = Some(RowPick::Select(key.to_string()));
        }
        let Some(suggested) = question else {
            return;
        };
        let Some(default) = suggested.or(fallback) else {
            return;
        };
        ui.add_enabled_ui(off.is_none(), |ui| {
            let id = egui::Id::new(("undecided-row", key));
            let mut chosen: Subtype = ui.data(|d| d.get_temp(id)).unwrap_or(default);
            egui::ComboBox::from_id_salt(id)
                .selected_text(subtype_label(chosen))
                .show_ui(ui, |ui| {
                    for (heading, members) in offered_groups(model.target) {
                        ui.label(egui::RichText::new(heading).weak());
                        for st in members {
                            ui.selectable_value(&mut chosen, st, subtype_label(st));
                        }
                    }
                })
                .response
                .on_disabled_hover_text(why);
            ui.data_mut(|d| d.insert_temp(id, chosen));
            if ui
                .button("\u{2713}")
                .on_hover_text("Decide this texture")
                .on_disabled_hover_text(why)
                .clicked()
            {
                pick = Some(RowPick::Decide(key.to_string(), chosen));
            }
        });
    });
    // The questions that are not a sub-type: a small fixed set of choices each, answered inline.
    ui.add_enabled_ui(off.is_none(), |ui| {
        for d in &item.decisions {
            ui.horizontal_wrapped(|ui| match d {
                Decision::AlphaMeaning => {
                    ui.label("Alpha means:");
                    for (v, label) in alpha_options() {
                        let Ok(policy) = serde_json::from_value::<AlphaPolicy>(v) else {
                            continue;
                        };
                        if ui.button(label).on_disabled_hover_text(why).clicked() {
                            pick = Some(RowPick::Answer(key.to_string(), Answer::Alpha(policy)));
                        }
                    }
                }
                Decision::Acknowledge(code) => {
                    ui.label(code.as_str());
                    if ui
                        .button("Acknowledge for this texture")
                        .on_disabled_hover_text(why)
                        .clicked()
                    {
                        pick = Some(RowPick::Answer(key.to_string(), Answer::Acknowledge(*code)));
                    }
                }
                Decision::Subtype { .. } => {}
            });
        }
    });
    pick
}

/// A Problems row's fix buttons (A1), one set per open question of the texture `key`: Set type… (the suggestion
/// first), Decide… (the alpha options), Acknowledge. All disabled with `off` as hover.
pub fn fix_buttons(
    ui: &mut egui::Ui,
    model: &AppModel,
    key: &str,
    off: Option<&str>,
) -> Option<RowPick> {
    let item = model.item(key)?;
    let why = off.unwrap_or_default();
    let mut pick = None;
    let mut decisions: Vec<_> = item.decisions.iter().collect();
    // A Problems row adds its buttons right to left; keep them in reading order.
    if ui.layout().prefer_right_to_left() {
        decisions.reverse();
    }
    ui.add_enabled_ui(off.is_none(), |ui| {
        for d in decisions {
            match d {
                Decision::Subtype { suggested } => {
                    ui.menu_button("Set type…", |ui| {
                        if let Some(st) = suggested {
                            if ui
                                .button(format!("{} (suggested)", subtype_label(*st)))
                                .clicked()
                            {
                                pick = Some(RowPick::Decide(key.to_string(), *st));
                                ui.close();
                            }
                            ui.separator();
                        }
                        for (heading, members) in offered_groups(model.target) {
                            ui.label(egui::RichText::new(heading).weak());
                            for st in members {
                                if ui.button(subtype_label(st)).clicked() {
                                    pick = Some(RowPick::Decide(key.to_string(), st));
                                    ui.close();
                                }
                            }
                        }
                    })
                    .response
                    .on_disabled_hover_text(why);
                }
                Decision::AlphaMeaning => {
                    ui.menu_button("Decide…", |ui| {
                        for (v, label) in alpha_options() {
                            let Ok(policy) = serde_json::from_value::<AlphaPolicy>(v) else {
                                continue;
                            };
                            if ui.button(label).clicked() {
                                let a = Answer::Alpha(policy);
                                pick = Some(RowPick::Answer(key.to_string(), a));
                                ui.close();
                            }
                        }
                    })
                    .response
                    .on_disabled_hover_text(why);
                }
                Decision::Acknowledge(code) => {
                    if ui
                        .button("Acknowledge")
                        .on_disabled_hover_text(why)
                        .clicked()
                    {
                        pick = Some(RowPick::Answer(key.to_string(), Answer::Acknowledge(*code)));
                    }
                }
            }
        }
    });
    pick
}

enum Act {
    AlsoTick(AlsoTarget, bool),
    AlsoSet(Box<dyn FnOnce(&mut ExportConfig)>),
    AlsoLocation,
    Skip(String, bool),
    Recipe,
    GoTo(String),
    Row(RowPick),
    AcceptAll,
    Tick(String, bool),
    AckAll,
    Others,
    Json(bool),
    Export,
    Close,
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// R6: one line per JSON file under What will be written.
fn json_line_text(l: &JsonLine) -> String {
    let path = l.path.display();
    match (l.state, l.kind) {
        (DiskState::New, _) => format!("{path}: will be written"),
        (DiskState::Ours, _) => format!("{path}: will be replaced (written by Tessalith)"),
        (DiskState::Edited, JsonKind::PgPatcher) => format!("{path}: edited outside Tessalith"),
        (DiskState::Edited, _) => format!("{path}: edited outside Tessalith — kept, with a note"),
        (DiskState::Unreadable, _) => format!("{path}: cannot be read; kept"),
    }
}

/// A column: its heading, then `body` in a scroll area of its own.
fn column(ui: &mut egui::Ui, heading: &str, body: impl FnOnce(&mut egui::Ui)) {
    ui.strong(heading);
    // ui-rules 3.2: a bar drawn without hover, so a column that overflows says so.
    ui.spacing_mut().scroll = egui::style::ScrollStyle::thin();
    egui::ScrollArea::vertical()
        .id_salt(("review", heading))
        .auto_shrink([false, true])
        .show(ui, body);
}

/// P2 / TA30: the CM lines Review shows: the engine's install text (the report prints the same), plus "the Vanilla
/// folder is written too" when only CM is ticked.
pub fn also_cm_lines(cfg: &ExportConfig) -> Vec<&'static str> {
    let mut v = engine::also::cm_lines(cfg);
    if !cfg.also.contains(&AlsoTarget::Vanilla) {
        v.push(engine::also::CM_NEEDS_VANILLA);
    }
    v
}

/// One `[export.names]` field (PBR, Vanilla or CM), committed on focus loss (ui-rules 7.5); Esc cancels. Empty means
/// the default name, shown as the hint.
fn name_field(
    ui: &mut egui::Ui,
    root: Option<&std::path::Path>,
    which: usize,
    saved: Option<&str>,
    hint: &str,
    act: &mut Option<Act>,
) {
    // Keyed by the opened folder too: a draft never carries into another project (review M2).
    let id = egui::Id::new(("also-name", which, root));
    let saved = saved.unwrap_or_default().to_string();
    let mut t: String = ui.data(|d| d.get_temp(id)).unwrap_or_else(|| saved.clone());
    let r = ui.add(
        egui::TextEdit::singleline(&mut t)
            .hint_text(hint)
            .desired_width(f32::INFINITY),
    );
    let name = format!("{} folder name", ["PBR", "Vanilla", "CM"][which]);
    let enabled = ui.is_enabled();
    r.widget_info(|| {
        let mut i = egui::WidgetInfo::text_edit(enabled, saved.clone(), t.clone(), hint);
        i.label = Some(name.clone());
        i
    });
    if r.changed() {
        ui.data_mut(|d| d.insert_temp(id, t.clone()));
    }
    if r.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(id));
        if t != saved && !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            let v = (!t.is_empty()).then_some(t);
            *act = Some(Act::AlsoSet(Box::new(move |c| match which {
                0 => c.names.pbr = v,
                1 => c.names.vanilla = v,
                _ => c.names.cm = v,
            })));
        }
    }
}

/// Review's "Also export as" column (Note CM, owner answers 5 and 6): the ticks, the location, the folder names, Keep
/// metal brightness and the CM lines, the zip, the sets to skip, and the gate's Also reason (`gate`). It names what
/// is written and why it cannot be yet, never that it will succeed (Task 11 M5). The recipe is in Advanced.
fn also_section(ui: &mut egui::Ui, m: &AppModel, gate: Option<&str>, act: &mut Option<Act>) {
    let cfg = m.also_config();
    let (v, c) = m.also_ticks();
    let off = m
        .export
        .running
        .is_some()
        .then(|| also::EXPORT_RUNNING.to_string())
        .or_else(|| m.toml_read_only_reason());
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
    ui.add_enabled_ui(off.is_none(), |ui| {
        ui.horizontal(|ui| {
            for (t, on, text) in [
                (AlsoTarget::Vanilla, v, "Vanilla"),
                (AlsoTarget::Cm, c, "Complex material"),
            ] {
                // A tick left from the cs mode stays clickable, so it can be cleared.
                let cs = (!on && m.target != Target::Cs).then_some(also::NEEDS_CS);
                let mut now = on;
                let r = ui.add_enabled(cs.is_none(), egui::Checkbox::new(&mut now, text));
                if r
                    .on_disabled_hover_text(off.as_deref().or(cs).unwrap_or_default())
                    .changed()
                {
                    *act = Some(Act::AlsoTick(t, now));
                }
            }
        });
        ui.weak("Export location");
        let loc = m.also_location().display().to_string();
        egui::Sides::new().shrink_left().truncate().show(
            ui,
            |ui| {
                let glyph = ui.fonts_mut(|f| {
                    f.glyph_width(&egui::TextStyle::Body.resolve(ui.style()), '0')
                });
                let chars = (ui.available_width() / glyph).floor().max(8.0) as usize;
                let r = ui.add(egui::Label::new(super::header::elide_middle(&loc, chars)));
                let name = format!("Export location: {loc}");
                r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &name));
                r.on_hover_text(&loc);
            },
            |ui| {
                let r = ui.add_enabled(!m.picker.busy(), egui::Button::new("Choose…"));
                r.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        r.enabled(),
                        "Choose the Also export location…",
                    )
                });
                let r = r
                    .on_hover_text("Where the PBR, Vanilla and CM folders are written")
                    .on_disabled_hover_text(off.as_deref().unwrap_or(picker::BUSY_HOVER));
                if r.clicked() {
                    *act = Some(Act::AlsoLocation);
                }
            },
        );
        let folders = m.also_folders();
        let hint = |i: usize| {
            folders[i]
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        egui::Grid::new("also-names").num_columns(2).show(ui, |ui| {
            let names = [&cfg.names.pbr, &cfg.names.vanilla, &cfg.names.cm];
            for (i, label) in ["PBR", "Vanilla", "CM"].into_iter().enumerate() {
                ui.label(label);
                name_field(ui, m.root.as_deref(), i, names[i].as_deref(), &hint(i), act);
                ui.end_row();
            }
        });
        if c {
            let mut keep = cfg.legacy.keep_metal_brightness;
            if ui
                .checkbox(&mut keep, "Keep metal brightness")
                .on_hover_text("The CM diffuse keeps the metals' brightness instead of reusing the Vanilla diffuse")
                .changed()
            {
                *act = Some(Act::AlsoSet(Box::new(move |c| {
                    c.legacy.keep_metal_brightness = keep
                })));
            }
            ui.add_enabled_ui(keep, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Strength");
                    let now = cfg.legacy.keep_metal_strength;
                    // One edit (one undo step) per drag, not per frame.
                    let (r, done) =
                        super::advanced::held_value(ui, "also-keep-strength", now, |ui, s| {
                            let r = ui.add(egui::Slider::new(s, 0.0..=1.0));
                            let v = *s;
                            r.widget_info(|| {
                                egui::WidgetInfo::slider(
                                    r.enabled(),
                                    v,
                                    "Keep metal brightness strength",
                                )
                            });
                            r
                        });
                    r.on_hover_text("How much of the metal darkening it undoes");
                    if let Some(s) = done {
                        *act = Some(Act::AlsoSet(Box::new(move |c| {
                            c.legacy.keep_metal_strength = s
                        })));
                    }
                });
            });
            for l in also_cm_lines(&cfg) {
                // The engine's text is Markdown (the report prints it); no backticks on screen.
                ui.add(egui::Label::new(egui::RichText::new(l.replace('`', "")).weak()).wrap());
            }
        }
        let mut zip = cfg.zip;
        if ui
            .checkbox(&mut zip, "Also write a zip of each folder")
            .on_hover_text("With a SHA-256 sums file")
            .changed()
        {
            *act = Some(Act::AlsoSet(Box::new(move |c| c.zip = zip)));
        }
        skip_list(ui, m, &cfg, act);
    })
    .response
    .on_disabled_hover_text(off.as_deref().unwrap_or_default());
    if let Some(why) = gate {
        ui.add(egui::Label::new(egui::RichText::new(why).color(RED)).wrap());
    }
    if ui
        .link("Recipe settings are in Advanced")
        .on_hover_text("Brightness, AO, specular and the other [export.legacy] values")
        .clicked()
    {
        *act = Some(Act::Recipe);
    }
}

/// "Skip these sets" (C2, owner answer 6): one row per PBR set, checked = left out of Vanilla and CM. Reachable from
/// the 3D view too, where the set panel is not. At most 8 rows tall; only the rows on screen are drawn.
fn skip_list(ui: &mut egui::Ui, m: &AppModel, cfg: &ExportConfig, act: &mut Option<Act>) {
    ui.label("Skip these sets")
        .on_hover_text("Checked sets are left out of the Vanilla and CM folders");
    let views = sets::views(m);
    let pbr: Vec<_> = views.iter().filter(|v| also::is_pbr_set(v)).collect();
    if pbr.is_empty() {
        ui.weak("No PBR sets");
        return;
    }
    let rows = |ui: &mut egui::Ui, range: std::ops::Range<usize>, act: &mut Option<Act>| {
        for v in &pbr[range] {
            let on = cfg.skip.iter().any(|s| engine::sets::same_id(s, &v.id));
            let mut now = on;
            let r = ui.checkbox(&mut now, super::dialogs::short_name(&v.label));
            r.widget_info(|| {
                egui::WidgetInfo::selected(egui::WidgetType::Checkbox, r.enabled(), now, &v.label)
            });
            if r.on_hover_text(&v.id).changed() {
                *act = Some(Act::Skip(v.id.clone(), now));
            }
        }
    };
    // Up to 8 rows are drawn inline, so the column's own scroll (and scroll-to-focus) reaches them: a nested area
    // takes every scroll target set inside it (ui-rules gotcha, CC18).
    if pbr.len() <= 8 {
        rows(ui, 0..pbr.len(), act);
        return;
    }
    let row_h = ui.spacing().interact_size.y;
    ui.scope(|ui| {
        ui.spacing_mut().scroll = egui::style::ScrollStyle::thin();
        egui::ScrollArea::vertical()
            .id_salt("also-skip")
            .max_height(8.0 * (row_h + ui.spacing().item_spacing.y))
            .auto_shrink([false, true])
            .show_rows(ui, row_h, pbr.len(), |ui, range| rows(ui, range, act));
    });
}

pub fn tab(ui: &mut egui::Ui, model: &mut AppModel) {
    let Some(review) = &model.review else {
        return;
    };
    let m = &*model;
    // R-b: the asks and the blocks are cached per change, so the gate below reads them again for free.
    let blocks = pbr_export::export_blocks(m);
    let asks = pbr_export::export_asks(m);
    // A4: the warnings Problems lists that Review does not ask about.
    let others = problems(m)
        .iter()
        .filter(|p| p.level == Level::Warning)
        .count()
        .saturating_sub(asks.len());
    let undecided = m.undecided_keys();
    let off = m.fix_off();
    let any_suggestion = undecided
        .iter()
        .any(|k| matches!(m.subtype_question(k), Some(Some(_))));
    let gate = m.review_gate();
    let json = m.review_json();
    // Note CM85: a fourth column in the cs mode or while a tick is on; the Also reason only with a tick, as the gate.
    let also_on = m.also_on() || m.target == Target::Cs;
    let also_err = if m.also_on() {
        m.also_gate().err()
    } else {
        None
    };
    let mut act = None;
    // The footer first, pinned to the bottom, so the columns scroll above it and it is never clipped.
    egui::Panel::bottom("review-footer")
        .resizable(false)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong("What will be written");
                let files = m.export_files();
                ui.label(format!("{files} DDS file{}", plural(files)));
            });
            let answer = review.json_answer.as_ref().map(|a| a.overwrite);
            egui::ScrollArea::vertical()
                .id_salt("review-json")
                .max_height(96.0)
                .show(ui, |ui| {
                    for l in &json {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(json_line_text(l));
                            // C4: only the PGPatcher JSON can be answered here; none is picked until clicked.
                            if l.kind == JsonKind::PgPatcher && l.state == DiskState::Edited {
                                if ui
                                    .radio(answer == Some(false), "Keep the edited file")
                                    .clicked()
                                {
                                    act = Some(Act::Json(false));
                                }
                                if ui.radio(answer == Some(true), "Overwrite it").clicked() {
                                    act = Some(Act::Json(true));
                                }
                            }
                        });
                    }
                });
            ui.horizontal(|ui| {
                if ui.button("Close").clicked() {
                    act = Some(Act::Close);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let button = egui::Button::new(m.review_button_text());
                    // CF11: only the paint fades in; the button is live from the first frame. Asked every frame, so
                    // an untick snaps the state to 0 (0 s) and a re-tick fades again.
                    let t = ui.ctx().animate_bool_with_time_and_easing(
                        egui::Id::new("review-export-ready"),
                        gate.is_ok(),
                        if gate.is_ok() {
                            super::motion::fade_dur(ui.ctx(), 0.15)
                        } else {
                            0.0
                        },
                        egui::emath::easing::cubic_out,
                    );
                    match &gate {
                        Ok(_) => {
                            let v = ui.visuals();
                            let fill = v
                                .widgets
                                .inactive
                                .weak_bg_fill
                                .lerp_to_gamma(v.selection.bg_fill, t);
                            if ui.add(button.fill(fill)).clicked() {
                                act = Some(Act::Export);
                            }
                        }
                        Err(why) => {
                            ui.add_enabled(false, button).on_disabled_hover_text(why);
                            ui.colored_label(RED, why);
                        }
                    }
                });
            });
        });
    // `columns` sizes to max_rect, which still reaches under the footer; bound it to the space left above it.
    ui.allocate_ui(ui.available_size(), |ui| {
        ui.columns(if also_on { 4 } else { 3 }, |cols| {
            column(&mut cols[0], "Blockers", |ui| {
                if blocks.is_empty() {
                    ui.weak("None");
                }
                for b in &blocks {
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(RED, &b.text);
                        // A project-wide issue has no set to go to.
                        if !b.set_key.is_empty() && ui.button("Go to set").clicked() {
                            act = Some(Act::GoTo(b.set_key.clone()));
                        }
                    });
                }
            });
            column(&mut cols[1], "Undecided", |ui| {
                if undecided.is_empty() {
                    ui.weak("None");
                }
                for k in &undecided {
                    if let Some(p) = undecided_row(ui, m, k, off.as_deref()) {
                        act = Some(Act::Row(p));
                    }
                }
                let accept = ui
                    .add_enabled(
                        any_suggestion && off.is_none(),
                        egui::Button::new("Accept all suggestions"),
                    )
                    .on_hover_text(
                        "Decide every texture that has a suggestion; the others stay listed",
                    )
                    .on_disabled_hover_text(
                        off.as_deref()
                            .unwrap_or("No undecided texture has a suggestion"),
                    );
                if accept.clicked() {
                    act = Some(Act::AcceptAll);
                }
            });
            column(&mut cols[2], "Warnings", |ui| {
                if asks.is_empty() {
                    ui.weak("None");
                }
                let mut set = None;
                for a in &asks {
                    // Grouped by set: `export_asks` lists each set's pitfalls together.
                    if set != Some(&a.set_id) {
                        set = Some(&a.set_id);
                        ui.colored_label(AMBER, &a.set_id);
                    }
                    let mut on = review.acknowledged.contains(&a.text);
                    if ui.checkbox(&mut on, &a.text).changed() {
                        act = Some(Act::Tick(a.text.clone(), on));
                    }
                }
                if !asks.is_empty() && ui.button("Acknowledge all").clicked() {
                    act = Some(Act::AckAll);
                }
                if others > 0
                    && ui
                        .link(format!(
                            "{others} other warning{} — see Problems",
                            plural(others)
                        ))
                        .clicked()
                {
                    act = Some(Act::Others);
                }
            });
            if also_on {
                column(&mut cols[3], "Also export as", |ui| {
                    also_section(ui, m, also_err.as_deref(), &mut act)
                });
            }
        })
    });
    match act {
        Some(Act::AlsoTick(t, on)) => model.set_also_tick(t, on),
        Some(Act::AlsoSet(f)) => model.set_also_settings(f),
        Some(Act::AlsoLocation) => {
            model.pick(ui.ctx(), PickerPurpose::AlsoLocation, PickerSpec::folder());
        }
        Some(Act::Skip(id, on)) => model.set_skip_also(&id, on),
        Some(Act::Recipe) => {
            super::advanced::open_recipe(ui.ctx());
            model.settings.advanced_open = true;
            model.save_settings();
        }
        Some(Act::GoTo(k)) => {
            model.select_set(&k);
            super::header::set_meshes_view(ui.ctx(), false);
        }
        Some(Act::Row(p)) => apply(model, p),
        Some(Act::AcceptAll) => model.accept_all_suggestions(),
        Some(Act::Tick(t, on)) => model.acknowledge_ask(&t, on),
        Some(Act::AckAll) => model.acknowledge_all_asks(),
        Some(Act::Others) => model.open_problems(None),
        Some(Act::Json(overwrite)) => model.answer_review_json(overwrite),
        // Export saves first: wait for a focused field to commit, as the header Export does (Note BS14).
        Some(Act::Export) => {
            if !super::header::field_focused(ui, model, "Export") {
                model.export_from_review();
            }
        }
        Some(Act::Close) => model.close_review(),
        None => {}
    }
}
