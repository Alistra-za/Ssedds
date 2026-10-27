//! Equirectangular (lat-long) mapping, Z-up world, row 0 = +Z, column growing clockwise seen from above (NIF Note DL).
//! `yaw` is the azimuth added before the column lookup; render3d's preview calibration `YAW` stays in render3d.
use std::f32::consts::{PI, TAU};

/// World direction (unit, Z-up) -> image coordinates `(u, v)` in 0..1 (`v` = 0 at +Z).
pub fn dir_to_uv(d: [f32; 3], yaw: f32) -> (f32, f32) {
    let u = (0.5 - (d[1].atan2(d[0]) + yaw) / TAU).rem_euclid(1.0);
    let v = d[2].clamp(-1.0, 1.0).acos() / PI;
    (u, v)
}

/// Inverse of [`dir_to_uv`].
pub fn uv_to_dir(u: f32, v: f32, yaw: f32) -> [f32; 3] {
    let phi = (0.5 - u) * TAU - yaw;
    let theta = v * PI;
    [
        theta.sin() * phi.cos(),
        theta.sin() * phi.sin(),
        theta.cos(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: [f32; 3], b: [f32; 3]) -> bool {
        (0..3).all(|k| (a[k] - b[k]).abs() < 1e-4)
    }

    #[test]
    fn yaw_zero_pins_render3d_convention() {
        assert!(near(uv_to_dir(0.5, 0.5, 0.0), [1.0, 0.0, 0.0]));
        assert!(near(uv_to_dir(0.25, 0.5, 0.0), [0.0, 1.0, 0.0]));
        assert!(near(uv_to_dir(0.3, 0.0, 0.0), [0.0, 0.0, 1.0]));
    }

    #[test]
    fn dir_uv_round_trip_with_yaw() {
        for yaw in [0.0, -2.798] {
            for d in [[0.6, 0.0, 0.8], [-0.3, 0.5, -0.812_403_8], [0.0, -1.0, 0.0]] {
                let (u, v) = dir_to_uv(d, yaw);
                assert!(near(d, uv_to_dir(u, v, yaw)), "{d:?} yaw {yaw}");
            }
        }
    }
}
