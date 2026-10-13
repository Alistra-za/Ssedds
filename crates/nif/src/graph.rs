//! Parent NiNode transforms for the 3D preview (03 §4). Only each node's children list is read: subclass
//! fields come after NiNode's children and effects lists, so `NiObjectNET + NiAVObject + u32 n + n refs` is
//! valid for every type in [`NODE_TYPES`]. Nothing here fails a file: problems are strings, a bad link is
//! skipped.
use crate::geom::av::read_av_head;
use crate::geom::skin_partition::block_cursor;
use crate::header::Header;
use crate::NifError;
use std::collections::HashMap;

pub type Mat3x4 = [[f32; 4]; 3];
pub const IDENTITY: Mat3x4 = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];
/// Longest parent chain `world` composes (the nearest 256 ancestors) and `read_graph` accepts without a problem.
pub const MAX_DEPTH: usize = 256;

/// `NiNode` and every type that inherits from it, transitively (nif.xml `inherit="NiNode"` closure, sorted).
/// `node_types_match_nifxml_closure` re-derives it.
pub const NODE_TYPES: &[&str] = &[
    "AvoidNode",
    "BSBlastNode",
    "BSDamageStage",
    "BSDebrisNode",
    "BSDistantObjectInstancedNode",
    "BSFadeNode",
    "BSLeafAnimNode",
    "BSMasterParticleSystem",
    "BSMultiBoundNode",
    "BSOrderedNode",
    "BSRangeNode",
    "BSTreeNode",
    "BSValueNode",
    "CsNiNode",
    "FxButton",
    "FxRadioButton",
    "FxWidget",
    "JPSJigsawNode",
    "NiBSAnimationNode",
    "NiBSParticleNode",
    "NiBillboardNode",
    "NiBone",
    "NiCollisionSwitch",
    "NiLODNode",
    "NiNode",
    "NiRoom",
    "NiRoomGroup",
    "NiSortAdjustNode",
    "NiSwitchNode",
    "NiWall",
    "RootCollisionNode",
];

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NodeGraph {
    /// Child block -> parent node block (first parent wins).
    pub parent: HashMap<u32, u32>,
    /// Node block -> local transform (`Mesh.transform` convention).
    pub local: HashMap<u32, Mat3x4>,
    /// Out-of-range child refs, extra parents, cycles, unreadable nodes, depth over 256; one line each.
    pub problems: Vec<String>,
}

/// `a · b` for 3×4 affine matrices (the implicit fourth row is 0 0 0 1).
fn mul(a: &Mat3x4, b: &Mat3x4) -> Mat3x4 {
    let mut r = [[0.0f32; 4]; 3];
    for (i, row) in r.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            let t = if j == 3 { a[i][3] } else { 0.0 };
            *v = (0..3).map(|k| a[i][k] * b[k][j]).sum::<f32>() + t;
        }
    }
    r
}

fn children(h: &Header, b: &[u8], block: usize) -> Result<(Mat3x4, Vec<i32>), NifError> {
    let mut c = block_cursor(h, b, block)?;
    let av = read_av_head(h, &mut c)?;
    let n = c.u32()? as usize;
    if n.saturating_mul(4) > c.remaining() {
        return Err(NifError::Truncated {
            at: c.pos(),
            need: n.saturating_mul(4),
        });
    }
    let kids = (0..n).map(|_| c.i32()).collect::<Result<Vec<_>, _>>()?;
    Ok((av.transform, kids))
}

/// True when `target` is `from` or one of its first `MAX_DEPTH + 2` ancestors. `parent` is acyclic (every insert is
/// checked), so the walk ends; the cap keeps a long chain linear. A cycle closing only beyond the cap is not caught,
/// which is harmless: `world` and `depth` both stop at `MAX_DEPTH`.
fn reaches(parent: &HashMap<u32, u32>, from: u32, target: u32) -> bool {
    reaches_counted(parent, from, target).0
}

/// [`reaches`] and the number of parent lookups it made.
fn reaches_counted(parent: &HashMap<u32, u32>, from: u32, target: u32) -> (bool, usize) {
    let mut cur = from;
    let mut lookups = 0;
    for _ in 0..=MAX_DEPTH + 2 {
        if cur == target {
            return (true, lookups);
        }
        lookups += 1;
        match parent.get(&cur) {
            Some(&p) => cur = p,
            None => return (false, lookups),
        }
    }
    (false, lookups)
}

fn depth(parent: &HashMap<u32, u32>, mut b: u32) -> usize {
    let mut d = 0;
    while let Some(&p) = parent.get(&b) {
        d += 1;
        b = p;
        if d > MAX_DEPTH + 1 {
            break;
        }
    }
    d
}

/// Never fails. Walks every block whose type is in [`NODE_TYPES`], in block order.
pub fn read_graph(header: &Header, bytes: &[u8]) -> NodeGraph {
    let mut g = NodeGraph::default();
    let total = header.sizes.len();
    for i in 0..total {
        let kind = header.kind(i);
        if !NODE_TYPES.iter().any(|t| t.as_bytes() == kind) {
            continue;
        }
        let (local, kids) = match children(header, bytes, i) {
            Ok(x) => x,
            Err(e) => {
                g.problems.push(format!("block {i}: {e}"));
                continue;
            }
        };
        let node = i as u32;
        g.local.insert(node, local);
        for k in kids {
            if k == -1 {
                continue;
            }
            let Some(c) = usize::try_from(k).ok().filter(|&c| c < total) else {
                g.problems
                    .push(format!("block {i}: child ref {k} out of range"));
                continue;
            };
            let c = c as u32;
            if let Some(p) = g.parent.get(&c) {
                g.problems
                    .push(format!("block {i}: child {c} already has parent {p}"));
            } else if reaches(&g.parent, node, c) {
                g.problems
                    .push(format!("block {i}: child {c} would make a cycle"));
            } else {
                g.parent.insert(c, node);
            }
        }
    }
    let mut keys: Vec<u32> = g.parent.keys().copied().collect();
    keys.sort_unstable();
    for b in keys {
        if depth(&g.parent, b) == MAX_DEPTH + 1 {
            g.problems.push(format!(
                "block {b}: deeper than {MAX_DEPTH} nodes; the ancestors above {MAX_DEPTH} are ignored"
            ));
        }
    }
    g
}

/// The shape's world transform: root … parent chain × `shape_local`, root node included (as NifSkope does).
/// A block with no parent entry keeps `shape_local`. Composes at most [`MAX_DEPTH`] ancestors, so a cycle
/// built by hand still terminates.
pub fn world(g: &NodeGraph, shape_block: u32, shape_local: &Mat3x4) -> Mat3x4 {
    let (mut m, mut cur) = (*shape_local, shape_block);
    for _ in 0..MAX_DEPTH {
        let Some(&p) = g.parent.get(&cur) else { break };
        if let Some(l) = g.local.get(&p) {
            m = mul(l, &m);
        }
        cur = p;
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// 03 T-T1. Root: scale 2 + translate (10,0,0). A: rotate 90° about z (x -> y) + translate (0,5,0).
    /// Shape: translate (1,0,0). World = Root · A · Shape: shape point (0,0,0) -> (1,0,0) -> (0,1,0)+(0,5,0)
    /// = (0,6,0) -> (2·0+10, 2·6, 0) = (10,12,0).
    #[test]
    fn world_composes_parent_chain() {
        let root = [
            [2.0, 0.0, 0.0, 10.0],
            [0.0, 2.0, 0.0, 0.0],
            [0.0, 0.0, 2.0, 0.0],
        ];
        let a = [
            [0.0, -1.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 5.0],
            [0.0, 0.0, 1.0, 0.0],
        ];
        let shape = [
            [1.0, 0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ];
        let g = NodeGraph {
            parent: HashMap::from([(1, 0), (2, 1)]),
            local: HashMap::from([(0, root), (1, a)]),
            problems: vec![],
        };
        let want = [
            [0.0, -2.0, 0.0, 10.0],
            [2.0, 0.0, 0.0, 12.0],
            [0.0, 0.0, 2.0, 0.0],
        ];
        assert_eq!(world(&g, 2, &shape), want);
        // the wrong (child-first) order is a different matrix, so the test pins the order
        assert_ne!(mul(&mul(&shape, &a), &root), want);
        // a block with no parent keeps its own transform; the root node is included for its children
        assert_eq!(world(&g, 7, &shape), shape);
        assert_eq!(
            world(&g, 1, &IDENTITY),
            root,
            "block 1: only its parent (the root node) is composed"
        );
    }

    /// A 50k-node chain: the uncapped walk costs N^2/2 lookups (many seconds), the capped one N * 258. Counts lookups, so
    /// the check does not depend on the clock.
    #[test]
    fn cycle_check_is_linear_on_a_long_chain() {
        let mut parent = HashMap::new();
        let mut total = 0;
        for i in 1..50_000u32 {
            let (hit, n) = reaches_counted(&parent, i - 1, i);
            assert!(!hit);
            assert!(n <= MAX_DEPTH + 3, "{n} lookups at {i}");
            total += n;
            parent.insert(i, i - 1);
        }
        assert!(total < 50_000 * (MAX_DEPTH + 3), "{total}");
    }

    #[test]
    fn world_terminates_on_a_hand_built_cycle() {
        let t = [
            [1.0, 0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ];
        let g = NodeGraph {
            parent: HashMap::from([(1, 2), (2, 1)]),
            local: HashMap::from([(1, t), (2, t)]),
            problems: vec![],
        };
        assert_eq!(world(&g, 1, &IDENTITY)[0][3], MAX_DEPTH as f32);
    }

    /// 03 T-T1. The list is the closure of `inherit="NiNode"` over the committed nif.xml inheritance list
    /// (`tests/fixtures/nifxml_inherit.txt`, regenerated from nif.xml with the command in the P1-B plan).
    #[test]
    fn node_types_match_nifxml_closure() {
        let mut inherit: HashMap<&str, &str> = HashMap::new();
        for l in include_str!("../tests/fixtures/nifxml_inherit.txt").lines() {
            let (n, p) = l.split_once(' ').unwrap();
            inherit.insert(n, p);
        }
        let is_node = |mut n: &str| loop {
            if n == "NiNode" {
                return true;
            }
            match inherit.get(n) {
                Some(p) => n = p,
                None => return false,
            }
        };
        let mut want: Vec<&str> = inherit.keys().copied().filter(|n| is_node(n)).collect();
        want.sort_unstable();
        assert_eq!(want, NODE_TYPES, "regenerate NODE_TYPES from nif.xml");
    }
}
