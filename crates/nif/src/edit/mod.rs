//! Edit ops, anchors and in-place field edits (spec §3.4, plan A8). An op records where it was made (an `Anchor`)
//! and the value it saw (`base`); applying it re-resolves the anchor by role in the current file and refuses to
//! write over a value that changed underneath it. Every op runs on the bytes the previous op produced (R1).
use crate::extra::ExtraValue;
use crate::fields::{locate, Field, Value};
use crate::keys::KeyData;
use crate::model::{read_model, NifModel, ShapeModel};
use crate::write::NifDoc;
use crate::NifError;

mod structural;
pub use structural::{convert_check, trailer_defaults};
pub mod validate;

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
    /// Replaces the keys of the anchored controller's own interpolator (`clip: None`), or of a sequence's
    /// controlled-block interpolator bound to that controller (`clip: Some(sequence block)`, v2 B7).
    SetKeys {
        at: Anchor, // Role::Ctrl
        clip: Option<u32>,
        keys: KeyData,
        base: KeyData,
    },
    /// Appends data, interpolator and controller, links them at the end of the property's chain, and sets root
    /// BSXFlags bit 0 (D9, G1). `at` is Role::Shader or Role::Alpha of a shape.
    AddController {
        at: Anchor,
        class: String,
        var: Option<u32>,
        keys: KeyData,
        flags: u16,
        #[cfg_attr(feature = "serde", serde(with = "crate::keys::f32_bits"))]
        freq: f32,
        #[cfg_attr(feature = "serde", serde(with = "crate::keys::f32_bits"))]
        start: f32,
        #[cfg_attr(feature = "serde", serde(with = "crate::keys::f32_bits"))]
        stop: f32,
    },
    /// Unlinks the controller and clears its Active bit; its interpolator and data stay as orphans (spec §5.3).
    RemoveController {
        at: Anchor, // Role::Ctrl
    },
    /// Appends a clip to the root's NiControllerManager, creating the manager, an MTTC, the palette and text keys
    /// when the root has none (v2 C2). Each track's controller becomes manager-controlled.
    AddSequence {
        at: Anchor, // Role::Root
        name: String,
        cycle: u32,
        #[cfg_attr(feature = "serde", serde(with = "crate::keys::f32_bits"))]
        freq: f32,
        #[cfg_attr(feature = "serde", serde(with = "crate::keys::f32_bits"))]
        start: f32,
        #[cfg_attr(feature = "serde", serde(with = "crate::keys::f32_bits"))]
        stop: f32,
        tracks: Vec<ClipTrack>,
    },
    /// Unlinks the `nth` clip named `name` from the root's manager (v2 C2, spec §5.3).
    RemoveSequence {
        at: Anchor, // Role::Root
        name: String,
        nth: u32,
    },
    /// Points the `nth` clip named `name` at the string `new_name`.
    RenameSequence {
        at: Anchor, // Role::Root
        name: String,
        nth: u32,
        new_name: String,
    },
    /// Turns the free controllers of a shader or alpha property into one clip on the root's manager: C2's AddSequence
    /// with one track per free controller, their keys as they are (v2 C3, D11). One op, so one undo step.
    ConvertChainToClips {
        at: Anchor, // Role::Shader or Role::Alpha of a shape
        name: String,
    },
}

/// The name the GUI offers for a converted chain, and the hover beside it (plan C5).
pub const CONVERT_NAME: &str = "SpecialIdle";
pub const CONVERT_HOVER: &str =
    "SpecialIdle loops constantly in game; Open/Close play as door states.";

/// One track of a new clip. Its anchor is resolved on every apply; `rebase` never writes it back (F3).
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct ClipTrack {
    pub at: Anchor, // Role::Ctrl: the controller this clip drives
    pub keys: KeyData,
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
                            && crate::text::lossy(e.name.as_deref().unwrap_or(&[])) == *name
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
        // a clip drag never merges with a free-curve drag
        Op::SetKeys { at, clip, .. } => {
            ("SetKeys", at, clip.map_or(String::new(), |c| c.to_string()))
        }
        Op::AddController { at, class, var, .. } => {
            ("AddController", at, format!("{class}/{var:?}"))
        }
        Op::RemoveController { at } => ("RemoveController", at, String::new()),
        Op::AddSequence { at, name, .. } => ("AddSequence", at, name.clone()),
        Op::RemoveSequence { at, name, nth } => ("RemoveSequence", at, format!("{name}/{nth}")),
        Op::RenameSequence { at, name, nth, .. } => ("RenameSequence", at, format!("{name}/{nth}")),
        Op::ConvertChainToClips { at, name } => ("ConvertChainToClips", at, name.clone()),
    };
    format!("{v}|{}|{what}", at.block)
}

/// The keys `SetKeys` edits on controller `ctrl` (v2 B6, the Controllers tab): its own interpolator's (`clip: None`)
/// exactly as `apply` compares an op's `base`. Err gives why there is nothing to edit (shown in the tab).
pub fn keys_of(b: &[u8], m: &NifModel, ctrl: u32, clip: Option<u32>) -> Result<KeyData, String> {
    let h = crate::header::Header::parse(b).map_err(|e| e.to_string())?;
    keys_of_in(&h, b, m, ctrl, clip)
}

/// [`keys_of`] with `b`'s header parsed once by the caller (B-P41: one parse per edit, not per controller).
pub fn keys_of_in(
    h: &crate::header::Header,
    b: &[u8],
    m: &NifModel,
    ctrl: u32,
    clip: Option<u32>,
) -> Result<KeyData, String> {
    structural::key_target(h, b, m, ctrl, clip).map(|t| t.keys)
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

fn anchor_of(op: &Op) -> &Anchor {
    match op {
        Op::SetField { at, .. }
        | Op::SetBits { at, .. }
        | Op::SplitAlpha { at }
        | Op::AddAlpha { at, .. }
        | Op::RemoveAlpha { at }
        | Op::AddExtra { at, .. }
        | Op::EditExtra { at, .. }
        | Op::RemoveExtra { at }
        | Op::SetKeys { at, .. }
        | Op::AddController { at, .. }
        | Op::RemoveController { at }
        | Op::AddSequence { at, .. }
        | Op::RemoveSequence { at, .. }
        | Op::RenameSequence { at, .. }
        | Op::ConvertChainToClips { at, .. }
        | Op::SetTexturePath { at, .. } => at,
    }
}

/// Re-runs `ops` on a changed source: every op that `apply` re-anchored gets the block it resolved to.
pub fn rebase(new_src: &[u8], ops: &[Op]) -> Result<(Vec<Op>, Vec<Outcome>), NifError> {
    let r = apply(new_src, ops)?;
    let mut out = ops.to_vec();
    for (op, o) in out.iter_mut().zip(&r.outcomes) {
        if let Outcome::ReAnchored { block } = o {
            match op {
                Op::SetField { at, .. }
                | Op::SetBits { at, .. }
                | Op::SplitAlpha { at }
                | Op::AddAlpha { at, .. }
                | Op::RemoveAlpha { at }
                | Op::AddExtra { at, .. }
                | Op::EditExtra { at, .. }
                | Op::RemoveExtra { at }
                | Op::SetKeys { at, .. }
                | Op::AddController { at, .. }
                | Op::RemoveController { at }
                | Op::AddSequence { at, .. }
                | Op::RemoveSequence { at, .. }
                | Op::RenameSequence { at, .. }
                | Op::ConvertChainToClips { at, .. }
                | Op::SetTexturePath { at, .. } => at.block = *block,
            }
        }
    }
    Ok((out, r.outcomes))
}

/// Keep mine: op `i`'s `base` becomes the value now at its anchor, in the bytes `ops[..i]` produce from
/// `new_src`. An op without a base, or whose anchor is missing or ambiguous, is returned unchanged.
pub fn keep_mine(new_src: &[u8], ops: &[Op], i: usize) -> Result<Vec<Op>, NifError> {
    let mut out = ops.to_vec();
    let Some(op) = ops.get(i) else { return Ok(out) };
    let cur = apply(new_src, &ops[..i])?.bytes;
    let (Ok(m), Ok(doc)) = (read_model(&cur), NifDoc::open(&cur)) else {
        return Ok(out);
    };
    let Ok((block, _)) = resolve(&m, anchor_of(op)) else {
        return Ok(out);
    };
    let field_now = |f: Field| {
        locate(doc.header(), &cur, field_block(&m, block, f) as usize, f)
            .ok()
            .map(|r| r.1)
    };
    match &mut out[i] {
        Op::SetField { field, base, .. } => {
            if let Some(v) = field_now(*field) {
                *base = v;
            }
        }
        Op::SetBits { field, base, .. } => match field_now(*field) {
            Some(Value::U8(v)) => *base = v as u32,
            Some(Value::U16(v)) => *base = v as u32,
            Some(Value::U32(v)) => *base = v,
            _ => {}
        },
        Op::SetTexturePath { slot, base, .. } => {
            let slots = m
                .shape(block)
                .and_then(|s| s.shader)
                .and_then(|s| m.lighting.get(&s))
                .and_then(|l| l.texture_set)
                .and_then(|t| m.texsets.get(&t))
                .map(|t| &t.raw.slots);
            if let Some(s) = slots.and_then(|s| s.get(*slot as usize)) {
                *base = encoding_rs::WINDOWS_1252.decode(s).0.into_owned();
            }
        }
        Op::EditExtra { base, .. } => {
            if let Some(v) = m.extras.get(&block).and_then(|x| x.value.clone()) {
                *base = v;
            }
        }
        Op::SetKeys { base, clip, .. } => {
            if let Ok(t) = structural::key_target(doc.header(), &cur, &m, block, *clip) {
                *base = t.keys;
            }
        }
        _ => {}
    }
    Ok(out)
}

/// Take source: drops op `i`.
pub fn take_source(ops: &[Op], i: usize) -> Vec<Op> {
    let mut v = ops.to_vec();
    if i < v.len() {
        v.remove(i);
    }
    v
}

/// The block that holds `field` of the anchored block: an interpolator value lives on the controller's interpolator.
fn field_block(m: &NifModel, block: u32, field: Field) -> u32 {
    match (field, m.ctrls.get(&block).and_then(|c| c.interp)) {
        (Field::InterpValue(_), Some(i)) if i >= 0 => i as u32,
        _ => block,
    }
}

fn step(cur: &[u8], op: &Op) -> (Option<Vec<u8>>, Outcome) {
    let refused = |s: String| (None, Outcome::Refused(s));
    let at = anchor_of(op);
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
    // Ruling P8: an alpha field edit on a shared alpha would change every user; it needs a split first.
    // The same holds for a controller on a shared alpha's chain.
    let alpha_users = match at.role {
        Role::Alpha => m.alphas.get(&block).map(|a| a.prop.users.len()),
        Role::Ctrl {
            owner: Owner::Alpha,
            ..
        } => m
            .alphas
            .values()
            .find(|a| a.prop.ctrls.contains(&block))
            .map(|a| a.prop.users.len()),
        _ => None,
    };
    if let Some(users) = alpha_users {
        if users > 1 {
            return (
                None,
                Outcome::Conflict(format!(
                    "This alpha is shared by {users} shapes: split it before editing."
                )),
            );
        }
    }
    if matches!(op, Op::SetKeys { .. }) {
        return match structural::set_keys(&mut doc, cur, &m, block, op) {
            Ok(Outcome::Applied) => (Some(doc.bytes()), outcome),
            Ok(o) => (None, o),
            Err(e) => refused(e),
        };
    }
    if matches!(op, Op::AddController { .. } | Op::RemoveController { .. }) {
        return match structural::ctrl_op(&mut doc, &m, at, block, op) {
            Ok(Outcome::Applied) => (Some(doc.bytes()), outcome),
            Ok(o) => (None, o),
            Err(e) => refused(e),
        };
    }
    if matches!(
        op,
        Op::AddSequence { .. } | Op::RemoveSequence { .. } | Op::RenameSequence { .. }
    ) {
        return match structural::clip_op(&mut doc, cur, &m, block, op) {
            Ok(Outcome::Applied) => (Some(doc.bytes()), outcome),
            Ok(o) => (None, o),
            Err(e) => refused(e),
        };
    }
    if matches!(op, Op::ConvertChainToClips { .. }) {
        let r = match at.role {
            Role::Shader | Role::Alpha => structural::convert_op(&mut doc, cur, &m, at, block, op),
            _ => Err("Convert anchors on a shader or alpha property.".into()),
        };
        return match r {
            Ok(Outcome::Applied) => (Some(doc.bytes()), outcome),
            Ok(o) => (None, o),
            Err(e) => refused(e),
        };
    }
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
            Some(s) => structural::alpha_op(&mut doc, cur, &m, s, op),
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
    let ctrl = block;
    let block = field_block(&m, block, field);
    // Ruling P9: an interpolator that several controllers reference would change all of them.
    if block != ctrl {
        let n = m
            .ctrls
            .values()
            .filter(|c| c.interp == Some(block as i32))
            .count();
        if n > 1 {
            return (
                None,
                Outcome::Conflict(format!(
                    "This interpolator is shared by {n} controllers: editing it would change all of them."
                )),
            );
        }
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
    // Phase A ruling R8 lifted (v2 B7): a managed controller's clips follow its new variable.
    if let (
        Field::Variable,
        Op::SetField {
            value: Value::U32(v),
            base: Value::U32(o),
            ..
        },
    ) = (field, op)
    {
        if m.managed(ctrl) && v != o {
            if let Err(e) = structural::rewrite_cb_ids(&mut doc, &m, ctrl, *o, *v) {
                return refused(e);
            }
        }
    }
    (Some(doc.bytes()), outcome)
}

fn finish(doc: NifDoc, r: Result<(), String>, ok: Outcome) -> (Option<Vec<u8>>, Outcome) {
    match r {
        Ok(()) => (Some(doc.bytes()), ok),
        Err(e) => (None, Outcome::Refused(e)),
    }
}
