//! Filtering spec §3: placed bases, head-part flags, and which winning records are inert (`rec_inert`).
#[path = "../../esp/tests/common/mod.rs"]
mod b;
mod common;
use b::{alt, edid, form, grup, plugin, rec, sub, top, zstr};
use common::{instance_fixture, put};
use modscan::mo2::{loose_origins, Instance};
use modscan::plugin_store::MissingRef;
use modscan::plugins::{scan_plugins, PluginStats};
use modscan::store::Store;
use std::sync::atomic::AtomicBool;

const ORDER: &str = "*Mod.esp\n*Ghost.esp\n";
const OVER: &str = "*Mod.esp\n*Ghost.esp\n*Over.esp\n";

fn txst(id: u32, name: &str, tx00: Option<&str>) -> Vec<u8> {
    let mut s = vec![edid(name)];
    if let Some(p) = tx00 {
        s.push(sub(b"TX00", &zstr(p)));
    }
    rec(b"TXST", id, &s)
}

fn model(sig: &[u8; 4], id: u32, name: &str, nif: &str, swap: u32) -> Vec<u8> {
    rec(
        sig,
        id,
        &[
            edid(name),
            sub(b"MODL", &zstr(nif)),
            sub(b"MODS", &alt(&[("Shape0", swap, 0)])),
        ],
    )
}

fn hdpt(id: u32, name: &str, data: Option<u8>, extra: &[u32], tnam: Option<u32>) -> Vec<u8> {
    let mut s = vec![edid(name)];
    if let Some(f) = data {
        s.push(sub(b"DATA", &[f]));
    }
    s.extend(extra.iter().map(|&h| form(b"HNAM", h)));
    if let Some(t) = tnam {
        s.push(form(b"TNAM", t));
    }
    rec(b"HDPT", id, &s)
}

/// One interior cell (block 0, sub-block 0) whose temporary children are `(ref id, base, is ACHR)`.
fn cell(cell_id: u32, refs: &[(u32, u32, bool)]) -> Vec<u8> {
    let children: Vec<Vec<u8>> = refs
        .iter()
        .map(|&(id, base, achr)| {
            rec(
                if achr { b"ACHR" } else { b"REFR" },
                id,
                &[form(b"NAME", base)],
            )
        })
        .collect();
    let c = cell_id.to_le_bytes();
    grup(
        b"CELL",
        0,
        &[grup(
            &0u32.to_le_bytes(),
            2,
            &[grup(
                &0u32.to_le_bytes(),
                3,
                &[
                    rec(b"CELL", cell_id, &[edid("Cell")]),
                    grup(&c, 6, &[grup(&c, 9, &children)]),
                ],
            )],
        )],
    )
}

/// Skyrim.esm (Data) defines everything; Mod.esp places PlacedStat, the Decal TXST and the Guard NPC; Ghost.esp
/// (master Missing.esm, not loaded) places a form of Missing.esm with LonelyStat's local id; Over.esp makes
/// PlayableHair non-playable.
fn fixture(plugins_txt: &str) -> tempfile::TempDir {
    let d = instance_fixture("+B\n+A\n");
    let r = d.path();
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
                        txst(0xA01, "UsedByPlaced", Some("live\\p_d.dds")),
                        txst(0xA02, "UsedByUnplaced", Some("inert\\u_d.dds")),
                        txst(0xA03, "Decal", None),
                        txst(0xA04, "Impact", None),
                        txst(0xA05, "Orphan", Some("inert\\o_d.dds")),
                        txst(0xA06, "HairTex", None),
                        txst(0xA07, "NpcHairTex", None),
                        txst(0xA08, "WeapTex", None),
                    ],
                ),
                top(
                    b"STAT",
                    &[
                        model(b"STAT", 0xB01, "PlacedStat", "placed.nif", 0xA01),
                        model(b"STAT", 0xB02, "LonelyStat", "lonely.nif", 0xA02),
                    ],
                ),
                top(
                    b"WEAP",
                    &[model(b"WEAP", 0xB03, "Sword", "sword.nif", 0xA08)],
                ),
                top(
                    b"IPCT",
                    &[rec(b"IPCT", 0xB04, &[edid("Imp"), form(b"DNAM", 0xA04)])],
                ),
                top(
                    b"HDPT",
                    &[
                        hdpt(0xC01, "PlayableHair", Some(0x01), &[0xC02], Some(0xA06)),
                        hdpt(0xC02, "ExtraPart", Some(0x08), &[], None),
                        hdpt(0xC03, "NpcOnly", Some(0x02), &[0xC04], Some(0xA07)),
                        hdpt(0xC04, "NpcExtra", Some(0x00), &[], None),
                        hdpt(0xC05, "NoData", None, &[], None),
                    ],
                ),
                top(
                    b"GRAS",
                    &[
                        rec(b"GRAS", 0xD01, &[edid("Grass")]),
                        rec(b"GRAS", 0xD02, &[edid("LonelyGrass")]),
                    ],
                ),
                top(
                    b"LTEX",
                    &[rec(b"LTEX", 0xD03, &[edid("Dirt"), form(b"GNAM", 0xD01)])],
                ),
                top(b"ARMA", &[rec(b"ARMA", 0xE01, &[edid("Arma")])]),
                top(b"NPC_", &[rec(b"NPC_", 0xE02, &[edid("Guard")])]),
            ],
        ),
    );
    put(
        r,
        "mods/A/Mod.esp",
        &plugin(
            0,
            &["Skyrim.esm"],
            &[cell(
                0x0100_0800,
                &[
                    (0x0100_0801, 0x0000_0B01, false),
                    (0x0100_0802, 0x0000_0A03, false),
                    (0x0100_0803, 0x0000_0E02, true),
                ],
            )],
        ),
    );
    put(
        r,
        "mods/A/Ghost.esp",
        &plugin(
            0,
            &["Missing.esm"],
            &[cell(0x0100_0800, &[(0x0100_0801, 0x0000_0B02, false)])],
        ),
    );
    put(
        r,
        "mods/B/Over.esp",
        &plugin(
            0,
            &["Skyrim.esm"],
            &[top(
                b"HDPT",
                &[hdpt(
                    0xC01,
                    "PlayableHair",
                    Some(0x00),
                    &[0xC02],
                    Some(0xA06),
                )],
            )],
        ),
    );
    put(r, "profiles/Default/plugins.txt", plugins_txt.as_bytes());
    d
}

fn scan(d: &tempfile::TempDir) -> (Store, PluginStats) {
    let inst = Instance::load(d.path(), None).unwrap();
    let (loose, _) = loose_origins(&inst).unwrap();
    let mut store = Store::open(&d.path().join("scan.db")).unwrap();
    let st = scan_plugins(&inst, &loose, &mut store, &AtomicBool::new(false), &|_| {}).unwrap();
    (store, st)
}

fn one_i64(s: &Store, sql: &str) -> i64 {
    s.conn().query_row(sql, [], |r| r.get(0)).unwrap()
}

/// Spec §3 + ruling R7/R10: placed bases resolve through their plugin's masters (NULL when the master is not loaded);
/// HDPT flags are stored (NULL without DATA); HNAM/GNAM edges get load-order targets.
#[test]
fn placed_bases_head_part_flags_and_edges_are_stored() {
    let d = fixture(ORDER);
    let (s, _) = scan(&d);
    let placed: Vec<(String, Option<i64>)> = s
        .conn()
        .prepare("SELECT pl.name, p.base_formid FROM placed p JOIN plugin pl ON pl.id = p.plugin_id ORDER BY 1, 2")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        placed,
        vec![
            ("Ghost.esp".to_string(), None),
            ("Mod.esp".to_string(), Some(0xA03)),
            ("Mod.esp".to_string(), Some(0xB01)),
            ("Mod.esp".to_string(), Some(0xE02)),
        ]
    );
    assert_eq!(
        one_i64(&s, "SELECT hdpt_flags FROM rec WHERE edid = 'PlayableHair'"),
        1
    );
    assert_eq!(
        one_i64(
            &s,
            "SELECT count(*) FROM rec WHERE edid = 'NoData' AND hdpt_flags IS NULL"
        ),
        1
    );
    assert_eq!(
        one_i64(&s, "SELECT txst_formid FROM txst_ref WHERE role = 'GNAM'"),
        0xD01
    );
    assert_eq!(
        one_i64(&s, "SELECT count(*) FROM txst_ref WHERE role = 'HNAM'"),
        2
    );
    assert_eq!(
        one_i64(&s, "SELECT count(*) FROM txst_ref WHERE role = 'DNAM'"),
        1,
        "IPCT"
    );
}

/// Master.esp (full) defines nothing but is a master; L.esp (ESL flag) likewise; Other.esp only shifts indices;
/// Placer.esp (masters Master.esp, L.esp) places one base of each.
fn lifecycle_fixture(plugins_txt: &str) -> tempfile::TempDir {
    let d = instance_fixture("+B\n+A\n");
    let r = d.path();
    put(r, "Game/Data/Skyrim.esm", &plugin(esp::FLAG_ESM, &[], &[]));
    put(r, "mods/A/Master.esp", &plugin(0, &[], &[]));
    put(r, "mods/A/Other.esp", &plugin(0, &[], &[]));
    put(r, "mods/A/L.esp", &plugin(esp::FLAG_ESL, &[], &[]));
    put_placer(r, &[0x0000_0801, 0x0100_0802]);
    put(r, "profiles/Default/plugins.txt", plugins_txt.as_bytes());
    d
}

fn put_placer(r: &std::path::Path, bases: &[u32]) {
    let refs: Vec<(u32, u32, bool)> = bases
        .iter()
        .enumerate()
        .map(|(i, &b)| (0x0200_0801 + i as u32, b, false))
        .collect();
    put(
        r,
        "mods/A/Placer.esp",
        &plugin(0, &["Master.esp", "L.esp"], &[cell(0x0200_0800, &refs)]),
    );
}

const LO1: &str = "*Master.esp\n*Other.esp\n*L.esp\n*Placer.esp\n";
const LO2: &str = "*Other.esp\n*Master.esp\n*L.esp\n*Placer.esp\n";

fn placed_ids(s: &Store) -> Vec<i64> {
    let mut st = s
        .conn()
        .prepare("SELECT base_formid FROM placed ORDER BY 1")
        .unwrap();
    let v = st
        .query_map([], |r| r.get::<_, Option<i64>>(0))
        .unwrap()
        .map(|x| x.unwrap().unwrap())
        .collect();
    v
}

/// Review: bases in a non-zero-index master and in a light master resolve to load-order FormIDs; a reorder with no
/// file change remaps them; replacing / dropping the plugin replaces / removes its rows.
#[test]
fn placed_rows_follow_masters_reorders_replacement_and_removal() {
    let d = lifecycle_fixture(LO1);
    let (s, _) = scan(&d);
    assert_eq!(
        placed_ids(&s),
        vec![0x0100_0801, 0xFE00_0802],
        "Master.esp is full index 1, L.esp is light slot 0"
    );
    drop(s);
    std::fs::write(d.path().join("profiles/Default/plugins.txt"), LO2).unwrap();
    let (s, st) = scan(&d);
    assert_eq!(st.cached, st.n, "no file changed");
    assert_eq!(
        placed_ids(&s),
        vec![0x0200_0801, 0xFE00_0802],
        "Master.esp moved to full index 2"
    );
    drop(s);
    put_placer(d.path(), &[0x0000_0801]);
    let (s, _) = scan(&d);
    assert_eq!(
        placed_ids(&s),
        vec![0x0200_0801],
        "the new file's rows replace the old ones"
    );
    drop(s);
    std::fs::write(
        d.path().join("profiles/Default/plugins.txt"),
        "*Other.esp\n*Master.esp\n*L.esp\n",
    )
    .unwrap();
    let (s, _) = scan(&d);
    assert_eq!(
        one_i64(&s, "SELECT count(*) FROM placed"),
        0,
        "a dropped plugin's rows go with it"
    );
}

/// Review: `rec_inert` is cleared wherever `rec_winner` is (resolve, unload, retain).
#[test]
fn rec_inert_is_cleared_with_rec_winner() {
    let seed = |s: &Store| {
        s.conn()
            .execute("INSERT INTO rec_inert VALUES (1, 1)", [])
            .unwrap()
    };
    let d = lifecycle_fixture(LO1);
    let (mut s, _) = scan(&d);
    seed(&s);
    s.unload_plugins().unwrap();
    assert_eq!(one_i64(&s, "SELECT count(*) FROM rec_inert"), 0, "unload");
    seed(&s);
    s.retain_plugins(&["Master.esp".to_string()]).unwrap();
    assert_eq!(one_i64(&s, "SELECT count(*) FROM rec_inert"), 0, "retain");
    drop(s);
    let d = lifecycle_fixture(LO1);
    let (s, _) = scan(&d);
    seed(&s);
    drop(s);
    std::fs::write(d.path().join("profiles/Default/plugins.txt"), LO2).unwrap();
    let (s, _) = scan(&d);
    assert_eq!(one_i64(&s, "SELECT count(*) FROM rec_inert"), 0, "resolve");
}

/// (EDID of the winning copy, reason) of every inert record.
fn inert(s: &Store) -> Vec<(String, i64)> {
    s.conn()
        .prepare(
            "SELECT r.edid, i.reason FROM rec_inert i JOIN rec_winner w ON w.formid = i.formid
             JOIN rec r ON r.plugin_id = w.plugin_id AND r.formid = w.formid ORDER BY r.edid",
        )
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// Spec §3 (rulings R8-R10, R13): placeables need a placement, TXSTs a live user or a placement, GRAS an LTEX, HDPT
/// Playable or a playable parent; items, IPCT, NPC_, ARMA and records without DATA are never inert.
#[test]
fn only_proven_unreached_records_are_inert() {
    let d = fixture(ORDER);
    let (s, _) = scan(&d);
    let want: Vec<(String, i64)> = [
        ("LonelyGrass", 1),
        ("LonelyStat", 2),
        ("NpcExtra", 3),
        ("NpcHairTex", 1),
        ("NpcOnly", 3),
        ("Orphan", 1),
        ("UsedByUnplaced", 1),
    ]
    .into_iter()
    .map(|(e, r)| (e.to_string(), r))
    .collect();
    assert_eq!(inert(&s), want);
}

/// Review focus 5: the winning copy decides. Over.esp makes PlayableHair non-playable, so it, its extra part and its
/// texture set become inert.
#[test]
fn reachability_follows_the_winning_copy() {
    let d = fixture(OVER);
    let (s, _) = scan(&d);
    let i = inert(&s);
    for (e, r) in [("PlayableHair", 3), ("ExtraPart", 3), ("HairTex", 1)] {
        assert!(i.contains(&(e.to_string(), r)), "{e}: {i:?}");
    }
}

#[test]
fn inert_rows_go_with_the_winners() {
    let d = fixture(ORDER);
    let (mut s, _) = scan(&d);
    assert!(one_i64(&s, "SELECT count(*) FROM rec_inert") > 0);
    s.unload_plugins().unwrap();
    assert_eq!(one_i64(&s, "SELECT count(*) FROM rec_inert"), 0);
}

/// Controller ruling: a faulted active plugin may hide placements, so nothing is judged "not placed" (or unused TXST);
/// "no reference" for grass and "head part not playable" still apply.
#[test]
fn a_plugin_error_suppresses_not_placed() {
    let d = fixture(&format!(
        "{ORDER}*Bad.esp
"
    ));
    put(d.path(), "mods/A/Bad.esp", b"not a plugin");
    let (s, _) = scan(&d);
    assert_eq!(
        one_i64(
            &s,
            "SELECT count(*) FROM plugin WHERE lo_index IS NOT NULL AND error_text IS NOT NULL"
        ),
        1
    );
    let i = inert(&s);
    assert!(i.iter().all(|(_, r)| *r != 2), "{i:?}");
    assert_eq!(
        one_i64(
            &s,
            "SELECT count(*) FROM rec_inert i JOIN rec_winner w ON w.formid = i.formid
             JOIN rec r ON r.plugin_id = w.plugin_id AND r.formid = w.formid WHERE r.sig = 'TXST'"
        ),
        0,
        "no TXST is inert under a fault: {i:?}"
    );
    for (e, r) in [("LonelyGrass", 1), ("NpcOnly", 3), ("NpcExtra", 3)] {
        assert!(i.contains(&(e.to_string(), r)), "{e}: {i:?}");
    }
}

/// Review I1: Coll.esp (masters Skyrim, Missing; load-order slot 1) holds a valid 02000ABC that resolves to 01000ABC
/// and an unresolved 01000ABC (owner_ok = 0) keeping that same value. The ghost's edges must not be credited to the winner.
#[test]
fn unresolved_copies_do_not_lend_their_edges_to_the_winner() {
    let d = fixture(&format!(
        "*Coll.esp
{ORDER}"
    ));
    put(
        d.path(),
        "mods/A/Coll.esp",
        &plugin(
            0,
            &["Skyrim.esm", "Missing.esm"],
            &[
                top(
                    b"HDPT",
                    &[
                        hdpt(0x0200_0ABC, "Victim", Some(0x01), &[], None),
                        hdpt(0x0200_0ABD, "Child", Some(0x00), &[], None),
                        hdpt(
                            0x0100_0ABC,
                            "Ghost",
                            Some(0x01),
                            &[0x0200_0ABD],
                            Some(0x0200_0ABE),
                        ),
                    ],
                ),
                top(b"TXST", &[txst(0x0200_0ABE, "Tex", None)]),
            ],
        ),
    );
    let (s, _) = scan(&d);
    let i = inert(&s);
    for (e, r) in [("Child", 3), ("Tex", 1)] {
        assert!(i.contains(&(e.to_string(), r)), "{e}: {i:?}");
    }
}

/// Registers `path` as a winning texture of a fresh enabled origin.
fn provide(s: &Store, path: &str) {
    let c = s.conn();
    let n: i64 = c
        .query_row("SELECT count(*) FROM origin", [], |r| r.get(0))
        .unwrap();
    c.execute(
        "INSERT INTO origin(kind, name, root, priority, enabled) VALUES (9, 'x', ?1, 0, 1)",
        [format!("fake{n}")],
    )
    .unwrap();
    let o = c.last_insert_rowid();
    c.execute("INSERT OR IGNORE INTO path(p) VALUES (?1)", [path])
        .unwrap();
    let p: i64 = c
        .query_row("SELECT id FROM path WHERE p = ?1", [path], |r| r.get(0))
        .unwrap();
    c.execute(
        "INSERT INTO tex_winner(path_id, origin_id, n_copies) VALUES (?1, ?2, 1)",
        [p, o],
    )
    .unwrap();
}

/// Spec §3 (ruling R12): inert records name no Missing path and use no texture; paths only they name are listed apart,
/// labels ending in the reason.
#[test]
fn missing_and_unused_ignore_inert_records() {
    let d = fixture(ORDER);
    let (s, _) = scan(&d);
    provide(&s, "textures/zzz/other.dds"); // a completed scan has an enabled origin
    let (live, inert) = s.plugin_missing_with_inert().unwrap();
    let paths = |v: &[MissingRef]| v.iter().map(|m| m.path.clone()).collect::<Vec<_>>();
    assert!(
        paths(&live).contains(&"textures/live/p_d.dds".to_string()),
        "{live:?}"
    );
    assert!(
        paths(&live).contains(&"meshes/placed.nif".to_string()),
        "{live:?}"
    );
    assert!(
        !paths(&live).contains(&"meshes/lonely.nif".to_string()),
        "{live:?}"
    );
    let lonely = inert
        .iter()
        .find(|m| m.path == "meshes/lonely.nif")
        .unwrap();
    assert_eq!(
        lonely.records,
        ["LonelyStat [00000B02] STAT — Skyrim.esm (not placed)"]
    );
    let u = inert
        .iter()
        .find(|m| m.path == "textures/inert/u_d.dds")
        .unwrap();
    assert_eq!(
        u.records,
        ["UsedByUnplaced [00000A02] TXST — Skyrim.esm (no reference)"]
    );
    assert_eq!(s.plugin_missing_files().unwrap(), live);
    provide(&s, "textures/inert/u_d.dds");
    provide(&s, "textures/live/p_d.dds");
    let unused = s.unused_textures().unwrap();
    assert!(
        unused.contains(&"textures/inert/u_d.dds".to_string()),
        "{unused:?}"
    );
    assert!(
        !unused.contains(&"textures/live/p_d.dds".to_string()),
        "{unused:?}"
    );
    let r = s.inert_of(&[0xB01, 0xB02, 0xC03]).unwrap();
    assert_eq!(r.get(&0xB02), Some(&"not placed"));
    assert_eq!(r.get(&0xC03), Some(&"head part not playable"));
    assert!(!r.contains_key(&0xB01));
}

/// Final review I2: a TXST used only by a record type the scanner reads late (ARTO alternate texture, PROJ decal, an
/// FLST entry such as an ARMA skin-swap list) is reached; one nothing names is still inert.
#[test]
fn txsts_reached_through_more_record_types_stay_visible() {
    let d = instance_fixture("+A\n");
    let mut decal = vec![0u8; 88];
    decal[84..88].copy_from_slice(&0xA03u32.to_le_bytes());
    put(
        d.path(),
        "Game/Data/Skyrim.esm",
        &plugin(
            esp::FLAG_ESM,
            &[],
            &[
                top(
                    b"TXST",
                    &[
                        txst(0xA01, "ViaArto", None),
                        txst(0xA02, "ViaFlst", None),
                        txst(0xA03, "ViaProjDecal", None),
                        txst(0xA04, "Nobody", None),
                    ],
                ),
                top(b"ARTO", &[model(b"ARTO", 0xB01, "Art", "art.nif", 0xA01)]),
                top(
                    b"FLST",
                    &[rec(b"FLST", 0xB02, &[edid("Swaps"), form(b"LNAM", 0xA02)])],
                ),
                top(
                    b"PROJ",
                    &[rec(b"PROJ", 0xB03, &[edid("Bolt"), sub(b"DATA", &decal)])],
                ),
            ],
        ),
    );
    put(d.path(), "profiles/Default/plugins.txt", b"# none\n");
    let (s, _) = scan(&d);
    assert_eq!(inert(&s), [("Nobody".to_string(), 1)]);
}
