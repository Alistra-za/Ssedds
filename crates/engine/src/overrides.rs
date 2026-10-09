//! `--set` overrides (spec §8): `field=value`, `subtype:<id>.field=value`, `file:<glob>.field=value`.
use crate::config::Subtype;
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    All,
    Subtype(Subtype),
    File(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SetOverride {
    pub scope: Scope,
    pub field: String,
    pub value: Value,
}

/// Every settable dotted path (Profile fields minus identity fields target/category).
pub const FIELD_PATHS: &[&str] = &[
    "subtype",
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
    "mips.normal",
    "mips.coverage",
    "format",
    "header",
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
];

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
        if !known(field) {
            return Err(format!("--set '{s}': unknown field '{field}'"));
        }
        (Scope::Subtype(st), field)
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
