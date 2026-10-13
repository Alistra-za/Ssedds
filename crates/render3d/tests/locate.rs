use render3d::locate::*;
use std::fs;
use std::path::{Path, PathBuf};

fn cs(root: &std::path::Path, body: &str) {
    fs::create_dir_all(root.join("Shaders/Common")).unwrap();
    fs::write(root.join("Shaders/Lighting.hlsl"), body).unwrap();
    fs::write(root.join("Shaders/Common/Color.hlsli"), "// c").unwrap();
}

#[test]
fn hash_changes_with_content_only() {
    let d = tempfile::tempdir().unwrap();
    cs(d.path(), "a");
    let a = fingerprint(&d.path().join("Shaders"), Some("CS".into())).unwrap();
    let b = fingerprint(&d.path().join("Shaders"), Some("CS".into())).unwrap();
    assert_eq!(a.hash, b.hash);
    fs::write(d.path().join("Shaders/Lighting.hlsl"), "b").unwrap();
    let c = fingerprint(&d.path().join("Shaders"), Some("CS".into())).unwrap();
    assert_ne!(a.hash, c.hash);
    assert!(matches!(compare(Some(&a), Some(c)), Change::Updated));
}

#[test]
fn renamed_root_triggers_popup() {
    let d = tempfile::tempdir().unwrap();
    let old = d.path().join("CommunityShaders_AIO-2026-10-03T06-46Z");
    cs(&old, "a");
    let saved = fingerprint(
        &old.join("Shaders"),
        Some("CommunityShaders_AIO-2026-10-03T06-46Z".into()),
    )
    .unwrap();
    let new = d.path().join("CommunityShaders_AIO-2026-11-01T00-00Z");
    fs::rename(&old, &new).unwrap();
    let found = find_in_mods(&[(
        "CommunityShaders_AIO-2026-11-01T00-00Z".into(),
        new.clone(),
        5,
    )])
    .unwrap();
    let fp = fingerprint(&found.1.join("Shaders"), Some(found.0)).unwrap();
    assert!(matches!(
        compare(Some(&saved), Some(fp)),
        Change::Moved { .. }
    ));
}

#[test]
fn missing_root_is_not_found() {
    let d = tempfile::tempdir().unwrap();
    cs(d.path(), "a");
    let saved = fingerprint(&d.path().join("Shaders"), None).unwrap();
    assert!(find_in_mods(&[("X".into(), d.path().join("nope"), 1)]).is_none());
    assert!(matches!(
        compare(Some(&saved), None),
        Change::NotFound { old: Some(_) }
    ));
}

#[test]
fn highest_priority_provider_wins() {
    let d = tempfile::tempdir().unwrap();
    cs(&d.path().join("A"), "a");
    cs(&d.path().join("B"), "b");
    let got = find_in_mods(&[
        ("A".into(), d.path().join("A"), 1),
        ("B".into(), d.path().join("B"), 9),
    ])
    .unwrap();
    assert_eq!(got.0, "B");
}

#[test]
fn nonexistent_root_is_error() {
    let d = tempfile::tempdir().unwrap();
    assert!(fingerprint(&d.path().join("nope"), None).is_err());
    let f = d.path().join("file.txt");
    fs::write(&f, "x").unwrap();
    assert!(fingerprint(&f, None).is_err());
}

#[test]
fn same_folder_two_spellings_is_not_moved() {
    let d = tempfile::tempdir().unwrap();
    cs(d.path(), "a");
    let root = d.path().join("Shaders");
    let saved = fingerprint(&root, Some("CS".into())).unwrap();
    let alt = std::path::PathBuf::from(
        root.to_string_lossy()
            .to_ascii_uppercase()
            .replace('\\', "/"),
    );
    let mut found = fingerprint(&root, Some("cs".into())).unwrap();
    found.root = alt;
    assert!(matches!(compare(Some(&saved), Some(found)), Change::Same));
}

const USER: &str = "SKSE/Plugins/CommunityShaders/SettingsUser.json";
const DEFAULT: &str = "SKSE/Plugins/CommunityShaders/SettingsDefault.json";
const INFO: &str = "ShaderCache/Info.ini";

fn put(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, "{}").unwrap();
}

/// An MO2-shaped tree with origins as `modscan::mo2::loose_origins` reports them: game Data at -1, mods 0..,
/// overwrite at `i32::MAX`. SettingsUser.json is in Data, the CS mod and overwrite; SettingsDefault.json only
/// in the CS mod; no Info.ini anywhere. `Other` has no folder on disk.
fn mo2(d: &Path) -> Vec<(String, PathBuf, i32)> {
    let data = d.join("Data");
    let cs_mod = d.join("mods/CommunityShaders");
    let ow = d.join("overwrite");
    cs(&cs_mod, "a");
    for root in [&data, &cs_mod, &ow] {
        put(&root.join(USER));
    }
    put(&cs_mod.join(DEFAULT));
    vec![
        ("Data".into(), data, -1),
        ("CommunityShaders".into(), cs_mod, 0),
        ("Other".into(), d.join("mods/Other"), 1),
        ("overwrite".into(), ow, i32::MAX),
    ]
}

/// 03 T-S4
#[test]
fn mo2_overwrite_wins() {
    let d = tempfile::tempdir().unwrap();
    let o = mo2(d.path());
    assert_eq!(
        find_file(&o, USER),
        Some(("overwrite".into(), d.path().join("overwrite").join(USER)))
    );
    assert_eq!(
        find_file(&o, DEFAULT),
        Some((
            "CommunityShaders".into(),
            d.path().join("mods/CommunityShaders").join(DEFAULT)
        )),
        "a file only in a mod comes from that mod"
    );
    assert_eq!(find_file(&o, INFO), None);
    put(&d.path().join("Data").join(INFO));
    assert_eq!(
        find_file(&o, INFO),
        Some(("Data".into(), d.path().join("Data").join(INFO))),
        "game Data is the last resort"
    );
}

#[test]
fn settings_paths_with_origins_use_them() {
    let d = tempfile::tempdir().unwrap();
    let o = mo2(d.path());
    let shaders = d.path().join("mods/CommunityShaders/Shaders");
    fs::create_dir_all(shaders.join("Features")).unwrap();
    let p = settings_paths(Some(&o), &shaders);
    assert_eq!(p.user, Some(d.path().join("overwrite").join(USER)));
    assert_eq!(
        p.default,
        Some(d.path().join("mods/CommunityShaders").join(DEFAULT))
    );
    assert_eq!(p.info_ini, None);
    assert_eq!(p.features_dir, Some(shaders.join("Features")));
}

/// Folder target or "Choose folder…": the Data folder is the parent of the Shaders root.
#[test]
fn settings_paths_without_origins_look_in_the_parent_of_the_shaders_root() {
    let d = tempfile::tempdir().unwrap();
    let data = d.path().join("Data");
    cs(&data, "a");
    put(&data.join(USER));
    put(&data.join(INFO));
    let p = settings_paths(None, &data.join("Shaders"));
    assert_eq!(p.user, Some(data.join(USER)));
    assert_eq!(p.default, None);
    assert_eq!(p.info_ini, Some(data.join(INFO)));
    assert_eq!(p.features_dir, None, "no Shaders/Features folder");
    // An empty origin list (MO2 failed to load) is the same as none.
    assert_eq!(
        settings_paths(Some(&[]), &data.join("Shaders")).user,
        Some(data.join(USER))
    );
    // A chosen CS mod folder holds no settings (the owner's CS mod has none, 01 §1): nothing found, no error.
    let m = d.path().join("CS");
    cs(&m, "a");
    fs::create_dir_all(m.join("Shaders/Features")).unwrap();
    let p = settings_paths(None, &m.join("Shaders"));
    assert_eq!((p.user, p.default, p.info_ini), (None, None, None));
    assert_eq!(p.features_dir, Some(m.join("Shaders/Features")));
}
