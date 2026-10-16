//! LOD content (PGPatcher's blocklist) is skipped entirely: no LOD NIF copies, no LOD paths from plugin records, no
//! LOD textures in Unused.
#[path = "../../esp/tests/common/mod.rs"]
mod b;
mod common;
use b::{edid, form, grup, plugin, rec, sub, top, zstr};
use common::{bsa, instance_fixture, put};
use modscan::mo2::Instance;
use modscan::scan::{run, Source};
use modscan::store::Store;
use std::sync::atomic::AtomicBool;

#[test]
fn lod_meshes_textures_and_plugin_paths_are_skipped() {
    let d = instance_fixture("+A\n");
    let r = d.path();
    let n = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../nif/tests/fixtures/TestNifFile_Skinned_SE.nif"
    ))
    .unwrap();
    put(r, "mods/A/meshes/lod/l.nif", &n);
    put(r, "mods/A/meshes/ok.nif", &n);
    put(r, "mods/A/textures/t/x_lod.dds", b"DDS ");
    put(r, "mods/A/textures/t/lonely.dds", b"DDS ");
    put(
        r,
        "mods/A/A.bsa",
        &bsa(&[
            ("meshes\\bsa", "b_lod_0.nif", &n),
            ("meshes\\bsa", "okb.nif", &n),
        ]),
    );
    let refs = [
        rec(b"REFR", 0x0F00, &[form(b"NAME", 0x801)]),
        rec(b"REFR", 0x0F01, &[form(b"NAME", 0x800)]),
    ];
    let cell = 0x0F80u32.to_le_bytes();
    let placed = grup(
        b"CELL",
        0,
        &[grup(
            &0u32.to_le_bytes(),
            2,
            &[grup(&0u32.to_le_bytes(), 3, &[grup(&cell, 9, &refs)])],
        )],
    );
    put(
        r,
        "mods/A/A.esp",
        &plugin(
            0,
            &[],
            &[
                top(
                    b"TXST",
                    &[rec(
                        b"TXST",
                        0x800,
                        &[edid("LodTxst"), sub(b"TX00", &zstr("t\\gone_lod.dds"))],
                    )],
                ),
                top(
                    b"STAT",
                    &[rec(
                        b"STAT",
                        0x801,
                        &[edid("LodStat"), sub(b"MODL", &zstr("lod\\gone.nif"))],
                    )],
                ),
                placed,
            ],
        ),
    );
    put(r, "profiles/Default/plugins.txt", b"*A.esp\n");
    let mut s = Store::open(&r.join("scan.db")).unwrap();
    let src = Source::Mo2(Instance::load(r, None).unwrap());
    let rep = run(&src, &mut s, &AtomicBool::new(false), &|_| {}, &|| vec![]).unwrap();
    assert_eq!(rep.nifs, 2);
    assert_eq!(s.copy_ids().unwrap().len(), 2);
    assert!(s.plugin_missing_files().unwrap().is_empty());
    // LOD textures stay in tex_winner: a normal NIF may reference one.
    let n: i64 = s
        .conn()
        .query_row("SELECT count(*) FROM tex_winner", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 2);
    assert_eq!(s.unused_textures().unwrap(), ["textures/t/lonely.dds"]);
}
