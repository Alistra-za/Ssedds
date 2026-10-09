//! Push-pull dilation (spec §5.4 step 4): colour under α = 0 is filled before any filtering [MIP].
//!
//! Texels whose alpha rounds to 8-bit code 0 carry no meaningful colour, but mip filtering and
//! bilinear sampling would otherwise blend that garbage into visible neighbours. They are given
//! colour pulled from a push-pull pyramid built only from visible texels. Alpha is never touched.
use crate::image::Rgba32F;

/// One coarser pyramid level; `rgb[i]` is meaningful only where `filled[i]`.
struct Level {
    w: usize,
    h: usize,
    rgb: Vec<[f32; 3]>,
    filled: Vec<bool>,
}

/// A texel is visible when its alpha rounds to 8-bit code 1 or more. NaN and negative alpha are invisible.
fn visible(a: f32) -> bool {
    (a * 255.0).round() >= 1.0
}

/// Mean RGB of the filled texels among the (up to) 2x2 children of coarse texel `(x, y)`.
/// Children beyond the edge of an odd-sized parent are skipped, so every parent texel is
/// covered exactly once and no source texel is dropped.
fn push_texel(
    x: usize,
    y: usize,
    pw: usize,
    ph: usize,
    child: impl Fn(usize) -> Option<[f32; 3]>,
) -> Option<[f32; 3]> {
    let (mut sum, mut n) = ([0.0f32; 3], 0.0f32);
    for sy in (y * 2)..(y * 2 + 2).min(ph) {
        for sx in (x * 2)..(x * 2 + 2).min(pw) {
            if let Some(c) = child(sy * pw + sx) {
                for (s, v) in sum.iter_mut().zip(c) {
                    *s += v;
                }
                n += 1.0;
            }
        }
    }
    (n > 0.0).then(|| sum.map(|v| v / n))
}

/// Replace the RGB of every texel whose alpha rounds to code 0 with colour from the nearest
/// coarser level of a push-pull pyramid built from visible texels. Alpha is never changed.
///
/// An image with no visible texel (including an empty one) is left unchanged: there is no
/// colour to propagate, and inventing one would be a guess. Any size is accepted, including
/// non-power-of-two and 1x1.
///
/// Memory: level 0 is read and written in place in `img`; only the coarser levels (about a
/// third of the image, 13 bytes per texel) are allocated.
pub fn dilate_under_zero_alpha(img: &mut Rgba32F) {
    if !img.px.iter().any(|p| visible(p[3])) {
        return;
    }
    let (w0, h0) = (img.width as usize, img.height as usize);

    // push: mean of the visible children, down to 1x1
    let mut levels: Vec<Level> = Vec::new();
    let (mut pw, mut ph) = (w0, h0);
    while pw > 1 || ph > 1 {
        let (w, h) = (pw.div_ceil(2), ph.div_ceil(2));
        let mut rgb = vec![[0.0f32; 3]; w * h];
        let mut filled = vec![false; w * h];
        for y in 0..h {
            for x in 0..w {
                let c = match levels.last() {
                    None => push_texel(x, y, pw, ph, |i| {
                        let p = img.px[i];
                        visible(p[3]).then_some([p[0], p[1], p[2]])
                    }),
                    Some(prev) => {
                        push_texel(x, y, pw, ph, |i| prev.filled[i].then_some(prev.rgb[i]))
                    }
                };
                if let Some(c) = c {
                    rgb[y * w + x] = c;
                    filled[y * w + x] = true;
                }
            }
        }
        levels.push(Level { w, h, rgb, filled });
        (pw, ph) = (w, h);
    }

    // pull: fill empty texels from the parent, coarse to fine
    // (the 1x1 top is filled because at least one texel is visible)
    for l in (0..levels.len().saturating_sub(1)).rev() {
        let (lo, hi) = levels.split_at_mut(l + 1);
        let (cur, parent) = (&mut lo[l], &hi[0]);
        for y in 0..cur.h {
            for x in 0..cur.w {
                let i = y * cur.w + x;
                if !cur.filled[i] {
                    cur.rgb[i] = parent.rgb[(y / 2) * parent.w + x / 2];
                    cur.filled[i] = true;
                }
            }
        }
    }
    // A 1x1 image has no pyramid: its single texel is visible here, so there is nothing to fill.
    if let Some(first) = levels.first() {
        for y in 0..h0 {
            for x in 0..w0 {
                let p = &mut img.px[y * w0 + x];
                if !visible(p[3]) {
                    let c = first.rgb[(y / 2) * first.w + x / 2];
                    p[..3].copy_from_slice(&c);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_invisible_rgb_from_visible_neighbours() {
        let mut img = Rgba32F::new(4, 4, [0.0, 1.0, 0.0, 0.0]); // green garbage, invisible
        for y in 0..4 {
            for x in 0..2 {
                *img.at_mut(x, y) = [1.0, 0.0, 0.0, 1.0]; // left half visible red
            }
        }
        dilate_under_zero_alpha(&mut img);
        for p in &img.px {
            assert!(p[0] > 0.99 && p[1] < 0.01, "{p:?}");
        }
        assert_eq!(img.at(3, 3)[3], 0.0);
    }

    #[test]
    fn all_invisible_is_unchanged() {
        let mut img = Rgba32F::new(2, 2, [0.2, 0.3, 0.4, 0.0]);
        let before = img.clone();
        dilate_under_zero_alpha(&mut img);
        assert_eq!(img, before);
    }

    #[test]
    fn all_invisible_non_square_and_empty_are_unchanged() {
        let mut img = Rgba32F::new(5, 3, [0.9, 0.1, 0.4, 0.0]);
        let before = img.clone();
        dilate_under_zero_alpha(&mut img);
        assert_eq!(img, before);
        let mut empty = Rgba32F::new(0, 0, [0.0; 4]);
        dilate_under_zero_alpha(&mut empty);
        assert!(empty.px.is_empty());
    }

    #[test]
    fn one_by_one_is_unchanged() {
        for px in [[0.3, 0.6, 0.9, 0.0], [0.3, 0.6, 0.9, 1.0]] {
            let mut img = Rgba32F::new(1, 1, px);
            dilate_under_zero_alpha(&mut img);
            assert_eq!(img.px, vec![px]);
        }
    }

    #[test]
    fn single_visible_texel_floods_whole_image_at_any_size() {
        for (w, h) in [(1, 7), (7, 1), (3, 3), (5, 3), (9, 6), (17, 5)] {
            for (vx, vy) in [(0, 0), (w - 1, h - 1), (w / 2, h / 2)] {
                let mut img = Rgba32F::new(w, h, [0.0, 1.0, 0.0, 0.0]);
                *img.at_mut(vx, vy) = [0.8, 0.2, 0.4, 1.0];
                dilate_under_zero_alpha(&mut img);
                for (i, p) in img.px.iter().enumerate() {
                    assert_eq!(
                        p[..3],
                        [0.8, 0.2, 0.4],
                        "{w}x{h} visible at ({vx},{vy}) texel {i}"
                    );
                    let want_a = if i == (vy * w + vx) as usize {
                        1.0
                    } else {
                        0.0
                    };
                    assert_eq!(p[3], want_a);
                }
            }
        }
    }

    #[test]
    fn odd_edge_column_is_not_dropped() {
        // Only the last (odd, unpaired) column is visible; it must still colour the rest.
        let mut img = Rgba32F::new(3, 2, [0.0; 4]);
        for y in 0..2 {
            *img.at_mut(2, y) = [0.1, 0.2, 0.9, 0.5];
        }
        dilate_under_zero_alpha(&mut img);
        for p in &img.px {
            assert_eq!(p[..3], [0.1, 0.2, 0.9]);
        }
    }

    #[test]
    fn alpha_and_visible_rgb_are_never_changed() {
        let (w, h) = (7u32, 5u32);
        let mut img = Rgba32F::new(w, h, [0.0; 4]);
        for y in 0..h {
            for x in 0..w {
                let a = if (x + 2 * y) % 3 == 0 {
                    0.0
                } else {
                    0.25 + 0.1 * ((x + y) % 5) as f32
                };
                *img.at_mut(x, y) = [x as f32 / 7.0, y as f32 / 5.0, 0.5, a];
            }
        }
        let before = img.clone();
        dilate_under_zero_alpha(&mut img);
        for (a, b) in img.px.iter().zip(&before.px) {
            assert_eq!(a[3], b[3]);
            if b[3] > 0.0 {
                assert_eq!(a, b);
            } else {
                assert_ne!(a[..3], b[..3]);
                assert!(a[..3].iter().all(|v| (0.0..=1.0).contains(v)));
            }
        }
    }

    #[test]
    fn alpha_threshold_is_the_8bit_code() {
        // 0.001 * 255 rounds to code 0 -> invisible; 1/255 is code 1 -> visible and untouched.
        let mut img = Rgba32F::new(3, 1, [0.0; 4]);
        *img.at_mut(0, 0) = [1.0, 0.0, 0.0, 1.0];
        *img.at_mut(1, 0) = [0.0, 1.0, 0.0, 0.001];
        *img.at_mut(2, 0) = [0.0, 0.0, 1.0, 1.0 / 255.0];
        dilate_under_zero_alpha(&mut img);
        assert_eq!(img.at(1, 0)[3], 0.001);
        assert_eq!(img.at(2, 0), [0.0, 0.0, 1.0, 1.0 / 255.0]);
        // the invisible middle texel gets the colour of its coarse block, not its own green
        assert_ne!(img.at(1, 0)[..3], [0.0, 1.0, 0.0]);
    }

    #[test]
    fn nearest_source_wins_on_a_long_strip() {
        // red far left, blue far right, gap between: each end stays closer to its own colour.
        let mut img = Rgba32F::new(16, 1, [0.0; 4]);
        *img.at_mut(0, 0) = [1.0, 0.0, 0.0, 1.0];
        *img.at_mut(15, 0) = [0.0, 0.0, 1.0, 1.0];
        dilate_under_zero_alpha(&mut img);
        assert!(img.at(1, 0)[0] > img.at(1, 0)[2]);
        assert!(img.at(14, 0)[2] > img.at(14, 0)[0]);
    }

    #[test]
    fn nan_alpha_is_treated_as_invisible() {
        let mut img = Rgba32F::new(2, 1, [0.0; 4]);
        *img.at_mut(0, 0) = [0.5, 0.5, 0.5, 1.0];
        *img.at_mut(1, 0) = [9.0, 9.0, 9.0, f32::NAN];
        dilate_under_zero_alpha(&mut img);
        assert_eq!(img.at(1, 0)[..3], [0.5, 0.5, 0.5]);
        assert!(img.at(1, 0)[3].is_nan());
    }
}
