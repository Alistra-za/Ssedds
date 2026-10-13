//! Final-review fixes for the esp scanner: M1 (unresolved TXST masters), I1 (TXST vs NIF slot order).
#[path = "../../esp/tests/common/mod.rs"]
mod b;
mod common;
use b::{alt, edid, plugin, rec, sub, top, zstr};
use common::{instance_fixture, put};
use modscan::mo2::Instance;
use modscan::scan::{run, Source};
use modscan::store::Store;
use std::sync::atomic::AtomicBool;

const TXST: u32 = 0x0000_0ABC;

/// Skyrim.esm: EnvTXST (TX00 a_d, TX02 env/x_m). Mod.esp (master Missing.esm, not loaded): a STAT whose alternate
/// texture names local id 0x00000ABC = a TXST of the missing master, which collides with Skyrim.esm's EnvTXST.
fn open() -> (tempfile::TempDir, Store) {
    let d = instance_fixture("+A\n");
    let r = d.path();
    put(
        r,
        "Game/Data/Skyrim.esm",
        &plugin(
            esp::FLAG_ESM,
            &[],
            &[top(
                b"TXST",
                &[rec(
                    b"TXST",
                    TXST,
                    &[
                        edid("EnvTXST"),
                        sub(b"TX00", &zstr("A/a_d.dds")),
                        sub(b"TX02", &zstr("Env/x_m.dds")),
                    ],
                )],
            )],
        ),
    );
    put(
        r,
        "mods/A/Mod.esp",
        &plugin(
            0,
            &["Missing.esm"],
            &[top(
                b"STAT",
                &[rec(
                    b"STAT",
                    0x0100_0DEF,
                    &[
                        edid("OrphanSwap"),
                        sub(b"MODL", &zstr("x.nif")),
                        sub(b"MODS", &alt(&[("Shape0", TXST, 0)])),
                    ],
                )],
            )],
        ),
    );
    put(r, "profiles/Default/plugins.txt", b"*Mod.esp\n");
    let mut s = Store::open(&r.join("scan.db")).unwrap();
    let src = Source::Mo2(Instance::load(r, None).unwrap());
    run(&src, &mut s, &AtomicBool::new(false), &|_| {}, &|| vec![]).unwrap();
    (d, s)
}

#[test]
fn an_alt_texture_with_a_missing_txst_master_joins_no_winner() {
    let (_d, s) = open();
    assert!(s.alt_users_of(&[TXST]).unwrap().is_empty());
    let uses = s.plugin_uses_of_mesh("meshes/x.nif").unwrap();
    assert_eq!(uses.len(), 1, "fixture: the STAT is resolved");
    assert_eq!(uses[0].swaps[0].txst_edid, None, "no TXST winner joined");
}
