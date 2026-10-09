//! The configuration model (spec §3.2): one set of types for CLI, GUI, presets and the published schema.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

macro_rules! cfg_enum {
    ($(#[$m:meta])* $name:ident, $case:literal { $($v:ident $(= $ren:literal)?),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
        #[serde(rename_all = $case)]
        $(#[$m])*
        pub enum $name { $($(#[serde(rename = $ren)])? $v),+ }
    };
}

cfg_enum!(Target, "lowercase" { Cs, Cpm, Vanilla });
cfg_enum!(Category, "snake_case" { Color, Normal, MixedPacked, Grayscale, Hdr });
cfg_enum!(Subtype, "snake_case" {
    Diffuse, LandscapeDiffuse, PbrAlbedo, PbrEmissive, Glow, PbrSubsurface, PbrFuzz, SubsurfaceTint,
    BacklightTint, UiSky, Normal, LandscapeNormal, PbrNormal, Msn, CoatNormal, Envmask, ComplexMaterial,
    Rmaos, Rfaos, Flow, Height, MsnSpecular, GenericTwoChannel, GenericData, HdrSky
});
cfg_enum!(ColorContent, "lowercase" { Srgb, Linear });
cfg_enum!(ColorTag, "lowercase" { Srgb, Unorm, NotApplicable = "n/a" });
cfg_enum!(AlphaPolicy, "snake_case" { Remove, Cutout, Blend, CutoutBlend, Data });
cfg_enum!(AlphaCompare, "lowercase" { Gt, Ge });
cfg_enum!(GraySource, "lowercase" { R, Luma });
cfg_enum!(MipKeyword, "lowercase" { Full, None });
cfg_enum!(MipFilter, "lowercase" { Box });
cfg_enum!(NormalMips, "snake_case" { Renormalize, RenormalizeBake });
cfg_enum!(CoverageMode, "lowercase" { Auto, On, Off });
cfg_enum!(Format, "lowercase" { Bc1, Bc3, Bc4, Bc5, Bc6hUf16 = "bc6h_uf16", Bc7 });
cfg_enum!(HeaderForm, "lowercase" { Auto, Dx10, Legacy });
cfg_enum!(Backend, "snake_case" { Bc7e, Rgbcx, IspcBc6h, Nvtt });
cfg_enum!(Effort, "lowercase" { Ultrafast, Veryfast, Fast, Basic, Veryslow, Slowest });
cfg_enum!(SearchRadius, "lowercase" { Default, Max });

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum MipLevels {
    Keyword(MipKeyword),
    Count(u32),
}

impl Subtype {
    pub const ALL: [Subtype; 25] = {
        use Subtype::*;
        [
            Diffuse,
            LandscapeDiffuse,
            PbrAlbedo,
            PbrEmissive,
            Glow,
            PbrSubsurface,
            PbrFuzz,
            SubsurfaceTint,
            BacklightTint,
            UiSky,
            Normal,
            LandscapeNormal,
            PbrNormal,
            Msn,
            CoatNormal,
            Envmask,
            ComplexMaterial,
            Rmaos,
            Rfaos,
            Flow,
            Height,
            MsnSpecular,
            GenericTwoChannel,
            GenericData,
            HdrSky,
        ]
    };

    pub fn category(self) -> Category {
        use Subtype::*;
        match self {
            Diffuse | LandscapeDiffuse | PbrAlbedo | PbrEmissive | Glow | PbrSubsurface
            | PbrFuzz | SubsurfaceTint | BacklightTint | UiSky => Category::Color,
            Normal | LandscapeNormal | PbrNormal | Msn | CoatNormal => Category::Normal,
            Envmask | Height | MsnSpecular => Category::Grayscale,
            ComplexMaterial | Rmaos | Rfaos | Flow | GenericTwoChannel | GenericData => {
                Category::MixedPacked
            }
            HdrSky => Category::Hdr,
        }
    }

    /// PBR colour data: the only sub-types that may carry `_SRGB` (cs only) [D#4, D#5].
    pub fn is_pbr_color(self) -> bool {
        matches!(
            self,
            Subtype::PbrAlbedo | Subtype::PbrEmissive | Subtype::PbrSubsurface | Subtype::PbrFuzz
        )
    }

    /// Alpha meaning depends on the mesh (NiAlphaProperty), so it is never a high-confidence guess [SC §1].
    pub fn alpha_from_mesh(self) -> bool {
        matches!(self, Subtype::Diffuse | Subtype::PbrAlbedo | Subtype::UiSky)
    }

    pub fn is_roughness_member(self) -> bool {
        matches!(self, Subtype::Rmaos | Subtype::Rfaos)
    }

    pub fn id(self) -> String {
        serde_json::to_value(self)
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned()
    }
}

impl Format {
    pub fn block_bytes(self) -> usize {
        match self {
            Format::Bc1 | Format::Bc4 => 8,
            _ => 16,
        }
    }
    pub fn has_alpha(self) -> bool {
        matches!(self, Format::Bc1 | Format::Bc3 | Format::Bc7)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ColorSpec {
    pub content: ColorContent,
    pub tag: ColorTag,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AlphaSpec {
    pub policy: AlphaPolicy,
    /// 8-bit alpha-test threshold, per texture; allowed 5..=200 (checked in policy).
    pub threshold: u8,
    pub compare: AlphaCompare,
    pub exact_opaque: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChannelSpec {
    /// Four characters from r, g, b, a, 0, 1 (e.g. "bgra", "rrr1"); None = identity.
    #[serde(default)]
    pub swizzle: Option<String>,
    pub flip_green: bool,
    pub reconstruct_z: bool,
    pub grayscale_source: GraySource,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MipSpec {
    pub levels: MipLevels,
    pub filter: MipFilter,
    pub normal: NormalMips,
    pub coverage: CoverageMode,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EncoderSpec {
    pub backend: Backend,
    pub effort: Effort,
    pub rgbcx_level: u32,
    pub perceptual: bool,
    pub bc1_transparent_black: bool,
    pub bc45_search_radius: SearchRadius,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ValidationSpec {
    pub min_mip_psnr_db: f64,
    pub coverage_tolerance_pts: f64,
    pub complex_m_warn_alpha: u8,
    pub bc6h_min_peak_ratio: f64,
    pub pgpatcher_non_white_share: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub target: Target,
    pub category: Category,
    pub subtype: Subtype,
    pub color: ColorSpec,
    pub alpha: AlphaSpec,
    pub channels: ChannelSpec,
    pub mips: MipSpec,
    pub format: Format,
    pub header: HeaderForm,
    pub encoder: EncoderSpec,
    pub validation: ValidationSpec,
}

impl Profile {
    /// Coverage preservation: `auto` = on for cutout and cutout_blend (DECISIONS Note E).
    pub fn coverage_enabled(&self) -> bool {
        match self.mips.coverage {
            CoverageMode::On => true,
            CoverageMode::Off => false,
            CoverageMode::Auto => matches!(
                self.alpha.policy,
                AlphaPolicy::Cutout | AlphaPolicy::CutoutBlend
            ),
        }
    }
    pub fn alpha_weighted_mips(&self) -> bool {
        matches!(
            self.alpha.policy,
            AlphaPolicy::Cutout | AlphaPolicy::Blend | AlphaPolicy::CutoutBlend
        )
    }
    pub fn srgb_tag(&self) -> bool {
        self.color.tag == ColorTag::Srgb
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_names_match_the_spec() {
        assert_eq!(
            serde_json::to_string(&Format::Bc6hUf16).unwrap(),
            "\"bc6h_uf16\""
        );
        assert_eq!(
            serde_json::to_string(&ColorTag::NotApplicable).unwrap(),
            "\"n/a\""
        );
        assert_eq!(
            serde_json::to_string(&AlphaPolicy::CutoutBlend).unwrap(),
            "\"cutout_blend\""
        );
        assert_eq!(
            serde_json::to_string(&Subtype::MsnSpecular).unwrap(),
            "\"msn_specular\""
        );
        assert_eq!(
            serde_json::from_str::<MipLevels>("\"full\"").unwrap(),
            MipLevels::Keyword(MipKeyword::Full)
        );
        assert_eq!(
            serde_json::from_str::<MipLevels>("3").unwrap(),
            MipLevels::Count(3)
        );
    }

    #[test]
    fn subtypes_map_to_the_categories_of_section_4_1() {
        assert_eq!(Subtype::ALL.len(), 25);
        assert_eq!(Subtype::PbrEmissive.category(), Category::Color);
        assert_eq!(Subtype::Msn.category(), Category::Normal);
        assert_eq!(Subtype::Envmask.category(), Category::Grayscale);
        assert_eq!(Subtype::Flow.category(), Category::MixedPacked);
        assert_eq!(Subtype::HdrSky.category(), Category::Hdr);
        assert!(Subtype::Rfaos.is_roughness_member());
        assert!(Subtype::PbrFuzz.is_pbr_color());
        assert!(!Subtype::Glow.is_pbr_color());
        assert_eq!(Subtype::ComplexMaterial.id(), "complex_material");
    }

    #[test]
    fn schema_lists_enum_values() {
        let s = serde_json::to_string(&schemars::schema_for!(Profile)).unwrap();
        for needle in [
            "bc6h_uf16",
            "n/a",
            "cutout_blend",
            "renormalize_bake",
            "ispc_bc6h",
        ] {
            assert!(s.contains(needle), "{needle} missing from schema");
        }
    }

    #[test]
    fn subtype_ids_are_unique_and_round_trip() {
        let mut ids: Vec<String> = Subtype::ALL.iter().map(|s| s.id()).collect();
        for s in Subtype::ALL {
            let back: Subtype = serde_json::from_str(&format!("\"{}\"", s.id())).unwrap();
            assert_eq!(back, s);
        }
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 25);
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let err = serde_json::from_str::<ColorSpec>(r#"{"content":"srgb","tag":"srgb","extra":1}"#);
        assert!(err.is_err());
        assert_eq!(Format::Bc7.block_bytes(), 16);
        assert_eq!(Format::Bc4.block_bytes(), 8);
        assert!(!Format::Bc5.has_alpha());
    }
}
