//! Four threads race the first calls of a cold library; the result must equal a serial run (R7, Task 2).
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

#[test]
fn concurrent_calls_equal_serial() {
    let cases = Arc::new(cases());
    let barrier = Arc::new(Barrier::new(cases.len()));
    let handles: Vec<_> = (0..cases.len())
        .map(|i| {
            let (cases, barrier) = (cases.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                let (s, px) = &cases[i];
                (0..10).map(|_| run(s, px)).collect::<Vec<_>>()
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    for (i, rounds) in results.iter().enumerate() {
        let serial = run(&cases[i].0, &cases[i].1);
        for r in rounds {
            assert_eq!(*r, serial, "case {i} differs from the serial run");
        }
    }
}
