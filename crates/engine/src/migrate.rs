//! M6 Track D (spec 2026-10-12-sdc-m6-presets-design.md §5, DECISIONS Note T9): one migration hook per schema
//! family. Each reads the stored version, applies the family's steps from that version up to the current one, and
//! returns the upgraded value; the loaders deserialise only what a hook returns. A later bump adds one named function
//! to its family's table.
use crate::manifest::MANIFEST_SCHEMA;
use crate::presets::SCHEMA_VERSION as PRESET_SCHEMA;

/// Version of `config.toml` / `sdc.toml`; an absent `schema_version` is 1.
pub const CONFIG_SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MigrateError {
    #[error("missing or invalid {0} (a whole number from 1)")]
    NoVersion(&'static str),
    #[error("{key} {found} is newer than this sdc supports ({current})")]
    Newer {
        key: &'static str,
        found: u32,
        current: u32,
    },
}

type TomlStep = fn(toml::Value) -> toml::Value;
type JsonStep = fn(serde_json::Value) -> serde_json::Value;

/// Entry `i` upgrades version `i + 1` to `i + 2`. Presets: v1 is current.
const PRESET_STEPS: &[TomlStep] = &[];
/// Configs: v1 is current.
const CONFIG_STEPS: &[TomlStep] = &[];
/// Manifest records: v1 → v2.
const MANIFEST_STEPS: &[JsonStep] = &[manifest_v1_to_v2];

// Every table reaches its family's current version.
const _: () = assert!(PRESET_STEPS.len() as u32 == PRESET_SCHEMA - 1);
const _: () = assert!(CONFIG_STEPS.len() as u32 == CONFIG_SCHEMA - 1);
const _: () = assert!(MANIFEST_STEPS.len() as u32 == MANIFEST_SCHEMA - 1);

/// v1 → v2: the reader already tolerates every field v2 added (`ManifestEntry::from_value` reads leniently), so
/// only the version changes.
fn manifest_v1_to_v2(mut v: serde_json::Value) -> serde_json::Value {
    v["schema"] = 2.into();
    v
}

fn run<V>(
    mut v: V,
    from: u32,
    current: u32,
    key: &'static str,
    steps: &[fn(V) -> V],
) -> Result<V, MigrateError> {
    if from == 0 {
        return Err(MigrateError::NoVersion(key));
    }
    if from > current {
        return Err(MigrateError::Newer {
            key,
            found: from,
            current,
        });
    }
    for step in &steps[(from - 1) as usize..(current - 1) as usize] {
        v = step(v);
    }
    Ok(v)
}

fn toml_version(
    v: &toml::Value,
    key: &'static str,
    absent: Option<u32>,
) -> Result<u32, MigrateError> {
    match v.get(key) {
        None => absent.ok_or(MigrateError::NoVersion(key)),
        Some(x) => x
            .as_integer()
            .and_then(|i| u32::try_from(i).ok())
            .ok_or(MigrateError::NoVersion(key)),
    }
}

/// A preset file's table; `schema_version` is required.
pub fn migrate_preset(v: toml::Value) -> Result<toml::Value, MigrateError> {
    let from = toml_version(&v, "schema_version", None)?;
    run(v, from, PRESET_SCHEMA, "schema_version", PRESET_STEPS)
}

/// A config file's table; an absent `schema_version` is 1.
pub fn migrate_config(v: toml::Value) -> Result<toml::Value, MigrateError> {
    let from = toml_version(&v, "schema_version", Some(1))?;
    run(v, from, CONFIG_SCHEMA, "schema_version", CONFIG_STEPS)
}

/// One manifest line; its version key is `schema`. A newer record is refused here and kept raw by the caller
/// (`ManifestEntry::is_newer_schema`).
pub fn migrate_manifest_record(v: serde_json::Value) -> Result<serde_json::Value, MigrateError> {
    let from = v
        .get("schema")
        .and_then(serde_json::Value::as_u64)
        .map(|s| u32::try_from(s).unwrap_or(u32::MAX))
        .ok_or(MigrateError::NoVersion("schema"))?;
    run(v, from, MANIFEST_SCHEMA, "schema", MANIFEST_STEPS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn t(s: &str) -> toml::Value {
        toml::Value::Table(toml::from_str(s).unwrap())
    }

    #[test]
    fn presets_need_a_version_and_refuse_a_newer_one() {
        let v1 = t("schema_version = 1\nid = \"x\"");
        assert_eq!(migrate_preset(v1.clone()), Ok(v1));
        assert_eq!(
            migrate_preset(t("id = \"x\"")),
            Err(MigrateError::NoVersion("schema_version"))
        );
        assert_eq!(
            migrate_preset(t("schema_version = 0")),
            Err(MigrateError::NoVersion("schema_version"))
        );
        assert_eq!(
            migrate_preset(t("schema_version = \"1\"")),
            Err(MigrateError::NoVersion("schema_version"))
        );
        assert_eq!(
            migrate_preset(t("schema_version = 2")),
            Err(MigrateError::Newer {
                key: "schema_version",
                found: 2,
                current: 1
            })
        );
    }

    #[test]
    fn configs_default_to_version_1() {
        let none = t("preset = \"cs-quality\"");
        assert_eq!(migrate_config(none.clone()), Ok(none));
        let v1 = t("schema_version = 1");
        assert_eq!(migrate_config(v1.clone()), Ok(v1));
        assert_eq!(
            migrate_config(t("schema_version = 7")),
            Err(MigrateError::Newer {
                key: "schema_version",
                found: 7,
                current: CONFIG_SCHEMA
            })
        );
    }

    #[test]
    fn manifest_records_step_from_1_to_2_and_newer_ones_are_refused() {
        assert_eq!(
            migrate_manifest_record(json!({"schema": 1, "output": "a.dds"})),
            Ok(json!({"schema": 2, "output": "a.dds"}))
        );
        let v2 = json!({"schema": 2, "output": "a.dds"});
        assert_eq!(migrate_manifest_record(v2.clone()), Ok(v2));
        assert!(matches!(
            migrate_manifest_record(json!({"schema": 3})),
            Err(MigrateError::Newer {
                found: 3,
                current: 2,
                ..
            })
        ));
        assert_eq!(
            migrate_manifest_record(json!({"output": "a.dds"})),
            Err(MigrateError::NoVersion("schema"))
        );
    }
}
