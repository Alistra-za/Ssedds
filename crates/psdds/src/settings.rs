//! Dialog settings → engine profile: the greying rules (spec + plan ruling R1), the quality lists (R4) and the
//! profile overrides (R2, R3).
use engine::config::{
    AlphaPolicy, ColorContent, ColorTag, Effort, Format, MipFilter, MipKeyword, MipLevels, Profile,
    ResizeMode, SearchRadius, Subtype, Target,
};
use engine::policy::default_backend;
use engine::presets::PresetRegistry;
use std::ffi::CStr;
use std::sync::OnceLock;

/// The Save dialog's values. Every field is an index; see `photoshop/psdds.h`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PsddsSettings {
    /// 0 BC1, 1 BC3, 2 BC4, 3 BC5, 4 BC7, 5 RGBA8.
    pub format: u32,
    /// 0 Colour, 1 Normal, 2 Linear mask.
    pub texture_type: u32,
    /// 0 off, 1 on.
    pub srgb: u32,
    /// 0 Box, 1 Tent, 2 Mitchell, 3 Kaiser.
    pub filter: u32,
    /// 0 none, 1 full chain.
    pub mips: u32,
    /// Index into the format's quality list (`psdds_quality_label`); 0 where the list is empty.
    pub quality: u32,
}

/// Which dialog controls are editable (1) or greyed (0).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PsddsControls {
    pub srgb_enabled: u32,
    pub filter_enabled: u32,
    pub quality_enabled: u32,
}

const FORMATS: [Format; 6] = [
    Format::Bc1,
    Format::Bc3,
    Format::Bc4,
    Format::Bc5,
    Format::Bc7,
    Format::Rgba8,
];
const SUBTYPES: [Subtype; 3] = [Subtype::Diffuse, Subtype::Normal, Subtype::GenericData];
const FILTERS: [MipFilter; 4] = [
    MipFilter::Box,
    MipFilter::Tent,
    MipFilter::Mitchell,
    MipFilter::Kaiser,
];
const FORMAT_BC7: u32 = 4;
/// BC7 Veryslow, the `sdc-base` effort: `defaults`' value when the presets cannot be read (never in practice).
pub(crate) const FALLBACK_QUALITY: u32 = 4;

// The GUI's per-format quality values (crates/gui/src/ui/settings_panel.rs `options_of` and the rgbcx slider),
// copied here because psdds must not depend on the GPL-only gui crate.
const EFFORTS: [(Effort, &CStr); 7] = [
    (Effort::Ultrafast, c"Ultrafast"),
    (Effort::Veryfast, c"Veryfast"),
    (Effort::Fast, c"Fast"),
    (Effort::Basic, c"Basic"),
    (Effort::Veryslow, c"Veryslow"),
    (Effort::Slowest, c"Slowest"),
    (Effort::Max, c"Max"),
];
const RADII: [(SearchRadius, &CStr); 3] = [
    (SearchRadius::Default, c"Default"),
    (SearchRadius::Max, c"Maximum"),
    (SearchRadius::Fast, c"Fast (preview)"),
];
// shortcut: 0..=18 is encoders_sys::RGBCX_MAX_LEVEL, which psdds cannot name (engine-only dependency); the
// `quality_lists_are_pinned` test fails if the GUI's slider range would need a new entry here.
const RGBCX_LEVELS: [&CStr; 19] = [
    c"0", c"1", c"2", c"3", c"4", c"5", c"6", c"7", c"8", c"9", c"10", c"11", c"12", c"13", c"14",
    c"15", c"16", c"17", c"18",
];

/// The quality list of a format (by index); empty for RGBA8 and an unknown format.
pub fn quality_labels(format: u32) -> Vec<&'static CStr> {
    match format {
        0 | 1 => RGBCX_LEVELS.to_vec(),
        2 | 3 => RADII.iter().map(|r| r.1).collect(),
        4 => EFFORTS.iter().map(|e| e.1).collect(),
        _ => vec![],
    }
}

/// The `cs-quality` profile each texture type starts from (R3).
fn base(texture_type: u32) -> Result<&'static Profile, String> {
    static BASES: OnceLock<Result<Vec<Profile>, String>> = OnceLock::new();
    let bases = BASES.get_or_init(|| {
        let reg = PresetRegistry::builtin().map_err(|e| e.to_string())?;
        SUBTYPES
            .iter()
            .map(|&st| {
                reg.profile("cs-quality", Target::Cs, st)
                    .map_err(|e| e.to_string())
            })
            .collect()
    });
    let bases = bases.as_ref().map_err(Clone::clone)?;
    bases
        .get(texture_type as usize)
        .ok_or_else(|| format!("texture type {texture_type} out of range"))
}

fn filter_index(f: MipFilter) -> u32 {
    FILTERS.iter().position(|&m| m == f).unwrap_or(0) as u32
}

/// The base profile's value as an index into the format's quality list (R4 default).
fn preset_quality(format: u32, p: &Profile) -> u32 {
    let at = |i: Option<usize>| i.unwrap_or(0) as u32;
    match format {
        0 | 1 => p.encoder.rgbcx_level.min(RGBCX_LEVELS.len() as u32 - 1),
        2 | 3 => at(RADII
            .iter()
            .position(|r| r.0 == p.encoder.bc45_search_radius)),
        4 => at(EFFORTS.iter().position(|e| e.0 == p.encoder.effort)),
        _ => 0,
    }
}

/// The dialog's start values for a texture type: BC7, sRGB on for Colour, the preset's filter, full mips and the
/// preset's quality. An unknown type gets Colour's.
pub fn defaults(texture_type: u32) -> PsddsSettings {
    let ty = if texture_type < 3 { texture_type } else { 0 };
    let (filter, quality) = base(ty).map_or((0, FALLBACK_QUALITY), |p| {
        (filter_index(p.mips.filter), preset_quality(FORMAT_BC7, p))
    });
    PsddsSettings {
        format: FORMAT_BC7,
        texture_type: ty,
        srgb: u32::from(ty == 0),
        filter,
        mips: 1,
        quality,
    }
}

/// The greying rules: the values the dialog shows and the encoder uses, and which controls are editable.
pub fn resolve(
    s: &PsddsSettings,
    has_alpha: bool,
) -> Result<(PsddsSettings, PsddsControls), String> {
    let range = |name: &str, v: u32, n: u32| {
        if v < n {
            Ok(())
        } else {
            Err(format!("{name} {v} out of range 0..{n}"))
        }
    };
    range("format", s.format, 6)?;
    range("texture type", s.texture_type, 3)?;
    range("sRGB", s.srgb, 2)?;
    range("filter", s.filter, 4)?;
    range("mips", s.mips, 2)?;
    let count = quality_labels(s.format).len() as u32;
    let colour = s.texture_type == 0;
    // R1: only BC1, BC3, BC7 and RGBA8 have an _SRGB form.
    let srgb_capable = matches!(s.format, 0 | 1 | 4 | 5);
    let mut controls = PsddsControls {
        srgb_enabled: u32::from(colour && srgb_capable),
        filter_enabled: u32::from(colour),
        quality_enabled: u32::from(count > 0),
    };
    let mut effective = PsddsSettings {
        srgb: if controls.srgb_enabled == 1 {
            s.srgb
        } else {
            0
        },
        filter: if colour {
            s.filter
        } else {
            filter_index(base(s.texture_type)?.mips.filter)
        },
        // DC8: an index outside the format's list (remembered from another format) falls back to the preset's.
        quality: if s.quality < count {
            s.quality
        } else {
            preset_quality(s.format, base(s.texture_type)?)
        },
        ..*s
    };
    // DC12: where the engine's own rule would mip with box (linear content, alpha-weighted Blend mips), the
    // filter greys and shows Box. Probed with Kaiser, as every non-box filter shares the rule outside HDR.
    if colour {
        let probe = build(
            &PsddsSettings {
                filter: 3,
                ..effective
            },
            has_alpha,
        )?;
        if engine::filter::filter_allowed(&probe).is_err() {
            controls.filter_enabled = 0;
            effective.filter = filter_index(MipFilter::Box);
        }
    }
    Ok((effective, controls))
}

/// The engine profile for the dialog's values (R2–R4), after the greying rules.
pub fn profile_for(s: &PsddsSettings, has_alpha: bool) -> Result<Profile, String> {
    build(&resolve(s, has_alpha)?.0, has_alpha)
}

/// The profile for settings that are already resolved.
fn build(e: &PsddsSettings, has_alpha: bool) -> Result<Profile, String> {
    let mut p = base(e.texture_type)?.clone();
    p.format = FORMATS[e.format as usize];
    p.encoder.backend = default_backend(p.format);
    // DC9: the sRGB control sets content and tag together; a preset with tag n/a keeps its colour fields.
    if p.color.tag != ColorTag::NotApplicable {
        (p.color.content, p.color.tag) = if e.srgb == 1 {
            (ColorContent::Srgb, ColorTag::Srgb)
        } else {
            (ColorContent::Linear, ColorTag::Unorm)
        };
    }
    p.mips.filter = FILTERS[e.filter as usize];
    p.mips.levels = MipLevels::Keyword(if e.mips == 1 {
        MipKeyword::Full
    } else {
        MipKeyword::None
    });
    // R2: keep alpha only where the document has it and the format stores it.
    p.alpha.policy = if has_alpha && matches!(e.format, 1 | 4 | 5) {
        AlphaPolicy::Blend
    } else {
        AlphaPolicy::Remove
    };
    match e.format {
        0 | 1 => p.encoder.rgbcx_level = e.quality,
        2 | 3 => p.encoder.bc45_search_radius = RADII[e.quality as usize].0,
        4 => p.encoder.effort = EFFORTS[e.quality as usize].0,
        _ => {}
    }
    p.resize = ResizeMode::None;
    p.drop_top_mip = false;
    Ok(p)
}
