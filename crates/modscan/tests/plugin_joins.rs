//! esp scanner spec §6 "Store joins": set → TXST, NIF → alternate texture, missing / unused.
#[path = "../../esp/tests/common/mod.rs"]
mod b;
mod common;
use b::{alt, edid, form, grup, plugin, rec, sub, top, zstr};
use common::{instance_fixture, put};
use modscan::mo2::Instance;
use modscan::plugin_store::{ShapeSwap, TxstUse};
use modscan::scan::{run, Report, Source};
use modscan::store::{Store, TexKey};
use std::sync::atomic::AtomicBool;

const DIRT: u32 = 0x0000_0ABC;
const GHOST: u32 = 0x0000_0AB0;

/// A CELL group placing `bases` (filtering spec §3), so the fixture's TXSTs and STATs are live, not inert.
fn placing(bases: &[u32]) -> Vec<u8> {
    let refs: Vec<Vec<u8>> = bases
        .iter()
        .enumerate()
        .map(|(i, &b)| rec(b"REFR", 0x0F00 + i as u32, &[form(b"NAME", b)]))
        .collect();
    let cell = 0x0F80u32.to_le_bytes();
    grup(
        b"CELL",
        0,
        &[grup(
            &0u32.to_le_bytes(),
            2,
            &[grup(&0u32.to_le_bytes(), 3, &[grup(&cell, 9, &refs)])],
        )],
    )
}

/// Skyrim.esm: DirtTXST (TX00 dirt02, TX01 dirt02_n, TX02 old/gone), GhostTXST (missing texture), RockStat
/// (meshes/x.nif with Shape0 → DirtTXST), GhostStat (missing mesh), FireFX (EFSH, present texture). A.esp overrides
/// DirtTXST without TX02. Loose textures: dirt02, dirt02_n, effects/fire, unused/lonely, pbr/landscape/dirt02.
fn fixture() -> tempfile::TempDir {
    let d = instance_fixture("+B\n+A\n");
    let r = d.path();
    let nif = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../nif/tests/fixtures/TestNifFile_Skinned_SE.nif"
    ))
    .unwrap();
    put(r, "mods/A/meshes/x.nif", &nif);
    for t in [
        "landscape/dirt02.dds",
        "landscape/dirt02_n.dds",
        "effects/fire.dds",
        "unused/lonely.dds",
        "pbr/landscape/dirt02.dds",
    ] {
        put(r, &format!("mods/A/textures/{t}"), b"dds");
    }
    let dirt = |old: bool| {
        let mut s = vec![
            edid("DirtTXST"),
            sub(b"TX00", &zstr("Landscape\\Dirt02.dds")),
            sub(b"TX01", &zstr("Landscape\\Dirt02_n.dds")),
        ];
        if old {
            s.push(sub(b"TX02", &zstr("Old\\Gone.dds")));
        }
        rec(b"TXST", DIRT, &s)
    };
    put(
        r,
        "Game/Data/Skyrim.esm",
        &plugin(
            esp::FLAG_ESM,
            &[],
            &[
                top(
                    b"TXST",
                    &[
                        dirt(true),
                        rec(
                            b"TXST",
                            GHOST,
                            &[
                                edid("GhostTXST"),
                                sub(b"TX00", &zstr("Missing\\NotHere.dds")),
                            ],
                        ),
                    ],
                ),
                top(
                    b"STAT",
                    &[
                        rec(
                            b"STAT",
                            0xDEF,
                            &[
                                edid("RockStat"),
                                sub(b"MODL", &zstr("x.nif")),
                                sub(b"MODS", &alt(&[("Shape0", DIRT, 0)])),
                            ],
                        ),
                        rec(
                            b"STAT",
                            0xD00,
                            &[edid("GhostStat"), sub(b"MODL", &zstr("Missing\\Ghost.nif"))],
                        ),
                    ],
                ),
                top(
                    b"EFSH",
                    &[rec(
                        b"EFSH",
                        0xE00,
                        &[edid("FireFX"), sub(b"ICON", &zstr("Effects\\Fire.dds"))],
                    )],
                ),
                placing(&[DIRT, GHOST, 0xDEF, 0xD00]),
            ],
        ),
    );
    put(
        r,
        "mods/A/A.esp",
        &plugin(0, &["Skyrim.esm"], &[top(b"TXST", &[dirt(false)])]),
    );
    put(r, "profiles/Default/plugins.txt", b"*A.esp\n");
    d
}

fn mo2(d: &tempfile::TempDir, store: &mut Store) -> Report {
    let src = Source::Mo2(Instance::load(d.path(), None).unwrap());
    run(&src, store, &AtomicBool::new(false), &|_| {}, &|| vec![]).unwrap()
}

fn dirt_key() -> TexKey {
    TexKey::Suffix {
        slot: 0,
        stem: "landscape\\dirt02".into(),
    }
}

#[test]
fn set_key_finds_the_winning_txst() {
    let d = fixture();
    let mut s = Store::open(&d.path().join("scan.db")).unwrap();
    mo2(&d, &mut s);
    let want = TxstUse {
        formid: DIRT,
        edid: Some("DirtTXST".into()),
        plugin: "A.esp".into(),
        slot: 0,
        path: "textures/landscape/dirt02.dds".into(),
        n_overrides: 1,
    };
    assert_eq!(s.txsts_matching(&dirt_key(), 50).unwrap(), vec![want]);
    let normal = s
        .txsts_matching(
            &TexKey::Exact {
                slot: 1,
                path: "textures\\Landscape\\Dirt02_n.dds".into(),
            },
            50,
        )
        .unwrap();
    assert_eq!(normal.len(), 1);
    assert_eq!(normal[0].slot, 1);
    assert!(
        s.txsts_matching(
            &TexKey::Suffix {
                slot: 0,
                stem: "irt02".into()
            },
            50
        )
        .unwrap()
        .is_empty(),
        "whole components"
    );
}

#[test]
fn txst_users_and_mesh_overrides() {
    let d = fixture();
    let mut s = Store::open(&d.path().join("scan.db")).unwrap();
    mo2(&d, &mut s);
    let swap = ShapeSwap {
        shape_name: "Shape0".into(),
        txst_formid: DIRT,
        txst_edid: Some("DirtTXST".into()),
        index: 0,
    };
    let users = s.alt_users_of(&[DIRT]).unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(
        (
            users[0].edid.as_deref(),
            users[0].sig.as_str(),
            users[0].plugin.as_str()
        ),
        (Some("RockStat"), "STAT", "Skyrim.esm")
    );
    assert_eq!(users[0].swap, swap);
    let uses = s.plugin_uses_of_mesh("meshes/x.nif").unwrap();
    assert_eq!(uses.len(), 1);
    assert_eq!(
        (uses[0].formid, uses[0].model_slot, uses[0].n_overrides),
        (0xDEF, 0, 0)
    );
    assert_eq!(uses[0].swaps, vec![swap]);
    assert!(s
        .plugin_uses_of_mesh("meshes/other.nif")
        .unwrap()
        .is_empty());
}

/// Review Focus 5: a path only an overridden copy names is not missing; PGPatcher output is not unused.
#[test]
fn missing_and_unused_lists() {
    let d = fixture();
    let mut s = Store::open(&d.path().join("scan.db")).unwrap();
    mo2(&d, &mut s);
    let missing = s.plugin_missing_files().unwrap();
    let paths: Vec<&str> = missing.iter().map(|m| m.path.as_str()).collect();
    assert_eq!(
        paths,
        ["meshes/missing/ghost.nif", "textures/missing/nothere.dds"]
    );
    assert_eq!(
        missing[1].records,
        ["GhostTXST [00000AB0] TXST — Skyrim.esm"]
    );
    assert_eq!(missing[1].n_records, 1);
    let unused = s.unused_textures().unwrap();
    assert!(
        unused.contains(&"textures/unused/lonely.dds".to_string()),
        "{unused:?}"
    );
    for used in [
        "textures/landscape/dirt02.dds",
        "textures/landscape/dirt02_n.dds",
        "textures/effects/fire.dds",
        "textures/pbr/landscape/dirt02.dds",
    ] {
        assert!(
            !unused.contains(&used.to_string()),
            "{used} listed: {unused:?}"
        );
    }
}

/// Review Focus 4.
#[test]
fn a_folder_scan_hides_plugin_rows_and_the_next_mo2_scan_restores_them() {
    let d = fixture();
    let mut s = Store::open(&d.path().join("scan.db")).unwrap();
    mo2(&d, &mut s);
    let other = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(other.path().join("Loose")).unwrap();
    run(
        &Source::Folder(other.path().join("Loose")),
        &mut s,
        &AtomicBool::new(false),
        &|_| {},
        &|| vec![],
    )
    .unwrap();
    assert!(s.txsts_matching(&dirt_key(), 50).unwrap().is_empty());
    assert!(s.plugin_missing_files().unwrap().is_empty());
    let r = mo2(&d, &mut s);
    let p = r.plugins.unwrap();
    assert_eq!(p.cached, p.n, "nothing re-parsed");
    assert_eq!(s.txsts_matching(&dirt_key(), 50).unwrap().len(), 1);
}

/// Skyrim.esm (master Missing.esm, not loaded) with: TXSTs naming a BSA-only, an inactive-origin, a FaceGen and a
/// seven-times-referenced texture; an unresolved STAT whose raw FormID collides with a resolved one.
fn fixture2() -> tempfile::TempDir {
    let d = instance_fixture("+A\\n");
    let r = d.path();
    let t = |id: u32, name: &str, path: &str| {
        rec(
            b"TXST",
            0x0100_0000 | id,
            &[edid(name), sub(b"TX00", &zstr(path))],
        )
    };
    let mut txsts = vec![
        t(0x101, "BsaTXST", "Bsa\\OnlyBsa.dds"),
        t(0x102, "OffTXST", "Off\\Inactive.dds"),
        t(
            0x103,
            "FaceTXST",
            "Actors\\Character\\FaceGenData\\FaceTint\\Skyrim.esm\\00000001.dds",
        ),
        t(0x104, "NifTXST", "Nifonly\\Used.dds"),
    ];
    for i in 0..7u32 {
        txsts.push(t(0x200 + i, &format!("Lot{i}"), "Many\\Lots.dds"));
    }
    let stat =
        |id: u32, name: &str, m: &str| rec(b"STAT", id, &[edid(name), sub(b"MODL", &zstr(m))]);
    put(
        r,
        "Game/Data/Skyrim.esm",
        &plugin(
            esp::FLAG_ESM,
            &["Missing.esm"],
            &[
                top(b"TXST", &txsts),
                top(
                    b"STAT",
                    &[
                        stat(0x0000_0ABC, "Orphan", "Missing\\Orphan.nif"),
                        stat(0x0100_0ABC, "Real", "Real.nif"),
                    ],
                ),
                placing(&[
                    0x0100_0101,
                    0x0100_0102,
                    0x0100_0103,
                    0x0100_0104,
                    0x0100_0200,
                    0x0100_0201,
                    0x0100_0202,
                    0x0100_0203,
                    0x0100_0204,
                    0x0100_0205,
                    0x0100_0206,
                    0x0100_0ABC,
                ]),
            ],
        ),
    );
    put(r, "profiles/Default/plugins.txt", b"# none\\n");
    d
}

/// Registers `path` as a winning texture of a fresh origin (a BSA-like one when `enabled`, an inactive one otherwise).
fn provide(s: &Store, path: &str, enabled: bool) {
    let c = s.conn();
    let n: i64 = c
        .query_row("SELECT count(*) FROM origin", [], |r| r.get(0))
        .unwrap();
    c.execute(
        "INSERT INTO origin(kind, name, root, priority, enabled) VALUES (9, 'x', ?1, 0, ?2)",
        rusqlite::params![format!("fake{n}"), enabled],
    )
    .unwrap();
    let o = c.last_insert_rowid();
    c.execute("INSERT OR IGNORE INTO path(p) VALUES (?1)", [path])
        .unwrap();
    let p: i64 = c
        .query_row("SELECT id FROM path WHERE p = ?1", [path], |r| r.get(0))
        .unwrap();
    c.execute(
        "INSERT INTO tex_copy(path_id, origin_id) VALUES (?1, ?2)",
        [p, o],
    )
    .unwrap();
    c.execute(
        "INSERT INTO tex_winner(path_id, origin_id, n_copies) VALUES (?1, ?2, 1)",
        [p, o],
    )
    .unwrap();
}

#[test]
fn missing_respects_archives_inactive_origins_facegen_and_caps_records() {
    let d = fixture2();
    let mut s = Store::open(&d.path().join("scan.db")).unwrap();
    mo2(&d, &mut s);
    provide(&s, "textures/bsa/onlybsa.dds", true);
    provide(&s, "textures/off/inactive.dds", false);
    let missing = s.plugin_missing_files().unwrap();
    let paths: Vec<&str> = missing.iter().map(|m| m.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "meshes/real.nif",
            "textures/many/lots.dds",
            "textures/nifonly/used.dds",
            "textures/off/inactive.dds"
        ]
    );
    let lots = &missing[1];
    assert_eq!(lots.n_records, 7);
    assert_eq!(lots.records.len(), 5);
    assert_eq!(lots.records[0], "Lot0 [00000200] TXST — Skyrim.esm");
}

/// An unresolved record (owner_ok = 0) keeps a raw FormID that can equal a resolved winner's.
#[test]
fn an_unresolved_record_colliding_with_a_winner_is_ignored() {
    let d = fixture2();
    let mut s = Store::open(&d.path().join("scan.db")).unwrap();
    mo2(&d, &mut s);
    let ok: i64 = s
        .conn()
        .query_row(
            "SELECT count(*) FROM rec WHERE edid = 'Orphan' AND owner_ok = 0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(ok, 1, "fixture: Orphan is unresolved");
    let missing = s.plugin_missing_files().unwrap();
    assert!(
        missing
            .iter()
            .all(|m| m.path != "meshes/missing/orphan.nif"),
        "{missing:?}"
    );
    assert!(s
        .plugin_uses_of_mesh("meshes/missing/orphan.nif")
        .unwrap()
        .is_empty());
    let real = s.plugin_uses_of_mesh("meshes/real.nif").unwrap();
    assert_eq!(real.len(), 1);
    assert_eq!(real[0].edid.as_deref(), Some("Real"));
}

#[test]
fn a_texture_only_a_nif_uses_is_not_unused() {
    let d = fixture();
    let mut s = Store::open(&d.path().join("scan.db")).unwrap();
    mo2(&d, &mut s);
    let used: String = s
        .conn()
        .query_row(
            "SELECT p.p FROM tex_use u JOIN path p ON p.id = u.tex_path_id WHERE p.p NOT IN
               (SELECT p2.p FROM txst_slot t JOIN path p2 ON p2.id = t.path_id) LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    provide(&s, &used, true);
    assert!(!s.unused_textures().unwrap().contains(&used), "{used}");
    provide(&s, "textures/zzz/orphan.dds", true);
    assert!(s
        .unused_textures()
        .unwrap()
        .contains(&"textures/zzz/orphan.dds".to_string()));
}

#[test]
fn zero_limit_matches_nothing() {
    let d = fixture();
    let mut s = Store::open(&d.path().join("scan.db")).unwrap();
    mo2(&d, &mut s);
    assert!(s.txsts_matching(&dirt_key(), 0).unwrap().is_empty());
}

/// Final review I1: a scan cancelled mid-parse leaves every origin disabled; Missing and Unused are then empty, not
/// "every referenced file is missing".
#[test]
fn a_cancelled_scan_leaves_missing_and_unused_empty() {
    let d = fixture();
    let mut s = Store::open(&d.path().join("scan.db")).unwrap();
    mo2(&d, &mut s);
    assert!(!s.plugin_missing_files().unwrap().is_empty());
    for i in 0..600 {
        put(d.path(), &format!("mods/A/meshes/c{i}.nif"), b"x");
    }
    let cancel = AtomicBool::new(false);
    let src = Source::Mo2(Instance::load(d.path(), None).unwrap());
    let r = run(
        &src,
        &mut s,
        &cancel,
        &|p| {
            if p.phase == "parse" {
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        },
        &|| vec![],
    );
    assert!(matches!(r, Err(modscan::ScanError::Cancelled)), "{r:?}");
    assert!(s.plugin_missing_files().unwrap().is_empty());
    assert!(s.plugin_missing_with_inert().unwrap().1.is_empty());
    assert!(s.unused_textures().unwrap().is_empty());
}
