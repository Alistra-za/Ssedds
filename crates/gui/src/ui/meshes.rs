//! The Meshes view (plan A Task 12): mods, with the NIFs of the open mod under it (GUI tweaks G9) | shapes of the
//! selected NIF. Draws `MeshesModel`; every store read happens in the model on a click, never here.
use crate::model::meshes::{
    display_path, overridden_text, plugins_line, shader_type_name, slot_name, Banner, MeshesModel,
    ModLine, ScanTarget, ALL_MIN_CHARS, DERIVED_CHIP, LE_WARNING, MISSING_NOTE,
    PLUGINS_NOT_READ_NOTE, SEARCH_DEBOUNCE, STALE_CHIP, UNUSED_NOTE,
};
use crate::model::picker::{self, PickerPurpose, PickerSpec};
use eframe::egui;
use modscan::plugin_store::{record_label, MeshUse};
use modscan::store::{NifRow, ShapeView, TexResolution, KIND_BSA, KIND_DATA};

const AMBER: egui::Color32 = egui::Color32::from_rgb(230, 160, 30);
const RED: egui::Color32 = egui::Color32::from_rgb(210, 60, 60);
const GREEN: egui::Color32 = egui::Color32::from_rgb(90, 170, 90);
const BLUE: egui::Color32 = egui::Color32::from_rgb(90, 140, 220);
/// Problems listed under "Scan details"; the count is always shown.
const MAX_LISTED: usize = 200;

/// The whole page in the app: the view toggle, then the view.
pub fn page(ui: &mut egui::Ui, m: &mut MeshesModel) {
    egui::Panel::top("meshes-header").show(ui, |ui| {
        ui.horizontal(super::header::view_toggle);
    });
    show(ui, m);
}

pub fn show(ui: &mut egui::Ui, m: &mut MeshesModel) {
    egui::Panel::top("meshes-bar").show(ui, |ui| bar(ui, m));
    egui::Panel::left("meshes-tree")
        .default_size(620.0)
        .show(ui, |ui| tree(ui, m));
    egui::CentralPanel::default().show(ui, |ui| detail(ui, m));
    dialogs(ui.ctx(), m);
}

fn chip(ui: &mut egui::Ui, color: egui::Color32, text: &str) -> egui::Response {
    ui.label(egui::RichText::new(text).small().color(color))
}

/// The scan source and its buttons: shared by the Meshes view's bar and the Textures view's scan row.
pub fn scan_controls(ui: &mut egui::Ui, m: &mut MeshesModel) {
    ui.horizontal_wrapped(|ui| {
        let source = match &m.target {
            Some(ScanTarget::Mo2 { root, profile }) => {
                format!("MO2 instance {}, profile {profile}", root.display())
            }
            Some(ScanTarget::Folder(f)) => format!("Folder {}", f.display()),
            None => "No source chosen".into(),
        };
        ui.label(source);
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
        if pick(ui, "Choose MO2 instance…") {
            m.picker.request(
                ui.ctx(),
                PickerPurpose::ChooseMo2,
                None,
                PickerSpec::folder(),
            );
        }
        if pick(ui, "Scan a folder…") {
            m.picker.request(
                ui.ctx(),
                PickerPurpose::ScanFolder,
                None,
                PickerSpec::folder(),
            );
        }
        if m.scanning() {
            ui.spinner();
            if let Some((phase, done, total)) = m.progress {
                ui.label(format!("{phase} {done}/{total}"));
            }
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
}

/// The Textures view's scan row: the same source and buttons, the status, and an error. The details, banners and
/// plugin references stay in the Meshes view.
pub fn scan_row(ui: &mut egui::Ui, m: &mut MeshesModel) {
    scan_controls(ui, m);
    // The MO2 question (and any other Meshes dialog) can come from these buttons too.
    dialogs(ui.ctx(), m);
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

fn bar(ui: &mut egui::Ui, m: &mut MeshesModel) {
    scan_controls(ui, m);
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

/// Hover text on a PGPatcher-derived mod, which cannot be opened.
const DERIVED_HOVER: &str = "Generated by PGPatcher. Open the source mods instead: their meshes show as \"Patched by PGPatcher (in game)\".";

/// The arrow of a row that opens and closes: ▼ opens it, ▲ closes it.
fn fold_arrow(open: bool) -> &'static str {
    if open {
        "\u{25b2}"
    } else {
        "\u{25bc}"
    }
}

/// One row of the Mods and meshes column.
#[derive(Clone, Copy)]
enum TreeRow {
    Line(ModLine),
    /// A NIF of the open mod (an index from `visible_nifs`).
    Nif(usize),
}

/// GUI tweaks G9: mods and their meshes in one column. A mod opens (its arrow, or a click on its name) to list its
/// NIFs under it; the chips and the path filter narrow those. With All meshes the column lists the search instead.
fn tree(ui: &mut egui::Ui, m: &mut MeshesModel) {
    ui.heading("Mods and meshes");
    if m.mods.is_empty() {
        ui.label("No scan yet.");
        return;
    }
    ui.add(
        egui::TextEdit::singleline(&mut m.mod_filter)
            .hint_text("Filter mods")
            .desired_width(f32::INFINITY),
    );
    ui.horizontal_wrapped(|ui| {
        for f in crate::model::meshes::NifFilter::ALL {
            if ui.selectable_label(m.filter == f, f.label()).clicked() {
                m.filter = f;
            }
        }
    });
    ui.horizontal(|ui| {
        ui.checkbox(&mut m.all_meshes, "All meshes");
        ui.add(
            egui::TextEdit::singleline(&mut m.path_filter)
                .hint_text("Filter paths")
                .desired_width(f32::INFINITY),
        );
    });
    if m.update_search(std::time::Instant::now()) {
        ui.ctx().request_repaint_after(SEARCH_DEBOUNCE);
    }
    if m.all_meshes {
        all_meshes(ui, m);
        return;
    }
    let visible = m.visible_nifs();
    let mut rows: Vec<TreeRow> = vec![];
    for line in m.mod_lines() {
        rows.push(TreeRow::Line(line));
        if let ModLine::Mod(i) = line {
            if m.selected_mod == Some(m.mods[i].id) {
                rows.extend(visible.iter().map(|&n| TreeRow::Nif(n)));
            }
        }
    }
    let row_h = ui.spacing().interact_size.y;
    let (mut picked, mut toggled, mut nif_picked, mut close) = (None, None, None, false);
    egui::ScrollArea::vertical()
        .id_salt("meshes-tree-scroll")
        .auto_shrink([false, false])
        .show_rows(ui, row_h, rows.len(), |ui, range| {
            for row in &rows[range] {
                match *row {
                    TreeRow::Line(ModLine::Header { idx, count, open }) => {
                        let r = &m.mods[idx];
                        let name = r.name.strip_suffix("_separator").unwrap_or(&r.name);
                        ui.horizontal(|ui| {
                            let text = egui::RichText::new(format!("{} {name}", fold_arrow(open)))
                                .strong();
                            if ui.add(egui::Button::new(text).frame(false)).clicked() {
                                toggled = Some(r.id);
                            }
                            ui.weak(count.to_string());
                        });
                    }
                    TreeRow::Line(ModLine::Mod(i)) => {
                        let r = &m.mods[i];
                        let open = m.selected_mod == Some(r.id);
                        ui.horizontal(|ui| {
                            ui.add_space(12.0);
                            if !r.derived {
                                let arrow = ui
                                    .add(egui::Button::new(fold_arrow(open)).frame(false).small())
                                    .on_hover_text(if open {
                                        "Close this mod"
                                    } else {
                                        "List this mod's meshes"
                                    });
                                if arrow.clicked() {
                                    if open {
                                        close = true;
                                    } else {
                                        picked = Some(r.id);
                                    }
                                }
                            }
                            let label =
                                ui.add_enabled(!r.derived, egui::Button::selectable(open, &r.name));
                            if r.derived {
                                label.on_disabled_hover_text(DERIVED_HOVER);
                            } else if label.clicked() {
                                picked = Some(r.id);
                            }
                            if r.derived {
                                chip(ui, BLUE, DERIVED_CHIP);
                            }
                            match r.kind {
                                KIND_BSA => {
                                    chip(ui, egui::Color32::GRAY, "BSA");
                                }
                                KIND_DATA => {
                                    chip(ui, egui::Color32::GRAY, "game Data");
                                }
                                _ => {}
                            }
                            if open {
                                ui.weak(format!("{} of {} NIFs", visible.len(), m.listed()));
                            }
                        });
                    }
                    TreeRow::Nif(i) => {
                        let (n, origin) = m.nif_at(i);
                        ui.horizontal(|ui| {
                            ui.add_space(36.0);
                            if ui
                                .selectable_label(
                                    m.selected_nif == Some(n.copy_id),
                                    display_path(&n.path),
                                )
                                .clicked()
                            {
                                nif_picked = Some(n.copy_id);
                            }
                            if let Some(o) = origin {
                                ui.weak(o);
                            }
                            badges(ui, n);
                        });
                    }
                }
            }
        });
    if let Some(id) = toggled {
        m.toggle_group(id);
    }
    // While a scan runs the store's winner tables are being rewritten: the selection waits for the result.
    if !m.scanning() {
        if close {
            m.close_mod();
        } else if let Some(id) = picked.filter(|id| m.selected_mod != Some(*id)) {
            m.select_mod(id);
        }
        if let Some(c) = nif_picked {
            m.select_nif(c);
        }
    }
}

/// All meshes: the search's winning NIFs, each with its mod.
fn all_meshes(ui: &mut egui::Ui, m: &mut MeshesModel) {
    if m.all_needs_text() {
        ui.label(format!(
            "Type at least {ALL_MIN_CHARS} characters of a path to search all meshes"
        ));
        return;
    }
    let visible = m.visible_nifs();
    ui.label(format!("{} of {} NIFs", visible.len(), m.listed()));
    if m.all_truncated() {
        ui.label(format!(
            "Showing the first {} matches — type more to narrow it",
            m.all_limit
        ));
    }
    let row_h = ui.spacing().interact_size.y;
    let mut picked = None;
    egui::ScrollArea::vertical()
        .id_salt("meshes-nifs-scroll")
        .auto_shrink([false, false])
        .show_rows(ui, row_h, visible.len(), |ui, range| {
            for &i in &visible[range] {
                let (n, origin) = m.nif_at(i);
                ui.horizontal(|ui| {
                    if ui
                        .selectable_label(m.selected_nif == Some(n.copy_id), display_path(&n.path))
                        .clicked()
                    {
                        picked = Some(n.copy_id);
                    }
                    if let Some(o) = origin {
                        ui.weak(o);
                    }
                    badges(ui, n);
                });
            }
        });
    if let Some(c) = picked.filter(|_| !m.scanning()) {
        m.select_nif(c);
    }
}

fn badges(ui: &mut egui::Ui, n: &NifRow) {
    let color = if n.wins {
        GREEN
    } else if n.patched_in_game {
        BLUE
    } else {
        egui::Color32::GRAY
    };
    let r = chip(ui, color, n.status_label());
    if let Some(by) = &n.overridden_by {
        r.on_hover_text(format!("Overridden by {by} · {} copies", n.n_copies));
    } else if n.n_copies > 1 {
        r.on_hover_text(format!("{} copies", n.n_copies));
    }
    if n.derived {
        chip(ui, BLUE, "PGPatcher");
    }
    if n.derive_check
        .as_deref()
        .is_some_and(|c| c.starts_with("stale"))
    {
        chip(ui, AMBER, STALE_CHIP);
    }
    if n.le_mesh {
        chip(ui, AMBER, "LE");
    }
    if let Some(p) = &n.problem {
        chip(ui, RED, "Problem").on_hover_text(p);
    }
}

fn detail(ui: &mut egui::Ui, m: &mut MeshesModel) {
    let row = m.selected_row().cloned();
    if row.is_none() && m.shapes.is_empty() {
        ui.centered_and_justified(|ui| ui.label("Select a NIF."));
        return;
    }
    if let Some(n) = &row {
        ui.heading(display_path(&n.path));
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
    if m.pair.is_some() {
        ui.horizontal(|ui| {
            if ui.selectable_label(!m.view_source, "In game").clicked() {
                m.set_view_source(false);
            }
            if ui.selectable_label(m.view_source, "Source").clicked() {
                m.set_view_source(true);
            }
        });
    }
    ui.horizontal(|ui| {
        if ui.selectable_label(!m.tab_3d, "Shapes").clicked() {
            m.tab_3d = false;
        }
        if ui.selectable_label(m.tab_3d, "3D").clicked() {
            m.tab_3d = true;
        }
    });
    if m.tab_3d {
        super::viewport3d::show(ui, &mut m.v3);
        return;
    }
    let mut clicked = None;
    egui::ScrollArea::vertical()
        .id_salt("meshes-shapes-scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if let Some((path, users)) = &m.tex_users {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.label(egui::RichText::new(format!("Users of {path}")).strong());
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
            if m.shapes.is_empty() {
                ui.label("No shapes.");
            }
            for s in &m.shapes {
                if let Some(p) = shape(ui, m, s) {
                    clicked = Some(p);
                }
                ui.separator();
            }
        });
    if let Some(p) = clicked {
        m.show_users(&p);
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

/// One shape; returns the texture path that was clicked.
fn shape(ui: &mut egui::Ui, m: &MeshesModel, s: &ShapeView) -> Option<String> {
    let mut clicked = None;
    ui.label(
        egui::RichText::new(format!(
            "#{} {} — {}",
            s.block,
            s.name.as_deref().unwrap_or("(unnamed)"),
            s.kind
        ))
        .strong(),
    );
    let flags = |s: &ShapeView| match (s.slsf1, s.slsf2) {
        (Some(a), Some(b)) => format!(" · SLSF1 0x{a:08X} · SLSF2 0x{b:08X}"),
        _ => String::new(),
    };
    ui.label(match s.shader_kind {
        1 => format!(
            "Lighting: {}{}",
            s.shader_type
                .map_or_else(|| "type ?".into(), shader_type_name),
            flags(s)
        ),
        2 => format!("Effect shader{}", flags(s)),
        3 => format!("Other shader {}", s.params.as_deref().unwrap_or("")),
        _ => "No shader".into(),
    });
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
    if let Some(a) = s.alpha_flags {
        geo.push(format!(
            "alpha 0x{a:04X} / {}",
            s.alpha_threshold.unwrap_or(0)
        ));
    }
    if !geo.is_empty() {
        ui.label(geo.join(" · "));
    }
    for (i, t) in s.textures.iter().enumerate() {
        ui.horizontal_wrapped(|ui| {
            let name = slot_name(s.shader_kind, i);
            ui.label(if name.is_empty() {
                format!("slot {i}:")
            } else {
                format!("slot {i} ({name}):")
            });
            match t {
                None => {
                    ui.weak("(empty)");
                }
                Some(p) => {
                    if ui.link(p).clicked() {
                        clicked = Some(p.clone());
                    }
                    match m.resolved.get(p) {
                        Some(TexResolution::Found { origin, n_copies }) if *n_copies > 1 => {
                            ui.label(format!("→ {origin} ({n_copies} copies)"));
                        }
                        Some(TexResolution::Found { origin, .. }) => {
                            ui.label(format!("→ {origin}"));
                        }
                        Some(TexResolution::Missing) => {
                            ui.colored_label(RED, "→ missing");
                        }
                        None => {}
                    }
                }
            }
            if let Some(Some(raw)) = s.raw.get(i) {
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
                let tail = if names.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", names.join(", "))
                };
                ui.weak(format!("raw: {}{tail}", raw.raw));
            }
        });
    }
    clicked
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
