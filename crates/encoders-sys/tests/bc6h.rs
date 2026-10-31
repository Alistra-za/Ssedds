use encoders_sys::{bc6h_compress_rows, Bc6hLevel};
use half::f16;
use sha2::{Digest, Sha256};

#[test]
fn constant_hdr_block_round_trips_within_two_percent() {
    let c = [1.5f32, 0.25, 0.0];
    let px = vec![
        [
            f16::from_f32(c[0]).to_bits(),
            f16::from_f32(c[1]).to_bits(),
            f16::from_f32(c[2]).to_bits(),
            f16::ONE.to_bits()
        ];
        16
    ];
    let mut out = [0u8; 16];
    bc6h_compress_rows(&px, 4, 4, &mut out, Bc6hLevel::Veryslow);
    let mut dec = [0f32; 48];
    bcdec_rs::bc6h_float(&out, &mut dec, 12, false);
    for t in 0..16 {
        for ch in 0..3 {
            let d = dec[t * 3 + ch];
            assert!(
                (d - c[ch]).abs() <= 0.02 * c[ch].max(0.05),
                "texel {t} ch {ch}: {d}"
            );
        }
    }
}

/// The 4x4 block that holds venice_sunset's sun (bench corpus, CC0): blue is exactly 0 wherever the sun is, as an
/// RGBE source stores it, and veryslow decodes the 6464 peak as 1464.
const SUN: [[f32; 3]; 16] = [
    [12.44, 3.19, 0.69],
    [16.0, 3.62, 0.5],
    [17.75, 3.75, 0.38],
    [14.06, 3.38, 0.56],
    [17.12, 3.5, 0.38],
    [308.0, 40.0, 0.0],
    [968.0, 144.0, 0.0],
    [29.0, 5.25, 0.12],
    [20.38, 3.88, 0.25],
    [3280.0, 480.0, 0.0],
    [6464.0, 960.0, 0.0],
    [186.0, 27.0, 0.0],
    [15.19, 3.0, 0.38],
    [258.0, 40.0, 0.0],
    [732.0, 112.0, 0.0],
    [29.25, 4.88, 0.12],
];

/// A deterministic 64x64 HDR image: a dim warm sky ramp with noise, venice_sunset's sun block and a bright neutral
/// highlight, so the peak-preserving path has work to do.
fn hdr_image() -> Vec<[u16; 4]> {
    let mut s = 0x2545_f491u32;
    let mut px = Vec::with_capacity(64 * 64);
    for y in 0..64u32 {
        for x in 0..64u32 {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let n = 1.0 + ((s >> 8) & 0xff) as f32 / 2048.0;
            let mut c = [
                (0.2 + x as f32 * 0.3) * n,
                (0.1 + y as f32 * 0.05) * n,
                0.02 + (63 - y) as f32 * 0.01,
            ];
            if (20..24).contains(&x) && (40..44).contains(&y) {
                c = SUN[((y - 40) * 4 + x - 20) as usize];
            }
            if (x, y) == (50, 10) {
                c = [900.0, 850.0, 800.0];
            }
            px.push([
                f16::from_f32(c[0]).to_bits(),
                f16::from_f32(c[1]).to_bits(),
                f16::from_f32(c[2]).to_bits(),
                f16::ONE.to_bits(),
            ]);
        }
    }
    px
}

/// The five shipped levels encode byte-identically to the objects vendored before the peak-preserving patch.
/// ISPC dispatches per CPU and its target paths differ, so the hashes are those of the AVX2 path (recorded on
/// Windows, ISPC 1.31.0, 2026-10-31); a CPU without AVX2 skips. `SDC_RECORD_LEVEL_HASHES=1` prints them.
#[test]
fn shipped_levels_are_byte_identical_to_the_recorded_objects() {
    const RECORDED: [(Bc6hLevel, &str); 5] = [
        (
            Bc6hLevel::Veryfast,
            "f803b49eba0cad2880b811f039f65567133a4b7e8e281d0df42541522afb57a5",
        ),
        (
            Bc6hLevel::Fast,
            "fc9954432343d094bca7a2e7bddad47486fc3c00a14c5335a1bfe4810b14fcb1",
        ),
        (
            Bc6hLevel::Basic,
            "5b5dd790d10d4ebe36482cd6a5e4f956fe7ec9429cc9e58b8fb4405a61b030f8",
        ),
        (
            Bc6hLevel::Slow,
            "49804dcc003a5cb1a8eab90dcbbd1151e0f7ebfd68c070181fe17cb7226b1e6f",
        ),
        (
            Bc6hLevel::Veryslow,
            "cc6fbdc096496cd67c9b728eb3809f6899c6e73ea634498925afbf3d2b85ecb6",
        ),
    ];
    if !std::is_x86_feature_detected!("avx2") {
        eprintln!("skipped: the hashes are for ISPC's AVX2 path");
        return;
    }
    let px = hdr_image();
    let record = std::env::var_os("SDC_RECORD_LEVEL_HASHES").is_some();
    for (level, want) in RECORDED {
        let mut out = vec![0u8; 16 * 16 * 16];
        bc6h_compress_rows(&px, 64, 64, &mut out, level);
        let got = hex::encode(Sha256::digest(&out));
        if record {
            println!("{level:?} {got}");
        } else {
            assert_eq!(got, want, "{level:?}");
        }
    }
}

/// Largest decoded luminance over the image divided by the source's.
fn peak_ratio(px: &[[u16; 4]], level: Bc6hLevel) -> f32 {
    let mut out = vec![0u8; 16 * 16 * 16];
    bc6h_compress_rows(px, 64, 64, &mut out, level);
    let lum = |c: &[f32]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    let src = px
        .iter()
        .map(|p| {
            lum(&p[..3]
                .iter()
                .map(|&h| f16::from_bits(h).to_f32())
                .collect::<Vec<_>>())
        })
        .fold(0f32, f32::max);
    let mut dec = 0f32;
    for b in out.chunks(16) {
        let mut d = [0f32; 48];
        bcdec_rs::bc6h_float(b, &mut d, 12, false);
        dec = d.chunks(3).map(lum).fold(dec, f32::max);
    }
    dec / src
}

#[test]
fn peak_mode_keeps_a_blue_zero_sun() {
    let px = hdr_image();
    let r = peak_ratio(&px, Bc6hLevel::VeryslowPeak);
    eprintln!(
        "peak ratio {r}, veryslow {}",
        peak_ratio(&px, Bc6hLevel::Veryslow)
    );
    assert!(
        r >= 0.9,
        "peak ratio {r} (veryslow: {})",
        peak_ratio(&px, Bc6hLevel::Veryslow)
    );
}

#[test]
#[should_panic(expected = "multiple of 4")]
fn rejects_unaligned_width() {
    let px = vec![[0u16; 4]; 6 * 4];
    let mut out = [0u8; 32];
    bc6h_compress_rows(&px, 6, 4, &mut out, Bc6hLevel::Veryslow);
}
