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

/// Note BM (owner): the plain name a field row shows; the exact JSON key is its hover text.
pub fn field_label(name: &str) -> &str {
    match name {
        "roughnessScale" => "Roughness scale",
        "displacementScale" => "Displacement scale",
        "specularLevel" => "Specular level",
        "subsurfaceColor" => "Subsurface colour",
        "subsurfaceOpacity" => "Subsurface opacity",
        "coatColor" => "Coat colour",
        "coatStrength" => "Coat strength",
        "coatRoughness" => "Coat roughness",
        "coatSpecularLevel" => "Coat specular level",
        "innerLayerDisplacementOffset" => "Inner layer displacement offset",
        "fuzzColor" => "Fuzz colour",
        "fuzzWeight" => "Fuzz weight",
        "glintParameters" => "Glint parameters",
        "baseColorScale" => "Base colour scale",
        "roughness" => "Roughness",
        "screenSpaceScale" => "Screen-space scale",
        "logMicrofacetDensity" => "Log microfacet density",
        "microfacetRoughness" => "Microfacet roughness",
        "densityRandomization" => "Density randomization",
        _ => name,
    }
}

/// Note BM (owner): texture-set fields CS's landscape shader never reads. `Lighting.hlsl` (CS AIO 2026-10-03) reads
/// per landscape layer only `PBRParams` (roughness scale, displacement scale, specular level) and the glint
/// parameters (`LIGHTING_LANDSCAPE_BLEND_ONE_LAYER_PBR`); the subsurface, coat (two-layer) and fuzz branches sit
/// under `#if !defined(LANDSCAPE) && !defined(LODLANDSCAPE)`, and the inner-layer offset belongs to the coat's
/// interlayer parallax. The card shows these only when the set's PGPatcher entry is written (meshes sharing the
/// texture read them) or when they hold a value.
pub const LANDSCAPE_UNREAD: [&str; 9] = [
    "subsurfaceColor",
    "subsurfaceOpacity",
    "coatColor",
    "coatStrength",
    "coatRoughness",
    "coatSpecularLevel",
    "innerLayerDisplacementOffset",
    "fuzzColor",
    "fuzzWeight",
];

/// Note BM16 (owner): fields the texture set and the material object share with one meaning, edited once in the
/// card's Shared section. Only the glint parameters: CS reads both blocks' glint as the same struct
/// (`Lighting.hlsl` lerps the surface's `glintParameters` to the material object's `SparkleParams`). The texture
/// set's `roughnessScale` / `specularLevel` multiply the `_rmaos` channels (`rawRMAOS *= float4(PBRParams1.x, 1, 1,
/// PBRParams1.z)`), while the material object's `roughness` / `specularLevel` replace them
/// (`rawRMAOS.xyw = lerp(…, float3(ParallaxOccData.x, 0, ParallaxOccData.y), …)`), so they stay separate rows.
pub const SHARED_FIELDS: [&str; 1] = ["glintParameters"];
/// The block key listing a block's unsynced shared fields; not a CS field, so never exported.
pub const UNSYNCED: &str = "unsynced";

/// Note BM16: what a field's value means, for the rows whose name alone is ambiguous (hover text).
pub fn field_note(kind: Kind, name: &str) -> Option<&'static str> {
    Some(match (kind, name) {
        (Kind::Txst, "roughnessScale") => "Multiplies the _rmaos roughness channel.",
        (Kind::Txst, "specularLevel") => "Multiplies the _rmaos specular channel.",
        (Kind::Mato, "roughness") => {
            "Absolute roughness where the material object projects (replaces the surface's; not a scale)."
        }
        (Kind::Mato, "specularLevel") => {
            "Absolute specular level where the material object projects (replaces the surface's; not a scale)."
        }
        _ => return None,
    })
}

/// Whether a material object follows the texture set's value of shared field `field`: not listed as unsynced and
/// holding the same value (differing blocks, as in an older project, show as unsynced and are never overwritten).
pub fn mato_synced(txst: Option<&CsBlock>, mato: &CsBlock, field: &str) -> bool {
    !mato.unsynced.iter().any(|f| f == field)
        && txst.and_then(|b| b.values.get(field)) == mato.values.get(field)
}

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

use super::AppModel;
use crate::pgp::settings::Settings;
use crate::project::item_json;
use crate::sets_doc::SetsDoc;
use engine::sets::same_id;
use serde_json::json;
use toml_edit::TableLike;

/// Texture sets: when the set's PGPatcher entry is written (design §5.1). `statics = "auto" | "always" | "never"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Statics {
    #[default]
    Auto,
    Always,
    Never,
}

impl Statics {
    pub fn parse(s: Option<&str>) -> Statics {
        match s {
            Some("always") => Statics::Always,
            Some("never") => Statics::Never,
            _ => Statics::Auto,
        }
    }
    /// The value written; `None` for the default.
    pub fn as_str(self) -> Option<&'static str> {
        match self {
            Statics::Auto => None,
            Statics::Always => Some("always"),
            Statics::Never => Some("never"),
        }
    }
}

/// One block as read from `sdc-project.toml` (design §3).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CsBlock {
    pub edids: Vec<String>,
    /// The last applied template (informational).
    pub template: Option<String>,
    pub statics: Statics,
    /// Note BM16: the shared fields this block keeps its own value for (`unsynced = [...]`; never exported).
    pub unsynced: Vec<String>,
    /// Known fields with a usable value, as read (`glintParameters` an object).
    pub values: Map<String, Json>,
    /// Known fields whose value has the wrong type: kept in the file, warned about, not exported (design §8).
    pub bad: Vec<String>,
}

/// Reads a block table. Unknown keys are left alone: they stay in the file and are never exported.
pub(crate) fn block_of(t: &dyn TableLike, kind: Kind) -> CsBlock {
    let mut b = CsBlock::default();
    for (k, item) in t.iter() {
        match k {
            "edids" => {
                b.edids = item
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default()
            }
            "template" => b.template = item.as_str().map(str::to_string),
            "statics" => b.statics = Statics::parse(item.as_str()),
            UNSYNCED => {
                b.unsynced = item
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default()
            }
            _ => {
                if let Some(f) = field(kind, k) {
                    match item_json(item).filter(|v| valid_value(f.kind, v)) {
                        Some(v) => {
                            b.values.insert(k.to_string(), v);
                        }
                        None => b.bad.push(k.to_string()),
                    }
                }
            }
        }
    }
    b
}

/// CS texture-set field → PGPatcher settings key (design §5.1, research §1.5). The `subsurface` and `multilayer`
/// switches stay the user's: they need textures and NIF flags a texture set cannot know (plan ruling R7).
pub const PGP_MAP: [(&str, &str); 14] = [
    ("roughnessScale", "roughness_scale"),
    ("specularLevel", "specular_level"),
    ("displacementScale", "displacement_scale"),
    ("subsurfaceColor", "subsurface_color"),
    ("subsurfaceOpacity", "subsurface_opacity"),
    ("coatColor", "coat_color"),
    ("coatStrength", "coat_strength"),
    ("coatRoughness", "coat_roughness"),
    ("coatSpecularLevel", "coat_specular_level"),
    ("glintParameters.enabled", "glint_on"),
    (
        "glintParameters.screenSpaceScale",
        "glint.screen_space_scale",
    ),
    (
        "glintParameters.logMicrofacetDensity",
        "glint.log_microfacet_density",
    ),
    (
        "glintParameters.microfacetRoughness",
        "glint.microfacet_roughness",
    ),
    (
        "glintParameters.densityRandomization",
        "glint.density_randomization",
    ),
];

/// The PGPatcher settings a texture-set block stands for: every number as a float, colours too (AG133).
pub fn cs_to_pgp(values: &Map<String, Json>) -> Settings {
    let mut out = Settings::new();
    for (cs, pgp) in PGP_MAP {
        let v = match cs.split_once('.') {
            Some((o, k)) => values.get(o).and_then(|g| g.get(k)),
            None => values.get(cs),
        };
        let Some(v) = v else { continue };
        out.insert(pgp.to_string(), floats(v));
    }
    out
}

/// A PGPatcher edit of a mapped key in CS form: (dotted block key, value); `None` when `key` is not mapped.
pub fn pgp_to_cs(key: &str, v: Option<&Json>) -> Option<(&'static str, Option<Json>)> {
    let &(cs, _) = PGP_MAP.iter().find(|(_, p)| *p == key)?;
    let v = v.map(|v| {
        // Only the pre-AG133 0-255 int form is scaled; float colours are already the block's form.
        if cs.ends_with("Color") && crate::pgp::settings::rgb_is_legacy(v) {
            Json::Array(
                v.as_array()
                    .into_iter()
                    .flatten()
                    .map(|c| {
                        json!((c.as_f64().unwrap_or(0.0) / 255.0 * 10_000.0).round() / 10_000.0)
                    })
                    .collect(),
            )
        } else {
            floats(v)
        }
    });
    Some((cs, v))
}

/// Design §5.1: a texture-set block is the single source of the keys it maps; the set's own copies are dropped.
pub fn overlay(settings: &mut Settings, b: &CsBlock) {
    for (_, pgp) in PGP_MAP {
        settings.remove(pgp);
    }
    settings.extend(cs_to_pgp(&b.values));
}

impl AppModel {
    /// Set `set`'s block of `kind`, as the document holds it.
    pub fn cs_block(&self, set: &str, kind: Kind) -> Option<CsBlock> {
        let entries = self.sets_doc.as_ref()?.entries();
        let e = entries.iter().find(|e| same_id(&e.id, set))?;
        match kind {
            Kind::Txst => e.cs_txst.clone(),
            Kind::Mato => e.cs_mato.clone(),
        }
    }

    /// One undoable, JSON-only edit of the sets document (design §3; nothing is planned, DECISIONS P7, except when a
    /// texture-set block comes or goes: Note BM4).
    fn cs_edit(
        &mut self,
        set: &str,
        label: &str,
        merge: Option<String>,
        f: impl FnOnce(&mut SetsDoc, &str),
    ) {
        if !self.sets_editable() {
            return;
        }
        let s = set.to_string();
        let txst = super::sets::cs_txst_sets(self);
        self.edit_docs(label, merge, false, move |_, d| f(d, &s));
        // Note BM: a texture-set block that comes or goes is shader-pick input, so that edit plans again.
        if super::sets::cs_txst_sets(self) != txst {
            self.replan_after_edit();
        }
    }

    pub fn add_cs_block(&mut self, set: &str, kind: Kind) {
        self.cs_edit(set, &format!("Add {}", kind.label()), None, |d, s| {
            d.add_cs_block(s, kind)
        });
    }

    pub fn remove_cs_block(&mut self, set: &str, kind: Kind) {
        self.cs_edit(set, &format!("Remove {}", kind.label()), None, |d, s| {
            d.remove_cs_block(s, kind)
        });
    }

    /// Set (or with `None` remove) block key `key` (dotted: `glintParameters.enabled`). Drags merge into one step.
    pub fn set_cs_value(&mut self, set: &str, kind: Kind, key: &str, v: Option<Json>) {
        let merge = Some(format!("{set}:{}:{key}", kind.key()));
        let k = key.to_string();
        self.cs_edit(set, &format!("Change {key}"), merge, move |d, s| {
            d.set_cs_value(s, kind, &k, v)
        });
    }

    pub fn set_cs_edids(&mut self, set: &str, kind: Kind, edids: &[String]) {
        let v = json!(edids);
        self.cs_edit(set, "Change EDIDs", None, move |d, s| {
            d.set_cs_value(s, kind, "edids", Some(v))
        });
    }

    /// Design §4: one undo step; see `SetsDoc::apply_cs_template`.
    pub fn apply_cs_template(&mut self, set: &str, kind: Kind, name: &str) -> Result<(), String> {
        let t =
            template(kind, name).ok_or_else(|| format!("no {} template {name}", kind.label()))?;
        self.cs_edit(set, &format!("Apply template {name}"), None, move |d, s| {
            d.apply_cs_template(s, t)
        });
        Ok(())
    }

    pub fn set_cs_statics(&mut self, set: &str, statics: Statics) {
        self.set_cs_value(set, Kind::Txst, "statics", statics.as_str().map(Json::from));
    }

    /// Note BM16: a Shared-section edit (`key` dotted under a shared field): the texture set, and the material object
    /// when it follows, in one undo step. The PGPatcher keys follow the texture set (AD2).
    pub fn set_shared_value(&mut self, set: &str, key: &str, v: Option<Json>) {
        let field = key.split('.').next().unwrap_or(key);
        let txst = self.cs_block(set, Kind::Txst);
        let mato = self
            .cs_block(set, Kind::Mato)
            .is_some_and(|b| mato_synced(txst.as_ref(), &b, field));
        let merge = Some(format!("{set}:shared:{key}"));
        let k = key.to_string();
        self.cs_edit(set, &format!("Change {key}"), merge, move |d, s| {
            d.set_cs_value(s, Kind::Txst, &k, v.clone());
            if mato {
                d.set_cs_value(s, Kind::Mato, &k, v);
            }
        });
    }

    /// Note BM16: "Sync" copies the texture set's value of `field` into the material object and drops it from
    /// `unsynced`; "Unsync" lists it, keeping the value. One undo step each.
    pub fn set_mato_synced(&mut self, set: &str, field: &str, synced: bool) {
        let Some(mato) = self.cs_block(set, Kind::Mato) else {
            return;
        };
        let shared = self
            .cs_block(set, Kind::Txst)
            .and_then(|b| b.values.get(field).cloned());
        let mut list: Vec<String> = mato.unsynced.into_iter().filter(|f| f != field).collect();
        if !synced {
            list.push(field.to_string());
        }
        let list = (!list.is_empty()).then(|| json!(list));
        let f = field.to_string();
        let label = if synced { "Sync" } else { "Unsync" };
        self.cs_edit(set, &format!("{label} {field}"), None, move |d, s| {
            if synced {
                d.set_cs_value(s, Kind::Mato, &f, shared);
            }
            d.set_cs_value(s, Kind::Mato, UNSYNCED, list);
        });
    }

    /// Note BM16: the card's "Add PBR material object": the new block starts with the texture set's shared values,
    /// so it follows them (one undo step).
    pub fn add_mato_synced(&mut self, set: &str) {
        let txst = self.cs_block(set, Kind::Txst).unwrap_or_default();
        self.cs_edit(set, "Add PBR material object", None, move |d, s| {
            d.add_cs_block(s, Kind::Mato);
            for f in SHARED_FIELDS {
                if let Some(v) = txst.values.get(f) {
                    d.set_cs_value(s, Kind::Mato, f, Some(v.clone()));
                }
            }
        });
    }
}

use super::plugin_refs::{set_key, LIMIT, NO_SCAN};
use super::sets::SetView;
use modscan::plugin_store::WinRec;
use modscan::store::{MatchRow, Store, TexKey};
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

/// Shape rows read for the statics count; more only lowers a count that is already far above 0.
pub const STATICS_ROWS: usize = 20_000;

/// What the scan says about one set's diffuse (design §5.1, §6).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SetFacts {
    /// EDIDs of the winning TXSTs whose slot matches the set (`txsts_matching`): suggestions; W4 when empty.
    pub txst_edids: Vec<String>,
    /// Non-LOD winning NIFs whose shapes the set's PGPatcher entry matches (ruling R6).
    pub statics: Vec<String>,
}

/// The scan's winning TXST and MATO records with an EDID (W1–W3, the material-object picker).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScanRecords {
    pub txst: Vec<WinRec>,
    pub mato: Vec<WinRec>,
}

impl ScanRecords {
    pub fn of(&self, kind: Kind) -> &[WinRec] {
        match kind {
            Kind::Txst => &self.txst,
            Kind::Mato => &self.mato,
        }
    }
}

/// The scan database file's (mtime, length); `None` without one.
pub(crate) type Stamp = Option<(Option<SystemTime>, u64)>;

pub(crate) fn stamp(db: Option<&Path>) -> Stamp {
    let md = std::fs::metadata(db?).ok().filter(|m| m.is_file())?;
    Some((md.modified().ok(), md.len()))
}

/// The scan reads of the CS PBR section, the statics rule and the badges, for the session. shortcut: read on the UI
/// thread on demand (card open, a badge, export start) like "Used by plugins"; move to a worker if a modlist makes
/// them slow. Errors are cached like answers (an old-schema database is not re-opened every frame); everything is
/// forgotten when the per-frame database stamp (`AppModel::poll`, Note BI4) changes (a new scan, or a running scan's
/// checkpoint) and on "Refresh scan". Reads in one frame share one open database (Note BM, review M2).
#[derive(Default)]
pub struct CsPbrScan {
    pub facts: RefCell<HashMap<TexKey, Result<SetFacts, String>>>,
    pub records: RefCell<Option<Result<Arc<ScanRecords>, String>>>,
    /// The database stamp the cached reads came from.
    stamp: RefCell<Stamp>,
    /// This frame's open database (dropped by `new_frame`, so the file is never held across frames).
    store: RefCell<Option<std::rc::Rc<Store>>>,
}

impl std::fmt::Debug for CsPbrScan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CsPbrScan")
            .field("facts", &self.facts)
            .field("records", &self.records)
            .field("stamp", &self.stamp)
            .finish_non_exhaustive()
    }
}

fn open(db: Option<&Path>) -> Result<Store, String> {
    let p = db
        .filter(|p| p.is_file())
        .ok_or_else(|| NO_SCAN.to_string())?;
    Store::open_read_only(p).map_err(|e| e.to_string())
}

/// Distinct mesh paths of `rows`, LOD meshes left out (`is_lod_path`, PGPatcher's own blocklist).
pub fn non_lod_nifs(rows: &[MatchRow]) -> Vec<String> {
    rows.iter()
        .map(|r| r.nif_path.as_str())
        .filter(|p| !nif::path::is_lod_path(p))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(str::to_string)
        .collect()
}

fn read_facts(s: &Store, key: &TexKey) -> Result<SetFacts, String> {
    let mut txst_edids: Vec<String> = s
        .txsts_matching(key, LIMIT)
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter_map(|t| t.edid)
        .collect();
    txst_edids.sort();
    txst_edids.dedup();
    let statics = statics_of(STATICS_ROWS, |n| {
        s.nifs_using_texture(key, n).map_err(|e| e.to_string())
    })?;
    Ok(SetFacts {
        txst_edids,
        statics,
    })
}

/// The non-LOD meshes of `read(cap)`; a full read of LOD meshes only is read again without the cap, so LOD shapes
/// ahead of a static in the store's order never hide it.
#[doc(hidden)]
pub fn statics_of(
    cap: usize,
    read: impl Fn(usize) -> Result<Vec<MatchRow>, String>,
) -> Result<Vec<String>, String> {
    let rows = read(cap)?;
    let statics = non_lod_nifs(&rows);
    if statics.is_empty() && rows.len() >= cap {
        return Ok(non_lod_nifs(&read(usize::MAX)?));
    }
    Ok(statics)
}

fn read_records(s: &Store) -> Result<Arc<ScanRecords>, String> {
    let w = |sig: &str| s.winning_records(sig).map_err(|e| e.to_string());
    Ok(Arc::new(ScanRecords {
        txst: w("TXST")?,
        mato: w("MATO")?,
    }))
}

impl CsPbrScan {
    /// Forget the cached reads when `now` (the frame's database stamp) is not the one they came from.
    fn sync(&self, now: Stamp) {
        if *self.stamp.borrow() != now {
            self.refresh();
            *self.stamp.borrow_mut() = now;
        }
    }

    /// Called once per frame (`AppModel::poll`): the next read opens the database afresh.
    pub fn new_frame(&self) {
        *self.store.borrow_mut() = None;
    }

    fn with_store<T>(
        &self,
        db: Option<&Path>,
        f: impl FnOnce(&Store) -> Result<T, String>,
    ) -> Result<T, String> {
        // Bound first: a borrow in the match scrutinee lives through the arms, and the `None` arm borrows mutably.
        let cached = self.store.borrow().clone();
        let s = match cached {
            Some(s) => s,
            None => {
                let s = std::rc::Rc::new(open(db)?);
                *self.store.borrow_mut() = Some(s.clone());
                s
            }
        };
        f(&s)
    }

    pub fn facts(&self, db: Option<&Path>, now: Stamp, key: &TexKey) -> Result<SetFacts, String> {
        self.sync(now);
        if let Some(r) = self.facts.borrow().get(key) {
            return r.clone();
        }
        let r = self.with_store(db, |s| read_facts(s, key));
        self.facts.borrow_mut().insert(key.clone(), r.clone());
        r
    }

    pub fn records(&self, db: Option<&Path>, now: Stamp) -> Result<Arc<ScanRecords>, String> {
        self.sync(now);
        if let Some(r) = &*self.records.borrow() {
            return r.clone();
        }
        let r = self.with_store(db, read_records);
        *self.records.borrow_mut() = Some(r.clone());
        r
    }

    pub fn refresh(&self) {
        self.facts.borrow_mut().clear();
        *self.records.borrow_mut() = None;
        *self.store.borrow_mut() = None;
    }
}

impl AppModel {
    /// The scan facts of a set's texture key, through the session cache and this frame's stamp.
    pub fn scan_facts(&self, key: &TexKey) -> Result<SetFacts, String> {
        self.cs_pbr
            .facts(self.scan_db.as_deref(), self.scan_stamp, key)
    }

    pub fn scan_records(&self) -> Result<Arc<ScanRecords>, String> {
        self.cs_pbr
            .records(self.scan_db.as_deref(), self.scan_stamp)
    }
}

/// Whether and why a texture-set set gets its PGPatcher entry (design §5.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticsStatus {
    Never,
    Always,
    /// The non-LOD meshes of the latest scan whose shapes the entry matches.
    Used(Vec<String>),
    Unused,
    NoScan(String),
}

pub fn statics_text(s: &StaticsStatus) -> String {
    match s {
        StaticsStatus::Used(p) => format!("Used by {} meshes: PGPatcher entry written", p.len()),
        StaticsStatus::Unused => {
            "No mesh of the latest scan uses this diffuse (LOD meshes do not count): no PGPatcher entry".into()
        }
        StaticsStatus::NoScan(e) => {
            format!("{e} No PGPatcher entry is written until a scan finds meshes using this diffuse.")
        }
        StaticsStatus::Always => "statics = always: PGPatcher entry written".into(),
        StaticsStatus::Never => "statics = never: no PGPatcher entry".into(),
    }
}

/// Note BM: the PBR Landscape card's "Also write into the PGPatcher JSON" box (it replaces the statics choice).
pub const PGP_BOX: &str = "Also write into the PGPatcher JSON";

/// The line under the PGPatcher box (PBR Landscape sets only): the auto reason, or that the user set it. `None` = no
/// texture-set block yet.
pub fn pgp_box_text(s: Option<&StaticsStatus>) -> String {
    match s {
        None => "No texture set yet: the PGPatcher entry is written as for any PBR set".into(),
        Some(StaticsStatus::Always) => "Ticked by you: PGPatcher entry written".into(),
        Some(StaticsStatus::Never) => "Unticked by you: no PGPatcher entry".into(),
        // Note BM6 (owner): with no usable scan a PBR Landscape set keeps its entry.
        Some(StaticsStatus::NoScan(e)) => format!(
            "{e} The PGPatcher entry is written until a scan finds no mesh using this diffuse (LOD meshes do not count)."
        ),
        Some(s) => statics_text(s),
    }
}

/// Note BM (review I2): why the box is off and disabled for a PBR Landscape set that cannot have a PGPatcher entry.
pub const PGP_BOX_UNAVAILABLE: &str = "Not available: the set's colour map is not a PBR albedo (pbr_albedo), so it \
                                       has no PGPatcher entry. Set its type to pbr_albedo to write one.";

impl AppModel {
    /// `None` without a texture-set block (the set's entry is written as before).
    pub fn statics_status(&self, v: &SetView) -> Option<StaticsStatus> {
        let b = self.cs_block(&v.id, Kind::Txst)?;
        Some(match b.statics {
            Statics::Never => StaticsStatus::Never,
            Statics::Always => StaticsStatus::Always,
            Statics::Auto => match set_key(v) {
                None => StaticsStatus::Unused,
                Some(k) => match self.scan_facts(&k) {
                    Err(e) => StaticsStatus::NoScan(e),
                    Ok(f) if f.statics.is_empty() => StaticsStatus::Unused,
                    Ok(f) => StaticsStatus::Used(f.statics),
                },
            },
        })
    }

    /// The statics rule keeps the set's PGPatcher entry. Note BM6 (owner): a PBR Landscape set on auto with no usable
    /// scan keeps it too, so the card's first edit (which creates the block) never turns the entry off.
    pub fn pgp_entry_wanted(&self, v: &SetView) -> bool {
        match self.statics_status(v) {
            None | Some(StaticsStatus::Always | StaticsStatus::Used(_)) => true,
            Some(StaticsStatus::NoScan(_)) => super::sets::is_pbr_landscape(v),
            Some(StaticsStatus::Never | StaticsStatus::Unused) => false,
        }
    }

    /// Note BM (review I2): the set's PGPatcher entry is written: a PGPatcher JSON set the statics rule keeps.
    pub fn pgp_written(&self, v: &SetView) -> bool {
        super::sets::is_json_set(v, self.target) && self.pgp_entry_wanted(v)
    }

    /// Note BM (review I2): the set writes CS records: a texture-set or material-object block with an EDID.
    pub fn cs_records_written(&self, v: &SetView) -> bool {
        [Kind::Txst, Kind::Mato]
            .into_iter()
            .any(|k| self.cs_block(&v.id, k).is_some_and(|b| !b.edids.is_empty()))
    }
}

pub const NOT_CHECKED: &str = "EDIDs not checked: no plugin scan yet";
/// W4.
pub const NO_TXST_USES: &str = "No texture set of the latest scan uses this texture";

/// Design §6 W1–W3 for one EDID against the scan's winning records of its kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EdidCheck {
    Found,
    /// W2: only a case-insensitive match; the scanned record's exact spelling (CS is case-sensitive).
    Case(String),
    /// W1.
    NotFound,
    /// W3.
    NotSinglePass,
    /// W3, a DATA too short to carry flags.
    SinglePassUnknown,
}

pub fn check_edid(kind: Kind, edid: &str, recs: &[WinRec]) -> EdidCheck {
    match recs.iter().find(|r| r.edid == edid) {
        Some(r) if kind == Kind::Mato => match r.mato_flags {
            None => EdidCheck::SinglePassUnknown,
            Some(f) if f & 1 == 0 => EdidCheck::NotSinglePass,
            Some(_) => EdidCheck::Found,
        },
        Some(_) => EdidCheck::Found,
        None => match recs.iter().find(|r| r.edid.eq_ignore_ascii_case(edid)) {
            Some(r) => EdidCheck::Case(r.edid.clone()),
            None => EdidCheck::NotFound,
        },
    }
}

/// The warning shown next to the EDID; `None` when it is fine. W2's exact spelling is the fix button's label.
pub fn check_text(c: &EdidCheck) -> Option<&'static str> {
    Some(match c {
        EdidCheck::Found => return None,
        EdidCheck::Case(_) => "the scanned record differs in case only (CS matches the exact case)",
        EdidCheck::NotFound => "not found among the scanned winning records",
        EdidCheck::NotSinglePass => "CS ignores this JSON unless the record is single-pass",
        EdidCheck::SinglePassUnknown => {
            "single-pass unknown (the record's DATA is too short to carry flags)"
        }
    })
}

pub fn single_pass_text(flags: Option<u32>) -> &'static str {
    match flags {
        None => "single-pass unknown",
        Some(f) if f & 1 == 0 => "not single-pass",
        Some(_) => "single-pass",
    }
}
