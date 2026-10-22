//! GTAO-style ambient occlusion marched over the solved height (Task 8, Note CO).
use super::{AoOpts, Slopes};
use rayon::prelude::*;
use std::f64::consts::{FRAC_PI_2, PI};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// tan(4°): horizons this little above the tangent plane are noise on flats and are ignored.
const BIAS: f64 = 0.0699;

/// The height being marched, with its tiling (wrap: indices mod the size; mirror: clamped to the edge).
struct Field<'a> {
    h: &'a [f64],
    w: usize,
    hgt: usize,
    wrap: bool,
}

impl Field<'_> {
    fn bilinear(&self, x: f64, y: f64) -> f64 {
        let axis = |v: f64, n: usize| {
            let i = v.floor();
            let t = v - i;
            let i = i as i64;
            let at = |j: i64| {
                if self.wrap {
                    j.rem_euclid(n as i64) as usize
                } else {
                    j.clamp(0, n as i64 - 1) as usize
                }
            };
            (at(i), at(i + 1), t)
        };
        let (x0, x1, tx) = axis(x, self.w);
        let (y0, y1, ty) = axis(y, self.hgt);
        let row = |y: usize| {
            let (a, b) = (self.h[y * self.w + x0], self.h[y * self.w + x1]);
            a + (b - a) * tx
        };
        let (top, bot) = (row(y0), row(y1));
        top + (bot - top) * ty
    }
}

/// Slice directions `(cos φ, sin φ)`, `φ = (k + ½)·π/n`.
fn directions(n: usize) -> Vec<(f64, f64)> {
    (0..n)
        .map(|k| {
            let phi = (k as f64 + 0.5) * PI / n as f64;
            (phi.cos(), phi.sin())
        })
        .collect()
}

/// Linear falloff over the outer 61.5 % of the radius (XeGTAO's range).
fn falloff(r: f64, big_r: f64) -> f64 {
    ((big_r - r) / (0.615 * big_r)).clamp(0.0, 1.0)
}

/// Cosine-weighted visibility at `(x, y)`, clamped to [0, 1]: the GTAO arc integral per slice, horizons searched
/// at `radii` (`(r, falloff)`) on both sides.
fn texel(
    f: &Field,
    s: &Slopes,
    x: usize,
    y: usize,
    dirs: &[(f64, f64)],
    radii: &[(f64, f64)],
    bias: f64,
) -> f64 {
    let i = y * f.w + x;
    let (p, q) = (s.p[i], s.q[i]);
    let l = (p * p + q * q + 1.0).sqrt();
    let (nx, ny, nz) = (-p / l, -q / l, 1.0 / l);
    let h0 = f.h[i];
    let (px, py) = (x as f64, y as f64);
    let mut vis = 0.0;
    for &(dx, dy) in dirs {
        let nd = nx * dx + ny * dy;
        let proj = nd.hypot(nz);
        let gamma = nd.atan2(nz);
        let (cg, sg) = (gamma.cos(), gamma.sin());
        let mut arc = 0.0;
        for side in [1.0, -1.0] {
            let tan_t = -side * nd / nz;
            let mut best = tan_t;
            for &(r, w) in radii {
                let e = (f.bilinear(px + side * r * dx, py + side * r * dy) - h0) / r;
                let c = tan_t + (e - tan_t) * w;
                if c > tan_t + bias {
                    best = best.max(c);
                }
            }
            // Horizon angle from zenith, signed toward side·d; `best` starts at the tangent, so no GTAO clamp.
            let theta = side * (FRAC_PI_2 - best.atan());
            arc += 0.25 * (-(2.0 * theta - gamma).cos() + cg + 2.0 * theta * sg);
        }
        vis += proj * arc;
    }
    (vis / dirs.len() as f64).clamp(0.0, 1.0)
}

/// AO in [0, 1] (1 = open) of height `h` (texel units) with its de-tilted slopes `s`; `None` when cancelled.
#[cfg(test)] // `generate` calls [`march_with_progress`]
pub(crate) fn march(
    h: &[f64],
    s: &Slopes,
    wrap: bool,
    o: &AoOpts,
    cancel: &AtomicBool,
) -> Option<Vec<f32>> {
    run(h, s, wrap, o, None, cancel, BIAS)
}

/// `generate`'s per-mille progress span the march fills, per finished row: from `.0` to `.0 + .1`.
pub(crate) const AO_PROGRESS: (u32, u32) = (870, 130);

/// [`march`], storing [`AO_PROGRESS`] into `progress` as rows finish.
pub(crate) fn march_with_progress(
    h: &[f64],
    s: &Slopes,
    wrap: bool,
    o: &AoOpts,
    progress: &AtomicU32,
    cancel: &AtomicBool,
) -> Option<Vec<f32>> {
    run(h, s, wrap, o, Some(progress), cancel, BIAS)
}

/// [`march`] with its angle bias as a parameter (the test seam).
#[cfg(test)]
pub(crate) fn march_with_bias(
    h: &[f64],
    s: &Slopes,
    wrap: bool,
    o: &AoOpts,
    cancel: &AtomicBool,
    bias: f64,
) -> Option<Vec<f32>> {
    run(h, s, wrap, o, None, cancel, bias)
}

fn run(
    h: &[f64],
    s: &Slopes,
    wrap: bool,
    o: &AoOpts,
    progress: Option<&AtomicU32>,
    cancel: &AtomicBool,
    bias: f64,
) -> Option<Vec<f32>> {
    let (w, hgt) = (s.width as usize, s.height as usize);
    let big_r = f64::from(o.radius) * w as f64;
    if big_r < 0.5 {
        return Some(vec![1.0; w * hgt]);
    }
    let ns = (o.steps.max(1) as usize).min(big_r.ceil() as usize);
    let radii: Vec<(f64, f64)> = (1..=ns)
        .map(|i| {
            let r = big_r * (i as f64 / ns as f64).powi(2);
            (r, falloff(r, big_r))
        })
        .collect();
    let dirs = directions(o.slices.max(1) as usize);
    let f = Field { h, w, hgt, wrap };
    let power = f64::from(o.intensity);
    let done = AtomicU32::new(0);
    let rows: Option<Vec<Vec<f32>>> = (0..hgt)
        .into_par_iter()
        .map(|y| {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            let row = (0..w)
                .map(|x| texel(&f, s, x, y, &dirs, &radii, bias).powf(power) as f32)
                .collect();
            if let Some(p) = progress {
                let d = u64::from(done.fetch_add(1, Ordering::Relaxed) + 1);
                let pm = u64::from(AO_PROGRESS.1) * d / hgt as u64;
                p.fetch_max(AO_PROGRESS.0 + pm as u32, Ordering::Relaxed);
            }
            Some(row)
        })
        .collect();
    Some(rows?.concat())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GreenConvention::{self, Directx, Opengl};
    use crate::from_normal::solve::solve;
    use crate::from_normal::solve::tests::{blobs, bump, constant, gauss, torus};
    use crate::from_normal::{generate, slopes, AoOpts, Gray, Opts, Tiling};
    use crate::image::Rgba32F;
    use std::sync::atomic::{AtomicBool, AtomicU32};

    fn no() -> AtomicBool {
        AtomicBool::new(false)
    }

    fn ao_opts(radius: f32, slices: u32) -> AoOpts {
        AoOpts {
            radius,
            slices,
            ..AoOpts::default()
        }
    }

    fn gen_ao(n: &Rgba32F, tiling: Tiling, green: GreenConvention, ao: AoOpts) -> Gray {
        gen_ao_at(n, n.width.max(n.height), tiling, green, ao)
    }

    fn gen_ao_at(
        n: &Rgba32F,
        size: u32,
        tiling: Tiling,
        green: GreenConvention,
        ao: AoOpts,
    ) -> Gray {
        let o = Opts {
            size,
            tiling,
            relief: 0.3,
            green: Some(green),
            height: None,
            ao: Some(ao),
            curvature: false,
        };
        let g = generate(n, &o, &AtomicU32::new(0), &no()).expect("generates");
        let ao = g.ao.expect("an AO");
        let mean = ao.px.iter().map(|&v| f64::from(v)).sum::<f64>() / ao.px.len() as f64;
        assert!((f64::from(g.report.ao_mean.unwrap()) - mean).abs() < 1e-4);
        let dark = ao.px.iter().filter(|&&v| v < 0.5).count() as f32 / ao.px.len() as f32;
        assert_eq!(g.report.ao_dark_share, Some(dark));
        ao
    }

    #[test]
    fn the_dark_share_counts_texels_below_one_half() {
        // A deep narrow pit: its floor is below 0.5, so the share is not 0 and a wrong comparison shows.
        let n = 128;
        let hf: Vec<f64> = (0..n * n)
            .map(|i| -9.0 * gauss((i % n) as f64, (i / n) as f64, 64.0, 64.0, 3.0))
            .collect();
        let g = gen_ao(
            &bump(&hf, n, n, 0.5, true),
            Tiling::Wrap,
            Directx,
            AoOpts::default(),
        );
        let dark = g.px.iter().filter(|&&v| v < 0.5).count();
        let quarter = g.px.iter().filter(|&&v| v < 0.25).count();
        assert!(dark > quarter, "below 0.5 {dark}, below 0.25 {quarter}");
    }

    /// Analytic height plus its central-difference slopes (`k = 0.5`: the slope, not twice it).
    fn analytic(hf: &[f64], w: usize, h: usize, wrap: bool) -> super::super::Slopes {
        slopes(&bump(hf, w, h, 0.5, wrap), Directx)
    }

    #[test]
    fn ao_on_a_flat_surface_is_one() {
        let s = slopes(&constant(64, 64, [0.5, 0.5, 1.0, 1.0]), Directx);
        let h = vec![0.0; 64 * 64];
        for wrap in [true, false] {
            let ao = march(&h, &s, wrap, &AoOpts::default(), &no()).unwrap();
            assert!(ao.iter().all(|&v| (v - 1.0).abs() < 1e-6), "wrap {wrap}");
        }
        // R < 0.5 texel: AO = 1 everywhere, without marching, even on relief.
        let t = torus(64, 64);
        let ao = march(
            &t,
            &analytic(&t, 64, 64, true),
            true,
            &ao_opts(0.0, 16),
            &no(),
        )
        .unwrap();
        assert!(ao.iter().all(|&v| v == 1.0));
    }

    #[test]
    fn a_tilted_constant_normal_gives_ao_one() {
        let n = constant(64, 64, [0.6, 0.45, 1.0, 1.0]);
        for tiling in [Tiling::Wrap, Tiling::Mirror] {
            let ao = gen_ao(&n, tiling, Directx, AoOpts::default());
            let min = ao.px.iter().copied().fold(1.0f32, f32::min);
            assert!(min >= 0.999, "{tiling:?}: min {min}");
        }
    }

    #[test]
    fn a_pit_centre_matches_the_open_cone() {
        let n = 256;
        let (c, sigma, depth) = (128.0, 4.0, 6.0);
        let hf: Vec<f64> = (0..n * n)
            .map(|i| -depth * gauss((i % n) as f64, (i / n) as f64, c, c, sigma))
            .collect();
        let big_r = 64.0;
        let hr = |r: f64| -depth * (-(r * r) / (2.0 * sigma * sigma)).exp();
        let e_star = (1..=640_000)
            .map(|i| {
                let r = f64::from(i) * 1e-4;
                ((hr(r) - hr(0.0)) / r).atan()
            })
            .fold(f64::MIN, f64::max);
        let want = e_star.cos().powi(2);
        let o = ao_opts((big_r / n as f64) as f32, 16);
        let centre = 128 * n + 128;
        let s = analytic(&hf, n, n, true);
        let ao = march_with_bias(&hf, &s, true, &o, &no(), 0.0).unwrap();
        assert!(
            (f64::from(ao[centre]) - want).abs() < 0.01,
            "analytic: {} vs cos²(e*) {want}",
            ao[centre]
        );
        let hs = solve(&s, true, 0.0);
        let ao = march_with_bias(&hs, &s, true, &o, &no(), 0.0).unwrap();
        assert!(
            (f64::from(ao[centre]) - want).abs() < 0.02,
            "from the normal: {} vs cos²(e*) {want}",
            ao[centre]
        );
    }

    #[test]
    fn the_default_march_is_close_to_a_brute_force_reference() {
        let n = 256;
        let hf = torus(n, n);
        let s = analytic(&hf, n, n, true);
        let o = AoOpts::default();
        let fast = march(&hf, &s, true, &o, &no()).unwrap();
        // Reference: 128 slices, every texel out to R (linear, 1-texel steps), same falloff and bias.
        let big_r = f64::from(o.radius) * n as f64;
        let dirs = directions(128);
        let radii: Vec<(f64, f64)> = (1..=big_r.ceil() as usize)
            .map(|i| (i as f64, falloff(i as f64, big_r)))
            .collect();
        let field = Field {
            h: &hf,
            w: n,
            hgt: n,
            wrap: true,
        };
        let mut d: Vec<f64> = (0..n * n)
            .map(|i| {
                let r = texel(&field, &s, i % n, i / n, &dirs, &radii, BIAS);
                (r - f64::from(fast[i])).abs()
            })
            .collect();
        let mean = d.iter().sum::<f64>() / d.len() as f64;
        d.sort_by(f64::total_cmp);
        let p99 = d[(0.99 * (d.len() - 1) as f64).round() as usize];
        assert!(mean < 0.02 && p99 < 0.06, "mean {mean}, p99 {p99}");
    }

    #[test]
    fn more_depth_lowers_ao() {
        // Not "never at any texel" (brief): with the source normal fixed, a sample between the tangent plane and P's
        // level on a concave descent (tan_t < e < 0) falls below the tangent at 2x depth, so that side opens. Steep
        // consistent geometry does the same (Note CO). Pinned: a flat-normal texel gets darker, the mean falls, and no
        // texel rises by more than 0.02 (0.015 measured on this fixture).
        let n = bump(&torus(128, 128), 128, 128, 4.0, true);
        let run = |depth: f32| {
            gen_ao(
                &n,
                Tiling::Wrap,
                Directx,
                AoOpts {
                    depth,
                    ..AoOpts::default()
                },
            )
        };
        let (a1, a2) = (run(1.0), run(2.0));
        let rise = a1
            .px
            .iter()
            .zip(&a2.px)
            .map(|(x, y)| y - x)
            .fold(f32::MIN, f32::max);
        assert!(rise <= 0.02, "max rise {rise}");
        let mean = |g: &Gray| g.px.iter().map(|&v| f64::from(v)).sum::<f64>() / g.px.len() as f64;
        assert!(
            mean(&a2) < mean(&a1) - 0.02,
            "{} vs {}",
            mean(&a2),
            mean(&a1)
        );
        // A flat normal (the pit centre) has tan_t = 0 in every direction: deeper is strictly darker there.
        let n = 128;
        let hf: Vec<f64> = (0..n * n)
            .map(|i| -6.0 * gauss((i % n) as f64, (i / n) as f64, 64.0, 64.0, 4.0))
            .collect();
        let s = analytic(&hf, n, n, true);
        let h2: Vec<f64> = hf.iter().map(|v| v * 2.0).collect();
        let o = AoOpts::default();
        let (b1, b2) = (
            march(&hf, &s, true, &o, &no()).unwrap(),
            march(&h2, &s, true, &o, &no()).unwrap(),
        );
        let c = 64 * n + 64;
        assert!(
            b2[c] < b1[c],
            "pit centre: depth 2 {} vs depth 1 {}",
            b2[c],
            b1[c]
        );
    }

    /// Seam step ≤ 1.5 × the largest interior neighbour step, in x and y.
    fn assert_tiles(g: &Gray) {
        let (w, h) = (g.width as usize, g.height as usize);
        let at = |x: usize, y: usize| f64::from(g.px[y * w + x]);
        let (mut seam_x, mut int_x, mut seam_y, mut int_y) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        for y in 0..h {
            seam_x = seam_x.max((at(0, y) - at(w - 1, y)).abs());
            for x in 0..w - 1 {
                int_x = int_x.max((at(x, y) - at(x + 1, y)).abs());
            }
        }
        for x in 0..w {
            seam_y = seam_y.max((at(x, 0) - at(x, h - 1)).abs());
            for y in 0..h - 1 {
                int_y = int_y.max((at(x, y) - at(x, y + 1)).abs());
            }
        }
        assert!(seam_x <= 1.5 * int_x, "x seam {seam_x} vs interior {int_x}");
        assert!(seam_y <= 1.5 * int_y, "y seam {seam_y} vs interior {int_y}");
    }

    #[test]
    fn a_wrapped_ao_tiles() {
        let n = bump(&torus(256, 256), 256, 256, 4.0, true);
        assert_tiles(&gen_ao(&n, Tiling::Wrap, Directx, AoOpts::default()));
    }

    #[test]
    fn a_wrapped_ao_above_the_solve_size_tiles() {
        // 2048 × 32: the solve runs at 1024 × 16 and AO is upsampled (wrap-aware) to 2048 × 32.
        let (w, h) = (2048, 32);
        let n = bump(&torus(w, h), w, h, 4.0, true);
        let g = gen_ao(&n, Tiling::Wrap, Directx, AoOpts::default());
        assert_eq!((g.width, g.height), (2048, 32));
        assert_tiles(&g);
        // A wrapped tile has no edge: AO of the normal rolled by half a tile is the AO rolled. A clamped upsample
        // breaks this near the old edge (now mid-tile), which the seam bound alone does not catch.
        let roll = |px: &[[f32; 4]]| -> Vec<[f32; 4]> {
            (0..w * h)
                .map(|i| px[((i / w + h / 2) % h) * w + (i % w + w / 2) % w])
                .collect()
        };
        let rolled = Rgba32F {
            px: roll(&n.px),
            ..n.clone()
        };
        let gr = gen_ao(&rolled, Tiling::Wrap, Directx, AoOpts::default());
        let d = (0..w * h)
            .map(|i| (gr.px[i] - g.px[((i / w + h / 2) % h) * w + (i % w + w / 2) % w]).abs())
            .fold(0.0f32, f32::max);
        assert!(d < 1e-4, "rolled AO differs by {d}");
        // The same surface at 1024 (same UV radius, heights in its own texels): the upsampled height must be scaled
        // by ao_size / solve_size and the slopes must not, or the 2048 AO is lighter or darker (review I1).
        // 45° triangle ridges along y (period 256) with a gentle y swell: smooth at the 16-row solve, unlike the torus,
        // whose y content reaches Nyquist there; its valleys make AO sensitive to the height scale.
        let hf: Vec<f64> = (0..w * h)
            .map(|i| {
                let (x, y) = ((i % w) as f64, (i / w) as f64);
                let tau = 2.0 * std::f64::consts::PI;
                let m = x % 256.0;
                m.min(256.0 - m) + 2.0 * (tau * y / 32.0).cos()
            })
            .collect();
        let n = bump(&hf, w, h, 0.5, true);
        let big = gen_ao(&n, Tiling::Wrap, Directx, AoOpts::default());
        let small = gen_ao_at(&n, 1024, Tiling::Wrap, Directx, AoOpts::default());
        assert_eq!((small.width, small.height), (1024, 16));
        let mean = |g: &Gray| g.px.iter().map(|&v| f64::from(v)).sum::<f64>() / g.px.len() as f64;
        let (m2, m1) = (mean(&big), mean(&small));
        let mad = (0..1024 * 16)
            .map(|i| {
                let (x, y) = (i % 1024, i / 1024);
                let b = |xx: usize, yy: usize| f64::from(big.px[yy * 2048 + xx]);
                let avg = (b(2 * x, 2 * y)
                    + b(2 * x + 1, 2 * y)
                    + b(2 * x, 2 * y + 1)
                    + b(2 * x + 1, 2 * y + 1))
                    / 4.0;
                (avg - f64::from(small.px[i])).abs()
            })
            .sum::<f64>()
            / (1024.0 * 16.0);
        assert!((m2 - m1).abs() < 0.01, "2048 mean {m2} vs 1024 mean {m1}");
        // Per texel too: a dropped ratio darkens the downhill sides and opens the uphill ones, so the mean barely
        // moves (0.006) while each texel does (mean |Δ| 0.036 against 0.001).
        assert!(mad < 0.01, "mean |2048 (2×2 average) − 1024| {mad}");
    }

    #[test]
    fn the_wrong_green_changes_ao_strongly() {
        let n = bump(&blobs(128, 128), 128, 128, 2.0, true);
        let run = |g, slices| gen_ao(&n, Tiling::Wrap, g, ao_opts(0.0625, slices)).px;
        let mean_diff = |a: &[f32], b: &[f32]| {
            a.iter()
                .zip(b)
                .map(|(x, y)| f64::from((x - y).abs()))
                .sum::<f64>()
                / a.len() as f64
        };
        let (right, right32, wrong) = (run(Directx, 16), run(Directx, 32), run(Opengl, 16));
        let (dw, ds) = (mean_diff(&right, &wrong), mean_diff(&right, &right32));
        assert!(dw > 4.0 * ds, "wrong green {dw} vs slices {ds}");
    }

    fn refused(ao: AoOpts) -> bool {
        let o = Opts {
            size: 64,
            tiling: Tiling::Wrap,
            relief: 0.3,
            green: Some(Directx),
            height: None,
            ao: Some(ao),
            curvature: false,
        };
        let n = bump(&torus(64, 64), 64, 64, 4.0, true);
        matches!(
            generate(&n, &o, &AtomicU32::new(0), &no()),
            Err(crate::from_normal::GenerateError::InvalidOpts(_))
        )
    }

    #[test]
    fn an_ao_radius_outside_0_to_0_25_is_refused() {
        for radius in [f32::NAN, -0.01, 0.26, f32::INFINITY] {
            assert!(refused(ao_opts(radius, 16)), "radius {radius}");
        }
        assert!(!refused(ao_opts(0.0, 16)) && !refused(ao_opts(0.25, 16)));
    }

    #[test]
    fn ao_slices_outside_1_to_64_are_refused() {
        for slices in [0, 65] {
            assert!(refused(ao_opts(0.0625, slices)), "slices {slices}");
        }
    }

    #[test]
    fn ao_steps_outside_1_to_64_are_refused() {
        for steps in [0, 65] {
            let o = AoOpts {
                steps,
                ..AoOpts::default()
            };
            assert!(refused(o), "steps {steps}");
        }
    }

    #[test]
    fn an_ao_depth_outside_0_25_to_4_is_refused() {
        for depth in [f32::NAN, 0.2, 4.1, f32::INFINITY] {
            let o = AoOpts {
                depth,
                ..AoOpts::default()
            };
            assert!(refused(o), "depth {depth}");
        }
    }

    #[test]
    fn an_ao_intensity_outside_0_5_to_3_is_refused() {
        for intensity in [f32::NAN, 0.4, 3.1, f32::INFINITY] {
            let o = AoOpts {
                intensity,
                ..AoOpts::default()
            };
            assert!(refused(o), "intensity {intensity}");
        }
    }

    #[test]
    fn the_bias_ignores_tan_4_degree_horizons_only() {
        // Ridges along y, period 64, R = 16: amplitude 1 peaks about tan 2.4° above the tangent, amplitude 4 about
        // tan 7° (review M2). BIAS = 0 darkens the first; BIAS = 0.2 opens the second.
        let n = 256;
        let min_ao = |a: f64, bias: Option<f64>| {
            let hf: Vec<f64> = (0..n * n)
                .map(|i| a * (2.0 * std::f64::consts::PI * (i % n) as f64 / 64.0).sin())
                .collect();
            let s = analytic(&hf, n, n, true);
            let o = AoOpts::default();
            match bias {
                Some(b) => march_with_bias(&hf, &s, true, &o, &no(), b),
                None => march(&hf, &s, true, &o, &no()),
            }
            .unwrap()
            .into_iter()
            .fold(1.0f32, f32::min)
        };
        assert_eq!(min_ao(1.0, None), 1.0, "a gentle ridge is fully open");
        assert!(
            min_ao(1.0, Some(0.0)) < 1.0 - 1e-4,
            "bias 0 sees its horizons"
        );
        assert!(min_ao(4.0, None) < 0.999, "a 7° horizon is not ignored");
    }

    #[test]
    fn mirror_samples_clamp_to_the_edge() {
        // A tall bump 4 texels from the right edge: wrapped it shadows column 0; clamped it is out of reach (review M1).
        let n = 128;
        let hf: Vec<f64> = (0..n * n)
            .map(|i| 30.0 * gauss((i % n) as f64, (i / n) as f64, 124.0, 64.0, 3.0))
            .collect();
        let s = analytic(&hf, n, n, false);
        let o = AoOpts::default();
        let col0 = |wrap: bool| {
            let ao = march(&hf, &s, wrap, &o, &no()).unwrap();
            (0..n).map(|y| ao[y * n]).fold(1.0f32, f32::min)
        };
        assert!(col0(false) >= 0.999, "mirror: column 0 {}", col0(false));
        assert!(col0(true) < 0.99, "wrap: column 0 {}", col0(true));
    }

    #[test]
    fn generate_marches_the_solve_times_depth_with_unscaled_slopes() {
        // Review M4: at the solve size, generate's AO is march(solve · depth, slopes); the slopes are not scaled.
        let (w, h) = (128, 128);
        let n = bump(&torus(w, h), w, h, 4.0, true);
        let o = AoOpts {
            depth: 2.0,
            ..AoOpts::default()
        };
        let g = gen_ao(&n, Tiling::Wrap, Directx, o.clone());
        let s = slopes(&n, Directx);
        let hs: Vec<f64> = solve(&s, true, 0.3).iter().map(|v| v * 2.0).collect();
        let want = march(&hs, &s, true, &o, &no()).unwrap();
        let d =
            g.px.iter()
                .zip(&want)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0f32, f32::max);
        assert!(d < 1e-6, "max |Δ| {d}");
    }

    #[test]
    fn the_march_reports_progress_per_row() {
        // Review M3: progress moves from AO_PROGRESS.0 to AO_PROGRESS.0 + AO_PROGRESS.1 as rows finish.
        let t = torus(64, 64);
        let s = analytic(&t, 64, 64, true);
        let p = AtomicU32::new(0);
        march_with_progress(&t, &s, true, &AoOpts::default(), &p, &no()).unwrap();
        assert_eq!(p.into_inner(), AO_PROGRESS.0 + AO_PROGRESS.1);
    }

    #[test]
    fn cancel_stops_the_march() {
        let t = torus(64, 64);
        let s = analytic(&t, 64, 64, true);
        assert!(march(&t, &s, true, &AoOpts::default(), &AtomicBool::new(true)).is_none());
    }

    #[test]
    fn an_open_ramp_is_not_darkened_like_dot_up() {
        // Ridges along y with 45° faces, period 64: valley at x ≡ 0, peak at x ≡ 32 (mod 64).
        let n = 256;
        let tri = |x: usize| {
            let m = (x % 64) as f64;
            if m <= 32.0 {
                m
            } else {
                64.0 - m
            }
        };
        let hf: Vec<f64> = (0..n * n).map(|i| tri(i % n)).collect();
        let g = gen_ao(
            &bump(&hf, n, n, 0.5, true),
            Tiling::Wrap,
            Directx,
            AoOpts::default(),
        );
        let row = 100 * n;
        let (face, valley) = (g.px[row + 64 + 16], g.px[row + 64]);
        assert!(face > 0.9, "mid-face {face}");
        assert!(valley < face - 0.1, "valley {valley} vs mid-face {face}");
    }
}
