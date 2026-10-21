//! The configuration model (spec §3.2): one set of types for CLI, GUI, presets and the published schema.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

macro_rules! cfg_enum {
    ($(#[$m:meta])* $name:ident, $case:literal { $($(#[$vm:meta])* $v:ident $(= $ren:literal)?),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
        #[serde(rename_all = $case)]
        $(#[$m])*
        pub enum $name { $($(#[$vm])* $(#[serde(rename = $ren)])? $v),+ }
    };
}

cfg_enum!(Target, "lowercase" { Cs, Vanilla });
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
    // Complex grass (design 2026-10-12): one 1:2 image, top = diffuse + alpha, bottom = normal + specular in alpha.
    // A `//` comment on purpose: a `///` doc on one variant makes schemars emit `oneOf` instead of the flat `enum`,
    // which rewrites every schema and drops the `rfaos` alias `subtype_schema` pushes into `enum`.
    ComplexGrass,
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
cfg_enum!(MipFilter, "lowercase" { Box, Tent, Mitchell, Kaiser });
cfg_enum!(#[derive(Default)] MipEdge, "lowercase" { #[default] Wrap, Clamp });
cfg_enum!(#[derive(Default)] ResizeMode, "kebab-case" { #[default] None, PotDown, Half });
impl MipEdge {
    pub fn is_wrap(&self) -> bool {
        *self == MipEdge::Wrap
    }
}
impl ResizeMode {
    pub fn is_none(&self) -> bool {
        *self == ResizeMode::None
    }
}
cfg_enum!(#[derive(Default)] ValidationMips, "lowercase" { #[default] All, Smallest });
impl ValidationMips {
    pub fn is_all(&self) -> bool {
        *self == ValidationMips::All
    }
}
cfg_enum!(NormalMips, "snake_case" { Renormalize, RenormalizeBake });
cfg_enum!(NormalBase, "lowercase" { Renormalize, Keep });
cfg_enum!(CoverageMode, "lowercase" { Auto, On, Off });
cfg_enum!(Format, "lowercase" { Bc1, Bc3, Bc4, Bc5, Bc6hUf16 = "bc6h_uf16", Bc7, Rgba8, R8, Rg8, Rgba16f });
cfg_enum!(HeaderForm, "lowercase" { Auto, Dx10, Legacy });
cfg_enum!(Backend, "snake_case" { Bc7e, Rgbcx, IspcBc6h, Nvtt, Uncompressed });
// bc7e quality level. `max` is sdc patch 0002 (veryslow's search plus a fully refined, wider partition ranking,
// uber 4, two least-squares passes and a quantised endpoint search); it needs the patch-0002 bc7e objects and is
// refused with `CFG-INVALID` without them (docs/research/bc7e-tuning.md).
cfg_enum!(Effort, "lowercase" { Ultrafast, Veryfast, Fast, Basic, Veryslow, Slowest, Max });
cfg_enum!(SearchRadius, "lowercase" { Default, Max, Fast });
cfg_enum!(GreenConvention, "lowercase" { Directx, Opengl });
cfg_enum!(
    /// One RNM normal layer (M6 Track F2 spec §2.1).
    LayerId, "lowercase" { Bump, Bump2, Detail, Detail2, Bent }
);
cfg_enum!(#[derive(Default)] BentMode, "snake_case" { #[default] AoBlend, Detail });
cfg_enum!(
    /// Which base normal a layer file feeds (F2 spec §2.3): `n` (`_n`), `cnr` (`_cnr`) or both.
    RnmTarget, "lowercase" { N, Cnr, Both }
);

impl LayerId {
    /// The default blend order (F2 spec §3.1 step 5).
    pub const ORDER: [LayerId; 5] = [
        LayerId::Bent,
        LayerId::Bump,
        LayerId::Bump2,
        LayerId::Detail,
        LayerId::Detail2,
    ];
    pub fn id(self) -> &'static str {
        match self {
            LayerId::Bump => "bump",
            LayerId::Bump2 => "bump2",
            LayerId::Detail => "detail",
            LayerId::Detail2 => "detail2",
            LayerId::Bent => "bent",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            LayerId::Bump => "bump",
            LayerId::Bump2 => "bump 2",
            LayerId::Detail => "detail",
            LayerId::Detail2 => "detail 2",
            LayerId::Bent => "bent",
        }
    }
    /// F2 spec §3.4 (rnm.py's defaults).
    pub fn default_strength(self) -> f64 {
        match self {
            LayerId::Bump | LayerId::Bump2 => 0.4,
            LayerId::Detail => 0.6,
            LayerId::Detail2 => 0.3,
            LayerId::Bent => 0.35,
        }
    }
}

impl BentMode {
    pub fn is_ao_blend(&self) -> bool {
        *self == BentMode::AoBlend
    }
}

fn yes() -> bool {
    true
}
fn is_true(b: &bool) -> bool {
    *b
}

/// One layer's settings (F2 spec §4). Every field is optional at its default: `strength` = the layer's
/// `default_strength`, `flip_green` = the base's own `channels.flip_green`, `mask` = on when the mask file exists.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LayerSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strength: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flip_green: Option<bool>,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub mask: bool,
}

impl Default for LayerSpec {
    fn default() -> Self {
        LayerSpec {
            strength: None,
            flip_green: None,
            mask: true,
        }
    }
}

impl LayerSpec {
    pub fn is_default(&self) -> bool {
        *self == LayerSpec::default()
    }
}

/// The `bent` layer: a layer plus its AO-blend settings (F2 spec §3.1, §3.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BentSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strength: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flip_green: Option<bool>,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub mask: bool,
    #[serde(default, skip_serializing_if = "BentMode::is_ao_blend")]
    pub mode: BentMode,
    /// The power on the occlusion `1 − AO`; `None` = 1.5.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ao_power: Option<f64>,
    /// Read the AO source; `false` = a uniform weight.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub ao: bool,
}

impl Default for BentSpec {
    fn default() -> Self {
        BentSpec {
            strength: None,
            flip_green: None,
            mask: true,
            mode: BentMode::AoBlend,
            ao_power: None,
            ao: true,
        }
    }
}

impl BentSpec {
    pub fn is_default(&self) -> bool {
        *self == BentSpec::default()
    }
}

/// A layer's resolved settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayerView {
    pub strength: f64,
    /// `None` = follow the base's `channels.flip_green`.
    pub flip_green: Option<bool>,
    pub mask: bool,
}

/// RNM normal layers (M6 Track F2 spec §4). A field of every profile, read only by base normals (`Subtype::rnm_base`);
/// not serialised at its default, so profile hashes written before F2 hold (DECISIONS Note W6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RnmSpec {
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    /// Blend order; see [`RnmSpec::effective_order`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<Vec<LayerId>>,
    #[serde(default, skip_serializing_if = "LayerSpec::is_default")]
    pub bump: LayerSpec,
    #[serde(default, skip_serializing_if = "LayerSpec::is_default")]
    pub bump2: LayerSpec,
    #[serde(default, skip_serializing_if = "LayerSpec::is_default")]
    pub detail: LayerSpec,
    #[serde(default, skip_serializing_if = "LayerSpec::is_default")]
    pub detail2: LayerSpec,
    #[serde(default, skip_serializing_if = "BentSpec::is_default")]
    pub bent: BentSpec,
}

impl Default for RnmSpec {
    fn default() -> Self {
        RnmSpec {
            enabled: true,
            order: None,
            bump: LayerSpec::default(),
            bump2: LayerSpec::default(),
            detail: LayerSpec::default(),
            detail2: LayerSpec::default(),
            bent: BentSpec::default(),
        }
    }
}

impl RnmSpec {
    pub fn is_default(&self) -> bool {
        *self == RnmSpec::default()
    }
    /// `order` without repeats (the first wins), then the missing ids in [`LayerId::ORDER`] (DECISIONS Note W6).
    pub fn effective_order(&self) -> Vec<LayerId> {
        let mut v: Vec<LayerId> = vec![];
        for id in self.order.iter().flatten().chain(LayerId::ORDER.iter()) {
            if !v.contains(id) {
                v.push(*id);
            }
        }
        v
    }
    pub fn layer(&self, id: LayerId) -> LayerView {
        let (strength, flip_green, mask) = match id {
            LayerId::Bump => (self.bump.strength, self.bump.flip_green, self.bump.mask),
            LayerId::Bump2 => (self.bump2.strength, self.bump2.flip_green, self.bump2.mask),
            LayerId::Detail => (
                self.detail.strength,
                self.detail.flip_green,
                self.detail.mask,
            ),
            LayerId::Detail2 => (
                self.detail2.strength,
                self.detail2.flip_green,
                self.detail2.mask,
            ),
            LayerId::Bent => (self.bent.strength, self.bent.flip_green, self.bent.mask),
        };
        LayerView {
            strength: strength.unwrap_or(id.default_strength()),
            flip_green,
            mask,
        }
    }
    pub fn ao_power(&self) -> f64 {
        self.bent.ao_power.unwrap_or(1.5)
    }
}
cfg_enum!(RawMap, "lowercase" { Roughness, Metallic, Ao, Specular, Glossiness, Height, Opacity, Fuzz, Orm, Curves, Cavity, Rmaos });

impl RawMap {
    /// The single-channel roles, each with a `packing.fill.<role>`. `Orm` is not one (DECISIONS Note AL), nor are
    /// `Curves` (a 0.5-centred curvature map), `Cavity` and the multi-channel `Rmaos`, read only by Also export
    /// (Note CN).
    pub const ALL: [RawMap; 8] = {
        use RawMap::*;
        [
            Roughness, Metallic, Ao, Specular, Glossiness, Height, Opacity, Fuzz,
        ]
    };

    /// Every raw-map role a file name can carry: [`RawMap::ALL`], the multi-channel `Orm` (Note AL), `Curves` and
    /// `Cavity` (Note CN). Not `Rmaos`: `"rmaos"` is also the §6.1 suffix of a pre-packed file, and callers match
    /// `set.suffixes` on `id()`.
    pub const INPUTS: [RawMap; 11] = {
        use RawMap::*;
        [
            Roughness, Metallic, Ao, Specular, Glossiness, Height, Opacity, Fuzz, Orm, Curves,
            Cavity,
        ]
    };

    /// The role id a raw map counts as after Discover (spec §4.4): `roughness`, `ao`, …
    pub fn id(self) -> String {
        serde_id(self)
    }

    /// Spec §4.3 item 1, §6.4: an input that starts the cs `_rmaos`/`_rfaos` recipe, so its set is PBR.
    pub fn starts_rmaos(self) -> bool {
        matches!(
            self,
            RawMap::Roughness | RawMap::Glossiness | RawMap::Metallic | RawMap::Ao | RawMap::Orm
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

/// The serde string a unit enum variant serialises as (its id).
fn serde_id<T: serde::Serialize>(v: T) -> String {
    serde_json::to_value(v)
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned()
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
            RawMap::Orm | RawMap::Curves | RawMap::Cavity | RawMap::Rmaos => None,
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
    /// Which base an RNM layer stack can blend into (F2 spec §2.3, DECISIONS Note W5); `None` for every other
    /// sub-type, `msn` included.
    pub fn rnm_base(self) -> Option<RnmTarget> {
        match self {
            Subtype::Normal | Subtype::PbrNormal | Subtype::LandscapeNormal => Some(RnmTarget::N),
            Subtype::CoatNormal => Some(RnmTarget::Cnr),
            _ => None,
        }
    }

    pub const ALL: [Subtype; 29] = {
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
            ComplexGrass,
        ]
    };

    pub fn category(self) -> Category {
        use Subtype::*;
        match self {
            Diffuse | LandscapeDiffuse | PbrAlbedo | PbrEmissive | Glow | PbrSubsurface
            | PbrFuzz | SubsurfaceTint | BacklightTint | UiSky | SkinAlbedo => Category::Color,
            Normal | LandscapeNormal | PbrNormal | Msn | CoatNormal | SkinWet => Category::Normal,
            Envmask | Height | MsnSpecular | SkinSk => Category::Grayscale,
            ComplexMaterial | Rmaos | SkinRfaos | Flow | GenericTwoChannel | GenericData
            | ComplexGrass => Category::MixedPacked,
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
            Subtype::Diffuse
                | Subtype::PbrAlbedo
                | Subtype::UiSky
                | Subtype::SkinAlbedo
                | Subtype::ComplexGrass
        )
    }

    pub fn is_roughness_member(self) -> bool {
        matches!(self, Subtype::Rmaos | Subtype::SkinRfaos)
    }

    pub fn id(self) -> String {
        serde_id(self)
    }
}

impl Format {
    pub fn id(self) -> String {
        serde_id(self)
    }
    /// The DDS pixel format this format and tag are written as (DECISIONS S4: `rgba8` is B8G8R8A8).
    pub fn pixel_format(self, srgb: bool) -> crate::dds::DdsPixelFormat {
        use crate::dds::DdsPixelFormat as P;
        match self {
            Format::Bc1 => P::Bc1 { srgb },
            Format::Bc3 => P::Bc3 { srgb },
            Format::Bc4 => P::Bc4,
            Format::Bc5 => P::Bc5,
            Format::Bc6hUf16 => P::Bc6hUf16,
            Format::Bc7 => P::Bc7 { srgb },
            Format::Rgba8 => P::Bgra8 { srgb },
            Format::R8 => P::R8,
            Format::Rg8 => P::Rg8,
            Format::Rgba16f => P::Rgba16f,
        }
    }
    /// Bytes per 4x4 block, or `None` for an uncompressed format.
    pub fn block_bytes(self) -> Option<usize> {
        self.pixel_format(false).block_bytes()
    }
    /// Bytes per texel of an uncompressed format (0 for a block format).
    pub fn bytes_per_pixel(self) -> usize {
        self.pixel_format(false).bytes_per_pixel()
    }
    pub fn is_block(self) -> bool {
        self.block_bytes().is_some()
    }
    /// Float formats: no `_SRGB`/UNORM tag (`n/a`), the `Hdr` domain, `cs` only (DECISIONS S4).
    pub fn is_hdr(self) -> bool {
        matches!(self, Format::Bc6hUf16 | Format::Rgba16f)
    }
    pub fn has_alpha(self) -> bool {
        matches!(
            self,
            Format::Bc1 | Format::Bc3 | Format::Bc7 | Format::Rgba8 | Format::Rgba16f
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ColorSpec {
    pub content: ColorContent,
    pub tag: ColorTag,
}

/// The allowed `alpha.threshold` values (owner O2): a NIF threshold is clamped into it, a preset value outside it is
/// CFG-INVALID.
pub const ALPHA_THRESHOLD: std::ops::RangeInclusive<u8> = 1..=254;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AlphaSpec {
    pub policy: AlphaPolicy,
    /// 8-bit alpha-test threshold, per texture; allowed 1..=254 (checked in policy).
    pub threshold: u8,
    pub compare: AlphaCompare,
    pub exact_opaque: bool,
    /// `complex_material` only (DECISIONS AH2): `false` writes alpha 0 at every mip (complex material on, parallax
    /// off); `true` keeps the source alpha as height. Resolve sets it from the source alpha; `None` elsewhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parallax: Option<bool>,
    /// An OpenEXR source with alpha is straight alpha, read as stored: no un-premultiply on load (trace-debug #30,
    /// Note BQ15). Off: RGB / A, as OpenEXR stores premultiplied colour. Read only for an `.exr` source with alpha.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub exr_straight: bool,
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
    /// Edge handling of the mip filter taps: read only by tent, Mitchell and Kaiser (spec §3.2).
    #[serde(default, skip_serializing_if = "MipEdge::is_wrap")]
    pub edge: MipEdge,
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
    /// M6 Track D (DECISIONS Notes T5, Y6): `smallest` runs the per-mip PSNR check on the smallest mip only and the
    /// coverage check on every level (`fast-preview`). Not serialised at its default, so existing profile hashes hold.
    #[serde(default, skip_serializing_if = "ValidationMips::is_all")]
    pub mips: ValidationMips,
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
    /// Preprocess step 6 (spec §5.4); `pot-down` = largest power of two ≤ each axis, `half` = both axes halved when the larger is above 512 (M6 Track D).
    #[serde(default, skip_serializing_if = "ResizeMode::is_none")]
    pub resize: ResizeMode,
    /// Write the texture at half size, from its mip 1; every lower level is unchanged.
    // Not serialised at its default, so existing profile hashes hold (v2 artist checks, DECISIONS Note CL21).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub drop_top_mip: bool,
    pub encoder: EncoderSpec,
    pub validation: ValidationSpec,
    pub packing: PackingSpec,
    /// RNM normal layers (M6 Track F2); read only by base normals (`Subtype::rnm_base`).
    #[serde(default, skip_serializing_if = "RnmSpec::is_default")]
    pub rnm: RnmSpec,
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
        assert_eq!(Subtype::ALL.len(), 29);
        assert_eq!(Subtype::ComplexGrass.id(), "complex_grass");
        assert_eq!(Subtype::ComplexGrass.category(), Category::MixedPacked);
        assert_eq!(Subtype::ALL.last(), Some(&Subtype::ComplexGrass));
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
            "rgba16f",
            "uncompressed",
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
        assert_eq!(ids.len(), 29);
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
        assert_eq!(Format::Bc7.block_bytes(), Some(16));
        assert_eq!(Format::Bc4.block_bytes(), Some(8));
        assert_eq!(Format::Rgba8.block_bytes(), None);
        assert!(!Format::Bc5.has_alpha());
    }

    #[test]
    fn uncompressed_formats_serialise_and_classify() {
        for (f, id) in [
            (Format::Rgba8, "rgba8"),
            (Format::R8, "r8"),
            (Format::Rg8, "rg8"),
            (Format::Rgba16f, "rgba16f"),
        ] {
            assert_eq!(f.id(), id);
            assert!(!f.is_block());
        }
        assert!(Format::Rgba16f.is_hdr() && Format::Bc6hUf16.is_hdr() && !Format::Rgba8.is_hdr());
        assert!(
            Format::Rgba8.has_alpha() && Format::Rgba16f.has_alpha() && !Format::R8.has_alpha()
        );
        assert_eq!(
            serde_json::to_string(&Backend::Uncompressed).unwrap(),
            "\"uncompressed\""
        );
        assert_eq!(
            (
                Format::Rgba8.bytes_per_pixel(),
                Format::Rgba16f.bytes_per_pixel()
            ),
            (4, 8)
        );
    }

    #[test]
    fn m6_mip_and_resize_ids_match_the_spec() {
        for (f, id) in [
            (MipFilter::Box, "box"),
            (MipFilter::Tent, "tent"),
            (MipFilter::Mitchell, "mitchell"),
            (MipFilter::Kaiser, "kaiser"),
        ] {
            assert_eq!(serde_json::to_value(f).unwrap(), id);
        }
        assert_eq!(serde_json::to_value(MipEdge::Clamp).unwrap(), "clamp");
        assert_eq!(
            serde_json::to_value(ResizeMode::PotDown).unwrap(),
            "pot-down"
        );
        assert_eq!(serde_json::to_value(ResizeMode::None).unwrap(), "none");
    }

    #[test]
    fn default_edge_and_resize_are_not_serialised() {
        // S-mips-9: profile_sha256 must not change for unchanged settings.
        let p = crate::presets::PresetRegistry::builtin()
            .unwrap()
            .profile("cs-quality", Target::Cs, Subtype::Diffuse)
            .unwrap();
        assert_eq!((p.mips.edge, p.resize), (MipEdge::Wrap, ResizeMode::None));
        let v = serde_json::to_value(&p).unwrap();
        assert!(
            v.get("resize").is_none() && v["mips"].get("edge").is_none(),
            "{v}"
        );
        let mut q = p.clone();
        q.resize = ResizeMode::PotDown;
        q.mips.edge = MipEdge::Clamp;
        let v = serde_json::to_value(&q).unwrap();
        assert_eq!(
            (v["resize"].as_str(), v["mips"]["edge"].as_str()),
            (Some("pot-down"), Some("clamp"))
        );
        assert_eq!(serde_json::from_value::<Profile>(v).unwrap(), q);
    }
}
