mod builder;
use builder::scenes::{self, one_block_nif};
use nif::extra::{encode_extra, read_extra, ExtraValue};
use nif::header::Header;
use nif::props::{decode_texture_set_raw, encode_texture_set, MAX_TEXTURE_SLOTS};
use nif::NifError;

#[test]
fn every_editable_extra_round_trips_through_its_encoder() {
    let b = scenes::all_extras();
    let h = Header::parse(&b).unwrap();
    for i in 0..h.sizes.len() {
        let e = read_extra(&h, &b, i).unwrap();
        let Some(v) = &e.value else {
            assert_eq!(e.kind, "NiStringsExtraData");
            continue;
        };
        let at = h.offsets()[i];
        let name = i32::from_le_bytes(b[at..at + 4].try_into().unwrap());
        let str_index = if let ExtraValue::Str(_) = v {
            i32::from_le_bytes(b[at + 4..at + 8].try_into().unwrap())
        } else {
            -1
        };
        let (kind, body) = encode_extra(name, v, str_index);
        assert_eq!(kind, e.kind);
        assert_eq!(body.as_slice(), &b[at..at + h.sizes[i] as usize]);
    }
}

#[test]
fn extra_values_and_read_only_row() {
    let b = scenes::all_extras();
    let h = Header::parse(&b).unwrap();
    let v = |i| read_extra(&h, &b, i).unwrap();
    assert_eq!(v(0).value, Some(ExtraValue::Bsx(0x83)));
    assert_eq!(v(0).name.as_deref(), Some(&b"BSX"[..]));
    assert_eq!(v(1).value, Some(ExtraValue::Str("a\r\nb".into())));
    assert_eq!(v(2).value, Some(ExtraValue::Int(7)));
    assert_eq!(v(3).value, Some(ExtraValue::Bool(1)));
    assert_eq!(v(4).value, Some(ExtraValue::Float((-0.0f32).to_bits())));
    assert_eq!(
        v(5).value,
        Some(ExtraValue::Inv {
            rot: [4712, 6283, 0],
            zoom: 1.0f32.to_bits()
        })
    );
    let ro = v(6);
    assert_eq!((ro.value, ro.kind.as_str()), (None, "NiStringsExtraData"));
    assert_eq!(ro.size, h.sizes[6]);
}

#[test]
fn extra_refuses_bad_blocks_and_sizes() {
    let b = scenes::all_extras();
    let h = Header::parse(&b).unwrap();
    assert!(matches!(
        read_extra(&h, &b, 99),
        Err(NifError::Geometry { .. })
    ));
    for len in [9usize, 6] {
        let bad = one_block_nif("BSXFlags", vec![0; len]);
        let h = Header::parse(&bad).unwrap();
        assert!(read_extra(&h, &bad, 0).is_err(), "len {len}");
    }
}

#[test]
fn texture_set_raw_keeps_case_slashes_and_count() {
    for n in [9usize, 6, 4, 3, 2] {
        let slots: Vec<Vec<u8>> = (0..n)
            .map(|i| format!("Data\\Textures/X{i}.DDS").into_bytes())
            .collect();
        let b = one_block_nif("BSShaderTextureSet", encode_texture_set(&slots));
        let h = Header::parse(&b).unwrap();
        assert_eq!(decode_texture_set_raw(&h, &b, 0).unwrap().slots, slots);
    }
}

#[test]
fn texture_set_raw_refuses_hostile_input() {
    let b = one_block_nif("BSShaderTextureSet", encode_texture_set(&[b"a".to_vec()]));
    let h = Header::parse(&b).unwrap();
    assert!(matches!(
        decode_texture_set_raw(&h, &b, 5),
        Err(NifError::Geometry { .. })
    ));
    let many = vec![Vec::new(); MAX_TEXTURE_SLOTS + 1];
    let b = one_block_nif("BSShaderTextureSet", encode_texture_set(&many));
    let h = Header::parse(&b).unwrap();
    assert!(matches!(
        decode_texture_set_raw(&h, &b, 0),
        Err(NifError::Geometry { .. })
    ));
}

#[test]
fn shared_scene_shares_alpha_and_set() {
    let b = scenes::shared_alpha_and_set();
    let h = Header::parse(&b).unwrap();
    assert_eq!(h.kind(3), b"BSShaderTextureSet");
    assert_eq!(h.kind(4), b"NiAlphaProperty");
    assert_eq!(decode_texture_set_raw(&h, &b, 3).unwrap().slots.len(), 1);
}

#[test]
fn string_extra_null_index_is_empty_but_dangling_is_an_error() {
    let body = |i: i32| builder::W::default().i32(-1).i32(i).0;
    let b = one_block_nif("NiStringExtraData", body(-1));
    let h = Header::parse(&b).unwrap();
    assert_eq!(
        read_extra(&h, &b, 0).unwrap().value,
        Some(ExtraValue::Str(String::new()))
    );
    for i in [0, 7, -2, i32::MIN] {
        let b = one_block_nif("NiStringExtraData", body(i));
        let h = Header::parse(&b).unwrap();
        assert!(
            matches!(read_extra(&h, &b, 0), Err(NifError::Geometry { .. })),
            "{i}"
        );
    }
}
