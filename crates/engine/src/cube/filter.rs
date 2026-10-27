//! Cube yaw and mirror (DECISIONS DN14), and colour filters on linear-light faces (exposure, highlight clamp, display-space grading in the game's 1.6 gamma, DECISIONS DN11). Alpha is untouched.
use super::world::{to_cube, to_world};
use super::{cube_dir, Cube};
use crate::config::{CubemapSpec, GroundFill};
use crate::ops::{game_gamma_to_linear, linear_to_game_gamma};

/// The cube turned `degrees` about world up, positive clockwise seen from above (Skyrim's heading sense: 90 moves
/// east to south). 0 returns the input unchanged.
pub fn yaw(c: &Cube, degrees: f64) -> Cube {
    if degrees == 0.0 {
        return c.clone();
    }
    // out(d) = in(R d), R the counter-clockwise-from-above turn by `degrees` about world Z (up), in world space.
    let (s, co) = (degrees.to_radians() as f32).sin_cos();
    super::sample::resample(c, c.size, |d| {
        let w = to_world(d);
        to_cube([w[0] * co - w[1] * s, w[0] * s + w[1] * co, w[2]])
    })
}

/// The cube with world east and west swapped: `out(d) = in(to_cube(diag(-1, 1, 1) * to_world(d)))`.
pub fn mirror(c: &Cube) -> Cube {
    super::sample::resample(c, c.size, |d| {
        let w = to_world(d);
        to_cube([-w[0], w[1], w[2]])
    })
}

/// Fill the cube below `horizon_deg` (DECISIONS DN17); alpha untouched.
pub fn ground_fill(c: &mut Cube, fill: &GroundFill, horizon_deg: f64) {
    let size = c.size;
    let elev = |f: usize, i: usize| {
        let d = cube_dir(f, i as u32 % size, i as u32 / size, size);
        (to_world(d)[2].clamp(-1.0, 1.0) as f64).asin().to_degrees()
    };
    // Horizon colour: mean over [h, h+5]; if empty (tiny cubes), mean of the texels nearest above h.
    let (mut band, mut nb) = ([0f64; 3], 0u32);
    let mut near: (f64, [f64; 3], u32) = (f64::MAX, [0.0; 3], 0);
    for (f, face) in c.faces.iter().enumerate() {
        for (i, p) in face.px.iter().enumerate() {
            let e = elev(f, i);
            let rgb = [p[0] as f64, p[1] as f64, p[2] as f64];
            if (horizon_deg..=horizon_deg + 5.0).contains(&e) {
                (0..3).for_each(|k| band[k] += rgb[k]);
                nb += 1;
            } else if e >= horizon_deg {
                if e < near.0 - 0.5 {
                    near = (e, rgb, 1);
                } else if e < near.0 + 0.5 {
                    (0..3).for_each(|k| near.1[k] += rgb[k]);
                    near.2 += 1;
                }
            }
        }
    }
    let (sum, n) = if nb > 0 { (band, nb) } else { (near.1, near.2) };
    let hc = sum.map(|v| (v / n.max(1) as f64) as f32);
    let ground = match fill {
        GroundFill::Color(rgb) => Some(rgb.map(|v| game_gamma_to_linear(v as f32 / 255.0))),
        GroundFill::Mode(_) => None,
    };
    for (f, face) in c.faces.iter_mut().enumerate() {
        for (i, p) in face.px.iter_mut().enumerate() {
            let e = elev(f, i);
            // 1 below horizon-1, 0 above horizon+1, linear between.
            let a = ((horizon_deg + 1.0 - e) / 2.0).clamp(0.0, 1.0) as f32;
            if a == 0.0 {
                continue;
            }
            let target = ground.unwrap_or_else(|| {
                let t = ((horizon_deg - e) / (horizon_deg + 90.0)).clamp(0.0, 1.0) as f32;
                hc.map(|v| v * (1.0 - 0.5 * t))
            });
            for k in 0..3 {
                p[k] = p[k] * (1.0 - a) + target[k] * a;
            }
        }
    }
}

fn luma(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// Apply the pinned filter order; every step is skipped at its default, so a default spec is bit-identical.
pub fn colour(c: &mut Cube, s: &CubemapSpec) {
    let gain = 2f32.powf(s.exposure as f32);
    let exposure = s.exposure != 0.0;
    let clamp = s.highlight_clamp.map(|h| (h * h) as f32);
    let display = s.brightness != 0.0
        || s.contrast != 0.0
        || s.midtones != 1.0
        || s.saturation != 1.0
        || s.tint.is_some();
    if !exposure && clamp.is_none() && !display {
        return;
    }
    let (b, k, inv_m, sat) = (
        s.brightness as f32,
        1.0 + s.contrast as f32,
        1.0 / s.midtones as f32,
        s.saturation as f32,
    );
    let tint = s.tint.map(|t| t.map(|v| v as f32 / 255.0));
    for face in c.faces.iter_mut() {
        for p in face.px.iter_mut() {
            let mut rgb = [p[0], p[1], p[2]];
            if exposure {
                rgb = rgb.map(|v| v * gain);
            }
            if let Some(h2) = clamp {
                let l = luma(rgb);
                if l > 0.0 {
                    let l2 = (l * (1.0 + l / h2) / (1.0 + l)).min(1.0);
                    rgb = rgb.map(|v| v * (l2 / l));
                }
            }
            if display {
                let mut x = rgb.map(linear_to_game_gamma);
                if s.brightness != 0.0 || s.contrast != 0.0 {
                    x = x.map(|v| (v + b - 0.5) * k + 0.5);
                }
                if s.midtones != 1.0 {
                    x = x.map(|v| v.max(0.0).powf(inv_m));
                }
                if s.saturation != 1.0 {
                    let y = luma(x);
                    x = x.map(|v| y + (v - y) * sat);
                }
                if let Some(t) = tint {
                    x = [x[0] * t[0], x[1] * t[1], x[2] * t[2]];
                }
                rgb = x.map(|v| game_gamma_to_linear(v.clamp(0.0, 1.0)));
            }
            p[..3].copy_from_slice(&rgb);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::Rgba32F;

    fn flat(rgb: [f32; 3]) -> Cube {
        let faces = std::array::from_fn(|_| Rgba32F::new(4, 4, [rgb[0], rgb[1], rgb[2], 0.7]));
        Cube { size: 4, faces }
    }

    #[test]
    fn default_spec_is_identity() {
        let mut c = flat([0.1, 0.2, 0.3]);
        for (i, f) in c.faces.iter_mut().enumerate() {
            let col = [0.05 * i as f32, 0.9, 3.5, 1.0];
            for y in 1..3 {
                for x in 0..4 {
                    f.px[y * 4 + x] = col;
                }
            }
        }
        let before = c.clone();
        colour(&mut c, &CubemapSpec::default());
        assert_eq!(c, before);
    }

    #[test]
    fn exposure_one_doubles_linear() {
        let mut c = flat([0.2, 0.2, 0.2]);
        let s = CubemapSpec {
            exposure: 1.0,
            ..Default::default()
        };
        colour(&mut c, &s);
        assert!((c.faces[0].px[0][0] - 0.4).abs() < 1e-6);
        assert_eq!(c.faces[0].px[0][3], 0.7);
    }

    #[test]
    fn highlight_clamp_maps_white_point_to_one() {
        let s = CubemapSpec {
            highlight_clamp: Some(4.0),
            ..Default::default()
        };
        let mut c = flat([4.0, 4.0, 4.0]);
        colour(&mut c, &s);
        assert!((c.faces[0].px[0][0] - 1.0).abs() < 1e-5);
        let mut c = flat([0.1, 0.1, 0.1]);
        colour(&mut c, &s);
        let v = c.faces[0].px[0][0];
        assert!(v < 0.1 && v > 0.08, "{v}");
    }

    #[test]
    fn saturation_zero_is_grey() {
        let mut c = flat([0.8, 0.2, 0.1]);
        let s = CubemapSpec {
            saturation: 0.0,
            ..Default::default()
        };
        colour(&mut c, &s);
        let p = c.faces[0].px[0];
        assert!((p[0] - p[1]).abs() < 1e-6 && (p[1] - p[2]).abs() < 1e-6);
    }

    #[test]
    fn tint_multiplies_in_display_space() {
        let mut c = flat([0.5, 0.5, 0.5]);
        let d = linear_to_game_gamma(0.5);
        let s = CubemapSpec {
            tint: Some([255, 128, 255]),
            ..Default::default()
        };
        colour(&mut c, &s);
        let p = c.faces[0].px[0];
        assert!((linear_to_game_gamma(p[1]) - d * 128.0 / 255.0).abs() < 1e-5);
        assert!((linear_to_game_gamma(p[0]) - d).abs() < 1e-5);
    }

    fn elevation(face: usize, x: u32, y: u32, size: u32) -> f64 {
        let w = to_world(crate::cube::cube_dir(face, x, y, size));
        (w[2] as f64).asin().to_degrees()
    }

    fn texels(size: u32) -> impl Iterator<Item = (usize, usize, f64)> {
        (0..6).flat_map(move |f| {
            (0..size * size).map(move |i| (f, i as usize, elevation(f, i % size, i / size, size)))
        })
    }

    #[test]
    fn ground_colour_replaces_the_lower_hemisphere() {
        let mut c = Cube {
            size: 16,
            faces: std::array::from_fn(|_| Rgba32F::new(16, 16, [0.5, 0.5, 0.5, 0.7])),
        };
        ground_fill(&mut c, &GroundFill::Color([0, 0, 0]), 0.0);
        let down = crate::cube::dir_to_face(super::super::world::UP.map(|v| -v)).0;
        assert_eq!(down, 5);
        assert_eq!(c.faces[5].px[8 * 16 + 8], [0.0, 0.0, 0.0, 0.7]);
        assert_eq!(c.faces[4].px[8 * 16 + 8], [0.5, 0.5, 0.5, 0.7]);
    }

    #[test]
    fn colour_code_decodes_with_game_gamma() {
        let mut c = flat([0.5; 3]);
        ground_fill(&mut c, &GroundFill::Color([255, 0, 128]), 0.0);
        let p = c.faces[5].px[0];
        assert!((p[0] - 1.0).abs() < 1e-6 && p[1] == 0.0);
        assert!((p[2] - game_gamma_to_linear(128.0 / 255.0)).abs() < 1e-6);
    }

    #[test]
    fn gradient_starts_at_the_horizon_colour() {
        let size = 64;
        let mut c = Cube {
            size,
            faces: std::array::from_fn(|_| Rgba32F::new(size, size, [0.6, 0.6, 0.6, 1.0])),
        };
        ground_fill(
            &mut c,
            &GroundFill::Mode(crate::config::GroundMode::Gradient),
            0.0,
        );
        let (mut near, mut nadir) = (f32::MAX, 1.0f32);
        for (f, i, e) in texels(size) {
            let v = c.faces[f].px[i][0];
            if (-3.0..-1.5).contains(&e) {
                near = near.min(v);
            }
            if e < -85.0 {
                nadir = nadir.min(v);
            }
        }
        assert!(near > 0.585 && near <= 0.6, "{near}");
        assert!((nadir - 0.3).abs() < 0.01, "{nadir}");
    }

    #[test]
    fn empty_band_uses_the_nearest_row_above_and_stays_finite() {
        let mut c = flat([0.4; 3]);
        ground_fill(
            &mut c,
            &GroundFill::Mode(crate::config::GroundMode::Gradient),
            0.0,
        );
        assert!(c
            .faces
            .iter()
            .all(|f| f.px.iter().all(|p| p[..3].iter().all(|v| v.is_finite()))));
    }

    #[test]
    fn horizon_moves_the_line() {
        let size = 32;
        let run = |h: f64| {
            let mut c = Cube {
                size,
                faces: std::array::from_fn(|_| Rgba32F::new(size, size, [0.5, 0.5, 0.5, 1.0])),
            };
            ground_fill(&mut c, &GroundFill::Color([0, 0, 0]), h);
            c
        };
        let (a, b) = (run(0.0), run(20.0));
        let (f, i, _) = texels(size).find(|t| (9.0..11.0).contains(&t.2)).unwrap();
        assert_eq!(a.faces[f].px[i][0], 0.5);
        assert_eq!(b.faces[f].px[i][0], 0.0);
    }
}
