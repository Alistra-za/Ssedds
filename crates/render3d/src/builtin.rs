//! Built-in preview shapes (03 §6): sphere, plane, cube; Z-up, centred at the origin, radius / half-extent
//! [`BUILTIN_SIZE`]. The tangent frame follows the convention measured on nifly's fixtures (test
//! `tangent_convention_from_fixture`): `Mesh.tangents` (the file's `Tangent` bytes) run along +dP/dv,
//! `Mesh.bitangents` (the w's) along +dP/du, and cross(tangent, bitangent) is the normal. Triangles wind
//! counter-clockwise around the normal (cross(p1 − p0, p2 − p0) · n > 0), as every fixture triangle does.
//! Texture v runs downwards on screen (DirectX): seen from outside with "up" up, u goes right and v goes down.
use nif::geom::Mesh;

/// Radius (sphere) or half-extent (plane, cube) in NIF units.
pub const BUILTIN_SIZE: f32 = 32.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Builtin {
    Sphere,
    Plane,
    Cube,
}

pub fn builtin_mesh(b: Builtin) -> Mesh {
    match b {
        Builtin::Sphere => sphere(),
        Builtin::Plane => plane(),
        Builtin::Cube => cube(),
    }
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// 64 × 32 lat-long with a duplicated seam column (65 × 33 = 2,145 vertices). u = longitude / 2π (east),
/// v = colatitude / π (north pole v = 0). dP/dv points south, dP/du east; cross(south, east) = outward. The
/// degenerate triangle of each quad touching a pole is left out.
fn sphere() -> Mesh {
    const COLS: u16 = 64;
    const ROWS: u16 = 32;
    let mut m = Mesh::default();
    for i in 0..=ROWS {
        let v = i as f32 / ROWS as f32;
        let theta = v * std::f32::consts::PI;
        for j in 0..=COLS {
            let u = j as f32 / COLS as f32;
            let phi = u * std::f32::consts::TAU;
            let n = [
                theta.sin() * phi.cos(),
                theta.sin() * phi.sin(),
                theta.cos(),
            ];
            m.positions.push(n.map(|c| c * BUILTIN_SIZE));
            m.normals.push(n);
            m.uvs.push([u, v]);
            m.tangents.push([
                theta.cos() * phi.cos(),
                theta.cos() * phi.sin(),
                -theta.sin(),
            ]);
            m.bitangents.push([-phi.sin(), phi.cos(), 0.0]);
        }
    }
    let at = |i: u16, j: u16| i * (COLS + 1) + j;
    for i in 0..ROWS {
        for j in 0..COLS {
            let (a, b, c, d) = (at(i, j), at(i, j + 1), at(i + 1, j), at(i + 1, j + 1));
            if i + 1 < ROWS {
                m.triangles.push([a, c, d]);
            }
            if i > 0 {
                m.triangles.push([a, d, b]);
            }
        }
    }
    m
}

/// Two triangles facing +Y (towards `Camera::Auto`, which looks along −Y). Seen from +Y, screen right is −X and
/// screen down is −Z, so u runs along −X and v along −Z.
fn plane() -> Mesh {
    let s = BUILTIN_SIZE;
    Mesh {
        positions: vec![[s, 0.0, s], [-s, 0.0, s], [s, 0.0, -s], [-s, 0.0, -s]],
        uvs: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
        normals: vec![[0.0, 1.0, 0.0]; 4],
        tangents: vec![[0.0, 0.0, -1.0]; 4],
        bitangents: vec![[-1.0, 0.0, 0.0]; 4],
        triangles: vec![[0, 2, 3], [0, 3, 1]],
        ..Mesh::default()
    }
}

/// 24 vertices, one 0..1 UV square per face. Per face with normal N: dP/dv = V (−Z on the sides, −Y on top,
/// +Y underneath), dP/du = U = N × V, so cross(V, U) = N.
fn cube() -> Mesh {
    let s = BUILTIN_SIZE;
    let faces: [[f32; 3]; 6] = [
        [1.0, 0.0, 0.0],
        [-1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, -1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, -1.0],
    ];
    let mut m = Mesh::default();
    for (f, n) in faces.iter().enumerate() {
        let v_dir = if n[2] > 0.5 {
            [0.0, -1.0, 0.0]
        } else if n[2] < -0.5 {
            [0.0, 1.0, 0.0]
        } else {
            [0.0, 0.0, -1.0]
        };
        let u_dir = cross(*n, v_dir);
        for (u, v) in [(0.0f32, 0.0f32), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
            let p = [0, 1, 2]
                .map(|k| s * (n[k] + (2.0 * u - 1.0) * u_dir[k] + (2.0 * v - 1.0) * v_dir[k]));
            m.positions.push(p);
            m.uvs.push([u, v]);
            m.normals.push(*n);
            m.tangents.push(v_dir);
            m.bitangents.push(u_dir);
        }
        let b = (4 * f) as u16;
        m.triangles.push([b, b + 2, b + 3]);
        m.triangles.push([b, b + 3, b + 1]);
    }
    m
}
