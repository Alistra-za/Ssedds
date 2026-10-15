//! A set's feature keys that turn on by themselves from its member textures (owner, 2026-10-14; DECISIONS AJ7-AJ11).
//! They are defaults, never stored: an explicit value (including `false`) wins, and a removed member removes the value.
use super::sets::{SlotFill, SlotKind};
use crate::pgp::settings::{blocked_by, feature_on, Feature, Settings, FOLIAGE_KEY};
use serde_json::{json, Value};
use std::collections::BTreeMap;

type Why = (&'static str, &'static str);

fn add(
    eff: &mut Settings,
    out: &mut Vec<Why>,
    key: &'static str,
    f: Option<Feature>,
    why: &'static str,
) {
    if eff.contains_key(key) || f.is_some_and(|f| blocked_by(eff, f).is_some()) {
        return;
    }
    eff.insert(key.to_string(), json!(true));
    out.push((key, why));
}

/// The keys the members switch on, each with why, in the order they were decided. `pbr` false (not a PBR set): none.
pub fn auto_flags(
    pbr: bool,
    slots: &BTreeMap<SlotKind, SlotFill>,
    explicit: &Settings,
) -> Vec<Why> {
    auto_flags_for(pbr, slots, explicit, &SsHint::default())
}

/// [`auto_flags`] with the scan and path evidence that tells Standard from Foliage subsurface.
pub fn auto_flags_for(
    pbr: bool,
    slots: &BTreeMap<SlotKind, SlotFill>,
    explicit: &Settings,
    hint: &SsHint,
) -> Vec<Why> {
    let mut out: Vec<Why> = vec![];
    if !pbr {
        return out;
    }
    let mut eff = explicit.clone();
    let has = |k: SlotKind| !matches!(slots.get(&k), None | Some(SlotFill::Empty));
    if has(SlotKind::Coat) {
        let why = "on because the set has a _cnr";
        add(
            &mut eff,
            &mut out,
            "multilayer",
            Some(Feature::Multilayer),
            why,
        );
        add(&mut eff, &mut out, "coat_normal", None, why);
    }
    if has(SlotKind::Fuzz) {
        add(
            &mut eff,
            &mut out,
            "fuzz_on",
            Some(Feature::Fuzz),
            "on because the set has an _f",
        );
    }
    if has(SlotKind::Height) {
        add(
            &mut eff,
            &mut out,
            "parallax",
            Some(Feature::Parallax),
            "on because the set has a _p",
        );
    }
    if has(SlotKind::Emissive) {
        add(
            &mut eff,
            &mut out,
            "emissive",
            Some(Feature::Emissive),
            "on because the set has a _g",
        );
    }
    if has(SlotKind::Subsurface) {
        if feature_on(&eff, Feature::Multilayer) {
            add(
                &mut eff,
                &mut out,
                "coat_diffuse",
                None,
                "on because the set has an _s and Multilayer is on",
            );
        } else if !eff.contains_key(FOLIAGE_KEY) && !eff.contains_key("subsurface") {
            // An explicit choice (either key, even false) wins (AG102).
            let (key, why) = hint.pick();
            add(&mut eff, &mut out, key, Some(Feature::Subsurface), why);
        }
    }
    out
}

/// The explicit settings with the auto ones filled in where the key is unset.
pub fn resolved(pbr: bool, slots: &BTreeMap<SlotKind, SlotFill>, explicit: &Settings) -> Settings {
    resolved_for(pbr, slots, explicit, &SsHint::default())
}

pub fn resolved_for(
    pbr: bool,
    slots: &BTreeMap<SlotKind, SlotFill>,
    explicit: &Settings,
    hint: &SsHint,
) -> Settings {
    let mut s = explicit.clone();
    for (k, _) in auto_flags_for(pbr, slots, explicit, hint) {
        s.insert(k.to_string(), Value::Bool(true));
    }
    s
}

/// What decides Standard or Foliage for an `_s` (DECISIONS AG102): the scan's Tree_Anim answer for the set's meshes
/// (`None`: no scan evidence) and the set's path.
#[derive(Debug, Clone, Default)]
pub struct SsHint {
    pub tree_anim: Option<bool>,
    pub path: String,
}

/// Folders that hold trees and plants, matched on `\`-bounded path parts.
const TREE_PATHS: [(&str, &str); 3] = [
    (
        "\\textures\\trees\\",
        "Foliage: path under textures\\trees (no scan)",
    ),
    (
        "\\landscape\\trees\\",
        "Foliage: path under landscape\\trees (no scan)",
    ),
    (
        "\\plants\\",
        "Foliage: path under a plants folder (no scan)",
    ),
];

impl SsHint {
    fn pick(&self) -> Why {
        if self.tree_anim == Some(true) {
            return (FOLIAGE_KEY, "Foliage: the set's meshes use Tree Anim");
        }
        // A scan that finds no Tree_Anim mesh beats a path guess.
        if self.tree_anim.is_none() {
            let p = format!("\\{}", self.path.to_lowercase().replace('/', "\\"));
            // `derived_match` may have no `textures\` lead; add it so `textures\trees\` reads the same either way.
            let p = if p.starts_with("\\textures\\") {
                p
            } else {
                format!("\\textures{p}")
            };
            if let Some((_, why)) = TREE_PATHS.iter().find(|(n, _)| p.contains(n)) {
                return (FOLIAGE_KEY, why);
            }
        }
        ("subsurface", "on because the set has an _s")
    }
}

/// The hint for set `v`: its diffuse's mesh key looked up in the scan evidence the plan already read.
pub fn hint(m: &crate::model::AppModel, v: &super::sets::SetView) -> SsHint {
    let key = match v.slots.get(&SlotKind::Diffuse) {
        Some(SlotFill::Own { item }) => m.item(item),
        _ => None,
    }
    .and_then(|i| i.job.as_ref())
    .and_then(|j| {
        engine::shader::colour_mesh_key(&j.game_path, &j.output, Some(j.profile.subtype))
    });
    SsHint {
        tree_anim: key.and_then(|k| m.mesh_tree_anim.get(&k).copied()),
        path: v.derived_match.clone().unwrap_or_else(|| v.id.clone()),
    }
}
