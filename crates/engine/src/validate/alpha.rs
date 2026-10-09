//! Decoded-alpha statistics and the all-255-block check (spec §5.8).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Counts of decoded alpha values over one image.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AlphaStats {
    pub pixels: u64,
    pub eq255: u64,
    pub eq254: u64,
    pub below254: u64,
    pub min: u8,
}

impl AlphaStats {
    pub fn share_254(&self) -> f64 {
        if self.pixels == 0 {
            0.0
        } else {
            self.eq254 as f64 / self.pixels as f64
        }
    }

    pub fn share_not_255(&self) -> f64 {
        if self.pixels == 0 {
            0.0
        } else {
            (self.pixels - self.eq255) as f64 / self.pixels as f64
        }
    }
}

pub fn alpha_stats(decoded: &[[u8; 4]]) -> AlphaStats {
    let mut s = AlphaStats {
        min: 255,
        ..Default::default()
    };
    for p in decoded {
        s.pixels += 1;
        match p[3] {
            255 => s.eq255 += 1,
            254 => s.eq254 += 1,
            _ => s.below254 += 1,
        }
        s.min = s.min.min(p[3]);
    }
    s
}

/// Texels whose decoded alpha is not 255 inside 4x4 blocks whose encoder-input alpha is 255 on every
/// in-image texel (blocks at the right and bottom edge cover only the texels inside the image).
///
/// Never panics. If a buffer is shorter than `width * height`, a missing input texel is not known to be
/// opaque (its block is skipped) and a missing decoded texel inside an opaque block counts as a violation.
pub fn opaque_block_violations(
    input: &[[u8; 4]],
    decoded: &[[u8; 4]],
    width: u32,
    height: u32,
) -> u64 {
    let (w, h) = (width as usize, height as usize);
    let mut bad = 0u64;
    for by in (0..h).step_by(4) {
        for bx in (0..w).step_by(4) {
            let texels = || {
                (by..(by + 4).min(h))
                    .flat_map(move |y| (bx..(bx + 4).min(w)).map(move |x| y * w + x))
            };
            if texels().all(|i| input.get(i).is_some_and(|p| p[3] == 255)) {
                bad += texels()
                    .filter(|&i| decoded.get(i).is_none_or(|p| p[3] != 255))
                    .count() as u64;
            }
        }
    }
    bad
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_255_254_and_below() {
        let px = [
            [0, 0, 0, 255],
            [0, 0, 0, 254],
            [0, 0, 0, 10],
            [0, 0, 0, 255],
        ];
        let s = alpha_stats(&px);
        assert_eq!(
            (s.pixels, s.eq255, s.eq254, s.below254, s.min),
            (4, 2, 1, 1, 10)
        );
        assert!((s.share_254() - 0.25).abs() < 1e-12);
        assert!((s.share_not_255() - 0.5).abs() < 1e-12);
    }

    #[test]
    fn violations_only_count_blocks_whose_input_is_all_255() {
        // 8x4 image = two blocks. Block 0 input all 255, decoded has two 254s. Block 1 input has a 200.
        let mut input = vec![[0u8, 0, 0, 255]; 32];
        input[6] = [0, 0, 0, 200]; // row 0, x = 6 (block 1)
        let mut dec = input.clone();
        dec[0] = [0, 0, 0, 254];
        dec[9] = [0, 0, 0, 254]; // row 1, x = 1 (block 0)
        dec[7] = [0, 0, 0, 100]; // block 1: not counted
        assert_eq!(opaque_block_violations(&input, &dec, 8, 4), 2);
    }

    /// Opaque `w` x `h` input and a decoded copy with alpha 254 at every `(x, y)` in `hits`.
    fn opaque_with_hits(w: u32, h: u32, hits: &[(u32, u32)]) -> (Vec<[u8; 4]>, Vec<[u8; 4]>) {
        let input = vec![[1u8, 2, 3, 255]; (w * h) as usize];
        let mut dec = input.clone();
        for &(x, y) in hits {
            dec[(y * w + x) as usize][3] = 254;
        }
        (input, dec)
    }

    #[test]
    fn edge_blocks_cover_only_in_image_texels() {
        // 5x3: blocks at x 0..4 and x 4..5, both 3 rows tall. The right block has 3 in-image texels.
        let (input, dec) = opaque_with_hits(5, 3, &[(4, 0), (4, 2), (3, 1)]);
        assert_eq!(opaque_block_violations(&input, &dec, 5, 3), 3);
        // A non-255 input texel in the right-edge block exempts that block only.
        let (mut input, dec) = opaque_with_hits(5, 3, &[(4, 0), (3, 1)]);
        input[2 * 5 + 4][3] = 7;
        assert_eq!(opaque_block_violations(&input, &dec, 5, 3), 1);
    }

    #[test]
    fn partial_blocks_on_both_axes() {
        // 6x6: blocks (0,0) 4x4, (1,0) 2x4, (0,1) 4x2, (1,1) 2x2.
        let (input, dec) = opaque_with_hits(6, 6, &[(5, 5), (4, 4), (5, 0), (0, 5), (2, 2)]);
        assert_eq!(opaque_block_violations(&input, &dec, 6, 6), 5);
        // Only the bottom-right 2x2 block is non-opaque in the input: its two hits are exempt.
        let (mut input, dec) = opaque_with_hits(6, 6, &[(5, 5), (4, 4), (5, 0), (0, 5), (2, 2)]);
        input[4 * 6 + 5][3] = 0;
        assert_eq!(opaque_block_violations(&input, &dec, 6, 6), 3);
    }

    #[test]
    fn sub_block_images_and_short_buffers_do_not_panic() {
        let (input, dec) = opaque_with_hits(1, 1, &[(0, 0)]);
        assert_eq!(opaque_block_violations(&input, &dec, 1, 1), 1);
        assert_eq!(opaque_block_violations(&[], &[], 0, 0), 0);
        let (input, dec) = opaque_with_hits(5, 3, &[]);
        // Decoded buffer too short: the missing texels of opaque blocks count as violations.
        assert_eq!(opaque_block_violations(&input, &dec[..4], 5, 3), 11);
        // Input buffer too short: blocks with a missing input texel are skipped.
        assert_eq!(opaque_block_violations(&input[..4], &dec, 5, 3), 0);
        assert_eq!(opaque_block_violations(&[], &[], 8, 8), 0);
    }
}
