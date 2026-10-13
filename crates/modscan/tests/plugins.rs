//! esp scanner spec §2.2: the active load order, MO2 resolution of plugin files, and load-order FormIDs.
mod common;
use common::instance_fixture;
use modscan::archives::{active_plugins, ActivePlugin};
use modscan::mo2::{loose_origins, Instance};
use modscan::plugins::{clamp_local, is_light, plugin_files, prefixes};
use std::fs;
use std::path::Path;
#[path = "../../esp/tests/common/mod.rs"]
mod b;
use b::{alt, edid, form, plugin, rec, sub, top, zstr};
use modscan::plugins::{scan_plugins, PluginStats};
use modscan::store::Store;
use modscan::ScanError;
use std::sync::atomic::AtomicBool;

fn put(root: &Path, rel: &str, bytes: &[u8]) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, bytes).unwrap();
}

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
