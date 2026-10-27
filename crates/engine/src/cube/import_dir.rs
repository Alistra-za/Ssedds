//! Direction-based importers: equirect panoramas (DECISIONS DN14), mirror balls and angular maps (DN15) to a cube.
use super::latlong::dir_to_uv;
use super::world::to_world;
use super::{dir_at, sample, Cube};
use crate::image::Rgba32F;

/// Bilinear sample of `img` at `(u, v)` in 0..1: `u` wraps when `wrap` (the 360-degree seam), else clamps; `v` clamps.
fn bilinear(img: &Rgba32F, u: f32, v: f32, wrap: bool) -> [f32; 4] {
    let (w, h) = (img.width as i64, img.height as i64);
    let (fx, fy) = (u * w as f32 - 0.5, v * h as f32 - 0.5);
    let (x0, y0) = (fx.floor(), fy.floor());
    let (wx, wy) = (fx - x0, fy - y0);
    let (x0, y0) = (x0 as i64, y0 as i64);
    let at = |x: i64, y: i64| {
        let x = if wrap {
            x.rem_euclid(w)
        } else {
            x.clamp(0, w - 1)
        };
        img.at(x as u32, y.clamp(0, h - 1) as u32)
    };
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
    project(img, size, true, |d| dir_to_uv(d, 0.0))
}

/// Camera-space `(east, up, south)` of a world direction (DN15): the camera stands at world south looking north.
fn camera(d: [f32; 3]) -> [f32; 3] {
    let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-12);
    [d[0] / l, d[2] / l, -d[1] / l]
}

/// A `size` cube from a mirror-ball photo (DN15): `n = normalize(c + (0,0,1))`, `(u, v) = (n.x, n.y)` in the unit
/// disc, radius clamped to 0.98 so the back pole (north) reads from the outer ring.
pub fn mirror_ball(img: &Rgba32F, size: u32) -> Cube {
    project(img, size, false, |d| {
        let c = camera(d);
        let n = [c[0], c[1], c[2] + 1.0];
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        let m = (n[0] * n[0] + n[1] * n[1]).sqrt();
        if len < 1e-6 {
            // Exactly north: no unique ring point; take image right.
            return (0.98, 0.0);
        }
        if m < 1e-9 {
            return (0.0, 0.0);
        }
        let k = (m / len).min(0.98) / m;
        (n[0] * k, n[1] * k)
    })
}

/// A `size` cube from an angular map (DN15): `r = acos(-c.z) / pi` along `(c.x, c.y)`, centre north, rim south.
pub fn angular(img: &Rgba32F, size: u32) -> Cube {
    project(img, size, false, |d| {
        let c = camera(d);
        let r = (-c[2]).clamp(-1.0, 1.0).acos() / std::f32::consts::PI;
        let m = (c[0] * c[0] + c[1] * c[1]).sqrt();
        if m < 1e-9 {
            (r, 0.0)
        } else {
            (r * c[0] / m, r * c[1] / m)
        }
    })
}

/// Each cube texel is the mean of 3x3 bilinear samples over its footprint; `map` takes a world direction to the
/// panorama's `(u, v)` (equirect: 0..1; disc layouts: -1..1 with v up, mapped to the image here).
fn project(
    img: &Rgba32F,
    size: u32,
    equirect: bool,
    map: impl Fn([f32; 3]) -> (f32, f32) + Sync,
) -> Cube {
    // shortcut: fixed 3x3 taps alias when face_size is far below the source cell (a large source to a small cube);
    // scale the tap count with source_cell/size when that matters.
    const K: u32 = 3;
    sample::build(size, |face, x, y| {
        let mut sum = [0.0f32; 4];
        for j in 0..K {
            for i in 0..K {
                let s = (x as f32 + (i as f32 + 0.5) / K as f32) / size as f32;
                let t = (y as f32 + (j as f32 + 0.5) / K as f32) / size as f32;
                let (u, v) = map(to_world(dir_at(face, s, t)));
                let (u, v) = if equirect {
                    (u, v)
                } else {
                    ((u + 1.0) / 2.0, (1.0 - v) / 2.0)
                };
                let p = bilinear(img, u, v, equirect);
                for k in 0..4 {
                    sum[k] += p[k];
                }
            }
        }
        sum.map(|v| v / (K * K) as f32)
    })
}
