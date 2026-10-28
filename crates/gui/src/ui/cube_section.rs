//! The cubemap section of a cube's settings (cubemap GUI step, Note DT7-DT12): one row per `cubemap.*` key, in five
//! groups. Drawing only: the edits come back as (item key, dotted field, value) and the panel applies them through
//! `AppModel::set_field` / `clear_field`, so sdc.toml stays the only place a setting is written.
use super::columns::RED;
use super::settings_panel::{combo, options_of, Options};
use crate::model::fields::{cube_views, provenance_text, FieldView};
use crate::model::AppModel;
use eframe::egui;
use engine::config::{CubeLayout, FaceFlip, GroundFill, GroundMode, Subtype};
use engine::policy::ResolvedJob;
use serde_json::{json, Value};
use std::ops::RangeInclusive;

/// (item key, field, value); `None` clears the field.
pub type Edits = Vec<(String, String, Option<Value>)>;

/// A field's edit: `Some(Some(v))` sets it, `Some(None)` clears it, `None` leaves it.
type Edit = Option<Option<Value>>;

/// The first ground colour a "Colour" pick writes: a dark earth brown, raw 0..255 codes (Note DT10).
const FIRST_GROUND: [u8; 3] = [40, 30, 20];
/// What ticking "Highlight clamp" or "Blur" first writes (Note DT10): the engine's own examples.
const FIRST_CLAMP: f64 = 4.0;
const FIRST_BLUR: f64 = 0.35;
/// The photo layout's blur when `cubemap.blur` is unset (engine DS16).
const PHOTO_BLUR: &str = "0.6 (photo default)";
const FACE_NAMES: [&str; 6] = ["+X", "-X", "+Y", "-Y", "+Z", "-Z"];

/// Every selected item is a cube; `key` alone when the selection is a set or empty.
fn all_cubes(model: &AppModel, key: &str) -> bool {
    let mut keys = model.selected_items();
    if keys.is_empty() {
        keys.push(key.to_string());
    }
    keys.iter().all(|k| {
        model
            .item(k)
            .and_then(|i| i.job.as_ref())
            .is_some_and(|j| j.profile.subtype == Subtype::Cubemap)
    })
}

/// A drag value that returns its value once per gesture (drag release or Enter), not every frame. The value in
/// flight lives in `ui.data` under `id`; one already sent stays on show until the re-plan lands. The engine range
/// bounds the widget, but a value already outside it is shown as it is and never rewritten (ruling R2).
#[allow(clippy::too_many_arguments)]
fn num(
    ui: &mut egui::Ui,
    busy: bool,
    id: egui::Id,
    current: f64,
    range: RangeInclusive<f64>,
    speed: f64,
    suffix: &str,
    name: &str,
) -> (Option<f64>, egui::Response) {
    let mut pending: Option<(f64, bool)> = ui.data(|d| d.get_temp(id));
    if pending.is_some_and(|(v, sent)| v == current || (sent && !busy)) {
        pending = None;
    }
    let mut v = pending.map_or(current, |p| p.0);
    let r = ui.add(
        egui::DragValue::new(&mut v)
            .range(range)
            .clamp_existing_to_range(false)
            .speed(speed)
            .max_decimals(3)
            .suffix(suffix),
    );
    r.widget_info(|| {
        let mut i = egui::WidgetInfo::new(egui::WidgetType::DragValue);
        i.enabled = r.enabled();
        i.label = Some(name.to_string());
        i.value = Some(v);
        i
    });
    if r.changed() {
        pending = Some((v, false));
    }
    let mut out = None;
    let settled =
        r.drag_stopped() || r.lost_focus() || (r.changed() && !r.dragged() && !r.has_focus());
    if let Some((pv, false)) = pending {
        if settled && pv != current {
            out = Some(pv);
            pending = Some((pv, true));
        }
    }
    ui.data_mut(|d| match pending {
        Some(p) => {
            d.insert_temp(id, p);
        }
        None => d.remove::<(f64, bool)>(id),
    });
    (out, r)
}

/// A colour button over raw 0..255 codes; the new codes once the picker's pointer is released.
fn colour(
    ui: &mut egui::Ui,
    busy: bool,
    id: egui::Id,
    current: [u8; 3],
    name: &str,
) -> (Option<[u8; 3]>, egui::Response) {
    let mut pending: Option<([u8; 3], bool)> = ui.data(|d| d.get_temp(id));
    if pending.is_some_and(|(v, sent)| v == current || (sent && !busy)) {
        pending = None;
    }
    let mut c = pending.map_or(current, |p| p.0);
    let r = ui.color_edit_button_srgb(&mut c);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, r.enabled(), name));
    if r.changed() {
        pending = Some((c, false));
    }
    let mut out = None;
    if let Some((pv, false)) = pending {
        if !ui.input(|i| i.pointer.any_down()) && pv != current {
            out = Some(pv);
            pending = Some((pv, true));
        }
    }
    ui.data_mut(|d| match pending {
        Some(p) => {
            d.insert_temp(id, p);
        }
        None => d.remove::<([u8; 3], bool)>(id),
    });
    (out, r)
}

/// A combo over `options`, named for a screen reader with the shown option as its value.
fn named_combo(
    ui: &mut egui::Ui,
    id: egui::Id,
    current: &Value,
    options: &Options,
    name: &str,
) -> (Option<Value>, egui::Response) {
    let (picked, r) = combo(ui, id, current, options, false);
    let shown = options
        .iter()
        .find(|(v, _)| v == current)
        .map_or_else(|| current.to_string(), |(_, l)| l.clone());
    r.widget_info(|| {
        let mut i = egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, r.enabled(), name);
        i.current_text_value = Some(shown.clone());
        i
    });
    (picked, r)
}

fn checkbox(ui: &mut egui::Ui, on: bool, name: &str) -> (Option<bool>, egui::Response) {
    let mut v = on;
    let r = ui.add(egui::Checkbox::new(&mut v, ""));
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, r.enabled(), v, name));
    (r.changed().then_some(v), r)
}

/// What a section draws with: the job, why sdc.toml is read-only, whether a plan runs.
struct Cx<'a> {
    key: &'a str,
    job: &'a ResolvedJob,
    ro: Option<String>,
    busy: bool,
}

impl Cx<'_> {
    fn id(&self, field: &str) -> egui::Id {
        egui::Id::new(("cube", self.key, field))
    }

    /// The engine's CFG-INVALID lines for `field`, shown under its row.
    fn errors(&self, field: &str) -> Vec<&str> {
        let head = format!("{field} ");
        self.job
            .warnings
            .iter()
            .filter(|w| w.code == engine::codes::Code::CfgInvalid && w.message.starts_with(&head))
            .map(|w| w.message.as_str())
            .collect()
    }

    /// One grid row: label, the control (disabled with its reason on hover when locked or read-only), where the value
    /// came from or the lock's reason, a reset when the texture's own entry sets it, then the engine's error lines.
    fn row(
        &self,
        ui: &mut egui::Ui,
        f: &FieldView,
        body: impl FnOnce(&mut egui::Ui) -> (Edit, egui::Response),
    ) -> Edit {
        ui.label(f.label);
        let off = f.lock.is_some() || self.ro.is_some();
        let (mut edit, r) = ui
            .scope(|ui| {
                if off {
                    ui.disable();
                }
                body(ui)
            })
            .inner;
        if let Some(why) = f.lock.map(str::to_string).or(self.ro.clone()) {
            r.on_disabled_hover_text(why);
        }
        match f.lock {
            Some(why) => ui.label(egui::RichText::new(why).small().weak()),
            None => ui.label(
                egui::RichText::new(provenance_text(&f.provenance))
                    .small()
                    .weak(),
            ),
        };
        if f.edited
            && ui
                .add_enabled(self.ro.is_none(), egui::Button::new("↺").small())
                .on_hover_text("Use the inherited value")
                .clicked()
        {
            edit = Some(None);
        }
        ui.end_row();
        for e in self.errors(f.field) {
            ui.label("");
            ui.colored_label(RED, e);
            ui.end_row();
        }
        edit
    }
}

fn find<'a>(views: &'a [FieldView], field: &str) -> &'a FieldView {
    views
        .iter()
        .find(|f| f.field == field)
        .expect("a cube field")
}

fn grid<R>(ui: &mut egui::Ui, cx: &Cx, name: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.label(egui::RichText::new(name).strong());
    egui::Grid::new(cx.id(name))
        .num_columns(4)
        .show(ui, add)
        .inner
}

/// The cubemap section; no edits and nothing drawn unless every selected item is a cube (a mixed selection shows no
/// cube rows and writes no `cubemap.*` key, Note DT8).
pub fn cube_section(ui: &mut egui::Ui, model: &AppModel, key: &str) -> Edits {
    if !all_cubes(model, key) {
        return vec![];
    }
    let Some(job) = model.item(key).and_then(|i| i.job.clone()) else {
        return vec![];
    };
    let views = cube_views(model.target, &job);
    let cx = Cx {
        key,
        job: &job,
        ro: model.toml_read_only_reason(),
        busy: model.busy(),
    };
    let c = &job.profile.cubemap;
    // The planned layout (a six-file set is `faces`) wins over the setting, which may be unset (auto).
    let layout = job.cube.as_ref().map(|j| j.layout).or(c.layout);
    let six_files = layout == Some(CubeLayout::Faces);
    let mut edits: Edits = vec![];
    let mut put = |field: &str, e: Edit| {
        if let Some(v) = e {
            edits.push((key.to_string(), field.to_string(), v));
        }
    };
    let opts = |field: &str| options_of(field, model.target, &job.profile).unwrap_or_default();
    let name = |f: &FieldView| format!("Cubemap {}", f.label.to_lowercase());
    ui.add_space(6.0);
    egui::CollapsingHeader::new("Cubemap")
        .id_salt(("cube-section", key))
        .default_open(true)
        .show(ui, |ui| {
            grid(ui, &cx, "Source", |ui| {
                let f = find(&views, "cubemap.layout");
                let e = cx.row(ui, f, |ui| {
                    if six_files {
                        let r = ui.label("Six face files");
                        return (None, r);
                    }
                    let cur = c.layout.map_or(json!("auto"), |l| json!(l));
                    let (p, r) = named_combo(ui, cx.id(f.field), &cur, &opts(f.field), &name(f));
                    // Auto is the unset layout.
                    (p.map(|v| (v != json!("auto")).then_some(v)), r)
                });
                put(f.field, e);
                let f = find(&views, "cubemap.face_size");
                let e = cx.row(ui, f, |ui| {
                    let cur = c.face_size.map_or(json!("auto"), |n| json!(n));
                    let (p, r) = named_combo(ui, cx.id(f.field), &cur, &opts(f.field), &name(f));
                    (p.map(|v| (v != json!("auto")).then_some(v)), r)
                });
                put(f.field, e);
                if layout == Some(CubeLayout::Vcross) {
                    let f = find(&views, "cubemap.vcross_negz_rotate");
                    let e = cx.row(ui, f, |ui| {
                        let (p, r) = checkbox(ui, c.vcross_negz_rotate, &name(f));
                        // Ticked is the default, so it drops the key rather than writing `true`.
                        (p.map(|on| (!on).then_some(json!(false))), r)
                    });
                    put(f.field, e);
                }
            });
            grid(ui, &cx, "Orientation", |ui| {
                let f = find(&views, "cubemap.yaw");
                let e = cx.row(ui, f, |ui| {
                    let (p, r) = num(
                        ui,
                        cx.busy,
                        cx.id(f.field),
                        c.yaw,
                        -360.0..=360.0,
                        1.0,
                        "°",
                        &name(f),
                    );
                    (p.map(|v| Some(json!(v))), r)
                });
                put(f.field, e);
                let f = find(&views, "cubemap.mirror");
                let e = cx.row(ui, f, |ui| {
                    let (p, r) = checkbox(ui, c.mirror, &name(f));
                    (p.map(|on| on.then_some(json!(true))), r)
                });
                put(f.field, e);
            });
            let has_faces = matches!(
                layout,
                Some(
                    CubeLayout::Faces
                        | CubeLayout::Hcross
                        | CubeLayout::Vcross
                        | CubeLayout::Hstrip
                        | CubeLayout::Vstrip
                )
            );
            if has_faces {
                faces(ui, &cx, &views, &opts, &mut put);
            }
            grid(ui, &cx, "Colour grade", |ui| {
                let rows: [(&str, f64, RangeInclusive<f64>, f64); 5] = [
                    ("cubemap.exposure", c.exposure, -10.0..=10.0, 0.05),
                    ("cubemap.brightness", c.brightness, -1.0..=1.0, 0.01),
                    ("cubemap.contrast", c.contrast, -1.0..=1.0, 0.01),
                    ("cubemap.midtones", c.midtones, 0.1..=10.0, 0.01),
                    ("cubemap.saturation", c.saturation, 0.0..=2.0, 0.01),
                ];
                for (field, cur, range, speed) in rows {
                    let f = find(&views, field);
                    let e = cx.row(ui, f, |ui| {
                        let (p, r) =
                            num(ui, cx.busy, cx.id(field), cur, range, speed, "", &name(f));
                        (p.map(|v| Some(json!(v))), r)
                    });
                    put(field, e);
                }
                let f = find(&views, "cubemap.tint");
                let e = cx.row(ui, f, |ui| {
                    ui.horizontal(|ui| match c.tint {
                        Some(t) => {
                            let (p, r) = colour(ui, cx.busy, cx.id(f.field), t, &name(f));
                            (p.map(|v| Some(json!(v))), r)
                        }
                        None => {
                            let r = ui.button("Set a tint");
                            // White is the neutral tint.
                            ((r.clicked()).then(|| Some(json!([255, 255, 255]))), r)
                        }
                    })
                    .inner
                });
                put(f.field, e);
                let f = find(&views, "cubemap.highlight_clamp");
                let e = cx.row(ui, f, |ui| {
                    ui.horizontal(|ui| {
                        let (tick, r) = checkbox(ui, c.highlight_clamp.is_some(), &name(f));
                        let Some(v) = c.highlight_clamp else {
                            return (tick.map(|_| Some(json!(FIRST_CLAMP))), r);
                        };
                        let (p, r2) = num(
                            ui,
                            cx.busy,
                            cx.id(f.field),
                            v,
                            1.0..=f64::from(f32::MAX),
                            0.1,
                            "",
                            "Cubemap highlight clamp value",
                        );
                        // Unticked is Off: the key goes.
                        (tick.map(|_| None).or(p.map(|v| Some(json!(v)))), r2)
                    })
                    .inner
                });
                put(f.field, e);
            });
            grid(ui, &cx, "Ground", |ui| {
                let f = find(&views, "cubemap.ground_fill");
                let e = cx.row(ui, f, |ui| {
                    ui.horizontal(|ui| {
                        let cur = match c.ground_fill {
                            None => "none",
                            Some(GroundFill::Mode(GroundMode::Gradient)) => "gradient",
                            Some(GroundFill::Color(_)) => "colour",
                        };
                        let (p, r) =
                            named_combo(ui, cx.id(f.field), &json!(cur), &opts(f.field), &name(f));
                        let mut edit: Edit = p.map(|v| match v.as_str() {
                            Some("gradient") => Some(json!("gradient")),
                            Some("colour") => Some(json!(FIRST_GROUND)),
                            _ => None,
                        });
                        if let Some(GroundFill::Color(rgb)) = c.ground_fill {
                            let (q, _) = colour(
                                ui,
                                cx.busy,
                                cx.id("cubemap.ground_colour"),
                                rgb,
                                "Cubemap ground colour",
                            );
                            edit = edit.or(q.map(|v| Some(json!(v))));
                        }
                        (edit, r)
                    })
                    .inner
                });
                put(f.field, e);
                let f = find(&views, "cubemap.horizon");
                let e = cx.row(ui, f, |ui| {
                    let (p, r) = num(
                        ui,
                        cx.busy,
                        cx.id(f.field),
                        c.horizon,
                        -45.0..=45.0,
                        0.5,
                        "°",
                        &name(f),
                    );
                    (p.map(|v| Some(json!(v))), r)
                });
                put(f.field, e);
            });
            grid(ui, &cx, "Look", |ui| {
                let f = find(&views, "cubemap.style");
                let e = cx.row(ui, f, |ui| {
                    let cur = c.style.map_or(json!("none"), |s| json!(s));
                    let (p, r) = named_combo(ui, cx.id(f.field), &cur, &opts(f.field), &name(f));
                    (p.map(|v| (v != json!("none")).then_some(v)), r)
                });
                put(f.field, e);
                let f = find(&views, "cubemap.blur");
                let photo_auto = layout == Some(CubeLayout::Photo) && !f.edited;
                let e = cx.row(ui, f, |ui| {
                    ui.horizontal(|ui| {
                        let (tick, r) = checkbox(ui, c.blur.is_some(), &name(f));
                        let Some(v) = c.blur else {
                            return (tick.map(|_| Some(json!(FIRST_BLUR))), r);
                        };
                        let (p, r2) = num(
                            ui,
                            cx.busy,
                            cx.id(f.field),
                            v,
                            0.0..=1.0,
                            0.01,
                            "",
                            "Cubemap blur value",
                        );
                        if photo_auto {
                            ui.weak(PHOTO_BLUR);
                        }
                        // Unticked is Off: the key goes.
                        (tick.map(|_| None).or(p.map(|v| Some(json!(v)))), r2)
                    })
                    .inner
                });
                put(f.field, e);
            });
        });
    edits
}

/// One compact row per face: rotation and flip. Both fields are whole arrays, so a pick rewrites the array, and an
/// array back at its default drops the key.
fn faces(
    ui: &mut egui::Ui,
    cx: &Cx,
    views: &[FieldView],
    opts: &dyn Fn(&str) -> Options,
    put: &mut dyn FnMut(&str, Edit),
) {
    let c = &cx.job.profile.cubemap;
    ui.label(egui::RichText::new("Faces").strong());
    let off = cx.ro.is_some();
    let mut rot = c.face_rotate;
    let mut flip = c.face_flip;
    let (mut rot_edit, mut flip_edit) = (false, false);
    for (i, face) in FACE_NAMES.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(*face);
            ui.add_enabled_ui(!off, |ui| {
                let (p, _) = named_combo(
                    ui,
                    egui::Id::new(("cube-rot", cx.key, i)),
                    &json!(rot[i]),
                    &opts("cubemap.face_rotate"),
                    &format!("Cubemap {face} rotation"),
                );
                if let Some(d) = p.and_then(|v| v.as_u64()) {
                    rot[i] = d as u16;
                    rot_edit = true;
                }
                let (p, _) = named_combo(
                    ui,
                    egui::Id::new(("cube-flip", cx.key, i)),
                    &json!(flip[i]),
                    &opts("cubemap.face_flip"),
                    &format!("Cubemap {face} flip"),
                );
                if let Some(f) = p.and_then(|v| serde_json::from_value::<FaceFlip>(v).ok()) {
                    flip[i] = f;
                    flip_edit = true;
                }
            });
        });
    }
    let own = |field: &str| find(views, field).edited;
    for (field, edited, is_default, value, reset) in [
        (
            "cubemap.face_rotate",
            rot_edit,
            rot == [0; 6],
            json!(rot),
            own("cubemap.face_rotate"),
        ),
        (
            "cubemap.face_flip",
            flip_edit,
            flip == [FaceFlip::None; 6],
            json!(flip),
            own("cubemap.face_flip"),
        ),
    ] {
        if edited {
            put(field, Some((!is_default).then_some(value)));
        }
        for e in cx.errors(field) {
            ui.colored_label(RED, e);
        }
        if reset
            && ui
                .add_enabled(
                    !off,
                    egui::Button::new(format!("↺ {}", find(views, field).label)).small(),
                )
                .on_hover_text("Use the inherited value")
                .clicked()
        {
            put(field, Some(None));
        }
    }
}
