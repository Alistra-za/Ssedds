use encoders_sys::bc7_fixup::{force_opaque_mode6, force_opaque_rotated};
use encoders_sys::{
    bc7e_compress_blocks, gather_blocks, Bc7Level, Bc7Params, BC7E_EXACT_OPAQUE_NATIVE,
};

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

/// Mode 4: mode bits 0-4 (`10000`), rotation 5-6, index selection 7, then R0 R1 G0 G1 B0 B1 (5 bits each,
/// from bit 8) and A0 A1 (6 bits each, from 38). Mode 5: mode bits 0-5 (`100000`), rotation 6-7, then R0..B1
/// (7 bits each, from 8) and A0 A1 (8 bits each, from 50). Returns (header bits 0-7, colour bits, alpha bits).
fn header(mode: u32, rot: u128, isel: u128) -> (u128, u32, u32) {
    match mode {
        4 => (1 << 4 | rot << 5 | isel << 7, 5, 6),
        5 => (1 << 5 | rot << 6, 7, 8),
        _ => unreachable!(),
    }
}

/// Bits `force_opaque_rotated` may set: both endpoints of colour channel `rot - 1`.
fn rotated_mask(mode: u32, rot: u128) -> u128 {
    let pair = if mode == 4 { 10 } else { 14 };
    ((1u128 << pair) - 1) << (8 + pair * (rot as u32 - 1))
}

/// All indices 0, so every texel decodes endpoint 0: checks the layout above and the rotation swap against bcdec_rs.
#[test]
fn mode4_mode5_layout_and_rotation_agree_with_independent_decoder() {
    let expand = |x: u128, bits: u32| ((x << (8 - bits)) | (x >> (2 * bits - 8))) as u8;
    for (mode, isel) in [(4, 0), (4, 1), (5, 0)] {
        for rot in 0..4u128 {
            let (mut v, cb, ab) = header(mode, rot, isel);
            let (rgb0, a0) = ([3u128, 9, 17], 21u128);
            for (c, e) in rgb0.iter().enumerate() {
                v |= e << (8 + 2 * cb * c as u32); // endpoint 0 of channel c; endpoint 1 stays 0
            }
            v |= a0 << (8 + 6 * cb);
            let d = decode(&v.to_le_bytes());
            let mut want = [
                expand(rgb0[0], cb),
                expand(rgb0[1], cb),
                expand(rgb0[2], cb),
                expand(a0, ab),
            ];
            if rot != 0 {
                want.swap(rot as usize - 1, 3);
            }
            for i in 0..16 {
                assert_eq!(texel(&d, i), want, "mode {mode} isel {isel} rot {rot}");
            }
        }
    }
}

/// Hand-built blocks per mode, rotation and (mode 4) index selection. The rotated colour channel holds
/// endpoints 0 and 13, so every texel decodes alpha < 255 before the fix and 255 after; RGB is unchanged.
#[test]
fn rotated_mode4_mode5_blocks_decode_opaque_with_rgb_unchanged() {
    for (mode, isel) in [(4, 0), (4, 1), (5, 0)] {
        for rot in 1..4u128 {
            let (mut v, cb, ab) = header(mode, rot, isel);
            for c in 0..3u32 {
                let (e0, e1) = if c as u128 == rot - 1 {
                    (0u128, 13u128)
                } else {
                    (4 + 5 * c as u128, 30 - 3 * c as u128)
                };
                v |= e0 << (8 + 2 * cb * c) | e1 << (8 + 2 * cb * c + cb);
            }
            v |= 7 << (8 + 6 * cb) | 40 << (8 + 6 * cb + ab); // stored alpha = the swapped colour channel
            v |= 0x5A5A_5A5A_5A5A_5A5A_5A5Au128 << (8 + 6 * cb + 2 * ab); // mixed indices
            let mut b = v.to_le_bytes();
            let before = decode(&b);
            assert!(
                before.chunks(4).all(|t| t[3] < 255),
                "mode {mode} rot {rot}: must start non-opaque"
            );
            assert!(force_opaque_rotated(&mut b));
            let after = decode(&b);
            for (p, q) in before.chunks(4).zip(after.chunks(4)) {
                assert_eq!(q[3], 255, "mode {mode} isel {isel} rot {rot}");
                assert_eq!(
                    p[..3],
                    q[..3],
                    "mode {mode} isel {isel} rot {rot}: RGB changed"
                );
            }
            assert_eq!(
                u128::from_le_bytes(b),
                v | rotated_mask(mode, rot),
                "only the rotated endpoints may change"
            );
        }
    }
}

fn xorshift(mut s: u64) -> impl FnMut() -> u64 {
    move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    }
}

#[test]
fn rotated_fix_leaves_rotation_0_and_other_modes_alone() {
    let mut next = xorshift(0x2545_F491_4F6C_DD1D);
    for mode in 0..8u32 {
        for _ in 0..200 {
            let r = (next() as u128) << 64 | next() as u128;
            // Mode bits: `1 << mode` in the low mode+1 bits; modes 4 and 5 get rotation 0.
            let mut v = (r & !((1u128 << (mode + 1)) - 1)) | 1 << mode;
            match mode {
                4 => v &= !(3 << 5),
                5 => v &= !(3 << 6),
                _ => {}
            }
            let mut b = v.to_le_bytes();
            assert!(!force_opaque_rotated(&mut b), "mode {mode}");
            assert_eq!(u128::from_le_bytes(b), v, "mode {mode}");
        }
    }
    let mut reserved = [0u8; 16]; // no mode bit set
    assert!(!force_opaque_rotated(&mut reserved));
    assert_eq!(reserved, [0u8; 16]);
}

/// Random rotated mode 4/5 blocks: alpha decodes 255 everywhere, RGB is bit-identical, and only the
/// rotated channel's endpoint bits are set.
#[test]
fn random_rotated_blocks_decode_opaque_with_rgb_unchanged() {
    let mut next = xorshift(0x9E37_79B9_7F4A_7C15);
    for i in 0..20_000u32 {
        let r = (next() as u128) << 64 | next() as u128;
        let (mode, rot) = (4 + i % 2, 1 + (i / 2 % 3) as u128);
        let (h, _, _) = header(mode, rot, (r >> 7) & 1);
        let v = (r & !0xff) | h; // both headers fill bits 0-7
        let mut b = v.to_le_bytes();
        let before = decode(&b);
        assert!(force_opaque_rotated(&mut b));
        assert_eq!(u128::from_le_bytes(b), v | rotated_mask(mode, rot));
        let after = decode(&b);
        for (p, q) in before.chunks(4).zip(after.chunks(4)) {
            assert_eq!(q[3], 255, "block {v:#034x}");
            assert_eq!(p[..3], q[..3], "block {v:#034x}");
        }
    }
}

/// The review's probe: bc7e (perceptual off, exact-opaque flag as built) on opaque noise emits rotated mode
/// 4/5 blocks whose alpha decodes below 255; the fix makes every texel 255 with RGB bit-identical.
#[test]
fn bc7e_opaque_noise_rotated_blocks_fixed_losslessly() {
    let mut next = xorshift(0x1234_5678_9ABC_DEF1);
    let px: Vec<[u8; 4]> = (0..128 * 128)
        .map(|_| {
            let s = next();
            [s as u8, (s >> 8) as u8, (s >> 16) as u8, 255]
        })
        .collect();
    let blocks = gather_blocks(&px, 128, 128);
    let mut out = vec![0u8; blocks.len()];
    let params = Bc7Params {
        level: Bc7Level::Veryslow,
        perceptual: false,
        exact_opaque: BC7E_EXACT_OPAQUE_NATIVE,
    };
    for (o, b) in out.chunks_mut(64 * 16).zip(blocks.chunks(64 * 16)) {
        bc7e_compress_blocks(b, o, params);
    }
    let mut needed = 0;
    for blk in out.chunks(16) {
        let orig: [u8; 16] = blk.try_into().unwrap();
        let mut b = orig;
        if !force_opaque_rotated(&mut b) {
            continue;
        }
        let (before, after) = (decode(&orig), decode(&b));
        needed += before.chunks(4).any(|t| t[3] != 255) as usize;
        for (p, q) in before.chunks(4).zip(after.chunks(4)) {
            assert_eq!(q[3], 255);
            assert_eq!(p[..3], q[..3]);
        }
    }
    assert!(
        needed > 0,
        "no rotated mode 4/5 block decoded alpha below 255"
    );
}

/// DECISIONS AH1: a mode-6 block with alpha endpoints 0 but p-bit 1 decodes alpha 1; the fix makes it exactly 0.
#[test]
fn zero_alpha_fix_clears_mode6_pbits() {
    let mut b = mode6_block(10, 0, 1, 1);
    assert_eq!(decode(&b)[3], 1);
    assert!(encoders_sys::bc7_fixup::force_zero_alpha(&mut b));
    let d = decode(&b);
    assert!(d.chunks(4).all(|p| p[3] == 0));
    assert_eq!(d[0], 20); // RGB endpoint shifts down one LSB: (10 << 1 | 0)
}

/// Every bc7e mode choice on noisy RGB with alpha 0 throughout decodes to exactly 0 after the fix.
#[test]
fn zero_alpha_fix_gives_exact_0_on_bc7e_output() {
    let mut seed = 12345u32;
    let mut rnd = || {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
        (seed >> 16) as u8
    };
    let texels: Vec<[u8; 4]> = (0..64 * 64).map(|_| [rnd(), rnd(), rnd(), 0]).collect();
    let blocks = gather_blocks(&texels, 64, 64);
    // Blocks bc7e itself decoded above 0: the fix must have something to do, or the test proves nothing.
    let mut needed = 0;
    for level in [Bc7Level::Ultrafast, Bc7Level::Basic, Bc7Level::Slow] {
        let mut out = vec![0u8; blocks.len() / 16 * 16];
        bc7e_compress_blocks(
            &blocks,
            &mut out,
            Bc7Params {
                level,
                perceptual: false,
                exact_opaque: false,
            },
        );
        for blk in out.chunks_mut(16) {
            let blk: &mut [u8; 16] = blk.try_into().unwrap();
            needed += decode(blk).chunks(4).any(|p| p[3] != 0) as usize;
            encoders_sys::bc7_fixup::force_zero_alpha(blk);
            assert!(
                decode(blk).chunks(4).all(|p| p[3] == 0),
                "{level:?}: {blk:?}"
            );
        }
    }
    assert!(needed > 0, "no bc7e block decoded alpha above 0");
}

/// Mode 7 and rotated mode 4/5 blocks: alpha endpoints (and mode 7's p-bits) cleared give alpha 0.
#[test]
fn zero_alpha_fix_covers_modes_4_5_and_7() {
    for mut b in [
        [
            0x80u8, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
            0xff, 0xff,
        ],
        [
            0x10 | (2 << 5),
            0xff,
            0xff,
            0xff,
            0xff,
            0xff,
            0xff,
            0xff,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ],
        [
            0x20 | (3 << 6),
            0xff,
            0xff,
            0xff,
            0xff,
            0xff,
            0xff,
            0xff,
            0xff,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ],
        [
            0x10, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
        [
            0x20, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 0, 0, 0,
        ],
    ] {
        assert!(encoders_sys::bc7_fixup::force_zero_alpha(&mut b));
        assert!(decode(&b).chunks(4).all(|p| p[3] == 0), "{b:?}");
    }
}
