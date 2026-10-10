//! The preset inspector (main spec §9 #3): every resolved field, where it came from, and whether it differs from
//! the base preset's value for the same target and sub-type.
use engine::policy::ResolvedJob;
use engine::presets::{leaf_paths, PresetRegistry};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct InspectorRow {
    pub field: String,
    pub value: Value,
    pub provenance: String,
    pub differs_from_base: bool,
}

fn at<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(v, |v, k| v.get(k))
}

pub fn inspector_rows(job: &ResolvedJob, registry: &PresetRegistry) -> Vec<InspectorRow> {
    let now = serde_json::to_value(&job.profile).unwrap_or_default();
    let base = serde_json::to_value(registry.base_profile(job.profile.target, job.profile.subtype))
        .unwrap_or_default();
    let mut paths = vec![];
    leaf_paths(&now, "", &mut paths);
    paths
        .into_iter()
        .map(|field| {
            let value = at(&now, &field).cloned().unwrap_or(Value::Null);
            InspectorRow {
                differs_from_base: at(&base, &field) != Some(&value),
                provenance: job
                    .provenance
                    .get(&field)
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "builtin".into()),
                value,
                field,
            }
        })
        .collect()
}
