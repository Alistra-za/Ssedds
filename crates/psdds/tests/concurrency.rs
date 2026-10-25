//! Five threads (four encodes and a TruePBR pair) race the first calls of a cold library; the result must equal a serial run (R7, Task 2).
//! Own test binary, one test, so no earlier test has warmed the profile caches.
use psdds::*;
use std::ffi::c_char;
use std::sync::{Arc, Barrier};

const W: u32 = 64;
const H: u32 = 48;

fn pixels(seed: u32) -> Vec<f32> {
    (0..W * H)
        .flat_map(|i| {
            let (x, y) = (i % W, i / W);
            let v = |k: u32| ((x * (3 + k) + y * (5 + seed) + k * 17) % 256) as f32 / 255.0;
            [v(0), v(1), v(2), 0.25 + 0.5 * ((x + y) % 2) as f32]
        })
        .collect()
}

/// BC7 colour with sRGB set explicitly (DC34), BC1 discarding alpha, BC5 normal, BC4 linear mask.
fn cases() -> Vec<(PsddsSettings, Vec<f32>)> {
    let s = |format, texture_type, srgb, discard_alpha| PsddsSettings {
        format,
        texture_type,
        srgb,
        filter: 0,
        mips: 1,
        quality: 0,
        discard_alpha,
    };
    vec![
        (s(4, 0, 1, 0), pixels(1)),
        (s(0, 0, 0, 1), pixels(2)),
        (s(3, 1, 0, 0), pixels(3)),
        (s(2, 2, 0, 0), pixels(4)),
    ]
}

/// resolve, encode and decode one case; returns (dds bytes, decoded rgba8).
fn run(s: &PsddsSettings, px: &[f32]) -> (Vec<u8>, Vec<u8>) {
    unsafe {
        assert_eq!(psdds_format_allowed(s.texture_type, s.format), 1);
        let (mut eff, mut ctl) = (*s, std::mem::zeroed::<PsddsControls>());
        assert_eq!(psdds_resolve(s, 1, &mut eff, &mut ctl), 0);
        let mut err = [0 as c_char; 256];
        let mut buf = PsddsBuf {
            data: std::ptr::null_mut(),
            len: 0,
        };
        let rc = psdds_encode(
            px.as_ptr(),
            W,
            H,
            1,
            &eff,
            &mut buf,
            err.as_mut_ptr(),
            err.len(),
        );
        assert_eq!(rc, 0, "encode failed");
        let dds = std::slice::from_raw_parts(buf.data, buf.len).to_vec();
        let (mut w, mut h, mut a) = (0, 0, 0);
        let mut out = PsddsBuf {
            data: std::ptr::null_mut(),
            len: 0,
        };
        let rc = psdds_decode(
            dds.as_ptr(),
            dds.len(),
            &mut w,
            &mut h,
            &mut a,
            &mut out,
            err.as_mut_ptr(),
            err.len(),
        );
        assert_eq!(rc, 0, "decode failed");
        assert_eq!((w, h), (W, H));
        let rgba = std::slice::from_raw_parts(out.data, out.len).to_vec();
        assert_eq!(rgba.len(), (W * H * 4) as usize);
        psdds_free(buf);
        psdds_free(out);
        (dds, rgba)
    }
}

/// The fifth thread (Task 7): a TruePBR pair, roughness + AO + a normal; returns (_rmaos, _n).
fn pair() -> (Vec<u8>, Vec<u8>) {
    let r: Vec<f32> = pixels(5).iter().step_by(4).copied().collect();
    let n: Vec<f32> = (0..64 * 64)
        .flat_map(|i| [0.5 + 0.3 * ((i % 7) as f32 / 7.0), 0.5, 1.0, 1.0])
        .collect();
    let source = |px: &[f32], width, height| PsddsSource {
        px: px.as_ptr(),
        width,
        height,
        channels: 1,
        channel: 0,
    };
    let none = PsddsSource {
        px: std::ptr::null(),
        width: 0,
        height: 0,
        channels: 0,
        channel: 0,
    };
    let job = PsddsRmaosJob {
        source: [source(&r, W, H), none, source(&r, W, H), none],
        fill: [-1; 4],
        quality: 0,
        mips: 1,
        normal: n.as_ptr(),
        normal_width: 64,
        normal_height: 64,
        normal_settings: psdds_defaults(1),
    };
    let empty = || PsddsBuf {
        data: std::ptr::null_mut(),
        len: 0,
    };
    let mut res = PsddsRmaosResult {
        rmaos: empty(),
        normal: empty(),
        defaulted: 0,
        baked: 0,
    };
    let mut err = [0 as c_char; 256];
    unsafe {
        let rc = psdds_rmaos_encode(
            &job,
            &mut res,
            std::ptr::null_mut(),
            0,
            err.as_mut_ptr(),
            err.len(),
        );
        assert_eq!(rc, 0, "pair failed");
        let copy = |b: &PsddsBuf| std::slice::from_raw_parts(b.data, b.len).to_vec();
        let out = (copy(&res.rmaos), copy(&res.normal));
        psdds_free(res.rmaos);
        psdds_free(res.normal);
        out
    }
}

/// Case `i`: the encode cases, then the pair.
fn case(cases: &[(PsddsSettings, Vec<f32>)], i: usize) -> (Vec<u8>, Vec<u8>) {
    cases.get(i).map_or_else(pair, |(s, px)| run(s, px))
}

#[test]
fn concurrent_calls_equal_serial() {
    let cases = Arc::new(cases());
    let threads = cases.len() + 1;
    let barrier = Arc::new(Barrier::new(threads));
    let handles: Vec<_> = (0..threads)
        .map(|i| {
            let (cases, barrier) = (cases.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                (0..10).map(|_| case(&cases, i)).collect::<Vec<_>>()
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    for (i, rounds) in results.iter().enumerate() {
        let serial = case(&cases, i);
        for r in rounds {
            assert_eq!(*r, serial, "case {i} differs from the serial run");
        }
    }
}
