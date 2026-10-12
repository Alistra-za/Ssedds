//! P1-C (03 §3, T-S1/T-S2): the user's CS settings files → merged settings, loaded features, change hash and
//! preview defines. File I/O on fixtures and tempdirs only: never the owner's real files, no GPU.
use render3d::cs_settings::{preview_defines, read_settings, CsSettings, SettingsPaths};
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
