//! World <-> cube basis, measured in game (Note DN13): Skyrim world X east, Y north, Z up.

/// Column `i` is the cube-space axis that world axis `i` samples.
pub const WORLD_TO_CUBE: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]; // identity, determinant +1

/// World up in cube space (column 2).
pub const UP: [f32; 3] = [0.0, 0.0, 1.0];

pub fn to_cube(w: [f32; 3]) -> [f32; 3] {
    let m = WORLD_TO_CUBE;
    std::array::from_fn(|r| m[r][0] * w[0] + m[r][1] * w[1] + m[r][2] * w[2])
}

pub fn to_world(d: [f32; 3]) -> [f32; 3] {
    let m = WORLD_TO_CUBE;
    std::array::from_fn(|c| m[0][c] * d[0] + m[1][c] * d[1] + m[2][c] * d[2])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cube::{cube_dir, dir_to_face};

    const E: [f32; 3] = [1.0, 0.0, 0.0];
    const W: [f32; 3] = [-1.0, 0.0, 0.0];
    const N: [f32; 3] = [0.0, 1.0, 0.0];
    const S: [f32; 3] = [0.0, -1.0, 0.0];
    const U: [f32; 3] = [0.0, 0.0, 1.0];
    const D: [f32; 3] = [0.0, 0.0, -1.0];

    /// results.md table: (world direction, face index, row-0 band points, column-0 band points).
    type Row = ([f32; 3], usize, [f32; 3], [f32; 3]);
    const TABLE: [Row; 6] = [
        (U, 4, N, W),
        (D, 5, N, E),
        (N, 2, D, W),
        (S, 3, U, W),
        (E, 0, N, U),
        (W, 1, N, D),
    ];

    fn dominant(v: [f32; 3]) -> [f32; 3] {
        let i = (0..3)
            .max_by(|&a, &b| v[a].abs().total_cmp(&v[b].abs()))
            .unwrap();
        let mut o = [0.0; 3];
        o[i] = v[i].signum();
        o
    }

    #[test]
    fn basis_matches_the_in_game_table() {
        for (w, face, _, _) in TABLE {
            assert_eq!(dir_to_face(to_cube(w)).0, face, "{w:?}");
        }
        assert_eq!(UP, to_cube(U));
    }

    #[test]
    fn bands_match_d3d_addressing() {
        let n = 64;
        for (_, f, row0, col0) in TABLE {
            let c = cube_dir(f, n / 2, n / 2, n);
            let r: [f32; 3] = std::array::from_fn(|i| cube_dir(f, n / 2, 0, n)[i] - c[i]);
            let k: [f32; 3] = std::array::from_fn(|i| cube_dir(f, 0, n / 2, n)[i] - c[i]);
            assert_eq!(dominant(to_world(r)), row0, "face {f} row 0");
            assert_eq!(dominant(to_world(k)), col0, "face {f} column 0");
        }
    }

    #[test]
    fn basis_is_orthonormal() {
        let m = WORLD_TO_CUBE;
        for i in 0..3 {
            for j in 0..3 {
                let dot: f32 = (0..3).map(|k| m[i][k] * m[j][k]).sum();
                assert_eq!(dot, (i == j) as u8 as f32);
            }
        }
    }
}
