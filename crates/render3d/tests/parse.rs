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

/// Trace 2026-10-28 #1 fix round 2: 2000 zero-size BSTriShapes (not Lighting shapes, so each gets a warning and a
/// skipped entry) all named by one 100 000-byte string of 0x80 (`€` decoded). The messages carry a short label, so
/// the text stays small (about 1.2 GB with the full name).
#[test]
fn skipped_shape_messages_use_a_short_label() {
    let mut b = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    b.extend(0x1402_0007u32.to_le_bytes());
    b.push(1);
    b.extend(12u32.to_le_bytes());
    b.extend(2000u32.to_le_bytes());
    b.extend(100u32.to_le_bytes());
    b.extend([1u8, 0, 1, 0, 1, 0]); // author, process and export script
    b.extend(1u16.to_le_bytes());
    b.extend(10u32.to_le_bytes());
    b.extend(b"BSTriShape");
    b.extend([0u8; 2 * 2000]); // type indices
    b.extend([0u8; 4 * 2000]); // sizes: every block starts at the footer
    b.extend(31u32.to_le_bytes());
    b.extend(100_000u32.to_le_bytes());
    for k in 0..30 {
        b.extend(1u32.to_le_bytes());
        b.push(b'a' + k as u8);
    }
    b.extend(100_000u32.to_le_bytes());
    b.extend(vec![0x80u8; 100_000]);
    b.extend(0u32.to_le_bytes()); // groups
    b.extend(30u32.to_le_bytes()); // footer: 30 roots, read as name index 30
    b.extend([0u8; 4 * 30]);
    let p = parse(&b);
    assert_eq!(p.skipped.len(), 2000);
    let texts = || {
        p.warnings
            .iter()
            .chain(&p.errors)
            .chain(p.skipped.iter().map(|s| &s.1))
    };
    let total: usize = texts().map(String::len).sum();
    assert!(total < 4 << 20, "{total} bytes of message text");
    for t in texts().filter(|t| t.starts_with('€')) {
        let label = t.split(": ").next().unwrap();
        assert!(
            label.chars().count() <= nif::text::LABEL_CAP + 1,
            "{}",
            label.len()
        );
        assert!(label.ends_with('…'));
    }
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
    let skull = p
        .shapes
        .iter()
        .find(|x| &*x.name == "SkullPeg01:19")
        .unwrap();
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
    let names: Vec<(u32, &str)> = p.shapes.iter().map(|x| (x.id.0, &*x.name)).collect();
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
