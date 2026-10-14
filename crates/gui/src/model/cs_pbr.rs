//! Community Shaders PBR texture sets and material objects (design 2026-10-13): the field tables, the built-in
//! template library, the per-set blocks, the PGPatcher mapping, the scan reads and the EDID checks. No egui types.
use serde::Deserialize;
use serde_json::{Map, Value as Json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Txst,
    Mato,
}

impl Kind {
    /// The block's key in a `[[sets]]` table.
    pub fn key(self) -> &'static str {
        match self {
            Kind::Txst => "cs_txst",
            Kind::Mato => "cs_mato",
        }
    }
    /// The folder CS reads under `Data` (sdc writes it under the export root).
    pub fn folder(self) -> &'static str {
        match self {
            Kind::Txst => "PBRTextureSets",
            Kind::Mato => "PBRMaterialObjects",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Kind::Txst => "PBR texture set",
            Kind::Mato => "PBR material object",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Num,
    /// Three floats.
    Rgb,
    /// `glintParameters { enabled, screenSpaceScale, logMicrofacetDensity, microfacetRoughness, densityRandomization }`.
    Glint,
}

/// One CS field (`TruePBR.h`): its in-game slider range (the editor's drag range; JSON is not clamped) and CS's
/// default (a colour's default is each channel's).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Field {
    pub name: &'static str,
    pub kind: FieldKind,
    pub range: (f64, f64),
    pub default: f64,
}

const fn f(name: &'static str, kind: FieldKind, lo: f64, hi: f64, default: f64) -> Field {
    Field {
        name,
        kind,
        range: (lo, hi),
        default,
    }
}

use FieldKind::{Glint, Num, Rgb};

/// Texture-set fields, in CS's struct order (research §2).
pub const TXST_FIELDS: [Field; 13] = [
    f("roughnessScale", Num, 0.0, 3.0, 1.0),
    f("displacementScale", Num, 0.0, 3.0, 1.0),
    f("specularLevel", Num, 0.0, 3.0, 0.04),
    f("subsurfaceColor", Rgb, 0.0, 1.0, 0.0),
    f("subsurfaceOpacity", Num, 0.0, 1.0, 0.0),
    f("coatColor", Rgb, 0.0, 1.0, 1.0),
    f("coatStrength", Num, 0.0, 1.0, 1.0),
    f("coatRoughness", Num, 0.0, 1.0, 1.0),
    f("coatSpecularLevel", Num, 0.0, 1.0, 0.04),
    f("innerLayerDisplacementOffset", Num, 0.0, 3.0, 0.0),
    f("fuzzColor", Rgb, 0.0, 1.0, 0.0),
    f("fuzzWeight", Num, 0.0, 1.0, 0.0),
    f("glintParameters", Glint, 0.0, 0.0, 0.0),
];

/// Material-object fields. `roughness` is absolute, not a scale; CS's `specularLevel` default is 1.0.
pub const MATO_FIELDS: [Field; 4] = [
    f("baseColorScale", Rgb, 0.0, 10.0, 1.0),
    f("roughness", Num, 0.0, 1.0, 1.0),
    f("specularLevel", Num, 0.0, 1.0, 1.0),
    f("glintParameters", Glint, 0.0, 0.0, 0.0),
];

/// The numeric glint fields (`enabled`, a bool defaulting to false, is not listed).
pub const GLINT_FIELDS: [Field; 4] = [
    f("screenSpaceScale", Num, 0.0, 3.0, 1.5),
    f("logMicrofacetDensity", Num, 0.0, 40.0, 40.0),
    f("microfacetRoughness", Num, 0.0, 1.0, 0.015),
    f("densityRandomization", Num, 0.0, 5.0, 2.0),
];

pub fn fields(kind: Kind) -> &'static [Field] {
    match kind {
        Kind::Txst => &TXST_FIELDS,
        Kind::Mato => &MATO_FIELDS,
    }
}

pub fn field(kind: Kind, name: &str) -> Option<&'static Field> {
    fields(kind).iter().find(|f| f.name == name)
}

/// Whether `v` has the type CS reads for a field of kind `fk` (unknown glint sub-keys are allowed: never written).
pub fn valid_value(fk: FieldKind, v: &Json) -> bool {
    match fk {
        FieldKind::Num => v.is_number(),
        FieldKind::Rgb => v
            .as_array()
            .is_some_and(|a| a.len() == 3 && a.iter().all(Json::is_number)),
        FieldKind::Glint => v.as_object().is_some_and(|o| {
            o.iter().all(|(k, x)| match k.as_str() {
                "enabled" => x.is_boolean(),
                k if GLINT_FIELDS.iter().any(|g| g.name == k) => x.is_number(),
                _ => true,
            })
        }),
    }
}

/// One built-in template (design §4).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Template {
    pub name: String,
    pub kind: Kind,
    /// The EDIDs Faultier used these values for.
    pub edids: Vec<String>,
    pub values: Map<String, Json>,
}

/// The library: Faultier's PBR landscape modders resource, deduplicated by `tools/gen-cs-pbr-templates.py`. A
/// parse failure is a test failure (`tests/cs_pbr.rs`), never a runtime path.
pub fn templates() -> &'static [Template] {
    static LIB: std::sync::OnceLock<Vec<Template>> = std::sync::OnceLock::new();
    LIB.get_or_init(|| {
        serde_json::from_str(include_str!("../../assets/cs_pbr_templates.json"))
            .expect("cs_pbr_templates.json parses")
    })
}

pub fn template(kind: Kind, name: &str) -> Option<&'static Template> {
    templates()
        .iter()
        .find(|t| t.kind == kind && t.name == name)
}

/// Every number as a float (`16` → `16.0`): CS silently drops a colour array of int literals (research §2).
pub fn floats(v: &Json) -> Json {
    match v {
        Json::Number(n) => n.as_f64().map_or(Json::Null, |x| serde_json::json!(x)),
        Json::Array(a) => Json::Array(a.iter().map(floats).collect()),
        Json::Object(o) => Json::Object(o.iter().map(|(k, x)| (k.clone(), floats(x))).collect()),
        _ => v.clone(),
    }
}
