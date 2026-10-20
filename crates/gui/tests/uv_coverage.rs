//! Task 7: the UV coverage model (scan-DB query, UV rasterisation, gutter measure).
use sdc_gui::model::uv_coverage::*;
use sdc_gui::model::viewport3d::FileSource;
use std::path::Path;
use std::sync::atomic::AtomicBool;

fn tri_mask(uvs: &[[f32; 2]], tris: &[[u16; 3]]) -> UvMask {
    let mut m = UvMask::new(MASK_SIZE);
    rasterize(&mut m, uvs, tris);
    m
}

#[test]
fn a_triangle_covers_its_texels_and_wraps_past_1() {
    let a = tri_mask(&[[0.0, 0.0], [0.5, 0.0], [0.0, 0.5]], &[[0, 1, 2]]);
    assert!((a.used_share() - 0.125).abs() < 0.01, "{}", a.used_share());
    let b = tri_mask(&[[1.0, 0.0], [1.5, 0.0], [1.0, 0.5]], &[[0, 1, 2]]);
    assert_eq!(a, b);
    // Winding does not matter.
    let c = tri_mask(&[[0.0, 0.0], [0.0, 0.5], [0.5, 0.0]], &[[0, 1, 2]]);
    assert_eq!(a, c);
    // Empty case: no texels, no division by zero.
    assert_eq!(UvMask::new(0).used_share(), 0.0);
    assert_eq!(UvMask::new(MASK_SIZE).used_share(), 0.0);
}

#[test]
fn a_trim_strip_repeating_in_u_marks_only_its_band() {
    let uvs = [[0.0, 0.1], [4.0, 0.1], [4.0, 0.2], [0.0, 0.2]];
    let m = tri_mask(&uvs, &[[0, 1, 2], [0, 2, 3]]);
    assert!((m.used_share() - 0.1).abs() < 0.01, "{}", m.used_share());
    assert!(!m.is_full());
    assert!(m.get(100, 60) && !m.get(100, 200));
}

#[test]
fn a_tiling_triangle_past_the_cap_fills_the_mask() {
    let m = tri_mask(&[[0.0, 0.0], [40.0, 0.0], [0.0, 40.0]], &[[0, 1, 2]]);
    assert!(m.is_full());
    assert_eq!(m.used_share(), 1.0);
}

#[test]
fn bad_geometry_is_skipped_not_a_panic() {
    // K24: an index past the UVs (a shape with no UV attribute has none) and non-finite UVs.
    let m = tri_mask(&[], &[[0, 1, 2]]);
    assert_eq!(m.used_share(), 0.0);
    let m = tri_mask(
        &[[0.0, 0.0], [f32::INFINITY, 0.0], [0.0, f32::NAN]],
        &[[0, 1, 2], [0, 1, 7]],
    );
    assert_eq!(m.used_share(), 0.0);
    let m = tri_mask(&[[0.0, 0.0], [1e30, 0.0], [0.0, -1e30]], &[[0, 1, 2]]);
    assert!(m.is_full());
}

fn square(m: &mut UvMask, x0: u32, x1: u32) {
    for y in 100..=109 {
        for x in x0..=x1 {
            m.set(x, y);
        }
    }
}

#[test]
fn islands_closer_than_8_texels_count_as_gutter_and_far_ones_do_not() {
    let mut near = UvMask::new(MASK_SIZE);
    square(&mut near, 0, 9);
    square(&mut near, 12, 21);
    let g = gutter_share(&near, 2048).unwrap();
    assert!(g > 0.0, "{g}");
    // d = 2 mask texels (8 full-res texels): a gap of at most 2 empty texels counts. Only the facing columns
    // (x = 9 and x = 12) are gutter texels: 20 of 200.
    assert!((g - 0.1).abs() < 1e-9, "{g}");
    let mut far = UvMask::new(MASK_SIZE);
    square(&mut far, 0, 9);
    square(&mut far, 50, 59);
    assert_eq!(gutter_share(&far, 2048), Some(0.0));
    assert_eq!(gutter_share(&near, 4096), None);
    assert_eq!(gutter_share(&near, 0), None);
    // One island alone has no gutter, however close its own texels are.
    let mut one = UvMask::new(MASK_SIZE);
    square(&mut one, 0, 30);
    assert_eq!(gutter_share(&one, 256), Some(0.0));
    // Empty case.
    assert_eq!(gutter_share(&UvMask::new(MASK_SIZE), 1024), Some(0.0));
}

// ---- The scan DB ----

const TEX: &str = "textures/a/wall.dds";

/// Inserts a path once, returning its id.
fn path_id(c: &rusqlite::Connection, p: &str) -> i64 {
    c.execute("INSERT OR IGNORE INTO path(p) VALUES (?1)", [p])
        .unwrap();
    c.query_row("SELECT id FROM path WHERE p = ?1", [p], |r| r.get(0))
        .unwrap()
}

fn origin(c: &rusqlite::Connection, id: i64, root: &Path, prio: i32, derived: bool) {
    let root = root.to_string_lossy();
    c.execute(
        "INSERT INTO origin(id, kind, name, root, priority, enabled, derived_by) VALUES (?1, 1, ?2, ?2, ?3, 1, ?4)",
        rusqlite::params![id, root, prio, derived.then_some(1)],
    )
    .unwrap();
}

/// One NIF copy; with `blocks`, those shapes name the texture in slot 0 of a lighting shader.
fn copy(
    c: &rusqlite::Connection,
    id: i64,
    nif: &str,
    origin: i64,
    le: bool,
    derived: bool,
    blocks: &[u32],
) {
    let p = path_id(c, nif);
    c.execute(
        "INSERT INTO nif_copy(id, path_id, origin_id, parse_status, le_mesh, is_derived) VALUES (?1, ?2, ?3, 0, ?4, ?5)",
        rusqlite::params![id, p, origin, le, derived],
    )
    .unwrap();
    for b in blocks {
        c.execute(
            "INSERT INTO shape(copy_id, block, kind, shader_kind, shader_type, slsf2, texset_id) VALUES (?1, ?2, 'BSTriShape', 1, 0, 0, 1)",
            rusqlite::params![id, b],
        )
        .unwrap();
    }
}

fn winner(c: &rusqlite::Connection, nif: &str, copy: i64, src: Option<i64>) {
    let p = path_id(c, nif);
    c.execute(
        "INSERT INTO nif_winner(path_id, copy_id, n_copies) VALUES (?1, ?2, 1)",
        rusqlite::params![p, copy],
    )
    .unwrap();
    if let Some(s) = src {
        c.execute(
            "INSERT INTO nif_winner_src(path_id, copy_id) VALUES (?1, ?2)",
            rusqlite::params![p, s],
        )
        .unwrap();
    }
}

fn store(db: &Path) -> modscan::store::Store {
    let s = modscan::store::Store::open(db).unwrap();
    let t = path_id(s.conn(), TEX);
    s.conn()
        .execute_batch(&format!(
            "INSERT INTO texset(id, n, t0) VALUES (1, 1, {t});
             INSERT INTO tex_use(tex_path_id, texset_id, slot) VALUES ({t}, 1, 0);"
        ))
        .unwrap();
    s
}

struct Fixture {
    _d: tempfile::TempDir,
    db: std::path::PathBuf,
    high: std::path::PathBuf,
    src: std::path::PathBuf,
}

/// a.nif: winner (blocks 3, 5) and a loser copy; b.nif: a PGPatcher output winner whose source copy names the
/// texture; c.nif: an LE mesh.
fn fixture() -> Fixture {
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("scan.db");
    let (low, high, src, pgp) = (
        d.path().join("Low"),
        d.path().join("High"),
        d.path().join("Src"),
        d.path().join("PGPatcher"),
    );
    let s = store(&db);
    let c = s.conn();
    origin(c, 1, &low, 1, false);
    origin(c, 2, &high, 10, false);
    origin(c, 3, &src, 5, false);
    origin(c, 4, &pgp, 20, true);
    copy(c, 10, "meshes/a.nif", 1, false, false, &[3, 5]);
    copy(c, 11, "meshes/a.nif", 2, false, false, &[5, 3]);
    winner(c, "meshes/a.nif", 11, None);
    copy(c, 20, "meshes/b.nif", 3, false, false, &[0]);
    copy(c, 21, "meshes/b.nif", 4, false, true, &[]);
    winner(c, "meshes/b.nif", 21, Some(20));
    copy(c, 30, "meshes/c.nif", 2, true, false, &[1]);
    winner(c, "meshes/c.nif", 30, None);
    Fixture {
        _d: d,
        db,
        high,
        src,
    }
}

#[test]
fn winning_shapes_lists_winning_copies_with_their_blocks() {
    let f = fixture();
    let mut got = winning_shapes(&f.db, TEX).unwrap();
    got.sort_by(|a, b| a.nif.cmp(&b.nif));
    assert_eq!(
        got,
        vec![
            ShapeRef {
                source: FileSource::Loose(f.high.join("meshes\\a.nif")),
                nif: "meshes/a.nif".into(),
                blocks: vec![3, 5],
            },
            ShapeRef {
                source: FileSource::Loose(f.src.join("meshes\\b.nif")),
                nif: "meshes/b.nif".into(),
                blocks: vec![0],
            },
        ]
    );
    // The key adds `textures\` when it is missing.
    assert_eq!(winning_shapes(&f.db, "a/wall.dds").unwrap().len(), 2);
    assert!(winning_shapes(&f.db, "textures/a/other.dds")
        .unwrap()
        .is_empty());
}

#[test]
fn an_unreadable_nif_is_an_error_line_not_a_failure() {
    let f = fixture();
    let before = uv_reads_on_this_thread();
    let c = coverage_for(&f.db, TEX, 1024, &AtomicBool::new(false)).unwrap();
    assert_eq!(uv_reads_on_this_thread(), before + 1);
    assert_eq!(c.nifs, 0);
    assert_eq!(c.errors.len(), 2, "{:?}", c.errors);
    assert!(c.errors.iter().any(|e| e.starts_with("meshes/a.nif: ")));
    assert_eq!(c.used, 0.0);
    assert_eq!(c.gutter, Some(0.0));
    // A missing DB is an Err (T8 caches it, K18).
    assert!(coverage_for(
        &f.db.with_extension("x"),
        TEX,
        1024,
        &AtomicBool::new(false)
    )
    .is_err());
}

#[test]
fn a_cancelled_read_is_an_err_not_a_partial_mask() {
    let f = fixture();
    assert!(coverage_for(&f.db, TEX, 1024, &AtomicBool::new(true)).is_err());
}

fn nif_fixture() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../nif/tests/fixtures/TestNifFile_Skinned_SE.nif"
    ))
    .unwrap()
}

#[test]
fn a_real_nif_marks_its_uvs() {
    let bytes = nif_fixture();
    let h = nif::header::Header::parse(&bytes).unwrap();
    let blocks: Vec<u32> = nif::scan::scan(&bytes)
        .unwrap()
        .shapes
        .iter()
        .map(|s| s.block)
        .filter(|&b| nif::geom::read_shape(&h, &bytes, b as usize).is_ok_and(|m| !m.uvs.is_empty()))
        .collect();
    assert!(!blocks.is_empty(), "the fixture has a shape with UVs");

    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("scan.db");
    let root = d.path().join("Mod");
    std::fs::create_dir_all(root.join("meshes")).unwrap();
    std::fs::write(root.join("meshes").join("body.nif"), &bytes).unwrap();
    let s = store(&db);
    origin(s.conn(), 1, &root, 1, false);
    // Block 9999 does not exist: one error line, the rest still counts.
    let mut with_bad = blocks.clone();
    with_bad.push(9999);
    copy(s.conn(), 10, "meshes/body.nif", 1, false, false, &with_bad);
    winner(s.conn(), "meshes/body.nif", 10, None);
    drop(s);

    let c = coverage_for(&db, TEX, 1024, &AtomicBool::new(false)).unwrap();
    assert_eq!(c.nifs, 1);
    assert_eq!(c.errors.len(), 1, "{:?}", c.errors);
    assert!(
        c.errors[0].starts_with("meshes/body.nif: "),
        "{:?}",
        c.errors
    );
    assert!(c.used > 0.0 && c.used <= 1.0, "{}", c.used);
    assert_eq!(c.used, c.mask.used_share());
    assert!(c.gutter.is_some());
}

#[test]
fn an_empty_or_old_scan_db_has_no_rows() {
    let d = tempfile::tempdir().unwrap();
    let empty = d.path().join("empty.db");
    drop(modscan::store::Store::open(&empty).unwrap());
    let before = uv_reads_on_this_thread();
    assert!(!scan_has_rows(&empty));
    assert_eq!(uv_reads_on_this_thread(), before + 1);

    let old = d.path().join("old.db");
    let s = store(&old);
    origin(s.conn(), 1, d.path(), 1, false);
    s.conn()
        .execute_batch(&format!(
            "PRAGMA user_version = {}",
            modscan::store::SCHEMA_VERSION - 1
        ))
        .unwrap();
    drop(s);
    assert!(!scan_has_rows(&old));
    assert!(!scan_has_rows(&d.path().join("missing.db")));

    let f = fixture();
    assert!(scan_has_rows(&f.db));
}

#[test]
#[ignore] // SDC_ACHERUS + a scan DB in SDC_SCAN_DB (scratch copy, read-only)
fn owner_coverage_of_a_single_user_texture_is_fast() {
    let (Ok(_), Ok(db)) = (std::env::var("SDC_ACHERUS"), std::env::var("SDC_SCAN_DB")) else {
        eprintln!("SDC_ACHERUS / SDC_SCAN_DB unset: skipped");
        return;
    };
    let db = Path::new(&db);
    // Slot-0 textures named by exactly one NIF copy's lighting shapes; the first whose winner reads as one NIF.
    let conn =
        rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let mut st = conn
        .prepare(
            "SELECT tp.p FROM path tp JOIN tex_use tu ON tu.tex_path_id = tp.id AND tu.slot = 0
             JOIN shape s ON s.texset_id = tu.texset_id AND s.shader_kind = 1
             GROUP BY tp.id HAVING count(DISTINCT s.copy_id) = 1 LIMIT 200",
        )
        .unwrap();
    let texs: Vec<String> = st
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let tex = texs
        .iter()
        .find(|t| winning_shapes(db, t).is_ok_and(|w| w.len() == 1))
        .expect("a texture with one winning NIF");
    let t = std::time::Instant::now();
    let c = coverage_for(db, tex, 1024, &AtomicBool::new(false)).unwrap();
    let ms = t.elapsed().as_millis();
    eprintln!(
        "{tex}: used {:.3}, gutter {:?}, {ms} ms, errors {:?}",
        c.used, c.gutter, c.errors
    );
    assert!(ms < 200, "{ms} ms");
    assert!(c.used > 0.0 && c.used <= 1.0, "{}", c.used);
}
