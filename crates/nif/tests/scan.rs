mod builder;
use builder::scenes::{avobj, bstri, lsp, net, texset};
use builder::Nif;
use nif::scan::{scan, Shader};
use nif::shape_head::ShapeKind;

#[test]
fn bstrishape_links_shader_texset_alpha() {
    let mut n = Nif::new(100);
    let nm = n.string("Torso");
    // block 0 shape, 1 lsp, 2 texset, 3 alpha
    let shape = avobj(net(nm))
        .fs(&[0.0, 0.0, 0.0, 10.0])
        .i32(-1)
        .i32(1)
        .i32(3)
        .u64(0x0000_B000_0000_0007)
        .u16(12)
        .u16(30)
        .u32(0)
        .raw(&[0u8; 16]);
    n.block("BSTriShape", shape.0);
    n.block("BSLightingShaderProperty", lsp(0, 2));
    n.block(
        "BSShaderTextureSet",
        texset(&[
            "textures\\a_d.dds",
            "textures\\a_n.dds",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
        ]),
    );
    n.block("NiAlphaProperty", net(-1).u16(0x00ED).u8(128).0);
    let s = scan(&n.bytes()).unwrap();
    assert_eq!((s.bsver, s.le_mesh), (100, false));
    assert_eq!(s.shapes.len(), 1);
    let r = &s.shapes[0];
    assert_eq!(r.head.kind, ShapeKind::BSTriShape);
    assert_eq!(r.head.name.as_deref(), Some("Torso"));
    assert_eq!(
        (r.head.num_triangles, r.head.num_vertices),
        (Some(12), Some(30))
    );
    match &r.shader {
        Shader::Lighting(l, slots) => {
            assert_eq!(l.shader_type, 0);
            assert_eq!(slots[1].norm, "textures/a_n.dds");
            assert_eq!(slots.len(), 9);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(r.alpha.unwrap().threshold, 128);
}

#[test]
fn nitrishape_le_layout_and_flag() {
    let mut n = Nif::new(83);
    let shape = avobj(net(-1))
        .i32(-1)
        .i32(-1)
        .u32(0)
        .i32(-1)
        .u8(0)
        .i32(1)
        .i32(-1);
    n.block("NiTriShape", shape.0);
    n.block(
        "BSLightingShaderProperty",
        lsp(1, -1).into_iter().chain(0.5f32.to_le_bytes()).collect(),
    );
    let s = scan(&n.bytes()).unwrap();
    assert!(s.le_mesh);
    assert!(
        matches!(&s.shapes[0].shader, Shader::Lighting(l, slots) if l.shader_type == 1 && slots.is_empty())
    );
}

#[test]
fn particle_system_owns_effect_shader() {
    let mut n = Nif::new(100);
    let ps = avobj(net(-1))
        .fs(&[0.0; 4])
        .i32(-1)
        .i32(1)
        .i32(-1)
        .raw(&[0u8; 20]);
    n.block("NiParticleSystem", ps.0);
    let esp = net(-1)
        .u32(0)
        .u32(0)
        .fs(&[0.0, 0.0])
        .fs(&[1.0, 1.0])
        .sized("textures\\fx\\a.dds")
        .u8(3)
        .u8(0)
        .u8(0)
        .u8(0)
        .fs(&[0.0; 4])
        .fs(&[1.0; 4])
        .f(1.0)
        .f(100.0)
        .sized("");
    n.block("BSEffectShaderProperty", esp.0);
    let s = scan(&n.bytes()).unwrap();
    assert!(
        matches!(&s.shapes[0].shader, Shader::Effect(e) if e.source.norm == "textures/fx/a.dds" && e.greyscale.flags.empty)
    );
}

#[test]
fn bad_property_block_is_a_problem_not_a_failure() {
    let mut n = Nif::new(100);
    let shape = avobj(net(-1))
        .fs(&[0.0; 4])
        .i32(-1)
        .i32(1)
        .i32(-1)
        .u64(0)
        .u16(0)
        .u16(0)
        .u32(0);
    n.block("BSTriShape", shape.0);
    n.block("BSLightingShaderProperty", {
        let mut v = lsp(0, -1);
        v.extend([0u8; 4]);
        v
    });
    let s = scan(&n.bytes()).unwrap();
    assert!(matches!(s.shapes[0].shader, Shader::None));
    assert_eq!(s.problems.len(), 1);
}

#[test]
fn ref_out_of_range_is_a_problem() {
    let mut n = Nif::new(100);
    let shape = avobj(net(-1))
        .fs(&[0.0; 4])
        .i32(-1)
        .i32(42)
        .i32(-1)
        .u64(0)
        .u16(0)
        .u16(0)
        .u32(0);
    n.block("BSTriShape", shape.0);
    let s = scan(&n.bytes()).unwrap();
    assert!(matches!(s.shapes[0].shader, Shader::None));
    assert_eq!(s.problems.len(), 1);
}

#[test]
fn bad_texture_set_ref_is_a_problem() {
    // out of range
    let mut n = Nif::new(100);
    n.block("BSTriShape", bstri(1, -1));
    n.block("BSLightingShaderProperty", lsp(0, 99));
    let s = scan(&n.bytes()).unwrap();
    assert!(matches!(&s.shapes[0].shader, Shader::Lighting(_, t) if t.is_empty()));
    assert_eq!(s.problems.len(), 1, "{:?}", s.problems);
    assert!(s.problems[0].contains("texture set"), "{:?}", s.problems);
    // wrong kind (points at the alpha block)
    let mut n = Nif::new(100);
    n.block("BSTriShape", bstri(1, 2));
    n.block("BSLightingShaderProperty", lsp(0, 2));
    n.block("NiAlphaProperty", net(-1).u16(0x00ED).u8(128).0);
    let s = scan(&n.bytes()).unwrap();
    assert_eq!(s.problems.len(), 1, "{:?}", s.problems);
    assert!(
        s.problems[0].contains("NiAlphaProperty"),
        "{:?}",
        s.problems
    );
    // -1 is "no texture set", not a problem
    let mut n = Nif::new(100);
    n.block("BSTriShape", bstri(1, -1));
    n.block("BSLightingShaderProperty", lsp(0, -1));
    assert!(scan(&n.bytes()).unwrap().problems.is_empty());
}

#[test]
fn bad_alpha_ref_is_a_problem() {
    let mut n = Nif::new(100);
    n.block("BSTriShape", bstri(-1, 7));
    let s = scan(&n.bytes()).unwrap();
    assert!(s.shapes[0].alpha.is_none());
    assert_eq!(s.problems.len(), 1, "{:?}", s.problems);
    let mut n = Nif::new(100);
    n.block("BSTriShape", bstri(-1, 1));
    n.block("BSLightingShaderProperty", lsp(0, -1));
    let s = scan(&n.bytes()).unwrap();
    assert!(s.shapes[0].alpha.is_none());
    assert_eq!(s.problems.len(), 1, "{:?}", s.problems);
    assert!(s.problems[0].contains("alpha"), "{:?}", s.problems);
}

/// Note GB4: the shader is decoded once per property, but a bad ref in it is still reported once per shape.
#[test]
fn a_bad_texture_set_ref_is_reported_once_per_shape() {
    let mut n = Nif::new(100);
    n.block("BSLightingShaderProperty", builder::scenes::lsp(0, 99));
    for _ in 0..3 {
        n.block("BSTriShape", builder::scenes::bstri(0, -1));
    }
    n.roots = vec![];
    let s = scan(&n.bytes()).unwrap();
    assert_eq!(s.shapes.len(), 3);
    assert_eq!(
        s.problems,
        vec!["block 0: texture set ref 99 out of range"; 3]
    );
}
