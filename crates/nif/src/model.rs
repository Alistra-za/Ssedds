//! One owned, decoded view of a NIF for the editor (spec §3). Built in a single pass over the blocks by kind;
//! a block that fails to decode becomes a `problems` entry, never a panic. Only a header or file-length failure is an Err.
use crate::ctrl::{self, Interp, Sequence, TimeCtrl};
use crate::extra::{read_extra, Extra};
use crate::header::Header;
use crate::net::{read_net, ObjectNet};
use crate::props::{
    decode_alpha, decode_lighting, decode_texture_set_raw, Alpha, Lighting, TexSetRaw,
};
use crate::shape_head::{decode_head_at, RefAt, ShapeKind};
use crate::NifError;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq)]
pub struct ShapeModel {
    pub block: u32,
    pub name: Option<std::sync::Arc<str>>,
    pub kind: ShapeKind,
    pub net: ObjectNet,
    pub shader: Option<u32>,
    pub alpha: Option<u32>,
    pub at: RefAt,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PropModel {
    pub block: u32,
    pub net: ObjectNet,
    pub ctrls: Vec<u32>,
    /// Shape blocks that reference this property.
    pub users: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LightingModel {
    pub prop: PropModel,
    pub lighting: Lighting,
    pub texture_set: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlphaModel {
    pub prop: PropModel,
    pub alpha: Alpha,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TexSetModel {
    pub raw: TexSetRaw,
    /// BSLightingShaderProperty blocks that reference this set.
    pub users: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NifModel {
    pub bsver: u32,
    /// Footer roots[0], when it is in range.
    pub root: Option<u32>,
    /// Header type name of the root block; empty without a root.
    pub root_kind: String,
    /// When the root block has an NiObjectNET prefix (node types).
    pub root_net: Option<ObjectNet>,
    pub shapes: Vec<ShapeModel>,
    /// Every BSLightingShaderProperty block, orphans too.
    pub lighting: BTreeMap<u32, LightingModel>,
    pub alphas: BTreeMap<u32, AlphaModel>,
    pub texsets: BTreeMap<u32, TexSetModel>,
    pub ctrls: BTreeMap<u32, TimeCtrl>,
    pub interps: BTreeMap<u32, Interp>,
    pub extras: BTreeMap<u32, Extra>,
    pub sequences: Vec<Sequence>,
    /// Every readable NiControllerManager: how many sequences it lists (Note CU8).
    pub managers: BTreeMap<u32, usize>,
    pub problems: Vec<String>,
    /// Every shape the root reaches, with its PGPatcher 3D index (`graph::index3d`); an orphan shape is absent.
    pub index3d: BTreeMap<u32, u32>,
}

impl NifModel {
    pub fn shape(&self, block: u32) -> Option<&ShapeModel> {
        self.shapes.iter().find(|s| s.block == block)
    }

    /// Flags bit 0x20 (Manager Controlled), or named by any controlled block of a sequence.
    pub fn managed(&self, ctrl: u32) -> bool {
        self.ctrls.get(&ctrl).is_some_and(|c| c.flags & 0x20 != 0)
            || self
                .sequences
                .iter()
                .flat_map(|s| &s.blocks)
                .any(|cb| cb.controller == ctrl as i32)
    }

    /// The net's extra-data blocks in list order; null and unreadable refs are skipped.
    pub fn extras_of(&self, net: &ObjectNet) -> Vec<&Extra> {
        net.extra
            .iter()
            .filter_map(|&e| u32::try_from(e).ok().and_then(|e| self.extras.get(&e)))
            .collect()
    }
}

fn is_extra(kind: &[u8]) -> bool {
    kind.ends_with(b"ExtraData")
        || kind == b"BSXFlags"
        || matches!(
            kind,
            b"BSInvMarker" | b"BSBound" | b"BSFurnitureMarker" | b"BSFurnitureMarkerNode"
        )
        || kind.starts_with(b"BSConnectPoint")
}

fn is_node(kind: &[u8]) -> bool {
    kind.ends_with(b"Node")
}

pub fn read_model(b: &[u8]) -> Result<NifModel, NifError> {
    let h = Header::parse(b)?;
    h.check_file_length(b)?;
    let roots = h.roots(b)?;
    let n = h.sizes.len();
    let mut m = NifModel {
        bsver: h.bsver,
        root: roots
            .first()
            .and_then(|&r| u32::try_from(r).ok())
            .filter(|&r| (r as usize) < n),
        root_kind: String::new(),
        root_net: None,
        shapes: vec![],
        lighting: BTreeMap::new(),
        alphas: BTreeMap::new(),
        texsets: BTreeMap::new(),
        ctrls: BTreeMap::new(),
        interps: BTreeMap::new(),
        extras: BTreeMap::new(),
        sequences: vec![],
        managers: BTreeMap::new(),
        problems: vec![],
        index3d: crate::graph::index3d(&h, b),
    };
    m.root_kind = m
        .root
        .map_or(String::new(), |r| crate::text::lossy(h.kind(r as usize)));
    let mut p = vec![];
    let mut shape_refs = vec![];

    for i in 0..n {
        let kind = h.kind(i);
        let prop = |net: ObjectNet| PropModel {
            block: i as u32,
            net,
            ctrls: vec![],
            users: vec![],
        };
        if let Some(k) = ShapeKind::from_name(kind) {
            match (decode_head_at(&h, b, i, k), read_net(&h, b, i)) {
                (Ok((head, at)), Ok(net)) => {
                    shape_refs.push((i as u32, head.shader, head.alpha));
                    m.shapes.push(ShapeModel {
                        block: i as u32,
                        name: head.name,
                        kind: k,
                        net,
                        shader: None,
                        alpha: None,
                        at,
                    });
                }
                (Err(e), _) | (_, Err(e)) => p.push(format!("block {i}: {e}")),
            }
        } else if kind == b"BSLightingShaderProperty" {
            match (decode_lighting(&h, b, i), read_net(&h, b, i)) {
                (Ok(lighting), Ok(net)) => {
                    let texture_set = u32::try_from(lighting.texture_set)
                        .ok()
                        .filter(|&t| (t as usize) < n);
                    if texture_set.is_none() && lighting.texture_set != -1 {
                        p.push(format!("block {i}: texture set ref outside the file"));
                    }
                    m.lighting.insert(
                        i as u32,
                        LightingModel {
                            prop: prop(net),
                            lighting,
                            texture_set,
                        },
                    );
                }
                (Err(e), _) | (_, Err(e)) => p.push(format!("block {i}: {e}")),
            }
        } else if kind == b"NiAlphaProperty" {
            match (decode_alpha(&h, b, i), read_net(&h, b, i)) {
                (Ok(alpha), Ok(net)) => {
                    m.alphas.insert(
                        i as u32,
                        AlphaModel {
                            prop: prop(net),
                            alpha,
                        },
                    );
                }
                (Err(e), _) | (_, Err(e)) => p.push(format!("block {i}: {e}")),
            }
        } else if kind == b"BSShaderTextureSet" {
            match decode_texture_set_raw(&h, b, i) {
                Ok(raw) => {
                    m.texsets
                        .insert(i as u32, TexSetModel { raw, users: vec![] });
                }
                Err(e) => p.push(format!("block {i}: {e}")),
            }
        } else if is_extra(kind) {
            match read_extra(&h, b, i) {
                Ok(x) => {
                    m.extras.insert(i as u32, x);
                }
                Err(e) => p.push(format!("block {i}: {e}")),
            }
        } else if kind == b"NiControllerSequence" {
            match ctrl::read_sequence(&h, b, i) {
                Ok(s) => m.sequences.push(s),
                Err(e) => p.push(format!("block {i}: {e}")),
            }
        } else if Some(i as u32) == m.root && is_node(kind) {
            match read_net(&h, b, i) {
                Ok(net) => m.root_net = Some(net),
                Err(e) => p.push(format!("block {i}: {e}")),
            }
        }
    }

    // An unlisted sequence never plays: with a readable manager, keep only the sequences some manager lists (F2).
    let mut listed: Option<BTreeSet<i32>> = None;
    for i in (0..n).filter(|&i| h.kind(i) == b"NiControllerManager") {
        match ctrl::read_manager(&h, b, i) {
            Ok(mgr) => {
                m.managers.insert(i as u32, mgr.sequences.len());
                listed.get_or_insert_default().extend(mgr.sequences)
            }
            Err(e) => p.push(format!("block {i}: {e}")),
        }
    }
    if let Some(l) = listed {
        m.sequences.retain(|s| l.contains(&(s.block as i32)));
    }

    // Shape refs and users. `shape_refs` is pushed with `m.shapes`, so they pair by index (trace 2026-10-28 #43).
    for (s, (block, shader, alpha)) in m.shapes.iter_mut().zip(shape_refs) {
        let mut r = |v: i32, what: &str| match u32::try_from(v) {
            Ok(x) if (x as usize) < n => Some(x),
            Ok(_) => {
                p.push(format!("block {block}: {what} ref {v} outside the file"));
                None
            }
            Err(_) => None,
        };
        let (shader, alpha) = (r(shader, "shader"), r(alpha, "alpha"));
        if let Some(l) = shader.and_then(|s| m.lighting.get_mut(&s)) {
            l.prop.users.push(block);
        }
        if let Some(a) = alpha.and_then(|a| m.alphas.get_mut(&a)) {
            a.prop.users.push(block);
        }
        (s.shader, s.alpha) = (shader, alpha);
    }
    for (&lb, l) in &m.lighting {
        if let Some(t) = l.texture_set.and_then(|t| m.texsets.get_mut(&t)) {
            t.users.push(lb);
        }
    }

    // Controller chains from every owner, each first link walked once.
    let mut owners: Vec<i32> = vec![];
    owners.extend(m.root_net.iter().map(|x| x.controller));
    owners.extend(m.shapes.iter().map(|s| s.net.controller));
    owners.extend(m.lighting.values().map(|l| l.prop.net.controller));
    owners.extend(m.alphas.values().map(|a| a.prop.net.controller));
    let mut walked = BTreeSet::new();
    let mut chains: BTreeMap<i32, Vec<u32>> = BTreeMap::new();
    for first in owners {
        if first >= 0 && walked.insert(first) {
            let ids = ctrl::chain(&h, b, first, &mut p);
            chains.insert(first, ids);
        }
    }
    for id in chains.values().flatten() {
        match ctrl::read_ctrl(&h, b, *id as usize) {
            Ok(c) => {
                m.ctrls.insert(*id, c);
            }
            Err(e) => p.push(format!("block {id}: {e}")),
        }
    }
    let chain_of = |first: i32| chains.get(&first).cloned().unwrap_or_default();
    for l in m.lighting.values_mut() {
        l.prop.ctrls = chain_of(l.prop.net.controller);
    }
    for a in m.alphas.values_mut() {
        a.prop.ctrls = chain_of(a.prop.net.controller);
    }

    // Interpolators named by controllers and sequence controlled blocks.
    let named: Vec<i32> = m
        .ctrls
        .values()
        .filter_map(|c| c.interp)
        .chain(
            m.sequences
                .iter()
                .flat_map(|s| s.blocks.iter().map(|cb| cb.interp)),
        )
        .collect();
    for i in named {
        let Ok(i) = usize::try_from(i) else { continue };
        if i >= n {
            p.push(format!("interpolator ref {i} outside the file"));
        } else if let std::collections::btree_map::Entry::Vacant(e) = m.interps.entry(i as u32) {
            match ctrl::read_interp(&h, b, i) {
                Ok(x) => {
                    e.insert(x);
                }
                Err(err) => p.push(format!("block {i}: {err}")),
            }
        }
    }

    // Extra-data refs that point outside the file.
    let nets = m
        .root_net
        .iter()
        .chain(m.shapes.iter().map(|s| &s.net))
        .chain(m.lighting.values().map(|l| &l.prop.net))
        .chain(m.alphas.values().map(|a| &a.prop.net));
    for e in nets.flat_map(|x| &x.extra) {
        if usize::try_from(*e).is_ok_and(|e| e >= n) {
            p.push(format!("extra-data ref {e} outside the file"));
        }
    }
    m.problems = p;
    Ok(m)
}
