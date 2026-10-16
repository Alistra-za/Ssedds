//! The set preview (P1-A, spec §2.3): the set's 3D view, in the centre pane under the set editor's header row while
//! the 2D | 3D switch is on 3D (AG78). Draws `SetPreview`; the frame, the CS toggle and the problem lists are the shared
//! `ui::viewport3d::show`. No logic beyond mapping input to model calls.
use crate::model::set_preview::{
    badge_text, builtin_name, cap_text, count_text, followed_set, Candidates, EncState, MeshChoice,
    Outcome, Pick, PreviewMode, SetPreview, BUILTINS, GRASS_APPROX, JSON_WINS, MODE_LABEL_ENCODED,
    MODE_LABEL_SOURCE, NEEDS_CS, NO_MATCH, OFF_FEATURES, TILING, WAITS_FOR_EXPORT,
};
use crate::model::AppModel;
use eframe::egui;
use render3d::ShapeStatus;

/// SLSF1.29 External Emittance: the preview draws the NIF colour (captures C1, frame6171: the engine sends exactly that
/// in an interior); outdoors the engine can substitute the cell or weather colour.
const EMISSIVE_APPROX: &str =
    "External emittance: in game the cell or weather can change this glow colour";
/// SLSF1 bit 29: External Emittance.
const SLSF1_EMISSIVE: u32 = 1 << 29;
const AMBER: egui::Color32 = egui::Color32::from_rgb(230, 160, 30);
const RED: egui::Color32 = egui::Color32::from_rgb(210, 60, 60);
/// The width the centre pane keeps at the least.
pub const CENTRE_MIN: f32 = 360.0;
/// The tallest the shape warnings above the frame get.
const LIST_MAX: f32 = 80.0;

/// The switch is on 3D and a set is followed; the view draws in the centre in place of the set editor's body
/// while the set itself is selected (AG78).
pub fn wanted(m: &AppModel) -> bool {
    m.settings.set_view_3d && followed_set(m).is_some()
}

/// The 2D | 3D switch of a set: the slot cards, or this 3D preview. The TruePBR settings show in both.
pub fn view_switch(ui: &mut egui::Ui, m: &mut AppModel) {
    let mut three_d = m.settings.set_view_3d;
    if ui
        .selectable_label(!three_d, "2D")
        .on_hover_text("Show the set's slot cards")
        .clicked()
    {
        three_d = false;
    }
    if ui
        .selectable_label(three_d, "3D")
        .on_hover_text("Show the set on a mesh in 3D")
        .clicked()
    {
        three_d = true;
    }
    if three_d != m.settings.set_view_3d {
        m.settings.set_view_3d = three_d;
        m.save_settings();
    }
}

pub fn draw(ui: &mut egui::Ui, m: &mut AppModel, s: &mut SetPreview) {
    s.shown = true;
    // Edits arrive as keys, text or clicks; pointer moves and drags (the orbit) keep the throttle.
    if ui.ctx().input(|i| {
        i.events.iter().any(|e| {
            matches!(
                e,
                egui::Event::Key { .. }
                    | egui::Event::Text(_)
                    | egui::Event::Paste(_)
                    | egui::Event::PointerButton { .. }
            )
        })
    }) {
        s.input = true;
    }
    // AG132: the set's name and path are the set editor's header row above this view; no heading of its own.
    if followed_set(m).is_none() {
        ui.label("Select a set to preview it in 3D.");
        return;
    }
    let Some(fp) = s.fp.clone() else {
        ui.label(PREPARING);
        return;
    };
    let simulated = fp.entry.is_some();
    crate::ui::viewport3d::prelude(ui.ctx(), &mut s.v3);
    // One toolbar line, then what needs attention; the frame takes the rest above the collapsed Details.
    egui::Panel::top("set-3d-bar")
        .frame(egui::Frame::new())
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                mode_toggle(ui, s);
                if fp.landscape {
                    // Note BM (review M4): short, so the toolbar stays one line at 1024 pt (BH13).
                    ui.label("Mesh: Plane").on_hover_text(
                        "A PBR texture set or PBR Landscape set previews on the plane",
                    );
                    let mut t = s.tiling;
                    if ui
                        .add(
                            egui::Slider::new(&mut t, 1.0..=3.0)
                                .step_by(0.5)
                                .text(TILING),
                        )
                        .changed()
                    {
                        s.set_tiling(t);
                    }
                } else {
                    mesh_picker(ui, s);
                }
                crate::ui::viewport3d::toolbar(ui, &mut s.v3);
            });
            if !simulated {
                ui.colored_label(AMBER, NEEDS_CS);
            }
            if fp.grass {
                ui.colored_label(AMBER, GRASS_APPROX);
            }
            if s.enc.paused {
                ui.colored_label(AMBER, WAITS_FOR_EXPORT);
            }
            if let Candidates::Error(e) = &s.candidates {
                ui.colored_label(RED, e);
            }
            members(ui, s, true);
            shapes(ui, s, true);
            for n in &s.notices {
                ui.colored_label(AMBER, n);
            }
            crate::ui::viewport3d::warnings(ui, &mut s.v3);
        });
    egui::Panel::bottom("set-3d-details")
        .frame(egui::Frame::new())
        .resizable(false)
        .show(ui, |ui| {
            crate::ui::viewport3d::details_footer(ui, crate::ui::viewport3d::SET_DETAILS, |ui| {
                if simulated {
                    ui.label(JSON_WINS);
                }
                ui.label(egui::RichText::new(cap_text(s.enc.cap)).small());
                mesh_line(ui, s, simulated);
                members(ui, s, false);
                shapes(ui, s, false);
                if emissive_shown(s) {
                    ui.label(EMISSIVE_APPROX);
                }
                timings(ui, s);
                // Until the CS settings are read the 3D part names only Dynamic Cubemaps: keep the P0 list here.
                if s.v3.off_in_preview.is_empty() {
                    ui.label(egui::RichText::new(OFF_FEATURES).small());
                }
                crate::ui::viewport3d::details(ui, &mut s.v3);
            });
        });
    if !crate::ui::viewport3d::le_mesh(&s.v3) {
        crate::ui::viewport3d::image(ui, &mut s.v3);
    }
    if s.busy() || s.v3.busy() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(16));
    }
}

/// The mesh combo, on the toolbar line: "Mesh: Auto (Sphere)" and its list.
fn mesh_picker(ui: &mut egui::Ui, s: &mut SetPreview) {
    let pick = s.pick();
    let mut chosen = None;
    let pick_name = match &pick {
        Pick::Nif(c) => c.label(),
        Pick::Builtin(b) => builtin_name(*b).to_string(),
    };
    let current = match s.choice() {
        MeshChoice::Auto => format!("Mesh: Auto ({pick_name})"),
        _ => format!("Mesh: {pick_name}"),
    };
    egui::ComboBox::from_id_salt("set-3d-mesh")
        .selected_text(current)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(s.choice() == MeshChoice::Auto, "Auto")
                .clicked()
            {
                chosen = Some(MeshChoice::Auto);
            }
            if let Candidates::Ready(list) = &s.candidates {
                for c in list {
                    let sel = matches!(&pick, Pick::Nif(p) if p.load_copy_id == c.load_copy_id);
                    let r = ui
                        .add_enabled_ui(!c.le_mesh, |ui| ui.selectable_label(sel, c.label()))
                        .inner;
                    if r.clicked() {
                        chosen = Some(MeshChoice::Nif(c.load_copy_id));
                    }
                }
            }
            ui.separator();
            for (b, name) in BUILTINS {
                let sel = matches!(pick, Pick::Builtin(x) if x == b);
                if ui.selectable_label(sel, name).clicked() {
                    chosen = Some(MeshChoice::Builtin(b));
                }
            }
        });
    if let Some(c) = chosen {
        s.choose(c);
    }
}

/// Where the mesh list comes from, in Details (an error is a warning above the frame instead).
fn mesh_line(ui: &mut egui::Ui, s: &SetPreview, simulated: bool) {
    match &s.candidates {
        Candidates::NoStore(t) => {
            ui.label(t);
        }
        Candidates::Querying => {
            ui.label("Looking for meshes that use this diffuse…");
        }
        Candidates::Ready(v) if v.is_empty() && simulated => {
            ui.label(NO_MATCH);
        }
        Candidates::Ready(v) if !v.is_empty() => {
            ui.label(count_text(v.len()));
        }
        _ => {}
    }
}

/// The maps' encode states: with `attention`, the failed, stale and blocked ones (above the frame); else the others
/// (Details).
fn members(ui: &mut egui::Ui, s: &SetPreview, attention: bool) {
    let shown: Vec<_> = s
        .enc
        .members
        .values()
        .filter(|m| {
            attention
                == matches!(
                    m.state,
                    EncState::Failed(_) | EncState::Stale(_) | EncState::Blocked(_)
                )
        })
        .collect();
    if shown.is_empty() {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        for m in shown {
            let mut text = format!("{}: {}", m.slot.kind.label(), m.state.label());
            // The time of the bytes on show, so the two modes can be compared (Fresh already says it).
            if let (false, Some(e)) = (matches!(m.state, EncState::Fresh(_)), m.state.shown()) {
                text += &format!(" · last {:.0} ms", e.ms);
            }
            match &m.state {
                EncState::Failed(_) => ui.colored_label(RED, text),
                EncState::Stale(_) | EncState::Blocked(_) => ui.colored_label(AMBER, text),
                _ => ui.label(text),
            };
        }
    });
}

fn mode_toggle(ui: &mut egui::Ui, s: &mut SetPreview) {
    let mut mode = s.mode();
    ui.radio_value(&mut mode, PreviewMode::Encoded, MODE_LABEL_ENCODED);
    ui.radio_value(&mut mode, PreviewMode::Source, MODE_LABEL_SOURCE);
    if mode != s.mode() {
        s.set_mode(mode);
    }
}

/// Any shape on show has an emissive colour (SLSF1 bit 29): the panel's rows, or the viewport's loaded shapes.
fn emissive_shown(s: &SetPreview) -> bool {
    let on = |m: &nif::material::MaterialState| !m.hidden && m.slsf1 & SLSF1_EMISSIVE != 0;
    s.rows.iter().any(|r| on(&r.material))
        || s.v3
            .shapes
            .iter()
            .any(|i| i.status != ShapeStatus::Hidden && on(&i.material))
}

/// The shapes: with `attention`, the failed shaders, the skipped shapes and the patch badges (above the frame); else
/// the rest, compiling and patched lines (Details).
fn shapes(ui: &mut egui::Ui, s: &SetPreview, attention: bool) {
    // Spec §1 criterion 4: a shader that failed to compile is named, and one still compiling says so.
    for i in &s.v3.shapes {
        match &i.status {
            ShapeStatus::Failed {
                error,
                showing_previous,
            } if attention => {
                let kept = if *showing_previous {
                    " (previous look kept)"
                } else {
                    ""
                };
                ui.colored_label(RED, format!("{}: shader failed: {error}{kept}", i.name));
            }
            ShapeStatus::Pending { .. } if !attention => {
                ui.label(format!("{}: compiling…", i.name));
            }
            _ => {}
        }
    }
    let needs = |o: &Outcome| match o {
        Outcome::Skip(_) => true,
        Outcome::Patched { badges } => !badges.is_empty(),
        Outcome::NoMatch | Outcome::Plain => false,
    };
    let rows: Vec<_> = s
        .rows
        .iter()
        .filter(|r| needs(&r.outcome) == attention)
        .collect();
    if rows.is_empty() {
        return;
    }
    let list = |ui: &mut egui::Ui| {
        for r in rows {
            match &r.outcome {
                Outcome::Skip(_) => {
                    ui.colored_label(RED, r.text());
                }
                Outcome::Patched { badges } => {
                    ui.label(r.text());
                    for b in badges {
                        ui.colored_label(AMBER, badge_text(b));
                    }
                }
                Outcome::NoMatch | Outcome::Plain => {
                    ui.label(r.text());
                }
            }
        }
    };
    // Above the frame the list is capped; Details scrolls as a whole.
    if attention {
        egui::ScrollArea::vertical()
            .id_salt("set-3d-shapes")
            .max_height(LIST_MAX)
            .show(ui, list);
    } else {
        list(ui);
    }
}

fn timings(ui: &mut egui::Ui, s: &SetPreview) {
    egui::CollapsingHeader::new("Timings")
        .id_salt("set-3d-timings")
        .show(ui, |ui| {
            let f = |x: Option<f64>| x.map_or("–".to_string(), |v| format!("{v:.1} ms"));
            ui.label(format!("Mesh query: {}", f(s.timings.query_ms)));
            ui.label(format!("NIF read + parse: {}", f(s.timings.nif_ms)));
            ui.label(format!("Simulate + scene: {}", f(s.timings.push_ms)));
            for m in s.enc.members.values() {
                if let Some(e) = m.state.shown() {
                    ui.label(format!("Encode {}: {:.0} ms", m.slot.kind.label(), e.ms));
                }
            }
            if let Some((k, t)) = s.v3.last_edit {
                ui.label(format!("Last material edit: {k:?}, {t:.2} ms"));
            }
            if let Some(p) = render3d::perf_log::path() {
                ui.horizontal(|ui| {
                    ui.label(format!("Timing log: {}", p.display()));
                    if ui.button("Copy log path").clicked() {
                        ui.ctx().copy_text(p.display().to_string());
                    }
                });
            } else {
                ui.label("Timing log: off (SDC_PREVIEW_LOG=0)");
            }
        });
}

/// What the 3D view says until its first fingerprint is ready.
pub const PREPARING: &str = "Preparing the preview…";
/// The header-row button that shows or hides the 3D view's settings window (AG132).
pub const SETTINGS_BUTTON: &str = "TruePBR settings…";
/// The settings window's id: one window whatever it shows, so its place and size carry over.
pub fn settings_window_id() -> egui::Id {
    egui::Id::new("set-3d-settings-window")
}

/// The settings toggle, beside the input name box (or the texture's name) in the 3D header row.
pub fn settings_toggle(ui: &mut egui::Ui, m: &mut AppModel) {
    let mut on = m.settings.set_settings_open;
    if ui
        .toggle_value(&mut on, SETTINGS_BUTTON)
        .on_hover_text(
            "A window with the set's TruePBR settings (and a selected texture's own settings); edits show in 3D",
        )
        .changed()
    {
        m.settings.set_settings_open = on;
        m.save_settings();
    }
}

/// AG132: the 3D view's settings in a movable, resizable window: a selected member's own settings, then the set's
/// TruePBR settings (coat group, NiAlphaProperty and the rest) and, for an Advanced skin set, the skin card. It follows
/// the selection; a selection with none of them closes it. Nothing shows while the switch is on 2D.
pub fn settings_window(ctx: &egui::Context, m: &mut AppModel, three_d: bool) {
    if !three_d || !m.settings.set_settings_open {
        return;
    }
    let item = match &m.selection {
        crate::model::Selection::Item(k) => Some(k.clone()),
        _ => None,
    };
    let set = followed_set(m).and_then(|k| {
        crate::model::sets::set_views(m)
            .into_iter()
            .find(|v| v.key == k)
    });
    let json = set
        .as_ref()
        .is_some_and(|v| crate::model::sets::shows_json_panel(v, m.target));
    let skin = set.as_ref().is_some_and(|v| v.skin);
    // The CS PBR section (CS target) and the complex material card show here too, as they do in 2D.
    let cs = set.is_some() && m.target == engine::config::Target::Cs;
    let cm = set
        .as_ref()
        .is_some_and(|v| crate::model::cm_json::shows_card(m, v));
    if item.is_none() && !json && !skin && !cs && !cm {
        m.settings.set_settings_open = false;
        m.save_settings();
        return;
    }
    let title = match &item {
        Some(k) => format!(
            "{} settings",
            m.item(k).map_or(k.as_str(), |i| i.name.as_str())
        ),
        None => "TruePBR settings".to_string(),
    };
    let rect = m.settings.layout.settings_window.map_or(
        egui::Rect::from_min_size(egui::pos2(120.0, 120.0), egui::vec2(460.0, 620.0)),
        |[x, y, w, h]| egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h)),
    );
    let mut open = true;
    egui::Window::new(title)
        .id(settings_window_id())
        .open(&mut open)
        .resizable(true)
        .default_rect(rect)
        .constrain(true)
        .scroll([false, true])
        .show(ctx, |ui| {
            if let Some(k) = &item {
                crate::ui::settings_panel::draw(ui, m, k, false);
            }
            if let Some(v) = &set {
                if item.is_some() && (json || skin || cs || cm) {
                    ui.separator();
                    ui.strong(format!("Set {}", v.label));
                }
                if json {
                    crate::ui::cs_pbr::pgp_panel(ui, m, v);
                }
                if skin {
                    crate::ui::skin_card::card(ui, m, v);
                }
                crate::ui::cs_pbr::card(ui, m, v);
                crate::ui::cs_pbr::section(ui, m, v);
                crate::ui::cm_card::section(ui, m, v);
            }
        });
    if !open {
        m.settings.set_settings_open = false;
        m.save_settings();
    }
}
