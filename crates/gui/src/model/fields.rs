//! Pure field logic for the settings panel (M3a spec §2.4): labels, options, locks, provenance.
use engine::config::*;
use engine::policy::ResolvedJob;
use serde_json::Value;

/// The editable fields in display order, with the label each row shows.
pub const FIELDS: [(&str, &str); 13] = [
    ("subtype", "Sub-type"),
    ("color.content", "Content"),
    ("color.tag", "DDS tag"),
    ("alpha.policy", "Alpha"),
    ("alpha.threshold", "Threshold"),
    ("alpha.compare", "Comparison"),
    ("alpha.exact_opaque", "Exact opaque alpha"),
    ("mips.levels", "Levels"),
    ("mips.normal", "Normal handling"),
    ("mips.coverage", "Coverage"),
    ("format", "Format"),
    ("encoder.effort", "Effort"),
    ("encoder.perceptual", "Perceptual"),
];

/// The rarely changed fields the Advanced drawer edits (M3a spec §2.5).
pub const ADVANCED_FIELDS: [(&str, &str); 5] = [
    ("encoder.perceptual", "Perceptual"),
    ("encoder.bc1_transparent_black", "BC1 transparent black"),
    ("encoder.bc45_search_radius", "BC4/BC5 search radius"),
    ("encoder.rgbcx_level", "rgbcx level"),
    ("header", "DDS header"),
];

pub fn group_of(field: &str) -> &'static str {
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
    }
}

pub fn format_options(target: Target) -> Vec<Format> {
    [
        Format::Bc1,
        Format::Bc3,
        Format::Bc4,
        Format::Bc5,
        Format::Bc6hUf16,
        Format::Bc7,
    ]
    .into_iter()
    .filter(|f| *f != Format::Bc6hUf16 || target == Target::Cs)
    .collect()
}

/// Why a field cannot be changed for this profile, or `None`.
pub fn field_lock(target: Target, p: &Profile, field: &str) -> Option<&'static str> {
    let cutout = matches!(
        p.alpha.policy,
        AlphaPolicy::Cutout | AlphaPolicy::CutoutBlend
    );
    match field {
        "color.tag" if p.format == Format::Bc6hUf16 => Some("BC6H has no _SRGB form"),
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
        "mips.normal" if p.subtype.category() != Category::Normal => Some("normal maps only"),
        "encoder.bc1_transparent_black" if p.format != Format::Bc1 => Some("BC1 only"),
        "encoder.bc45_search_radius" if !matches!(p.format, Format::Bc4 | Format::Bc5) => {
            Some("BC4 and BC5 only")
        }
        "encoder.rgbcx_level" if p.encoder.backend != Backend::Rgbcx => {
            Some("used only by the rgbcx encoder (BC1, BC3, BC4, BC5)")
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

/// The Advanced drawer's fields, with their locks.
pub fn advanced_field_views(target: Target, job: &ResolvedJob) -> Vec<FieldView> {
    views_of(&ADVANCED_FIELDS, target, job)
}

fn views_of(
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
                .unwrap_or(Value::Null);
            let provenance = job
                .provenance
                .get(field)
                .map(ToString::to_string)
                .unwrap_or_else(|| "builtin".into());
            FieldView {
                field,
                label,
                value,
                edited: provenance == "project_file",
                provenance,
                lock: field_lock(target, &job.profile, field),
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
        "fallback" => "fallback sub-type".into(),
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

/// A packed channel's provenance (`source:<file>:R`, `default:<v>`, `fill:<code>`) as a reader sees it.
pub fn channel_text(p: &str) -> String {
    match p.split_once(':') {
        Some(("source", s)) => format!("from {s}"),
        Some(("default", v)) => format!("default value {v}"),
        Some(("fill", v)) => format!("fill value {v}"),
        _ => provenance_text(p),
    }
}
