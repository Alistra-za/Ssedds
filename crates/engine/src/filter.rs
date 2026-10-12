//! Mip and resize filters (spec §5.5, DECISIONS S-mips-2).
use crate::config::{Category, MipEdge, MipFilter, Profile};
use crate::image::Rgba32F;
use rayon::prelude::*;

const NOT_SRGB_COLOUR: &str =
    "tent, Mitchell and Kaiser are for sRGB colour only; this texture uses box (spec §5.5)";
const HDR_NOT_COLOUR: &str =
    "HDR mips use box or tent, and tent only on colour or HDR maps without alpha weighting (spec §5.5)";
const ALPHA_WEIGHTED: &str =
    "alpha-tested and blended colour uses the alpha-weighted box (spec §5.5)";

/// Why `p.mips.filter` cannot be used for this profile (S-mips-2), or Ok. Box is always allowed. Tent, Mitchell
/// and Kaiser need sRGB colour without alpha weighting; an HDR format (`is_hdr()`) on a colour or HDR sub-type
/// without alpha weighting takes tent only.
pub fn filter_allowed(p: &Profile) -> Result<(), &'static str> {
    use crate::ops::{domain_for, Domain};
    let f = p.mips.filter;
    if f == MipFilter::Box {
        return Ok(());
    }
    let domain = domain_for(p);
    if domain == Domain::Hdr {
        if f != MipFilter::Tent {
            return Err("HDR mips use box or tent (spec §5.5)");
        }
        if !matches!(p.category, Category::Color | Category::Hdr) {
            return Err(HDR_NOT_COLOUR);
        }
    } else if p.category != Category::Color || domain != Domain::LinearLight {
        return Err(NOT_SRGB_COLOUR);
    }
    if p.alpha_weighted_mips() {
        return Err(ALPHA_WEIGHTED);
    }
    Ok(())
}

/// Kernel support in destination texels (RESULTS-2 B8, S-mips-4).
pub fn radius(f: MipFilter) -> f64 {
    match f {
        MipFilter::Box => 0.5,
        MipFilter::Tent => 1.0,
        MipFilter::Mitchell => 2.0,
        MipFilter::Kaiser => 3.0,
    }
}

/// Modified Bessel I0 as the 25-term power series `Σ_{k=0}^{24} ((z/2)^k / k!)²`.
fn bessel_i0(z: f64) -> f64 {
    let (mut term, mut sum) = (1.0f64, 1.0f64);
    for k in 1..25 {
        term *= z / 2.0 / k as f64;
        sum += term * term;
    }
    sum
}

fn sinc(t: f64) -> f64 {
    if t == 0.0 {
        return 1.0;
    }
    let a = std::f64::consts::PI * t;
    a.sin() / a
}

/// Kernel weight at `x` destination texels from the centre, as RESULTS-2 B8 measured them (S-mips-4): tent,
/// Mitchell B = C = 1/3, Kaiser width 3, alpha 4, stretch 1. Box is the unit box of half-width 0.5.
pub fn kernel(f: MipFilter, x: f64) -> f64 {
    let a = x.abs();
    match f {
        MipFilter::Box => f64::from(u8::from(a <= 0.5)),
        MipFilter::Tent => (1.0 - a).max(0.0),
        MipFilter::Mitchell => {
            const B: f64 = 1.0 / 3.0;
            const C: f64 = 1.0 / 3.0;
            if a < 1.0 {
                ((12.0 - 9.0 * B - 6.0 * C) * a * a * a
                    + (-18.0 + 12.0 * B + 6.0 * C) * a * a
                    + (6.0 - 2.0 * B))
                    / 6.0
            } else if a < 2.0 {
                ((-B - 6.0 * C) * a * a * a
                    + (6.0 * B + 30.0 * C) * a * a
                    + (-12.0 * B - 48.0 * C) * a
                    + (8.0 * B + 24.0 * C))
                    / 6.0
            } else {
                0.0
            }
        }
        MipFilter::Kaiser => {
            const W: f64 = 3.0;
            const ALPHA: f64 = 4.0;
            const STRETCH: f64 = 1.0;
            let t = x / W;
            if t.abs() < 1.0 {
                sinc(x * STRETCH) * bessel_i0(ALPHA * (1.0 - t * t).sqrt()) / bessel_i0(ALPHA)
            } else {
                0.0
            }
        }
    }
}

/// Normalised `(source index, weight)` taps for each of `m` destination texels resampled from `n` source texels
/// (RESULTS-2 B8 `weight_matrix`, generalised to any n -> m; S-mips-4). The kernel is stretched by `n/m`, so a
/// downscale widens it in source texels. Edges wrap or clamp; duplicate indices are merged and each row sums to 1.
/// Not used for Box: the box chain is the recursive pair/area path in [`crate::mips`].
pub fn axis_taps(n: u32, m: u32, f: MipFilter, edge: MipEdge) -> Vec<Vec<(u32, f64)>> {
    debug_assert!(n >= 1 && m >= 1, "axis_taps {n} -> {m}");
    debug_assert!(n >= m, "axis_taps downscales only: {n} -> {m}");
    if n == m {
        return (0..n).map(|i| vec![(i, 1.0)]).collect();
    }
    let s = f64::from(n) / f64::from(m);
    let reach = (radius(f) * s).ceil() + 1.0;
    let last = i64::from(n) - 1;
    (0..m)
        .map(|i| {
            let centre = (f64::from(i) + 0.5) * s;
            let mut row: Vec<(u32, f64)> = Vec::new();
            for j in (centre - reach).floor() as i64..=(centre + reach).ceil() as i64 {
                let w = kernel(f, ((j as f64 + 0.5) - centre) / s);
                if w == 0.0 {
                    continue;
                }
                let k = match edge {
                    MipEdge::Wrap => j.rem_euclid(i64::from(n)),
                    MipEdge::Clamp => j.clamp(0, last),
                };
                row.push((k as u32, w));
            }
            row.sort_unstable_by_key(|t| t.0);
            row.dedup_by(|cur, prev| {
                let same = cur.0 == prev.0;
                if same {
                    prev.1 += cur.1;
                }
                same
            });
            let sum: f64 = row.iter().map(|t| t.1).sum();
            if sum == 0.0 {
                let k = (u64::from(i) * u64::from(n) / u64::from(m)).min(last as u64);
                return vec![(k as u32, 1.0)];
            }
            for t in &mut row {
                t.1 /= sum;
            }
            row
        })
        .collect()
}

/// Separable polyphase resample of RGB to `(w, h)` (RESULTS-2 B8, S-mips-4): horizontal then vertical, each tap
/// row accumulated in f64. Alpha of the result is 0.0; there is no clamping here (the caller clamps overshoot).
pub fn resample_rgb(src: &Rgba32F, (w, h): (u32, u32), f: MipFilter, edge: MipEdge) -> Rgba32F {
    debug_assert!(src.width >= 1 && src.height >= 1 && w >= 1 && h >= 1);
    let tx = axis_taps(src.width, w, f, edge);
    let ty = axis_taps(src.height, h, f, edge);
    let (sw, dw) = (src.width as usize, w as usize);
    let mut mid = vec![[0f32; 3]; src.height as usize * dw];
    mid.par_chunks_mut(dw).enumerate().for_each(|(y, row)| {
        let line = &src.px[y * sw..(y + 1) * sw];
        for (o, taps) in row.iter_mut().zip(&tx) {
            let mut acc = [0f64; 3];
            for &(k, wt) in taps {
                for (a, v) in acc.iter_mut().zip(line[k as usize]) {
                    *a += wt * f64::from(v);
                    // Saturate: a huge finite source must not become Inf (then NaN in the vertical pass).
                }
            }
            *o = acc.map(|v| v.clamp(f64::from(f32::MIN), f64::from(f32::MAX)) as f32);
            // saturate: a huge finite source must not become Inf, then NaN
        }
    });
    let mut out = Rgba32F::new(w, h, [0.0; 4]);
    out.px
        .par_chunks_mut(dw)
        .zip(ty.par_iter())
        .for_each(|(row, taps)| {
            for (x, o) in row.iter_mut().enumerate() {
                let mut acc = [0f64; 3];
                for &(k, wt) in taps {
                    for (a, v) in acc.iter_mut().zip(mid[k as usize * dw + x]) {
                        *a += wt * f64::from(v);
                    }
                }
                *o = [acc[0] as f32, acc[1] as f32, acc[2] as f32, 0.0];
            }
        });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_values_at_known_points() {
        let close = |a: f64, b: f64| (a - b).abs() < 1e-12;
        assert!(
            close(kernel(MipFilter::Tent, 0.5), 0.5) && close(kernel(MipFilter::Tent, 1.0), 0.0)
        );
        assert!(close(kernel(MipFilter::Mitchell, 0.0), 8.0 / 9.0));
        assert!(close(kernel(MipFilter::Mitchell, 1.0), 1.0 / 18.0));
        assert!(close(kernel(MipFilter::Mitchell, 2.0), 0.0));
        assert!(close(kernel(MipFilter::Kaiser, 0.0), 1.0));
        assert!(
            kernel(MipFilter::Kaiser, 1.0).abs() < 1e-12
                && kernel(MipFilter::Kaiser, 2.0).abs() < 1e-12
        );
        assert_eq!(kernel(MipFilter::Kaiser, 3.0), 0.0);
        assert!(kernel(MipFilter::Kaiser, 1.5) < 0.0); // a negative lobe
    }

    #[test]
    fn kernel_literal_values_derived_by_hand() {
        // Mitchell B = C = 1/3 on 1 <= |x| < 2 is (-7/3 x³ + 12 x² - 20 x + 32/3) / 6; at 1.5 that is -5/144.
        assert!((kernel(MipFilter::Mitchell, 1.5) - (-0.034_722_222_222)).abs() < 1e-9);
        // Kaiser w3 α4: sinc(0.5) I0(4 √(1 - 1/36)) / I0(4) = (2/π)(10.769546 / 11.301922), by a Taylor expansion
        // of I0 about 4 (I0(4) = 11.3019219521, I1(4) = 9.7594651537).
        assert!((kernel(MipFilter::Kaiser, 0.5) - 0.606_631_826).abs() < 1e-8);
    }

    #[test]
    fn every_tap_row_sums_to_one() {
        for f in [MipFilter::Tent, MipFilter::Mitchell, MipFilter::Kaiser] {
            for edge in [MipEdge::Wrap, MipEdge::Clamp] {
                for (n, m) in [
                    (4u32, 2u32),
                    (4, 1),
                    (8, 1),
                    (16, 4),
                    (1000, 512),
                    (600, 512),
                    (5, 2),
                    (3, 1),
                ] {
                    for row in axis_taps(n, m, f, edge) {
                        let s: f64 = row.iter().map(|t| t.1).sum();
                        assert!((s - 1.0).abs() < 1e-12, "{f:?} {edge:?} {n}->{m}: {s}");
                        assert!(row.iter().all(|t| t.0 < n));
                    }
                }
            }
        }
    }

    #[test]
    fn tent_edges_wrap_and_clamp_differ_as_computed_by_hand() {
        // 4 -> 2, input [1, 0, 0, 0]. Taps of each output: 1/8, 3/8, 3/8, 1/8 at j-1..j+2 around centre 2i+1.
        let mut img = Rgba32F::new(4, 1, [0.0; 4]);
        img.px[0][0] = 1.0;
        let w = resample_rgb(&img, (2, 1), MipFilter::Tent, MipEdge::Wrap);
        let c = resample_rgb(&img, (2, 1), MipFilter::Tent, MipEdge::Clamp);
        let r = |i: &Rgba32F| [i.px[0][0], i.px[1][0]];
        assert!(
            (r(&w)[0] - 0.375).abs() < 1e-6 && (r(&w)[1] - 0.125).abs() < 1e-6,
            "{:?}",
            r(&w)
        );
        assert!(
            (r(&c)[0] - 0.5).abs() < 1e-6 && r(&c)[1].abs() < 1e-6,
            "{:?}",
            r(&c)
        );
    }

    #[test]
    fn a_huge_finite_value_stays_finite() {
        // 16 -> 8: output 4 sits at centre 9; its positive lobe (px 7..=10) at +MAX and the rest at -MAX make the
        // positive weights sum above 1, so an unsaturated f32 cast overflows.
        let mut img = Rgba32F::new(16, 1, [0.0; 4]);
        for (x, p) in img.px.iter_mut().enumerate() {
            let v = if (7..=10).contains(&x) {
                f32::MAX
            } else {
                -f32::MAX
            };
            *p = [v, v, v, 1.0];
        }
        for f in [MipFilter::Mitchell, MipFilter::Kaiser] {
            let o = resample_rgb(&img, (8, 1), f, MipEdge::Clamp);
            assert!(
                o.px.iter().all(|p| p[..3].iter().all(|v| v.is_finite())),
                "{f:?}"
            );
        }
    }

    #[test]
    fn a_constant_image_stays_constant() {
        let img = Rgba32F::new(12, 7, [0.25, 0.5, 0.75, 1.0]);
        for f in [MipFilter::Tent, MipFilter::Mitchell, MipFilter::Kaiser] {
            for edge in [MipEdge::Wrap, MipEdge::Clamp] {
                let o = resample_rgb(&img, (4, 2), f, edge);
                assert!(o.px.iter().all(|p| (p[0] - 0.25).abs() < 1e-6
                    && (p[1] - 0.5).abs() < 1e-6
                    && (p[2] - 0.75).abs() < 1e-6
                    && p[3] == 0.0));
            }
        }
    }

    /// b8.py `weight_matrix`, restated in f64: dense `n_in/factor × n_in`, wrap edges, rows normalised.
    fn weight_matrix(n_in: usize, factor: usize, f: MipFilter) -> Vec<Vec<f64>> {
        let reach = (radius(f) * factor as f64).ceil() as i64 + 1;
        (0..n_in / factor)
            .map(|j| {
                let centre = (j as f64 + 0.5) * factor as f64;
                let idx =
                    (centre - reach as f64).floor() as i64..=(centre + reach as f64).ceil() as i64;
                let w: Vec<(i64, f64)> = idx
                    .map(|i| (i, kernel(f, ((i as f64 + 0.5) - centre) / factor as f64)))
                    .collect();
                let s: f64 = w.iter().map(|t| t.1).sum();
                let mut row = vec![0.0; n_in];
                for (i, v) in w {
                    row[i.rem_euclid(n_in as i64) as usize] += v / s;
                }
                row
            })
            .collect()
    }

    /// b8.py `down`: `Wy · img · Wxᵀ` per RGB channel, in f64.
    fn reference(img: &Rgba32F, factor: usize, f: MipFilter) -> Vec<[f64; 3]> {
        let w = img.width as usize;
        let (wx, wy) = (
            weight_matrix(w, factor, f),
            weight_matrix(img.height as usize, factor, f),
        );
        let mut out = Vec::new();
        for ry in &wy {
            for rx in &wx {
                let mut acc = [0.0f64; 3];
                for (i, a) in ry.iter().enumerate() {
                    for (j, b) in rx.iter().enumerate() {
                        let p = img.px[i * w + j];
                        for (s, v) in acc.iter_mut().zip(p) {
                            *s += a * b * f64::from(v);
                        }
                    }
                }
                out.push(acc);
            }
        }
        out
    }

    #[test]
    fn resample_matches_an_f64_weight_matrix_reference() {
        let mut state: u64 = 0x5dc0de;
        let mut img = Rgba32F::new(16, 8, [0.0; 4]);
        for p in img.px.iter_mut() {
            for c in p.iter_mut() {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                *c = (state >> 40) as f32 / (1u64 << 24) as f32;
            }
        }
        for f in [MipFilter::Tent, MipFilter::Mitchell, MipFilter::Kaiser] {
            let o = resample_rgb(&img, (4, 2), f, MipEdge::Wrap);
            let r = reference(&img, 4, f);
            assert_eq!(o.px.len(), r.len());
            let max =
                o.px.iter()
                    .zip(&r)
                    .flat_map(|(a, b)| (0..3).map(move |c| (f64::from(a[c]) - b[c]).abs()))
                    .fold(0.0f64, f64::max);
            assert!(max < 1e-5, "{f:?}: max |sdc - reference| = {max}");
        }
    }

    #[test]
    fn identity_size_is_a_copy() {
        let mut img = Rgba32F::new(3, 2, [0.0; 4]);
        for (i, p) in img.px.iter_mut().enumerate() {
            p[0] = i as f32;
        }
        let o = resample_rgb(&img, (3, 2), MipFilter::Kaiser, MipEdge::Wrap);
        assert!(o.px.iter().zip(&img.px).all(|(a, b)| a[0] == b[0]));
    }
}
