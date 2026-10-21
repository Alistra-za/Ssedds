//! Track A: turn PBR inputs into vanilla and complex-material maps (DECISIONS Note CM).
pub mod assemble;
pub mod recipe;

use crate::config::{GraySource, Subtype};
use crate::pack::ChannelSource;
use crate::policy::SourceStamp;
use recipe::LegacyParams;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// What a derived job writes (Note CM11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeriveKind {
    // The vanilla diffuse, `<base>.dds`.
    VanillaDiffuse,
    // The complex-material diffuse (keep metal brightness), `<base>.dds` in the CM folder.
    CmDiffuse,
    // The vanilla `_n`: the normal's RGB, the specular mask in alpha.
    LegacyN,
    // The complex-material `_m`.
    CmM,
    // The vanilla env mask `_m`.
    EnvM,
    // Complex grass: the diffuse on top, the `_n` below.
    GrassStack,
}

impl DeriveKind {
    pub fn id(self) -> &'static str {
        match self {
            DeriveKind::VanillaDiffuse => "vanilla_diffuse",
            DeriveKind::CmDiffuse => "cm_diffuse",
            DeriveKind::LegacyN => "legacy_n",
            DeriveKind::CmM => "cm_m",
            DeriveKind::EnvM => "env_m",
            DeriveKind::GrassStack => "grass_stack",
        }
    }

    /// The output's suffix; `None` writes `<base>.dds`.
    pub fn suffix(self) -> Option<&'static str> {
        match self {
            DeriveKind::LegacyN => Some("n"),
            DeriveKind::CmM | DeriveKind::EnvM => Some("m"),
            DeriveKind::VanillaDiffuse | DeriveKind::CmDiffuse | DeriveKind::GrassStack => None,
        }
    }

    /// The sub-type the output is encoded as.
    pub fn subtype(self, landscape: bool) -> Subtype {
        match (self, landscape) {
            (DeriveKind::VanillaDiffuse | DeriveKind::CmDiffuse, false) => Subtype::Diffuse,
            (DeriveKind::VanillaDiffuse | DeriveKind::CmDiffuse, true) => Subtype::LandscapeDiffuse,
            (DeriveKind::LegacyN, false) => Subtype::Normal,
            (DeriveKind::LegacyN, true) => Subtype::LandscapeNormal,
            (DeriveKind::CmM, _) => Subtype::ComplexMaterial,
            (DeriveKind::EnvM, _) => Subtype::Envmask,
            (DeriveKind::GrassStack, _) => Subtype::ComplexGrass,
        }
    }
}

/// One file a derived job reads, with the slots it fills (`rough+metal+ao` for an `_rmaos` feeding all three) and
/// its stamp when planning read it. `PackSource` can't hold it: its role is a `RawMap`, which has no albedo or normal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DeriveSource {
    pub path: PathBuf,
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stamp: Option<SourceStamp>,
}

/// How a derived output is built (`ResolvedJob::derive`, Note CM11). `sources[0].path` is the job's `input`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DeriveJob {
    pub kind: DeriveKind,
    pub landscape: bool,
    pub albedo: Option<PathBuf>,
    pub normal: Option<PathBuf>,
    pub rough: ChannelSource,
    pub metal: ChannelSource,
    pub ao: ChannelSource,
    /// The `_p` R.
    pub height: Option<ChannelSource>,
    pub curves: Option<ChannelSource>,
    pub cavity: Option<ChannelSource>,
    pub gray: GraySource,
    pub params: LegacyParams,
    /// Every file read, each once, in slot order.
    pub sources: Vec<DeriveSource>,
}

impl DeriveJob {
    /// SHA-256 of the job without its sources' stamps: every setting, slot and path (Note CM12). Recorded on each
    /// source (`SourceRef::derive`), so `if-changed` re-encodes when any changes.
    pub fn identity_sha256(&self) -> String {
        let mut j = self.clone();
        for s in &mut j.sources {
            s.stamp = None;
        }
        crate::manifest::sha256_json(&j).expect("a DeriveJob serialises")
    }
}
