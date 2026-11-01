//! The header (layout v2 A4): one row, Open ▾ · Undo · Redo | MO2… · Preset | the view tabs · the column eyes,
//! then Export to ▾ · Save · Export at the right edge; then the status line and banner. Drawn in both views (C7).
use super::meshes::UndoTarget;
use crate::model::picker::{self, PickerPurpose};
use crate::model::{also, AppModel};
use eframe::egui;
use engine::config::Target;
use engine::presets::AlsoTarget;

/// The Export to ▾ entry that opens the PGPatcher JSON editor (R14; was the header's PG JSON Editor, Note BH7).
pub const JSON_EDITOR: &str = "PGPatcher JSON editor…";
/// The Export to ▾ entry that opens the marker cube dialog (cubemap GUI, Task 6).
pub const MAKE_MARKER: &str = "Make marker cube…";
/// Export's width: twice what it was (Note BH3).
const EXPORT_WIDTH: f32 = 240.0;
/// CF12: the export bar's fraction as drawn: it glides linearly to `frac` over 0.3 s (at once under reduce motion). A
/// new export (a new `drawer_epoch`) is a new animation, whose first frame is its own value, so it never sweeps back.
#[doc(hidden)]
pub fn shown_progress(ctx: &egui::Context, epoch: u64, frac: f32) -> f32 {
    let id = egui::Id::new(("export-progress", epoch));
    ctx.animate_value_with_time(id, frac, super::motion::move_dur(ctx, 0.3))
}

/// What the export menu's button says with no project open (Note F?5, was Note FB1's "no folder chosen").
const MORE_TEXT: &str = "More ▾";
/// What it says with a project open and no export folder chosen for it (Note F?5).
const NOT_CHOSEN_TEXT: &str = "Export to: not chosen ▾";
/// The fewest path characters Export to ▾ shows (K1).
const PATH_MIN_CHARS: usize = 12;
/// Why the export folder cannot change now.
pub const EXPORT_RUNNING_HOVER: &str = "An export is running: change the folder after it ends";

pub(super) fn target_name(t: Target) -> &'static str {
    match t {
        Target::Cs => "cs",
        Target::Vanilla => "vanilla",
    }
}

pub(super) fn target_help(t: Target) -> &'static str {
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
    // Note F?4: automatic types alone are not marked.
    if model.prompts_unsaved() {
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
        // AG126: the row wraps in a narrow window instead of drawing controls on top of each other. A group that is one
        // nested layout (a combo, a disabled scope) is moved whole by `fit`.
        ui.horizontal_wrapped(|ui| row(ui, model, scan));
    });
    status(ui, model);
}

/// A4, left to right: Open ▾ · Undo · Redo | MO2… · Preset | the view tabs · the eyes (Textures view only),
/// then Export to ▾ · Save · Export pushed to the right edge.
fn row(
    ui: &mut egui::Ui,
    model: &mut AppModel,
    scan: Option<&mut crate::model::meshes::MeshesModel>,
) {
    // Save and Export's height from the start, so every control centres on one line (not lower after the taller tabs).
    ui.set_row_height(2.0 * ui.spacing().interact_size.y);
    open_menu(ui, model);
    undo_redo(ui, model);
    ui.separator();
    // Note F?16: the Mode control is in the Presets tab; MO2… stands alone, where the app has a scan model.
    if let Some(m) = scan {
        fit(ui, button_width(ui, "MO2…") + ui.spacing().item_spacing.x);
        super::meshes::mo2_button(ui, m);
    }
    preset_combo(ui, model);
    ui.separator();
    view_toggle(ui);
    if model.root.is_some() && !meshes_view_active(ui.ctx()) {
        column_toggles(ui, model);
    }
    right_end(ui, model);
}

/// Export to ▾, Save and Export against the row's right edge; Save and Export two rows tall (A4).
fn right_end(ui: &mut egui::Ui, model: &mut AppModel) {
    let gap = ui.spacing().item_spacing.x;
    let h = 2.0 * ui.spacing().interact_size.y;
    let save_w = button_width(ui, "Save");
    let glyph = button_width(ui, "0") - 2.0 * ui.spacing().button_padding.x;
    // Note F?5: with a project open the menu reads `Export to: {folder} ▾` or `Export to: not chosen ▾`; with none it
    // is `More ▾` (the folder in `gui.toml` is not this project's, and nothing exports without a project).
    let full = model
        .root
        .as_ref()
        .and_then(|_| model.export_folder_chosen())
        .map(|p| p.display().to_string());
    let fixed_text = match (&model.root, &full) {
        (None, _) => Some(MORE_TEXT),
        (Some(_), None) => Some(NOT_CHOSEN_TEXT),
        _ => None,
    };
    // K1: what Export to's path may take is the room left after the button's own text, Save and Export.
    let (lead, min_path) = match fixed_text {
        Some(t) => (t, 0.0),
        None => ("Export to:  ▾", PATH_MIN_CHARS as f32 * glyph),
    };
    let fixed = button_width(ui, lead) + gap + save_w + gap + EXPORT_WIDTH;
    fit(ui, fixed + min_path);
    let mut chars =
        (((room(ui) - fixed - gap) / glyph).floor().max(0.0) as usize).max(PATH_MIN_CHARS);
    let others = gap + save_w + gap + EXPORT_WIDTH;
    let text = loop {
        if let Some(t) = fixed_text {
            break t.to_string();
        }
        let Some(p) = &full else {
            unreachable!("a chip without fixed text has a folder");
        };
        let text = format!("Export to: {} ▾", elide_middle(p, chars));
        // A path's characters can be wider than '0': drop more until the trio fits (never below the minimum).
        if chars <= PATH_MIN_CHARS || button_width(ui, &text) + others <= room(ui) - 0.5 {
            break text;
        }
        chars -= 1;
    };
    push_right(ui, button_width(ui, &text) + others);
    export_to_menu(ui, model, text, full);
    // Note FC8: a focused text field may hold the only unsaved edit, so Save is live while one has the focus.
    let r = ui.add_enabled(
        model.is_dirty() || super::actions::text_field_focused(ui.ctx()),
        egui::Button::new("Save").min_size(egui::vec2(save_w, h)),
    );
    let r = if model.only_automatic_unsaved() {
        r.on_hover_text(
            "Save the texture types Tessalith set automatically, so sdc.exe plans the same",
        )
    } else {
        r
    };
    // Note FC8 (supersedes BS7's refusal): the field commits first, then this click saves. That relies on the header
    // and the bottom panel drawing before the central fields (`blur_on_press` runs first); `after_commit` covers a
    // press and release in one frame.
    super::commit_click(ui, &r, model, |m| {
        if m.is_dirty() {
            // A failed save raises its own dialog.
            let _ = m.save();
        }
    });
    export_button(ui, model, egui::vec2(EXPORT_WIDTH, h));
}

/// `path` cut in the middle to at most `max_chars` characters: the first component (`C:`), `…`, then as many whole
/// trailing components as fit. A path that fits is returned as it is; when even the last component does not fit,
/// `…` and its last `max_chars - 1` characters.
pub fn elide_middle(path: &str, max_chars: usize) -> String {
    let len = |s: &str| s.chars().count();
    if len(path) <= max_chars {
        return path.to_string();
    }
    let parts: Vec<&str> = path.split(['\\', '/']).collect();
    let sep = path
        .chars()
        .find(|c| *c == '\\' || *c == '/')
        .unwrap_or('\\');
    let last = parts.last().copied().unwrap_or(path);
    let head = format!("{}{sep}…", parts[0]);
    let mut tail = String::new();
    for p in parts[1..].iter().rev() {
        let next = format!("{sep}{p}{tail}");
        if len(&head) + len(&next) > max_chars {
            break;
        }
        tail = next;
    }
    if tail.is_empty() {
        let keep = max_chars.saturating_sub(1);
        let skip = len(last).saturating_sub(keep);
        return format!("…{}", last.chars().skip(skip).collect::<String>());
    }
    format!("{head}{tail}")
}

/// What is left of this line of a wrapped row.
fn room(ui: &egui::Ui) -> f32 {
    ui.max_rect().right() - ui.cursor().min.x
}

/// Moves the cursor so a `w`-wide widget ends at the row's right edge.
fn push_right(ui: &mut egui::Ui, w: f32) {
    let s = room(ui) - w - 0.5;
    if s > 0.0 {
        ui.add_space(s);
    }
}

/// Export, `size` big. While an export runs its lower half is the progress bar (done/total; the file on hover).
/// Note FB1: it reads `Review {n} files` in the plain button colour until Review would write without skipping
/// anything, then `Export {n} files`, accent-filled. A click opens Review either way (CA1); a clean project exports.
fn export_button(ui: &mut egui::Ui, model: &mut AppModel, size: egui::Vec2) {
    let can_export = model.can_export();
    let running = model.export.running.is_some();
    let gap = ui.spacing().item_spacing.y;
    let button_h = if running {
        (size.y - gap) / 2.0
    } else {
        size.y
    };
    // Once per frame: the text, the accent and the hover all read the same answer.
    let clean = model.export_is_clean();
    let mut export = egui::Button::new(egui::RichText::new(model.export_text(clean)).strong())
        .min_size(egui::vec2(size.x, button_h));
    // CF11: the accent fades in (0.15 s) and snaps out. Asked every frame, so losing readiness resets it.
    let ctx = ui.ctx().clone();
    let secs = if clean {
        super::motion::fade_dur(&ctx, 0.15)
    } else {
        0.0
    };
    let t = ctx.animate_bool_with_time_and_easing(
        egui::Id::new("header-export-ready"),
        clean,
        secs,
        egui::emath::easing::cubic_out,
    );
    if t > 0.0 {
        let v = ui.visuals();
        export = export.fill(
            v.widgets
                .inactive
                .weak_bg_fill
                .lerp_to_gamma(v.selection.bg_fill, t),
        );
    }
    ui.allocate_ui_with_layout(size, egui::Layout::top_down(egui::Align::Center), |ui| {
        // A12: the hover counts the files and the problems. FB1: and, while it reads Review, what is still open.
        // Built lazily, only while hovered (ui-rules 9.3).
        let hover = |ui: &mut egui::Ui| {
            // Note FC5 (supersedes CZ6's hover): NIF edits are Review rows, so the Meshes view's line is gone.
            ui.label(export_hover(model));
            if !clean {
                let nifs = model.nif_review_rows().len();
                match model.review_gate() {
                    Err(why) => ui.label(format!("Review first: {why}")),
                    Ok(_) if model.undecided() == 0 && nifs > 0 => {
                        ui.label(format!("Review first: NIF edits ({nifs})"))
                    }
                    Ok(_) => ui.label(format!("Review first: {} undecided", model.undecided())),
                };
            }
        };
        let r = ui
            .add_enabled(can_export, export)
            .on_hover_ui(hover)
            .on_disabled_hover_ui(hover);
        // Export saves first: a focused field commits before it runs (Note FC8, supersedes BS14's refusal).
        super::commit_click(ui, &r, model, AppModel::export_clicked);
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
                egui::ProgressBar::new(shown_progress(ui.ctx(), model.drawer_epoch, frac))
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

/// Export's hover (A12): "{n} files, {m} problems"; with an "Also export as" tick on, "{n} files to {k} folders, {m}
/// problems" (re-check A7, Note CN).
pub fn export_hover(model: &AppModel) -> String {
    let n = crate::model::bottom::flagged(model).ready_rows;
    let files = model.export_files();
    if model.also_on() {
        format!(
            "{files} files to {} folders, {n} problems",
            model.also_folder_count()
        )
    } else {
        format!("{files} files, {n} problems")
    }
}

/// Export to ▾'s two "Also export as" items (owner answer 5): checkable, and the menu stays open on a tick. Ticking
/// needs the cs mode (TA20); a tick left on stays clickable so it can be cleared (the model allows the untick).
fn also_items(
    ui: &mut egui::Ui,
    model: &AppModel,
    busy: Option<&str>,
) -> Option<(AlsoTarget, bool)> {
    let (v, c) = model.also_ticks();
    let mut out = None;
    for (t, on, text, hover) in [
        (
            AlsoTarget::Vanilla,
            v,
            "Also export as Vanilla",
            "Also write a Vanilla folder: the PBR sets converted to vanilla textures, the rest copied",
        ),
        (
            AlsoTarget::Cm,
            c,
            "Also export as Complex material",
            "Also write a Complex material folder (and the Vanilla folder it installs on)",
        ),
    ] {
        // Without a project the model's edit is a silent no-op: say why instead.
        let why = busy
            .map(str::to_string)
            .or_else(|| {
                model
                    .project
                    .is_none()
                    .then(|| "Open a folder first".to_string())
            })
            .or_else(|| model.toml_read_only_reason())
            .or_else(|| (!on && model.target != Target::Cs).then(|| also::NEEDS_CS.to_string()));
        let mut now = on;
        let r = ui.add_enabled(why.is_none(), egui::Checkbox::new(&mut now, text));
        let r = match &why {
            Some(w) => r.on_disabled_hover_text(w),
            None => r.on_hover_text(hover),
        };
        if r.changed() {
            out = Some((t, now));
        }
    }
    out
}

/// The PGPatcher JSON editor is offered: cs mode, a folder open and its `sdc-project.toml` loaded.
fn shows_json_editor(model: &AppModel) -> bool {
    model.target == Target::Cs && model.root.is_some() && model.sets_doc.is_some()
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

/// Open ▾ (R14): Open folder… (off while a file dialog is open), then the recent folders.
fn open_menu(ui: &mut egui::Ui, model: &mut AppModel) {
    let mut open = None;
    let mut pick = false;
    ui.menu_button("Open", |ui| {
        if picker::button(ui, &model.picker, egui::Button::new("Open folder…")) {
            pick = true;
            ui.close();
        }
        ui.separator();
        if model.settings.recent.is_empty() {
            ui.add_enabled(false, egui::Button::new("No recent folders"));
        }
        for p in &model.settings.recent {
            if ui.button(p.display().to_string()).clicked() {
                open = Some(p.clone());
                ui.close();
            }
        }
    });
    if pick {
        model.pick(
            ui.ctx(),
            PickerPurpose::OpenFolder,
            model.open_folder_spec(),
        );
    }
    if let Some(p) = open {
        model.request_open(p);
    }
}

/// One header Undo or Redo button as drawn: its text, its accessible name (the text unshortened; `Undo (NIF edit
/// history)` while the NIF history is the target), its hover (which holds the whole step).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoButton {
    pub text: String,
    pub name: String,
    pub hover: String,
    pub enabled: bool,
}

/// The header's Undo (`undo`) or Redo button for the history it acts on (Note FC12, CI23 labelled): `Undo` for the
/// Textures one, whose next step is `textures`, or `Undo NIF edit: {step}` while a NIF edit is the target (`nif`).
/// The hover names the history; a long NIF step is shortened on the button only.
pub fn undo_button(nif: Option<&UndoTarget>, undo: bool, textures: Option<&str>) -> UndoButton {
    const MAX: usize = 40;
    let verb = if undo { "Undo" } else { "Redo" };
    let keys = super::actions::keys_text(if undo { "undo" } else { "redo" });
    // (the button's full text, what the hover says it does, enabled, the history)
    let (full, does, enabled, history) = match nif {
        Some(t) => {
            let (can, label) = if undo {
                (t.can_undo, &t.undo_label)
            } else {
                (t.can_redo, &t.redo_label)
            };
            let name = match label.as_deref().filter(|_| can) {
                Some(l) => format!("{verb} NIF edit: {l}"),
                None => format!("{verb} NIF edit"),
            };
            (
                name.clone(),
                name,
                can,
                "the NIF edit history; the Textures history is untouched",
            )
        }
        None => (
            verb.to_string(),
            textures.map_or_else(String::new, |s| format!("{verb}: {s}")),
            textures.is_some(),
            "the Textures history (sdc.toml and sdc-project.toml)",
        ),
    };
    let hover = match (enabled, nif.is_some()) {
        (false, true) => format!("Nothing to {} in the NIF edit history", verb.to_lowercase()),
        (false, false) => format!("Nothing to {}", verb.to_lowercase()),
        (true, _) => format!("{does}  ({keys}). Acts on {history}."),
    };
    let text = if full.chars().count() > MAX {
        format!("{}…", full.chars().take(MAX - 1).collect::<String>())
    } else {
        full.clone()
    };
    // Fix round 1 (rule 8.4): the NIF panel's own buttons are `Undo NIF edit`, so the header's name says whose history
    // it is; the step is in the hover.
    let name = match nif {
        Some(_) => format!("{verb} (NIF edit history)"),
        None => full,
    };
    UndoButton {
        text,
        name,
        hover,
        enabled,
    }
}

/// The Undo and Redo buttons (item 2): disabled with no history, the step's label in the hover text.
fn undo_redo(ui: &mut egui::Ui, model: &mut AppModel) {
    // A20: the NIF history while a NIF edit is open in the Meshes view.
    let nif = super::meshes::undo_target(ui.ctx());
    // Note F?5 (A19): a fixed slot, so a long NIF step label never moves the controls after it. `Undo NIF edit` is the
    // widest text without a step (the Textures view draws `Undo` / `Redo`), so every no-step text fits whole.
    let slot = egui::vec2(
        button_width(ui, "Undo NIF edit").max(button_width(ui, "Redo NIF edit")),
        ui.spacing().interact_size.y,
    );
    for undo in [true, false] {
        let textures = if undo {
            model.undo_label()
        } else {
            model.redo_label()
        };
        let b = undo_button(nif.as_ref(), undo, textures);
        // The text truncates inside the slot; the hover holds the whole step and the name stays `b.name`.
        let r = ui
            .add_enabled_ui(b.enabled, |ui| {
                ui.add_sized(slot, egui::Button::new(&b.text).truncate())
            })
            .inner;
        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, b.enabled, &b.name));
        if r.on_hover_text(b.hover.clone())
            .on_disabled_hover_text(b.hover)
            .clicked()
            && !super::meshes::request_undo(ui.ctx(), undo)
        {
            if undo {
                model.undo();
            } else {
                model.redo();
            }
        }
    }
}

/// The project's preset (AM12): the target's default, or a preset of this target (a user preset appears once saved).
fn preset_combo(ui: &mut egui::Ui, model: &mut AppModel) {
    let default = format!(
        "default ({})",
        engine::presets::default_preset_for(model.target)
    );
    let cur = model.project_preset();
    let mode = target_name(model.target);
    let gap = ui.spacing().item_spacing.x;
    fit(
        ui,
        button_width(ui, "Preset") + gap + ui.spacing().combo_width,
    );
    ui.label("Preset");
    let mut picked = None;
    let mut presets = false;
    let has_project = model.project.is_some();
    let choices = model.preset_choices();
    // K5: the popup always opens, so Presets… is reachable with no folder; only the presets need a project.
    let mut body = |ui: &mut egui::Ui| {
        ui.add_enabled_ui(has_project, |ui| {
            let entry = |ui: &mut egui::Ui, on: bool, text: &str| {
                ui.selectable_label(on, text)
                    .on_disabled_hover_text("Open a folder first")
                    .clicked()
            };
            if entry(ui, cur.is_none(), &default) {
                picked = Some(None);
            }
            for id in &choices {
                if entry(ui, cur.as_deref() == Some(id.as_str()), id) {
                    picked = Some(Some(id.clone()));
                }
            }
        });
        ui.separator();
        if ui
            .button("Presets…")
            .on_hover_text("Make and edit your own presets (saved in %APPDATA%\\sdc\\presets)")
            .clicked()
        {
            presets = true;
        }
    };
    let combo = egui::ComboBox::from_id_salt("preset")
        .selected_text(cur.clone().unwrap_or_else(|| default.clone()))
        .show_ui(ui, |ui| body(ui))
        .response
        .on_hover_ui(|ui| {
            ui.label(format!(
                "The preset the folder converts with. Saved in sdc.toml as preset. Mode: {mode}; the list shows this mode's presets. Change the mode in Presets…."
            ));
        });
    // Note FC1 (AM14): a Problems row's `Choose preset…` opens the same menu under the combo, as a popup the header
    // owns (egui 0.36 has no public way to open a combo). A request is taken by the next pass that draws this combo
    // and honoured only within two passes of the click: one left while the combo was not drawn is dropped then, as
    // stale, and never opens the menu later.
    let now = ui.ctx().cumulative_pass_nr();
    let request = model.choose_preset.take().is_some_and(|at| now <= at + 2);
    egui::Popup::menu(&combo)
        .id(egui::Id::new("preset_choose_menu"))
        .open_memory(request.then_some(egui::SetOpenCommand::Bool(true)))
        .show(|ui| body(ui));
    if presets {
        model.open_presets();
    }
    if let Some(p) = picked.filter(|p| *p != cur) {
        model.set_preset(p);
    }
}

/// Top-level view toggle (plan A Task 12), kept in egui memory for the session. Note BH8: two tabs, taller than the
/// row, rounded on top only; the selected one is filled and bold. R21: they show "Textures" / "Meshes" and keep the
/// accessible names "Textures view" / "Meshes view".
pub fn view_toggle(ui: &mut egui::Ui) {
    let mut meshes = meshes_view_active(ui.ctx());
    let tab = |ui: &mut egui::Ui, on: bool, text: &str, name: &str| {
        let rich = egui::RichText::new(text);
        let w = button_width(ui, "Textures") + 12.0;
        let h = ui.spacing().interact_size.y + 6.0;
        let r = ui.add(
            egui::Button::selectable(on, if on { rich.strong() } else { rich })
                .frame_when_inactive(true)
                .corner_radius(egui::CornerRadius {
                    nw: 6,
                    ne: 6,
                    sw: 0,
                    se: 0,
                })
                .min_size(egui::vec2(w, h)),
        );
        let enabled = ui.is_enabled();
        r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, on, name));
        r
    };
    fit(ui, view_tabs_width(ui));
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        if tab(ui, !meshes, "Textures", "Textures view").clicked() {
            meshes = false;
        }
        if tab(ui, meshes, "Meshes", "Meshes view").clicked() {
            meshes = true;
        }
    });
    set_meshes_view(ui.ctx(), meshes);
}

/// The width `view_toggle` takes: two tabs and the spacing after them.
fn view_tabs_width(ui: &egui::Ui) -> f32 {
    2.0 * (button_width(ui, "Textures") + 12.0) + 2.0 + ui.spacing().item_spacing.x
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

/// Export to ▾ (A4): `text` shows the folder cut in the middle; the hover and the accessible name keep all of it (K8).
/// Without a chosen folder (`full` None) it is "Export to: not chosen ▾", or "More ▾" with no project (Note F?5). Entries: Choose export folder…, Open export folder
/// and, in cs mode, the PGPatcher JSON editor (R14).
fn export_to_menu(ui: &mut egui::Ui, model: &mut AppModel, text: String, full: Option<String>) {
    let mut choose = false;
    let mut open = false;
    let mut json = false;
    let mut marker = false;
    let mut tick = None;
    // A click inside keeps it open (egui's default closes on any click), so the ticks can be set without reopening;
    // the three entries below close it themselves.
    let r = super::path_picker::menu(ui, &text, |ui| {
        // AG129: the drawer's links point at the export folder, so it stays put while an export runs.
        let off = if model.picker.busy() {
            Some(picker::BUSY_HOVER)
        } else if model.export.running.is_some() {
            Some(EXPORT_RUNNING_HOVER)
        } else {
            None
        };
        let r = ui.add_enabled(off.is_none(), egui::Button::new("Choose export folder…"));
        let r = match off {
            Some(why) => r.on_disabled_hover_text(why),
            None => r.on_hover_text("Choose where converted textures are written"),
        };
        choose = r.clicked();
        // A2: stat only while the menu is open.
        let state = model.export_folder_state();
        let r = ui.add_enabled(state.is_ok(), egui::Button::new("Open export folder"));
        open = match &state {
            Ok(p) => r.on_hover_text(format!("Show {} in Explorer", p.display())),
            Err(why) => r.on_disabled_hover_text(why),
        }
        .clicked();
        let running = model
            .export
            .running
            .is_some()
            .then_some(EXPORT_RUNNING_HOVER);
        tick = also_items(ui, model, running);
        if shows_json_editor(model) {
            json = ui
                .button(JSON_EDITOR)
                .on_hover_text(
                    "Edit the whole PGPatcher JSON: its file name, every set's entry and the delete sections",
                )
                .clicked();
        }
        // Always offered: the file is saved wherever the user says, so it needs no project.
        marker = ui
            .add_enabled(!model.picker.busy(), egui::Button::new(MAKE_MARKER))
            .on_hover_text("Write a 1×1 marker cube (a solid F0 colour and roughness) that Community Shaders reads")
            .on_disabled_hover_text(picker::BUSY_HOVER)
            .clicked();
        if choose || open || json || marker {
            ui.close();
        }
    });
    let name = match (&full, model.root.is_some()) {
        (Some(f), _) => format!("Export to: {f}"),
        (None, true) => "Export to: not chosen".to_string(),
        (None, false) => "More export options".to_string(),
    };
    let enabled = ui.is_enabled();
    r.response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &name));
    let root_open = model.root.is_some();
    r.response.on_hover_text(match full {
        Some(f) => f,
        None if root_open => {
            "No export folder chosen yet: choose one here or in Review".to_string()
        }
        None => "Open a folder to choose where its textures are written".to_string(),
    });
    if choose {
        model.pick_export_folder(ui.ctx());
    }
    if open {
        // As the Files tab's: the entry is enabled only when the folder is there.
        let _ = model.open_export_folder();
    }
    if json {
        model.open_json_editor();
    }
    if marker {
        model.open_marker_dialog();
    }
    if let Some((t, on)) = tick {
        model.set_also_tick(t, on);
    }
}

fn status(ui: &mut egui::Ui, model: &mut AppModel) {
    if model.status_line.is_empty() && model.banner.is_none() {
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
