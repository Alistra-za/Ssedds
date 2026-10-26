//! Note DH: "Alpha is transparency" (alpha_mode 1) writes exactly the bytes psdds wrote before ABI 4; alpha_mode 0
//! keeps the colour under alpha-0 pixels; outputs without kept alpha do not depend on it.
use engine::dds::read_dds;
use psdds::{
    psdds_defaults, psdds_encode_depth, psdds_free, psdds_preset_settings, PsddsBuf, PsddsSettings,
};

const W: u32 = 32;
const H: u32 = 32;

fn fnv(b: &[u8]) -> u64 {
    b.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &x| {
        (h ^ u64::from(x)).wrapping_mul(0x100_0000_01b3)
    })
}

/// Hash-noise RGB; alpha 0 on the left quarter (colour there must survive), 0.5 and 1 elsewhere.
fn fixture() -> Vec<f32> {
    let mut v = Vec::new();
    for y in 0..H {
        for x in 0..W {
            for c in 0..3u32 {
                let mut k = (y * W + x) * 4 + c;
                k = k.wrapping_mul(0x9E37_79B9) ^ 0x5bd1_e995;
                k ^= k >> 15;
                k = k.wrapping_mul(0x2c1b_3c6d);
                k ^= k >> 12;
                v.push((k % 256) as f32 / 255.0);
            }
            v.push(if x < W / 4 {
                0.0
            } else if (x + y) % 3 == 0 {
                0.5
            } else {
                1.0
            });
        }
    }
    v
}

fn enc(px: &[f32], depth: u32, s: &PsddsSettings) -> Vec<u8> {
    let mut out = PsddsBuf {
        data: std::ptr::null_mut(),
        len: 0,
    };
    let rc = unsafe {
        psdds_encode_depth(
            px.as_ptr(),
            W,
            H,
            1,
            depth,
            s,
            &mut out,
            std::ptr::null_mut(),
            0,
        )
    };
    assert_eq!(rc, 0);
    let b = unsafe { std::slice::from_raw_parts(out.data, out.len) }.to_vec();
    unsafe { psdds_free(out) };
    b
}

fn seed(i: u32) -> PsddsSettings {
    let mut s = psdds_defaults(0);
    assert_eq!(unsafe { psdds_preset_settings(i, &mut s) }, 0);
    s
}

fn colour(format: u32, quality: u32) -> PsddsSettings {
    PsddsSettings {
        format,
        quality,
        ..psdds_defaults(0)
    }
}

/// (name, settings, depth, alpha kept, bytes before ABI 4, alpha_mode 0 bytes). The "before" hashes were recorded
/// on main 448e54fb (ABI 3, always Blend) with this fixture.
fn cases() -> Vec<(&'static str, PsddsSettings, u32, bool, u64, u64)> {
    vec![
        (
            "Colour BC7",
            seed(0),
            8,
            true,
            0xb1eb92b554cbe893,
            0x3fe975165d71a4db,
        ),
        (
            "Colour BC1",
            seed(1),
            8,
            false,
            0x65c5d398742db583,
            0x65c5d398742db583,
        ),
        (
            "Normal BC7",
            seed(2),
            8,
            true,
            0xf99b69614d2cdd4c,
            0xbdc6872594246632,
        ),
        (
            "Mask/data BC7",
            seed(3),
            8,
            true,
            0x45fa0e5d3191578b,
            0xc1df32cb3a467064,
        ),
        (
            "Greyscale mask BC4",
            seed(4),
            8,
            false,
            0x13632ccf569f565c,
            0x13632ccf569f565c,
        ),
        (
            "Colour BC3",
            colour(1, 18),
            8,
            true,
            0xe3bece8075a889fa,
            0x5f6b5ede081f8ba1,
        ),
        (
            "Colour RGBA8",
            colour(5, 0),
            8,
            true,
            0xdbc1beb53c1b506d,
            0x525839c2044b658e,
        ),
        (
            "Colour RGBA16F",
            colour(7, 0),
            16,
            true,
            0xf24ce5e9c0c37887,
            0xfca1d0b579751434,
        ),
    ]
}

#[test]
fn transparency_writes_the_old_bytes_and_data_is_pinned() {
    let px = fixture();
    for (name, s, depth, kept, before, data) in cases() {
        let mode = |m| PsddsSettings { alpha_mode: m, ..s };
        let blend = enc(&px, depth, &mode(1));
        let plain = enc(&px, depth, &mode(0));
        assert_eq!(fnv(&blend), before, "{name}: alpha_mode 1 is today's bytes");
        if kept {
            assert!(
                blend != plain,
                "{name}: alpha_mode 0 must change kept-alpha bytes"
            );
        } else {
            assert!(
                blend == plain,
                "{name}: no kept alpha, so alpha_mode changes nothing"
            );
        }
        assert_eq!(fnv(&plain), data, "{name}: alpha_mode 0 bytes");
    }
}

/// Two flat colours: red-ish under alpha 0 on the left quarter, green-ish (alpha 0.5 and 1) elsewhere. Dilation
/// would paint the left quarter green; flat colours compress well, so block error stays small.
fn flat() -> Vec<f32> {
    (0..H)
        .flat_map(|_| 0..W)
        .flat_map(|x| {
            if x < W / 4 {
                [0.9, 0.1, 0.2, 0.0]
            } else {
                [0.1, 0.8, 0.3, if x % 2 == 0 { 0.5 } else { 1.0 }]
            }
        })
        .collect()
}

fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// The top mip's RGB where the source alpha is 0: the largest absolute error against the source (linearised for the
/// HDR formats, which decode to linear light, DF3).
fn error_under_clear(px: &[f32], bytes: &[u8], linear: bool) -> f32 {
    let dds = read_dds(bytes).unwrap();
    let (w, h, r) = dds.levels[0].clone();
    let top = engine::decode::decode_level_rgba(dds.format, w, h, &dds.data[r], None).unwrap();
    let mut worst = 0.0f32;
    for (src, got) in px.as_chunks::<4>().0.iter().zip(&top) {
        if src[3] == 0.0 {
            for c in 0..3 {
                let want = if linear {
                    srgb_to_linear(src[c])
                } else {
                    src[c]
                };
                worst = worst.max((want - got[c]).abs());
            }
        }
    }
    worst
}

#[test]
fn data_keeps_the_colour_under_clear_pixels() {
    let px = flat();
    // Normal is left out: its RGB is a vector the engine renormalises, not a colour to compare.
    for (name, s, depth, kept, _, _) in cases() {
        if !kept || s.texture_type == 1 {
            continue;
        }
        let linear = s.format == 7;
        let mode = |m| PsddsSettings { alpha_mode: m, ..s };
        let data = error_under_clear(&px, &enc(&px, depth, &mode(0)), linear);
        let blend = error_under_clear(&px, &enc(&px, depth, &mode(1)), linear);
        assert!(blend > 0.3, "{name}: transparency repaints it ({blend})");
        assert!(data < 0.02, "{name}: data keeps the colour ({data})");
    }
}
