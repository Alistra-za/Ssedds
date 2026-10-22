//! The edit panel's Extra data tab (spec §7, plan A18): the selected shape's extra data and the root's. Editable
//! types are changed in place, added and removed; every other type is a read-only row.
use super::edit_fields::{row_label, set_name, warnings, Act};
use crate::model::meshes::nif_edit::{heel_z_of, sdta_npc_z, sdta_with_npc_z, NifEdit, Selection};
use crate::ui::status::AMBER;
use eframe::egui;
use nif::edit::validate::{Target, HEEL_EARLIER_WINS, HEEL_NOT_ON_SHAPE};
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
                list(ui, e, act, false, (net, Some(at)), s.name.clone(), animated);
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
            list(
                ui,
                e,
                act,
                true,
                (e.model.extras_of(net), at),
                None,
                animated,
            );
        }
        None => {
            ui.label("The root is not a node: it has no extra data.");
        }
    }
}

/// One list. `add_at` is the anchor an added block hangs on; `shape` the owning shape name (None for the root).
fn list(
    ui: &mut egui::Ui,
    e: &NifEdit,
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
        let at = extra_anchor(&extras, i, shape.clone(), root);
        ui.push_id((x.block, root), |ui| {
            row(ui, act, x, &name, &at, root, animated);
            // Meshes UI T5 (bug 3): the row's own warnings, such as "BSXFlags must be named BSX".
            warnings(ui, e, &Target::Extra { block: x.block }, act);
        });
    }
    if let Some(at) = add_at {
        add_row(ui, e, act, root, at, &extras);
    }
}

/// The anchor of `extras[i]`: its block, kind and name, and how many earlier entries share both (`nth`).
fn extra_anchor(extras: &[&Extra], i: usize, shape: Option<String>, root: bool) -> Anchor {
    let x = extras[i];
    let nth = extras[..i]
        .iter()
        .filter(|p| p.kind == x.kind && p.name == x.name)
        .count() as u32;
    Anchor {
        block: x.block,
        kind: x.kind.clone(),
        shape,
        role: Role::Extra {
            on_root: root,
            name: nif::text::lossy(x.name.as_deref().unwrap_or(&[])),
            kind: x.kind.clone(),
            nth,
        },
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
            let r = ui.add(egui::Label::new(egui::RichText::new(name).strong()).truncate());
            if let Some(tip) = known_tip(x) {
                r.on_hover_text(tip);
            }
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

/// One entry of the Known picker (Meshes UI T5, Note CZ17). `root`: it goes on the root, else on a shape.
struct Known {
    name: &'static str,
    kind: &'static str,
    root: bool,
    value: KnownValue,
    unconfirmed: bool,
    tip: &'static str,
}

/// A Known entry's value: a fixed default, or a string typed in the Value field (trimmed on Add). `Text(Some(why))`:
/// Add waits for a value and says `why` while it is off.
#[derive(Clone, Copy)]
enum KnownValue {
    Fixed(fn() -> ExtraValue),
    Text(Option<&'static str>),
    /// Meshes UI T5b: heel height, set through its own row (`heel_row`).
    Heel,
}

/// The confirmed (and owner-approved unconfirmed) entries; sources in Note CZ17. Everything else is Custom…. BODYTRI is
/// left out on purpose: BodySlide / Outfit Studio writes it (owner, 2026-10-22).
const KNOWN: [Known; 6] = [
    Known {
        name: "AnisotropicAlphaMaterial",
        kind: "NiIntegerExtraData",
        root: false,
        value: KnownValue::Fixed(|| ExtraValue::Int(3)),
        unconfirmed: false,
        tip: "Community Shaders see-through look for this shape. 0 off, 1 rim edge, 2 fabric, 3 woven fabric. \
              Only on alpha-blended lit shapes.",
    },
    Known {
        name: "BSX",
        kind: "BSXFlags",
        root: true,
        value: KnownValue::Fixed(|| ExtraValue::Bsx(0)),
        unconfirmed: false,
        tip: "Engine flags for the whole model: animation, collision, ragdoll, external emit. Must be named BSX.",
    },
    Known {
        name: "INV",
        kind: "BSInvMarker",
        root: true,
        value: KnownValue::Fixed(|| ExtraValue::Inv {
            rot: INV_DEFAULT.0,
            zoom: INV_DEFAULT.1.to_bits(),
        }),
        unconfirmed: false,
        tip: "How the item turns and zooms in the inventory view. Leave the default if unsure.",
    },
    Known {
        name: "HDT Skinned Mesh Physics Object",
        kind: "NiStringExtraData",
        root: true,
        value: KnownValue::Text(Some("Type the path first.")),
        unconfirmed: false,
        tip: "Gives this model SMP physics. The value is the path to the physics xml. Needs HDT-SMP installed. \
              The name must be exact.",
    },
    Known {
        name: "Prn",
        kind: "NiStringExtraData",
        root: true,
        value: KnownValue::Text(Some("Type the node name first.")),
        unconfirmed: true,
        tip: "Unconfirmed: no source checked yet. Names the skeleton node this item attaches to, for example \
              WeaponBack or SHIELD.",
    },
    Known {
        name: "HH_OFFSET",
        kind: "NiFloatExtraData",
        root: false,
        value: KnownValue::Heel,
        unconfirmed: false,
        tip: HEEL_TIP,
    },
];

/// T5b-heel-height.md "Tooltip".
const HEEL_TIP: &str =
    "Raises the actor so heels stand on the ground. RaceMenu reads it. Calculate measures how far \
                        the heel goes below the foot.";

fn known_label(k: &Known) -> String {
    match (k.value, k.unconfirmed) {
        (KnownValue::Heel, _) => "Heel height (RaceMenu)".to_string(),
        (_, true) => format!("{} (unconfirmed)", k.name),
        (_, false) => k.name.to_string(),
    }
}

/// Why `k` cannot be added here, if it cannot: the other owner's entry, or one this owner already has.
fn known_off(k: &Known, root: bool, have: &[&Extra]) -> Option<&'static str> {
    if let KnownValue::Heel = k.value {
        // RaceMenu reads heel data only on a shape; a shape's own is edited in place, never added twice.
        return root.then_some(HEEL_NOT_ON_SHAPE);
    }
    if k.root != root {
        return Some(if k.root {
            "Goes on the root"
        } else {
            "Goes on a shape"
        });
    }
    let present = have.iter().any(|x| {
        x.kind == k.kind
            && x.name
                .as_deref()
                .is_some_and(|n| n.eq_ignore_ascii_case(k.name.as_bytes()))
    });
    present.then_some(if root {
        "Already on this root"
    } else {
        "Already on this shape"
    })
}

/// The tooltip of an existing row whose kind and name are a Known entry's.
fn known_tip(x: &Extra) -> Option<&'static str> {
    KNOWN
        .iter()
        .find(|k| {
            x.kind == k.kind
                && x.name
                    .as_deref()
                    .is_some_and(|n| n.eq_ignore_ascii_case(k.name.as_bytes()))
        })
        .map(|k| k.tip)
}

/// What the Add row holds between frames (egui memory, until Add): the Known pick (None = Custom…), the Custom
/// type and name, and a Known entry's typed value.
#[derive(Clone, Default)]
struct AddState {
    known: Option<usize>,
    kind: usize,
    name: String,
    text: String,
    /// The heel row's Z while it differs from the shape's stored one, and its "Also write SDTA" tick.
    z: Option<f32>,
    sdta: bool,
    /// The last Calculate found no geometry to measure.
    calc_none: bool,
}

/// The Known picker above the type picker, a name and Add. A Known entry fills type, name and value; Custom… keeps
/// free typing.
fn add_row(ui: &mut egui::Ui, e: &NifEdit, act: &mut Act, root: bool, at: Anchor, have: &[&Extra]) {
    // T5b review Important 1: keyed by the session too, so a pick or a typed value never reaches the same block of
    // another NIF or session.
    let id = egui::Id::new(("meshes-edit-extra-add", e.session, root, at.block));
    let mut st: AddState = ui.data(|d| d.get_temp(id)).unwrap_or_else(|| AddState {
        name: prefill(EDITABLE[0]).to_string(),
        ..Default::default()
    });
    ui.horizontal(|ui| {
        row_label(ui, "Known");
        let shown = st
            .known
            .map_or("Custom…".to_string(), |i| known_label(&KNOWN[i]));
        let r = egui::ComboBox::from_id_salt(id.with("known"))
            .width(200.0)
            .selected_text(&shown)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut st.known, None, "Custom…");
                for (i, k) in KNOWN.iter().enumerate() {
                    let off = known_off(k, root, have);
                    let r = ui.add_enabled(
                        off.is_none(),
                        egui::Button::selectable(st.known == Some(i), known_label(k)),
                    );
                    if r.clicked() {
                        st.known = Some(i);
                        st.text.clear();
                        st.z = None;
                        st.calc_none = false;
                    }
                    match off {
                        Some(why) => r.on_disabled_hover_text(why),
                        None => r.on_hover_text(k.tip),
                    };
                }
            })
            .response;
        set_name(
            &r,
            egui::WidgetType::ComboBox,
            &scoped(root, "Known extra data"),
            None,
            Some(shown),
        );
    });
    // A pick made while it was free can become taken (an Add, an undo): back to Custom….
    if st
        .known
        .is_some_and(|i| known_off(&KNOWN[i], root, have).is_some())
    {
        st.known = None;
    }
    if let (Some(i), false) = (st.known, root) {
        if let KnownValue::Heel = KNOWN[i].value {
            heel_row(ui, e, act, at, have, &mut st);
            ui.data_mut(|d| d.insert_temp(id, st));
            return;
        }
    }
    if let Some(i) = st.known {
        known_row(ui, act, root, at, &KNOWN[i], &mut st);
        ui.data_mut(|d| d.insert_temp(id, st));
        return;
    }
    let (mut kind, mut name) = (st.kind, std::mem::take(&mut st.name));
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
    ui.data_mut(|d| {
        d.insert_temp(
            id,
            AddState {
                known: None,
                kind,
                name,
                text: st.text,
                z: None,
                sdta: st.sdta,
                calc_none: false,
            },
        )
    });
}

/// A Known entry's row: its type and name as set, a Value field for a string entry, and Add.
fn known_row(
    ui: &mut egui::Ui,
    act: &mut Act,
    root: bool,
    at: Anchor,
    k: &Known,
    st: &mut AddState,
) {
    ui.horizontal(|ui| {
        ui.weak(k.kind);
        ui.label(k.name).on_hover_text(k.tip);
    });
    ui.horizontal(|ui| {
        if let KnownValue::Text(_) = k.value {
            let r = ui.add(
                egui::TextEdit::singleline(&mut st.text)
                    .hint_text("Value")
                    .desired_width(ui.available_width().min(240.0)),
            );
            set_name(
                &r,
                egui::WidgetType::TextEdit,
                &scoped(root, "Value"),
                None,
                Some(st.text.clone()),
            );
            r.on_hover_text(k.tip);
        }
        let ready = !matches!(k.value, KnownValue::Text(Some(_))) || !st.text.trim().is_empty();
        let r = ui.add_enabled(ready, egui::Button::new("Add"));
        let label = if root { "Add to root" } else { "Add to shape" };
        set_name(&r, egui::WidgetType::Button, label, None, None);
        if r.clicked() {
            let value = match k.value {
                KnownValue::Text(_) => {
                    ExtraValue::Str(std::mem::take(&mut st.text).trim().to_string())
                }
                KnownValue::Fixed(f) => f(),
                // The heel entry has its own row (`heel_row`); nothing to add here (T5b review Minor 8).
                KnownValue::Heel => return,
            };
            act.push = Some((
                vec![Op::AddExtra {
                    at,
                    kind: k.kind.into(),
                    name: k.name.into(),
                    value,
                }],
                None,
            ));
        } else if let (false, KnownValue::Text(Some(why))) = (ready, k.value) {
            r.on_disabled_hover_text(why);
        }
    });
}

/// True for heel data: an extra named HH_OFFSET or SDTA (any case).
fn is_heel(x: &Extra) -> bool {
    x.name
        .as_deref()
        .is_some_and(|n| n.eq_ignore_ascii_case(b"HH_OFFSET") || n.eq_ignore_ascii_case(b"SDTA"))
}

/// The first extra of `kind` named `name` (any case) and its index in `have`.
fn first_named<'a>(have: &[&'a Extra], kind: &str, name: &[u8]) -> Option<(usize, &'a Extra)> {
    have.iter().enumerate().find_map(|(i, x)| {
        (x.kind == kind
            && x.name
                .as_deref()
                .is_some_and(|n| n.eq_ignore_ascii_case(name)))
        .then_some((i, *x))
    })
}

/// Meshes UI T5b (Note CZ19): heel height on a shape. Set writes HH_OFFSET (edited in place when the shape has one),
/// moves the first SDTA's last "NPC" entry to the same Z when the shape has an SDTA, or adds an SDTA only when
/// ticked. Calculate fills Z from the shape's lowest bind-pose vertex; the value stays editable.
fn heel_row(
    ui: &mut egui::Ui,
    e: &NifEdit,
    act: &mut Act,
    at: Anchor,
    have: &[&Extra],
    st: &mut AddState,
) {
    let shape = at.block;
    // T5b review Minor 3: an HH_OFFSET the editor cannot set (another type, or a value that did not read) blocks Set:
    // adding a second one would be ignored in game.
    let hh_any = have.iter().position(|x| {
        x.name
            .as_deref()
            .is_some_and(|n| n.eq_ignore_ascii_case(b"HH_OFFSET"))
    });
    let hh = hh_any.map(|i| (i, have[i])).filter(|(_, x)| {
        x.kind == "NiFloatExtraData" && matches!(x.value, Some(ExtraValue::Float(_)))
    });
    let blocked = hh_any.is_some() && hh.is_none();
    let sdta = first_named(have, "NiStringExtraData", b"SDTA");
    let stored = match (hh.map(|h| &h.1.value), sdta.map(|s| &s.1.value)) {
        (Some(Some(ExtraValue::Float(b))), _) => Some(f32::from_bits(*b)),
        (_, Some(Some(ExtraValue::Str(s)))) => sdta_npc_z(s),
        _ => None,
    };
    // T5b review Important 1: `st.z` holds only a value the user typed or Calculate gave; otherwise the field follows
    // the stored value (an undo, a row edit, a Revert).
    let mut z = st.z.unwrap_or(stored.unwrap_or(0.0));
    ui.horizontal(|ui| {
        ui.label("Heel height").on_hover_text(HEEL_TIP);
        let r = ui.add(egui::DragValue::new(&mut z).speed(0.1).max_decimals(2));
        set_name(
            &r,
            egui::WidgetType::DragValue,
            &scoped(false, "Heel height"),
            Some(z as f64),
            None,
        );
        if r.changed() {
            st.z = Some(z);
        }
        let calc = ui.button("Calculate").on_hover_text(HEEL_TIP);
        if calc.clicked() {
            match heel_z_of(&e.patched, shape) {
                Some(c) => {
                    z = c;
                    st.z = Some(c);
                    st.calc_none = false;
                }
                None => st.calc_none = true,
            }
        }
    });
    if st.z.is_some() && st.z == stored {
        st.z = None;
    }
    if st.calc_none {
        ui.weak("This shape has no geometry to measure: type the height.");
    }
    if z < 0.0 {
        ui.weak("A negative height lowers the actor.");
    }
    if blocked {
        ui.colored_label(AMBER, HEEL_HH_UNSET);
    }
    if sdta.is_none() {
        ui.checkbox(&mut st.sdta, "Also write SDTA (OStim and others read it)");
    }
    // Only shapes the root reaches count, in block order (the controller ruling; see `validate`).
    let earlier = e
        .model
        .shapes
        .iter()
        .take_while(|s| s.block != shape)
        .filter(|s| e.model.index3d.contains_key(&s.block))
        .any(|s| e.model.extras_of(&s.net).into_iter().any(is_heel));
    // A shape with heel data of its own already shows this under that row (validate).
    if earlier && !have.iter().any(|x| is_heel(x)) {
        ui.colored_label(AMBER, HEEL_EARLIER_WINS);
    }
    if let Some(sib) = &e.weight_sibling {
        let sib = sib.replace('/', "\\");
        ui.add(
            egui::Label::new(
                egui::RichText::new(format!(
                    "Apply the same value to the other weight file ({sib})."
                ))
                .weak(),
            )
            .wrap(),
        );
    }
    let r = ui.add_enabled(!blocked, egui::Button::new("Set heel height"));
    set_name(&r, egui::WidgetType::Button, "Set heel height", None, None);
    if !r.clicked() {
        r.on_disabled_hover_text(HEEL_HH_UNSET);
        return;
    }
    let mut ops = vec![];
    // T5b review Important 2: the same number goes to HH_OFFSET and to the SDTA (`sdta_with_npc_z` writes `z` as is).
    let value = ExtraValue::Float(z.to_bits());
    match hh {
        Some((i, x)) => {
            if let Some(base) = x.value.clone().filter(|b| *b != value) {
                ops.push(Op::EditExtra {
                    at: extra_anchor(have, i, at.shape.clone(), false),
                    value,
                    base,
                });
            }
        }
        None => ops.push(Op::AddExtra {
            at: at.clone(),
            kind: "NiFloatExtraData".into(),
            name: "HH_OFFSET".into(),
            value,
        }),
    }
    match sdta {
        Some((i, x)) => {
            // T5b review Minor 5: an SDTA that already has this height is left as it is (a rewrite would reformat it).
            if let Some(ExtraValue::Str(cur)) =
                x.value.as_ref().filter(|_| sdta_npc_z_of(x) != Some(z))
            {
                // shortcut: an SDTA that is not a JSON list is left as it is (HH_OFFSET wins in game anyway).
                if let Some(new) = sdta_with_npc_z(cur, z) {
                    ops.push(Op::EditExtra {
                        at: extra_anchor(have, i, at.shape.clone(), false),
                        value: ExtraValue::Str(new),
                        base: ExtraValue::Str(cur.clone()),
                    });
                }
            }
        }
        None if st.sdta => {
            if let Some(new) = sdta_with_npc_z("[]", z) {
                ops.push(Op::AddExtra {
                    at,
                    kind: "NiStringExtraData".into(),
                    name: "SDTA".into(),
                    value: ExtraValue::Str(new),
                });
            }
        }
        None => {}
    }
    if !ops.is_empty() {
        act.push = Some((ops, None));
    }
    st.z = None;
}

/// The last "NPC" Z of an SDTA extra, when its value is a string.
fn sdta_npc_z_of(x: &Extra) -> Option<f32> {
    match &x.value {
        Some(ExtraValue::Str(s)) => sdta_npc_z(s),
        _ => None,
    }
}

/// T5b review Minor 3: the note and the disabled hover of Set when the shape's HH_OFFSET cannot be set.
const HEEL_HH_UNSET: &str =
    "This shape's HH_OFFSET is not a number the editor can set: remove that row first.";
