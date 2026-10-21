//! Track A: turn PBR inputs into vanilla and complex-material maps (DECISIONS Note CN).
pub mod assemble;
pub mod plan;
pub mod recipe;

use crate::config::{GraySource, Subtype};
use crate::pack::ChannelSource;
use crate::policy::SourceStamp;
use crate::shader::Shader;
use recipe::LegacyParams;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

/// Which folder a derive plan writes (Note CN).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeriveMode {
    Vanilla,
    Cm,
}

/// One PBR set to convert, read from the PBR phase's plan (TA11).
#[derive(Debug, Clone, PartialEq)]
pub struct DeriveSet {
    /// The set id relative to the output root (`--set set:<id>` form).
    pub id: String,
    pub shader: Shader,
    /// Every input of the set, so the vanilla plan can drop them.
    pub members: Vec<PathBuf>,
    pub albedo: Option<PathBuf>,
    pub normal: Option<PathBuf>,
    pub rough: ChannelSource,
    pub metal: ChannelSource,
    pub ao: ChannelSource,
    pub height: Option<ChannelSource>,
    pub curves: Option<ChannelSource>,
    pub cavity: Option<ChannelSource>,
    pub gray: GraySource,
    /// Members the recipe needs that are DDS (never re-encoded, M9): the set is skipped.
    pub dds: Vec<PathBuf>,
}

/// `BatchRequest::derive`: replace each PBR set's jobs with derived jobs (Note CN).
#[derive(Debug, Clone, PartialEq)]
pub struct DeriveRequest {
    pub mode: DeriveMode,
    pub params: LegacyParams,
    pub sets: Vec<DeriveSet>,
    /// Members of PBR sets left out (C2 opt-out): their jobs are removed as a derived set's are.
    pub drop: Vec<PathBuf>,
    /// The mesh votes were read (TA13): tells "no scan" from "no env-map mesh".
    pub scanned: bool,
}

/// How many trailing components of a path mirror `game_path` and sit below a `textures/pbr/` fold (TA12), or
/// `None` when the game path has no `pbr` folder directly below `textures/`.
fn pbr_depth(game_path: &str) -> Option<usize> {
    let tail: Vec<&str> = game_path.split('/').collect();
    (tail.len() > 2
        && tail[0].eq_ignore_ascii_case("textures")
        && tail[1].eq_ignore_ascii_case("pbr"))
    .then(|| tail.len() - 1)
}

/// TA12: `rel` (or any path that ends with the game path's tail, such as an output) without the `pbr` component
/// that sits directly below the game path's `textures/`. Unchanged when the game path has no such folder, or the
/// component at that position is not `pbr` (ASCII case-insensitive).
pub fn vanilla_rel(rel: &Path, game_path: &str) -> PathBuf {
    let comps: Vec<Component> = rel.components().collect();
    match pbr_depth(game_path) {
        Some(n)
            if comps.len() >= n
                && comps[comps.len() - n]
                    .as_os_str()
                    .to_string_lossy()
                    .eq_ignore_ascii_case("pbr") =>
        {
            let cut = comps.len() - n;
            comps
                .iter()
                .enumerate()
                .filter(|&(i, _)| i != cut)
                .map(|(_, c)| c.as_os_str())
                .collect()
        }
        _ => rel.to_path_buf(),
    }
}

/// TA12 on a `/` game path: `textures/pbr/arm/steel` → `textures/arm/steel`.
pub fn vanilla_game_path(game_path: &str) -> String {
    match pbr_depth(game_path) {
        Some(_) => {
            let (head, rest) = game_path.split_at("textures/".len());
            format!("{head}{}", &rest["pbr/".len()..])
        }
        None => game_path.to_string(),
    }
}

/// What a derived job writes (Note CN11).
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

/// How a derived output is built (`ResolvedJob::derive`, Note CN11). `sources[0].path` is the job's `input`.
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
    /// SHA-256 of the job without its sources' stamps: every setting, slot and path (Note CN12). Recorded on each
    /// source (`SourceRef::derive`), so `if-changed` re-encodes when any changes.
    // shortcut: absolute paths are hashed, so a moved project re-encodes once; clear them before release (CN12).
    pub fn identity_sha256(&self) -> String {
        let mut j = self.clone();
        for s in &mut j.sources {
            s.stamp = None;
        }
        crate::manifest::sha256_json(&j).expect("a DeriveJob serialises")
    }
}
