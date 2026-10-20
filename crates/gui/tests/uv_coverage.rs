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

#[test]
fn a_long_trim_strip_marks_its_band_not_the_whole_mask() {
    // M1: 301 tiles in u, 1 in v.
    let uvs = [[0.0, 0.1], [300.0, 0.1], [300.0, 0.2], [0.0, 0.2]];
    let m = tri_mask(&uvs, &[[0, 1, 2], [0, 2, 3]]);
    assert!((m.used_share() - 0.1).abs() < 0.01, "{}", m.used_share());
    assert!(!m.is_full());
}

/// Texels x0..=x1 by y0..=y1 as one shape (a quad of two triangles); x0 and y0 may be fractional.
fn quad(i: &mut UvIslands, x0: f32, x1: u32, y0: f32, y1: u32) {
    let n = MASK_SIZE as f32;
    let (a, b, c, d) = (x0 / n, (x1 + 1) as f32 / n, y0 / n, (y1 + 1) as f32 / n);
    i.add(&[[a, c], [b, c], [b, d], [a, d]], &[[0, 1, 2], [0, 2, 3]]);
}

fn pair(a: (f32, u32), b: (f32, u32)) -> UvIslands {
    let mut i = UvIslands::new(MASK_SIZE);
    quad(&mut i, a.0, a.1, 100.0, 109);
    quad(&mut i, b.0, b.1, 100.0, 109);
    i
}

#[test]
fn islands_closer_than_8_texels_count_as_gutter_and_far_ones_do_not() {
    // 2048 wide: d = 2 mask texels (8 px). A gap of 1 empty texel (4 px) counts; only x = 9 and x = 11 face each
    // other within 2: 20 of 200. A gap of 2 (8 px) does not.
    assert_eq!(
        gutter_share(&pair((0.0, 9), (11.0, 20)), [2048, 2048]),
        Some(0.1)
    );
    assert_eq!(
        gutter_share(&pair((0.0, 9), (12.0, 21)), [2048, 2048]),
        Some(0.0)
    );
    // 1024 wide: d = 4. A gap of 3 (6 px) counts, a gap of 4 (8 px) does not.
    assert_eq!(
        gutter_share(&pair((0.0, 9), (13.0, 22)), [1024, 1024]),
        Some(0.1)
    );
    assert_eq!(
        gutter_share(&pair((0.0, 9), (14.0, 23)), [1024, 1024]),
        Some(0.0)
    );
    assert_eq!(
        gutter_share(&pair((0.0, 9), (50.0, 59)), [2048, 2048]),
        Some(0.0)
    );
    let near = pair((0.0, 9), (11.0, 20));
    assert_eq!(gutter_share(&near, [4096, 4096]), None);
    assert_eq!(gutter_share(&near, [0, 1024]), None);
    assert_eq!(gutter_share(&near, [1024, 0]), None);
    // One island alone has no gutter. Empty case: no division by zero.
    let mut one = UvIslands::new(MASK_SIZE);
    quad(&mut one, 0.0, 30, 0.0, 30);
    assert_eq!(gutter_share(&one, [256, 256]), Some(0.0));
    assert_eq!(
        gutter_share(&UvIslands::new(MASK_SIZE), [1024, 1024]),
        Some(0.0)
    );
}

#[test]
fn islands_touching_or_overlapping_are_told_apart_by_the_mesh() {
    // I2: two meshes whose islands touch (no empty texel between) are still two islands: x 8..=11 are gutter.
    let g = gutter_share(&pair((0.0, 9), (10.0, 19)), [2048, 2048]).unwrap();
    assert!((g - 0.2).abs() < 1e-9, "{g}");
    // Touching only at a corner.
    let mut diag = UvIslands::new(MASK_SIZE);
    quad(&mut diag, 0.0, 9, 100.0, 109);
    quad(&mut diag, 10.2, 19, 110.2, 119);
    assert!(gutter_share(&diag, [2048, 2048]).unwrap() > 0.0);
    // One shape: two quads sharing an edge are one island, also when the edge's vertices are duplicated (a hard
    // edge keeps the UV).
    let n = MASK_SIZE as f32;
    let (x0, x1, x2, y0, y1) = (0.0, 10.0 / n, 20.0 / n, 100.0 / n, 110.0 / n);
    let mut one = UvIslands::new(MASK_SIZE);
    one.add(
        &[
            [x0, y0],
            [x1, y0],
            [x1, y1],
            [x0, y1],
            [x1, y0],
            [x2, y0],
            [x2, y1],
            [x1, y1],
        ],
        &[[0, 1, 2], [0, 2, 3], [4, 5, 6], [4, 6, 7]],
    );
    assert_eq!(gutter_share(&one, [2048, 2048]), Some(0.0));
    // Two meshes over the same texels (the same layout in two NIFs, or overlapping islands) are one island.
    assert_eq!(
        gutter_share(&pair((0.0, 9), (0.0, 9)), [2048, 2048]),
        Some(0.0)
    );
    assert_eq!(
        gutter_share(&pair((0.0, 9), (5.0, 14)), [2048, 2048]),
        Some(0.0)
    );
}

#[test]
fn the_gutter_radius_follows_each_side_of_the_texture() {
    // M7: 2048 x 512 has d = 2 across and d = 8 down. A gap of 5 empty texels is 20 px across and 5 px down.
    let across = pair((0.0, 9), (15.0, 24));
    assert_eq!(gutter_share(&across, [2048, 512]), Some(0.0));
    let mut down = UvIslands::new(MASK_SIZE);
    quad(&mut down, 0.0, 9, 100.0, 109);
    quad(&mut down, 0.0, 9, 115.0, 124);
    assert!(gutter_share(&down, [2048, 512]).unwrap() > 0.0);
    assert_eq!(gutter_share(&down, [2048, 2048]), Some(0.0));
    // NPOT 1536 wide: d = ceil(8 x 512 / 1536) = 3, so a gap of 2 (5.3 px) counts and a gap of 3 (8 px) does not.
    assert_eq!(
        gutter_share(&pair((0.0, 9), (12.0, 21)), [1536, 1536]),
        Some(0.1)
    );
    assert_eq!(
        gutter_share(&pair((0.0, 9), (13.0, 22)), [1536, 1536]),
        Some(0.0)
    );
}

#[test]
fn islands_mark_the_same_mask_as_rasterize() {
    let uvs = [[0.1, 0.1], [0.7, 0.2], [0.3, 0.9], [2.4, 0.5]];
    let tris = [[0, 1, 2], [1, 3, 2]];
    let mut i = UvIslands::new(MASK_SIZE);
    i.add(&uvs, &tris);
    assert_eq!(i.mask, tri_mask(&uvs, &tris));
    // Past the cap on both axes: full, one island, and later shapes change nothing.
    let mut f = UvIslands::new(MASK_SIZE);
    f.add(&[[0.0, 0.0], [40.0, 0.0], [0.0, 40.0]], &[[0, 1, 2]]);
    quad(&mut f, 0.0, 9, 100.0, 109);
    assert!(f.mask.is_full());
    assert_eq!(gutter_share(&f, [1024, 1024]), Some(0.0));
}

#[test]
fn a_reader_opens_each_archive_once() {
    // I4: a missing archive is tried once per reader, however many NIFs name it.
    let r = sdc_gui::model::viewport3d::FileReader::default();
    let bsa = |n: &str| FileSource::Bsa {
        archive: "Z:/missing/Meshes.bsa".into(),
        norm: n.into(),
    };
    assert!(r.read(&bsa("meshes/a.nif")).is_err());
    assert!(r.read(&bsa("meshes/b.nif")).is_err());
    assert_eq!(r.archives(), 1);
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
    let c = coverage_for(&f.db, TEX, [1024, 1024], &AtomicBool::new(false)).unwrap();
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
        [1024, 1024],
        &AtomicBool::new(false)
    )
    .is_err());
}

#[test]
fn a_cancelled_read_is_an_err_not_a_partial_mask() {
    let f = fixture();
    let before = uv_reads_on_this_thread();
    let r = coverage_for(&f.db, TEX, [1024, 1024], &AtomicBool::new(true));
    assert_eq!(r.unwrap_err(), CANCELLED);
    // M3: checked before the query.
    assert_eq!(uv_reads_on_this_thread(), before);
}

/// A winning copy whose slot 0 names `TEX` (texset 1) or another path (texset 2) through the raw string `raw`.
fn alias(c: &rusqlite::Connection, id: i64, nif: &str, texset: i64, raw: &str) {
    copy(c, id, nif, 2, false, false, &[]);
    c.execute(
        "INSERT INTO shape(copy_id, block, kind, shader_kind, shader_type, slsf2, texset_id) VALUES (?1, 7, 'BSTriShape', 1, 0, 0, ?2)",
        rusqlite::params![id, texset],
    )
    .unwrap();
    c.execute(
        "INSERT INTO tex_raw(copy_id, block, slot, raw, flags) VALUES (?1, 7, 0, ?2, 2)",
        rusqlite::params![id, raw],
    )
    .unwrap();
    winner(c, nif, id, None);
}

#[test]
fn a_texture_named_under_an_alias_is_found_by_its_canonical_path() {
    // I3: matched as "Used by meshes" matches, on the canonical slot path, whatever the raw string.
    let f = fixture();
    let s = modscan::store::Store::open(&f.db).unwrap();
    let c = s.conn();
    let other = path_id(c, "textures/b/a/wall.dds");
    c.execute_batch(&format!(
        "INSERT INTO texset(id, n, t0) VALUES (2, 1, {other});
         INSERT INTO tex_use(tex_path_id, texset_id, slot) VALUES ({other}, 2, 0);"
    ))
    .unwrap();
    alias(c, 40, "meshes/d.nif", 1, r"Data\Textures\a\wall.dds");
    alias(c, 41, "meshes/e.nif", 1, r"a\wall.dds");
    alias(c, 42, "meshes/f.nif", 1, r"\textures\a\wall.dds");
    // Another texture whose path contains the same fragment.
    alias(c, 43, "meshes/g.nif", 2, r"textures\b\a\wall.dds");
    drop(s);
    let mut got: Vec<String> = winning_shapes(&f.db, TEX)
        .unwrap()
        .into_iter()
        .map(|r| r.nif)
        .collect();
    got.sort();
    assert_eq!(
        got,
        [
            "meshes/a.nif",
            "meshes/b.nif",
            "meshes/d.nif",
            "meshes/e.nif",
            "meshes/f.nif"
        ]
    );
    let b: Vec<String> = winning_shapes(&f.db, "textures/b/a/wall.dds")
        .unwrap()
        .into_iter()
        .map(|r| r.nif)
        .collect();
    assert_eq!(b, ["meshes/g.nif"]);
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

    let c = coverage_for(&db, TEX, [1024, 1024], &AtomicBool::new(false)).unwrap();
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
    let c = coverage_for(db, tex, [1024, 1024], &AtomicBool::new(false)).unwrap();
    let ms = t.elapsed().as_millis();
    eprintln!(
        "{tex}: used {:.3}, gutter {:?}, {ms} ms, errors {:?}",
        c.used, c.gutter, c.errors
    );
    assert!(ms < 200, "{ms} ms");
    assert!(c.used > 0.0 && c.used <= 1.0, "{}", c.used);
}

// ---- Task 8: the GUI side (UvState, the toggle, the gutter warning, Check all) ----

mod support;
use eframe::egui;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use engine::codes::Code;
use sdc_gui::model::AppModel;
use sdc_gui::settings::GuiSettings;
use sdc_gui::worker::WorkerMsg;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

const SCAN_FIRST: &str = "Scan your MO2 instance in the Meshes view first";
const COLOUR_ONLY: &str = "Only colour textures on meshes have UV coverage";
const UNSETTLED: &str =
    "A Meshes scan is running or was stopped: wait for it to finish, or scan again";
const WALL: &str = "a/wall.png";

/// A scan DB whose one winning NIF (the nif crate's skinned fixture) names `TEX` in every shape with UVs.
fn real_db(dir: &Path) -> PathBuf {
    let bytes = nif_fixture();
    let h = nif::header::Header::parse(&bytes).unwrap();
    let blocks: Vec<u32> = nif::scan::scan(&bytes)
        .unwrap()
        .shapes
        .iter()
        .map(|s| s.block)
        .filter(|&b| nif::geom::read_shape(&h, &bytes, b as usize).is_ok_and(|m| !m.uvs.is_empty()))
        .collect();
    let db = dir.join("scan.db");
    let root = dir.join("Mod");
    std::fs::create_dir_all(root.join("meshes")).unwrap();
    std::fs::write(root.join("meshes").join("body.nif"), &bytes).unwrap();
    let s = store(&db);
    origin(s.conn(), 1, &root, 1, false);
    copy(s.conn(), 10, "meshes/body.nif", 1, false, false, &blocks);
    winner(s.conn(), "meshes/body.nif", 10, None);
    db
}

/// Three plain diffuses in `a/` (`a/wall.png` is the one the scan names) and a landscape diffuse, opened with
/// `db` as the scan database. The scan-has-rows probe has landed.
fn uv_model(dir: &Path, db: Option<PathBuf>) -> AppModel {
    let root = dir.join("textures");
    for p in [
        "a/wall.png",
        "a/wall2.png",
        "a/wall3.png",
        "landscape/ground.png",
    ] {
        let f = root.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        image::RgbImage::from_pixel(16, 16, image::Rgb([120, 90, 60]))
            .save(f)
            .unwrap();
    }
    let mut m = AppModel::new(GuiSettings::default(), None, support::session());
    m.scan_db = db;
    m.open_folder(root);
    assert!(m.wait_idle(Duration::from_secs(60)));
    m
}

fn harness(model: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(egui::vec2(1600.0, 1000.0))
        .build_ui_state(
            |ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model),
            model,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

/// A read that waits (up to 5 s, or until cancelled) before it reads.
fn slow(db: &Path, tex: &str, size: [u32; 2], cancel: &AtomicBool) -> Result<Coverage, String> {
    let end = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < end && !cancel.load(std::sync::atomic::Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(5));
    }
    coverage_for(db, tex, size, cancel)
}

fn panics(_: &Path, _: &str, _: [u32; 2], _: &AtomicBool) -> Result<Coverage, String> {
    panic!("forced UV read panic")
}

/// The UV toggle of the expanded `item`: (disabled, its disabled hover shows `uv_offered`'s reason).
fn toggle_state(mut m: AppModel, item: &str) -> (bool, bool) {
    m.expand(item);
    assert!(m.wait_idle(Duration::from_secs(60)));
    let reason = m.uv_offered(item).err().unwrap_or("never shown");
    let mut h = harness(m);
    h.run();
    let node = h.get_by_label("UV coverage overlay");
    let disabled = node.accesskit_node().is_disabled();
    node.hover();
    h.run();
    (disabled, h.query_by_label_contains(reason).is_some())
}

#[test]
fn uv_button_is_disabled_without_a_scan() {
    let d = tempfile::tempdir().unwrap();
    let m = uv_model(d.path(), None);
    assert_eq!(m.uv_offered(WALL), Err(SCAN_FIRST));
    let (disabled, reason) = toggle_state(m, WALL);
    assert!(disabled && reason, "disabled {disabled}, hover {reason}");
}

#[test]
fn uv_button_is_disabled_with_an_empty_scan_db() {
    let d = tempfile::tempdir().unwrap();
    let empty = d.path().join("empty.db");
    drop(modscan::store::Store::open(&empty).unwrap());
    let m = uv_model(d.path(), Some(empty));
    assert!(!m.uv.probe_pending());
    assert_eq!(m.uv_offered(WALL), Err(SCAN_FIRST));
    let (disabled, reason) = toggle_state(m, WALL);
    assert!(disabled && reason, "disabled {disabled}, hover {reason}");

    let d = tempfile::tempdir().unwrap();
    let mut m = uv_model(d.path(), Some(real_db(d.path())));
    assert_eq!(m.uv_offered(WALL), Ok(()));
    assert_eq!(
        m.item("landscape/ground.png")
            .and_then(|i| i.job.as_ref())
            .map(|j| j.profile.subtype),
        Some(engine::config::Subtype::LandscapeDiffuse)
    );
    assert_eq!(m.uv_offered("landscape/ground.png"), Err(COLOUR_ONLY));
    assert!(m.uv_coverage("landscape/ground.png").is_none());
    assert!(!m.v2_jobs_pending());
    let (disabled, reason) = toggle_state(m, "landscape/ground.png");
    assert!(disabled && reason, "disabled {disabled}, hover {reason}");
}

/// A new mtime on `p` (what a scan's checkpoint does to the stamp).
fn bump_mtime(p: &Path) {
    let f = std::fs::OpenOptions::new().write(true).open(p).unwrap();
    f.set_modified(std::time::SystemTime::now() + Duration::from_secs(60))
        .unwrap();
}

#[test]
fn uv_is_not_offered_while_a_meshes_scan_is_unsettled() {
    // Re-check C12, A12.
    let d = tempfile::tempdir().unwrap();
    let db = real_db(d.path());
    let root = d.path().join("textures/a");
    std::fs::create_dir_all(&root).unwrap();
    image::RgbImage::from_pixel(16, 16, image::Rgb([120, 90, 60]))
        .save(root.join("wall.png"))
        .unwrap();
    let mut m = AppModel::new(GuiSettings::default(), None, support::session());
    m.scan_store_unsettled = true;
    m.scan_db = Some(db.clone());
    m.open_folder(d.path().join("textures"));
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.uv_offered(WALL), Err(UNSETTLED));
    assert!(m.uv_coverage(WALL).is_none());
    assert!(!m.v2_jobs_pending());
    // A running scan changes the stamp: no probe starts while unsettled.
    bump_mtime(&db);
    m.poll();
    assert!(!m.uv.probe_pending(), "a probe started mid-scan");
    assert!(m.uv_coverage(WALL).is_none());
    // The scan settles: the probe runs and the item is offered.
    m.scan_store_unsettled = false;
    m.poll();
    assert!(m.uv.probe_pending());
    assert_eq!(
        m.uv_offered(WALL),
        Err(SCAN_FIRST),
        "offered before the answer"
    );
    assert!(m.wait_idle(Duration::from_secs(10)));
    assert_eq!(m.uv_offered(WALL), Ok(()));
}

#[test]
fn check_all_polls_without_greying_advanced() {
    let d = tempfile::tempdir().unwrap();
    let mut m = uv_model(d.path(), Some(real_db(d.path())));
    m.uv.read = slow;
    m.settings.advanced_open = true;
    m.select_item(WALL);
    m.check_all_uv();
    assert_eq!(m.uv.all_progress(), Some((0, 3)));
    assert!(m.v2_jobs_pending());
    assert!(!m.busy());
    let mut h = harness(m);
    h.run_steps(3);
    // Review M3: the bar is named for a screen reader, not just "0 / 3".
    assert!(h.query_by_label("Checking UV coverage: 0 of 3").is_some());
    let stop = h.get_by_label("Stop checking UV coverage");
    assert!(!stop.accesskit_node().is_disabled());
    assert!(h.query_by_label("Check all UV coverage").is_none());
    // Stop: "Stopping…" (disabled) until the reads in flight finish.
    h.state_mut().cancel_check_all_uv(); // the button pushes this action (it may sit below the fold here)
    h.run_steps(2);
    assert!(h.state().uv.all_stopping());
    assert!(h.get_by_label("Stopping…").accesskit_node().is_disabled());
    // The Advanced fields stay enabled while the run polls.
    let fields: Vec<bool> = h
        .query_all_by_label("Perceptual")
        .map(|n| n.accesskit_node().is_disabled())
        .collect();
    assert!(
        !fields.is_empty() && fields.iter().all(|d| !d),
        "{fields:?}"
    );
    let m = h.state_mut();
    m.uv.read = coverage_for;
    m.cancel_check_all_uv();
    assert!(m.wait_idle(Duration::from_secs(20)));
    assert!(!m.v2_jobs_pending() && m.uv.all_progress().is_none());
    // A full run lands every texture in the cache.
    m.check_all_uv();
    assert!(m.wait_idle(Duration::from_secs(20)));
    // CL44: answers only; no item is expanded, so no overlay image is built.
    assert!(m.uv_overlay(WALL).is_none() && !m.overlay_pending());
    assert_eq!(m.uv_coverage(WALL).map(|c| c.nifs), Some(1));
    assert_eq!(m.uv_coverage("a/wall2.png").map(|c| c.nifs), Some(0));
    h.run_steps(2);
    assert!(!h
        .get_by_label("Check all UV coverage")
        .accesskit_node()
        .is_disabled());
}

#[test]
fn check_all_is_disabled_with_the_reason_when_nothing_is_offered() {
    let d = tempfile::tempdir().unwrap();
    let mut m = uv_model(d.path(), None);
    m.settings.advanced_open = true;
    let mut h = harness(m);
    h.run();
    let button = h.get_by_label("Check all UV coverage");
    assert!(button.accesskit_node().is_disabled());
    button.hover();
    h.run();
    assert!(h.query_by_label_contains(SCAN_FIRST).is_some());
}

#[test]
fn coverage_lands_in_the_cache_and_a_finished_scan_clears_it() {
    let d = tempfile::tempdir().unwrap();
    let mut m = uv_model(d.path(), Some(real_db(d.path())));
    assert!(m.uv_coverage(WALL).is_none());
    assert!(m.uv.read_pending());
    assert!(m.wait_idle(Duration::from_secs(20)));
    let c = m.uv_coverage(WALL).expect("landed");
    assert_eq!(c.nifs, 1);
    assert!(c.used > 0.0);
    assert!(!m.uv.read_pending(), "a cached answer started a read");
    // A finished scan (the app calls this) clears the session cache.
    m.refresh_mesh_votes(false);
    assert!(m.uv_overlay(WALL).is_none());
    assert!(m.uv_coverage(WALL).is_none());
    assert!(m.uv.read_pending());
    assert!(m.wait_idle(Duration::from_secs(20)));
    assert!(m.uv_coverage(WALL).is_some());
}

/// K18: a failed read is cached (not retried every frame); a cancelled one is never cached.
#[test]
fn a_failed_read_is_cached_and_a_cancelled_one_is_not() {
    let d = tempfile::tempdir().unwrap();
    let mut m = uv_model(d.path(), Some(real_db(d.path())));
    m.uv.read = |_, _, _, _| Err("no such table: path".into());
    assert!(m.uv_coverage(WALL).is_none());
    assert!(m.wait_idle(Duration::from_secs(10)));
    let c = m.uv_coverage(WALL).expect("the error is cached");
    assert_eq!(c.nifs, 0);
    assert_eq!(c.errors, ["no such table: path"]);
    assert_eq!(c.gutter, None);
    assert!(!m.uv.read_pending(), "a failed read is retried");
    assert!(summary(&c).contains("no such table: path"));
    assert!(m
        .check_warnings(WALL)
        .iter()
        .all(|w| w.code != Code::ChkUvGutter));
    // Cancelled: the reads stop and nothing is cached.
    let d = tempfile::tempdir().unwrap();
    let mut m = uv_model(d.path(), Some(real_db(d.path())));
    static STARTED: AtomicBool = AtomicBool::new(false);
    fn signals(
        db: &Path,
        tex: &str,
        size: [u32; 2],
        cancel: &AtomicBool,
    ) -> Result<Coverage, String> {
        STARTED.store(true, std::sync::atomic::Ordering::SeqCst);
        slow(db, tex, size, cancel)
    }
    m.uv.read = signals;
    m.check_all_uv();
    // Cancel once a read is in flight, so the read itself returns `CANCELLED`.
    while !STARTED.load(std::sync::atomic::Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(1));
    }
    m.cancel_check_all_uv();
    assert!(m.wait_idle(Duration::from_secs(10)));
    assert!(m.uv_overlay(WALL).is_none(), "a cancelled read was cached");
    // A single read is cancelled by a clear (a finished scan), and nothing lands either.
    m.uv_coverage(WALL);
    m.refresh_mesh_votes(false);
    assert!(m.wait_idle(Duration::from_secs(10)));
    assert!(m.uv_overlay(WALL).is_none());
}

/// K9: a panicking read never leaves `v2_jobs_pending()` set, and its tex is not retried every frame.
#[test]
fn a_panicking_uv_read_clears_v2_jobs_pending() {
    let d = tempfile::tempdir().unwrap();
    let mut m = uv_model(d.path(), Some(real_db(d.path())));
    m.uv.read = panics;
    m.uv_coverage(WALL);
    m.check_all_uv();
    assert!(
        m.wait_idle(Duration::from_secs(10)),
        "wait_idle spun to its timeout"
    );
    assert!(!m.v2_jobs_pending());
    let c = m
        .uv_coverage(WALL)
        .expect("the panic is cached as an error");
    assert!(
        c.errors[0].contains("forced UV read panic"),
        "{:?}",
        c.errors
    );
    assert!(!m.uv.read_pending());
}

#[test]
fn a_close_island_pair_raises_chk_uv_gutter_after_the_click() {
    let d = tempfile::tempdir().unwrap();
    let mut m = uv_model(d.path(), Some(real_db(d.path())));
    let (tex, size) = m.uv_key(WALL).expect("a key");
    let gutter_rows = |m: &AppModel| {
        m.check_warnings(WALL)
            .iter()
            .filter(|w| w.code == Code::ChkUvGutter)
            .cloned()
            .collect::<Vec<_>>()
    };
    let inject = |m: &mut AppModel, gutter: Option<f64>| {
        let c = Coverage {
            mask: Arc::new(UvMask::new(MASK_SIZE)),
            used: 0.4,
            gutter,
            nifs: 1,
            errors: vec![],
        };
        let msg = WorkerMsg::UvCoverage {
            generation: m.uv.generation(),
            tex: tex.clone(),
            size,
            coverage: Arc::new(c),
        };
        m.sender().send(msg).unwrap();
        m.poll();
    };
    // Nothing until a read (on click only).
    assert!(gutter_rows(&m).is_empty());
    inject(&mut m, Some(0.25));
    let w = gutter_rows(&m);
    assert_eq!(w.len(), 1);
    assert_eq!(
        w[0].message,
        "UV islands sit closer than 8 texels for 25.0 % of used texels: colours bleed across them at distance (mip 3+)"
    );
    // A Problems row at Level::Check (it never changes "ready").
    let rows = sdc_gui::model::bottom::problems(&m);
    assert!(rows
        .iter()
        .any(|p| p.code == Some(Code::ChkUvGutter)
            && p.level == sdc_gui::model::bottom::Level::Check));
    // The switch hides it and brings it back, with no new read.
    let mut c = m.settings.checks;
    c.uv = false;
    m.set_checks(c);
    m.poll();
    assert!(gutter_rows(&m).is_empty());
    c.uv = true;
    m.set_checks(c);
    m.poll();
    assert_eq!(gutter_rows(&m).len(), 1);
    // At the share: no warning. Not measured: no warning, and the hover says why.
    inject(&mut m, Some(GUTTER_SHARE));
    assert!(gutter_rows(&m).is_empty());
    inject(&mut m, None);
    assert!(gutter_rows(&m).is_empty());
    let c = m.uv_coverage(WALL).unwrap();
    assert!(summary(&c).contains(
        "gutter not measured at 4096 px or wider on either axis, or until the size is known"
    ));
}

#[test]
fn check_all_cancels_between_textures() {
    // K17: the scoped runner with the cancel flag already set.
    let f = fixture();
    let work: Vec<(String, [u32; 2])> = ["a/wall.dds", "a/b.dds", "a/c.dds"]
        .iter()
        .map(|t| (t.to_string(), [1024, 1024]))
        .collect();
    let run = |work: Vec<(String, [u32; 2])>, cancel: bool| {
        let (tx, rx) = std::sync::mpsc::channel();
        let job = AllJob {
            run: 7,
            generation: 1,
            db: f.db.clone(),
            work,
            jobs: 4,
            read: coverage_for,
        };
        check_all_run(&tx, &job, &AtomicBool::new(cancel));
        drop(tx);
        rx.iter().collect::<Vec<_>>()
    };
    let msgs = run(work.clone(), true);
    assert_eq!(msgs.len(), 1);
    assert!(matches!(
        msgs[0],
        WorkerMsg::UvProgress {
            run: 7,
            done: 0,
            total: 3,
            finished: true
        }
    ));
    // 0 items: one (0, 0) and the run ends.
    let msgs = run(vec![], false);
    assert_eq!(msgs.len(), 1);
    assert!(matches!(
        msgs[0],
        WorkerMsg::UvProgress {
            done: 0,
            total: 0,
            finished: true,
            ..
        }
    ));
    // Not cancelled: one coverage per texture, then the end.
    let msgs = run(work, false);
    let covs = msgs
        .iter()
        .filter(|m| matches!(m, WorkerMsg::UvCoverage { .. }))
        .count();
    assert_eq!(covs, 3);
    assert!(matches!(
        msgs.last().unwrap(),
        WorkerMsg::UvProgress {
            done: 3,
            total: 3,
            finished: true,
            ..
        }
    ));
    // App level: cancelling ends the run and clears v2_jobs_pending.
    let d = tempfile::tempdir().unwrap();
    let mut m = uv_model(d.path(), Some(real_db(d.path())));
    m.uv.read = slow;
    m.check_all_uv();
    assert!(m.uv.all_running());
    m.cancel_check_all_uv();
    assert!(m.wait_idle(Duration::from_secs(10)));
    assert!(!m.uv.all_running() && !m.v2_jobs_pending());
    // Nothing offered (no scan): the run ends at once.
    let d = tempfile::tempdir().unwrap();
    let mut m = uv_model(d.path(), None);
    m.check_all_uv();
    assert!(m.wait_idle(Duration::from_secs(10)));
    assert!(!m.uv.all_running());
}

#[test]
fn the_ui_thread_never_reads_the_scan_db() {
    let d = tempfile::tempdir().unwrap();
    let before = uv_reads_on_this_thread();
    let mut m = uv_model(d.path(), Some(real_db(d.path())));
    m.expand(WALL);
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.uv.read = slow;
    m.view.overlay = Some(sdc_gui::model::overlay::OverlayKind::Uv);
    let mut h = harness(m);
    h.run_steps(4);
    assert!(h.state().uv.read_pending(), "the preview asked for no read");
    let m = h.state_mut();
    m.uv.read = coverage_for;
    assert!(m.wait_idle(Duration::from_secs(20)));
    h.run_steps(2);
    // The read landed (on the worker); this thread opened no scan DB, for the probe or the read.
    assert_eq!(h.state_mut().uv_coverage(WALL).map(|c| c.nifs), Some(1));
    assert_eq!(uv_reads_on_this_thread(), before);
    // CL44: the preview asked for the overlay; an overlay job builds it.
    assert!(h.state_mut().wait_idle(Duration::from_secs(10)));
    assert!(h.state_mut().uv_overlay(WALL).is_some());
}

#[test]
fn the_uv_overlay_dims_unused_texels_and_veils_used_ones() {
    use sdc_gui::model::overlay::{uv_overlay_image, UV_UNUSED, UV_USED};
    let mut mask = UvMask::new(4);
    mask.set(1, 0);
    let img = uv_overlay_image(&mask);
    assert_eq!(img.size, [4, 4]);
    let px = |c: [u8; 4]| egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]);
    assert_eq!(img.pixels[1], px(UV_USED));
    assert_eq!(img.pixels[0], px(UV_UNUSED));
    assert_eq!(img.pixels[4], px(UV_UNUSED));
    // Row 0 is v = 0, the image's top.
    assert_eq!(img.pixels.iter().filter(|&&p| p == px(UV_USED)).count(), 1);
    // Empty mask: an empty image, no panic.
    assert!(uv_overlay_image(&UvMask::new(0)).pixels.is_empty());
}

// ---- Task 8 fix round 1 ----

/// Review I1: no read before the written size is known, and a new written size reads again.
#[test]
fn an_unknown_size_reads_nothing_and_a_size_change_reads_again() {
    let d = tempfile::tempdir().unwrap();
    let mut m = uv_model(d.path(), Some(real_db(d.path())));
    let i = m.items.iter().position(|i| i.key == WALL).unwrap();
    let analysis = m.items[i].analysis.take();
    assert!(m.uv_key(WALL).is_none());
    assert!(m.uv_coverage(WALL).is_none());
    assert!(!m.uv.read_pending(), "read with no size");
    m.check_all_uv();
    assert_eq!(m.uv.all_progress(), Some((0, 2)), "Check all read it");
    assert!(m.wait_idle(Duration::from_secs(20)));
    assert!(m.uv_overlay(WALL).is_none());
    // The analysis lands: the read starts.
    m.items[i].analysis = analysis;
    assert!(m.uv_coverage(WALL).is_none());
    assert!(m.uv.read_pending());
    assert!(m.wait_idle(Duration::from_secs(20)));
    assert_eq!(m.uv_coverage(WALL).map(|c| c.nifs), Some(1));

    // A 1024² diffuse, then written at half size.
    let d = tempfile::tempdir().unwrap();
    let db = real_db(d.path());
    let root = d.path().join("textures/a");
    std::fs::create_dir_all(&root).unwrap();
    image::RgbImage::from_pixel(1024, 1024, image::Rgb([120, 90, 60]))
        .save(root.join("wall.png"))
        .unwrap();
    let mut m = AppModel::new(GuiSettings::default(), None, support::session());
    m.scan_db = Some(db);
    m.open_folder(d.path().join("textures"));
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.uv_key(WALL).map(|k| k.1), Some([1024, 1024]));
    m.uv_coverage(WALL);
    assert!(m.wait_idle(Duration::from_secs(20)));
    let full = m.uv_coverage(WALL).expect("read at 1024");
    m.set_field(WALL, "resize", serde_json::json!("half"));
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.uv_key(WALL).map(|k| k.1), Some([512, 512]));
    assert!(m.uv_coverage(WALL).is_none(), "the 1024 answer was reused");
    assert!(m.uv.read_pending());
    assert!(m.wait_idle(Duration::from_secs(20)));
    let half = m.uv_coverage(WALL).expect("read at 512");
    assert!(!Arc::ptr_eq(&full, &half));
}

/// Review I2: a failed read paints no "unused" overlay; the toggle is disabled with the error as its hover, and
/// the answer stays cached (K18).
#[test]
fn a_failed_read_paints_no_overlay_and_disables_the_toggle() {
    let d = tempfile::tempdir().unwrap();
    let mut m = uv_model(d.path(), Some(real_db(d.path())));
    m.uv.read = |_, _, _, _| Err("database is locked".into());
    m.expand(WALL);
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.uv_coverage(WALL);
    assert!(m.wait_idle(Duration::from_secs(10)));
    assert!(m.uv_failed(WALL).is_some());
    assert!(m.uv_overlay(WALL).is_none(), "a failed read has an overlay");
    m.view.overlay = Some(sdc_gui::model::overlay::OverlayKind::Uv);
    let mut h = harness(m);
    h.run();
    let node = h.get_by_label("UV coverage overlay");
    assert!(node.accesskit_node().is_disabled());
    node.hover();
    h.run();
    assert!(h.query_by_label_contains("database is locked").is_some());
    assert!(!h.state().uv.read_pending(), "the failed read is retried");
    // A true "no mesh uses it" answer (no error) still has its all-dim overlay.
    let c = Coverage {
        mask: Arc::new(UvMask::new(MASK_SIZE)),
        used: 0.0,
        gutter: Some(0.0),
        nifs: 0,
        errors: vec![],
    };
    assert!(!failed(&c));
}

/// Review M4: Check all needs the "UV coverage check" switch.
#[test]
fn check_all_is_disabled_while_the_uv_switch_is_off() {
    let d = tempfile::tempdir().unwrap();
    let mut m = uv_model(d.path(), Some(real_db(d.path())));
    assert_eq!(m.uv_check_all_offered(), Ok(()));
    let mut c = m.settings.checks;
    c.uv = false;
    m.settings.checks = c;
    assert_eq!(
        m.uv_check_all_offered(),
        Err("Turn on UV coverage check first")
    );
    m.settings.advanced_open = true;
    let mut h = harness(m);
    h.run();
    let button = h.get_by_label("Check all UV coverage");
    assert!(button.accesskit_node().is_disabled());
    button.hover();
    h.run();
    assert!(h
        .query_by_label_contains("Turn on UV coverage check first")
        .is_some());
}

/// Review M5: a new scan stamp cancels a running Check all, also on an empty cache.
#[test]
fn a_stamp_change_cancels_check_all_on_an_empty_cache() {
    let d = tempfile::tempdir().unwrap();
    let db = real_db(d.path());
    let mut m = uv_model(d.path(), Some(db.clone()));
    m.uv.read = slow;
    m.check_all_uv();
    let generation = m.uv.generation();
    bump_mtime(&db);
    m.poll();
    assert!(
        m.uv.generation() > generation,
        "answers of the old scan land"
    );
    assert!(
        m.wait_idle(Duration::from_secs(10)),
        "the run was not cancelled"
    );
    assert!(!m.uv.all_running());
}

/// Review M7: another folder keeps the cache (the key is the scan path); only answers in flight are dropped.
#[test]
fn opening_another_folder_keeps_the_uv_cache() {
    let d = tempfile::tempdir().unwrap();
    let mut m = uv_model(d.path(), Some(real_db(d.path())));
    m.uv_coverage(WALL);
    assert!(m.wait_idle(Duration::from_secs(20)));
    let first = m.uv_coverage(WALL).expect("landed");
    let other = d.path().join("other/textures");
    std::fs::create_dir_all(other.join("b")).unwrap();
    image::RgbImage::from_pixel(16, 16, image::Rgb([1, 2, 3]))
        .save(other.join("b/x.png"))
        .unwrap();
    m.open_folder(other);
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.open_folder(d.path().join("textures"));
    assert!(m.wait_idle(Duration::from_secs(60)));
    let again = m.uv_coverage(WALL).expect("the cache was cleared");
    assert!(Arc::ptr_eq(&first, &again));
    assert!(!m.uv.read_pending());
    // A read in flight when the folder changes is dropped.
    m.refresh_mesh_votes(false); // a finished scan does clear it
    assert!(m.uv_overlay(WALL).is_none());
    m.uv.read = slow;
    m.uv_coverage(WALL);
    let generation = m.uv.generation();
    m.open_folder(d.path().join("textures"));
    assert!(m.uv.generation() > generation);
    assert!(!m.uv.read_pending());
}

/// Review M8: answers landing during Check all re-run the checks at most every 250 ms, and once when it ends.
#[test]
fn check_all_answers_refresh_the_checks_in_batches() {
    let d = tempfile::tempdir().unwrap();
    let mut m = uv_model(d.path(), Some(real_db(d.path())));
    m.uv.read = slow;
    m.check_all_uv();
    let (tex, size) = m.uv_key(WALL).unwrap();
    let start = m.uv.revision();
    let t0 = std::time::Instant::now();
    for k in 0..5 {
        let c = Coverage {
            mask: Arc::new(UvMask::new(MASK_SIZE)),
            used: 0.4,
            gutter: Some(0.25),
            nifs: 1,
            errors: vec![],
        };
        m.sender()
            .send(WorkerMsg::UvCoverage {
                generation: m.uv.generation(),
                tex: format!("{tex}{k}"),
                size,
                coverage: Arc::new(c),
            })
            .unwrap();
        m.poll();
    }
    if t0.elapsed() < Duration::from_millis(250) {
        assert!(
            m.uv.revision() <= start + 1,
            "{} bumps",
            m.uv.revision() - start
        );
    }
    m.cancel_check_all_uv();
    assert!(m.wait_idle(Duration::from_secs(10)));
    assert!(
        m.uv.revision() > start,
        "the end of the run did not refresh"
    );
}

/// The mask's v = 0 is the image's top row, as DirectX UVs and the displayed DDS have it: an island at the top
/// left of UV space is at the top left of the overlay.
#[test]
fn the_overlay_puts_uv_v0_at_the_top() {
    use sdc_gui::model::overlay::{uv_overlay_image, UV_USED};
    // An L-shaped island: a bar along the top (v 0..0.1, u 0..0.5) and a leg down the left (u 0..0.1, v 0..0.5).
    let uvs = [
        [0.0, 0.0],
        [0.5, 0.0],
        [0.5, 0.1],
        [0.0, 0.1],
        [0.1, 0.1],
        [0.1, 0.5],
        [0.0, 0.5],
    ];
    let mut mask = UvMask::new(MASK_SIZE);
    rasterize(
        &mut mask,
        &uvs,
        &[[0, 1, 2], [0, 2, 3], [3, 4, 5], [3, 5, 6]],
    );
    let img = uv_overlay_image(&mask);
    let used = |x: usize, y: usize| {
        img.pixels[y * img.size[0] + x]
            == egui::Color32::from_rgba_unmultiplied(UV_USED[0], UV_USED[1], UV_USED[2], UV_USED[3])
    };
    let n = MASK_SIZE as usize;
    assert!(used(n / 4, 10), "the top bar is not at the top");
    assert!(used(10, n / 4), "the left leg is not at the left");
    assert!(!used(n / 4, n - 10), "the bar is at the bottom (v flipped)");
    assert!(!used(n - 10, n / 4), "the leg is at the right (u flipped)");
    assert!(!used(n / 4, n / 4));
}

/// A scan DB of realistic size: `n` winning NIFs with two lighting shapes each, every shape its own texture set
/// (`textures/m{k}/t{i}_{s}.dds` in slot 0, a `_n` in slot 1), so about 4n + n texture and mesh paths. NIF 0's first
/// shape names `TEX`.
fn big_db(dir: &Path, n: i64) -> PathBuf {
    let db = dir.join("big.db");
    let s = store(&db);
    let c = s.conn();
    origin(c, 1, &dir.join("Mod"), 1, false);
    c.execute_batch("BEGIN").unwrap();
    let tex_id: i64 = c
        .query_row("SELECT id FROM path WHERE p = ?1", [TEX], |r| r.get(0))
        .unwrap();
    {
        let mut path = c.prepare("INSERT INTO path(p) VALUES (?1)").unwrap();
        let mut copy = c
            .prepare("INSERT INTO nif_copy(id, path_id, origin_id, parse_status, le_mesh, is_derived) VALUES (?1, ?2, 1, 0, 0, 0)")
            .unwrap();
        let mut win = c
            .prepare("INSERT INTO nif_winner(path_id, copy_id, n_copies) VALUES (?1, ?2, 1)")
            .unwrap();
        let mut ts = c
            .prepare("INSERT INTO texset(id, n, t0, t1) VALUES (?1, 2, ?2, ?3)")
            .unwrap();
        let mut tu = c
            .prepare("INSERT INTO tex_use(tex_path_id, texset_id, slot) VALUES (?1, ?2, ?3)")
            .unwrap();
        let mut sh = c
            .prepare("INSERT INTO shape(copy_id, block, kind, shader_kind, shader_type, slsf2, texset_id) VALUES (?1, ?2, 'BSTriShape', 1, 0, 0, ?3)")
            .unwrap();
        let mut tset = 10i64;
        for i in 0..n {
            path.execute([format!("meshes/m{}/n{i}.nif", i % 97)])
                .unwrap();
            let p = c.last_insert_rowid();
            let id = 100 + i;
            copy.execute(rusqlite::params![id, p]).unwrap();
            win.execute(rusqlite::params![p, id]).unwrap();
            for b in 0..2i64 {
                let t0 = if i == 0 && b == 0 {
                    tex_id
                } else {
                    path.execute([format!("textures/m{}/t{i}_{b}.dds", i % 97)])
                        .unwrap();
                    c.last_insert_rowid()
                };
                path.execute([format!("textures/m{}/t{i}_{b}_n.dds", i % 97)])
                    .unwrap();
                let t1 = c.last_insert_rowid();
                ts.execute(rusqlite::params![tset, t0, t1]).unwrap();
                tu.execute(rusqlite::params![t0, tset, 0]).unwrap();
                tu.execute(rusqlite::params![t1, tset, 1]).unwrap();
                sh.execute(rusqlite::params![id, b, tset]).unwrap();
                tset += 1;
            }
        }
    }
    c.execute_batch("COMMIT").unwrap();
    db
}

/// Item 12 (option c): a texture no mesh uses is answered from the indexed `users_of_texture`, without the
/// `nifs_using_texture` scan (about 137 ms at 375k paths).
#[test]
fn texture_lookups_use_the_index() {
    let d = tempfile::tempdir().unwrap();
    let db = big_db(d.path(), 75_000);
    let _ = winning_shapes(&db, "textures/warm/up.dds"); // open the file once, so the page cache is warm
    let t = std::time::Instant::now();
    let none = winning_shapes(&db, "textures/a/unused.dds").unwrap();
    let ms = t.elapsed().as_millis();
    assert!(none.is_empty());
    assert!(ms < 50, "{ms} ms for an unused texture");
    // A used texture is indexed too (Note CK): no scan of `tex_use`.
    let t = std::time::Instant::now();
    let used = winning_shapes(&db, TEX).unwrap();
    let ms = t.elapsed().as_millis();
    assert!(ms < 50, "{ms} ms for a used texture");
    assert_eq!(used.len(), 1);
    assert_eq!(used[0].nif, "meshes/m0/n0.nif");
    assert_eq!(used[0].blocks, vec![0]);
}

/// CL44 (final review I2): Check all caches the coverage (its 32 KiB mask) only. No 1 MiB overlay image is kept per
/// texture; the expanded item's image is built on demand by an overlay job.
#[test]
fn check_all_over_many_textures_holds_no_overlay_images() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("textures");
    let items: Vec<String> = (0..40).map(|i| format!("a/t{i:02}.png")).collect();
    for p in &items {
        let f = root.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        image::RgbImage::from_pixel(16, 16, image::Rgb([120, 90, 60]))
            .save(f)
            .unwrap();
    }
    let mut m = AppModel::new(GuiSettings::default(), None, support::session());
    m.scan_db = Some(real_db(d.path()));
    m.open_folder(root);
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.uv.read = |_, _, _, _| {
        Ok(Coverage {
            mask: Arc::new(UvMask::new(MASK_SIZE)),
            used: 0.0,
            gutter: None,
            nifs: 1,
            errors: vec![],
        })
    };
    m.check_all_uv();
    assert!(m.wait_idle(Duration::from_secs(20)));
    for i in &items {
        assert!(m.uv_coverage(i).is_some(), "{i} was not read");
        assert!(m.uv_overlay(i).is_none(), "{i} holds an overlay image");
    }
    assert!(!m.overlay_pending() && m.overlay.image.is_none());
    // Expanded: the image is built by a worker job, and only for that item.
    m.expand(&items[0]);
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(m.uv_overlay(&items[0]).is_none(), "built on the UI thread");
    assert!(m.overlay_pending());
    assert!(m.wait_idle(Duration::from_secs(10)));
    assert_eq!(m.uv_overlay(&items[0]).map(|i| i.size), Some([512, 512]));
    assert!(m.uv_overlay(&items[1]).is_none());
}
