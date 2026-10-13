//! esp scanner spec §6 "Store joins": set → TXST, NIF → alternate texture, missing / unused.
#[path = "../../esp/tests/common/mod.rs"]
mod b;
mod common;
use b::{alt, edid, plugin, rec, sub, top, zstr};
use common::{instance_fixture, put};
use modscan::mo2::Instance;
use modscan::plugin_store::{ShapeSwap, TxstUse};
use modscan::scan::{run, Report, Source};
use modscan::store::{Store, TexKey};
use std::sync::atomic::AtomicBool;

const DIRT: u32 = 0x0000_0ABC;
const GHOST: u32 = 0x0000_0AB0;

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
