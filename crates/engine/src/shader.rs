//! M6 Track A (spec 2026-10-11-sdc-m6-shader-modes-design.md, DECISIONS Note S): the per-set shader table as data, auto-pick, member defaults and the extra/missing map check. The CLI and GUI read it; neither duplicates it.
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::path::Path;
use std::str::FromStr;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::codes::{Code, Warning};
use crate::config::{AlphaPolicy, Subtype, Target, ALPHA_THRESHOLD};
use crate::suggest::has_component;

/// A Skyrim shader a texture set is made for (spec §3).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Shader {
    Pbr,
    // Community Shaders PBR grass (PBR grass design): `RunGrass.hlsl` reads albedo, `_n`, `_rmaos` and `_s` only.
    PbrGrass,
    // Community Shaders PBR landscape (Note BM): a PBR texture set drawn by the landscape shader; CS only.
    PbrLandscape,
    AdvancedSkin,
    Default,
    Envmap,
    ComplexMaterial,
    Glow,
    Parallax,
    Skin,
    Hair,
    Eye,
    Multilayer,
    Landscape,
    ComplexGrass,
}

/// One map slot of a shader: a member fills it when its sub-type and suffix match (spec §3).
#[derive(Debug)]
pub struct MapRole {
    pub label: &'static str,
    pub subtypes: &'static [Subtype],
    pub suffixes: &'static [&'static str],
}

impl MapRole {
    /// True when each non-empty list contains the member's value; a role with both lists empty never matches.
    pub fn matches(&self, suffix: Option<&str>, subtype: Option<Subtype>) -> bool {
        (!self.subtypes.is_empty() || !self.suffixes.is_empty())
            && (self.subtypes.is_empty() || subtype.is_some_and(|t| self.subtypes.contains(&t)))
            && (self.suffixes.is_empty() || suffix.is_some_and(|s| self.suffixes.contains(&s)))
    }
}

/// The maps a shader needs and the ones it can also read.
#[derive(Debug)]
pub struct ShaderSpec {
    pub required: &'static [MapRole],
    pub optional: &'static [MapRole],
}

const fn role(label: &'static str, subtypes: &'static [Subtype]) -> MapRole {
    MapRole {
        label,
        subtypes,
        suffixes: &[],
    }
}
const fn sfx(
    label: &'static str,
    subtypes: &'static [Subtype],
    suffixes: &'static [&'static str],
) -> MapRole {
    MapRole {
        label,
        subtypes,
        suffixes,
    }
}

const D: &[Subtype] = &[Subtype::Diffuse];
const N: &[Subtype] = &[Subtype::Normal, Subtype::LandscapeNormal];
const BL: &[&str] = &["b", "bl"];

static PBR: ShaderSpec = ShaderSpec {
    required: &[
        role("albedo", &[Subtype::PbrAlbedo]),
        role("_n", &[Subtype::PbrNormal]),
        role("_rmaos", &[Subtype::Rmaos]),
    ],
    optional: &[
        role("emissive _g", &[Subtype::PbrEmissive]),
        role("subsurface/coat colour _s", &[Subtype::PbrSubsurface]),
        role("coat _cnr", &[Subtype::CoatNormal]),
        role("fuzz _f", &[Subtype::PbrFuzz]),
        role("_p", &[Subtype::Height]),
    ],
};
/// PBR grass design §4.1: what CS's grass shader binds (t0, t2, t3, t4; research §1.3).
static PBR_GRASS: ShaderSpec = ShaderSpec {
    required: &[
        role("albedo", &[Subtype::PbrAlbedo]),
        role("_n", &[Subtype::PbrNormal]),
        role("_rmaos", &[Subtype::Rmaos]),
    ],
    optional: &[role("subsurface _s", &[Subtype::PbrSubsurface])],
};
/// Note BM: CS's PBR landscape texture set (albedo, `_n`, `_rmaos`, displacement). Packing and formats are `pbr`'s:
/// no member defaults and no re-typing, so its outputs are byte-identical to the same set picked as `pbr`.
static PBR_LANDSCAPE: ShaderSpec = ShaderSpec {
    required: &[
        role(
            "albedo",
            &[
                Subtype::PbrAlbedo,
                Subtype::LandscapeDiffuse,
                Subtype::Diffuse,
            ],
        ),
        role(
            "_n",
            &[
                Subtype::PbrNormal,
                Subtype::Normal,
                Subtype::LandscapeNormal,
            ],
        ),
    ],
    optional: &[
        role("_rmaos", &[Subtype::Rmaos]),
        role("_p", &[Subtype::Height]),
    ],
};
static ADVANCED_SKIN: ShaderSpec = ShaderSpec {
    required: &[
        role("skin_albedo", &[Subtype::SkinAlbedo]),
        role(
            "_n/_msn",
            &[Subtype::PbrNormal, Subtype::Normal, Subtype::Msn],
        ),
        role("skin_rfaos", &[Subtype::SkinRfaos]),
    ],
    optional: &[
        role("skin_sk", &[Subtype::SkinSk]),
        role("skin_wet", &[Subtype::SkinWet]),
    ],
};
static DEFAULT: ShaderSpec = ShaderSpec {
    required: &[role("diffuse", D), role("_n", N)],
    optional: &[sfx("backlight _b/_bl", &[Subtype::BacklightTint], BL)],
};
static ENVMAP: ShaderSpec = ShaderSpec {
    required: &[role("diffuse", D), role("_n", N)],
    optional: &[
        role("_m env mask", &[Subtype::Envmask]),
        sfx("_e", &[], &["e"]),
    ],
};
static COMPLEX_MATERIAL: ShaderSpec = ShaderSpec {
    required: &[
        role("diffuse", D),
        role("_n", N),
        role("_m complex", &[Subtype::ComplexMaterial]),
    ],
    optional: &[sfx("_e", &[], &["e"])],
};
static GLOW: ShaderSpec = ShaderSpec {
    required: &[
        role("diffuse", D),
        role("_n", N),
        role("_g", &[Subtype::Glow]),
    ],
    optional: &[],
};
static PARALLAX: ShaderSpec = ShaderSpec {
    required: &[
        role("diffuse", D),
        role("_n", N),
        role("_p", &[Subtype::Height]),
    ],
    optional: &[],
};
static SKIN: ShaderSpec = ShaderSpec {
    required: &[
        role("diffuse", D),
        role("_n/_msn", &[Subtype::Normal, Subtype::Msn]),
    ],
    optional: &[
        sfx("_sk", &[], &["sk"]),
        role("_s (with _msn)", &[Subtype::MsnSpecular]),
    ],
};
static HAIR: ShaderSpec = ShaderSpec {
    required: &[role("diffuse", D), role("_n", N)],
    optional: &[],
};
/// Note CL35: CS hair also reads a `_flow`; vanilla hair does not (`HAIR`). By suffix, so a skipped DDS fills it.
static HAIR_CS: ShaderSpec = ShaderSpec {
    required: HAIR.required,
    optional: &[sfx("_flow", &[], &["flow"])],
};
static EYE: ShaderSpec = ShaderSpec {
    required: &[
        role("diffuse", D),
        role("_n (model-space)", &[Subtype::Msn]),
    ],
    optional: &[
        sfx("_sk", &[], &["sk"]),
        role("_m", &[Subtype::Envmask]),
        sfx("_e", &[], &["e"]),
    ],
};
static MULTILAYER: ShaderSpec = ShaderSpec {
    required: &[role("diffuse", D), role("_n", N), sfx("_i", &[], &["i"])],
    optional: &[
        role("_m", &[Subtype::Envmask]),
        sfx("_e", &[], &["e"]),
        sfx("_b", &[Subtype::BacklightTint], BL),
    ],
};
static COMPLEX_GRASS: ShaderSpec = ShaderSpec {
    required: &[role("complex_grass", &[Subtype::ComplexGrass])],
    optional: &[],
};
static LANDSCAPE: ShaderSpec = ShaderSpec {
    required: &[
        role("diffuse", &[Subtype::LandscapeDiffuse, Subtype::Diffuse]),
        role("_n", &[Subtype::LandscapeNormal, Subtype::Normal]),
    ],
    optional: &[],
};

impl Shader {
    pub const ALL: [Shader; 15] = [
        Shader::Pbr,
        Shader::PbrGrass,
        Shader::PbrLandscape,
        Shader::AdvancedSkin,
        Shader::Default,
        Shader::Envmap,
        Shader::ComplexMaterial,
        Shader::Glow,
        Shader::Parallax,
        Shader::Skin,
        Shader::Hair,
        Shader::Eye,
        Shader::Multilayer,
        Shader::Landscape,
        Shader::ComplexGrass,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Shader::Pbr => "pbr",
            Shader::PbrGrass => "pbr_grass",
            Shader::PbrLandscape => "pbr_landscape",
            Shader::AdvancedSkin => "advanced_skin",
            Shader::Default => "default",
            Shader::Envmap => "envmap",
            Shader::ComplexMaterial => "complex_material",
            Shader::Glow => "glow",
            Shader::Parallax => "parallax",
            Shader::Skin => "skin",
            Shader::Hair => "hair",
            Shader::Eye => "eye",
            Shader::Multilayer => "multilayer",
            Shader::Landscape => "landscape",
            Shader::ComplexGrass => "complex_grass",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Shader::Pbr => "PBR",
            Shader::PbrGrass => "PBR grass",
            Shader::PbrLandscape => "PBR Landscape",
            Shader::AdvancedSkin => "Advanced Skin",
            Shader::Default => "Default",
            Shader::Envmap => "Environment Map",
            Shader::ComplexMaterial => "Complex Material",
            Shader::Glow => "Glow",
            Shader::Parallax => "Parallax",
            Shader::Skin => "Skin",
            Shader::Hair => "Hair",
            Shader::Eye => "Eye",
            Shader::Multilayer => "Multilayer Parallax",
            Shader::Landscape => "Landscape",
            Shader::ComplexGrass => "Complex grass",
        }
    }

    /// The shaders a target can use: CS has all 15 (`pbr` first); Vanilla lacks `pbr`, `pbr_grass`, `pbr_landscape`
    /// and `advanced_skin` but has complex grass and complex material (Note AZ, BM).
    pub fn available(t: Target) -> &'static [Shader] {
        match t {
            Target::Cs => &Shader::ALL,
            // `ALL` starts with `pbr`, `pbr_grass`, `pbr_landscape`, `advanced_skin`.
            Target::Vanilla => &Shader::ALL[4..],
        }
    }

    pub fn is_available(self, t: Target) -> bool {
        Shader::available(t).contains(&self)
    }

    pub fn default_for(t: Target) -> Shader {
        match t {
            Target::Cs => Shader::Pbr,
            Target::Vanilla => Shader::Default,
        }
    }

    /// `spec` as `t` reads it: CS hair also reads `_flow` (Note CL35).
    pub fn spec_for(self, t: Target) -> &'static ShaderSpec {
        match (self, t) {
            (Shader::Hair, Target::Cs) => &HAIR_CS,
            _ => self.spec(),
        }
    }

    pub fn spec(self) -> &'static ShaderSpec {
        match self {
            Shader::Pbr => &PBR,
            Shader::PbrGrass => &PBR_GRASS,
            Shader::PbrLandscape => &PBR_LANDSCAPE,
            Shader::AdvancedSkin => &ADVANCED_SKIN,
            Shader::Default => &DEFAULT,
            Shader::Envmap => &ENVMAP,
            Shader::ComplexMaterial => &COMPLEX_MATERIAL,
            Shader::Glow => &GLOW,
            Shader::Parallax => &PARALLAX,
            Shader::Skin => &SKIN,
            Shader::Hair => &HAIR,
            Shader::Eye => &EYE,
            Shader::Multilayer => &MULTILAYER,
            Shader::Landscape => &LANDSCAPE,
            Shader::ComplexGrass => &COMPLEX_GRASS,
        }
    }

    /// True for the shaders that change their members' default settings.
    pub fn has_member_defaults(self) -> bool {
        matches!(
            self,
            Shader::Hair
                | Shader::Eye
                | Shader::Landscape
                | Shader::Multilayer
                | Shader::Envmap
                | Shader::Parallax
                | Shader::ComplexGrass
                | Shader::PbrGrass
        )
    }

    /// The shader a NIF `BSLightingShaderProperty` of nif.xml `BSLightingShaderType` `t` reports, or `None` (no
    /// vote), S-shader-10. Type 7 (Parallax Occlusion) maps to `parallax` (owner ruling 2026-10-11). No type yields
    /// `pbr` or `advanced_skin`: the TruePBR flag is PGPatcher's output marker, not a lighting type.
    pub fn from_lighting_type(t: u32) -> Option<Shader> {
        Some(match t {
            0 => Shader::Default,
            1 => Shader::Envmap,
            2 => Shader::Glow,
            3 | 7 => Shader::Parallax,
            5 => Shader::Skin,
            6 => Shader::Hair,
            8 => Shader::Landscape,
            11 => Shader::Multilayer,
            16 => Shader::Eye,
            _ => return None,
        })
    }
}

/// The scan-database key of a set (S-shader-10): the folder of `game_path` + `/` + `stem`, lower case; no folder
/// gives just `stem`. ASCII-folded like `nif::path` (Note BF): the scan side never folds non-ASCII. The database is asked for `<key>.dds`.
pub fn mesh_key(game_path: &str, stem: &str) -> String {
    match game_path.rsplit_once('/') {
        Some((dir, _)) => format!("{dir}/{stem}"),
        None => stem.to_owned(),
    }
    .to_ascii_lowercase()
}

/// The `mesh_key` of a set's colour member (final review I1), or `None` for any other member. A colour member has
/// a diffuse/colour sub-type, or an output stem with no suffix or `_d`; the key uses that output stem (`wall_d`),
/// which is the name the meshes reference. `subtype` is `None` for a member with no job (a skipped DDS input).
pub fn colour_mesh_key(
    game_path: &str,
    output: &std::path::Path,
    subtype: Option<Subtype>,
) -> Option<String> {
    let stem = output.file_stem()?.to_str()?;
    let colour = matches!(
        subtype,
        Some(
            Subtype::Diffuse | Subtype::LandscapeDiffuse | Subtype::PbrAlbedo | Subtype::SkinAlbedo
        )
    ) || matches!(
        crate::group::split_suffix(stem).1.as_deref(),
        None | Some("d")
    );
    colour.then(|| mesh_key(game_path, stem))
}

/// The GRAS key of a member (audit 08b-3): the folder of `game_path` and the stem of `output`, the name the NIF
/// references (`fieldgrass_basecolor.png` is written as `fieldgrass.dds`). Folded as `grass::path_key`.
pub fn gras_key(game_path: &str, output: &std::path::Path) -> String {
    mesh_key(
        game_path,
        &output.file_stem().unwrap_or_default().to_string_lossy(),
    )
}

/// The alpha property of one scanned lighting shape that uses a colour member in slot 0 (M6 Track E spec §4.1,
/// DECISIONS Note U5): the NiAlphaProperty blend and test enables and the test threshold. A shape without the
/// property is all false.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeshAlpha {
    pub blend: bool,
    pub test: bool,
    pub threshold: u8,
}

/// The owner's table (spec §4.2, DECISIONS Note U6): the alpha policy the shapes give their colour member, and the
/// cutout threshold when a shape tests; `None` = no evidence. Any shape with alpha beats the shapes without (there is
/// no vote), and blend and test anywhere, on one shape or two, is `cutout_blend`. The threshold is the largest of the
/// testing shapes (the stricter cutoff), clamped to sdc's 1..=254 (owner O2).
pub fn mesh_alpha_policy(shapes: &[MeshAlpha]) -> Option<(AlphaPolicy, Option<u8>)> {
    if shapes.is_empty() {
        return None;
    }
    let blend = shapes.iter().any(|s| s.blend);
    let threshold = shapes
        .iter()
        .filter(|s| s.test)
        .map(|s| {
            s.threshold
                .clamp(*ALPHA_THRESHOLD.start(), *ALPHA_THRESHOLD.end())
        })
        .max();
    Some(match (blend, threshold) {
        (false, None) => (AlphaPolicy::Remove, None),
        (true, None) => (AlphaPolicy::Blend, None),
        (false, t) => (AlphaPolicy::Cutout, t),
        (true, t) => (AlphaPolicy::CutoutBlend, t),
    })
}

/// The auto-pick rule that chose a shader (spec §4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoRule {
    AdvancedSkin,
    Pbr,
    PbrGrass,
    PbrLandscape,
    Mesh,
    ComplexM,
    EnvM,
    P,
    G,
    I,
    Sk,
    Flow,
    HairPath,
    EyesPath,
    ComplexGrass,
    Landscape,
    Otherwise,
}

impl AutoRule {
    const ALL: [AutoRule; 17] = [
        AutoRule::AdvancedSkin,
        AutoRule::Pbr,
        AutoRule::PbrGrass,
        AutoRule::PbrLandscape,
        AutoRule::ComplexGrass,
        AutoRule::Mesh,
        AutoRule::ComplexM,
        AutoRule::EnvM,
        AutoRule::P,
        AutoRule::G,
        AutoRule::I,
        AutoRule::Sk,
        AutoRule::Flow,
        AutoRule::HairPath,
        AutoRule::EyesPath,
        AutoRule::Landscape,
        AutoRule::Otherwise,
    ];

    pub fn id(self) -> &'static str {
        match self {
            AutoRule::AdvancedSkin => "advanced_skin",
            AutoRule::Pbr => "pbr",
            AutoRule::PbrGrass => "pbr_grass",
            AutoRule::PbrLandscape => "pbr_landscape",
            AutoRule::Mesh => "mesh",
            AutoRule::ComplexM => "complex_m",
            AutoRule::EnvM => "env_m",
            AutoRule::P => "p",
            AutoRule::G => "g",
            AutoRule::I => "i",
            AutoRule::Sk => "sk",
            AutoRule::Flow => "flow",
            AutoRule::HairPath => "hair_path",
            AutoRule::EyesPath => "eyes_path",
            AutoRule::Landscape => "landscape",
            AutoRule::ComplexGrass => "complex_grass",
            AutoRule::Otherwise => "default",
        }
    }
}

/// Why a set has its shader (spec §4.4). Serialises as a string: `set`, `auto:<rule id>`, `fallback`.
#[derive(Debug, Clone, PartialEq, Eq, JsonSchema)]
#[schemars(with = "String")]
pub enum ShaderOrigin {
    Set,
    Auto(AutoRule),
    Fallback,
}

impl fmt::Display for ShaderOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShaderOrigin::Set => f.write_str("set"),
            ShaderOrigin::Auto(r) => write!(f, "auto:{}", r.id()),
            ShaderOrigin::Fallback => f.write_str("fallback"),
        }
    }
}

impl FromStr for ShaderOrigin {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "set" => Ok(ShaderOrigin::Set),
            "fallback" => Ok(ShaderOrigin::Fallback),
            _ => s
                .strip_prefix("auto:")
                .and_then(|id| AutoRule::ALL.into_iter().find(|r| r.id() == id))
                .map(ShaderOrigin::Auto)
                .ok_or_else(|| format!("unknown shader provenance `{s}`")),
        }
    }
}

impl Serialize for ShaderOrigin {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ShaderOrigin {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// A set's shader, why it was chosen, and the shader asked for when that one was not available (`wanted`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ShaderPick {
    pub shader: Shader,
    pub provenance: ShaderOrigin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wanted: Option<Shader>,
}

pub struct PickInput<'a> {
    pub target: Target,
    /// The explicit choice for this set (GUI `sdc-project.toml` or `--set set:`), if any.
    pub explicit: Option<Shader>,
    /// `pack::plan::set_is_advanced_skin` for the set.
    pub advanced_skin: bool,
    /// `suggest::is_pbr_set` evaluated as under `cs`, whatever the target: rule 2 is target-independent, so a PBR set
    /// under vanilla auto-picks `pbr` and falls back with SHADER-UNAVAILABLE.
    pub pbr: bool,
    /// PBR grass design §4.2: a member is the slot-0 diffuse of a GRAS record's model, TruePBR shapes included
    /// (`BatchRequest::gras_all_diffuses`).
    pub gras: bool,
    /// Note BM: the GUI set has a `[sets.cs_txst]` block (`BatchRequest::cs_txst_sets`); always false for the CLI.
    pub cs_txst: bool,
    pub suffixes: &'a BTreeSet<String>,
    /// Pass-1 sub-types of the set's members (packing inputs excluded).
    pub subtypes: &'a [Subtype],
    pub game_paths: &'a [&'a str],
    /// S-shader-10: what the scanned meshes report for the set's diffuse, one entry per lighting shape
    /// (`BatchRequest::mesh_shaders`); empty = no evidence.
    pub mesh: &'a [Shader],
}

/// S-shader-10: the strict majority of the mesh votes (more than half), else `None` (a tie or no votes: the suffix
/// rules decide).
fn mesh_majority(i: &PickInput) -> Option<Shader> {
    let mut n: BTreeMap<Shader, usize> = BTreeMap::new();
    for s in i.mesh {
        *n.entry(*s).or_default() += 1;
    }
    let (top, c) = n.into_iter().max_by_key(|(_, c)| *c)?;
    (c * 2 > i.mesh.len()).then_some(top)
}

/// Spec §4.1-4.3: explicit choice, else the first matching auto rule; a shader the target lacks falls back to
/// `default_for(target)`. Order (owner answer 2026-10-11, "map wins, assume the user runs PGPatcher"): advanced
/// skin, PBR (PBR grass, then CS PBR landscape, then plain PBR; Note BM), then the PGPatcher-patchable maps (complex `_m`, `_p`), which beat the mesh, then the mesh majority
/// (S-shader-10), then envmap `_m`, `_g`, `_i`, `_sk`, `_flow` (Note CL35), hair/eyes/landscape paths and the default.
pub fn pick(i: &PickInput) -> ShaderPick {
    use Subtype::*;
    let has = |t: &[Subtype]| i.subtypes.iter().any(|s| t.contains(s));
    let sfx = |s: &str| i.suffixes.contains(s);
    let path = |c: &str| {
        i.game_paths
            .iter()
            .any(|p| has_component(&p.to_ascii_lowercase(), c))
    };
    let landscape_path = || {
        i.game_paths
            .iter()
            .any(|p| p.to_ascii_lowercase().starts_with("textures/landscape/"))
            || path("landscape")
    };
    let (shader, rule) = if i.advanced_skin || has(&[SkinAlbedo, SkinRfaos, SkinSk, SkinWet]) {
        (Shader::AdvancedSkin, AutoRule::AdvancedSkin)
    } else if i.pbr
        || has(&[
            PbrAlbedo,
            PbrEmissive,
            PbrSubsurface,
            PbrFuzz,
            PbrNormal,
            CoatNormal,
            Rmaos,
        ])
    {
        if i.gras {
            (Shader::PbrGrass, AutoRule::PbrGrass)
        } else if i.target == Target::Cs && (i.cs_txst || landscape_path()) {
            // Note BM: CS only, so a vanilla PBR landscape set still falls back with `wanted: pbr` as before.
            (Shader::PbrLandscape, AutoRule::PbrLandscape)
        } else {
            (Shader::Pbr, AutoRule::Pbr)
        }
    } else if has(&[ComplexGrass]) {
        (Shader::ComplexGrass, AutoRule::ComplexGrass)
    } else if has(&[ComplexMaterial]) {
        (Shader::ComplexMaterial, AutoRule::ComplexM)
    } else if sfx("p") {
        (Shader::Parallax, AutoRule::P)
    } else if let Some(m) = mesh_majority(i) {
        (m, AutoRule::Mesh)
    } else if has(&[Envmask]) || sfx("m") || sfx("em") || sfx("envmask") {
        (Shader::Envmap, AutoRule::EnvM)
    } else if sfx("g") {
        (Shader::Glow, AutoRule::G)
    } else if sfx("i") {
        (Shader::Multilayer, AutoRule::I)
    } else if sfx("sk") {
        (Shader::Skin, AutoRule::Sk)
    } else if sfx("flow") && !path("water") && !landscape_path() {
        // Note CL35 (owner 2026-10-20): a `_flow` outside water and landscape is a hair flow map; a suffix rule, so
        // it beats the hair/eyes path rules. Water and landscape sets fall through to the rules below as before.
        (Shader::Hair, AutoRule::Flow)
    } else if path("hair") {
        (Shader::Hair, AutoRule::HairPath)
    } else if path("eyes") {
        (Shader::Eye, AutoRule::EyesPath)
    } else if has(&[LandscapeDiffuse]) || landscape_path() {
        (Shader::Landscape, AutoRule::Landscape)
    } else {
        (Shader::Default, AutoRule::Otherwise)
    };
    let (shader, provenance) = match i.explicit {
        Some(s) => (s, ShaderOrigin::Set),
        None => (shader, ShaderOrigin::Auto(rule)),
    };
    if shader.is_available(i.target) {
        ShaderPick {
            shader,
            provenance,
            wanted: None,
        }
    } else {
        ShaderPick {
            shader: Shader::default_for(i.target),
            provenance: ShaderOrigin::Fallback,
            wanted: Some(shader),
        }
    }
}

/// The plan's jobs indexed once by input path and by set key, so the per-set shader pick and map check look up a
/// set's jobs instead of scanning every job per set. Both lookups keep plan order.
pub struct JobIndex<'a> {
    jobs: &'a [crate::policy::ResolvedJob],
    by_input: HashMap<&'a Path, Vec<usize>>,
    by_set: HashMap<&'a str, Vec<usize>>,
}

impl<'a> JobIndex<'a> {
    pub fn new(jobs: &'a [crate::policy::ResolvedJob]) -> Self {
        let mut by_input: HashMap<&Path, Vec<usize>> = HashMap::new();
        let mut by_set: HashMap<&str, Vec<usize>> = HashMap::new();
        for (i, j) in jobs.iter().enumerate() {
            by_input.entry(j.input.as_path()).or_default().push(i);
            by_set.entry(j.set_key.as_str()).or_default().push(i);
        }
        Self {
            jobs,
            by_input,
            by_set,
        }
    }

    fn get<'s>(
        &'s self,
        ix: Option<&'s Vec<usize>>,
    ) -> impl Iterator<Item = &'a crate::policy::ResolvedJob> + 's {
        ix.into_iter().flatten().map(|&i| &self.jobs[i])
    }

    /// The jobs whose input is `path` (a packed job's input is its first source's path).
    pub fn by_input<'s>(
        &'s self,
        path: &Path,
    ) -> impl Iterator<Item = &'a crate::policy::ResolvedJob> + 's {
        self.get(self.by_input.get(path))
    }

    /// The jobs of the set `key`.
    pub fn in_set<'s>(
        &'s self,
        key: &str,
    ) -> impl Iterator<Item = &'a crate::policy::ResolvedJob> + 's {
        self.get(self.by_set.get(key))
    }
}

/// `pick` for one set at plan time (spec §4): pass-1 `jobs` give the member sub-types (packing inputs excluded),
/// `mesh` the scanned votes keyed by `mesh_key`, `gras` the GRAS slot-0 diffuses (`grass::path_key` keys), `cs_txst` whether the set has a texture-set block.
#[allow(clippy::too_many_arguments)]
pub fn pick_for_set(
    ctx: &crate::policy::ResolveCtx,
    set: &crate::group::TextureSet,
    files: &[crate::discover::InputFile],
    jobs: &JobIndex,
    explicit: Option<Shader>,
    mesh: &BTreeMap<String, Vec<Shader>>,
    gras: &BTreeSet<String>,
    cs_txst: bool,
) -> ShaderPick {
    let first = set.members.first().map(|&i| &files[i]);
    let first_path = first.map_or("", |f| f.game_path.as_str());
    // Keyed by the colour member (its job's output, else the file's); the first colour key with votes wins.
    // shortcut: a GUI output-path override that renames the colour member moves its key away from the mesh's
    // texture path; look the votes up by the mesh-side path if that bites
    let votes = set
        .members
        .iter()
        .filter_map(|&i| {
            let f = &files[i];
            let job = jobs.by_input(&f.path).next();
            let output = job.map_or(f.output.as_path(), |j| j.output.as_path());
            colour_mesh_key(&f.game_path, output, job.map(|j| j.profile.subtype))
        })
        .find_map(|k| mesh.get(&k))
        .map_or(&[][..], Vec::as_slice);
    let subtypes: Vec<Subtype> = jobs
        .in_set(&set.key)
        .filter(|j| {
            !matches!(
                j.status,
                crate::policy::JobStatus::PackingInput { .. }
                    | crate::policy::JobStatus::NormalLayer { .. }
            )
        })
        .map(|j| j.profile.subtype)
        .collect();
    let game_paths: Vec<&str> = set
        .members
        .iter()
        .map(|&i| files[i].game_path.as_str())
        .collect();
    pick(&PickInput {
        target: ctx.target,
        explicit,
        advanced_skin: first.is_some_and(|f| crate::pack::plan::set_is_advanced_skin(ctx, set, f)),
        pbr: crate::suggest::is_pbr_set(&crate::suggest::SuggestContext {
            target: Target::Cs,
            suffix: None,
            game_path: first_path,
            set_suffixes: &set.suffixes,
            alpha: crate::presets::AlphaContent::None,
            is_float: false,
            advanced_skin: false,
        }),
        gras: set.members.iter().any(|&m| {
            let f = &files[m];
            let output = jobs
                .by_input(&f.path)
                .next()
                .map_or(f.output.as_path(), |j| j.output.as_path());
            gras.contains(&gras_key(&f.game_path, output))
        }),
        cs_txst,
        suffixes: &set.suffixes,
        subtypes: &subtypes,
        game_paths: &game_paths,
        mesh: votes,
    })
}

/// One member of a set as the map check sees it. A skipped DDS member has no sub-type.
pub struct MemberMap {
    pub name: String,
    pub suffix: Option<String>,
    pub subtype: Option<Subtype>,
}

/// The role each member fills, in `members` order. Roles keyed by suffix are tried first, so an `_i` that resolves
/// `diffuse` fills `_i`, not `diffuse`. `None` = the shader does not use the member.
pub fn assign<'s>(spec: &'s ShaderSpec, members: &[MemberMap]) -> Vec<Option<&'s MapRole>> {
    let roles = || spec.required.iter().chain(spec.optional);
    members
        .iter()
        .map(|m| {
            let hit = |r: &&MapRole| r.matches(m.suffix.as_deref(), m.subtype);
            roles()
                .filter(|r| !r.suffixes.is_empty())
                .find(hit)
                .or_else(|| roles().filter(|r| r.suffixes.is_empty()).find(hit))
        })
        .collect()
}

/// SHADER-EXTRA-MAP per unused member (data `{"member", "shader"}`) and SHADER-MISSING-MAP per required role no
/// member fills (data `{"map", "shader"}`), members first, then roles in table order.
pub fn check_maps(pick: &ShaderPick, target: Target, members: &[MemberMap]) -> Vec<Warning> {
    // Complex grass is drawn by the grass shader, which reads diffuse and normal from the one file (DECISIONS X9).
    if members
        .iter()
        .any(|m| m.subtype == Some(Subtype::ComplexGrass))
    {
        return vec![];
    }
    let id = pick.shader.id();
    let spec = pick.shader.spec_for(target);
    let filled = assign(spec, members);
    let extra = members
        .iter()
        .zip(&filled)
        .filter(|(_, r)| r.is_none())
        .map(|(m, _)| {
            Warning::new(
                Code::ShaderExtraMap,
                format!(
                    "{} is not used by the {id} shader; it is still converted",
                    m.name
                ),
            )
            .with_data(serde_json::json!({"member": m.name, "shader": id}))
        });
    let missing = spec
        .required
        .iter()
        .filter(|r| !filled.iter().flatten().any(|f| std::ptr::eq(*f, *r)))
        .map(|r| {
            Warning::new(
                Code::ShaderMissingMap,
                format!(
                    "the {id} shader needs {}; none is in the set (nothing is generated)",
                    r.label
                ),
            )
            .with_data(serde_json::json!({"map": r.label, "shader": id}))
        });
    extra.chain(missing).collect()
}

/// SHADER-AUTO for an `auto:` pick (data `{"shader", "rule"}`); SHADER-UNAVAILABLE for a fallback (data
/// `{"wanted", "shader"}`); nothing for `set`.
pub fn pick_notes(pick: &ShaderPick, target: Target) -> Vec<Warning> {
    let id = pick.shader.id();
    match (&pick.provenance, pick.wanted) {
        (ShaderOrigin::Auto(r), _) => vec![Warning::new(
            Code::ShaderAuto,
            format!("shader {id} picked automatically ({})", r.id()),
        )
        .with_data(serde_json::json!({"shader": id, "rule": r.id()}))],
        (ShaderOrigin::Fallback, Some(w)) => vec![Warning::new(
            Code::ShaderUnavailable,
            format!(
                "shader {} is not available under {target:?}; using {id}",
                w.id()
            ),
        )
        .with_data(serde_json::json!({"wanted": w.id(), "shader": id}))],
        _ => vec![],
    }
}

/// PBR grass design §4.4: CS's grass depth/shadow pass runs the complex-grass detector on a PBR grass albedo
/// (research §1.6), so an albedo whose bottom-left texel reads as a unit vector gets mismatched cutout shadows.
/// `GRASS_GUARD_MARGIN` to spare: BC encoding moves the texel (DECISIONS X2).
pub fn pbr_grass_albedo_note(pick: &ShaderPick, st: Subtype, corner_err: f32) -> Option<Warning> {
    use crate::grass::{CS_GRASS_THRESHOLD, GRASS_GUARD_MARGIN};
    (pick.shader == Shader::PbrGrass
        && matches!(st, Subtype::PbrAlbedo | Subtype::Diffuse)
        && corner_err < CS_GRASS_THRESHOLD + GRASS_GUARD_MARGIN)
        .then(|| {
            Warning::new(
                Code::PbrGrassDetect,
                format!(
                    "bottom-left texel has |L-1| = {corner_err:.3}: Community Shaders' grass shadow pass would read \
                     this albedo as complex grass, so shadows will not match; change that texel"
                ),
            )
        })
}

/// The settings `shader` gives a member of sub-type `st` (M6 spec §6): field path → value. The `bc4` rows restate
/// the built-in `envmask`/`height` format so a later built-in change cannot undo them.
pub fn member_defaults(shader: Shader, st: Subtype) -> Vec<(&'static str, serde_json::Value)> {
    use serde_json::json;
    match (shader, st) {
        (Shader::Hair, Subtype::Diffuse) => {
            vec![
                ("alpha.policy", json!("cutout_blend")),
                ("mips.coverage", json!("on")),
            ]
        }
        (Shader::Multilayer, Subtype::Diffuse) => vec![("alpha.policy", json!("data"))],
        (Shader::Envmap | Shader::Eye, Subtype::Envmask) | (Shader::Parallax, Subtype::Height) => {
            vec![("format", json!("bc4"))]
        }
        _ => vec![],
    }
}

/// The sub-type `shader` gives a member whose top suggestion is `suggested`, with why (S-shader-4).
pub fn member_subtype(shader: Shader, suggested: Subtype) -> Option<(Subtype, &'static str)> {
    match (shader, suggested) {
        (Shader::Eye, Subtype::Normal | Subtype::PbrNormal | Subtype::LandscapeNormal) => {
            Some((Subtype::Msn, "model-space `_n`"))
        }
        (Shader::Landscape, Subtype::Diffuse | Subtype::PbrAlbedo) => Some((
            Subtype::LandscapeDiffuse,
            "landscape diffuse (alpha = snow mask)",
        )),
        // Audit 08e-1, owner ruling (b) (Note BV5): PBR grass under `textures/landscape/` is PBR, not landscape.
        (Shader::PbrGrass, Subtype::LandscapeDiffuse) => {
            Some((Subtype::PbrAlbedo, "PBR grass albedo (sRGB)"))
        }
        (Shader::PbrGrass, Subtype::LandscapeNormal) => {
            Some((Subtype::PbrNormal, "PBR grass normal"))
        }
        (
            Shader::ComplexGrass,
            Subtype::Diffuse | Subtype::LandscapeDiffuse | Subtype::PbrAlbedo,
        ) => Some((
            Subtype::ComplexGrass,
            "complex grass (diffuse + normal in one file)",
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Subtype, Target};

    #[test]
    fn mesh_key_folds_ascii_only_like_the_scan_side() {
        // nif::path::canon_texture leaves "Ä" alone, so the key must too (Note BF).
        assert_eq!(
            mesh_key("textures/Ärger/A.png", "Wall_D"),
            "textures/Ärger/wall_d"
        );
    }

    #[test]
    fn ids_round_trip_and_are_snake_case() {
        for s in Shader::ALL {
            let j = serde_json::to_string(&s).unwrap();
            assert_eq!(j, format!("\"{}\"", s.id()));
            assert_eq!(serde_json::from_str::<Shader>(&j).unwrap(), s);
        }
        assert_eq!(Shader::AdvancedSkin.id(), "advanced_skin");
        assert_eq!(Shader::ComplexMaterial.id(), "complex_material");
        assert_eq!(Shader::Multilayer.id(), "multilayer");
    }

    #[test]
    fn lists_and_defaults_follow_the_mode() {
        assert_eq!(Shader::available(Target::Cs).len(), 15);
        assert_eq!(Shader::available(Target::Cs)[0], Shader::Pbr);
        assert_eq!(Shader::available(Target::Cs)[1], Shader::PbrGrass);
        assert_eq!(Shader::available(Target::Cs)[2], Shader::PbrLandscape);
        let l = Shader::available(Target::Vanilla);
        assert_eq!(l.len(), 11);
        assert!(
            !l.contains(&Shader::Pbr)
                && !l.contains(&Shader::PbrGrass)
                && !l.contains(&Shader::PbrLandscape)
                && !l.contains(&Shader::AdvancedSkin)
        );
        assert!(l.contains(&Shader::ComplexGrass) && l.contains(&Shader::ComplexMaterial));
        assert_eq!(Shader::default_for(Target::Vanilla), Shader::Default);
        assert_eq!(Shader::default_for(Target::Cs), Shader::Pbr);
        assert!(!Shader::Pbr.is_available(Target::Vanilla));
        assert!(Shader::ComplexMaterial.is_available(Target::Vanilla));
    }

    #[test]
    fn envmap_does_not_require_m() {
        let s = Shader::Envmap.spec();
        assert!(!s
            .required
            .iter()
            .any(|r| r.matches(Some("m"), Some(Subtype::Envmask))));
        assert!(s
            .optional
            .iter()
            .any(|r| r.matches(Some("m"), Some(Subtype::Envmask))));
    }

    #[test]
    fn roles_match_by_subtype_and_suffix() {
        let skin = Shader::Skin.spec();
        // `_s` counts only as the msn specular, not as a subsurface tint.
        assert!(skin
            .optional
            .iter()
            .any(|r| r.matches(Some("s"), Some(Subtype::MsnSpecular))));
        assert!(!skin
            .optional
            .iter()
            .any(|r| r.matches(Some("s"), Some(Subtype::SubsurfaceTint))));
        // `_e` is identified by suffix: a skipped DDS cubemap has no sub-type.
        assert!(Shader::Envmap
            .spec()
            .optional
            .iter()
            .any(|r| r.matches(Some("e"), None)));
        // An empty role never matches.
        let empty = MapRole {
            label: "x",
            subtypes: &[],
            suffixes: &[],
        };
        assert!(!empty.matches(None, Some(Subtype::Diffuse)));
    }

    #[test]
    fn origin_serialises_as_a_string() {
        let p = ShaderPick {
            shader: Shader::Hair,
            provenance: ShaderOrigin::Auto(AutoRule::HairPath),
            wanted: None,
        };
        let j = serde_json::to_value(&p).unwrap();
        assert_eq!(
            j,
            serde_json::json!({"shader": "hair", "provenance": "auto:hair_path"})
        );
        assert_eq!(serde_json::from_value::<ShaderPick>(j).unwrap(), p);
        assert_eq!(
            "fallback".parse::<ShaderOrigin>().unwrap(),
            ShaderOrigin::Fallback
        );
        assert_eq!("set".parse::<ShaderOrigin>().unwrap(), ShaderOrigin::Set);
        assert_eq!(
            "auto:mesh".parse::<ShaderOrigin>().unwrap(),
            ShaderOrigin::Auto(AutoRule::Mesh)
        );
        assert!("auto:nope".parse::<ShaderOrigin>().is_err());
    }

    #[test]
    fn lighting_types_map_to_the_twelve_or_nothing() {
        // 7 (Parallax Occlusion) → parallax: owner ruling 2026-10-11 (progress.md), overrides the brief.
        let want = [
            (0, Shader::Default),
            (1, Shader::Envmap),
            (2, Shader::Glow),
            (3, Shader::Parallax),
            (5, Shader::Skin),
            (6, Shader::Hair),
            (7, Shader::Parallax),
            (8, Shader::Landscape),
            (11, Shader::Multilayer),
            (16, Shader::Eye),
        ];
        for (t, s) in want {
            assert_eq!(Shader::from_lighting_type(t), Some(s), "{t}");
        }
        for t in [4, 9, 10, 12, 13, 14, 15, 17, 18, 19, 20, 99, u32::MAX] {
            assert_eq!(Shader::from_lighting_type(t), None, "{t}");
        }
    }

    #[test]
    fn mesh_key_is_the_folder_and_the_stem() {
        assert_eq!(mesh_key("textures/a/wall_n.png", "wall"), "textures/a/wall");
        assert_eq!(mesh_key("textures/a/Wall.png", "Wall"), "textures/a/wall");
        assert_eq!(mesh_key("wall.png", "wall"), "wall");
    }

    #[test]
    fn member_default_shaders() {
        let with: Vec<Shader> = Shader::ALL
            .into_iter()
            .filter(|s| s.has_member_defaults())
            .collect();
        assert_eq!(
            with,
            vec![
                Shader::PbrGrass,
                Shader::Envmap,
                Shader::Parallax,
                Shader::Hair,
                Shader::Eye,
                Shader::Multilayer,
                Shader::Landscape,
                Shader::ComplexGrass
            ]
        );
    }

    fn input<'a>(
        t: Target,
        suffixes: &'a BTreeSet<String>,
        subtypes: &'a [Subtype],
        paths: &'a [&'a str],
    ) -> PickInput<'a> {
        PickInput {
            target: t,
            explicit: None,
            advanced_skin: false,
            pbr: false,
            gras: false,
            cs_txst: false,
            suffixes,
            subtypes,
            game_paths: paths,
            mesh: &[],
        }
    }
    fn sx(s: &[&str]) -> BTreeSet<String> {
        s.iter().map(|x| x.to_string()).collect()
    }
    fn auto(p: &ShaderPick) -> (Shader, ShaderOrigin) {
        (p.shader, p.provenance.clone())
    }

    #[test]
    fn one_test_per_auto_rule() {
        use Subtype::*;
        let none = sx(&[]);
        let p = vec!["textures/a/b.png"];
        #[allow(clippy::type_complexity)]
        let cases: Vec<(
            Target,
            BTreeSet<String>,
            Vec<Subtype>,
            Vec<&str>,
            bool,
            bool,
            Shader,
            AutoRule,
        )> = vec![
            (
                Target::Cs,
                sx(&["sk"]),
                vec![SkinAlbedo, SkinSk],
                p.clone(),
                true,
                false,
                Shader::AdvancedSkin,
                AutoRule::AdvancedSkin,
            ),
            (
                Target::Cs,
                sx(&["n", "rmaos"]),
                vec![PbrAlbedo, PbrNormal, Rmaos],
                p.clone(),
                false,
                true,
                Shader::Pbr,
                AutoRule::Pbr,
            ),
            (
                Target::Cs,
                sx(&["n", "m"]),
                vec![Diffuse, Normal, ComplexMaterial],
                p.clone(),
                false,
                false,
                Shader::ComplexMaterial,
                AutoRule::ComplexM,
            ),
            (
                Target::Vanilla,
                sx(&["n", "m"]),
                vec![Diffuse, Normal, Envmask],
                p.clone(),
                false,
                false,
                Shader::Envmap,
                AutoRule::EnvM,
            ),
            (
                Target::Cs,
                sx(&["n", "p"]),
                vec![Diffuse, Normal, Height],
                p.clone(),
                false,
                false,
                Shader::Parallax,
                AutoRule::P,
            ),
            (
                Target::Vanilla,
                sx(&["n", "g"]),
                vec![Diffuse, Normal, Glow],
                p.clone(),
                false,
                false,
                Shader::Glow,
                AutoRule::G,
            ),
            (
                Target::Vanilla,
                sx(&["n", "i"]),
                vec![Diffuse, Normal, Diffuse],
                p.clone(),
                false,
                false,
                Shader::Multilayer,
                AutoRule::I,
            ),
            (
                Target::Vanilla,
                sx(&["n", "sk"]),
                vec![Diffuse, Normal, BacklightTint],
                p.clone(),
                false,
                false,
                Shader::Skin,
                AutoRule::Sk,
            ),
            (
                Target::Vanilla,
                sx(&["n"]),
                vec![Diffuse, Normal],
                vec!["textures/actors/character/Hair/braid.png"],
                false,
                false,
                Shader::Hair,
                AutoRule::HairPath,
            ),
            (
                Target::Vanilla,
                sx(&["n"]),
                vec![Diffuse, Normal],
                vec!["textures/actors/character/eyes/eye.png"],
                false,
                false,
                Shader::Eye,
                AutoRule::EyesPath,
            ),
            (
                Target::Vanilla,
                sx(&["n"]),
                vec![LandscapeDiffuse, LandscapeNormal],
                vec!["textures/landscape/dirt.png"],
                false,
                false,
                Shader::Landscape,
                AutoRule::Landscape,
            ),
            (
                Target::Vanilla,
                none.clone(),
                vec![Diffuse],
                p.clone(),
                false,
                false,
                Shader::Default,
                AutoRule::Otherwise,
            ),
            // Owner (plan review): a plain set on CS auto-picks `default`, never `pbr`.
            (
                Target::Cs,
                sx(&["n"]),
                vec![Diffuse, Normal],
                p.clone(),
                false,
                false,
                Shader::Default,
                AutoRule::Otherwise,
            ),
        ];
        for (t, s, st, paths, skin, pbr, want, rule) in cases {
            let mut i = input(t, &s, &st, &paths);
            i.advanced_skin = skin;
            i.pbr = pbr;
            assert_eq!(
                auto(&pick(&i)),
                (want, ShaderOrigin::Auto(rule)),
                "{want:?}"
            );
        }
    }

    #[test]
    fn rule_order() {
        use Subtype::*;
        let paths = ["textures/a/b.png"];
        // `_m` complex beats `_p`.
        let s = sx(&["m", "p"]);
        let st = [Diffuse, ComplexMaterial, Height];
        assert_eq!(
            pick(&input(Target::Cs, &s, &st, &paths)).shader,
            Shader::ComplexMaterial
        );
        // PBR beats `_p`.
        let st = [PbrAlbedo, Height, Rmaos];
        let mut i = input(Target::Cs, &s, &st, &paths);
        i.pbr = true;
        assert_eq!(pick(&i).shader, Shader::Pbr);
        // Advanced skin beats PBR.
        i.advanced_skin = true;
        assert_eq!(pick(&i).shader, Shader::AdvancedSkin);
        // A suffix rule beats a path rule: `_g` under hair/ is glow.
        let s = sx(&["g"]);
        let st = [Diffuse, Glow];
        assert_eq!(
            pick(&input(Target::Vanilla, &s, &st, &["textures/hair/x.png"])).shader,
            Shader::Glow
        );
    }

    #[test]
    fn explicit_wins_and_unavailable_falls_back() {
        let s = sx(&["n"]);
        let st = [Subtype::Diffuse, Subtype::Normal];
        let paths = ["textures/a/b.png"];
        let mut i = input(Target::Vanilla, &s, &st, &paths);
        i.explicit = Some(Shader::Hair);
        assert_eq!(auto(&pick(&i)), (Shader::Hair, ShaderOrigin::Set));
        i.explicit = Some(Shader::Pbr);
        assert_eq!(
            pick(&i),
            ShaderPick {
                shader: Shader::Default,
                provenance: ShaderOrigin::Fallback,
                wanted: Some(Shader::Pbr)
            }
        );
        // Auto `pbr` under vanilla falls back too.
        i.explicit = None;
        i.pbr = true;
        assert_eq!(
            pick(&i),
            ShaderPick {
                shader: Shader::Default,
                provenance: ShaderOrigin::Fallback,
                wanted: Some(Shader::Pbr)
            }
        );
    }

    #[test]
    fn mesh_majority_follows_owner_precedence() {
        use Subtype::*;
        let paths = ["textures/a/b.png"];
        let hair = [Shader::Hair, Shader::Default, Shader::Hair];
        let tie = [Shader::Hair, Shader::Default];
        let def = [Shader::Default];
        // `_p` beats the mesh: PGPatcher patches the mesh to parallax.
        let s = sx(&["n", "p"]);
        let st = [Diffuse, Normal, Height];
        let mut i = input(Target::Vanilla, &s, &st, &paths);
        i.mesh = &hair;
        assert_eq!(
            auto(&pick(&i)),
            (Shader::Parallax, ShaderOrigin::Auto(AutoRule::P))
        );
        // A complex `_m` beats a Default mesh.
        let s = sx(&["n", "m"]);
        let st = [Diffuse, Normal, ComplexMaterial];
        let mut i = input(Target::Vanilla, &s, &st, &paths);
        i.mesh = &def;
        assert_eq!(
            auto(&pick(&i)),
            (
                Shader::ComplexMaterial,
                ShaderOrigin::Auto(AutoRule::ComplexM)
            )
        );
        // The mesh beats `_g`; a tie falls through to `_g`.
        let s = sx(&["n", "g"]);
        let st = [Diffuse, Normal, Glow];
        let mut i = input(Target::Vanilla, &s, &st, &paths);
        i.mesh = &hair;
        assert_eq!(
            auto(&pick(&i)),
            (Shader::Hair, ShaderOrigin::Auto(AutoRule::Mesh))
        );
        i.mesh = &tie;
        assert_eq!(
            auto(&pick(&i)),
            (Shader::Glow, ShaderOrigin::Auto(AutoRule::G))
        );
        // An explicit choice beats the meshes.
        i.mesh = &hair;
        i.explicit = Some(Shader::Parallax);
        assert_eq!(auto(&pick(&i)), (Shader::Parallax, ShaderOrigin::Set));
        // A Default mesh beats an envmap `_m`.
        let s = sx(&["n", "m"]);
        let st = [Diffuse, Normal, Envmask];
        let mut i = input(Target::Vanilla, &s, &st, &paths);
        i.mesh = &def;
        assert_eq!(
            auto(&pick(&i)),
            (Shader::Default, ShaderOrigin::Auto(AutoRule::Mesh))
        );
        // PBR and advanced skin beat a Default mesh.
        i.target = Target::Cs;
        i.pbr = true;
        assert_eq!(
            auto(&pick(&i)),
            (Shader::Pbr, ShaderOrigin::Auto(AutoRule::Pbr))
        );
        i.advanced_skin = true;
        assert_eq!(
            auto(&pick(&i)),
            (
                Shader::AdvancedSkin,
                ShaderOrigin::Auto(AutoRule::AdvancedSkin)
            )
        );
    }

    #[test]
    fn subtype_only_triggers_and_plurality_is_no_evidence() {
        use Subtype::*;
        let paths = ["textures/a/b.png"];
        let s = sx(&["n"]);
        // Skin / PBR sub-types alone (flags false) pick the shader.
        let st = [SkinAlbedo, Normal];
        assert_eq!(
            auto(&pick(&input(Target::Cs, &s, &st, &paths))),
            (
                Shader::AdvancedSkin,
                ShaderOrigin::Auto(AutoRule::AdvancedSkin)
            )
        );
        let st = [PbrAlbedo, Normal];
        assert_eq!(
            auto(&pick(&input(Target::Cs, &s, &st, &paths))),
            (Shader::Pbr, ShaderOrigin::Auto(AutoRule::Pbr))
        );
        // 2 of 4 votes is a plurality, not a strict majority: the suffix rules decide.
        let s = sx(&["n", "g"]);
        let st = [Diffuse, Normal, Glow];
        let mut i = input(Target::Vanilla, &s, &st, &paths);
        let votes = [Shader::Hair, Shader::Hair, Shader::Default, Shader::Glow];
        i.mesh = &votes;
        assert_eq!(
            auto(&pick(&i)),
            (Shader::Glow, ShaderOrigin::Auto(AutoRule::G))
        );
    }

    fn m(name: &str, suffix: Option<&str>, st: Option<Subtype>) -> MemberMap {
        MemberMap {
            name: name.into(),
            suffix: suffix.map(str::to_string),
            subtype: st,
        }
    }
    fn codes(w: &[crate::codes::Warning]) -> Vec<crate::codes::Code> {
        w.iter().map(|w| w.code).collect()
    }

    #[test]
    fn extra_and_missing_maps() {
        use crate::codes::Code::*;
        let glow = ShaderPick {
            shader: Shader::Glow,
            provenance: ShaderOrigin::Set,
            wanted: None,
        };
        let ms = [
            m("lamp.dds", None, Some(Subtype::Diffuse)),
            m("lamp_p.dds", Some("p"), Some(Subtype::Height)),
        ];
        let w = check_maps(&glow, Target::Cs, &ms);
        assert_eq!(
            codes(&w),
            vec![ShaderExtraMap, ShaderMissingMap, ShaderMissingMap]
        );
        assert_eq!(w[0].data["member"], "lamp_p.dds");
        assert_eq!(w[1].data["map"], "_n");
        assert_eq!(w[2].data["map"], "_g");
    }

    #[test]
    fn a_complex_grass_set_raises_no_map_warnings() {
        let p = ShaderPick {
            shader: Shader::Default,
            provenance: ShaderOrigin::Set,
            wanted: None,
        };
        let ms = [m("g.dds", None, Some(Subtype::ComplexGrass))];
        assert!(check_maps(&p, Target::Cs, &ms).is_empty());
    }

    #[test]
    fn envmap_without_m_is_complete() {
        let p = ShaderPick {
            shader: Shader::Envmap,
            provenance: ShaderOrigin::Set,
            wanted: None,
        };
        let ms = [
            m("a.dds", None, Some(Subtype::Diffuse)),
            m("a_n.dds", Some("n"), Some(Subtype::Normal)),
        ];
        assert!(check_maps(&p, Target::Cs, &ms).is_empty());
    }

    #[test]
    fn an_i_that_resolves_diffuse_fills_i_not_diffuse() {
        let p = ShaderPick {
            shader: Shader::Multilayer,
            provenance: ShaderOrigin::Set,
            wanted: None,
        };
        let ms = [
            m("g_i.dds", Some("i"), Some(Subtype::Diffuse)),
            m("g_n.dds", Some("n"), Some(Subtype::Normal)),
        ];
        let w = check_maps(&p, Target::Cs, &ms);
        assert_eq!(codes(&w), vec![crate::codes::Code::ShaderMissingMap]);
        assert_eq!(w[0].data["map"], "diffuse");
    }

    /// A role with both lists set matches only when the sub-type AND the suffix match: a wrong or missing value on
    /// either side fails it.
    #[test]
    fn a_role_with_subtypes_and_suffixes_needs_both() {
        let r = &Shader::Default.spec().optional[0];
        assert!(!r.subtypes.is_empty() && !r.suffixes.is_empty(), "{r:?}");
        let bt = Some(Subtype::BacklightTint);
        assert!(r.matches(Some("b"), bt));
        assert!(r.matches(Some("bl"), bt));
        assert!(!r.matches(Some("n"), bt), "mismatched suffix");
        assert!(!r.matches(None, bt), "missing suffix");
        assert!(
            !r.matches(Some("b"), Some(Subtype::Diffuse)),
            "mismatched sub-type"
        );
        assert!(!r.matches(Some("b"), None), "missing sub-type");
    }

    /// A member without a sub-type (a skipped DDS) still fills a suffix-only role by its recognised suffix.
    #[test]
    fn a_member_without_a_subtype_fills_its_role_by_suffix() {
        let p = ShaderPick {
            shader: Shader::Envmap,
            provenance: ShaderOrigin::Set,
            wanted: None,
        };
        let ms = [
            m("a.dds", None, Some(Subtype::Diffuse)),
            m("a_n.dds", Some("n"), Some(Subtype::Normal)),
            m("a_e.dds", Some("e"), None),
        ];
        let filled = assign(p.shader.spec(), &ms);
        assert_eq!(filled[2].map(|r| r.label), Some("_e"));
        assert!(check_maps(&p, Target::Cs, &ms).is_empty());
    }

    /// Task 11 (Note CL35): `_flow` picks Hair on both targets, ahead of the path rules.
    #[test]
    fn a_flow_member_picks_hair_on_both_targets() {
        use Subtype::*;
        let s = sx(&["flow", "n"]);
        let st = [Flow, Diffuse, Normal];
        for t in [Target::Cs, Target::Vanilla] {
            assert_eq!(
                auto(&pick(&input(t, &s, &st, &["textures/a/x.png"]))),
                (Shader::Hair, ShaderOrigin::Auto(AutoRule::Flow)),
                "{t:?}"
            );
            // The eyes path rule loses to the suffix rule.
            let p = ["textures/actors/eyes/x.png"];
            assert_eq!(pick(&input(t, &s, &st, &p)).shader, Shader::Hair);
        }
        // Fix M2: the suffix rules above `_flow` still win.
        for (sf, want) in [("g", Shader::Glow), ("sk", Shader::Skin)] {
            let s = sx(&["flow", sf]);
            assert_eq!(
                pick(&input(Target::Vanilla, &s, &st, &["textures/a/x.png"])).shader,
                want,
                "{sf}"
            );
        }
        // Fix I1 (Note CL35): a water or landscape `_flow` is not hair; the later rules decide as before.
        for (p, want) in [
            (
                "textures/water/water.png",
                (Shader::Default, AutoRule::Otherwise),
            ),
            (
                "textures/a/water/x.png",
                (Shader::Default, AutoRule::Otherwise),
            ),
            (
                "textures/landscape/x.png",
                (Shader::Landscape, AutoRule::Landscape),
            ),
            (
                "textures/a/landscape/x.png",
                (Shader::Landscape, AutoRule::Landscape),
            ),
        ] {
            assert_eq!(
                auto(&pick(&input(Target::Vanilla, &s, &st, &[p]))),
                (want.0, ShaderOrigin::Auto(want.1)),
                "{p}"
            );
        }
        // A skipped DDS `_flow` has no sub-type; the suffix alone picks.
        assert_eq!(
            pick(&input(
                Target::Cs,
                &s,
                &[Diffuse, Normal],
                &["textures/a/x.png"]
            ))
            .shader,
            Shader::Hair
        );
        assert_eq!(
            "auto:flow".parse::<ShaderOrigin>().unwrap(),
            ShaderOrigin::Auto(AutoRule::Flow)
        );
    }

    /// Task 11 (Note CL35, CL36): CS hair reads `_flow`; vanilla hair does not, so it stays an extra map there.
    #[test]
    fn flow_is_a_cs_hair_map_and_extra_on_vanilla() {
        let p = ShaderPick {
            shader: Shader::Hair,
            provenance: ShaderOrigin::Auto(AutoRule::Flow),
            wanted: None,
        };
        let ms = [
            m("x.dds", None, Some(Subtype::Diffuse)),
            m("x_n.dds", Some("n"), Some(Subtype::Normal)),
            m("x_flow.dds", Some("flow"), Some(Subtype::Flow)),
            m("y_flow.dds", Some("flow"), None),
        ];
        assert!(check_maps(&p, Target::Cs, &ms).is_empty());
        let w = check_maps(&p, Target::Vanilla, &ms);
        assert_eq!(codes(&w), vec![crate::codes::Code::ShaderExtraMap; 2]);
        assert_eq!(w[0].data["member"], "x_flow.dds");
        assert_eq!(w[1].data["member"], "y_flow.dds");
    }

    #[test]
    fn notes_for_auto_and_fallback() {
        use crate::codes::Code::*;
        let a = ShaderPick {
            shader: Shader::Hair,
            provenance: ShaderOrigin::Auto(AutoRule::HairPath),
            wanted: None,
        };
        assert_eq!(codes(&pick_notes(&a, Target::Cs)), vec![ShaderAuto]);
        let f = ShaderPick {
            shader: Shader::Default,
            provenance: ShaderOrigin::Fallback,
            wanted: Some(Shader::Pbr),
        };
        let w = pick_notes(&f, Target::Vanilla);
        assert_eq!(codes(&w), vec![ShaderUnavailable]);
        assert_eq!(
            w[0].message,
            "shader pbr is not available under Vanilla; using default"
        );
        let s = ShaderPick {
            shader: Shader::Hair,
            provenance: ShaderOrigin::Set,
            wanted: None,
        };
        assert!(pick_notes(&s, Target::Cs).is_empty());
    }

    fn rj(input: &str, set: &str, output: &str) -> crate::policy::ResolvedJob {
        crate::policy::ResolvedJob {
            input: input.into(),
            output: output.into(),
            game_path: input.into(),
            set_key: set.into(),
            profile: crate::presets::PresetRegistry::builtin()
                .unwrap()
                .base_profile(Target::Cs, Subtype::Diffuse),
            provenance: Default::default(),
            suggestions: vec![],
            warnings: vec![],
            status: crate::policy::JobStatus::Ready,
            suffix_alias: None,
            source: None,
            pack: None,
            bake: None,
            rnm: None,
            derive: None,
            shader: None,
        }
    }

    /// One index per plan replaces a scan of every job per set: lookups by input and by set keep plan order.
    #[test]
    fn job_index_keeps_plan_order_by_input_and_by_set() {
        let jobs = [
            rj("a/x.png", "a/x", "x.dds"),
            rj("b/y.png", "b/y", "y.dds"),
            rj("a/x.png", "a/x", "x_rmaos.dds"),
            rj("a/x_n.png", "a/x", "x_n.dds"),
        ];
        let ix = JobIndex::new(&jobs);
        let outs = |it: &mut dyn Iterator<Item = &crate::policy::ResolvedJob>| {
            it.map(|j| j.output.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            outs(&mut ix.by_input(std::path::Path::new("a/x.png"))),
            ["x.dds", "x_rmaos.dds"]
        );
        assert_eq!(
            outs(&mut ix.in_set("a/x")),
            ["x.dds", "x_rmaos.dds", "x_n.dds"]
        );
        assert_eq!(outs(&mut ix.in_set("b/y")), ["y.dds"]);
        assert!(ix.in_set("c/z").next().is_none());
        assert!(ix
            .by_input(std::path::Path::new("c/z.png"))
            .next()
            .is_none());
    }

    #[test]
    fn auto_mesh_label_is_mesh() {
        let a = ShaderPick {
            shader: Shader::Hair,
            provenance: ShaderOrigin::Auto(AutoRule::Mesh),
            wanted: None,
        };
        assert_eq!(
            pick_notes(&a, Target::Cs)[0].message,
            "shader hair picked automatically (mesh)"
        );
    }

    const BLEND: MeshAlpha = MeshAlpha {
        blend: true,
        test: false,
        threshold: 0,
    };
    const NO_ALPHA: MeshAlpha = MeshAlpha {
        blend: false,
        test: false,
        threshold: 0,
    };
    const fn tested(threshold: u8) -> MeshAlpha {
        MeshAlpha {
            blend: false,
            test: true,
            threshold,
        }
    }

    /// M6 Track E spec §4.2: every row of the owner's table. Shapes without alpha never outvote one with alpha, and
    /// blend on one mesh with test on another is cutout_blend, not a majority (Review Focus 1).
    #[test]
    fn mesh_alpha_policy_follows_the_owner_table() {
        use AlphaPolicy::*;
        assert_eq!(mesh_alpha_policy(&[]), None);
        assert_eq!(
            mesh_alpha_policy(&[NO_ALPHA, NO_ALPHA]),
            Some((Remove, None))
        );
        assert_eq!(mesh_alpha_policy(&[BLEND, BLEND]), Some((Blend, None)));
        assert_eq!(
            mesh_alpha_policy(&[NO_ALPHA, BLEND, NO_ALPHA]),
            Some((Blend, None))
        );
        assert_eq!(
            mesh_alpha_policy(&[tested(90), tested(128), NO_ALPHA]),
            Some((Cutout, Some(128)))
        );
        assert_eq!(
            mesh_alpha_policy(&[tested(90), BLEND, BLEND, BLEND]),
            Some((CutoutBlend, Some(90)))
        );
        let both = MeshAlpha {
            blend: true,
            test: true,
            threshold: 100,
        };
        assert_eq!(
            mesh_alpha_policy(&[both, NO_ALPHA]),
            Some((CutoutBlend, Some(100)))
        );
    }

    /// Note U6 + owner O2: a NIF threshold outside sdc's 1..=254 is clamped into it (a NIF may say 0 or 255).
    #[test]
    fn mesh_threshold_is_clamped_to_the_sdc_range() {
        assert_eq!(
            mesh_alpha_policy(&[tested(0)]),
            Some((AlphaPolicy::Cutout, Some(1)))
        );
        assert_eq!(
            mesh_alpha_policy(&[tested(255), tested(30)]),
            Some((AlphaPolicy::Cutout, Some(254)))
        );
    }

    /// 2026-10-13: a set with a complex_grass member picks `complex_grass` (ENB and CS), before the mesh majority
    /// and the landscape path; vanilla has it too since Note AZ (ruling R1 reversed).
    #[test]
    fn complex_grass_is_picked_by_its_member() {
        use Subtype::*;
        let s = sx(&[]);
        let st = [ComplexGrass];
        let paths = ["textures/landscape/grass/g.dds"];
        let mesh = [Shader::Landscape, Shader::Landscape];
        for t in [Target::Cs, Target::Vanilla] {
            let mut i = input(t, &s, &st, &paths);
            i.mesh = &mesh;
            let p = pick(&i);
            assert_eq!(
                auto(&p),
                (
                    Shader::ComplexGrass,
                    ShaderOrigin::Auto(AutoRule::ComplexGrass)
                ),
                "{t:?}"
            );
        }
        assert!(Shader::available(Target::Vanilla).contains(&Shader::ComplexGrass));
        assert_eq!(Shader::ComplexGrass.id(), "complex_grass");
        assert_eq!(Shader::ComplexGrass.label(), "Complex grass");
    }

    /// Note BM: a CS PBR set on a landscape path or with a texture-set block picks `pbr_landscape`, ahead of the mesh
    /// vote; GRAS evidence and an explicit choice win; vanilla and non-PBR landscape sets are unchanged.
    #[test]
    fn pbr_landscape_is_picked_for_cs_pbr_landscape_sets() {
        use Subtype::*;
        assert_eq!(
            (Shader::PbrLandscape.id(), Shader::PbrLandscape.label()),
            ("pbr_landscape", "PBR Landscape")
        );
        let spec = Shader::PbrLandscape.spec();
        let labels = |roles: &[MapRole]| roles.iter().map(|r| r.label).collect::<Vec<_>>();
        assert_eq!(labels(spec.required), ["albedo", "_n"]);
        assert_eq!(labels(spec.optional), ["_rmaos", "_p"]);
        assert!(!Shader::PbrLandscape.has_member_defaults());
        for st in [PbrAlbedo, Diffuse, LandscapeDiffuse, Rmaos, Height] {
            assert!(member_defaults(Shader::PbrLandscape, st).is_empty());
            assert_eq!(member_subtype(Shader::PbrLandscape, st), None);
        }
        let s = sx(&["n", "rmaos"]);
        let st = [PbrAlbedo, PbrNormal, Rmaos];
        let want = (
            Shader::PbrLandscape,
            ShaderOrigin::Auto(AutoRule::PbrLandscape),
        );
        for p in [
            "textures/landscape/dirt.png",
            "textures/mod/landscape/d.png",
        ] {
            let paths = [p];
            let mut i = input(Target::Cs, &s, &st, &paths);
            i.mesh = &[Shader::Landscape];
            assert_eq!(auto(&pick(&i)), want, "{p}");
            i.gras = true;
            assert_eq!(pick(&i).shader, Shader::PbrGrass);
            i.gras = false;
            i.explicit = Some(Shader::Pbr);
            assert_eq!(auto(&pick(&i)), (Shader::Pbr, ShaderOrigin::Set));
            i.explicit = None;
            i.target = Target::Vanilla;
            assert_eq!(pick(&i).wanted, Some(Shader::Pbr), "vanilla unchanged");
        }
        let paths = ["textures/rocks/r.png"];
        let mut i = input(Target::Cs, &s, &st, &paths);
        assert_eq!(pick(&i).shader, Shader::Pbr);
        i.cs_txst = true;
        assert_eq!(auto(&pick(&i)), want);
        // A texture-set block alone does not make a non-PBR set PBR landscape.
        let plain = [Diffuse, Normal];
        let mut j = input(Target::Cs, &s, &plain, &paths);
        j.cs_txst = true;
        assert_eq!(pick(&j).shader, Shader::Default);
        let lpaths = ["textures/landscape/dirt.png"];
        let k = input(Target::Cs, &s, &plain, &lpaths);
        assert_eq!(pick(&k).shader, Shader::Landscape);
        assert_eq!(
            "auto:pbr_landscape".parse::<ShaderOrigin>().unwrap(),
            ShaderOrigin::Auto(AutoRule::PbrLandscape)
        );
        // `_rmaos` is optional and a landscape diffuse fills the albedo: no map warnings.
        let mm = |name: &str, st| MemberMap {
            name: name.into(),
            suffix: None,
            subtype: Some(st),
        };
        let p = ShaderPick {
            shader: Shader::PbrLandscape,
            provenance: ShaderOrigin::Set,
            wanted: None,
        };
        assert!(check_maps(
            &p,
            Target::Cs,
            &[mm("d", LandscapeDiffuse), mm("n", Normal)]
        )
        .is_empty());
    }

    /// PBR grass design §4.1-4.2: four maps; a PBR set with GRAS evidence picks it; an explicit choice wins; CS only.
    #[test]
    fn pbr_grass_needs_a_pbr_set_and_gras_evidence() {
        use Subtype::*;
        assert_eq!(
            (Shader::PbrGrass.id(), Shader::PbrGrass.label()),
            ("pbr_grass", "PBR grass")
        );
        let spec = Shader::PbrGrass.spec();
        let labels = |roles: &[MapRole]| roles.iter().map(|r| r.label).collect::<Vec<_>>();
        assert_eq!(labels(spec.required), ["albedo", "_n", "_rmaos"]);
        assert_eq!(labels(spec.optional), ["subsurface _s"]);
        let s = sx(&["n", "rmaos"]);
        let st = [PbrAlbedo, PbrNormal, Rmaos];
        let paths = ["textures/plants/g.png"];
        let mut i = input(Target::Cs, &s, &st, &paths);
        i.pbr = true;
        assert_eq!(
            auto(&pick(&i)),
            (Shader::Pbr, ShaderOrigin::Auto(AutoRule::Pbr))
        );
        i.gras = true;
        assert_eq!(
            auto(&pick(&i)),
            (Shader::PbrGrass, ShaderOrigin::Auto(AutoRule::PbrGrass))
        );
        assert_eq!(
            "auto:pbr_grass".parse::<ShaderOrigin>().unwrap(),
            ShaderOrigin::Auto(AutoRule::PbrGrass)
        );
        i.explicit = Some(Shader::Pbr);
        assert_eq!(auto(&pick(&i)), (Shader::Pbr, ShaderOrigin::Set));
        i.explicit = None;
        i.target = Target::Vanilla;
        assert_eq!(
            pick(&i),
            ShaderPick {
                shader: Shader::Default,
                provenance: ShaderOrigin::Fallback,
                wanted: Some(Shader::PbrGrass)
            }
        );
        // GRAS evidence alone does not make a set PBR grass.
        let (s2, st2) = (sx(&["n"]), [Diffuse, Normal]);
        let mut j = input(Target::Cs, &s2, &st2, &paths);
        j.gras = true;
        assert_ne!(pick(&j).shader, Shader::PbrGrass);
    }

    /// PBR grass design §4.1: `_g`, `_p`, `_cnr` and `_f` are maps the grass shader cannot read.
    #[test]
    fn pbr_grass_reports_the_maps_grass_cannot_read() {
        let p = ShaderPick {
            shader: Shader::PbrGrass,
            provenance: ShaderOrigin::Set,
            wanted: None,
        };
        let m = |name: &str, st| MemberMap {
            name: name.into(),
            suffix: None,
            subtype: Some(st),
        };
        let w = check_maps(
            &p,
            Target::Cs,
            &[
                m("g", Subtype::PbrAlbedo),
                m("g_n", Subtype::PbrNormal),
                m("g_rmaos", Subtype::Rmaos),
                m("g_s", Subtype::PbrSubsurface),
                m("g_g", Subtype::PbrEmissive),
            ],
        );
        assert_eq!(
            w.iter().map(|w| w.code).collect::<Vec<_>>(),
            [Code::ShaderExtraMap]
        );
        assert_eq!(w[0].data["member"], "g_g");
    }

    /// PBR grass design §4.4: the albedo of a pbr_grass set whose corner reads as a unit vector (0.01 to spare).
    #[test]
    fn a_pbr_grass_albedo_that_reads_as_complex_grass_is_noted() {
        let grass = ShaderPick {
            shader: Shader::PbrGrass,
            provenance: ShaderOrigin::Set,
            wanted: None,
        };
        let pbr = ShaderPick {
            shader: Shader::Pbr,
            ..grass.clone()
        };
        let note = |p: &ShaderPick, st, e| pbr_grass_albedo_note(p, st, e).map(|w| w.code);
        assert_eq!(
            note(&grass, Subtype::PbrAlbedo, 0.0),
            Some(Code::PbrGrassDetect)
        );
        assert_eq!(
            note(&grass, Subtype::Diffuse, 0.039),
            Some(Code::PbrGrassDetect)
        );
        assert_eq!(note(&grass, Subtype::PbrAlbedo, 0.05), None);
        assert_eq!(note(&grass, Subtype::PbrAlbedo, f32::INFINITY), None);
        assert_eq!(note(&grass, Subtype::PbrNormal, 0.0), None);
        assert_eq!(note(&pbr, Subtype::PbrAlbedo, 0.0), None);
    }
}
