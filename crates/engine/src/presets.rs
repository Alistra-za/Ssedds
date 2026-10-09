//! Presets (spec §3.5): versioned bundles of profile defaults plus ordered rules, with single inheritance.
use crate::config::*;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

pub const SCHEMA_VERSION: u32 = 1;
pub const BASE_ID: &str = "sdc-base";
pub const DEFAULT_PRESET: &str = "cs-quality";

pub fn default_preset_for(t: Target) -> &'static str {
    match t {
        Target::Cs => "cs-quality",
        Target::Cpm => "cpm-quality",
        Target::Vanilla => "vanilla-quality",
    }
}

const BUILTIN: [&str; 4] = [
    include_str!("../../../presets/sdc-base.toml"),
    include_str!("../../../presets/cs-quality.toml"),
    include_str!("../../../presets/cpm-quality.toml"),
    include_str!("../../../presets/vanilla-quality.toml"),
];

#[derive(Debug, Clone, thiserror::Error, PartialEq)]
pub enum PresetError {
    #[error("preset parse error: {0}")]
    Parse(String),
    #[error("unknown preset '{0}'")]
    Unknown(String),
    #[error("preset inheritance cycle at '{0}'")]
    Cycle(String),
    #[error("preset schema_version {0} is newer than this sdc supports ({SCHEMA_VERSION})")]
    NewerSchema(u32),
    #[error("invalid profile: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AlphaContent {
    None,
    Constant255,
    Varying,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuleMatch {
    /// Glob over the game-relative path (lower case, '/'), e.g. "**/armor/**/*_n.png".
    #[serde(default)]
    pub glob: Option<String>,
    /// Recognised suffix without underscore, e.g. "n", "rmaos"; "" = no suffix.
    #[serde(default)]
    pub suffix: Option<String>,
    /// The file's texture set contains a member with this suffix.
    #[serde(default)]
    pub set_has: Option<String>,
    #[serde(default)]
    pub alpha: Option<AlphaContent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    #[serde(rename = "match")]
    pub matcher: RuleMatch,
    #[serde(default)]
    pub subtype: Option<Subtype>,
    #[serde(default)]
    pub overrides: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PresetFile {
    pub schema_version: u32,
    pub id: String,
    pub version: String,
    #[serde(default)]
    pub base: Option<String>,
    #[serde(default)]
    pub target: Option<Target>,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub defaults: Value,
    #[serde(default)]
    pub subtypes: BTreeMap<Subtype, Value>,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

/// User config (%APPDATA%\sdc\config.toml) and project config (sdc.toml / --config).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigFile {
    #[serde(default)]
    pub preset: Option<String>,
    #[serde(default)]
    pub target: Option<Target>,
    #[serde(default)]
    pub defaults: Value,
    #[serde(default)]
    pub subtypes: BTreeMap<Subtype, Value>,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

impl ConfigFile {
    pub fn load(path: &Path) -> Result<ConfigFile, PresetError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| PresetError::Parse(format!("{}: {e}", path.display())))?;
        toml::from_str(&text).map_err(|e| PresetError::Parse(format!("{}: {e}", path.display())))
    }
}

/// Where a resolved field value came from (spec §5.2). Declaration order is precedence order, lowest first.
///
/// Serialises as a string: `builtin`, `preset:<id>`, `rule:<source>#<n>` (the `Rule` payload is the
/// `<source>#<n>` part), and the snake_case variant name for the rest.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, JsonSchema)]
#[schemars(with = "String")]
pub enum Provenance {
    Builtin,
    Preset(String),
    UserConfig,
    ProjectConfig,
    Rule(String),
    Analysis,
    Suggestion,
    Set,
    User,
    /// The sub-type was replaced by its spec §4.2 fallback, accepted with `--allow CFG-UNAVAILABLE`.
    Fallback,
}

impl fmt::Display for Provenance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Provenance::Builtin => f.write_str("builtin"),
            Provenance::Preset(id) => write!(f, "preset:{id}"),
            Provenance::UserConfig => f.write_str("user_config"),
            Provenance::ProjectConfig => f.write_str("project_config"),
            Provenance::Rule(src) => write!(f, "rule:{src}"),
            Provenance::Analysis => f.write_str("analysis"),
            Provenance::Suggestion => f.write_str("suggestion"),
            Provenance::Set => f.write_str("set"),
            Provenance::User => f.write_str("user"),
            Provenance::Fallback => f.write_str("fallback"),
        }
    }
}

impl std::str::FromStr for Provenance {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        if let Some(id) = s.strip_prefix("preset:") {
            return Ok(Provenance::Preset(id.to_string()));
        }
        if let Some(src) = s.strip_prefix("rule:") {
            return Ok(Provenance::Rule(src.to_string()));
        }
        Ok(match s {
            "builtin" => Provenance::Builtin,
            "user_config" => Provenance::UserConfig,
            "project_config" => Provenance::ProjectConfig,
            "analysis" => Provenance::Analysis,
            "suggestion" => Provenance::Suggestion,
            "set" => Provenance::Set,
            "user" => Provenance::User,
            "fallback" => Provenance::Fallback,
            other => return Err(format!("unknown provenance '{other}'")),
        })
    }
}

impl Serialize for Provenance {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Provenance {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// Deep-merge `patch` into `base`: objects merge key by key, other values replace, `null` patches change nothing.
pub fn merge_json(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(b), Value::Object(p)) => {
            for (k, v) in p {
                merge_json(b.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
        (_, Value::Null) => {}
        (b, p) => *b = p.clone(),
    }
}

/// Dotted paths of every non-null leaf of `v`.
pub fn leaf_paths(v: &Value, prefix: &str, out: &mut Vec<String>) {
    match v {
        Value::Object(m) => {
            for (k, child) in m {
                let p = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                leaf_paths(child, &p, out);
            }
        }
        Value::Null => {}
        _ => out.push(prefix.to_string()),
    }
}

#[derive(Debug, Clone, Default)]
pub struct PresetRegistry {
    presets: BTreeMap<String, PresetFile>,
}

impl PresetRegistry {
    pub fn builtin() -> Result<Self, PresetError> {
        let mut r = PresetRegistry::default();
        for text in BUILTIN {
            r.add_toml(text)?;
        }
        Ok(r)
    }

    pub fn add_toml(&mut self, text: &str) -> Result<String, PresetError> {
        let p: PresetFile = toml::from_str(text).map_err(|e| PresetError::Parse(e.to_string()))?;
        if p.schema_version > SCHEMA_VERSION {
            return Err(PresetError::NewerSchema(p.schema_version));
        }
        let id = p.id.clone();
        self.presets.insert(id.clone(), p);
        Ok(id)
    }

    pub fn add_file(&mut self, path: &Path) -> Result<String, PresetError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| PresetError::Parse(format!("{}: {e}", path.display())))?;
        self.add_toml(&text)
    }

    pub fn get(&self, id: &str) -> Option<&PresetFile> {
        self.presets.get(id)
    }

    /// True when `id` is loaded and identical to the built-in preset of that id (a user preset loaded under a
    /// built-in id is not built in).
    pub fn is_builtin(&self, id: &str) -> bool {
        static BUILTINS: std::sync::OnceLock<BTreeMap<String, PresetFile>> =
            std::sync::OnceLock::new();
        let builtins = BUILTINS.get_or_init(|| {
            BUILTIN
                .iter()
                .filter_map(|t| toml::from_str::<PresetFile>(t).ok())
                .map(|p| (p.id.clone(), p))
                .collect()
        });
        matches!((self.presets.get(id), builtins.get(id)), (Some(a), Some(b)) if a == b)
    }

    pub fn selectable(&self) -> Vec<&PresetFile> {
        self.presets.values().filter(|p| !p.hidden).collect()
    }

    /// The inheritance chain, root base first.
    pub fn chain(&self, id: &str) -> Result<Vec<&PresetFile>, PresetError> {
        let mut out: Vec<&PresetFile> = vec![];
        let mut cur = Some(id.to_string());
        while let Some(c) = cur {
            if out.iter().any(|p| p.id == c) {
                return Err(PresetError::Cycle(c));
            }
            let p = self
                .presets
                .get(&c)
                .ok_or_else(|| PresetError::Unknown(c.clone()))?;
            out.push(p);
            cur = p.base.clone();
        }
        out.reverse();
        Ok(out)
    }

    /// SHA-256 of the preset's canonical JSON form (manifest provenance, spec §3.6).
    pub fn sha256(&self, id: &str) -> Option<String> {
        let p = self.presets.get(id)?;
        let canon = serde_json::to_vec(&serde_json::to_value(p).ok()?).ok()?;
        Some(hex::encode(Sha256::digest(canon)))
    }

    /// Patches in precedence order, lowest first: every `defaults` table (base to child), then every
    /// `subtypes.<st>` table (base to child). A sub-type-specific setting at any level therefore beats a
    /// generic default at any level. The hidden base preset is labelled `Builtin`.
    pub fn layers(
        &self,
        id: &str,
        subtype: Subtype,
    ) -> Result<Vec<(Provenance, Value)>, PresetError> {
        let chain = self.chain(id)?;
        let label = |p: &PresetFile| {
            if p.id == BASE_ID {
                Provenance::Builtin
            } else {
                Provenance::Preset(p.id.clone())
            }
        };
        let defaults = chain
            .iter()
            .filter(|p| !p.defaults.is_null())
            .map(|p| (label(p), p.defaults.clone()));
        let specific = chain
            .iter()
            .filter_map(|p| p.subtypes.get(&subtype).map(|s| (label(p), s.clone())));
        Ok(defaults.chain(specific).collect())
    }

    /// Profile from presets alone (no config, rules, analysis or overrides). Used by tests and `presets show`.
    pub fn profile(
        &self,
        id: &str,
        target: Target,
        subtype: Subtype,
    ) -> Result<Profile, PresetError> {
        let mut doc = Value::Object(Map::new());
        for (_, patch) in self.layers(id, subtype)? {
            merge_json(&mut doc, &patch);
        }
        finish_profile(doc, target, subtype)
    }
}

/// Insert identity fields and deserialise; errors name the offending field.
pub fn finish_profile(
    mut doc: Value,
    target: Target,
    subtype: Subtype,
) -> Result<Profile, PresetError> {
    merge_json(
        &mut doc,
        &serde_json::json!({ "target": target, "subtype": subtype, "category": subtype.category() }),
    );
    serde_json::from_value(doc).map_err(|e| PresetError::Invalid(e.to_string()))
}
