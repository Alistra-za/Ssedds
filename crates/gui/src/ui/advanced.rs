//! The right-hand Advanced drawer (M3a spec §2.5): the preset inspector, batch, legacy project-wide fills and preview settings,
//! the rarely changed texture fields, and the `[files]` entries that match nothing. Drawing only: every edit is a
//! call on the model, applied once the drawing is over.
use super::columns::{strip_button, AMBER, SETS_MIN, STRIP_WIDTH, TEXTURES_MIN};
use super::settings_panel::{field_row, toml_read_only_reason, Ctx, RowEdit};
use crate::model::fields::{advanced_field_views, levels_view, provenance_text};
use crate::model::inspector::{inspector_rows, InspectorRow};
use crate::model::picker::{self, PickerKind, PickerPurpose, PickerSpec};
use crate::model::{AppModel, Selection};
use crate::settings::{Bc1Choice, ChecksSettings, PREVIEW_SIZES};
use eframe::egui;
use engine::derive::recipe::LegacyParams;
use engine::policy::ResolvedJob;
use serde_json::Value;

/// The drawer's narrowest width.
const ADV_MIN: f32 = 260.0;

#[derive(Debug, Default)]
pub struct AdvancedState {
    pub only_differences: bool,
}

/// What a control asks of the model.
enum Action {
    SetField(String, &'static str, Value),
    ClearField(String, &'static str),
    ClearLegacyFills,
    RemoveUnmatched(String),
    PreviewMax(u32),
    Allow16k(bool),
    Live(bool),
    Bc1(Bc1Choice),
    ReduceMotion(bool),
    NvttPath(Option<std::path::PathBuf>),
    Checks(ChecksSettings),
    CheckAllUv,
    CancelCheckAllUv,
    AlsoRecipe(LegacyParams),
}

/// K10: Check-all progress epochs are disjoint from the export's `drawer_epoch`, so the two bars never share one
/// `shown_progress` animation.
const UV_EPOCH: u64 = 1 << 63;

/// The drawer: a 24 px "Advanced" strip while closed, the panel while `settings.advanced_open`.
pub fn draw(ui: &mut egui::Ui, model: &mut AppModel, state: &mut AdvancedState) {
    if !model.settings.advanced_open {
        egui::Panel::right("advanced-strip")
            .resizable(false)
            .exact_size(STRIP_WIDTH)
            .frame(egui::Frame::new().fill(ui.visuals().panel_fill))
            .show(ui, |ui| {
                if strip_button(ui, "Advanced")
                    .on_hover_text("Open the Advanced drawer")
                    .clicked()
                {
                    model.settings.advanced_open = true;
                    model.save_settings();
                }
            });
        return;
    }
    let mut close = false;
    // Drawn before the columns: it leaves them their minimums and the centre its floor (K8, rule §2.3).
    let max = ui.available_width() - TEXTURES_MIN - SETS_MIN - super::centre_floor(model);
    let panel = egui::Panel::right("advanced")
        .default_size(380.0)
        .size_range(ADV_MIN..=max.max(ADV_MIN));
    // The user's width survives the cap (a narrow window, an expanded preview) and is what gui.toml saves.
    super::chosen_width_panel(ui, "advanced", 380.0, panel, |ui| {
        ui.horizontal(|ui| {
            ui.heading("Advanced");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                close = ui.button("Close").clicked();
            });
        });
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| body(ui, model, state));
    });
    if close {
        model.settings.advanced_open = false;
        model.save_settings();
    }
}

/// The drawer's sections (also the set panel's Advanced tab, layout v2 C6); applies its edits once they are drawn.
pub fn body(ui: &mut egui::Ui, model: &mut AppModel, state: &mut AdvancedState) {
    let mut actions = vec![];
    inspector(ui, model, state);
    batch(ui, model, &mut actions);
    nvtt(ui, model, &mut actions);
    legacy_fills(ui, model, &mut actions);
    preview_settings(ui, model, &mut actions);
    interface(ui, model, &mut actions);
    checks(ui, model, &mut actions);
    also_recipe(ui, model, &mut actions);
    mipmaps(ui, model, &mut actions);
    texture_fields(ui, model, &mut actions);
    unmatched(ui, model, &mut actions);
    for a in actions {
        match a {
            Action::SetField(k, f, v) => model.set_field(&k, f, v),
            Action::ClearField(k, f) => model.clear_field(&k, f),
            Action::ClearLegacyFills => model.clear_legacy_fills(),
            Action::RemoveUnmatched(k) => model.remove_unmatched(&k),
            Action::PreviewMax(n) => model.set_preview_max(n),
            Action::Allow16k(on) => model.set_allow_16k_preview(on),
            Action::Live(on) => model.set_live(on),
            Action::Bc1(c) => model.set_bc1_choice(c),
            Action::ReduceMotion(on) => model.set_reduce_motion(on),
            Action::NvttPath(p) => model.set_nvtt_path(p),
            Action::Checks(c) => model.set_checks(c),
            Action::CheckAllUv => model.check_all_uv(),
            Action::CancelCheckAllUv => model.cancel_check_all_uv(),
            Action::AlsoRecipe(p) => model.set_also_settings(|c| c.legacy = p),
        }
    }
}

/// The selected texture's key and resolved job, when it has one.
fn selected_job(model: &AppModel) -> Option<(String, ResolvedJob)> {
    // F3a: the drawer edits one texture; with several selected it waits for one.
    if model.in_bulk() {
        return None;
    }
    let Selection::Item(key) = &model.selection else {
        return None;
    };
    let job = model.item(key)?.job.clone()?;
    Some((key.clone(), job))
}

fn value_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn section(ui: &mut egui::Ui, name: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::CollapsingHeader::new(egui::RichText::new(name).strong())
        .default_open(true)
        .show(ui, body);
}

fn inspector(ui: &mut egui::Ui, model: &AppModel, state: &mut AdvancedState) {
    section(ui, "Preset inspector", |ui| {
        ui.label(format!("Batch preset: {}", model.batch_preset));
        if model.loaded_user_presets.is_empty() {
            ui.label(
                egui::RichText::new(
                    "No user presets (add .toml files to %APPDATA%\\sdc\\presets).",
                )
                .small()
                .weak(),
            );
        } else {
            ui.label("User presets:");
            for (id, file) in &model.loaded_user_presets {
                ui.label(egui::RichText::new(format!("{id} — {file}")).small());
            }
        }
        for (file, reason) in &model.skipped_user_presets {
            ui.label(
                egui::RichText::new(format!("Skipped: {file} — {reason}"))
                    .small()
                    .color(ui.visuals().warn_fg_color),
            );
        }
        ui.label(
            egui::RichText::new(
                "Built-in presets are read-only here. Change a texture in the centre panel.",
            )
            .small()
            .weak(),
        );
        let (Some((_, job)), Some(session)) = (selected_job(model), model.session.as_ref()) else {
            ui.weak("Select a texture to see how its settings were resolved.");
            return;
        };
        ui.checkbox(&mut state.only_differences, "Show only differences");
        let rows: Vec<InspectorRow> = inspector_rows(&job, session.engine().registry())
            .into_iter()
            .filter(|r| !state.only_differences || r.differs_from_base)
            .collect();
        if rows.is_empty() {
            ui.weak("Nothing differs from the base preset.");
            return;
        }
        egui::Grid::new("inspector")
            .num_columns(3)
            .striped(true)
            .show(ui, |ui| {
                for r in rows {
                    let text = |s: String| {
                        if r.differs_from_base {
                            egui::RichText::new(s).strong()
                        } else {
                            egui::RichText::new(s)
                        }
                    };
                    ui.label(text(r.field.clone()));
                    ui.label(text(value_text(&r.value)));
                    ui.label(
                        egui::RichText::new(provenance_text(&r.provenance))
                            .small()
                            .weak(),
                    );
                    ui.end_row();
                }
            });
    });
}

fn bc1_label(c: Bc1Choice) -> &'static str {
    match c {
        Bc1Choice::Ideal => "Ideal",
        Bc1Choice::IdealRound4 => "Ideal (round 4)",
        Bc1Choice::Nvidia => "NVIDIA",
        Bc1Choice::Amd => "AMD",
    }
}

/// A number box; true once its value is settled (a drag ended, or a typed value was accepted).
pub(super) fn settled_drag<T: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    value: &mut T,
    speed: f64,
    range: std::ops::RangeInclusive<T>,
) -> bool {
    let r = ui.add(egui::DragValue::new(value).speed(speed).range(range));
    r.drag_stopped() || (r.changed() && !r.dragged())
}

fn batch(ui: &mut egui::Ui, model: &mut AppModel, actions: &mut Vec<Action>) {
    section(ui, "Batch", |ui| {
        let mut settled = false;
        egui::Grid::new("batch").num_columns(2).show(ui, |ui| {
            ui.label("Worker threads");
            settled |= settled_drag(ui, &mut model.settings.jobs, 0.2, 0..=256);
            ui.end_row();
            ui.label("Memory budget (MiB)");
            settled |= settled_drag(ui, &mut model.settings.mem_budget_mb, 16.0, 0..=1_048_576);
            ui.end_row();
            ui.label("BC1 decoder");
            let current = model.settings.bc1_decoder;
            egui::ComboBox::from_id_salt("bc1-decoder")
                .selected_text(bc1_label(current))
                .show_ui(ui, |ui| {
                    for c in Bc1Choice::ALL {
                        if ui.selectable_label(c == current, bc1_label(c)).clicked() && c != current
                        {
                            actions.push(Action::Bc1(c));
                        }
                    }
                });
            ui.end_row();
        });
        ui.label(
            egui::RichText::new("0 threads = one per core; 0 MiB = half of the physical memory.")
                .small()
                .weak(),
        );
        if model.restart_needed {
            ui.colored_label(AMBER, "restart Tessalith to apply");
        }
        if settled {
            model.save_settings();
        }
    });
}

/// M5 (DECISIONS Note NV): where nvcompress is and what the probe found. NVTT is never bundled.
fn nvtt(ui: &mut egui::Ui, model: &AppModel, actions: &mut Vec<Action>) {
    section(ui, "NVTT", |ui| {
        ui.horizontal(|ui| {
            ui.label("NVTT path");
            let shown = model
                .settings
                .nvtt_path
                .as_ref()
                .map_or("automatic (PATH, then the NVIDIA folder)".into(), |p| {
                    p.display().to_string()
                });
            ui.label(egui::RichText::new(shown).small());
        });
        ui.horizontal(|ui| {
            if picker::button(
                ui,
                &model.picker,
                egui::Button::new("Choose nvcompress.exe…"),
            ) {
                let kind = PickerKind::File {
                    filter: ("nvcompress", &["exe"]),
                };
                let spec = PickerSpec { title: None, kind };
                model.pick(ui.ctx(), PickerPurpose::NvttPath, spec);
            }
            if model.settings.nvtt_path.is_some() && ui.button("Automatic").clicked() {
                actions.push(Action::NvttPath(None));
            }
        });
        ui.label(egui::RichText::new(model.nvtt_status()).small());
    });
}

/// The project-wide fills an older `sdc.toml` may still hold (`defaults.packing.fill.*`). They still apply as a
/// fallback under each packed output's own fills; new fills are set on the packed output's card.
fn legacy_fills(ui: &mut egui::Ui, model: &AppModel, actions: &mut Vec<Action>) {
    let fills = model.legacy_fills();
    if fills.is_empty() {
        return;
    }
    let list: Vec<String> = fills
        .iter()
        .map(|(role, v)| format!("{} {v}", role.id()))
        .collect();
    ui.horizontal_wrapped(|ui| {
        ui.label(format!("Project-wide fills (legacy): {}", list.join(", ")));
        let why = toml_read_only_reason(model);
        let editable = model.project.as_ref().is_some_and(|p| !p.read_only());
        if ui
            .add_enabled(editable, egui::Button::new("Clear"))
            .on_hover_text(
                "Remove the project-wide fills; set fills on each packed output's card instead",
            )
            .on_disabled_hover_text(why.unwrap_or_default())
            .clicked()
        {
            actions.push(Action::ClearLegacyFills);
        }
    });
}

fn preview_settings(ui: &mut egui::Ui, model: &AppModel, actions: &mut Vec<Action>) {
    section(ui, "Preview", |ui| {
        let allow = model.settings.allow_16k_preview;
        let mut size = model.settings.preview_max;
        ui.horizontal(|ui| {
            ui.label("Largest previewed side");
            egui::ComboBox::from_id_salt("preview-size")
                .selected_text(format!("{} px", model.settings.preview_max))
                .show_ui(ui, |ui| {
                    for n in PREVIEW_SIZES.into_iter().filter(|n| *n < 16384 || allow) {
                        ui.selectable_value(&mut size, n, format!("{n} px"));
                    }
                });
        });
        if size != model.settings.preview_max {
            actions.push(Action::PreviewMax(size));
        }
        let mut allow_now = allow;
        if ui
            .checkbox(&mut allow_now, "Allow 16384 previews")
            .on_hover_text(
                "A 16384 px mip needs a very large GPU texture, so it is off unless you ask",
            )
            .changed()
        {
            actions.push(Action::Allow16k(allow_now));
        }
        let mut live = model.settings.live_encode;
        if ui
            .checkbox(&mut live, "Live encode")
            .on_hover_text(
                "Encode the expanded preview after every edit (otherwise use Encode now)",
            )
            .changed()
        {
            actions.push(Action::Live(live));
        }
    });
}

fn interface(ui: &mut egui::Ui, model: &AppModel, actions: &mut Vec<Action>) {
    section(ui, "Interface", |ui| {
        let mut on = model.settings.reduce_motion;
        if ui
            .checkbox(&mut on, "Reduce motion")
            .on_hover_text(
                "Shorter fades and no sliding or scaling: toasts, the Export highlight, progress bars and press feedback",
            )
            .changed()
        {
            actions.push(Action::ReduceMotion(on));
        }
    });
}

/// The artist checks' switches (DECISIONS Note CL). Each box's AccessKit name ends in "check", apart from same-text
/// controls such as the shader's "Parallax" (K22, ui-rules 8.4).
fn checks(ui: &mut egui::Ui, model: &AppModel, actions: &mut Vec<Action>) {
    section(ui, "Checks", |ui| {
        let now = model.settings.checks;
        let mut c = now;
        let boxes: [(&mut bool, &str, &str); 6] = [
            (&mut c.range, "Value range", "Value range check"),
            (&mut c.plastic, "Plastic look", "Plastic look check"),
            (
                &mut c.low_detail,
                "Fake resolution",
                "Fake resolution check",
            ),
            (&mut c.green, "Green direction", "Green direction check"),
            (&mut c.parallax, "Parallax", "Parallax check"),
            (&mut c.uv, "UV coverage", "UV coverage check"),
        ];
        for (on, text, name) in boxes {
            let r = ui.checkbox(on, text);
            let v = *on;
            r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, v, name));
        }
        // K22: without Fake resolution there is nothing to drop.
        ui.add_enabled(
            c.low_detail,
            egui::Checkbox::new(&mut c.drop_top_mip, "Drop mip 0 when it adds nothing"),
        )
        .on_hover_text(
            "Hold textures whose top mip adds almost nothing; choose at the end of the export which to write at half size",
        )
        .on_disabled_hover_text("Turn on Fake resolution first");
        if c != now {
            actions.push(Action::Checks(c));
        }
        check_all_uv(ui, model, actions);
    });
}

/// A number widget over a model value that is only written once it settles: the value in flight lives in egui
/// memory under `id` while it is dragged, so the drag follows the pointer and ends in exactly one edit (one undo
/// step). `Some(value)` on the frame it settles (a drag ended, a click, a typed value accepted).
pub(super) fn held_value(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    current: f64,
    add: impl FnOnce(&mut egui::Ui, &mut f64) -> egui::Response,
) -> (egui::Response, Option<f64>) {
    let id = egui::Id::new(id);
    let mut x = ui.data(|d| d.get_temp::<f64>(id)).unwrap_or(current);
    let r = add(ui, &mut x);
    let settled = r.drag_stopped() || (r.changed() && !r.dragged());
    if r.dragged() {
        ui.data_mut(|d| d.insert_temp(id, x));
    } else {
        ui.data_mut(|d| d.remove::<f64>(id));
    }
    (r, (settled && x != current).then_some(x))
}

/// "Also export recipe" (owner answer 5, C4): one number box per `[export.legacy]` value, ranges as
/// `LegacyParams::check`. P17: Keep metal brightness and its strength are only in Review (with the CM tick), and
/// Dielectric env is one box (0 = off), not a box plus a checkbox.
fn also_recipe(ui: &mut egui::Ui, model: &AppModel, actions: &mut Vec<Action>) {
    section(ui, "Also export recipe", |ui| {
        let off = if model.project.is_none() {
            Some("Open a folder first".to_string())
        } else if model.export.running.is_some() {
            Some(crate::model::also::EXPORT_RUNNING.to_string())
        } else {
            toml_read_only_reason(model)
        };
        let now = model.also_config().legacy;
        let mut p = now;
        let rows: [(&mut f64, &str, &str, f64, f64, &str); 9] = [
            (
                &mut p.brightness,
                "Diffuse brightness",
                "brightness",
                0.0,
                1.5,
                "The vanilla diffuse's brightness factor",
            ),
            (
                &mut p.ao_strength,
                "AO strength",
                "ao_strength",
                0.0,
                1.0,
                "How much AO darkens an object's diffuse",
            ),
            (
                &mut p.ao_strength_landscape,
                "AO strength (landscape)",
                "ao_strength_landscape",
                0.0,
                1.0,
                "How much AO darkens a landscape diffuse",
            ),
            (
                &mut p.curves_opacity,
                "Curves opacity",
                "curves_opacity",
                0.0,
                1.0,
                "Opacity of the curves soft light and the cavity multiply",
            ),
            (
                &mut p.metal_darken,
                "Metal darkening",
                "metal_darken",
                0.0,
                1.0,
                "The vanilla diffuse's factor at full metal",
            ),
            (
                &mut p.spec_scale,
                "Specular scale",
                "spec_scale",
                0.0,
                2.0,
                "Scale of the specular in the _n alpha",
            ),
            (
                &mut p.spec_metal_boost,
                "Specular boost at full metal",
                "spec_metal_boost",
                1.0,
                3.0,
                "Boost of the _n alpha at full metal",
            ),
            (
                &mut p.landscape_spec,
                "Landscape spec factor",
                "landscape_spec",
                0.0,
                5.0,
                "Landscape _n alpha factor, applied before saturation",
            ),
            (
                &mut p.cm_dielectric_env,
                "Dielectric env from gloss",
                "cm_dielectric_env",
                0.0,
                1.0,
                "0 = off; the spec's option is 0.35",
            ),
        ];
        let mut settled = false;
        ui.add_enabled_ui(off.is_none(), |ui| {
            egui::Grid::new("also-recipe")
                .num_columns(2)
                .show(ui, |ui| {
                    // The number box first: in a narrow drawer the text is cut, never the box.
                    for (v, label, key, lo, hi, hover) in rows {
                        let (r, done) = held_value(ui, ("also-recipe", key), *v, |ui, x| {
                            ui.add(
                                egui::DragValue::new(x)
                                    .speed(0.01)
                                    .range(lo..=hi)
                                    .max_decimals(3)
                                    .update_while_editing(false),
                            )
                        });
                        let shown = r.enabled();
                        r.widget_info(|| {
                            let mut i = egui::WidgetInfo::drag_value(shown, *v);
                            i.label = Some(label.to_string());
                            i
                        });
                        if let Some(x) = done {
                            *v = x;
                            settled = true;
                        }
                        // Built only while hovered (ui-rules 9.3).
                        let tip = |ui: &mut egui::Ui| {
                            ui.label(format!("{hover} (sdc.toml export.legacy.{key})"));
                        };
                        r.on_hover_ui(tip);
                        ui.horizontal(|ui| {
                            ui.label(label);
                            ui.label(egui::RichText::new(key).small().weak());
                        })
                        .response
                        .on_hover_ui(tip);
                        ui.end_row();
                    }
                });
        })
        .response
        .on_disabled_hover_text(off.as_deref().unwrap_or_default());
        if settled && p != now {
            actions.push(Action::AlsoRecipe(p));
        }
    });
}

/// "Check all UV coverage" for the open folder, or while it runs its bar (gliding only through `shown_progress`,
/// CF12, with its own epoch, K10) and the button that stops it. Never greyed by `busy()` (owner answer 8).
fn check_all_uv(ui: &mut egui::Ui, model: &AppModel, actions: &mut Vec<Action>) {
    if let (Some((done, total)), Some(run)) = (model.uv.all_progress(), model.uv.all_run()) {
        let frac = if total == 0 {
            0.0
        } else {
            done as f32 / total as f32
        };
        let bar = ui.add(
            egui::ProgressBar::new(super::header::shown_progress(
                ui.ctx(),
                UV_EPOCH | run,
                frac,
            ))
            .text(format!("{done} / {total}"))
            .animate(false),
        );
        // Review M3: the visible "3 / 72" alone names nothing.
        bar.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::ProgressIndicator,
                true,
                format!("Checking UV coverage: {done} of {total}"),
            )
        });
        // After Stop, the reads in flight still finish (a big NIF can take seconds).
        if model.uv.all_stopping() {
            ui.add_enabled(false, egui::Button::new("Stopping…"));
        } else if ui.button("Stop checking UV coverage").clicked() {
            actions.push(Action::CancelCheckAllUv);
        }
        return;
    }
    let offered = model.uv_check_all_offered();
    if ui
        .add_enabled(offered.is_ok(), egui::Button::new("Check all UV coverage"))
        .on_hover_text("Read the UV coverage of every colour texture in this folder from the meshes in your scan")
        .on_disabled_hover_text(offered.err().unwrap_or_default())
        .clicked()
    {
        actions.push(Action::CheckAllUv);
    }
}

/// The mip level count (Full chain, None or 2..N levels); the basic panel only has the on/off tick box.
fn mipmaps(ui: &mut egui::Ui, model: &AppModel, actions: &mut Vec<Action>) {
    section(ui, "Mipmaps", |ui| {
        let Some((key, job)) = selected_job(model) else {
            ui.weak("Select a texture to edit its mip level count.");
            return;
        };
        let read_only = model.project.as_ref().is_none_or(|p| p.read_only());
        let why = toml_read_only_reason(model);
        let f = levels_view(model.target, &job);
        ui.add_enabled_ui(!read_only, |ui| {
            egui::Grid::new(("advanced-mips", &key))
                .num_columns(4)
                .show(ui, |ui| {
                    let cx = Ctx {
                        key: &key,
                        job: &job,
                        target: model.target,
                        busy: model.busy(),
                        compact: false,
                        toml_read_only: why.clone(),
                        warnings: &[],
                        nvtt_found: model.nvtt_found(),
                    };
                    match field_row(ui, &cx, &f) {
                        Some(RowEdit::Set(v)) => {
                            actions.push(Action::SetField(key.clone(), f.field, v))
                        }
                        Some(RowEdit::Clear) => {
                            actions.push(Action::ClearField(key.clone(), f.field))
                        }
                        None => {}
                    }
                });
        });
    });
}

fn texture_fields(ui: &mut egui::Ui, model: &AppModel, actions: &mut Vec<Action>) {
    section(ui, "Advanced texture fields", |ui| {
        let Some((key, job)) = selected_job(model) else {
            ui.weak("Select a texture to edit its encoder and header fields.");
            return;
        };
        let read_only = model.project.as_ref().is_none_or(|p| p.read_only());
        let busy = model.busy();
        let why = toml_read_only_reason(model);
        ui.add_enabled_ui(!read_only, |ui| {
            egui::Grid::new(("advanced-fields", &key))
                .num_columns(4)
                .show(ui, |ui| {
                    let cx = Ctx {
                        key: &key,
                        job: &job,
                        target: model.target,
                        busy,
                        compact: false,
                        toml_read_only: why.clone(),
                        warnings: &[],
                        nvtt_found: model.nvtt_found(),
                    };
                    for f in advanced_field_views(model.target, &job) {
                        match field_row(ui, &cx, &f) {
                            Some(RowEdit::Set(v)) => {
                                actions.push(Action::SetField(key.clone(), f.field, v))
                            }
                            Some(RowEdit::Clear) => {
                                actions.push(Action::ClearField(key.clone(), f.field))
                            }
                            None => {}
                        }
                    }
                });
        });
    });
}

fn unmatched(ui: &mut egui::Ui, model: &AppModel, actions: &mut Vec<Action>) {
    section(ui, "Unmatched [files] entries", |ui| {
        if model.unmatched_keys.is_empty() {
            ui.weak("Every [files] entry in sdc.toml matches a file.");
            return;
        }
        ui.label(
            egui::RichText::new(
                "These entries name no file under the current target and have no effect.",
            )
            .small()
            .weak(),
        );
        let editable = model.project.as_ref().is_some_and(|p| !p.read_only());
        for k in &model.unmatched_keys {
            ui.horizontal(|ui| {
                ui.label(k);
                if ui
                    .add_enabled(editable, egui::Button::new("Remove"))
                    .clicked()
                {
                    actions.push(Action::RemoveUnmatched(k.clone()));
                }
            });
        }
    });
}
