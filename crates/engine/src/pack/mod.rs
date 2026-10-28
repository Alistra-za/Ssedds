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
    /// Complex `<base>_m.dds` of a vanilla complex-material set: env mask, glossiness, metallic, height (Note AZ3).
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

/// The channel of a CS `_rmaos` that holds `slot` (R roughness, G metallic, B AO, A specular; DECISIONS Note CN).
pub fn rmaos_channel(slot: Slot) -> Option<u8> {
    match slot {
        Slot::Roughness => Some(0),
        Slot::Metallic => Some(1),
        Slot::Ao => Some(2),
        Slot::Specular => Some(3),
        _ => None,
    }
}

/// Whether the B (AO) of a pre-packed `_rmaos`/`_rfaos` image holds AO (Note CO103): it varies by 2/255 or more, or
/// it is not near white. A flat white B is empty, and a generated AO may fill it.
pub fn b_has_content(b: &crate::analyse::ChannelStats) -> bool {
    // A hair under the stated thresholds, so 8-bit codes 2 apart or a mean of exactly 250 count.
    f64::from(b.max - b.min) * 255.0 >= 2.0 - 1e-3 || b.mean * 255.0 <= 250.0 + 1e-3
}

/// The `ChannelSource::Source.channel` that reads a source's luma (Note DT), past R, G, B and A.
pub const LUMA_CHANNEL: u8 = 4;

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

/// Why a raw map of `role` takes part in no recipe of `target` (DECISIONS Note M2); `complex` is a vanilla set
/// that packs the complex `_m` (Note AZ).
fn unused_reason(
    target: Target,
    complex: bool,
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
        // `Rmaos` is never a file-name role (`raw_role`), so its arm is a fallback only.
        (_, RawMap::Curves | RawMap::Cavity | RawMap::Rmaos) => {
            "read only by Also export as (Vanilla / Complex material)".into()
        }
        (_, RawMap::Opacity) => "only fills a colour texture's alpha; no target in the set".into(),
        (_, RawMap::FuzzWeight) => "only fills a PBR _f alpha; no _f planned".into(),
        (_, RawMap::SubsurfaceWeight) => "only fills a PBR _s alpha; no _s planned".into(),
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
        (Target::Vanilla, RawMap::Roughness) if complex && has(RawMap::Glossiness) => {
            "glossiness is used: the complex _m G channel is glossiness".into()
        }
        (Target::Vanilla, RawMap::Specular) if has(RawMap::Metallic) => {
            "the env mask comes from the metallic source".into()
        }
        (Target::Vanilla, RawMap::Ao | RawMap::Fuzz) if complex => {
            format!("the complex _m has no {} channel", role.id())
        }
        (Target::Vanilla, _) if complex => {
            "nothing to pack: the complex _m needs a roughness, glossiness, metallic or height source"
                .into()
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
/// `packing.recipe = auto`, for a set whose shader is not complex material.
pub fn recipes_for(target: Target, raw: &[RawInput]) -> (Vec<Recipe>, Vec<Unused>) {
    recipes_for_with(target, raw, PackRecipe::Auto, false)
}

/// [`recipes_for`] with the set's `packing.recipe` (it only matters on cs, DECISIONS Note R-fills-1) and whether
/// the set's shader is complex material: under vanilla such a set packs the complex `_m` instead of the env-mask
/// `_m` (Note AZ); cs ignores it.
pub fn recipes_for_with(
    target: Target,
    raw: &[RawInput],
    pack_recipe: PackRecipe,
    complex: bool,
) -> (Vec<Recipe>, Vec<Unused>) {
    recipes_for_sel(target, raw, pack_recipe, complex, &BTreeMap::new())
}

/// The slot channel `ch` of a `kind` recipe holds; `None` for a mirrored or opaque filler.
pub(crate) fn slot_of(kind: RecipeKind, ch: usize) -> Option<Slot> {
    use Slot::*;
    let slots = match kind {
        RecipeKind::Rmaos => [Some(Roughness), Some(Metallic), Some(Ao), Some(Specular)],
        RecipeKind::Rfaos => [Some(Roughness), Some(Fuzz), Some(Ao), Some(Specular)],
        RecipeKind::Parallax => [Some(Height), None, None, None],
        RecipeKind::CpmM => [
            Some(EnvMask),
            Some(Glossiness),
            Some(Metallic),
            Some(Height),
        ],
        RecipeKind::VanillaM => [Some(EnvMask), None, None, None],
    };
    slots[ch]
}

/// [`recipes_for_with`] with each raw input's hand-set `helper.*` selectors (DECISIONS Note DT; empty = today).
/// An input with `helper.fills` takes no suggested slot (the next file of its role may, M13) and fills the packed
/// channels it names, creating that recipe when the set would build none; among inputs naming one channel the first
/// by path wins. `helper.read` and `helper.invert` apply wherever an input lands, suggested or hand-set.
pub fn recipes_for_sel(
    target: Target,
    raw: &[RawInput],
    pack_recipe: PackRecipe,
    complex: bool,
    sel: &BTreeMap<PathBuf, crate::helper::HelperSel>,
) -> (Vec<Recipe>, Vec<Unused>) {
    use crate::helper::FillTarget;
    use ChannelNeed::{MirrorR, Missing, Opaque};
    use RawMap::*;
    let assigned = |p: &Path| sel.get(p).is_some_and(|s| s.assigned());
    let mut sorted: Vec<&RawInput> = raw.iter().collect();
    // Note DT (Ruling 8): an assigned source (hand-set role or fills) comes before a suggested one.
    sorted.sort_by(|a, b| (!assigned(&a.path), &a.path).cmp(&(!assigned(&b.path), &b.path)));
    // DECISIONS Note M13: one source per role, the first by path.
    let mut by_role: BTreeMap<RawMap, &RawInput> = BTreeMap::new();
    let mut unused = vec![];
    let mut hand: Vec<(&RawInput, &[FillTarget])> = vec![];
    for r in sorted {
        if let Some(fills) = sel.get(&r.path).and_then(|s| s.fills.as_deref()) {
            hand.push((r, fills));
            continue;
        }
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
    // The layout of a pre-packed `_rmaos`/`_rfaos` image, from its name.
    let prepacked = by_role.get(&Rmaos).map(|img| {
        let stem = img.path.file_stem().unwrap_or_default().to_string_lossy();
        match crate::group::split_suffix(&stem).1.as_deref() {
            Some("rfaos") => RecipeKind::Rfaos,
            _ => RecipeKind::Rmaos,
        }
    });
    // The cs `_rmaos`/`_rfaos` this set packs, as built below.
    let cs_pack = prepacked.unwrap_or(
        if pack_recipe == PackRecipe::Rfaos || (pack_recipe == PackRecipe::Auto && has(Fuzz)) {
            RecipeKind::Rfaos
        } else {
            RecipeKind::Rmaos
        },
    );
    // A hand-set target's recipe and channel; `Err(None)` for an alpha target (not packing's).
    let packed_target = |t: &FillTarget| -> Result<(RecipeKind, usize), Option<String>> {
        let FillTarget::Packed { suffix, channel } = *t else {
            return Err(None);
        };
        let kind = match (target, suffix) {
            (Target::Cs, "p") => Ok(RecipeKind::Parallax),
            (Target::Cs, "rmaos" | "rfaos") if suffix == cs_pack.suffix() => Ok(cs_pack),
            (Target::Cs, "rmaos" | "rfaos") => Err(format!("the set packs _{}", cs_pack.suffix())),
            (Target::Vanilla, "m") if complex => Ok(RecipeKind::CpmM),
            (Target::Vanilla, "m") if channel == 0 => Ok(RecipeKind::VanillaM),
            (Target::Vanilla, "m") => Err("the env-mask _m has only R".into()),
            (Target::Cs, _) => Err(format!("the cs target packs no _{suffix}")),
            (Target::Vanilla, _) => Err(format!("the vanilla target packs no _{suffix}")),
        };
        kind.map(|k| (k, usize::from(channel)))
            .map_err(|e| Some(format!("{}: {e}", t.id())))
    };
    // Recipes a hand-set fill builds even when the set's suggested sources start none.
    let forced: BTreeSet<RecipeKind> = hand
        .iter()
        .flat_map(|(_, fills)| fills.iter())
        .filter_map(|t| packed_target(t).ok().map(|(k, _)| k))
        .collect();
    let mut recipes = vec![];
    match target {
        Target::Cs => {
            // `_rfaos` has no metallic channel, so a lone metallic map cannot start a forced `_rfaos`.
            let triggers: &[RawMap] = if pack_recipe == PackRecipe::Rfaos {
                &[Roughness, Glossiness, Ao, Orm]
            } else {
                &[Roughness, Glossiness, Ao, Metallic, Orm]
            };
            // Note CO104 (Track A R1): `plan_packing` passes a pre-packed `_rmaos`/`_rfaos` image only beside a lone
            // generated AO, when the image's B is empty. R, G (metallic, or fuzz on `_rfaos`) and A come from the
            // image, B from the AO.
            if let (Some(img), Some(kind)) = (by_role.get(&Rmaos), prepacked) {
                let ch = |slot: Slot| ChannelNeed::Source {
                    path: img.path.clone(),
                    role: Rmaos,
                    invert: false,
                    channel: rmaos_channel(slot),
                };
                recipes.push(Recipe {
                    kind,
                    channels: [
                        ch(Slot::Roughness),
                        ch(Slot::Metallic),
                        src(Ao, false).unwrap_or(Missing(Slot::Ao)),
                        ch(Slot::Specular),
                    ],
                    notes: vec![],
                });
            } else if triggers.iter().copied().any(has) || forced.contains(&cs_pack) {
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
                        // Note AL5: an ORM's B has nowhere to go; say so, as a loose metallic map would be unused.
                        notes: {
                            if let Some(ChannelNeed::Source { path, .. }) = orm(2, false) {
                                notes.push(format!(
                                    "_rfaos has no metallic channel; the metallic (B) of {} is not used",
                                    file_name(&path)
                                ));
                            }
                            notes
                        },
                        kind: RecipeKind::Rfaos,
                        channels: [rough, fuzz, ao, spec],
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
            if let Some(h) = src(Height, false).or_else(|| {
                forced
                    .contains(&RecipeKind::Parallax)
                    .then_some(Missing(Slot::Height))
            }) {
                recipes.push(Recipe {
                    kind: RecipeKind::Parallax,
                    channels: [h, MirrorR, MirrorR, Opaque],
                    notes: vec![],
                });
            }
        }
        Target::Vanilla if complex => {
            if [Roughness, Glossiness, Metallic, Height, Orm]
                .into_iter()
                .any(has)
                || forced.contains(&RecipeKind::CpmM)
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
                .or_else(|| {
                    forced
                        .contains(&RecipeKind::VanillaM)
                        .then_some(Missing(Slot::EnvMask))
                })
            {
                recipes.push(Recipe {
                    kind: RecipeKind::VanillaM,
                    channels: [env, MirrorR, MirrorR, Opaque],
                    notes,
                });
            }
        }
    }
    // Hand-set fills, in path order: an assigned source replaces what the channel holds (DECISIONS Note DT).
    let mut winner: BTreeMap<(RecipeKind, usize), &Path> = BTreeMap::new();
    let mut displaced: BTreeMap<PathBuf, String> = BTreeMap::new();
    for (r, fills) in &hand {
        let mut landed = false;
        let mut why = vec![];
        for t in fills.iter() {
            let (kind, ch) = match packed_target(t) {
                Ok(k) => k,
                Err(None) => continue,
                Err(Some(e)) => {
                    why.push(e);
                    continue;
                }
            };
            if let Some(w) = winner.get(&(kind, ch)) {
                why.push(format!("{} is filled by {}", t.id(), file_name(w)));
                continue;
            }
            let Some(recipe) = recipes.iter_mut().find(|x| x.kind == kind) else {
                why.push(format!("{}: no _{} is planned", t.id(), kind.suffix()));
                continue;
            };
            // A hand-set role that took this slot is assigned too: the first by path keeps it (Ruling 8).
            if let ChannelNeed::Source { path: held, .. } = &recipe.channels[ch] {
                if assigned(held) && *held < r.path {
                    why.push(format!("{} is filled by {}", t.id(), file_name(held)));
                    continue;
                }
            }
            // Glossiness into a roughness slot (or back) is stored inverted, as a suggested source is.
            let invert = matches!(
                (r.role, slot_of(kind, ch)),
                (Glossiness, Some(Slot::Roughness)) | (Roughness, Some(Slot::Glossiness))
            );
            let old = std::mem::replace(
                &mut recipe.channels[ch],
                ChannelNeed::Source {
                    path: r.path.clone(),
                    role: r.role,
                    invert,
                    channel: None,
                },
            );
            if let ChannelNeed::Source { path, .. } = old {
                displaced
                    .entry(path)
                    .or_insert_with(|| format!("{} is filled by {}", t.id(), file_name(&r.path)));
            }
            winner.insert((kind, ch), &r.path);
            landed = true;
        }
        if !landed {
            let reason = why.into_iter().next().unwrap_or_else(|| {
                if fills.is_empty() {
                    "helper.fills is empty: not used".into()
                } else {
                    "helper.fills names no packed channel".into()
                }
            });
            unused.push(Unused {
                path: r.path.clone(),
                role: r.role,
                reason,
            });
        }
    }
    // `helper.read` and `helper.invert` apply wherever a selected source landed.
    for recipe in &mut recipes {
        for c in &mut recipe.channels {
            if let ChannelNeed::Source {
                path,
                invert,
                channel,
                ..
            } = c
            {
                if let Some(s) = sel.get(path) {
                    *invert ^= s.invert;
                    if let Some(read) = s.read {
                        *channel = Some(read.channel());
                    }
                }
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
                reason: displaced.remove(&r.path).unwrap_or_else(|| {
                    unused_reason(target, complex, pack_recipe, r.role, &by_role)
                }),
            });
        }
    }
    unused.sort_by(|a, b| a.path.cmp(&b.path));
    (recipes, unused)
}

/// The suffixes a set's recipes would add (spec §6.4 "Set membership", DECISIONS Note M7), from the raw roles
/// present, before anything is resolved.
pub fn predicted_suffixes(
    target: Target,
    complex: bool,
    roles: &BTreeSet<RawMap>,
) -> Vec<&'static str> {
    let raw: Vec<RawInput> = roles
        .iter()
        .map(|r| RawInput {
            path: PathBuf::from(r.id()),
            role: *r,
        })
        .collect();
    recipes_for_with(target, &raw, PackRecipe::Auto, complex)
        .0
        .iter()
        .map(|r| r.kind.suffix())
        .collect()
}

/// What a packed channel holds; serialised in `file_planned` (`job.pack.channels`) and the manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "from", rename_all = "snake_case")]
pub enum ChannelSource {
    /// R (or luma) of a raw source; `invert` stores 1 − x. `channel` reads that channel of an `_orm` (0..=2,
    /// DECISIONS Note AL) or an `_rmaos` (0..=3, alpha included, Note CN) instead; absent otherwise, so no other
    /// job's JSON changes.
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
                    (Some(LUMA_CHANNEL), _) => "luma",
                    (Some(c), _) => ["R", "G", "B", "A"][usize::from(*c)],
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
    /// Note DT: the source's explicit `helper.read`, kept apart from the channel a recipe assigns (`_orm`, `_rmaos`),
    /// so the record names it. Absent otherwise, so no other job's JSON changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read: Option<crate::helper::HelperRead>,
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

/// Note DT: `read:<id>` for a source with an explicit `helper.read` (recorded as `SourceRef.fill`), so a changed
/// read re-encodes under `if-changed`; `None` without one.
pub fn read_of(s: &PackSource) -> Option<String> {
    s.read.map(|r| format!("read:{}", r.id()))
}
