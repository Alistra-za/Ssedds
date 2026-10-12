//! M6 Track A (spec 2026-10-11-sdc-m6-shader-modes-design.md, DECISIONS Note S): the per-set shader table as data, auto-pick, member defaults and the extra/missing map check. The CLI and GUI read it; neither duplicates it.
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::config::{Subtype, Target};
use crate::suggest::has_component;

/// A Skyrim shader a texture set is made for (spec §3).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Shader {
    Pbr,
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
static LANDSCAPE: ShaderSpec = ShaderSpec {
    required: &[
        role("diffuse", &[Subtype::LandscapeDiffuse, Subtype::Diffuse]),
        role("_n", &[Subtype::LandscapeNormal, Subtype::Normal]),
    ],
    optional: &[],
};

impl Shader {
    pub const ALL: [Shader; 12] = [
        Shader::Pbr,
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
    ];

    pub fn id(self) -> &'static str {
        match self {
            Shader::Pbr => "pbr",
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
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Shader::Pbr => "PBR",
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
        }
    }

    /// The shaders a target can use: CS has all 12 (`pbr` first); Vanilla and CPM lack `pbr` and `advanced_skin`.
    pub fn available(t: Target) -> &'static [Shader] {
        match t {
            Target::Cs => &Shader::ALL,
            // `ALL` starts with `pbr`, `advanced_skin`.
            Target::Vanilla | Target::Cpm => &Shader::ALL[2..],
        }
    }

    pub fn is_available(self, t: Target) -> bool {
        Shader::available(t).contains(&self)
    }

    pub fn default_for(t: Target) -> Shader {
        match t {
            Target::Cs => Shader::Pbr,
            Target::Vanilla | Target::Cpm => Shader::Default,
        }
    }

    pub fn spec(self) -> &'static ShaderSpec {
        match self {
            Shader::Pbr => &PBR,
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
/// gives just `stem`. The database is asked for `<key>.dds`.
pub fn mesh_key(game_path: &str, stem: &str) -> String {
    match game_path.rsplit_once('/') {
        Some((dir, _)) => format!("{dir}/{stem}"),
        None => stem.to_owned(),
    }
    .to_lowercase()
}

/// The auto-pick rule that chose a shader (spec §4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoRule {
    AdvancedSkin,
    Pbr,
    Mesh,
    ComplexM,
    EnvM,
    P,
    G,
    I,
    Sk,
    HairPath,
    EyesPath,
    Landscape,
    Otherwise,
}

impl AutoRule {
    const ALL: [AutoRule; 13] = [
        AutoRule::AdvancedSkin,
        AutoRule::Pbr,
        AutoRule::Mesh,
        AutoRule::ComplexM,
        AutoRule::EnvM,
        AutoRule::P,
        AutoRule::G,
        AutoRule::I,
        AutoRule::Sk,
        AutoRule::HairPath,
        AutoRule::EyesPath,
        AutoRule::Landscape,
        AutoRule::Otherwise,
    ];

    pub fn id(self) -> &'static str {
        match self {
            AutoRule::AdvancedSkin => "advanced_skin",
            AutoRule::Pbr => "pbr",
            AutoRule::Mesh => "mesh",
            AutoRule::ComplexM => "complex_m",
            AutoRule::EnvM => "env_m",
            AutoRule::P => "p",
            AutoRule::G => "g",
            AutoRule::I => "i",
            AutoRule::Sk => "sk",
            AutoRule::HairPath => "hair_path",
            AutoRule::EyesPath => "eyes_path",
            AutoRule::Landscape => "landscape",
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
/// skin, PBR, then the PGPatcher-patchable maps (complex `_m`, `_p`), which beat the mesh, then the mesh majority
/// (S-shader-10), then envmap `_m`, `_g`, `_i`, `_sk`, hair/eyes/landscape paths and the default.
pub fn pick(i: &PickInput) -> ShaderPick {
    use Subtype::*;
    let has = |t: &[Subtype]| i.subtypes.iter().any(|s| t.contains(s));
    let sfx = |s: &str| i.suffixes.contains(s);
    let path = |c: &str| {
        i.game_paths
            .iter()
            .any(|p| has_component(&p.to_ascii_lowercase(), c))
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
        (Shader::Pbr, AutoRule::Pbr)
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
    } else if path("hair") {
        (Shader::Hair, AutoRule::HairPath)
    } else if path("eyes") {
        (Shader::Eye, AutoRule::EyesPath)
    } else if has(&[LandscapeDiffuse])
        || i.game_paths
            .iter()
            .any(|p| p.to_ascii_lowercase().starts_with("textures/landscape/"))
        || path("landscape")
    {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Subtype, Target};

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
        assert_eq!(Shader::available(Target::Cs).len(), 12);
        assert_eq!(Shader::available(Target::Cs)[0], Shader::Pbr);
        for t in [Target::Vanilla, Target::Cpm] {
            let l = Shader::available(t);
            assert_eq!(l.len(), 10, "{t:?}");
            assert!(!l.contains(&Shader::Pbr) && !l.contains(&Shader::AdvancedSkin));
            assert_eq!(Shader::default_for(t), Shader::Default);
        }
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
                Shader::Envmap,
                Shader::Parallax,
                Shader::Hair,
                Shader::Eye,
                Shader::Multilayer,
                Shader::Landscape
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
        // cpm uses the vanilla list.
        i.target = Target::Cpm;
        assert_eq!(pick(&i).shader, Shader::Default);
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
}
