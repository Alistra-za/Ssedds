mod builder;
use builder::scenes::{lsp_body, one_block_nif};
use builder::W;
use nif::fields::{locate, Field, Value};
use nif::header::Header;
use nif::net::{encode_net, read_net};
use nif::NifError;

#[test]
fn bslsp_prefix_keeps_extra_list_with_nulls_and_controller() {
    let b = one_block_nif(
        "BSLightingShaderProperty",
        lsp_body(1, &[5, -1], 7, &[0.75]),
    );
    let h = Header::parse(&b).unwrap();
    let n = read_net(&h, &b, 0).unwrap();
    assert_eq!(
        (n.name, n.extra.clone(), n.controller),
        (-1, vec![5, -1], 7)
    );
    assert_eq!(n.start, h.offsets()[0] + 4);
    assert_eq!(n.end, n.start + 4 + 4 + 8 + 4);
    assert_eq!(n.extra_at(1), n.start + 12);
    assert_eq!(n.controller_at(), n.end - 4);
    let (at, v) = locate(&h, &b, 0, Field::Trailer(0)).unwrap();
    assert_eq!(v, Value::F32(0.75f32.to_bits()));
    assert_eq!(at, (n.end - h.offsets()[0]) + 84);
    assert!(locate(&h, &b, 0, Field::Trailer(1)).is_err());
    assert_eq!(
        locate(&h, &b, 0, Field::ShaderType).unwrap(),
        (0, Value::U32(1))
    );
}

#[test]
fn alpha_flags_and_threshold_offsets() {
    let body = W::default()
        .raw(&encode_net(-1, &[], -1))
        .u16(4845)
        .u8(128)
        .0;
    let b = one_block_nif("NiAlphaProperty", body);
    let h = Header::parse(&b).unwrap();
    assert_eq!(
        locate(&h, &b, 0, Field::AlphaFlags).unwrap(),
        (12, Value::U16(4845))
    );
    assert_eq!(
        locate(&h, &b, 0, Field::AlphaThreshold).unwrap(),
        (14, Value::U8(128))
    );
    assert!(locate(&h, &b, 0, Field::Glossiness).is_err());
}

#[test]
fn extra_count_past_block_is_an_error() {
    let body = W::default().i32(-1).u32(1000).i32(-1).i32(-1).0;
    assert_eq!(body.len(), 16);
    let b = one_block_nif("NiAlphaProperty", body);
    let h = Header::parse(&b).unwrap();
    assert!(matches!(
        read_net(&h, &b, 0),
        Err(NifError::Geometry { .. })
    ));
    assert!(matches!(
        locate(&h, &b, 0, Field::AlphaFlags),
        Err(NifError::Geometry { .. })
    ));
}

#[test]
fn negative_zero_and_huge_floats_read_as_exact_bits() {
    let mut body = lsp_body(1, &[], -1, &[1.0]);
    // P = 4 (shader type) + 12 (net) = 16; emissive multiple P+40, glossiness P+56
    body[56..60].copy_from_slice(&(-0.0f32).to_le_bytes());
    body[72..76].copy_from_slice(&1e38f32.to_le_bytes());
    let b = one_block_nif("BSLightingShaderProperty", body);
    let h = Header::parse(&b).unwrap();
    assert_eq!(
        locate(&h, &b, 0, Field::EmissiveMultiple).unwrap(),
        (56, Value::F32((-0.0f32).to_bits()))
    );
    let (_, g) = locate(&h, &b, 0, Field::Glossiness).unwrap();
    assert_eq!(g, Value::F32(1e38f32.to_bits()));
    assert_eq!(g.as_f32(), Some(1e38));
    assert_eq!(g.bytes(), 1e38f32.to_le_bytes());
}

#[test]
fn encode_net_round_trips() {
    let body = encode_net(3, &[1, -1], 9);
    let b = one_block_nif("NiAlphaProperty", body);
    let h = Header::parse(&b).unwrap();
    let n = read_net(&h, &b, 0).unwrap();
    assert_eq!((n.name, n.extra, n.controller), (3, vec![1, -1], 9));
}

#[test]
fn out_of_range_block_is_an_error_not_a_panic() {
    let b = one_block_nif("NiAlphaProperty", encode_net(-1, &[], -1));
    let h = Header::parse(&b).unwrap();
    assert!(matches!(
        read_net(&h, &b, 1),
        Err(NifError::Geometry { .. })
    ));
    assert!(matches!(
        locate(&h, &b, 1, Field::AlphaFlags),
        Err(NifError::Geometry { .. })
    ));
}

#[test]
fn trailer_counts_per_shader_type() {
    for (t, n) in [
        (1u32, 1u8),
        (5, 3),
        (6, 3),
        (7, 2),
        (11, 5),
        (14, 4),
        (16, 7),
    ] {
        let body = lsp_body(t, &[], -1, &vec![0.5; n as usize]);
        let b = one_block_nif("BSLightingShaderProperty", body);
        let h = Header::parse(&b).unwrap();
        assert!(locate(&h, &b, 0, Field::Trailer(n - 1)).is_ok(), "type {t}");
        assert!(locate(&h, &b, 0, Field::Trailer(n)).is_err(), "type {t}");
    }
}
