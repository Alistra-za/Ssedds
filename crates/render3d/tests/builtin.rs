//! Built-in sphere / plane / cube: geometry checks, the tangent convention measured on nifly's fixtures, and the
//! convention seen by a shader (WARP, stand-in `STANDIN_TBN`).
mod common;
use common::{channel, covered, dds_rgba8, fixture, renderer};
use nif::geom::Mesh;
use nif::material::MaterialState;
use render3d::scene::Scene;
use render3d::{builtin_mesh, parse, Builtin, ShapeId, ShapeStatus};

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn len(a: [f32; 3]) -> f32 {
    dot(a, a).sqrt()
}
fn unit(a: [f32; 3]) -> [f32; 3] {
    let l = len(a);
    [a[0] / l, a[1] / l, a[2] / l]
}

/// Means over triangles of the cosines between the first vertex's frame and the triangle's dP/du, dP/dv.
#[derive(Debug)]
struct Frame {
    t_dv: f64,
    b_du: f64,
    t_du_abs: f64,
    b_dv_abs: f64,
    handed: f64,
    n: usize,
}

fn frame_stats(meshes: &[&Mesh]) -> Frame {
    let mut s = [0f64; 5];
    let mut n = 0;
    for m in meshes {
        if m.tangents.is_empty() || m.uvs.is_empty() {
            continue;
        }
        for t in &m.triangles {
            let [i0, i1, i2] = t.map(|i| i as usize);
            let (e1, e2) = (
                sub(m.positions[i1], m.positions[i0]),
                sub(m.positions[i2], m.positions[i0]),
            );
            let (du1, dv1) = (m.uvs[i1][0] - m.uvs[i0][0], m.uvs[i1][1] - m.uvs[i0][1]);
            let (du2, dv2) = (m.uvs[i2][0] - m.uvs[i0][0], m.uvs[i2][1] - m.uvs[i0][1]);
            let det = du1 * dv2 - du2 * dv1;
            if det.abs() < 1e-8 {
                continue;
            }
            let dpdu = [0, 1, 2].map(|k| (e1[k] * dv2 - e2[k] * dv1) / det);
            let dpdv = [0, 1, 2].map(|k| (e2[k] * du1 - e1[k] * du2) / det);
            let (tg, bt) = (m.tangents[i0], m.bitangents[i0]);
            if len(dpdu) < 1e-6 || len(dpdv) < 1e-6 || len(tg) < 0.5 || len(bt) < 0.5 {
                continue;
            }
            let (dpdu, dpdv, tg, bt) = (unit(dpdu), unit(dpdv), unit(tg), unit(bt));
            s[0] += dot(tg, dpdv) as f64;
            s[1] += dot(bt, dpdu) as f64;
            s[2] += dot(tg, dpdu).abs() as f64;
            s[3] += dot(bt, dpdv).abs() as f64;
            s[4] += dot(cross(tg, bt), m.normals[i0]) as f64;
            n += 1;
        }
    }
    let k = n.max(1) as f64;
    Frame {
        t_dv: s[0] / k,
        b_du: s[1] / k,
        t_du_abs: s[2] / k,
        b_dv_abs: s[3] / k,
        handed: s[4] / k,
        n,
    }
}

/// 03 T-B2. Measured when this plan was written: t_dv 1.000, b_du 1.000, |t·du| 0.002, |b·dv| 0.004, handed 0.941.
#[test]
fn tangent_convention_from_fixture() {
    let p = parse(&fixture("TestNifFile_Furniture_Col_SE.nif"));
    let meshes: Vec<&Mesh> = p.shapes.iter().map(|s| &*s.mesh).collect();
    let f = frame_stats(&meshes);
    assert!(f.n > 100, "{f:?}");
    assert!(f.t_dv > 0.9, "Mesh.tangents run along +dP/dv: {f:?}");
    assert!(f.b_du > 0.9, "Mesh.bitangents run along +dP/du: {f:?}");
    assert!(f.t_du_abs < 0.1 && f.b_dv_abs < 0.1, "{f:?}");
    assert!(
        f.handed > 0.5,
        "cross(tangent, bitangent) points along N: {f:?}"
    );
}

#[test]
fn fixture_triangles_wind_around_their_normals() {
    for name in [
        "TestNifFile_Furniture_Col_SE.nif",
        "TestNifFile_DeepGraph_SE.nif",
    ] {
        for s in parse(&fixture(name)).shapes {
            let m = &s.mesh;
            for t in &m.triangles {
                let [a, b, c] = t.map(|i| i as usize);
                let cr = cross(
                    sub(m.positions[b], m.positions[a]),
                    sub(m.positions[c], m.positions[a]),
                );
                let n = [0, 1, 2].map(|k| m.normals[a][k] + m.normals[b][k] + m.normals[c][k]);
                assert!(dot(cr, n) > 0.0, "{name} {}: {t:?}", s.name);
            }
        }
    }
}

#[test]
fn builtins_follow_the_fixture_convention() {
    for b in [Builtin::Sphere, Builtin::Plane, Builtin::Cube] {
        let m = builtin_mesh(b);
        let f = frame_stats(&[&m]);
        assert!(f.n > 0, "{b:?}");
        assert!(f.t_dv > 0.95 && f.b_du > 0.95, "{b:?}: {f:?}");
        assert!(f.t_du_abs < 0.05 && f.b_dv_abs < 0.05, "{b:?}: {f:?}");
        assert!(f.handed > 0.99, "{b:?}: {f:?}");
    }
}

/// 03 T-B1.
#[test]
fn builtins_validate() {
    for b in [Builtin::Sphere, Builtin::Plane, Builtin::Cube] {
        let m = builtin_mesh(b);
        render3d::mesh::validate(&m).unwrap();
        assert!(m.colors.is_empty() && m.bone_weights.is_empty() && m.bone_indices.is_empty());
        assert!(m.eye.is_empty());
        assert_eq!(m.transform, Mesh::default().transform);
        assert!(m.positions.len() <= u16::MAX as usize);
        for i in 0..m.positions.len() {
            let (n, t, bt, uv) = (m.normals[i], m.tangents[i], m.bitangents[i], m.uvs[i]);
            for v in [n, t, bt] {
                assert!((len(v) - 1.0).abs() < 1e-4, "{b:?} vertex {i}: {v:?}");
            }
            assert!(dot(n, t).abs() < 1e-4 && dot(n, bt).abs() < 1e-4 && dot(t, bt).abs() < 1e-4);
            assert!((0.0..=1.0).contains(&uv[0]) && (0.0..=1.0).contains(&uv[1]));
        }
        for t in &m.triangles {
            let [a, bb, c] = t.map(|i| i as usize);
            let (pa, pb, pc) = (m.positions[a], m.positions[bb], m.positions[c]);
            let cr = cross(sub(pb, pa), sub(pc, pa));
            assert!(len(cr) > 1e-3, "{b:?}: degenerate {t:?}");
            let n = [0, 1, 2].map(|k| m.normals[a][k] + m.normals[bb][k] + m.normals[c][k]);
            assert!(dot(cr, n) > 0.0, "{b:?}: {t:?} winds clockwise");
            if b != Builtin::Plane {
                let centroid = [0, 1, 2].map(|k| (pa[k] + pb[k] + pc[k]) / 3.0);
                assert!(dot(cr, centroid) > 0.0, "{b:?}: {t:?} faces inwards");
            }
        }
    }
    assert_eq!(builtin_mesh(Builtin::Sphere).positions.len(), 65 * 33);
    assert_eq!(builtin_mesh(Builtin::Plane).positions.len(), 4);
    assert_eq!(builtin_mesh(Builtin::Cube).positions.len(), 24);
    // The plane faces the Auto camera (+Y).
    assert!(builtin_mesh(Builtin::Plane)
        .normals
        .iter()
        .all(|n| *n == [0.0, 1.0, 0.0]));
}

#[test]
fn load_builtin_is_shape_zero() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), &["STANDIN_OPAQUE_OK"]);
    let mut m = MaterialState::builtin_default();
    m.skinned = true; // geometry-bound: forced off for a built-in
    m.has_vertex_colors = true;
    let rep = r.load_builtin(Builtin::Sphere, m, &|_| None);
    assert!(
        rep.errors.is_empty() && rep.compile_errors.is_empty(),
        "{rep:?}"
    );
    assert_eq!(rep.shapes, 1);
    let s = r.shapes();
    assert_eq!(s.len(), 1);
    assert_eq!((s[0].id, s[0].name.as_str()), (ShapeId(0), "Sphere"));
    assert_eq!(s[0].status, ShapeStatus::Drawing);
    assert!(!s[0].material.skinned && !s[0].material.has_vertex_colors);
    let (c, radius) = r.bounds().unwrap();
    assert!(
        c.iter().all(|v| v.abs() < 1e-3) && (radius - 32.0).abs() < 1e-2,
        "{c:?} {radius}"
    );
    assert!(covered(&r.render(&Scene::neutral(), 64, 64).unwrap()) > 64 * 64 / 10);
    // Edits work on built-ins like on NIF shapes.
    let mut uv = s[0].material.clone();
    uv.params.uv_scale = [4.0, 4.0];
    assert_eq!(
        r.set_material(ShapeId(0), uv, &|_| None),
        Ok(render3d::EditKind::Cbuffer)
    );
}

/// 03 T-B3: a normal-map texel tilted towards tangent-space +x (+u) on the plane is lit by a light on the plane's
/// +u side and not by one on the −u side. The plane's +u is world −X (Builtin::Plane faces +Y).
#[test]
fn normal_map_tilt_follows_u_on_the_plane() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), &["STANDIN_OPAQUE_OK", "STANDIN_TBN"]);
    // (0.70, 0.00, 0.70) in tangent space.
    let tilt = dds_rgba8(1, 1, &[[217, 128, 217, 255]]);
    let resolve = |p: &str| (p == "textures/standin/tilt_n.dds").then(|| tilt.clone());
    let mut m = MaterialState::builtin_default();
    m.set_slot_raw(1, "textures\\standin\\tilt_n.dds");
    let rep = r.load_builtin(Builtin::Plane, m, &resolve);
    assert!(
        rep.missing_textures.is_empty() && rep.errors.is_empty(),
        "{rep:?}"
    );
    let lit_from = |r: &mut render3d::Renderer, d: [f32; 3]| {
        let mut s = Scene::neutral();
        s.dir_light.direction = d;
        channel(&r.render(&s, 64, 64).unwrap(), 0)
    };
    let h = std::f32::consts::FRAC_1_SQRT_2;
    let plus_u = lit_from(&mut r, [-h, h, 0.0]);
    let minus_u = lit_from(&mut r, [h, h, 0.0]);
    assert!(plus_u > 4 * minus_u.max(1), "+u {plus_u} vs −u {minus_u}");
    // Control: with the flat default normal map both lights give the same picture (within 2 %).
    let rep = r.load_builtin(Builtin::Plane, MaterialState::builtin_default(), &|_| None);
    assert_eq!(rep.shapes, 1);
    let (a, b) = (
        lit_from(&mut r, [-h, h, 0.0]),
        lit_from(&mut r, [h, h, 0.0]),
    );
    assert!((a as f64 - b as f64).abs() < 0.02 * a as f64, "{a} vs {b}");
}
