use encoders_sys::bc7_fixup::force_opaque_mode6;

fn mode6_block(rgb: u128, alpha: u128, p0: u128, p1: u128) -> [u8; 16] {
    let mut v: u128 = 1 << 6; // mode 6
    for (i, c) in [rgb, rgb, rgb, rgb, rgb, rgb, alpha, alpha]
        .iter()
        .enumerate()
    {
        v |= c << (7 + 7 * i);
    }
    v |= p0 << 63 | p1 << 64; // indices stay 0
    v.to_le_bytes()
}

fn decode(b: &[u8; 16]) -> [u8; 64] {
    let mut o = [0u8; 64];
    bcdec_rs::bc7(b, &mut o, 16);
    o
}

#[test]
fn rewrites_mode6_pbits_so_alpha_decodes_255() {
    let mut b = mode6_block(10, 127, 0, 0);
    assert_eq!(decode(&b)[3], 254);
    assert!(force_opaque_mode6(&mut b));
    let d = decode(&b);
    assert!(d.chunks(4).all(|p| p[3] == 255));
    assert_eq!(d[0], 21); // RGB endpoint shifts by one LSB: (10 << 1 | 1)
}

#[test]
fn leaves_other_modes_alone() {
    let mut b = [0u8; 16];
    b[0] = 0b0010_0000; // mode 5
    let before = b;
    assert!(!force_opaque_mode6(&mut b));
    assert_eq!(b, before);
}

/// Bits 49..=64: both alpha endpoints and both p-bits; everything else must survive the patch.
const PATCHED_BITS: u128 = ((1u128 << 16) - 1) << 49;

fn texel(d: &[u8; 64], i: usize) -> [u8; 4] {
    [d[4 * i], d[4 * i + 1], d[4 * i + 2], d[4 * i + 3]]
}

/// Mode 6 per the BC7 spec: 7-bit RGBA endpoints x2 in R0 R1 G0 G1 B0 B1 A0 A1 order, one p-bit per endpoint
/// (shared by all four channels), then 16 4-bit indices with the anchor's (texel 0) MSB implied 0.
/// The expected texels come from the spec's decode rule `(e << 1) | p`, checked through bcdec_rs.
#[test]
fn mode6_layout_agrees_with_independent_decoder() {
    let (e0, e1) = ([5u128, 17, 33, 40], [100u128, 90, 120, 127]); // RGBA endpoints, all distinct
    let mut v: u128 = 1 << 6;
    for c in 0..4 {
        v |= e0[c] << (7 + 14 * c) | e1[c] << (14 + 14 * c);
    }
    v |= 1 << 64; // p0 = 0, p1 = 1
    v |= 0b1111 << 68; // texel 1 index 15 (texel 0 is the 3-bit anchor at 65..=67, index 0)
    let mut b = v.to_le_bytes();

    let d = decode(&b);
    assert_eq!(texel(&d, 0), [10, 34, 66, 80]); // endpoint 0, p = 0
    assert_eq!(texel(&d, 1), [201, 181, 241, 255]); // endpoint 1, p = 1
    assert_eq!(texel(&d, 2), [10, 34, 66, 80]);

    assert!(force_opaque_mode6(&mut b));
    let d = decode(&b);
    assert!(d.chunks(4).all(|p| p[3] == 255));
    assert_eq!(texel(&d, 0), [11, 35, 67, 255]); // p0 forced to 1: RGB up one LSB
    assert_eq!(texel(&d, 1), [201, 181, 241, 255]); // indices and endpoint 1 untouched
    assert_eq!(
        u128::from_le_bytes(b) & !PATCHED_BITS,
        v & !PATCHED_BITS,
        "only alpha endpoints and p-bits may change"
    );
}

/// Random mode-6 blocks: after the patch every texel decodes alpha 255 and RGB moves by at most one LSB
/// (exactly zero when both p-bits were already 1).
#[test]
fn random_mode6_blocks_decode_opaque_with_rgb_within_one_lsb() {
    let mut s: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    };
    for _ in 0..10_000 {
        let v = ((next() as u128) << 64 | next() as u128) & !0x7f | 0x40;
        let both_p_set = (v >> 63) & 0b11 == 0b11;
        let mut b = v.to_le_bytes();
        let before = decode(&b);
        assert!(force_opaque_mode6(&mut b));
        let after = decode(&b);
        assert_eq!(u128::from_le_bytes(b) & !PATCHED_BITS, v & !PATCHED_BITS);
        for (p, q) in before.chunks(4).zip(after.chunks(4)) {
            assert_eq!(q[3], 255, "block {v:#034x}");
            for c in 0..3 {
                let diff = q[c] as i32 - p[c] as i32;
                assert!(
                    (0..=1).contains(&diff) && (diff == 0 || !both_p_set),
                    "block {v:#034x}: channel {c} {} -> {}",
                    p[c],
                    q[c]
                );
            }
        }
    }
}
