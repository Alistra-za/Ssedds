//! Presets (spec §3.5): versioned bundles of profile defaults plus ordered rules, with single inheritance.
use crate::codes::Code;
use crate::config::*;
use crate::group::SUFFIXES;
use crate::overrides::{Scope, SetOverride};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
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
    /// A configuration the registry refuses (an invalid or conflicting suffix alias, spec §3.7, or a user preset
    /// under the reserved id `sdc-base`); the message starts `CFG-INVALID:`.
    #[error("{0}")]
    Alias(String),
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
    #[schemars(schema_with = "crate::schemas::partial_profile_schema")]
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
    #[schemars(schema_with = "crate::schemas::partial_profile_schema")]
    pub defaults: Value,
    #[serde(default)]
    #[schemars(schema_with = "crate::schemas::subtypes_schema")]
    pub subtypes: BTreeMap<Subtype, Value>,
    #[serde(default)]
    pub rules: Vec<Rule>,
    /// Exact suffix aliases (spec §3.7, DECISIONS Note G): `alias = "canonical"`, e.g. `rmaosultrahigh = "rmaos"`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub suffix_aliases: BTreeMap<String, String>,
}

/// User config (%APPDATA%\sdc\config.toml) and project config (sdc.toml / --config).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigFile {
    #[serde(default)]
    pub preset: Option<String>,
    #[serde(default)]
    pub target: Option<Target>,
    #[schemars(schema_with = "crate::schemas::partial_profile_schema")]
    #[serde(default)]
    pub defaults: Value,
    #[schemars(schema_with = "crate::schemas::subtypes_schema")]
    #[serde(default)]
    pub subtypes: BTreeMap<Subtype, Value>,
    #[serde(default)]
    pub rules: Vec<Rule>,
    /// Per-file edits (DECISIONS N2): key relative to the folder holding this `sdc.toml` → partial profile, plus
    /// `allow = [codes]`. Only a project config may hold it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[schemars(schema_with = "crate::schemas::files_table_schema")]
    pub files: BTreeMap<String, Value>,
}

/// What `ConfigFile::file_entries` returns: the `[files]` overrides and the per-file `allow` codes by key.
pub type FileEntries = (Vec<SetOverride>, BTreeMap<String, BTreeSet<Code>>);

impl ConfigFile {
    pub fn load(path: &Path) -> Result<ConfigFile, PresetError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| PresetError::Parse(format!("{}: {e}", path.display())))?;
        toml::from_str(&text).map_err(|e| PresetError::Parse(format!("{}: {e}", path.display())))
    }

    /// The `[files]` table as `Scope::Path` overrides (key order, then leaf order) and per-file `allow` codes.
    pub fn file_entries(&self) -> Result<FileEntries, PresetError> {
        let mut overrides = vec![];
        let mut allow = BTreeMap::new();
        for (key, entry) in &self.files {
            let bad = |m: String| PresetError::Parse(format!("[files.\"{key}\"]: {m}"));
            let mut entry = entry.clone();
            let obj = entry
                .as_object_mut()
                .ok_or_else(|| bad("expected a table".into()))?;
            if let Some(a) = obj.remove("allow") {
                let list = a
                    .as_array()
                    .ok_or_else(|| bad("allow must be a list of codes".into()))?;
                let mut codes = BTreeSet::new();
                for c in list {
                    let s = c.as_str().unwrap_or_default();
                    codes.insert(Code::parse(s).ok_or_else(|| bad(format!("unknown code '{s}'")))?);
                }
                allow.insert(key.clone(), codes);
            }
            let mut paths = vec![];
            leaf_paths(&entry, "", &mut paths);
            for field in paths {
                if !crate::overrides::FIELD_PATHS.contains(&field.as_str()) {
                    return Err(bad(format!("unknown field '{field}'")));
                }
                let value = field
                    .split('.')
                    .try_fold(&entry, |v, part| v.get(part))
                    .cloned()
                    .unwrap_or(Value::Null);
                overrides.push(SetOverride {
                    scope: Scope::Path(key.clone()),
                    field,
                    value,
                });
            }
        }
        Ok((overrides, allow))
    }
}

/// Where a resolved field value came from (spec §5.2). Declaration order is precedence order, lowest first.
///
/// Serialises as a string: `builtin`, `preset:<id>`, `rule:<source>#<n>` (the `Rule` payload is the
/// `<source>#<n>` part), `analysis:<fact>`, `suffix:<term>`, `source:<file>`, `default:<value>`, `fill:<code>`,
/// `recipe:<kind>`, and the snake_case variant name for the rest.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, JsonSchema)]
#[schemars(with = "String")]
pub enum Provenance {
    Builtin,
    Preset(String),
    UserConfig,
    ProjectConfig,
    Rule(String),
    Analysis,
    /// A named fact derived from the source's analysis, applied at the analysis position. Shown as
    /// `analysis:<fact>`, e.g. `analysis:float-source`.
    AnalysisFact(String),
    /// A fact stated by an exporter vocabulary term, e.g. `_normalgl` (spec §4.4); applied at the analysis
    /// position. Shown as `suffix:<term>`.
    Suffix(String),
    Suggestion,
    /// A value from the project config's `[files]` table (DECISIONS N2): applied after analysis facts, before the
    /// command line. Shown as `project_file`.
    ProjectFile,
    User,
    /// The sub-type was replaced by its spec §4.2 fallback, accepted with `--allow CFG-UNAVAILABLE`.
    Fallback,
    /// A packed channel read from a raw source (spec §6.4, DECISIONS Note M12): `source:<file>:R`,
    /// `source:<file>:luma`, `source:1-<file>:R` when stored inverted.
    Source(String),
    /// A packed channel without a source, at its neutral default: `default:<value>`.
    Default(String),
    /// A packed channel without a source, at the user's fill value: `fill:<code written>`.
    Fill(String),
    /// The sub-type of a packed output, set by its recipe: `recipe:<kind>`.
    Recipe(String),
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
            Provenance::AnalysisFact(t) => write!(f, "analysis:{t}"),
            Provenance::Suffix(t) => write!(f, "suffix:{t}"),
            Provenance::Suggestion => f.write_str("suggestion"),
            Provenance::ProjectFile => f.write_str("project_file"),
            Provenance::User => f.write_str("user"),
            Provenance::Fallback => f.write_str("fallback"),
            Provenance::Source(s) => write!(f, "source:{s}"),
            Provenance::Default(v) => write!(f, "default:{v}"),
            Provenance::Fill(v) => write!(f, "fill:{v}"),
            Provenance::Recipe(k) => write!(f, "recipe:{k}"),
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
        if let Some(t) = s.strip_prefix("analysis:") {
            return Ok(Provenance::AnalysisFact(t.to_string()));
        }
        if let Some(t) = s.strip_prefix("suffix:") {
            return Ok(Provenance::Suffix(t.to_string()));
        }
        if let Some(s) = s.strip_prefix("source:") {
            return Ok(Provenance::Source(s.to_string()));
        }
        if let Some(v) = s.strip_prefix("default:") {
            return Ok(Provenance::Default(v.to_string()));
        }
        if let Some(v) = s.strip_prefix("fill:") {
            return Ok(Provenance::Fill(v.to_string()));
        }
        if let Some(k) = s.strip_prefix("recipe:") {
            return Ok(Provenance::Recipe(k.to_string()));
        }
        Ok(match s {
            "builtin" => Provenance::Builtin,
            "user_config" => Provenance::UserConfig,
            "project_config" => Provenance::ProjectConfig,
            "analysis" => Provenance::Analysis,
            "suggestion" => Provenance::Suggestion,
            "project_file" => Provenance::ProjectFile,
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
    /// Alias → canonical suffix, over every loaded preset.
    aliases: BTreeMap<String, String>,
    /// Alias → id of the preset that declares it.
    alias_owner: BTreeMap<String, String>,
}

impl PresetRegistry {
    pub fn builtin() -> Result<Self, PresetError> {
        let mut r = PresetRegistry::default();
        for text in BUILTIN {
            r.insert_toml(text, true)?;
        }
        Ok(r)
    }

    /// Load a user preset. The internal base id `sdc-base` is reserved: it can be neither replaced nor extended
    /// (`CFG-INVALID`), so the error path of `policy::resolve_file` can always fall back on the built-in base.
    pub fn add_toml(&mut self, text: &str) -> Result<String, PresetError> {
        self.insert_toml(text, false)
    }

    fn insert_toml(&mut self, text: &str, builtin: bool) -> Result<String, PresetError> {
        let p: PresetFile = toml::from_str(text).map_err(|e| PresetError::Parse(e.to_string()))?;
        if !builtin && p.id == BASE_ID {
            return Err(PresetError::Alias(format!(
                "CFG-INVALID: preset id '{BASE_ID}' is reserved for the built-in base profile"
            )));
        }
        if p.schema_version > SCHEMA_VERSION {
            return Err(PresetError::NewerSchema(p.schema_version));
        }
        self.check_aliases(&p)?;
        let id = p.id.clone();
        // Reloading a preset id replaces its aliases.
        let owner = &self.alias_owner;
        self.aliases.retain(|a, _| owner.get(a) != Some(&id));
        self.alias_owner.retain(|_, o| *o != id);
        for (alias, canonical) in &p.suffix_aliases {
            self.aliases.insert(alias.clone(), canonical.clone());
            self.alias_owner.insert(alias.clone(), id.clone());
        }
        self.presets.insert(id.clone(), p);
        Ok(id)
    }

    /// Spec §3.7: an alias is `[a-z0-9]+`, is neither a §6.1 suffix nor an exporter vocabulary term (§4.4),
    /// maps to a §6.1 suffix and is unique across loaded presets.
    fn check_aliases(&self, p: &PresetFile) -> Result<(), PresetError> {
        let bad = |m: String| PresetError::Alias(format!("CFG-INVALID: preset '{}': {m}", p.id));
        for (alias, canonical) in &p.suffix_aliases {
            if alias.is_empty()
                || !alias
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            {
                return Err(bad(format!(
                    "suffix alias '{alias}' must be lower-case ASCII letters and digits, without '_'"
                )));
            }
            if SUFFIXES.contains(&alias.as_str()) {
                return Err(bad(format!(
                    "suffix alias '{alias}' is a canonical suffix (spec §6.1)"
                )));
            }
            if crate::group::is_vocabulary(alias) {
                return Err(bad(format!(
                    "suffix alias '{alias}' is an exporter vocabulary term (spec §4.4)"
                )));
            }
            if !SUFFIXES.contains(&canonical.as_str()) {
                return Err(bad(format!(
                    "suffix alias '{alias}' maps to '{canonical}', which is not a canonical suffix (spec §6.1)"
                )));
            }
            if let Some(owner) = self.alias_owner.get(alias).filter(|o| **o != p.id) {
                return Err(bad(format!(
                    "suffix alias '{alias}' is already declared by preset '{owner}'"
                )));
            }
        }
        Ok(())
    }

    /// `(canonical suffix, declaring preset id)` for an alias, case-insensitive.
    pub fn alias_for(&self, alias: &str) -> Option<(&str, &str)> {
        let a = alias.to_ascii_lowercase();
        Some((
            self.aliases.get(&a)?.as_str(),
            self.alias_owner.get(&a)?.as_str(),
        ))
    }

    /// Alias → canonical suffix over every loaded preset (input to `group::split_suffix_with`,
    /// `discover::canonicalise_outputs` and `group::group_sets_with`).
    pub fn alias_table(&self) -> &BTreeMap<String, String> {
        &self.aliases
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

    /// The chain that resolves a file matched by one of `id`'s suffix aliases (spec §3.7): `id`'s own chain, or,
    /// when `id` declares no `base`, the built-in quality preset chain of `target` followed by `id`, so the alias
    /// preset layers on top of the target preset.
    pub fn alias_chain(&self, id: &str, target: Target) -> Result<Vec<&PresetFile>, PresetError> {
        let p = self
            .presets
            .get(id)
            .ok_or_else(|| PresetError::Unknown(id.to_string()))?;
        if p.base.is_some() {
            return self.chain(id);
        }
        let mut chain = self.chain(default_preset_for(target))?;
        if !chain.iter().any(|c| c.id == p.id) {
            chain.push(p);
        }
        Ok(chain)
    }

    /// The `sdc-base` profile of `subtype`, for error paths that need a placeholder profile. Read from this registry,
    /// where `sdc-base` is the built-in base (user presets cannot replace it); a registry without one (e.g.
    /// `PresetRegistry::default()`) falls back on the embedded built-in base. Never fails.
    pub fn base_profile(&self, target: Target, subtype: Subtype) -> Profile {
        static EMBEDDED: std::sync::OnceLock<PresetRegistry> = std::sync::OnceLock::new();
        self.profile(BASE_ID, target, subtype).unwrap_or_else(|_| {
            EMBEDDED
                .get_or_init(|| {
                    PresetRegistry::builtin()
                        .expect("embedded presets are valid (tests/presets.rs)")
                })
                .profile(BASE_ID, target, subtype)
                .expect("embedded sdc-base is valid for every sub-type (tests/presets.rs)")
        })
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
        Ok(Self::layers_of(&self.chain(id)?, subtype))
    }

    /// `layers` over an explicit chain (root base first), e.g. one from `alias_chain`.
    pub fn layers_of(chain: &[&PresetFile], subtype: Subtype) -> Vec<(Provenance, Value)> {
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
        defaults.chain(specific).collect()
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
