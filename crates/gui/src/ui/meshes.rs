//! The Meshes view (plan A Task 12): a flat list of meshes (Note GC) | shapes of the selected NIF. Draws `MeshesModel`; every store read happens in the model on a click, never here.
use crate::model::meshes::export::DocBadge;
use crate::model::meshes::nif_edit::{Selection, ANIM_INVALID, EDIT_HIDDEN};
use crate::model::meshes::pg_entries::{
    is_error, is_warning, INSTANCE_UNREADABLE, IN_GAME_SCOPE, NO_INSTANCE, PGP_ENV_SCALE,
    READING_JSON,
};
use crate::model::meshes::{
    alpha_line, display_path, flag_bits, overridden_text, plugins_line, shader_type_name,
    slot_name, Banner, MeshesModel, ScanTarget, DERIVED_CHIP, LE_WARNING, MISSING_NOTE,
    PLUGINS_NOT_READ_NOTE, SLSF1_NAMES, SLSF2_NAMES, STALE_CHIP, UNUSED_NOTE,
};
use crate::model::picker::{self, PickerPurpose, PickerSpec};
use eframe::egui;
use modscan::plugin_store::{record_label, MeshUse};
use modscan::store::{NifRow, ShapeView, TexResolution, KIND_BSA, KIND_DATA};
use std::collections::HashMap;

// Meshes UI T7: status colours come from `ui::status` (rule 4.1); blue is the "derived" chip, not a status.
use super::status::{AMBER, GREEN, RED};
const BLUE: egui::Color32 = egui::Color32::from_rgb(90, 140, 220);
/// The left column's narrowest width.
const TREE_MIN: f32 = 200.0;
/// The NIF edit panel's narrowest width, and the 3D view's narrowest beside it.
const EDIT_MIN: f32 = 300.0;
const EDIT_CENTRE_MIN: f32 = 240.0;
/// A side panel's frame adds its inner margin (`Frame::side_top_panel`, 8 + 8) to the width its range sets.
const PANEL_MARGIN_X: f32 = 16.0;
/// The edit panel's starting width.
const EDIT_DEFAULT: f32 = 380.0;
/// The left column's widest share of the window (T7 review Minor 1).
const TREE_MAX_SHARE: f32 = 0.4;

mod curve;
mod edit_ctrl;
mod edit_extra;
mod edit_fields;
mod edit_panel;
/// Problems listed under "Scan details"; the count is always shown.
const MAX_LISTED: usize = 200;

fn undo_ids() -> (egui::Id, egui::Id) {
    (
        egui::Id::new("nif-undo-target"),
        egui::Id::new("nif-undo-request"),
    )
}

/// The NIF history the shared Undo / Redo act on (Note GD4): what each can do, and the name of the step it would
/// undo or redo ("Set shader type").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoTarget {
    pub can_undo: bool,
    pub can_redo: bool,
    pub undo_label: Option<String>,
    pub redo_label: Option<String>,
}

/// A20 (owner option 1): which history the shared Undo / Redo (header, palette, Ctrl+Z / Ctrl+Y) act on now.
/// `Some` with the NIF history's state while a NIF edit is open in the Meshes view, `None` for Textures.
pub fn undo_target(ctx: &egui::Context) -> Option<UndoTarget> {
    if !super::header::meshes_view_active(ctx) {
        return None;
    }
    ctx.data(|d| d.get_temp::<Option<UndoTarget>>(undo_ids().0))
        .flatten()
}

/// Undoes (or redoes) the NIF edit when it is the [`undo_target`]; false means the Textures history is.
pub fn request_undo(ctx: &egui::Context, undo: bool) -> bool {
    if undo_target(ctx).is_none() {
        return false;
    }
    // Run at the next `show` (this frame's for the header and the keys, the next for the palette).
    ctx.data_mut(|d| d.insert_temp(undo_ids().1, undo));
    ctx.request_repaint();
    true
}

pub fn show(ui: &mut egui::Ui, m: &mut MeshesModel) {
    let (target, request) = undo_ids();
    match ui.ctx().data_mut(|d| d.remove_temp::<bool>(request)) {
        Some(true) => {
            m.undo_edit();
        }
        Some(false) => {
            m.redo_edit();
        }
        None => {}
    }
    draw(ui, m);
    // After the draw: the panel may have opened or closed the edit.
    let state = m.edit.as_ref().map(|e| UndoTarget {
        can_undo: e.history.can_undo(),
        can_redo: e.history.can_redo(),
        undo_label: e.history.undo_label().map(str::to_string),
        redo_label: e.history.redo_label().map(str::to_string),
    });
    ui.ctx().data_mut(|d| d.insert_temp(target, state));
}

fn draw(ui: &mut egui::Ui, m: &mut MeshesModel) {
    if bar_has_content(m) {
        egui::Panel::top("meshes-bar").show(ui, |ui| bar(ui, m));
    }
    // Meshes UI T7 (rule 2.3): the column leaves the 3D view its minimum, and the edit panel its own while editing;
    // on a window too small for all three the minimum wins.
    // The edit panel's width as drawn last frame (its frame margins included), else its default (T7 review Minor 2:
    // the frame an edit opens has no width yet).
    // shortcut: after the user drags the edit panel wider, this cap is one frame stale; use the panel's response if
    // the 3D view's dip below its minimum for that frame shows.
    let edit_w = egui::containers::panel::PanelState::load(ui.ctx(), egui::Id::new("meshes-edit"))
        .map_or(EDIT_DEFAULT + PANEL_MARGIN_X, |p| {
            p.outer_rect.width().max(EDIT_MIN)
        });
    let others = EDIT_CENTRE_MIN + if m.edit.is_some() { edit_w } else { 0.0 };
    // T7 review Minor 1 (controller ruling): also at most ~40% of the width, so the 3D view gets the slack.
    let avail = ui.available_width();
    let max = (avail - others - PANEL_MARGIN_X)
        .min(TREE_MAX_SHARE * avail)
        .max(TREE_MIN);
    egui::Panel::left("meshes-tree")
        .default_size(420.0)
        .size_range(TREE_MIN..=max)
        .show(ui, |ui| tree(ui, m));
    if m.edit.is_some() {
        // The 3D view keeps at least EDIT_CENTRE_MIN; when the window is too small the panel's minimum wins.
        let max = (ui.available_width() - EDIT_CENTRE_MIN - PANEL_MARGIN_X).max(EDIT_MIN);
        egui::Panel::right("meshes-edit")
            .default_size(EDIT_DEFAULT)
            .size_range(EDIT_MIN..=max)
            .show(ui, |ui| edit_panel::panel(ui, m));
    } else {
        ui.skip_ahead_auto_ids(1);
    }
    egui::CentralPanel::default().show(ui, |ui| detail(ui, m));
    dialogs(ui.ctx(), m);
}

fn chip(ui: &mut egui::Ui, color: egui::Color32, text: &str) -> egui::Response {
    ui.label(egui::RichText::new(text).small().color(color))
}

/// The header's "MO2…" button (Note AM): a popup with the instance and its Choose…, the profiles, Scan a folder…
/// and Scan or Cancel. A click inside keeps it open; a click outside or Esc closes it.
pub fn mo2_button(ui: &mut egui::Ui, m: &mut MeshesModel) {
    let hover = m.source_text();
    let r = super::path_picker::menu(ui, "MO2…", |ui| {
        let (idle, busy) = (!m.scanning(), m.picker.busy());
        let pick = |ui: &mut egui::Ui, label: &str| {
            let r = ui.add_enabled(idle && !busy, egui::Button::new(label));
            (if busy {
                r.on_disabled_hover_text(picker::BUSY_HOVER)
            } else {
                r
            })
            .clicked()
        };
        ui.horizontal(|ui| {
            let shown = match &m.target {
                Some(ScanTarget::Mo2 { root, .. }) => format!("Instance: {}", root.display()),
                _ => "Instance: none".into(),
            };
            ui.label(shown);
            if pick(ui, "Choose…") {
                m.picker.request(
                    ui.ctx(),
                    PickerPurpose::ChooseMo2,
                    None,
                    PickerSpec::folder(),
                );
            }
        });
        if let Some(ScanTarget::Mo2 { root, profile }) = m.target.clone() {
            ui.label("Profile:");
            let mut picked = None;
            ui.add_enabled_ui(idle, |ui| {
                for p in m.profiles() {
                    if ui.selectable_label(*p == profile, p).clicked() && *p != profile {
                        picked = Some(p.clone());
                    }
                }
            });
            if let Some(p) = picked {
                m.use_instance(root, p);
            }
        }
        ui.separator();
        if pick(ui, "Scan a folder…") {
            m.picker.request(
                ui.ctx(),
                PickerPurpose::ScanFolder,
                None,
                PickerSpec::folder(),
            );
        }
        if m.scanning() {
            if ui.button("Cancel").clicked() {
                m.cancel_scan();
            }
        } else if ui
            .add_enabled(
                m.target.is_some(),
                egui::Button::new(egui::RichText::new("Scan").strong()),
            )
            .clicked()
        {
            m.request_scan(false);
        }
    });
    r.response.on_hover_text(hover);
}

/// The scan's progress and ETA with its Cancel, while a scan runs: shared by the Meshes view's bar and the Textures
/// view's scan row.
pub fn scan_controls(ui: &mut egui::Ui, m: &mut MeshesModel) {
    ui.horizontal_wrapped(|ui| {
        if m.scanning() {
            ui.spinner();
            if let (Some((_, done, total)), Some(text)) =
                (m.progress, m.scan_line(std::time::Instant::now()))
            {
                let frac = if total == 0 {
                    0.0
                } else {
                    done as f32 / total as f32
                };
                ui.add(egui::ProgressBar::new(frac).desired_width(260.0).text(text));
            }
            if ui.button("Cancel").clicked() {
                m.cancel_scan();
            }
        }
    });
}

/// The Textures view draws its scan row only when it has something to show (Note AM: the source moved to MO2…).
pub fn scan_row_shown(m: &MeshesModel) -> bool {
    m.scanning() || !m.status.is_empty() || m.error.is_some()
}

/// The Meshes dialogs (the MO2 question can come from the header's MO2… button in either view).
pub fn scan_dialogs(ctx: &egui::Context, m: &mut MeshesModel) {
    dialogs(ctx, m);
}

/// The Textures view's scan row: the progress, the status, and an error. The details, banners and plugin references
/// stay in the Meshes view.
pub fn scan_row(ui: &mut egui::Ui, m: &mut MeshesModel) {
    if m.scanning() {
        scan_controls(ui, m);
    }
    if !m.status.is_empty() {
        ui.weak(&m.status);
    }
    if let Some(e) = m.error.clone() {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(RED, e);
            if ui.small_button("Dismiss").clicked() {
                m.error = None;
            }
        });
    }
}

/// Whether `bar` draws anything; an empty bar strip is not drawn (layout v2 ask). Keep in step with `bar`.
fn bar_has_content(m: &MeshesModel) -> bool {
    let details = m.report.as_ref().is_some_and(|r| {
        !r.notes.is_empty()
            || !r.missing_mods.is_empty()
            || !r.problems.is_empty()
            || r.plugins.is_some()
    });
    m.scanning()
        || !m.status.is_empty()
        || details
        || m.has_store()
        || m.file_refs.is_some()
        || m.error.is_some()
        || m.banner.is_some()
}

fn bar(ui: &mut egui::Ui, m: &mut MeshesModel) {
    if m.scanning() {
        scan_controls(ui, m);
    }
    if !m.status.is_empty() {
        ui.label(&m.status);
    }
    if let Some(r) = &m.report {
        if !r.notes.is_empty()
            || !r.missing_mods.is_empty()
            || !r.problems.is_empty()
            || r.plugins.is_some()
        {
            egui::CollapsingHeader::new("Scan details")
                .id_salt("meshes-scan-details")
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(160.0)
                        .show(ui, |ui| {
                            if let Some(p) = &r.plugins {
                                ui.label(plugins_line(p));
                                for (plugin, msg) in p.problems.iter().take(MAX_LISTED) {
                                    ui.label(format!("Plugin {plugin}: {msg}"));
                                }
                                if p.problems.len() > MAX_LISTED {
                                    ui.label(format!(
                                        "… and {} more plugin problems",
                                        p.problems.len() - MAX_LISTED
                                    ));
                                }
                            }
                            for n in &r.notes {
                                ui.label(format!("Note: {n}"));
                            }
                            for n in &r.missing_mods {
                                ui.label(format!("Missing mod folder: {n}"));
                            }
                            for (at, msg) in r.problems.iter().take(MAX_LISTED) {
                                ui.label(format!("{at}: {msg}"));
                            }
                            if r.problems.len() > MAX_LISTED {
                                ui.label(format!("… and {} more", r.problems.len() - MAX_LISTED));
                            }
                        });
                });
        }
    }
    if (m.has_store() || m.file_refs.is_some()) && ui.button("Plugin file references…").clicked()
    {
        m.refs_open = true;
    }
    if let Some(e) = m.error.clone() {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(RED, e);
            if ui.small_button("Dismiss").clicked() {
                m.error = None;
            }
        });
    }
    if let Some(b) = m.banner.clone() {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(AMBER, b.text());
            match b {
                Banner::PgpRunning => {
                    if ui.button("Retry").clicked() {
                        m.request_scan(false);
                    }
                    if ui.button("Scan without PGPatcher output").clicked() {
                        m.request_scan(true);
                    }
                }
                Banner::PgpStale(_) => {
                    if ui.button("Show list").clicked() {
                        m.show_stale_list();
                    }
                }
                Banner::PgpIncomplete(_) => {}
            }
        });
    }
}

/// A NIF row's text: the path with its `meshes/` dimmed, so a row like `!ube/…` reads as a path (review C17).
fn nif_label(ui: &egui::Ui, path: &str) -> egui::text::LayoutJob {
    let rest = display_path(path);
    let font = egui::TextStyle::Body.resolve(ui.style());
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &path[..path.len() - rest.len()],
        0.0,
        egui::TextFormat::simple(font.clone(), ui.visuals().weak_text_color()),
    );
    // PLACEHOLDER takes the widget's own text colour, so a selected row keeps egui's selected colour.
    job.append(
        rest,
        0.0,
        egui::TextFormat::simple(font, egui::Color32::PLACEHOLDER),
    );
    job
}

/// One cell of a list row: `width` wide, its text cut to fit (rule 3.6), drawn left to right.
fn cell<R>(ui: &mut egui::Ui, width: f32, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let h = ui.spacing().interact_size.y;
    ui.allocate_ui_with_layout(
        egui::vec2(width, h),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_max_width(width);
            ui.set_clip_rect(ui.max_rect().intersect(ui.clip_rect()));
            add(ui)
        },
    )
    .inner
}

/// Note GC (owner, supersedes GUI tweaks G9's mods tree): one flat list of meshes, Path | Mod | status chips, with
/// one search box and the filter chips over it.
fn tree(ui: &mut egui::Ui, m: &mut MeshesModel) {
    // Meshes UI T7 (rule 3.2): thin bars that stay visible without hover, for every list in this column.
    ui.spacing_mut().scroll = egui::style::ScrollStyle::thin();
    // Rule 3.6: long paths and mod names are cut, not let widen the column.
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
    // Meshes UI T2 (owner decision 2): while Edit is open the column stays on the shapes, the only way to pick one.
    let locked = m.edit.is_some();
    let can = m.selected_row().is_some() && !locked;
    if locked {
        m.shapes_col = true;
    } else if !can {
        m.shapes_col = false;
    }
    ui.horizontal(|ui| {
        ui.heading("Meshes");
        let on = m.shapes_col;
        let r = ui.add_enabled(can, egui::Button::selectable(on, "Shapes"));
        let r = r.on_disabled_hover_text(if locked {
            "Close Edit to see the mesh list"
        } else {
            "Select a mesh first"
        });
        let r = r.on_hover_text("Show the shape list instead of the mesh list");
        if r.clicked() {
            m.shapes_col = !on;
        }
    });
    if m.shapes_col {
        shapes_column(ui, m);
        return;
    }
    if m.poll_rows() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(50));
    }
    if m.mods.is_empty() {
        ui.label("No scan yet.");
        return;
    }
    ui.add(
        egui::TextEdit::singleline(&mut m.search)
            .hint_text("Search paths and mods")
            .desired_width(f32::INFINITY),
    );
    mod_picker(ui, m);
    ui.horizontal_wrapped(|ui| {
        for f in crate::model::meshes::NifFilter::ALL {
            // Note GC: every row of "Any mod" wins, so Wins is offered only with a picked mod.
            if f == crate::model::meshes::NifFilter::Wins && m.selected_mod.is_none() {
                continue;
            }
            if ui.selectable_label(m.filter == f, f.label()).clicked() {
                m.filter = f;
            }
        }
        // The texture chip (Note GC): set by "Show in Meshes"; its ✕ clears it.
        if let Some((tex, users)) = &m.tex_filter {
            let waiting = users.is_none();
            let chip = egui::Button::selectable(true, format!("Uses {tex} ✕")).truncate();
            let r = ui
                .add(chip)
                .on_hover_text("Show every mesh again, not only those that use this texture");
            // Note GC15: an unsettled store reads no users, so the chip waits instead of emptying the list.
            if waiting {
                ui.weak("cuts the list after a completed scan");
            }
            if r.clicked() {
                m.set_tex_filter(None);
            }
        }
    });
    if m.selected_mod.is_none() && m.rows_loading() {
        ui.weak("Reading the scan…");
        return;
    }
    let visible = m.visible_nifs();
    // Rule 6.2: the filters hide rows and never deselect, so say when the selected NIF is one of the hidden.
    ui.weak(if m.selection_hidden() {
        format!(
            "{} of {} NIFs (the selected NIF is hidden by the filters)",
            visible.len(),
            m.listed()
        )
    } else {
        format!("{} of {} NIFs", visible.len(), m.listed())
    });
    let row_h = ui.spacing().interact_size.y;
    // Rule 3.7: a few pixels short of the column, so a full row never flips the scroll bar.
    let gap = ui.spacing().item_spacing.x;
    let w = ui.available_width() - 8.0 - 2.0 * gap;
    let (mod_w, chips_w) = ((0.25 * w).min(180.0), (0.25 * w).min(200.0));
    let path_w = (w - mod_w - chips_w).max(0.0);
    let mut picked = None;
    egui::ScrollArea::vertical()
        .id_salt("meshes-tree-scroll")
        .auto_shrink([false, false])
        .show_rows(ui, row_h, visible.len(), |ui, range| {
            for &i in &visible[range] {
                let (n, origin) = m.nif_at(i);
                ui.horizontal(|ui| {
                    let r = cell(ui, path_w, |ui| {
                        let b = egui::Button::selectable(
                            m.selected_nif == Some(n.copy_id),
                            nif_label(ui, &n.path),
                        )
                        .truncate()
                        .min_size(egui::vec2(path_w, 0.0));
                        ui.add(b)
                    });
                    // Rules 3.4, 9.3: the whole path on hover, built only while hovered.
                    if r.on_hover_ui(|ui| {
                        ui.label(n.path.as_str());
                    })
                    .clicked()
                    {
                        picked = Some(n.copy_id);
                    }
                    cell(ui, mod_w, |ui| ui.weak(origin));
                    cell(ui, chips_w, |ui| chips_cell(ui, m, n, origin));
                });
            }
        });
    // While a scan runs the store's winner tables are being rewritten: the selection waits for the result.
    if let Some(c) = picked.filter(|_| !m.scanning()) {
        m.select_nif(c);
    }
}

/// The mod picker (Note GC): "Any mod" lists every path's winner; a mod lists its own copies. Greyed while a scan
/// runs, as the list's selection is.
fn mod_picker(ui: &mut egui::Ui, m: &mut MeshesModel) {
    let text = match m.selected_mod {
        Some(id) => m.mod_name(id).to_string(),
        None => "Any mod".to_string(),
    };
    let mut pick = None;
    // Rule 3.5: the combo's width is a minimum, so it gets an exact slot.
    let size = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
    ui.allocate_ui(size, |ui| {
        ui.add_enabled_ui(!m.scanning(), |ui| {
            egui::ComboBox::from_id_salt("meshes-mod-picker")
                .selected_text(text)
                .width(size.x)
                .truncate()
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(m.selected_mod.is_none(), "Any mod")
                        .clicked()
                    {
                        pick = Some(None);
                    }
                    for r in m.picker_mods() {
                        if ui
                            .selectable_label(m.selected_mod == Some(r.id), &r.name)
                            .clicked()
                        {
                            pick = Some(Some(r.id));
                        }
                    }
                })
                .response
                .on_disabled_hover_text("Available after the scan finishes");
        });
    });
    match pick {
        Some(Some(id)) if m.selected_mod != Some(id) => m.select_mod(id),
        Some(None) if m.selected_mod.is_some() => m.any_mod(),
        _ => {}
    }
}

/// A row's chips, warnings first (Note GC16): the problem, the out-of-date patch, LE, the edit badge, the status, a
/// PGPatcher copy, then the mod's own chips (generated by PGPatcher, a BSA, the game Data folder). Each is
/// (colour, the short text the row shows, the full text).
fn row_chips(
    m: &MeshesModel,
    n: &NifRow,
    origin: &str,
) -> Vec<(egui::Color32, &'static str, &'static str)> {
    let mut v = Vec::new();
    if n.problem.is_some() {
        v.push((RED, "Problem", "Problem"));
    }
    if n.derive_check
        .as_deref()
        .is_some_and(|c| c.starts_with("stale"))
    {
        v.push((AMBER, "Stale patch", STALE_CHIP));
    }
    if n.le_mesh {
        v.push((AMBER, "LE", "LE"));
    }
    if let Some(b) = m.doc_badges.get(&n.path).copied() {
        let color = match b {
            DocBadge::Pending => AMBER,
            DocBadge::Exported => GREEN,
            DocBadge::Conflict => RED,
        };
        v.push((color, b.chip(), b.chip()));
    }
    let status = n.status_label();
    if n.wins {
        v.push((GREEN, status, status));
    } else if n.patched_in_game {
        v.push((BLUE, "In game", status));
    } else {
        v.push((egui::Color32::GRAY, status, status));
    }
    if n.derived {
        v.push((BLUE, "PGPatcher", "PGPatcher"));
    }
    // shortcut: a scan of `mods` per drawn row (only the rows on screen); index it by name if mod lists grow past
    // a few thousand.
    if let Some(r) = m.mods.iter().find(|r| r.name == origin) {
        if r.derived {
            v.push((BLUE, "Generated", DERIVED_CHIP));
        }
        match r.kind {
            KIND_BSA => v.push((egui::Color32::GRAY, "BSA", "BSA")),
            KIND_DATA => v.push((egui::Color32::GRAY, "game Data", "game Data")),
            _ => {}
        }
    }
    v
}

/// Draws a row's chips in its cell; those past the cell's edge are cut, so hovering the cell lists every chip in
/// full, with the copies and the problem's text (rules 3.4, 9.3). A shortened chip keeps its full text as its
/// AccessKit name (rule 8.1).
fn chips_cell(ui: &mut egui::Ui, m: &MeshesModel, n: &NifRow, origin: &str) {
    let chips = row_chips(m, n, origin);
    for &(color, short, full) in &chips {
        let r = chip(ui, color, short);
        if short != full {
            r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, full));
        }
    }
    let cover = ui.interact(
        ui.max_rect(),
        ui.id().with("row-chips"),
        egui::Sense::hover(),
    );
    cover.on_hover_ui(|ui| {
        for &(color, _, full) in &chips {
            ui.label(egui::RichText::new(full).color(color));
        }
        if let Some(by) = &n.overridden_by {
            ui.weak(format!("Overridden by {by} · {} copies", n.n_copies));
        } else if n.n_copies > 1 {
            ui.weak(format!("{} copies", n.n_copies));
        }
        if let Some(p) = &n.problem {
            ui.weak(p);
        }
    });
}

fn detail(ui: &mut egui::Ui, m: &mut MeshesModel) {
    let row = m.selected_row().cloned();
    if row.is_none() && m.shapes.is_empty() {
        ui.centered_and_justified(|ui| ui.label("Select a NIF."));
        return;
    }
    if let Some(n) = &row {
        let editing = m.edit.is_some();
        let mut toggle = false;
        egui::Sides::new().shrink_left().truncate().show(
            ui,
            |ui| {
                ui.add(
                    egui::Label::new(egui::RichText::new(display_path(&n.path)).heading())
                        .truncate(),
                );
            },
            |ui| {
                toggle = ui
                    .add(egui::Button::selectable(editing, "Edit"))
                    .on_hover_text(if editing {
                        "Close the NIF edit panel"
                    } else {
                        "Edit this NIF's shaders, alpha and extra data"
                    })
                    .clicked();
            },
        );
        if toggle {
            if editing {
                m.close_edit();
            } else {
                m.open_edit();
                // The session starts on Root: show the cards so a shape can be picked.
                m.shapes_col |= m.edit.is_some();
            }
        }
        if n.le_mesh {
            ui.colored_label(AMBER, LE_WARNING);
        }
        if let Some(p) = &n.problem {
            ui.colored_label(RED, format!("Problem: {p}"));
        }
        if let Some(by) = &n.overridden_by {
            ui.label(format!("Overridden by {by}"));
        }
        if let Some(c) = &n.derive_check {
            ui.label(format!("PGPatcher check: {c}"));
        }
        plugin_overrides(ui, m);
    }
    // v2 B8: during an edit the toggle always shows; In game is pgpsim over the edited NIF.
    let editing = m.edit.as_ref().is_some_and(|e| !e.le_mesh);
    if m.pair.is_some() || editing {
        let (game, source) = match editing {
            true => ("In game (simulated)", "Edited source"),
            false => ("In game", "Source"),
        };
        // Wrapped: the two edit-mode labels are wider than the centre column at 1100 px.
        ui.horizontal_wrapped(|ui| {
            let r = ui.selectable_label(!m.view_source, game);
            // Note GD7: the simulated side says what it simulates.
            let r = match editing {
                true => r.on_hover_text(pgpsim::SIMULATED_AS),
                false => r,
            };
            if r.clicked() {
                m.set_view_source(false);
            }
            if ui.selectable_label(m.view_source, source).clicked() {
                m.set_view_source(true);
            }
        });
        if let Some(e) = m.edit.as_ref().filter(|_| editing && !m.view_source) {
            if m.pg_reading() {
                ui.add(egui::Label::new(READING_JSON).wrap());
                // The worker's result arrives through `poll`, which needs a frame to run.
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(100));
            } else if let Some(err) = m.pg_error() {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(format!("{INSTANCE_UNREADABLE} {err}")).color(AMBER),
                    )
                    .wrap(),
                );
            } else {
                let scope = match m.target {
                    Some(ScanTarget::Mo2 { .. }) => IN_GAME_SCOPE,
                    _ => NO_INSTANCE,
                };
                ui.add(egui::Label::new(scope).wrap());
            }
            if let (Selection::Shape(b), Some(rows)) = (e.selection, &e.in_game) {
                for n in rows
                    .iter()
                    .filter(|(id, _, _)| id.0 == b)
                    .flat_map(|(_, _, n)| n)
                {
                    let mut t = egui::RichText::new(n.as_str());
                    if n == PGP_ENV_SCALE || is_error(n) {
                        t = t.color(RED);
                    } else if is_warning(n) {
                        t = t.color(AMBER);
                    }
                    ui.add(egui::Label::new(t).wrap());
                }
            }
        }
        // R11 (kept by v2 B8, H9): the game still loads PGPatcher's copy, which was made from the unedited NIF.
        if m.edit
            .as_ref()
            .is_some_and(|e| e.doc.export.ops_hash.is_some())
        {
            ui.colored_label(
                AMBER,
                "PGPatcher output predates your edit: re-run PGPatcher.",
            );
        }
        if m.edit_preview_hidden() {
            ui.label(EDIT_HIDDEN);
        }
    }
    // v2 B3: the controls first (a click this frame is ticked this frame), then the tick, then the 3D view, whose
    // request_frame then carries the new time and materials: one Render per frame.
    // B-P33: non-finite controller times get no clock and no slider, only this line.
    let invalid = m.edit.as_ref().is_some_and(|e| e.anim_invalid());
    // No clock controls In game (simulated): the clock is paused there (F21).
    if m.v3.nif_view && m.view_source && (m.v3.clock.is_some() || invalid) {
        egui::Panel::bottom("meshes-3d-anim")
            .frame(egui::Frame::new())
            .resizable(false)
            .show(ui, |ui| {
                if invalid {
                    ui.colored_label(AMBER, ANIM_INVALID);
                } else {
                    super::viewport3d::clock_controls(ui, &mut m.v3);
                }
            });
    } else {
        ui.skip_ahead_auto_ids(1);
    }
    // One step per frame: a discarded first pass must not advance the clock twice.
    let dt = if ui.ctx().current_pass_index() == 0 {
        ui.input(|i| i.stable_dt)
    } else {
        0.0
    };
    m.tick_anim(dt);
    let image = super::viewport3d::show(ui, &mut m.v3);
    // Note GH9 (Q1): a plain right-click on the frame while a NIF edit is open. egui opens a context menu only on a
    // secondary click, not a drag, so the right-drag orbit and the Shift+right-drag sun are untouched.
    if let Some(r) = image.filter(|_| m.edit.is_some()) {
        r.context_menu(|ui| quick_export_menu(ui, m));
    }
}

/// Note GH9 (Q2-Q4): the 3D view's menu. Export this NIF writes the open edit without Review, and is off with its
/// reason on hover; with no export folder chosen Choose export folder… opens the header's picker.
fn quick_export_menu(ui: &mut egui::Ui, m: &mut MeshesModel) {
    let block = m.quick_export_block();
    let r = ui.add_enabled(block.is_none(), egui::Button::new("Export this NIF"));
    let r = match &block {
        Some(why) => r.on_disabled_hover_text(why),
        None => r.on_hover_text("Write this NIF to the export folder now, without Review"),
    };
    if r.clicked() {
        m.quick_export();
        ui.close();
    }
    if m.nif_export_root.is_none() {
        let r = ui.add_enabled(
            !m.export_running,
            egui::Button::new("Choose export folder…"),
        );
        let r = match m.export_running {
            true => r.on_disabled_hover_text("An export is running"),
            false => r.on_hover_text("Choose where converted textures and NIF edits are written"),
        };
        if r.clicked() {
            m.pick_export_folder_request = true;
            ui.close();
        }
    }
}

/// The shape cards of the selected NIF, in the left column (owner, 2026-10-14). A click on a texture asks who else
/// uses it.
fn shapes_column(ui: &mut egui::Ui, m: &mut MeshesModel) {
    // Meshes UI T8: the Controllers lines outside Edit come from a worker; look for its answer while one runs.
    if m.poll_card_ctrls() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(50));
    }
    let scan_ctrls = m.shown_copy().and_then(|c| m.scan_ctrls.get(&c).cloned());
    let (mut clicked, mut open, mut pick, mut pick_root) = (None, None, None, None);
    egui::ScrollArea::vertical()
        .id_salt("meshes-shapes-scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if let Some((path, users)) = &m.tex_users {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new(format!("Users of {path}")).strong());
                        open_button(ui, m, path, &mut open);
                    });
                    if users.is_empty() {
                        ui.label("No NIF of the latest scan uses it.");
                    }
                    for (nif, slot, kind) in users.iter().take(MAX_LISTED) {
                        ui.label(format!(
                            "{} · slot {slot} ({})",
                            display_path(nif),
                            slot_name(*kind, *slot as usize)
                        ));
                    }
                    if users.len() > MAX_LISTED {
                        ui.label(format!("… and {} more", users.len() - MAX_LISTED));
                    }
                });
            }
            // Edit mode (spec §7): a Root card first, and a click on a card's title selects it for the panel.
            let editing = m.edit.as_ref().map(|e| e.selection);
            if let Some(sel) = editing {
                let on = sel == Selection::Root;
                card(ui, on, |ui| {
                    let r = ui
                        .add(egui::Button::selectable(
                            on,
                            egui::RichText::new("Root").strong(),
                        ))
                        .on_hover_text("The root node: its extra data");
                    if r.clicked() {
                        pick_root = Some(Selection::Root);
                    }
                });
                ui.add_space(CARD_GAP);
            }
            // While editing, the edited copy's shapes (the shown copy may be PGPatcher's, numbered differently); each
            // with its card from the edited bytes (Note CZ1), else a title only. The lookup is built once a frame.
            let cards: Vec<(u32, Option<&ShapeView>, Option<&str>, String)> = match &m.edit {
                Some(e) => {
                    let by: HashMap<(u32, Option<&str>), &ShapeView> = e
                        .cards
                        .iter()
                        .map(|c| ((c.block, c.name.as_deref()), c))
                        .collect();
                    e.model
                        .shapes
                        .iter()
                        .map(|s| {
                            let card = by.get(&(s.block, s.name.as_deref())).copied();
                            (s.block, card, s.name.as_deref(), format!("{:?}", s.kind))
                        })
                        .collect()
                }
                None => m
                    .shapes
                    .iter()
                    .map(|s| (s.block, Some(s), s.name.as_deref(), String::new()))
                    .collect(),
            };
            if cards.is_empty() {
                ui.label("No shapes.");
            }
            for (block, view, name, fallback) in cards {
                let on = match (editing, &m.card_sel) {
                    (Some(sel), _) => sel == Selection::Shape(block),
                    (None, Some((b, n))) => *b == block && n.as_deref() == name,
                    (None, None) => false,
                };
                // Inside Edit the edited model's lines (unsaved edits included); outside, the file's (T8).
                let ctrls = match &m.edit {
                    Some(e) => e.card_ctrls.get(&block),
                    None => scan_ctrls.as_ref().and_then(|c| c.get(&block)),
                }
                .map(String::as_str);
                card(ui, on, |ui| {
                    let title = match view {
                        Some(s) => {
                            let (p, title) = shape(ui, m, s, &mut open, on, ctrls);
                            if p.is_some() {
                                clicked = p;
                            }
                            title
                        }
                        None => {
                            // No scanned card, so no 3D index: "#–" gives every header the same form (T2 review
                            // Minor 5).
                            let title = format!("#– {}", name.unwrap_or("(unnamed)"));
                            let label = format!("Select shape {title}");
                            header(ui, on, &title, &fallback, &label, block)
                        }
                    };
                    if title {
                        pick = Some((block, name.map(String::from)));
                    }
                });
                ui.add_space(CARD_GAP);
            }
        });
    if let Some(p) = clicked {
        m.show_users(&p);
    }
    match (pick, m.edit.is_some()) {
        (Some((block, name)), true) => {
            m.select_edit(Selection::Shape(block));
            // T2 review Minor 2: the outline and the next Edit follow this pick. T4 review Minor 5: kept in the shown
            // copy's numbering (it may be PGPatcher's): the same block and name, else the only card with that name.
            let same = m
                .shapes
                .iter()
                .any(|s| s.block == block && s.name.as_deref() == name.as_deref());
            let mut named = m
                .shapes
                .iter()
                .filter(|s| name.is_some() && s.name.as_deref() == name.as_deref());
            let shown = match (same, named.next(), named.next()) {
                (false, Some(s), None) => s.block,
                _ => block,
            };
            m.card_sel = Some((shown, name));
        }
        (Some(p), false) => m.card_sel = Some(p),
        (None, _) => {}
    }
    if let Some(s) = pick_root {
        m.select_edit(s);
        // Final review Minor 2: the next Edit opens on Root too (no card picked).
        m.card_sel = None;
    }
    if open.is_some() {
        m.open_in_textures = open;
    }
}

/// The gap between two shape cards.
const CARD_GAP: f32 = 8.0;

/// A shape card's frame (Meshes UI T2: every card, in both modes): the selected card is outlined and tinted with the
/// selection colour, the others get the plain group stroke.
fn card(ui: &mut egui::Ui, selected: bool, add: impl FnOnce(&mut egui::Ui)) {
    let frame = egui::Frame::new().inner_margin(6.0).corner_radius(4.0);
    let frame = match selected {
        true => {
            let s = ui.visuals().selection;
            frame.stroke(s.stroke).fill(s.bg_fill.gamma_multiply(0.15))
        }
        false => frame.stroke(ui.visuals().widgets.noninteractive.bg_stroke),
    };
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui)
    });
}

/// A card's full-width header button: `title` and `kind` shown, `name` for AccessKit ("Select shape #3 Body"), the NIF block on
/// hover (built only while hovered). True when clicked.
fn header(ui: &mut egui::Ui, on: bool, title: &str, kind: &str, name: &str, block: u32) -> bool {
    // The title on the left, the block type on the right (a grow atom between them): weak on an unselected card,
    // plain on the selection fill, where weak grey is hard to read (final review Minor 4).
    let kind = match on {
        true => egui::RichText::new(kind),
        false => egui::RichText::new(kind).weak(),
    };
    let b = egui::Button::selectable(on, egui::RichText::new(title).strong())
        .right_text(kind)
        .truncate()
        .min_size(egui::vec2(ui.available_width(), 0.0));
    let r = ui.add(b);
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, name));
    let clicked = r.clicked();
    r.on_hover_ui(|ui| {
        ui.label(format!(
            "NIF block {block}\nPGPatcher 3D index: the number plugins use for this shape's alternate textures"
        ));
    });
    clicked
}

/// Item 9: "Open in Textures" after a texture path that is an item of the open folder.
fn open_button(ui: &mut egui::Ui, m: &MeshesModel, path: &str, open: &mut Option<String>) {
    if let Some(k) = m.folder_items.get(path) {
        if ui
            .small_button("Open in Textures")
            .on_hover_text("Select this texture in the Textures view")
            .clicked()
        {
            *open = Some(k.clone());
        }
    }
}

/// Winning plugin records that use this mesh and the shapes they re-texture (esp spec §4.2). Inert records (filtering
/// spec §3) only behind "Show inert", dimmed, with their reason. Information only.
fn plugin_overrides(ui: &mut egui::Ui, m: &mut MeshesModel) {
    let (uses, inert, show) = (&m.plugin_uses, &m.plugin_inert, &mut m.show_inert_uses);
    let n_inert = uses
        .iter()
        .filter(|u| inert.contains_key(&u.formid))
        .count();
    egui::CollapsingHeader::new(format!("Plugin overrides ({})", uses.len() - n_inert))
        .id_salt("meshes-plugin-overrides")
        .show(ui, |ui| {
            if n_inert > 0 {
                ui.checkbox(&mut *show, format!("Show inert ({n_inert})"));
            }
            let shown: Vec<&MeshUse> = uses
                .iter()
                .filter(|u| *show || !inert.contains_key(&u.formid))
                .collect();
            egui::ScrollArea::vertical()
                .id_salt("meshes-plugin-overrides-scroll")
                .max_height(240.0)
                .show(ui, |ui| {
                    if shown.is_empty() {
                        ui.label("No plugin record of the latest scan uses this mesh.");
                    }
                    for (i, u) in shown.iter().take(MAX_LISTED).enumerate() {
                        let text = format!(
                            "{}{}",
                            record_label(u.edid.as_deref(), u.formid, &u.sig, &u.plugin),
                            overridden_text(u.n_overrides)
                        );
                        match inert.get(&u.formid) {
                            Some(why) => {
                                ui.weak(format!("{text} · {why}"));
                            }
                            None => {
                                ui.label(text);
                            }
                        }
                        ui.indent(("plugin-swaps", i), |ui| {
                            for s in &u.swaps {
                                // formid 0 = the TXST's master is not in the load order
                                let target = if s.txst_formid == 0 {
                                    "unresolved TXST".to_string()
                                } else {
                                    format!(
                                        "{} [{:08X}]",
                                        s.txst_edid.as_deref().unwrap_or("(no EDID)"),
                                        s.txst_formid
                                    )
                                };
                                ui.label(format!("{} → {target}", s.shape_name));
                            }
                        });
                    }
                    if shown.len() > MAX_LISTED {
                        ui.label(format!("… and {} more", shown.len() - MAX_LISTED));
                    }
                });
        });
}

/// A window, not part of the top bar: opening it must not resize the panels.
fn refs_window(ctx: &egui::Context, m: &mut MeshesModel) {
    let mut open = m.refs_open;
    egui::Window::new("Plugin file references")
        .id(egui::Id::new("meshes-plugin-file-refs-window"))
        .open(&mut open)
        .default_size([900.0, 500.0])
        .resizable(true)
        .vscroll(false)
        .show(ctx, |ui| {
            if m.scanning() {
                ui.label("Available after the scan finishes.");
                return;
            }
            if m.file_refs.is_none() {
                m.load_file_refs();
            }
            file_refs(ui, m);
        });
    m.refs_open = open;
}

/// Most rows drawn in the file-reference list.
const MAX_REF_ROWS: usize = 1000;

/// "Plugin file references" (esp spec §4.4): counts, filter, copy; no actions.
fn file_refs(ui: &mut egui::Ui, m: &mut MeshesModel) {
    let (missing, unused) = m
        .file_refs
        .as_ref()
        .map_or((0, 0), |r| (r.missing.len(), r.unused.len()));
    let mut copy = false;
    ui.horizontal(|ui| {
        if ui
            .selectable_label(!m.refs_unused, format!("Missing ({missing})"))
            .clicked()
        {
            m.refs_unused = false;
        }
        if ui
            .selectable_label(m.refs_unused, format!("Unused ({unused})"))
            .clicked()
        {
            m.refs_unused = true;
        }
        ui.add(
            egui::TextEdit::singleline(&mut m.refs_filter)
                .hint_text("filter")
                .desired_width(200.0),
        );
        copy = ui.button("Copy").clicked();
    });
    ui.weak(if m.refs_unused {
        UNUSED_NOTE
    } else {
        MISSING_NOTE
    });
    if m.refs_unused && m.report.as_ref().is_some_and(|r| r.plugins.is_none()) {
        ui.weak(PLUGINS_NOT_READ_NOTE);
    }
    if !m.refs_unused && !m.missing_inert.is_empty() {
        let n = m.missing_inert.len();
        ui.checkbox(&mut m.show_inert_refs, format!("Show inert ({n})"));
    }
    // Rows are chosen after the controls above, so a tab or filter change shows this frame, not the next.
    let inert: std::sync::Arc<Vec<String>> = if !m.refs_unused && m.show_inert_refs {
        m.refs_inert_lines()
    } else {
        Default::default()
    };
    if copy {
        let mut text = m.refs_lines().join("\n");
        if !inert.is_empty() {
            text.push('\n');
            text.push_str(&inert.join("\n"));
        }
        ui.ctx().copy_text(text);
    }
    let n = m.refs_lines().len();
    if n == 0 && inert.is_empty() {
        ui.label("Nothing listed.");
        return;
    }
    ui.set_max_width(ui.available_width());
    // Wrapping rows bounded to the window; rendering is capped so 68k Unused rows stay fast (Copy still takes all).
    egui::ScrollArea::vertical()
        .id_salt("meshes-file-refs-scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // One budget over live then inert rows.
            for l in m.refs_lines().iter().take(MAX_REF_ROWS) {
                ui.add(egui::Label::new(l.as_str()).wrap());
            }
            let left = MAX_REF_ROWS.saturating_sub(n);
            for l in inert.iter().take(left) {
                ui.add(egui::Label::new(egui::RichText::new(l.as_str()).weak()).wrap());
            }
            let more = (n + inert.len()).saturating_sub(MAX_REF_ROWS);
            if more > 0 {
                ui.weak(format!("… and {more} more — narrow with the filter"));
            }
        });
}

/// One shape card, numbered by PGPatcher's 3D index (Meshes UI T2): the header button, then Shader, Textures, Alpha
/// and (in edit mode, `ctrls`) Controllers; the rest in a closed Details section. Returns the texture path that was
/// clicked, and whether the header was clicked.
fn shape(
    ui: &mut egui::Ui,
    m: &MeshesModel,
    s: &ShapeView,
    open: &mut Option<String>,
    on: bool,
    ctrls: Option<&str>,
) -> (Option<String>, bool) {
    let mut clicked = None;
    let number = s
        .index3d
        .map_or_else(|| "#– (not in scene tree)".to_string(), |i| format!("#{i}"));
    let name = s.name.as_deref().unwrap_or("(unnamed)");
    let picked = header(
        ui,
        on,
        &format!("{number} {name}"),
        &s.kind,
        &format!("Select shape {number} {name}"),
        s.block,
    );
    let bold = |t: String| egui::RichText::new(t).strong();
    ui.label(match s.shader_kind {
        1 => bold(
            s.shader_type
                .map_or_else(|| "type ?".into(), shader_type_name),
        ),
        2 => bold("Effect shader".into()),
        3 => bold(format!(
            "Other shader {}",
            s.params.as_deref().unwrap_or("")
        )),
        _ => egui::RichText::new("No shader"),
    });
    let empty = s.textures.iter().filter(|t| t.is_none()).count();
    ui.indent(("slots", s.block), |ui| {
        for (i, t) in s.textures.iter().enumerate() {
            let Some(p) = t else { continue };
            ui.horizontal_wrapped(|ui| {
                let name = slot_name(s.shader_kind, i);
                ui.label(if name.is_empty() {
                    format!("slot {i}:")
                } else {
                    format!("slot {i} ({name}):")
                });
                if ui.link(p).clicked() {
                    clicked = Some(p.clone());
                }
                open_button(ui, m, p, open);
                // A missing texture stays in sight; which mod wins is a detail.
                if let Some(TexResolution::Missing) = m.resolved.get(p) {
                    ui.colored_label(RED, "→ missing");
                }
            });
        }
        match empty {
            0 => {}
            1 => {
                ui.weak("(1 empty slot)");
            }
            n => {
                ui.weak(format!("({n} empty slots)"));
            }
        }
    });
    match s.alpha_flags {
        Some(a) => {
            ui.label(alpha_line(a, s.alpha_threshold.unwrap_or(0)))
                .on_hover_ui(|ui| {
                    ui.label(format!("flags 0x{a:04X} ({a})"));
                });
        }
        None => {
            ui.label("No NiAlphaProperty");
        }
    }
    if let Some(c) = ctrls {
        ui.horizontal(|ui| {
            ui.label("Controllers:");
            ui.label(c);
        });
    }
    let details = egui::CollapsingHeader::new("Details")
        // Final review Minor 1: per edit session (0 outside Edit), so another NIF's card never inherits it.
        .id_salt((
            "shape-details",
            m.edit.as_ref().map_or(0, |e| e.session),
            s.block,
        ))
        .show(ui, |ui| shape_details(ui, m, s));
    // One "Details" per card: each is named after its shape (rule 8.4).
    let label = format!("Details of NIF block {}", s.block);
    details.header_response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::CollapsingHeader, true, &label)
    });
    (clicked, picked)
}

/// The closed part of a shape card: block and 3D index, SLSF1/SLSF2, geometry, which mod wins each texture, raw slot
/// strings.
fn shape_details(ui: &mut egui::Ui, m: &MeshesModel, s: &ShapeView) {
    let index = s
        .index3d
        .map_or_else(|| "not in the scene tree".to_string(), |i| i.to_string());
    ui.label(format!(
        "NIF block {} · PGPatcher 3D index {index}",
        s.block
    ));
    for (label, v, names) in [
        ("SLSF1", s.slsf1, &SLSF1_NAMES),
        ("SLSF2", s.slsf2, &SLSF2_NAMES),
    ] {
        if let Some(v) = v {
            egui::CollapsingHeader::new(format!("{label} 0x{v:08X}"))
                .id_salt(("slsf", s.block, label))
                .show(ui, |ui| {
                    for l in flag_bits(names, v) {
                        ui.label(l);
                    }
                });
        }
    }
    let mut geo = vec![];
    if s.skinned {
        geo.push("skinned".to_string());
    }
    if let Some(v) = s.num_vertices {
        geo.push(format!("{v} vertices"));
    }
    if let Some(t) = s.num_triangles {
        geo.push(format!("{t} triangles"));
    }
    if !geo.is_empty() {
        ui.label(geo.join(" · "));
    }
    for (i, t) in s.textures.iter().enumerate() {
        let origin = match t.as_ref().and_then(|p| m.resolved.get(p)) {
            Some(TexResolution::Found { origin, n_copies }) if *n_copies > 1 => {
                Some(format!("→ {origin} ({n_copies} copies)"))
            }
            Some(TexResolution::Found { origin, .. }) => Some(format!("→ {origin}")),
            _ => None,
        };
        let raw = s.raw.get(i).and_then(|r| r.as_ref()).map(|raw| {
            let f = &raw.flags;
            let names: Vec<&str> = [
                (f.empty, "empty"),
                (f.rebased, "rebased"),
                (f.prefixed, "prefixed"),
                (f.non_dds, "not .dds"),
            ]
            .into_iter()
            .filter_map(|(on, n)| on.then_some(n))
            .collect();
            match names.is_empty() {
                true => format!("raw: {}", raw.raw),
                false => format!("raw: {} [{}]", raw.raw, names.join(", ")),
            }
        });
        if origin.is_none() && raw.is_none() {
            continue;
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("slot {i}"));
            if let Some(o) = origin {
                ui.label(o);
            }
            if let Some(r) = raw {
                ui.weak(r);
            }
        });
    }
}

fn dialogs(ctx: &egui::Context, m: &mut MeshesModel) {
    refs_window(ctx, m);
    if let Some(d) = m.dialog.clone() {
        let (mut use_it, mut choose, mut folder) = (false, false, false);
        egui::Modal::new(egui::Id::new("meshes-mo2")).show(ctx, |ui| {
            ui.set_max_width(480.0);
            ui.label(d.text());
            ui.horizontal(|ui| {
                use_it = ui.button("Use").clicked();
                choose = picker::button(ui, &m.picker, egui::Button::new("Choose instance…"));
                folder = picker::button(ui, &m.picker, egui::Button::new("Open a folder instead"));
            });
        });
        if use_it {
            m.dialog_use();
        } else if choose {
            m.picker
                .request(ctx, PickerPurpose::ChooseMo2, None, PickerSpec::folder());
        } else if folder {
            m.picker
                .request(ctx, PickerPurpose::ScanFolder, None, PickerSpec::folder());
        }
    }
    if let Some(list) = &m.stale_list {
        let mut close = false;
        egui::Modal::new(egui::Id::new("meshes-stale")).show(ctx, |ui| {
            ui.set_max_width(560.0);
            ui.heading("Out-of-date PGPatcher patches");
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    for (path, origin) in list {
                        ui.label(format!("{} ({origin})", display_path(path)));
                    }
                });
            close = ui.button("Close").clicked();
        });
        if close {
            m.stale_list = None;
        }
    }
}
