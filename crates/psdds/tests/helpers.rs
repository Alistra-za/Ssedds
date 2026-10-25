//! The shared host rules, through the C ABI: size check, openable, DC23 open alpha, DC31 save alpha, file bound.
use psdds::{
    psdds_alpha_used, psdds_check_size, psdds_dds_openable, psdds_encode, psdds_max_file_bytes,
    psdds_opens_with_alpha, PsddsBuf,
};
use std::ffi::{c_char, CStr};

const TOO_LARGE: &str =
    "This image is larger than 16384 pixels on a side, the most Skyrim supports.";

/// A 148-byte magic + header + DX10 header with only the fields the rule reads set.
fn dds_header(fourcc: [u8; 4], caps2: u32, dxgi: u32, misc: u32, array_size: u32) -> [u8; 148] {
    let mut h = [0u8; 148];
    h[..4].copy_from_slice(b"DDS ");
    h[84..88].copy_from_slice(&fourcc);
    for (at, v) in [(112, caps2), (128, dxgi), (136, misc), (140, array_size)] {
        h[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    h
}

fn openable(h: &[u8]) -> bool {
    unsafe { psdds_dds_openable(h.as_ptr(), h.len()) == 1 }
}

#[test]
fn only_openable_headers_are_claimed() {
    let dx10 = *b"DX10";
    assert!(openable(&dds_header(*b"DXT5", 0, 0, 0, 0)), "legacy BC3");
    assert!(openable(&dds_header(dx10, 0, 98, 0, 1)), "DX10 BC7");
    assert!(!openable(&dds_header(dx10, 0, 95, 0, 1)), "BC6H UF16");
    assert!(!openable(&dds_header(dx10, 0, 96, 0, 1)), "BC6H SF16");
    assert!(!openable(&dds_header(dx10, 0, 10, 0, 1)), "RGBA16F");
    assert!(!openable(&dds_header(dx10, 0, 2, 0, 1)), "RGBA32F");
    assert!(!openable(&dds_header(dx10, 0, 98, 0x4, 1)), "DX10 cube map");
    assert!(!openable(&dds_header(dx10, 0, 98, 0, 6)), "DX10 array");
    assert!(
        !openable(&dds_header(*b"DXT1", 0x200, 0, 0, 0)),
        "legacy cube map"
    );
    let f16 = 113u32.to_le_bytes();
    assert!(
        !openable(&dds_header(f16, 0, 0, 0, 0)),
        "legacy A16B16G16R16F"
    );
    let mut h = dds_header(*b"DXT5", 0, 0, 0, 0);
    assert!(openable(&h[..128]), "128 bytes, no DX10 header needed");
    assert!(openable(&h[..40]), "truncated file is claimed");
    assert!(!openable(&h[..3]), "too short for the magic");
    h[0] = b'X';
    assert!(!openable(&h), "wrong magic");
    assert_eq!(unsafe { psdds_dds_openable(std::ptr::null(), 148) }, 0);
}

#[test]
fn openable_refuses_a_len_over_isize_max() {
    let h = dds_header(*b"DXT5", 0, 0, 0, 0);
    // A caller bug; without the bound, the debug from_raw_parts check aborts the host.
    assert_eq!(unsafe { psdds_dds_openable(h.as_ptr(), usize::MAX) }, 0);
}

fn opens(px: &[u8], has_alpha: u32) -> bool {
    unsafe { psdds_opens_with_alpha(px.as_ptr(), px.len() / 4, has_alpha) == 1 }
}

#[test]
fn open_alpha_rule() {
    let opaque = [1, 2, 3, 255, 4, 5, 6, 255];
    let floor = [1, 2, 3, 254, 4, 5, 6, 255];
    let real = [1, 2, 3, 255, 4, 5, 6, 253];
    let cut = [1, 2, 3, 0, 4, 5, 6, 255];
    assert!(!opens(&opaque, 1), "all 255");
    assert!(!opens(&floor, 1), "254 is the opaque floor");
    assert!(opens(&real, 1), "253");
    assert!(opens(&cut, 1), "0");
    assert!(!opens(&cut, 0), "has_alpha 0");
    assert_eq!(unsafe { psdds_opens_with_alpha(std::ptr::null(), 2, 1) }, 0);
    assert_eq!(
        unsafe { psdds_opens_with_alpha(opaque.as_ptr(), usize::MAX, 1) },
        0
    );
}

fn used(px: &[[f32; 4]]) -> bool {
    unsafe { psdds_alpha_used(px.as_ptr().cast(), px.len()) == 1 }
}

#[test]
fn save_alpha_rule() {
    let a = |v: f32| [[0.1, 0.2, 0.3, 1.0], [0.4, 0.5, 0.6, v]];
    assert!(!used(&a(254.0 * (1.0 / 255.0))), "8-bit 254 and 255");
    assert!(used(&a(253.0 * (1.0 / 255.0))), "8-bit 253");
    assert!(
        !used(&a(32640.0 / 32768.0)),
        "16-bit 32640 is just above 254/255"
    );
    assert!(used(&a(32639.0 / 32768.0)), "16-bit 32639 is just below");
    assert!(!used(&a(f32::NAN)), "NaN alpha does not count");
    assert!(!used(&a(1.0)));
    assert_eq!(unsafe { psdds_alpha_used(std::ptr::null(), 2) }, 0);
    assert_eq!(
        unsafe { psdds_alpha_used([0.0f32; 4].as_ptr(), usize::MAX) },
        0
    );
}

#[test]
fn max_file_bytes_is_pinned() {
    assert_eq!(psdds_max_file_bytes(), 1_431_655_912);
}

fn size(w: u32, h: u32, cap: usize) -> (i32, String) {
    let mut err = [0x7f as c_char; 256];
    let rc = unsafe { psdds_check_size(w, h, err.as_mut_ptr(), cap) };
    let s = unsafe { CStr::from_ptr(err.as_ptr()) }
        .to_string_lossy()
        .into_owned();
    (rc, if cap == 0 { String::new() } else { s })
}

#[test]
fn check_size_is_the_one_rule() {
    assert_eq!(size(16385, 1, 256), (1, TOO_LARGE.into()));
    assert_eq!(size(1, 16385, 256), (1, TOO_LARGE.into()));
    assert_eq!(size(u32::MAX, u32::MAX, 256), (1, TOO_LARGE.into()));
    assert_eq!(size(16384, 16384, 256).0, 0);
    let (rc, zero) = size(0, 5, 256);
    assert_eq!(rc, 1);
    assert!(!zero.contains("psdds_"), "no function name: {zero}");
    assert_eq!(size(5, 0, 256).0, 1);
    // A 3-byte buffer holds 2 chars plus the NUL; a NULL buffer is allowed.
    assert_eq!(size(16385, 1, 3), (1, "Th".into()));
    assert_eq!(size(16385, 1, 0).0, 1);
    assert_eq!(
        unsafe { psdds_check_size(16385, 1, std::ptr::null_mut(), 8) },
        1
    );
}

fn encode_err(rgba: *const f32, w: u32, h: u32) -> String {
    let s = psdds::psdds_defaults(0);
    let mut out = PsddsBuf {
        data: std::ptr::null_mut(),
        len: 0,
    };
    let mut err = [0 as c_char; 256];
    let rc = unsafe { psdds_encode(rgba, w, h, 0, &s, &mut out, err.as_mut_ptr(), err.len()) };
    assert_ne!(rc, 0);
    unsafe { CStr::from_ptr(err.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}

#[test]
fn encode_reports_check_size_text_and_keeps_its_order() {
    let px = [0.0f32; 4];
    assert_eq!(encode_err(px.as_ptr(), 16385, 1), TOO_LARGE);
    let zero = encode_err(px.as_ptr(), 0, 1);
    assert_eq!(zero, size(0, 1, 256).1);
    assert_eq!(
        encode_err(std::ptr::null(), 16385, 1),
        "psdds_encode: null pointer argument"
    );
}
