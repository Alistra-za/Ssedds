//! Built-in preview shapes (03 §6): sphere, plane, cube, Z-up, centred at the origin, radius / half-extent
//! [`BUILTIN_SIZE`]; and a material ball (sphere radius [`BUILTIN_SIZE`]) standing on the floor z = 0. The
//! tangent frame follows the convention measured on nifly's fixtures (test `tangent_convention_from_fixture`):
//! `Mesh.tangents` (the file's `Tangent` bytes) run along +dP/dv, `Mesh.bitangents` (the w's) along +dP/du, and
//! cross(tangent, bitangent) is the normal. Triangles wind counter-clockwise around the normal
//! (cross(p1 − p0, p2 − p0) · n > 0), as every fixture triangle does.
//! Texture v runs downwards on screen (DirectX): seen from outside with "up" up, u goes right and v goes down.
use nif::geom::Mesh;

/// Radius (sphere) or half-extent (plane, cube) in NIF units.
pub const BUILTIN_SIZE: f32 = 32.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Builtin {
    Sphere,
    Plane,
    Cube,
    MaterialBall,
}

impl Builtin {
    pub const ALL: [Builtin; 4] = [
        Builtin::Sphere,
        Builtin::Plane,
        Builtin::Cube,
        Builtin::MaterialBall,
    ];
}

pub fn builtin_mesh(b: Builtin) -> Mesh {
    match b {
        Builtin::Sphere => sphere(),
        Builtin::Plane => plane(),
        Builtin::Cube => cube(),
        Builtin::MaterialBall => material_ball(),
    }
}

/// a + k·b.
fn mad(a: [f32; 3], k: f32, b: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|i| a[i] + k * b[i])
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

/// An origin and a right-handed basis: cross(x, y) = z, z being the lathe axis.
struct Frame {
    o: [f32; 3],
    x: [f32; 3],
    y: [f32; 3],
    z: [f32; 3],
}

const Z_UP: Frame = Frame {
    o: [0.0; 3],
    x: [1.0, 0.0, 0.0],
    y: [0.0, 1.0, 0.0],
    z: [0.0, 0.0, 1.0],
};

/// A profile point: (radius, height) and the unit direction of travel along the profile, (dr, dh).
type P = [f32; 4];

/// Arc around `c` of radius `r` from angle `a0` to `a1` (degrees, from +radius towards +height), `n` steps.
fn arc(c: [f32; 2], r: f32, a0: f32, a1: f32, n: u16) -> Vec<P> {
    let k = (a1 - a0).signum();
    (0..=n)
        .map(|i| {
            let (s, co) = (a0 + (a1 - a0) * i as f32 / n as f32)
                .to_radians()
                .sin_cos();
            [c[0] + r * co, c[1] + r * s, -k * s, k * co]
        })
        .collect()
}

fn line(a: [f32; 2], b: [f32; 2]) -> Vec<P> {
    let l = (b[0] - a[0]).hypot(b[1] - a[1]);
    let d = [(b[0] - a[0]) / l, (b[1] - a[1]) / l];
    vec![[a[0], a[1], d[0], d[1]], [b[0], b[1], d[0], d[1]]]
}

/// Sweeps `profile` around `f.z` from angle `sweep.0` to `sweep.1` (radians, increasing, from `f.x` towards
/// `f.y`), `segments` steps. Each inner `Vec` is one smooth segment; consecutive segments share their end point as
/// duplicated vertices (a hard edge). u = sweep fraction, v = profile length so far / total. dP/du runs around the
/// axis and dP/dv along the profile, so cross(dP/dv, dP/du) = (dr, dh) turned +90° in the (radius, height)
/// plane: a profile that runs down an outer wall faces outwards. Quads touching the axis lose their degenerate
/// triangle, as in [`sphere`].
fn lathe(m: &mut Mesh, profile: &[Vec<P>], f: &Frame, sweep: (f32, f32), segments: u16) {
    let pts: Vec<&P> = profile.iter().flatten().collect();
    let step = |a: &P, b: &P| (b[0] - a[0]).hypot(b[1] - a[1]);
    let total: f32 = pts.windows(2).map(|w| step(w[0], w[1])).sum();
    let (mut s, mut prev) = (0.0, pts[0]);
    for seg in profile {
        let base = m.positions.len() as u16;
        for p in seg {
            s += step(prev, p);
            prev = p;
            for j in 0..=segments {
                let u = j as f32 / segments as f32;
                let (sn, cs) = (sweep.0 + u * (sweep.1 - sweep.0)).sin_cos();
                let rho = mad(mad([0.0; 3], cs, f.x), sn, f.y);
                let around = mad(mad([0.0; 3], -sn, f.x), cs, f.y);
                m.positions.push(mad(mad(f.o, p[0], rho), p[1], f.z));
                m.normals.push(mad(mad([0.0; 3], -p[3], rho), p[2], f.z));
                m.tangents.push(mad(mad([0.0; 3], p[2], rho), p[3], f.z));
                m.bitangents.push(around);
                m.uvs.push([u, s / total]);
            }
        }
        let at = |i: usize, j: u16| base + i as u16 * (segments + 1) + j;
        for i in 0..seg.len() - 1 {
            for j in 0..segments {
                let (a, b, c, d) = (at(i, j), at(i, j + 1), at(i + 1, j), at(i + 1, j + 1));
                if seg[i + 1][0] > 1e-3 {
                    m.triangles.push([a, c, d]);
                }
                if seg[i][0] > 1e-3 {
                    m.triangles.push([a, d, b]);
                }
            }
        }
    }
}

/// A shader ball (no logo) on the floor z = 0, all parts lathed:
/// - pedestal: disc of radius 0.75·R, height 0.12·R, rounded top edge;
/// - neck: open cylinder of radius 0.25·R from the pedestal top into the ball;
/// - ball: radius R, bottom 0.1·R above the pedestal, lathed around the window axis `w` (+Y turned 30° towards
///   −X, tilted up 15°, facing `Camera::Auto` off-axis) from the front pole to the back: window floor (cap of
///   radius 0.88·R out to 38° from `w`), rim wall, rounded lip, outer sphere, groove (105°–111°, 0.08·R deep),
///   outer sphere to the back pole. The seam (u = 0) runs underneath.
/// - C-ring: rounded-rectangle section (0.18·R wide, 0.28·R high) around radius 1.05·R, swept 250° leaving the
///   gap towards +Y, closed by flat fan end caps (dP/dv = −Z, dP/du = the cap's outward normal × −Z).
fn material_ball() -> Mesh {
    let r = BUILTIN_SIZE;
    let mut m = Mesh::default();
    let (top, zc) = (0.12 * r, 1.22 * r);
    let full = (0.0, std::f32::consts::TAU);
    // Pedestal: top face outwards, rounded edge, side down | underside inwards.
    let k = 0.03 * r;
    let pedestal = [
        [
            vec![[0.0, top, 1.0, 0.0]],
            arc([0.75 * r - k, top - k], k, 90.0, 0.0, 4),
            vec![[0.75 * r, 0.0, 0.0, -1.0]],
        ]
        .concat(),
        line([0.75 * r, 0.0], [0.0, 0.0]),
    ];
    lathe(&mut m, &pedestal, &Z_UP, full, 64);
    let neck = [line([0.25 * r, zc - 0.9 * r], [0.25 * r, top])];
    lathe(&mut m, &neck, &Z_UP, full, 64);

    // Ball, in (radius, height) about `w`; `pol(ρ, θ)` = the point at distance ρ, θ degrees from `w`.
    let (s15, c15) = 15f32.to_radians().sin_cos();
    let w = [-0.5 * c15, 0.75f32.sqrt() * c15, s15];
    let down = mad([0.0, 0.0, -1.0], s15, w);
    let x = down.map(|c| c / c15);
    let ball = Frame {
        o: [0.0, 0.0, zc],
        x,
        y: cross(w, x),
        z: w,
    };
    let pol = |rho: f32, t: f32| {
        let (s, c) = t.to_radians().sin_cos();
        [rho * s, rho * c]
    };
    let rim = pol(0.88 * r, 38.0);
    let (sr, cr) = 38f32.to_radians().sin_cos();
    // Lip: fillet of radius l between the rim wall (radial at 38°) and the outer sphere.
    let l = 0.03 * r;
    let tc = 38.0 + (l / (r - l)).asin().to_degrees();
    let profile = [
        arc([0.0; 2], 0.88 * r, 90.0, 52.0, 10),
        [
            vec![[rim[0], rim[1], sr, cr]],
            arc(pol(r - l, tc), l, 142.0, 90.0 - tc, 2),
            arc([0.0; 2], r, 90.0 - tc, -15.0, 16)[1..].to_vec(),
        ]
        .concat(),
        line(pol(r, 105.0), pol(0.92 * r, 105.0)),
        arc([0.0; 2], 0.92 * r, -15.0, -21.0, 2),
        line(pol(0.92 * r, 111.0), pol(r, 111.0)),
        arc([0.0; 2], r, -21.0, -90.0, 17),
    ];
    lathe(&mut m, &profile, &ball, full, 64);

    // C-ring: rounded rectangle walked clockwise (top, outer wall, bottom, inner wall), closed.
    let (r0, r1, h, k) = (0.96 * r, 1.14 * r, 0.28 * r, 0.04 * r);
    let mut sec = [
        arc([r1 - k, h - k], k, 90.0, 0.0, 4),
        arc([r1 - k, k], k, 0.0, -90.0, 4),
        arc([r0 + k, k], k, -90.0, -180.0, 4),
        arc([r0 + k, h - k], k, 180.0, 90.0, 4),
    ]
    .concat();
    sec.push(sec[0]);
    let (a0, a1) = (145f32.to_radians(), 395f32.to_radians());
    lathe(&mut m, &[sec.clone()], &Z_UP, (a0, a1), 45);
    for (phi, sign) in [(a0, -1.0), (a1, 1.0)] {
        let base = m.positions.len() as u16;
        let (sn, cs) = phi.sin_cos();
        let (rho, around) = ([cs, sn, 0.0], [-sn, cs, 0.0]);
        let n = sec.len() - 1;
        for p in &sec[..n] {
            m.positions.push(mad(rho.map(|c| c * p[0]), p[1], Z_UP.z));
            m.normals.push(around.map(|c| c * sign));
            m.tangents.push([0.0, 0.0, -1.0]);
            m.bitangents.push(rho.map(|c| -c * sign));
            m.uvs.push(
                [0.5 - sign * (p[0] - 1.05 * r) / (r1 - r0), (h - p[1]) / h]
                    .map(|c| c.clamp(0.0, 1.0)),
            );
        }
        for i in 1..n as u16 - 1 {
            let (b, c) = (base + i, base + i + 1);
            m.triangles.push(if sign > 0.0 {
                [base, b, c]
            } else {
                [base, c, b]
            });
        }
    }
    m
}
