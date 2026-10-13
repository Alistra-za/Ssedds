//! The user's Community Shaders settings (03 §3): `SettingsUser.json` over `SettingsDefault.json` per key, which
//! features CS loaded (`ShaderCache/Info.ini`, else `Shaders/Features/*.ini` minus "Disable at Boot"), a change
//! hash, and the feature defines the preview compiles. Never fails: problems become notes and fallbacks.
use crate::cbuffer::CbWriter;
use crate::compile::{CbLayout, VarKind};
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
/// Reflected only by the new Linear Lighting (AIO 1.9.0): π on the CPU, LL switched by a compile-time define.
pub const NEW_LL: &str = "linearLightingSettings.diffuseCurve";
/// Reflected only by the old Linear Lighting (1.6, 1.8, Nexus 1.9.1): π in the shader, LL switched by a cbuffer flag.
pub const OLD_LL: &str = "linearLightingSettings.isDirLightLinear";

/// Whether Linear Lighting is on in the preview. Old LL: the setting, with no key = off (the shader's flag reads the
/// written 0, Nexus before CS's first run). New LL: `CsSettings::linear_lighting` (no key = the P0 default, on).
pub fn linear_lighting_on(s: &CsSettings, old_ll: bool, features_on: bool) -> bool {
    features_on
        && if old_ll {
            setting(&s.merged, "Linear Lighting", "enableLinearLighting").is_some_and(|x| x != 0.0)
        } else {
            s.linear_lighting
        }
}

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

/// Settings keys with no FeatureData field by design (03 report 3, 6.3): CPU-only (`SkinnedOnly`, the sweat
/// timers, `DetailTexture`, `EnableVerboseJsonLogging`), consumed by a derivation (`diffuseMidReflectance` ->
/// diffuseCurve, `enableLinearLighting` -> the LL switch) or unused by the shader (`enableACEScg`, `gameGamma`,
/// 1.9 `emitColorMult`, `UseDynamicWetness`).
const NO_FIELD_KEYS: &[&str] = &[
    "SkinnedOnly",
    "enableACEScg",
    "gameGamma",
    "emitColorMult",
    "diffuseMidReflectance",
    "enableLinearLighting",
    "EnableVerboseJsonLogging",
    "DetailTexture",
    "FullSweat",
    "StartSweat",
    "WetFadeTime",
    "UseDynamicWetness",
];

/// Every `"Advanced Skin"` key [`pack_skin`] reads (profile or section root).
const SKIN_KEYS: &[&str] = &[
    "EnableSkin",
    "ExtraSkinWetness",
    "WetParams",
    "SkinMainRoughness",
    "SkinSecondRoughness",
    "SkinSpecularTexMultiplier",
    "SecondarySpecularStrength",
    "F0",
    "BaseColorMultiplier",
    "SkinDetailTiling",
    "BodyTilingMultiplier",
    "SkinDetailStrength",
    "EnableSkinDetail",
    "Translucency",
    "sssWidth",
    "SSSAmount",
    "UseSSS",
    "FuzzStrength",
    "FuzzRoughness",
    "FuzzF0",
    "ExtraEdgeRoughness",
    "PhysicalMainRoughnessMultiplier",
    "PhysicalSecondRoughnessMultiplier",
    "PhysicalSpecularStrength",
    "WetResponse",
];

/// The struct field a section key lands in (C5 renames).
fn renamed<'a>(renames: &[(&str, &'a str)], key: &'a str) -> &'a str {
    renames.iter().find(|r| r.0 == key).map_or(key, |r| r.1)
}

/// What one program's settings write missed (03 report 6.3): reflected fields nothing wrote, and settings keys
/// that matched no field.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Coverage {
    pub unset: BTreeSet<String>,
    pub unmatched: BTreeSet<String>,
}

impl Coverage {
    /// The panel line; `None` when there is nothing to report.
    pub fn line(&self) -> Option<String> {
        (!self.unset.is_empty() || !self.unmatched.is_empty()).then(|| {
            format!(
                "CS settings: {} fields unset, {} keys unmatched",
                self.unset.len(),
                self.unmatched.len()
            )
        })
    }
}

/// `fields` (one program's [`reflected_fields`]) against `written` (its [`feature_data`]) and the settings. Checked
/// fields: the 7 sections' structs, `skinData`, b7 and b8, minus `pad*`. Checked keys: the 7 sections', minus
/// [`NO_FIELD_KEYS`], strings and objects; a section whose struct this build lacks is skipped.
pub fn coverage(
    s: &CsSettings,
    fields: &BTreeSet<String>,
    written: &[(String, Vec<f64>)],
) -> Coverage {
    let under = |f: &str, p: &str| f.strip_prefix(p).is_some_and(|r| r.starts_with('.'));
    let has = |p: &str| fields.iter().any(|f| under(f.as_str(), p));
    let in_scope = |f: &str| {
        SECTIONS
            .iter()
            .map(|sec| sec.1)
            .chain(["skinData", "skinPerGeometryProfile"])
            .any(|p| under(f, p))
            || matches!(f, "skinPerGeometry" | "skinMaterialFlags" | "emissiveMult")
    };
    let is_pad = |f: &str| f.rsplit('.').next().is_some_and(|l| l.starts_with("pad"));
    let wrote: BTreeSet<&str> = written.iter().map(|(n, _)| n.as_str()).collect();
    let unset = fields
        .iter()
        .filter(|f| in_scope(f.as_str()) && !is_pad(f.as_str()) && !wrote.contains(&f.as_str()))
        .cloned()
        .collect();
    let counts = |k: &str, v: &Value| {
        matches!(v, Value::Number(_) | Value::Bool(_) | Value::Array(_))
            && !NO_FIELD_KEYS.contains(&k)
    };
    let mut unmatched = BTreeSet::new();
    for (section, prefix, renames) in SECTIONS {
        let Some(Value::Object(m)) = s.merged.get(*section) else {
            continue;
        };
        if !has(prefix) {
            continue;
        }
        for (key, v) in m {
            if counts(key.as_str(), v)
                && !fields.contains(&format!("{prefix}.{}", renamed(renames, key)))
            {
                unmatched.insert(format!("{section}.{key}"));
            }
        }
    }
    if let (Some(Value::Object(skin)), true) = (s.merged.get("Advanced Skin"), has("skinData")) {
        let profile = match skin.get("DefaultProfile") {
            Some(Value::Object(d)) => Some(d),
            _ => None,
        };
        for (key, v) in skin.iter().chain(profile.into_iter().flatten()) {
            if counts(key.as_str(), v) && !SKIN_KEYS.contains(&key.as_str()) {
                unmatched.insert(format!("Advanced Skin.{key}"));
            }
        }
    }
    Coverage { unset, unmatched }
}

/// Linear Lighting's `diffuseCurve` (C4, no settings key): the inverse of CS's DiffuseToWorking curve at mid
/// grey. `x = 0.5^gamma; mid·(1−x) / (x·(white−mid))`.
pub fn diffuse_curve(gamma: f64, mid: f64, white: f64) -> f64 {
    let x = 0.5f64.powf(gamma);
    mid * (1.0 - x) / (x * (white - mid))
}

/// Every cbuffer variable name of `layouts` without its namespace (`SharedData::linearLightingSettings.diffuseCurve`
/// → `linearLightingSettings.diffuseCurve`): the shape evidence [`feature_data`] keys its exceptions on (P1-D).
pub fn reflected_fields<'a>(layouts: impl IntoIterator<Item = &'a CbLayout>) -> BTreeSet<String> {
    layouts
        .into_iter()
        .flat_map(|l| &l.vars)
        .map(|v| unqualified(&v.name).to_string())
        .collect()
}

/// `ns::a.b` → `a.b`. Only the part before the first `.` can carry a namespace (struct members are dotted).
fn unqualified(name: &str) -> &str {
    let head = name.split('.').next().unwrap_or(name);
    head.rfind("::").map_or(name, |i| &name[i + 2..])
}

/// FeatureData values by cbuffer name, raw numbers (the writer types them from the reflection): the
/// capture-11178 constants, overwritten by every number the settings have (C7: a missing file or key keeps the
/// constant). Names this CS build lacks are dropped at write time. `fields` = the reflected names of the program
/// the values are for ([`reflected_fields`]); `head` = the draw is a head (per-geometry skin tiling, P1-D).
pub fn feature_data(
    s: &CsSettings,
    fields: &BTreeSet<String>,
    features_on: bool,
    head: bool,
) -> Vec<(String, Vec<f64>)> {
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
            let field = renamed(renames, key);
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
    let ll_on = linear_lighting_on(s, fields.contains(OLD_LL), features_on);
    if fields.contains(NEW_LL) {
        // New LL: the CPU uploads π × pointLightMult and the shader no longer multiplies (dev LinearLighting.cpp:
        // 228-229; capture 11178: setting 1 → π). Directional stays raw: the capture shows 1.0.
        let point = f64::from(std::f32::consts::PI) * ll("pointLightMult").unwrap_or(1.0);
        put(
            &mut out,
            "linearLightingSettings.pointLightMult",
            vec![point],
        );
        if !ll_on {
            // From dev source only (LinearLighting.cpp:207-225, 2026-10-09), not verified against the installed
            // DLL: with LL inactive CS uploads every multiplier and fireEffectCurve as 1.0; the 1.9 shader reads
            // them ungated (ColorManagement.hlsli:61-137).
            for f in fields.iter().filter(|f| {
                f.strip_prefix("linearLightingSettings.")
                    .is_some_and(|m| m.ends_with("Mult") || m == "fireEffectCurve")
            }) {
                put(&mut out, f, vec![1.0]);
            }
        }
    } else if fields.contains(OLD_LL) {
        // Old LL: raw multipliers, the shader applies π itself (Color.hlsli:270-278). isDirLightLinear and
        // dirLightMult are engine-derived (advanced-skin LinearLighting.cpp:125-133); 0 would black out the sun
        // (1.8 Lighting.hlsl:2730). The flag is the LL switch (Color.hlsli:7).
        put(
            &mut out,
            "linearLightingSettings.pointLightMult",
            vec![ll("pointLightMult").unwrap_or(1.0)],
        );
        put(
            &mut out,
            "linearLightingSettings.enableLinearLighting",
            vec![f64::from(u8::from(ll_on))],
        );
        put(
            &mut out,
            "linearLightingSettings.isDirLightLinear",
            vec![0.0],
        );
        put(&mut out, "linearLightingSettings.dirLightMult", vec![1.0]);
        // b8 LLPerGeometry (Color.hlsli:10-14): 0 turns emissive black (Color.hlsli:283).
        put(&mut out, "emissiveMult", vec![1.0]);
    }
    put(
        &mut out,
        "linearLightingSettings.isMainOrLoadingMenu",
        vec![0.0],
    );
    // New packing iff the build reflects the per-geometry profile (AIO 1.9.0); always packed, so an old build with
    // no skin section still gets the un-multiplied tiling.
    let new_skin = fields.contains("skinPerGeometryProfile.skinParams");
    let empty = Map::new();
    let skin = match s.merged.get("Advanced Skin") {
        Some(Value::Object(m)) => m,
        _ => &empty,
    };
    let tiling = pack_skin(skin, new_skin, &mut out);
    // C2: the PS reads the profile from `SkinPerGeometry` (b7), not FeatureData (Skin.hlsli:21-31). No skin
    // extra / wetness / sk textures in the preview: skinMaterialFlags 0 here is the FeatureData/static default; the per-shape value (RFAOS bound at t71) is written per draw (material.rs); per-geometry wetness 0 (Skin.hlsli:254 returns 0).
    let profile: Vec<(String, Vec<f64>)> = out
        .iter()
        .filter_map(|(n, v)| {
            n.strip_prefix("skinData.")
                .map(|f| (format!("skinPerGeometryProfile.{f}"), v.clone()))
        })
        .collect();
    out.extend(profile);
    if head && new_skin {
        // CS: bodyTiling = 1 on a head (advanced-skin Renderer.cpp:263-264); FeatureData keeps the body value
        // (GetCommonBufferData passes false).
        if let Some((_, v)) = out
            .iter_mut()
            .find(|(n, _)| n == "skinPerGeometryProfile.skinDetailParams")
        {
            v[0] = tiling;
            v[1] = 1.0;
        }
    }
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

/// `"Advanced Skin"` → `skinData.*` (C5). The profile is `DefaultProfile`, else the section itself (CS's own
/// migration, advanced-skin Skin.cpp:22: 1.6/1.8 files are flat with the same key names). `new` (the build reflects
/// `skinPerGeometryProfile`): advanced-skin Renderer.cpp:260-268, the CPU pre-multiplies the tiling. Otherwise dev
/// Skin.cpp:267-273: the shader multiplies x·y itself (1.8 Lighting.hlsl:2049). Returns SkinDetailTiling (for the
/// head copy). A key the files lack keeps the capture-11178 component.
fn pack_skin(skin: &Map<String, Value>, new: bool, out: &mut [(String, Vec<f64>)]) -> f64 {
    let profile = match skin.get("DefaultProfile") {
        Some(Value::Object(d)) => d,
        _ => skin,
    };
    let p = |k: &str| profile.get(k).and_then(num);
    let top = |k: &str| skin.get(k).and_then(num);
    // Capture 11178: x = 20 = 10 × 2.
    let tiling = p("SkinDetailTiling").unwrap_or(10.0);
    let body = p("BodyTilingMultiplier").unwrap_or(2.0);
    let (detail_x, detail_on, sss_amount, wet_response) = if new {
        (
            tiling * body,
            p("EnableSkinDetail"),
            p("SSSAmount"),
            p("WetResponse"),
        )
    } else {
        // A missing EnableSkin falls back to the capture's skinParams.w before the AND.
        let skin_on = top("EnableSkin").or_else(|| {
            out.iter()
                .find(|(n, _)| n == "skinData.skinParams")
                .map(|(_, v)| v[3])
        });
        let on = p("EnableSkinDetail")
            .zip(skin_on)
            .map(|(d, e)| f64::from(u8::from(d != 0.0 && e != 0.0)));
        (tiling, on, Some(0.0), Some(0.0))
    };
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
        // x detail UV tiling (new: × body, :1673; old: the shader × y), y body tiling, z detail strength (:1688),
        // w detail on (:1682).
        (
            "skinDetailParams",
            [
                Some(detail_x),
                Some(body),
                p("SkinDetailStrength"),
                detail_on,
            ],
        ),
        // x·w translucency (Skin.hlsli:166), y width (Skin.hlsli:162), z amount (:3159; new only, old 0).
        (
            "sssParams",
            [p("Translucency"), p("sssWidth"), sss_amount, p("UseSSS")],
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
        // x, y roughness multipliers (:1972-1973), z specular with a skin-extra map (:1979), w wet response
        // (new only, :2216; old 0).
        (
            "physicalParams",
            [
                p("PhysicalMainRoughnessMultiplier"),
                p("PhysicalSecondRoughnessMultiplier"),
                p("PhysicalSpecularStrength"),
                wet_response,
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
    tiling
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
