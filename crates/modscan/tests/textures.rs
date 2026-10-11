mod common;
use common::instance_fixture;
use modscan::store::TexResolution::*;

fn put(d: &tempfile::TempDir, m: &str, f: &str) {
    let p = d.path().join("mods").join(m).join(f);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, b"DDS ").unwrap();
}

fn scan(d: &tempfile::TempDir) -> modscan::store::Store {
    let inst = modscan::mo2::Instance::load(d.path(), None).unwrap();
    let mut store = modscan::store::Store::open(&d.path().join("scan.db")).unwrap();
    modscan::scan::run(
        &modscan::scan::Source::Mo2(inst),
        &mut store,
        &Default::default(),
        &|_| {},
        &|| vec![],
    )
    .unwrap();
    store
}

#[test]
fn slot_textures_resolve_to_winning_origin_or_missing() {
    let d = instance_fixture("+B\n+A\n");
    for (m, f) in [
        ("A", "textures/a_d.dds"),
        ("B", "textures/a_d.dds"),
        ("A", "textures/only_a.dds"),
        (
            "A",
            "textures/actors/character/facegendata/facegeom/x/y.dds",
        ),
    ] {
        put(&d, m, f);
    }
    let store = scan(&d);
    assert_eq!(
        store.resolve_texture("textures/a_d.dds").unwrap(),
        Found {
            origin: "B".into(),
            n_copies: 2
        }
    );
    assert_eq!(
        store.resolve_texture("textures/only_a.dds").unwrap(),
        Found {
            origin: "A".into(),
            n_copies: 1
        }
    );
    assert_eq!(store.resolve_texture("textures/nope.dds").unwrap(), Missing);
    assert_eq!(
        store
            .resolve_texture("textures/actors/character/facegendata/facegeom/x/y.dds")
            .unwrap(),
        Missing
    );
}

#[test]
fn rescan_drops_removed_textures() {
    let d = instance_fixture("+B\n+A\n");
    put(&d, "A", "textures/gone.dds");
    let store = scan(&d);
    assert!(matches!(
        store.resolve_texture("textures/gone.dds").unwrap(),
        Found { .. }
    ));
    drop(store);
    std::fs::remove_file(d.path().join("mods/A/textures/gone.dds")).unwrap();
    let store = scan(&d);
    assert_eq!(store.resolve_texture("textures/gone.dds").unwrap(), Missing);
}

#[test]
fn v1_database_is_rebuilt_at_current_schema() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("old.db");
    {
        let c = rusqlite::Connection::open(&p).unwrap();
        c.execute_batch("CREATE TABLE path (id INTEGER PRIMARY KEY, p TEXT NOT NULL UNIQUE); PRAGMA user_version = 1;")
            .unwrap();
    }
    let store = modscan::store::Store::open(&p).unwrap();
    let n: i64 = store
        .conn()
        .query_row("SELECT count(*) FROM tex_winner", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0);
    let v: i64 = store
        .conn()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v, modscan::store::SCHEMA_VERSION);
}
