//! `psdds_encode` writes the engine's bytes for the resolved profile, on any size, and fails without panicking.
use engine::dds::{check_header, full_chain_len, read_dds, DDPF_ALPHAPIXELS};
use engine::image::Rgba32F;
use engine::memory::encode_image;
use psdds::settings::profile_for;
use psdds::{psdds_defaults, psdds_encode, psdds_free, PsddsBuf, PsddsSettings};
use std::ffi::CStr;

const BC1: u32 = 0;
const BC4: u32 = 2;
const BC5: u32 = 3;
const BC7: u32 = 4;

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
                ..settings(f, 0)
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

#[test]
fn alpha_dropped_for_formats_without_alpha() {
    let (w, h) = (16, 8);
    let with = gradient(w, h, true);
    let opaque = gradient(w, h, false);
    for f in [BC1, BC4, BC5] {
        let s = settings(f, 0);
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
        ..settings(BC5, 0)
    };
    let dds = read_dds(&encode(&px, 8, 8, false, &bc5).unwrap()).unwrap();
    assert_eq!(dds.dx10.unwrap().dxgi, 83, "BC5_UNORM");
    // The same remembered sRGB On does tag a BC7 file.
    let bc7 = PsddsSettings {
        srgb: 1,
        ..settings(BC7, 0)
    };
    let dds = read_dds(&encode(&px, 8, 8, false, &bc7).unwrap()).unwrap();
    assert_eq!(dds.dx10.unwrap().dxgi, 99, "BC7_UNORM_SRGB");
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
        assert!(!msg.is_empty() && msg.len() <= 15, "{msg:?}");
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
    // Freeing an empty buffer is a no-op.
    unsafe { psdds_free(out) };
}
