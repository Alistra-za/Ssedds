use render3d::locate::*;
use std::fs;

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
