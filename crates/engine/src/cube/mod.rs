//! Cube-map maths shared by the 3D preview and the cubemap pipeline (moved from render3d, Note DN1).
pub mod checks;
pub mod filter;
pub mod latlong;
pub mod layout;
pub mod marker;
pub mod plan;
pub mod rgbe;
pub(crate) mod run;
pub mod sample;

use crate::config::CubeLayout;
use crate::image::Rgba32F;
use crate::policy::SourceStamp;
use std::path::PathBuf;

/// How a cube job is built (`plan::plan_cubes`): one cross/strip/cube DDS file, or six face files in +X..-Z order.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct CubeJob {
    pub layout: CubeLayout,
    pub sources: Vec<CubeSource>,
}

/// One source file of a cube job, with its stamp when planned; `face` is its face index for a six-file cube.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct CubeSource {
    pub path: PathBuf,
    pub stamp: Option<SourceStamp>,
    pub face: Option<usize>,
}

/// Face names in D3D order (+X -X +Y -Y +Z -Z).
pub const FACES: [&str; 6] = ["posx", "negx", "posy", "negy", "posz", "negz"];

/// CS's GetSamplingVector (DynamicCubemaps/CaptureCommon.hlsli:48): texel centre of face 0..6 (+X -X +Y -Y +Z -Z) ->
/// unit world direction. This is D3D's cube addressing.
pub fn cube_dir(face: usize, x: u32, y: u32, size: u32) -> [f32; 3] {
    let s = (x as f32 + 0.5) / size as f32;
    let t = (y as f32 + 0.5) / size as f32;
    let (u, v) = (2.0 * s - 1.0, 2.0 * (1.0 - t) - 1.0);
    let d = match face {
        0 => [1.0, v, -u],
        1 => [-1.0, v, u],
        2 => [u, 1.0, -v],
        3 => [u, -1.0, v],
        4 => [u, v, 1.0],
        _ => [-u, v, -1.0],
    };
    let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    d.map(|c| c / n)
}

/// Six square faces (+X -X +Y -Y +Z -Z), each `size` x `size`.
#[derive(Debug, Clone, PartialEq)]
pub struct Cube {
    pub size: u32,
    pub faces: [Rgba32F; 6],
}

/// Unit direction at continuous face coordinates `s, t` in `[0, 1]` (same addressing as `cube_dir`; values outside
/// `[0, 1]` extend the face plane past its edge).
pub fn dir_at(face: usize, s: f32, t: f32) -> [f32; 3] {
    let (u, v) = (2.0 * s - 1.0, 1.0 - 2.0 * t);
    let d = match face {
        0 => [1.0, v, -u],
        1 => [-1.0, v, u],
        2 => [u, 1.0, -v],
        3 => [u, -1.0, v],
        4 => [u, v, 1.0],
        _ => [-u, v, -1.0],
    };
    let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    d.map(|c| c / n)
}

/// Inverse of `dir_at`: the face of the major axis (ties go to X, then Y), then `s, t` in `[0, 1]`. The zero vector
/// maps to the centre of +X.
pub fn dir_to_face(d: [f32; 3]) -> (usize, f32, f32) {
    let [x, y, z] = d;
    let (ax, ay, az) = (x.abs(), y.abs(), z.abs());
    if ax.max(ay).max(az) == 0.0 {
        return (0, 0.5, 0.5);
    }
    let (face, u, v) = if ax >= ay && ax >= az {
        if x > 0.0 {
            (0, -z / ax, y / ax)
        } else {
            (1, z / ax, y / ax)
        }
    } else if ay >= az {
        if y > 0.0 {
            (2, x / ay, -z / ay)
        } else {
            (3, x / ay, z / ay)
        }
    } else if z > 0.0 {
        (4, x / az, y / az)
    } else {
        (5, -x / az, y / az)
    };
    (face, (u + 1.0) * 0.5, (1.0 - v) * 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_dir_face_centres_are_the_axes() {
        let axes = [
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
        ];
        for (f, a) in axes.iter().enumerate() {
            let d = cube_dir(f, 0, 0, 1);
            assert!(
                (0..3).all(|k| (d[k] - a[k]).abs() < 1e-6),
                "face {f}: {d:?}"
            );
        }
    }

    #[test]
    fn cube_dir_row0_of_pos_x_points_up_in_d3d() {
        assert!(cube_dir(0, 1, 0, 4)[1] > 0.0);
        assert!(cube_dir(0, 0, 1, 4)[2] > 0.0);
    }
}
