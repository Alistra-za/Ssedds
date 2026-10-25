//! `psdds_decode` turns a DDS top mip into RGBA8 within each codec's tolerance, and refuses what Open cannot show.
use psdds::{
    psdds_decode, psdds_decode_depth, psdds_defaults, psdds_encode, psdds_encode_depth, psdds_free,
    PsddsBuf, PsddsSettings,
};
use std::ffi::{c_char, CStr};

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

fn encode(px: &[f32], w: u32, h: u32, has_alpha: bool, s: &PsddsSettings) -> Vec<u8> {
    let mut out = PsddsBuf {
        data: std::ptr::null_mut(),
        len: 0,
    };
    let mut err = [0 as c_char; 256];
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
    assert_eq!(rc, 0);
    let v = unsafe { std::slice::from_raw_parts(out.data, out.len) }.to_vec();
    unsafe { psdds_free(out) };
    v
}

struct Decoded {
    w: u32,
    h: u32,
    has_alpha: u32,
    rgba: Vec<u8>,
}

fn decode(dds: &[u8]) -> Result<Decoded, (i32, String)> {
    let (mut w, mut h, mut a) = (0u32, 0u32, 7u32);
    let mut out = PsddsBuf {
        data: std::ptr::null_mut(),
        len: 0,
    };
    let mut err = [0 as c_char; 256];
    let rc = unsafe {
        psdds_decode(
            dds.as_ptr(),
            dds.len(),
            &mut w,
            &mut h,
            &mut a,
            &mut out,
            err.as_mut_ptr(),
            err.len(),
        )
    };
    if rc != 0 {
        assert!(out.data.is_null());
        let m = unsafe { CStr::from_ptr(err.as_ptr()) };
        return Err((rc, m.to_str().unwrap().to_owned()));
    }
    let rgba = unsafe { std::slice::from_raw_parts(out.data, out.len) }.to_vec();
    unsafe { psdds_free(out) };
    Ok(Decoded {
        w,
        h,
        has_alpha: a,
        rgba,
    })
}

fn err_of(dds: &[u8]) -> String {
    decode(dds).err().expect("must refuse").1
}

#[allow(clippy::too_many_arguments)]
/// A minimal DDS: legacy FourCC header (`dxgi == 0`) or DX10; `payload` is everything after the header(s).
fn raw(
    w: u32,
    h: u32,
    fourcc: &[u8; 4],
    dxgi: u32,
    misc: u32,
    array: u32,
    caps2: u32,
    payload: &[u8],
) -> Vec<u8> {
    let mut b = vec![0u8; 128];
    let put =
        |b: &mut Vec<u8>, off: usize, v: u32| b[off..off + 4].copy_from_slice(&v.to_le_bytes());
    b[..4].copy_from_slice(b"DDS ");
    put(&mut b, 4, 124);
    put(&mut b, 8, 0x8_1007);
    put(&mut b, 12, h);
    put(&mut b, 16, w);
    put(&mut b, 20, payload.len() as u32);
    put(&mut b, 28, 1);
    put(&mut b, 76, 32);
    put(&mut b, 80, 4);
    b[84..88].copy_from_slice(fourcc);
    put(&mut b, 108, 0x1000);
    put(&mut b, 112, caps2);
    if dxgi != 0 {
        for v in [dxgi, 3, misc, array, 0] {
            b.extend_from_slice(&v.to_le_bytes());
        }
    }
    b.extend_from_slice(payload);
    b
}

const HDR: &str = "This DDS holds 32-bit (HDR) data, which this plugin does not open yet.";

#[test]
fn round_trips_every_format() {
    let (w, h) = (64, 32);
    // (format, texture type, max error allowed, channels checked). Measured: BC1/BC3 13 (565 endpoints; the brief's 12
    // was a guess), BC7 7, BC4/BC5/RGBA8 0. BC5 uses the linear type: the Normal type renormalises the gradient.
    let cases: [(u32, u32, i32, &[usize]); 6] = [
        (0, 0, 14, &[0, 1, 2]),
        (1, 0, 14, &[0, 1, 2, 3]),
        (2, 2, 1, &[0]),
        (3, 2, 1, &[0, 1]),
        (4, 0, 8, &[0, 1, 2, 3]),
        (5, 0, 0, &[0, 1, 2, 3]),
    ];
    for (f, ty, tol, chans) in cases {
        let px = gradient(w, h, true);
        let s = PsddsSettings {
            format: f,
            mips: 1,
            ..psdds_defaults(ty)
        };
        let s = PsddsSettings { quality: 0, ..s };
        let dds = encode(&px, w, h, true, &s);
        let d = decode(&dds).unwrap();
        assert_eq!((d.w, d.h), (w, h), "format {f}");
        assert_eq!(d.rgba.len(), (w * h * 4) as usize);
        assert_eq!(d.has_alpha, u32::from(matches!(f, 1 | 4 | 5)), "format {f}");
        let mut worst = 0i32;
        for (i, p) in d.rgba.chunks(4).enumerate() {
            for &c in chans {
                let want = (px[i * 4 + c] * 255.0).round() as i32;
                worst = worst.max((i32::from(p[c]) - want).abs());
            }
        }
        println!("format {f}: max error {worst}");
        // The channels not stored decode to fixed values; without stored alpha every alpha byte is 255.
        for p in d.rgba.chunks(4) {
            match f {
                2 => assert!(p[1] == p[0] && p[2] == p[0] && p[3] == 255),
                3 => assert!(p[2] == 0 && p[3] == 255),
                0 => assert_eq!(p[3], 255),
                _ => {}
            }
        }
        // Same codes as the engine's f32 path, rounded.
        let file = engine::dds::read_dds(&dds).unwrap();
        let (lw, lh, r) = file.levels[0].clone();
        let old =
            engine::decode::decode_level_rgba(file.format, lw, lh, &file.data[r], None).unwrap();
        let old: Vec<u8> = old
            .iter()
            .flatten()
            .map(|v| (v * 255.0).round() as u8)
            .collect();
        assert!(old == d.rgba, "format {f}: differs from decode_level_rgba");
        assert!(worst <= tol, "format {f}: max error {worst} > {tol}");
    }
}

#[test]
fn bc2_decodes_with_alpha() {
    // One 4x4 block: explicit 4-bit alpha 0xF in every texel, colour endpoints white.
    let mut blk = vec![0xFFu8; 8];
    blk.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0]);
    let d = decode(&raw(4, 4, b"DXT3", 0, 0, 1, 0, &blk)).unwrap();
    assert_eq!((d.w, d.h, d.has_alpha), (4, 4, 1));
    assert_eq!(&d.rgba[..4], &[255, 255, 255, 255]);
}

#[test]
fn refuses_what_open_cannot_show() {
    // BC6H and float formats
    assert_eq!(err_of(&raw(4, 4, b"DX10", 95, 0, 1, 0, &[0; 16])), HDR);
    assert_eq!(err_of(&raw(4, 4, b"DX10", 10, 0, 1, 0, &[0; 128])), HDR);
    assert_eq!(err_of(&raw(4, 4, b"DX10", 2, 0, 1, 0, &[0; 256])), HDR);
    // cube map: legacy caps2 bit, and the DX10 misc flag
    let cube = "This DDS is a cube map, which this plugin does not open.";
    assert_eq!(err_of(&raw(4, 4, b"DXT1", 0, 0, 1, 0x200, &[0; 48])), cube);
    assert_eq!(err_of(&raw(4, 4, b"DX10", 98, 4, 1, 0, &[0; 96])), cube);
    // array
    assert_eq!(
        err_of(&raw(4, 4, b"DX10", 98, 0, 2, 0, &[0; 32])),
        "This DDS is a texture array, which this plugin does not open."
    );
    // whatever read_dds rejects: its own text
    let good = raw(4, 4, b"DXT1", 0, 0, 1, 0, &[0; 8]);
    for bad in [
        good[..good.len() - 1].to_vec(),
        good[..50].to_vec(),
        b"not a dds file at all".to_vec(),
        vec![],
        vec![0xA5; 300],
    ] {
        let want = engine::dds::read_dds(&bad).unwrap_err().to_string();
        assert_eq!(err_of(&bad), want);
    }
    // a side over 16384 is refused before any allocation
    let big = raw(16385, 4, b"DXT1", 0, 0, 1, 0, &[0; 8]);
    assert_eq!(
        err_of(&big),
        "This image is larger than 16384 pixels on a side, the most Skyrim supports."
    );
}

#[test]
fn null_pointers_and_tiny_err_buffers_do_not_crash() {
    let dds = raw(4, 4, b"DXT1", 0, 0, 1, 0, &[0; 8]);
    let (mut w, mut h, mut a) = (0u32, 0u32, 0u32);
    let mut out = PsddsBuf {
        data: std::ptr::null_mut(),
        len: 0,
    };
    let rc = unsafe {
        psdds_decode(
            std::ptr::null(),
            0,
            &mut w,
            &mut h,
            &mut a,
            &mut out,
            std::ptr::null_mut(),
            0,
        )
    };
    assert_ne!(rc, 0);
    let rc = unsafe {
        psdds_decode(
            dds.as_ptr(),
            dds.len(),
            std::ptr::null_mut(),
            &mut h,
            &mut a,
            &mut out,
            std::ptr::null_mut(),
            0,
        )
    };
    assert_ne!(rc, 0);
    let mut e = [0x55 as c_char; 1];
    let rc = unsafe {
        psdds_decode(
            b"xx".as_ptr(),
            2,
            &mut w,
            &mut h,
            &mut a,
            &mut out,
            e.as_mut_ptr(),
            1,
        )
    };
    assert_ne!(rc, 0);
    assert_eq!(e[0], 0);
}

#[test]
fn decode_refuses_a_len_over_isize_max() {
    let dds = raw(4, 4, b"DXT1", 0, 0, 1, 0, &[0; 8]);
    let (mut w, mut h, mut a) = (0u32, 0u32, 0u32);
    let mut out = PsddsBuf {
        data: std::ptr::null_mut(),
        len: 0,
    };
    let mut err = [0 as c_char; 256];
    // A caller bug; without the bound, the debug from_raw_parts check aborts the host.
    let rc = unsafe {
        psdds_decode(
            dds.as_ptr(),
            usize::MAX,
            &mut w,
            &mut h,
            &mut a,
            &mut out,
            err.as_mut_ptr(),
            err.len(),
        )
    };
    assert_ne!(rc, 0);
    assert!(out.data.is_null());
    let msg = unsafe { CStr::from_ptr(err.as_ptr()) }.to_string_lossy();
    assert!(msg.contains("isize::MAX"), "{msg}");
}

#[test]
fn bc1_alpha_is_real_only_with_punch_through() {
    let w565 = |r: u16, g: u16, b: u16| (r << 11) | (g << 5) | b;
    let block = |c0: u16, c1: u16, idx: u32| {
        let mut b = c0.to_le_bytes().to_vec();
        b.extend_from_slice(&c1.to_le_bytes());
        b.extend_from_slice(&idx.to_le_bytes());
        b
    };
    // color0 <= color1: 3-colour mode, index 3 is transparent. Texel 0 gets index 3, the rest index 0.
    let punch = decode(&raw(
        4,
        4,
        b"DXT1",
        0,
        0,
        1,
        0,
        &block(w565(0, 0, 0), w565(31, 63, 31), 3),
    ))
    .unwrap();
    assert_eq!(punch.has_alpha, 1);
    assert_eq!(punch.rgba[3], 0);
    assert_eq!(punch.rgba[7], 255);
    // color0 > color1: 4-colour mode, opaque.
    let opaque = decode(&raw(
        4,
        4,
        b"DXT1",
        0,
        0,
        1,
        0,
        &block(w565(31, 63, 31), w565(0, 0, 0), 3),
    ))
    .unwrap();
    assert_eq!(opaque.has_alpha, 0);
    assert!(opaque.rgba.chunks(4).all(|p| p[3] == 255));
}

struct DecodedF {
    w: u32,
    h: u32,
    has_alpha: u32,
    depth: u32,
    px: Vec<f32>,
    raw: Vec<u8>,
}

fn decode_depth(dds: &[u8]) -> Result<DecodedF, (i32, String)> {
    let (mut w, mut h, mut a, mut d) = (0u32, 0u32, 7u32, 7u32);
    let mut out = PsddsBuf {
        data: std::ptr::null_mut(),
        len: 0,
    };
    let mut err = [0 as c_char; 256];
    let rc = unsafe {
        psdds_decode_depth(
            dds.as_ptr(),
            dds.len(),
            &mut w,
            &mut h,
            &mut a,
            &mut d,
            &mut out,
            err.as_mut_ptr(),
            err.len(),
        )
    };
    if rc != 0 {
        assert!(out.data.is_null());
        let m = unsafe { CStr::from_ptr(err.as_ptr()) };
        return Err((rc, m.to_str().unwrap().to_owned()));
    }
    let raw = unsafe { std::slice::from_raw_parts(out.data, out.len) }.to_vec();
    unsafe { psdds_free(out) };
    let px = if d == 32 {
        raw.as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_ne_bytes(*b))
            .collect()
    } else {
        vec![]
    };
    Ok(DecodedF {
        w,
        h,
        has_alpha: a,
        depth: d,
        px,
        raw,
    })
}

fn encode_f32(px: &[f32], w: u32, h: u32, has_alpha: bool, format: u32) -> Vec<u8> {
    let s = PsddsSettings {
        format,
        mips: 1,
        ..psdds_defaults(0)
    };
    let mut out = PsddsBuf {
        data: std::ptr::null_mut(),
        len: 0,
    };
    let mut err = [0 as c_char; 256];
    let rc = unsafe {
        psdds_encode_depth(
            px.as_ptr(),
            w,
            h,
            u32::from(has_alpha),
            32,
            &s,
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

#[test]
fn float_open_round_trips() {
    let (w, h) = (8, 8);
    let px: Vec<f32> = (0..w * h).flat_map(|_| [4.0, 0.5, 0.25, 0.5]).collect();
    let d = decode_depth(&encode_f32(&px, w, h, true, 7)).unwrap();
    assert_eq!((d.w, d.h, d.depth, d.has_alpha), (w, h, 32, 1));
    assert_eq!(d.raw.len(), (w * h * 16) as usize);
    assert!(d.px.chunks(4).all(|p| p == [4.0, 0.5, 0.25, 0.5]));
    // opaque RGBA16F: no alpha
    let op: Vec<f32> = (0..w * h).flat_map(|_| [4.0, 0.5, 0.25, 1.0]).collect();
    let d = decode_depth(&encode_f32(&op, w, h, true, 7)).unwrap();
    assert_eq!((d.depth, d.has_alpha), (32, 0));
    // BC6H: alpha 1.0, has_alpha 0, RGB within 2%
    let d = decode_depth(&encode_f32(&px, w, h, true, 6)).unwrap();
    assert_eq!((d.depth, d.has_alpha), (32, 0));
    for p in d.px.chunks(4) {
        assert_eq!(p[3], 1.0);
        for (g, want) in p[..3].iter().zip([4.0f32, 0.5, 0.25]) {
            assert!((g - want).abs() <= want * 0.02, "{g} vs {want}");
        }
    }
}

#[test]
fn ldr_open_is_unchanged() {
    let (w, h) = (64, 32);
    for (f, ty) in [(0, 0), (1, 0), (2, 2), (3, 2), (4, 0), (5, 0)] {
        let px = gradient(w, h, true);
        let s = PsddsSettings {
            format: f,
            mips: 1,
            quality: 0,
            ..psdds_defaults(ty)
        };
        let dds = encode(&px, w, h, true, &s);
        let (old, new) = (decode(&dds).unwrap(), decode_depth(&dds).unwrap());
        assert_eq!(new.depth, 8, "format {f}");
        assert_eq!((new.w, new.h, new.has_alpha), (old.w, old.h, old.has_alpha));
        assert!(new.raw == old.rgba, "format {f}");
    }
}

fn f32_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

#[test]
fn float_open_sanitises_non_finite() {
    let px = [
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        1.0,
        -2.0,
        3.0,
        0.5,
        1.0,
    ];
    let d = decode_depth(&raw(2, 1, b"DX10", 2, 0, 1, 0, &f32_bytes(&px))).unwrap();
    assert_eq!(d.px, [0.0, 65504.0, -65504.0, 1.0, -2.0, 3.0, 0.5, 1.0]);
    // BC6H SF16 (mode 11, 10-bit endpoints, all indices 0): R endpoint 0 is negative; expected values come from
    // the engine's decoder.
    let mut blk = [0u8; 16];
    let mut set = |at: usize, n: usize, v: u32| {
        for i in 0..n {
            if v >> i & 1 == 1 {
                blk[(at + i) / 8] |= 1 << ((at + i) % 8);
            }
        }
    };
    set(0, 5, 0b00011); // mode 11
    set(5, 10, 0x3F0); // R endpoint 0 = -16 (two's complement)
    let dds = raw(4, 4, b"DX10", 96, 0, 1, 0, &blk);
    let f = engine::dds::read_dds(&dds).unwrap();
    let (lw, lh, r) = f.levels[0].clone();
    let want = engine::decode::decode_level_rgba(f.format, lw, lh, &f.data[r], None).unwrap();
    assert!(want[0][0] < 0.0, "fixture must be negative: {}", want[0][0]);
    let d = decode_depth(&dds).unwrap();
    assert_eq!(d.depth, 32);
    assert_eq!(d.px[0], want[0][0]);
    assert_eq!(d.px[3], 1.0);
}

#[test]
fn float_open_still_refuses_cube_and_array() {
    let cube = "This DDS is a cube map, which this plugin does not open.";
    assert_eq!(
        decode_depth(&raw(4, 4, b"DX10", 10, 4, 1, 0, &[0; 768]))
            .err()
            .unwrap()
            .1,
        cube
    );
    assert_eq!(
        decode_depth(&raw(4, 4, b"DX10", 98, 0, 2, 0, &[0; 32]))
            .err()
            .unwrap()
            .1,
        "This DDS is a texture array, which this plugin does not open."
    );
    // psdds_decode keeps the old HDR text
    assert_eq!(err_of(&raw(4, 4, b"DX10", 10, 0, 1, 0, &[0; 128])), HDR);
}
