//! The edit panel's Shader and Alpha tabs (spec §7, plan A17). Widgets only read the session; every change is
//! collected in an [`Act`] that `edit_panel::panel` applies after drawing.
use crate::model::meshes::nif_edit::{edit_slot_label, NifEdit, Selection};
use crate::model::meshes::{shader_type_name, MeshesModel, SHADER_TYPES};
use crate::pgp::alpha::{AlphaFlags, BLEND_MODES, DEFAULT_FLAGS, DEFAULT_THRESH, TEST_FUNCS};
use crate::ui::status::{AMBER, RED};
use eframe::egui;
use nif::edit::validate::{Level, Target};
use nif::edit::{anchor_role, anchor_shape, merge_key, Anchor, Op, Role};
use nif::fields::{Field, Value};
use nif::material::{SLSF1_NAMES, SLSF2_NAMES, SLSF2_TRUE_PBR};
use nif::model::{NifModel, ShapeModel};
use nif::props::LightingTrailer;
use render3d::material::{engine_blend, verified_blend};

/// What the user did this frame; applied after the panel is drawn (the widgets only borrow the session).
#[derive(Default)]
pub(super) struct Act {
    pub push: Option<(Vec<Op>, Option<String>)>,
    /// A drag or a typed value ended: the next change is a new undo step.
    pub flush: bool,
    pub undo: bool,
    pub redo: bool,
    pub solo: Option<bool>,
    pub check: Option<bool>,
    pub keep_mine: Option<usize>,
    pub take_source: Option<usize>,
    pub revert: Option<usize>,
    pub open: Option<String>,
}

const LABEL_W: f32 = 132.0;
const SKINNED_HOVER: &str = "Set by the mesh; changing it breaks skinning.";
const PBR_HOVER: &str = "TruePBR marker: PGPatcher and CS rely on it.";
const TYPE_HOVER: &str = "Changing the type changes the trailer fields.";
const BROWSE_LIMIT: usize = 200;

/// A row's name, painted: the control beside it carries the same name for AccessKit, so the name is one node.
fn row_label(ui: &mut egui::Ui, text: &str) {
    let h = ui.spacing().interact_size.y;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(LABEL_W, h), egui::Sense::hover());
    ui.painter_at(rect).text(
        rect.left_center(),
        egui::Align2::LEFT_CENTER,
        text,
        egui::TextStyle::Body.resolve(ui.style()),
        ui.visuals().text_color(),
    );
}

/// Warnings of `target` under their field: amber, red for errors, with "Fix" when the warning carries a fix.
fn warnings(ui: &mut egui::Ui, e: &NifEdit, target: &Target, act: &mut Act) {
    for w in e.warnings.iter().filter(|w| w.target == *target) {
        let color = if w.level == Level::Error { RED } else { AMBER };
        ui.horizontal(|ui| {
            ui.add_space(LABEL_W);
            ui.add(egui::Label::new(egui::RichText::new(&w.text).color(color)).wrap());
            if let Some(fix) = &w.fix {
                if ui.small_button("Fix").clicked() {
                    act.push = Some((vec![fix.clone()], None));
                }
            }
        });
    }
}

/// A float field: a DragValue that pushes `SetField` (one op per drag, merged by `merge_key`).
fn float(
    ui: &mut egui::Ui,
    e: &NifEdit,
    act: &mut Act,
    at: &Anchor,
    name: &str,
    field: Field,
    v: f32,
) {
    ui.horizontal(|ui| {
        row_label(ui, name);
        let mut x = v;
        let speed = (v.abs() as f64 * 0.01).max(0.01);
        let r = ui.add(egui::DragValue::new(&mut x).speed(speed));
        set_name(&r, egui::WidgetType::DragValue, name, Some(x as f64), None);
        if r.changed() && x.to_bits() != v.to_bits() {
            let op = Op::SetField {
                at: at.clone(),
                field,
                value: Value::F32(x.to_bits()),
                base: Value::F32(v.to_bits()),
            };
            let key = merge_key(&op);
            act.push = Some((vec![op], Some(key)));
        }
        if r.drag_stopped() || r.lost_focus() {
            act.flush = true;
        }
    });
    warnings(
        ui,
        e,
        &Target::Field {
            block: at.block,
            field,
        },
        act,
    );
}

/// Gives `r` (after `ui.add`) the role `typ` and the accessible name `name`, with its numeric or text value.
fn set_name(
    r: &egui::Response,
    typ: egui::WidgetType,
    name: &str,
    value: Option<f64>,
    text: Option<String>,
) {
    let (name, enabled) = (name.to_string(), r.enabled());
    r.widget_info(move || {
        let mut i = egui::WidgetInfo::labeled(typ, enabled, name.clone());
        i.value = value;
        i.current_text_value = text.clone();
        i
    });
}

/// An RGB colour: a picker and the three exact floats. A real change pushes all three channels (an untouched
/// channel with its own bits, so −0.0 survives) as one merged step.
fn color(
    ui: &mut egui::Ui,
    e: &NifEdit,
    act: &mut Act,
    at: &Anchor,
    name: &str,
    field: fn(u8) -> Field,
    v: [f32; 3],
) {
    let mut x = v;
    let (mut done, mut changed) = (false, false);
    ui.horizontal(|ui| {
        row_label(ui, name);
        // egui writes the picker's colour back through HSV every frame (not bit-exact): use it only on a change.
        let mut picked = v;
        let r = ui.color_edit_button_rgb(&mut picked);
        set_name(&r, egui::WidgetType::ColorButton, name, None, None);
        if r.changed() {
            x = picked;
            changed = true;
        }
        for (i, c) in ["R", "G", "B"].into_iter().enumerate() {
            let mut f = x[i];
            let r = ui.add(egui::DragValue::new(&mut f).speed(0.005));
            let n = format!("{name} {c}");
            set_name(&r, egui::WidgetType::DragValue, &n, Some(f as f64), None);
            if r.changed() {
                x[i] = f;
                changed = true;
            }
            done |= r.drag_stopped() || r.lost_focus();
        }
    });
    if changed && x.iter().zip(&v).any(|(a, b)| a.to_bits() != b.to_bits()) {
        let ops: Vec<Op> = (0..3)
            .map(|i| Op::SetField {
                at: at.clone(),
                field: field(i as u8),
                value: Value::F32(x[i].to_bits()),
                base: Value::F32(v[i].to_bits()),
            })
            .collect();
        act.push = Some((ops, Some(format!("Color|{}|{name}", at.block))));
    }
    act.flush |= done;
    warnings(
        ui,
        e,
        &Target::Field {
            block: at.block,
            field: field(0),
        },
        act,
    );
}

/// The trailer floats of a shader type, named, in file order (`Field::Trailer(n)`).
fn trailer_fields(t: &LightingTrailer) -> Vec<(String, f32)> {
    let named = |names: &[&str], v: &[f32]| -> Vec<(String, f32)> {
        names
            .iter()
            .map(|n| n.to_string())
            .zip(v.iter().copied())
            .collect()
    };
    match t {
        LightingTrailer::None => vec![],
        LightingTrailer::EnvMap { scale } => named(&["Env map scale"], &[*scale]),
        LightingTrailer::SkinTint(c) => named(&["Skin tint R", "Skin tint G", "Skin tint B"], c),
        LightingTrailer::HairTint(c) => named(&["Hair tint R", "Hair tint G", "Hair tint B"], c),
        LightingTrailer::ParallaxOcc { max_passes, scale } => {
            named(&["POM max passes", "POM scale"], &[*max_passes, *scale])
        }
        LightingTrailer::MultiLayer {
            inner_thickness,
            refraction_scale,
            inner_texture_scale,
            envmap_strength,
        } => named(
            &[
                "MLP inner thickness",
                "MLP refraction scale",
                "MLP inner scale U",
                "MLP inner scale V",
                "MLP env map strength",
            ],
            &[
                *inner_thickness,
                *refraction_scale,
                inner_texture_scale[0],
                inner_texture_scale[1],
                *envmap_strength,
            ],
        ),
        LightingTrailer::SparkleSnow(s) => {
            named(&["Sparkle R", "Sparkle G", "Sparkle B", "Sparkle A"], s)
        }
        LightingTrailer::Eye {
            cubemap_scale,
            left_center,
            right_center,
        } => named(
            &[
                "Eye cubemap scale",
                "Left eye X",
                "Left eye Y",
                "Left eye Z",
                "Right eye X",
                "Right eye Y",
                "Right eye Z",
            ],
            &[
                *cubemap_scale,
                left_center[0],
                left_center[1],
                left_center[2],
                right_center[0],
                right_center[1],
                right_center[2],
            ],
        ),
    }
}

/// A shape whose extra data has `NiBooleanExtraData` "PG_IGNORE": PGPatcher leaves it alone.
fn pg_ignore(m: &NifModel, s: &ShapeModel) -> bool {
    m.extras_of(&s.net)
        .iter()
        .any(|x| x.kind == "NiBooleanExtraData" && x.name.as_deref() == Some(b"PG_IGNORE"))
}

/// The selected shape, or a line saying why there is none.
fn selected<'a>(ui: &mut egui::Ui, e: &'a NifEdit, what: &str) -> Option<&'a ShapeModel> {
    let Selection::Shape(b) = e.selection else {
        ui.label(format!("Select a shape card to edit its {what}."));
        return None;
    };
    let s = e.model.shape(b);
    if s.is_none() {
        ui.label("The selected shape is not in this NIF.");
    }
    s
}

pub(super) fn shader_tab(ui: &mut egui::Ui, m: &MeshesModel, e: &NifEdit, act: &mut Act) {
    let Some(shape) = selected(ui, e, "shader") else {
        return;
    };
    if pg_ignore(&e.model, shape) {
        ui.colored_label(AMBER, "PGPatcher skips this shape (PG_IGNORE).");
    }
    let lit = shape
        .shader
        .and_then(|s| e.model.lighting.get(&s))
        .zip(anchor_role(&e.model, Some(shape.block), Role::Shader));
    let Some((l, at)) = lit else {
        ui.label("This shape has no BSLightingShaderProperty.");
        return;
    };
    let users = l.prop.users.len();
    if users > 1 {
        ui.add(
            egui::Label::new(
                egui::RichText::new(format!(
                    "Shared by {users} shapes: shader edits change all of them."
                ))
                .color(AMBER),
            )
            .wrap(),
        );
    }
    let (lt, p) = (&l.lighting, &l.lighting.params);
    let pbr = lt.slsf2 & SLSF2_TRUE_PBR != 0;

    ui.horizontal(|ui| {
        row_label(ui, "Shader type");
        let mut t = lt.shader_type;
        let shown = shader_type_name(t);
        let r = egui::ComboBox::from_id_salt("meshes-edit-shader-type")
            .width(200.0)
            .selected_text(&shown)
            .show_ui(ui, |ui| {
                for (i, n) in SHADER_TYPES.iter().enumerate() {
                    ui.selectable_value(&mut t, i as u32, format!("{n} ({i})"));
                }
            })
            .response;
        let typ = egui::WidgetType::ComboBox;
        set_name(&r, typ, "Shader type", None, Some(shown));
        r.on_hover_text(TYPE_HOVER);
        if t != lt.shader_type {
            act.push = Some((
                vec![Op::SetField {
                    at: at.clone(),
                    field: Field::ShaderType,
                    value: Value::U32(t),
                    base: Value::U32(lt.shader_type),
                }],
                None,
            ));
        }
    });
    warnings(
        ui,
        e,
        &Target::Field {
            block: at.block,
            field: Field::ShaderType,
        },
        act,
    );

    let f = |ui: &mut egui::Ui, act: &mut Act, name: &str, field, v| {
        float(ui, e, act, &at, name, field, v)
    };
    f(ui, act, "UV offset U", Field::UvOffset(0), p.uv_offset[0]);
    f(ui, act, "UV offset V", Field::UvOffset(1), p.uv_offset[1]);
    f(ui, act, "UV scale U", Field::UvScale(0), p.uv_scale[0]);
    f(ui, act, "UV scale V", Field::UvScale(1), p.uv_scale[1]);
    color(
        ui,
        e,
        act,
        &at,
        "Emissive colour",
        Field::EmissiveColor,
        p.emissive_color,
    );
    f(
        ui,
        act,
        "Emissive multiple",
        Field::EmissiveMultiple,
        p.emissive_multiple,
    );
    ui.horizontal(|ui| {
        row_label(ui, "Clamp mode");
        let mut c = p.texture_clamp_mode;
        let r = ui.add(egui::DragValue::new(&mut c));
        set_name(
            &r,
            egui::WidgetType::DragValue,
            "Clamp mode",
            Some(c as f64),
            None,
        );
        if c != p.texture_clamp_mode {
            let op = Op::SetField {
                at: at.clone(),
                field: Field::ClampMode,
                value: Value::U32(c),
                base: Value::U32(p.texture_clamp_mode),
            };
            let key = merge_key(&op);
            act.push = Some((vec![op], Some(key)));
        }
        act.flush |= r.drag_stopped() || r.lost_focus();
    });
    warnings(
        ui,
        e,
        &Target::Field {
            block: at.block,
            field: Field::ClampMode,
        },
        act,
    );
    f(ui, act, "Shader alpha", Field::Alpha, p.alpha);
    f(
        ui,
        act,
        "Refraction strength",
        Field::Refraction,
        p.refraction_strength,
    );
    let gloss = if pbr { "Specular level" } else { "Glossiness" };
    f(ui, act, gloss, Field::Glossiness, p.glossiness);
    color(
        ui,
        e,
        act,
        &at,
        "Specular colour",
        Field::SpecularColor,
        p.specular_color,
    );
    let strength = if pbr {
        "Roughness scale"
    } else {
        "Specular strength"
    };
    f(
        ui,
        act,
        strength,
        Field::SpecularStrength,
        p.specular_strength,
    );
    f(
        ui,
        act,
        "Lighting effect 1",
        Field::LightingEffect1,
        p.lighting_effect_1,
    );
    f(
        ui,
        act,
        "Lighting effect 2",
        Field::LightingEffect2,
        p.lighting_effect_2,
    );
    for (n, (name, v)) in trailer_fields(&p.trailer).into_iter().enumerate() {
        f(ui, act, &name, Field::Trailer(n as u8), v);
    }

    texture_slots(ui, m, e, shape, l, act);
    ui.separator();
    for (label, field, v, names) in [
        ("SLSF1", Field::Slsf1, lt.slsf1, &SLSF1_NAMES),
        ("SLSF2", Field::Slsf2, lt.slsf2, &SLSF2_NAMES),
    ] {
        egui::CollapsingHeader::new(format!("{label} 0x{v:08X}"))
            .id_salt(("meshes-edit-flags", label))
            .default_open(true)
            .show(ui, |ui| {
                warnings(
                    ui,
                    e,
                    &Target::Field {
                        block: at.block,
                        field,
                    },
                    act,
                );
                for (bit, name) in names.iter().enumerate() {
                    let mask = 1u32 << bit;
                    let mut on = v & mask != 0;
                    let locked = match field {
                        Field::Slsf1 if bit == 1 => Some(SKINNED_HOVER),
                        Field::Slsf2 if bit == 23 => Some(PBR_HOVER),
                        _ => None,
                    };
                    let r = ui.add_enabled(locked.is_none(), egui::Checkbox::new(&mut on, *name));
                    if let Some(h) = locked {
                        r.on_disabled_hover_text(h);
                    } else if r.changed() {
                        act.push = Some((
                            vec![Op::SetBits {
                                at: at.clone(),
                                field,
                                mask,
                                value: if on { mask } else { 0 },
                                base: v,
                            }],
                            None,
                        ));
                    }
                }
            });
    }
}

fn texture_slots(
    ui: &mut egui::Ui,
    m: &MeshesModel,
    e: &NifEdit,
    shape: &ShapeModel,
    l: &nif::model::LightingModel,
    act: &mut Act,
) {
    let Some((set, ts)) = l
        .texture_set
        .and_then(|t| e.model.texsets.get(&t).map(|s| (t, s)))
    else {
        return;
    };
    ui.separator();
    ui.strong("Texture slots");
    let users = e
        .model
        .shapes
        .iter()
        .filter(|s| {
            s.shader
                .and_then(|x| e.model.lighting.get(&x))
                .and_then(|l| l.texture_set)
                == Some(set)
        })
        .count();
    // A shared shader refuses path edits (P5): its own notice says so, so no "copy" promise here.
    if users > 1 && l.prop.users.len() <= 1 {
        ui.add(
            egui::Label::new(
                egui::RichText::new(format!(
                    "Shared by {users} shapes: editing makes a copy for this shape."
                ))
                .color(AMBER),
            )
            .wrap(),
        );
    }
    let mut check = e.check_exists;
    if ui.checkbox(&mut check, "Check that files exist").changed() {
        act.check = Some(check);
    }
    let lt = &l.lighting;
    let anchor = anchor_shape(&e.model, shape.block);
    for (i, raw) in ts.raw.slots.iter().enumerate() {
        let name = edit_slot_label(lt.shader_type, lt.slsf1, lt.slsf2, i);
        let text = nif::text::lossy(raw);
        let norm = nif::path::canon_texture(raw).norm;
        let mut commit = None;
        ui.horizontal(|ui| {
            ui.strong(name);
            for w in e.warnings.iter().filter(|w| {
                w.target
                    == Target::Slot {
                        set,
                        slot: i as u32,
                    }
            }) {
                let color = if w.level == Level::Error { RED } else { AMBER };
                ui.label(egui::RichText::new(&w.text).small().color(color))
                    .on_hover_text(&w.text);
                if let Some(fix) = &w.fix {
                    if ui.small_button("Fix").clicked() {
                        act.push = Some((vec![fix.clone()], None));
                    }
                }
            }
        });
        let row = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
        ui.allocate_ui_with_layout(
            row,
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                let mut open = None;
                super::open_button(ui, m, &norm, &mut open);
                if open.is_some() {
                    act.open = open;
                }
                let browse =
                    super::super::path_picker::menu(ui, "Browse", |ui| browse(ui, m, &mut commit));
                let bname = format!("Browse for {name} (slot {i})");
                browse.response.widget_info(move || {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, bname.clone())
                });
                path_field(
                    ui,
                    set,
                    i,
                    &format!("{name} path (slot {i})"),
                    &text,
                    &mut commit,
                );
            },
        );
        if let Some(path) = commit.filter(|p| *p != text) {
            act.push = Some((
                vec![Op::SetTexturePath {
                    at: anchor.clone(),
                    slot: i as u32,
                    path,
                    base: text.clone(),
                }],
                None,
            ));
        }
    }
}

/// The raw path, edited in a buffer and committed on Enter or focus loss (Esc drops the buffer).
fn path_field(
    ui: &mut egui::Ui,
    set: u32,
    slot: usize,
    name: &str,
    text: &str,
    commit: &mut Option<String>,
) {
    let id = egui::Id::new(("meshes-edit-slot", set, slot));
    let mut buf = ui
        .data(|d| d.get_temp::<String>(id))
        .unwrap_or_else(|| text.to_string());
    let r = ui.add(
        egui::TextEdit::singleline(&mut buf)
            .id(id.with("field"))
            .desired_width(ui.available_width()),
    );
    set_name(
        &r,
        egui::WidgetType::TextEdit,
        name,
        None,
        Some(buf.clone()),
    );
    if r.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(id));
        if !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            *commit = Some(buf);
        }
    } else if r.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, buf));
    } else {
        ui.data_mut(|d| d.remove::<String>(id));
    }
}

/// The Browse popup: a filter and the scanned winning textures that match it; a click picks `textures\…`.
fn browse(ui: &mut egui::Ui, m: &MeshesModel, commit: &mut Option<String>) {
    let fid = egui::Id::new("meshes-edit-browse-filter");
    let cid = egui::Id::new("meshes-edit-browse-list");
    let mut filter: String = ui.data(|d| d.get_temp(fid)).unwrap_or_default();
    ui.add(
        egui::TextEdit::singleline(&mut filter)
            .hint_text("Filter textures")
            .desired_width(320.0),
    );
    // One query per filter, not per frame.
    let cached: Option<(String, Vec<String>)> = ui.data(|d| d.get_temp(cid));
    let list = match cached {
        Some((f, l)) if f == filter => l,
        _ => {
            let l = m.texture_paths(&filter, BROWSE_LIMIT);
            ui.data_mut(|d| d.insert_temp(cid, (filter.clone(), l.clone())));
            l
        }
    };
    ui.data_mut(|d| d.insert_temp(fid, filter));
    if list.is_empty() {
        ui.weak("No scanned texture matches.");
    }
    egui::ScrollArea::vertical()
        .id_salt("meshes-edit-browse-scroll")
        .max_height(260.0)
        .show(ui, |ui| {
            for p in &list {
                if ui.selectable_label(false, p.as_str()).clicked() {
                    let p = p.replace('/', "\\");
                    *commit = Some(if p.to_ascii_lowercase().starts_with("textures\\") {
                        p
                    } else {
                        format!("textures\\{p}")
                    });
                    ui.close();
                }
            }
        });
}

/// The display name of blend code `c` (11-15 read as ONE in the engine).
fn blend_name(c: u8) -> String {
    BLEND_MODES
        .get(c as usize)
        .map_or("One (invalid)".to_string(), |n| n.to_string())
}

pub(super) fn alpha_tab(ui: &mut egui::Ui, e: &NifEdit, act: &mut Act) {
    let Some(shape) = selected(ui, e, "alpha") else {
        return;
    };
    let on_shape = anchor_shape(&e.model, shape.block);
    let Some(a) = shape.alpha.and_then(|b| e.model.alphas.get(&b)) else {
        ui.label("This shape has no NiAlphaProperty.");
        if ui.button("Add alpha").clicked() {
            act.push = Some((
                vec![Op::AddAlpha {
                    at: on_shape,
                    flags: DEFAULT_FLAGS,
                    threshold: DEFAULT_THRESH,
                }],
                None,
            ));
        }
        return;
    };
    let users = a.prop.users.len();
    let shared = users > 1;
    if shared {
        ui.add(
            egui::Label::new(
                egui::RichText::new(format!("Shared by {users} shapes: editing will split."))
                    .color(AMBER),
            )
            .wrap(),
        );
    }
    // `nif::edit` refuses that split, and the edit after it would re-anchor on the shared block (every user).
    let blocked = shared && a.prop.ctrls.iter().any(|&c| e.model.managed(c));
    if blocked {
        ui.add(
            egui::Label::new(
                egui::RichText::new(
                    "Shared alpha with a clip controller: splitting needs Phase C.",
                )
                .color(RED),
            )
            .wrap(),
        );
    }
    // Built only when an edit is pushed. After a split this shape's alpha is the block the split appends: the end
    // of the file.
    let at = || {
        let own = Anchor {
            block: a.prop.block,
            kind: "NiAlphaProperty".into(),
            shape: shape.name.clone(),
            role: Role::Alpha,
        };
        match shared {
            true => Anchor {
                block: nif::header::Header::parse(&e.patched).map_or(0, |h| h.sizes.len() as u32),
                ..own
            },
            false => anchor_role(&e.model, Some(shape.block), Role::Alpha).unwrap_or(own),
        }
    };
    let edit = |act: &mut Act, op: Op, merge: Option<String>| {
        let ops = match shared {
            true => vec![
                Op::SplitAlpha {
                    at: on_shape.clone(),
                },
                op,
            ],
            false => vec![op],
        };
        act.push = Some((ops, merge));
    };
    let flags = a.alpha.flags;
    let fl = AlphaFlags::from_raw(flags);
    let bits = |mask: u32, value: u32| Op::SetBits {
        at: at(),
        field: Field::AlphaFlags,
        mask,
        value,
        base: flags as u32,
    };
    let check = |ui: &mut egui::Ui, act: &mut Act, name: &str, bit: u32| {
        let mut on = flags as u32 & (1 << bit) != 0;
        if ui.checkbox(&mut on, name).changed() {
            edit(act, bits(1 << bit, if on { 1 << bit } else { 0 }), None);
        }
    };
    let combo = |ui: &mut egui::Ui,
                 act: &mut Act,
                 name: &str,
                 shift: u32,
                 width: u32,
                 cur: u8,
                 n: u8,
                 label: &dyn Fn(u8) -> String| {
        ui.horizontal(|ui| {
            row_label(ui, name);
            let mut v = cur;
            let r = egui::ComboBox::from_id_salt(("meshes-edit-alpha", name))
                .width(180.0)
                .selected_text(label(cur))
                .show_ui(ui, |ui| {
                    for c in 0..n {
                        ui.selectable_value(&mut v, c, format!("{} ({c})", label(c)));
                    }
                })
                .response;
            set_name(&r, egui::WidgetType::ComboBox, name, None, Some(label(cur)));
            if v != cur {
                let mask = ((1u32 << width) - 1) << shift;
                edit(act, bits(mask, (v as u32) << shift), None);
            }
        });
    };

    ui.add_enabled_ui(!blocked, |ui| {
        check(ui, act, "Blend", 0);
        combo(ui, act, "Source", 1, 4, fl.src, 16, &blend_name);
        combo(ui, act, "Destination", 5, 4, fl.dst, 16, &blend_name);
        if fl.blend {
            let ((_, _), mapped) = engine_blend(fl.src, fl.dst);
            if mapped {
                ui.weak("Shown as the engine draws it");
            }
            if !verified_blend(fl.src, fl.dst) {
                ui.weak("Not verified in game");
            }
        }
        check(ui, act, "Test", 9);
        combo(ui, act, "Function", 10, 3, fl.func, 8, &|c| {
            TEST_FUNCS[c as usize].to_string()
        });
        ui.weak("Shown as CS evaluates it (alpha ≥ threshold)");
        ui.horizontal(|ui| {
            row_label(ui, "Threshold");
            let mut t = a.alpha.threshold;
            let r = ui.add(egui::Slider::new(&mut t, 0..=255));
            set_name(
                &r,
                egui::WidgetType::Slider,
                "Threshold",
                Some(t as f64),
                None,
            );
            if t != a.alpha.threshold {
                let op = Op::SetField {
                    at: at(),
                    field: Field::AlphaThreshold,
                    value: Value::U8(t),
                    base: Value::U8(a.alpha.threshold),
                };
                let key = merge_key(&op);
                edit(act, op, Some(key));
            }
            act.flush |= r.drag_stopped() || r.lost_focus();
        });
        check(ui, act, "No sorter", 13);
        check(ui, act, "Clone unique", 14);
        check(ui, act, "Editor threshold", 15);
    });
    ui.label(format!("Flags 0x{flags:04X} ({flags})"));
    let w = Target::Field {
        block: a.prop.block,
        field: Field::AlphaFlags,
    };
    warnings(ui, e, &w, act);
    if ui.button("Remove alpha").clicked() {
        act.push = Some((vec![Op::RemoveAlpha { at: on_shape }], None));
    }
}
