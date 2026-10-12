//! The set preview panel (P1-A, spec §2.3): the right side of the Textures view while a set, or a texture of a set,
//! is selected. Draws `SetPreview`; the frame, the CS toggle and the problem lists are the shared
//! `ui::viewport3d::show`. No logic beyond mapping input to model calls.
use crate::model::set_preview::{
    badge_text, builtin_name, cap_text, count_text, followed_set, Candidates, EncState, MeshChoice,
    Outcome, Pick, PreviewMode, SetPreview, BUILTINS, JSON_WINS, MODE_LABEL_ENCODED,
    MODE_LABEL_SOURCE, NEEDS_CS, NO_MATCH, OFF_FEATURES, PANEL_TITLE, WAITS_FOR_EXPORT,
};
use crate::model::AppModel;
use eframe::egui;
use render3d::ShapeStatus;

/// Spec D1: the PBR emissive colour is the NIF's, the engine supplies the real one.
const EMISSIVE_APPROX: &str = "Emissive colour is approximate (in game the engine supplies it)";
/// SLSF1 bit 29: the shape has an emissive colour.
const SLSF1_EMISSIVE: u32 = 1 << 29;
const AMBER: egui::Color32 = egui::Color32::from_rgb(230, 160, 30);
const RED: egui::Color32 = egui::Color32::from_rgb(210, 60, 60);
/// The panel's narrowest width, and the width the central columns keep at the least.
pub const PANEL_MIN: f32 = 240.0;
pub const CENTRE_MIN: f32 = 360.0;
/// Height of the shape list before the frame.
const LIST_MAX: f32 = 160.0;

/// The panel is wanted while a set, or a texture of a set, is selected (it stays beside the expanded view).
pub fn wanted(m: &AppModel) -> bool {
    followed_set(m).is_some()
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
    ui.heading(PANEL_TITLE);
    if followed_set(m).is_none() {
        ui.label("Select a set to preview it in 3D.");
        return;
    }
    let Some(fp) = s.fp.clone() else {
        ui.label("Preparing the preview…");
        return;
    };
    ui.label(egui::RichText::new(&fp.label).strong());
    if fp.entry.is_some() {
        ui.label(JSON_WINS);
    } else {
        ui.colored_label(AMBER, NEEDS_CS);
    }
    ui.label(egui::RichText::new(cap_text(s.enc.cap)).small());
    if s.enc.paused {
        ui.colored_label(AMBER, WAITS_FOR_EXPORT);
    }
    mode_toggle(ui, s);
    mesh_picker(ui, s, fp.entry.is_some());
    members(ui, s);
    shapes(ui, s);
    if emissive_shown(s) {
        ui.label(EMISSIVE_APPROX);
    }
    for n in &s.notices {
        ui.colored_label(AMBER, n);
    }
    ui.label(egui::RichText::new(OFF_FEATURES).small());
    timings(ui, s);
    ui.separator();
    crate::ui::viewport3d::show(ui, &mut s.v3);
    if s.busy() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(50));
    }
}

fn mesh_picker(ui: &mut egui::Ui, s: &mut SetPreview, simulated: bool) {
    let pick = s.pick();
    let mut chosen = None;
    ui.horizontal_wrapped(|ui| {
        ui.label("Mesh:");
        let pick_name = match &pick {
            Pick::Nif(c) => c.label(),
            Pick::Builtin(b) => builtin_name(*b).to_string(),
        };
        let current = match s.choice() {
            MeshChoice::Auto => format!("Auto ({pick_name})"),
            _ => pick_name,
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
    });
    if let Some(c) = chosen {
        s.choose(c);
    }
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
        Candidates::Error(e) => {
            ui.colored_label(RED, e);
        }
        _ => {}
    }
}

fn members(ui: &mut egui::Ui, s: &SetPreview) {
    ui.horizontal_wrapped(|ui| {
        for m in s.enc.members.values() {
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
    ui.horizontal_wrapped(|ui| {
        ui.radio_value(&mut mode, PreviewMode::Encoded, MODE_LABEL_ENCODED);
        ui.radio_value(&mut mode, PreviewMode::Source, MODE_LABEL_SOURCE);
    });
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

fn shapes(ui: &mut egui::Ui, s: &SetPreview) {
    // Spec §1 criterion 4: a shader that failed to compile is named here.
    for i in &s.v3.shapes {
        if let ShapeStatus::Failed(e) = &i.status {
            ui.colored_label(RED, format!("{}: shader failed: {e}", i.name));
        }
    }
    if s.rows.is_empty() {
        return;
    }
    egui::ScrollArea::vertical()
        .id_salt("set-3d-shapes")
        .max_height(LIST_MAX)
        .show(ui, |ui| {
            for r in &s.rows {
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
        });
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
        });
}
