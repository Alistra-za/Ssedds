//! The header (M3a spec §2.1, Note BH): two rows of controls, then Messages over Save and the two-row Export at the
//! right edge; then the status line and banner.
use crate::model::picker::{self, PickerPurpose, PickerSpec};
use crate::model::AppModel;
use eframe::egui;
use engine::config::Target;

/// The button that opens the PGPatcher JSON editor (Note BH7).
pub const JSON_EDITOR: &str = "PG JSON Editor";
/// Export's width: twice what it was (Note BH3).
const EXPORT_WIDTH: f32 = 240.0;
/// The widest the Export folder button gets; a longer path is cut with "…".
const EXPORT_FOLDER_MAX: f32 = 280.0;
/// The narrowest the Export folder button is cut to before it starts a line of its own.
const EXPORT_FOLDER_MIN: f32 = 120.0;
/// Why the export folder cannot change now.
pub const EXPORT_RUNNING_HOVER: &str = "An export is running: change the folder after it ends";

fn target_name(t: Target) -> &'static str {
    match t {
        Target::Cs => "cs",
        Target::Vanilla => "vanilla",
    }
}

fn target_help(t: Target) -> &'static str {
    match t {
        Target::Cs => "Community Shaders: TruePBR sets and its packed maps",
        Target::Vanilla => {
            "The base game's shaders and texture layout, plus complex material and complex grass"
        }
    }
}

/// `Tessalith — <folder>`, plus ` ●` when there are unsaved edits.
pub fn window_title(model: &AppModel) -> String {
    let mut t = match &model.root {
        Some(r) => format!("{} — {}", crate::PRODUCT_NAME, r.display()),
        None => crate::PRODUCT_NAME.to_string(),
    };
    if model.is_dirty() {
        t.push_str(" ●");
    }
    t
}

/// Send the title only when it changes.
fn set_title(ctx: &egui::Context, title: String) {
    let id = egui::Id::new("sdc-window-title");
    let same = ctx.data(|d| d.get_temp::<String>(id).is_some_and(|t| t == title));
    if !same {
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
        ctx.data_mut(|d| d.insert_temp(id, title));
    }
}

pub fn draw(
    ui: &mut egui::Ui,
    model: &mut AppModel,
    scan: Option<&mut crate::model::meshes::MeshesModel>,
) {
    set_title(ui.ctx(), window_title(model));
    egui::Panel::top("header").show(ui, |ui| {
        // Note BH: two rows on the left; Messages over Save, then Export, anchored right at the rows' full height.
        let gap = ui.spacing().item_spacing;
        let stack_w =
            button_width(ui, &super::messages::button_text(model)).max(button_width(ui, "Save"));
        let right_w = stack_w + gap.x + EXPORT_WIDTH;
        ui.horizontal_top(|ui| {
            let left_w = (ui.available_width() - right_w - gap.x).max(0.0);
            let left = ui.allocate_ui_with_layout(
                egui::vec2(left_w, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(left_w);
                    // AG126: each row wraps in a narrow window instead of drawing controls on top of each other. A
                    // group that is one nested layout (a combo, a disabled scope) is moved whole by `fit`.
                    ui.horizontal_wrapped(|ui| top_row(ui, model, scan));
                    ui.horizontal_wrapped(|ui| bottom_row(ui, model));
                },
            );
            // Two rows tall at least; as tall as the rows when a narrow window wraps them.
            let h = left
                .response
                .rect
                .height()
                .max(2.0 * ui.spacing().interact_size.y + gap.y);
            ui.allocate_ui_with_layout(
                egui::vec2(stack_w, h),
                egui::Layout::top_down(egui::Align::Center),
                |ui| {
                    let size = egui::vec2(stack_w, (h - gap.y) / 2.0);
                    super::messages::button(ui, model, size);
                    if ui
                        .add_enabled(model.is_dirty(), egui::Button::new("Save").min_size(size))
                        .clicked()
                    {
                        // AG23, trace-debug #54: as Ctrl+S, wait for a focused field to commit.
                        if !field_focused(ui, model, "Save") {
                            // A failed save raises its own dialog.
                            let _ = model.save();
                        }
                    }
                },
            );
            export_button(ui, model, egui::vec2(EXPORT_WIDTH, h));
        });
    });
    status(ui, model);
}

/// Open folder…, Recent, Undo, Redo, MO2… with Mode, Preset, Presets…, then the export folder at the right (BH12).
fn top_row(
    ui: &mut egui::Ui,
    model: &mut AppModel,
    scan: Option<&mut crate::model::meshes::MeshesModel>,
) {
    if picker::button(ui, &model.picker, egui::Button::new("Open folder…")) {
        model.pick(ui.ctx(), PickerPurpose::OpenFolder, PickerSpec::folder());
    }
    recent_menu(ui, model);
    undo_redo(ui, model);
    ui.separator();
    mode_combo(ui, model, scan);
    preset_combo(ui, model);
    fit(ui, button_width(ui, "Presets…"));
    if ui
        .button("Presets…")
        .on_hover_text("Make and edit your own presets (saved in %APPDATA%\\sdc\\presets)")
        .clicked()
    {
        model.open_presets();
    }
    export_folder_button(ui, model);
}

/// The column eye toggles at the far left, the view tabs centred in the space left (BH14), then PG JSON Editor at the
/// right (BH12).
fn bottom_row(ui: &mut egui::Ui, model: &mut AppModel) {
    column_toggles(ui, model);
    let right = if shows_json_editor(model) {
        button_width(ui, JSON_EDITOR) + ui.spacing().item_spacing.x
    } else {
        0.0
    };
    let tabs = view_tabs_width(ui);
    let free = room(ui) - right;
    if free > tabs {
        ui.add_space((free - tabs) / 2.0);
    }
    view_toggle(ui);
    json_editor_button(ui, model);
}

/// What is left of this line of a wrapped row.
fn room(ui: &egui::Ui) -> f32 {
    ui.max_rect().right() - ui.cursor().min.x
}

/// Moves the cursor so a `w`-wide widget ends at the row's right edge, against the Messages/Save stack (BH12).
fn push_right(ui: &mut egui::Ui, w: f32) {
    let s = room(ui) - w - 0.5;
    if s > 0.0 {
        ui.add_space(s);
    }
}

/// True when a text field has the keyboard focus: `what` (Save, Export) then does nothing and says why, so the
/// field commits its typed value on the focus loss first (AG23, trace-debug #54).
fn field_focused(ui: &egui::Ui, model: &mut AppModel, what: &str) -> bool {
    if !ui.ctx().egui_wants_keyboard_input() {
        return false;
    }
    model.status_line = format!("{what}: leave the text field first");
    // The commit's plan replaces the status line at once; the log keeps it.
    model.messages.log(
        crate::model::messages::Severity::Info,
        model.status_line.clone(),
    );
    true
}

/// Export, `size` big. While an export runs its lower half is the progress bar (done/total; the file on hover).
fn export_button(ui: &mut egui::Ui, model: &mut AppModel, size: egui::Vec2) {
    let can_export = can_export(model);
    let running = model.export.running.is_some();
    let gap = ui.spacing().item_spacing.y;
    let button_h = if running {
        (size.y - gap) / 2.0
    } else {
        size.y
    };
    let mut export = egui::Button::new(egui::RichText::new("Export").strong())
        .min_size(egui::vec2(size.x, button_h));
    if can_export {
        export = export.fill(ui.visuals().selection.bg_fill);
    }
    ui.allocate_ui_with_layout(size, egui::Layout::top_down(egui::Align::Center), |ui| {
        // Export saves first: like Save, wait for a focused field to commit (Note BS14).
        if ui.add_enabled(can_export, export).clicked() && !field_focused(ui, model, "Export") {
            model.export_clicked();
        }
        if running {
            let (done, total) = model.export.progress();
            let frac = if total == 0 {
                0.0
            } else {
                done as f32 / total as f32
            };
            let hover = match model.export.current_file() {
                Some(f) => format!("Exporting {f}. Cancel is in the Files tab."),
                None => "Exporting. Cancel is in the Files tab.".to_string(),
            };
            ui.add(
                egui::ProgressBar::new(frac)
                    .desired_width(size.x)
                    .desired_height(size.y - gap - button_h)
                    .text(format!("{done}/{total}"))
                    .animate(false),
            )
            .on_hover_text(hover);
            // The drawer also asks; the bar must move even with the drawer closed.
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(500));
        }
    });
}

/// PG JSON Editor is offered: cs mode, a folder open and its `sdc-project.toml` loaded.
fn shows_json_editor(model: &AppModel) -> bool {
    model.target == Target::Cs && model.root.is_some() && model.sets_doc.is_some()
}

/// The PGPatcher JSON editor (CS only, with a folder open); the JSON name is its top field (Note BH7).
fn json_editor_button(ui: &mut egui::Ui, model: &mut AppModel) {
    if !shows_json_editor(model) {
        return;
    }
    let w = button_width(ui, JSON_EDITOR);
    fit(ui, w);
    push_right(ui, w);
    if ui
        .button(JSON_EDITOR)
        .on_hover_text(
            "Edit the whole TruePBR JSON: its file name, every set's entry and the delete sections",
        )
        .clicked()
    {
        model.json_editor_open = true;
    }
}

/// Starts a new line of a wrapped row unless `width` fits in what is left of this one. A nested layout (a combo, a
/// scope) is placed where the cursor is; only single widgets wrap by themselves.
fn fit(ui: &mut egui::Ui, width: f32) {
    if ui.max_rect().right() - ui.cursor().min.x < width {
        ui.end_row();
    }
}

/// The width a button with `text` takes.
fn button_width(ui: &egui::Ui, text: &str) -> f32 {
    let galley = egui::WidgetText::from(text).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Button,
    );
    galley.size().x + 2.0 * ui.spacing().button_padding.x
}

/// Which of the Textures and Sets columns are shown at all (BH15: eye toggles). Each column's own arrow collapses it
/// to a strip.
fn column_toggles(ui: &mut egui::Ui, model: &mut AppModel) {
    let st = &mut model.settings;
    let mut textures = !st.textures_hidden;
    let mut sets = !st.sets_hidden;
    let mut changed = eye_toggle(ui, &mut textures, "Textures").changed();
    changed |= eye_toggle(ui, &mut sets, "Sets").changed();
    if changed {
        st.textures_hidden = !textures;
        st.sets_hidden = !sets;
        model.save_settings();
    }
}

/// An eye and `text`: open while the `text` column shows, struck through while it is hidden. To accessibility it is a
/// check box named "<text> column" (what tests and screen readers find); the hover says what a click does.
fn eye_toggle(ui: &mut egui::Ui, on: &mut bool, text: &str) -> egui::Response {
    let galley = egui::WidgetText::from(text).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Button,
    );
    let pad = ui.spacing().button_padding.x;
    let size = egui::vec2(
        pad + EYE_W + 4.0 + galley.size().x + pad,
        ui.spacing().interact_size.y,
    );
    let (rect, mut r) = ui.allocate_exact_size(size, egui::Sense::click());
    if r.clicked() {
        *on = !*on;
        r.mark_changed();
    }
    let label = format!("{text} column");
    let enabled = ui.is_enabled();
    let shown = *on;
    r.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, shown, &label)
    });
    if ui.is_rect_visible(rect) {
        let v = ui.style().interact(&r);
        if r.hovered() || r.has_focus() {
            ui.painter().rect(
                rect,
                v.corner_radius,
                v.weak_bg_fill,
                v.bg_stroke,
                egui::StrokeKind::Inside,
            );
        }
        let color = if shown {
            v.text_color()
        } else {
            ui.visuals().weak_text_color()
        };
        let eye = egui::pos2(rect.left() + pad + EYE_W / 2.0, rect.center().y);
        paint_eye(ui.painter(), eye, color, shown);
        let at = egui::pos2(
            rect.left() + pad + EYE_W + 4.0,
            rect.center().y - galley.size().y / 2.0,
        );
        ui.painter().galley(at, galley, color);
    }
    let verb = if shown { "Hide" } else { "Show" };
    r.on_hover_text(format!("{verb} the {text} column"))
}

/// The eye's width.
const EYE_W: f32 = 14.0;

/// An eye centred on `c`: a lens outline and a pupil when `open`, a lens struck through when not.
fn paint_eye(painter: &egui::Painter, c: egui::Pos2, color: egui::Color32, open: bool) {
    let (rx, ry) = (EYE_W / 2.0, 4.5);
    let n = 12;
    let edge = |u: f32, sign: f32| {
        egui::pos2(
            c.x - rx + 2.0 * rx * u,
            c.y + sign * ry * (std::f32::consts::PI * u).sin(),
        )
    };
    let lens: Vec<egui::Pos2> = (0..=n)
        .map(|i| edge(i as f32 / n as f32, -1.0))
        .chain((1..n).map(|i| edge(1.0 - i as f32 / n as f32, 1.0)))
        .collect();
    painter.add(egui::Shape::closed_line(
        lens,
        egui::Stroke::new(1.3, color),
    ));
    if open {
        painter.circle_filled(c, 2.3, color);
    } else {
        painter.line_segment(
            [
                c + egui::vec2(-rx + 1.0, ry + 1.0),
                c + egui::vec2(rx - 1.0, -ry - 1.0),
            ],
            egui::Stroke::new(1.6, color),
        );
    }
}

fn recent_menu(ui: &mut egui::Ui, model: &mut AppModel) {
    let mut open = None;
    fit(ui, button_width(ui, "Recent"));
    ui.add_enabled_ui(!model.settings.recent.is_empty(), |ui| {
        ui.menu_button("Recent", |ui| {
            for p in &model.settings.recent {
                if ui.button(p.display().to_string()).clicked() {
                    open = Some(p.clone());
                    ui.close();
                }
            }
        });
    });
    if let Some(p) = open {
        model.request_open(p);
    }
}

/// Export can start: a folder is open, no export runs, and `sdc.toml` is not ignored (M3a spec §7.4: an export would
/// not be the project's settings).
pub fn can_export(model: &AppModel) -> bool {
    model.can_export()
}

/// The Undo and Redo buttons (item 2): disabled with no history, the step's label in the hover text.
fn undo_redo(ui: &mut egui::Ui, model: &mut AppModel) {
    for (undo, name) in [(true, "Undo"), (false, "Redo")] {
        let label = if undo {
            model.undo_label()
        } else {
            model.redo_label()
        };
        let hover = match label {
            Some(l) => format!(
                "{name}: {l}  ({})",
                super::actions::keys_text(if undo { "undo" } else { "redo" })
            ),
            None => format!("Nothing to {}", name.to_lowercase()),
        };
        let r = ui.add_enabled(label.is_some(), egui::Button::new(name));
        if r.on_hover_text(hover.clone())
            .on_disabled_hover_text(hover)
            .clicked()
        {
            if undo {
                model.undo();
            } else {
                model.redo();
            }
        }
    }
}

/// MO2… and Mode as one framed unit (Note BH6); MO2… only where the app has a scan model.
fn mode_combo(
    ui: &mut egui::Ui,
    model: &mut AppModel,
    scan: Option<&mut crate::model::meshes::MeshesModel>,
) {
    let gap = ui.spacing().item_spacing.x;
    let mo2 = if scan.is_some() {
        button_width(ui, "MO2…") + gap
    } else {
        0.0
    };
    fit(
        ui,
        mo2 + button_width(ui, "Mode") + gap + ui.spacing().combo_width + 8.0,
    );
    egui::Frame::new()
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(3, 0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(m) = scan {
                    super::meshes::mo2_button(ui, m);
                }
                mode_picker(ui, model);
            });
        });
}

fn mode_picker(ui: &mut egui::Ui, model: &mut AppModel) {
    ui.label("Mode");
    let mut picked = model.target;
    ui.add_enabled_ui(model.project.is_some(), |ui| {
        egui::ComboBox::from_id_salt("mode")
            .selected_text(target_name(model.target))
            .show_ui(ui, |ui| {
                for t in [Target::Cs, Target::Vanilla] {
                    ui.selectable_value(&mut picked, t, target_name(t))
                        .on_hover_text(target_help(t));
                }
            })
            .response
            .on_hover_text(
                "The target the textures are converted for: cs or vanilla. Saved in sdc.toml as target.",
            );
    });
    if picked != model.target {
        model.set_target(picked);
    }
}

/// The project's preset (AM12): the target's default, or a preset of this target (a user preset appears once saved).
fn preset_combo(ui: &mut egui::Ui, model: &mut AppModel) {
    let default = format!(
        "default ({})",
        engine::presets::default_preset_for(model.target)
    );
    let cur = model.project_preset();
    let gap = ui.spacing().item_spacing.x;
    fit(
        ui,
        button_width(ui, "Preset") + gap + ui.spacing().combo_width,
    );
    ui.label("Preset");
    let mut picked = None;
    ui.add_enabled_ui(model.project.is_some(), |ui| {
        egui::ComboBox::from_id_salt("preset")
            .selected_text(cur.clone().unwrap_or_else(|| default.clone()))
            .show_ui(ui, |ui| {
                if ui.selectable_label(cur.is_none(), &default).clicked() {
                    picked = Some(None);
                }
                for id in model.preset_choices() {
                    if ui
                        .selectable_label(cur.as_deref() == Some(id.as_str()), &id)
                        .clicked()
                    {
                        picked = Some(Some(id));
                    }
                }
            })
            .response
            .on_hover_text("The preset the folder converts with. Saved in sdc.toml as preset.");
    });
    if let Some(p) = picked.filter(|p| *p != cur) {
        model.set_preset(p);
    }
}

/// Top-level view toggle (plan A Task 12): "Textures | Meshes", kept in egui memory for the session. Note BH8: two
/// tabs, taller than the row, rounded on top only and set apart by space; the selected one is filled and bold.
pub fn view_toggle(ui: &mut egui::Ui) {
    let mut meshes = meshes_view_active(ui.ctx());
    let tab = |ui: &mut egui::Ui, on: bool, text: &str| {
        let text = egui::RichText::new(text);
        let w = button_width(ui, "Textures view") + 12.0;
        let h = ui.spacing().interact_size.y + 6.0;
        ui.add(
            egui::Button::selectable(on, if on { text.strong() } else { text })
                .frame_when_inactive(true)
                .corner_radius(egui::CornerRadius {
                    nw: 6,
                    ne: 6,
                    sw: 0,
                    se: 0,
                })
                .min_size(egui::vec2(w, h)),
        )
    };
    fit(ui, view_tabs_width(ui));
    ui.add_space(16.0);
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        if tab(ui, !meshes, "Textures view").clicked() {
            meshes = false;
        }
        if tab(ui, meshes, "Meshes view").clicked() {
            meshes = true;
        }
    });
    ui.add_space(16.0);
    set_meshes_view(ui.ctx(), meshes);
}

/// The width `view_toggle` takes: two tabs, their 16 pt margins and the spacing after them.
fn view_tabs_width(ui: &egui::Ui) -> f32 {
    2.0 * (button_width(ui, "Textures view") + 12.0) + 2.0 + 32.0 + ui.spacing().item_spacing.x
}

/// Switch the view (also the saved one coming back at start, AG17).
pub fn set_meshes_view(ctx: &egui::Context, meshes: bool) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("sdc-meshes-view"), meshes));
}

/// True when the Meshes view is shown instead of the Textures view.
pub fn meshes_view_active(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(egui::Id::new("sdc-meshes-view")))
        .unwrap_or(false)
}

fn export_folder_button(ui: &mut egui::Ui, model: &mut AppModel) {
    // The folder an export would write to: the setting, else `<root>/sdc-out`.
    let shown = if model.settings.export_folder.is_some() || model.root.is_some() {
        model.export_root().display().to_string()
    } else {
        "not set".to_string()
    };
    let text = format!("Export folder: {shown}");
    // A long path is cut to EXPORT_FOLDER_MAX, or to what is left of the row but at least EXPORT_FOLDER_MIN (the
    // hover and the accessible name keep all of it), so a narrow window keeps its row one line.
    let w = button_width(ui, &text)
        .min(EXPORT_FOLDER_MAX)
        .min(room(ui).max(EXPORT_FOLDER_MIN));
    fit(ui, w);
    push_right(ui, w);
    let height = ui.spacing().interact_size.y;
    // Exactly `w` wide, so its right edge is the row's (BH12).
    let b = egui::Button::new(text)
        .truncate()
        .min_size(egui::vec2(w, height));
    // AG129: the drawer's links point at the export folder, so it stays put while an export runs.
    let off = if model.picker.busy() {
        Some(picker::BUSY_HOVER)
    } else if model.export.running.is_some() {
        Some(EXPORT_RUNNING_HOVER)
    } else {
        None
    };
    let r = ui
        .allocate_ui(egui::vec2(w, height), |ui| ui.add_enabled(off.is_none(), b))
        .inner;
    let r = match off {
        Some(why) => r.on_disabled_hover_text(why),
        None => r.on_hover_text(format!(
            "Choose where converted textures are written (now {shown})"
        )),
    };
    if r.clicked() {
        model.pick(ui.ctx(), PickerPurpose::ExportFolder, PickerSpec::folder());
    }
}

fn status(ui: &mut egui::Ui, model: &mut AppModel) {
    if model.status_line.is_empty() && model.banner.is_none() && model.encoding_note.is_none() {
        return;
    }
    egui::Panel::top("status").show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            // Item 13: a bar with Cancel while a folder opens; a re-plan after an edit keeps the plain line.
            match model.plan_progress.filter(|_| model.can_cancel_open()) {
                Some((done, total)) => {
                    let frac = if total == 0 {
                        0.0
                    } else {
                        done as f32 / total as f32
                    };
                    ui.add(
                        egui::ProgressBar::new(frac)
                            .desired_width(320.0)
                            .text(&model.status_line),
                    );
                }
                None => {
                    ui.label(&model.status_line);
                }
            }
            if model.can_cancel_open()
                && ui
                    .button("Cancel")
                    .on_hover_text(model.cancel_open_hover())
                    .clicked()
            {
                model.cancel_open();
            }
            if !model.items.is_empty() {
                let sel = model.selected_items();
                let text = model.vram_text(&sel);
                ui.weak(format!("· {text}")).on_hover_text(
                    "File sizes of the DDS files an export writes; a compressed size is also its video memory",
                );
            }
            // Task 9 (Z11): the last "encoding follows type" cleanup; every texture on hover.
            if let Some((line, all)) = &model.encoding_note {
                ui.weak(format!("· {line}")).on_hover_text(all);
            }
        });
        if let Some(b) = model.banner.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(super::status::AMBER, format!("⚠ {b}"));
                if model.project.is_some() && ui.button("Reload").clicked() {
                    model.request_reload();
                }
                if model.kept_banner.is_some()
                    && ui
                        .button("Dismiss")
                        .on_hover_text("Hide the startup warning")
                        .clicked()
                {
                    model.dismiss_kept_banner();
                }
            });
        }
    });
}
