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
