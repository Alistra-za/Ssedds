//! The edit panel's Extra data tab (spec §7, plan A18): the selected shape's extra data and the root's. Editable
//! types are changed in place, added and removed; every other type is a read-only row.
use super::edit_fields::{row_label, set_name, Act};
use crate::model::meshes::nif_edit::{NifEdit, Selection};
use eframe::egui;
use nif::edit::{anchor_role, anchor_shape, merge_key, Anchor, Op, Role};
use nif::extra::{Extra, ExtraValue, BSX_NAMES, EDITABLE, INV_DEFAULT};
use nif::model::NifModel;

const ANIMATED_HOVER: &str = "Set because a controller in this NIF animates a property.";
const PREFILL: [(&str, &str); 2] = [("BSXFlags", "BSX"), ("BSInvMarker", "INV")];

/// True while any property has a controller no clip drives (D9): the game needs BSX bit 0 then.
fn free_controller(m: &NifModel) -> bool {
    let chains = m
        .lighting
        .values()
        .map(|l| &l.prop.ctrls)
        .chain(m.alphas.values().map(|a| &a.prop.ctrls));
    chains.flatten().any(|&c| !m.managed(c))
}

pub(super) fn extra_tab(ui: &mut egui::Ui, e: &NifEdit, act: &mut Act) {
    let animated = free_controller(&e.model);
    ui.strong("Shape");
    match e.selection {
        Selection::Shape(b) => match e.model.shape(b) {
            Some(s) => {
                let net = e.model.extras_of(&s.net);
                let at = anchor_shape(&e.model, b);
                list(ui, act, false, (net, Some(at)), s.name.clone(), animated);
            }
            None => {
                ui.label("The selected shape is not in this NIF.");
            }
        },
        Selection::Root => {
            ui.label("Select a shape card to see its extra data.");
        }
    }
    ui.separator();
    ui.strong("Root");
    match &e.model.root_net {
        Some(net) => {
            let at = anchor_role(&e.model, None, Role::Root);
            list(ui, act, true, (e.model.extras_of(net), at), None, animated);
        }
        None => {
            ui.label("The root is not a node: it has no extra data.");
        }
    }
}

/// One list. `add_at` is the anchor an added block hangs on; `shape` the owning shape name (None for the root).
fn list(
    ui: &mut egui::Ui,
    act: &mut Act,
    root: bool,
    (extras, add_at): (Vec<&Extra>, Option<Anchor>),
    shape: Option<String>,
    animated: bool,
) {
    if extras.is_empty() {
        ui.weak("No extra data.");
    }
    for (i, x) in extras.iter().enumerate() {
        let name = nif::text::lossy(x.name.as_deref().unwrap_or(&[]));
        let nth = extras[..i]
            .iter()
            .filter(|p| p.kind == x.kind && p.name == x.name)
            .count() as u32;
        let at = Anchor {
            block: x.block,
            kind: x.kind.clone(),
            shape: shape.clone(),
            role: Role::Extra {
                on_root: root,
                name: name.clone(),
                kind: x.kind.clone(),
                nth,
            },
        };
        ui.push_id((x.block, root), |ui| {
            row(ui, act, x, &name, &at, root, animated)
        });
    }
    if let Some(at) = add_at {
        add_row(ui, act, root, at);
    }
}

/// The accessible name of a control: the root list's plain, the shape list's marked.
fn scoped(root: bool, name: &str) -> String {
    match root {
        true => name.to_string(),
        false => format!("{name} (shape)"),
    }
}

fn row(
    ui: &mut egui::Ui,
    act: &mut Act,
    x: &Extra,
    name: &str,
    at: &Anchor,
    root: bool,
    animated: bool,
) {
    let editable = x.value.is_some();
    egui::Sides::new().shrink_left().truncate().show(
        ui,
        |ui| {
            ui.add(egui::Label::new(egui::RichText::new(name).strong()).truncate());
            ui.weak(&x.kind);
        },
        |ui| {
            if editable {
                let r = ui.small_button("Remove");
                set_name(
                    &r,
                    egui::WidgetType::Button,
                    &scoped(root, &format!("Remove {name}")),
                    None,
                    None,
                );
                if r.clicked() {
                    act.push = Some((vec![Op::RemoveExtra { at: at.clone() }], None));
                }
            }
        },
    );
    let Some(v) = &x.value else {
        ui.weak(format!("{} · {} bytes", x.kind, x.size));
        return;
    };
    let vname = scoped(root, &format!("{name} value"));
    let edit = |act: &mut Act, new: ExtraValue, drag: bool| {
        let op = Op::EditExtra {
            at: at.clone(),
            value: new,
            base: v.clone(),
        };
        let key = drag.then(|| merge_key(&op));
        act.push = Some((vec![op], key));
    };
    match v {
        ExtraValue::Bsx(bits) => bsx(ui, act, *bits, x.block, root, animated, |a, n| {
            edit(a, n, false)
        }),
        ExtraValue::Str(s) => {
            let mut buf = s.clone();
            let r = ui.add(
                egui::TextEdit::multiline(&mut buf)
                    .desired_rows(2)
                    .desired_width(ui.available_width()),
            );
            set_name(
                &r,
                egui::WidgetType::TextEdit,
                &vname,
                None,
                Some(buf.clone()),
            );
            if r.changed() && buf != *s {
                edit(act, ExtraValue::Str(buf), true);
            }
            act.flush |= r.lost_focus();
        }
        ExtraValue::Int(n) => {
            let mut x = *n;
            let r = ui.add(egui::DragValue::new(&mut x));
            set_name(
                &r,
                egui::WidgetType::DragValue,
                &vname,
                Some(x as f64),
                None,
            );
            if r.changed() && x != *n {
                edit(act, ExtraValue::Int(x), true);
            }
            act.flush |= r.drag_stopped() || r.lost_focus();
        }
        ExtraValue::Bool(b) => {
            let mut on = *b != 0;
            let r = ui.add(egui::Checkbox::without_text(&mut on));
            let (n, enabled, sel) = (vname.clone(), r.enabled(), on);
            r.widget_info(move || {
                egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, sel, n.clone())
            });
            if r.changed() {
                edit(act, ExtraValue::Bool(on as u8), false);
            }
        }
        ExtraValue::Float(bits) => {
            let mut f = f32::from_bits(*bits);
            let r = ui.add(egui::DragValue::new(&mut f).speed(0.01));
            set_name(
                &r,
                egui::WidgetType::DragValue,
                &vname,
                Some(f as f64),
                None,
            );
            if r.changed() && f.to_bits() != *bits {
                edit(act, ExtraValue::Float(f.to_bits()), true);
            }
            act.flush |= r.drag_stopped() || r.lost_focus();
        }
        ExtraValue::Inv { rot, zoom } => inv(ui, act, rot, *zoom, root, |a, n| edit(a, n, true)),
    }
}

/// The named BSXFlags bits. Bit 0 stays ticked and locked while a free controller animates the model (D9).
fn bsx(
    ui: &mut egui::Ui,
    act: &mut Act,
    bits: u32,
    block: u32,
    root: bool,
    animated: bool,
    edit: impl Fn(&mut Act, ExtraValue),
) {
    egui::CollapsingHeader::new(format!("Flags 0x{bits:X}"))
        .id_salt(("meshes-edit-bsx", block, root))
        .default_open(true)
        .show(ui, |ui| {
            for (i, name) in BSX_NAMES.iter().enumerate() {
                let mask = 1u32 << i;
                let locked = i == 0 && animated;
                let mut on = locked || bits & mask != 0;
                let r = ui.add_enabled(!locked, egui::Checkbox::new(&mut on, *name));
                let (n, enabled, sel) = (scoped(root, name), r.enabled(), on);
                r.widget_info(move || {
                    egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, sel, n.clone())
                });
                if locked {
                    r.on_disabled_hover_text(ANIMATED_HOVER);
                } else if r.changed() {
                    edit(act, ExtraValue::Bsx(bits ^ mask));
                }
            }
        });
}

/// BSInvMarker: rotation X/Y/Z in degrees (the file holds radians x 1000 as u16) and the zoom.
fn inv(
    ui: &mut egui::Ui,
    act: &mut Act,
    rot: &[u16; 3],
    zoom: u32,
    root: bool,
    edit: impl Fn(&mut Act, ExtraValue),
) {
    let mut new = *rot;
    let mut z = f32::from_bits(zoom);
    let (mut zoom_moved, mut done) = (false, false);
    for (i, axis) in ["X", "Y", "Z"].into_iter().enumerate() {
        ui.horizontal(|ui| {
            row_label(ui, &format!("Rotation {axis}"));
            let mut deg = rot[i] as f64 / 1000.0 * 180.0 / std::f64::consts::PI;
            let r = ui.add(egui::DragValue::new(&mut deg).speed(1.0).suffix("°"));
            let n = scoped(root, &format!("Rotation {axis}"));
            set_name(&r, egui::WidgetType::DragValue, &n, Some(deg), None);
            if r.changed() {
                // shortcut: saturates outside 0-65535 (about 3754 degrees); no wrap-around.
                new[i] = (deg.to_radians() * 1000.0).round().clamp(0.0, 65535.0) as u16;
            }
            done |= r.drag_stopped() || r.lost_focus();
        });
    }
    ui.horizontal(|ui| {
        row_label(ui, "Zoom");
        let r = ui.add(egui::DragValue::new(&mut z).speed(0.01));
        set_name(
            &r,
            egui::WidgetType::DragValue,
            &scoped(root, "Zoom"),
            Some(z as f64),
            None,
        );
        zoom_moved = r.changed() && z.to_bits() != zoom;
        done |= r.drag_stopped() || r.lost_focus();
    });
    // Only a real change pushes: a drag that rounds back to the same u16 is not an edit.
    if new != *rot || zoom_moved {
        edit(
            act,
            ExtraValue::Inv {
                rot: new,
                zoom: z.to_bits(),
            },
        );
    }
    act.flush |= done;
}

fn default_value(kind: &str) -> ExtraValue {
    match kind {
        "BSXFlags" => ExtraValue::Bsx(0),
        "NiStringExtraData" => ExtraValue::Str(String::new()),
        "NiIntegerExtraData" => ExtraValue::Int(0),
        "NiBooleanExtraData" => ExtraValue::Bool(0),
        "NiFloatExtraData" => ExtraValue::Float(0.0f32.to_bits()),
        _ => ExtraValue::Inv {
            rot: INV_DEFAULT.0,
            zoom: INV_DEFAULT.1.to_bits(),
        },
    }
}

fn prefill(kind: &str) -> &'static str {
    PREFILL.iter().find(|p| p.0 == kind).map_or("", |p| p.1)
}

/// The type picker, a name and Add. The pick and the name live in egui memory until Add.
fn add_row(ui: &mut egui::Ui, act: &mut Act, root: bool, at: Anchor) {
    let id = egui::Id::new(("meshes-edit-extra-add", root, at.block));
    let (mut kind, mut name): (usize, String) = ui
        .data(|d| d.get_temp(id))
        .unwrap_or_else(|| (0, prefill(EDITABLE[0]).to_string()));
    ui.horizontal(|ui| {
        ui.label("Add");
        let before = kind;
        let r = egui::ComboBox::from_id_salt(id.with("type"))
            .width(170.0)
            .selected_text(EDITABLE[kind])
            .show_ui(ui, |ui| {
                for (i, k) in EDITABLE.iter().enumerate() {
                    ui.selectable_value(&mut kind, i, *k);
                }
            })
            .response;
        let n = scoped(root, "Add type");
        set_name(
            &r,
            egui::WidgetType::ComboBox,
            &n,
            None,
            Some(EDITABLE[kind].into()),
        );
        // A name the picker filled in follows the type; one the user typed stays.
        if kind != before && (name.is_empty() || name == prefill(EDITABLE[before])) {
            name = prefill(EDITABLE[kind]).to_string();
        }
    });
    ui.horizontal(|ui| {
        let r = ui.add(
            egui::TextEdit::singleline(&mut name)
                .hint_text("Name")
                .desired_width(150.0),
        );
        set_name(
            &r,
            egui::WidgetType::TextEdit,
            &scoped(root, "Name"),
            None,
            Some(name.clone()),
        );
        let r = ui.add_enabled(!name.is_empty(), egui::Button::new("Add"));
        let label = if root { "Add to root" } else { "Add to shape" };
        set_name(&r, egui::WidgetType::Button, label, None, None);
        if r.clicked() {
            let k = EDITABLE[kind];
            act.push = Some((
                vec![Op::AddExtra {
                    at,
                    kind: k.into(),
                    name: std::mem::take(&mut name),
                    value: default_value(k),
                }],
                None,
            ));
            name = prefill(k).to_string();
        }
    });
    ui.data_mut(|d| d.insert_temp(id, (kind, name)));
}
