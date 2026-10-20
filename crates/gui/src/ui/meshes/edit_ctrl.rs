//! The edit panel's Controllers tab (spec §7, plan A18): the scalars of each time controller on the selected
//! shape's shader and alpha chains. Adding and removing controllers, and key data, are Phase B.
use super::edit_fields::{float_named, row_label, selected, set_name, warnings, Act};
use crate::model::meshes::nif_edit::NifEdit;
use crate::ui::status::{AMBER, RED};
use eframe::egui;
use nif::ctrl::TimeCtrl;
use nif::edit::validate::Target;
use nif::edit::{anchor_shape, Anchor, Op, Owner, Role};
use nif::fields::{Field, Value};
use nif::model::{PropModel, ShapeModel};

const LSCF: &str = "BSLightingShaderPropertyFloatController";
const LSCC: &str = "BSLightingShaderPropertyColorController";
const TEST_REF: &str = "BSNiAlphaPropertyTestRefController";
/// The values the Variable combo offers on a float controller.
const FLOAT_VARS: [u32; 10] = [0, 8, 9, 10, 11, 12, 20, 21, 22, 23];
const COLOR_VARS: [u32; 2] = [0, 1];
const CYCLES: [&str; 3] = ["Loop", "Reverse", "Clamp"];
const MANAGED_HOVER: &str = "Driven by a clip: change it in Phase B.";
const CLIP_SHARED: &str = "Shared alpha with a clip controller: splitting needs Phase C.";

fn var_name(class: &str, v: u32) -> String {
    let known = match (class, v) {
        (LSCF, 0) => "Refraction strength",
        (LSCF, 8) => "Env map scale",
        (LSCF, 9) => "Glossiness",
        (LSCF, 10) => "Specular strength",
        (LSCF, 11) => "Emissive multiple",
        (LSCF, 12) => "Alpha",
        (LSCF, 20) => "U offset",
        (LSCF, 21) => "U scale",
        (LSCF, 22) => "V offset",
        (LSCF, 23) => "V scale",
        (LSCC, 0) => "Specular colour",
        (LSCC, 1) => "Emissive colour",
        _ => return format!("Variable {v}"),
    };
    known.to_string()
}

/// The class without its Bethesda prefix, for the small grey tag beside a row's title.
fn short_class(class: &str) -> &str {
    class
        .strip_prefix("BSLightingShaderProperty")
        .or_else(|| class.strip_prefix("BSEffectShaderProperty"))
        .or_else(|| class.strip_prefix("BSNiAlphaProperty"))
        .unwrap_or(class)
}

fn editable(c: &TimeCtrl) -> bool {
    c.known && matches!(c.class.as_str(), LSCF | LSCC | TEST_REF)
}

pub(super) fn controllers_tab(ui: &mut egui::Ui, e: &NifEdit, act: &mut Act) {
    let Some(shape) = selected(ui, e, "controllers") else {
        return;
    };
    let shader = shape
        .shader
        .and_then(|b| e.model.lighting.get(&b))
        .map(|l| (Owner::Shader, "Shader property", &l.prop));
    let alpha = shape
        .alpha
        .and_then(|b| e.model.alphas.get(&b))
        .map(|a| (Owner::Alpha, "Alpha property", &a.prop));
    let chains: Vec<_> = [shader, alpha]
        .into_iter()
        .flatten()
        .filter(|(_, _, p)| !p.ctrls.is_empty())
        .collect();
    if chains.is_empty() {
        ui.label("This shape has no controllers. Adding them comes in Phase B.");
        return;
    }
    for (owner, title, prop) in chains {
        ui.strong(title);
        chain(ui, e, act, shape, owner, prop);
        ui.separator();
    }
}

fn chain(
    ui: &mut egui::Ui,
    e: &NifEdit,
    act: &mut Act,
    shape: &ShapeModel,
    owner: Owner,
    prop: &PropModel,
) {
    let users = prop.users.len();
    let mut locked = false;
    if users > 1 {
        let managed = prop.ctrls.iter().any(|&c| e.model.managed(c));
        if owner == Owner::Alpha {
            // An alpha edit on a shared alpha is a conflict: the split comes first (as on the Alpha tab).
            locked = true;
            if managed {
                ui.add(egui::Label::new(egui::RichText::new(CLIP_SHARED).color(RED)).wrap());
            } else {
                let text =
                    format!("Shared by {users} shapes: split the alpha to edit its controllers.");
                ui.add(egui::Label::new(egui::RichText::new(text).color(AMBER)).wrap());
                if ui.button("Split alpha").clicked() {
                    let at = anchor_shape(&e.model, shape.block);
                    act.push = Some((vec![Op::SplitAlpha { at }], None));
                }
            }
        } else {
            let text = format!("Shared by {users} shapes: controller edits change all of them.");
            ui.add(egui::Label::new(egui::RichText::new(text).color(AMBER)).wrap());
        }
    }
    ui.add_enabled_ui(!locked, |ui| {
        for (i, &id) in prop.ctrls.iter().enumerate() {
            let Some(c) = e.model.ctrls.get(&id) else {
                continue;
            };
            let nth = prop.ctrls[..i]
                .iter()
                .filter_map(|b| e.model.ctrls.get(b))
                .filter(|p| p.class == c.class && p.var == c.var)
                .count() as u32;
            let at = Anchor {
                block: c.block,
                kind: c.class.clone(),
                shape: shape.name.clone(),
                role: Role::Ctrl {
                    owner,
                    class: c.class.clone(),
                    var: c.var,
                    nth,
                },
            };
            ui.group(|ui| row(ui, e, act, c, &at));
        }
    });
}

fn row(ui: &mut egui::Ui, e: &NifEdit, act: &mut Act, c: &TimeCtrl, at: &Anchor) {
    let managed = e.model.managed(c.block);
    let title = match (c.known, c.var) {
        (false, _) => c.class.clone(),
        (true, Some(v)) => var_name(&c.class, v),
        (true, None) => "Alpha test threshold".to_string(),
    };
    ui.horizontal(|ui| {
        ui.strong(&title);
        ui.weak(short_class(&c.class));
        if managed {
            ui.colored_label(AMBER, "Managed by a clip");
        }
        if !c.known {
            ui.weak("unknown (read-only)");
        } else if !editable(c) {
            ui.weak("read-only");
        }
    });
    if !editable(c) {
        ui.weak(format!(
            "Frequency {} · Phase {} · Start {} · Stop {}",
            c.frequency, c.phase, c.start, c.stop
        ));
        return;
    }
    if let Some(v) = c.var {
        variable(ui, e, act, c, at, &title, v, managed);
    }
    let cycle = (c.flags >> 1) & 3;
    ui.horizontal(|ui| {
        row_label(ui, "Cycle");
        let mut pick = cycle;
        let shown = CYCLES
            .get(cycle as usize)
            .map_or(format!("Cycle {cycle}"), |s| s.to_string());
        let r = egui::ComboBox::from_id_salt(("meshes-edit-ctrl-cycle", c.block))
            .width(120.0)
            .selected_text(&shown)
            .show_ui(ui, |ui| {
                for (i, n) in CYCLES.iter().enumerate() {
                    ui.selectable_value(&mut pick, i as u16, *n);
                }
            })
            .response;
        set_name(
            &r,
            egui::WidgetType::ComboBox,
            &format!("{title} cycle"),
            None,
            Some(shown),
        );
        if pick != cycle {
            act.push = Some((vec![bits(at, c.flags, 0x6, (pick as u32) << 1)], None));
        }
    });
    for (label, what, mask) in [
        ("Active", "active", 0x8u32),
        ("Play backwards", "play backwards", 0x10),
    ] {
        ui.horizontal(|ui| {
            row_label(ui, label);
            let mut on = c.flags as u32 & mask != 0;
            let r = ui.add(egui::Checkbox::without_text(&mut on));
            let (name, enabled, sel) = (format!("{title} {what}"), r.enabled(), on);
            r.widget_info(move || {
                egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, sel, name.clone())
            });
            if r.changed() {
                act.push = Some((
                    vec![bits(at, c.flags, mask, if on { mask } else { 0 })],
                    None,
                ));
            }
        });
    }
    warnings(
        ui,
        e,
        &Target::Field {
            block: c.block,
            field: Field::CtrlFlags,
        },
        act,
    );
    for (label, what, field, v) in [
        ("Frequency", "frequency", Field::Frequency, c.frequency),
        ("Phase", "phase", Field::Phase, c.phase),
        ("Start", "start", Field::StartTime, c.start),
        ("Stop", "stop", Field::StopTime, c.stop),
    ] {
        float_named(ui, e, act, at, label, &format!("{title} {what}"), field, v);
    }
    let interp = c
        .interp
        .and_then(|i| u32::try_from(i).ok())
        .and_then(|i| e.model.interps.get(&i))
        .filter(|i| i.known);
    if let Some(i) = interp {
        let axes = if c.class == LSCC {
            ["R", "G", "B"]
        } else {
            ["X", "Y", "Z"]
        };
        // P9: a value several controllers share is not edited through one of them.
        let users = e
            .model
            .ctrls
            .values()
            .filter(|p| p.interp == c.interp)
            .count();
        let r = ui.add_enabled_ui(users <= 1, |ui| {
            for (k, &v) in i.value.iter().enumerate() {
                let (label, name) = match i.value.len() {
                    1 => ("Value".to_string(), format!("{title} value")),
                    _ => (
                        format!("Value {}", axes[k]),
                        format!("{title} value {}", axes[k]),
                    ),
                };
                float_named(
                    ui,
                    e,
                    act,
                    at,
                    &label,
                    &name,
                    Field::InterpValue(k as u8),
                    v,
                );
            }
        });
        if users > 1 {
            r.response.on_disabled_hover_text(format!(
                "This interpolator is shared by {users} controllers: editing it would change all of them."
            ));
        }
    }
}

fn bits(at: &Anchor, flags: u16, mask: u32, value: u32) -> Op {
    Op::SetBits {
        at: at.clone(),
        field: Field::CtrlFlags,
        mask,
        value,
        base: flags as u32,
    }
}

#[allow(clippy::too_many_arguments)]
fn variable(
    ui: &mut egui::Ui,
    e: &NifEdit,
    act: &mut Act,
    c: &TimeCtrl,
    at: &Anchor,
    title: &str,
    v: u32,
    managed: bool,
) {
    ui.horizontal(|ui| {
        row_label(ui, "Variable");
        let offered: &[u32] = if c.class == LSCC {
            &COLOR_VARS
        } else {
            &FLOAT_VARS
        };
        let mut pick = v;
        let shown = var_name(&c.class, v);
        let r = ui
            .add_enabled_ui(!managed, |ui| {
                egui::ComboBox::from_id_salt(("meshes-edit-ctrl-var", c.block))
                    .width(180.0)
                    .selected_text(&shown)
                    .show_ui(ui, |ui| {
                        // A value outside the list stays selectable, so the current one is never lost.
                        let extra = (!offered.contains(&v)).then_some(v);
                        for &o in extra.iter().chain(offered) {
                            ui.selectable_value(
                                &mut pick,
                                o,
                                format!("{} ({o})", var_name(&c.class, o)),
                            );
                        }
                    })
                    .response
            })
            .inner;
        set_name(
            &r,
            egui::WidgetType::ComboBox,
            &format!("{title} variable"),
            None,
            Some(shown),
        );
        if managed {
            r.on_disabled_hover_text(MANAGED_HOVER);
        }
        if pick != v {
            act.push = Some((
                vec![Op::SetField {
                    at: at.clone(),
                    field: Field::Variable,
                    value: Value::U32(pick),
                    base: Value::U32(v),
                }],
                None,
            ));
        }
    });
    warnings(
        ui,
        e,
        &Target::Field {
            block: c.block,
            field: Field::Variable,
        },
        act,
    );
}
