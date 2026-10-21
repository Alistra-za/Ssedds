//! Generate a height (and later AO and curvature) from a normal map (v2 Tracks B + D, DECISIONS Note CO).
// The solve's consumers (sizes, ranges, PNG) arrive in Task 4; until then only the tests call it.
#![allow(dead_code)]
mod fft;
mod solve;

use crate::config::GreenConvention;
use crate::image::Rgba32F;
use serde::{Deserialize, Serialize};

#[allow(unused_imports)] // used by Task 4
pub(crate) use solve::{detect_tiling, fit, solve};

/// A one-channel image, row-major.
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
