//! Direction-based importers: a 2:1 equirect panorama to a cube (cubemap support v1, DECISIONS DN14).
use super::latlong::dir_to_uv;
use super::world::to_world;
use super::{dir_at, sample, Cube};
use crate::image::Rgba32F;

/// Bilinear sample of `img` at `(u, v)` in 0..1: `u` wraps (the 360-degree seam), `v` clamps at the poles.
fn bilinear(img: &Rgba32F, u: f32, v: f32) -> [f32; 4] {
    let (w, h) = (img.width as i64, img.height as i64);
    let (fx, fy) = (u * w as f32 - 0.5, v * h as f32 - 0.5);
    let (x0, y0) = (fx.floor(), fy.floor());
    let (wx, wy) = (fx - x0, fy - y0);
    let (x0, y0) = (x0 as i64, y0 as i64);
    let at = |x: i64, y: i64| img.at(x.rem_euclid(w) as u32, y.clamp(0, h - 1) as u32);
    let (a, b, c, d) = (
        at(x0, y0),
        at(x0 + 1, y0),
        at(x0, y0 + 1),
        at(x0 + 1, y0 + 1),
    );
    std::array::from_fn(|k| {
        (a[k] * (1.0 - wx) + b[k] * wx) * (1.0 - wy) + (c[k] * (1.0 - wx) + d[k] * wx) * wy
    })
}

/// A `size` cube from the equirect `img` (standard 360-degree mapping, `latlong::dir_to_uv` at yaw 0): each texel is
/// the mean of 3x3 bilinear samples over its footprint.
pub fn equirect(img: &Rgba32F, size: u32) -> Cube {
    const K: u32 = 3;
    sample::build(size, |face, x, y| {
        let mut sum = [0.0f32; 4];
        for j in 0..K {
            for i in 0..K {
                let s = (x as f32 + (i as f32 + 0.5) / K as f32) / size as f32;
                let t = (y as f32 + (j as f32 + 0.5) / K as f32) / size as f32;
                let (u, v) = dir_to_uv(to_world(dir_at(face, s, t)), 0.0);
                let p = bilinear(img, u, v);
                for k in 0..4 {
                    sum[k] += p[k];
                }
            }
        }
        sum.map(|v| v / (K * K) as f32)
    })
}
