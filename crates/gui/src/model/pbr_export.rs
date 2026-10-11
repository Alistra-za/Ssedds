//! Export of PBR sets (M3b spec §6): the checks that block or ask, and the JSON entries. No egui types.
use super::sets::{
    engine_inputs, item_output_key, output_candidates, set_views, SetView, SlotFill, SlotKind,
};
use super::AppModel;
use crate::pgp::prefix::{check_rel_path, pitfalls};
use crate::pgp::settings::{blocked_by, feature_key, feature_on, Feature, FEATURE_KEYS};
use crate::pgp::validate::validate;
use crate::pgp::writer::{document, EntryInput};
use engine::config::Target;
use engine::sets::OutputKey;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportIssue {
    pub set_id: String,
    /// The engine key, for selecting the set (empty for a created set with no members, and for project-wide issues).
    pub set_key: String,
    pub text: String,
}

fn issue(v: &SetView, text: String) -> ExportIssue {
    ExportIssue {
        set_id: v.id.clone(),
        set_key: v.key.clone(),
        text,
    }
}

fn general(text: String) -> ExportIssue {
    ExportIssue {
        set_id: String::new(),
        set_key: String::new(),
        text,
    }
}

/// The JSON file name (without `.json`): the project's, else the opened folder's name.
pub fn json_name(m: &AppModel) -> String {
    m.sets_doc
        .as_ref()
        .and_then(|d| d.json_name())
        .filter(|n| !n.trim().is_empty())
        .or_else(|| {
            m.root
                .as_ref()
                .and_then(|r| r.file_name())
                .map(|n| n.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "sdc".into())
}

/// The final slot paths of a set: a borrowed slot points at the owner's output; an explicit `slotN` setting wins.
fn slots_of(
    v: &SetView,
    views: &[SetView],
    paths: &BTreeMap<OutputKey, PathBuf>,
    m: &AppModel,
) -> BTreeMap<u8, String> {
    let mut out = BTreeMap::new();
    for (kind, fill) in &v.slots {
        let SlotFill::Borrowed { item, .. } = fill else {
            continue;
        };
        let path = m
            .item(item)
            .and_then(|it| item_output_key(views, it))
            .and_then(|k| paths.get(&k));
        if let Some(p) = path {
            out.insert(kind.slot_number(), p.to_string_lossy().replace('/', "\\"));
        }
    }
    for n in 1..=8u8 {
        if let Some(Value::String(s)) = v.settings.get(&format!("slot{n}")) {
            out.insert(n, s.clone());
        }
    }
    out
}

/// Every PBR set's JSON entry, in display order (CS only; empty otherwise).
pub fn json_entries(m: &AppModel) -> Vec<(SetView, EntryInput)> {
    if m.target != Target::Cs {
        return vec![];
    }
    let (_, paths) = engine_inputs(m);
    let views = set_views(m);
    views
        .iter()
        .filter(|v| v.pbr)
        .map(|v| {
            let e = EntryInput {
                settings: v.settings.clone(),
                slots: slots_of(v, &views, &paths, m),
                derived_match: v.derived_match.clone().unwrap_or_default(),
            };
            (v.clone(), e)
        })
        .collect()
}

const NEEDS: [(Feature, SlotKind, &str, &str); 3] = [
    (
        Feature::Emissive,
        SlotKind::Emissive,
        "emissive",
        "an emissive texture (_g)",
    ),
    (
        Feature::Parallax,
        SlotKind::Height,
        "parallax",
        "a height texture (_p)",
    ),
    (
        Feature::Subsurface,
        SlotKind::Subsurface,
        "subsurface",
        "a subsurface texture (_s)",
    ),
];

fn has(v: &SetView, s: SlotKind) -> bool {
    !matches!(v.slots.get(&s), None | Some(SlotFill::Empty))
}

/// What stops Export (spec §6.3). On CS the PBR sets are checked; slot clashes apply to every set.
pub fn export_blocks(m: &AppModel) -> Vec<ExportIssue> {
    let mut out = vec![];
    for v in &set_views(m) {
        if has(v, SlotKind::Coat) && has(v, SlotKind::Fuzz) {
            out.push(issue(v, format!("{}: _cnr and _f both use slot 7", v.id)));
        }
        if m.target != Target::Cs || !v.pbr {
            continue;
        }
        let s = &v.settings;
        for (f, slot, name, what) in NEEDS {
            if feature_on(s, f) && blocked_by(s, f).is_none() && !has(v, slot) {
                out.push(issue(
                    v,
                    format!("{}: {name} is on but the set has no {what}", v.id),
                ));
            }
        }
        if feature_on(s, Feature::Multilayer)
            && s.get("coat_normal") == Some(&Value::Bool(true))
            && !has(v, SlotKind::Coat)
        {
            out.push(issue(
                v,
                format!(
                    "{}: coat normal is on but the set has no coat normal texture (_cnr)",
                    v.id
                ),
            ));
        }
        if feature_on(s, Feature::Fuzz)
            && s.get("fuzz.texture") == Some(&Value::Bool(true))
            && !has(v, SlotKind::Fuzz)
        {
            out.push(issue(
                v,
                format!(
                    "{}: fuzz texture is on but the set has no fuzz texture (_f)",
                    v.id
                ),
            ));
        }
        for (f, _) in FEATURE_KEYS {
            if let Some(b) = feature_on(s, f).then(|| blocked_by(s, f)).flatten() {
                if f < b {
                    out.push(issue(
                        v,
                        format!(
                            "{}: {} and {} can't both be on",
                            v.id,
                            feature_key(f),
                            feature_key(b)
                        ),
                    ));
                }
            }
        }
        if let Some(e) = &v.prefix_error {
            out.push(issue(v, format!("{}: {e}", v.id)));
        }
        for n in 1..=8u8 {
            if let Some(Value::String(p)) = s.get(&format!("slot{n}")) {
                if !p.is_empty() {
                    if let Err(e) = check_rel_path(p, &format!("slot{n}")) {
                        out.push(issue(v, format!("{}: {}", v.id, e.0)));
                    }
                }
            }
        }
    }
    out.extend(path_collisions(m));
    if m.target == Target::Cs {
        if let Err(e) = check_rel_path(&json_name(m), "JSON name") {
            out.push(general(e.0));
        }
        let entries: Vec<EntryInput> = json_entries(m).into_iter().map(|(_, e)| e).collect();
        if !entries.is_empty() {
            if let Err(errs) = validate(&document(&entries).to_value()) {
                out.push(general(format!(
                    "PGPatcher JSON does not validate: {}",
                    errs[0]
                )));
            }
        }
    }
    out
}

fn path_lower(p: &std::path::Path) -> String {
    p.to_string_lossy().replace('\\', "/").to_lowercase()
}

/// Two outputs on one path. Owned outputs are compared before `engine_inputs` drops the second, so the engine never
/// sees the collision (Review focus 2). An owned output is also compared with the mirrored path of every output that
/// has no explicit path: the engine does not re-check those (Task 10 review, Minor 8).
pub fn path_collisions(m: &AppModel) -> Vec<ExportIssue> {
    if m.target != Target::Cs {
        return vec![];
    }
    let views = set_views(m);
    let name = |k: &OutputKey| match k {
        OutputKey::File(f) => f.clone(),
        OutputKey::Packed { set, suffix } => format!("{set} ({suffix})"),
    };
    let key_of_set = |id: &str| {
        views
            .iter()
            .find(|v| v.id == id)
            .map(|v| v.key.clone())
            .unwrap_or_default()
    };
    let candidates = output_candidates(m, &views);
    let mut seen: BTreeMap<String, (String, OutputKey)> = BTreeMap::new();
    let mut out = vec![];
    for (owner, k, p) in &candidates {
        match seen.get(&path_lower(p)) {
            Some((first_owner, first)) => out.push(ExportIssue {
                set_id: owner.clone(),
                set_key: key_of_set(owner),
                text: format!(
                    "{first_owner} and {owner} both write {} ({} and {})",
                    p.display(),
                    name(first),
                    name(k)
                ),
            }),
            None => {
                seen.insert(path_lower(p), (owner.clone(), k.clone()));
            }
        }
    }
    // Mirrored outputs: items that will write a file but have no explicit path.
    let owned: Vec<&OutputKey> = candidates.iter().map(|(_, k, _)| k).collect();
    for it in &m.items {
        if it.job.is_none() || it.job_key.is_empty() {
            continue;
        }
        if item_output_key(&views, it).is_some_and(|k| owned.contains(&&k)) {
            continue;
        }
        if let Some((owner, k)) = seen.get(&it.job_key.replace('\\', "/").to_lowercase()) {
            out.push(ExportIssue {
                set_id: owner.clone(),
                set_key: key_of_set(owner),
                text: format!(
                    "{owner} writes {} ({}), the same file {} is written to (no path of its own)",
                    it.job_key,
                    name(k),
                    it.key
                ),
            });
        }
    }
    out
}

/// What asks "Proceed anyway / Cancel": the pitfalls of every PBR set.
pub fn export_asks(m: &AppModel) -> Vec<ExportIssue> {
    if m.target != Target::Cs {
        return vec![];
    }
    set_views(m)
        .iter()
        .filter(|v| v.pbr)
        .flat_map(|v| {
            pitfalls(&v.settings)
                .into_iter()
                .map(move |w| issue(v, format!("{}: {w}", v.id)))
        })
        .collect()
}
