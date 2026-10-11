// crates/modscan/tests/scan.rs
mod common;
use common::instance_fixture;
use modscan::mo2::Instance;
use modscan::scan::{run, Source};
use modscan::store::Store;
use std::sync::atomic::AtomicBool;

fn put(root: &std::path::Path, rel: &str, bytes: Option<&[u8]>) {
    let fixture = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../nif/tests/fixtures/TestNifFile_Skinned_SE.nif"
    ))
    .unwrap();
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes.unwrap_or(&fixture)).unwrap();
}

fn go(d: &tempfile::TempDir, procs: Vec<String>) -> (Store, modscan::scan::Report) {
    let inst = Instance::load(d.path(), None).unwrap();
    let mut store = Store::open(&d.path().join("scan.db")).unwrap();
    let r = run(
        &Source::Mo2(inst),
        &mut store,
        &AtomicBool::new(false),
        &|_| {},
        &move || procs.clone(),
    )
    .unwrap();
    (store, r)
}

#[test]
fn winner_and_source_with_pgpatcher_output() {
    let d = instance_fixture("+PGP\n+A\n");
    std::fs::create_dir_all(d.path().join("mods/PGP")).unwrap();
    put(d.path(), "mods/A/meshes/x.nif", None);
    let a_bytes = std::fs::read(d.path().join("mods/A/meshes/x.nif")).unwrap();
    put(d.path(), "mods/PGP/meshes/x.nif", None);
    put(d.path(), "mods/PGP/PGPatcher.esp", Some(b"e"));
    let crc = crc32fast::hash(&a_bytes);
    put(
        d.path(),
        "mods/PGP/ParallaxGen_Diff.json",
        Some(
            format!(r#"{{"meshes\\x.nif":{{"crc32original":{crc},"crc32patched":1}}}}"#).as_bytes(),
        ),
    );
    let (store, r) = go(&d, vec![]);
    assert!(!r.tainted);
    assert_eq!(r.pgp.len(), 1);
    let mods = store.mods();
    let a = mods.iter().find(|m| m.name == "A").unwrap();
    let nifs = store.nifs_of(a.id);
    assert_eq!(nifs[0].status_label(), "Patched by PGPatcher (in game)");
    assert_eq!(nifs[0].derive_check.as_deref(), Some("match"));
}

#[test]
fn stale_when_source_changed_even_with_older_mtime() {
    let d = instance_fixture("+PGP\n+A\n");
    put(d.path(), "mods/A/meshes/x.nif", None);
    put(d.path(), "mods/PGP/meshes/x.nif", None);
    put(d.path(), "mods/PGP/PGPatcher.esp", Some(b"e"));
    put(
        d.path(),
        "mods/PGP/ParallaxGen_Diff.json",
        Some(br#"{"meshes\\x.nif":{"crc32original":1,"crc32patched":2}}"#),
    );
    let (store, _) = go(&d, vec![]);
    let a = store.mods().into_iter().find(|m| m.name == "A").unwrap();
    assert_eq!(
        store.nifs_of(a.id)[0].derive_check.as_deref(),
        Some("stale_changed")
    );
}

#[test]
fn pgp_running_blocks_scan() {
    let d = instance_fixture("+A\n");
    put(d.path(), "mods/A/meshes/x.nif", None);
    let inst = Instance::load(d.path(), None).unwrap();
    let mut store = Store::open(&d.path().join("scan.db")).unwrap();
    let r = run(
        &Source::Mo2(inst),
        &mut store,
        &AtomicBool::new(false),
        &|_| {},
        &|| vec!["pgpatcher.exe".into()],
    );
    assert!(matches!(r, Err(modscan::ScanError::PgpRunning)));
}

#[test]
fn pgp_started_mid_scan_taints_result() {
    let d = instance_fixture("+PGP\n+A\n");
    put(d.path(), "mods/A/meshes/x.nif", None);
    put(d.path(), "mods/PGP/meshes/x.nif", None);
    put(d.path(), "mods/PGP/PGPatcher.esp", Some(b"e"));
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let inst = Instance::load(d.path(), None).unwrap();
    let mut store = Store::open(&d.path().join("scan.db")).unwrap();
    let r = run(
        &Source::Mo2(inst),
        &mut store,
        &AtomicBool::new(false),
        &|_| {},
        &|| {
            if calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                vec![]
            } else {
                vec!["pgtools.exe".into()]
            }
        },
    )
    .unwrap();
    assert!(r.tainted);
    let pgp = store.mods().into_iter().find(|m| m.name == "PGP").unwrap();
    assert!(
        store.nifs_of(pgp.id).is_empty(),
        "derived rows from a tainted run must not be committed"
    );
}

#[test]
fn not_skyrim_and_le_are_counted_not_fatal() {
    let d = instance_fixture("+A\n");
    put(
        d.path(),
        "mods/A/meshes/fo4.nif",
        Some(
            &std::fs::read(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../nif/tests/fixtures/TestNifFile_Skinned_FO4.nif"
            ))
            .unwrap(),
        ),
    );
    put(
        d.path(),
        "mods/A/meshes/le.nif",
        Some(
            &std::fs::read(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../nif/tests/fixtures/TestNifFile_Animated_LE.nif"
            ))
            .unwrap(),
        ),
    );
    put(d.path(), "mods/A/meshes/empty.nif", Some(b""));
    let (_, r) = go(&d, vec![]);
    assert_eq!(r.not_skyrim, 1);
    assert_eq!(r.le_meshes, 1);
    assert_eq!(r.problems.len(), 1);
}

#[test]
fn rescan_reuses_unchanged_copies() {
    let d = instance_fixture("+A\n");
    put(d.path(), "mods/A/meshes/x.nif", None);
    let (_, r1) = go(&d, vec![]);
    let (_, r2) = go(&d, vec![]);
    assert_eq!((r1.parsed, r2.parsed, r2.reused), (1, 0, 1));
}

// ---- additional coverage (beyond the brief) ----

fn mod_id(store: &Store, name: &str) -> i64 {
    store
        .mods()
        .into_iter()
        .find(|m| m.name == name)
        .unwrap()
        .id
}

#[test]
fn stale_reordered_when_diff_matches_a_deeper_copy() {
    // 05 §5.4 case 3: B inserted between PGP and A; the diff CRC matches A (deeper), not B (the source).
    let d = instance_fixture("+PGP\n+B\n+A\n");
    put(d.path(), "mods/A/meshes/x.nif", None);
    let a = std::fs::read(d.path().join("mods/A/meshes/x.nif")).unwrap();
    let mut b = a.clone();
    b.push(0); // different bytes (a file-length failure, still a copy)
    put(d.path(), "mods/B/meshes/x.nif", Some(&b));
    put(d.path(), "mods/PGP/meshes/x.nif", None);
    put(d.path(), "mods/PGP/PGPatcher.esp", Some(b"e"));
    let crc = crc32fast::hash(&a);
    put(
        d.path(),
        "mods/PGP/ParallaxGen_Diff.json",
        Some(
            format!(r#"{{"meshes\\x.nif":{{"crc32original":{crc},"crc32patched":1}}}}"#).as_bytes(),
        ),
    );
    let (store, _) = go(&d, vec![]);
    let bn = &store.nifs_of(mod_id(&store, "B"))[0];
    assert!(bn.patched_in_game);
    assert_eq!(bn.derive_check.as_deref(), Some("stale_reordered"));
    let an = &store.nifs_of(mod_id(&store, "A"))[0];
    assert_eq!(an.status_label(), "Overridden");
    assert_eq!(an.overridden_by.as_deref(), Some("B"));
    let pn = &store.nifs_of(mod_id(&store, "PGP"))[0];
    assert!(pn.wins && pn.derived);
    assert_eq!(pn.n_copies, 3);
}

#[test]
fn dups_are_dup_of_and_overwrite_only_derives_listed_paths() {
    let d = instance_fixture("+A\n");
    put(d.path(), "mods/A/meshes/x.nif", None);
    put(d.path(), "mods/A/meshes/y.nif", None);
    put(d.path(), "overwrite/meshes/_pgpatcher_dups/1/x.nif", None);
    put(d.path(), "overwrite/meshes/y.nif", None); // ordinary overwrite file
    put(d.path(), "overwrite/PGPatcher.esp", Some(b"e"));
    let (store, r) = go(&d, vec![]);
    assert_eq!(r.pgp.len(), 1);
    assert_eq!(r.pgp[0].state, modscan::store::DerivedState::InOverwrite);
    let ow = store
        .mods()
        .into_iter()
        .find(|m| m.name == "overwrite")
        .unwrap();
    assert!(!ow.derived);
    let rows = store.nifs_of(ow.id);
    let dup = rows
        .iter()
        .find(|n| n.path.contains("_pgpatcher_dups"))
        .unwrap();
    assert!(dup.derived);
    assert_eq!(dup.derive_check.as_deref(), Some("dup_of"));
    let y = rows.iter().find(|n| n.path == "meshes/y.nif").unwrap();
    assert!(!y.derived && y.wins);
    let ay = store
        .nifs_of(mod_id(&store, "A"))
        .into_iter()
        .find(|n| n.path == "meshes/y.nif")
        .unwrap();
    assert_eq!(ay.overridden_by.as_deref(), Some("overwrite"));
}

#[test]
fn separators_are_listed_above_their_group() {
    let d = instance_fixture("+B\n-Sep_separator\n+A\n");
    let (store, _) = go(&d, vec![]);
    let names: Vec<(String, i32)> = store.mods().into_iter().map(|m| (m.name, m.kind)).collect();
    let pos = |n: &str| names.iter().position(|m| m.0 == n).unwrap();
    assert!(pos("B") < pos("Sep_separator") && pos("Sep_separator") < pos("A"));
    assert_eq!(names[pos("Sep_separator")].1, 3);
}

#[test]
fn shapes_textures_and_texset_dedup() {
    let d = instance_fixture("+B\n+A\n");
    put(d.path(), "mods/A/meshes/x.nif", None);
    put(d.path(), "mods/B/meshes/x.nif", None);
    let (store, _) = go(&d, vec![]);
    let a = &store.nifs_of(mod_id(&store, "A"))[0];
    let shapes = store.shapes_of(a.copy_id);
    assert!(!shapes.is_empty());
    let lit = shapes
        .iter()
        .find(|s| s.shader_kind == 1)
        .expect("a lighting shape");
    assert!(lit
        .params
        .as_deref()
        .is_some_and(|p| p.contains("uv_scale")));
    let tex = lit
        .textures
        .iter()
        .flatten()
        .next()
        .expect("a texture slot")
        .clone();
    let users = store.users_of_texture(&tex);
    assert!(users.iter().any(|(n, _, k)| n == "meshes/x.nif" && *k == 1));
    // Two identical copies share texture sets, including sets with empty slots.
    let sets: i64 = store
        .conn()
        .query_row("SELECT count(*) FROM texset", [], |r| r.get(0))
        .unwrap();
    let used: i64 = store
        .conn()
        .query_row("SELECT count(DISTINCT texset_id) FROM shape", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(sets, used);
}

#[test]
fn removed_files_are_deleted_and_cancel_stops() {
    let d = instance_fixture("+A\n");
    put(d.path(), "mods/A/meshes/x.nif", None);
    put(d.path(), "mods/A/meshes/y.nif", None);
    go(&d, vec![]);
    std::fs::remove_file(d.path().join("mods/A/meshes/y.nif")).unwrap();
    let (store, r) = go(&d, vec![]);
    assert_eq!((r.nifs, r.reused), (1, 1));
    assert_eq!(store.nifs_of(mod_id(&store, "A")).len(), 1);
    put(d.path(), "mods/A/meshes/z.nif", None);
    let inst = Instance::load(d.path(), None).unwrap();
    let mut store = Store::open(&d.path().join("scan.db")).unwrap();
    let r = run(
        &Source::Mo2(inst),
        &mut store,
        &AtomicBool::new(true),
        &|_| {},
        &|| vec![],
    );
    assert!(matches!(r, Err(modscan::ScanError::Cancelled)));
}

/// A read failure (here: the file held open with no sharing) is not cached; the next scan re-reads it.
#[cfg(windows)]
#[test]
fn read_failure_is_retried_next_scan() {
    use std::os::windows::fs::OpenOptionsExt;
    let d = instance_fixture("+A\n");
    put(d.path(), "mods/A/meshes/x.nif", None);
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(d.path().join("mods/A/meshes/x.nif"))
        .unwrap();
    let (_, r1) = go(&d, vec![]);
    assert_eq!((r1.parsed, r1.problems.len()), (1, 1), "{:?}", r1.problems);
    drop(lock);
    let (store, r2) = go(&d, vec![]);
    assert_eq!((r2.parsed, r2.reused, r2.problems.len()), (1, 0, 0));
    let a = &store.nifs_of(mod_id(&store, "A"))[0];
    assert!(a.problem.is_none());
    assert!(!store.shapes_of(a.copy_id).is_empty());
}

#[test]
fn folder_source_scans_one_origin() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "Loose/meshes/x.nif", None);
    let mut store = Store::open(&d.path().join("scan.db")).unwrap();
    let r = run(
        &Source::Folder(d.path().join("Loose")),
        &mut store,
        &AtomicBool::new(false),
        &|_| {},
        &|| vec![],
    )
    .unwrap();
    assert_eq!((r.nifs, r.parsed), (1, 1));
    let m = store.mods();
    assert_eq!(m.len(), 1);
    assert_eq!(store.nifs_of(m[0].id)[0].status_label(), "Wins");
}

/// Owner-instance smoke run (plan step 5). Read-only on the instance; writes only %TEMP%\sdc-scan-smoke.db.
#[cfg(windows)]
#[test]
#[ignore]
fn owner_smoke() {
    let root = std::path::Path::new(r"C:\_mount\moddingvol\Acherus");
    if !root.join("ModOrganizer.ini").is_file() {
        println!("owner_smoke: {} not found, skipping", root.display());
        return;
    }
    let db = std::env::temp_dir().join("sdc-scan-smoke.db");
    for ext in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{ext}", db.display()));
    }
    let inst = Instance::load(root, None).unwrap();
    let mut store = Store::open(&db).unwrap();
    let t = std::time::Instant::now();
    let r = run(
        &Source::Mo2(inst),
        &mut store,
        &AtomicBool::new(false),
        &|_| {},
        &modscan::win::process_names,
    )
    .unwrap();
    let el = t.elapsed();
    println!(
        "report: nifs={} parsed={} reused={} problems={} not_skyrim={} le_meshes={} tainted={} missing_mods={:?}",
        r.nifs,
        r.parsed,
        r.reused,
        r.problems.len(),
        r.not_skyrim,
        r.le_meshes,
        r.tainted,
        r.missing_mods
    );
    println!("pgp: {:?}", r.pgp);
    for p in r.problems.iter().take(10) {
        println!("problem: {p:?}");
    }
    println!("elapsed: {:.1}s", el.as_secs_f64());
    let c = store.conn();
    let q = |sql: &str| -> i64 { c.query_row(sql, [], |r| r.get(0)).unwrap() };
    let w = q("SELECT count(*) FROM nif_winner");
    let from =
        "FROM nif_winner w JOIN nif_copy n ON n.id = w.copy_id JOIN origin o ON o.id = n.origin_id";
    let pg = q(&format!(
        "SELECT count(*) {from} WHERE o.name = 'ParallaxGen_Output'"
    ));
    let loose_w = q(&format!("SELECT count(*) {from} WHERE o.kind IN (0, 1, 2)"));
    println!(
        "game winners: {w} (loose {loose_w}); ParallaxGen_Output: {pg} = {:.1}% of all, {:.1}% of loose",
        100.0 * pg as f64 / w.max(1) as f64,
        100.0 * pg as f64 / loose_w.max(1) as f64
    );
    let checks: Vec<(Option<i64>, i64)> = c
        .prepare("SELECT derive_check, count(*) FROM nif_copy WHERE is_derived = 1 GROUP BY 1")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    println!("derive_check counts (0 match,1 stale_changed,2 stale_reordered,3 unverified,4 unknown,5 dup_of): {checks:?}");
}
