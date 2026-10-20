//! Edit ops, anchors and in-place field edits (spec §3.4, plan A8). An op records where it was made (an `Anchor`)
//! and the value it saw (`base`); applying it re-resolves the anchor by role in the current file and refuses to
//! write over a value that changed underneath it. Every op runs on the bytes the previous op produced (R1).
use crate::extra::ExtraValue;
use crate::fields::{locate, Field, Value};
use crate::model::{read_model, NifModel, ShapeModel};
use crate::write::NifDoc;
use crate::NifError;

mod structural;
pub use structural::trailer_defaults;

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    Shader,
    Alpha,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    Shape,
    Root,
    Shader,
    Alpha,
    TextureSet,
    Ctrl {
        owner: Owner,
        class: String,
        var: Option<u32>,
        nth: u32,
    },
    Extra {
        on_root: bool,
        name: String,
        kind: String,
        nth: u32,
    },
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchor {
    pub block: u32,
    pub kind: String,
    /// Name of the owning shape; `None` for the root, an unnamed shape, or root-owned extras.
    pub shape: Option<String>,
    pub role: Role,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    SetField {
        at: Anchor,
        field: Field,
        value: Value,
        base: Value,
    },
    SetBits {
        at: Anchor,
        field: Field,
        mask: u32,
        value: u32,
        base: u32,
    },
    SetTexturePath {
        at: Anchor,
        slot: u32,
        path: String,
        base: String,
    },
    SplitAlpha {
        at: Anchor,
    },
    AddAlpha {
        at: Anchor,
        flags: u16,
        threshold: u8,
    },
    RemoveAlpha {
        at: Anchor,
    },
    AddExtra {
        at: Anchor,
        kind: String,
        name: String,
        value: ExtraValue,
    },
    EditExtra {
        at: Anchor,
        value: ExtraValue,
        base: ExtraValue,
    },
    RemoveExtra {
        at: Anchor,
    },
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Applied,
    ReAnchored { block: u32 },
    Conflict(String),
    Refused(String),
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    pub bytes: Vec<u8>,
    pub outcomes: Vec<Outcome>,
}

fn owned(role: &Role) -> bool {
    !matches!(role, Role::Root | Role::Extra { on_root: true, .. })
}

type Cand = (u32, String);

/// Blocks that satisfy `role` for the shapes in `sel` (sorted, deduplicated).
fn role_cands(m: &NifModel, sel: &[&ShapeModel], role: &Role) -> Vec<Cand> {
    let mut v: Vec<Cand> = vec![];
    let shader_block = |s: &&ShapeModel| s.shader.filter(|b| m.lighting.contains_key(b));
    match role {
        Role::Root => v.extend(m.root.map(|r| (r, m.root_kind.clone()))),
        Role::Shape => v.extend(sel.iter().map(|s| (s.block, format!("{:?}", s.kind)))),
        Role::Shader => v.extend(
            sel.iter()
                .filter_map(shader_block)
                .map(|b| (b, "BSLightingShaderProperty".to_string())),
        ),
        Role::Alpha => v.extend(
            sel.iter()
                .filter_map(|s| s.alpha.filter(|b| m.alphas.contains_key(b)))
                .map(|b| (b, "NiAlphaProperty".to_string())),
        ),
        Role::TextureSet => v.extend(
            sel.iter()
                .filter_map(shader_block)
                .filter_map(|b| m.lighting[&b].texture_set)
                .filter(|t| m.texsets.contains_key(t))
                .map(|t| (t, "BSShaderTextureSet".to_string())),
        ),
        Role::Ctrl {
            owner,
            class,
            var,
            nth,
        } => {
            for s in sel {
                let chain = match owner {
                    Owner::Shader => shader_block(s).map(|b| &m.lighting[&b].prop.ctrls),
                    Owner::Alpha => s
                        .alpha
                        .and_then(|b| m.alphas.get(&b))
                        .map(|a| &a.prop.ctrls),
                };
                let hit = chain
                    .into_iter()
                    .flatten()
                    .filter_map(|c| m.ctrls.get(c))
                    .filter(|c| c.class == *class && c.var == *var)
                    .nth(*nth as usize);
                v.extend(hit.map(|c| (c.block, c.class.clone())));
            }
        }
        Role::Extra {
            on_root,
            name,
            kind,
            nth,
        } => {
            let nets: Vec<_> = if *on_root {
                m.root_net.iter().collect()
            } else {
                sel.iter().map(|s| &s.net).collect()
            };
            for net in nets {
                let hit = m
                    .extras_of(net)
                    .into_iter()
                    .filter(|e| {
                        e.kind == *kind
                            && String::from_utf8_lossy(e.name.as_deref().unwrap_or(&[]))
                                == name.as_str()
                    })
                    .nth(*nth as usize);
                v.extend(hit.map(|e| (e.block, e.kind.clone())));
            }
        }
    }
    v.sort();
    v.dedup();
    v
}

/// Finds the block an anchor means in `m`: the recorded index if it still satisfies the role, else the single
/// block that does. Returns `(block, re-anchored)`.
pub fn resolve(m: &NifModel, a: &Anchor) -> Result<(u32, bool), String> {
    let own = owned(&a.role);
    let sel: Vec<&ShapeModel> = match (&a.shape, own) {
        (Some(n), true) => m
            .shapes
            .iter()
            .filter(|s| s.name.as_deref() == Some(n.as_str()))
            .collect(),
        _ => m.shapes.iter().collect(),
    };
    let c = role_cands(m, &sel, &a.role);
    if let Some((b, _)) = c.iter().find(|(b, k)| *b == a.block && *k == a.kind) {
        return Ok((*b, false));
    }
    if own && a.shape.is_none() {
        return Err("missing".into()); // unnamed shape: the index is all there is
    }
    match c.as_slice() {
        [] => Err("missing".into()),
        [(b, _)] => Ok((*b, true)),
        _ => Err(match (&a.role, &a.shape) {
            (Role::Shape, Some(n)) => format!("ambiguous: {} shapes named {n}", c.len()),
            _ => format!("ambiguous: {} candidates", c.len()),
        }),
    }
}

/// Builds an anchor for `role` from the current model; `shape` is the owning shape block (ignored for root roles).
pub fn anchor_role(m: &NifModel, shape: Option<u32>, role: Role) -> Option<Anchor> {
    let s = shape.and_then(|b| m.shape(b)).filter(|_| owned(&role));
    let sel: Vec<&ShapeModel> = s.into_iter().collect();
    let (block, kind) = role_cands(m, &sel, &role).into_iter().next()?;
    Some(Anchor {
        block,
        kind,
        shape: s.and_then(|s| s.name.clone()),
        role,
    })
}

pub fn anchor_shape(m: &NifModel, shape: u32) -> Anchor {
    anchor_role(m, Some(shape), Role::Shape).unwrap_or(Anchor {
        block: shape,
        kind: String::new(),
        shape: None,
        role: Role::Shape,
    })
}

/// `"<variant>|<block>|<field or slot>"`: ops with the same key are the same edit (a slider drag).
pub fn merge_key(op: &Op) -> String {
    let (v, at, what) = match op {
        Op::SetField { at, field, .. } => ("SetField", at, format!("{field:?}")),
        Op::SetBits { at, field, .. } => ("SetBits", at, format!("{field:?}")),
        Op::SetTexturePath { at, slot, .. } => ("SetTexturePath", at, slot.to_string()),
        Op::SplitAlpha { at } => ("SplitAlpha", at, String::new()),
        Op::AddAlpha { at, .. } => ("AddAlpha", at, String::new()),
        Op::RemoveAlpha { at } => ("RemoveAlpha", at, String::new()),
        Op::AddExtra { at, kind, name, .. } => ("AddExtra", at, format!("{kind}/{name}")),
        Op::EditExtra { at, .. } => ("EditExtra", at, String::new()),
        Op::RemoveExtra { at } => ("RemoveExtra", at, String::new()),
    };
    format!("{v}|{}|{what}", at.block)
}

/// Err only when `src` itself does not open. A later op that cannot run is a `Conflict` or `Refused` outcome and
/// the ops after it still run.
pub fn apply(src: &[u8], ops: &[Op]) -> Result<Applied, NifError> {
    NifDoc::open(src)?;
    let mut bytes = src.to_vec();
    let mut outcomes = Vec::with_capacity(ops.len());
    for op in ops {
        let (next, outcome) = step(&bytes, op);
        if let Some(n) = next {
            bytes = n;
        }
        outcomes.push(outcome);
    }
    Ok(Applied { bytes, outcomes })
}

fn step(cur: &[u8], op: &Op) -> (Option<Vec<u8>>, Outcome) {
    let refused = |s: String| (None, Outcome::Refused(s));
    let at = match op {
        Op::SetField { at, .. }
        | Op::SetBits { at, .. }
        | Op::SplitAlpha { at }
        | Op::AddAlpha { at, .. }
        | Op::RemoveAlpha { at }
        | Op::AddExtra { at, .. }
        | Op::EditExtra { at, .. }
        | Op::RemoveExtra { at }
        | Op::SetTexturePath { at, .. } => at,
    };
    let m = match read_model(cur) {
        Ok(m) => m,
        Err(e) => return refused(e.to_string()),
    };
    let mut doc = match NifDoc::open(cur) {
        Ok(d) => d,
        Err(e) => return refused(e.to_string()),
    };
    let (block, re) = match resolve(&m, at) {
        Ok(r) => r,
        Err(e) => return (None, Outcome::Conflict(e)),
    };
    let outcome = if re {
        Outcome::ReAnchored { block }
    } else {
        Outcome::Applied
    };
    if matches!(op, Op::SetTexturePath { .. }) {
        let r = match m.shape(block).filter(|_| at.role == Role::Shape) {
            Some(s) => structural::texture_path_op(&mut doc, &m, s, op),
            None => Err("texture paths anchor on a shape".into()),
        };
        return match r {
            Ok(Outcome::Applied) => (Some(doc.bytes()), outcome),
            Ok(o) => (None, o),
            Err(e) => refused(e),
        };
    }
    if matches!(
        op,
        Op::AddExtra { .. } | Op::EditExtra { .. } | Op::RemoveExtra { .. }
    ) {
        return match structural::extra_op(&mut doc, &m, at, block, op) {
            Ok(Outcome::Applied) => (Some(doc.bytes()), outcome),
            Ok(o) => (None, o),
            Err(e) => refused(e),
        };
    }
    if matches!(
        op,
        Op::SplitAlpha { .. } | Op::AddAlpha { .. } | Op::RemoveAlpha { .. }
    ) {
        let r = match m.shape(block).filter(|_| at.role == Role::Shape) {
            Some(s) => structural::alpha_op(&mut doc, &m, s, op),
            None => Err("alpha ops anchor on a shape".into()),
        };
        return finish(doc, r, outcome);
    }
    let field = match op {
        Op::SetField { field, .. } | Op::SetBits { field, .. } => *field,
        _ => unreachable!("filtered above"),
    };
    if field == Field::ShaderType && matches!(op, Op::SetBits { .. }) {
        return refused("the shader type is not a bit field".into());
    }
    if field == Field::Variable && m.managed(block) {
        return refused("the variable of a manager-controlled controller cannot change".into());
    }
    let (rel, current) = match locate(doc.header(), cur, block as usize, field) {
        Ok(r) => r,
        Err(e) => return refused(e.to_string()),
    };
    let new = match op {
        Op::SetField { value, base, .. } => {
            if current != *base {
                return (
                    None,
                    Outcome::Conflict(format!(
                        "changed in the source: was {base:?}, now {current:?}"
                    )),
                );
            }
            if value.bytes().len() != current.bytes().len() {
                return refused(format!("{value:?} does not fit {field:?}"));
            }
            if let (Field::ShaderType, Value::U32(t)) = (field, value) {
                let r = structural::set_shader_type(&mut doc, &m, block, *t);
                return finish(doc, r, outcome);
            }
            value.bytes()
        }
        Op::SetBits {
            mask, value, base, ..
        } => {
            let (bits, width) = match current {
                Value::U8(v) => (v as u32, 1),
                Value::U16(v) => (v as u32, 2),
                Value::U32(v) => (v, 4),
                Value::F32(_) => return refused(format!("{field:?} is not a bit field")),
            };
            if width < 4 && mask >> (8 * width) != 0 {
                return refused(format!("mask {mask:#x} is wider than {field:?}"));
            }
            if bits & mask != base & mask {
                return (
                    None,
                    Outcome::Conflict(format!(
                        "changed in the source: was {:#x}, now {:#x}",
                        base & mask,
                        bits & mask
                    )),
                );
            }
            ((bits & !mask) | (value & mask)).to_le_bytes()[..width].to_vec()
        }
        _ => unreachable!("filtered above"),
    };
    if let Err(e) = doc.patch(block, rel, &new) {
        return refused(e.to_string());
    }
    (Some(doc.bytes()), outcome)
}

fn finish(doc: NifDoc, r: Result<(), String>, ok: Outcome) -> (Option<Vec<u8>>, Outcome) {
    match r {
        Ok(()) => (Some(doc.bytes()), ok),
        Err(e) => (None, Outcome::Refused(e)),
    }
}
