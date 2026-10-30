//! M6 Track F3b (spec §5): the rename dialog. Drawing only: the rows come from `rename::plan_rename`, and Apply is
//! one model call.
use super::columns::{AMBER, RED};
use super::dialogs::{dialog, Role};
use crate::model::rename::{Case, RenameDialog, RenameMode, RenamePlan, RenameRow};
use crate::model::{AppModel, Dialog};
use eframe::egui;

pub fn draw(ctx: &egui::Context, model: &mut AppModel, mut d: RenameDialog) {
    // Drawing only: the cache keeps the plan across frames while nothing it reads changes; Apply plans for itself.
    let plan = model.rename_plan_cached(&d);
    let mut mode = d.mode;
    let n = d.keys.len();
    let title = match d.mode {
        RenameMode::Inputs => format!("Rename {n} input{}", if n == 1 { "" } else { "s" }),
        RenameMode::Outputs => {
            format!("Rename outputs of {n} set{}", if n == 1 { "" } else { "s" })
        }
    };
    let can = plan.can_apply();
    let buttons = [("Cancel", Role::Plain, true), ("Apply", Role::Default, can)];
    // K4: the shared keys. Enter applies when Apply is enabled, not on the opening frame and not while a widget
    // keeps the focus (a single-line field gives it up on that Enter); a click outside does nothing (typed input).
    let (k, clicked) = dialog(ctx, "rename", false, 860.0, &title, &buttons, |ui| {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut mode, RenameMode::Inputs, "Inputs")
                .on_hover_text("Read the files under new names (sdc.toml [inputs]); the files on disk are not touched.");
            ui.selectable_value(&mut mode, RenameMode::Outputs, "Outputs")
                .on_hover_text("Give each set's outputs a new base name; every output keeps its folder and suffix.");
        });
        ui.label(
            egui::RichText::new(match d.mode {
                RenameMode::Inputs if d.recipe.path => "The folders and the base name change; the map suffix (_n, _normal…) and the extension stay.",
                RenameMode::Inputs => "Only the base name changes (the part before _n, _normal…). Tick Whole path to change the folders too.",
                RenameMode::Outputs => "A TruePBR set on Community Shaders renames through its PGPatcher rename.",
            })
            .small()
            .weak(),
        );
        ui.add_space(4.0);
        egui::Grid::new("rename-recipe")
            .num_columns(4)
            .show(ui, |ui| {
                ui.label("Find");
                ui.text_edit_singleline(&mut d.recipe.find);
                ui.label("Replace");
                ui.text_edit_singleline(&mut d.recipe.replace);
                ui.end_row();
                ui.label("Prefix");
                ui.text_edit_singleline(&mut d.recipe.prefix);
                ui.label("Suffix");
                ui.text_edit_singleline(&mut d.recipe.suffix);
                ui.end_row();
                ui.checkbox(&mut d.recipe.regex, "Regex")
                    .on_hover_text("$1 or ${name} in Replace for a capture group");
                if d.mode == RenameMode::Inputs {
                    ui.checkbox(&mut d.recipe.path, "Whole path").on_hover_text(
                        "Find, Replace, Prefix and Suffix work on the folders and the base name",
                    );
                }
                egui::ComboBox::from_id_salt("rename-case")
                    .selected_text(case_label(d.recipe.case))
                    .show_ui(ui, |ui| {
                        for c in [Case::Keep, Case::Lower, Case::Upper] {
                            ui.selectable_value(&mut d.recipe.case, c, case_label(c));
                        }
                    });
                ui.end_row();
            });
        match &plan.error {
            Some(e) => {
                ui.colored_label(RED, e);
            }
            None if !d.recipe.find.is_empty() => {
                let (k, m) = plan.matched();
                ui.weak(format!("{k} of {m} matched"));
            }
            None => {}
        }
        ui.separator();
        rows_table(ui, &plan, &mut d);
    });
    let apply = clicked == Some(1) || (k.enter && can);
    if clicked == Some(0) || k.cancel {
        model.dialog = None;
        model.clear_rename_cache();
        return;
    }
    // The edits made this frame are kept before anything acts on them.
    if let Some(Dialog::Rename(kept)) = model.dialog.as_mut() {
        *kept = d;
    }
    if mode != model_mode(model) {
        model.set_rename_mode(mode);
    }
    if apply {
        model.apply_rename();
    }
}

fn model_mode(model: &AppModel) -> RenameMode {
    match &model.dialog {
        Some(Dialog::Rename(d)) => d.mode,
        _ => RenameMode::Inputs,
    }
}

fn case_label(c: Case) -> &'static str {
    match c {
        Case::Keep => "Keep case",
        Case::Lower => "lower case",
        Case::Upper => "UPPER CASE",
    }
}

/// The preview's column widths: fixed from the dialog's width, so a long name never moves a column.
struct Cols {
    tick: f32,
    old: f32,
    kind: f32,
    output: f32,
    note: f32,
}

impl Cols {
    fn new(total: f32, gap: f32) -> Cols {
        let (tick, kind) = (22.0, 130.0);
        let rest = (total - tick - kind - 4.0 * gap).max(120.0);
        let (old, output) = (rest * 0.32, rest * 0.36);
        Cols {
            tick,
            old,
            kind,
            output,
            note: rest - old - output,
        }
    }

    fn sum(&self, gap: f32) -> f32 {
        self.tick + self.old + self.kind + self.output + self.note + 4.0 * gap
    }
}

/// One table row: `cells` get their column's rectangle, left to right.
fn table_row(
    ui: &mut egui::Ui,
    cols: &Cols,
    height: f32,
    stripe: bool,
    cells: [&mut dyn FnMut(&mut egui::Ui); 5],
) {
    let gap = ui.spacing().item_spacing.x;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(cols.sum(gap), height), egui::Sense::hover());
    if stripe {
        ui.painter()
            .rect_filled(rect, 0.0, ui.visuals().faint_bg_color);
    }
    let mut x = rect.left();
    let widths = [cols.tick, cols.old, cols.kind, cols.output, cols.note];
    for (cell, w) in cells.into_iter().zip(widths) {
        let r = egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(w, height));
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(r)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        child.set_clip_rect(r.intersect(ui.clip_rect()));
        cell(&mut child);
        x += w + gap;
    }
}

/// The header and the rows, drawn with `show_rows`: only the rows in view are laid out (Note FA).
fn rows_table(ui: &mut egui::Ui, plan: &RenamePlan, d: &mut RenameDialog) {
    ui.scope(|ui| {
        ui.style_mut().spacing.scroll = egui::style::ScrollStyle::thin();
        ui.style_mut().spacing.item_spacing.y = 2.0;
        let gap = ui.spacing().item_spacing.x;
        let height = ui.spacing().interact_size.y;
        // 8 px of margin: a row exactly as wide as its column would flip the scroll bar on and off (ui-rules 3.7).
        let total = ui.available_width() - ui.spacing().scroll.allocated_width() - 8.0;
        let cols = Cols::new(total, gap);
        // shortcut: no New column (CG2): in this fixed-size dialog it repeated Output and was clipped too narrow to
        // read, so one row is renamed by unticking the others; bring per-row typing back once the dialog is
        // resizable (Note FC8).
        let head = |s: &'static str| {
            move |ui: &mut egui::Ui| {
                ui.label(egui::RichText::new(s).strong());
            }
        };
        let (mut h0, mut h1, mut h2, mut h3, mut h4) = (
            head(""),
            head("Old"),
            head("Kind"),
            head("Output"),
            head(""),
        );
        table_row(
            ui,
            &cols,
            height,
            false,
            [&mut h0, &mut h1, &mut h2, &mut h3, &mut h4],
        );
        egui::ScrollArea::vertical()
            .max_height(360.0)
            .auto_shrink([false, true])
            .show_rows(ui, height, plan.rows.len(), |ui, range| {
                for i in range {
                    let r = &plan.rows[i];
                    row(ui, &cols, height, i % 2 == 1, r, d);
                }
            });
    });
}

fn row(
    ui: &mut egui::Ui,
    cols: &Cols,
    height: f32,
    stripe: bool,
    r: &RenameRow,
    d: &mut RenameDialog,
) {
    let weak = !r.included || r.skipped.is_some();
    let text = |s: &str| {
        let t = egui::RichText::new(s);
        if weak {
            t.weak()
        } else {
            t
        }
    };
    let cell = |ui: &mut egui::Ui, s: egui::RichText| {
        ui.add(egui::Label::new(s).truncate());
    };
    let kind = r.kind.as_ref().map_or(String::new(), |(a, b)| {
        if a == b {
            a.clone()
        } else {
            format!("{a} → {b}")
        }
    });
    let mut tick = |ui: &mut egui::Ui| {
        let mut on = r.included;
        let tick = ui.add_enabled(r.skipped.is_none(), egui::Checkbox::without_text(&mut on));
        if tick.changed() {
            if on {
                d.exclude.remove(&r.key);
            } else {
                d.exclude.insert(r.key.clone());
            }
        }
    };
    let mut old = |ui: &mut egui::Ui| cell(ui, text(&r.old));
    let mut kind_cell = |ui: &mut egui::Ui| cell(ui, text(&kind));
    let mut output = |ui: &mut egui::Ui| cell(ui, text(&r.output));
    let mut note = |ui: &mut egui::Ui| match (&r.skipped, &r.problem, &r.warning) {
        (Some(s), _, _) => cell(ui, egui::RichText::new(s).weak()),
        (_, Some(p), _) if r.included => cell(ui, egui::RichText::new(p).color(RED)),
        (_, _, Some(w)) if r.included => cell(ui, egui::RichText::new(w).color(AMBER)),
        _ => {}
    };
    table_row(
        ui,
        cols,
        height,
        stripe,
        [&mut tick, &mut old, &mut kind_cell, &mut output, &mut note],
    );
}
