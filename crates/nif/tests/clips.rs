mod builder;
use builder::scenes::{self, ctrl_body, one_block_nif};
use builder::{Nif, W};
use nif::clips::*;
use nif::ctrl::*;
use nif::header::Header;

fn blk<'a>(h: &Header, b: &'a [u8], i: usize) -> &'a [u8] {
    &b[h.offsets()[i]..h.offsets()[i] + h.sizes[i] as usize]
}

#[test]
fn managed_clip_blocks_reencode_byte_identical() {
    let b = scenes::managed_clip();
    let h = Header::parse(&b).unwrap();
    let m = read_manager(&h, &b, 1).unwrap();
    assert_eq!(m.cumulative, 1);
    assert_eq!(encode_manager(&m), blk(&h, &b, 1));
    for i in [2, 3] {
        let s = read_sequence_out(&h, &b, i).unwrap();
        assert_eq!(encode_sequence(&s), blk(&h, &b, i), "sequence {i}");
    }
    let (scene, objs) = read_palette(&h, &b, 4).unwrap();
    assert_eq!(encode_palette(scene, &objs), blk(&h, &b, 4));
    let x = read_blend_interp(&h, &b, 8).unwrap();
    assert_eq!(encode_blend(&x), blk(&h, &b, 8));
    assert_eq!(encode_mttc(&read_ctrl(&h, &b, 9).unwrap()), blk(&h, &b, 9));
    assert_eq!(
        encode_base(&read_ctrl(&h, &b, 7).unwrap()),
        &blk(&h, &b, 7)[..BASE_LEN]
    );
}

#[test]
fn managed_pair_text_keys_reencode_byte_identical() {
    let b = scenes::managed_pair();
    let h = Header::parse(&b).unwrap();
    let start = (0..)
        .find(|&i| h.string(i) == Some(b"start".as_slice()))
        .unwrap();
    let end = (0..)
        .find(|&i| h.string(i) == Some(b"end".as_slice()))
        .unwrap();
    let (name, keys) = read_text_keys(&h, &b, 17).unwrap();
    assert_eq!((name, keys.clone()), (-1, vec![(0.0, start), (1.0, end)]));
    assert_eq!(encode_text_keys(name, &keys), blk(&h, &b, 17));
    let a = read_sequence_out(&h, &b, 1).unwrap();
    assert_eq!(a.text_keys, 17);
    let bb = read_sequence_out(&h, &b, 2).unwrap();
    assert_eq!(bb.blocks[2].controller, -1);
    assert_eq!(encode_sequence(&a), blk(&h, &b, 1));
    assert_eq!(encode_sequence(&bb), blk(&h, &b, 2));
}

#[test]
fn palette_names_are_sized_strings_not_string_indices() {
    let b = scenes::managed_clip();
    let h = Header::parse(&b).unwrap();
    assert_eq!(
        read_palette(&h, &b, 4).unwrap(),
        (0, vec![(b"shape".to_vec(), 5)])
    );
    let p = blk(&h, &b, 4);
    assert_eq!(&p[8..12], &5u32.to_le_bytes());
    assert_eq!(&p[12..17], b"shape");
    assert_eq!(encode_palette(0, &[(b"shape".to_vec(), 5)]), p);
}

#[test]
fn manager_cumulative_is_the_raw_byte() {
    let b = one_block_nif(
        "NiControllerManager",
        ctrl_body(-1, 0x48, -1, W::default().u8(2).u32(0).i32(-1)),
    );
    let h = Header::parse(&b).unwrap();
    let m = read_manager(&h, &b, 0).unwrap();
    assert_eq!(m.cumulative, 2);
    assert_eq!(encode_manager(&m), blk(&h, &b, 0));
}

#[test]
fn blend_interp_authoring_form() {
    let f = encode_blend_interp("NiBlendFloatInterpolator", &[0.5]);
    let mut want = vec![1, 0, 0, 0, 0, 0];
    want.extend(0.5f32.to_le_bytes());
    assert_eq!(f, want);
    let p = encode_blend_interp("NiBlendPoint3Interpolator", &[1.0, 2.0, 3.0]);
    assert_eq!(p.len(), 18);
    for (class, bytes, value) in [
        ("NiBlendFloatInterpolator", f, vec![0.5f32]),
        ("NiBlendPoint3Interpolator", p, vec![1.0, 2.0, 3.0]),
    ] {
        let b = one_block_nif(class, bytes);
        let h = Header::parse(&b).unwrap();
        let x = read_blend_interp(&h, &b, 0).unwrap();
        assert_eq!((x.flags, x.array_size, x.items), (1, 0, None));
        let bits = |v: &[f32]| v.iter().map(|f| f.to_bits()).collect::<Vec<_>>();
        assert_eq!(bits(&x.value), bits(&value));
    }
    // unmanaged: flags 0, size 1, 20 counter bytes, one 17-byte item, value
    let body = W::default()
        .u8(0)
        .u8(1)
        .f(0.25)
        .raw(&[1, 0, 2, 0xFF])
        .fs(&[0.0, 1.0, 0.0, 0.0])
        .i32(3)
        .f(1.0)
        .f(1.0)
        .u8(0)
        .f(0.0)
        .f(f32::from_bits(0xFF7F_FFFF))
        .0;
    let b = one_block_nif("NiBlendFloatInterpolator", body.clone());
    let h = Header::parse(&b).unwrap();
    let x = read_blend_interp(&h, &b, 0).unwrap();
    assert_eq!(x.items.as_ref().map(Vec::len), Some(37));
    assert_eq!(encode_blend(&x), body);
}

#[test]
fn sequence_with_notes_and_priority_round_trips() {
    let mut n = Nif::new(100);
    for s in ["a", "b", "c", "d"] {
        n.string(s);
    }
    let body = W::default()
        .i32(0) // name
        .u32(1)
        .u32(5) // array grow by
        .i32(4)
        .i32(5)
        .u8(7) // priority
        .i32(1)
        .i32(2)
        .i32(-1)
        .i32(0)
        .i32(3) // interpolator ID
        .f(0.5) // weight
        .i32(-1)
        .u32(2)
        .fs(&[1.5, 0.0, 2.0])
        .i32(-1)
        .i32(-1) // accum root
        .u16(2)
        .i32(8)
        .i32(9)
        .0;
    n.block("NiControllerSequence", body.clone());
    let b = n.bytes();
    let h = Header::parse(&b).unwrap();
    let s = read_sequence_out(&h, &b, 0).unwrap();
    assert_eq!(
        s,
        SequenceOut {
            name: 0,
            blocks: vec![CtrlBlockOut {
                interp: 4,
                controller: 5,
                priority: 7,
                node: 1,
                property: 2,
                ctrl_type: -1,
                ctrl_id: 0,
                interp_id: 3,
            }],
            array_grow_by: 5,
            weight: 0.5,
            text_keys: -1,
            cycle: 2,
            freq: 1.5,
            start: 0.0,
            stop: 2.0,
            manager: -1,
            accum_root: -1,
            notes: vec![8, 9],
        }
    );
    assert_eq!(encode_sequence(&s), body);
}

#[test]
fn hostile_clip_blocks_are_errors() {
    let one = |kind: &str, body: Vec<u8>| one_block_nif(kind, body);
    let seq = one(
        "NiControllerSequence",
        W::default().i32(-1).u32(u32::MAX).u32(1).raw(&[0; 40]).0,
    );
    let h = Header::parse(&seq).unwrap();
    assert!(read_sequence_out(&h, &seq, 0).is_err());
    let pal = one(
        "NiDefaultAVObjectPalette",
        W::default().i32(0).u32(1).u32(100).raw(b"ab").i32(5).0,
    );
    let h = Header::parse(&pal).unwrap();
    assert!(read_palette(&h, &pal, 0).is_err());
    let pal = one(
        "NiDefaultAVObjectPalette",
        W::default().i32(0).u32(u32::MAX).u32(1).raw(b"a").i32(5).0,
    );
    let h = Header::parse(&pal).unwrap();
    assert!(read_palette(&h, &pal, 0).is_err());
    let tk = one(
        "NiTextKeyExtraData",
        W::default().i32(-1).u32(3).f(0.0).i32(-1).0,
    );
    let h = Header::parse(&tk).unwrap();
    assert!(read_text_keys(&h, &tk, 0).is_err());
    let mgr = one(
        "NiControllerManager",
        ctrl_body(-1, 0x48, -1, W::default().u8(0).u32(u32::MAX).i32(-1)),
    );
    let h = Header::parse(&mgr).unwrap();
    assert!(read_manager(&h, &mgr, 0).is_err());
    // wrong kind for every reader
    let other = one("NiNode", vec![0; 64]);
    let h = Header::parse(&other).unwrap();
    assert!(read_sequence_out(&h, &other, 0).is_err());
    assert!(read_palette(&h, &other, 0).is_err());
    assert!(read_text_keys(&h, &other, 0).is_err());
    assert!(read_manager(&h, &other, 0).is_err());
    assert!(read_blend_interp(&h, &other, 0).is_err());
    // no such block
    assert!(read_palette(&h, &other, 5).is_err());
}
