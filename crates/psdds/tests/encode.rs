//! `psdds_encode` writes the engine's bytes for the resolved profile, on any size, and fails without panicking.
use engine::dds::{check_header, full_chain_len, read_dds, DDPF_ALPHAPIXELS};
use engine::image::Rgba32F;
use engine::memory::encode_image;
use psdds::settings::profile_for;
use psdds::{
    psdds_defaults, psdds_encode, psdds_encode_depth, psdds_free, PsddsBuf, PsddsSettings,
};
use std::ffi::CStr;

const BC1: u32 = 0;
const BC3: u32 = 1;
const BC4: u32 = 2;
const BC5: u32 = 3;
const BC7: u32 = 4;
const RGBA8: u32 = 5;

fn gradient(w: u32, h: u32, alpha: bool) -> Vec<f32> {
    let c = |v: u32| (v % 256) as f32 / 255.0;
    (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .flat_map(|(x, y)| {
            let a = if alpha { c(255 - x * 3) } else { 1.0 };
            [c(x * 4), c(y * 8), c(x * 2 + y * 3), a]
        })
        .collect()
}

fn image(px: &[f32], w: u32, h: u32) -> Rgba32F {
    Rgba32F {
        width: w,
        height: h,
        px: px.as_chunks::<4>().0.to_vec(),
    }
}

/// The encoded bytes, or the error text and return code.
fn encode(
    px: &[f32],
    w: u32,
    h: u32,
    has_alpha: bool,
    s: &PsddsSettings,
) -> Result<Vec<u8>, (i32, String)> {
    let mut out = PsddsBuf {
        data: std::ptr::null_mut(),
        len: 0,
    };
    let mut err = [0 as std::ffi::c_char; 256];
    let rc = unsafe {
        psdds_encode(
            px.as_ptr(),
            w,
            h,
            u32::from(has_alpha),
            s,
            &mut out,
            err.as_mut_ptr(),
            err.len(),
        )
    };
    if rc != 0 {
        assert!(out.data.is_null());
        let msg = unsafe { CStr::from_ptr(err.as_ptr()) };
        return Err((rc, msg.to_str().unwrap().to_owned()));
    }
    let bytes = unsafe { std::slice::from_raw_parts(out.data, out.len) }.to_vec();
    unsafe { psdds_free(out) };
    Ok(bytes)
}

/// psdds_encode_depth: the bytes, or the return code and error text.
fn encode_d(
    px: &[f32],
    w: u32,
    h: u32,
    has_alpha: bool,
    depth: u32,
    s: &PsddsSettings,
) -> Result<Vec<u8>, (i32, String)> {
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
            u32::from(has_alpha),
            depth,
            s,
            &mut out,
            err.as_mut_ptr(),
            err.len(),
        )
    };
    if rc != 0 {
        assert!(out.data.is_null());
        let msg = unsafe { CStr::from_ptr(err.as_ptr()) };
        return Err((rc, msg.to_str().unwrap().to_owned()));
    }
    let bytes = unsafe { std::slice::from_raw_parts(out.data, out.len) }.to_vec();
    unsafe { psdds_free(out) };
    Ok(bytes)
}

/// The top mip as floats, whatever the format (psdds_decode_depth comes later; this is the engine's decode).
fn decode_top(bytes: &[u8]) -> Vec<[f32; 4]> {
    let dds = read_dds(bytes).unwrap();
    let (w, h, r) = dds.levels[0].clone();
    engine::decode::decode_level_rgba(dds.format, w, h, &dds.data[r], None).unwrap()
}

/// The type's defaults with another format, at the `cs-quality` quality for it (rgbcx 18, radius default,
/// effort veryslow); a quality index belongs to its format's list.
fn settings(format: u32, texture_type: u32) -> PsddsSettings {
    PsddsSettings {
        format,
        quality: [18, 18, 0, 0, 4, 0, 1, 0][format as usize],
        ..psdds_defaults(texture_type)
    }
}

#[test]
fn encode_matches_engine() {
    let (w, h) = (64, 32);
    let px = gradient(w, h, true);
    for ty in 0..3 {
        for f in 0..6 {
            let s = settings(f, ty);
            let got = encode(&px, w, h, true, &s).unwrap();
            let want = encode_image(image(&px, w, h), &profile_for(&s, true).unwrap()).unwrap();
            assert!(got == want, "type {ty} format {f}");
        }
    }
    // BC7 Max needs the patch-0002 bc7e objects; it must encode, not fail.
    let max = PsddsSettings {
        quality: 6,
        ..settings(BC7, 0)
    };
    let px = gradient(4, 4, true);
    let want = encode_image(image(&px, 4, 4), &profile_for(&max, true).unwrap()).unwrap();
    assert!(encode(&px, 4, 4, true, &max).unwrap() == want, "BC7 Max");
}

#[test]
fn odd_sizes_encode_and_read_back() {
    let mut cases = vec![];
    for f in 0..6 {
        cases.push((f, 5, 3));
        cases.push((f, 1, 1));
    }
    cases.push((BC1, 4097, 17));
    for (f, w, h) in cases {
        for mips in [0, 1] {
            let px = gradient(w, h, false);
            let s = PsddsSettings {
                mips,
                ..settings(f, 2) // Linear mask offers every format
            };
            let bytes = encode(&px, w, h, false, &s).unwrap();
            let dds = read_dds(&bytes).unwrap_or_else(|e| panic!("{f} {w}x{h}: {e}"));
            assert_eq!((dds.width, dds.height), (w, h));
            let levels = if mips == 1 { full_chain_len(w, h) } else { 1 };
            assert_eq!(dds.mip_count as usize, levels, "{f} {w}x{h} mips {mips}");
            assert_eq!(dds.levels.len(), levels);
            assert!(check_header(&dds).is_empty(), "{:?}", check_header(&dds));
        }
    }
}

/// Hash noise: a gradient can box- and Kaiser-filter to the same blocks; noise cannot.
fn noise(w: u32, h: u32) -> Vec<f32> {
    (0..w * h * 4)
        .map(|i| {
            let mut x = i.wrapping_mul(0x9E37_79B9) ^ 0x5bd1_e995;
            x ^= x >> 15;
            x = x.wrapping_mul(0x2c1b_3c6d);
            x ^= x >> 12;
            if i % 4 == 3 {
                0.5 // translucent, so a kept alpha means Blend
            } else {
                (x % 256) as f32 / 255.0
            }
        })
        .collect()
}

/// DC12 (I1): the filter the dialog shows is the one the file gets.
#[test]
fn kaiser_applies_only_where_the_dialog_shows_it() {
    let (w, h) = (32, 32);
    let px = noise(w, h);
    let with = |set: PsddsSettings, filter: u32, alpha: bool| {
        encode(&px, w, h, alpha, &PsddsSettings { filter, ..set }).unwrap()
    };
    let srgb_bc7 = PsddsSettings {
        srgb: 1,
        ..settings(BC7, 0)
    };
    assert!(
        with(srgb_bc7, 3, false) != with(srgb_bc7, 0, false),
        "Kaiser must change the mips"
    );
    // DC39: sRGB Off keeps the Diffuse profile's sRGB content, so Kaiser still applies.
    let unorm_bc7 = PsddsSettings {
        srgb: 0,
        ..srgb_bc7
    };
    assert!(
        with(unorm_bc7, 3, false) != with(unorm_bc7, 0, false),
        "Kaiser must change the mips with sRGB Off"
    );
    // Kept alpha (Blend) mips with box whatever the filter.
    for set in [srgb_bc7, unorm_bc7] {
        assert!(with(set, 3, true) == with(set, 0, true), "{set:?}");
    }
}

/// DC39: the sRGB checkbox sets only the DDS tag. Colour content (gamma-correct filtering) follows the type, so
/// sRGB On and Off give the same mips and differ only in the DXGI format.
#[test]
fn srgb_sets_only_the_tag() {
    let (w, h) = (32, 32);
    let px = noise(w, h);
    for filter in [0, 3] {
        let on = PsddsSettings {
            srgb: 1,
            filter,
            ..settings(BC7, 0)
        };
        let off = PsddsSettings { srgb: 0, ..on };
        let a = encode(&px, w, h, false, &on).unwrap();
        let b = encode(&px, w, h, false, &off).unwrap();
        assert_eq!(
            read_dds(&a).unwrap().dx10.unwrap().dxgi,
            99,
            "BC7_UNORM_SRGB"
        );
        assert_eq!(read_dds(&b).unwrap().dx10.unwrap().dxgi, 98, "BC7_UNORM");
        // 4 magic + 124 header + 20 DX10 header, then the same blocks.
        assert_eq!(a.len(), b.len());
        assert!(a[148..] == b[148..], "filter {filter}: same mips");
    }
}

/// DC12 (I2): a side above 16384 is refused before the caller's buffer is read.
#[test]
fn oversized_image_is_refused() {
    let px = [0.0f32; 4]; // far smaller than the claimed size: reading it would be out of bounds
    for (w, h) in [(16385, 1), (1, 16385), (u32::MAX, u32::MAX)] {
        let (rc, msg) = encode(&px, w, h, false, &settings(BC7, 0)).unwrap_err();
        assert_ne!(rc, 0);
        assert_eq!(
            msg,
            "This image is larger than 16384 pixels on a side, the most Skyrim supports."
        );
    }
}

#[test]
fn alpha_dropped_for_formats_without_alpha() {
    let (w, h) = (16, 8);
    let with = gradient(w, h, true);
    let opaque = gradient(w, h, false);
    for f in [BC1, BC4, BC5] {
        let s = settings(f, 2);
        let bytes = encode(&with, w, h, true, &s).unwrap();
        let dds = read_dds(&bytes).unwrap();
        assert_eq!(dds.pf_flags & DDPF_ALPHAPIXELS, 0, "format {f}");
        // Dropping alpha leaves what the same pixels give fully opaque.
        assert!(
            bytes == encode(&opaque, w, h, false, &s).unwrap(),
            "format {f}"
        );
    }
}

#[test]
fn srgb_forced_off_without_srgb_variant() {
    let px = gradient(8, 8, false);
    let bc5 = PsddsSettings {
        srgb: 1,
        ..settings(BC5, 2)
    };
    // BC5 has no _SRGB form, so 83 holds either way: this pins only that the encode succeeds and is untagged. The
    // rule itself is checked in tests/settings.rs; the BC7 cases below show the tag does follow the setting.
    let dds = read_dds(&encode(&px, 8, 8, false, &bc5).unwrap()).unwrap();
    assert_eq!(dds.dx10.unwrap().dxgi, 83, "BC5_UNORM");
    // The same remembered sRGB On does tag a BC7 file.
    let bc7 = PsddsSettings {
        srgb: 1,
        ..settings(BC7, 0)
    };
    let dds = read_dds(&encode(&px, 8, 8, false, &bc7).unwrap()).unwrap();
    assert_eq!(dds.dx10.unwrap().dxgi, 99, "BC7_UNORM_SRGB");
    // And sRGB Off writes the non-sRGB form.
    let off = PsddsSettings { srgb: 0, ..bc7 };
    let dds = read_dds(&encode(&px, 8, 8, false, &off).unwrap()).unwrap();
    assert_eq!(dds.dx10.unwrap().dxgi, 98, "BC7_UNORM");
}

#[test]
fn bad_input_is_an_error_not_a_panic() {
    let px = gradient(4, 4, false);
    let ok = settings(BC7, 0);
    let mut out = PsddsBuf {
        data: std::ptr::null_mut(),
        len: 0,
    };
    let call = |rgba: *const f32, w: u32, h: u32, s: *const PsddsSettings, out: *mut PsddsBuf| {
        let mut err = [0x7f as std::ffi::c_char; 16];
        let rc = unsafe { psdds_encode(rgba, w, h, 0, s, out, err.as_mut_ptr(), err.len()) };
        assert_ne!(rc, 0);
        let msg = unsafe { CStr::from_ptr(err.as_ptr()) }.to_bytes();
        // Every message is longer than 15 bytes, so each one is cut to fit with its NUL.
        assert_eq!(msg.len(), 15, "{msg:?}");
        std::str::from_utf8(msg).unwrap().to_owned()
    };
    // Depths other than 8, 16 and 32 are refused before rgba is read.
    let call_d = |depth: u32, rgba: *const f32, cap: usize| {
        let mut err = [0x7f as std::ffi::c_char; 64];
        let mut o = PsddsBuf {
            data: std::ptr::null_mut(),
            len: 0,
        };
        let rc = unsafe {
            psdds_encode_depth(
                rgba,
                4,
                4,
                0,
                depth,
                &ok,
                &mut o,
                err.as_mut_ptr(),
                cap.min(64),
            )
        };
        assert_eq!(rc, 1);
        assert!(o.data.is_null());
        unsafe { CStr::from_ptr(err.as_ptr()) }
            .to_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(call_d(24, px.as_ptr(), 64), "depth 24 out of range");
    assert_eq!(call_d(0, px.as_ptr(), 64), "depth 0 out of range");
    assert_eq!(
        call_d(24, std::ptr::null(), 64),
        "psdds_encode: null pointer argument"
    );
    assert_eq!(call_d(24, px.as_ptr(), 16).len(), 15, "cut to fit");
    call(std::ptr::null(), 4, 4, &ok, &mut out);
    call(px.as_ptr(), 0, 4, &ok, &mut out);
    call(px.as_ptr(), 4, 0, &ok, &mut out);
    call(px.as_ptr(), u32::MAX, u32::MAX, &ok, &mut out);
    call(px.as_ptr(), 4, 4, std::ptr::null(), &mut out);
    call(px.as_ptr(), 4, 4, &ok, std::ptr::null_mut());
    let bad = PsddsSettings { format: 9, ..ok };
    call(px.as_ptr(), 4, 4, &bad, &mut out);
    assert!(out.data.is_null());
    // No error buffer at all is still just an error.
    let rc = unsafe {
        psdds_encode(
            std::ptr::null(),
            4,
            4,
            0,
            &ok,
            &mut out,
            std::ptr::null_mut(),
            0,
        )
    };
    assert_ne!(rc, 0);
    // A one-byte error buffer gets just the NUL.
    let mut one = [0x7f as std::ffi::c_char; 1];
    let rc = unsafe {
        psdds_encode(
            std::ptr::null(),
            4,
            4,
            0,
            &ok,
            &mut out,
            one.as_mut_ptr(),
            1,
        )
    };
    assert_ne!(rc, 0);
    assert_eq!(one[0], 0);
    // Freeing an empty buffer is a no-op.
    unsafe { psdds_free(out) };
}

/// DC36: with "Discard transparency" a document with alpha saves exactly as the same pixels fully opaque.
#[test]
fn discard_transparency_saves_opaque() {
    let (w, h) = (16, 8);
    let with = gradient(w, h, true);
    let opaque = gradient(w, h, false);
    for f in [BC3, BC7, RGBA8] {
        let s = PsddsSettings {
            discard_alpha: 1,
            ..settings(f, 0)
        };
        let bytes = encode(&with, w, h, true, &s).unwrap();
        assert!(
            bytes == encode(&opaque, w, h, false, &s).unwrap(),
            "format {f}"
        );
        let kept = encode(&with, w, h, true, &settings(f, 0)).unwrap();
        assert!(
            bytes != kept,
            "format {f}: without discard the alpha is kept"
        );
    }
}

#[test]
fn same_input_gives_same_bytes() {
    // Owner saw two saves of one document differ by a few levels; this pins the encoder side.
    let (w, h) = (512, 512);
    let mut px = noise(w, h);
    for a in px.iter_mut().skip(3).step_by(4) {
        *a = 1.0;
    }
    for quality in 0..psdds::psdds_quality_count(BC7) {
        let s = PsddsSettings {
            quality,
            ..settings(BC7, 0)
        };
        let first = encode(&px, w, h, false, &s).unwrap();
        for _ in 0..3 {
            assert!(
                encode(&px, w, h, false, &s).unwrap() == first,
                "BC7 quality {quality}: bytes changed between runs"
            );
        }
    }
}

fn colour(format: u32) -> PsddsSettings {
    settings(format, 0)
}

fn dxgi(bytes: &[u8]) -> u32 {
    read_dds(bytes).unwrap().dx10.unwrap().dxgi
}

fn lin(v: f32) -> f32 {
    engine::ops::srgb_to_linear(v)
}

/// Spec table row 1: the old call is the 8-bit call, and a deep document with an old format saves the same bytes.
#[test]
fn old_calls_are_depth_8() {
    let (w, h) = (32, 16);
    for px in [gradient(w, h, true), noise(w, h)] {
        for ty in 0..3 {
            for f in 0..6 {
                for alpha in [false, true] {
                    let s = settings(f, ty);
                    let old = encode(&px, w, h, alpha, &s).unwrap();
                    let d8 = encode_d(&px, w, h, alpha, 8, &s).unwrap();
                    let d16 = encode_d(&px, w, h, alpha, 16, &s).unwrap();
                    assert!(
                        old == d8 && old == d16,
                        "type {ty} format {f} alpha {alpha}"
                    );
                }
            }
        }
        // Review Focus 4: BC6H at depth 8 is the DC32 fallback, BC7 at the same quality index (DC32 keeps it).
        let bc6h = colour(6);
        let bc7 = PsddsSettings {
            format: BC7,
            quality: bc6h.quality,
            ..bc6h
        };
        for alpha in [false, true] {
            let want = encode(&px, w, h, alpha, &bc7).unwrap();
            assert!(encode(&px, w, h, alpha, &bc6h).unwrap() == want, "old call");
            assert!(
                encode_d(&px, w, h, alpha, 8, &bc6h).unwrap() == want,
                "depth 8"
            );
        }
    }
}

#[test]
fn colour_32_bit_to_hdr_keeps_values_above_one() {
    let px: Vec<f32> = (0..16).flat_map(|_| [4.0, 0.5, 0.25, 1.0]).collect();
    let bytes = encode_d(&px, 4, 4, false, 32, &colour(7)).unwrap();
    assert_eq!(dxgi(&bytes), 10);
    // DX10 header ends at 148; halves follow.
    let at = |i: usize| engine_half(u16::from_le_bytes([bytes[148 + 2 * i], bytes[149 + 2 * i]]));
    assert_eq!([at(0), at(1), at(2)], [4.0, 0.5, 0.25]);
    let bc6h = encode_d(&px, 4, 4, false, 32, &colour(6)).unwrap();
    assert_eq!(dxgi(&bc6h), 95);
    let got = decode_top(&bc6h)[0][0];
    assert!((got - 4.0).abs() <= 0.08, "{got}");
    // An odd size keeps its size.
    let px: Vec<f32> = (0..15).flat_map(|_| [4.0, 0.5, 0.25, 1.0]).collect();
    for f in [6, 7] {
        let dds = read_dds(&encode_d(&px, 5, 3, false, 32, &colour(f)).unwrap()).unwrap();
        assert_eq!((dds.width, dds.height), (5, 3), "format {f}");
    }
}

/// The half-float value of IEEE binary16 bits (exact in f32).
fn engine_half(b: u16) -> f32 {
    let (s, e, m) = (b >> 15, i32::from((b >> 10) & 31), f32::from(b & 1023));
    let v = if e == 0 {
        m * 2f32.powi(-24)
    } else {
        (1.0 + m / 1024.0) * 2f32.powi(e - 15)
    };
    if s == 1 {
        -v
    } else {
        v
    }
}

#[test]
fn colour_32_bit_to_ldr_is_linear_then_srgb() {
    let (w, h) = (8, 8);
    let img = |v: f32| -> Vec<f32> { (0..w * h).flat_map(|_| [v, v, v, 1.0]).collect() };
    for (f, tol) in [(RGBA8, 1.0), (BC7, 2.0)] {
        let s = colour(f);
        // Linear 0.214... at depth 32 is sRGB 0.5 at depth 16.
        let a = decode_top(&encode_d(&img(lin(0.5)), w, h, false, 32, &s).unwrap())[0][0];
        let b = decode_top(&encode_d(&img(0.5), w, h, false, 16, &s).unwrap())[0][0];
        assert!(
            (a - b).abs() <= tol / 255.0 + 1e-6,
            "format {f}: {a} vs {b}"
        );
        // The ends are exact, and 4.0 clips to 1.0 after the conversion.
        for (v32, v16) in [(0.0, 0.0), (1.0, 1.0), (4.0, 1.0)] {
            assert!(
                encode_d(&img(v32), w, h, false, 32, &s).unwrap()
                    == encode_d(&img(v16), w, h, false, 16, &s).unwrap(),
                "format {f} value {v32}"
            );
        }
    }
}

/// Review Focus 2: Normal and Linear mask data is not light, so 32 bits are clamped, not converted.
#[test]
fn raw_data_from_32_bit_is_clamped_not_converted() {
    let (w, h) = (8, 8);
    let clamp = |v: f32| if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) };
    for odd in [[1.2, -0.1, f32::NAN, 0.5], [f32::INFINITY, 2.0, -5.0, 1.5]] {
        let px: Vec<f32> = (0..w * h).flat_map(|_| odd).collect();
        let want: Vec<f32> = px.iter().map(|&v| clamp(v)).collect();
        for (ty, f) in [(1, BC5), (1, BC7), (2, BC4), (2, BC7)] {
            let s = settings(f, ty);
            assert!(
                encode_d(&px, w, h, true, 32, &s).unwrap()
                    == encode_d(&want, w, h, true, 16, &s).unwrap(),
                "type {ty} format {f} {odd:?}"
            );
        }
    }
}

/// Review Focus 3: a stored (sRGB-coded) 16-bit Colour value goes to HDR as linear light.
#[test]
fn sixteen_bit_colour_to_hdr_is_decoded_to_linear() {
    let px: Vec<f32> = (0..16).flat_map(|_| [0.5, 0.5, 0.5, 1.0]).collect();
    let got = decode_top(&encode_d(&px, 4, 4, false, 16, &colour(7)).unwrap())[0][0];
    assert!((got - lin(0.5)).abs() <= 1e-3, "{got}");
}

/// Review Focus 5: nothing non-finite reaches the file, and huge values stop at the half maximum.
#[test]
fn hdr_non_finite_and_huge_values() {
    let px = [
        f32::INFINITY,
        f32::NAN,
        1e9,
        1.5, // RGB is not clamped; alpha is
        -3.0,
        -3.0,
        -3.0,
        1.5,
    ];
    let s = PsddsSettings {
        mips: 0,
        ..colour(7)
    };
    let bytes = encode_d(&px, 2, 1, true, 32, &s).unwrap();
    let halves: Vec<u16> = bytes[148..]
        .chunks(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    assert_eq!(halves.len(), 8);
    assert!(halves.iter().all(|h| (h >> 10) & 31 != 31), "inf or NaN");
    let v: Vec<f32> = halves.into_iter().map(engine_half).collect();
    assert_eq!(v, [65504.0, 0.0, 65504.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
}

#[test]
fn rgba16f_keeps_alpha_bc6h_drops_it() {
    let (w, h) = (8, 8);
    let rgb = |i: u32| (i % 7) as f32 * 0.3;
    let make = |a: f32| -> Vec<f32> {
        (0..w * h)
            .flat_map(|i| [rgb(i), rgb(i + 2), rgb(i + 4), a])
            .collect()
    };
    let s = PsddsSettings {
        mips: 0,
        ..colour(7)
    };
    let d = decode_top(&encode_d(&make(0.5), w, h, true, 32, &s).unwrap());
    assert!(d.iter().all(|p| p[3] == 0.5));
    let b = colour(6);
    assert!(
        encode_d(&make(0.5), w, h, true, 32, &b).unwrap()
            == encode_d(&make(1.0), w, h, false, 32, &b).unwrap()
    );
}
