//! esp scanner spec §2.2: the active load order, MO2 resolution of plugin files, and load-order FormIDs.
mod common;
use common::{instance_fixture, put};
use modscan::archives::{active_plugins, ActivePlugin};
use modscan::mo2::{loose_origins, Instance};
use modscan::plugins::{clamp_local, is_light, plugin_files, prefixes};
use std::fs;
#[path = "../../esp/tests/common/mod.rs"]
mod b;
use b::{alt, edid, form, plugin, rec, sub, top, zstr};
use modscan::plugins::{scan_plugins, PluginStats};
use modscan::scan::{run, run_with, Source, PLUGINS_CANCELLED};
use modscan::store::Store;
use modscan::ScanError;
use std::sync::atomic::AtomicBool;

fn ap(name: &str, optional: bool) -> ActivePlugin {
    ActivePlugin {
        name: name.into(),
        optional,
    }
}

/// Base masters, then Skyrim.ccc entries not in plugins.txt (optional), then plugins.txt `*` entries; disabled and
/// repeated names are left out.
#[test]
fn active_plugins_in_game_order() {
    let d = instance_fixture("+A\n");
    let r = d.path();
    put(
        r,
        "Game/Skyrim.ccc",
        b"ccQSSSE001-Fish.esm\nccBGSSSE025-AdvDSGS.esm\n",
    );
    put(
        r,
        "profiles/Default/plugins.txt",
        b"# comment\n*ccBGSSSE025-AdvDSGS.esm\n*A.esp\nB.esp\n*a.esp\n",
    );
    let inst = Instance::load(r, None).unwrap();
    assert_eq!(
        active_plugins(&inst),
        [
            ap("skyrim.esm", false),
            ap("update.esm", false),
            ap("dawnguard.esm", false),
            ap("hearthfires.esm", false),
            ap("dragonborn.esm", false),
            ap("ccqssse001-fish.esm", true),
            ap("ccbgssse025-advdsgs.esm", false),
            ap("a.esp", false),
        ]
    );
}

/// modlist.txt is highest priority first: B beats A; the game Data folder is lowest. Only top-level plugin files count.
#[test]
fn plugin_files_take_the_highest_priority_origin() {
    let d = instance_fixture("+B\n+A\n");
    let r = d.path();
    put(r, "mods/A/X.esp", b"a");
    put(r, "mods/B/X.esp", b"b");
    put(r, "mods/A/Only.ESM", b"a");
    put(r, "Game/Data/Skyrim.esm", b"s");
    put(r, "mods/A/readme.txt", b"t");
    put(r, "mods/A/sub/Deep.esp", b"d");
    let inst = Instance::load(r, None).unwrap();
    let (loose, _) = loose_origins(&inst).unwrap();
    let f = plugin_files(&loose);
    assert_eq!(fs::read(&f["x.esp"].0).unwrap(), b"b");
    assert_eq!(
        f["only.esm"].1, "Only.ESM",
        "the on-disk name is kept for display"
    );
    assert!(f["skyrim.esm"].0.starts_with(r.join("Game/Data")));
    assert!(!f.contains_key("readme.txt") && !f.contains_key("deep.esp"));
}

/// Full plugins count their own `LL`; light ones their own `LLL` in FE space (ruling R2).
#[test]
fn prefixes_count_full_and_light_separately() {
    assert_eq!(
        prefixes(&[false, true, false, true]),
        [
            Some((0x0000_0000, 0x00FF_FFFF)),
            Some((0xFE00_0000, 0xFFF)),
            Some((0x0100_0000, 0x00FF_FFFF)),
            Some((0xFE00_1000, 0xFFF)),
        ]
    );
    let full = prefixes(&[false; 255]);
    assert_eq!(full[253], Some((0xFD00_0000, 0x00FF_FFFF)));
    assert_eq!(full[254], None, "0xFE is the light space");
    let light = prefixes(&[true; 4097]);
    assert_eq!(light[4095], Some((0xFEFF_F000, 0xFFF)));
    assert_eq!(light[4096], None);
}

#[test]
fn a_master_index_past_the_list_is_the_plugin_itself() {
    assert_eq!(clamp_local(0x0500_0123, 2), 0x0200_0123);
    assert_eq!(clamp_local(0x0100_0123, 2), 0x0100_0123);
    assert_eq!(clamp_local(0x0200_0123, 2), 0x0200_0123);
}

#[test]
fn light_by_flag_or_extension() {
    assert!(is_light("X.esl", 0));
    assert!(is_light("X.esp", esp::FLAG_ESL));
    assert!(!is_light("X.esm", esp::FLAG_ESM));
}

/// Same-name plugins in several origins: overwrite beats a mod, a mod beats the game Data folder.
#[test]
fn plugin_collisions_resolve_by_origin_priority() {
    let d = instance_fixture("+A\n");
    let r = d.path();
    put(r, "Game/Data/Both.esp", b"game");
    put(r, "mods/A/Both.esp", b"mod");
    put(r, "Game/Data/Three.esp", b"game");
    put(r, "mods/A/Three.esp", b"mod");
    put(r, "overwrite/Three.esp", b"over");
    let inst = Instance::load(r, None).unwrap();
    let (loose, _) = loose_origins(&inst).unwrap();
    let f = plugin_files(&loose);
    assert_eq!(fs::read(&f["both.esp"].0).unwrap(), b"mod");
    assert_eq!(fs::read(&f["three.esp"].0).unwrap(), b"over");
}

const TXST_DIRT: u32 = 0x0000_0ABC;

fn txst(id: u32, name: &str, tx00: &str) -> Vec<u8> {
    rec(b"TXST", id, &[edid(name), sub(b"TX00", &zstr(tx00))])
}

fn stat(id: u32, name: &str, model: &str) -> Vec<u8> {
    rec(b"STAT", id, &[edid(name), sub(b"MODL", &zstr(model))])
}

/// Skyrim.esm in Data; A.esp (full) and L.esp (ESL flag) in mod A; Z.esl (light by extension) and F.esp (full,
/// master L.esp) in mod B. With `*A.esp *L.esp *Z.esl *F.esp`: Skyrim 00, A 01, L FE000, Z FE001, F 02.
fn load_order(plugins_txt: &str) -> tempfile::TempDir {
    let d = instance_fixture("+B\n+A\n");
    let r = d.path();
    put(
        r,
        "Game/Data/Skyrim.esm",
        &plugin(
            esp::FLAG_ESM,
            &[],
            &[top(
                b"TXST",
                &[txst(TXST_DIRT, "DirtTXST", "Landscape\\Dirt02.dds")],
            )],
        ),
    );
    put(
        r,
        "mods/A/A.esp",
        &plugin(
            0,
            &["Skyrim.esm"],
            &[
                top(
                    b"TXST",
                    &[txst(TXST_DIRT, "DirtTXST", "Landscape\\Dirt02.dds")],
                ),
                top(b"STAT", &[stat(0x0100_0800, "AStat", "a.nif")]),
            ],
        ),
    );
    put(
        r,
        "mods/A/L.esp",
        &plugin(
            esp::FLAG_ESL,
            &["Skyrim.esm", "A.esp"],
            &[
                top(
                    b"STAT",
                    &[rec(
                        b"STAT",
                        0x0200_0801,
                        &[
                            edid("LStat"),
                            sub(b"MODL", &zstr("l.nif")),
                            sub(b"MODS", &alt(&[("Shape0", TXST_DIRT, 0)])),
                        ],
                    )],
                ),
                top(
                    b"LTEX",
                    &[rec(
                        b"LTEX",
                        0x0200_0802,
                        &[edid("LTex"), form(b"TNAM", 0x0100_0800)],
                    )],
                ),
            ],
        ),
    );
    put(
        r,
        "mods/B/Z.esl",
        &plugin(
            0,
            &["Skyrim.esm"],
            &[top(b"STAT", &[stat(0x0100_0803, "ZStat", "z.nif")])],
        ),
    );
    put(
        r,
        "mods/B/F.esp",
        &plugin(
            0,
            &["Skyrim.esm", "L.esp"],
            &[top(
                b"LTEX",
                &[rec(
                    b"LTEX",
                    0x0200_0900,
                    &[edid("FTex"), form(b"TNAM", 0x0100_0801)],
                )],
            )],
        ),
    );
    put(r, "profiles/Default/plugins.txt", plugins_txt.as_bytes());
    d
}

const ORDER: &str = "*A.esp\n*L.esp\n*Z.esl\n*F.esp\n";

fn try_scan(d: &tempfile::TempDir, cancel: bool) -> (Store, Result<PluginStats, ScanError>) {
    let inst = Instance::load(d.path(), None).unwrap();
    let (loose, _) = loose_origins(&inst).unwrap();
    let mut store = Store::open(&d.path().join("scan.db")).unwrap();
    let r = scan_plugins(&inst, &loose, &mut store, &AtomicBool::new(cancel), &|_| {});
    (store, r)
}

fn scan(d: &tempfile::TempDir) -> (Store, PluginStats) {
    let (s, r) = try_scan(d, false);
    (s, r.unwrap())
}

fn one_i64(s: &Store, sql: &str, p: impl rusqlite::Params) -> i64 {
    s.conn().query_row(sql, p, |r| r.get(0)).unwrap()
}

fn formid(s: &Store, edid: &str) -> u32 {
    one_i64(s, "SELECT formid FROM rec WHERE edid = ?1", [edid]) as u32
}

/// (winning plugin, n_overrides) of a load-order FormID.
fn winner(s: &Store, id: u32) -> (String, i64) {
    s.conn()
        .query_row(
            "SELECT p.name, w.n_overrides FROM rec_winner w JOIN plugin p ON p.id = w.plugin_id WHERE w.formid = ?1",
            [i64::from(id)],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
}

#[test]
fn formids_follow_the_load_order_full_and_light() {
    let d = load_order(ORDER);
    let (s, st) = scan(&d);
    assert_eq!((st.n, st.cached), (5, 0));
    assert_eq!(formid(&s, "AStat"), 0x0100_0800);
    assert_eq!(formid(&s, "LStat"), 0xFE00_0801, "light plugin: FE 000 801");
    assert_eq!(
        formid(&s, "ZStat"),
        0xFE00_1803,
        ".esl without the flag is light too"
    );
    assert_eq!(formid(&s, "FTex"), 0x0200_0900);
    let txst_of = |e: &str| {
        one_i64(&s, "SELECT t.txst_formid FROM txst_ref t JOIN rec r ON r.plugin_id = t.plugin_id AND r.local = t.local WHERE r.edid = ?1", [e]) as u32
    };
    assert_eq!(
        txst_of("LTex"),
        0x0100_0800,
        "a light plugin's reference into a full master"
    );
    assert_eq!(
        txst_of("FTex"),
        0xFE00_0801,
        "Review Focus 1: a full plugin's reference into a light master"
    );
    assert_eq!(
        one_i64(
            &s,
            "SELECT txst_formid FROM alt_tex WHERE shape_name = 'Shape0'",
            []
        ) as u32,
        TXST_DIRT
    );
    assert!(
        st.problems
            .iter()
            .any(|(p, m)| p == "update.esm" && m.contains("not found")),
        "{:?}",
        st.problems
    );
    assert_eq!(st.records, 7, "Skyrim 1 + A 2 + L 2 + Z 1 + F 1");
}

#[test]
fn last_plugin_wins_and_overrides_are_counted() {
    let d = load_order(ORDER);
    let (s, _) = scan(&d);
    assert_eq!(winner(&s, TXST_DIRT), ("A.esp".to_string(), 1));
    assert_eq!(winner(&s, 0xFE00_0801), ("L.esp".to_string(), 0));
}

/// Review Focus 2: nothing is re-parsed when only the order changes, and the FormIDs follow it.
#[test]
fn unchanged_plugins_are_cached_and_a_reorder_remaps() {
    let d = load_order(ORDER);
    let (s, _) = scan(&d);
    drop(s);
    let (s, st) = scan(&d);
    assert_eq!((st.n, st.cached), (5, 5));
    assert_eq!(formid(&s, "ZStat"), 0xFE00_1803);
    drop(s);
    fs::write(
        d.path().join("profiles/Default/plugins.txt"),
        "*Z.esl\n*A.esp\n*L.esp\n*F.esp\n",
    )
    .unwrap();
    let (s, st) = scan(&d);
    assert_eq!(st.cached, 5);
    assert_eq!(formid(&s, "ZStat"), 0xFE00_0803);
    assert_eq!(formid(&s, "LStat"), 0xFE00_1801);
    assert_eq!(
        formid(&s, "AStat"),
        0x0100_0800,
        "full indices count full plugins only"
    );
    drop(s);
    put(
        d.path(),
        "mods/B/Z.esl",
        &plugin(
            0,
            &["Skyrim.esm"],
            &[top(
                b"STAT",
                &[
                    stat(0x0100_0803, "ZStat", "z.nif"),
                    stat(0x0100_0804, "Z2", "z2.nif"),
                ],
            )],
        ),
    );
    let (s, st) = scan(&d);
    assert_eq!(st.cached, 4);
    assert_eq!(formid(&s, "Z2"), 0xFE00_0804);
}

/// Review Focus 3: a row stored after a failed read (size NULL) never counts as cached.
#[test]
fn a_plugin_whose_read_failed_is_read_again() {
    let d = load_order(ORDER);
    let (s, _) = scan(&d);
    s.conn()
        .execute("UPDATE plugin SET size = NULL WHERE name = 'Z.esl'", [])
        .unwrap();
    drop(s);
    let (_, st) = scan(&d);
    assert_eq!(st.cached, 4);
}

/// Spec §5: the record keeps its raw FormID and the plugin gets a note; it never wins.
#[test]
fn a_missing_master_is_noted_and_keeps_the_raw_formid() {
    let d = load_order("*A.esp\n*M.esp\n");
    put(
        d.path(),
        "mods/A/M.esp",
        &plugin(
            0,
            &["Missing.esm"],
            &[top(b"STAT", &[stat(0x0000_0900, "MStat", "m.nif")])],
        ),
    );
    let (s, st) = scan(&d);
    assert!(
        st.problems.contains(&(
            "M.esp".to_string(),
            "missing master Missing.esm".to_string()
        )),
        "{:?}",
        st.problems
    );
    assert_eq!(formid(&s, "MStat"), 0x0000_0900);
    assert_eq!(
        one_i64(&s, "SELECT owner_ok FROM rec WHERE edid = 'MStat'", []),
        0
    );
    assert_eq!(
        one_i64(
            &s,
            "SELECT count(*) FROM rec_winner WHERE formid = 2304",
            []
        ),
        0
    );
}

#[test]
fn not_found_disabled_and_dropped_plugins() {
    let d = load_order(ORDER);
    let (s, _) = scan(&d);
    drop(s);
    fs::write(
        d.path().join("profiles/Default/plugins.txt"),
        "*A.esp\nZ.esl\n*Gone.esp\n",
    )
    .unwrap();
    let (s, st) = scan(&d);
    assert_eq!(st.n, 2, "Skyrim.esm and A.esp");
    assert!(
        st.problems
            .iter()
            .any(|(p, m)| p == "gone.esp" && m.contains("not found")),
        "{:?}",
        st.problems
    );
    assert_eq!(
        one_i64(&s, "SELECT count(*) FROM plugin", []),
        2,
        "plugins that left the order are dropped"
    );
    assert_eq!(
        one_i64(&s, "SELECT count(*) FROM rec WHERE edid = 'ZStat'", []),
        0
    );
}

/// Spec §5: parse errors are reported with the plugin name on every scan, cached or not.
#[test]
fn parse_errors_are_reported_on_every_scan() {
    let d = load_order("*Bad.esp\n");
    put(d.path(), "mods/A/Bad.esp", b"junk");
    let want = (
        "Bad.esp".to_string(),
        "offset 0x0: not a plugin (no TES4 header)".to_string(),
    );
    let (s, st) = scan(&d);
    assert!(st.problems.contains(&want), "{:?}", st.problems);
    drop(s);
    let (_, st) = scan(&d);
    assert_eq!(st.cached, st.n);
    assert!(st.problems.contains(&want), "{:?}", st.problems);
}

#[test]
fn cancel_stops_the_plugin_pass() {
    let d = load_order(ORDER);
    let (_, r) = try_scan(&d, true);
    assert!(matches!(r, Err(ScanError::Cancelled)), "{r:?}");
}

fn scan_in(d: &tempfile::TempDir, s: &mut Store, cancel: bool) -> Result<PluginStats, ScanError> {
    let inst = Instance::load(d.path(), None).unwrap();
    let (loose, _) = loose_origins(&inst).unwrap();
    scan_plugins(&inst, &loose, s, &AtomicBool::new(cancel), &|_| {})
}

fn orphan_winners(s: &Store) -> i64 {
    one_i64(
        s,
        "SELECT count(*) FROM rec_winner WHERE plugin_id NOT IN (SELECT id FROM plugin)",
        [],
    )
}

/// Review I1: a resolution that fails after the removal committed must not be skipped on the retry.
#[test]
fn a_failed_resolution_after_a_removal_is_redone() {
    let d = load_order(ORDER);
    drop(scan(&d).0);
    fs::write(
        d.path().join("profiles/Default/plugins.txt"),
        "*A.esp\n*L.esp\n*Z.esl\n",
    )
    .unwrap();
    let mut s = Store::open(&d.path().join("scan.db")).unwrap();
    s.conn().execute_batch("CREATE TRIGGER boom BEFORE INSERT ON rec_winner BEGIN SELECT RAISE(ABORT, 'boom'); END").unwrap();
    assert!(scan_in(&d, &mut s, false).is_err());
    s.conn().execute_batch("DROP TRIGGER boom").unwrap();
    let st = scan_in(&d, &mut s, false).unwrap();
    assert_eq!(st.n, 4);
    assert_eq!(orphan_winners(&s), 0);
    assert_eq!(winner(&s, TXST_DIRT), ("A.esp".to_string(), 1));
}

#[test]
fn an_order_that_becomes_empty_clears_everything() {
    let d = load_order(ORDER);
    drop(scan(&d).0);
    fs::remove_file(d.path().join("Game/Data/Skyrim.esm")).unwrap();
    fs::write(d.path().join("profiles/Default/plugins.txt"), "").unwrap();
    let (s, st) = scan(&d);
    assert_eq!(st.n, 0);
    assert_eq!(one_i64(&s, "SELECT count(*) FROM plugin", []), 0);
    assert_eq!(one_i64(&s, "SELECT count(*) FROM rec_winner", []), 0);
}

/// Review I2: an all-cached pass still honours cancel and changes nothing.
#[test]
fn cancel_applies_to_an_all_cached_pass() {
    let d = load_order(ORDER);
    drop(scan(&d).0);
    fs::write(
        d.path().join("profiles/Default/plugins.txt"),
        "*Z.esl\n*A.esp\n*L.esp\n*F.esp\n",
    )
    .unwrap();
    let mut s = Store::open(&d.path().join("scan.db")).unwrap();
    assert!(matches!(
        scan_in(&d, &mut s, true),
        Err(ScanError::Cancelled)
    ));
    assert_eq!(
        formid(&s, "ZStat"),
        0xFE00_1803,
        "the old order is untouched"
    );
}

/// Review I3: unload is one transaction.
#[test]
fn unload_plugins_rolls_back_as_one() {
    let d = load_order(ORDER);
    let (mut s, _) = scan(&d);
    s.conn().execute_batch("CREATE TRIGGER boom BEFORE DELETE ON rec_winner BEGIN SELECT RAISE(ABORT, 'boom'); END").unwrap();
    assert!(s.unload_plugins().is_err());
    assert_eq!(
        one_i64(&s, "SELECT count(*) FROM plugin WHERE lo_index IS NULL", []),
        0
    );
}

/// Review round 2: the check just before resolution (cancel set by the last progress call) leaves the store as it was.
#[test]
fn cancel_set_during_progress_stops_before_resolution() {
    let d = load_order(ORDER);
    drop(scan(&d).0);
    fs::write(
        d.path().join("profiles/Default/plugins.txt"),
        "*Z.esl\n*A.esp\n*L.esp\n*F.esp\n",
    )
    .unwrap();
    let mut s = Store::open(&d.path().join("scan.db")).unwrap();
    let inst = Instance::load(d.path(), None).unwrap();
    let (loose, _) = loose_origins(&inst).unwrap();
    let cancel = AtomicBool::new(false);
    let r = scan_plugins(&inst, &loose, &mut s, &cancel, &|_| {
        cancel.store(true, std::sync::atomic::Ordering::Relaxed)
    });
    assert!(matches!(r, Err(ScanError::Cancelled)), "{r:?}");
    assert_eq!(
        formid(&s, "ZStat"),
        0xFE00_1803,
        "the old order is untouched"
    );
    assert_eq!(winner(&s, TXST_DIRT), ("A.esp".to_string(), 1));
}

fn full_run(store: &mut Store, src: Source) -> modscan::scan::Report {
    run(&src, store, &AtomicBool::new(false), &|_| {}, &|| vec![]).unwrap()
}

/// Spec §2.2: the plugin pass runs in the scan worker after the NIF/texture pass, MO2 sources only.
#[test]
fn the_scan_runs_the_plugin_pass_for_mo2_only() {
    let d = load_order(ORDER);
    let mut store = Store::open(&d.path().join("scan.db")).unwrap();
    let r = full_run(
        &mut store,
        Source::Mo2(Instance::load(d.path(), None).unwrap()),
    );
    let p = r.plugins.expect("plugin stats");
    assert_eq!((p.n, p.cached, p.records), (5, 0, 7));
    assert_eq!(r.phase_ms.last().map(|x| x.0), Some("plugins"));
    let other = tempfile::tempdir().unwrap();
    fs::create_dir_all(other.path().join("Loose")).unwrap();
    let r = full_run(&mut store, Source::Folder(other.path().join("Loose")));
    assert!(r.plugins.is_none());
    assert!(r.phase_ms.iter().all(|x| x.0 != "plugins"));
    assert_eq!(
        one_i64(&store, "SELECT count(*) FROM rec_winner", []),
        0,
        "no load order after a Folder scan"
    );
}

/// Ruling F-6: "Scan without PGPatcher output" drops PGPatcher mods from the NIF pass only; plugins stay in the order.
#[test]
fn skip_derived_keeps_plugins_of_pgpatcher_mods() {
    let d = load_order(ORDER);
    fs::create_dir_all(d.path().join("mods/B/meshes/_pgpatcher_dups")).unwrap();
    let mut store = Store::open(&d.path().join("scan.db")).unwrap();
    let src = Source::Mo2(Instance::load(d.path(), None).unwrap());
    let r = run_with(
        &src,
        &mut store,
        &AtomicBool::new(false),
        &|_| {},
        &|| vec![],
        true,
    )
    .unwrap();
    let p = r.plugins.expect("plugin stats");
    assert_eq!(p.n, 5, "{:?}", p.problems);
    assert!(
        !p.problems.iter().any(|(n, _)| n == "z.esl" || n == "f.esp"),
        "{:?}",
        p.problems
    );
    assert_eq!(formid(&store, "ZStat"), 0xFE00_1803);
}

/// Ruling R13/F-5: a cancel during the plugin pass ends the run Ok with a note and no plugin stats.
#[test]
fn cancel_during_the_plugin_pass_is_a_noted_ok() {
    let d = load_order(ORDER);
    let mut store = Store::open(&d.path().join("scan.db")).unwrap();
    let src = Source::Mo2(Instance::load(d.path(), None).unwrap());
    let cancel = AtomicBool::new(false);
    let r = run(
        &src,
        &mut store,
        &cancel,
        &|p| {
            if p.phase == "plugins" {
                cancel.store(true, std::sync::atomic::Ordering::Relaxed)
            }
        },
        &|| vec![],
    )
    .unwrap();
    assert!(
        r.notes.iter().any(|n| n == PLUGINS_CANCELLED),
        "{:?}",
        r.notes
    );
    assert!(r.plugins.is_none());
    assert!(r.phase_ms.iter().all(|x| x.0 != "plugins"));
}

/// The owner's full load order (spec §1, §6). Read-only; the store is a temp file.
/// `$env:SDC_ACHERUS='<your MO2 instance>'; cargo test -p modscan --release --test plugins owner_load_order -- --ignored --nocapture`
#[test]
#[ignore]
fn owner_load_order() {
    let root = std::path::PathBuf::from(
        std::env::var("SDC_ACHERUS").expect("set SDC_ACHERUS to the MO2 instance root"),
    );
    let db = tempfile::tempdir().unwrap();
    let inst = Instance::load(&root, None).unwrap();
    let (loose, _) = loose_origins(&inst).unwrap();
    let mut store = Store::open(&db.path().join("scan.db")).unwrap();
    let go = |store: &mut Store| {
        let t = std::time::Instant::now();
        let s = scan_plugins(&inst, &loose, store, &AtomicBool::new(false), &|_| {}).unwrap();
        (s, t.elapsed())
    };
    let (cold, cold_t) = go(&mut store);
    let (warm, warm_t) = go(&mut store);
    println!(
        "cold: n={} records={} ms={} ({:.2} s); warm: cached={}/{} ({:.2} s)",
        cold.n,
        cold.records,
        cold.ms,
        cold_t.as_secs_f64(),
        warm.cached,
        warm.n,
        warm_t.as_secs_f64()
    );
    for p in cold.problems.iter().take(40) {
        println!("problem: {p:?}");
    }
    let (edid, id, slots): (String, i64, i64) = store
        .conn()
        .query_row(
            "SELECT r.edid, r.formid, count(s.slot) FROM rec r JOIN plugin p ON p.id = r.plugin_id
             JOIN txst_slot s ON s.plugin_id = r.plugin_id AND s.local = r.local
             WHERE p.name = 'Skyrim.esm' AND r.sig = 'TXST' AND r.edid IS NOT NULL GROUP BY r.plugin_id, r.local LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    println!("Skyrim.esm TXST: {edid} [{id:08X}], {slots} slot paths");
    assert_eq!(
        id >> 24,
        0,
        "a Skyrim.esm record maps to load-order index 00"
    );
    assert!(slots >= 1);
    // Ruling F-7: alt-texture rows exist and carry load-order FormIDs.
    let n_alt = one_i64(&store, "SELECT count(*) FROM alt_tex", []);
    assert!(n_alt > 0, "no alt_tex rows");
    let (shape, txst): (String, i64) = store
        .conn()
        .query_row(
            "SELECT shape_name, txst_formid FROM alt_tex WHERE txst_formid IS NOT NULL LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    println!("alt_tex: {n_alt} rows, e.g. shape {shape:?} -> txst {txst:08X}");
    for base in [
        "skyrim.esm",
        "update.esm",
        "dawnguard.esm",
        "hearthfires.esm",
        "dragonborn.esm",
    ] {
        assert!(
            !cold
                .problems
                .iter()
                .any(|(p, _)| p.eq_ignore_ascii_case(base)),
            "{base}: {:?}",
            cold.problems
        );
    }
    assert_eq!(warm.cached, warm.n);
    assert!(cold_t.as_secs_f64() < 10.0, "cold {cold_t:?}");
    assert!(warm_t.as_secs_f64() < 1.0, "warm {warm_t:?}");
}

/// A master that is in the load order but past the 254 full-plugin limit is not "missing".
#[test]
fn a_master_past_the_limit_is_not_reported_missing() {
    let mut txt = String::from("*M.esp\n");
    for i in 0..255 {
        txt += &format!("*P{i:03}.esp\n");
    }
    let d = load_order(&txt);
    for i in 0..255 {
        put(
            d.path(),
            &format!("mods/A/P{i:03}.esp"),
            &plugin(0, &[], &[]),
        );
    }
    put(d.path(), "mods/A/M.esp", &plugin(0, &["P254.esp"], &[]));
    let (_, st) = scan(&d);
    let m: Vec<&str> = st
        .problems
        .iter()
        .filter(|(p, _)| p == "M.esp")
        .map(|(_, t)| t.as_str())
        .collect();
    assert!(
        m.iter()
            .any(|t| t.contains("P254.esp") && t.contains("past the load-order limit")),
        "{m:?}"
    );
    assert!(!m.iter().any(|t| t.contains("missing master")), "{m:?}");
}

/// Trace-debug #24: an ANSI byte or a UTF-8 BOM in plugins.txt must not empty the load order.
#[test]
fn active_plugins_survive_an_ansi_byte_and_a_bom_in_plugins_txt() {
    let d = instance_fixture("+A\n");
    let r = d.path();
    put(
        r,
        "profiles/Default/plugins.txt",
        b"\xEF\xBB\xBF# Caf\xE9 list\n*A.esp\n",
    );
    let inst = Instance::load(r, None).unwrap();
    assert!(active_plugins(&inst).contains(&ap("a.esp", false)));
}

/// MATO preview (Note GG): a stored plugin's path by name, ASCII case-insensitive; an unknown name is `None`.
#[test]
fn plugin_path_lookup() {
    let d = tempfile::tempdir().unwrap();
    let mut s = Store::open(&d.path().join("scan.db")).unwrap();
    let f = modscan::plugin_store::PluginFile {
        name: "Snow.esp",
        path: r"C:\mods\A\Snow.esp",
        size: Some(1),
        mtime: 0,
    };
    s.write_plugin(&f, &esp::PluginFacts::default()).unwrap();
    let want = Some(std::path::PathBuf::from(r"C:\mods\A\Snow.esp"));
    assert_eq!(s.plugin_path("Snow.esp").unwrap(), want);
    assert_eq!(s.plugin_path("sNOW.ESP").unwrap(), want);
    assert_eq!(s.plugin_path("Other.esp").unwrap(), None);
}
