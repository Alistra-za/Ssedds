//! P1-C (03 §3, T-S1/T-S2): the user's CS settings files → merged settings, loaded features, change hash and
//! preview defines. File I/O on fixtures and tempdirs only: never the owner's real files, no GPU.
use render3d::cs_settings::{
    diffuse_curve, feature_data, preview_defines, read_settings, CsSettings, SettingsPaths,
};
use render3d::scene::{FEATURE_FLOATS, FEATURE_UINTS};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/cs_settings"
    ))
}

/// The owner-shaped fixture files (each optional) and a features folder.
fn paths(user: bool, default: bool, info: bool, features: Option<&Path>) -> SettingsPaths {
    let f = fixtures();
    SettingsPaths {
        user: user.then(|| f.join("SettingsUser.json")),
        default: default.then(|| f.join("SettingsDefault.json")),
        info_ini: info.then(|| f.join("Info.ini")),
        features_dir: features.map(Path::to_path_buf),
    }
}

/// A `Shaders/Features` folder with one CS feature ini per name.
fn features_dir(names: &[&str]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for n in names {
        std::fs::write(
            d.path().join(format!("{n}.ini")),
            "[Info]\nVersion = 1-0-0\n",
        )
        .unwrap();
    }
    d
}

fn names_set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|s| s.to_string()).collect()
}

/// Info.ini fixture sections with `Enabled = true` (UnifiedWater is false, `[Cache]` is not a feature).
const INFO_LOADED: &[&str] = &[
    "ExtendedMaterials",
    "ExtendedTranslucency",
    "HairSpecular",
    "LinearLighting",
    "Skin",
    "TruePBR",
    "VanillaFresnel",
];

/// T-S1: per key, User over Default; a key in neither is left out of `merged` (the built-in table fills it).
#[test]
fn settings_overlay_user_over_default_over_builtin() {
    let d = tempfile::tempdir().unwrap();
    let (user, default) = (
        d.path().join("SettingsUser.json"),
        d.path().join("SettingsDefault.json"),
    );
    std::fs::write(
        &default,
        r#"{"Linear Lighting": {"glowmapMult": 0.25, "ambientMult": 2.0}, "Version": "1-9-0-0"}"#,
    )
    .unwrap();
    std::fs::write(&user, r#"{"Linear Lighting": {"glowmapMult": 0.5}}"#).unwrap();
    let s = read_settings(&SettingsPaths {
        user: Some(user),
        default: Some(default),
        ..Default::default()
    });
    let ll = &s.merged["Linear Lighting"];
    assert_eq!(ll["glowmapMult"].as_f64(), Some(0.5), "User wins");
    assert_eq!(
        ll["ambientMult"].as_f64(),
        Some(2.0),
        "Default fills a key User lacks"
    );
    assert!(ll.get("directionalLightMult").is_none(), "in neither file");
    assert_eq!(s.merged["Version"].as_str(), Some("1-9-0-0"));
    assert!(
        !s.parse_failed && s.notes.is_empty(),
        "T-S1 notes: {:?}",
        s.notes
    );
    let fd = feature_data(&s);
    assert_eq!(value(&fd, "linearLightingSettings.glowmapMult"), [0.5]);
    assert_eq!(value(&fd, "linearLightingSettings.ambientMult"), [2.0]);
    assert_eq!(
        value(&fd, "linearLightingSettings.directionalLightMult"),
        [1.0],
        "built-in (capture 11178) when neither file has it"
    );
    // Owner shape: the nested DefaultProfile merges per key; User's "Disable at Boot" object replaces Default's null.
    let o = read_settings(&paths(true, true, false, None));
    let profile = &o.merged["Advanced Skin"]["DefaultProfile"];
    assert_eq!(profile["EnableSkinDetail"].as_bool(), Some(false));
    assert_eq!(profile["SkinDetailTiling"].as_f64(), Some(10.0));
    assert_eq!(
        o.merged["Disable at Boot"]["UnifiedWater"].as_bool(),
        Some(true)
    );
    let only_default = read_settings(&paths(false, true, false, None));
    assert!(only_default.merged["Disable at Boot"].is_null());
    assert_eq!(
        only_default.merged["Advanced Skin"]["DefaultProfile"]["EnableSkinDetail"].as_bool(),
        Some(true)
    );
}

/// T-S2: Info.ini decides; feature inis it does not list (Upscaling) or disables (UnifiedWater) do not count.
#[test]
fn info_ini_wins() {
    let f = features_dir(&["HairSpecular", "Skin", "UnifiedWater", "Upscaling"]);
    let s = read_settings(&paths(true, true, true, Some(f.path())));
    assert_eq!(s.loaded, names_set(INFO_LOADED));
    assert!(!s.loaded.contains("Cache"));
    assert!(s.notes.is_empty(), "{:?}", s.notes);
}

/// T-S2: no Info.ini → `Features/*.ini` minus "Disable at Boot" (User's object; Default's null disables nothing).
#[test]
fn fallback_to_feature_inis_minus_disable_at_boot() {
    let f = features_dir(&["HairSpecular", "Skin", "UnifiedWater", "VanillaFresnel"]);
    let s = read_settings(&paths(true, true, false, Some(f.path())));
    assert_eq!(
        s.loaded,
        names_set(&["HairSpecular", "Skin", "VanillaFresnel"])
    );
    assert!(
        s.notes.iter().any(|n| n.contains("Info.ini")),
        "{:?}",
        s.notes
    );
    assert!(!s.parse_failed);
    let d = read_settings(&paths(false, true, false, Some(f.path())));
    assert_eq!(
        d.loaded,
        names_set(&["HairSpecular", "Skin", "UnifiedWater", "VanillaFresnel"])
    );
}

/// The owner's Info.ini starts with a UTF-8 BOM before `[Cache]`; a BOM before a feature section must not hide it.
#[test]
fn info_ini_bom_and_cache_section() {
    let bytes = std::fs::read(fixtures().join("Info.ini")).unwrap();
    assert_eq!(
        &bytes[..3],
        &[0xEF, 0xBB, 0xBF],
        "the fixture must keep its BOM"
    );
    let s = read_settings(&paths(false, false, true, None));
    assert_eq!(s.loaded, names_set(INFO_LOADED));
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("Info.ini");
    std::fs::write(
        &p,
        "\u{feff}[TruePBR]\r\nEnabled = true\r\n\r\n[Skin]\r\nEnabled = false\r\n",
    )
    .unwrap();
    let s = read_settings(&SettingsPaths {
        info_ini: Some(p),
        ..Default::default()
    });
    assert_eq!(s.loaded, names_set(&["TruePBR"]));
}

/// C10: same bytes → same hash; any byte, or present-but-empty vs absent → a different hash; nothing found =
/// `CsSettings::default()` exactly (so the GUI sends nothing).
#[test]
fn hash_is_stable_and_changes_with_any_byte() {
    let d = tempfile::tempdir().unwrap();
    for n in ["SettingsUser.json", "SettingsDefault.json", "Info.ini"] {
        std::fs::copy(fixtures().join(n), d.path().join(n)).unwrap();
    }
    let info = d.path().join("Info.ini");
    let p = SettingsPaths {
        user: Some(d.path().join("SettingsUser.json")),
        default: Some(d.path().join("SettingsDefault.json")),
        info_ini: Some(info.clone()),
        features_dir: None,
    };
    let a = read_settings(&p);
    assert_eq!(a.hash.len(), 64);
    assert_eq!(read_settings(&p).hash, a.hash);
    let mut b = std::fs::read(&info).unwrap();
    *b.last_mut().unwrap() ^= 1;
    std::fs::write(&info, &b).unwrap();
    assert_ne!(read_settings(&p).hash, a.hash);
    std::fs::write(&info, b"").unwrap();
    let empty = read_settings(&p).hash;
    std::fs::remove_file(&info).unwrap();
    assert_ne!(read_settings(&p).hash, empty);
    assert_eq!(
        read_settings(&SettingsPaths::default()),
        CsSettings::default()
    );
}

/// C10: a file CS is half-way through writing → a note naming it, `parse_failed`, and the bad file contributes
/// nothing (no value from a previous read either); the other file still applies; its bytes still change the hash.
#[test]
fn malformed_json_notes_and_falls_back() {
    let d = tempfile::tempdir().unwrap();
    let user = d.path().join("SettingsUser.json");
    std::fs::write(&user, "{\"Linear Lighting\": {\"enableLinearLighting\": 0,").unwrap();
    let s = read_settings(&SettingsPaths {
        user: Some(user),
        default: Some(fixtures().join("SettingsDefault.json")),
        ..Default::default()
    });
    assert!(s.parse_failed);
    assert!(
        s.notes.iter().any(|n| n.contains("SettingsUser.json")),
        "{:?}",
        s.notes
    );
    assert!(
        s.linear_lighting,
        "the bad file's enableLinearLighting 0 is not applied"
    );
    assert_eq!(
        s.merged["Advanced Skin"]["DefaultProfile"]["EnableSkinDetail"].as_bool(),
        Some(true)
    );
    let good = read_settings(&paths(false, true, false, None));
    assert_ne!(s.hash, good.hash);
}

/// C8: defines from loaded features and their own switches; LL from the setting; off toggle → none.
#[test]
fn preview_defines_follow_loaded_and_switches() {
    let s = read_settings(&paths(true, true, true, None));
    assert_eq!(
        preview_defines(&s, true),
        [
            "CS_HAIR",
            "CS_SKIN",
            "ENABLE_LL",
            "EXTENDED_MATERIALS",
            "EXTENDED_TRANSLUCENCY",
            "VANILLA_FRESNEL"
        ]
    );
    assert!(preview_defines(&s, false).is_empty());
    // No files: exactly today's default (Part B's set_features_on relies on it).
    assert_eq!(
        preview_defines(&CsSettings::default(), true),
        render3d::DEFAULT_FEATURES
    );
    let mut off = s.clone();
    off.merged.get_mut("Hair Specular").unwrap()["Enabled"] = 0.into();
    off.merged.get_mut("Vanilla Fresnel").unwrap()["Enable"] = 0.into();
    off.linear_lighting = false;
    assert_eq!(
        preview_defines(&off, true),
        ["CS_SKIN", "EXTENDED_MATERIALS", "EXTENDED_TRANSLUCENCY"]
    );
    let d = tempfile::tempdir().unwrap();
    let user = d.path().join("SettingsUser.json");
    std::fs::write(&user, r#"{"Linear Lighting": {"enableLinearLighting": 0}}"#).unwrap();
    let ll_off = read_settings(&SettingsPaths {
        user: Some(user),
        ..Default::default()
    });
    assert!(!ll_off.linear_lighting);
    assert!(preview_defines(&ll_off, true).is_empty());
}

fn value<'a>(fd: &'a [(String, Vec<f64>)], name: &str) -> &'a [f64] {
    &fd.iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("{name} missing"))
        .1
}

fn bits(v: &[f64]) -> Vec<u32> {
    v.iter().map(|x| (*x as f32).to_bits()).collect()
}

/// Every FEATURE_FLOATS / FEATURE_UINTS value, bit-exact as written to the cbuffer.
fn assert_constants(fd: &[(String, Vec<f64>)]) {
    for (n, v) in FEATURE_FLOATS {
        let want: Vec<u32> = v.iter().map(|x| x.to_bits()).collect();
        assert_eq!(bits(value(fd, n)), want, "{n}");
    }
    for (n, u) in FEATURE_UINTS {
        assert_eq!(value(fd, n), [f64::from(*u)], "{n}");
    }
}

/// T-S3 (C4): the derived diffuseCurve from the owner's diffuseGamma / mid / white reflectance.
#[test]
fn diffuse_curve_derivation() {
    let c = diffuse_curve(2.200000047683716, 0.28717461228370667, 1.0);
    assert!((c - 1.448_227_7).abs() < 1e-6, "{c}");
}

/// T-S3: the owner-shaped files reproduce every capture-11178 value; pointLightMult stays π although the file says 1.
#[test]
fn owner_settings_reproduce_capture_values() {
    let s = read_settings(&paths(true, true, true, None));
    assert_eq!(
        s.merged["Linear Lighting"]["pointLightMult"].as_f64(),
        Some(1.0)
    );
    let fd = feature_data(&s);
    assert_constants(&fd);
    assert_eq!(
        bits(value(&fd, "linearLightingSettings.pointLightMult")),
        [std::f32::consts::PI.to_bits()]
    );
}

/// No files: exactly today's constants, plus the C2 per-geometry copy and zeros.
#[test]
fn default_settings_reproduce_todays_constants() {
    let fd = feature_data(&CsSettings::default());
    assert_constants(&fd);
    for f in [
        "skinParams",
        "skinParams2",
        "skinDetailParams",
        "sssParams",
        "fuzzParams",
        "physicalParams",
        "wetParams",
    ] {
        assert_eq!(
            value(&fd, &format!("skinPerGeometryProfile.{f}")),
            value(&fd, &format!("skinData.{f}")),
            "{f}"
        );
    }
    assert_eq!(value(&fd, "skinPerGeometry"), [0.0; 4]);
    assert_eq!(value(&fd, "skinMaterialFlags"), [0.0; 4]);
    assert_eq!(
        value(&fd, "linearLightingSettings.isMainOrLoadingMenu"),
        [0.0]
    );
}

/// C5: section → struct prefix with the renamed keys; skin packing from the profile.
#[test]
fn owner_section_keys_map_to_struct_fields() {
    let fd = feature_data(&read_settings(&paths(true, true, false, None)));
    assert_eq!(
        value(&fd, "extendedTranslucencySettings.MaterialModel"),
        [3.0]
    );
    assert_eq!(
        bits(value(&fd, "extendedTranslucencySettings.Reduction")),
        [0.15f32.to_bits()]
    );
    assert_eq!(value(&fd, "extendedTranslucencySettings.Strength"), [0.0]);
    assert_eq!(
        value(&fd, "extendedMaterialSettings.EnableTerrainParallax"),
        [0.0]
    );
    assert_eq!(value(&fd, "extendedMaterialSettings.EnableParallax"), [1.0]);
    assert!(!fd.iter().any(|(n, _)| n.ends_with("AlphaMode")));
    assert_eq!(
        value(&fd, "skinData.skinDetailParams"),
        [20.0, 2.0, 0.25, 0.0]
    );
    // Default alone has EnableSkinDetail true: detail on.
    let d = feature_data(&read_settings(&paths(false, true, false, None)));
    assert_eq!(value(&d, "skinData.skinDetailParams")[3], 1.0);
}

#[test]
fn an_unreadable_settings_file_is_parse_failed_with_a_note() {
    // A directory where the file should be: a read error that is not NotFound.
    let d = tempfile::tempdir().unwrap();
    let mut p = paths(false, true, true, None);
    p.user = Some(d.path().to_path_buf());
    let s = read_settings(&p);
    assert!(s.parse_failed);
    assert!(
        s.notes
            .iter()
            .any(|n| n.contains(&d.path().display().to_string())),
        "{:?}",
        s.notes
    );
    // Info.ini unreadable stays a note only.
    let mut p = paths(true, true, false, None);
    p.info_ini = Some(d.path().to_path_buf());
    assert!(!read_settings(&p).parse_failed);
}

/// Task 8 fix round 2: the hash covers the loaded feature set, so features found only through the Features
/// folder (no settings file) differ from `CsSettings::default()`.
#[test]
fn the_hash_covers_the_loaded_features() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("Skin.ini"), "[Info]\n").unwrap();
    let s = read_settings(&SettingsPaths {
        features_dir: Some(d.path().to_path_buf()),
        ..Default::default()
    });
    assert!(s.loaded.contains("Skin"));
    assert_ne!(s.hash, CsSettings::default().hash);
}
