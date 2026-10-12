use modscan::pgp::*;
use std::fs;

fn mk(root: &std::path::Path, rel: &str, bytes: &[u8]) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, bytes).unwrap();
}

#[test]
fn process_gate() {
    assert!(running(&["explorer.exe".into(), "pgpatcher.exe".into()]));
    assert!(running(&["pgtools.exe".into()]));
    assert!(!running(&["parallaxgen.exe.bak".into()]));
}

#[test]
fn signatures() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    assert_eq!(signature(r), Signature::None);
    mk(r, "meshes/x.nif", b"n");
    mk(r, "PGPatcher.esp", b"e");
    assert_eq!(signature(r), Signature::Incomplete);
    mk(r, "ParallaxGen_Diff.json", b"{}");
    assert_eq!(signature(r), Signature::Strong);
    let d2 = tempfile::tempdir().unwrap();
    mk(d2.path(), "meshes/_pgpatcher_dups/1/armor/x.nif", b"n");
    assert_eq!(signature(d2.path()), Signature::Medium);
    let d3 = tempfile::tempdir().unwrap();
    mk(d3.path(), "PG_12.esp", b"e");
    mk(d3.path(), "ParallaxGen_Diff.json", b"{}");
    assert_eq!(signature(d3.path()), Signature::Strong);
}

#[test]
fn diff_json_keys_are_canonical() {
    let d = tempfile::tempdir().unwrap();
    mk(
        d.path(),
        "ParallaxGen_Diff.json",
        br#"{"meshes\\Armor\\Iron\\Cuirass.nif":{"crc32original":305419896,"crc32patched":1}}"#,
    );
    let diff = read_diff(d.path()).unwrap();
    assert_eq!(diff.crc_orig["meshes/armor/iron/cuirass.nif"], 0x1234_5678);
    assert!(is_derived_path(
        "meshes/armor/iron/cuirass.nif",
        Some(&diff)
    ));
    assert!(!is_derived_path("meshes/other.nif", Some(&diff)));
    assert!(is_derived_path("meshes/_pgpatcher_dups/3/other.nif", None));
}

#[test]
fn dup_paths_map_to_source() {
    assert_eq!(
        dup_source("meshes/_pgpatcher_dups/12/armor/x.nif").as_deref(),
        Some("meshes/armor/x.nif")
    );
    assert_eq!(dup_source("meshes/armor/x.nif"), None);
}

#[test]
fn staleness_uses_crc_not_mtime() {
    assert_eq!(check(Some(7), Some(7), &[]), DeriveCheck::Match);
    assert_eq!(check(Some(7), Some(8), &[]), DeriveCheck::StaleChanged);
    assert_eq!(
        check(Some(7), Some(8), &[9, 7]),
        DeriveCheck::StaleReordered
    );
    assert_eq!(check(Some(7), None, &[]), DeriveCheck::Unverified);
    assert_eq!(check(None, Some(8), &[]), DeriveCheck::UnknownSource);
}

#[test]
fn incomplete_needs_a_pgpatcher_marker_not_any_esp() {
    let d = tempfile::tempdir().unwrap();
    mk(d.path(), "meshes/x.nif", b"n");
    mk(d.path(), "Foo.esp", b"e");
    assert_eq!(signature(d.path()), Signature::None);
    let d2 = tempfile::tempdir().unwrap();
    mk(d2.path(), "meshes/x.nif", b"n");
    mk(d2.path(), "PGPatcher_UpdateCache.bin", b"c");
    assert_eq!(signature(d2.path()), Signature::Incomplete);
    let d3 = tempfile::tempdir().unwrap();
    mk(d3.path(), "meshes/x.nif", b"n");
    mk(d3.path(), "PG_3.esp", b"e");
    assert_eq!(signature(d3.path()), Signature::Incomplete);
}
