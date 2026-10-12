//! M6 Track A (spec 2026-10-11-sdc-m6-shader-modes-design.md, DECISIONS Note S): the per-set shader table as data, auto-pick, member defaults and the extra/missing map check. The CLI and GUI read it; neither duplicates it.
use std::fmt;
use std::str::FromStr;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::config::{Subtype, Target};

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
}
