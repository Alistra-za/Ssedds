//! Warning and error codes (spec §10). Codes added by this plan beyond the spec's original catalogue: VAL-MIP-ERROR
//! (§5.8 per-mip error warning), CFG-INVALID (a field value outside its allowed range or an encoder/format
//! mismatch), IN-NONFINITE (NaN or infinite source samples replaced in Preprocess) and IN-FLOAT-LDR (retired in M6:
//! never emitted) and the four `PACK-*` codes of channel packing (spec §6.4, plan M2.5), and CFG-FILE-UNMATCHED (a
//! `[files]` key of `sdc.toml` that matches no file, M3a) and SET-EDIT-STALE (a GUI set edit that names a file or
//! set that no longer exists, M3b), PACK-RECIPE-CONFLICT and PACK-RECIPE-UNBUILDABLE (a `packing.recipe` the set
//! could not follow, packing-fills Task 1) and SKIN-WET-FORM (which form `skin_wet` was read as, Task 1b) and
//! IN-ICC-IGNORED (an embedded ICC profile, M6) and FMT-HDR-SOURCE (why `pbr_emissive` is or is not BC6H, O2) and
//! DDS-HEADER-PATCHED / DDS-HEADER-UNPATCHABLE (a header-only DDS fix applied or refused, M6 DECISIONS S7) and
//! DIM-RESIZED (`resize = pot-down` changed the base size, S-mips-7) and CFG-FILTER-IGNORED (an unscoped non-box
//! `mips.filter` a texture cannot use, which then uses box, S-mips-13).
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Blocking,
    Error,
}

macro_rules! codes {
    ($($v:ident = $s:literal => $sev:ident),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
        pub enum Code { $(#[serde(rename = $s)] $v),+ }
        impl Code {
            pub const ALL: &'static [Code] = &[$(Code::$v),+];
            pub fn as_str(self) -> &'static str { match self { $(Code::$v => $s),+ } }
            pub fn severity(self) -> Severity { match self { $(Code::$v => Severity::$sev),+ } }
            pub fn parse(s: &str) -> Option<Code> { match s { $($s => Some(Code::$v),)+ _ => None } }
        }
    };
}

codes! {
    LossAlphaData = "LOSS-ALPHA-DATA" => Blocking,
    LossClampHdr = "LOSS-CLAMP-HDR" => Warning,
    LossBitdepth = "LOSS-BITDEPTH" => Info,
    LossReencode = "LOSS-REENCODE" => Warning,
    ChanNotGray = "CHAN-NOT-GRAY" => Warning,
    ChanConstantAlphaData = "CHAN-CONSTANT-ALPHA-DATA" => Info,
    ChanNormalLength = "CHAN-NORMAL-LENGTH" => Warning,
    ChanNormalShort = "CHAN-NORMAL-SHORT" => Info,
    ChanAoInverted = "CHAN-AO-INVERTED" => Warning,
    ChanGlossAsRough = "CHAN-GLOSS-AS-ROUGH" => Warning,
    FmtBc5Normal = "FMT-BC5-NORMAL" => Blocking,
    FmtLandscapeBc1 = "FMT-LANDSCAPE-BC1" => Blocking,
    FmtBc1Alpha = "FMT-BC1-ALPHA" => Blocking,
    FmtPerceptualData = "FMT-PERCEPTUAL-DATA" => Error,
    FmtHdrSource = "FMT-HDR-SOURCE" => Info,
    AlphaExactOffM = "ALPHA-EXACT-OFF-M" => Blocking,
    Alpha254 = "ALPHA-254" => Info,
    AlphaNearOpaque = "ALPHA-NEAR-OPAQUE" => Info,
    AlphaPbrNormalDropped = "ALPHA-PBR-NORMAL-DROPPED" => Info,
    TagSrgbVanilla = "TAG-SRGB-VANILLA" => Error,
    TagSrgbNonpbr = "TAG-SRGB-NONPBR" => Blocking,
    TagUnormPbr = "TAG-UNORM-PBR" => Warning,
    TagSrgbLod = "TAG-SRGB-LOD" => Blocking,
    TagContentMismatch = "TAG-CONTENT-MISMATCH" => Warning,
    CfgUnavailable = "CFG-UNAVAILABLE" => Blocking,
    CfgInvalid = "CFG-INVALID" => Error,
    CfgFileUnmatched = "CFG-FILE-UNMATCHED" => Info,
    CfgFilterIgnored = "CFG-FILTER-IGNORED" => Info,
    DimNpot = "DIM-NPOT" => Blocking,
    DimSmall = "DIM-SMALL" => Blocking,
    DimFlowSmall = "DIM-FLOW-SMALL" => Warning,
    DimResized = "DIM-RESIZED" => Info,
    DimTooLarge = "DIM-TOO-LARGE" => Error,
    SetMissingCompanion = "SET-MISSING-COMPANION" => Info,
    SetCompanionOutside = "SET-COMPANION-OUTSIDE" => Info,
    SetCompanionDds = "SET-COMPANION-DDS" => Warning,
    SetBakeRatio = "SET-BAKE-RATIO" => Warning,
    SetEditStale = "SET-EDIT-STALE" => Info,
    SugLowConfidence = "SUG-LOW-CONFIDENCE" => Blocking,
    SugContradiction = "SUG-CONTRADICTION" => Warning,
    ValAlphaGuarantee = "VAL-ALPHA-GUARANTEE" => Error,
    ValCoverage = "VAL-COVERAGE" => Warning,
    ValMComplexMargin = "VAL-M-COMPLEX-MARGIN" => Warning,
    ValMGrayscale = "VAL-M-GRAYSCALE" => Warning,
    ValMHeightClip = "VAL-M-HEIGHT-CLIP" => Warning,
    ValPgpatcher50 = "VAL-PGPATCHER-50" => Warning,
    ValBc6hPeak = "VAL-BC6H-PEAK" => Warning,
    ValHeader = "VAL-HEADER" => Error,
    ValMipError = "VAL-MIP-ERROR" => Warning,
    EncNvttMissing = "ENC-NVTT-MISSING" => Blocking,
    EncNvttVersion = "ENC-NVTT-VERSION" => Blocking,
    InDuplicate = "IN-DUPLICATE" => Blocking,
    InDdsSkipped = "IN-DDS-SKIPPED" => Info,
    InPackingInput = "IN-PACKING-INPUT" => Warning,
    InGreenUnstated = "IN-GREEN-UNSTATED" => Blocking,
    InDecode = "IN-DECODE" => Error,
    InNonfinite = "IN-NONFINITE" => Warning,
    // Retired in M6 (DECISIONS S2): never emitted; kept so --allow and [files] allow lists still load.
    InFloatLdr = "IN-FLOAT-LDR" => Info,
    InIccIgnored = "IN-ICC-IGNORED" => Info,
    OutUnknownExisting = "OUT-UNKNOWN-EXISTING" => Warning,
    PackDefaulted = "PACK-DEFAULTED" => Warning,
    PackSizeMismatch = "PACK-SIZE-MISMATCH" => Error,
    PackConflict = "PACK-CONFLICT" => Warning,
    PackUnusedInput = "PACK-UNUSED-INPUT" => Warning,
    PackRecipeConflict = "PACK-RECIPE-CONFLICT" => Warning,
    PackRecipeUnbuildable = "PACK-RECIPE-UNBUILDABLE" => Warning,
    SkinWetForm = "SKIN-WET-FORM" => Info,
    DdsHeaderPatched = "DDS-HEADER-PATCHED" => Info,
    DdsHeaderUnpatchable = "DDS-HEADER-UNPATCHABLE" => Warning,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Warning {
    pub code: Code,
    pub severity: Severity,
    pub message: String,
    #[serde(default)]
    pub data: serde_json::Value,
}

impl Warning {
    pub fn new(code: Code, message: impl Into<String>) -> Self {
        Warning {
            code,
            severity: code.severity(),
            message: message.into(),
            data: serde_json::Value::Null,
        }
    }
    pub fn with_data(mut self, data: serde_json::Value) -> Self {
        self.data = data;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn severities_follow_spec_section_10() {
        assert_eq!(Code::InFloatLdr.severity(), Severity::Info);
        assert_eq!(Code::FmtBc5Normal.severity(), Severity::Blocking);
        assert_eq!(Code::ChanNormalShort.severity(), Severity::Info);
        assert_eq!(Code::TagSrgbVanilla.severity(), Severity::Error);
        assert_eq!(Code::Alpha254.severity(), Severity::Info);
        assert_eq!(Code::AlphaNearOpaque.severity(), Severity::Info);
        assert_eq!(
            Code::parse("ALPHA-NEAR-OPAQUE"),
            Some(Code::AlphaNearOpaque)
        );
        assert_eq!(Code::ValCoverage.severity(), Severity::Warning);
        assert_eq!(Code::ValMipError.severity(), Severity::Warning);
        assert_eq!(Code::CfgInvalid.severity(), Severity::Error);
        assert_eq!(Code::DimResized.severity(), Severity::Info);
        assert_eq!(Code::CfgFilterIgnored.severity(), Severity::Info);
        assert_eq!(Code::InPackingInput.severity(), Severity::Warning);
        assert_eq!(Code::InGreenUnstated.severity(), Severity::Blocking);
        assert_eq!(
            Code::parse("IN-GREEN-UNSTATED"),
            Some(Code::InGreenUnstated)
        );
        assert_eq!(Code::parse("IN-PACKING-INPUT"), Some(Code::InPackingInput));
    }

    #[test]
    fn codes_serialise_as_their_strings_and_are_unique() {
        assert_eq!(
            serde_json::to_string(&Code::LossAlphaData).unwrap(),
            "\"LOSS-ALPHA-DATA\""
        );
        assert_eq!(Code::parse("VAL-PGPATCHER-50"), Some(Code::ValPgpatcher50));
        let names: BTreeSet<_> = Code::ALL.iter().map(|c| c.as_str()).collect();
        assert_eq!(names.len(), Code::ALL.len());
        assert_eq!(Code::ALL.len(), 69);
        assert_eq!(Code::InIccIgnored.severity(), Severity::Info);
    }
}
