mod builder;
use builder::scenes::{managed_clip, one_block_nif};
use builder::{Nif, W};
use nif::ctrl::{read_blend_interp, read_interp};
use nif::fields::{locate, Field, Value};
use nif::header::Header;
use nif::keys::{encode_data, read_data, Key, KeyData, KeyGroup};
use nif::NifError;

fn read(kind: &str, body: &[u8]) -> Result<KeyData, NifError> {
    let b = one_block_nif(kind, body.to_vec());
    let h = Header::parse(&b).unwrap();
    read_data(&h, &b, 0)
}

/// Reads the block, checks `encode_data` reproduces the body byte for byte, returns the decoded data.
fn round_trip(kind: &str, body: &[u8]) -> KeyData {
    let d = read(kind, body).unwrap();
    assert_eq!(encode_data(&d), body, "{kind}");
    d
}

fn float(d: KeyData) -> KeyGroup<f32> {
    match d {
        KeyData::Float(g) => g,
        o => panic!("not Float: {o:?}"),
    }
}

#[test]
fn float_data_linear_and_quadratic_round_trip() {
    let lin = W::default().u32(2).u32(1).fs(&[0.0, 0.0, 1.0, 1.0]).0;
    let g = float(round_trip("NiFloatData", &lin));
    assert_eq!(g.key_type, Some(1));
    assert_eq!(g.keys.len(), 2);
    assert_eq!((g.keys[1].time, g.keys[1].value), (1.0, 1.0));
    assert!(g
        .keys
        .iter()
        .all(|k| k.forward.is_none() && k.backward.is_none() && k.tbc.is_none()));

    let quad = W::default()
        .u32(3)
        .u32(2)
        .fs(&[0.0, 0.0, 0.5, 2.0])
        .fs(&[0.5, 1.0, -1.0, 0.25])
        .fs(&[2.0, 3.0, 0.0, 0.0])
        .0;
    let g = float(round_trip("NiFloatData", &quad));
    assert_eq!(g.key_type, Some(2));
    let tangents: Vec<_> = g.keys.iter().map(|k| (k.forward, k.backward)).collect();
    assert_eq!(
        tangents,
        vec![
            (Some(0.5), Some(2.0)),
            (Some(-1.0), Some(0.25)),
            (Some(0.0), Some(0.0))
        ]
    );
    assert_eq!(g.keys[2].time, 2.0);

    let pos = W::default()
        .u32(2)
        .u32(2)
        .f(0.0)
        .fs(&[1.0, 2.0, 3.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6])
        .f(1.5)
        .fs(&[4.0, 5.0, 6.0, -0.1, -0.2, -0.3, -0.4, -0.5, -0.6])
        .0;
    assert_eq!(pos.len(), 8 + 2 * 40);
    match round_trip("NiPosData", &pos) {
        KeyData::Pos(g) => {
            assert_eq!(g.key_type, Some(2));
            assert_eq!(
                g.keys[1],
                Key {
                    time: 1.5,
                    value: [4.0, 5.0, 6.0],
                    forward: Some([-0.1, -0.2, -0.3]),
                    backward: Some([-0.4, -0.5, -0.6]),
                    tbc: None,
                }
            );
            assert_eq!(g.keys[0].value, [1.0, 2.0, 3.0]);
        }
        o => panic!("not Pos: {o:?}"),
    }

    let boolean = W::default().u32(2).u32(1).f(0.0).u8(0).f(1.0).u8(1).0;
    match round_trip("NiBoolData", &boolean) {
        KeyData::Bool(g) => {
            assert_eq!(g.key_type, Some(1));
            assert_eq!(
                g.keys.iter().map(|k| (k.time, k.value)).collect::<Vec<_>>(),
                vec![(0.0, 0), (1.0, 1)]
            );
        }
        o => panic!("not Bool: {o:?}"),
    }

    // LE (BSVER 83) has the same layout
    let mut n = Nif::new(83);
    n.block("NiFloatData", lin.clone());
    let b = n.bytes();
    let h = Header::parse(&b).unwrap();
    assert_eq!(encode_data(&read_data(&h, &b, 0).unwrap()), lin);

    // NaN payload and -0.0 survive as bits
    let bits = W::default()
        .u32(2)
        .u32(1)
        .f(0.0)
        .u32(0x7FC0_0001)
        .f(1.0)
        .f(-0.0)
        .0;
    let g = float(round_trip("NiFloatData", &bits));
    assert_eq!(g.keys[0].value.to_bits(), 0x7FC0_0001);
    assert_eq!(g.keys[1].value.to_bits(), (-0.0f32).to_bits());
}

#[test]
fn zero_keys_have_no_interpolation_field() {
    let zero = W::default().u32(0).0;
    for kind in ["NiFloatData", "NiPosData", "NiBoolData"] {
        let d = round_trip(kind, &zero);
        let (t, n) = match &d {
            KeyData::Float(g) => (g.key_type, g.keys.len()),
            KeyData::Pos(g) => (g.key_type, g.keys.len()),
            KeyData::Bool(g) => (g.key_type, g.keys.len()),
        };
        assert_eq!((t, n), (None, 0), "{kind}");
    }
    let extra = W::default().u32(0).u32(1).0;
    assert!(matches!(
        read("NiFloatData", &extra),
        Err(NifError::BlockSize { .. })
    ));
    let empty = KeyData::Float(KeyGroup {
        key_type: Some(2),
        keys: vec![],
    });
    assert_eq!(encode_data(&empty), zero);
}

#[test]
fn tbc_xyz_const_decode_and_preserve() {
    let tbc = W::default()
        .u32(2)
        .u32(3)
        .fs(&[0.0, 1.0, 0.1, 0.2, 0.3])
        .fs(&[1.0, 2.0, 0.4, 0.5, 0.6])
        .0;
    assert_eq!(tbc.len(), 8 + 2 * 20);
    let g = float(round_trip("NiFloatData", &tbc));
    assert_eq!(g.key_type, Some(3));
    assert_eq!(g.keys[0].tbc, Some([0.1, 0.2, 0.3]));
    assert_eq!(g.keys[1].tbc, Some([0.4, 0.5, 0.6]));
    assert!(g.keys.iter().all(|k| k.forward.is_none()));

    let ptbc = W::default()
        .u32(2)
        .u32(3)
        .fs(&[0.0, 1.0, 2.0, 3.0, 0.1, 0.2, 0.3])
        .fs(&[1.0, 4.0, 5.0, 6.0, 0.4, 0.5, 0.6])
        .0;
    assert_eq!(ptbc.len(), 8 + 2 * 28);
    match round_trip("NiPosData", &ptbc) {
        KeyData::Pos(g) => assert_eq!(g.keys[1].tbc, Some([0.4, 0.5, 0.6])),
        o => panic!("not Pos: {o:?}"),
    }

    for t in [4u32, 5] {
        let body = W::default().u32(2).u32(t).fs(&[0.0, 1.0, 1.0, 2.0]).0;
        assert_eq!(body.len(), 8 + 2 * 8);
        let g = float(round_trip("NiFloatData", &body));
        assert_eq!(g.key_type, Some(t));
        assert!(g
            .keys
            .iter()
            .all(|k| k.forward.is_none() && k.tbc.is_none()));
    }

    let odd = W::default().u32(1).u32(7).fs(&[0.5, 9.0]).0;
    let g = float(round_trip("NiFloatData", &odd));
    assert_eq!(g.key_type, Some(7));
    assert_eq!((g.keys[0].time, g.keys[0].value), (0.5, 9.0));
}

#[test]
fn key_count_past_block_is_an_error() {
    let huge = W::default().u32(0x4000_0000).u32(1).fs(&[0.0, 1.0]).0;
    match read("NiFloatData", &huge) {
        Err(NifError::Geometry { msg, .. }) => assert!(msg.contains("key count"), "{msg}"),
        o => panic!("{o:?}"),
    }
    let short = W::default().u32(3).u32(1).fs(&[0.0, 1.0, 1.0, 2.0]).0;
    assert!(read("NiFloatData", &short).is_err());
    assert!(read("NiFloatData", &[1, 0]).is_err());

    let b = one_block_nif("NiFloatInterpolator", W::default().f(0.0).i32(-1).0);
    let h = Header::parse(&b).unwrap();
    assert!(read_data(&h, &b, 0).is_err());
    assert!(read_data(&h, &b, 5).is_err());
}

#[test]
fn blend_interp_items_only_when_flag_clear() {
    let open = |kind: &str, body: Vec<u8>| {
        let b = one_block_nif(kind, body);
        let h = Header::parse(&b).unwrap();
        (h, b)
    };

    let (h, b) = open(
        "NiBlendFloatInterpolator",
        W::default().u8(1).u8(0).f(0.0).f(0.75).0,
    );
    let x = read_blend_interp(&h, &b, 0).unwrap();
    assert_eq!(
        (x.items.clone(), x.value.clone(), x.value_at),
        (None, vec![0.75], 6)
    );
    assert_eq!(
        locate(&h, &b, 0, Field::InterpValue(0)).unwrap(),
        (6, Value::F32(0.75f32.to_bits()))
    );
    assert!(locate(&h, &b, 0, Field::InterpValue(1)).is_err());

    let (h, b) = open(
        "NiBlendFloatInterpolator",
        W::default().u8(1).u8(0).f(0.0).u32(0xFF7F_FFFF).0,
    );
    let x = read_blend_interp(&h, &b, 0).unwrap();
    assert_eq!(x.value[0].to_bits(), 0xFF7F_FFFF);

    let m = f32::MIN_POSITIVE;
    let body = W::default()
        .u8(0)
        .u8(2)
        .f(0.25)
        .u8(2)
        .u8(255)
        .u8(0x80)
        .u8(0x80)
        .fs(&[m, m, m, m])
        .i32(3)
        .fs(&[1.0, 1.0])
        .u8(0)
        .f(0.0)
        .i32(-1)
        .fs(&[0.0, 0.0])
        .u8(0)
        .f(0.0)
        .f(0.5)
        .0;
    let (h, b) = open("NiBlendFloatInterpolator", body.clone());
    let x = read_blend_interp(&h, &b, 0).unwrap();
    assert_eq!(x.items, Some(body[6..60].to_vec()));
    assert_eq!(
        (x.value_at, x.weight_threshold, x.array_size, x.flags),
        (60, 0.25, 2, 0)
    );
    assert_eq!(x.value, vec![0.5]);
    assert_eq!(locate(&h, &b, 0, Field::InterpValue(0)).unwrap().0, 60);

    let (h, b) = open(
        "NiBlendPoint3Interpolator",
        W::default().u8(1).u8(0).f(0.0).fs(&[0.1, 0.2, 0.3]).0,
    );
    let x = read_blend_interp(&h, &b, 0).unwrap();
    assert_eq!((x.value_at, x.value.clone()), (6, vec![0.1, 0.2, 0.3]));
    assert_eq!(
        locate(&h, &b, 0, Field::InterpValue(2)).unwrap(),
        (14, Value::F32(0.3f32.to_bits()))
    );
    assert!(locate(&h, &b, 0, Field::InterpValue(3)).is_err());

    // array size 3, only 2 items present
    let (h, b) = open(
        "NiBlendFloatInterpolator",
        [&body[..1], &[3], &body[2..]].concat(),
    );
    assert!(read_blend_interp(&h, &b, 0).is_err());

    let (h, b) = open("NiFloatInterpolator", W::default().f(0.0).i32(-1).0);
    assert!(read_blend_interp(&h, &b, 0).is_err());

    let b = managed_clip();
    let h = Header::parse(&b).unwrap();
    let x = read_blend_interp(&h, &b, 8).unwrap();
    assert_eq!((x.flags, x.value), (1, vec![0.0]));
    assert!(!read_interp(&h, &b, 8).unwrap().known);
}
