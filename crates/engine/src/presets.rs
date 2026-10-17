//! Presets (spec §3.5): versioned bundles of profile defaults plus ordered rules, with single inheritance.
use crate::codes::{Code, Warning};
use crate::config::*;
use crate::group::SUFFIXES;
use crate::overrides::{Scope, SetOverride};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

pub const SCHEMA_VERSION: u32 = 1;
pub const BASE_ID: &str = "sdc-base";
pub const DEFAULT_PRESET: &str = "cs-quality";
/// [`PresetRegistry::source`] of an embedded preset.
pub const SOURCE_BUILTIN: &str = "builtin";
/// [`PresetRegistry::source`] of a preset added as text ([`PresetRegistry::add_toml`]).
pub const SOURCE_INLINE: &str = "inline";

pub fn default_preset_for(t: Target) -> &'static str {
    match t {
        Target::Cs => "cs-quality",
        Target::Vanilla => "vanilla-quality",
    }
}

const BUILTIN: [&str; 6] = [
    include_str!("../../../presets/sdc-base.toml"),
    include_str!("../../../presets/cs-quality.toml"),
    include_str!("../../../presets/vanilla-quality.toml"),
    include_str!("../../../presets/cs-compact.toml"),
    include_str!("../../../presets/vanilla-compact.toml"),
    include_str!("../../../presets/fast-preview.toml"),
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
    /// Spec §5 (`ConfigError::NewerSchema`, Note T9): a config file of a newer schema; names the file.
    #[error(
        "{0}: config schema_version {1} is newer than this sdc supports ({cur})",
        cur = crate::migrate::CONFIG_SCHEMA
    )]
    NewerConfigSchema(String, u32),
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
    /// The source's alpha content; `none` also matches varying alpha that mesh evidence decided to remove (M6 Track E,
    /// owner O1).
    #[serde(default)]
    pub alpha: Option<AlphaContent>,
    /// M6 Track D (DECISIONS Note T2): the sub-type being resolved is one of these; empty = any. Matched only when the
    /// profile is layered, so a rule that names sub-types here never decides a sub-type.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subtypes: Vec<Subtype>,
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
    /// Schema of this file (spec §5); absent = 1. Not written back when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub schema_version: Option<u32>,
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
    /// Input aliases (M6 Track F3b): key relative to the folder holding this `sdc.toml` → the path the file is read
    /// as. Only a project config may hold it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, ReadAs>,
}

/// One `[inputs."<real path>"]` entry: the file is grouped, named and written as if it were at `as`. The file on
/// disk is never touched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadAs {
    /// The path the file is read as, relative to the folder holding `sdc.toml` (same extension as the file; it may
    /// be left out).
    #[serde(rename = "as")]
    pub path: String,
}

/// What `ConfigFile::file_entries` returns: the `[files]` overrides and the per-file `allow` codes by key.
pub type FileEntries = (Vec<SetOverride>, BTreeMap<String, BTreeSet<Code>>);

impl ConfigFile {
    pub fn load(path: &Path) -> Result<ConfigFile, PresetError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| PresetError::Parse(format!("{}: {e}", path.display())))?;
        Self::parse(&text, &path.display().to_string())
    }

    /// A config file's text (`origin` names it in errors): BOM stripped, the migration hook (spec §5), then
    /// deserialised.
    pub fn parse(text: &str, origin: &str) -> Result<ConfigFile, PresetError> {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let bad = |m: String| PresetError::Parse(format!("{origin}: {m}"));
        let raw: toml::Table = toml::from_str(text).map_err(|e| bad(e.to_string()))?;
        let migrated =
            crate::migrate::migrate_config(toml::Value::Table(raw)).map_err(|e| match e {
                crate::migrate::MigrateError::Newer { found, .. } => {
                    PresetError::NewerConfigSchema(origin.to_string(), found)
                }
                e => bad(e.to_string()),
            })?;
        migrated
            .try_into()
            .map_err(|e: toml::de::Error| bad(e.to_string()))
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
            // `leaf_paths` skips nulls, which only a non-finite number makes (audit #8, Note BC).
            if crate::overrides::non_finite(&entry) {
                return Err(bad(
                    "not a finite number (inf, -inf and nan are refused)".into()
                ));
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
                if field == crate::overrides::REENCODE_FIELD && !value.is_boolean() {
                    return Err(bad("reencode takes true or false".into()));
                }
                if field == crate::overrides::RNM_TARGET_FIELD
                    && crate::overrides::rnm_target_value(&value).is_none()
                {
                    return Err(bad("rnm.target takes n, cnr or both".into()));
                }
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
/// `<source>#<n>` part), `analysis:<fact>`, `shader:<id>`, `mesh:alpha`, `suffix:<term>`, `source:<file>`,
/// `default:<value>`, `fill:<code>`, `recipe:<kind>`, and the snake_case variant name for the rest.
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
    /// A member default of the set's shader (M6 spec §5): above built-in profiles, below every explicit layer and
    /// analysis fact. Shown as `shader:<id>`.
    Shader(String),
    /// The alpha policy (and threshold) the meshes that use the colour member give it (M6 Track E, DECISIONS Note
    /// U7): above shader defaults, below every explicit layer and the analysis `remove`. Shown as `mesh:alpha`.
    MeshAlpha,
    /// A fact stated by an exporter vocabulary term, e.g. `_normalgl` (spec §4.4); applied at the analysis
    /// position. Shown as `suffix:<term>`.
    Suffix(String),
    Suggestion,
    /// A value from the project config's `[files]` table (DECISIONS N2): applied after analysis facts, before the
    /// command line. Shown as `project_file`.
    ProjectFile,
    User,
    /// A fallback value stood in: the sub-type replaced by its spec §4.2 fallback (accepted with `--allow CFG-UNAVAILABLE`),
    /// or box for an unscoped `mips.filter` the texture cannot use (CFG-FILTER-IGNORED).
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
    /// Decided by the texture set, not by a layer: the variance bake's `mips.normal = renormalize_bake` on a
    /// `pbr_normal` (spec §5.2, §6.2, DECISIONS R6). Shown as `set`.
    Set,
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
            Provenance::Shader(id) => write!(f, "shader:{id}"),
            Provenance::MeshAlpha => f.write_str("mesh:alpha"),
            Provenance::Suffix(t) => write!(f, "suffix:{t}"),
            Provenance::Suggestion => f.write_str("suggestion"),
            Provenance::ProjectFile => f.write_str("project_file"),
            Provenance::User => f.write_str("user"),
            Provenance::Fallback => f.write_str("fallback"),
            Provenance::Source(s) => write!(f, "source:{s}"),
            Provenance::Default(v) => write!(f, "default:{v}"),
            Provenance::Fill(v) => write!(f, "fill:{v}"),
            Provenance::Recipe(k) => write!(f, "recipe:{k}"),
            Provenance::Set => f.write_str("set"),
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
        if let Some(id) = s.strip_prefix("shader:") {
            return Ok(Provenance::Shader(id.to_string()));
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
            "mesh:alpha" => Provenance::MeshAlpha,
            "suggestion" => Provenance::Suggestion,
            "project_file" => Provenance::ProjectFile,
            "user" => Provenance::User,
            "set" => Provenance::Set,
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

/// Deep-merge `patch` into `base`: objects merge key by key, other values replace, and a `null` patch changes
/// nothing: it neither erases a value nor adds an absent key (carried fix 2, DECISIONS Note T10).
pub fn merge_json(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (_, Value::Null) => {}
        (Value::Object(b), Value::Object(p)) => {
            for (k, v) in p {
                match b.get_mut(k) {
                    Some(slot) => merge_json(slot, v),
                    None => {
                        if let Some(v) = without_nulls(v) {
                            b.insert(k.clone(), v);
                        }
                    }
                }
            }
        }
        (b, p) => {
            if let Some(v) = without_nulls(p) {
                *b = v;
            }
        }
    }
}

/// `v` without its `null` leaves; `None` for `null` itself.
fn without_nulls(v: &Value) -> Option<Value> {
    match v {
        Value::Null => None,
        Value::Object(m) => Some(Value::Object(
            m.iter()
                .filter_map(|(k, x)| Some((k.clone(), without_nulls(x)?)))
                .collect(),
        )),
        _ => Some(v.clone()),
    }
}

/// Audit EC-4 (Note BC): a rule's `overrides.subtype` is overwritten by the decided sub-type, so it would do nothing.
pub(crate) const SUBTYPE_IN_OVERRIDES: &str =
    "set the sub-type with `subtype =` on the rule, not in overrides";

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

/// Where a preset came from. The kind is kept apart from the label so a file named `builtin` or `inline` stays a file.
#[derive(Debug, Clone, PartialEq)]
enum Source {
    Builtin,
    Inline,
    /// The path as given (`Path::display`).
    File(String),
}

impl Source {
    fn label(&self) -> &str {
        match self {
            Source::Builtin => SOURCE_BUILTIN,
            Source::Inline => SOURCE_INLINE,
            Source::File(p) => p,
        }
    }
}

/// Whether a registry source is the file `path` (both canonicalised).
fn same_file(source: &Source, path: &Path) -> bool {
    matches!(source, Source::File(s) if matches!(
        (std::fs::canonicalize(s), std::fs::canonicalize(path)),
        (Ok(a), Ok(b)) if a == b
    ))
}

#[derive(Debug, Clone, Default)]
pub struct PresetRegistry {
    presets: BTreeMap<String, PresetFile>,
    /// Alias → canonical suffix, over every loaded preset.
    aliases: BTreeMap<String, String>,
    /// Alias → id of the preset that declares it.
    alias_owner: BTreeMap<String, String>,
    /// Id → where the preset was loaded from: `builtin`, `inline` or a file path (M6 Track D, Note T7).
    sources: BTreeMap<String, Source>,
    /// Id → the preset's TOML table as loaded (`presets export`, Note T8).
    tables: BTreeMap<String, toml::Table>,
}

impl PresetRegistry {
    pub fn builtin() -> Result<Self, PresetError> {
        let mut r = PresetRegistry::default();
        for text in BUILTIN {
            r.insert_toml(text, Source::Builtin)?;
        }
        Ok(r)
    }

    /// Load a user preset from text. The internal base id `sdc-base` is reserved: it can be neither replaced nor
    /// extended (`CFG-INVALID`), so the error path of `policy::resolve_file` can always fall back on the built-in base.
    /// An id that is already loaded is refused (Note T7).
    pub fn add_toml(&mut self, text: &str) -> Result<String, PresetError> {
        self.insert_toml(text, Source::Inline)
    }

    fn insert_toml(&mut self, text: &str, source: Source) -> Result<String, PresetError> {
        // Review Focus 1: a Windows editor's BOM (CRLF is valid TOML).
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let raw: toml::Table =
            toml::from_str(text).map_err(|e| PresetError::Parse(e.to_string()))?;
        // Spec §5: the migration hook runs before deserialising.
        let migrated =
            crate::migrate::migrate_preset(toml::Value::Table(raw)).map_err(|e| match e {
                crate::migrate::MigrateError::Newer { found, .. } => {
                    PresetError::NewerSchema(found)
                }
                e => PresetError::Parse(e.to_string()),
            })?;
        let toml::Value::Table(table) = migrated else {
            return Err(PresetError::Parse("a preset file is a TOML table".into()));
        };
        let p: PresetFile = toml::Value::Table(table.clone())
            .try_into()
            .map_err(|e: toml::de::Error| PresetError::Parse(e.to_string()))?;
        if source != Source::Builtin && p.id == BASE_ID {
            return Err(PresetError::Alias(format!(
                "CFG-INVALID: preset id '{BASE_ID}' from {} is reserved for the built-in base profile, already loaded from {SOURCE_BUILTIN}",
                source.label()
            )));
        }
        if let Some(first) = self.sources.get(&p.id) {
            return Err(PresetError::Alias(format!(
                "CFG-INVALID: preset id '{}' from {} is already loaded from {}",
                p.id,
                source.label(),
                first.label()
            )));
        }
        self.check_aliases(&p)?;
        let id = p.id.clone();
        for (alias, canonical) in &p.suffix_aliases {
            self.aliases.insert(alias.clone(), canonical.clone());
            self.alias_owner.insert(alias.clone(), id.clone());
        }
        self.sources.insert(id.clone(), source);
        self.tables.insert(id.clone(), table);
        self.presets.insert(id.clone(), p);
        Ok(id)
    }

    /// Spec §3.7: an alias is `[a-z0-9]+`, is neither a §6.1 suffix, an exporter vocabulary term (§4.4) nor an RNM layer term,
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
            if alias == "orm" {
                return Err(bad(
                    "suffix alias 'orm' is an exporter vocabulary term: orm is a built-in packing input since Note AL (AO, roughness, metallic); remove the alias".into(),
                ));
            }
            if crate::group::is_vocabulary(alias) {
                return Err(bad(format!(
                    "suffix alias '{alias}' is an exporter vocabulary term (spec §4.4)"
                )));
            }
            // Audit 08e-11: layer terms are file-name terms too (`rnm::split_layer`).
            if crate::rnm::LAYER_VOCABULARY.iter().any(|v| v.term == alias) {
                return Err(bad(format!(
                    "suffix alias '{alias}' is an RNM layer term (DECISIONS Note W)"
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

    /// Load a user preset file. The same file already loaded (e.g. from the user folder) is not loaded again
    /// (Note T6); another file under a loaded id is refused (Note T7).
    pub fn add_file(&mut self, path: &Path) -> Result<String, PresetError> {
        if let Some((id, _)) = self.sources.iter().find(|(_, s)| same_file(s, path)) {
            return Ok(id.clone());
        }
        let text = std::fs::read_to_string(path)
            .map_err(|e| PresetError::Parse(format!("{}: {e}", path.display())))?;
        self.insert_toml(&text, Source::File(path.display().to_string()))
            .map_err(|e| match e {
                PresetError::Parse(m) => PresetError::Parse(format!("{}: {m}", path.display())),
                e => e,
            })
    }

    pub fn get(&self, id: &str) -> Option<&PresetFile> {
        self.presets.get(id)
    }

    /// True when `id` was loaded from the embedded list (a user preset can never take a built-in id, Note T7).
    pub fn is_builtin(&self, id: &str) -> bool {
        self.sources.get(id) == Some(&Source::Builtin)
    }

    pub fn selectable(&self) -> Vec<&PresetFile> {
        self.presets.values().filter(|p| !p.hidden).collect()
    }

    /// Where `id` was loaded from: [`SOURCE_BUILTIN`], [`SOURCE_INLINE`] or the file path.
    pub fn source(&self, id: &str) -> Option<&str> {
        self.sources.get(id).map(Source::label)
    }

    /// `(id, source)` of every preset not loaded from the embedded list, in id order.
    pub fn user_presets(&self) -> Vec<(&str, &str)> {
        self.sources
            .iter()
            .filter(|(_, s)| **s != Source::Builtin)
            .map(|(id, s)| (id.as_str(), s.label()))
            .collect()
    }

    /// The preset's TOML table as loaded.
    pub fn table(&self, id: &str) -> Option<&toml::Table> {
        self.tables.get(id)
    }

    /// Forget a preset and its aliases (a user folder file that failed its check; the GUI preset creator validates an
    /// edited user preset in a copy without its old self, DECISIONS AM8).
    pub fn remove(&mut self, id: &str) {
        self.presets.remove(id);
        self.sources.remove(id);
        self.tables.remove(id);
        let owner = &self.alias_owner;
        self.aliases
            .retain(|a, _| owner.get(a).map(String::as_str) != Some(id));
        self.alias_owner.retain(|_, o| o != id);
    }

    /// Spec §3/§4 (`presets validate`, the user folder): the chain resolves and passes [`check_chain`] for each target
    /// it allows (its nearest `target`, else both).
    pub fn check_preset(&self, id: &str) -> Result<(), PresetError> {
        let chain = self.chain(id)?;
        let targets = match chain.iter().rev().find_map(|p| p.target) {
            Some(t) => vec![t],
            None => vec![Target::Cs, Target::Vanilla],
        };
        for t in targets {
            check_chain(&chain, &[], t)?;
        }
        Ok(())
    }

    /// The user presets folder (spec §3, Note T6): every `*.toml` directly in `dir`, in file-name order. Every file is
    /// loaded first, then each loaded preset must pass [`PresetRegistry::check_preset`], so a preset may base on one
    /// later in the order (Review Focus 2). A file that fails is skipped with CFG-PRESET-SKIPPED, and so is a preset
    /// based on a skipped one. A file refused while a preset skipped later held its id or an alias is tried again.
    /// A missing folder loads nothing.
    pub fn add_dir(&mut self, dir: &Path) -> Vec<Warning> {
        let skip = |file: &Path, reason: String| {
            Warning::new(
                Code::CfgPresetSkipped,
                format!("user preset {} skipped: {reason}", file.display()),
            )
            .with_data(serde_json::json!({ "file": file.display().to_string(), "reason": reason }))
        };
        let rd = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return vec![],
            Err(e) => return vec![skip(dir, e.to_string())],
        };
        let mut warnings = vec![];
        let mut files: Vec<PathBuf> = vec![];
        for entry in rd {
            let p = match entry {
                Ok(e) => e.path(),
                // The entry's name is unknown, so the warning names the folder.
                Err(e) => {
                    warnings.push(skip(dir, e.to_string()));
                    continue;
                }
            };
            if !p
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("toml"))
            {
                continue;
            }
            // Follows links, like `Path::is_file`, but reports a dangling or unreadable entry.
            match std::fs::metadata(&p) {
                Ok(m) if m.is_file() => files.push(p),
                Ok(_) => {}
                Err(e) => warnings.push(skip(&p, e.to_string())),
            }
        }
        files.sort();
        let mut loaded = vec![];
        let mut pending = files;
        loop {
            let mut refused = vec![];
            for f in pending {
                match self.add_file(&f) {
                    Ok(id) => loaded.push((id, f)),
                    Err(e) => refused.push((f, e)),
                }
            }
            // Removing a preset can orphan one based on it, so check again until nothing fails.
            let mut removed = false;
            while let Some((i, e)) = loaded
                .iter()
                .enumerate()
                .find_map(|(i, (id, _))| self.check_preset(id).err().map(|e| (i, e)))
            {
                let (id, f) = loaded.remove(i);
                self.remove(&id);
                warnings.push(skip(&f, e.to_string()));
                removed = true;
            }
            // Trace-debug #15 (Note BQ): a removed preset frees its id and aliases, so a file refused for claiming
            // them is tried again. Each pass removes a preset that never comes back, so this ends.
            if !removed || refused.is_empty() {
                warnings.extend(refused.into_iter().map(|(f, e)| skip(&f, e.to_string())));
                return warnings;
            }
            pending = refused.into_iter().map(|(f, _)| f).collect();
        }
    }

    /// The id of the preset loaded from the file `path` (compared canonically), if any.
    pub fn loaded_from(&self, path: &Path) -> Option<&str> {
        self.sources
            .iter()
            .find(|(_, s)| same_file(s, path))
            .map(|(id, _)| id.as_str())
    }

    /// `sdc presets validate` (spec §4): load `path` into a copy of this registry (without a preset already loaded
    /// from that same file, so a user folder file validates in place of itself) and check it. One line per problem;
    /// loading stops at the first, so a file has at most one (Note T8). Empty = clean.
    pub fn validate_file(&self, path: &Path) -> Vec<String> {
        let mut r = self.clone();
        if let Some(id) = self.loaded_from(path).map(str::to_string) {
            r.remove(&id);
        }
        match r.add_file(path) {
            Ok(id) => r
                .check_preset(&id)
                .err()
                .map(|e| e.to_string())
                .into_iter()
                .collect(),
            Err(e) => vec![e.to_string()],
        }
    }

    /// `sdc presets export` (spec §4, Note T8): `id`'s own table as loaded (`base` stays a link) under `new_id`,
    /// re-serialised (comments dropped, keys may reorder). `suffix_aliases` are copied as they are, so a copy of a
    /// preset with aliases does not load beside the original (the aliases conflict) until they are renamed or removed.
    pub fn export(&self, id: &str, new_id: &str) -> Result<String, PresetError> {
        let mut t = self
            .tables
            .get(id)
            .cloned()
            .ok_or_else(|| PresetError::Unknown(id.to_string()))?;
        if let Some(src) = self.source(new_id) {
            return Err(PresetError::Invalid(format!(
                "CFG-INVALID: preset id '{new_id}' is already loaded from {src}"
            )));
        }
        t.insert("id".into(), toml::Value::String(new_id.to_string()));
        toml::to_string(&t).map_err(|e| PresetError::Invalid(e.to_string()))
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

/// One sub-type's profile document before identity fields: the chain's layers, then each config's defaults and its
/// `[subtypes]` table for `st`.
pub fn chain_doc(chain: &[&PresetFile], configs: &[&ConfigFile], st: Subtype) -> Value {
    let mut doc = Value::Object(Map::new());
    for (_, patch) in PresetRegistry::layers_of(chain, st) {
        merge_json(&mut doc, &patch);
    }
    for c in configs {
        merge_json(&mut doc, &c.defaults);
        if let Some(s) = c.subtypes.get(&st) {
            merge_json(&mut doc, s);
        }
    }
    doc
}

/// Spec §3/§4/§8, audit EC-6 (Note BJ): the checks `presets validate` and the batch share. Every rule of `chain` and
/// of `configs` compiles its glob (audit #7), leaves `subtype` out of its overrides (EC-4) and names only known
/// fields; every sub-type's profile ([`chain_doc`]) deserialises under `target`, alone and with each rule applied.
pub fn check_chain(
    chain: &[&PresetFile],
    configs: &[&ConfigFile],
    target: Target,
) -> Result<(), PresetError> {
    let invalid = PresetError::Invalid;
    let rules: Vec<(String, &Rule)> = chain
        .iter()
        .flat_map(|p| {
            p.rules
                .iter()
                .enumerate()
                .map(|(i, r)| (format!("preset '{}' rule #{i}", p.id), r))
        })
        .chain(configs.iter().flat_map(|c| {
            c.rules
                .iter()
                .enumerate()
                .map(|(i, r)| (format!("config rule #{i}"), r))
        }))
        .collect();
    for (at, r) in &rules {
        if let Some(g) = &r.matcher.glob {
            crate::policy::compile_glob(g).map_err(|e| invalid(format!("{at}: {e}")))?;
        }
        if r.overrides.get("subtype").is_some() {
            return Err(invalid(format!("{at}: {SUBTYPE_IN_OVERRIDES}")));
        }
        let mut paths = vec![];
        leaf_paths(&r.overrides, "", &mut paths);
        if let Some(f) = paths
            .iter()
            .find(|f| !crate::overrides::FIELD_PATHS.contains(&f.as_str()))
        {
            return Err(invalid(format!("{at}: unknown field '{f}'")));
        }
    }
    let id = chain.last().map_or("", |p| p.id.as_str());
    for st in Subtype::ALL {
        let doc = chain_doc(chain, configs, st);
        finish_profile(doc.clone(), target, st)
            .map_err(|e| invalid(format!("preset '{id}', {} under {target:?}: {e}", st.id())))?;
        for (at, r) in &rules {
            let mut d = doc.clone();
            merge_json(&mut d, &r.overrides);
            finish_profile(d, target, st)
                .map_err(|e| invalid(format!("{at}, {} under {target:?}: {e}", st.id())))?;
        }
    }
    Ok(())
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
