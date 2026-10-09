//! Compress (spec §5.6): one block row per rayon task; bc7e gets at most 64 blocks per call.
//!
//! Output is a pure function of the input: every block is encoded independently from its own 16
//! texels, so the bytes do not depend on the rayon thread count or on band scheduling.
use crate::config::*;
use crate::quantize::EncoderInput;
use encoders_sys::bc7_fixup::{force_opaque_mode6, force_opaque_rotated};
use encoders_sys::*;
use rayon::prelude::*;
use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Blocks per bc7e call (what the bench used).
const BC7_BLOCKS_PER_CALL: usize = 64;
/// Texels per 4x4 block.
const BLOCK_TEXELS: usize = 16;
/// Bytes per BC7 block.
const BC7_BLOCK_BYTES: usize = 16;

#[derive(Debug, Clone, Default)]
pub struct EncodeCtx {
    /// When set to true (from any thread), the level in flight stops at the next band boundary.
    pub cancel: Option<Arc<AtomicBool>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("cancelled")]
pub struct Cancelled;

pub fn bc7_level(e: Effort) -> Bc7Level {
    match e {
        Effort::Ultrafast => Bc7Level::Ultrafast,
        Effort::Veryfast => Bc7Level::Veryfast,
        Effort::Fast => Bc7Level::Fast,
        Effort::Basic => Bc7Level::Basic,
        Effort::Veryslow => Bc7Level::Veryslow,
        Effort::Slowest => Bc7Level::Slowest,
    }
}

pub fn bc6h_level(e: Effort) -> Bc6hLevel {
    match e {
        Effort::Ultrafast | Effort::Veryfast => Bc6hLevel::Veryfast,
        Effort::Fast => Bc6hLevel::Fast,
        Effort::Basic => Bc6hLevel::Basic,
        Effort::Veryslow | Effort::Slowest => Bc6hLevel::Veryslow,
    }
}

pub fn radius(r: SearchRadius) -> u32 {
    match r {
        SearchRadius::Default => BC45_RADIUS_DEFAULT,
        SearchRadius::Max => BC45_RADIUS_MAX,
    }
}

/// Encode one band (one block row) of an LDR image. `b` holds 16 texels per block (see
/// `gather_blocks`), `o` the matching output bytes.
///
/// BC7 blocks whose input alpha is 255 throughout (spec §5.6, §5.8):
/// - Rotated mode 4/5 blocks always get the lossless rotated-slot fix, whatever `alpha.exact_opaque`
///   and the alpha policy (fix batch 41). Without it bc7e can decode such a block's alpha well below
///   254 (239 on white noise), which would break the `remove` guarantee.
/// - `alpha.exact_opaque` controls only mode 6, the part that costs RGB: `native` selects the bc7e
///   flag (else the mode-6 byte patch). With the flag off, mode 6 is the only source of alpha 254.
fn encode_band(b: &[[u8; 4]], o: &mut [u8], p: &Profile, native: bool) {
    let e = &p.encoder;
    match p.format {
        Format::Bc7 => {
            let params = Bc7Params {
                level: bc7_level(e.effort),
                perceptual: e.perceptual,
                exact_opaque: p.alpha.exact_opaque && native,
            };
            for (oc, bc) in o
                .chunks_mut(BC7_BLOCKS_PER_CALL * BC7_BLOCK_BYTES)
                .zip(b.chunks(BC7_BLOCKS_PER_CALL * BLOCK_TEXELS))
            {
                bc7e_compress_blocks(bc, oc, params);
            }
            for (blk, src) in o.chunks_mut(BC7_BLOCK_BYTES).zip(b.chunks(BLOCK_TEXELS)) {
                if src.iter().all(|t| t[3] == 255) {
                    let blk: &mut [u8; 16] = blk.try_into().unwrap();
                    if p.alpha.exact_opaque && !native {
                        force_opaque_mode6(blk);
                    }
                    // Rotated mode 4/5 carry alpha in a colour endpoint pair; neither the native flag
                    // nor the mode-6 patch covers them. Lossless for RGB, so it runs with the flag off.
                    force_opaque_rotated(blk);
                }
            }
        }
        f => {
            let bs = f.block_bytes();
            for (blk, src) in o.chunks_mut(bs).zip(b.chunks(BLOCK_TEXELS)) {
                let src: &[[u8; 4]; 16] = src.try_into().unwrap();
                match f {
                    // BC1 is opaque-only in v1 (DECISIONS Note F): alpha is ignored by rgbcx.
                    Format::Bc1 => blk.copy_from_slice(&bc1_encode_block(
                        src,
                        e.rgbcx_level,
                        true,
                        e.bc1_transparent_black,
                    )),
                    Format::Bc3 => blk.copy_from_slice(&bc3_encode_block(
                        src,
                        e.rgbcx_level,
                        radius(e.bc45_search_radius),
                    )),
                    // Every single-channel sub-type (envmask, height, msn_specular) keeps its map in R.
                    Format::Bc4 => {
                        blk.copy_from_slice(&bc4_encode_block(src, 0, radius(e.bc45_search_radius)))
                    }
                    Format::Bc5 => {
                        blk.copy_from_slice(&bc5_encode_block(src, radius(e.bc45_search_radius)))
                    }
                    Format::Bc7 | Format::Bc6hUf16 => unreachable!("handled elsewhere"),
                }
            }
        }
    }
}

/// Pad a row-major half image to a multiple of 4 in both directions by edge replication.
/// Borrows `px` when both dimensions are already multiples of 4.
fn pad_hdr(px: &[[u16; 4]], w: u32, h: u32) -> (Cow<'_, [[u16; 4]]>, u32, u32) {
    assert!(w > 0 && h > 0, "empty image");
    assert_eq!(
        px.len(),
        w as usize * h as usize,
        "pixel count does not match width*height"
    );
    let (pw, ph) = (w.div_ceil(4) * 4, h.div_ceil(4) * 4);
    if (pw, ph) == (w, h) {
        return (Cow::Borrowed(px), w, h);
    }
    let mut out = Vec::with_capacity(pw as usize * ph as usize);
    for y in 0..ph {
        for x in 0..pw {
            out.push(px[(y.min(h - 1) as usize) * w as usize + x.min(w - 1) as usize]);
        }
    }
    (Cow::Owned(out), pw, ph)
}

/// Compress one `w` x `h` level to `p.format`: blocks in raster order,
/// `ceil(w/4) * ceil(h/4) * block_bytes` bytes. Levels smaller than a block (2x2, 1x1, or a
/// non-multiple-of-4 edge) are padded to whole 4x4 blocks by edge replication (`gather_blocks`
/// for LDR, `pad_hdr` for BC6H). Cancellation is checked before each band and once more at the end.
///
/// Panics if `input` does not match `p.format` (HDR input exactly for BC6H) or `w * h` pixels.
/// Both are programmer-error invariants, not user-reachable: `quantize::to_encoder_input` yields
/// HDR input iff the format is BC6H, and the level's pixel count comes from the mip chain.
pub fn encode_level(
    input: &EncoderInput,
    w: u32,
    h: u32,
    p: &Profile,
    ctx: &EncodeCtx,
) -> Result<Vec<u8>, Cancelled> {
    let cancelled = || {
        ctx.cancel
            .as_ref()
            .is_some_and(|c| c.load(Ordering::Relaxed))
    };
    encode_level_with(input, w, h, p, &cancelled)
}

/// `encode_level` with the cancellation probe as a closure (called before each band and at the end).
fn encode_level_with(
    input: &EncoderInput,
    w: u32,
    h: u32,
    p: &Profile,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<Vec<u8>, Cancelled> {
    assert_eq!(
        matches!(input, EncoderInput::Hdr(_)),
        p.format == Format::Bc6hUf16,
        "encoder input kind does not match format {:?}",
        p.format
    );
    // Set when any band is skipped, so its zero-filled output can never be returned, whatever the
    // probe reads at the end (a shared flag may be cleared by another holder).
    let skipped = AtomicBool::new(false);
    let go = || {
        let stop = cancelled();
        if stop {
            skipped.store(true, Ordering::Relaxed);
        }
        !stop
    };
    let out = match input {
        EncoderInput::Ldr(px) => {
            let blocks = gather_blocks(px, w, h);
            let (bw, bs) = (w.div_ceil(4) as usize, p.format.block_bytes());
            let mut out = vec![0u8; blocks.len() / BLOCK_TEXELS * bs];
            out.par_chunks_mut(bw * bs)
                .zip(blocks.par_chunks(bw * BLOCK_TEXELS))
                .for_each(|(o, b)| {
                    if go() {
                        encode_band(b, o, p, BC7E_EXACT_OPAQUE_NATIVE);
                    }
                });
            out
        }
        EncoderInput::Hdr(px) => {
            let (padded, pw, ph) = pad_hdr(px, w, h);
            let level = bc6h_level(p.encoder.effort);
            let mut out = vec![0u8; (pw / 4 * (ph / 4) * 16) as usize];
            out.par_chunks_mut((pw / 4 * 16) as usize)
                .zip(padded.par_chunks((pw * 4) as usize))
                .for_each(|(o, rows)| {
                    if go() {
                        bc6h_compress_rows(rows, pw, 4, o, level);
                    }
                });
            out
        }
    };
    if skipped.load(Ordering::Relaxed) || cancelled() {
        Err(Cancelled)
    } else {
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets::PresetRegistry;

    fn bc7_exact_profile() -> Profile {
        let mut p = PresetRegistry::builtin()
            .unwrap()
            .profile("cs-quality", Target::Cs, Subtype::ComplexMaterial)
            .unwrap();
        p.format = Format::Bc7;
        p.alpha.exact_opaque = true;
        p
    }

    /// 64x64 tiling of the dark opaque block that upstream bc7e u5 encodes as mode 6 with a p-bit-0
    /// endpoint (alpha 254); see `encoders-sys/tests/bc7.rs`.
    fn dark_ramp(w: u32, h: u32) -> Vec<[u8; 4]> {
        (0..w * h)
            .map(|i| {
                let t = ((i / w % 4) * 4 + i % 4) as u8;
                [t, t / 2, 100 + t, 255]
            })
            .collect()
    }

    fn alpha_bad(block: &[u8]) -> usize {
        let mut d = [0u8; 64];
        bcdec_rs::bc7(block, &mut d, 16);
        d.chunks(4).filter(|t| t[3] != 255).count()
    }

    #[test]
    fn byte_patch_fallback_forces_opaque_alpha_on_mode6_blocks() {
        let p = bc7_exact_profile();
        let blocks = gather_blocks(&dark_ramp(16, 16), 16, 16);
        let mut plain = vec![0u8; blocks.len()];
        let mut fixed = vec![0u8; blocks.len()];
        let mut off = p.clone();
        off.alpha.exact_opaque = false;
        encode_band(&blocks, &mut plain, &off, false);
        encode_band(&blocks, &mut fixed, &p, false);
        assert!(
            plain.chunks(16).all(|b| b[0] & 0x7f == 0x40),
            "test image should be mode 6"
        );
        assert!(plain.chunks(16).map(alpha_bad).sum::<usize>() > 0);
        assert_eq!(fixed.chunks(16).map(alpha_bad).sum::<usize>(), 0);
    }

    /// A band skipped by cancellation must fail the level even if the probe reads false again
    /// afterwards (e.g. a shared flag reset by another holder).
    #[test]
    fn skipped_band_fails_even_if_cancel_flag_clears() {
        use std::sync::atomic::AtomicUsize;
        let px: Vec<[u8; 4]> = (0..16 * 16u32).map(|i| [i as u8, 7, 9, 255]).collect();
        let input = EncoderInput::Ldr(px);
        for fmt in [Format::Bc7, Format::Bc1] {
            let mut p = bc7_exact_profile();
            p.format = fmt;
            let calls = AtomicUsize::new(0);
            let first_only = || calls.fetch_add(1, Ordering::Relaxed) == 0;
            assert_eq!(
                encode_level_with(&input, 16, 16, &p, &first_only),
                Err(Cancelled),
                "{fmt:?}"
            );
        }
        let calls = AtomicUsize::new(0);
        let first_only = || calls.fetch_add(1, Ordering::Relaxed) == 0;
        let hdr = EncoderInput::Hdr(vec![[0x3C00; 4]; 8 * 8]);
        let mut p = bc7_exact_profile();
        p.format = Format::Bc6hUf16;
        assert_eq!(
            encode_level_with(&hdr, 8, 8, &p, &first_only),
            Err(Cancelled)
        );
    }

    #[test]
    fn pad_hdr_borrows_unless_padding_is_needed() {
        let px = vec![[0x3C00u16; 4]; 8 * 4];
        assert!(matches!(pad_hdr(&px, 8, 4), (Cow::Borrowed(_), 8, 4)));
        let px = vec![[0x3C00u16; 4]; 6 * 5];
        let (padded, pw, ph) = pad_hdr(&px, 6, 5);
        assert!(matches!(padded, Cow::Owned(_)));
        assert_eq!((padded.len(), pw, ph), (8 * 8, 8, 8));
    }

    #[test]
    fn byte_patch_skips_blocks_with_non_opaque_input() {
        let p = bc7_exact_profile();
        let mut px = dark_ramp(4, 4);
        px[5][3] = 10;
        let blocks = gather_blocks(&px, 4, 4);
        let mut off = p.clone();
        off.alpha.exact_opaque = false;
        let (mut plain, mut fixed) = ([0u8; 16], [0u8; 16]);
        encode_band(&blocks, &mut plain, &off, false);
        encode_band(&blocks, &mut fixed, &p, false);
        assert_eq!(plain, fixed);
    }
}
