//! Pins the BC4 reference search (docs/research/bc4-family.md): `shipped` to the upstream rgbcx path, `search` to its
//! never-worse rule, to the patched C++ (BC3/BC4/BC5 entry points) and to the engine's encode path.
use encoders_sys::{
    bc3_encode_block, bc4_encode_block, bc5_encode_block, gather_blocks, RGBCX_MAX_LEVEL,
};
use sdc_bench::bc4ref::{search, shipped};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn byte(&mut self) -> u8 {
        (self.next() >> 24) as u8
    }
}

/// Uniform, two-level 0/255 plus noise, ramps and near-constant blocks, in rotation.
fn blocks(n: usize) -> Vec<[u8; 16]> {
    let mut r = Rng(0x9E37_79B9_7F4A_7C15);
    (0..n)
        .map(|i| {
            let mut v = [0u8; 16];
            match i % 4 {
                0 => v.iter_mut().for_each(|x| *x = r.byte()),
                1 => v.iter_mut().for_each(|x| {
                    let base = if r.next() & 1 == 0 { 0i32 } else { 255 };
                    let noise = if r.next().is_multiple_of(3) {
                        (r.byte() % 9) as i32
                    } else {
                        0
                    };
                    *x = (if base == 0 {
                        base + noise
                    } else {
                        base - noise
                    }) as u8;
                }),
                2 => {
                    let (a, b) = (r.byte() as i32, r.byte() as i32);
                    for (k, x) in v.iter_mut().enumerate() {
                        *x = (a + (b - a) * k as i32 / 15) as u8;
                    }
                }
                _ => {
                    let c = r.byte();
                    v.iter_mut()
                        .for_each(|x| *x = c.saturating_add(r.byte() % 3).saturating_sub(1))
                }
            }
            v
        })
        .collect()
}

fn px(v: &[u8; 16], ch: usize) -> [[u8; 4]; 16] {
    let mut p = [[7u8; 4]; 16];
    for i in 0..16 {
        p[i][ch] = v[i];
    }
    p
}

fn sse(b: &[u8; 8], v: &[u8; 16]) -> u64 {
    let mut d = [0u8; 16];
    bcdec_rs::bc4(b, &mut d, 4, false);
    d.iter()
        .zip(v)
        .map(|(&x, &y)| (x as i64 - y as i64).pow(2) as u64)
        .sum()
}

fn flips(b: &[u8; 8], v: &[u8; 16]) -> u32 {
    let mut d = [0u8; 16];
    bcdec_rs::bc4(b, &mut d, 4, false);
    d.iter()
        .zip(v)
        .filter(|(&x, &y)| (x >= 128) != (y >= 128))
        .count() as u32
}

#[test]
fn shipped_port_equals_ffi() {
    for v in blocks(20_000) {
        for ch in 0..4 {
            let p = px(&v, ch);
            for r in [4, 5, 10] {
                assert_eq!(
                    shipped(&v, r),
                    encoders_sys::bc4_encode_block_flags(
                        &p,
                        ch,
                        r,
                        encoders_sys::BC4_USE_ALL_MODES
                    ),
                    "{v:?} ch {ch} r {r}"
                );
            }
        }
    }
}

#[test]
fn search_never_worse() {
    for v in blocks(20_000) {
        for r in [4, 5, 10] {
            let s = shipped(&v, r);
            let a = search(&v, r, false);
            assert!(sse(&a, &v) <= sse(&s, &v), "{v:?} r {r}");
            let g = search(&v, r, true);
            assert!(sse(&g, &v) <= sse(&s, &v), "{v:?} r {r} guard");
            assert!(flips(&g, &v) <= flips(&s, &v), "{v:?} r {r} guard");
        }
    }
}

#[test]
fn const_and_binary_blocks_unchanged() {
    let mut checker = [0u8; 16];
    for (i, x) in checker.iter_mut().enumerate() {
        *x = if (i + i / 4) % 2 == 0 { 0 } else { 255 };
    }
    for v in [[0u8; 16], [255; 16], [77; 16], checker] {
        assert_eq!(search(&v, 5, false), shipped(&v, 5), "{v:?}");
        assert_eq!(search(&v, 5, true), shipped(&v, 5), "{v:?}");
    }
}

#[test]
fn trimmed_range_beats_shipped_on_near_extremes() {
    let v = [
        0, 0, 1, 2, 40, 80, 120, 160, 200, 253, 254, 255, 255, 255, 128, 64,
    ];
    assert!(sse(&search(&v, 5, false), &v) < sse(&shipped(&v, 5), &v));
}

// The vendored C++ (shim flags BC4_SDC_SEARCH, plus BC4_SDC_GUARD128 for BC3 alpha) equals the reference byte for
// byte; radius 10 is `max`.
#[test]
fn ffi_equals_reference() {
    let bs = blocks(20_000);
    for (i, v) in bs.iter().enumerate() {
        let w = &bs[(i + 1) % bs.len()];
        for r in [4, 5, 10] {
            for ch in 0..4 {
                let p = px(v, ch);
                assert_eq!(
                    bc4_encode_block(&p, ch, r),
                    search(v, r, false),
                    "{v:?} ch {ch} r {r}"
                );
            }
            assert_eq!(
                bc3_encode_block(&px(v, 3), RGBCX_MAX_LEVEL, r)[..8],
                search(v, r, true),
                "{v:?} bc3 r {r}"
            );
            let mut p = px(v, 0);
            for t in 0..16 {
                p[t][1] = w[t];
            }
            let e = bc5_encode_block(&p, r);
            assert_eq!(e[..8], search(v, r, false), "{v:?} bc5 r r {r}");
            assert_eq!(e[8..], search(w, r, false), "{w:?} bc5 g r {r}");
        }
    }
}

#[test]
fn bc5_halves_equal_bc4_per_channel() {
    let bs = blocks(4_000);
    for (i, v) in bs.iter().enumerate() {
        let w = &bs[(i + 1) % bs.len()];
        let mut p = px(v, 0);
        for t in 0..16 {
            p[t][1] = w[t];
        }
        let e = bc5_encode_block(&p, 5);
        assert_eq!(e[..8], bc4_encode_block(&p, 0, 5), "{v:?}");
        assert_eq!(e[8..], bc4_encode_block(&p, 1, 5), "{w:?}");
    }
}

#[test]
fn guard_only_on_bc3() {
    // First block of `blocks(20_000)` where the guard changes the reference result.
    let v = [
        101, 185, 152, 193, 87, 248, 130, 84, 241, 174, 246, 200, 103, 217, 60, 5,
    ];
    let (v4, v5) = (search(&v, 5, false), search(&v, 5, true));
    assert_ne!(v4, v5);
    assert_eq!(bc4_encode_block(&px(&v, 0), 0, 5), v4);
    assert_eq!(bc5_encode_block(&px(&v, 0), 5)[..8], v4);
    assert_eq!(bc3_encode_block(&px(&v, 3), RGBCX_MAX_LEVEL, 5)[..8], v5);
}

/// The engine's BC4 and BC3 paths (default preset, its radius) end to end against the reference.
#[test]
fn engine_bc4_bytes_equal_reference() {
    use engine::encode::{encode_level, radius, EncodeCtx};
    use engine::quantize::EncoderInput;
    let mut r = Rng(0x1234_5678_9ABC_DEF1);
    let img: Vec<[u8; 4]> = (0..64)
        .map(|_| [r.byte(), r.byte(), r.byte(), r.byte()])
        .collect();
    let gathered = gather_blocks(&img, 8, 8);
    for (fmt, ch, bs) in [("bc4", 0, 8), ("bc3", 3, 16)] {
        let p = sdc_bench::encodeone::engine_profile(fmt, "default").unwrap();
        let rad = radius(p.encoder.bc45_search_radius);
        let out = encode_level(
            &EncoderInput::Ldr(img.clone()),
            8,
            8,
            &p,
            &EncodeCtx::default(),
        )
        .unwrap();
        for (b, src) in gathered.chunks(16).enumerate() {
            let v: [u8; 16] = std::array::from_fn(|t| src[t][ch]);
            assert_eq!(
                out[b * bs..b * bs + 8],
                search(&v, rad, fmt == "bc3"),
                "{fmt} block {b}"
            );
        }
    }
}

/// Every BC4 block of the bench's BC3 alpha, BC4 and BC5 sets (rounds 1-2), at each radius; BC5 also through
/// `bc5_encode_block`. Panics when run without `SDC_BENCH_ROOT`, so a mis-set environment never passes for the proof.
#[test]
#[ignore = "needs SDC_BENCH_ROOT"]
fn ffi_equals_reference_on_corpus() {
    let root = sdc_bench::corpus::corpus_root().expect(
        "SDC_BENCH_ROOT must point at the bench corpus (e.g. F:\\ai\\dds\\bench) to run this test",
    );
    let (mut checked, mut bad, mut bc5_checked, mut bc5_bad) = (0u64, 0u64, 0u64, 0u64);
    for (dir, set, chans, bc3) in [
        ("images2", "mask_real", &[0usize][..], false),
        ("images2", "normal_real", &[0, 1][..], false),
        ("images", "cutout_real", &[3][..], true),
    ] {
        for png in sdc_bench::corpus::set_files(&root, dir, set, "png").unwrap() {
            let img = sdc_bench::corpus::load_png_rgba8(&png).unwrap();
            let gathered = gather_blocks(&img.px, img.width, img.height);
            for src in gathered.chunks(16) {
                let src: &[[u8; 4]; 16] = src.try_into().unwrap();
                for &ch in chans {
                    let v: [u8; 16] = std::array::from_fn(|t| src[t][ch]);
                    for r in [4, 5, 10] {
                        let got = if bc3 {
                            bc3_encode_block(src, RGBCX_MAX_LEVEL, r)[..8]
                                .try_into()
                                .unwrap()
                        } else {
                            bc4_encode_block(src, ch, r)
                        };
                        checked += 1;
                        bad += (got != search(&v, r, bc3)) as u64;
                    }
                }
                // BC5 through its own entry point: R and G halves
                if set == "normal_real" {
                    for r in [4, 5, 10] {
                        let e = bc5_encode_block(src, r);
                        for (k, ch) in [0usize, 1].into_iter().enumerate() {
                            let v: [u8; 16] = std::array::from_fn(|t| src[t][ch]);
                            bc5_checked += 1;
                            bc5_bad += (e[8 * k..8 * k + 8] != search(&v, r, false)) as u64;
                        }
                    }
                }
            }
        }
    }
    eprintln!(
        "corpus: {checked} block-channel-radius checks, {bad} mismatches; bc5_encode_block: {bc5_checked} half-radius checks, {bc5_bad} mismatches"
    );
    assert!(checked > 0 && bc5_checked > 0);
    assert_eq!(
        (bad, bc5_bad),
        (0, 0),
        "{bad} of {checked}, bc5 {bc5_bad} of {bc5_checked} mismatched"
    );
}
