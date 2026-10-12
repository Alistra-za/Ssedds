// read_graph on synthetic NIFs: links, problems, caps. Never an Err.
mod builder;
use builder::{Nif, W};
use nif::graph::{read_graph, world};
use nif::header::Header;

const ID: [[f32; 4]; 3] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];

/// A NiNode-shaped body (BSVER 100): av prefix with translation `t` and identity rotation, children, 0 effects.
fn node(t: [f32; 3], kids: &[i32]) -> Vec<u8> {
    let mut w = W::default()
        .i32(-1)
        .u32(0)
        .i32(-1)
        .u32(14)
        .fs(&t)
        .fs(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0])
        .f(1.0)
        .i32(-1)
        .u32(kids.len() as u32);
    for k in kids {
        w = w.i32(*k);
    }
    w.u32(0).0
}

fn graph(f: &Nif) -> nif::graph::NodeGraph {
    let bytes = f.bytes();
    read_graph(&Header::parse(&bytes).unwrap(), &bytes)
}

#[test]
fn links_children_and_reads_locals_for_every_node_subclass() {
    let mut f = Nif::new(100);
    f.block("BSFadeNode", node([1.0, 2.0, 3.0], &[1, 2]));
    f.block("NiNode", node([0.0, 5.0, 0.0], &[3]));
    f.block("BSOrderedNode", node([0.0, 0.0, 0.0], &[]));
    f.block("BSTriShape", vec![]); // not a node: never read
    let g = graph(&f);
    assert!(g.problems.is_empty(), "{:?}", g.problems);
    assert_eq!(g.parent.len(), 3);
    assert_eq!((g.parent[&1], g.parent[&2], g.parent[&3]), (0, 0, 1));
    assert_eq!(
        g.local[&1],
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 5.0],
            [0.0, 0.0, 1.0, 0.0]
        ]
    );
    assert_eq!(g.local[&0][2][3], 3.0);
    assert!(!g.local.contains_key(&3));
    assert_eq!(world(&g, 3, &ID)[1][3], 7.0, "5 (NiNode) + 2 (root)");
}

#[test]
fn unknown_type_children_stay_unparented_without_a_problem() {
    let mut f = Nif::new(100);
    f.block("BSFadeNode", node([0.0; 3], &[1]));
    f.block("FooNode", node([0.0; 3], &[2]));
    f.block("BSTriShape", vec![]);
    let g = graph(&f);
    assert!(g.problems.is_empty(), "{:?}", g.problems);
    assert_eq!(g.parent.len(), 1);
    assert!(!g.parent.contains_key(&2));
}

#[test]
fn bad_refs_are_problems_not_errors() {
    let mut f = Nif::new(100);
    f.block("NiNode", node([0.0; 3], &[5, 1, -1])); // 5 is out of range, -1 is none
    f.block("NiNode", node([0.0; 3], &[]));
    f.block("NiNode", node([0.0; 3], &[1])); // block 1 already has parent 0
    let g = graph(&f);
    assert_eq!(g.parent.len(), 1);
    assert_eq!(g.parent[&1], 0, "first parent wins");
    assert_eq!(
        g.problems,
        [
            "block 0: child ref 5 out of range",
            "block 2: child 1 already has parent 0"
        ]
    );
}

#[test]
fn cycles_are_cut_and_reported() {
    let mut f = Nif::new(100);
    f.block("NiNode", node([0.0; 3], &[1, 3]));
    f.block("NiNode", node([0.0; 3], &[0])); // 0 -> 1 -> 0
    f.block("NiNode", node([0.0; 3], &[2])); // itself, no other parent
    f.block("NiNode", node([0.0; 3], &[]));
    let g = graph(&f);
    assert_eq!(g.parent.len(), 2);
    assert_eq!(
        g.problems,
        [
            "block 1: child 0 would make a cycle",
            "block 2: child 2 would make a cycle"
        ]
    );
    assert_eq!(world(&g, 2, &ID), ID);
}

#[test]
fn a_truncated_node_is_a_problem_and_a_huge_child_count_does_not_allocate() {
    let mut f = Nif::new(100);
    f.block("NiNode", node([0.0; 3], &[1]));
    f.block("NiNode", W::default().i32(-1).u32(0).0); // ends inside the av prefix
    let mut huge = node([0.0; 3], &[]);
    let n = huge.len();
    huge[n - 8..n - 4].copy_from_slice(&u32::MAX.to_le_bytes()); // child count, before the effects count
    f.block("NiNode", huge);
    let g = graph(&f);
    assert_eq!(g.problems.len(), 2, "{:?}", g.problems);
    assert!(g.problems[0].starts_with("block 1: "), "{:?}", g.problems);
    assert!(g.problems[1].starts_with("block 2: "), "{:?}", g.problems);
    assert_eq!(g.parent[&1], 0);
}

#[test]
fn depth_over_256_is_reported_once_and_world_stops_at_256_ancestors() {
    let mut f = Nif::new(100);
    for i in 0..300i32 {
        let kids: &[i32] = if i < 299 { &[i + 1] } else { &[] };
        f.block("NiNode", node([1.0, 0.0, 0.0], kids));
    }
    let g = graph(&f);
    assert_eq!(g.problems.len(), 1, "{:?}", g.problems);
    assert!(g.problems[0].starts_with("block 257: "), "{:?}", g.problems);
    assert_eq!(world(&g, 299, &ID)[0][3], 256.0);
}
