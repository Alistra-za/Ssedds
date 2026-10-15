//! Parent NiNode transforms for the 3D preview (03 §4). Only each node's children list is read: subclass
//! fields come after NiNode's children and effects lists, so `NiObjectNET + NiAVObject + u32 n + n refs` is
//! valid for every type in [`NODE_TYPES`]. Nothing here fails a file: problems are strings, a bad link is
//! skipped.
use crate::cursor::Cursor;
use crate::geom::av::read_av_head;
use crate::geom::skin_partition::block_cursor;
use crate::header::Header;
use crate::shape_head::ShapeKind;
use crate::NifError;
use std::collections::{BTreeMap, HashMap, HashSet};

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

/// A block that takes PGPatcher's next 3D index: every [`ShapeKind`] (nifly's NiShape family plus the two particle
/// systems sdc scans) and `NiMeshParticleSystem` (a nif.xml NiParticleSystem sdc does not scan; PGPatcher counts it).
pub(crate) fn takes_index(kind: &[u8]) -> bool {
    ShapeKind::from_name(kind).is_some() || kind == b"NiMeshParticleSystem"
}

/// nif.xml closure of NiPSysMeshUpdateModifier: NiPSysModifier, then `u32 n` + `n` NiAVObject refs.
const MESH_UPDATE_MODIFIERS: &[&str] = &["BSPSysHavokUpdateModifier", "NiPSysMeshUpdateModifier"];
/// nif.xml closure of NiPSysFieldModifier: NiPSysModifier, then the Field Object ref.
const FIELD_MODIFIERS: &[&str] = &[
    "NiPSysAirFieldModifier",
    "NiPSysDragFieldModifier",
    "NiPSysFieldModifier",
    "NiPSysGravityFieldModifier",
    "NiPSysRadialFieldModifier",
    "NiPSysTurbulenceFieldModifier",
    "NiPSysVortexFieldModifier",
];

fn refs(c: &mut Cursor, n: usize) -> Result<Vec<i32>, NifError> {
    if n.saturating_mul(4) > c.remaining() {
        return Err(NifError::Truncated {
            at: c.pos(),
            need: n.saturating_mul(4),
        });
    }
    (0..n).map(|_| c.i32()).collect()
}

/// The refs of `block` that nifly's `GetChildIndices` lists and that can lead to an NiAVObject, in nifly's order
/// (NiObjectNET/NiAVObject refs first in nifly, but extra data, controllers and collision never reach one):
/// node `children` + `effects` (+ BSTreeNode `bones1`/`bones2`, + BSMasterParticleSystem `particleSysRefs`); a
/// particle system's data and modifiers; a mesh-update modifier's meshes (+ BSPSysHavokUpdateModifier's
/// `modifierRef`); a field modifier's field object; NiMeshPSysData's `nodeRef`; NiCamera's scene. A block that fails
/// to read part-way keeps the refs read before the fault.
fn av_refs(h: &Header, b: &[u8], block: usize) -> Vec<i32> {
    let mut out = vec![];
    let _ = av_refs_into(h, b, block, &mut out);
    out
}

fn av_refs_into(h: &Header, b: &[u8], block: usize, out: &mut Vec<i32>) -> Result<(), NifError> {
    let kind = h.kind(block);
    let is = |names: &[&str]| names.iter().any(|t| t.as_bytes() == kind);
    let mut c = block_cursor(h, b, block)?;
    if is(NODE_TYPES) {
        read_av_head(h, &mut c)?;
        for _ in 0..2 {
            // children, then effects (both present below FO4)
            let n = c.u32()? as usize;
            out.extend(refs(&mut c, n)?);
        }
        if kind == b"BSTreeNode" {
            for _ in 0..2 {
                let n = c.u32()? as usize;
                out.extend(refs(&mut c, n)?);
            }
        } else if kind == b"BSMasterParticleSystem" {
            c.u16()?; // max emitter objects
            let n = c.u32()? as usize;
            out.extend(refs(&mut c, n)?);
        }
    } else if takes_index(kind) && kind.ends_with(b"ParticleSystem") {
        // nifly NiParticleSystem::Sync; dataRef and psysDataRef are one ref in Skyrim, listed first.
        read_av_head(h, &mut c)?;
        let data;
        if h.bsver >= 100 {
            c.skip(16)?; // bounds
            c.skip(12)?; // skin, shader, alpha
            c.skip(8)?; // vertex flags
            c.skip(8)?; // far/near begin/end
            data = c.i32()?;
        } else {
            data = c.i32()?;
            c.i32()?; // skin
            let m = c.u32()? as usize;
            c.skip(m.saturating_mul(8))?; // material names + extra data
            c.skip(4 + 1 + 8)?; // active material, needs update, shader, alpha
            c.skip(8)?; // far/near begin/end
        }
        out.push(data);
        c.u8()?; // world space
        let n = c.u32()? as usize;
        out.extend(refs(&mut c, n)?);
    } else if is(MESH_UPDATE_MODIFIERS) || is(FIELD_MODIFIERS) {
        c.skip(4 + 4 + 4 + 1)?; // NiPSysModifier: name, order, target, active
        if is(FIELD_MODIFIERS) {
            out.push(c.i32()?);
        } else {
            let n = c.u32()? as usize;
            out.extend(refs(&mut c, n)?);
            if kind == b"BSPSysHavokUpdateModifier" {
                out.push(c.i32()?); // modifierRef, after the nodes
            }
        }
    } else if kind == b"NiMeshPSysData" {
        // "Particle Meshes" is the block's last field (nif.xml, nifly NiMeshPSysData::Sync), after the
        // variable-length NiPSysData part, so it is read from the block's end.
        let size = h.sizes[block] as usize;
        if size >= 4 {
            c.skip(size - 4)?;
            out.push(c.i32()?);
        }
    } else if kind == b"NiCamera" {
        read_av_head(h, &mut c)?;
        c.skip(2 + 24 + 1 + 16 + 4)?; // flags, frustum, ortho, viewport, LOD adjust
        out.push(c.i32()?);
    }
    Ok(())
}

/// PGPatcher's 3D index (PGNIFUtil::shapesWith3DIdx over nifly's GetTree): a pre-order walk from the root over
/// [`av_refs`], first visit wins, iterative, blocks deeper than [`MAX_DEPTH`] not expanded. Every block
/// [`takes_index`] takes the next index (a particle system too: PGPatcher increments for it), then is expanded like
/// any other block (a particle system's modifiers can list shapes). Unreachable blocks are absent. The root is block
/// 0 when it is a [`NODE_TYPES`] block, else the first block that is.
pub fn index3d(header: &Header, bytes: &[u8]) -> BTreeMap<u32, u32> {
    let total = header.sizes.len();
    let is_node = |i: usize| NODE_TYPES.iter().any(|t| t.as_bytes() == header.kind(i));
    let mut out = BTreeMap::new();
    let Some(root) = (0..total).find(|&i| is_node(i)) else {
        return out;
    };
    let mut seen: HashSet<usize> = HashSet::new();
    let mut stack = vec![(root, 0usize)];
    let mut next = 0u32;
    while let Some((b, depth)) = stack.pop() {
        if !seen.insert(b) {
            continue;
        }
        if takes_index(header.kind(b)) {
            out.insert(b as u32, next);
            next += 1;
        }
        if depth >= MAX_DEPTH {
            continue;
        }
        // Reversed so the first ref pops first (pre-order).
        for k in av_refs(header, bytes, b).into_iter().rev() {
            if let Some(c) = usize::try_from(k).ok().filter(|&c| c < total) {
                stack.push((c, depth + 1));
            }
        }
    }
    out
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

    /// The blocks that take a 3D index are the nif.xml closures of NiTriBasedGeom, BSTriShape and NiParticleSystem
    /// (nifly's NiShape plus PGPatcher's particle increment), minus the abstract NiTriBasedGeom and the non-Skyrim
    /// NiClod, NiLines and NiScreenElements (nifly does not read them). `ShapeKind` covers all but
    /// NiMeshParticleSystem, which `takes_index` adds.
    #[test]
    fn index_takers_match_the_nifxml_shape_and_particle_closures() {
        let mut inherit: HashMap<&str, &str> = HashMap::new();
        for l in include_str!("../tests/fixtures/nifxml_inherit.txt").lines() {
            let (n, p) = l.split_once(' ').unwrap();
            inherit.insert(n, p);
        }
        let under = |mut n: &str| loop {
            if matches!(n, "NiTriBasedGeom" | "BSTriShape" | "NiParticleSystem") {
                return true;
            }
            match inherit.get(n) {
                Some(p) => n = p,
                None => return false,
            }
        };
        let not_skyrim = ["NiTriBasedGeom", "NiClod", "NiLines", "NiScreenElements"];
        let mut want: Vec<&str> = inherit
            .keys()
            .copied()
            .filter(|n| under(n) && !not_skyrim.contains(n))
            .collect();
        want.sort_unstable();
        let mut got: Vec<&str> = inherit
            .keys()
            .copied()
            .filter(|n| takes_index(n.as_bytes()))
            .collect();
        got.sort_unstable();
        assert_eq!(got, want);
        assert_eq!(
            want.len(),
            11,
            "the 10 ShapeKind names and NiMeshParticleSystem"
        );
    }

    #[test]
    fn modifier_lists_match_the_nifxml_closures() {
        let mut inherit: HashMap<&str, &str> = HashMap::new();
        for l in include_str!("../tests/fixtures/nifxml_inherit.txt").lines() {
            let (n, p) = l.split_once(' ').unwrap();
            inherit.insert(n, p);
        }
        let closure = |base: &str| {
            let mut v: Vec<&str> = inherit
                .keys()
                .copied()
                .filter(|&k| {
                    let mut n = k;
                    loop {
                        if n == base {
                            return true;
                        }
                        match inherit.get(n) {
                            Some(p) => n = p,
                            None => return false,
                        }
                    }
                })
                .collect();
            v.sort_unstable();
            v
        };
        assert_eq!(closure("NiPSysMeshUpdateModifier"), MESH_UPDATE_MODIFIERS);
        assert_eq!(closure("NiPSysFieldModifier"), FIELD_MODIFIERS);
    }
}
