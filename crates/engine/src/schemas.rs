//! Published JSON Schemas (spec §2 `schema/`, §3): generated from the Rust types so they cannot drift.
//!
//! The committed files in `schema/` are exactly [`schema_json`]'s output; `tests/schemas.rs` regenerates them
//! and compares byte for byte, so a type change without a schema update fails CI. Output is deterministic
//! (object keys sorted, LF line endings, one trailing newline).
use crate::config::Profile;
use crate::events::EngineEvent;
use crate::manifest::ManifestRecord;
use crate::presets::{ConfigFile, PresetFile};
use crate::reports::{DdsInfo, DdsValidation};
use serde_json::{Map, Value};
use std::fmt;
use std::path::Path;
use std::str::FromStr;

/// One published schema, or `All` of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaKind {
    All,
    Profile,
    Preset,
    Config,
    /// One line of `--json` output of `convert` (and the plan events of `inspect`).
    Event,
    /// One line of `<output_root>/_sdc/manifest.jsonl`.
    Manifest,
    /// One `dds_info` object of `sdc inspect --json`.
    DdsInfo,
    /// One `dds_validation` object of `sdc validate --json`.
    DdsValidation,
}

impl SchemaKind {
    /// Every kind that has a schema file (everything but `All`), in publication order.
    pub const FILES: [SchemaKind; 7] = [
        SchemaKind::Profile,
        SchemaKind::Preset,
        SchemaKind::Config,
        SchemaKind::Event,
        SchemaKind::Manifest,
        SchemaKind::DdsInfo,
        SchemaKind::DdsValidation,
    ];

    /// The name used on the command line and in the file name (`<name>.schema.json`).
    pub fn name(self) -> &'static str {
        match self {
            SchemaKind::All => "all",
            SchemaKind::Profile => "profile",
            SchemaKind::Preset => "preset",
            SchemaKind::Config => "config",
            SchemaKind::Event => "event",
            SchemaKind::Manifest => "manifest",
            SchemaKind::DdsInfo => "dds-info",
            SchemaKind::DdsValidation => "dds-validation",
        }
    }

    /// `<name>.schema.json`.
    pub fn file_name(self) -> String {
        format!("{}.schema.json", self.name())
    }
}

impl fmt::Display for SchemaKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for SchemaKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        std::iter::once(SchemaKind::All)
            .chain(SchemaKind::FILES)
            .find(|k| k.name() == s)
            .ok_or_else(|| format!("unknown schema '{s}'"))
    }
}

/// `v` with every object's keys in sorted order, whatever order or map type the generator used.
fn sorted(v: Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut entries: Vec<(String, Value)> =
                m.into_iter().map(|(k, v)| (k, sorted(v))).collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(entries.into_iter().collect::<Map<_, _>>())
        }
        Value::Array(a) => Value::Array(a.into_iter().map(sorted).collect()),
        other => other,
    }
}

fn schema_value(kind: SchemaKind) -> Value {
    let schema = match kind {
        SchemaKind::Profile => serde_json::to_value(schemars::schema_for!(Profile)),
        SchemaKind::Preset => serde_json::to_value(schemars::schema_for!(PresetFile)),
        SchemaKind::Config => serde_json::to_value(schemars::schema_for!(ConfigFile)),
        SchemaKind::Event => serde_json::to_value(schemars::schema_for!(EngineEvent)),
        SchemaKind::Manifest => serde_json::to_value(schemars::schema_for!(ManifestRecord)),
        SchemaKind::DdsInfo => serde_json::to_value(schemars::schema_for!(DdsInfo)),
        SchemaKind::DdsValidation => serde_json::to_value(schemars::schema_for!(DdsValidation)),
        SchemaKind::All => {
            return Value::Object(
                SchemaKind::FILES
                    .iter()
                    .map(|k| (k.name().to_string(), schema_value(*k)))
                    .collect(),
            )
        }
    };
    sorted(schema.expect("a schema is plain JSON"))
}

/// The schema as pretty-printed JSON with a trailing newline. `All` gives one object holding every schema by
/// name; the published files are the individual kinds.
pub fn schema_json(kind: SchemaKind) -> String {
    let mut text = serde_json::to_string_pretty(&schema_value(kind)).expect("a schema serialises");
    text.push('\n');
    text
}

/// Writes `<name>.schema.json` for every kind into `dir` (created if missing).
pub fn write_all(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for kind in SchemaKind::FILES {
        std::fs::write(dir.join(kind.file_name()), schema_json(kind))?;
    }
    Ok(())
}

/// The `#/$defs/` prefix of the generator's references.
fn defs_prefix(gen: &schemars::SchemaGenerator) -> String {
    let path = &gen.settings().definitions_path;
    format!("#{}/", path.trim_end_matches('/'))
}

/// `Partial<name>`: a copy of definition `name` with every `required` list removed, and every object definition it
/// refers to likewise replaced by its `Partial` copy. A definition without `properties` (an enum) is shared, not
/// copied. Returns the name to refer to.
fn partial_def(defs: &mut Map<String, Value>, prefix: &str, name: &str) -> String {
    let partial = format!("Partial{name}");
    if defs.contains_key(&partial) {
        return partial;
    }
    let Some(mut def) = defs.get(name).cloned() else {
        return name.to_string();
    };
    if def.get("properties").is_none() {
        return name.to_string();
    }
    // Reserve the name first so a self-referential definition terminates.
    defs.insert(partial.clone(), Value::Null);
    fn walk(v: &mut Value, defs: &mut Map<String, Value>, prefix: &str) {
        match v {
            Value::Object(m) => {
                if m.get("required").is_some_and(Value::is_array) {
                    m.remove("required");
                }
                let target = m
                    .get("$ref")
                    .and_then(Value::as_str)
                    .and_then(|r| r.strip_prefix(prefix))
                    .map(str::to_string);
                if let Some(target) = target {
                    let new = partial_def(defs, prefix, &target);
                    m.insert("$ref".into(), Value::String(format!("{prefix}{new}")));
                }
                m.values_mut().for_each(|c| walk(c, defs, prefix));
            }
            Value::Array(a) => a.iter_mut().for_each(|c| walk(c, defs, prefix)),
            _ => {}
        }
    }
    walk(&mut def, defs, prefix);
    defs.insert(partial.clone(), def);
    partial
}

/// A partial `Profile`: the `Profile` schema with every `required` list removed, in the definitions it refers to
/// too (plan M3 carried "partial-Profile schema"). Returned as a `$ref` to the `PartialProfile` definition.
pub fn partial_profile_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
    let prefix = defs_prefix(gen);
    // Registers `Profile` and what it refers to in the generator's definitions.
    gen.subschema_for::<Profile>();
    let mut defs = gen.definitions().clone();
    let name = partial_def(&mut defs, &prefix, "Profile");
    *gen.definitions_mut() = defs;
    schemars::json_schema!({ "$ref": format!("{prefix}{name}") })
}

/// `subtypes`: sub-type id → partial profile.
pub fn subtypes_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
    let partial = partial_profile_schema(gen);
    let mut s = gen.subschema_for::<std::collections::BTreeMap<crate::config::Subtype, Value>>();
    if let Some(Value::Object(props)) = s.ensure_object().get_mut("properties") {
        props
            .values_mut()
            .for_each(|p| *p = partial.as_value().clone());
        // The pre-rename id still loads (DECISIONS Note R-skin-1); editors should strike it through.
        if let Some(old) = props.get_mut("rfaos") {
            *old = serde_json::json!({
                "allOf": [partial.as_value().clone()],
                "deprecated": true,
                "description": "Deprecated alias of skin_rfaos: it still loads and is written back as skin_rfaos."
            });
        }
    }
    s
}

/// `[files]`: key → partial profile plus `allow` (a list of codes). The entry is its own definition because a
/// profile rejects unknown fields; the identity fields `target` and `category` are not settable per file.
pub fn files_table_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
    let prefix = defs_prefix(gen);
    partial_profile_schema(gen);
    let mut entry = gen
        .definitions()
        .get("PartialProfile")
        .cloned()
        .unwrap_or_default();
    if let Some(Value::Object(props)) = entry.get_mut("properties") {
        props.remove("target");
        props.remove("category");
        props.insert(
            "allow".into(),
            serde_json::json!({
                "description": "Codes acknowledged for this file only, e.g. [\"DIM-NPOT\"].",
                "type": "array",
                "items": { "type": "string" }
            }),
        );
        // Audit #17 (Note BC): the per-file selectors the loader accepts (`overrides::is_selector`).
        props.insert(
            crate::overrides::REENCODE_FIELD.into(),
            serde_json::json!({
                "description": "Re-encode this DDS input (DECISIONS S6).",
                "type": "boolean"
            }),
        );
        let target = gen.subschema_for::<crate::config::RnmTarget>();
        let mut rnm = gen
            .definitions()
            .get("PartialRnmSpec")
            .cloned()
            .unwrap_or_default();
        if let Some(Value::Object(rp)) = rnm.get_mut("properties") {
            rp.insert("target".into(), target.to_value());
        }
        gen.definitions_mut().insert("FileRnmSpec".into(), rnm);
        if let Some(Value::Object(r)) = props.get_mut("rnm") {
            r.insert("$ref".into(), format!("{prefix}FileRnmSpec").into());
        }
        // Note DT: the helper-texture selectors.
        let roles: Vec<String> = crate::helper::HelperRole::all()
            .iter()
            .map(|r| r.id())
            .collect();
        let targets: Vec<String> = crate::helper::FillTarget::all()
            .iter()
            .map(|t| t.id())
            .collect();
        let read = gen.subschema_for::<crate::helper::HelperRead>();
        props.insert(
            "helper".into(),
            serde_json::json!({
                "description": "Helper texture: a grey map that fills a channel of another output of its set (Note DT).",
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "role": {
                        "description": "What the file is as a helper; `none` converts it on its own.",
                        "type": "string",
                        "enum": roles
                    },
                    "fills": {
                        "description": "The channels it fills; [] = not used.",
                        "type": "array",
                        "items": { "type": "string", "enum": targets }
                    },
                    "read": read.to_value(),
                    "invert": {
                        "description": "Store 1 - x.",
                        "type": "boolean"
                    }
                }
            }),
        );
    }
    gen.definitions_mut().insert("FileEntry".into(), entry);
    schemars::json_schema!({
        "type": "object",
        "additionalProperties": { "$ref": format!("{prefix}FileEntry") }
    })
}
