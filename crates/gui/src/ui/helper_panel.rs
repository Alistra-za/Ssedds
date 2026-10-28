//! Note DT17: a helper's panel (helper textures spec Goal 6): its role, the channels it fills, the channel it is read
//! from and whether it is inverted. Every value shown is the engine's `HelperReport`; the GUI only writes the keys.
use crate::model::helpers::{set_base, written_fills};
use crate::model::AppModel;
use eframe::egui;
use engine::config::RnmTarget;
use engine::helper::report::HelperReport;
use engine::helper::{FillTarget, HelperRead, HelperRole};
use serde_json::json;

/// The fixed width of the panel's combos (egui-ui-rules 3.5: `ComboBox::width` is only a minimum).
const COMBO_W: f32 = 200.0;

const READS: [(Option<HelperRead>, &str); 6] = [
    (None, "Auto (R)"),
    (Some(HelperRead::R), "R"),
    (Some(HelperRead::G), "G"),
    (Some(HelperRead::B), "B"),
    (Some(HelperRead::A), "A"),
    (Some(HelperRead::Luma), "Luminance"),
];

enum Edit {
    Role(Option<HelperRole>),
    Fills(Option<Vec<FillTarget>>),
    Read(Option<HelperRead>),
    Invert(bool),
    LayerTarget(RnmTarget),
    Select(String),
}

/// Names `r` (a combo or a button with a short visible text) for a screen reader and tests.
fn named(r: &egui::Response, kind: egui::WidgetType, name: &str, value: Option<&str>) {
    r.widget_info(|| {
        let mut i = egui::WidgetInfo::labeled(kind, r.enabled(), name);
        i.current_text_value = value.map(str::to_string);
        i
    });
}

/// The read-only reason on a disabled control's hover.
fn why(r: egui::Response, ro: &Option<String>) -> egui::Response {
    match ro {
        Some(w) => r.on_disabled_hover_text(w),
        None => r,
    }
}

/// A combo in an exact-width slot; `f` draws the options. Returns its response.
fn combo(
    ui: &mut egui::Ui,
    id: (&str, &str),
    shown: &str,
    f: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    let h = ui.spacing().interact_size.y;
    ui.allocate_ui(egui::vec2(COMBO_W, h), |ui| {
        egui::ComboBox::from_id_salt(id)
            .width(COMBO_W)
            .truncate()
            .selected_text(shown)
            .show_ui(ui, f)
            .response
    })
    .inner
}

pub fn draw(ui: &mut egui::Ui, model: &mut AppModel, key: &str, h: &HelperReport) {
    let Some(item) = model.item(key) else { return };
    let (name, set_key) = (item.name.clone(), item.set_key.clone());
    let ro = model.toml_read_only_reason();
    let base = set_base(h);
    let mut edits = vec![];
    if let Some(sk) = set_key.filter(|k| model.sets.iter().any(|s| &s.key == k)) {
        let label = super::columns::set_label(&super::columns::export_prefix(model), &sk);
        if ui.link(format!("← {label}")).clicked() {
            model.select_set(&sk);
        }
    }
    ui.heading(&name);
    ui.weak(crate::model::items::helper_target_text(h));
    ui.add_space(4.0);
    // The layer's base when `rnm.target` is unset comes from its name, as the RNM section shows it.
    let layer_target = h.role.is_normal().then(|| {
        model
            .project
            .as_ref()
            .and_then(|p| p.file_value(key, "rnm.target"))
            .and_then(|v| serde_json::from_value::<RnmTarget>(v).ok())
            .unwrap_or_else(|| crate::model::rnm::default_target(&h.path))
    });
    ui.add_enabled_ui(ro.is_none(), |ui| {
        egui::Grid::new(("helper-panel", key))
            .num_columns(3)
            .show(ui, |ui| {
                role_row(ui, key, h, &ro, &mut edits);
                match layer_target {
                    Some(t) => layer_row(ui, key, t, &ro, &mut edits),
                    None => {
                        fills_rows(ui, key, h, &base, &ro, &mut edits);
                        read_rows(ui, key, h, &ro, &mut edits);
                    }
                }
            });
        for (t, reason) in &h.skipped_targets {
            ui.weak(format!("Skipped {}: {reason}", t.label(&base)));
        }
    });
    for o in &h.outputs {
        let target = model
            .items
            .iter()
            .find(|i| i.job.as_ref().is_some_and(|j| &j.output == o))
            .map(|i| (i.key.clone(), i.name.clone()));
        if let Some((k, shown)) = target {
            if ui.button(format!("Show {shown}")).clicked() {
                edits.push(Edit::Select(k));
            }
        }
    }
    for e in edits {
        match e {
            Edit::Role(r) => model.set_helper_role(key, r),
            Edit::Fills(f) => model.set_helper_fills(key, f),
            Edit::Read(r) => model.set_helper_read(key, r),
            Edit::Invert(on) => model.set_helper_invert(key, on),
            Edit::LayerTarget(t) => model.set_field(key, "rnm.target", json!(t)),
            Edit::Select(k) => model.select_item(&k),
        }
    }
}

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).small().weak());
}

fn reset(ui: &mut egui::Ui, name: &str, hover: &str, ro: &Option<String>) -> bool {
    let r = ui.small_button("Reset");
    named(&r, egui::WidgetType::Button, name, None);
    why(r.on_hover_text(hover), ro).clicked()
}

fn role_row(
    ui: &mut egui::Ui,
    key: &str,
    h: &HelperReport,
    ro: &Option<String>,
    edits: &mut Vec<Edit>,
) {
    ui.label("Role");
    ui.horizontal(|ui| {
        let shown = h.role.label();
        let r = combo(ui, (key, "helper-role"), &shown, |ui| {
            // "Not a helper" first (`all()`'s order), so it is in view without scrolling the long list.
            for role in HelperRole::all() {
                if ui.selectable_label(role == h.role, role.label()).clicked() && role != h.role {
                    edits.push(Edit::Role(Some(role)));
                }
                if role == HelperRole::NotHelper {
                    ui.separator();
                }
            }
        });
        named(&r, egui::WidgetType::ComboBox, "Helper role", Some(&shown));
        why(r, ro);
        if h.role_hand_set && reset(ui, "Reset role", "Use the role guessed from the name", ro) {
            edits.push(Edit::Role(None));
        }
    });
    hint(
        ui,
        if h.role_hand_set {
            "set by you"
        } else {
            "guessed from the name"
        },
    );
    ui.end_row();
}

fn layer_row(
    ui: &mut egui::Ui,
    key: &str,
    t: RnmTarget,
    ro: &Option<String>,
    edits: &mut Vec<Edit>,
) {
    ui.label("Blends into");
    let shown = super::rnm_section::target_label(t);
    let r = combo(ui, (key, "helper-layer-target"), shown, |ui| {
        for o in [RnmTarget::N, RnmTarget::Cnr, RnmTarget::Both] {
            if ui
                .selectable_label(o == t, super::rnm_section::target_label(o))
                .clicked()
                && o != t
            {
                edits.push(Edit::LayerTarget(o));
            }
        }
    });
    named(&r, egui::WidgetType::ComboBox, "Layer target", Some(shown));
    why(r, ro);
    hint(ui, "rnm.target");
    ui.end_row();
}

fn fills_rows(
    ui: &mut egui::Ui,
    key: &str,
    h: &HelperReport,
    base: &str,
    ro: &Option<String>,
    edits: &mut Vec<Edit>,
) {
    let now = written_fills(h);
    ui.label("Fills");
    ui.vertical(|ui| {
        for t in &now {
            let label = t.label(base);
            ui.horizontal(|ui| {
                ui.label(&label);
                let hover = format!("Stop filling {label}");
                let r = ui.small_button("✕");
                named(&r, egui::WidgetType::Button, &hover, None);
                if why(r.on_hover_text(&hover), ro).clicked() {
                    // Removing one fill writes the rest as hand-set: the list is the user's from now on.
                    let rest = now.iter().copied().filter(|x| x != t).collect();
                    edits.push(Edit::Fills(Some(rest)));
                }
            });
        }
        let more: Vec<FillTarget> = h
            .options
            .iter()
            .copied()
            .filter(|o| !now.contains(o))
            .collect();
        if !more.is_empty() {
            let r = combo(ui, (key, "helper-add-fill"), "Add target", |ui| {
                for o in &more {
                    if ui.selectable_label(false, o.label(base)).clicked() {
                        let mut next = now.clone();
                        next.push(*o);
                        edits.push(Edit::Fills(Some(next)));
                    }
                }
            });
            named(
                &r,
                egui::WidgetType::ComboBox,
                "Add fill target",
                Some("Add target"),
            );
            why(r, ro);
        }
    });
    if h.fills_hand_set {
        if reset(ui, "Reset fills", "Use the suggested targets", ro) {
            edits.push(Edit::Fills(None));
        }
    } else {
        hint(ui, "suggested");
    }
    ui.end_row();
}

fn read_rows(
    ui: &mut egui::Ui,
    key: &str,
    h: &HelperReport,
    ro: &Option<String>,
    edits: &mut Vec<Edit>,
) {
    ui.label("Read from");
    let shown = READS.iter().find(|r| r.0 == h.read).map_or("", |r| r.1);
    let r = combo(ui, (key, "helper-read"), shown, |ui| {
        for (v, label) in READS {
            if ui.selectable_label(v == h.read, label).clicked() && v != h.read {
                edits.push(Edit::Read(v));
            }
        }
    });
    named(&r, egui::WidgetType::ComboBox, "Read from", Some(shown));
    why(r, ro);
    hint(ui, "the helper's channel used");
    ui.end_row();
    ui.label("");
    let mut on = h.invert;
    let r = ui.checkbox(&mut on, "Invert");
    if why(r, ro).changed() {
        edits.push(Edit::Invert(on));
    }
    hint(ui, "white becomes black");
    ui.end_row();
}
