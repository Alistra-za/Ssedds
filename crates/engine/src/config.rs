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
/// A texture sub-type. `skin_rfaos` was `rfaos` before the Advanced skin sub-types were prefixed (DECISIONS Note
/// R-skin-1): the old id still loads, and the new one is written back.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
#[schemars(transform = subtype_schema)]
pub enum Subtype {
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
    #[serde(alias = "rfaos")]
    SkinRfaos,
    Flow,
    Height,
    MsnSpecular,
    GenericTwoChannel,
    GenericData,
    HdrSky,
    SkinAlbedo,
    SkinSk,
    SkinWet,
}

/// The schema lists the deprecated id `rfaos` next to `skin_rfaos`, so an old file still validates.
fn subtype_schema(s: &mut schemars::Schema) {
    if let Some(serde_json::Value::Array(ids)) = s.get_mut("enum") {
        ids.push("rfaos".into());
    }
    s.insert(
        "description".into(),
        "A texture sub-type. `rfaos` is a deprecated alias of `skin_rfaos`: it still loads and is written back as \
         `skin_rfaos`."
            .into(),
    );
}
cfg_enum!(ColorContent, "lowercase" { Srgb, Linear });
cfg_enum!(ColorTag, "lowercase" { Srgb, Unorm, NotApplicable = "n/a" });
cfg_enum!(AlphaPolicy, "snake_case" { Remove, Cutout, Blend, CutoutBlend, Data });
cfg_enum!(AlphaCompare, "lowercase" { Gt, Ge });
cfg_enum!(GraySource, "lowercase" { R, Luma });
cfg_enum!(MipKeyword, "lowercase" { Full, None });
cfg_enum!(MipFilter, "lowercase" { Box });
cfg_enum!(NormalMips, "snake_case" { Renormalize, RenormalizeBake });
cfg_enum!(NormalBase, "lowercase" { Renormalize, Keep });
cfg_enum!(CoverageMode, "lowercase" { Auto, On, Off });
cfg_enum!(Format, "lowercase" { Bc1, Bc3, Bc4, Bc5, Bc6hUf16 = "bc6h_uf16", Bc7 });
cfg_enum!(HeaderForm, "lowercase" { Auto, Dx10, Legacy });
cfg_enum!(Backend, "snake_case" { Bc7e, Rgbcx, IspcBc6h, Nvtt });
cfg_enum!(Effort, "lowercase" { Ultrafast, Veryfast, Fast, Basic, Veryslow, Slowest });
cfg_enum!(SearchRadius, "lowercase" { Default, Max });
cfg_enum!(GreenConvention, "lowercase" { Directx, Opengl });
cfg_enum!(RawMap, "lowercase" { Roughness, Metallic, Ao, Specular, Glossiness, Height, Opacity, Fuzz });

impl RawMap {
    pub const ALL: [RawMap; 8] = {
        use RawMap::*;
        [
            Roughness, Metallic, Ao, Specular, Glossiness, Height, Opacity, Fuzz,
        ]
    };

    /// The role id a raw map counts as after Discover (spec §4.4): `roughness`, `ao`, …
    pub fn id(self) -> String {
        serde_json::to_value(self)
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned()
    }

    /// Spec §4.3 item 1, §6.4: an input that starts the cs `_rmaos`/`_rfaos` recipe, so its set is PBR.
    pub fn starts_rmaos(self) -> bool {
        matches!(
            self,
            RawMap::Roughness | RawMap::Glossiness | RawMap::Metallic | RawMap::Ao
        )
    }
}

cfg_enum!(PackPrefer, "lowercase" { Prepacked, Raw });

/// Which cs recipe a set packs (DECISIONS Note R-fills-1): `auto` = `_rfaos` when a fuzz source exists, else
/// `_rmaos`; `rfaos`/`rmaos` force one.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum PackRecipe {
    #[default]
    Auto,
    Rmaos,
    Rfaos,
}

impl PackRecipe {
    pub fn id(self) -> &'static str {
        match self {
            PackRecipe::Auto => "auto",
            PackRecipe::Rmaos => "rmaos",
            PackRecipe::Rfaos => "rfaos",
        }
    }

    pub fn is_auto(&self) -> bool {
        *self == PackRecipe::Auto
    }
}

/// Global fill values for packed channels without a source map (spec §6.4, DECISIONS Notes H and M5): an 8-bit
/// code per §4.4 raw-map role, `None` = the neutral default. Unset fills are not serialised.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FillSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roughness: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metallic: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ao: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub specular: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glossiness: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fuzz: Option<u8>,
}

impl FillSpec {
    pub fn get(&self, role: RawMap) -> Option<u8> {
        match role {
            RawMap::Roughness => self.roughness,
            RawMap::Metallic => self.metallic,
            RawMap::Ao => self.ao,
            RawMap::Specular => self.specular,
            RawMap::Glossiness => self.glossiness,
            RawMap::Height => self.height,
            RawMap::Opacity => self.opacity,
            RawMap::Fuzz => self.fuzz,
        }
    }
}

/// Channel-packing settings (spec §3.2, §6.4). Fields of the packed output's profile, resolved like any other.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PackingSpec {
    /// Pack this output's recipe; `false` leaves its raw maps as packing inputs (`IN-PACKING-INPUT`).
    pub enabled: bool,
    /// Neutral roughness (0..=1) for a channel without a roughness or glossiness source.
    pub roughness_default: f64,
    /// Which side wins when the set already holds the packed output as a file (`PACK-CONFLICT`).
    pub prefer: PackPrefer,
    #[serde(default)]
    pub fill: FillSpec,
    /// Which cs recipe this set packs (`auto`, `rmaos`, `rfaos`; DECISIONS Note R-fills-1). Set per set at either packed key,
    /// `[files."<folder/base>_rfaos"]` or `..._rmaos"]`.
    #[serde(default, skip_serializing_if = "PackRecipe::is_auto")]
    pub recipe: PackRecipe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum MipLevels {
    Keyword(MipKeyword),
    Count(u32),
}

impl Subtype {
    pub const ALL: [Subtype; 28] = {
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
            SkinRfaos,
            Flow,
            Height,
            MsnSpecular,
            GenericTwoChannel,
            GenericData,
            HdrSky,
            SkinAlbedo,
            SkinSk,
            SkinWet,
        ]
    };

    pub fn category(self) -> Category {
        use Subtype::*;
        match self {
            Diffuse | LandscapeDiffuse | PbrAlbedo | PbrEmissive | Glow | PbrSubsurface
            | PbrFuzz | SubsurfaceTint | BacklightTint | UiSky | SkinAlbedo => Category::Color,
            Normal | LandscapeNormal | PbrNormal | Msn | CoatNormal | SkinWet => Category::Normal,
            Envmask | Height | MsnSpecular | SkinSk => Category::Grayscale,
            ComplexMaterial | Rmaos | SkinRfaos | Flow | GenericTwoChannel | GenericData => {
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
        matches!(
            self,
            Subtype::Diffuse | Subtype::PbrAlbedo | Subtype::UiSky | Subtype::SkinAlbedo
        )
    }

    pub fn is_roughness_member(self) -> bool {
        matches!(self, Subtype::Rmaos | Subtype::SkinRfaos)
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
    pub fn id(self) -> String {
        serde_json::to_value(self)
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned()
    }
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
    /// Green (Y) convention of a normal source (spec §3.2, §4.4, DECISIONS Note H); None = unstated.
    /// `opengl` makes Resolve set `flip_green`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub green_convention: Option<GreenConvention>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MipSpec {
    pub levels: MipLevels,
    pub filter: MipFilter,
    pub normal: NormalMips,
    /// Mip 0 of a normal map (spec §5.4, DECISIONS R1): `renormalize` (default) stores every base vector at unit
    /// length; `keep` stores the source as authored. Read only for the Normal category.
    #[serde(default = "default_normal_base")]
    pub normal_base: NormalBase,
    /// Clamp `t` of the variance bake (spec §6.2, DECISIONS Note E): the added variance is at most `t²`. 0.25;
    /// allowed (0, 1]. Read from the profile the bake writes into: a roughness member, or a `_cnr`.
    #[serde(default = "default_bake_clamp")]
    pub bake_clamp: f64,
    /// Coverage preservation (spec §5.5). It only ever applies to the alpha-tested policies `cutout` and
    /// `cutout_blend`: `auto` and `on` both enable it there, `off` disables it, and on any other policy (`blend`,
    /// `data`, `remove`) every mode is a no-op, because blended alpha must keep box-averaged opacity.
    pub coverage: CoverageMode,
}

fn default_normal_base() -> NormalBase {
    NormalBase::Renormalize
}

fn default_bake_clamp() -> f64 {
    0.25
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
    pub packing: PackingSpec,
}

impl Profile {
    /// Coverage preservation: on for `cutout` and `cutout_blend` unless `mips.coverage = off` (DECISIONS Note E).
    /// Never on for other policies, even with `mips.coverage = on` (spec §5.5: blended alpha keeps box-averaged
    /// opacity).
    pub fn coverage_enabled(&self) -> bool {
        self.mips.coverage != CoverageMode::Off
            && matches!(
                self.alpha.policy,
                AlphaPolicy::Cutout | AlphaPolicy::CutoutBlend
            )
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
        assert_eq!(Subtype::ALL.len(), 28);
        assert_eq!(Subtype::PbrEmissive.category(), Category::Color);
        assert_eq!(Subtype::Msn.category(), Category::Normal);
        assert_eq!(Subtype::Envmask.category(), Category::Grayscale);
        assert_eq!(Subtype::Flow.category(), Category::MixedPacked);
        assert_eq!(Subtype::HdrSky.category(), Category::Hdr);
        assert!(Subtype::SkinRfaos.is_roughness_member());
        assert!(Subtype::PbrFuzz.is_pbr_color());
        assert!(!Subtype::Glow.is_pbr_color());
        assert_eq!(Subtype::ComplexMaterial.id(), "complex_material");
    }

    #[test]
    fn mip_spec_defaults_the_m4_fields() {
        let m: MipSpec = serde_json::from_str(
            r#"{"levels":"full","filter":"box","normal":"renormalize","coverage":"auto"}"#,
        )
        .unwrap();
        assert_eq!(
            (m.normal_base, m.bake_clamp),
            (NormalBase::Renormalize, 0.25)
        );
        assert_eq!(
            serde_json::to_string(&NormalBase::Keep).unwrap(),
            "\"keep\""
        );
    }

    #[test]
    fn schema_lists_enum_values() {
        let s = serde_json::to_string(&schemars::schema_for!(Profile)).unwrap();
        for needle in [
            "bc6h_uf16",
            "n/a",
            "cutout_blend",
            "renormalize_bake",
            "normal_base",
            "keep",
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
        assert_eq!(ids.len(), 28);
    }

    /// M6: coverage preservation only ever applies to alpha-tested policies; `on` cannot force it onto blended
    /// or data alpha (spec §5.5: blended alpha keeps box-averaged opacity).
    #[test]
    fn coverage_applies_only_to_cutout_policies() {
        let reg = crate::presets::PresetRegistry::builtin().unwrap();
        let mut p = reg.base_profile(Target::Cs, Subtype::Diffuse);
        for (mode, policy, want) in [
            (CoverageMode::On, AlphaPolicy::Blend, false),
            (CoverageMode::On, AlphaPolicy::Data, false),
            (CoverageMode::On, AlphaPolicy::Remove, false),
            (CoverageMode::On, AlphaPolicy::Cutout, true),
            (CoverageMode::On, AlphaPolicy::CutoutBlend, true),
            (CoverageMode::Auto, AlphaPolicy::Cutout, true),
            (CoverageMode::Auto, AlphaPolicy::CutoutBlend, true),
            (CoverageMode::Auto, AlphaPolicy::Blend, false),
            (CoverageMode::Off, AlphaPolicy::Cutout, false),
            (CoverageMode::Off, AlphaPolicy::CutoutBlend, false),
        ] {
            p.mips.coverage = mode;
            p.alpha.policy = policy;
            assert_eq!(p.coverage_enabled(), want, "{mode:?} {policy:?}");
        }
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
