//! `parse`: pure NIF → per-shape data, keyed by block index.
use nif::scan::{scan, Shader};
use render3d::{parse, ParsedNif, ShapeId};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../nif/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

#[test]
fn parsed_nif_is_send_and_sync() {
    fn check<T: Send + Sync + 'static>() {}
    check::<ParsedNif>();
}

#[test]
fn deep_graph_shapes_are_keyed_by_block() {
    let b = fixture("TestNifFile_DeepGraph_SE.nif");
    let p = parse(&b);
    assert!(!p.le_mesh);
    assert!(p.errors.is_empty(), "{:?}", p.errors);
    let s = scan(&b).unwrap();
    let lighting: Vec<u32> = s
        .shapes
        .iter()
        .filter(|r| matches!(r.shader, Shader::Lighting(..)))
        .map(|r| r.block)
        .collect();
    assert_eq!(lighting.len(), 27);
    assert_eq!(
        p.shapes.iter().map(|x| x.id.0).collect::<Vec<_>>(),
        lighting
    );
    assert_eq!(p.shapes.len() + p.skipped.len(), s.shapes.len());
    let skull = p.shapes.iter().find(|x| x.name == "SkullPeg01:19").unwrap();
    assert_eq!(skull.id, ShapeId(143));
    assert!(
        skull.material.alpha.is_none(),
        "the one shape without NiAlphaProperty"
    );
    for x in &p.shapes {
        assert_eq!(x.material.slots.len(), nif::material::SLOT_COUNT);
        assert!(render3d::mesh::validate(&x.mesh).is_ok(), "{}", x.name);
        assert!(!x.material.hidden);
    }
}

#[test]
fn furniture_names_and_materials() {
    let b = fixture("TestNifFile_Furniture_Col_SE.nif");
    let p = parse(&b);
    let names: Vec<(u32, &str)> = p.shapes.iter().map(|x| (x.id.0, x.name.as_str())).collect();
    assert_eq!(names, [(9, "cylinder_1"), (12, "cylinder_2")]);
    let s = scan(&b).unwrap();
    for x in &p.shapes {
        let row = s.shapes.iter().find(|r| r.block == x.id.0).unwrap();
        let Shader::Lighting(l, slots) = &row.shader else {
            panic!("not lighting")
        };
        let want = nif::material::MaterialState::from_lighting(&row.head, l, slots, row.alpha);
        assert_eq!(x.material, want);
    }
}

#[test]
fn le_mesh_parses_to_nothing() {
    let p = parse(&fixture("TestNifFile_Animated_LE.nif"));
    assert!(p.le_mesh);
    assert!(p.shapes.is_empty());
    assert!(p.errors.is_empty(), "{:?}", p.errors);
}

#[test]
fn garbage_is_an_error_not_a_panic() {
    let p = parse(b"not a nif at all");
    assert!(p.shapes.is_empty());
    assert!(
        p.errors.iter().any(|e| e.starts_with("NIF: ")),
        "{:?}",
        p.errors
    );
}
