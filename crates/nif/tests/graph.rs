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

#[test]
fn a_ring_longer_than_the_depth_cap_is_cut_at_its_closing_edge() {
    let mut f = Nif::new(100);
    for i in 0..258i32 {
        f.block("NiNode", node([1.0, 0.0, 0.0], &[(i + 1) % 258]));
    }
    let g = graph(&f);
    let cycles = g
        .problems
        .iter()
        .filter(|p| p.contains("cycle"))
        .collect::<Vec<_>>();
    assert_eq!(cycles, ["block 257: child 0 would make a cycle"]);
    assert!(!g.parent.contains_key(&0));
    assert_eq!(world(&g, 0, &ID), ID);
}

// ---- PGPatcher 3D index (GUI suggestions item 6) ----

fn idx(f: &Nif) -> Vec<(u32, u32)> {
    let bytes = f.bytes();
    nif::graph::index3d(&Header::parse(&bytes).unwrap(), &bytes)
        .into_iter()
        .collect()
}

#[test]
fn shapes_are_numbered_in_tree_order_not_block_order() {
    let mut f = Nif::new(100);
    f.block("BSFadeNode", node([0.0; 3], &[3, 1])); // 0
    f.block("NiNode", node([0.0; 3], &[2])); // 1
    f.block("BSTriShape", vec![]); // 2
    f.block("BSTriShape", vec![]); // 3
    assert_eq!(idx(&f), [(2, 1), (3, 0)]);
}

#[test]
fn a_particle_system_takes_an_index() {
    let mut f = Nif::new(100);
    f.block("BSFadeNode", node([0.0; 3], &[1, 2, 3]));
    f.block("BSTriShape", vec![]);
    f.block("NiParticleSystem", vec![]);
    f.block("BSTriShape", vec![]);
    assert_eq!(idx(&f), [(1, 0), (2, 1), (3, 2)]);
}

#[test]
fn an_unreachable_shape_has_no_index() {
    let mut f = Nif::new(100);
    f.block("BSFadeNode", node([0.0; 3], &[1]));
    f.block("BSTriShape", vec![]);
    f.block("BSTriShape", vec![]); // nobody's child
    assert_eq!(idx(&f), [(1, 0)]);
}

#[test]
fn a_shared_shape_keeps_its_first_visit() {
    let mut f = Nif::new(100);
    f.block("BSFadeNode", node([0.0; 3], &[1, 3])); // 0
    f.block("NiNode", node([0.0; 3], &[2])); // 1
    f.block("BSTriShape", vec![]); // 2: under 1 and 3
    f.block("NiNode", node([0.0; 3], &[2, 4])); // 3
    f.block("BSTriShape", vec![]); // 4
    assert_eq!(idx(&f), [(2, 0), (4, 1)]);
}

#[test]
fn a_cycle_terminates_and_a_shape_below_max_depth_is_absent() {
    let mut f = Nif::new(100);
    f.block("BSFadeNode", node([0.0; 3], &[1])); // 0
    f.block("NiNode", node([0.0; 3], &[0, 2])); // 1: points back at the root
    f.block("BSTriShape", vec![]); // 2
    assert_eq!(idx(&f), [(2, 0)], "the cycle is walked once");
    let mut g = Nif::new(100);
    g.block("BSFadeNode", node([0.0; 3], &[1, 302]));
    for i in 1..=300 {
        g.block("NiNode", node([0.0; 3], &[i + 1])); // a chain 1 → 2 → … → 301
    }
    g.block("BSTriShape", vec![]); // 301, depth 301 > MAX_DEPTH
    g.block("BSTriShape", vec![]); // 302, a child of the root
    assert_eq!(idx(&g), [(302, 0)]);
}

#[test]
fn the_root_is_the_first_node_when_block_0_is_not_one() {
    let mut f = Nif::new(100);
    f.block("BSTriShape", vec![]); // 0: not a node, not under the root
    f.block("BSFadeNode", node([0.0; 3], &[2])); // 1
    f.block("BSTriShape", vec![]); // 2
    assert_eq!(idx(&f), [(2, 0)]);
    let mut g = Nif::new(100);
    g.block("BSTriShape", vec![]);
    assert!(idx(&g).is_empty(), "no node at all: no index");
}

/// NiObjectNET + NiAVObject prefix (no name, identity transform) shared by the bodies below.
fn av() -> W {
    W::default()
        .i32(-1)
        .u32(0)
        .i32(-1)
        .u32(14)
        .fs(&[0.0; 3])
        .fs(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0])
        .f(1.0)
        .i32(-1)
}

/// An NiParticleSystem body at `bsver` (nifly NiParticleSystem::Sync) with these modifiers and no data.
fn particles(bsver: u32, modifiers: &[i32]) -> Vec<u8> {
    let mut w = av();
    w = if bsver >= 100 {
        // bounds, skin, shader, alpha, vertex flags, far/near, psys data
        w.fs(&[0.0; 4])
            .i32(-1)
            .i32(-1)
            .i32(-1)
            .u64(0)
            .u16(0)
            .u16(0)
            .u16(0)
            .u16(0)
            .i32(-1)
    } else {
        // data, skin, 1 material (name + extra data), active material, needs update, shader, alpha, far/near
        w.i32(-1)
            .i32(-1)
            .u32(1)
            .i32(-1)
            .i32(-1)
            .i32(0)
            .u8(0)
            .i32(-1)
            .i32(-1)
            .u16(0)
            .u16(0)
            .u16(0)
            .u16(0)
    };
    w = w.u8(0).u32(modifiers.len() as u32);
    for m in modifiers {
        w = w.i32(*m);
    }
    w.0
}

/// NiPSysModifier (name, order, target, active) + `u32 n` + mesh refs.
fn mesh_update(meshes: &[i32]) -> Vec<u8> {
    let mut w = W::default()
        .i32(-1)
        .u32(0)
        .i32(-1)
        .u8(1)
        .u32(meshes.len() as u32);
    for m in meshes {
        w = w.i32(*m);
    }
    w.0
}

#[test]
fn a_shape_listed_by_a_particle_mesh_update_modifier_is_numbered_inside_the_particle_system() {
    for bsver in [100, 83] {
        let mut f = Nif::new(bsver);
        f.block("BSFadeNode", node([0.0; 3], &[1, 3])); // 0
        f.block("NiParticleSystem", particles(bsver, &[2])); // 1
        f.block("NiPSysMeshUpdateModifier", mesh_update(&[4])); // 2
        f.block("BSTriShape", vec![]); // 3: B, the root's second child
        f.block("NiTriShape", vec![]); // 4: A, only the modifier lists it
        assert_eq!(
            idx(&f),
            [(1, 0), (3, 2), (4, 1)],
            "nifly GetTree order at BSVER {bsver}"
        );
    }
}

#[test]
fn a_field_modifier_object_and_a_master_particle_system_are_followed() {
    let mut f = Nif::new(100);
    let master = W::default()
        .raw(&node([0.0; 3], &[]))
        .u16(20)
        .u32(1)
        .i32(1)
        .0;
    f.block("BSMasterParticleSystem", master); // 0: no children, one particle system
    f.block("NiParticleSystem", particles(100, &[2])); // 1
    let field = W::default()
        .i32(-1)
        .u32(0)
        .i32(-1)
        .u8(1)
        .i32(3)
        .f(1.0)
        .f(0.0)
        .u8(0)
        .f(0.0)
        .0;
    f.block("NiPSysGravityFieldModifier", field); // 2
    f.block("BSTriShape", vec![]); // 3
    assert_eq!(idx(&f), [(1, 0), (3, 1)]);
}

#[test]
fn bstreenode_bones_are_followed_after_children_and_effects() {
    let mut f = Nif::new(100);
    let tree = W::default()
        .raw(&node([0.0; 3], &[1]))
        .u32(1)
        .i32(2) // bones1
        .u32(1)
        .i32(4) // bones2
        .0;
    f.block("BSTreeNode", tree); // 0
    f.block("BSTriShape", vec![]); // 1: a child
    f.block("NiNode", node([0.0; 3], &[3])); // 2: a bones1 node
    f.block("BSTriShape", vec![]); // 3: only under a bone
    f.block("NiNode", node([0.0; 3], &[5])); // 4: a bones2 node
    f.block("BSTriShape", vec![]); // 5
    assert_eq!(idx(&f), [(1, 0), (3, 1), (5, 2)]);
}
