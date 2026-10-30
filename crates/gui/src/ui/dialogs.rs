//! Modal dialogs for `model.dialog` (M3a spec §2.1).
use crate::model::{AfterExport, AppModel, Dialog, DocKind};
use eframe::egui;

pub fn draw(ctx: &egui::Context, model: &mut AppModel) {
    super::actions::draw(ctx, model);
    let dialog = model.dialog.clone();
    if !matches!(dialog, Some(Dialog::Rename(_))) {
        model.clear_rename_cache();
    }
    let Some(dialog) = dialog else {
        return;
    };
    match dialog {
        Dialog::CloseUnsaved => close_unsaved(ctx, model),
        Dialog::OpenUnsaved(root) => open_unsaved(ctx, model, root),
        Dialog::ReloadUnsaved => reload_unsaved(ctx, model),
        Dialog::ExportRunning(action) => export_running(ctx, model, action),
        Dialog::ExternalChange(which) => external_change(ctx, model, which),
        Dialog::OverwriteJson(path) => overwrite_json(ctx, model, &path),
        Dialog::Error(msg) => error(ctx, model, &msg),
        Dialog::CopySettings { from, to, fields } => copy_settings(ctx, model, &from, &to, fields),
        Dialog::ConfirmResetAll(keys) => confirm_reset_all(ctx, model, &keys),
        Dialog::Rename(d) => super::rename_dialog::draw(ctx, model, d),
        Dialog::GenerateFromNormal(_) => super::from_normal::draw(ctx, model),
        Dialog::MakeMarker => super::marker_dialog::draw(ctx, model),
        Dialog::ConfirmDeleteGenerated(r) => confirm_delete_generated(ctx, model, &r),
    }
}

/// A dialog's list is at most this share of the window height, so the footer stays on screen (CE19; first used by
/// the half-size box, which Note FC2 removed).
pub const DIALOG_LIST_SHARE: f32 = 0.6;

/// What the keys (and a click outside) did to a dialog this frame (item 4, DECISIONS AG11).
#[derive(Debug, Clone, Copy, Default)]
pub struct Keys {
    /// Esc, or a click outside when the dialog allows it: the dialog's Cancel.
    pub cancel: bool,
    /// Enter, while no widget has the keyboard focus (a focused button takes Enter itself) and not on the frame the
    /// dialog opened: the dialog's safe default.
    pub enter: bool,
}

/// A modal dialog. `outside`: whether a click outside cancels it; false where a stray click could lose typed input
/// or an open choice. `fade_in`: egui's 0.2 s fade; false for overlays opened from the keyboard (Note CF3). The caller
/// decides what `Keys` mean for each dialog.
pub fn modal_keys(
    ctx: &egui::Context,
    id: &str,
    outside: bool,
    fade_in: bool,
    add: impl FnOnce(&mut egui::Ui),
) -> Keys {
    let modal = egui::Modal::new(egui::Id::new(id));
    let area = egui::Modal::default_area(egui::Id::new(id)).fade_in(fade_in);
    let shown = modal.area(area).show(ctx, |ui| {
        ui.set_max_width(420.0);
        add(ui);
    });
    let was_open = opened_before(ctx, id);
    let outside_click = shown.backdrop_response.clicked();
    Keys {
        cancel: shown.should_close() && (outside || !outside_click),
        enter: was_open
            && ctx.memory(|m| m.focused().is_none())
            && ctx.input(|i| i.key_pressed(egui::Key::Enter)),
    }
}

/// Whether dialog `id` was drawn the frame before this one; call once per frame it is drawn. False on the frame it
/// opens, so the Enter that opened it (a palette pick) is not also the dialog's Enter.
fn opened_before(ctx: &egui::Context, id: &str) -> bool {
    let (seen, frame) = (egui::Id::new(("modal-seen", id)), ctx.cumulative_frame_nr());
    let was_open = ctx
        .data(|d| d.get_temp::<u64>(seen))
        .is_some_and(|f| f + 1 >= frame);
    ctx.data_mut(|d| d.insert_temp(seen, frame));
    was_open
}

/// A footer button's part in a dialog (layout v2 A7, R18).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Plain,
    /// Outlined in red: it drops, overwrites, stops or deletes something.
    Destructive,
    /// The one button Enter presses, filled with the accent. A dialog whose Enter does nothing has none.
    Default,
}

/// A7 / R18: left-to-right footer order: Plain buttons as given, then Destructive ones, then the one Default.
pub fn footer_order(roles: &[Role]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..roles.len()).collect();
    order.sort_by_key(|&i| match roles[i] {
        Role::Plain => 0,
        Role::Destructive => 1,
        Role::Default => 2,
    });
    order
}

/// The one dialog frame (A7): title strip (heading + separator), body, separator, right-aligned footer, exactly
/// `width` px wide (the right-to-left footer fills the row). The Default is filled with `visuals.selection.bg_fill`; a Destructive one has a 1.5 px `RED`
/// outline. `buttons` are (label, role, enabled); a disabled one is drawn disabled and never clicked. Built on
/// [`modal_keys`]: returns its Keys and the index into `buttons` that was clicked. The caller maps Enter to its
/// Default (the button Enter pressed before A7; Enter semantics are unchanged, DECISIONS AG11).
pub fn dialog(
    ctx: &egui::Context,
    id: &str,
    outside: bool,
    width: f32,
    title: &str,
    buttons: &[(&str, Role, bool)],
    body: impl FnOnce(&mut egui::Ui),
) -> (Keys, Option<usize>) {
    dialog_hover(ctx, id, outside, width, title, buttons, &[], body)
}

/// [`dialog`] with a hover reason for each button while it is disabled (`hovers[i]`, by the index into `buttons`).
#[allow(clippy::too_many_arguments)]
pub fn dialog_hover(
    ctx: &egui::Context,
    id: &str,
    outside: bool,
    width: f32,
    title: &str,
    buttons: &[(&str, Role, bool)],
    hovers: &[Option<&str>],
    body: impl FnOnce(&mut egui::Ui),
) -> (Keys, Option<usize>) {
    debug_assert!(
        buttons.iter().filter(|b| b.1 == Role::Default).count() <= 1,
        "a dialog has at most one Enter default"
    );
    let mut clicked = None;
    let keys = modal_keys(ctx, id, outside, true, |ui| {
        ui.set_max_width(width);
        ui.heading(title);
        ui.separator();
        body(ui);
        ui.separator();
        let roles: Vec<Role> = buttons.iter().map(|b| b.1).collect();
        // K5: right-to-left, so the buttons go in reversed; a long row wraps instead of widening the dialog.
        // I1: rows start one button tall, as `horizontal_wrapped` seeds them. `with_layout` seeded them with last
        // frame's leftover height, and a wrapped footer then doubled the modal's height every frame.
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), ui.spacing().interact_size.y),
            egui::Layout::right_to_left(egui::Align::Center).with_main_wrap(true),
            |ui| {
                for i in footer_order(&roles).into_iter().rev() {
                    let (label, role, enabled) = buttons[i];
                    let b = egui::Button::new(label);
                    let b = match role {
                        Role::Plain => b,
                        Role::Destructive => b.stroke(egui::Stroke::new(1.5, super::status::RED)),
                        Role::Default => b.fill(ui.visuals().selection.bg_fill),
                    };
                    let mut r = ui.add_enabled(enabled, b);
                    if let Some(Some(h)) = hovers.get(i) {
                        r = r.on_disabled_hover_text(*h);
                    }
                    if r.clicked() {
                        clicked = Some(i);
                    }
                }
            },
        );
    });
    (keys, clicked)
}

/// `name` cut in the middle for a button label: its first 20 and last 16 characters around `…` when it is longer
/// than 40. The body line above the buttons carries the whole name (I1).
pub fn short_name(name: &str) -> String {
    let n = name.chars().count();
    if n <= 40 {
        return name.to_string();
    }
    let head: String = name.chars().take(20).collect();
    let tail: String = name.chars().skip(n - 16).collect();
    format!("{head}…{tail}")
}

/// The weak line under a body that names the files an unsaved-changes question is about.
fn files_hint(ui: &mut egui::Ui) {
    ui.weak("sdc.toml, sdc-project.toml");
}

/// F3a D4: the copy checklist, every field ticked to start. The ticks live in the dialog itself.
fn copy_settings(
    ctx: &egui::Context,
    model: &mut AppModel,
    from: &str,
    to: &[String],
    mut fields: Vec<(String, bool)>,
) {
    let name = model
        .item(from)
        .map_or(from.to_string(), |i| i.name.clone());
    let title = format!("Copy settings from {name} to {} selected", to.len());
    // Apply's enabled state is read before the body: a Check none shows on the next frame.
    let any = fields.iter().any(|f| f.1);
    // R18: Enter does nothing here, so neither button is the Default.
    let buttons = [("Cancel", Role::Plain, true), ("Apply", Role::Plain, any)];
    let (k, clicked) = dialog(ctx, "copy-settings", false, 420.0, &title, &buttons, |ui| {
        ui.horizontal(|ui| {
            if ui.button("Check all").clicked() {
                fields.iter_mut().for_each(|f| f.1 = true);
            }
            if ui.button("Check none").clicked() {
                fields.iter_mut().for_each(|f| f.1 = false);
            }
        });
        for (f, on) in fields.iter_mut() {
            let value = model
                .project
                .as_ref()
                .and_then(|p| p.file_value(from, f))
                .map_or(String::new(), |v| match v {
                    serde_json::Value::String(s) => s,
                    v => v.to_string(),
                });
            ui.checkbox(
                on,
                format!("{}: {value}", crate::model::bulk::field_label(f)),
            );
        }
    });
    let close = clicked == Some(0) || k.cancel;
    if clicked == Some(1) && any {
        let ticked: Vec<String> = fields.iter().filter(|f| f.1).map(|f| f.0.clone()).collect();
        model.apply_copy_settings(from, to, &ticked);
    } else if close {
        model.dialog = None;
    } else if let Some(Dialog::CopySettings { fields: kept, .. }) = model.dialog.as_mut() {
        *kept = fields;
    }
}

/// F3a D4: Reset all asks first; it names how many textures lose their settings.
fn confirm_reset_all(ctx: &egui::Context, model: &mut AppModel, keys: &[String]) {
    let title = format!("Reset all settings of {} textures?", keys.len());
    let buttons = [
        ("Reset", Role::Destructive, true),
        ("Cancel", Role::Default, true),
    ];
    let (k, clicked) = dialog(
        ctx,
        "confirm-reset-all",
        true,
        420.0,
        &title,
        &buttons,
        |ui| {
            ui.label(
            "Their entries in sdc.toml are removed; acknowledged warnings are kept. Settings in \
             sdc-project.toml are not touched. Undo brings everything back.",
        );
        },
    );
    if clicked == Some(0) {
        model.reset_all_settings(keys);
    } else if clicked == Some(1) || k.cancel || k.enter {
        model.dialog = None;
    }
}

/// Note CO (CE3, P13): the "Delete generated file" box's buttons, `name` the file's short name: a red-outlined
/// Delete and Cancel, the Enter default (ui-rules 5.2). Both are enabled.
pub fn delete_generated_buttons(name: &str) -> [(String, Role, bool); 2] {
    [
        (format!("Delete {name}"), Role::Destructive, true),
        ("Cancel".into(), Role::Default, true),
    ]
}

/// Note CO: "Delete generated file…" asks first; the body keeps each whole name (CE19) and says why a file is kept.
fn confirm_delete_generated(
    ctx: &egui::Context,
    model: &mut AppModel,
    r: &crate::model::from_normal::DeleteGenerated,
) {
    let owned = delete_generated_buttons(&short_name(&r.name));
    let buttons: Vec<(&str, Role, bool)> =
        owned.iter().map(|(l, r, e)| (l.as_str(), *r, *e)).collect();
    let (k, clicked) = dialog(
        ctx,
        "delete-generated",
        true,
        420.0,
        "Delete generated file",
        &buttons,
        |ui| {
            ui.label(&r.body);
            if let Some(why) = &r.refused {
                ui.weak(format!("Not deleted: {why}."));
            }
            ui.weak("This cannot be undone.");
        },
    );
    if clicked == Some(0) {
        model.confirm_delete_generated();
    } else if clicked == Some(1) || k.cancel || k.enter {
        model.dialog = None;
    }
}

fn close_unsaved(ctx: &egui::Context, model: &mut AppModel) {
    let buttons = [
        ("Cancel", Role::Plain, true),
        ("Discard", Role::Destructive, true),
        ("Save", Role::Default, true),
    ];
    let (k, clicked) = dialog(
        ctx,
        "close-unsaved",
        true,
        420.0,
        "Unsaved changes",
        &buttons,
        |ui| {
            ui.label("Your edits are not saved.");
            files_hint(ui);
        },
    );
    if clicked == Some(2) || k.enter {
        model.dialog = None;
        // A failed save raises its own dialog (changed on disk, or an error) and the window stays open.
        if model.save().is_ok() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    } else if clicked == Some(1) {
        model.dialog = None;
        // Dropping the edits makes the close request pass `request_close`.
        if let Some(p) = model.project.as_mut() {
            p.reload();
        }
        if let Some(s) = model.sets_doc.as_mut() {
            s.reload();
        }
        model.history.clear();
        model.refresh_banner();
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    } else if clicked == Some(0) || k.cancel {
        model.dialog = None;
    }
}

/// Trace-debug #10: Enter and Esc cancel; only the button drops the edits.
fn reload_unsaved(ctx: &egui::Context, model: &mut AppModel) {
    let buttons = [
        ("Discard and reload", Role::Destructive, true),
        ("Cancel", Role::Default, true),
    ];
    let (k, clicked) = dialog(
        ctx,
        "reload-unsaved",
        true,
        420.0,
        "Unsaved changes",
        &buttons,
        |ui| {
            ui.label("Reload drops your unsaved edits and Undo.");
            files_hint(ui);
        },
    );
    if clicked == Some(0) {
        model.reload_documents();
    } else if clicked == Some(1) || k.cancel || k.enter {
        model.dialog = None;
    }
}

fn external_change(ctx: &egui::Context, model: &mut AppModel, which: DocKind) {
    let title = format!("{} changed on disk", which.file());
    let buttons = [
        ("Reload from disk", Role::Plain, true),
        ("Overwrite with my changes", Role::Destructive, true),
        ("Cancel", Role::Default, true),
    ];
    let (k, clicked) = dialog(
        ctx,
        "external-change",
        true,
        420.0,
        &title,
        &buttons,
        |ui| {
            ui.label("Another program changed the file after it was opened.");
        },
    );
    // Enter keeps the edits in memory and the file on disk as they are; it is never Overwrite (AG11).
    match clicked {
        Some(0) => model.resolve_external_change(which, false),
        Some(1) => model.resolve_external_change(which, true),
        _ if clicked.is_some() || k.cancel || k.enter => model.dialog = None,
        _ => {}
    }
}

fn error(ctx: &egui::Context, model: &mut AppModel, msg: &str) {
    let (k, clicked) = dialog(
        ctx,
        "error",
        true,
        420.0,
        "Error",
        &[("OK", Role::Default, true)],
        |ui| {
            ui.label(msg);
        },
    );
    if clicked.is_some() || k.cancel || k.enter {
        model.dialog = None;
    }
}

fn open_unsaved(ctx: &egui::Context, model: &mut AppModel, root: std::path::PathBuf) {
    let buttons = [
        ("Cancel", Role::Plain, true),
        ("Discard", Role::Destructive, true),
        ("Save", Role::Default, true),
    ];
    let (k, clicked) = dialog(
        ctx,
        "open-unsaved",
        true,
        420.0,
        "Unsaved changes",
        &buttons,
        |ui| {
            ui.label("Your edits are not saved. Opening another folder would lose them.");
            files_hint(ui);
        },
    );
    if clicked == Some(2) || k.enter {
        model.resolve_open_unsaved(root, true);
    } else if clicked == Some(1) {
        model.resolve_open_unsaved(root, false);
    } else if clicked == Some(0) || k.cancel {
        model.dialog = None;
    }
}

fn export_running(ctx: &egui::Context, model: &mut AppModel, action: AfterExport) {
    let (question, stop) = match &action {
        AfterExport::Close => ("Stop it and close?", "Stop the export and close"),
        AfterExport::Open(_) => (
            "Stop it and open the other folder?",
            "Stop the export and open the folder",
        ),
    };
    let buttons = [
        (stop, Role::Destructive, true),
        ("Keep exporting", Role::Default, true),
    ];
    let (k, clicked) = dialog(
        ctx,
        "export-running",
        true,
        420.0,
        "An export is running",
        &buttons,
        |ui| {
            ui.label(question);
        },
    );
    if clicked == Some(0) {
        model.resolve_export_running(action, true);
    } else if clicked == Some(1) || k.cancel || k.enter {
        model.resolve_export_running(action, false);
    }
}

fn overwrite_json(ctx: &egui::Context, model: &mut AppModel, path: &std::path::Path) {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let short = short_name(&name);
    let (overwrite, keep) = (
        format!("Overwrite {short}"),
        format!("Keep the edited {short}"),
    );
    let buttons = [
        (overwrite.as_str(), Role::Destructive, true),
        (keep.as_str(), Role::Default, true),
    ];
    let title = "The PGPatcher JSON was edited";
    let (k, clicked) = dialog(ctx, "overwrite-json", true, 420.0, title, &buttons, |ui| {
        ui.label(format!(
            "{name} was edited outside Tessalith since the last export."
        ));
    });
    if clicked == Some(0) {
        model.resolve_overwrite_json(true);
    } else if clicked == Some(1) || k.cancel || k.enter {
        model.resolve_overwrite_json(false);
    }
}
