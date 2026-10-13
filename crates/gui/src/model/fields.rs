//! Pure field logic for the settings panel (M3a spec §2.4): labels, options, locks, provenance.
use engine::config::*;
use engine::policy::{JobStatus, Need, Provenance, ResolvedJob};
use engine::suggest::Confidence;
use serde_json::Value;

/// The editable fields in display order, with the label each row shows.
pub const FIELDS: [(&str, &str); 13] = [
    ("subtype", "Sub-type"),
    ("color.content", "Content"),
    ("color.tag", "DDS tag"),
    ("alpha.policy", "Alpha"),
    ("alpha.threshold", "Threshold"),
    ("alpha.compare", "Comparison"),
    ("mips.coverage", "Coverage"),
    ("alpha.exact_opaque", "Exact opaque alpha"),
    ("mips.normal", "Normal handling"),
    ("mips.normal_base", "Normal base level"),
    ("format", "Format"),
    ("encoder.effort", "Effort"),
    ("encoder.perceptual", "Perceptual"),
];

/// The rarely changed fields the Advanced drawer edits (M3a spec §2.5).
pub const ADVANCED_FIELDS: [(&str, &str); 6] = [
    ("encoder.perceptual", "Perceptual"),
    ("encoder.bc1_transparent_black", "BC1 transparent black"),
    ("encoder.bc45_search_radius", "BC4/BC5 search radius"),
    ("encoder.rgbcx_level", "rgbcx level"),
    ("header", "DDS header"),
    ("resize", "Resize"),
];

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

pub fn format_label(format: Format, tag: ColorTag) -> String {
    let space = if tag == ColorTag::Srgb {
        "sRGB"
    } else {
        "linear"
    };
    match format {
        Format::Bc1 => format!("BC1 · {space}"),
        Format::Bc3 => format!("BC3 · {space}"),
        Format::Bc7 => format!("BC7 · {space}"),
        Format::Bc4 => "BC4 · linear (one channel)".into(),
        Format::Bc5 => "BC5 · linear (two channels)".into(),
        Format::Bc6hUf16 => "BC6H · HDR linear".into(),
        Format::Rgba8 => format!("RGBA8 · {space} (uncompressed)"),
        Format::R8 => "R8 · linear (one channel, uncompressed)".into(),
        Format::Rg8 => "RG8 · linear (two channels, uncompressed)".into(),
        Format::Rgba16f => "RGBA16F · HDR linear (uncompressed)".into(),
    }
}

/// Appended to the label of a current value a picker keeps although the engine does not allow it.
pub const NOT_VALID: &str = " (not valid for this map)";

/// The (format, tag) pairs a format picker lists for `p` under `target`, labelled (owner, 2026-10-13): the pairs
/// `engine::policy::allowed_encodings` allows, then the current pair, marked `NOT_VALID`, when it is not one of them
/// (an old project or a hand edit: shown so it can be changed, never rewritten).
pub fn encoding_options(target: Target, p: &Profile) -> Vec<((Format, ColorTag), String)> {
    let cur = (p.format, p.color.tag);
    let allowed = engine::policy::allowed_encodings(p.subtype, target);
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
    field == "mips.coverage" && !is_cutout(p)
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
        "encoder.rgbcx_level" if p.encoder.backend != Backend::Rgbcx => {
            Some("used only by the rgbcx encoder (BC1, BC3, BC4, BC5)")
        }
        "mips.edge" if p.mips.filter == MipFilter::Box => {
            Some("only tent, Mitchell and Kaiser read the edge")
        }
        _ => None,
    }
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
            let value = profile
                .pointer(&format!("/{}", field.replace('.', "/")))
                .cloned()
                // Defaults the profile does not serialise (F5).
                .unwrap_or_else(|| match field {
                    "mips.edge" => Value::from("wrap"),
                    "resize" => Value::from("none"),
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
                    .then_some("set by the set's Complex grass shader"));
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
