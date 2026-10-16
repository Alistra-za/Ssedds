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

use super::AppModel;
use crate::pgp::settings::Settings;
use crate::sets_doc::SetsDoc;
use engine::sets::same_id;
use serde_json::json;
use toml_edit::{Item, TableLike};

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
    /// Known fields with a usable value, as read (`glintParameters` an object).
    pub values: Map<String, Json>,
    /// Known fields whose value has the wrong type: kept in the file, warned about, not exported (design §8).
    pub bad: Vec<String>,
}

/// A TOML item as JSON; a `[table]` (or dotted keys) becomes an object.
fn item_json(item: &Item) -> Option<Json> {
    match item {
        Item::Table(t) => Some(Json::Object(
            t.iter()
                .filter_map(|(k, i)| Some((k.to_string(), item_json(i)?)))
                .collect(),
        )),
        _ => crate::project::toml_to_json(item),
    }
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

/// The PGPatcher settings a texture-set block stands for: colours as 0–255 ints (DECISIONS P10), numbers as floats.
pub fn cs_to_pgp(values: &Map<String, Json>) -> Settings {
    let mut out = Settings::new();
    for (cs, pgp) in PGP_MAP {
        let v = match cs.split_once('.') {
            Some((o, k)) => values.get(o).and_then(|g| g.get(k)),
            None => values.get(cs),
        };
        let Some(v) = v else { continue };
        let v = if cs.ends_with("Color") {
            Json::Array(
                v.as_array()
                    .into_iter()
                    .flatten()
                    .map(|c| {
                        json!((c.as_f64().unwrap_or(0.0) * 255.0)
                            .round()
                            .clamp(0.0, 255.0) as i64)
                    })
                    .collect(),
            )
        } else {
            floats(v)
        };
        out.insert(pgp.to_string(), v);
    }
    out
}

/// A PGPatcher edit of a mapped key in CS form: (dotted block key, value); `None` when `key` is not mapped.
pub fn pgp_to_cs(key: &str, v: Option<&Json>) -> Option<(&'static str, Option<Json>)> {
    let &(cs, _) = PGP_MAP.iter().find(|(_, p)| *p == key)?;
    let v = v.map(|v| {
        if cs.ends_with("Color") {
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

    /// One undoable, JSON-only edit of the sets document (design §3; nothing is planned, DECISIONS P7).
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
        self.edit_docs(label, merge, false, move |_, d| f(d, &s));
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

/// The scan reads of the CS PBR section and the statics rule, for the session. shortcut: read on the UI thread on
/// demand (section open, export start) like "Used by plugins"; move to a worker if a modlist makes them slow.
/// Errors are cached like answers (an old-schema database is not re-opened every frame); everything is forgotten
/// when the database file's mtime or length changes (a new scan) and on "Refresh scan".
#[derive(Debug, Default)]
pub struct CsPbrScan {
    pub facts: RefCell<HashMap<TexKey, Result<SetFacts, String>>>,
    pub records: RefCell<Option<Result<Arc<ScanRecords>, String>>>,
    /// The database file the cached reads came from.
    stamp: RefCell<Stamp>,
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

fn read_facts(db: Option<&Path>, key: &TexKey) -> Result<SetFacts, String> {
    let s = open(db)?;
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

fn read_records(db: Option<&Path>) -> Result<Arc<ScanRecords>, String> {
    let s = open(db)?;
    let w = |sig: &str| s.winning_records(sig).map_err(|e| e.to_string());
    Ok(Arc::new(ScanRecords {
        txst: w("TXST")?,
        mato: w("MATO")?,
    }))
}

impl CsPbrScan {
    /// Forget the cached reads when `db` is not the file they came from (one stat per call).
    fn sync(&self, db: Option<&Path>) {
        let now = stamp(db);
        if *self.stamp.borrow() != now {
            self.refresh();
            *self.stamp.borrow_mut() = now;
        }
    }

    pub fn facts(&self, db: Option<&Path>, key: &TexKey) -> Result<SetFacts, String> {
        self.sync(db);
        if let Some(r) = self.facts.borrow().get(key) {
            return r.clone();
        }
        let r = read_facts(db, key);
        self.facts.borrow_mut().insert(key.clone(), r.clone());
        r
    }

    pub fn records(&self, db: Option<&Path>) -> Result<Arc<ScanRecords>, String> {
        self.sync(db);
        if let Some(r) = &*self.records.borrow() {
            return r.clone();
        }
        let r = read_records(db);
        *self.records.borrow_mut() = Some(r.clone());
        r
    }

    pub fn refresh(&self) {
        self.facts.borrow_mut().clear();
        *self.records.borrow_mut() = None;
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

impl AppModel {
    /// `None` without a texture-set block (the set's entry is written as before).
    pub fn statics_status(&self, v: &SetView) -> Option<StaticsStatus> {
        let b = self.cs_block(&v.id, Kind::Txst)?;
        Some(match b.statics {
            Statics::Never => StaticsStatus::Never,
            Statics::Always => StaticsStatus::Always,
            Statics::Auto => match set_key(v) {
                None => StaticsStatus::Unused,
                Some(k) => match self.cs_pbr.facts(self.scan_db.as_deref(), &k) {
                    Err(e) => StaticsStatus::NoScan(e),
                    Ok(f) if f.statics.is_empty() => StaticsStatus::Unused,
                    Ok(f) => StaticsStatus::Used(f.statics),
                },
            },
        })
    }

    pub fn pgp_entry_wanted(&self, v: &SetView) -> bool {
        matches!(
            self.statics_status(v),
            None | Some(StaticsStatus::Always | StaticsStatus::Used(_))
        )
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
