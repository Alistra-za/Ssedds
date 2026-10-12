// MaterialState over nifly's GPL-3.0 fixtures and synthetic heads.
use nif::material::{empty_slot, MaterialState, SLOT_COUNT};
use nif::props::{Alpha, LightingTrailer};
use nif::scan::{scan, Shader};
use nif::shape_head::{ShapeHead, ShapeKind};

fn read(f: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/fixtures/{f}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn head(vertex_desc: Option<u64>, skinned: bool) -> ShapeHead {
    ShapeHead {
        kind: ShapeKind::BSTriShape,
        name: Some("x".into()),
        skinned,
        shader: 1,
        alpha: -1,
        vertex_desc,
        num_triangles: Some(1),
        num_vertices: Some(3),
    }
}

#[test]
fn from_lighting_copies_the_nif_fields() {
    let b = read("TestNifFile_Furniture_Col_SE.nif");
    let s = scan(&b).unwrap();
    let mut n = 0;
    for row in &s.shapes {
        let Shader::Lighting(l, slots) = &row.shader else {
            continue;
        };
        let m = MaterialState::from_lighting(&row.head, l, slots, row.alpha);
        assert_eq!(m.slots.len(), SLOT_COUNT);
        for (i, p) in slots.iter().take(SLOT_COUNT).enumerate() {
            assert_eq!(&m.slots[i], p, "slot {i}");
        }
        assert_eq!(
            (m.shader_type, m.slsf1, m.slsf2),
            (l.shader_type, l.slsf1, l.slsf2)
        );
        assert_eq!(m.params, l.params);
        assert_eq!(m.alpha, row.alpha);
        assert_eq!(m.skinned, row.head.skinned);
        // VertexAttribute Vertex_Colors = bit 5 of the flags at bit 44 (nif.xml BSVertexDesc).
        let vc = row.head.vertex_desc.is_some_and(|d| (d >> 44) & 0x20 != 0);
        assert_eq!(m.has_vertex_colors, vc);
        assert!(!m.hidden);
        n += 1;
    }
    assert_eq!(n, 2, "cylinder_1 and cylinder_2");
}

#[test]
fn slots_are_padded_and_truncated_to_nine() {
    let b = read("TestNifFile_Furniture_Col_SE.nif");
    let s = scan(&b).unwrap();
    let (row, l, slots) = s
        .shapes
        .iter()
        .find_map(|r| match &r.shader {
            Shader::Lighting(l, slots) => Some((r, l, slots)),
            _ => None,
        })
        .unwrap();
    let short = MaterialState::from_lighting(&row.head, l, &slots[..1], None);
    assert_eq!(short.slots.len(), SLOT_COUNT);
    assert_eq!(short.slots[0], slots[0]);
    assert!(short.slots[1..].iter().all(|p| *p == empty_slot()));
    let long: Vec<_> = (0..11)
        .map(|i| nif::path::canon_texture(format!("t{i}.dds").as_bytes()))
        .collect();
    let m = MaterialState::from_lighting(&row.head, l, &long, None);
    assert_eq!(m.slots, long[..SLOT_COUNT].to_vec());
}

#[test]
fn vertex_colours_come_from_the_vertex_desc() {
    let b = read("TestNifFile_Furniture_Col_SE.nif");
    let s = scan(&b).unwrap();
    let l = s
        .shapes
        .iter()
        .find_map(|r| match &r.shader {
            Shader::Lighting(l, _) => Some(l.clone()),
            _ => None,
        })
        .unwrap();
    // VF_Vertex | VF_UVs | VF_Normals | VF_Tangents | VF_Vertex_Colors = 0x3B at bit 44.
    let with =
        MaterialState::from_lighting(&head(Some(0x3Bu64 << 44 | 0x408), true), &l, &[], None);
    assert!(with.has_vertex_colors && with.skinned);
    let without =
        MaterialState::from_lighting(&head(Some(0x1Bu64 << 44 | 0x408), false), &l, &[], None);
    assert!(!without.has_vertex_colors && !without.skinned);
    assert!(!MaterialState::from_lighting(&head(None, false), &l, &[], None).has_vertex_colors);
}

#[test]
fn builtin_default_is_neutral() {
    let m = MaterialState::builtin_default();
    // nif.xml SkyrimShaderPropertyFlags1: Specular 0, Recieve_Shadows 8, Cast_Shadows 9; Flags2: ZBuffer_Write 0.
    assert_eq!(m.slsf1, 1 | 1 << 8 | 1 << 9);
    assert_eq!(m.slsf2, 1);
    assert_eq!(m.shader_type, 0);
    assert!(!m.is_pbr() && !m.blend() && !m.hidden && !m.skinned && !m.has_vertex_colors);
    assert_eq!(m.alpha, None);
    let p = &m.params;
    assert_eq!(p.uv_scale, [1.0, 1.0]);
    assert_eq!(p.uv_offset, [0.0, 0.0]);
    assert_eq!(p.alpha, 1.0);
    assert_eq!(p.glossiness, 80.0);
    assert_eq!(p.specular_color, [1.0; 3]);
    assert_eq!(p.specular_strength, 1.0);
    assert_eq!(p.lighting_effect_1, 0.3);
    assert_eq!(p.lighting_effect_2, 2.0);
    assert_eq!(p.emissive_color, [0.0; 3]);
    assert_eq!(p.emissive_multiple, 1.0);
    assert_eq!(p.trailer, LightingTrailer::None);
    assert_eq!(m.slots, vec![empty_slot(); SLOT_COUNT]);
}

#[test]
fn pbr_and_blend_flags() {
    let mut m = MaterialState::builtin_default();
    m.slsf2 |= 1 << 23;
    assert!(m.is_pbr());
    m.alpha = Some(Alpha {
        flags: 0x0200,
        threshold: 128,
    });
    assert!(!m.blend(), "test-only alpha does not blend");
    m.alpha = Some(Alpha {
        flags: 0x0201,
        threshold: 128,
    });
    assert!(m.blend());
}

#[test]
fn set_slot_raw_keeps_raw_and_recomputes_norm() {
    let mut m = MaterialState::builtin_default();
    m.set_slot_raw(0, "Textures\\PBR\\Foo\\Bar.DDS");
    assert_eq!(m.slots[0].raw, "Textures\\PBR\\Foo\\Bar.DDS");
    assert_eq!(m.slots[0].norm, "textures/pbr/foo/bar.dds");
    m.set_slot_raw(0, "");
    assert_eq!(m.slots[0], empty_slot());
    assert!(m.slots[0].flags.empty);
}

#[test]
#[should_panic(expected = "slot 9 out of range")]
fn set_slot_raw_past_nine_panics() {
    MaterialState::builtin_default().set_slot_raw(9, "x.dds");
}
