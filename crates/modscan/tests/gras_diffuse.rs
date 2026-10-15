//! Complex grass design §5 condition 1: the diffuses of the NIFs GRAS records use.
#[path = "../../esp/tests/common/mod.rs"]
mod b;
mod common;
use b::{edid, plugin, rec, sub, top, zstr};
use common::instance_fixture;
use modscan::mo2::Instance;
use modscan::scan::{run, Source};
use modscan::store::Store;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::atomic::AtomicBool;

fn put(root: &Path, rel: &str, bytes: &[u8]) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes).unwrap();
}

/// Skyrim.esm with one record `sig` (EDID Tundra) whose MODL is `x.nif`; mod A ships `meshes/x.nif`.
fn scanned(sig: &[u8; 4]) -> (tempfile::TempDir, Store) {
    let d = instance_fixture("+A\n");
    let nif = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../nif/tests/fixtures/TestNifFile_Skinned_SE.nif"
    ))
    .unwrap();
    put(d.path(), "mods/A/meshes/x.nif", &nif);
    let r = rec(sig, 0xA01, &[edid("Tundra"), sub(b"MODL", &zstr("x.nif"))]);
    put(
        d.path(),
        "Game/Data/Skyrim.esm",
        &plugin(esp::FLAG_ESM, &[], &[top(sig, &[r])]),
    );
    put(d.path(), "profiles/Default/plugins.txt", b"");
    let mut s = Store::open(&d.path().join("scan.db")).unwrap();
    let src = Source::Mo2(Instance::load(d.path(), None).unwrap());
    run(&src, &mut s, &AtomicBool::new(false), &|_| {}, &|| vec![]).unwrap();
    (d, s)
}

/// Slot-0 textures of the lighting shapes of `meshes/x.nif`, read through the NIF API.
fn x_diffuses(s: &Store) -> BTreeSet<String> {
    let a = s
        .mods()
        .unwrap()
        .into_iter()
        .find(|m| m.name == "A")
        .unwrap()
        .id;
    let copy = s.nifs_of(a).unwrap()[0].copy_id;
    s.shapes_of(copy)
        .unwrap()
        .into_iter()
        .filter(|sh| sh.shader_kind == 1)
        .filter_map(|sh| sh.textures.first().cloned().flatten())
        .collect()
}

#[test]
fn a_gras_record_names_its_model_diffuses() {
    let (_d, s) = scanned(b"GRAS");
    let want = x_diffuses(&s);
    assert!(!want.is_empty());
    assert_eq!(s.gras_diffuse_textures().unwrap(), want);
}

#[test]
fn a_stat_with_the_same_model_names_none() {
    let (_d, s) = scanned(b"STAT");
    assert!(s.gras_diffuse_textures().unwrap().is_empty());
}

/// A PGPatcher-derived winner has no useful shapes of its own: the source copy's diffuse counts.
#[test]
fn a_derived_winner_resolves_through_its_source_copy() {
    let (_d, s) = scanned(b"GRAS");
    let want = x_diffuses(&s);
    assert!(!want.is_empty());
    let c = s.conn();
    let (pid, src): (i64, i64) = c
        .query_row("SELECT path_id, copy_id FROM nif_winner", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    c.execute(
        "INSERT INTO origin(kind, name, root, priority, enabled) VALUES (9, 'Out', 'out', 99, 1)",
        [],
    )
    .unwrap();
    let o = c.last_insert_rowid();
    c.execute(
        "INSERT INTO nif_copy(path_id, origin_id, parse_status, is_derived, source_copy_id) VALUES (?1, ?2, 0, 1, ?3)",
        [pid, o, src],
    )
    .unwrap();
    let derived = c.last_insert_rowid();
    c.execute("UPDATE nif_winner SET copy_id = ?1", [derived])
        .unwrap();
    c.execute(
        "INSERT OR REPLACE INTO nif_winner_src VALUES (?1, ?2)",
        [pid, src],
    )
    .unwrap();
    assert_eq!(s.gras_diffuse_textures().unwrap(), want);
}

/// TruePBR shapes do not count.
#[test]
fn a_truepbr_shape_is_excluded() {
    let (_d, s) = scanned(b"GRAS");
    s.conn()
        .execute("UPDATE shape SET slsf2 = ifnull(slsf2,0) | 8388608", [])
        .unwrap();
    assert!(s.gras_diffuse_textures().unwrap().is_empty());
}

/// A modlist-sized store: the planner's own join order scanned `path` first and hung the GUI for minutes.
#[test]
fn a_large_store_answers_quickly() {
    let (_d, s) = scanned(b"GRAS");
    let want = x_diffuses(&s);
    s.conn()
        .execute_batch(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 50000)
             INSERT INTO path(p) SELECT 'textures/pad/' || i || '.dds' FROM n;
             WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 20000)
             INSERT INTO model_ref(formid, local, plugin_id, slot, path_id)
               SELECT NULL, 100000 + i, 999, 0, i FROM n;
             WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 5000)
             INSERT INTO tex_use(tex_path_id, texset_id, slot) SELECT i, 900000 + i, 0 FROM n;",
        )
        .unwrap();
    let t = std::time::Instant::now();
    assert_eq!(s.gras_diffuse_textures().unwrap(), want);
    assert!(t.elapsed().as_secs_f64() < 2.0, "took {:?}", t.elapsed());
}

/// PBR grass design §4.2 (ruling R5): the `all` variant keeps TruePBR shapes; the complex-grass query still drops them.
#[test]
fn the_all_variant_keeps_truepbr_shapes() {
    let (_d, s) = scanned(b"GRAS");
    let want = x_diffuses(&s);
    assert!(!want.is_empty());
    assert_eq!(s.gras_all_diffuse_textures().unwrap(), want);
    s.conn()
        .execute("UPDATE shape SET slsf2 = ifnull(slsf2,0) | 8388608", [])
        .unwrap();
    assert_eq!(s.gras_all_diffuse_textures().unwrap(), want);
    assert!(s.gras_diffuse_textures().unwrap().is_empty());
}

#[test]
fn the_all_variant_names_none_for_a_stat() {
    let (_d, s) = scanned(b"STAT");
    assert!(s.gras_all_diffuse_textures().unwrap().is_empty());
}
