//! The header (M3a spec §2.1): open, recent, mode, collapse, export folder, save, export; then the status line and banner.
use crate::model::picker::{self, PickerPurpose, PickerSpec};
use crate::model::AppModel;
use eframe::egui;
use engine::config::Target;

const WARNING: egui::Color32 = egui::Color32::from_rgb(230, 160, 30);
/// The JSON name field's width; it is never squeezed (AG126).
const JSON_FIELD_WIDTH: f32 = 120.0;
/// The widest the Export folder button gets; a longer path is cut with "…".
const EXPORT_FOLDER_MAX: f32 = 280.0;
/// Why the export folder cannot change now.
pub const EXPORT_RUNNING_HOVER: &str = "An export is running: change the folder after it ends";

fn target_name(t: Target) -> &'static str {
    match t {
        Target::Cs => "cs",
        Target::Vanilla => "vanilla",
        Target::Cpm => "cpm",
    }
}

fn target_help(t: Target) -> &'static str {
    match t {
        Target::Cs => "Community Shaders: TruePBR sets and its packed maps",
        Target::Vanilla => "The base game's shaders and texture layout",
        Target::Cpm => "Complex Parallax Materials: packed maps with a height channel",
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
        // AG126: one wrapped row, so a narrow window moves controls to a second line instead of drawing them on top
        // of each other. A group that is one nested layout (a combo, a disabled scope) is moved whole by `fit`.
        ui.horizontal_wrapped(|ui| {
            if picker::button(ui, &model.picker, egui::Button::new("Open folder…")) {
                model.pick(ui.ctx(), PickerPurpose::OpenFolder, PickerSpec::folder());
            }
            recent_menu(ui, model);
            undo_redo(ui, model);
            view_toggle(ui);
            if let Some(m) = scan {
                fit(ui, button_width(ui, "MO2…"));
                super::meshes::mo2_button(ui, m);
            }
            mode_combo(ui, model);
            preset_combo(ui, model);
            fit(ui, button_width(ui, "Presets…"));
            if ui
                .button("Presets…")
                .on_hover_text("Make and edit your own presets (saved in %APPDATA%\\sdc\\presets)")
                .clicked()
            {
                model.open_presets();
            }
            column_checkboxes(ui, model);
            json_name_field(ui, model);
            export_folder_button(ui, model);
            super::messages::button(ui, model);
            if ui
                .add_enabled(model.is_dirty(), egui::Button::new("Save"))
                .clicked()
            {
                // A failed save raises its own dialog.
                let _ = model.save();
            }
            let can_export = can_export(model);
            let mut export = egui::Button::new(egui::RichText::new("Export").strong())
                .min_size(egui::vec2(120.0, 30.0));
            if can_export {
                export = export.fill(ui.visuals().selection.bg_fill);
            }
            if ui.add_enabled(can_export, export).clicked() {
                model.export_clicked();
            }
        });
    });
    status(ui, model);
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

/// Which of the Textures and Sets columns are shown at all. Each column's own arrow collapses it to a strip.
fn column_checkboxes(ui: &mut egui::Ui, model: &mut AppModel) {
    let st = &mut model.settings;
    let mut textures = !st.textures_hidden;
    let mut sets = !st.sets_hidden;
    let mut changed = ui
        .checkbox(&mut textures, "Textures column")
        .on_hover_text("Show the Textures column")
        .changed();
    changed |= ui
        .checkbox(&mut sets, "Sets column")
        .on_hover_text("Show the Sets column")
        .changed();
    if changed {
        st.textures_hidden = !textures;
        st.sets_hidden = !sets;
        model.save_settings();
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
    let read_only = model.project.as_ref().is_some_and(|p| p.read_only());
    model.root.is_some() && model.export.running.is_none() && !read_only
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

fn mode_combo(ui: &mut egui::Ui, model: &mut AppModel) {
    let gap = ui.spacing().item_spacing.x;
    fit(
        ui,
        button_width(ui, "Mode") + gap + ui.spacing().combo_width,
    );
    ui.label("Mode");
    let mut picked = model.target;
    ui.add_enabled_ui(model.project.is_some(), |ui| {
        egui::ComboBox::from_id_salt("mode")
            .selected_text(target_name(model.target))
            .show_ui(ui, |ui| {
                for t in [Target::Cs, Target::Vanilla, Target::Cpm] {
                    ui.selectable_value(&mut picked, t, target_name(t))
                        .on_hover_text(target_help(t));
                }
            })
            .response
            .on_hover_text(
                "The target the textures are converted for: cs, vanilla or cpm. Saved in sdc.toml as target.",
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

/// Top-level view toggle (plan A Task 12): "Textures | Meshes", kept in egui memory for the session.
pub fn view_toggle(ui: &mut egui::Ui) {
    let mut meshes = meshes_view_active(ui.ctx());
    ui.separator();
    if ui.selectable_label(!meshes, "Textures view").clicked() {
        meshes = false;
    }
    if ui.selectable_label(meshes, "Meshes view").clicked() {
        meshes = true;
    }
    set_meshes_view(ui.ctx(), meshes);
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
    // A long path is cut to EXPORT_FOLDER_MAX (the hover and the accessible name keep all of it).
    let w = button_width(ui, &text).min(EXPORT_FOLDER_MAX);
    fit(ui, w);
    let height = ui.spacing().interact_size.y;
    let b = egui::Button::new(text).truncate();
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

/// The PGPatcher JSON name (CS only, with a folder open). The default shows as hint text; empty restores it.
fn json_name_field(ui: &mut egui::Ui, model: &mut AppModel) {
    if model.target != Target::Cs || model.root.is_none() || model.sets_doc.is_none() {
        return;
    }
    let id = egui::Id::new("json-name");
    let mut t = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| {
        model
            .sets_doc
            .as_ref()
            .and_then(|d| d.json_name())
            .unwrap_or_default()
    });
    let editable = model.sets_doc.as_ref().is_some_and(|d| !d.read_only());
    let why = model
        .sets_doc
        .as_ref()
        .and_then(|d| d.error())
        .unwrap_or("sdc-project.toml is read-only")
        .to_string();
    let hover = format!(
        "The PGPatcher JSON is written to {}\\PBRNIFPatcher\\<name>.json",
        model.export_root().display()
    );
    // The label, the field and Editor… stay on one line, the field never narrower than JSON_FIELD_WIDTH.
    let gap = ui.spacing().item_spacing.x;
    fit(
        ui,
        button_width(ui, "JSON:") + JSON_FIELD_WIDTH + button_width(ui, "Editor…") + 2.0 * gap,
    );
    ui.label("JSON:");
    let r = ui
        .add_enabled(
            editable,
            egui::TextEdit::singleline(&mut t)
                .hint_text(crate::model::pbr_export::json_name(model))
                .desired_width(JSON_FIELD_WIDTH),
        )
        .on_hover_text(hover)
        .on_disabled_hover_text(why);
    if ui
        .button("Editor…")
        .on_hover_text("Edit the whole TruePBR JSON: every set's entry and the delete sections")
        .clicked()
    {
        model.json_editor_open = true;
    }
    if r.changed() {
        ui.data_mut(|d| d.insert_temp(id, t.clone()));
    }
    if r.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(id));
        model.set_json_name(&t);
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
                ui.colored_label(WARNING, format!("⚠ {b}"));
                if model.project.is_some() && ui.button("Reload").clicked() {
                    model.reload_documents();
                }
            });
        }
    });
}
