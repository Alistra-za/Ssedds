//! `psdds_encode` writes the engine's bytes for the resolved profile, on any size, and fails without panicking.
use engine::dds::{check_header, full_chain_len, read_dds, DDPF_ALPHAPIXELS};
use engine::image::Rgba32F;
use engine::memory::encode_image;
use psdds::settings::profile_for;
use psdds::{psdds_defaults, psdds_encode, psdds_free, PsddsBuf, PsddsSettings};
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

/// The type's defaults with another format, at the `cs-quality` quality for it (rgbcx 18, radius default,
/// effort veryslow); a quality index belongs to its format's list.
fn settings(format: u32, texture_type: u32) -> PsddsSettings {
    PsddsSettings {
        format,
        quality: [18, 18, 0, 0, 4, 0][format as usize],
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
