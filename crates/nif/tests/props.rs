// crates/nif/tests/props.rs
mod builder;
use builder::{Nif, W};
use nif::header::Header;
use nif::props::*;
use nif::NifError;

/// BSLightingShaderProperty body (plan 01 Q2 table), no extra data, type-specific trailer appended by caller.
fn lsp(shader_type: u32, name: i32, texset: i32) -> W {
    W::default()
        .u32(shader_type)
        .i32(name)
        .u32(0)
        .i32(-1)
        .u32(0x8000_0001)
        .u32(0x0000_0021) // SLSF1, SLSF2
        .fs(&[0.5, 0.25])
        .fs(&[2.0, 2.0]) // uv offset, uv scale
        .i32(texset)
        .fs(&[0.1, 0.2, 0.3])
        .f(1.5) // emissive colour, multiple
        .u32(3)
        .f(1.0)
        .f(0.0)
        .f(80.0) // clamp, alpha, refraction, glossiness
        .fs(&[1.0, 0.9, 0.8])
        .f(2.0) // specular colour, strength
        .f(0.3)
        .f(2.0) // lighting effect 1, 2
}

#[test]
#[allow(clippy::type_complexity)]
fn lighting_default_and_trailers() {
    let cases: Vec<(u32, Vec<f32>, fn(&LightingTrailer) -> bool)> = vec![
        (0, vec![], |t| matches!(t, LightingTrailer::None)),
        (
            1,
            vec![0.7],
            |t| matches!(t, LightingTrailer::EnvMap { scale } if *scale == 0.7),
        ),
        (
            5,
            vec![1.0, 0.5, 0.25],
            |t| matches!(t, LightingTrailer::SkinTint(c) if c[1] == 0.5),
        ),
        (6, vec![1.0, 0.5, 0.25], |t| {
            matches!(t, LightingTrailer::HairTint(_))
        }),
        (7, vec![4.0, 0.1], |t| {
            matches!(t, LightingTrailer::ParallaxOcc { .. })
        }),
        (
            11,
            vec![1.0, 2.0, 3.0, 4.0, 5.0],
            |t| matches!(t, LightingTrailer::MultiLayer { envmap_strength, .. } if *envmap_strength == 5.0),
        ),
        (14, vec![1.0, 2.0, 3.0, 4.0], |t| {
            matches!(t, LightingTrailer::SparkleSnow(_))
        }),
        (
            16,
            vec![1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            |t| matches!(t, LightingTrailer::Eye { right_center, .. } if right_center[2] == 1.0),
        ),
        (2, vec![], |t| matches!(t, LightingTrailer::None)),
    ];
    for (st, trailer, check) in cases {
        let mut n = Nif::new(100);
        let name = n.string("mat");
        n.block("BSLightingShaderProperty", lsp(st, name, 1).fs(&trailer).0);
        let b = n.bytes();
        let h = Header::parse(&b).unwrap();
        let l = decode_lighting(&h, &b, 0).unwrap_or_else(|e| panic!("type {st}: {e}"));
        assert_eq!(l.shader_type, st);
        assert_eq!((l.slsf1, l.slsf2, l.texture_set), (0x8000_0001, 0x21, 1));
        assert_eq!(l.name.as_deref(), Some("mat"));
        assert_eq!(l.params.uv_scale, [2.0, 2.0]);
        assert_eq!(l.params.glossiness, 80.0);
        assert!(check(&l.params.trailer), "type {st}");
    }
}

#[test]
fn lighting_with_extra_data_refs() {
    let mut n = Nif::new(100);
    let body = W::default()
        .u32(0)
        .i32(-1)
        .u32(2)
        .i32(5)
        .i32(6)
        .i32(-1)
        .raw(&lsp(0, -1, 3).0[16..]); // reuse the tail after the NiObjectNET prefix
    n.block("BSLightingShaderProperty", body.0);
    let b = n.bytes();
    let h = Header::parse(&b).unwrap();
    let l = decode_lighting(&h, &b, 0).unwrap();
    assert_eq!(l.texture_set, 3);
    assert_eq!(l.name, None);
}

#[test]
fn wrong_declared_size_is_reported() {
    let mut n = Nif::new(100);
    n.block("BSLightingShaderProperty", lsp(0, -1, 1).u32(0).0); // 4 stray bytes
    let b = n.bytes();
    let h = Header::parse(&b).unwrap();
    assert!(
        matches!(decode_lighting(&h, &b, 0), Err(NifError::BlockSize { declared, consumed, .. }) if declared == consumed + 4)
    );
}

#[test]
fn texture_set_reads_its_count() {
    for count in [9u32, 6, 1, 0] {
        let mut w = W::default().u32(count);
        for i in 0..count {
            w = w.sized(if i == 0 { "textures\\a_d.dds" } else { "" });
        }
        let mut n = Nif::new(100);
        n.block("BSShaderTextureSet", w.0);
        let b = n.bytes();
        let h = Header::parse(&b).unwrap();
        let t = decode_texture_set(&h, &b, 0).unwrap();
        assert_eq!(t.slots.len(), count as usize);
        if count > 0 {
            assert_eq!(t.slots[0].norm, "textures/a_d.dds");
        }
    }
}

/// Audit 2026-10-14 finding 1: a slot count past `MAX_TEXTURE_SLOTS` is refused before any slot is kept.
#[test]
fn texture_set_with_an_absurd_slot_count_is_an_error() {
    let mut n = Nif::new(100);
    n.block("BSShaderTextureSet", W::default().u32(2_000_000).0);
    let b = n.bytes();
    let h = Header::parse(&b).unwrap();
    assert!(matches!(
        decode_texture_set(&h, &b, 0),
        Err(NifError::Geometry { block: 0, msg }) if msg.contains("2000000 texture slots")
    ));
}

#[test]
fn effect_has_two_inline_strings() {
    let body = W::default()
        .i32(-1)
        .u32(0)
        .i32(-1)
        .u32(1)
        .u32(2)
        .fs(&[0.0, 0.0])
        .fs(&[1.0, 1.0])
        .sized("textures\\effects\\fx.dds")
        .u8(3)
        .u8(255)
        .u8(0)
        .u8(0)
        .fs(&[90.0, 0.0, 1.0, 0.0])
        .fs(&[1.0, 1.0, 1.0, 1.0])
        .f(2.0)
        .f(100.0)
        .sized("textures\\effects\\gradients\\g.dds");
    let mut n = Nif::new(100);
    n.block("BSEffectShaderProperty", body.0);
    let b = n.bytes();
    let h = Header::parse(&b).unwrap();
    let e = decode_effect(&h, &b, 0).unwrap();
    assert_eq!(e.source.norm, "textures/effects/fx.dds");
    assert_eq!(e.greyscale.norm, "textures/effects/gradients/g.dds");
    assert_eq!(e.params.base_color_scale, 2.0);
    assert_eq!(e.params.lighting_influence, 255);
}

#[test]
fn alpha_flags() {
    let mut n = Nif::new(100);
    n.block(
        "NiAlphaProperty",
        W::default().i32(-1).u32(0).i32(-1).u16(0x12ED).u8(128).0,
    );
    let b = n.bytes();
    let h = Header::parse(&b).unwrap();
    let a = decode_alpha(&h, &b, 0).unwrap();
    assert_eq!(a.threshold, 128);
    assert!(a.blend() && a.test());
}
