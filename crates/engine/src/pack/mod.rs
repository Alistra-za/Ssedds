//! Channel packing (spec §6.4, DECISIONS Notes H and M, plan M2.5): the raw single-channel maps of one texture
//! set become packed outputs per target. This file is pure (which recipes run, what each channel holds);
//! `assemble` builds the image and `plan` wires both into `Engine::plan`.
pub mod assemble;
pub mod plan;

use crate::config::{GraySource, PackRecipe, PackingSpec, RawMap, Subtype, Target};
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
            RecipeKind::Rfaos => Subtype::SkinRfaos,
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
    /// The G channel of `_rfaos` (DECISIONS Note R-fills-1).
    Fuzz,
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
            Slot::Fuzz => "fuzz",
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
    /// R (or luma) of a source; `invert` stores 1 − x (glossiness ↔ roughness). `channel` reads that channel
    /// (0..=2) instead, for a multi-channel `_orm` (DECISIONS Note AL).
    Source {
        path: PathBuf,
        role: RawMap,
        invert: bool,
        channel: Option<u8>,
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
    /// A loose map that won a slot over an `_orm` channel, one line each (`PACK-ORM-OVERRIDDEN`, Note AL2).
    pub notes: Vec<String>,
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
fn unused_reason(
    target: Target,
    recipe: PackRecipe,
    role: RawMap,
    have: &BTreeMap<RawMap, &RawInput>,
) -> String {
    // An `_orm` counts as a roughness, metallic and AO source (Note AL1).
    let has = |r: RawMap| {
        have.contains_key(&r)
            || (matches!(r, RawMap::Roughness | RawMap::Metallic | RawMap::Ao)
                && have.contains_key(&RawMap::Orm))
    };
    match (target, role) {
        (_, RawMap::Orm) => "a loose map is used for each channel it could supply".into(),
        (_, RawMap::Opacity) => "no recipe packs opacity yet (spec §14 #19)".into(),
        (Target::Cs, RawMap::Glossiness) if has(RawMap::Roughness) => {
            "roughness is used: the _rmaos/_rfaos R channel is roughness".into()
        }
        (Target::Cs, RawMap::Fuzz) if recipe == PackRecipe::Rmaos => {
            "the set is set to _rmaos (packing.recipe)".into()
        }
        (Target::Cs, RawMap::Metallic) if recipe == PackRecipe::Rfaos || has(RawMap::Fuzz) => {
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

/// `loose` wins over the `_orm` channel `orm` for `slot` (DECISIONS Note AL2); a note says so when both exist.
fn prefer(
    loose: Option<ChannelNeed>,
    orm: Option<ChannelNeed>,
    slot: &str,
    notes: &mut Vec<String>,
) -> Option<ChannelNeed> {
    if let (Some(ChannelNeed::Source { path: l, .. }), Some(ChannelNeed::Source { path: o, .. })) =
        (&loose, &orm)
    {
        notes.push(format!(
            "{} is used for {slot}, not the {} channel",
            file_name(l),
            file_name(o)
        ));
    }
    loose.or(orm)
}

/// The recipes of one set under `target` (spec §6.4 recipe table) and the raw maps none of them uses, with
/// `packing.recipe = auto`.
pub fn recipes_for(target: Target, raw: &[RawInput]) -> (Vec<Recipe>, Vec<Unused>) {
    recipes_for_with(target, raw, PackRecipe::Auto)
}

/// [`recipes_for`] with the set's `packing.recipe`: it only matters on cs (DECISIONS Note R-fills-1).
pub fn recipes_for_with(
    target: Target,
    raw: &[RawInput],
    pack_recipe: PackRecipe,
) -> (Vec<Recipe>, Vec<Unused>) {
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
            channel: None,
        })
    };
    // Note AL1: an `_orm` holds AO in R, roughness in G and metallic in B.
    let orm = |channel: u8, invert: bool| {
        by_role.get(&Orm).map(|r| ChannelNeed::Source {
            path: r.path.clone(),
            role: Orm,
            invert,
            channel: Some(channel),
        })
    };
    let has = |role: RawMap| by_role.contains_key(&role);
    let mut recipes = vec![];
    match target {
        Target::Cs => {
            // `_rfaos` has no metallic channel, so a lone metallic map cannot start a forced `_rfaos`.
            let triggers: &[RawMap] = if pack_recipe == PackRecipe::Rfaos {
                &[Roughness, Glossiness, Ao, Orm]
            } else {
                &[Roughness, Glossiness, Ao, Metallic, Orm]
            };
            if triggers.iter().copied().any(has) {
                let mut notes = vec![];
                let rough = prefer(
                    src(Roughness, false).or_else(|| src(Glossiness, true)),
                    orm(1, false),
                    "roughness",
                    &mut notes,
                )
                .unwrap_or(Missing(Slot::Roughness));
                let ao = prefer(src(Ao, false), orm(0, false), "AO", &mut notes)
                    .unwrap_or(Missing(Slot::Ao));
                let spec = src(Specular, false).unwrap_or(Missing(Slot::Specular));
                // DECISIONS Note R-fills-1: `auto` is `_rfaos` when a fuzz source exists; `rfaos` always is, with
                // the G channel filled or defaulted when there is none; `rmaos` never is.
                let fuzz = match pack_recipe {
                    PackRecipe::Auto => src(Fuzz, false),
                    PackRecipe::Rfaos => Some(src(Fuzz, false).unwrap_or(Missing(Slot::Fuzz))),
                    PackRecipe::Rmaos => None,
                };
                match fuzz {
                    Some(fuzz) => recipes.push(Recipe {
                        kind: RecipeKind::Rfaos,
                        channels: [rough, fuzz, ao, spec],
                        notes,
                    }),
                    None => {
                        let metal =
                            prefer(src(Metallic, false), orm(2, false), "metallic", &mut notes)
                                .unwrap_or(Missing(Slot::Metallic));
                        recipes.push(Recipe {
                            kind: RecipeKind::Rmaos,
                            channels: [rough, metal, ao, spec],
                            notes,
                        })
                    }
                }
            }
            if let Some(h) = src(Height, false) {
                recipes.push(Recipe {
                    kind: RecipeKind::Parallax,
                    channels: [h, MirrorR, MirrorR, Opaque],
                    notes: vec![],
                });
            }
        }
        Target::Cpm => {
            if [Roughness, Glossiness, Metallic, Height, Orm]
                .into_iter()
                .any(has)
            {
                let mut notes = vec![];
                let metal = prefer(src(Metallic, false), orm(2, false), "metallic", &mut notes);
                let gloss = prefer(
                    src(Glossiness, false).or_else(|| src(Roughness, true)),
                    orm(1, true),
                    "glossiness",
                    &mut notes,
                );
                recipes.push(Recipe {
                    kind: RecipeKind::CpmM,
                    channels: [
                        metal
                            .clone()
                            .or_else(|| src(Specular, false))
                            .unwrap_or(Missing(Slot::EnvMask)),
                        gloss.unwrap_or(Missing(Slot::Glossiness)),
                        metal.unwrap_or(Missing(Slot::Metallic)),
                        src(Height, false).unwrap_or(Missing(Slot::Height)),
                    ],
                    notes,
                });
            }
        }
        Target::Vanilla => {
            let mut notes = vec![];
            if let Some(env) = prefer(src(Metallic, false), orm(2, false), "metallic", &mut notes)
                .or_else(|| src(Specular, false))
            {
                recipes.push(Recipe {
                    kind: RecipeKind::VanillaM,
                    channels: [env, MirrorR, MirrorR, Opaque],
                    notes,
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
                reason: unused_reason(target, pack_recipe, r.role, &by_role),
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
    /// R (or luma) of a raw source; `invert` stores 1 − x. `channel` (0..=2) reads that channel of an `_orm`
    /// instead (DECISIONS Note AL); absent otherwise, so no other job's JSON changes.
    Source {
        path: PathBuf,
        role: RawMap,
        invert: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        channel: Option<u8>,
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
            ChannelSource::Source {
                path,
                invert,
                channel,
                ..
            } => Some(Provenance::Source(format!(
                "{}{}:{}",
                if *invert { "1-" } else { "" },
                file_name(path),
                match (channel, gray) {
                    (Some(c), _) => ["R", "G", "B"][usize::from(*c)],
                    (None, GraySource::R) => "R",
                    (None, GraySource::Luma) => "luma",
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
        Slot::Fuzz => direct(RawMap::Fuzz),
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
        // 0: no fuzz, for an `_rfaos` built without a fuzz source (Note R-fills-1).
        Slot::Fuzz => 0.0,
    }
}

/// The channels of a recipe with fills and defaults applied, and the channels that took a neutral default
/// (index, slot): each raises `PACK-DEFAULTED`; a filled channel does not (DECISIONS Note M5).
pub fn resolve_channels(r: &Recipe, p: &PackingSpec) -> ([ChannelSource; 4], Vec<(usize, Slot)>) {
    let mut defaulted = vec![];
    let channels = std::array::from_fn(|i| match &r.channels[i] {
        ChannelNeed::Source {
            path,
            role,
            invert,
            channel,
        } => ChannelSource::Source {
            path: path.clone(),
            role: *role,
            invert: *invert,
            channel: *channel,
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
