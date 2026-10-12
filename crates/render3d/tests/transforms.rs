//! Parent NiNode transforms (P1.1): ParsedShape.world, the bounds and the draw's model matrix.
#![allow(clippy::needless_range_loop)]
mod common;
use common::{fixture, renderer, FLAT};
use nif::graph::{read_graph, world};
use nif::header::Header;
use render3d::{parse, ParsedNif, ParsedShape, ShapeId};

type M = [[f32; 4]; 3];
const ID: M = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];

fn mul(a: &M, b: &M) -> M {
    let mut r = [[0.0f32; 4]; 3];
    for (i, row) in r.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            let t = if j == 3 { a[i][3] } else { 0.0 };
            *v = (0..3).map(|k| a[i][k] * b[k][j]).sum::<f32>() + t;
        }
    }
    r
}

/// Linear part within `lin`, translation column within `tr`.
fn close(a: &M, b: &M, lin: f32, tr: f32) {
    for i in 0..3 {
        for j in 0..4 {
            let tol = if j == 3 { tr } else { lin };
            assert!(
                (a[i][j] - b[i][j]).abs() <= tol,
                "[{i}][{j}] {a:?} vs {b:?}"
            );
        }
    }
}

fn graph_of(bytes: &[u8]) -> nif::graph::NodeGraph {
    read_graph(&Header::parse(bytes).unwrap(), bytes)
}

/// Bounding sphere of `positions × m(shape)` over the shapes (centre = box centre, the renderer's rule).
fn sphere(p: &ParsedNif, m: impl Fn(&ParsedShape) -> M) -> ([f32; 3], f32) {
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    let mut pts = vec![];
    for s in &p.shapes {
        assert!(!s.material.skinned, "{}", s.name);
        let t = m(s);
        for v in &s.mesh.positions {
            let q = [0, 1, 2].map(|r| t[r][0] * v[0] + t[r][1] * v[1] + t[r][2] * v[2] + t[r][3]);
            for k in 0..3 {
                lo[k] = lo[k].min(q[k]);
                hi[k] = hi[k].max(q[k]);
            }
            pts.push(q);
        }
    }
    let c = [0, 1, 2].map(|k| (lo[k] + hi[k]) / 2.0);
    let r = pts
        .iter()
        .map(|p| ((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2) + (p[2] - c[2]).powi(2)).sqrt())
        .fold(0.0f32, f32::max);
    (c, r)
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|k| (a[k] - b[k]).powi(2)).sum::<f32>().sqrt()
}

#[test]
fn deep_graph_every_lighting_shape_chains_to_block_zero() {
    let b = fixture("TestNifFile_DeepGraph_SE.nif");
    let p = parse(&b);
    let g = graph_of(&b);
    assert!(g.problems.is_empty(), "{:?}", g.problems);
    assert_eq!(p.shapes.len(), 27);
    assert!(
        p.warnings
            .iter()
            .all(|w| !w.contains("not under any known node")),
        "{:?}",
        p.warnings
    );
    for s in &p.shapes {
        let (mut cur, mut hops) = (s.id.0, 0);
        while let Some(&up) = g.parent.get(&cur) {
            cur = up;
            hops += 1;
            assert!(hops <= 256, "{}", s.name);
        }
        assert!(cur == 0 && hops > 0, "{} must hang under block 0", s.name);
    }
}

/// Golden: world matrix of node 77 (SkullPeg01, the parent of shape 143) = root · … · node 77, computed outside
/// Rust from the fixture's 14 node blocks in float64. The rotations cancel to the identity; the translation is
/// (48.22637, -0.15830, 327.96471). Composing in the wrong order gives translation (5.8, 227.7, 360.1).
const W77: M = [
    [1.0, 0.0, 0.0, 48.226_37],
    [0.0, 1.0, 0.0, -0.158_298_83],
    [0.0, 0.0, 1.0, 327.964_7],
];

#[test]
fn deep_graph_skull_peg_world_matches_the_chain_golden() {
    let b = fixture("TestNifFile_DeepGraph_SE.nif");
    let p = parse(&b);
    let g = graph_of(&b);
    assert_eq!(g.parent[&143], 77);
    close(&world(&g, 143, &ID), &W77, 1e-4, 0.02);
    let s = p.shapes.iter().find(|s| s.id == ShapeId(143)).unwrap();
    close(&s.world, &mul(&W77, &s.mesh.transform), 1e-3, 0.05);
    assert_ne!(
        s.world, s.mesh.transform,
        "the local transform alone is the P1-A placement"
    );
}

#[test]
fn deep_graph_bounds_follow_the_world_transforms() {
    let b = fixture("TestNifFile_DeepGraph_SE.nif");
    let p = parse(&b);
    let g = graph_of(&b);
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), FLAT);
    let rep = r.load(&b, &|_| None);
    assert_eq!(rep.shapes, 27, "{rep:?}");
    let (c, rad) = r.bounds().unwrap();
    let (wc, wr) = sphere(&p, |s| s.world);
    let (lc, _) = sphere(&p, |s| s.mesh.transform);
    // the chain composed child-first instead of root-first: what a wrong composition order would give
    let (rc, _) = sphere(&p, |s| {
        let (mut m, mut cur) = (s.mesh.transform, s.id.0);
        while let Some(&up) = g.parent.get(&cur) {
            m = mul(&m, &g.local[&up]);
            cur = up;
        }
        m
    });
    assert!(dist(c, wc) < 0.01, "centre {c:?} vs {wc:?}");
    assert!((rad - wr).abs() < 0.01 * wr.max(1.0), "{rad} vs {wr}");
    assert!(
        dist(c, lc) > 1.0,
        "bounds must differ from the local-only P1-A bounds"
    );
    assert!(
        dist(c, rc) > 1.0,
        "bounds must differ from the reverse-order composition"
    );
}

/// Furniture: node 3 "Furniture" is the only non-identity node, a pure +85.12416 translation in y above shapes 9, 12.
#[test]
fn furniture_shapes_sit_under_the_raised_node() {
    let b = fixture("TestNifFile_Furniture_Col_SE.nif");
    let p = parse(&b);
    let g = graph_of(&b);
    assert_eq!((g.parent[&9], g.parent[&12], g.parent[&3]), (3, 3, 0));
    assert_eq!(g.local[&3][1][3], 85.124_16);
    assert_eq!(p.shapes.len(), 2);
    for s in &p.shapes {
        let mut want = s.mesh.transform;
        want[1][3] += 85.124_16;
        close(&s.world, &want, 1e-6, 1e-3);
    }
    // bounds: pure translation, so the centre moves by 85.12416 in y and the radius does not change
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), FLAT);
    assert_eq!(r.load(&b, &|_| None).shapes, 2);
    let (c, rad) = r.bounds().unwrap();
    let (lc, lr) = sphere(&p, |s| s.mesh.transform);
    assert!(
        (c[1] - lc[1] - 85.124_16).abs() < 0.01,
        "{c:?} vs local {lc:?}"
    );
    assert!((c[0] - lc[0]).abs() < 0.01 && (c[2] - lc[2]).abs() < 0.01);
    assert!((rad - lr).abs() < 0.01 * lr.max(1.0), "{rad} vs {lr}");
}

/// Identity node chains leave world == local exactly, so these fixtures draw as before (the skinned VS ignores
/// World anyway).
#[test]
fn identity_chains_leave_world_equal_to_local() {
    let mut n = 0;
    for f in [
        "TestNifFile_Skinned_SE.nif",
        "TestNifFile_Skinned_Dynamic_SE.nif",
        "TestNifFile_Optimize_LE_to_SE.nif",
    ] {
        for s in parse(&fixture(f)).shapes {
            assert_eq!(s.world, s.mesh.transform, "{f} {}", s.name);
            n += 1;
        }
    }
    assert!(n > 0);
}

/// Unknown node types are reported, not fatal: rename NiNode to NiNodX (same length) so block 3 is no longer a
/// known node; its shapes lose their parent, keep their own transform and are warned about.
#[test]
fn unknown_node_type_unparents_its_shapes_with_a_warning() {
    let mut b = fixture("TestNifFile_Furniture_Col_SE.nif");
    let pat = *b"\x06\0\0\0NiNode";
    let at: Vec<usize> = (0..b.len() - pat.len())
        .filter(|&i| b[i..i + pat.len()] == pat)
        .collect();
    assert_eq!(at.len(), 1, "the type table names NiNode once");
    b[at[0] + pat.len() - 1] = b'X';
    let p = parse(&b);
    assert!(p.errors.is_empty(), "{:?}", p.errors);
    assert_eq!(p.shapes.len(), 2);
    for s in &p.shapes {
        assert_eq!(s.world, s.mesh.transform, "{}", s.name);
    }
    let orphan: Vec<&String> = p
        .warnings
        .iter()
        .filter(|w| w.contains("not under any known node"))
        .collect();
    assert_eq!(
        orphan,
        [
            "cylinder_1: not under any known node; placed by its own transform",
            "cylinder_2: not under any known node; placed by its own transform"
        ]
    );
}
