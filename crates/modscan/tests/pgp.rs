use modscan::pgp::*;
#[path = "../../esp/tests/common/mod.rs"]
mod b;
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

// --- scan after PGPatcher (REPORT #5) ---
mod common;
use modscan::mo2::Instance;
use modscan::scan::{run, Report, Source};
use modscan::store::Store;
use std::sync::atomic::AtomicBool;

fn scan(d: &tempfile::TempDir) -> (Store, Report) {
    let mut store = Store::open(&d.path().join("scan.db")).unwrap();
    let src = Source::Mo2(Instance::load(d.path(), None).unwrap());
    let r = run(
        &src,
        &mut store,
        &AtomicBool::new(false),
        &|_| {},
        &|| vec![],
    )
    .unwrap();
    (store, r)
}

fn add_pgp_output(r: &std::path::Path, nif: &[u8]) {
    common::put(r, "mods/PGP/meshes/x.nif", nif);
    common::put(r, "mods/PGP/PGPatcher.esp", b"e");
    let crc = crc32fast::hash(nif);
    common::put(
        r,
        "mods/PGP/ParallaxGen_Diff.json",
        format!(r#"{{"meshes\\x.nif":{{"crc32original":{crc},"crc32patched":1}}}}"#).as_bytes(),
    );
}

fn check_of(d: &tempfile::TempDir, origin: &str) -> Option<String> {
    let (s, _) = scan(d);
    let a = s
        .mods()
        .unwrap()
        .into_iter()
        .find(|m| m.name == origin)
        .unwrap();
    s.nifs_of(a.id).unwrap()[0].derive_check.clone()
}

fn nif_bytes() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../nif/tests/fixtures/TestNifFile_Skinned_SE.nif"
    ))
    .unwrap()
}

#[test]
fn rescan_after_pgpatcher_output_verifies_loose_source() {
    let d = common::instance_fixture("+PGP\n+A\n");
    let n = nif_bytes();
    common::put(d.path(), "mods/A/meshes/x.nif", &n);
    std::fs::create_dir_all(d.path().join("mods/PGP")).unwrap();
    let (_, r1) = scan(&d);
    assert_eq!((r1.parsed, r1.reused), (1, 0));
    add_pgp_output(d.path(), &n);
    let (s, r2) = scan(&d);
    // The unchanged source has no stored CRC, so it is re-parsed (to get one); the PGP copy is new.
    assert_eq!((r2.parsed, r2.reused), (2, 0));
    drop(s);
    assert_eq!(check_of(&d, "A").as_deref(), Some("match"));
    // Third scan: the CRC is stored now, everything is reused.
    let (_, r3) = scan(&d);
    assert_eq!((r3.parsed, r3.reused), (0, 2));
}

#[test]
fn rescan_after_pgpatcher_output_verifies_bsa_source() {
    let d = common::instance_fixture("+PGP\n+A\n");
    let n = nif_bytes();
    common::put(
        d.path(),
        "mods/A/A.bsa",
        &common::bsa(&[("meshes", "x.nif", &n)]),
    );
    common::put(d.path(), "mods/A/A.esp", &esp_plugin());
    common::put(
        d.path(),
        "profiles/Default/plugins.txt",
        b"*A.esp
",
    );
    std::fs::create_dir_all(d.path().join("mods/PGP")).unwrap();
    scan(&d);
    add_pgp_output(d.path(), &n);
    assert_eq!(check_of(&d, "A.bsa").as_deref(), Some("match"));
}

fn esp_plugin() -> Vec<u8> {
    b::plugin(0, &[], &[])
}
