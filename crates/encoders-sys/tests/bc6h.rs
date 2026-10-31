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

/// A deterministic 64x64 HDR image: a dim warm sky ramp with noise, plus a 3x3 sun (red up to 6464, blue exactly 0,
/// like the RGBE-derived venice_sunset) and a bright neutral highlight, so the peak-preserving path has work to do.
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
            let (dx, dy) = (x as i32 - 21, y as i32 - 41);
            if dx.abs() <= 1 && dy.abs() <= 1 {
                let k = 1u32 << (2 * (2 - dx.unsigned_abs().max(dy.unsigned_abs())));
                c = [404.0 * k as f32, 60.0 * k as f32, 0.0];
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
            "fae91f75a7ea2d8d84c305f349ed192236360bb9ec485d5e2abadd622509ea25",
        ),
        (
            Bc6hLevel::Fast,
            "4de953b4b70a7fa94dc5e9425b7e7976ff2c08c119c955515018a038996b2b91",
        ),
        (
            Bc6hLevel::Basic,
            "b6c60093b3d360756894b6c116b7c60a2d593ac8e1ae7ea7223970efb4b77ed0",
        ),
        (
            Bc6hLevel::Slow,
            "096fe5db8d9948d1b4246982621564764a5227e38b95eeec59446dc122dd561e",
        ),
        (
            Bc6hLevel::Veryslow,
            "7408d9c89f3c66cb4c91076eec4816bcc52ec8c0fa87af660c89cf9eefc08961",
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

#[test]
#[should_panic(expected = "multiple of 4")]
fn rejects_unaligned_width() {
    let px = vec![[0u16; 4]; 6 * 4];
    let mut out = [0u8; 32];
    bc6h_compress_rows(&px, 6, 4, &mut out, Bc6hLevel::Veryslow);
}
