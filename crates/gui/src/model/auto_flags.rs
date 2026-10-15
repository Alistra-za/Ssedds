//! A set's feature keys that turn on by themselves from its member textures (owner, 2026-10-14; DECISIONS AJ7-AJ11).
//! They are defaults, never stored: an explicit value (including `false`) wins, and a removed member removes the value.
use super::sets::{SlotFill, SlotKind};
use crate::pgp::settings::{blocked_by, feature_on, Feature, Settings};
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
        } else {
            add(
                &mut eff,
                &mut out,
                "subsurface",
                Some(Feature::Subsurface),
                "on because the set has an _s",
            );
        }
    }
    out
}

/// The explicit settings with the auto ones filled in where the key is unset.
pub fn resolved(pbr: bool, slots: &BTreeMap<SlotKind, SlotFill>, explicit: &Settings) -> Settings {
    let mut s = explicit.clone();
    for (k, _) in auto_flags(pbr, slots, explicit) {
        s.insert(k.to_string(), Value::Bool(true));
    }
    s
}
