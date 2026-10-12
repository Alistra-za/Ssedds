//! The user's Community Shaders settings (03 §3): `SettingsUser.json` over `SettingsDefault.json` per key, which
//! features CS loaded (`ShaderCache/Info.ini`, else `Shaders/Features/*.ini` minus "Disable at Boot"), a change
//! hash, and the feature defines the preview compiles. Never fails: problems become notes and fallbacks.
use crate::cbuffer::CbWriter;
use crate::compile::VarKind;
use crate::scene::{FEATURE_FLOATS, FEATURE_UINTS};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Where the settings files and the feature ini folder are; `None` = not found.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SettingsPaths {
    pub user: Option<PathBuf>,
    pub default: Option<PathBuf>,
    pub info_ini: Option<PathBuf>,
    pub features_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CsSettings {
    /// sha256 over the bytes read (C10): an equal hash means nothing to apply.
    pub hash: String,
    /// Info.ini short names of the features CS loaded.
    pub loaded: BTreeSet<String>,
    /// `"Linear Lighting".enableLinearLighting != 0`; true when absent (the P0 default).
    pub linear_lighting: bool,
    /// Every section, User over Default per key (nested objects per key too).
    pub merged: Map<String, Value>,
    /// Problems and fallbacks taken, for the panel.
    pub notes: Vec<String>,
    /// A present file failed to parse: the GUI keeps its previous settings and shows the note (C10).
    pub parse_failed: bool,
}

impl Default for CsSettings {
    /// No files: built-in values, Linear Lighting on, no feature loaded.
    fn default() -> Self {
        CsSettings {
            hash: settings_hash(None, None, None, &BTreeSet::new()),
            loaded: BTreeSet::new(),
            linear_lighting: true,
            merged: Map::new(),
            notes: vec![],
            parse_failed: false,
        }
    }
}

/// Material-local CS features the preview compiles when loaded and not switched off (03 §3.2, C8): Info.ini
/// name → define. A row whose installed-CS gate test (T-S5) fails is deleted here (C9).
pub const PREVIEW_FEATURES: &[(&str, &str)] = &[
    ("VanillaFresnel", "VANILLA_FRESNEL"),
    ("HairSpecular", "CS_HAIR"),
    ("Skin", "CS_SKIN"),
    ("ExtendedMaterials", "EXTENDED_MATERIALS"),
    ("ExtendedTranslucency", "EXTENDED_TRANSLUCENCY"),
];

/// Reads and merges the settings. Absent files are skipped; a file that does not parse is skipped with a note
/// and `parse_failed`, but its bytes still count in the hash.
pub fn read_settings(p: &SettingsPaths) -> CsSettings {
    let mut notes = vec![];
    let mut parse_failed = false;
    let user = read(p.user.as_deref(), &mut notes, &mut parse_failed);
    let default = read(p.default.as_deref(), &mut notes, &mut parse_failed);
    // An unreadable Info.ini is a note only: the Features inis stand in for it.
    let info = read(p.info_ini.as_deref(), &mut notes, &mut false);
    let mut merged = Map::new();
    for (file, bytes) in [
        ("SettingsDefault.json", &default),
        ("SettingsUser.json", &user),
    ] {
        let Some(b) = bytes else {
            continue;
        };
        match serde_json::from_slice::<Value>(strip_bom(b)) {
            Ok(Value::Object(m)) => overlay(&mut merged, m),
            Ok(_) => {
                parse_failed = true;
                notes.push(format!("{file} is not a JSON object: ignored"));
            }
            Err(e) => {
                parse_failed = true;
                notes.push(format!("{file} could not be parsed ({e}): ignored"));
            }
        }
    }
    let linear_lighting =
        setting(&merged, "Linear Lighting", "enableLinearLighting").is_none_or(|x| x != 0.0);
    let loaded = match info.as_deref().map(std::str::from_utf8) {
        Some(Ok(text)) => info_ini_features(text),
        Some(Err(e)) => {
            parse_failed = true;
            notes.push(format!("Info.ini is not UTF-8 ({e}): ignored"));
            feature_inis(p.features_dir.as_deref(), &merged, &mut notes)
        }
        None => feature_inis(p.features_dir.as_deref(), &merged, &mut notes),
    };
    CsSettings {
        hash: settings_hash(
            user.as_deref(),
            default.as_deref(),
            info.as_deref(),
            &loaded,
        ),
        loaded,
        linear_lighting,
        merged,
        notes,
        parse_failed,
    }
}

/// The feature defines for these settings (C8), sorted, so equal lists mean "defines unchanged".
pub fn preview_defines(s: &CsSettings, features_on: bool) -> Vec<String> {
    if !features_on {
        return vec![];
    }
    let mut out: Vec<String> = PREVIEW_FEATURES
        .iter()
        .filter(|(name, _)| s.loaded.contains(*name) && !switched_off(s, name))
        .map(|(_, define)| define.to_string())
        .collect();
    if s.linear_lighting {
        out.push("ENABLE_LL".into());
    }
    out.sort();
    out
}

/// The feature's own master switch is 0. Skin, ExtendedMaterials and ExtendedTranslucency have none.
fn switched_off(s: &CsSettings, feature: &str) -> bool {
    let (section, key) = match feature {
        "VanillaFresnel" => ("Vanilla Fresnel", "Enable"),
        "HairSpecular" => ("Hair Specular", "Enabled"),
        _ => return false,
    };
    setting(&s.merged, section, key).is_some_and(|x| x == 0.0)
}

fn setting(m: &Map<String, Value>, section: &str, key: &str) -> Option<f64> {
    m.get(section).and_then(|s| s.get(key)).and_then(num)
}

/// A JSON number, or a bool as 1/0 (CS writes enables as integers and some flags as bools).
fn num(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_bool().map(|b| f64::from(u8::from(b))))
}

/// `top` over `base` per key; two objects merge per key, anything else is replaced.
fn overlay(base: &mut Map<String, Value>, top: Map<String, Value>) {
    for (k, v) in top {
        match (base.get_mut(&k), v) {
            (Some(Value::Object(b)), Value::Object(t)) => overlay(b, t),
            (Some(slot), v) => *slot = v,
            (None, v) => {
                base.insert(k, v);
            }
        }
    }
}

/// A file that exists but cannot be read sets `failed` (the caller keeps its previous settings); absent is `None`.
fn read(p: Option<&Path>, notes: &mut Vec<String>, failed: &mut bool) -> Option<Vec<u8>> {
    let p = p?;
    match std::fs::read(p) {
        Ok(b) => Some(b),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            *failed = true;
            notes.push(format!("{}: {e}", p.display()));
            None
        }
    }
}

fn strip_bom(b: &[u8]) -> &[u8] {
    b.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(b)
}

/// Info.ini (BOM, CRLF or LF): every `[Section]` but `[Cache]` with `Enabled = true`.
fn info_ini_features(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut section = "";
    for line in text.trim_start_matches('\u{feff}').lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.trim();
        } else if let Some((k, v)) = line.split_once('=') {
            if !section.is_empty()
                && section != "Cache"
                && k.trim() == "Enabled"
                && v.trim().eq_ignore_ascii_case("true")
            {
                out.insert(section.to_string());
            }
        }
    }
    out
}

/// No usable Info.ini (disk cache off, or the game never ran): a feature is loaded iff `<dir>/<Name>.ini` exists
/// and "Disable at Boot" does not set it (03 §3.1).
fn feature_inis(
    dir: Option<&Path>,
    merged: &Map<String, Value>,
    notes: &mut Vec<String>,
) -> BTreeSet<String> {
    let Some(dir) = dir else {
        return BTreeSet::new();
    };
    notes.push(format!(
        "no usable Info.ini: loaded features taken from {}",
        dir.display()
    ));
    let off = |name: &str| setting(merged, "Disable at Boot", name).is_some_and(|x| x != 0.0);
    match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("ini")))
            .filter_map(|p| p.file_stem().and_then(|s| s.to_str()).map(str::to_string))
            .filter(|n| !off(n))
            .collect(),
        Err(e) => {
            notes.push(format!("{}: {e}", dir.display()));
            BTreeSet::new()
        }
    }
}

/// C10: sha256 over (role, length, bytes) or (role, "absent") for user, default, info_ini, in that order, then the
/// sorted loaded feature names: features found only through `Shaders/Features` (no Info.ini) change the hash too.
fn settings_hash(
    user: Option<&[u8]>,
    default: Option<&[u8]>,
    info: Option<&[u8]>,
    loaded: &BTreeSet<String>,
) -> String {
    let mut h = Sha256::new();
    for (role, b) in [("user", user), ("default", default), ("info_ini", info)] {
        h.update(role.as_bytes());
        match b {
            Some(b) => {
                h.update((b.len() as u64).to_le_bytes());
                h.update(b);
            }
            None => h.update(b"absent"),
        }
    }
    h.update(b"loaded");
    for n in loaded {
        h.update((n.len() as u64).to_le_bytes());
        h.update(n.as_bytes());
    }
    hex::encode(h.finalize())
}

/// JSON section → FeatureData struct prefix, with the keys whose struct field is named differently (C5).
type Section = (
    &'static str,
    &'static str,
    &'static [(&'static str, &'static str)],
);
const SECTIONS: &[Section] = &[
    ("Linear Lighting", "linearLightingSettings", &[]),
    ("Hair Specular", "hairSpecularSettings", &[]),
    ("Vanilla Fresnel", "vanillaFresnelSettings", &[]),
    ("True PBR", "truePBRSettings", &[]),
    (
        "Extended Materials",
        "extendedMaterialSettings",
        &[("EnableTerrain", "EnableTerrainParallax")],
    ),
    (
        "Extended Translucency",
        "extendedTranslucencySettings",
        &[
            ("AlphaMode", "MaterialModel"),
            ("AlphaReduction", "Reduction"),
            ("AlphaSoftness", "Softness"),
            ("AlphaStrength", "Strength"),
        ],
    ),
];

/// Linear Lighting's `diffuseCurve` (C4, no settings key): the inverse of CS's DiffuseToWorking curve at mid
/// grey. `x = 0.5^gamma; mid·(1−x) / (x·(white−mid))`.
pub fn diffuse_curve(gamma: f64, mid: f64, white: f64) -> f64 {
    let x = 0.5f64.powf(gamma);
    mid * (1.0 - x) / (x * (white - mid))
}

/// FeatureData values by cbuffer name, raw numbers (the writer types them from the reflection): the
/// capture-11178 constants, overwritten by every number the settings have (C7: a missing file or key keeps the
/// constant). Names this CS build lacks are dropped at write time.
pub fn feature_data(s: &CsSettings) -> Vec<(String, Vec<f64>)> {
    let mut out: Vec<(String, Vec<f64>)> = FEATURE_FLOATS
        .iter()
        .map(|(n, v)| (n.to_string(), v.iter().map(|x| f64::from(*x)).collect()))
        .chain(
            FEATURE_UINTS
                .iter()
                .map(|(n, u)| (n.to_string(), vec![f64::from(*u)])),
        )
        .collect();
    for (section, prefix, renames) in SECTIONS {
        let Some(Value::Object(m)) = s.merged.get(*section) else {
            continue;
        };
        for (key, v) in m {
            let field = renames
                .iter()
                .find(|r| r.0 == key.as_str())
                .map_or(key.as_str(), |r| r.1);
            let vals: Vec<f64> = match v {
                Value::Array(a) => a.iter().filter_map(num).collect(),
                v => num(v).into_iter().collect(),
            };
            if !vals.is_empty() {
                put(&mut out, &format!("{prefix}.{field}"), vals);
            }
        }
    }
    let ll = |k: &str| setting(&s.merged, "Linear Lighting", k);
    if let (Some(gamma), Some(mid)) = (ll("diffuseGamma"), ll("diffuseMidReflectance")) {
        let white = ll("diffuseWhiteReflectance").unwrap_or(1.0);
        put(
            &mut out,
            "linearLightingSettings.diffuseCurve",
            vec![diffuse_curve(gamma, mid, white)],
        );
    }
    // C4 / spec §2.2: the installed CS uploads π whatever the setting says (capture 11178).
    put(
        &mut out,
        "linearLightingSettings.pointLightMult",
        vec![f64::from(std::f32::consts::PI)],
    );
    put(
        &mut out,
        "linearLightingSettings.isMainOrLoadingMenu",
        vec![0.0],
    );
    if let Some(Value::Object(skin)) = s.merged.get("Advanced Skin") {
        pack_skin(skin, &mut out);
    }
    // C2: the PS reads the profile from `SkinPerGeometry` (b7), not FeatureData (Skin.hlsli:21-31). No skin
    // extra / wetness / sk textures in the preview: flags 0; per-geometry wetness 0 (Skin.hlsli:254 returns 0).
    let profile: Vec<(String, Vec<f64>)> = out
        .iter()
        .filter_map(|(n, v)| {
            n.strip_prefix("skinData.")
                .map(|f| (format!("skinPerGeometryProfile.{f}"), v.clone()))
        })
        .collect();
    out.extend(profile);
    out.push(("skinPerGeometry".into(), vec![0.0; 4]));
    out.push(("skinMaterialFlags".into(), vec![0.0; 4]));
    out
}

fn put(out: &mut Vec<(String, Vec<f64>)>, name: &str, vals: Vec<f64>) {
    match out.iter_mut().find(|(n, _)| n == name) {
        Some(slot) => slot.1 = vals,
        None => out.push((name.to_string(), vals)),
    }
}

/// `"Advanced Skin"` → `skinData.*` (C5). Source keys follow how the installed CS Lighting.hlsl / Skin.hlsli read
/// each component (line refs: CS 2026-10-03); a key the files lack keeps the capture-11178 component.
fn pack_skin(skin: &Map<String, Value>, out: &mut [(String, Vec<f64>)]) {
    let profile = skin.get("DefaultProfile");
    let p = |k: &str| profile.and_then(|d| d.get(k)).and_then(num);
    let top = |k: &str| skin.get(k).and_then(num);
    // shortcut: tiling × body multiplier is inferred from the captured 20 = 10 × 2 (the shader only reads the
    // product, :1673); revisit if a capture with BodyTilingMultiplier ≠ 2 disagrees.
    let tiling = p("SkinDetailTiling")
        .zip(p("BodyTilingMultiplier"))
        .map(|(t, b)| t * b);
    let packed = [
        // x roughness, y secondary roughness (:1969-1970), z × glossiness (:1969), w skin on (:1365).
        (
            "skinParams",
            [
                p("SkinMainRoughness"),
                p("SkinSecondRoughness"),
                p("SkinSpecularTexMultiplier"),
                top("EnableSkin"),
            ],
        ),
        // x secondary specular (:1977), y extra wetness (:1666, :2190), z F0 (:1979), w base colour × (:1555).
        (
            "skinParams2",
            [
                p("SecondarySpecularStrength"),
                top("ExtraSkinWetness"),
                p("F0"),
                p("BaseColorMultiplier"),
            ],
        ),
        // x detail UV tiling (:1673), y wet UV tiling (:1663), z detail strength (:1688), w detail on (:1682).
        (
            "skinDetailParams",
            [
                tiling,
                p("BodyTilingMultiplier"),
                p("SkinDetailStrength"),
                p("EnableSkinDetail"),
            ],
        ),
        // x·w translucency (Skin.hlsli:166), y width (Skin.hlsli:162), z amount (:3159).
        (
            "sssParams",
            [
                p("Translucency"),
                p("sssWidth"),
                p("SSSAmount"),
                p("UseSSS"),
            ],
        ),
        // x weight (:1982), y roughness (:1983), z colour (:1984), w edge roughness (:1968).
        (
            "fuzzParams",
            [
                p("FuzzStrength"),
                p("FuzzRoughness"),
                p("FuzzF0"),
                p("ExtraEdgeRoughness"),
            ],
        ),
        // x, y roughness multipliers (:1972-1973), z specular with a skin-extra map (:1979), w wet response (:2216).
        (
            "physicalParams",
            [
                p("PhysicalMainRoughnessMultiplier"),
                p("PhysicalSecondRoughnessMultiplier"),
                p("PhysicalSpecularStrength"),
                p("WetResponse"),
            ],
        ),
    ];
    for (field, comps) in packed {
        let name = format!("skinData.{field}");
        if let Some((_, v)) = out.iter_mut().find(|(n, _)| *n == name) {
            for (dst, c) in v.iter_mut().zip(comps) {
                if let Some(c) = c {
                    *dst = c;
                }
            }
        }
    }
    if let Some(Value::Array(a)) = skin.get("WetParams") {
        let w: Vec<f64> = a.iter().filter_map(num).collect();
        if w.len() == 4 {
            if let Some(slot) = out.iter_mut().find(|(n, _)| n == "skinData.wetParams") {
                slot.1 = w;
            }
        }
    }
}

/// Writes `values` by name, each as its reflected type (C6): float (and unknown) fields as f32, uint and bool
/// fields as u32, int fields as i32 bits. Names this build's layout lacks are skipped.
pub fn write_feature_data(w: &mut CbWriter, values: &[(String, Vec<f64>)]) {
    for (name, v) in values {
        match w.kind(name) {
            None => {}
            Some(VarKind::Float | VarKind::Other) => {
                let f: Vec<f32> = v.iter().map(|x| *x as f32).collect();
                w.set(name, &f);
            }
            Some(VarKind::Uint | VarKind::Bool) => {
                let u: Vec<u32> = v.iter().map(|x| *x as u32).collect();
                w.set_u32(name, &u);
            }
            Some(VarKind::Int) => {
                let u: Vec<u32> = v.iter().map(|x| *x as i32 as u32).collect();
                w.set_u32(name, &u);
            }
        }
    }
}
