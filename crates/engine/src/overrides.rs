//! `--set` overrides (spec §8): `field=value`, `subtype:<id>.field=value`, `file:<glob>.field=value`, and
//! `set:<id>.shader=<shader>` (M6, DECISIONS Note S-shader-1: a set's shader, the only field a set scope takes).
//! `Scope::Path` is never parsed from `--set`; it comes only from `sdc.toml`'s `[files]` table.
use crate::codes::Code;
use crate::config::Subtype;
use crate::discover::InputFile;
use crate::shader::Shader;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    All,
    Subtype(Subtype),
    File(String),
    /// A `[files]` key (DECISIONS N2): '/'-separated, relative to the project root, matched by `path_key_matches`.
    Path(String),
    /// `set:<id>`: a set id as `sets::set_id` prints it, or a bare base (M6, DECISIONS Note S-shader-1). Sets only
    /// `shader`; it never reaches a file's profile.
    Set(String),
}

/// Whether a `[files]` key names the file at project-relative path `rel` (DECISIONS N2): case-insensitively,
/// `rel == key`, or `rel` without the last extension of its file name `== key` (a packed output's key).
pub fn path_key_matches(key: &str, rel: &str) -> bool {
    let (k, r) = (key.to_lowercase(), rel.to_lowercase());
    if r == k {
        return true;
    }
    let (dir, name) = r.rsplit_once('/').unwrap_or(("", r.as_str()));
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => {
            let without = if dir.is_empty() {
                stem.to_string()
            } else {
                format!("{dir}/{stem}")
            };
            without == k
        }
        _ => false,
    }
}

/// `p` as an absolute path (the current directory for an empty or relative one); `p` itself when that fails.
pub(crate) fn absolute_or_same(p: &Path) -> PathBuf {
    let p = if p.as_os_str().is_empty() {
        Path::new(".")
    } else {
        p
    };
    std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())
}

/// `input`'s path relative to `root`, '/'-separated: the folder of `input.path` relative to `root`, then the file
/// name of `input.rel` (for a packed output, its virtual name). `None` when `input` is not under `root`.
pub fn project_rel(root: &Path, input: &InputFile) -> Option<String> {
    let lower = |p: &Path| p.to_string_lossy().replace('\\', "/").to_lowercase();
    // One path form: a relative root or input (e.g. `--config sdc.toml`) is resolved against the current directory.
    let (root, dir) = (
        absolute_or_same(root),
        absolute_or_same(input.path.parent()?),
    );
    let (root, dir) = (root.as_path(), dir.as_path());
    let (root_s, dir_s) = (lower(root), lower(dir));
    let root_s = root_s.trim_end_matches('/');
    let rest = if dir_s == root_s {
        ""
    } else {
        dir_s.strip_prefix(&format!("{root_s}/"))?
    };
    let dir_orig = dir.to_string_lossy().replace('\\', "/");
    // Keep the original case of the folder part, by taking the same number of bytes from the original string.
    // Lower-casing can change the byte length (non-ASCII): then fall back to the lower-cased part.
    let rest_orig = if dir_orig.len() == dir_s.len() {
        dir_orig.get(dir_orig.len() - rest.len()..).unwrap_or(rest)
    } else {
        rest
    };
    let name = input.rel.file_name()?.to_string_lossy();
    Some(if rest_orig.is_empty() {
        name.into_owned()
    } else {
        format!("{rest_orig}/{name}")
    })
}

/// The `[files]` layer (DECISIONS N2) and the per-file acknowledgements, resolved against one project root.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FileScopes {
    /// The folder `[files]` keys are relative to; `None` disables the layer.
    pub root: Option<PathBuf>,
    /// `[files]` field values, every one `Scope::Path`, in key order then field order.
    pub overrides: Vec<SetOverride>,
    /// Key → codes acknowledged for that file only.
    pub allow: BTreeMap<String, BTreeSet<Code>>,
}

impl FileScopes {
    pub const NONE: FileScopes = FileScopes {
        root: None,
        overrides: Vec::new(),
        allow: BTreeMap::new(),
    };

    /// The project-relative path of `input` when the layer is active.
    pub fn rel_of(&self, input: &InputFile) -> Option<String> {
        project_rel(self.root.as_deref()?, input)
    }
}

/// For `ResolveCtx` literals that have no `[files]` layer (tests, `inspect` on a single file).
pub static NO_FILE_SCOPES: FileScopes = FileScopes::NONE;

#[derive(Debug, Clone, PartialEq)]
pub struct SetOverride {
    pub scope: Scope,
    pub field: String,
    pub value: Value,
}

/// Every settable dotted path (Profile fields minus identity fields target/category), plus `reencode`, a
/// plan-time selector (DECISIONS S6).
pub const FIELD_PATHS: &[&str] = &[
    "subtype",
    "reencode",
    "color.content",
    "color.tag",
    "alpha.policy",
    "alpha.threshold",
    "alpha.compare",
    "alpha.exact_opaque",
    "channels.swizzle",
    "channels.flip_green",
    "channels.green_convention",
    "channels.reconstruct_z",
    "channels.grayscale_source",
    "mips.levels",
    "mips.filter",
    "mips.edge",
    "mips.normal",
    "mips.normal_base",
    "mips.bake_clamp",
    "mips.coverage",
    "format",
    "header",
    "resize",
    "encoder.backend",
    "encoder.effort",
    "encoder.rgbcx_level",
    "encoder.perceptual",
    "encoder.bc1_transparent_black",
    "encoder.bc45_search_radius",
    "validation.min_mip_psnr_db",
    "validation.coverage_tolerance_pts",
    "validation.complex_m_warn_alpha",
    "validation.bc6h_min_peak_ratio",
    "validation.pgpatcher_non_white_share",
    "validation.mips",
    "packing.enabled",
    "packing.roughness_default",
    "packing.prefer",
    "packing.recipe",
    "packing.fill.roughness",
    "packing.fill.metallic",
    "packing.fill.ao",
    "packing.fill.specular",
    "packing.fill.glossiness",
    "packing.fill.height",
    "packing.fill.opacity",
    "packing.fill.fuzz",
    "rnm.enabled",
    "rnm.order",
    "rnm.bump.strength",
    "rnm.bump.flip_green",
    "rnm.bump.mask",
    "rnm.bump2.strength",
    "rnm.bump2.flip_green",
    "rnm.bump2.mask",
    "rnm.detail.strength",
    "rnm.detail.flip_green",
    "rnm.detail.mask",
    "rnm.detail2.strength",
    "rnm.detail2.flip_green",
    "rnm.detail2.mask",
    "rnm.bent.strength",
    "rnm.bent.flip_green",
    "rnm.bent.mask",
    "rnm.bent.mode",
    "rnm.bent.ao_power",
    "rnm.bent.ao",
    "rnm.target",
];

pub const REENCODE_FIELD: &str = "reencode";

/// M6 Track F2 (DECISIONS Note W3): which base normal a layer file feeds; a per-file selector, not a profile field.
pub const RNM_TARGET_FIELD: &str = "rnm.target";

/// A plan-time per-file selector (`reencode`, `rnm.target`): never merged into a profile.
pub fn is_selector(field: &str) -> bool {
    field == REENCODE_FIELD || field == RNM_TARGET_FIELD
}

/// `rnm.target`'s value, when it is one of `n`, `cnr`, `both`.
pub fn rnm_target_value(v: &Value) -> Option<crate::config::RnmTarget> {
    serde_json::from_value(v.clone()).ok()
}

/// A TOML literal (`100`, `true`, `"x"`) keeps its type; anything else (`bc1`, `none`) is a string.
fn parse_value(raw: &str) -> Value {
    match toml::from_str::<toml::Table>(&format!("v = {raw}")) {
        Ok(t) => serde_json::to_value(&t["v"]).unwrap_or(Value::String(raw.to_string())),
        Err(_) => Value::String(raw.to_string()),
    }
}

pub fn parse_set(s: &str) -> Result<SetOverride, String> {
    let (key, raw) = s
        .split_once('=')
        .ok_or_else(|| format!("--set '{s}': expected <field>=<value>"))?;
    let value = parse_value(raw.trim());
    let known = |f: &str| FIELD_PATHS.contains(&f);
    let (scope, field) = if let Some(rest) = key.strip_prefix("subtype:") {
        let (id, field) = rest
            .split_once('.')
            .ok_or_else(|| format!("--set '{s}': expected subtype:<id>.<field>"))?;
        let st: Subtype = serde_json::from_value(Value::String(id.to_string()))
            .map_err(|_| format!("--set '{s}': unknown sub-type '{id}'"))?;
        if field == "subtype" {
            return Err(format!(
                "--set '{s}': a sub-type scope cannot set the sub-type"
            ));
        }
        if field == REENCODE_FIELD {
            return Err(format!(
                "--set '{s}': reencode is decided per file, before the sub-type: use reencode=true or file:<glob>.reencode=true"
            ));
        }
        if field == RNM_TARGET_FIELD {
            return Err(format!(
                "--set '{s}': rnm.target is decided per layer file: use rnm.target=<n|cnr|both> or file:<glob>.rnm.target=<…>"
            ));
        }
        if !known(field) {
            return Err(format!("--set '{s}': unknown field '{field}'"));
        }
        (Scope::Subtype(st), field)
    } else if let Some(rest) = key.strip_prefix("set:") {
        // Set ids may contain dots, so only the `.shader` suffix splits the key (DECISIONS Note S-shader-1).
        let id = match rest.strip_suffix(".shader") {
            Some(id) if !id.is_empty() => id,
            _ if rest.contains('.') && !rest.starts_with('.') => {
                return Err(format!("--set '{s}': a set scope sets only `shader`"))
            }
            _ => return Err(format!("--set '{s}': expected set:<id>.shader=<shader>")),
        };
        let raw = raw.trim();
        if serde_json::from_value::<Shader>(Value::String(raw.to_string())).is_err() {
            let ids: Vec<&str> = Shader::ALL.iter().map(|s| s.id()).collect();
            return Err(format!(
                "--set '{s}': unknown shader '{raw}' (one of: {})",
                ids.join(", ")
            ));
        }
        return Ok(SetOverride {
            scope: Scope::Set(id.to_string()),
            field: "shader".to_string(),
            value: Value::String(raw.to_string()),
        });
    } else if let Some(rest) = key.strip_prefix("file:") {
        // The glob may itself contain dots, so take the longest known field the key ends with.
        let field = FIELD_PATHS
            .iter()
            .filter(|f| rest.len() > f.len() + 1 && rest.ends_with(&format!(".{f}")))
            .max_by_key(|f| f.len())
            .ok_or_else(|| format!("--set '{s}': expected file:<glob>.<field>"))?;
        let glob = &rest[..rest.len() - field.len() - 1];
        (Scope::File(glob.to_string()), *field)
    } else if known(key) {
        (Scope::All, key)
    } else {
        return Err(format!("--set '{s}': unknown field '{key}'"));
    };
    // A sub-type decides the whole profile, so a misspelt one must not be ignored.
    if field == "subtype" && serde_json::from_value::<Subtype>(value.clone()).is_err() {
        return Err(format!("--set '{s}': unknown sub-type '{raw}'"));
    }
    if field == REENCODE_FIELD && !value.is_boolean() {
        return Err(format!("--set '{s}': reencode takes true or false"));
    }
    if field == RNM_TARGET_FIELD && rnm_target_value(&value).is_none() {
        return Err(format!("--set '{s}': rnm.target takes n, cnr or both"));
    }
    Ok(SetOverride {
        scope,
        field: field.to_string(),
        value,
    })
}

/// Dotted path and value to a nested JSON object: `("alpha.policy", "data")` → `{"alpha": {"policy": "data"}}`.
pub fn to_patch(field: &str, value: &Value) -> Value {
    let mut v = value.clone();
    for part in field.rsplit('.') {
        let mut m = Map::new();
        m.insert(part.to_string(), v);
        v = Value::Object(m);
    }
    v
}
