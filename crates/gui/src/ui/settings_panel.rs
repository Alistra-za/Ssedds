//! The centre panel for a selected texture (M3a spec §2.4): the decisions it still needs, then its settings by group,
//! each with where its value came from. Drawing only: every edit is a call on the model, applied after drawing.
use super::columns::{AMBER, RED};
use super::status::code_hover;
use super::type_picker::TypeKind;
use crate::model::code_text::{code_line, shown_for, without_codes};
use crate::model::fields::{
    auto_subtype_reason, backend_lock, channel_text, edge_view, encoding_options, field_hidden,
    field_views, filter_option_lock, filter_view, group_of, levels_view, mips_count, mips_on,
    provenance_text, weighting_option_lock, FieldView, NOT_VALID, PARALLAX_HOVER, WEIGHTING,
    WEIGHTING_OPTIONS,
};
use crate::model::items::{Decision, ItemKind, ItemStatus};
use crate::model::{Answer, AppModel};
use eframe::egui;
use encoders_sys::RGBCX_MAX_LEVEL;
use engine::codes::{Code, Warning};
use engine::config::{
    AlphaPolicy, Backend, ColorTag, CubeLayout, CubeStyle, FaceFlip, Format, GreenConvention,
    MipFilter, Profile, Subtype, Target, ALPHA_THRESHOLD,
};
use engine::policy::{offered, Provenance, ResolvedJob};
use serde_json::{json, Value};

use crate::model::review::NO_EXPORT_FOLDER;

/// What a control asks of the model; applied once the drawing is over.
enum Action {
    Set(&'static str, Value),
    Clear(&'static str),
    Decide(Answer),
    Select(String),
    PatchTag(Option<bool>),
    PatchAsk,
    PatchDo,
    PatchCancel,
    Reencode(bool),
}

/// The groups the compact strip keeps (Task 17's preview).
const COMPACT_GROUPS: [&str; 3] = ["Alpha", "Mipmaps", "Compression"];
pub(super) const GROUPS: [&str; 5] = ["Type", "Colour", "Alpha", "Mipmaps", "Compression"];
/// A 4096 px side has 13 mip levels.
const MAX_LEVELS: u32 = 13;

const FILTER_OPTIONS: [(MipFilter, &str); 4] = [
    (MipFilter::Box, "Box"),
    (MipFilter::Tent, "Tent"),
    (MipFilter::Mitchell, "Mitchell"),
    (MipFilter::Kaiser, "Kaiser"),
];

/// An enum's options as (JSON value, label).
pub type Options = Vec<(Value, String)>;

fn opt(v: Value, label: &str) -> (Value, String) {
    (v, label.to_string())
}

pub fn subtype_label(st: Subtype) -> String {
    crate::model::labels::subtype_name(st)
}

/// The heading of the PBR group in every sub-type dropdown.
pub const PBR_HEADING: &str = "PBR (Community Shaders)";
/// The heading of the Advanced skin group, right after the PBR one.
pub const SKIN_HEADING: &str = "Advanced skin (Community Shaders)";
/// The heading of the last group: what no other group claims.
pub const OTHER_HEADING: &str = "Other";

/// The sub-types `keep` accepts, grouped for a dropdown: the PBR ones first (every `pbr_*`, `rmaos` and
/// `coat_normal`) under `PBR_HEADING`, then the Advanced skin ones (skin_albedo, skin_rfaos, skin_sk, skin_wet) under `SKIN_HEADING`, then the
/// rest by category, and `ui_sky` last under `OTHER_HEADING`. Empty groups are left out.
pub fn subtype_groups(keep: impl Fn(Subtype) -> bool) -> Vec<(&'static str, Vec<Subtype>)> {
    use engine::config::Category;
    let is_pbr = |s: Subtype| {
        s.id().starts_with("pbr_") || matches!(s, Subtype::Rmaos | Subtype::CoatNormal)
    };
    // The skin group reads in the order a set uses them, not in `Subtype::ALL`'s.
    let skin = [
        Subtype::SkinAlbedo,
        Subtype::SkinRfaos,
        Subtype::SkinSk,
        Subtype::SkinWet,
    ];
    let is_skin = |s: Subtype| skin.contains(&s);
    let is_other = |s: Subtype| matches!(s, Subtype::UiSky | Subtype::Cubemap);
    let claimed = |s: Subtype| is_pbr(s) || is_skin(s) || is_other(s);
    let kept: Vec<Subtype> = Subtype::ALL.into_iter().filter(|s| keep(*s)).collect();
    let pick = |f: &dyn Fn(Subtype) -> bool| -> Vec<Subtype> {
        kept.iter().copied().filter(|s| f(*s)).collect()
    };
    let mut groups: Vec<(&'static str, Vec<Subtype>)> = vec![
        (PBR_HEADING, pick(&is_pbr)),
        (
            SKIN_HEADING,
            skin.into_iter().filter(|s| keep(*s)).collect(),
        ),
    ];
    for (cat, name) in [
        (Category::Color, "Colour maps"),
        (Category::Normal, "Normal maps"),
        (Category::Grayscale, "Grayscale maps"),
        (Category::MixedPacked, "Packed and data"),
        (Category::Hdr, "HDR maps"),
    ] {
        groups.push((name, pick(&|s| !claimed(s) && s.category() == cat)));
    }
    groups.push((OTHER_HEADING, pick(&is_other)));
    groups.retain(|(_, v)| !v.is_empty());
    groups
}

/// The sub-type groups a picker offers under `target` (`engine::policy::offered_for`: `complex_grass` only in a
/// grass context, complex grass design §3.7, Note FC10).
pub fn offered_groups(target: Target, grass: bool) -> Vec<(&'static str, Vec<Subtype>)> {
    subtype_groups(|s| engine::policy::offered_for(target, s, grass))
}

pub fn alpha_options() -> Options {
    vec![
        (json!(AlphaPolicy::Remove), "Remove".into()),
        (json!(AlphaPolicy::Cutout), "Cutout".into()),
        (json!(AlphaPolicy::Blend), "Blend".into()),
        (json!(AlphaPolicy::CutoutBlend), "Cutout + Blend".into()),
        (json!(AlphaPolicy::Data), "Data".into()),
    ]
}

/// One line on what an alpha meaning does (Note FB8, review C12): the hover of every place that offers one. Problems'
/// Decide… menu and Review's buttons use it, and the question box in a texture's settings.
pub fn alpha_hint(policy: AlphaPolicy) -> &'static str {
    match policy {
        AlphaPolicy::Remove => "Drops the alpha channel: the texture is fully solid",
        AlphaPolicy::Cutout => {
            "Alpha cuts holes: pixels over the threshold stay solid, the rest vanish (leaves, fences)"
        }
        AlphaPolicy::Blend => "Alpha fades smoothly: soft see-through edges (glass, hair)",
        AlphaPolicy::CutoutBlend => "Alpha cuts holes and also fades the edges",
        AlphaPolicy::Data => {
            "The alpha is kept as data (a mask or a height): it is written as it is, not as see-through"
        }
    }
}

fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

/// Options for the fields that pick from a list; `None` for the checkboxes and the slider.
pub(super) fn options_of(field: &str, target: Target, p: &Profile) -> Option<Options> {
    Some(match field {
        "subtype" => subtype_groups(|s| s == p.subtype || offered(target, s))
            .into_iter()
            .flat_map(|(heading, members)| {
                std::iter::once((Value::Null, heading.to_string()))
                    .chain(members.into_iter().map(|s| (json!(s), subtype_label(s))))
            })
            .collect(),
        "color.content" => vec![
            opt(json!("srgb"), "sRGB-encoded colour"),
            opt(json!("linear"), "Linear data"),
        ],
        // The tags the engine allows with the current format, and the current one marked when it is not allowed.
        "color.tag" => encoding_options(target, p)
            .into_iter()
            .filter(|((f, _), _)| *f == p.format)
            .filter_map(|((_, t), label)| {
                let name = match t {
                    ColorTag::Srgb => "_SRGB",
                    ColorTag::Unorm => "UNORM (linear)",
                    ColorTag::NotApplicable => return None,
                };
                let mark = if label.ends_with(NOT_VALID) {
                    NOT_VALID
                } else {
                    ""
                };
                Some((json!(t), format!("{name}{mark}")))
            })
            .collect(),
        "alpha.policy" => alpha_options(),
        "alpha.compare" => vec![
            opt(json!("gt"), "Greater than"),
            opt(json!("ge"), "Greater than or equal"),
        ],
        "mips.levels" => {
            let mut v = vec![
                opt(json!("full"), "Full chain"),
                opt(json!("none"), "None (base level only)"),
            ];
            v.extend((2..=MAX_LEVELS).map(|n| (json!(n), format!("{n} levels"))));
            v
        }
        "mips.normal" => vec![
            opt(json!("renormalize"), "Renormalise"),
            opt(json!("renormalize_bake"), "Renormalise and bake"),
        ],
        "mips.normal_base" => vec![
            opt(json!("renormalize"), "Renormalise to unit length"),
            opt(json!("keep"), "Keep as authored"),
        ],
        "mips.edge" => vec![opt(json!("wrap"), "Wrap"), opt(json!("clamp"), "Clamp")],
        "resize" => vec![
            opt(json!("none"), "None"),
            opt(json!("pot-down"), "Down to a power of two"),
            opt(json!("half"), "Halve if larger than 512"),
        ],
        "mips.coverage" => vec![
            opt(json!("auto"), "Auto"),
            opt(json!("on"), "On"),
            opt(json!("off"), "Off"),
        ],
        // A (format, tag) pair, so a pick never leaves a tag the format cannot take; applied with `set_format`.
        "format" => encoding_options(target, p)
            .into_iter()
            .map(|(e, label)| (json!(e), label))
            .collect(),
        "encoder.effort" => [
            "ultrafast",
            "veryfast",
            "fast",
            "basic",
            "veryslow",
            "slowest",
            "max",
        ]
        .into_iter()
        .map(|e| opt(json!(e), &capitalised(e)))
        .collect(),
        "encoder.bc45_search_radius" => {
            vec![
                opt(json!("default"), "Default"),
                opt(json!("max"), "Maximum"),
                opt(json!("fast"), "Fast (preview)"),
            ]
        }
        "header" => vec![
            opt(json!("auto"), "Auto"),
            opt(json!("dx10"), "DX10 extended header"),
            opt(json!("legacy"), "Legacy FourCC"),
        ],
        "mips.filter" => FILTER_OPTIONS
            .iter()
            .map(|(m, l)| opt(json!(m), l))
            .collect(),
        // `set_field` writes "Built-in" as each texture's own format default (DECISIONS Note NV).
        "encoder.backend" => vec![
            opt(json!(engine::policy::default_backend(p.format)), "Built-in"),
            opt(json!(Backend::Nvtt), "NVTT Production"),
        ],
        // Cubemap (Note DT7). Auto is the unset layout, "none" the unset style / ground fill.
        "cubemap.layout" => std::iter::once(opt(json!("auto"), "Auto"))
            // Faces is detected from a six-file set, never chosen.
            .chain(pickable_layouts().map(|(l, label)| opt(json!(l), label)))
            .collect(),
        "cubemap.style" => vec![
            opt(json!("none"), "None"),
            opt(json!(CubeStyle::VanillaMetal), "Vanilla metal"),
            opt(json!(CubeStyle::Room), "Room"),
            opt(json!(CubeStyle::Eye), "Eye"),
            opt(json!(CubeStyle::Hd), "HD"),
        ],
        "cubemap.face_size" => std::iter::once(opt(json!("auto"), "Auto"))
            .chain((2..=12).map(|n| (json!(1u32 << n), format!("{} px", 1u32 << n))))
            .collect(),
        "cubemap.face_rotate" => [0, 90, 180, 270]
            .map(|d| (json!(d), format!("{d}°")))
            .to_vec(),
        "cubemap.face_flip" => vec![
            opt(json!(FaceFlip::None), "No flip"),
            opt(json!(FaceFlip::H), "Flip H"),
            opt(json!(FaceFlip::V), "Flip V"),
        ],
        "cubemap.ground_fill" => vec![
            opt(json!("none"), "None"),
            opt(json!("gradient"), "Gradient"),
            opt(json!("colour"), "Colour"),
        ],
        _ => return None,
    })
}

/// The cube layouts a picker offers, in display order. `Faces` (a six-file set) is not chosen but detected.
pub const CUBE_LAYOUTS: [(CubeLayout, &str); 9] = [
    (CubeLayout::Hcross, "Horizontal cross"),
    (CubeLayout::Vcross, "Vertical cross"),
    (CubeLayout::Hstrip, "Horizontal strip"),
    (CubeLayout::Vstrip, "Vertical strip"),
    (CubeLayout::Equirect, "360° panorama"),
    (CubeLayout::MirrorBall, "Mirror ball"),
    (CubeLayout::Angular, "Angular map"),
    (CubeLayout::Photo, "Single photo"),
    (CubeLayout::Faces, "Six face files"),
];

/// The layouts a user can pick: all but `Faces`, which is detected from a six-file set.
fn pickable_layouts() -> impl Iterator<Item = &'static (CubeLayout, &'static str)> {
    CUBE_LAYOUTS.iter().filter(|(l, _)| *l != CubeLayout::Faces)
}

/// The layouts the unclear-layout decision offers (DT32): an image only asks when no cross or strip fits it.
pub(crate) fn unclear_layouts() -> impl Iterator<Item = &'static (CubeLayout, &'static str)> {
    use CubeLayout::*;
    CUBE_LAYOUTS
        .iter()
        .filter(|(l, _)| matches!(l, Equirect | MirrorBall | Angular | Photo))
}

fn display_value(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// A combo over `options`; the new value when one is picked.
pub(super) fn combo(
    ui: &mut egui::Ui,
    id: egui::Id,
    current: &Value,
    options: &Options,
    mixed: bool,
) -> (Option<Value>, egui::Response) {
    combo_for("", ui, id, current, options, mixed)
}

/// [`combo`] for `field`: the Alpha field's five options carry `alpha_hint` as their hover (Note FB8, review C12).
pub(super) fn combo_for(
    field: &str,
    ui: &mut egui::Ui,
    id: egui::Id,
    current: &Value,
    options: &Options,
    mixed: bool,
) -> (Option<Value>, egui::Response) {
    // Note FB4: "— mixed" and "— not set" (no value, nothing to mix) are drawn weak, never the word `null`.
    let shown = if mixed {
        egui::RichText::new("— mixed").weak()
    } else if current.is_null() {
        // A null option is a group heading, so a null value never matches one.
        egui::RichText::new("— not set").weak()
    } else {
        match options.iter().find(|(v, _)| v == current) {
            Some((_, l)) => egui::RichText::new(l),
            None => egui::RichText::new(display_value(current)),
        }
    };
    let mut picked = None;
    let r = egui::ComboBox::from_id_salt(id)
        .selected_text(shown)
        .show_ui(ui, |ui| {
            for (v, label) in options {
                // A null value is a non-selectable group heading.
                if v.is_null() {
                    ui.label(egui::RichText::new(label).weak());
                } else {
                    let mut r = ui.selectable_label(v == current, label);
                    if field == "alpha.policy" {
                        if let Ok(p) = serde_json::from_value::<AlphaPolicy>(v.clone()) {
                            r = r.on_hover_text(alpha_hint(p));
                        }
                    }
                    if r.clicked() {
                        picked = Some(v.clone());
                    }
                }
            }
        })
        .response;
    (picked.filter(|v| v != current), r)
}

/// A slider keeps its value locally while it is dragged and commits on release: every commit is a full re-plan.
/// `committed` marks a value already sent, shown until the new plan lands.
#[derive(Clone, Copy)]
struct Pending {
    value: u32,
    committed: bool,
}

/// A slider over `range` that returns a value only when the drag ends (or a click or key press settles it).
/// `id` keeps the in-progress value between frames; `name` is the accessible name.
pub fn commit_slider(
    ui: &mut egui::Ui,
    model_busy: bool,
    id: egui::Id,
    current: u32,
    range: std::ops::RangeInclusive<u32>,
    name: &str,
    mixed: bool,
) -> (Option<u32>, egui::Response) {
    let mut pending: Option<Pending> = ui.data(|d| d.get_temp(id));
    if let Some(p) = pending {
        if (!mixed && p.value == current) || (p.committed && !model_busy) {
            pending = None;
        }
    }
    let mut value = pending.map_or(current, |p| p.value);
    let mut slider = egui::Slider::new(&mut value, range);
    // A mixed value shows as text until the user drags; only a drag writes.
    if mixed && pending.is_none() {
        slider = slider.custom_formatter(|_, _| "— mixed".to_string());
    }
    let r = ui.add(slider);
    r.widget_info(|| egui::WidgetInfo::slider(r.enabled(), f64::from(value), name));
    if r.changed() {
        pending = Some(Pending {
            value,
            committed: false,
        });
    }
    let released = r.drag_stopped()
        || r.lost_focus()
        || r.clicked()
        || (r.changed() && !r.dragged() && !r.is_pointer_button_down_on());
    let mut out = None;
    if released {
        if let Some(Pending {
            value,
            committed: false,
        }) = pending
        {
            if mixed || value != current {
                out = Some(value);
                pending = Some(Pending {
                    value,
                    committed: true,
                });
            }
        }
    }
    ui.data_mut(|d| match pending {
        Some(p) => {
            d.insert_temp(id, p);
        }
        None => {
            d.remove::<Pending>(id);
        }
    });
    (out, r)
}

fn threshold(
    ui: &mut egui::Ui,
    model_busy: bool,
    key: &str,
    current: u8,
    mixed: bool,
) -> (Option<Value>, egui::Response) {
    let id = egui::Id::new(("threshold", key));
    let (out, r) = commit_slider(
        ui,
        model_busy,
        id,
        u32::from(current),
        u32::from(*ALPHA_THRESHOLD.start())..=u32::from(*ALPHA_THRESHOLD.end()),
        "Alpha threshold",
        mixed,
    );
    (out.map(|v| json!(v)), r)
}

/// The BC7 weighting dropdown (DECISIONS Note CP): every option with one line of hover text; Perceptual disabled with
/// its reason where the old Perceptual row was locked.
fn weighting_combo(
    ui: &mut egui::Ui,
    id: egui::Id,
    target: Target,
    job: &ResolvedJob,
    f: &FieldView,
    mixed: bool,
) -> (Option<Value>, egui::Response) {
    let current = f.value.as_str().unwrap_or("linear");
    let shown = if mixed {
        "— mixed"
    } else {
        WEIGHTING_OPTIONS
            .iter()
            .find(|o| o.0 == current)
            .map_or(current, |o| o.1)
    };
    let mut picked = None;
    let r = egui::ComboBox::from_id_salt(id)
        .selected_text(shown)
        .show_ui(ui, |ui| {
            for (v, label, hover) in WEIGHTING_OPTIONS {
                let lock = weighting_option_lock(target, &job.profile, v);
                let r = ui.add_enabled(
                    lock.is_none(),
                    egui::Button::selectable(!mixed && v == current, label),
                );
                match lock {
                    Some(why) => {
                        r.on_disabled_hover_text(format!("{hover} Not here: {why}."));
                    }
                    None => {
                        if r.on_hover_text(hover).clicked() && (mixed || v != current) {
                            picked = Some(json!(v));
                        }
                    }
                }
            }
        })
        .response;
    // M4: named for a screen reader, with the shown option as its value.
    r.widget_info(|| {
        let mut i =
            egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, r.enabled(), "BC7 weighting");
        i.current_text_value = Some(shown.to_string());
        i
    });
    (picked, r)
}

pub(super) fn editor(
    ui: &mut egui::Ui,
    key: &str,
    target: Target,
    job: &ResolvedJob,
    busy: bool,
    f: &FieldView,
    mixed: bool,
) -> (Option<Value>, egui::Response) {
    match f.field {
        "alpha.threshold" => {
            let cur = f.value.as_u64().unwrap_or(128).clamp(
                u64::from(*ALPHA_THRESHOLD.start()),
                u64::from(*ALPHA_THRESHOLD.end()),
            ) as u8;
            threshold(ui, busy, key, cur, mixed)
        }
        "alpha.parallax" => {
            let mut b = f.value.as_bool().unwrap_or(false);
            let r = ui
                .add(egui::Checkbox::new(&mut b, "").indeterminate(mixed))
                .on_hover_text(PARALLAX_HOVER);
            (r.changed().then(|| json!(b)), r)
        }
        WEIGHTING => weighting_combo(ui, egui::Id::new((key, f.field)), target, job, f, mixed),
        "alpha.exact_opaque" | "encoder.bc1_transparent_black" | "drop_top_mip" => {
            let mut b = f.value.as_bool().unwrap_or(false);
            let r = ui.add(egui::Checkbox::new(&mut b, "").indeterminate(mixed));
            (r.changed().then(|| json!(b)), r)
        }
        "alpha.exr_straight" => {
            let mut b = f.value.as_bool().unwrap_or(false);
            let r = ui
                .add(egui::Checkbox::new(&mut b, "").indeterminate(mixed))
                .on_hover_text(
                    "On: the colour is read as stored. Off: it is divided by alpha, as OpenEXR stores premultiplied colour",
                );
            (r.changed().then(|| json!(b)), r)
        }
        "encoder.rgbcx_level" => {
            let cur = f
                .value
                .as_u64()
                .unwrap_or(0)
                .min(u64::from(RGBCX_MAX_LEVEL)) as u32;
            let id = egui::Id::new(("rgbcx-level", key));
            let (out, r) =
                commit_slider(ui, busy, id, cur, 0..=RGBCX_MAX_LEVEL, "rgbcx level", mixed);
            (out.map(|v| json!(v)), r)
        }
        field => {
            let options = options_of(field, target, &job.profile).unwrap_or_default();
            let current = if field == "format" {
                json!((job.profile.format, job.profile.color.tag))
            } else {
                f.value.clone()
            };
            combo_for(
                field,
                ui,
                egui::Id::new((key, field)),
                &current,
                &options,
                mixed,
            )
        }
    }
}

/// Everything a group's rows need besides the group name.
pub struct Ctx<'a> {
    pub key: &'a str,
    pub job: &'a ResolvedJob,
    pub target: Target,
    pub busy: bool,
    pub compact: bool,
    /// Why `sdc.toml` cannot be edited, when it is read-only.
    pub toml_read_only: Option<String>,
    /// The item's warnings (the Compression group shows the HDR reason).
    pub warnings: &'a [Warning],
    /// The NVTT probe found nvcompress (the Encoder row offers NVTT only then).
    pub nvtt_found: bool,
}

/// What a field row asks of the model.
pub enum RowEdit {
    Set(Value),
    /// Use the inherited value again.
    Clear,
}

/// One grid row of a field: its label, its editor (disabled with the reason when locked), where the value came from
/// (or the lock's reason), and a reset when the texture's own entry sets it. `compact` leaves the provenance blank.
pub fn field_row(ui: &mut egui::Ui, cx: &Ctx, f: &FieldView) -> Option<RowEdit> {
    let mut edit = None;
    ui.label(f.label);
    let (set, r) = ui
        .scope(|ui| {
            if f.lock.is_some() {
                ui.disable();
            }
            if f.field == "subtype" {
                return subtype_field(ui, cx, f);
            }
            editor(ui, cx.key, cx.target, cx.job, cx.busy, f, false)
        })
        .inner;
    // Note CL40: on a `normal` in a complex material set, renormalize_bake is the opt-in gloss bake.
    let r = if f.field == "mips.normal"
        && cx.job.profile.subtype == Subtype::Normal
        && cx
            .job
            .shader
            .as_ref()
            .is_some_and(|s| s.shader == engine::shader::Shader::ComplexMaterial)
    {
        r.on_hover_text(
            "Renormalise and bake also bakes this set's complex _m gloss (anti-shimmer)",
        )
    } else {
        r
    };
    if let Some(reason) = f.lock.map(str::to_string).or(cx.toml_read_only.clone()) {
        r.on_disabled_hover_text(reason);
    }
    if let Some(v) = set {
        edit = Some(RowEdit::Set(v));
    }
    let mut cleared = false;
    hint_and_reset(ui, f.edited, &mut cleared, |ui| {
        if cx.compact {
            ui.label("");
        } else if let Some(reason) = f.lock {
            hint_label(ui, reason);
        } else if let Some(why) = (f.field == "subtype")
            .then(|| auto_subtype_reason(cx.job))
            .flatten()
        {
            // M6 Track E (Note U4): a sub-type an accepted High suggestion decided; hover says why.
            ui.label(egui::RichText::new("(auto)").small().weak())
                .on_hover_text(why);
        } else {
            hint_label(ui, &provenance_text(&f.provenance));
        }
    });
    if cleared {
        edit = Some(RowEdit::Clear);
    }
    ui.end_row();
    edit
}

/// A row's hint, cut to the room left with the whole text on one tooltip (Note F?12, rule 3.4).
fn hint_label(ui: &mut egui::Ui, text: &str) {
    ui.add(
        egui::Label::new(egui::RichText::new(text).small().weak())
            .truncate()
            .show_tooltip_when_elided(false),
    )
    .on_hover_text(text);
}

/// The last cell of a field row: ↺ (when `edited`) is laid out first on the right, the hint takes what is left
/// (Note F?12, rule 3.3), so the reset never leaves the panel. Sets `cleared` when ↺ is clicked.
fn hint_and_reset(
    ui: &mut egui::Ui,
    edited: bool,
    cleared: &mut bool,
    hint: impl FnOnce(&mut egui::Ui),
) {
    egui::Sides::new()
        .shrink_left()
        .truncate()
        .show(ui, hint, |ui| {
            if edited
                && ui
                    .small_button("↺")
                    .on_hover_text("Use the inherited value")
                    .clicked()
            {
                *cleared = true;
            }
        });
}

/// The Sub-type field (Note FB9, owner amendment J): a button showing the type that opens the type picker, the
/// engine's suggestion pinned on top. Named and valued as the combo it replaces.
fn subtype_field(ui: &mut egui::Ui, cx: &Ctx, f: &FieldView) -> (Option<Value>, egui::Response) {
    let current = serde_json::from_value::<Subtype>(f.value.clone()).ok();
    let shown = current.map_or_else(|| display_value(&f.value), subtype_label);
    let r = ui.button(format!("{shown} \u{25BE}"));
    r.widget_info(|| {
        let mut i = egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, r.enabled(), f.label);
        i.current_text_value = Some(shown.clone());
        i
    });
    let id = egui::Id::new(("sub-type-picker", cx.key));
    let suggested = cx.job.suggestions.first().map(|s| s.subtype);
    let grass = engine::policy::grass_context(cx.job);
    let kind = TypeKind::SubType;
    let picked = super::type_picker::show(ui, id, &r, current, suggested, kind, cx.target, grass);
    (picked.filter(|s| Some(*s) != current).map(|s| json!(s)), r)
}

/// The basic Mipmaps rows: the on/off tick box (the level count lives in the Advanced drawer), then the filter.
fn mipmaps_rows(ui: &mut egui::Ui, cx: &Ctx, actions: &mut Vec<Action>) {
    let f = levels_view(cx.target, cx.job);
    let mut on = mips_on(&f.value);
    let ticked = ui.add_enabled(
        cx.toml_read_only.is_none(),
        egui::Checkbox::new(&mut on, "Mipmaps"),
    );
    let ticked = match &cx.toml_read_only {
        Some(why) => ticked.on_disabled_hover_text(why),
        None => ticked,
    };
    if ticked.changed() {
        actions.push(Action::Set(
            f.field,
            json!(if on { "full" } else { "none" }),
        ));
    }
    // The hint sits in the editor column, so the third cell holds only ↺.
    let mut cleared = false;
    let hint_text = match mips_count(&f.value) {
        Some(n) if !cx.compact => Some(format!("({n} levels — see Advanced)")),
        _ if cx.compact => None,
        _ => Some(provenance_text(&f.provenance)),
    };
    match &hint_text {
        Some(t) => hint_label(ui, t),
        None => {
            ui.label("");
        }
    }
    hint_and_reset(ui, f.edited, &mut cleared, |_| {});
    if cleared {
        actions.push(Action::Clear(f.field));
    }
    ui.end_row();
    filter_row(ui, cx, actions);
    let e = edge_view(cx.target, cx.job);
    match field_row(ui, cx, &e) {
        Some(RowEdit::Set(v)) => actions.push(Action::Set(e.field, v)),
        Some(RowEdit::Clear) => actions.push(Action::Clear(e.field)),
        None => {}
    }
}

/// The live Filter combo: every option is listed, the ones S-mips-2 bars for this profile disabled with the reason.
fn filter_row(ui: &mut egui::Ui, cx: &Ctx, actions: &mut Vec<Action>) {
    const OPTIONS: [(MipFilter, &str); 4] = FILTER_OPTIONS;
    let f = filter_view(cx.target, cx.job);
    let current = cx.job.profile.mips.filter;
    let locks = OPTIONS.map(|(m, _)| filter_option_lock(&cx.job.profile, m));
    ui.label(f.label);
    let mut picked = None;
    let r = egui::ComboBox::from_id_salt((cx.key, f.field))
        .selected_text(OPTIONS.iter().find(|o| o.0 == current).map_or("", |o| o.1))
        .show_ui(ui, |ui| {
            for ((m, label), lock) in OPTIONS.into_iter().zip(locks) {
                let r = ui.add_enabled(
                    lock.is_none(),
                    egui::Button::selectable(m == current, label),
                );
                match lock {
                    Some(why) => {
                        r.on_disabled_hover_text(why);
                    }
                    None if r.clicked() && m != current => picked = Some(m),
                    None => {}
                }
            }
        })
        .response;
    // Only Box is open: say why on the combo itself.
    let r = match locks[1..]
        .iter()
        .all(Option::is_some)
        .then_some(locks[1])
        .flatten()
    {
        Some(why) => r.on_hover_text(why),
        None => r,
    };
    if let Some(why) = &cx.toml_read_only {
        r.on_disabled_hover_text(why);
    }
    if let Some(m) = picked {
        actions.push(Action::Set(f.field, json!(m)));
    }
    let mut cleared = false;
    hint_and_reset(ui, f.edited, &mut cleared, |ui| {
        if cx.compact {
            ui.label("");
        } else {
            hint_label(ui, &provenance_text(&f.provenance));
        }
    });
    if cleared {
        actions.push(Action::Clear(f.field));
    }
    ui.end_row();
}

fn group(ui: &mut egui::Ui, name: &'static str, cx: &Ctx, actions: &mut Vec<Action>) {
    egui::CollapsingHeader::new(name)
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new((cx.key, name))
                .num_columns(3)
                .show(ui, |ui| {
                    if name == "Mipmaps" {
                        mipmaps_rows(ui, cx, actions);
                    }
                    // Note FC10 (BQ15): an EXR source's Straight alpha ends the Alpha group.
                    let exr = (name == "Alpha")
                        .then(|| crate::model::fields::exr_straight_view(cx.target, cx.job))
                        .flatten();
                    for mut f in field_views(cx.target, cx.job)
                        .into_iter()
                        .filter(|f| {
                            group_of(f.field) == name && !field_hidden(&cx.job.profile, f.field)
                        })
                        .chain(exr)
                    {
                        if f.field == "encoder.backend" && f.lock.is_none() {
                            f.lock = backend_lock(&cx.job.profile, cx.nvtt_found);
                        }
                        match field_row(ui, cx, &f) {
                            Some(RowEdit::Set(v)) => actions.push(Action::Set(f.field, v)),
                            Some(RowEdit::Clear) => actions.push(Action::Clear(f.field)),
                            None => {}
                        }
                    }
                });
            if name == "Compression" {
                if let Some(w) = cx.warnings.iter().find(|w| w.code == Code::FmtHdrSource) {
                    ui.weak(&w.message);
                }
            }
        });
}

fn message_of(warnings: &[Warning], code: Code) -> Option<&str> {
    warnings
        .iter()
        .find(|w| w.code == code)
        .map(|w| w.message.as_str())
}

/// The amber "Undecided" box: one control per decision, each writing an explicit answer.
fn decision_box(
    ui: &mut egui::Ui,
    cx: &Ctx,
    decisions: &[Decision],
    warnings: &[Warning],
    actions: &mut Vec<Action>,
) {
    egui::Frame::new()
        .stroke(egui::Stroke::new(2.0, AMBER))
        .corner_radius(4.0)
        .inner_margin(8.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.colored_label(AMBER, egui::RichText::new("Undecided").strong());
            for d in decisions {
                match d {
                    Decision::Subtype { suggested } => {
                        subtype_decision(ui, cx, *suggested, actions)
                    }
                    Decision::AlphaMeaning => {
                        if let Some(m) = message_of(warnings, Code::SugLowConfidence) {
                            ui.label(m);
                        }
                        ui.label("What does the alpha channel mean?");
                        ui.horizontal_wrapped(|ui| {
                            for (policy, label) in alpha_options()
                                .into_iter()
                                .filter_map(|(v, l)| Some((serde_json::from_value(v).ok()?, l)))
                            {
                                if ui.button(label).on_hover_text(alpha_hint(policy)).clicked() {
                                    actions.push(Action::Decide(Answer::Alpha(policy)));
                                }
                            }
                        });
                    }
                    Decision::CubeLayout => {
                        ui.label("This image could be several cube layouts: pick one");
                        let r = egui::ComboBox::from_id_salt(("pick-cube-layout", cx.key))
                            .selected_text("Pick a layout…")
                            .show_ui(ui, |ui| {
                                for (l, label) in unclear_layouts() {
                                    if ui.selectable_label(false, *label).clicked() {
                                        actions.push(Action::Set("cubemap.layout", json!(l)));
                                    }
                                }
                            })
                            .response;
                        r.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::ComboBox,
                                r.enabled(),
                                "Cube layout",
                            )
                        });
                    }
                    Decision::Acknowledge(code) => {
                        // Note FB8: the sentence, with the code on its hover.
                        ui.label(egui::RichText::new(code_line(*code)).strong())
                            .on_hover_text(code.as_str());
                        if let Some(m) = message_of(warnings, *code) {
                            ui.label(m);
                        }
                        ui.horizontal(|ui| {
                            if ui.button("Acknowledge for this texture").clicked() {
                                actions.push(Action::Decide(Answer::Acknowledge(*code)));
                            }
                            if *code == Code::DimNpot
                                && ui
                                    .button("Resize to a power of two")
                                    .on_hover_text(
                                        "writes resize = pot-down for this texture; DIM-RESIZED reports the new size",
                                    )
                                    .clicked()
                            {
                                actions.push(Action::Set("resize", json!("pot-down")));
                            }
                        });
                    }
                }
                ui.add_space(4.0);
            }
        });
    ui.add_space(6.0);
}

/// A combo of the sub-types available under the target, the suggestion preselected, and a Confirm button.
fn subtype_decision(
    ui: &mut egui::Ui,
    cx: &Ctx,
    suggested: Option<Subtype>,
    actions: &mut Vec<Action>,
) {
    ui.label("What kind of texture is this?");
    let id = egui::Id::new(("confirm-subtype", cx.key));
    let mut chosen: Subtype = ui
        .data(|d| d.get_temp(id))
        .unwrap_or_else(|| suggested.unwrap_or(cx.job.profile.subtype));
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt(id)
            .selected_text(subtype_label(chosen))
            .show_ui(ui, |ui| {
                let grass = engine::policy::grass_context(cx.job);
                for (heading, members) in offered_groups(cx.target, grass) {
                    ui.label(egui::RichText::new(heading).weak());
                    for st in members {
                        ui.selectable_value(&mut chosen, st, subtype_label(st))
                            .on_hover_text(st.id());
                    }
                }
            });
        if ui.button("Confirm").clicked() {
            actions.push(Action::Decide(Answer::Subtype(chosen)));
        }
    });
    ui.data_mut(|d| d.insert_temp(id, chosen));
}

/// One line per packed channel, from the job's `pack.r` … `pack.a` provenance.
fn channels(ui: &mut egui::Ui, job: &ResolvedJob) {
    ui.label(egui::RichText::new("Channels").strong());
    for (ch, name) in [("r", "R"), ("g", "G"), ("b", "B"), ("a", "A")] {
        if let Some(p) = job.provenance.get(&format!("pack.{ch}")) {
            ui.label(format!("{name}: {}", channel_text(&p.to_string())));
        }
    }
    ui.add_space(6.0);
}

/// A failed texture's line (Note FB8: the sentences, not the codes; the codes are the label's hover).
fn error_text(codes: &[Code]) -> String {
    format!(
        "Error: {}",
        codes
            .iter()
            .map(|c| code_line(*c))
            .collect::<Vec<_>>()
            .join("; ")
    )
}

fn error_codes(codes: &[Code]) -> String {
    codes
        .iter()
        .map(|c| c.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Warnings the decision box does not already show.
fn other_warnings(ui: &mut egui::Ui, warnings: &[Warning]) {
    let rest: Vec<&Warning> = warnings
        .iter()
        .filter(|w| {
            !matches!(
                w.code,
                Code::SugLowConfidence
                    | Code::InGreenUnstated
                    | Code::ChkGreenGuess
                    | Code::FmtHdrSource
            )
        })
        .collect();
    if rest.is_empty() {
        return;
    }
    ui.add_space(6.0);
    ui.label(egui::RichText::new("Warnings").strong());
    for w in rest {
        ui.label(egui::RichText::new(&*shown_for(Some(w.code), &w.message)).small())
            .on_hover_ui(|ui| code_hover(ui, &w.message, w.code.as_str()));
    }
}

fn dds_header_panel(
    ui: &mut egui::Ui,
    v: &crate::model::dds_patch::DdsHeaderView,
    confirming: bool,
    last: Option<&str>,
    export_folder_chosen: bool,
    actions: &mut Vec<Action>,
) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new("DDS header").strong());
    if v.issues.is_empty() && matches!(v.patch, Ok(None)) {
        ui.label("Header: valid");
    }
    // Note F?3 (A23): one plain line; the raw header fields are on hover (rule 4.5).
    if !v.issues.is_empty() {
        let patch = if matches!(v.patch, Ok(Some(_))) {
            " Patch header… writes a copy with a fixed header."
        } else {
            ""
        };
        ui.label(
            egui::RichText::new(format!(
                "The DDS header has fields other tools may misread.{patch}"
            ))
            .small(),
        )
        .on_hover_text(v.issues.join("\n"));
    }
    ui.horizontal(|ui| {
        ui.label("Tag on patch:");
        for (label, tag) in [("keep", None), ("sRGB", Some(true)), ("UNORM", Some(false))] {
            if ui.radio(v.tag == tag, label).clicked() {
                actions.push(Action::PatchTag(tag));
            }
        }
    });
    match &v.patch {
        Ok(Some(fixes)) => {
            for f in fixes {
                ui.label(format!("• {f}"));
            }
            if confirming {
                ui.label("Writes a copy with a fixed header to the export folder; the pixel data is copied byte for byte.");
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(export_folder_chosen, egui::Button::new("Patch"))
                        .on_disabled_hover_text(NO_EXPORT_FOLDER)
                        .clicked()
                    {
                        actions.push(Action::PatchDo);
                    }
                    if ui.button("Cancel").clicked() {
                        actions.push(Action::PatchCancel);
                    }
                });
            } else if ui
                .add_enabled(export_folder_chosen, egui::Button::new("Patch header…"))
                .on_disabled_hover_text(NO_EXPORT_FOLDER)
                .clicked()
            {
                actions.push(Action::PatchAsk);
            }
        }
        Ok(None) => {}
        Err(reason) => {
            ui.colored_label(RED, format!("Cannot patch: {reason}"));
        }
    }
    if let Some(l) = last {
        // The result line is kept whole (code first) for the Messages log; the panel shows the words.
        ui.label(egui::RichText::new(without_codes(l)).small())
            .on_hover_text(l);
    }
}

/// Why `sdc.toml` cannot take an edit, when its file is read-only.
/// Item 9: the meshes that use this texture, winner first, from the scan database (read off the UI thread).
fn used_by(ui: &mut egui::Ui, model: &mut AppModel, key: &str) {
    let Some(u) = model.used_by_for(key) else {
        return;
    };
    let title = match &u {
        Some(u) => format!("Used by meshes ({})", u.rows.len() + u.more),
        None => "Used by meshes".to_string(),
    };
    egui::CollapsingHeader::new(title)
        .id_salt("used-by-meshes")
        .show(ui, |ui| {
            let Some(u) = u else {
                ui.weak("Reading the scan database…");
                return;
            };
            if let Some(e) = &u.error {
                ui.colored_label(RED, e);
            }
            if u.rows.is_empty() && u.error.is_none() {
                ui.weak(format!("No mesh of the latest scan uses {}.", u.tex));
            }
            for r in &u.rows {
                let prio = if r.bsa {
                    format!("BSA rank {}", r.priority)
                } else {
                    format!("priority {}", r.priority)
                };
                ui.horizontal_wrapped(|ui| {
                    ui.label(&r.nif);
                    ui.weak(format!("{} · {prio}", r.mod_name));
                    if r.decided_alpha {
                        ui.colored_label(AMBER, "decided alpha").on_hover_text(
                            "This mesh's alpha property decided the texture's alpha meaning",
                        );
                    }
                });
            }
            if u.more > 0 {
                ui.weak(format!("and {} more", u.more));
            }
            if !u.rows.is_empty() && ui.link("Show in Meshes").clicked() {
                model.show_in_meshes = Some(u.tex.clone()); // Note GC: app.rs switches the view
            }
        });
}

pub fn toml_read_only_reason(model: &AppModel) -> Option<String> {
    model.toml_read_only_reason()
}

/// Draw texture `key`'s settings. `compact` (the preview's strip) keeps the decision box and the Alpha, Mipmaps and
/// Compression groups, without provenance text.
pub fn draw(ui: &mut egui::Ui, model: &mut AppModel, key: &str, compact: bool) {
    let Some(item) = model.item(key) else {
        ui.weak("This texture is no longer in the plan.");
        return;
    };
    if let Some(h) = item.helper.clone() {
        return super::helper_panel::draw(ui, model, key, &h);
    }
    let (name, job_key, kind, status) = (
        item.name.clone(),
        item.job_key.clone(),
        item.kind,
        item.status.clone(),
    );
    // The texture's own list shows its check results, Info included (preflight K5, X3), and GEN-STALE (Note CO).
    let (decisions, warnings, job) = (
        item.decisions.clone(),
        model.item_messages(key),
        item.job.clone(),
    );
    let packed_output = match &status {
        ItemStatus::PackedInto(j) => model
            .items
            .iter()
            .find(|i| i.kind == ItemKind::Packed && &i.job_key == j)
            .map(|i| i.key.clone()),
        _ => None,
    };
    let (target, busy) = (model.target, model.busy());
    let read_only = model.project.as_ref().is_some_and(|p| p.read_only());
    let toml_read_only = toml_read_only_reason(model);
    let header = model.dds_header(key);
    let confirming = model.dds_patch.confirming.as_deref() == Some(key);
    let last = model.dds_patch.last_for(key).map(str::to_string);
    let export_folder_chosen = model.export_folder_chosen().is_some();
    // Note F?3: why the re-encode button is off (read-only first), and whether it would write over its source.
    let reencode_off = toml_read_only
        .clone()
        .or_else(|| model.reencode_blocker(key));
    let into_source = kind == ItemKind::Dds
        && job.is_none()
        && engine::dds::same_file(
            &model.item(key).map(|i| i.input.clone()).unwrap_or_default(),
            &model.output_root().join(&job_key),
        );
    let reencoding = model.reencode_on(key);
    let mut actions = vec![];
    if !compact {
        let set_key = model.item(key).and_then(|i| i.set_key.clone());
        if let Some(sk) = set_key.filter(|k| model.sets.iter().any(|s| &s.key == k)) {
            let label = super::columns::set_label(&super::columns::export_prefix(model), &sk);
            if ui.link(format!("← {label}")).clicked() {
                model.select_set(&sk);
            }
        }
        ui.heading(&name);
        ui.weak(if job_key.is_empty() { key } else { &job_key });
        // Note F?3: a DDS re-encoded by `sdc.toml` (it has a job, or failed at plan) offers the way back.
        if reencoding {
            let r = ui
                .add_enabled(
                    toml_read_only.is_none(),
                    egui::Button::new("Stop re-encoding"),
                )
                .on_hover_text(
                    "Leave the DDS file as it is again: it is not written to the export.",
                );
            let r = match &toml_read_only {
                Some(why) => r.on_disabled_hover_text(why),
                None => r,
            };
            if r.clicked() {
                actions.push(Action::Reencode(false));
            }
        }
        // Note DY17: a file the user made "Not a helper" (`helper.role = "none"`) says so, with the way back.
        let not_helper = model
            .project
            .as_ref()
            .and_then(|p| p.file_value(key, engine::helper::ROLE_FIELD))
            .is_some_and(|v| v == json!("none"));
        if not_helper {
            let mut reset = false;
            ui.horizontal(|ui| {
                ui.label("Not a helper (set by you)");
                let r =
                    ui.add_enabled(toml_read_only.is_none(), egui::Button::new("Reset").small());
                r.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, r.enabled(), "Reset role")
                });
                let r = r.on_hover_text("Use the role guessed from the name");
                let r = match &toml_read_only {
                    Some(why) => r.on_disabled_hover_text(why),
                    None => r,
                };
                reset = r.clicked();
            });
            if reset {
                model.set_helper_role(key, None);
            }
        }
        // Note DY17 (docs review): any image can be made a helper here, e.g. one whose name has no helper suffix.
        if kind == ItemKind::Image {
            let picked = ui
                .add_enabled_ui(toml_read_only.is_none(), |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Helper role");
                        super::helper_panel::role_combo(
                            ui,
                            key,
                            engine::helper::HelperRole::NotHelper,
                            not_helper,
                            &toml_read_only,
                        )
                    })
                    .inner
                })
                .inner;
            if let Some(role) = picked {
                model.set_helper_role(key, Some(role));
            }
        }
        ui.add_space(4.0);
    }
    ui.add_enabled_ui(!read_only, |ui| match &job {
        None => {
            match &status {
                ItemStatus::Skipped(r) => ui.label(format!("Skipped: {r}")),
                ItemStatus::Failed(e) => ui.label(format!("Failed: {e}")),
                ItemStatus::PackedInto(j) => ui.label(format!("This map is packed into {j}.")),
                ItemStatus::Error(c) => ui.label(error_text(c)).on_hover_text(error_codes(c)),
                ItemStatus::NeedsDecision | ItemStatus::Ready => ui.label(""),
            };
            if kind == ItemKind::Dds {
                ui.label(
                    "Already a DDS file: it is left as it is and not written to the export.",
                );
                // Note F?3: the S6 `reencode` selector from a button, not a hand edit.
                let mut hover = String::from("Decode it and encode it again with this project's settings. Block-compressed files lose some quality.");
                if into_source {
                    hover.push_str(" It cannot be written into the folder it comes from.");
                }
                let r = ui
                    .add_enabled(reencode_off.is_none(), egui::Button::new("Re-encode this file"))
                    .on_hover_text(hover);
                let r = match &reencode_off {
                    Some(why) => r.on_disabled_hover_text(why),
                    None => r,
                };
                if r.clicked() {
                    actions.push(Action::Reencode(true));
                }
                if let Some(v) = &header {
                    dds_header_panel(
                        ui,
                        v,
                        confirming,
                        last.as_deref(),
                        export_folder_chosen,
                        &mut actions,
                    );
                }
            }
            if let Some(out) = packed_output {
                if ui.button("Show packed output").clicked() {
                    actions.push(Action::Select(out));
                }
            }
        }
        Some(job) => {
            let cx = Ctx {
                key,
                job,
                target,
                busy,
                compact,
                toml_read_only: toml_read_only.clone(),
                warnings: &warnings,
                nvtt_found: model.nvtt_found(),
            };
            match &status {
                ItemStatus::Skipped(r) => {
                    ui.label(format!("Skipped: {r}"));
                }
                ItemStatus::Error(c) => {
                    ui.colored_label(RED, error_text(c))
                        .on_hover_text(error_codes(c));
                }
                _ => {}
            }
            let guess = message_of(&warnings, Code::ChkGreenGuess);
            if let Some(m) = message_of(&warnings, Code::InGreenUnstated) {
                // Note AW: an Info line, not a question; the OpenGL answer stays one click away.
                let hint = if guess.is_some() {
                    " Looks OpenGL (curl test)."
                } else {
                    ""
                };
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(format!("{m}{hint}")).small())
                        .on_hover_ui(|ui| code_hover(ui, m, Code::InGreenUnstated.as_str()));
                    if ui.small_button("Treat as OpenGL (flip green)").clicked() {
                        actions.push(Action::Decide(Answer::Green(GreenConvention::Opengl)));
                    }
                });
            } else if let Some(m) = guess {
                // No AW question was asked: the check's own line, same button, never an auto-flip.
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(m).small())
                        .on_hover_ui(|ui| code_hover(ui, m, Code::ChkGreenGuess.as_str()));
                    if ui.small_button("Treat as OpenGL (flip green)").clicked() {
                        actions.push(Action::Decide(Answer::Green(GreenConvention::Opengl)));
                    }
                });
            } else if job.profile.channels.green_convention == Some(GreenConvention::Opengl) {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Green: OpenGL (flipped)");
                    if matches!(
                        job.provenance.get("channels.green_convention"),
                        Some(Provenance::ProjectFile | Provenance::User)
                    ) {
                        let b = ui.add_enabled(
                            toml_read_only.is_none(),
                            egui::Button::new("Treat as DirectX").small(),
                        );
                        if b.clicked() {
                            actions.push(Action::Clear("channels.green_convention"));
                        }
                    }
                });
            }
            if !decisions.is_empty() {
                decision_box(ui, &cx, &decisions, &warnings, &mut actions);
            }
            if kind == ItemKind::Packed && !compact {
                channels(ui, job);
            }
            if !compact {
                size_line(ui, model, key);
            }
            let shown: &[&str] = if compact { &COMPACT_GROUPS } else { &GROUPS };
            for g in shown {
                group(ui, g, &cx, &mut actions);
            }
            if !compact {
                super::rnm_section::rnm_section(ui, model, key);
                for (k, f, v) in super::cube_section::cube_section(ui, model, key) {
                    match v {
                        Some(v) => model.set_field(&k, &f, v),
                        None => model.clear_field(&k, &f),
                    }
                }
                other_warnings(ui, &warnings);
            }
        }
    });
    if !compact {
        used_by(ui, model, key);
    }
    for a in actions {
        match a {
            Action::Set("format", v) => {
                if let Ok((f, t)) = serde_json::from_value::<(Format, ColorTag)>(v) {
                    model.set_format(key, f, t);
                }
            }
            Action::Set(f, v) => model.set_field(key, f, v),
            Action::Clear(f) => model.clear_field(key, f),
            Action::Decide(a) => model.decide(key, a),
            Action::Select(k) => model.select_item(&k),
            Action::PatchTag(t) => model.set_dds_patch_tag(key, t),
            Action::PatchAsk => model.dds_patch.confirming = Some(key.to_string()),
            Action::PatchCancel => model.dds_patch.confirming = None,
            Action::Reencode(on) => model.set_reencode(&[key.to_string()], on),
            Action::PatchDo => {
                // The result line lands in `dds_patch.last`, shown under the panel.
                let _ = model.patch_dds_header(key);
            }
        }
    }
}

/// Item 22 (DECISIONS AG118): the DDS this texture writes: format, size after any resize, mips, bytes. A compressed
/// size is also its video memory.
fn size_line(ui: &mut egui::Ui, model: &mut AppModel, key: &str) {
    use crate::model::{fields::format_name, out_size::human};
    match model.size_of(key) {
        Some(e) => {
            ui.label(format!(
                "Output: {} · {}×{} · {} mips · {} ≈ VRAM",
                format_name(e.format),
                e.width,
                e.height,
                e.mips,
                human(e.chain_bytes)
            ))
            .on_hover_text(format!(
                "{} bytes of mip data + {} bytes of header = {} bytes on disk",
                e.chain_bytes,
                e.header_bytes,
                e.file_bytes()
            ));
        }
        None => {
            ui.weak("Output size unknown: the source size could not be read.");
        }
    }
}
