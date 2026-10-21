use engine::config::*;
use engine::encode::*;
use engine::image::Rgba32F;
use engine::mips::build_chain;
use engine::ops::preprocess;
use engine::presets::PresetRegistry;
use engine::quantize::{to_encoder_input, EncoderInput};
use std::sync::{atomic::AtomicBool, Arc};

fn profile(st: Subtype) -> Profile {
    PresetRegistry::builtin()
        .unwrap()
        .profile("cs-quality", Target::Cs, st)
        .unwrap()
}

fn gradient(w: u32, h: u32) -> Vec<[u8; 4]> {
    (0..w * h)
        .map(|i| {
            [
                (i * 7 % 256) as u8,
                (i * 13 % 256) as u8,
                (i * 3 % 256) as u8,
                255,
            ]
        })
        .collect()
}

fn encode(input: &EncoderInput, w: u32, h: u32, p: &Profile) -> Vec<u8> {
    encode_level(input, w, h, p, &EncodeCtx::default())
        .ok()
        .unwrap()
}

#[test]
fn sizes_per_format_including_sub_block_levels() {
    for (st, fmt, bytes) in [
        (Subtype::Diffuse, Format::Bc7, 16),
        (Subtype::Diffuse, Format::Bc3, 16),
        (Subtype::Diffuse, Format::Bc1, 8),
        (Subtype::Envmask, Format::Bc4, 8),
        (Subtype::Flow, Format::Bc5, 16),
    ] {
        let mut p = profile(st);
        p.format = fmt;
        let out = encode(&EncoderInput::Ldr(gradient(8, 4)), 8, 4, &p);
        assert_eq!(out.len(), 2 * bytes, "{fmt:?} 8x4");
        for (w, h, blocks) in [(2, 1, 1), (1, 1, 1), (2, 2, 1), (5, 3, 2), (9, 9, 9)] {
            let out = encode(&EncoderInput::Ldr(gradient(w, h)), w, h, &p);
            assert_eq!(out.len(), blocks * bytes, "{fmt:?} {w}x{h}");
        }
    }
    let p = profile(Subtype::PbrEmissive);
    assert_eq!(p.format, Format::Bc6hUf16);
    let hdr = |n| EncoderInput::Hdr(vec![[0x3C00, 0x3C00, 0x3C00, 0x3C00]; n]);
    assert_eq!(encode(&hdr(2), 2, 1, &p).len(), 16);
    assert_eq!(encode(&hdr(1), 1, 1, &p).len(), 16);
    assert_eq!(encode(&hdr(30), 6, 5, &p).len(), 4 * 16);
}

#[test]
fn sub_block_levels_pad_by_edge_replication() {
    // A 1x1 level must encode exactly like the same colour filling a whole block.
    let mut p = profile(Subtype::Diffuse);
    for fmt in [
        Format::Bc7,
        Format::Bc1,
        Format::Bc3,
        Format::Bc4,
        Format::Bc5,
    ] {
        p.format = fmt;
        let px = [200u8, 100, 50, 255];
        let small = encode(&EncoderInput::Ldr(vec![px]), 1, 1, &p);
        let full = encode(&EncoderInput::Ldr(vec![px; 16]), 4, 4, &p);
        assert_eq!(small, full, "{fmt:?}");
    }
    let p = profile(Subtype::PbrEmissive);
    let h = [0x3C00u16, 0x4000, 0x4200, 0x3C00];
    assert_eq!(
        encode(&EncoderInput::Hdr(vec![h]), 1, 1, &p),
        encode(&EncoderInput::Hdr(vec![h; 16]), 4, 4, &p)
    );
}

/// A dark opaque ramp that upstream bc7e u5 encodes as mode 6 with a p-bit-0 endpoint, so alpha
/// decodes to 254 (the block `encoders-sys/tests/bc7.rs` uses), tiled over a 64x64 image.
fn dark_ramp_image() -> Vec<[u8; 4]> {
    (0..64 * 64u32)
        .map(|i| {
            let t = ((i / 64 % 4) * 4 + i % 4) as u8;
            [t, t / 2, 100 + t, 255]
        })
        .collect()
}

fn non_opaque_texels(bc7: &[u8]) -> usize {
    bc7.chunks(16)
        .map(|b| {
            let mut d = [0u8; 64];
            bcdec_rs::bc7(b, &mut d, 16);
            d.chunks(4).filter(|t| t[3] != 255).count()
        })
        .sum()
}

#[test]
fn exact_opaque_bc7_decodes_255_everywhere() {
    let mut p = profile(Subtype::ComplexMaterial); // exact_opaque on by default
    p.alpha.policy = AlphaPolicy::Remove;
    assert_eq!(p.format, Format::Bc7);
    assert!(p.alpha.exact_opaque);
    let input = EncoderInput::Ldr(dark_ramp_image());
    // Without the flag this image decodes alpha 254 (DECISIONS Note A), so the check below bites.
    let mut off = p.clone();
    off.alpha.exact_opaque = false;
    assert!(non_opaque_texels(&encode(&input, 64, 64, &off)) > 0);
    assert_eq!(non_opaque_texels(&encode(&input, 64, 64, &p)), 0);
    // A busier opaque image stays exact too.
    let out = encode(&EncoderInput::Ldr(gradient(64, 64)), 64, 64, &p);
    assert_eq!(non_opaque_texels(&out), 0);
}

/// Opaque white noise (xorshift64), alpha 255 everywhere.
fn opaque_noise(seed: u64, n: u32) -> Vec<[u8; 4]> {
    let mut s = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..n)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            [s as u8, (s >> 8) as u8, (s >> 16) as u8, 255]
        })
        .collect()
}

/// BC7 mode 4 or 5 with a non-zero rotation (alpha is carried by a colour endpoint pair).
fn rotated_mode45(b: &[u8]) -> bool {
    let v = u128::from_le_bytes(b.try_into().unwrap());
    (b[0] & 0x1f == 0x10 && (v >> 5) & 3 != 0) || (b[0] & 0x3f == 0x20 && (v >> 6) & 3 != 0)
}

#[test]
fn exact_opaque_covers_rotated_mode4_and_mode5_on_opaque_noise() {
    let mut p = profile(Subtype::ComplexMaterial);
    p.alpha.policy = AlphaPolicy::Remove;
    p.encoder.perceptual = false; // modes 4/5 are only tried for opaque blocks without perceptual
    p.encoder.effort = Effort::Veryslow;
    assert!(p.alpha.exact_opaque);
    let raw_params = encoders_sys::Bc7Params {
        level: bc7_level(p.encoder.effort),
        perceptual: false,
        exact_opaque: encoders_sys::BC7E_EXACT_OPAQUE_NATIVE,
    };
    let mut fixed_blocks = 0;
    for seed in 1..=6 {
        let px = opaque_noise(seed, 64 * 64);
        let out = encode(&EncoderInput::Ldr(px.clone()), 64, 64, &p);
        assert_eq!(non_opaque_texels(&out), 0, "seed {seed}");
        // The unfixed encode: the same bc7e call without any post-encode fix.
        let blocks = encoders_sys::gather_blocks(&px, 64, 64);
        let mut raw = vec![0u8; out.len()];
        for (o, b) in raw.chunks_mut(64 * 16).zip(blocks.chunks(64 * 16)) {
            encoders_sys::bc7e_compress_blocks(b, o, raw_params);
        }
        for (f, r) in out.chunks(16).zip(raw.chunks(16)) {
            // Mode-6 blocks differ only on a build without the native patch (byte-patch fallback).
            if f == r || r[0] & 0x7f == 0x40 {
                continue;
            }
            assert!(rotated_mode45(r), "only rotated mode 4/5 blocks may change");
            let (mut df, mut dr) = ([0u8; 64], [0u8; 64]);
            bcdec_rs::bc7(f, &mut df, 16);
            bcdec_rs::bc7(r, &mut dr, 16);
            for (tf, tr) in df.chunks(4).zip(dr.chunks(4)) {
                assert_eq!(tf[..3], tr[..3], "RGB must stay bit-identical");
            }
            assert!(dr.chunks(4).any(|t| t[3] != 255), "a fix must be needed");
            fixed_blocks += 1;
        }
    }
    assert!(fixed_blocks > 0, "no rotated mode 4/5 block needed a fix");
}

/// Raw bc7e output for a row-major image: the same calls as the encode stage, without any post-encode fix.
fn raw_bc7e(px: &[[u8; 4]], w: u32, h: u32, params: encoders_sys::Bc7Params) -> Vec<u8> {
    let blocks = encoders_sys::gather_blocks(px, w, h);
    let mut raw = vec![0u8; blocks.len()];
    for (o, b) in raw.chunks_mut(64 * 16).zip(blocks.chunks(64 * 16)) {
        encoders_sys::bc7e_compress_blocks(b, o, params);
    }
    raw
}

fn min_decoded_alpha(bc7: &[u8]) -> u8 {
    bc7.chunks(16)
        .map(|b| {
            let mut d = [0u8; 64];
            bcdec_rs::bc7(b, &mut d, 16);
            d.chunks(4).map(|t| t[3]).min().unwrap()
        })
        .min()
        .unwrap()
}

/// Checks `out` against the raw bc7e bytes of the same opaque input: every block that differs is a rotated
/// mode 4/5 block that decodes alpha 255 throughout and RGB bit-identical to the raw block. Returns the number
/// of changed blocks whose raw version decoded some alpha below 255 (the ones the fix was needed for).
fn only_rotated_blocks_fixed(out: &[u8], raw: &[u8], what: &str) -> usize {
    assert_eq!(out.len(), raw.len(), "{what}");
    let mut needed = 0;
    for (i, (f, r)) in out.chunks(16).zip(raw.chunks(16)).enumerate() {
        if f == r {
            continue;
        }
        assert!(
            rotated_mode45(r),
            "{what} block {i}: only rotated mode 4/5 blocks may change"
        );
        let (mut df, mut dr) = ([0u8; 64], [0u8; 64]);
        bcdec_rs::bc7(f, &mut df, 16);
        bcdec_rs::bc7(r, &mut dr, 16);
        for (tf, tr) in df.chunks(4).zip(dr.chunks(4)) {
            assert_eq!(
                tf[..3],
                tr[..3],
                "{what} block {i}: RGB must stay bit-identical"
            );
            assert_eq!(tf[3], 255, "{what} block {i}: alpha after the fix");
        }
        needed += dr.chunks(4).any(|t| t[3] != 255) as usize;
    }
    needed
}

/// With `exact_opaque` off, rotated mode 4/5 blocks of all-255 input are still fixed (the fix is lossless, so it is
/// unconditional; fix batch 41), whatever the alpha policy; every other block keeps bc7e's bytes.
#[test]
fn exact_opaque_off_still_fixes_rotated_blocks_and_nothing_else() {
    for policy in [AlphaPolicy::Remove, AlphaPolicy::Data] {
        let mut p = profile(Subtype::Diffuse);
        p.format = Format::Bc7;
        p.encoder.perceptual = false;
        p.alpha.exact_opaque = false;
        p.alpha.policy = policy;
        let px = opaque_noise(1, 64 * 64);
        let out = encode(&EncoderInput::Ldr(px.clone()), 64, 64, &p);
        let raw = raw_bc7e(
            &px,
            64,
            64,
            encoders_sys::Bc7Params {
                level: bc7_level(p.encoder.effort),
                perceptual: false,
                exact_opaque: false,
            },
        );
        assert!(
            min_decoded_alpha(&raw) < 254,
            "probe needs a broken rotated block"
        );
        let mut want = raw.clone();
        for b in want.chunks_mut(16) {
            encoders_sys::bc7_fixup::force_opaque_rotated(b.try_into().unwrap());
        }
        assert!(
            out == want,
            "{policy:?}: output != bc7e bytes with the rotated fix applied"
        );
        assert!(only_rotated_blocks_fixed(&out, &raw, &format!("{policy:?}")) > 0);
        assert_eq!(min_decoded_alpha(&out), 255, "{policy:?}");
    }
}

/// Task 41's CLI fixture (`cli/tests/cli_convert.rs`, `noise`): 64x64 RGB white noise from an LCG, as
/// `load_image` returns an 8-bit RGB PNG.
fn task41_noise() -> Rgba32F {
    let mut state = 12345u32;
    let mut next = move || {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        (state >> 24) as f32 / 255.0
    };
    let px = (0..64 * 64)
        .map(|_| {
            let (r, g, b) = (next(), next(), next());
            [r, g, b, 1.0]
        })
        .collect();
    Rgba32F {
        width: 64,
        height: 64,
        px,
    }
}

/// Task 41 / fix batch 41: a diffuse with the defaults (`remove`, `exact_opaque` off) decoded alpha 239 on white
/// noise. Through the pipeline's own stages every mip now decodes alpha >= 254 (spec §5.8), and the only
/// difference from the unfixed bc7e encode is the lossless rotated-block fix.
#[test]
fn remove_guarantee_holds_on_task41_noise_with_exact_opaque_off() {
    let p = profile(Subtype::Diffuse);
    assert_eq!(
        (p.format, p.alpha.policy, p.alpha.exact_opaque),
        (Format::Bc7, AlphaPolicy::Remove, false)
    );
    let work = preprocess(task41_noise(), &p);
    let chain = build_chain(&work, &p);
    assert_eq!(chain.len(), 7, "full chain 64 -> 1");
    let raw_params = encoders_sys::Bc7Params {
        level: bc7_level(p.encoder.effort),
        perceptual: p.encoder.perceptual,
        exact_opaque: false,
    };
    let (mut raw_broken, mut needed) = (0, 0);
    for (l, level) in chain.iter().enumerate() {
        let EncoderInput::Ldr(px) = to_encoder_input(level, &work, &p) else {
            panic!("LDR input expected");
        };
        assert!(
            px.iter().all(|t| t[3] == 255),
            "mip {l}: removed alpha is 255"
        );
        let (w, h) = (level.width, level.height);
        let out = encode(&EncoderInput::Ldr(px.clone()), w, h, &p);
        let min = min_decoded_alpha(&out);
        assert!(min >= 254, "mip {l}: decoded alpha {min} below 254");
        let raw = raw_bc7e(&px, w, h, raw_params);
        raw_broken += (min_decoded_alpha(&raw) < 254) as usize;
        needed += only_rotated_blocks_fixed(&out, &raw, &format!("mip {l}"));
    }
    assert!(
        raw_broken > 0,
        "the unfixed encode must break the guarantee here (Task 41 saw alpha 239)"
    );
    assert!(needed > 0);
}

#[test]
fn exact_opaque_leaves_transparent_blocks_alone() {
    // Blocks with any alpha < 255 must still decode their alpha (the flag is opaque-only). Every
    // block mixes alpha 255 and alpha 0 texels, so a wrongly forced block would show up as 255s.
    let mut p = profile(Subtype::ComplexMaterial);
    p.alpha.policy = AlphaPolicy::Data;
    p.encoder.perceptual = false; // also allow modes 4/5, which the rotated fix touches
    assert!(p.alpha.exact_opaque);
    let mut off = p.clone();
    off.alpha.exact_opaque = false; // the rotated fix runs with the flag off too
    let src: Vec<[u8; 4]> = (0..16 * 16u32)
        .map(|i| {
            [
                (i * 5) as u8,
                40,
                200,
                if (i + i / 16) % 2 == 0 { 255 } else { 0 },
            ]
        })
        .collect();
    for p in [&p, &off] {
        let out = encode(&EncoderInput::Ldr(src.clone()), 16, 16, p);
        for (bi, blk) in out.chunks(16).enumerate() {
            let mut d = [0u8; 64];
            bcdec_rs::bc7(blk, &mut d, 16);
            let (bx, by) = (bi as u32 % 4 * 4, bi as u32 / 4 * 4);
            for (ti, t) in d.chunks(4).enumerate() {
                let (x, y) = (bx + ti as u32 % 4, by + ti as u32 / 4);
                let want = src[(y * 16 + x) as usize][3] as i32;
                assert!(
                    (t[3] as i32 - want).abs() <= 8,
                    "exact_opaque {}: block {bi} texel {ti}: alpha {} for source {want}",
                    p.alpha.exact_opaque,
                    t[3]
                );
            }
        }
    }
}

/// Four flat quadrant colours on an 8x8 image: block 0 is top-left, 1 top-right, 2 bottom-left,
/// 3 bottom-right (raster order across a multi-row, multi-column level).
#[test]
fn blocks_are_in_raster_order() {
    let quads = [
        [255u8, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [255, 255, 255, 255],
    ];
    let px: Vec<[u8; 4]> = (0..64u32)
        .map(|i| quads[((i / 8 / 4) * 2 + i % 8 / 4) as usize])
        .collect();
    let input = EncoderInput::Ldr(px);
    let close = |a: &[u8], b: &[u8; 4]| a[..3].iter().zip(b).all(|(x, y)| x.abs_diff(*y) <= 8);
    let mut p = profile(Subtype::Diffuse);
    p.format = Format::Bc7;
    let bc7 = encode(&input, 8, 8, &p);
    p.format = Format::Bc1;
    let bc1 = encode(&input, 8, 8, &p);
    for (q, want) in quads.iter().enumerate() {
        let mut d = [0u8; 64];
        bcdec_rs::bc7(&bc7[q * 16..q * 16 + 16], &mut d, 16);
        assert!(d.chunks(4).all(|t| close(t, want)), "BC7 block {q}");
        let blk: &[u8; 8] = bc1[q * 8..q * 8 + 8].try_into().unwrap();
        let d = encoders_sys::bc1_decode_block(blk, encoders_sys::Bc1Model::Ideal);
        assert!(d.iter().all(|t| close(t, want)), "BC1 block {q}");
    }
}

#[test]
fn output_is_identical_for_any_thread_count() {
    let mut jobs: Vec<(Profile, EncoderInput)> = vec![];
    for fmt in [
        Format::Bc7,
        Format::Bc3,
        Format::Bc1,
        Format::Bc4,
        Format::Bc5,
    ] {
        let mut p = profile(Subtype::Diffuse);
        p.format = fmt;
        jobs.push((p, EncoderInput::Ldr(gradient(70, 37))));
    }
    let hdr: Vec<[u16; 4]> = (0..70 * 37u32)
        .map(|i| {
            [
                (i * 31 % 0x5000) as u16,
                (i * 17 % 0x4800) as u16,
                0x3C00,
                0x3C00,
            ]
        })
        .collect();
    jobs.push((profile(Subtype::PbrEmissive), EncoderInput::Hdr(hdr)));
    let one = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap();
    let many = rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap();
    for (p, input) in &jobs {
        let a = one.install(|| encode(input, 70, 37, p));
        let b = many.install(|| encode(input, 70, 37, p));
        let c = encode(input, 70, 37, p);
        assert_eq!(a, b, "{:?}: 1 vs 4 threads", p.format);
        assert_eq!(a, c, "{:?}: 1 thread vs default pool", p.format);
    }
}

#[test]
fn cancellation_stops_the_level() {
    let ctx = EncodeCtx {
        cancel: Some(Arc::new(AtomicBool::new(true))),
    };
    let ldr = EncoderInput::Ldr(gradient(16, 16));
    assert!(encode_level(&ldr, 16, 16, &profile(Subtype::Diffuse), &ctx).is_err());
    let hdr = EncoderInput::Hdr(vec![[0x3C00; 4]; 16]);
    assert_eq!(
        encode_level(&hdr, 4, 4, &profile(Subtype::PbrEmissive), &ctx),
        Err(Cancelled)
    );
    let idle = EncodeCtx {
        cancel: Some(Arc::new(AtomicBool::new(false))),
    };
    assert!(encode_level(&ldr, 16, 16, &profile(Subtype::Diffuse), &idle).is_ok());
}

#[test]
fn effort_and_radius_mappings() {
    assert_eq!(
        bc7_level(Effort::Veryslow),
        encoders_sys::Bc7Level::Veryslow
    );
    assert_eq!(
        bc6h_level(Effort::Slowest),
        encoders_sys::Bc6hLevel::Veryslow
    );
    assert_eq!(radius(SearchRadius::Default), 5);
    assert_eq!(radius(SearchRadius::Max), 10);
    assert_eq!(radius(SearchRadius::Fast), 4);
}

/// Note CP: random RGB over a smooth alpha ramp, the case alpha weighting is for.
fn colour_smooth_alpha(w: u32, h: u32) -> Vec<[u8; 4]> {
    let mut s = 0x2545_f491_u32;
    (0..w * h)
        .map(|i| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            let (x, y) = (i % w, i / w);
            let a = (40 + (x * 3 + y * 2) % 180) as u8;
            [s as u8, (s >> 8) as u8, (s >> 16) as u8, a]
        })
        .collect()
}

fn weighted(w: Option<Bc7Weights>) -> Profile {
    let mut p = profile(Subtype::Diffuse);
    p.alpha.policy = AlphaPolicy::Blend;
    p.alpha.exact_opaque = false;
    p.encoder.perceptual = false;
    p.encoder.bc7_weights = w;
    p
}

/// Every weighting but `linear` (from `Bc7Weights::ALL`, which a unit test ties to the enum).
fn weightings() -> impl Iterator<Item = Bc7Weights> {
    Bc7Weights::ALL
        .into_iter()
        .filter(|w| *w != Bc7Weights::Linear)
}

/// Note CP: unset is the plain bc7e call (Linear is byte-identical to before); every option changes the bytes; with
/// `perceptual` on, the weights are ignored.
#[test]
fn bc7_weighting_options_change_the_bytes_and_unset_is_the_plain_call() {
    let (w, h) = (64, 64);
    let px = colour_smooth_alpha(w, h);
    let input = EncoderInput::Ldr(px.clone());
    let p = weighted(None);
    let linear = encode(&input, w, h, &p);
    let raw = raw_bc7e(
        &px,
        w,
        h,
        encoders_sys::Bc7Params {
            level: bc7_level(p.encoder.effort),
            perceptual: false,
            exact_opaque: false,
        },
    );
    assert_eq!(linear, raw, "no block is opaque, so no fix-up runs");
    // Fix round 1: an explicit `linear` is the plain call too.
    assert_eq!(
        encode(&input, w, h, &weighted(Some(Bc7Weights::Linear))),
        linear
    );
    let mut n = 0;
    for o in weightings() {
        assert_ne!(encode(&input, w, h, &weighted(Some(o))), linear, "{o:?}");
        n += 1;
    }
    assert_eq!(n, Bc7Weights::ALL.len() - 1);
    let mut q = weighted(Some(Bc7Weights::Alpha8));
    q.encoder.perceptual = true;
    let mut r = weighted(None);
    r.encoder.perceptual = true;
    assert_eq!(encode(&input, w, h, &q), encode(&input, w, h, &r));
}

fn alpha_psnr(bc7: &[u8], px: &[[u8; 4]], w: u32, h: u32) -> f64 {
    let blocks = encoders_sys::gather_blocks(px, w, h);
    let mut se = 0f64;
    for (b, src) in bc7.chunks(16).zip(blocks.chunks(16)) {
        let mut d = [0u8; 64];
        bcdec_rs::bc7(b, &mut d, 16);
        for (t, s) in d.chunks(4).zip(src) {
            se += (f64::from(t[3]) - f64::from(s[3])).powi(2);
        }
    }
    let mse = se / blocks.len() as f64;
    10.0 * (255.0f64 * 255.0 / mse.max(1e-9)).log10()
}

/// Note CP: Alpha x8 raises alpha PSNR over Linear on a smooth alpha under noisy colour.
#[test]
fn bc7_alpha8_raises_alpha_psnr_on_smooth_alpha() {
    let (w, h) = (64, 64);
    let px = colour_smooth_alpha(w, h);
    let input = EncoderInput::Ldr(px.clone());
    let lin = alpha_psnr(&encode(&input, w, h, &weighted(None)), &px, w, h);
    let a8 = alpha_psnr(
        &encode(&input, w, h, &weighted(Some(Bc7Weights::Alpha8))),
        &px,
        w,
        h,
    );
    assert!(
        a8 > lin + 1.0,
        "alpha PSNR linear {lin:.2} dB, alpha8 {a8:.2} dB"
    );
}

/// Note CP: the field is not serialised when unset, so every existing profile hash holds.
#[test]
fn bc7_weights_change_the_profile_hash_only_when_set() {
    let p = weighted(None);
    let v = serde_json::to_value(&p).unwrap();
    assert!(v["encoder"].get("bc7_weights").is_none());
    let q = weighted(Some(Bc7Weights::Alpha8));
    assert_eq!(
        serde_json::to_value(&q).unwrap()["encoder"]["bc7_weights"],
        "alpha8"
    );
    let h = |p: &Profile| engine::manifest::profile_sha256(p).unwrap();
    assert_ne!(h(&p), h(&q));
    assert_eq!(h(&p), h(&weighted(None)));
}
