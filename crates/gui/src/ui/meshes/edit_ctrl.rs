//! The edit panel's Controllers tab (spec §7, plan A18): the scalars of each time controller on the selected
//! shape's shader and alpha chains, and (v2 B6) each controller's keys: a curve, a key table, Add and Remove key,
//! Interpolation, Remove controller, and an Add controller row per property.
use super::curve::{self, Part};
use super::edit_fields::{
    fit_width, float_named, from_picker, row_label, selected, set_name, warnings, Act, OUT_OF_RANGE,
};
use crate::model::meshes::nif_edit::{NifEdit, Playing};
use crate::model::meshes::MeshesModel;
use crate::model::viewport3d::Clock;
use crate::ui::status::{AMBER, RED};
use eframe::egui;
use nif::ctrl::{Sequence, TimeCtrl};
use nif::edit::validate::{ctrl_title, var_label, Target, ENV_SCALE_PBR};
use nif::edit::{
    anchor_role, anchor_shape, convert_check, merge_key, Anchor, ClipTrack, Op, Outcome, Owner,
    Role, CONVERT_HOVER, CONVERT_NAME,
};
use nif::fields::{Field, Value};
use nif::keys::{Key, KeyData, KeyGroup};
use nif::material::SLSF2_TRUE_PBR;
use nif::model::{PropModel, ShapeModel};
use nif::props::LightingTrailer;

const LSCF: &str = "BSLightingShaderPropertyFloatController";
const LSCC: &str = "BSLightingShaderPropertyColorController";
const TEST_REF: &str = "BSNiAlphaPropertyTestRefController";
/// The values the Variable combo offers on a float controller.
const FLOAT_VARS: [u32; 10] = [0, 8, 9, 10, 11, 12, 20, 21, 22, 23];
const COLOR_VARS: [u32; 2] = [0, 1];
const CYCLES: [&str; 3] = ["Loop", "Reverse", "Clamp"];
const PICK_CLIP: &str = "Pick a clip in Preview to rename or remove it.";
const NO_NAME: &str = "Type a name for the clip.";
const NO_TRACK: &str = "Tick at least one controller.";
const BAD_RANGE: &str = "Stop must be after start.";
const NOT_ROOT_CLIP: &str =
    "This clip is not on the root's manager: it cannot be renamed or removed here.";
/// C2's root refusal (`nif::edit` keeps it private), shown before the click.
const NO_ROOT: &str = "The root is not a node: a clip needs a node root.";

/// The shape's shader flags, 0 without a lit shader: the variable labels and the PBR guard (B-P39) read them.
fn shape_slsf2(e: &NifEdit, shape: &ShapeModel) -> u32 {
    shape
        .shader
        .and_then(|b| e.model.lighting.get(&b))
        .map_or(0, |l| l.lighting.slsf2)
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
    if !e.model.sequences.is_empty() {
        preview(ui, e, act);
        clip_actions(ui, e, act);
    }
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
    ui.strong("New clip");
    new_clip(ui, e, act, shape);
}

/// A clip's name, or "Clip {block}" for a null name.
fn clip_name(s: &Sequence) -> String {
    s.name
        .clone()
        .unwrap_or_else(|| format!("Clip {}", s.block))
}

fn cycle_name(c: u32) -> String {
    CYCLES
        .get(c as usize)
        .map_or(format!("Cycle {c}"), |s| s.to_string())
}

/// The Preview selector (v2 B7, E7): free chains or one clip. A clip pauses the free chains (E6).
fn preview(ui: &mut egui::Ui, e: &NifEdit, act: &mut Act) {
    ui.horizontal(|ui| {
        row_label(ui, "Preview");
        let mut pick = e.playing;
        let shown = match e.playing {
            Playing::Free => "Free chains".to_string(),
            Playing::Clip(b) => e
                .model
                .sequences
                .iter()
                .find(|s| s.block == b)
                .map_or(format!("Clip {b}"), clip_name),
        };
        let r = egui::ComboBox::from_id_salt("meshes-edit-preview")
            .width(fit_width(ui, 200.0))
            .truncate()
            .selected_text(&shown)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut pick, Playing::Free, "Free chains");
                for s in &e.model.sequences {
                    ui.selectable_value(&mut pick, Playing::Clip(s.block), clip_name(s));
                }
            })
            .response;
        set_name(
            &r,
            egui::WidgetType::ComboBox,
            "Preview animation",
            None,
            Some(shown),
        );
        r.on_hover_text("Shows one clip at a time; the free controllers pause while a clip shows.");
        if pick != e.playing {
            act.playing = Some(pick);
        }
    });
}

/// C5-D3: Some(why) when `ops` would not apply to the current bytes. Parses the file: call it on a click only.
fn refusal(e: &NifEdit, ops: &[Op]) -> Option<String> {
    match nif::edit::apply(&e.patched, ops) {
        Err(err) => Some(err.to_string()),
        Ok(a) => a.outcomes.into_iter().find_map(|o| match o {
            Outcome::Applied | Outcome::ReAnchored { .. } => None,
            Outcome::Conflict(s) | Outcome::Refused(s) => Some(s),
        }),
    }
}

/// C5-D3: pushes `ops` (one undo step) when they apply; otherwise keeps why under `id` for [`refused_line`],
/// tagged with the row's `inputs`.
fn try_push(
    ui: &egui::Ui,
    e: &NifEdit,
    act: &mut Act,
    id: egui::Id,
    inputs: String,
    ops: Result<Vec<Op>, String>,
) -> bool {
    match ops.and_then(|ops| refusal(e, &ops).map_or(Ok(ops), Err)) {
        Ok(ops) => {
            act.push = Some((ops, None));
            ui.data_mut(|d| d.remove::<(String, String)>(id));
            true
        }
        Err(why) => {
            ui.data_mut(|d| d.insert_temp(id, (inputs, why)));
            false
        }
    }
}

/// The red refusal under a row, while its `inputs` (and the file's bytes) are still those of the refused click.
fn refused_line(ui: &mut egui::Ui, id: egui::Id, inputs: &str) {
    let Some((i, why)) = ui.data(|d| d.get_temp::<(String, String)>(id)) else {
        return;
    };
    if i == inputs {
        ui.add(egui::Label::new(egui::RichText::new(why).color(RED)).wrap());
    } else {
        ui.data_mut(|d| d.remove::<(String, String)>(id));
    }
}

/// The current bytes' identity: a refusal shown for one file state is cleared by any edit, undo or reload.
fn bytes_tag(e: &NifEdit) -> String {
    format!("{:p}", std::sync::Arc::as_ptr(&e.patched))
}

/// Rename and Remove for the clip the Preview shows, with its cycle and range (C5-D1, F14).
fn clip_actions(ui: &mut egui::Ui, e: &NifEdit, act: &mut Act) {
    let seq = match e.playing {
        Playing::Clip(b) => e.model.sequences.iter().find(|s| s.block == b),
        Playing::Free => None,
    };
    let name = seq.map(clip_name).unwrap_or_default();
    let id = egui::Id::new("meshes-edit-clip");
    let err = id.with("refused");
    let inputs = format!("{}|{:?}", bytes_tag(e), seq.map(|s| s.block));
    // C5-D8: path_field's pattern; the buffer lives only while the field has focus.
    let mut commit = None;
    ui.horizontal(|ui| {
        row_label(ui, "Clip name");
        let mut buf = ui
            .data(|d| d.get_temp::<String>(id))
            .unwrap_or_else(|| name.clone());
        let r = ui.add_enabled(
            seq.is_some(),
            egui::TextEdit::singleline(&mut buf)
                .id(id.with("field"))
                .desired_width(fit_width(ui, 200.0)),
        );
        set_name(
            &r,
            egui::WidgetType::TextEdit,
            "Clip name",
            None,
            Some(buf.clone()),
        );
        if r.lost_focus() {
            ui.data_mut(|d| d.remove::<String>(id));
            if !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                commit = Some(buf);
            }
        } else if r.has_focus() {
            ui.data_mut(|d| d.insert_temp(id, buf));
        } else {
            ui.data_mut(|d| d.remove::<String>(id));
        }
        r.on_disabled_hover_text(PICK_CLIP);
    });
    let mut remove = false;
    egui::Sides::new().shrink_left().truncate().show(
        ui,
        |ui| {
            if let Some(s) = seq {
                ui.weak(format!(
                    "{} · {} to {} s",
                    cycle_name(s.cycle),
                    s.start,
                    s.stop
                ));
            }
        },
        |ui| {
            let r = ui.add_enabled(seq.is_some(), egui::Button::new("Remove clip"));
            let label = match seq {
                Some(_) => format!("Remove clip {name}"),
                None => "Remove clip".into(),
            };
            set_name(&r, egui::WidgetType::Button, &label, None, None);
            remove = r.clicked();
            r.on_disabled_hover_text(PICK_CLIP);
        },
    );
    if let Some(s) = seq {
        let root = anchor_role(&e.model, None, Role::Root).ok_or(NO_ROOT.to_string());
        // C5-D2: the manager's list order, read on the click only
        let key =
            || nif::edit::clip_key(&e.patched, &e.model, s.block).ok_or(NOT_ROOT_CLIP.to_string());
        match commit {
            Some(new) if new.is_empty() => {
                try_push(ui, e, act, err, inputs.clone(), Err(NO_NAME.into()));
            }
            Some(new) if new != name => {
                let ops = root.clone().and_then(|at| {
                    let (name, nth) = key()?;
                    Ok(vec![Op::RenameSequence {
                        at,
                        name,
                        nth,
                        new_name: new,
                    }])
                });
                try_push(ui, e, act, err, inputs.clone(), ops);
            }
            _ => {}
        }
        if remove {
            let ops = root.and_then(|at| {
                let (name, nth) = key()?;
                Ok(vec![Op::RemoveSequence { at, name, nth }])
            });
            try_push(ui, e, act, err, inputs.clone(), ops);
        }
    }
    refused_line(ui, err, &inputs);
    ui.separator();
}

/// Convert to clips for one property chain (C5-D4): a name field and the button.
fn convert_row(
    ui: &mut egui::Ui,
    e: &NifEdit,
    act: &mut Act,
    shape: &ShapeModel,
    owner: Owner,
    prop: &PropModel,
) {
    let (role, what) = match owner {
        Owner::Shader => (Role::Shader, "shader"),
        Owner::Alpha => (Role::Alpha, "alpha"),
    };
    let check = convert_check(&e.model, prop.block, shape.name.as_deref());
    let id = egui::Id::new(("meshes-edit-convert", prop.block));
    let mut name: String = ui
        .data(|d| d.get_temp(id))
        .unwrap_or_else(|| CONVERT_NAME.to_string());
    let mut clicked = false;
    egui::Sides::new().shrink_left().truncate().show(
        ui,
        |ui| {
            let r = ui.add_enabled(
                check.is_ok(),
                egui::TextEdit::singleline(&mut name)
                    .desired_width(ui.available_width().min(200.0)),
            );
            set_name(
                &r,
                egui::WidgetType::TextEdit,
                &format!("Convert {what} clip name"),
                None,
                Some(name.clone()),
            );
            match &check {
                Ok(_) => r.on_hover_text(CONVERT_HOVER),
                Err(why) => r.on_disabled_hover_text(why),
            };
        },
        |ui| {
            let r = ui.add_enabled(check.is_ok(), egui::Button::new("Convert to clips"));
            set_name(
                &r,
                egui::WidgetType::Button,
                &format!("Convert {what} controllers to clips"),
                None,
                None,
            );
            clicked = r.clicked();
            match &check {
                Ok(_) => r.on_hover_text(CONVERT_HOVER),
                Err(why) => r.on_disabled_hover_text(why),
            };
        },
    );
    let err = id.with("refused");
    let inputs = format!("{}|{name}", bytes_tag(e));
    if clicked {
        let ops = anchor_role(&e.model, Some(shape.block), role)
            .map(|at| {
                vec![Op::ConvertChainToClips {
                    at,
                    name: name.clone(),
                }]
            })
            .ok_or_else(|| "This property cannot be found again in the file.".to_string());
        try_push(ui, e, act, err, inputs.clone(), ops);
    }
    ui.data_mut(|d| d.insert_temp(id, name));
    refused_line(ui, err, &inputs);
}

#[derive(Clone, Debug, PartialEq)]
struct NewClip {
    name: String,
    cycle: u32,
    start: f32,
    stop: f32,
    picked: Vec<u32>,
}

/// C5-D6: no name, Clamp (the census's most common cycle), 0 to 1 s, nothing ticked.
impl Default for NewClip {
    fn default() -> Self {
        NewClip {
            name: String::new(),
            cycle: 2,
            start: 0.0,
            stop: 1.0,
            picked: vec![],
        }
    }
}

/// The New clip form (C5-D5..D7). State in temp data under `Id::new("meshes-edit-new-clip")`.
fn new_clip(ui: &mut egui::Ui, e: &NifEdit, act: &mut Act, shape: &ShapeModel) {
    let id = egui::Id::new("meshes-edit-new-clip");
    let mut nc: NewClip = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let root = anchor_role(&e.model, None, Role::Root).filter(|_| e.model.root_net.is_some());
    let no_root = root.is_none().then_some(NO_ROOT);
    let slsf2 = shape_slsf2(e, shape);
    ui.horizontal(|ui| {
        row_label(ui, "Name");
        let r = ui.add_enabled(
            no_root.is_none(),
            egui::TextEdit::singleline(&mut nc.name).desired_width(fit_width(ui, 200.0)),
        );
        set_name(
            &r,
            egui::WidgetType::TextEdit,
            "New clip name",
            None,
            Some(nc.name.clone()),
        );
        if let Some(why) = no_root {
            r.on_disabled_hover_text(why);
        }
    });
    ui.horizontal(|ui| {
        row_label(ui, "Cycle");
        let shown = cycle_name(nc.cycle);
        let r = egui::ComboBox::from_id_salt(id.with("cycle"))
            .width(120.0)
            .selected_text(&shown)
            .show_ui(ui, |ui| {
                for (i, n) in CYCLES.iter().enumerate() {
                    ui.selectable_value(&mut nc.cycle, i as u32, *n);
                }
            })
            .response;
        set_name(
            &r,
            egui::WidgetType::ComboBox,
            "New clip cycle",
            None,
            Some(shown),
        );
    });
    for (label, name, v) in [
        ("Start", "New clip start", &mut nc.start),
        ("Stop", "New clip stop", &mut nc.stop),
    ] {
        ui.horizontal(|ui| {
            row_label(ui, label);
            let r = ui.add(egui::DragValue::new(v).speed(0.01).suffix(" s"));
            set_name(&r, egui::WidgetType::DragValue, name, Some(*v as f64), None);
        });
    }
    // C5-D7: the selected shape's shader and alpha chains, editable classes only
    let props = [
        shape
            .shader
            .and_then(|b| e.model.lighting.get(&b))
            .map(|l| (Owner::Shader, &l.prop)),
        shape
            .alpha
            .and_then(|b| e.model.alphas.get(&b))
            .map(|a| (Owner::Alpha, &a.prop)),
    ];
    let mut tracks = vec![];
    for (owner, prop) in props.into_iter().flatten() {
        let shared = (owner == Owner::Alpha && prop.users.len() > 1).then(|| {
            format!(
                "This alpha is shared by {} shapes: split it before editing.",
                prop.users.len()
            )
        });
        for i in 0..prop.ctrls.len() {
            let Some((c, at)) = ctrl_anchor(e, shape, owner, prop, i).filter(|(c, _)| editable(c))
            else {
                continue;
            };
            let title = ctrl_title(c, slsf2);
            ui.horizontal(|ui| {
                let mut on = nc.picked.contains(&c.block);
                let r = ui.add_enabled(shared.is_none(), egui::Checkbox::new(&mut on, &title));
                let (name, enabled, sel) =
                    (format!("Add {title} to the new clip"), r.enabled(), on);
                r.widget_info(move || {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::Checkbox,
                        enabled,
                        sel,
                        name.clone(),
                    )
                });
                if r.changed() {
                    nc.picked.retain(|&b| b != c.block);
                    if on {
                        nc.picked.push(c.block);
                    }
                }
                if let Some(why) = &shared {
                    r.on_disabled_hover_text(why);
                }
                ui.weak(short_class(&c.class));
            });
            if shared.is_none() && nc.picked.contains(&c.block) {
                tracks.push((prop, c, at));
            }
        }
    }
    let why = no_root
        .or(nc.name.is_empty().then_some(NO_NAME))
        .or(tracks.is_empty().then_some(NO_TRACK))
        .or(
            (nc.stop.partial_cmp(&nc.start) != Some(std::cmp::Ordering::Greater))
                .then_some(BAD_RANGE),
        );
    let r = ui.add_enabled(why.is_none(), egui::Button::new("Add clip"));
    set_name(&r, egui::WidgetType::Button, "Add clip", None, None);
    let clicked = r.clicked();
    if let Some(why) = why {
        r.on_disabled_hover_text(why);
    }
    let err = id.with("refused");
    let inputs = format!("{}|{nc:?}", bytes_tag(e));
    if let (true, Some(at)) = (clicked, root) {
        let tracks = tracks
            .into_iter()
            .map(|(prop, c, at)| {
                // C5-D5: the controller's own keys; a managed or keyless one starts flat over the clip's range
                let keys = match e.keys.get(&(c.block, None)) {
                    Some(Ok(k)) if has_keys(k) => k.clone(),
                    _ => flat_keys(e, prop, &c.class, c.var, [nc.start, nc.stop]),
                };
                ClipTrack { at, keys }
            })
            .collect();
        let ops = vec![Op::AddSequence {
            at,
            name: nc.name.clone(),
            cycle: nc.cycle,
            freq: 1.0,
            start: nc.start,
            stop: nc.stop,
            tracks,
        }];
        if try_push(ui, e, act, err, inputs.clone(), Ok(ops)) {
            nc = NewClip::default();
        }
    }
    ui.data_mut(|d| d.insert_temp(id, nc));
    refused_line(ui, err, &inputs);
}

fn has_keys(k: &KeyData) -> bool {
    match k {
        KeyData::Float(g) => !g.keys.is_empty(),
        KeyData::Pos(g) => !g.keys.is_empty(),
        KeyData::Bool(g) => !g.keys.is_empty(),
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
    let slsf2 = shape_slsf2(e, shape);
    let users = prop.users.len();
    let mut locked = false;
    // D9: only a shared alpha must split before an add (clip controllers included, C4). A shared shader adds in
    // place (its banner says the edit changes every shape).
    if users > 1 {
        if owner == Owner::Alpha {
            // An alpha edit on a shared alpha is a conflict: the split comes first (as on the Alpha tab).
            locked = true;
            let text =
                format!("Shared by {users} shapes: split the alpha to edit its controllers.");
            ui.add(egui::Label::new(egui::RichText::new(text).color(AMBER)).wrap());
            if ui.button("Split alpha").clicked() {
                let at = anchor_shape(&e.model, shape.block);
                act.push = Some((vec![Op::SplitAlpha { at }], None));
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
        for i in 0..prop.ctrls.len() {
            if let Some((c, at)) = ctrl_anchor(e, shape, owner, prop, i) {
                ui.group(|ui| row(ui, e, act, c, &at, clock, slsf2));
            }
        }
    });
    // Outside the lock: a shared alpha shows convert_check's reason; it adds through Split + Add.
    if !prop.ctrls.is_empty() {
        convert_row(ui, e, act, shape, owner, prop);
    }
    add_row(ui, e, act, shape, owner, prop);
}

/// Controller `i` of `prop` and its row's anchor: class, variable, and how many earlier controllers share both.
fn ctrl_anchor<'a>(
    e: &'a NifEdit,
    shape: &ShapeModel,
    owner: Owner,
    prop: &PropModel,
    i: usize,
) -> Option<(&'a TimeCtrl, Anchor)> {
    let c = e.model.ctrls.get(prop.ctrls.get(i)?)?;
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
    Some((c, at))
}

fn row(
    ui: &mut egui::Ui,
    e: &NifEdit,
    act: &mut Act,
    c: &TimeCtrl,
    at: &Anchor,
    clock: Option<Clock>,
    slsf2: u32,
) {
    let managed = e.model.managed(c.block);
    let title = ctrl_title(c, slsf2);
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
        variable(ui, e, act, c, at, &title, v, managed, slsf2);
    }
    let cycle = (c.flags >> 1) & 3;
    ui.horizontal(|ui| {
        row_label(ui, "Cycle");
        let mut pick = cycle;
        let shown = cycle_name(cycle as u32);
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
/// in the clips that drive it (v2 B7).
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
        clips(ui, e, act, c, at, title, clock);
        return;
    }
    match e.keys.get(&(c.block, None)) {
        Some(Ok(keys)) => keys_body(ui, e, act, c, at, title, keys, None, clock),
        Some(Err(why)) => {
            ui.weak(why);
        }
        None => {}
    }
}

/// The clips that drive managed controller `c` (spec §7, v2 B7): each with its cycle, start and stop and text keys;
/// the selected clip's curve and keys.
fn clips(
    ui: &mut egui::Ui,
    e: &NifEdit,
    act: &mut Act,
    c: &TimeCtrl,
    at: &Anchor,
    title: &str,
    clock: Option<Clock>,
) {
    let mut any = false;
    for s in &e.model.sequences {
        let Some(entry) = e.keys.get(&(c.block, Some(s.block))) else {
            continue;
        };
        any = true;
        let name = clip_name(s);
        ui.strong(&name);
        ui.weak(format!(
            "{} · {} to {} s",
            cycle_name(s.cycle),
            s.start,
            s.stop
        ));
        if !s.text_keys.is_empty() {
            let list: Vec<String> = s
                .text_keys
                .iter()
                .map(|(t, n)| {
                    let n = n.as_deref().map_or("(none)".into(), nif::text::lossy);
                    format!("{n} at {t} s")
                })
                .collect();
            let text = format!("Text keys: {}", list.join(", "));
            ui.add(egui::Label::new(egui::RichText::new(&text).weak()).truncate())
                .on_hover_text(&text);
        }
        match entry {
            Err(why) => {
                ui.weak(why);
            }
            Ok(keys) if e.playing == Playing::Clip(s.block) => {
                let title = format!("{title} in {name}");
                keys_body(ui, e, act, c, at, &title, keys, Some(s), clock);
            }
            Ok(_) => {
                ui.weak("Pick this clip in Preview to see and edit its keys.");
            }
        }
    }
    if !any {
        // 0x20 set and no CB: the preview holds the property value (G2).
        ui.weak("No clip in this file drives it.");
    }
}

/// Interpolation, the curve, the key table and Add key on `keys`: controller `c`'s own (`clip: None`), or clip
/// `clip`'s, timed by its sequence (frequency, start, stop, cycle; phase 0, as `clip_tracks`).
#[allow(clippy::too_many_arguments)]
fn keys_body(
    ui: &mut egui::Ui,
    e: &NifEdit,
    act: &mut Act,
    c: &TimeCtrl,
    at: &Anchor,
    title: &str,
    keys: &KeyData,
    clip: Option<&Sequence>,
    clock: Option<Clock>,
) {
    let (span, freq, phase, flags) = match clip {
        Some(s) => {
            let flags = 0x08 | ((s.cycle.min(3) as u16) << 1);
            ((s.start, s.stop), s.frequency, 0.0, flags)
        }
        None => ((c.start, c.stop), c.frequency, c.phase, c.flags),
    };
    let seq = clip.map(|s| s.block);
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
            let r = egui::ComboBox::from_id_salt(("meshes-edit-ctrl-interp", c.block, seq))
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
                push_keys(act, e, c, at, seq, curve::convert(keys, pick), keys, false);
            }
        });
    }
    let cursor = clock.map(|k| nif::anim::ctrl_time(k.t, freq, phase, span.0, span.1, flags));
    let id = egui::Id::new(("meshes-edit-curve", c.block, seq));
    let out = curve::curve(ui, id, title, keys, span, cursor, editable);
    if let Some(k) = out.keys {
        push_keys(act, e, c, at, seq, k, keys, true);
    }
    act.flush |= out.released;
    if let Some(kt) = out.scrub {
        // shortcut: the first-cycle inverse of ctrl_time; a scrub on a looping or reversed controller past its first
        // cycle lands in the first one. Upgrade if users scrub multi-cycle curves.
        act.scrub = Some(if freq != 0.0 && freq.is_finite() {
            (kt - phase) / freq
        } else {
            kt
        });
    }
    key_table(ui, e, act, c, at, seq, title, keys, editable);
    add_key(ui, e, act, c, at, seq, title, keys, editable, span);
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

/// Colour key `k`'s picker swatch. Returns the keys with the picked colour when it changed, and whether the step
/// ended (the popup closed, or a drag in it was released). Disabled outside 0-1: the picker would clamp.
fn swatch(ui: &mut egui::Ui, keys: &KeyData, k: usize, name: &str) -> (Option<KeyData>, bool) {
    let v: [f32; 3] = std::array::from_fn(|ch| curve::get(keys, k, ch, Part::Value));
    let hdr = v.iter().any(|x| !(0.0..=1.0).contains(x));
    // egui writes the picker's colour back through HSV every frame (not bit-exact): use it only on a change.
    let lin = v.map(egui::ecolor::linear_from_gamma);
    let mut picked = lin;
    let (r, was, open) = ui
        .add_enabled_ui(!hdr, |ui| {
            // The id egui's colour button gives its popup (auto_id_with does not advance the id).
            let popup = ui.auto_id_with("popup");
            let was = egui::Popup::is_id_open(ui.ctx(), popup);
            let r = ui.color_edit_button_rgb(&mut picked);
            (r, was, egui::Popup::is_id_open(ui.ctx(), popup))
        })
        .inner;
    set_name(&r, egui::WidgetType::ColorButton, name, None, None);
    let done = (was && !open) || (open && ui.input(|i| i.pointer.any_released()));
    let changed = r.changed();
    if hdr {
        r.on_disabled_hover_text(OUT_OF_RANGE);
    }
    if !changed {
        return (None, done);
    }
    let x = from_picker(v, lin, picked);
    let mut d = keys.clone();
    let mut any = false;
    for ch in 0..3 {
        if x[ch].to_bits() != v[ch].to_bits() {
            curve::set(&mut d, k, ch, Part::Value, x[ch]);
            any = true;
        }
    }
    (any.then_some(d), done)
}

/// The key table: one row per key with its time, value(s) and, for QUADRATIC, tangents, and a Remove button.
#[allow(clippy::too_many_arguments)]
fn key_table(
    ui: &mut egui::Ui,
    e: &NifEdit,
    act: &mut Act,
    c: &TimeCtrl,
    at: &Anchor,
    clip: Option<u32>,
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
        // Narrow minimum cells (grid columns, DragValues, the swatch): a LINEAR colour row fits the panel at 1100 px.
        ui.spacing_mut().interact_size.x = 24.0;
        // shortcut: every key is a row; corpus curves have <= 8 keys. Use show_rows past a few hundred.
        egui::Grid::new(("meshes-edit-keys", c.block, clip))
            .striped(true)
            .show(ui, |ui| {
                ui.label("Key");
                ui.label("Time");
                if nch == 3 {
                    ui.label("Colour");
                }
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
                    if nch == 3 {
                        let (d, done) = swatch(ui, keys, k, &format!("{title} key {num} colour"));
                        act.flush |= done;
                        if d.is_some() {
                            next = d.map(|d| (d, true));
                        }
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
        push_keys(act, e, c, at, clip, d, keys, merge);
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

/// Add key (B-P43, replaces D10): after the last key, at its time plus the average key spacing (0.1 for one key or
/// a spacing of 0), with the last key's value; QUADRATIC tangents 0. The time is finite and strictly later (B-P45:
/// the next f32 up when the step rounds away; off when no finite time is later). An empty group
/// starts LINEAR at `span`'s start with the value of the interpolator it edits (the controller's, or the clip's
/// bound CB's). Past a controller's stop, `push_keys` moves the stop in the same step (B-P44).
#[allow(clippy::too_many_arguments)]
fn add_key(
    ui: &mut egui::Ui,
    e: &NifEdit,
    act: &mut Act,
    c: &TimeCtrl,
    at: &Anchor,
    clip: Option<u32>,
    title: &str,
    keys: &KeyData,
    editable: bool,
    span: (f32, f32),
) {
    let n = curve::len(keys);
    let t = match n {
        0 => Some(span.0),
        _ => {
            let last = curve::time(keys, n - 1);
            let avg = (last - curve::time(keys, 0)) / (n - 1).max(1) as f32;
            let step = if avg > 0.0 { avg } else { 0.1 };
            // B-P45: finite and strictly later; a step lost to rounding gives the next f32 up.
            let t = last + step;
            match t.is_finite() && t > last {
                true => Some(t),
                false => (last.is_finite() && last < f32::MAX).then(|| last.next_up()),
            }
        }
    };
    let r = ui.add_enabled(editable && t.is_some(), egui::Button::new("Add key"));
    set_name(
        &r,
        egui::WidgetType::Button,
        &format!("Add key to {title}"),
        None,
        None,
    );
    let clicked = r.clicked();
    if editable && t.is_none() {
        r.on_disabled_hover_text("No later time is possible for a new key.");
    }
    let Some(t) = t.filter(|_| clicked) else {
        return;
    };
    let v = if n > 0 {
        let nch = curve::channels(keys);
        std::array::from_fn(|ch| match ch < nch {
            true => curve::get(keys, n - 1, ch, Part::Value),
            false => 0.0,
        })
    } else {
        // A clip's interpolator through the shared binding (by ref or by strings); parsed on the click only.
        let interp = match clip {
            Some(seq) => nif::anim::clip_interp(&e.model, &e.patched, seq, c.block),
            None => c.interp,
        };
        let interp = interp
            .and_then(|i| u32::try_from(i).ok())
            .and_then(|i| e.model.interps.get(&i))
            .map(|i| i.value.clone())
            .unwrap_or_default();
        let at = |k: usize| interp.get(k).copied().unwrap_or(0.0);
        [at(0), at(1), at(2)]
    };
    fn append<T: Clone>(g: &KeyGroup<T>, t: f32, value: T, zero: T) -> KeyGroup<T> {
        let quad = g.key_type == Some(2);
        let mut g = g.clone();
        g.key_type.get_or_insert(1);
        let tangent = quad.then_some(zero);
        g.keys.push(Key {
            time: t,
            value,
            forward: tangent.clone(),
            backward: tangent,
            tbc: None,
        });
        g
    }
    let next = match keys {
        KeyData::Float(g) => KeyData::Float(append(g, t, v[0], 0.0)),
        KeyData::Pos(g) => KeyData::Pos(append(g, t, v, [0.0; 3])),
        KeyData::Bool(_) => return,
    };
    push_keys(act, e, c, at, clip, next, keys, false);
}

/// One SetKeys on the anchored controller's own keys (`clip: None`) or a clip's. B-P44: on a controller's own keys
/// the same step sets its stop to the last key's time, and its start to the first key's when that is earlier;
/// each only when it changes from its value before this edit (or drag). A clip's sequence keeps its times.
#[allow(clippy::too_many_arguments)]
fn push_keys(
    act: &mut Act,
    e: &NifEdit,
    c: &TimeCtrl,
    at: &Anchor,
    clip: Option<u32>,
    keys: KeyData,
    base: &KeyData,
    merge: bool,
) {
    let n = curve::len(&keys);
    let ends = (n > 0).then(|| (curve::time(&keys, 0), curve::time(&keys, n - 1)));
    let op = Op::SetKeys {
        at: at.clone(),
        clip,
        keys,
        base: base.clone(),
    };
    let key = merge_key(&op);
    let mut ops = vec![op];
    if let (None, Some((first, last))) = (clip, ends) {
        let drag = if merge { e.drag_ops(&key) } else { &[] };
        // A drag's start / stop op keeps the value from before the drag as its base.
        let was = |field: Field, now: f32| {
            drag.iter()
                .find_map(|d| match &d.op {
                    Op::SetField {
                        field: f,
                        base: Value::F32(b),
                        ..
                    } if *f == field => Some(f32::from_bits(*b)),
                    _ => None,
                })
                .unwrap_or(now)
        };
        let (start, stop) = (was(Field::StartTime, c.start), was(Field::StopTime, c.stop));
        for (field, from, to) in [
            (
                Field::StartTime,
                start,
                if first < start { first } else { start },
            ),
            (Field::StopTime, stop, last),
        ] {
            if to.is_finite() && to.to_bits() != from.to_bits() {
                ops.push(Op::SetField {
                    at: at.clone(),
                    field,
                    value: Value::F32(to.to_bits()),
                    base: Value::F32(from.to_bits()),
                });
            }
        }
    }
    act.push = Some((ops, merge.then_some(key)));
}

/// Adds a controller to `prop` (D9). Shader: a class (Float | Colour) and variable picker; alpha: TestRef only.
fn add_row(
    ui: &mut egui::Ui,
    e: &NifEdit,
    act: &mut Act,
    shape: &ShapeModel,
    owner: Owner,
    prop: &PropModel,
) {
    let slsf2 = shape_slsf2(e, shape);
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
        // B-P39: env map scale on PBR crashed the game. A remembered 8 falls back to the default.
        let pbr = slsf2 & SLSF2_TRUE_PBR != 0;
        let env_blocked = |class_idx: usize, v: u32| pbr && class_idx == 0 && v == 8;
        if env_blocked(class_idx, var) {
            var = 20;
        }
        ui.horizontal(|ui| {
            row_label(ui, "Variable");
            let (class, offered): (&str, &[u32]) = match class_idx {
                0 => (LSCF, &FLOAT_VARS),
                _ => (LSCC, &COLOR_VARS),
            };
            let shown = format!("{} ({var})", var_label(class, var, slsf2));
            let r = egui::ComboBox::from_id_salt(id.with("var"))
                .width(fit_width(ui, 180.0))
                .truncate()
                .selected_text(&shown)
                .show_ui(ui, |ui| {
                    for &o in offered {
                        let text = format!("{} ({o})", var_label(class, o, slsf2));
                        if env_blocked(class_idx, o) {
                            ui.add_enabled(false, egui::Button::selectable(false, text))
                                .on_disabled_hover_text(ENV_SCALE_PBR);
                        } else {
                            ui.selectable_value(&mut var, o, text);
                        }
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
        at.is_some() && !no_root,
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
    }
    if let (true, Some(at)) = (clicked, at) {
        let (class, var) = match owner {
            Owner::Shader if class_idx == 0 => (LSCF, Some(var)),
            Owner::Shader => (LSCC, Some(var)),
            Owner::Alpha => (TEST_REF, None),
        };
        let keys = flat_keys(e, prop, class, var, [0.0, 1.0]);
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

/// Two LINEAR keys at `times` holding the property's current value for (`class`, `var`): D9's flat start, and
/// C5-D5's track for a controller without keys of its own. TestRef holds the alpha threshold.
fn flat_keys(
    e: &NifEdit,
    prop: &PropModel,
    class: &str,
    var: Option<u32>,
    times: [f32; 2],
) -> KeyData {
    let var = var.unwrap_or(0);
    let value = match class {
        LSCF => float_value(e, prop.block, var),
        LSCC => color_value(e, prop.block, var),
        _ => {
            let t = e
                .model
                .alphas
                .get(&prop.block)
                .map_or(0.0, |a| a.alpha.threshold as f32);
            [t, 0.0, 0.0]
        }
    };
    fn linear<T>(time: f32, value: T) -> Key<T> {
        Key {
            time,
            value,
            forward: None,
            backward: None,
            tbc: None,
        }
    }
    match class {
        LSCC => KeyData::Pos(KeyGroup {
            key_type: Some(1),
            keys: times.map(|t| linear(t, value)).into(),
        }),
        _ => KeyData::Float(KeyGroup {
            key_type: Some(1),
            keys: times.map(|t| linear(t, value[0])).into(),
        }),
    }
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
    slsf2: u32,
) {
    ui.horizontal(|ui| {
        row_label(ui, "Variable");
        let offered: &[u32] = if c.class == LSCC {
            &COLOR_VARS
        } else {
            &FLOAT_VARS
        };
        let mut pick = v;
        let shown = var_label(&c.class, v, slsf2);
        // Phase A ruling R8 lifted (v2 B7): a managed controller's clips follow the new variable.
        let r = egui::ComboBox::from_id_salt(("meshes-edit-ctrl-var", c.block))
            .width(fit_width(ui, 180.0))
            .truncate()
            .selected_text(&shown)
            .show_ui(ui, |ui| {
                // A value outside the list stays selectable, so the current one is never lost.
                let extra = (!offered.contains(&v)).then_some(v);
                for &o in extra.iter().chain(offered) {
                    let text = format!("{} ({o})", var_label(&c.class, o, slsf2));
                    // B-P40: as in the Add picker. A current 8 stays shown (selected text) with its Error.
                    if c.class == LSCF && o == 8 && slsf2 & SLSF2_TRUE_PBR != 0 {
                        ui.add_enabled(false, egui::Button::selectable(pick == o, text))
                            .on_disabled_hover_text(ENV_SCALE_PBR);
                    } else {
                        ui.selectable_value(&mut pick, o, text);
                    }
                }
            })
            .response;
        set_name(
            &r,
            egui::WidgetType::ComboBox,
            &format!("{title} variable"),
            None,
            Some(shown),
        );
        if managed {
            r.on_hover_text("Its clips follow the new variable.");
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
