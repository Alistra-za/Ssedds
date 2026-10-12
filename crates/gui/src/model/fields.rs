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

pub fn format_options(target: Target) -> Vec<Format> {
    [
        Format::Bc1,
        Format::Bc3,
        Format::Bc4,
        Format::Bc5,
        Format::Bc6hUf16,
        Format::Bc7,
        Format::Rgba8,
        Format::R8,
        Format::Rg8,
        Format::Rgba16f,
    ]
    .into_iter()
    .filter(|f| !f.is_hdr() || target == Target::Cs)
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
        "set" => "decided by the texture set (variance bake)".into(),
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
