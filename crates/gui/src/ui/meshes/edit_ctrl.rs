//! The edit panel's Controllers tab (spec §7, plan A18): the scalars of each time controller on the selected
//! shape's shader and alpha chains, and (v2 B6) each controller's keys: a curve, a key table, Add and Remove key,
//! Interpolation, Remove controller, and an Add controller row per property.
use super::curve::{self, Part};
use super::edit_fields::{float_named, row_label, selected, set_name, warnings, Act};
use crate::model::meshes::nif_edit::NifEdit;
use crate::model::meshes::MeshesModel;
use crate::model::viewport3d::Clock;
use crate::ui::status::{AMBER, RED};
use eframe::egui;
use nif::ctrl::TimeCtrl;
use nif::edit::validate::Target;
use nif::edit::{anchor_role, anchor_shape, merge_key, Anchor, Op, Owner, Role};
use nif::fields::{Field, Value};
use nif::keys::{Key, KeyData, KeyGroup};
use nif::model::{PropModel, ShapeModel};
use nif::props::LightingTrailer;

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

pub(super) fn controllers_tab(ui: &mut egui::Ui, m: &MeshesModel, e: &NifEdit, act: &mut Act) {
    let Some(shape) = selected(ui, e, "controllers") else {
        return;
    };
    // B7: the "Preview" selector (Free chains | clip names) goes here.
    let clock = m.v3.clock;
    let shader = shape
        .shader
        .and_then(|b| e.model.lighting.get(&b))
        .map(|l| (Owner::Shader, "Shader property", &l.prop));
    let alpha = shape
        .alpha
        .and_then(|b| e.model.alphas.get(&b))
        .map(|a| (Owner::Alpha, "Alpha property", &a.prop));
    let chains: Vec<_> = [shader, alpha].into_iter().flatten().collect();
    if chains.is_empty() {
        ui.label("This shape has no shader or alpha property.");
        return;
    }
    for (owner, title, prop) in chains {
        ui.strong(title);
        chain(ui, e, act, shape, owner, prop, clock);
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
    clock: Option<Clock>,
) {
    let users = prop.users.len();
    let mut locked = false;
    let managed = users > 1 && prop.ctrls.iter().any(|&c| e.model.managed(c));
    // D9: only a shared alpha must split before an add, and a clip controller on it blocks the split. A shared
    // shader adds in place (its banner says the edit changes every shape).
    let clip_shared = owner == Owner::Alpha && managed;
    if users > 1 {
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
    if prop.ctrls.is_empty() {
        ui.weak("No controllers.");
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
            ui.group(|ui| row(ui, e, act, c, &at, clock));
        }
    });
    // Outside the lock: a shared, unmanaged alpha adds through Split + Add.
    add_row(ui, e, act, shape, owner, prop, clip_shared);
}

fn row(
    ui: &mut egui::Ui,
    e: &NifEdit,
    act: &mut Act,
    c: &TimeCtrl,
    at: &Anchor,
    clock: Option<Clock>,
) {
    let managed = e.model.managed(c.block);
    let title = match (c.known, c.var) {
        (false, _) => c.class.clone(),
        (true, Some(v)) => var_name(&c.class, v),
        (true, None) => "Alpha test threshold".to_string(),
    };
    egui::Sides::new().shrink_left().truncate().show(
        ui,
        |ui| {
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
        },
        |ui| {
            if !editable(c) {
                return;
            }
            let r = ui.add_enabled(!managed, egui::Button::new("Remove").small());
            set_name(
                &r,
                egui::WidgetType::Button,
                &format!("Remove {title} controller"),
                None,
                None,
            );
            if r.clicked() {
                act.push = Some((vec![Op::RemoveController { at: at.clone() }], None));
            }
            r.on_disabled_hover_text("Driven by a clip: it cannot be removed.");
        },
    );
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
    keys_section(ui, e, act, c, at, &title, managed, clock);
    // D13: the validate_ops errors of this controller's SetKeys.
    warnings(ui, e, &Target::Block { block: c.block }, act);
}

/// The keys of controller `c` (v2 B6): Interpolation, the curve, the key table and Add key. `managed`: its keys are
/// in the clips (B7); nothing here.
#[allow(clippy::too_many_arguments)]
fn keys_section(
    ui: &mut egui::Ui,
    e: &NifEdit,
    act: &mut Act,
    c: &TimeCtrl,
    at: &Anchor,
    title: &str,
    managed: bool,
    clock: Option<Clock>,
) {
    if managed {
        ui.weak("Its keys are in the clips that drive it.");
        return;
    }
    let Some(keys) = e.keys.get(&(c.block, None)) else {
        return;
    };
    let keys = match keys {
        Ok(k) => k,
        Err(why) => {
            ui.weak(why);
            return;
        }
    };
    let kt = curve::key_type(keys);
    // D11: TBC, XYZ, CONST and any other stored type are previewed, never authored.
    let editable = curve::channels(keys) > 0 && matches!(kt, None | Some(1) | Some(2));
    if !editable {
        ui.weak("These keys are previewed, not edited.");
    }
    let n = curve::len(keys);
    if editable && n > 0 {
        ui.horizontal(|ui| {
            row_label(ui, "Interpolation");
            let quad = kt == Some(2);
            let mut pick = quad;
            let shown = if quad { "Quadratic" } else { "Linear" };
            let r = egui::ComboBox::from_id_salt(("meshes-edit-ctrl-interp", c.block))
                .width(120.0)
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut pick, false, "Linear");
                    ui.selectable_value(&mut pick, true, "Quadratic");
                })
                .response;
            set_name(
                &r,
                egui::WidgetType::ComboBox,
                &format!("{title} interpolation"),
                None,
                Some(shown.into()),
            );
            if pick != quad {
                push_keys(act, at, curve::convert(keys, pick), keys, false);
            }
        });
    }
    let cursor =
        clock.map(|k| nif::anim::ctrl_time(k.t, c.frequency, c.phase, c.start, c.stop, c.flags));
    let id = egui::Id::new(("meshes-edit-curve", c.block));
    let out = curve::curve(ui, id, title, keys, (c.start, c.stop), cursor, editable);
    if let Some(k) = out.keys {
        push_keys(act, at, k, keys, true);
    }
    act.flush |= out.released;
    if let Some(kt) = out.scrub {
        // shortcut: the first-cycle inverse of ctrl_time; a scrub on a looping or reversed controller past its first
        // cycle lands in the first one. Upgrade if users scrub multi-cycle curves.
        act.scrub = Some(if c.frequency != 0.0 && c.frequency.is_finite() {
            (kt - c.phase) / c.frequency
        } else {
            kt
        });
    }
    key_table(ui, act, c, at, title, keys, editable);
    add_key(ui, e, act, c, at, title, keys, editable, cursor);
}

/// One exact float cell: f32's shortest round-trip text, so what is shown is the stored float. Returns the new value
/// when its bits changed, and whether the edit ended (the next change is a new undo step).
fn cell(ui: &mut egui::Ui, v: f32, name: &str) -> (Option<f32>, bool) {
    let mut x = v;
    let speed = (x.abs() as f64 * 0.01).max(0.001);
    let r = ui.add(
        egui::DragValue::new(&mut x)
            .speed(speed)
            .custom_formatter(|v, _| (v as f32).to_string()),
    );
    set_name(&r, egui::WidgetType::DragValue, name, Some(x as f64), None);
    let changed = (r.changed() && x.to_bits() != v.to_bits()).then_some(x);
    (changed, r.drag_stopped() || r.lost_focus())
}

/// The key table: one row per key with its time, value(s) and, for QUADRATIC, tangents, and a Remove button.
fn key_table(
    ui: &mut egui::Ui,
    act: &mut Act,
    c: &TimeCtrl,
    at: &Anchor,
    title: &str,
    keys: &KeyData,
    editable: bool,
) {
    let n = curve::len(keys);
    let nch = curve::channels(keys);
    if n == 0 || nch == 0 {
        return;
    }
    let quad = curve::key_type(keys) == Some(2);
    let rgb = |what: &str, ch: usize| match nch {
        1 => what.to_string(),
        _ => format!("{what} {}", ["R", "G", "B"][ch]),
    };
    let mut parts = vec![Part::Value];
    if quad {
        parts.extend([Part::Forward, Part::Backward]);
    }
    let word = |p: Part| match p {
        Part::Value => "value",
        Part::Forward => "forward",
        Part::Backward => "backward",
    };
    let mut next: Option<(KeyData, bool)> = None;
    ui.add_enabled_ui(editable, |ui| {
        // Grid cells keep last frame's width: truncation would never let "Remove" grow. The panel scrolls sideways.
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
        // shortcut: every key is a row; corpus curves have <= 8 keys. Use show_rows past a few hundred.
        egui::Grid::new(("meshes-edit-keys", c.block))
            .striped(true)
            .show(ui, |ui| {
                ui.label("Key");
                ui.label("Time");
                for &p in &parts {
                    for ch in 0..nch {
                        let head = match (p, nch) {
                            (Part::Value, 1) => "Value".to_string(),
                            (Part::Value, _) => ["R", "G", "B"][ch].to_string(),
                            (Part::Forward, _) => rgb("Forward", ch),
                            (Part::Backward, _) => rgb("Backward", ch),
                        };
                        ui.label(head);
                    }
                }
                ui.label("");
                ui.end_row();
                for k in 0..n {
                    let num = k + 1;
                    ui.label(num.to_string());
                    let (t, done) =
                        cell(ui, curve::time(keys, k), &format!("{title} key {num} time"));
                    act.flush |= done;
                    if let Some(t) = t {
                        let mut d = keys.clone();
                        curve::set_time(&mut d, k, t);
                        next = Some((d, true));
                    }
                    for &p in &parts {
                        for ch in 0..nch {
                            let name = format!("{title} key {num} {}", rgb(word(p), ch));
                            let (v, done) = cell(ui, curve::get(keys, k, ch, p), &name);
                            act.flush |= done;
                            if let Some(v) = v {
                                let mut d = keys.clone();
                                curve::set(&mut d, k, ch, p, v);
                                next = Some((d, true));
                            }
                        }
                    }
                    let r = ui.small_button("Remove");
                    set_name(
                        &r,
                        egui::WidgetType::Button,
                        &format!("Remove {title} key {num}"),
                        None,
                        None,
                    );
                    if r.clicked() {
                        next = Some((without_key(keys, k), false));
                    }
                    ui.end_row();
                }
            });
    });
    if let Some((d, merge)) = next {
        push_keys(act, at, d, keys, merge);
    }
}

/// `keys` without key `k`; the last key going leaves the empty group (`key_type: None`).
fn without_key(keys: &KeyData, k: usize) -> KeyData {
    fn drop_one<T: Clone>(g: &KeyGroup<T>, k: usize) -> KeyGroup<T> {
        let mut g = g.clone();
        g.keys.remove(k);
        if g.keys.is_empty() {
            g.key_type = None;
        }
        g
    }
    match keys {
        KeyData::Float(g) => KeyData::Float(drop_one(g, k)),
        KeyData::Pos(g) => KeyData::Pos(drop_one(g, k)),
        KeyData::Bool(g) => KeyData::Bool(drop_one(g, k)),
    }
}

/// Add key (D10): at the cursor time, else half-way between the last two keys (start / stop for 0 / 1 keys), with
/// the curve's value there; QUADRATIC tangents 0. An empty group starts LINEAR at the interpolator's value.
#[allow(clippy::too_many_arguments)]
fn add_key(
    ui: &mut egui::Ui,
    e: &NifEdit,
    act: &mut Act,
    c: &TimeCtrl,
    at: &Anchor,
    title: &str,
    keys: &KeyData,
    editable: bool,
    cursor: Option<f32>,
) {
    let n = curve::len(keys);
    let t = match (cursor, n) {
        (Some(t), _) => t,
        (None, 0) => c.start,
        (None, 1) => c.stop,
        (None, _) => (curve::time(keys, n - 2) + curve::time(keys, n - 1)) / 2.0,
    };
    let taken = (0..n).any(|k| curve::time(keys, k) == t);
    let r = ui.add_enabled(editable && !taken, egui::Button::new("Add key"));
    set_name(
        &r,
        egui::WidgetType::Button,
        &format!("Add key to {title}"),
        None,
        None,
    );
    let clicked = r.clicked();
    if taken {
        r.on_disabled_hover_text("A key is already at this time.");
    }
    if !clicked {
        return;
    }
    let interp = c
        .interp
        .and_then(|i| u32::try_from(i).ok())
        .and_then(|i| e.model.interps.get(&i))
        .map(|i| i.value.clone())
        .unwrap_or_default();
    let v = curve::sample(keys, t).unwrap_or_else(|| {
        let at = |k: usize| interp.get(k).copied().unwrap_or(0.0);
        [at(0), at(1), at(2)]
    });
    fn insert<T: Clone>(g: &KeyGroup<T>, t: f32, value: T, zero: T) -> KeyGroup<T> {
        let quad = g.key_type == Some(2);
        let mut g = g.clone();
        g.key_type.get_or_insert(1);
        let i = g
            .keys
            .iter()
            .position(|k| k.time > t)
            .unwrap_or(g.keys.len());
        let tangent = quad.then_some(zero);
        let key = Key {
            time: t,
            value,
            forward: tangent.clone(),
            backward: tangent,
            tbc: None,
        };
        g.keys.insert(i, key);
        g
    }
    let next = match keys {
        KeyData::Float(g) => KeyData::Float(insert(g, t, v[0], 0.0)),
        KeyData::Pos(g) => KeyData::Pos(insert(g, t, v, [0.0; 3])),
        KeyData::Bool(_) => return,
    };
    push_keys(act, at, next, keys, false);
}

fn push_keys(act: &mut Act, at: &Anchor, keys: KeyData, base: &KeyData, merge: bool) {
    let op = Op::SetKeys {
        at: at.clone(),
        clip: None,
        keys,
        base: base.clone(),
    };
    let key = merge.then(|| merge_key(&op));
    act.push = Some((vec![op], key));
}

/// Adds a controller to `prop` (D9). Shader: a class (Float | Colour) and variable picker; alpha: TestRef only.
/// `clip_shared`: a shared alpha with a clip controller (the red line above says why nothing is added).
#[allow(clippy::too_many_arguments)]
fn add_row(
    ui: &mut egui::Ui,
    e: &NifEdit,
    act: &mut Act,
    shape: &ShapeModel,
    owner: Owner,
    prop: &PropModel,
    clip_shared: bool,
) {
    let id = egui::Id::new(("meshes-edit-ctrl-add", prop.block));
    let (mut class_idx, mut var): (usize, u32) = ui.data(|d| d.get_temp(id)).unwrap_or((0, 20));
    if owner == Owner::Shader {
        ui.horizontal(|ui| {
            row_label(ui, "Add controller");
            let before = class_idx;
            let classes = ["Float", "Colour"];
            let r = egui::ComboBox::from_id_salt(id.with("class"))
                .width(80.0)
                .selected_text(classes[class_idx])
                .show_ui(ui, |ui| {
                    for (i, n) in classes.iter().enumerate() {
                        ui.selectable_value(&mut class_idx, i, *n);
                    }
                })
                .response;
            set_name(
                &r,
                egui::WidgetType::ComboBox,
                "Add controller class",
                None,
                Some(classes[class_idx].into()),
            );
            if class_idx != before {
                var = if class_idx == 0 { 20 } else { 1 };
            }
        });
        ui.horizontal(|ui| {
            row_label(ui, "Variable");
            let (class, offered): (&str, &[u32]) = match class_idx {
                0 => (LSCF, &FLOAT_VARS),
                _ => (LSCC, &COLOR_VARS),
            };
            let shown = format!("{} ({var})", var_name(class, var));
            let r = egui::ComboBox::from_id_salt(id.with("var"))
                .width(180.0)
                .selected_text(&shown)
                .show_ui(ui, |ui| {
                    for &o in offered {
                        ui.selectable_value(&mut var, o, format!("{} ({o})", var_name(class, o)));
                    }
                })
                .response;
            set_name(
                &r,
                egui::WidgetType::ComboBox,
                "Add controller variable",
                None,
                Some(shown),
            );
        });
    }
    ui.data_mut(|d| d.insert_temp(id, (class_idx, var)));
    let (role, what) = match owner {
        Owner::Shader => (Role::Shader, "shader"),
        Owner::Alpha => (Role::Alpha, "alpha"),
    };
    let at = anchor_role(&e.model, Some(shape.block), role);
    let no_root = e.model.root_net.is_none();
    let r = ui.add_enabled(
        at.is_some() && !clip_shared && !no_root,
        egui::Button::new("Add controller"),
    );
    set_name(
        &r,
        egui::WidgetType::Button,
        &format!("Add controller to {what} property"),
        None,
        None,
    );
    let clicked = r.clicked();
    if no_root {
        r.on_disabled_hover_text("The root is not a node: the BSX Animated bit cannot be set.");
    } else if at.is_none() {
        r.on_disabled_hover_text("This property cannot be found again in the file.");
    } else if clip_shared {
        r.on_disabled_hover_text(CLIP_SHARED);
    }
    if let (true, Some(at)) = (clicked, at) {
        let (class, var, value) = match owner {
            Owner::Shader if class_idx == 0 => (LSCF, Some(var), float_value(e, prop.block, var)),
            Owner::Shader => (LSCC, Some(var), color_value(e, prop.block, var)),
            Owner::Alpha => {
                let t = e
                    .model
                    .alphas
                    .get(&prop.block)
                    .map_or(0.0, |a| a.alpha.threshold as f32);
                (TEST_REF, None, [t, 0.0, 0.0])
            }
        };
        let key = |time: f32| Key {
            time,
            value: value[0],
            forward: None,
            backward: None,
            tbc: None,
        };
        let keys = match class {
            LSCC => KeyData::Pos(KeyGroup {
                key_type: Some(1),
                keys: [0.0, 1.0]
                    .map(|time| Key {
                        time,
                        value,
                        forward: None,
                        backward: None,
                        tbc: None,
                    })
                    .into(),
            }),
            _ => KeyData::Float(KeyGroup {
                key_type: Some(1),
                keys: vec![key(0.0), key(1.0)],
            }),
        };
        let add = Op::AddController {
            at,
            class: class.into(),
            var,
            keys,
            flags: 0x48,
            freq: 1.0,
            start: 0.0,
            stop: 1.0,
        };
        let ops = if owner == Owner::Alpha && prop.users.len() > 1 {
            // One history step; the add re-anchors on the new alpha (B5).
            vec![
                Op::SplitAlpha {
                    at: anchor_shape(&e.model, shape.block),
                },
                add,
            ]
        } else {
            vec![add]
        };
        act.push = Some((ops, None));
    }
    // AddController's validate_ops errors target the property.
    warnings(ui, e, &Target::Block { block: prop.block }, act);
}

/// D9: the property's current value of float variable `var` (the curve starts flat at it).
fn float_value(e: &NifEdit, lsp: u32, var: u32) -> [f32; 3] {
    let Some(p) = e.model.lighting.get(&lsp).map(|l| &l.lighting.params) else {
        return [0.0; 3];
    };
    let v = match var {
        0 => p.refraction_strength,
        8 => match p.trailer {
            LightingTrailer::EnvMap { scale } => scale,
            _ => 1.0,
        },
        9 => p.glossiness,
        10 => p.specular_strength,
        11 => p.emissive_multiple,
        12 => p.alpha,
        20 => p.uv_offset[0],
        21 => p.uv_scale[0],
        22 => p.uv_offset[1],
        23 => p.uv_scale[1],
        _ => 0.0,
    };
    [v, 0.0, 0.0]
}

/// D9: the property's current colour for colour variable `var` (0 specular, 1 emissive).
fn color_value(e: &NifEdit, lsp: u32, var: u32) -> [f32; 3] {
    e.model.lighting.get(&lsp).map_or([0.0; 3], |l| match var {
        0 => l.lighting.params.specular_color,
        _ => l.lighting.params.emissive_color,
    })
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
