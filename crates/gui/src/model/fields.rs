//! Pure field logic for the settings panel (M3a spec §2.4): labels, options, locks, provenance.
use engine::config::*;
use engine::policy::{JobStatus, Need, Provenance, ResolvedJob};
use engine::suggest::Confidence;
use serde_json::Value;

/// The editable fields in display order, with the label each row shows.
pub const FIELDS: [(&str, &str); 15] = [
    ("subtype", "Sub-type"),
    ("color.content", "Content"),
    ("color.tag", "DDS tag"),
    ("alpha.policy", "Alpha"),
    ("alpha.threshold", "Threshold"),
    ("alpha.compare", "Comparison"),
    ("mips.coverage", "Coverage"),
    ("alpha.exact_opaque", "Exact opaque alpha"),
    ("alpha.parallax", "Enable parallax"),
    ("mips.normal", "Normal handling"),
    ("mips.normal_base", "Normal base level"),
    ("format", "Format"),
    ("encoder.effort", "Effort"),
    ("encoder.backend", "Encoder"),
    (WEIGHTING, "BC7 weighting"),
];

/// The BC7 weighting row (DECISIONS Note CP): one dropdown over `encoder.perceptual` and `encoder.bc7_weights`.
pub const WEIGHTING: &str = "encoder.bc7_weights";

/// The BC7 weighting row's options: the row's value, its label, its hover text.
pub const WEIGHTING_OPTIONS: [(&str, &str, &str); 5] = [
    ("linear", "Linear", "Linear: every channel counts the same (the default)."),
    (
        "perceptual",
        "Perceptual",
        "Perceptual: bc7e's own weights, colour error as the eye sees it; colour maps without data alpha only.",
    ),
    (
        "alpha2",
        "Alpha ×2",
        "Alpha ×2: a light lean towards alpha accuracy, for little colour cost.",
    ),
    (
        "alpha8",
        "Alpha ×8",
        "Alpha ×8: favours alpha accuracy over colour; an alternative to BC3 for delicate alpha. Costs about 1 dB of RGB per 3 dB of alpha.",
    ),
    (
        "alpha16",
        "Alpha ×16",
        "Alpha ×16: the strongest alpha priority; best for a _n with specular in alpha.",
    ),
];

/// Why the BC7 weighting option `v` cannot be picked for `p`: Perceptual keeps the old Perceptual row's lock.
pub fn weighting_option_lock(target: Target, p: &Profile, v: &str) -> Option<&'static str> {
    (v == "perceptual")
        .then(|| field_lock(target, p, "encoder.perceptual"))
        .flatten()
}

/// What picking `pick` on the BC7 weighting row writes into a texture's own entry, as (`encoder.perceptual`,
/// `encoder.bc7_weights`); `None` clears that key. `p` is the texture's resolved profile, `own_*` its own values.
/// Note CP10: an inherited perceptual or weighting is overridden explicitly (`perceptual = false`, `bc7_weights =
/// "linear"`); with nothing inherited, Linear clears both. An own `false` / `"linear"` is such an override, so it
/// stands for an inherited value. Perceptual sets `true` and clears the weights.
pub fn weighting_writes(
    pick: &str,
    p: &Profile,
    own_p: Option<&Value>,
    own_w: Option<&Value>,
) -> (Option<Value>, Option<Value>) {
    let inherited_p = match own_p {
        None => p.encoder.perceptual,
        Some(v) => *v == Value::Bool(false),
    };
    let inherited_w = match own_w {
        None => p
            .encoder
            .bc7_weights
            .is_some_and(|w| w != engine::config::Bc7Weights::Linear),
        Some(v) => v.as_str() == Some("linear"),
    };
    match pick {
        "perceptual" => (Some(Value::Bool(true)), None),
        "linear" if inherited_p || inherited_w => {
            (Some(Value::Bool(false)), Some(Value::from("linear")))
        }
        "linear" => (None, None),
        w => (
            inherited_p.then_some(Value::Bool(false)),
            Some(Value::from(w)),
        ),
    }
}

/// The BC7 weighting row's value for `p`: perceptual wins (bc7e then uses its own weights), else the weights, else
/// `"linear"`.
pub fn weighting_value(p: &Profile) -> Value {
    if p.encoder.perceptual {
        return Value::from("perceptual");
    }
    p.encoder
        .bc7_weights
        .and_then(|w| serde_json::to_value(w).ok())
        .unwrap_or_else(|| Value::from("linear"))
}

/// The rarely changed fields the Advanced drawer edits (M3a spec §2.5).
pub const ADVANCED_FIELDS: [(&str, &str); 7] = [
    (WEIGHTING, "BC7 weighting"),
    ("encoder.bc1_transparent_black", "BC1 transparent black"),
    ("encoder.bc45_search_radius", "BC4/BC5 search radius"),
    ("encoder.rgbcx_level", "rgbcx level"),
    ("header", "DDS header"),
    ("resize", "Resize"),
    // Note FC2 (CL15): "Half size" is the one name; "mip 0" is hover text.
    ("drop_top_mip", "Half size"),
];

/// The cubemap section's fields in display order (Note DT7), in the groups `ui::cube_section` draws them in.
pub const CUBE_FIELDS: [(&str, &str); 18] = [
    ("cubemap.layout", "Layout"),
    ("cubemap.face_size", "Face size"),
    ("cubemap.vcross_negz_rotate", "Turn -Z upright"),
    ("cubemap.yaw", "Yaw"),
    ("cubemap.mirror", "Mirror east and west"),
    ("cubemap.face_rotate", "Face rotation"),
    ("cubemap.face_flip", "Face flip"),
    ("cubemap.exposure", "Exposure"),
    ("cubemap.brightness", "Brightness"),
    ("cubemap.contrast", "Contrast"),
    ("cubemap.midtones", "Midtones"),
    ("cubemap.saturation", "Saturation"),
    ("cubemap.tint", "Tint"),
    ("cubemap.highlight_clamp", "Highlight clamp"),
    ("cubemap.ground_fill", "Ground fill"),
    ("cubemap.horizon", "Horizon"),
    ("cubemap.style", "Style"),
    ("cubemap.blur", "Blur"),
];

/// Why the horizon row is disabled without a ground fill (the engine reads the horizon only with one).
pub const NEEDS_GROUND_FILL: &str = "needs a ground fill";

/// The cubemap section's field views (value, provenance, own entry, lock).
pub fn cube_views(target: Target, job: &ResolvedJob) -> Vec<FieldView> {
    views_of(&CUBE_FIELDS, target, job)
}

/// The mip level count: shown as the basic Mipmaps tick box, edited as a count in the Advanced drawer.
pub const LEVELS_FIELD: (&str, &str) = ("mips.levels", "Levels");
/// The mip filter: a live combo in the basic Mipmaps group, each option locked by `filter_option_lock`.
pub const FILTER_FIELD: (&str, &str) = ("mips.filter", "Filter");
pub const EDGE_FIELD: (&str, &str) = ("mips.edge", "Edge");

pub fn group_of(field: &str) -> &'static str {
    // Coverage only ever applies to the alpha-tested policies, so it sits with the alpha settings.
    if field == "mips.coverage" {
        return "Alpha";
    }
    match field.split('.').next().unwrap_or("") {
        "subtype" => "Type",
        "color" => "Colour",
        "alpha" => "Alpha",
        "mips" => "Mipmaps",
        _ => "Compression",
    }
}

/// "BC7": the one format-name table (AG130); `format_label` and the output size line read it.
pub fn format_name(format: Format) -> &'static str {
    match format {
        Format::Bc1 => "BC1",
        Format::Bc3 => "BC3",
        Format::Bc7 => "BC7",
        Format::Bc4 => "BC4",
        Format::Bc5 => "BC5",
        Format::Bc6hUf16 => "BC6H",
        Format::Rgba8 => "RGBA8",
        Format::R8 => "R8",
        Format::Rg8 => "RG8",
        Format::Rgba16f => "RGBA16F",
    }
}

/// "BC7 · sRGB": the name and what the format stores.
pub fn format_label(format: Format, tag: ColorTag) -> String {
    let space = if tag == ColorTag::Srgb {
        "sRGB"
    } else {
        "linear"
    };
    let what = match format {
        Format::Bc1 | Format::Bc3 | Format::Bc7 => space.to_string(),
        Format::Bc4 => "linear (one channel)".into(),
        Format::Bc5 => "linear (two channels)".into(),
        Format::Bc6hUf16 => "HDR linear".into(),
        Format::Rgba8 => format!("{space} (uncompressed)"),
        Format::R8 => "linear (one channel, uncompressed)".into(),
        Format::Rg8 => "linear (two channels, uncompressed)".into(),
        Format::Rgba16f => "HDR linear (uncompressed)".into(),
    };
    format!("{} · {what}", format_name(format))
}

/// Appended to the label of a current value a picker keeps although the engine does not allow it.
pub const NOT_VALID: &str = " (not valid for this map)";

/// The (format, tag) pairs a format picker lists for `p` under `target`, labelled (owner, 2026-10-13): the pairs
/// `engine::policy::allowed_encodings_for` allows (a grey map judged as one, #31), then the current pair, marked
/// `NOT_VALID`, when it is not one of them
/// (an old project or a hand edit: shown so it can be changed, never rewritten).
pub fn encoding_options(target: Target, p: &Profile) -> Vec<((Format, ColorTag), String)> {
    let cur = (p.format, p.color.tag);
    let allowed = engine::policy::allowed_encodings_for(
        p.subtype,
        target,
        p.category == engine::config::Category::Grayscale,
    );
    let stray = (!allowed.contains(&cur))
        .then(|| (cur, format!("{}{NOT_VALID}", format_label(cur.0, cur.1))));
    allowed
        .into_iter()
        .map(|(f, t)| ((f, t), format_label(f, t)))
        .chain(stray)
        .collect()
}

/// True for the alpha-tested policies (`cutout`, `cutout_blend`).
pub fn is_cutout(p: &Profile) -> bool {
    matches!(
        p.alpha.policy,
        AlphaPolicy::Cutout | AlphaPolicy::CutoutBlend
    )
}

/// A field the panel leaves out entirely for this profile (coverage outside the cutout policies).
pub fn field_hidden(p: &Profile, field: &str) -> bool {
    hidden_reason(p, field).is_some()
}

/// Why `field_hidden` leaves `field` out, per field (bulk reports it as the skip reason), or `None`.
pub fn hidden_reason(p: &Profile, field: &str) -> Option<&'static str> {
    match field {
        "mips.coverage" if !is_cutout(p) => Some("used only by cutout and cutout + blend"),
        "encoder.backend" if !p.format.is_block() => {
            Some("no encoder runs for uncompressed formats")
        }
        // Note CP: the weights reach bc7e only; the engine warns ENC-WEIGHTS-IGNORED anywhere else.
        WEIGHTING if p.format != Format::Bc7 => Some("BC7 only"),
        WEIGHTING if p.encoder.backend != Backend::Bc7e => {
            Some("used only by the built-in BC7 encoder")
        }
        "alpha.parallax" if p.subtype != Subtype::ComplexMaterial => {
            Some("complex material _m only")
        }
        // DECISIONS AH2: an explicit `remove` writes 255, so parallax has nothing to switch.
        "alpha.parallax" if p.alpha.policy == AlphaPolicy::Remove => Some("alpha removed: 255"),
        _ => None,
    }
}

/// The Encoder row's lock when NVTT was not found (DECISIONS Note NV): a texture already on NVTT keeps the row open
/// so it can go back to the built-in encoder.
pub const NVTT_NOT_FOUND: &str = "NVTT not found: set its path in Advanced";

/// Why the Encoder row cannot offer NVTT now, beyond `field_lock` (the format), or `None`.
pub fn backend_lock(p: &Profile, nvtt_found: bool) -> Option<&'static str> {
    (p.encoder.backend != Backend::Nvtt && !nvtt_found).then_some(NVTT_NOT_FOUND)
}

/// Why a field cannot be changed for this profile, or `None`.
pub fn field_lock(target: Target, p: &Profile, field: &str) -> Option<&'static str> {
    let cutout = is_cutout(p);
    match field {
        "color.tag" if p.format == Format::Bc6hUf16 => Some("BC6H has no _SRGB form"),
        "color.tag" if p.format == Format::Rgba16f => Some("float formats have no _SRGB form"),
        "color.tag" if matches!(p.format, Format::R8 | Format::Rg8) => {
            Some("R8 and RG8 have no _SRGB form")
        }
        "encoder.effort" | "encoder.perceptual" if !p.format.is_block() => {
            Some("no encoder runs for uncompressed formats")
        }
        "color.tag" if target != Target::Cs => Some("_SRGB is only written under cs"),
        // M6: one level has no mip 1 to write from (the engine ignores the field there, Note CL23).
        "drop_top_mip"
            if matches!(
                p.mips.levels,
                MipLevels::Keyword(MipKeyword::None) | MipLevels::Count(0..=1)
            ) =>
        {
            Some("No mips to drop")
        }
        "encoder.perceptual"
            if !(p.subtype.category() == Category::Color
                && p.alpha.policy != AlphaPolicy::Data
                && p.format == Format::Bc7) =>
        {
            Some("perceptual BC7 is only for colour without data alpha")
        }
        "alpha.threshold" | "alpha.compare" if !cutout => {
            Some("used only by cutout and cutout + blend")
        }
        "mips.normal" | "mips.normal_base" if p.subtype.category() != Category::Normal => {
            Some("normal maps only")
        }
        "encoder.bc1_transparent_black" if p.format != Format::Bc1 => Some("BC1 only"),
        "encoder.bc45_search_radius" if !matches!(p.format, Format::Bc4 | Format::Bc5) => {
            Some("BC4 and BC5 only")
        }
        "encoder.backend" if !engine::policy::backend_fits(Backend::Nvtt, p.format) => {
            Some("NVTT encodes BC7 and BC6H only")
        }
        "encoder.rgbcx_level" if p.encoder.backend != Backend::Rgbcx => {
            Some("used only by the rgbcx encoder (BC1, BC3, BC4, BC5)")
        }
        // Note CP (fix round 1, I2): the Advanced drawer shows the row everywhere; lock it where it does nothing.
        WEIGHTING => hidden_reason(p, WEIGHTING),
        "mips.edge" if p.mips.filter == MipFilter::Box => {
            Some("only tent, Mitchell and Kaiser read the edge")
        }
        "cubemap.horizon" if p.cubemap.ground_fill.is_none() => Some(NEEDS_GROUND_FILL),
        _ => None,
    }
}

/// Hover text of the "Enable parallax" check box (DECISIONS AH2).
pub const PARALLAX_HOVER: &str = "Off writes alpha 0: complex material on, parallax off";

/// Why a complex `_m` cannot turn parallax on (DECISIONS AH2): its source has no alpha channel. Read from the
/// resolve's analysis fact, or its ALPHA-PARALLAX-NO-ALPHA warning once the texture's own entry turned it on.
pub fn parallax_lock(job: &ResolvedJob) -> Option<&'static str> {
    let no_alpha = job
        .provenance
        .get("alpha.parallax")
        .is_some_and(|p| p.to_string() == "analysis:no-alpha")
        || job
            .warnings
            .iter()
            .any(|w| w.code == engine::codes::Code::AlphaParallaxNoAlpha);
    no_alpha.then_some("the source has no alpha channel: alpha 0 is written")
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldView {
    pub field: &'static str,
    pub label: &'static str,
    pub value: Value,
    pub provenance: String,
    /// The value comes from this texture's own `[files]` entry (so it can be reset).
    pub edited: bool,
    pub lock: Option<&'static str>,
}

pub fn field_views(target: Target, job: &ResolvedJob) -> Vec<FieldView> {
    views_of(&FIELDS, target, job)
}

/// The mip level count's view (value `"full"`, `"none"` or a number).
pub fn levels_view(target: Target, job: &ResolvedJob) -> FieldView {
    views_of(&[LEVELS_FIELD], target, job).remove(0)
}

/// The mip filter's view.
pub fn filter_view(target: Target, job: &ResolvedJob) -> FieldView {
    views_of(&[FILTER_FIELD], target, job).remove(0)
}

/// The mip filter's edge handling view.
pub fn edge_view(target: Target, job: &ResolvedJob) -> FieldView {
    views_of(&[EDGE_FIELD], target, job).remove(0)
}

/// Why `f` cannot be picked for this profile, or None (S-mips-2, via `engine::filter::filter_allowed`).
pub fn filter_option_lock(p: &Profile, f: MipFilter) -> Option<&'static str> {
    let mut q = p.clone();
    q.mips.filter = f;
    engine::filter::filter_allowed(&q).err()
}

/// Whether the level value means mipmaps are on: anything but `"none"`.
pub fn mips_on(levels: &Value) -> bool {
    levels.as_str() != Some("none")
}

/// The level count, when the value is a number rather than `"full"` or `"none"`.
pub fn mips_count(levels: &Value) -> Option<u64> {
    levels.as_u64()
}

/// The Advanced drawer's fields, with their locks.
pub fn advanced_field_views(target: Target, job: &ResolvedJob) -> Vec<FieldView> {
    views_of(&ADVANCED_FIELDS, target, job)
}

/// Every field the settings panel and the Advanced drawer edit, each once, in display order (F3a: bulk lookups and
/// copy-from-active).
pub fn all_fields() -> Vec<(&'static str, &'static str)> {
    let mut v = FIELDS.to_vec();
    v.extend([LEVELS_FIELD, FILTER_FIELD, EDGE_FIELD]);
    v.extend(
        ADVANCED_FIELDS
            .into_iter()
            .filter(|(f, _)| !FIELDS.iter().any(|(g, _)| g == f)),
    );
    v
}

/// One field's view, its lock included; `None` for a field `all_fields` does not list.
pub fn field_view(target: Target, job: &ResolvedJob, field: &str) -> Option<FieldView> {
    let entry = all_fields().into_iter().find(|(f, _)| *f == field)?;
    views_of(&[entry], target, job).pop()
}

pub(crate) fn views_of(
    fields: &[(&'static str, &'static str)],
    target: Target,
    job: &ResolvedJob,
) -> Vec<FieldView> {
    let profile = serde_json::to_value(&job.profile).unwrap_or_default();
    fields
        .iter()
        .map(|&(field, label)| {
            if field == WEIGHTING {
                return weighting_view(target, job, label);
            }
            let value = profile
                .pointer(&format!("/{}", field.replace('.', "/")))
                .cloned()
                // Defaults the profile does not serialise (F5).
                .unwrap_or_else(|| match field {
                    "mips.edge" => Value::from("wrap"),
                    "resize" => Value::from("none"),
                    "drop_top_mip" => Value::from(false),
                    _ => Value::Null,
                });
            let provenance = job
                .provenance
                .get(field)
                .map(ToString::to_string)
                .unwrap_or_else(|| "builtin".into());
            // The set's shader decided it: change the shader, not the row.
            let lock = field_lock(target, &job.profile, field)
                .or((provenance == "shader:complex_grass" && field == "subtype")
                    .then_some("set by the set's Complex grass shader"))
                .or((field == "alpha.parallax")
                    .then(|| parallax_lock(job))
                    .flatten());
            FieldView {
                field,
                label,
                value,
                edited: provenance == "project_file",
                provenance,
                lock,
            }
        })
        .collect()
}

/// The BC7 weighting row's view: its value and provenance come from whichever of its two fields decides it, and
/// either one set in the texture's own entry marks it edited (Note CP).
fn weighting_view(target: Target, job: &ResolvedJob, label: &'static str) -> FieldView {
    let prov = |f: &str| job.provenance.get(f).map(ToString::to_string);
    let e = &job.profile.encoder;
    let decided_by = if !e.perceptual && e.bc7_weights.is_some() {
        WEIGHTING
    } else {
        "encoder.perceptual"
    };
    FieldView {
        field: WEIGHTING,
        label,
        value: weighting_value(&job.profile),
        provenance: prov(decided_by).unwrap_or_else(|| "builtin".into()),
        edited: [WEIGHTING, "encoder.perceptual"]
            .iter()
            .any(|f| prov(f).as_deref() == Some("project_file")),
        lock: field_lock(target, &job.profile, WEIGHTING),
    }
}

/// Provenance as a reader sees it.
pub fn provenance_text(p: &str) -> String {
    match p {
        "project_file" => "from this texture's settings".into(),
        "user" => "from the command line".into(),
        "builtin" => "built-in default".into(),
        "project_config" => "from sdc.toml".into(),
        "user_config" => "from your config.toml".into(),
        "suggestion" => "suggested".into(),
        "analysis" => "from the image analysis".into(),
        "fallback" => "fallback value (sub-type or box filter)".into(),
        "set" => "decided by the texture set (variance bake)".into(),
        "mesh:alpha" => "from the meshes that use this texture".into(),
        _ => match p.split_once(':') {
            Some(("preset", id)) => format!("from preset {id}"),
            Some(("rule", r)) => format!("from rule {r}"),
            Some(("analysis", fact)) => format!("from the image analysis ({fact})"),
            Some(("suffix", term)) => format!("from the file name ({term})"),
            Some(("recipe", kind)) => format!("from the {kind} recipe"),
            Some((k, v)) => format!("{k}: {v}"),
            None => p.to_string(),
        },
    }
}

/// M6 Track E spec §2 (DECISIONS Note U4): the reason of the High suggestion that decided `job`'s sub-type, shown as
/// "(auto)" with this hover text. `None` when the user, a rule or a shader chose it, or it still asks.
pub fn auto_subtype_reason(job: &ResolvedJob) -> Option<&str> {
    let top = job.suggestions.first()?;
    let asks = matches!(&job.status, JobStatus::Unresolved { needs, .. } if needs.contains(&Need::Subtype));
    (job.provenance.get("subtype") == Some(&Provenance::Suggestion)
        && top.confidence == Confidence::High
        && !asks)
        .then_some(top.reason.as_str())
}

/// A packed channel's provenance (`source:<file>:R`, `default:<v>`, `fill:<code>`) as a reader sees it.
pub fn channel_text(p: &str) -> String {
    match p.split_once(':') {
        Some(("source", s)) => format!("from {s}"),
        Some(("default", v)) => format!("default value {v}"),
        Some(("fill", v)) => format!("fill value {v}"),
        _ => provenance_text(p),
    }
}
