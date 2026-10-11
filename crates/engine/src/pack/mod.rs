//! Channel packing (spec §6.4, DECISIONS Notes H and M, plan M2.5): the raw single-channel maps of one texture
//! set become packed outputs per target. This file is pure (which recipes run, what each channel holds);
//! `assemble` builds the image and `plan` wires both into `Engine::plan`.
pub mod assemble;
pub mod plan;

use crate::config::{GraySource, PackingSpec, RawMap, Subtype, Target};
use crate::policy::SourceStamp;
use crate::presets::Provenance;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum RecipeKind {
    /// `cs` `<base>_rmaos.dds`: roughness, metallic, AO, specular.
    Rmaos,
    /// `cs` `<base>_rfaos.dds`: roughness, fuzz, AO, specular.
    Rfaos,
    /// `cs` `<base>_p.dds`: height (BC4).
    Parallax,
    /// `cpm` complex `<base>_m.dds`: env mask, glossiness, metallic, height.
    CpmM,
    /// `vanilla` env-mask `<base>_m.dds` (BC4).
    VanillaM,
}

impl RecipeKind {
    pub fn subtype(self) -> Subtype {
        match self {
            RecipeKind::Rmaos => Subtype::Rmaos,
            RecipeKind::Rfaos => Subtype::Rfaos,
            RecipeKind::Parallax => Subtype::Height,
            RecipeKind::CpmM => Subtype::ComplexMaterial,
            RecipeKind::VanillaM => Subtype::Envmask,
        }
    }

    /// The §6.1 suffix of the output, `<base>_<suffix>.dds`.
    pub fn suffix(self) -> &'static str {
        match self {
            RecipeKind::Rmaos => "rmaos",
            RecipeKind::Rfaos => "rfaos",
            RecipeKind::Parallax => "p",
            RecipeKind::CpmM | RecipeKind::VanillaM => "m",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            RecipeKind::Rmaos => "rmaos",
            RecipeKind::Rfaos => "rfaos",
            RecipeKind::Parallax => "parallax",
            RecipeKind::CpmM => "cpm_m",
            RecipeKind::VanillaM => "vanilla_m",
        }
    }
}

/// The quantity a channel holds when no source map provides it (spec §6.4 defaults table).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Slot {
    Roughness,
    Glossiness,
    Metallic,
    Ao,
    Specular,
    EnvMask,
    Height,
}

impl Slot {
    pub fn id(self) -> &'static str {
        match self {
            Slot::Roughness => "roughness",
            Slot::Glossiness => "glossiness",
            Slot::Metallic => "metallic",
            Slot::Ao => "ao",
            Slot::Specular => "specular",
            Slot::EnvMask => "env_mask",
            Slot::Height => "height",
        }
    }
}

/// A raw map of one set waiting for a recipe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawInput {
    pub path: PathBuf,
    pub role: RawMap,
}

/// What a channel needs, before fills and defaults are known.
#[derive(Debug, Clone, PartialEq)]
pub enum ChannelNeed {
    /// R (or luma) of a source; `invert` stores 1 − x (glossiness ↔ roughness).
    Source {
        path: PathBuf,
        role: RawMap,
        invert: bool,
    },
    /// No source: a fill value or the neutral default of the slot.
    Missing(Slot),
    /// A copy of R: the unused G and B of a single-channel output, so the image stays grey.
    MirrorR,
    /// Opaque alpha of an output whose alpha carries nothing.
    Opaque,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Recipe {
    pub kind: RecipeKind,
    /// R, G, B, A.
    pub channels: [ChannelNeed; 4],
}

impl Recipe {
    /// Every source, in channel order, each path once. A recipe always has at least one (its trigger).
    pub fn sources(&self) -> Vec<(PathBuf, RawMap)> {
        let mut v: Vec<(PathBuf, RawMap)> = vec![];
        for c in &self.channels {
            if let ChannelNeed::Source { path, role, .. } = c {
                if !v.iter().any(|(p, _)| p == path) {
                    v.push((path.clone(), *role));
                }
            }
        }
        v
    }
}

/// A raw map that no recipe of the target uses (`PACK-UNUSED-INPUT`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unused {
    pub path: PathBuf,
    pub role: RawMap,
    pub reason: String,
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// Why a raw map of `role` takes part in no recipe of `target` (DECISIONS Note M2).
fn unused_reason(target: Target, role: RawMap, have: &BTreeMap<RawMap, &RawInput>) -> String {
    let has = |r: RawMap| have.contains_key(&r);
    match (target, role) {
        (_, RawMap::Opacity) => "no recipe packs opacity yet (spec §14 #19)".into(),
        (Target::Cs, RawMap::Glossiness) if has(RawMap::Roughness) => {
            "roughness is used: the _rmaos/_rfaos R channel is roughness".into()
        }
        (Target::Cs, RawMap::Metallic) if has(RawMap::Fuzz) => {
            "_rfaos has no metallic channel".into()
        }
        (Target::Cs, _) => {
            "nothing to pack: _rmaos/_rfaos need a roughness, glossiness, AO or metallic source"
                .into()
        }
        (Target::Cpm, RawMap::Roughness) if has(RawMap::Glossiness) => {
            "glossiness is used: the cpm _m G channel is glossiness".into()
        }
        (Target::Cpm, RawMap::Specular) if has(RawMap::Metallic) => {
            "the env mask comes from the metallic source".into()
        }
        (Target::Cpm, RawMap::Ao | RawMap::Fuzz) => {
            format!("the cpm _m has no {} channel", role.id())
        }
        (Target::Cpm, _) => {
            "nothing to pack: the cpm _m needs a roughness, glossiness, metallic or height source"
                .into()
        }
        (Target::Vanilla, RawMap::Specular) if has(RawMap::Metallic) => {
            "the env mask comes from the metallic source".into()
        }
        (Target::Vanilla, _) => {
            "vanilla packs only an env-mask _m, from a metallic or specular source".into()
        }
    }
}

/// The recipes of one set under `target` (spec §6.4 recipe table) and the raw maps none of them uses.
pub fn recipes_for(target: Target, raw: &[RawInput]) -> (Vec<Recipe>, Vec<Unused>) {
    use ChannelNeed::{MirrorR, Missing, Opaque};
    use RawMap::*;
    let mut sorted: Vec<&RawInput> = raw.iter().collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    // DECISIONS Note M13: one source per role, the first by path.
    let mut by_role: BTreeMap<RawMap, &RawInput> = BTreeMap::new();
    let mut unused = vec![];
    for r in sorted {
        match by_role.get(&r.role) {
            Some(first) => unused.push(Unused {
                path: r.path.clone(),
                role: r.role,
                reason: format!(
                    "another {} source is used: {}",
                    r.role.id(),
                    file_name(&first.path)
                ),
            }),
            None => {
                by_role.insert(r.role, r);
            }
        }
    }
    let src = |role: RawMap, invert: bool| {
        by_role.get(&role).map(|r| ChannelNeed::Source {
            path: r.path.clone(),
            role,
            invert,
        })
    };
    let has = |role: RawMap| by_role.contains_key(&role);
    let mut recipes = vec![];
    match target {
        Target::Cs => {
            if [Roughness, Glossiness, Ao, Metallic].into_iter().any(has) {
                let rough = src(Roughness, false)
                    .or_else(|| src(Glossiness, true))
                    .unwrap_or(Missing(Slot::Roughness));
                let ao = src(Ao, false).unwrap_or(Missing(Slot::Ao));
                let spec = src(Specular, false).unwrap_or(Missing(Slot::Specular));
                match src(Fuzz, false) {
                    Some(fuzz) => recipes.push(Recipe {
                        kind: RecipeKind::Rfaos,
                        channels: [rough, fuzz, ao, spec],
                    }),
                    None => recipes.push(Recipe {
                        kind: RecipeKind::Rmaos,
                        channels: [
                            rough,
                            src(Metallic, false).unwrap_or(Missing(Slot::Metallic)),
                            ao,
                            spec,
                        ],
                    }),
                }
            }
            if let Some(h) = src(Height, false) {
                recipes.push(Recipe {
                    kind: RecipeKind::Parallax,
                    channels: [h, MirrorR, MirrorR, Opaque],
                });
            }
        }
        Target::Cpm => {
            if [Roughness, Glossiness, Metallic, Height]
                .into_iter()
                .any(has)
            {
                recipes.push(Recipe {
                    kind: RecipeKind::CpmM,
                    channels: [
                        src(Metallic, false)
                            .or_else(|| src(Specular, false))
                            .unwrap_or(Missing(Slot::EnvMask)),
                        src(Glossiness, false)
                            .or_else(|| src(Roughness, true))
                            .unwrap_or(Missing(Slot::Glossiness)),
                        src(Metallic, false).unwrap_or(Missing(Slot::Metallic)),
                        src(Height, false).unwrap_or(Missing(Slot::Height)),
                    ],
                });
            }
        }
        Target::Vanilla => {
            if let Some(env) = src(Metallic, false).or_else(|| src(Specular, false)) {
                recipes.push(Recipe {
                    kind: RecipeKind::VanillaM,
                    channels: [env, MirrorR, MirrorR, Opaque],
                });
            }
        }
    }
    let used: BTreeSet<PathBuf> = recipes
        .iter()
        .flat_map(|r| r.sources())
        .map(|(p, _)| p)
        .collect();
    for r in by_role.values() {
        if !used.contains(&r.path) {
            unused.push(Unused {
                path: r.path.clone(),
                role: r.role,
                reason: unused_reason(target, r.role, &by_role),
            });
        }
    }
    unused.sort_by(|a, b| a.path.cmp(&b.path));
    (recipes, unused)
}

/// The suffixes a set's recipes would add (spec §6.4 "Set membership", DECISIONS Note M7), from the raw roles
/// present, before anything is resolved.
pub fn predicted_suffixes(target: Target, roles: &BTreeSet<RawMap>) -> Vec<&'static str> {
    let raw: Vec<RawInput> = roles
        .iter()
        .map(|r| RawInput {
            path: PathBuf::from(r.id()),
            role: *r,
        })
        .collect();
    recipes_for(target, &raw)
        .0
        .iter()
        .map(|r| r.kind.suffix())
        .collect()
}

/// What a packed channel holds; serialised in `file_planned` (`job.pack.channels`) and the manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "from", rename_all = "snake_case")]
pub enum ChannelSource {
    /// R (or luma) of a raw source; `invert` stores 1 − x.
    Source {
        path: PathBuf,
        role: RawMap,
        invert: bool,
    },
    /// A user fill (`packing.fill.<role>`); `code` is the 8-bit value written (255 − fill when inverted).
    Fill { role: RawMap, code: u8 },
    /// The neutral default of spec §6.4, 0..=1.
    Default { value: f32 },
    /// A copy of R.
    MirrorR,
    /// 1.0.
    Opaque,
}

impl ChannelSource {
    /// The channel's provenance (DECISIONS Note M12); `None` for a mirrored or opaque filler channel.
    pub fn provenance(&self, gray: GraySource) -> Option<Provenance> {
        match self {
            ChannelSource::Source { path, invert, .. } => Some(Provenance::Source(format!(
                "{}{}:{}",
                if *invert { "1-" } else { "" },
                file_name(path),
                match gray {
                    GraySource::R => "R",
                    GraySource::Luma => "luma",
                }
            ))),
            ChannelSource::Fill { code, .. } => Some(Provenance::Fill(code.to_string())),
            ChannelSource::Default { value } => Some(Provenance::Default(value.to_string())),
            ChannelSource::MirrorR | ChannelSource::Opaque => None,
        }
    }
}

/// A user fill for a missing slot (DECISIONS Note M5): the slot's own role, else the inverse quantity's role
/// inverted (roughness ↔ glossiness); the env mask takes the metallic, else the specular fill (Note M1).
fn fill_for(slot: Slot, p: &PackingSpec) -> Option<ChannelSource> {
    let direct = |role: RawMap| {
        p.fill
            .get(role)
            .map(|code| ChannelSource::Fill { role, code })
    };
    let inverted = |role: RawMap| {
        p.fill.get(role).map(|code| ChannelSource::Fill {
            role,
            code: 255 - code,
        })
    };
    match slot {
        Slot::Roughness => direct(RawMap::Roughness).or_else(|| inverted(RawMap::Glossiness)),
        Slot::Glossiness => direct(RawMap::Glossiness).or_else(|| inverted(RawMap::Roughness)),
        Slot::Metallic => direct(RawMap::Metallic),
        Slot::Ao => direct(RawMap::Ao),
        Slot::Specular => direct(RawMap::Specular),
        Slot::EnvMask => direct(RawMap::Metallic).or_else(|| direct(RawMap::Specular)),
        Slot::Height => direct(RawMap::Height),
    }
}

/// The neutral default of a slot (spec §6.4 defaults table, DECISIONS Note M1).
fn default_for(slot: Slot, p: &PackingSpec) -> f32 {
    match slot {
        Slot::Roughness => p.roughness_default as f32,
        Slot::Glossiness => (1.0 - p.roughness_default) as f32,
        Slot::Metallic | Slot::EnvMask => 0.0,
        Slot::Ao | Slot::Specular => 1.0,
        // 0: complex material on, parallax off (CS runs parallax only for 4/255 < alpha < 251/255, Spike S8; owner
        // 2026-10-10, amending Note M1's 0.5).
        Slot::Height => 0.0,
    }
}

/// The channels of a recipe with fills and defaults applied, and the channels that took a neutral default
/// (index, slot): each raises `PACK-DEFAULTED`; a filled channel does not (DECISIONS Note M5).
pub fn resolve_channels(r: &Recipe, p: &PackingSpec) -> ([ChannelSource; 4], Vec<(usize, Slot)>) {
    let mut defaulted = vec![];
    let channels = std::array::from_fn(|i| match &r.channels[i] {
        ChannelNeed::Source { path, role, invert } => ChannelSource::Source {
            path: path.clone(),
            role: *role,
            invert: *invert,
        },
        ChannelNeed::MirrorR => ChannelSource::MirrorR,
        ChannelNeed::Opaque => ChannelSource::Opaque,
        ChannelNeed::Missing(slot) => fill_for(*slot, p).unwrap_or_else(|| {
            defaulted.push((i, *slot));
            ChannelSource::Default {
                value: default_for(*slot, p),
            }
        }),
    });
    (channels, defaulted)
}

/// One source of a packed job and its size and modification time when planning read it (final review I7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PackSource {
    pub path: PathBuf,
    pub role: RawMap,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stamp: Option<SourceStamp>,
}

/// How a packed output is built (`ResolvedJob::pack`). `sources[0].path` is the job's `input` (Note M8).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PackJob {
    pub kind: RecipeKind,
    /// R, G, B, A.
    pub channels: [ChannelSource; 4],
    /// The `channels.grayscale_source` this recipe was planned with (provenance, analysis and report all use it).
    /// The pipeline assembles with this value, not with the final profile's, so the written image is the one that
    /// was analysed (DECISIONS Note M6). Serialised, so it shows in `file_planned` and the manifest.
    pub gray: GraySource,
    pub sources: Vec<PackSource>,
}

/// Whether the channel `path` feeds is stored inverted (DECISIONS N7).
pub fn invert_of(pk: &PackJob, path: &Path) -> bool {
    pk.channels
        .iter()
        .any(|c| matches!(c, ChannelSource::Source { path: p, invert: true, .. } if p == path))
}
