//! Generate a height (and later AO and curvature) from a normal map (v2 Tracks B + D, DECISIONS Note CO).
mod fft;
mod solve;

use crate::config::GreenConvention;
use crate::image::Rgba32F;
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use solve::{detect_tiling, fit, solve};

/// A one-channel image, row-major.
#[derive(Debug, Clone, PartialEq)]
pub struct Gray {
    pub width: u32,
    pub height: u32,
    pub px: Vec<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tiling {
    Auto,
    Wrap,
    Mirror,
}

/// Image-space slopes (rows down), de-tilted: `p` and `q` have zero mean, and `tilt` is the mean `(p̄, q̄)` removed.
#[derive(Clone)]
pub struct Slopes {
    pub width: u32,
    pub height: u32,
    pub p: Vec<f64>,
    pub q: Vec<f64>,
    pub tilt: (f64, f64),
}

/// Image-space slopes `(p, q)` (y down) of a stored normal, DirectX green: x and y decoded from R and G, z rebuilt
/// (B is ignored, so BC5's zero blue reads like BC1/3/7) and clamped to at least 0.1. The one slope decoder, shared
/// with [`crate::checks::curl`].
pub(crate) fn texel_slope(px: [f32; 4]) -> (f64, f64) {
    let (x, y) = (f64::from(px[0]) * 2.0 - 1.0, f64::from(px[1]) * 2.0 - 1.0);
    let z = (1.0 - x * x - y * y).max(0.0).sqrt().max(0.1);
    (-x / z, -y / z)
}

/// The de-tilted slopes of a stored normal map; OpenGL green is flipped first.
pub(crate) fn slopes(stored: &Rgba32F, green: GreenConvention) -> Slopes {
    let flip = if green == GreenConvention::Opengl {
        -1.0
    } else {
        1.0
    };
    let (mut p, mut q): (Vec<f64>, Vec<f64>) = stored
        .px
        .iter()
        .map(|&px| {
            let (p, q) = texel_slope(px);
            // A non-finite texel reads as flat, so it cannot poison the mean and the whole solve (as curl skips it).
            if p.is_finite() && q.is_finite() {
                (p, flip * q)
            } else {
                (0.0, 0.0)
            }
        })
        .unzip();
    let n = p.len().max(1) as f64;
    // Shifted means: a constant field de-tilts to exact zeros.
    let mean = |v: &[f64]| {
        v.first()
            .map_or(0.0, |&a| a + v.iter().map(|x| x - a).sum::<f64>() / n)
    };
    let tilt = (mean(&p), mean(&q));
    p.iter_mut().for_each(|v| *v -= tilt.0);
    q.iter_mut().for_each(|v| *v -= tilt.1);
    Slopes {
        width: stored.width,
        height: stored.height,
        p,
        q,
        tilt,
    }
}

/// The angle of the mean normal from +z, in degrees.
pub(crate) fn tilt_deg(s: &Slopes) -> f32 {
    s.tilt.0.hypot(s.tilt.1).atan().to_degrees() as f32
}

/// Height offer cap (D2, parallax cost) for the free Size choice: `sizes(normal, HEIGHT_MAX)`.
pub const HEIGHT_MAX: u32 = 1024;
/// Height cap where a recipe needs the set's size (OWNER-Q1): `generate` caps the height here; the caller picks.
pub const HEIGHT_MAX_RECIPE: u32 = 2048;
pub const AO_MAX: u32 = 2048;
pub const SOLVE_MAX: u32 = 1024;
pub const DETECT_SIZE: u32 = 1024; // tiling detection: min(normal's longer side, this); 07 measured the thresholds at 1024
pub const GREEN_SIZE: u32 = 512; // green detection: min(normal's longer side, this) (07 B3)
pub const GREEN_MARGIN: f32 = 1.25; // below this error ratio green is "undetermined" and stays DirectX (07 B3)
pub const COST_WARN_ABOVE: u32 = 512;
pub const HALF_LO: f32 = 8.0 / 255.0;
pub const HALF_HI: f32 = 247.0 / 255.0;
/// Relief of the green-detection solves: a constant, so detection depends on the normal alone (Note CO).
const GREEN_RELIEF: f64 = 0.0;

/// The stored value that means "no shift": 0.5 (Half) or 1.0 (White, PBR inner-layer parallax).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Neutral {
    Half,
    White,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeightOpts {
    /// 0.1–1.0.
    pub strength: f32,
    pub neutral: Neutral,
}

impl Default for HeightOpts {
    fn default() -> Self {
        Self {
            strength: 1.0,
            neutral: Neutral::Half,
        }
    }
}

/// GTAO settings (computed by Task 8).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AoOpts {
    pub radius: f32,
    pub slices: u32,
    pub steps: u32,
    pub depth: f32,
    pub intensity: f32,
}

impl Default for AoOpts {
    fn default() -> Self {
        Self {
            radius: 0.0625,
            slices: 16,
            steps: 16,
            depth: 1.0,
            intensity: 1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Opts {
    /// Requested longer side of the outputs; each map is capped at its own maximum.
    pub size: u32,
    pub tiling: Tiling,
    /// Relief balance, 0–1 (default 0.3).
    pub relief: f32,
    /// `None`: auto-detect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub green: Option<GreenConvention>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<HeightOpts>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ao: Option<AoOpts>,
    pub curvature: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub wrap: bool,
    pub seam_ratio: f32,
    pub ambiguous: bool,
    pub green: GreenConvention,
    pub green_auto: bool,
    pub green_sure: bool,
    pub green_margin: f32,
    pub tilt_deg: f32,
    pub fit_mean_deg: f32,
    pub fit_p95_deg: f32,
    pub clamped_share: f32,
    pub ao_mean: Option<f32>,
    pub ao_dark_share: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct Generated {
    pub height: Option<Gray>,
    pub ao: Option<Gray>,
    pub curvature: Option<Gray>,
    pub report: Report,
}

#[derive(Debug, thiserror::Error)]
pub enum GenerateError {
    #[error("the normal map is {0}×{1}; only power-of-two sizes can be generated")]
    NotPowerOfTwo(u32, u32),
    #[error("{0}")]
    InvalidOpts(&'static str),
    #[error("cancelled")]
    Cancelled,
}

/// The offered sizes: those of 256/512/1024/2048 within `cap` and the normal's longer side, else that side alone.
pub fn sizes(normal: (u32, u32), cap: u32) -> Vec<u32> {
    let longer = normal.0.max(normal.1);
    let v: Vec<u32> = [256, 512, 1024, 2048]
        .into_iter()
        .filter(|&s| s <= cap && s <= longer)
        .collect();
    if v.is_empty() {
        vec![longer]
    } else {
        v
    }
}

/// The normal's aspect with `long` on its longer side; each side at least 1.
fn dims((w, h): (u32, u32), long: u32) -> (u32, u32) {
    let l = u64::from(w.max(h));
    let side = |s: u32| ((u64::from(s) * u64::from(long) / l) as u32).max(1);
    (side(w), side(h))
}

/// `s` area-resampled to `size` (slopes are not rescaled); the full-size tilt is kept.
fn resize(s: &Slopes, size: (u32, u32)) -> Slopes {
    if (s.width, s.height) == size {
        return s.clone();
    }
    let img = Rgba32F {
        width: s.width,
        height: s.height,
        px: s
            .p
            .iter()
            .zip(&s.q)
            .map(|(&p, &q)| [p as f32, q as f32, 0.0, 0.0])
            .collect(),
    };
    let r = crate::mips::area_resample(&img, size, false);
    Slopes {
        width: size.0,
        height: size.1,
        p: r.px.iter().map(|v| f64::from(v[0])).collect(),
        q: r.px.iter().map(|v| f64::from(v[1])).collect(),
        tilt: s.tilt,
    }
}

/// The same slopes read as OpenGL (q negated).
fn flipped(s: &Slopes) -> Slopes {
    Slopes {
        q: s.q.iter().map(|v| -v).collect(),
        tilt: (s.tilt.0, -s.tilt.1),
        ..s.clone()
    }
}

/// The sorted value at index `round(q·(n − 1))` (as `fit`'s p95); 0 when empty. Selection, not a full sort.
fn percentile(mut v: Vec<f64>, q: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let i = (v.len().saturating_sub(1) as f64 * q).round() as usize;
    *v.select_nth_unstable_by(i, f64::total_cmp).1
}

/// Maps a zero-mean height to stored values (spec "Height range"); returns the image and the clamped share.
fn map_height(
    h: &[f64],
    (w, hgt): (u32, u32),
    o: &HeightOpts,
    cancel: &AtomicBool,
) -> Result<(Gray, f32), GenerateError> {
    let r = percentile(h.iter().map(|v| v.abs()).collect(), 0.995);
    let t = percentile(h.to_vec(), 0.995);
    let k = f64::from(o.strength) * 0.45 / r;
    let flat = r < 1e-12;
    // White: CS reads the stored d as 0.5·d (scale 0.5, offset −0.25), so 1.0 = no shift and nothing rises; the top
    // of the relief (t) sits at 1.0 at the same Strength scale as Half (Note CO).
    let map = |v: f64| match (o.neutral, flat) {
        (Neutral::Half, true) => (0.5, 0.0, 1.0),
        (Neutral::White, true) => (1.0, 0.0, 1.0),
        (Neutral::Half, false) => (0.5 + k * v, f64::from(HALF_LO), f64::from(HALF_HI)),
        (Neutral::White, false) => (1.0 - k * (t - v), 0.0, 1.0),
    };
    let mut px = Vec::with_capacity(h.len());
    let mut clamped = 0usize;
    for row in h.chunks(w.max(1) as usize) {
        if cancel.load(Ordering::Relaxed) {
            return Err(GenerateError::Cancelled);
        }
        for &v in row {
            let (x, lo, hi) = map(v);
            let c = x.clamp(lo, hi);
            clamped += usize::from(c != x);
            px.push(c as f32);
        }
    }
    let share = clamped as f32 / h.len().max(1) as f32;
    Ok((
        Gray {
            width: w,
            height: hgt,
            px,
        },
        share,
    ))
}

/// `g` resampled to `size`: a wrapped tile upsamples with a periodic bilinear (no seam line at the tile edge, review
/// M1); otherwise `rnm::blend::resample` (area down, bilinear up with clamped edges, exact for the mirror solve).
fn resample_gray(g: Gray, size: (u32, u32), wrap: bool) -> Gray {
    if (g.width, g.height) == size {
        return g;
    }
    if wrap && size.0 >= g.width && size.1 >= g.height {
        let axis = |o: u32, n: u32, m: u32| {
            let f = (f64::from(o) + 0.5) * f64::from(n) / f64::from(m) - 0.5;
            let i = f.floor();
            let i0 = (i as i64).rem_euclid(i64::from(n)) as usize;
            (i0, (i0 + 1) % n as usize, (f - i) as f32)
        };
        let xs: Vec<_> = (0..size.0).map(|x| axis(x, g.width, size.0)).collect();
        let w = g.width as usize;
        let mut px = Vec::with_capacity(size.0 as usize * size.1 as usize);
        for y in 0..size.1 {
            let (y0, y1, ty) = axis(y, g.height, size.1);
            for &(x0, x1, tx) in &xs {
                let at = |xx: usize, yy: usize| g.px[yy * w + xx];
                let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * tx;
                let bot = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * tx;
                px.push(top + (bot - top) * ty);
            }
        }
        return Gray {
            width: size.0,
            height: size.1,
            px,
        };
    }
    let img = Rgba32F {
        width: g.width,
        height: g.height,
        px: g.px.iter().map(|&v| [v, 0.0, 0.0, 0.0]).collect(),
    };
    let r = crate::rnm::blend::resample(img, size);
    Gray {
        width: size.0,
        height: size.1,
        px: r.px.iter().map(|v| v[0]).collect(),
    }
}

/// A height (and later AO and curvature) from a stored normal map. `progress` is per mille; `cancel` is read between
/// stages and between rows.
pub fn generate(
    stored: &Rgba32F,
    opts: &Opts,
    progress: &AtomicU32,
    cancel: &AtomicBool,
) -> Result<Generated, GenerateError> {
    use GenerateError::InvalidOpts;
    if !(0.0..=1.0).contains(&opts.relief) {
        return Err(InvalidOpts("the relief balance must be between 0 and 1"));
    }
    if opts.size == 0 {
        return Err(InvalidOpts("the size must be above 0"));
    }
    if opts
        .height
        .as_ref()
        .is_some_and(|h| !(0.1..=1.0).contains(&h.strength))
    {
        return Err(InvalidOpts("the height strength must be between 0.1 and 1"));
    }
    let (w, h) = (stored.width, stored.height);
    if w == 0 || h == 0 {
        return Err(InvalidOpts("the normal map is empty"));
    }
    if !w.is_power_of_two() || !h.is_power_of_two() {
        return Err(GenerateError::NotPowerOfTwo(w, h));
    }
    if opts.size > w.max(h) {
        // Spec: never upsample beyond the normal (Note CO).
        return Err(InvalidOpts("the size is above the normal map's own size"));
    }
    let step = |pm: u32| {
        progress.store(pm, Ordering::Relaxed);
        if cancel.load(Ordering::Relaxed) {
            Err(GenerateError::Cancelled)
        } else {
            Ok(())
        }
    };
    step(0)?;
    let longer = w.max(h);
    // shortcut: cancel is read between stages and rows, not inside one FFT solve (up to ~0.3 s at 1024); pass `cancel`
    // into `solve` if a longer solve ever makes that wait noticeable.
    let full = slopes(stored, GreenConvention::Directx);
    step(100)?;
    // Detection runs at fixed sizes, never the requested one, so the 512 preview and Generate agree (07 B2).
    // `det` keeps the tiling field when it was resampled, for the solve to reuse at the same size.
    let mut det = None;
    let (wrap, seam_ratio, ambiguous) = match opts.tiling {
        Tiling::Wrap => (true, 0.0, false),
        Tiling::Mirror => (false, 0.0, false),
        Tiling::Auto => {
            let d = dims((w, h), longer.min(DETECT_SIZE));
            if d.0.min(d.1) <= 16 {
                // Too few seam texels to judge: wrap, flagged ambiguous (Note CO).
                (true, 0.0, true)
            } else {
                det = (d != (w, h)).then(|| resize(&full, d));
                let (wrap, ratio, ambiguous) = detect_tiling(det.as_ref().unwrap_or(&full));
                (wrap, ratio as f32, ambiguous)
            }
        }
    };
    step(250)?;
    // Tiling detection squares q differences, so it is green-invariant and can run before green detection.
    let (green, green_auto, green_sure, green_margin) = match opts.green {
        Some(g) => (g, false, true, 0.0),
        None => {
            let dx = resize(&full, dims((w, h), longer.min(GREEN_SIZE)));
            let err = |s: &Slopes| fit(s, &solve(s, wrap, GREEN_RELIEF)).0;
            let e_dx = err(&dx);
            step(350)?;
            let e_gl = err(&flipped(&dx));
            let (lo, hi) = (e_dx.min(e_gl), e_dx.max(e_gl));
            let margin = if hi == 0.0 { 1.0 } else { hi / lo };
            if margin >= GREEN_MARGIN {
                let g = if e_gl < e_dx {
                    GreenConvention::Opengl
                } else {
                    GreenConvention::Directx
                };
                (g, true, true, margin)
            } else {
                (GreenConvention::Directx, true, false, margin)
            }
        }
    };
    step(450)?;
    let long = opts.size.min(HEIGHT_MAX_RECIPE);
    let solve_size = dims((w, h), long.min(SOLVE_MAX));
    let mut s = match det {
        Some(d) if (d.width, d.height) == solve_size => d,
        _ => resize(&full, solve_size),
    };
    if green == GreenConvention::Opengl {
        s = flipped(&s);
    }
    let hs = solve(&s, wrap, f64::from(opts.relief));
    let (fit_mean_deg, fit_p95_deg) = fit(&s, &hs);
    step(850)?;
    // Mapped at the solve size, then resampled: interpolation keeps the clamp bounds (Note CO).
    let (height, clamped_share) = match &opts.height {
        Some(o) => {
            let (g, share) = map_height(&hs, solve_size, o, cancel)?;
            (Some(resample_gray(g, dims((w, h), long), wrap)), share)
        }
        None => (None, 0.0),
    };
    progress.store(1000, Ordering::Relaxed);
    Ok(Generated {
        height,
        ao: None,
        curvature: None,
        report: Report {
            wrap,
            seam_ratio,
            ambiguous,
            green,
            green_auto,
            green_sure,
            green_margin,
            tilt_deg: tilt_deg(&full),
            fit_mean_deg,
            fit_p95_deg,
            clamped_share,
            ao_mean: None,
            ao_dark_share: None,
        },
    })
}

/// A 16-bit grey PNG: codes `round(clamp(v, 0, 1)·65535)`. Empty for a malformed `Gray` (a 0 side, or not one
/// value per texel), never a panic.
pub fn png16(g: &Gray) -> Vec<u8> {
    if g.width == 0
        || g.height == 0
        || g.px.len() as u64 != u64::from(g.width) * u64::from(g.height)
    {
        return Vec::new();
    }
    let codes: Vec<u16> =
        g.px.iter()
            .map(|&v| (v.clamp(0.0, 1.0) * 65535.0).round() as u16)
            .collect();
    let Some(img) =
        ::image::ImageBuffer::<::image::Luma<u16>, Vec<u16>>::from_raw(g.width, g.height, codes)
    else {
        return Vec::new();
    };
    let mut out = Cursor::new(Vec::new());
    match img.write_to(&mut out, ::image::ImageFormat::Png) {
        Ok(()) => out.into_inner(),
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::solve::tests::{blobs, bump, constant, diffs, encode, ramp, torus};
    use super::*;
    use crate::config::GreenConvention::{Directx, Opengl};
    use std::sync::atomic::{AtomicBool, AtomicU32};

    fn opts(size: u32, neutral: Neutral) -> Opts {
        Opts {
            size,
            tiling: Tiling::Auto,
            relief: 0.3,
            green: None,
            height: Some(HeightOpts {
                strength: 1.0,
                neutral,
            }),
            ao: None,
            curvature: false,
        }
    }

    fn run(n: &Rgba32F, o: &Opts) -> Result<Generated, GenerateError> {
        generate(n, o, &AtomicU32::new(0), &AtomicBool::new(false))
    }

    fn torus_n(w: usize, h: usize) -> Rgba32F {
        bump(&torus(w, h), w, h, 4.0, true)
    }

    fn height(n: &Rgba32F, o: &Opts) -> Gray {
        run(n, o).ok().and_then(|g| g.height).expect("a height")
    }

    fn report(n: &Rgba32F, o: &Opts) -> Report {
        run(n, o).expect("generates").report
    }

    fn pct(v: &[f32], q: f64) -> f64 {
        percentile(v.iter().map(|&x| f64::from(x)).collect(), q)
    }

    fn invalid(n: &Rgba32F, o: &Opts) -> bool {
        matches!(run(n, o), Err(GenerateError::InvalidOpts(_)))
    }

    #[test]
    fn half_range_is_centred_and_clamped_inside_the_complex_gate() {
        let g = height(&torus_n(256, 256), &opts(256, Neutral::Half));
        let mean = g.px.iter().map(|&v| f64::from(v)).sum::<f64>() / g.px.len() as f64;
        assert!((mean - 0.5).abs() < 0.02, "mean {mean}");
        let (lo, hi) =
            g.px.iter()
                .fold((1.0f32, 0.0f32), |(a, b), &v| (a.min(v), b.max(v)));
        assert!(lo >= HALF_LO && hi <= HALF_HI, "{lo}..{hi}");
        assert!(lo > 4.0 / 255.0 && hi < 251.0 / 255.0);
        let dev: Vec<f32> = g.px.iter().map(|v| (v - 0.5).abs()).collect();
        let p = pct(&dev, 0.995);
        assert!((p - 0.45).abs() < 0.01, "p99.5 |out - 0.5| {p}");
    }

    #[test]
    fn flat_in_gives_constant_half_and_constant_white() {
        let n = constant(64, 64, [0.5, 0.5, 1.0, 1.0]);
        let g = height(&n, &opts(64, Neutral::Half));
        assert!(g.px.iter().all(|&v| v == 0.5));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("flat_p_generated.png");
        std::fs::write(&path, png16(&g)).unwrap();
        let (img, _) = crate::load::load_image(&path).unwrap();
        assert_eq!((img.width, img.height), (64, 64));
        assert!(
            img.px.iter().all(|p| p[0] == 32768.0 / 65535.0),
            "{:?}",
            img.px[0]
        );
        let g = height(&n, &opts(64, Neutral::White));
        assert!(g.px.iter().all(|&v| v == 1.0));
    }

    #[test]
    fn inner_layer_mapping_matches_the_track_c_table() {
        let adjusted = |d: f32| (d - 0.5) * 0.5 + 0.5 - 0.25; // CS scale 0.5, offset -0.25
        assert_eq!(adjusted(1.0), 0.5); // stored 1.0 = no shift (Track C table)
        let n = torus_n(256, 256);
        let mut o = opts(256, Neutral::White);
        o.tiling = Tiling::Wrap;
        o.green = Some(Directx);
        let white = height(&n, &o);
        let h = solve(&slopes(&n, Directx), true, 0.3);
        let r = percentile(h.iter().map(|v| v.abs()).collect(), 0.995);
        let t = percentile(h.clone(), 0.995);
        let mut above = 0;
        for (&hv, &out) in h.iter().zip(&white.px) {
            if hv <= t {
                let want = 1.0 - 0.45 * (t - hv) / r;
                assert!(
                    (f64::from(out) - want).abs() < 1e-5,
                    "h {hv}: {out} vs {want}"
                );
            } else {
                assert_eq!(out, 1.0);
                above += 1;
            }
        }
        // 0.5 % by construction (t = p99.5), plus one texel for the percentile index's rounding.
        assert!(
            above as f64 <= 0.005 * h.len() as f64 + 1.0,
            "{above} clamped at 1"
        );
        let nearest = |x: f64| {
            (0..h.len())
                .min_by(|&a, &b| (h[a] - x).abs().total_cmp(&(h[b] - x).abs()))
                .unwrap()
        };
        assert!((f64::from(white.px[nearest(t)]) - 1.0).abs() < 1e-4);
        let z = nearest(0.0);
        assert!((f64::from(white.px[z]) - (1.0 - 0.45 * t / r)).abs() < 1e-4);
        o.height = Some(HeightOpts {
            strength: 1.0,
            neutral: Neutral::Half,
        });
        let half = height(&n, &o);
        let span = |g: &Gray| pct(&g.px, 0.995) - pct(&g.px, 0.005);
        let (sw, sh) = (span(&white), span(&half));
        assert!((sw / sh - 1.0).abs() < 0.02, "White span {sw} vs Half {sh}");
        // Both formulas per texel at S = 1 and S = 0.5: pins Half's sign and the Strength factor (review I1).
        for strength in [1.0f32, 0.5] {
            let k = f64::from(strength) * 0.45 / r;
            for neutral in [Neutral::Half, Neutral::White] {
                o.height = Some(HeightOpts { strength, neutral });
                let g = height(&n, &o);
                for (&hv, &out) in h.iter().zip(&g.px) {
                    let want = match neutral {
                        Neutral::Half => {
                            (0.5 + k * hv).clamp(f64::from(HALF_LO), f64::from(HALF_HI))
                        }
                        Neutral::White => (1.0 - k * (t - hv)).clamp(0.0, 1.0),
                    };
                    assert!(
                        (f64::from(out) - want).abs() < 1e-5,
                        "S {strength} {neutral:?} h {hv}: {out} vs {want}"
                    );
                }
            }
        }
        o.height = Some(HeightOpts {
            strength: 0.5,
            neutral: Neutral::Half,
        });
        let dev: Vec<f32> = height(&n, &o).px.iter().map(|v| (v - 0.5).abs()).collect();
        let p = pct(&dev, 0.995);
        assert!((p - 0.225).abs() < 0.005, "S 0.5: p99.5 |out - 0.5| {p}");
    }

    #[test]
    fn sizes_never_exceed_the_normal_or_the_cap() {
        assert_eq!(sizes((1024, 1024), HEIGHT_MAX), vec![256, 512, 1024]);
        assert_eq!(sizes((2048, 1024), AO_MAX), vec![256, 512, 1024, 2048]);
        assert_eq!(sizes((2048, 2048), HEIGHT_MAX), vec![256, 512, 1024]);
        assert_eq!(sizes((64, 64), HEIGHT_MAX), vec![64]);
        assert_eq!(
            sizes((2048, 2048), HEIGHT_MAX_RECIPE),
            vec![256, 512, 1024, 2048]
        );
    }

    #[test]
    fn generate_writes_the_requested_size_and_refuses_npot() {
        let g = height(&torus_n(1024, 1024), &opts(512, Neutral::Half));
        assert_eq!((g.width, g.height, g.px.len()), (512, 512, 512 * 512));
        let g = height(&torus_n(1024, 512), &opts(512, Neutral::Half));
        assert_eq!((g.width, g.height, g.px.len()), (512, 256, 512 * 256));
        let npot = constant(1000, 1000, [0.5, 0.5, 1.0, 1.0]);
        assert!(matches!(
            run(&npot, &opts(512, Neutral::Half)),
            Err(GenerateError::NotPowerOfTwo(1000, 1000))
        ));
    }

    #[test]
    fn a_recipe_height_may_reach_2048() {
        // OWNER-Q1: the solve runs at SOLVE_MAX and the height is resampled up to the recipe's size.
        let n = torus_n(2048, 2048);
        let g = height(&n, &opts(2048, Neutral::Half));
        assert_eq!((g.width, g.height), (2048, 2048));
        // A 4096 x 1 strip keeps the cap check cheap.
        let strip = constant(4096, 1, [0.5, 0.5, 1.0, 1.0]);
        let g = height(&strip, &opts(4096, Neutral::Half));
        assert_eq!(
            (g.width, g.height),
            (2048, 1),
            "capped at HEIGHT_MAX_RECIPE"
        );
    }

    #[test]
    fn green_auto_picks_the_lower_error_and_says_so() {
        let mut gl = bump(&blobs(128, 128), 128, 128, 2.0, true);
        gl.px.iter_mut().for_each(|p| p[1] = 1.0 - p[1]);
        let r = report(&gl, &opts(128, Neutral::Half));
        assert_eq!(r.green, Opengl);
        assert!(
            r.green_auto && r.green_sure && r.green_margin >= GREEN_MARGIN,
            "{r:?}"
        );
        let mut o = opts(128, Neutral::Half);
        o.green = Some(Directx);
        let r = report(&gl, &o);
        assert!(!r.green_auto && r.green_sure && r.green_margin == 0.0 && r.green == Directx);
        let flat = constant(128, 128, [0.5, 0.5, 1.0, 1.0]);
        let r = report(&flat, &opts(128, Neutral::Half));
        assert!(
            r.green == Directx && r.green_auto && !r.green_sure && r.green_margin == 1.0,
            "{r:?}"
        );
    }

    #[test]
    fn detection_does_not_depend_on_the_requested_size() {
        let n = bump(&ramp(1024, 1024), 1024, 1024, 4.0, false);
        let (a, b) = (
            report(&n, &opts(512, Neutral::Half)),
            report(&n, &opts(1024, Neutral::Half)),
        );
        assert_eq!(
            (
                a.wrap,
                a.seam_ratio,
                a.ambiguous,
                a.green,
                a.green_sure,
                a.green_margin
            ),
            (
                b.wrap,
                b.seam_ratio,
                b.ambiguous,
                b.green,
                b.green_sure,
                b.green_margin
            )
        );
    }

    #[test]
    fn a_tilted_normal_reports_its_tilt_and_not_a_fit_floor() {
        let (w, h) = (256, 256);
        let (p, q) = diffs(&torus(w, h), w, h, 4.0, true);
        let (pt, qt): (Vec<f64>, Vec<f64>) = (
            p.iter().map(|v| v + 0.2).collect(),
            q.iter().map(|v| v - 0.1).collect(),
        );
        let mut o = opts(256, Neutral::Half);
        o.tiling = Tiling::Wrap;
        o.green = Some(Directx);
        let flat = report(&encode(&p, &q, w, h), &o);
        let tilted = report(&encode(&pt, &qt, w, h), &o);
        let want = 0.2f64.hypot(0.1).atan().to_degrees() as f32;
        assert!(
            (tilted.tilt_deg - want).abs() < 0.1,
            "{} vs {want}",
            tilted.tilt_deg
        );
        assert!((tilted.fit_mean_deg - flat.fit_mean_deg).abs() < 0.01);
    }

    #[test]
    fn cancel_stops_generate() {
        let r = generate(
            &torus_n(64, 64),
            &opts(64, Neutral::Half),
            &AtomicU32::new(0),
            &AtomicBool::new(true),
        );
        assert!(matches!(r, Err(GenerateError::Cancelled)));
    }

    #[test]
    fn a_relief_outside_0_to_1_is_refused() {
        let n = torus_n(64, 64);
        for relief in [f32::NAN, -0.1, 1.5, f32::INFINITY] {
            let mut o = opts(64, Neutral::Half);
            o.relief = relief;
            assert!(invalid(&n, &o), "relief {relief}");
        }
    }

    #[test]
    fn a_zero_size_is_refused() {
        assert!(invalid(&torus_n(64, 64), &opts(0, Neutral::Half)));
    }

    #[test]
    fn an_empty_normal_is_refused_before_npot() {
        for (w, h) in [(0, 4), (4, 0), (0, 0)] {
            let n = Rgba32F {
                width: w,
                height: h,
                px: vec![],
            };
            assert!(invalid(&n, &opts(64, Neutral::Half)), "{w}x{h}");
        }
    }

    #[test]
    fn a_strength_outside_0_1_to_1_is_refused() {
        let n = torus_n(64, 64);
        for strength in [f32::NAN, 0.05, 1.01] {
            let mut o = opts(64, Neutral::Half);
            o.height = Some(HeightOpts {
                strength,
                neutral: Neutral::Half,
            });
            assert!(invalid(&n, &o), "strength {strength}");
        }
    }

    #[test]
    fn a_size_above_the_normal_is_refused() {
        // Spec: never upsample beyond the normal (review M2).
        assert!(invalid(&torus_n(256, 256), &opts(512, Neutral::Half)));
        assert!(invalid(&torus_n(256, 128), &opts(512, Neutral::Half)));
        assert!(run(&torus_n(256, 128), &opts(256, Neutral::Half)).is_ok());
    }

    #[test]
    fn a_wrapped_upsample_has_no_seam() {
        // A 1024 solve upsampled to 2048 (review M1). Output texels 2k+1 and 2k+2 straddle source k and k+1, so their
        // step is half the source step; across the tile edge (k = 1023, next = 0) it must be the same, not a full step.
        let n = 1024;
        let hs = solve(&slopes(&torus_n(n, n), Directx), true, 0.0);
        let row: Vec<f32> = hs[..n].iter().map(|&v| v as f32).collect();
        let col: Vec<f32> = (0..n).map(|y| hs[y * n + 7] as f32).collect();
        for (line, size) in [(row, (2048, 1)), (col, (1, 2048))] {
            let (w, h) = if size.0 == 1 {
                (1, n as u32)
            } else {
                (n as u32, 1)
            };
            let up = resample_gray(
                Gray {
                    width: w,
                    height: h,
                    px: line.clone(),
                },
                size,
                true,
            )
            .px;
            for k in [0, 300, 1022, 1023] {
                let (a, b) = (2 * k + 1, (2 * k + 2) % 2048);
                let want = 0.5 * (f64::from(line[(k + 1) % n]) - f64::from(line[k]));
                let got = f64::from(up[b]) - f64::from(up[a]);
                assert!((got - want).abs() < 1e-6, "{size:?} k {k}: {got} vs {want}");
            }
        }
    }

    #[test]
    fn png16_of_a_malformed_gray_is_empty_not_a_panic() {
        for (w, h, n) in [(0, 0, 0), (0, 4, 0), (2, 2, 3), (2, 2, 5)] {
            let g = Gray {
                width: w,
                height: h,
                px: vec![0.5; n],
            };
            assert!(png16(&g).is_empty(), "{w}x{h} with {n} px");
        }
    }

    #[test]
    fn a_detection_field_16_px_or_narrower_is_wrap_and_ambiguous() {
        for (w, h) in [(16, 16), (64, 8), (2048, 32)] {
            let r = report(&torus_n(w, h), &opts(w.max(h) as u32, Neutral::Half));
            assert!(
                r.wrap && r.ambiguous && r.seam_ratio == 0.0,
                "{w}x{h}: {r:?}"
            );
        }
    }
}
