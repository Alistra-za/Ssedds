//! `psdds_encode` clamps input to [0,1] (NaN -> 0, +inf -> 1, -inf -> 0), so float hosts need no copy of the rule.
use psdds::{psdds_defaults, psdds_encode_depth, psdds_free, PsddsBuf, PsddsSettings};

fn encode_d(px: &[f32], w: u32, h: u32, s: &PsddsSettings, depth: u32) -> Vec<u8> {
    let mut out = PsddsBuf {
        data: std::ptr::null_mut(),
        len: 0,
    };
    let mut err = [0 as std::ffi::c_char; 256];
    let rc = unsafe {
        psdds_encode_depth(
            px.as_ptr(),
            w,
            h,
            1,
            depth,
            s,
            &mut out,
            err.as_mut_ptr(),
            err.len(),
        )
    };
    assert_eq!(rc, 0);
    let v = unsafe { std::slice::from_raw_parts(out.data, out.len) }.to_vec();
    unsafe { psdds_free(out) };
    v
}

/// 16x16 with mips on: a checkerboard of out-of-range pixels and in-range ones, so every 2x2 block mixes them.
fn wild(odd: &[f32]) -> Vec<f32> {
    (0..16 * 16 * 4)
        .map(|i| {
            let (p, c) = (i / 4, i % 4);
            if (p % 2 + (p / 16) % 2) % 2 == 0 {
                odd[(p / 2 + c) % odd.len()]
            } else {
                ((p * 7 + c * 13) % 256) as f32 / 255.0
            }
        })
        .collect()
}

fn clamped(px: &[f32]) -> Vec<f32> {
    px.iter()
        .map(|&v| if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) })
        .collect()
}

fn check(odd: &[f32]) {
    check_at(odd, 8);
}

fn check_at(odd: &[f32], depth: u32) {
    let px = wild(odd);
    let want = clamped(&px);
    // BC7 Colour with sRGB set explicitly (DC34), alpha kept; and an RGBA8 Linear mask.
    let bc7 = PsddsSettings {
        format: 4,
        texture_type: 0,
        srgb: 1,
        mips: 1,
        ..psdds_defaults(0)
    };
    let rgba8 = PsddsSettings {
        format: 5,
        quality: 0,
        mips: 1,
        ..psdds_defaults(2)
    };
    for (name, s) in [("BC7 colour", bc7), ("RGBA8 mask", rgba8)] {
        assert!(
            encode_d(&px, 16, 16, &s, depth) == encode_d(&want, 16, 16, &s, depth),
            "{name} depth {depth}"
        );
    }
}

#[test]
fn finite_out_of_range_input_encodes_as_its_clamped_copy() {
    check(&[1.5, -0.25, 2.0, 7.0, -3.0]);
}

#[test]
fn nan_and_infinities_encode_as_their_clamped_copy() {
    check(&[f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1.5, -0.25]);
}

/// DF3: only a 32-bit Colour document skips the clamp; 8 and 16 bits clamp as before.
#[test]
fn clamp_holds_at_depth_8_and_16() {
    for depth in [8, 16] {
        check_at(&[1.5, -0.25, 2.0, 7.0, -3.0], depth);
        check_at(
            &[f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1.5, -0.25],
            depth,
        );
    }
}
