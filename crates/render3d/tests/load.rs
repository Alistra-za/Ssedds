//! The restructured Renderer: shapes keyed by block, `load` = `load_parsed(&parse(..))`.
mod common;
use common::{fixture, renderer};
use render3d::scene::Scene;
use render3d::{parse, ShapeId, ShapeStatus};

#[test]
fn load_lists_every_lighting_shape_by_block() {
    let cache = tempfile::tempdir().unwrap();
    // No STANDIN_OPAQUE_OK: the one shape without an NiAlphaProperty fails to compile (tests/standin.rs).
    let mut r = renderer(cache.path(), &[]);
    let rep = r.load(&fixture("TestNifFile_DeepGraph_SE.nif"), &|_| None);
    assert!(rep.errors.is_empty(), "{:?}", rep.errors);
    assert_eq!(rep.compile_errors.len(), 1, "{:?}", rep.compile_errors);
    assert!(rep.compile_errors[0].starts_with("SkullPeg01:19: ps:"));
    let shapes = r.shapes();
    assert_eq!(shapes.len(), 27);
    assert!(shapes.windows(2).all(|w| w[0].id < w[1].id), "id order");
    let skull = shapes.iter().find(|s| s.id == ShapeId(143)).unwrap();
    assert_eq!(skull.name, "SkullPeg01:19");
    assert!(
        matches!(&skull.status, ShapeStatus::Failed(e) if e.starts_with("ps:")),
        "{:?}",
        skull.status
    );
    let drawing = shapes
        .iter()
        .filter(|s| s.status == ShapeStatus::Drawing)
        .count();
    assert_eq!(drawing, 26);
    assert_eq!(rep.shapes, drawing);
    assert_eq!(shapes[0].material.slots.len(), 9);
}

#[test]
fn load_parsed_matches_load() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), &["STANDIN_OPAQUE_OK"]);
    let nif = fixture("TestNifFile_Furniture_Col_SE.nif");
    let a = r.load(&nif, &|_| None);
    let pa = r.render(&Scene::neutral(), 64, 64).unwrap();
    let b = r.load_parsed(&parse(&nif), &|_| None);
    let pb = r.render(&Scene::neutral(), 64, 64).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.shapes, 2);
    assert_eq!(pa, pb);
    assert_eq!(
        r.shapes().iter().map(|s| s.id).collect::<Vec<_>>(),
        [ShapeId(9), ShapeId(12)]
    );
}

#[test]
fn a_new_load_replaces_every_shape() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), &["STANDIN_OPAQUE_OK"]);
    r.load(&fixture("TestNifFile_DeepGraph_SE.nif"), &|_| None);
    r.load(&fixture("TestNifFile_Furniture_Col_SE.nif"), &|_| None);
    let names: Vec<String> = r.shapes().into_iter().map(|s| s.name).collect();
    assert_eq!(names, ["cylinder_1", "cylinder_2"]);
}

#[test]
fn le_mesh_loads_nothing() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), &["STANDIN_OPAQUE_OK"]);
    let rep = r.load(&fixture("TestNifFile_Animated_LE.nif"), &|_| None);
    assert!(rep.le_mesh);
    assert_eq!(rep.shapes, 0);
    assert!(r.shapes().is_empty());
    assert!(r.bounds().is_none());
}

#[test]
fn hidden_at_load_is_listed_but_not_drawn() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), &["STANDIN_OPAQUE_OK"]);
    let mut p = parse(&fixture("TestNifFile_Furniture_Col_SE.nif"));
    p.shapes[0].material.hidden = true;
    let rep = r.load_parsed(&p, &|_| None);
    assert_eq!(rep.shapes, 1);
    let s = r.shapes();
    assert_eq!(s[0].status, ShapeStatus::Hidden);
    assert_eq!(s[1].status, ShapeStatus::Drawing);
}
