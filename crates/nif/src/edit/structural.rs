//! Structural ops: shader type re-emit and alpha add/remove/split (spec §5, plan A9). Removal is unlink: the
//! block stays as an orphan. New blocks are appended; refs inside opaque blocks are never rewritten.
use super::validate::{ENV_SCALE_PBR, LSCC, LSCF, LSCF_VARS, TESTREF};
use super::{Anchor, ClipTrack, Op, Outcome, Owner, Role};
use crate::clips::{
    encode_blend_interp, encode_manager, encode_mttc, encode_palette, encode_sequence,
    encode_text_keys, read_mttc, read_palette, read_sequence_out, CtrlBlockOut, SequenceOut,
    MANAGER_FLAGS, MTTC_FLAGS,
};
use crate::ctrl::{read_manager, Manager, TimeCtrl, BASE_LEN, CB_LEN};
use crate::extra::{encode_extra, ExtraValue, EDITABLE};
use crate::header::Header;
use crate::keys::{encode_data, read_data, KeyData, KeyGroup};
use crate::material::SLSF2_TRUE_PBR;
use crate::model::{NifModel, PropModel, ShapeModel};
use crate::net::{encode_net, ObjectNet};
use crate::props::encode_texture_set;
use crate::write::NifDoc;

/// Default trailer floats after Lighting Effect 2 for a shader type (01-format §1.2).
pub fn trailer_defaults(shader_type: u32) -> Vec<f32> {
    match shader_type {
        1 => vec![1.0],
        5 | 6 => vec![1.0; 3],
        7 => vec![4.0, 1.0],
        11 => vec![5.0, 0.25, 1.0, 1.0, 1.0],
        14 => vec![0.0; 4],
        16 => vec![1.3, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        _ => vec![],
    }
}

/// Re-emits a BSLightingShaderProperty with a new shader type. The trailer floats after Lighting Effect 2 keep
/// their bytes when the old and new type have the same trailer length (5 <-> 6), else take the defaults.
pub(crate) fn set_shader_type(
    doc: &mut NifDoc,
    m: &NifModel,
    lsp: u32,
    new_type: u32,
) -> Result<(), String> {
    if new_type >= 20 {
        return Err("shader type 20 is not a Skyrim type".into());
    }
    let net = &m
        .lighting
        .get(&lsp)
        .ok_or("not a readable shader property")?
        .prop
        .net;
    let body = doc.block(lsp);
    let p = net.end - doc.header().offsets()[lsp as usize]; // block-relative end of the NiObjectNET
    let head = body
        .get(..p + 84)
        .ok_or("shader block shorter than its fixed fields")?;
    let old_type = u32::from_le_bytes(head[..4].try_into().unwrap());
    let mut out = head.to_vec();
    out[..4].copy_from_slice(&new_type.to_le_bytes());
    if trailer_defaults(old_type).len() == trailer_defaults(new_type).len() {
        out.extend_from_slice(&body[p + 84..]);
    } else {
        let t = trailer_defaults(new_type);
        out.extend(t.iter().flat_map(|f| f.to_le_bytes()));
    }
    doc.set_block(lsp, out);
    Ok(())
}

fn patch(doc: &mut NifDoc, block: u32, at: usize, v: i32) -> Result<(), String> {
    doc.patch(block, at, &v.to_le_bytes())
        .map_err(|e| e.to_string())
}

/// SplitAlpha, AddAlpha and RemoveAlpha on the shape `shape`.
pub(crate) fn alpha_op(
    doc: &mut NifDoc,
    cur: &[u8],
    m: &NifModel,
    shape: &ShapeModel,
    op: &Op,
) -> Result<(), String> {
    // the decoders give absolute offsets; patches are block-relative
    let ref_at = shape.at.alpha - doc.header().offsets()[shape.block as usize];
    let raw = doc
        .block(shape.block)
        .get(ref_at..ref_at + 4)
        .map(|s| i32::from_le_bytes(s.try_into().unwrap()))
        .ok_or("alpha ref outside the shape block")?;
    match op {
        Op::AddAlpha {
            flags, threshold, ..
        } => {
            if raw != -1 {
                return Err("the shape already has an alpha property".into());
            }
            let mut body = crate::net::encode_net(-1, &[], -1);
            body.extend(flags.to_le_bytes());
            body.push(*threshold);
            let new = enc(doc.append(b"NiAlphaProperty", body))?;
            patch(doc, shape.block, ref_at, new as i32)
        }
        Op::RemoveAlpha { .. } => {
            if raw == -1 {
                return Err("the shape has no alpha property".into());
            }
            patch(doc, shape.block, ref_at, -1)
        }
        Op::SplitAlpha { .. } => {
            let new = split(doc, cur, m, shape)?;
            match new {
                Some(new) => patch(doc, shape.block, ref_at, new as i32),
                None => Ok(()),
            }
        }
        _ => Err("not an alpha op".into()),
    }
}

fn copy(doc: &mut NifDoc, block: u32) -> Result<u32, String> {
    let (kind, body) = (doc.kind(block).to_vec(), doc.block(block).to_vec());
    enc(doc.append(&kind, body))
}

/// Appends a private copy of the shape's alpha with deep copies of its controller chain, managed ones included;
/// every live sequence that drives the chain gets CBs for both alphas (C4-D1). Returns the new alpha block, or None
/// when the alpha has a single user.
fn split(
    doc: &mut NifDoc,
    cur: &[u8],
    m: &NifModel,
    shape: &ShapeModel,
) -> Result<Option<u32>, String> {
    let a = shape
        .alpha
        .and_then(|a| m.alphas.get(&a))
        .ok_or("the shape has no readable alpha property")?;
    if a.prop.users.len() <= 1 {
        return Ok(None);
    }
    // every check runs here, before the first append
    let plan = cb_plan(doc.header(), cur, m, shape, &a.prop)?;
    // refs are bounded by the source, not the growing doc
    let source_blocks = doc.len();
    // C4-D6: the alpha copy stays the first appended block: the edit after a split anchors there (edit_fields.rs)
    let new = copy(doc, a.prop.block)?;
    // (controller copy, Some((interpolator copy, data copy, pose floats))) in chain order
    let mut copies = vec![];
    for c in &a.prop.ctrls {
        let nc = copy(doc, *c)?;
        let mut ni = None;
        if let Some(i) = m.ctrls[c].interp.and_then(|i| m.interps.get(&(i as u32))) {
            let n = copy(doc, i.block)?;
            let data = u32::try_from(i.data)
                .ok()
                .filter(|&d| i.known && d < source_blocks);
            ni = Some((n, data.map(|d| copy(doc, d)).transpose()?, i.value.len()));
        }
        copies.push((nc, ni));
    }
    for (k, (nc, ni)) in copies.iter().enumerate() {
        patch(doc, *nc, 0, copies.get(k + 1).map_or(-1, |c| c.0 as i32))?;
        patch(doc, *nc, 22, new as i32)?;
        if let Some((n, nd, floats)) = ni {
            patch(doc, *nc, BASE_LEN, *n as i32)?;
            if let Some(nd) = nd {
                patch(doc, *n, 4 * floats, *nd as i32)?;
            }
        }
    }
    if let Some((first, _)) = copies.first() {
        let at = a.prop.net.controller_at() - doc.header().offsets()[a.prop.block as usize];
        patch(doc, new, at, *first as i32)?;
    }
    let pairs: Vec<(u32, u32)> = a
        .prop
        .ctrls
        .iter()
        .zip(&copies)
        .map(|(&c, k)| (c, k.0))
        .collect();
    split_cbs(doc, cur, plan, &pairs)?;
    Ok(Some(new))
}

/// (CB index, original controller) pairs: each CB takes that controller's copy.
type Moves = Vec<(usize, u32)>;
/// A shape a CB names: (block, raw name, name index).
type Named = (u32, Vec<u8>, i32);

/// The sequence, palette and MTTC work of a split, planned before anything is appended.
struct CbPlan {
    /// (sequence, its re-emit, the CBs that move to a copy)
    seqs: Vec<(u32, SequenceOut, Moves)>,
    /// (a manager that lists a changed sequence, the shapes its new CBs name)
    managers: Vec<(Manager, Vec<Named>)>,
}

/// The Node Name a new CB gives shape `s` (C4-D3, C4-D4): its raw Name index, which must name no other shape.
fn cb_node(h: &Header, m: &NifModel, s: u32) -> Result<Named, String> {
    let sm = m.shape(s).ok_or("not a shape")?;
    let raw = shape_raw_name(h, m, sm, sm.name.as_deref())?;
    Ok((s, raw.to_vec(), sm.net.name))
}

/// C4-D1: for each CB of a live sequence bound to a controller of `prop`. A CB naming the edited shape (raw bytes)
/// moves to the copy and a new CB keeps the original on the first remaining user; any other CB gets a new CB for the
/// copy naming the edited shape. New CBs share the CB's interpolator and strings (C4-D2).
fn cb_plan(
    h: &Header,
    cur: &[u8],
    m: &NifModel,
    shape: &ShapeModel,
    prop: &PropModel,
) -> Result<CbPlan, String> {
    // C4 fix 1: the model drops a listed sequence that does not read, so the plan could not see its CBs
    if prop.ctrls.iter().any(|&c| m.managed(c)) {
        for i in (0..h.sizes.len()).filter(|&i| h.kind(i) == b"NiControllerManager") {
            if let Ok(man) = read_manager(h, cur, i) {
                clip_names(h, cur, &man.sequences)?;
            }
        }
    }
    let edited = h.string(shape.net.name);
    let rest = prop.users.iter().copied().find(|&u| u != shape.block);
    let mut plan = CbPlan {
        seqs: vec![],
        managers: vec![],
    };
    for s in &m.sequences {
        let mut out: Option<SequenceOut> = None;
        let (mut to_copy, mut named) = (vec![], vec![]);
        for &c in &prop.ctrls {
            for k in crate::anim::bound_cbs(m, h, s, c) {
                if out.is_none() {
                    out = Some(enc(read_sequence_out(h, cur, s.block as usize))?);
                }
                let o = out.as_mut().unwrap();
                let cb = o.blocks.get(k).ok_or("a CB outside its sequence")?.clone();
                if edited.is_some() && h.string(cb.node) == edited {
                    // by ref now: this CB no longer needs its strings to find the copy
                    to_copy.push((k, c));
                    let n = cb_node(h, m, rest.ok_or("The alpha has no other user.")?)?;
                    o.blocks.push(CtrlBlockOut {
                        controller: c as i32,
                        node: n.2,
                        ..cb
                    });
                    named.push(n);
                } else {
                    let n = cb_node(h, m, shape.block)?;
                    to_copy.push((o.blocks.len(), c));
                    o.blocks.push(CtrlBlockOut { node: n.2, ..cb });
                    named.push(n);
                }
            }
        }
        let Some(o) = out else { continue };
        // C4-D5, C-P31: only the palette and MTTC of a manager that lists this sequence
        let man = u32::try_from(o.manager)
            .ok()
            .and_then(|g| read_manager(h, cur, g as usize).ok())
            .filter(|man| man.sequences.contains(&(s.block as i32)));
        if let Some(man) = man {
            let k = match plan.managers.iter().position(|p| p.0 == man) {
                Some(k) => k,
                None => {
                    plan.managers.push((man, vec![]));
                    plan.managers.len() - 1
                }
            };
            for n in named {
                if !plan.managers[k].1.iter().any(|x| x.0 == n.0) {
                    plan.managers[k].1.push(n);
                }
            }
        }
        plan.seqs.push((s.block, o, to_copy));
    }
    Ok(plan)
}

/// After the chain copy: re-emits the planned sequences with each marked CB on its controller's copy (`pairs`:
/// original, copy), then grows the palettes and the MTTC targets.
fn split_cbs(
    doc: &mut NifDoc,
    cur: &[u8],
    plan: CbPlan,
    pairs: &[(u32, u32)],
) -> Result<(), String> {
    for (seq, mut out, to_copy) in plan.seqs {
        for (k, c) in to_copy {
            let nc = pairs
                .iter()
                .find(|p| p.0 == c)
                .ok_or("a controller without a copy")?;
            out.blocks[k].controller = nc.1 as i32;
        }
        doc.set_block(seq, enc(encode_sequence(&out))?);
    }
    // C-P35 M3: `ensure_*` read the source block, so managers sharing a palette or an MTTC grow it once, together
    let mut pals: Vec<(i32, Vec<Named>)> = vec![];
    let mut mttcs: Vec<(i32, Vec<Named>)> = vec![];
    for (man, shapes) in plan.managers {
        for (list, block) in [(&mut pals, man.palette), (&mut mttcs, man.ctrl.next)] {
            let k = match list.iter().position(|p| p.0 == block) {
                Some(k) => k,
                None => {
                    list.push((block, vec![]));
                    list.len() - 1
                }
            };
            for n in &shapes {
                if !list[k].1.iter().any(|x| x.0 == n.0) {
                    list[k].1.push(n.clone());
                }
            }
        }
    }
    for (pal, shapes) in pals.into_iter().filter(|p| p.0 >= 0) {
        ensure_palette(doc, cur, pal, -1, &shapes)?;
    }
    for (next, shapes) in mttcs {
        ensure_mttc(doc, cur, next, &shapes)?;
    }
    Ok(())
}

/// C-P26/C-P29: `targets` with each of `shapes` it lacks appended; existing targets stay in order.
fn with_shapes(mut targets: Vec<i32>, shapes: &[Named]) -> Vec<i32> {
    for (s, _, _) in shapes {
        if !targets.contains(&(*s as i32)) {
            targets.push(*s as i32);
        }
    }
    targets
}

/// The manager's MTTC (its `next`, when that is a NiMultiTargetTransformController) gains each of `shapes` it lacks;
/// it is re-emitted only when its target list grew (C-P29 I1, C-P31).
fn ensure_mttc(doc: &mut NifDoc, cur: &[u8], next: i32, shapes: &[Named]) -> Result<(), String> {
    let mttc = u32::try_from(next)
        .ok()
        .filter(|&n| doc.kind(n) == b"NiMultiTargetTransformController");
    if let Some(n) = mttc {
        let (base, old_t) = enc(read_mttc(doc.header(), cur, n as usize))?;
        let targets = with_shapes(old_t.clone(), shapes);
        if targets != old_t {
            doc.set_block(n, enc(encode_mttc(&base, &targets))?);
        }
    }
    Ok(())
}

/// C2-D11: adds each of `shapes` (block, raw name, name index) the palette `pal` lacks, matched by AV Object ptr,
/// and re-emits it only when it grew. `pal` < 0 appends a new palette with Scene `scene`. Returns the palette.
fn ensure_palette(
    doc: &mut NifDoc,
    cur: &[u8],
    pal: i32,
    scene: i32,
    shapes: &[Named],
) -> Result<i32, String> {
    let (scene, mut objs) = match pal {
        p if p >= 0 => enc(read_palette(doc.header(), cur, p as usize))?,
        _ => (scene, vec![]),
    };
    let before = objs.len();
    for (s, raw, _) in shapes {
        if !objs.iter().any(|(_, p)| *p == *s as i32) {
            objs.push((raw.clone(), *s as i32));
        }
    }
    let body = enc(encode_palette(scene, &objs))?;
    if pal < 0 {
        return Ok(enc(doc.append(b"NiDefaultAVObjectPalette", body))? as i32);
    }
    if objs.len() > before {
        doc.set_block(pal as u32, body);
    }
    Ok(pal)
}

/// The controller's own keyed interpolator: (interpolator block, pose float count, data block, current keys).
/// A data ref of -1 reads as the empty group of the interpolator's kind (`key_type: None, keys: []`).
pub(crate) fn ctrl_keys(
    h: &Header,
    b: &[u8],
    m: &NifModel,
    ctrl: u32,
) -> Result<(u32, usize, Option<u32>, KeyData), String> {
    let c = m.ctrls.get(&ctrl).ok_or("not a readable controller")?;
    let i = match c.interp {
        Some(i) if i >= 0 => i as usize,
        _ => return Err("The controller has no interpolator.".into()),
    };
    let (floats, data, keys) = interp_keys(h, b, m, i)?;
    Ok((i as u32, floats, data, keys))
}

/// Interpolator `i`'s keys: (pose float count, data block, current keys). A data ref of -1 reads as the empty group
/// of the interpolator's kind.
pub(crate) fn interp_keys(
    h: &Header,
    b: &[u8],
    m: &NifModel,
    i: usize,
) -> Result<(usize, Option<u32>, KeyData), String> {
    if i >= h.sizes.len() {
        return Err("The interpolator ref is outside the file.".into()); // before h.kind, which does not check
    }
    let ip = match m.interps.get(&(i as u32)) {
        Some(ip) if ip.known => ip,
        ip => {
            let class = ip.map_or_else(
                || String::from_utf8_lossy(h.kind(i)).into_owned(),
                |ip| ip.class.clone(),
            );
            return Err(format!("A {class} has no keys to edit."));
        }
    };
    let floats = ip.value.len();
    let mismatch = || "The keys do not match the interpolator's type.".to_string();
    if ip.data == -1 {
        let empty = match floats {
            1 => KeyData::Float(KeyGroup {
                key_type: None,
                keys: vec![],
            }),
            3 => KeyData::Pos(KeyGroup {
                key_type: None,
                keys: vec![],
            }),
            _ => return Err(mismatch()),
        };
        return Ok((floats, None, empty));
    }
    if ip.data < -1 || ip.data as usize >= h.sizes.len() {
        return Err("The interpolator's data ref is outside the file.".into());
    }
    let d = ip.data as usize;
    let keys = read_data(h, b, d).map_err(|e| e.to_string())?;
    match (&keys, floats) {
        (KeyData::Float(_), 1) | (KeyData::Pos(_), 3) => Ok((floats, Some(d as u32), keys)),
        _ => Err(mismatch()),
    }
}

/// What one SetKeys edits: the interpolator, the refs that name it for this op, its float count, data and keys.
pub(crate) struct KeyTarget {
    pub interp: u32,
    /// (block, block-relative offset) of every ref this op re-points when it copies the interpolator.
    pub refs: Vec<(u32, usize)>,
    pub floats: usize,
    pub data: Option<u32>,
    pub keys: KeyData,
}

/// The interpolator SetKeys edits on controller `ctrl`: its own (`clip: None`), or the CB interpolator of sequence
/// `seq` bound to it (v2 B7, E1, E4).
pub(crate) fn key_target(
    h: &Header,
    b: &[u8],
    m: &NifModel,
    ctrl: u32,
    clip: Option<u32>,
) -> Result<KeyTarget, String> {
    let Some(seq) = clip else {
        let (interp, floats, data, keys) = ctrl_keys(h, b, m, ctrl)?;
        return Ok(KeyTarget {
            interp,
            refs: vec![(ctrl, BASE_LEN)],
            floats,
            data,
            keys,
        });
    };
    let s = m
        .sequences
        .iter()
        .find(|s| s.block == seq)
        .ok_or("This clip is not in this file.")?;
    let ks = crate::anim::bound_cbs(m, h, s, ctrl);
    let first = match ks.first() {
        None => return Err("This clip does not animate this controller.".into()),
        Some(&k) => s.blocks[k].interp,
    };
    if ks.iter().any(|&k| s.blocks[k].interp != first) {
        return Err(
            "This clip has several interpolators for this controller: they cannot be edited."
                .into(),
        );
    }
    let i =
        usize::try_from(first).map_err(|_| "This clip has no interpolator for this controller.")?;
    let (floats, data, keys) = interp_keys(h, b, m, i)?;
    Ok(KeyTarget {
        interp: i as u32,
        refs: ks.iter().map(|&k| (seq, 12 + k * CB_LEN)).collect(),
        floats,
        data,
        keys,
    })
}

/// Variable `old` -> `new` on managed controller `ctrl` (spec §5.5, E2, E3): every CB bound to it whose Controller
/// ID is `old` in decimal gets `new` in decimal; null, empty and other IDs stay. Patches each CB's 4-byte index only.
pub(crate) fn rewrite_cb_ids(
    doc: &mut NifDoc,
    m: &NifModel,
    ctrl: u32,
    old: u32,
    new: u32,
) -> Result<(), String> {
    let old_id = old.to_string().into_bytes();
    // `Header` is not Clone: collect under the shared borrow, mutate after it ends.
    let targets: Vec<(u32, usize)> = {
        let h = doc.header();
        m.sequences
            .iter()
            .flat_map(|s| {
                crate::anim::bound_cbs(m, h, s, ctrl)
                    .into_iter()
                    .filter(|&k| s.blocks[k].ctrl_id.as_deref() == Some(&old_id[..]))
                    .map(|k| (s.block, 12 + k * CB_LEN + 21))
                    .collect::<Vec<_>>()
            })
            .collect()
    };
    if targets.is_empty() {
        return Ok(()); // 0x20 with no clip, or only null/empty IDs: nothing names the variable
    }
    let idx = enc(doc.string(new.to_string().as_bytes()))?;
    for (seq, at) in targets {
        patch(doc, seq, at, idx)?;
    }
    Ok(())
}

/// SetKeys on the controller `ctrl`, its own keys or (clip Some) a clip's. Ok(Conflict) for a stale base.
pub(crate) fn set_keys(
    doc: &mut NifDoc,
    cur: &[u8],
    m: &NifModel,
    ctrl: u32,
    op: &Op,
) -> Result<Outcome, String> {
    let Op::SetKeys {
        keys,
        base,
        clip,
        clip_name,
        ..
    } = op
    else {
        return Err("not a key op".into());
    };
    // C-P35 I1: a revert can shift another clip onto this block; its name tells them apart
    if let (Some(seq), Some(want)) = (clip, clip_name) {
        let s = m.sequences.iter().find(|s| s.block == *seq);
        if s.is_some_and(|s| s.name.as_deref() != Some(want.as_str())) {
            return Ok(Outcome::Conflict(format!(
                "The clip {want} is no longer at its place in the file."
            )));
        }
    }
    let t = key_target(doc.header(), cur, m, ctrl, *clip)?;
    let (i, floats, data, now) = (t.interp, t.floats, t.data, &t.keys);
    let kind: &[u8] = match (keys, now) {
        (KeyData::Float(_), KeyData::Float(_)) => b"NiFloatData",
        (KeyData::Pos(_), KeyData::Pos(_)) => b"NiPosData",
        _ => return Err("The keys do not match the interpolator's type.".into()),
    };
    // bytes, not KeyData ==: NaN != NaN (F19)
    if encode_data(base) != encode_data(now) {
        return Ok(Outcome::Conflict(
            "changed in the source: the keys differ from the edit's base".into(),
        ));
    }
    let interp_users = m
        .ctrls
        .values()
        .filter(|c| c.interp == Some(i as i32))
        .count()
        + m.sequences
            .iter()
            .flat_map(|s| &s.blocks)
            .filter(|cb| cb.interp == i as i32)
            .count();
    // shortcut: m.interps holds only interpolators a controller or controlled block names, so an
    // orphan interpolator on the same data is not counted; count raw refs if orphans ever come back to life.
    let data_users = data.map_or(0, |d| {
        m.interps.values().filter(|x| x.data == d as i32).count()
    });
    // E5: copy when something other than the refs this op re-points also names the interpolator.
    if interp_users > t.refs.len() {
        let ni = copy(doc, i)?;
        for &(blk, at) in &t.refs {
            patch(doc, blk, at, ni as i32)?;
        }
        let nd = enc(doc.append(kind, encode_data(keys)))?;
        patch(doc, ni, 4 * floats, nd as i32)?;
    } else if let Some(d) = data.filter(|_| data_users <= 1) {
        doc.set_block(d, encode_data(keys));
    } else {
        let nd = enc(doc.append(kind, encode_data(keys)))?;
        patch(doc, i, 4 * floats, nd as i32)?;
    }
    Ok(Outcome::Applied)
}

/// Block-relative offset of the property's own controller ref.
fn ctrl_ref_at(doc: &NifDoc, p: &PropModel) -> usize {
    p.net.controller_at() - doc.header().offsets()[p.block as usize]
}

/// Err unless the property's chain ends in -1. `ctrl::chain` stops at a loop, an out-of-range or unreadable next and
/// past 256 links, so in each of those cases the last listed controller's next is not -1 (B-P31).
fn whole_chain(m: &NifModel, p: &PropModel) -> Result<(), String> {
    let end = match p.ctrls.last() {
        None => p.net.controller == -1,
        Some(c) => m.ctrls.get(c).is_some_and(|c| c.next == -1),
    };
    if end {
        Ok(())
    } else {
        Err("This controller chain is broken: it cannot be edited.".into())
    }
}

/// The lighting and alpha properties whose chains list controller `block`.
fn owners(m: &NifModel, block: u32) -> Vec<&PropModel> {
    m.lighting
        .values()
        .map(|l| &l.prop)
        .chain(m.alphas.values().map(|a| &a.prop))
        .filter(|p| p.ctrls.contains(&block))
        .collect()
}

/// AddController (anchored on a property) and RemoveController (anchored on a controller).
pub(crate) fn ctrl_op(
    doc: &mut NifDoc,
    m: &NifModel,
    at: &Anchor,
    block: u32,
    op: &Op,
) -> Result<Outcome, String> {
    match op {
        Op::AddController {
            class,
            var,
            keys,
            flags,
            freq,
            start,
            stop,
            ..
        } => {
            let shader = matches!(at.role, Role::Shader);
            let (var_ok, float) = match class.as_str() {
                LSCF if shader => (var.is_some_and(|v| LSCF_VARS.contains(&v)), true),
                LSCC if shader => (matches!(var, Some(0 | 1)), false),
                TESTREF if at.role == Role::Alpha => (var.is_none(), true),
                LSCF | LSCC => return Err(format!("{class} goes on a shader property.")),
                TESTREF => return Err(format!("{class} goes on an alpha property.")),
                _ => return Err(format!("{class} cannot be added.")),
            };
            if !var_ok {
                return Err(format!("{class} does not take variable {var:?}."));
            }
            let pbr = m
                .lighting
                .get(&block)
                .is_some_and(|l| l.lighting.slsf2 & SLSF2_TRUE_PBR != 0);
            if class == LSCF && *var == Some(8) && pbr {
                return Err(ENV_SCALE_PBR.into());
            }
            if key_count(keys, float)? == 0 {
                return Err("A new controller needs at least one key.".into());
            }
            let prop = if shader {
                m.lighting.get(&block).map(|l| &l.prop)
            } else {
                m.alphas.get(&block).map(|a| &a.prop)
            }
            .ok_or("not a readable property")?;
            whole_chain(m, prop)?;
            let last = prop.ctrls.last();
            // chains that share a tail stay merged to the end, so the last link tells (Note CY4)
            if last.is_some_and(|&l| owners(m, l).len() > 1) {
                return Err("This controller chain is shared by several properties: a controller cannot be added.".into());
            }
            if m.root_net.is_none() {
                return Err("The root is not a node: the BSX Animated bit cannot be set.".into());
            }
            let i = append_keyed(doc, keys)?;
            let mut cb = (-1i32).to_le_bytes().to_vec();
            cb.extend(flags.to_le_bytes());
            for f in [*freq, 0.0, *start, *stop] {
                cb.extend(f.to_le_bytes());
            }
            cb.extend((block as i32).to_le_bytes());
            cb.extend((i as i32).to_le_bytes());
            cb.extend(var.map(u32::to_le_bytes).into_iter().flatten());
            let c = enc(doc.append(class.as_bytes(), cb))?;
            match last {
                None => patch(doc, block, ctrl_ref_at(doc, prop), c as i32)?,
                Some(&l) => patch(doc, l, 0, c as i32)?,
            }
            set_bsx_animated(doc, m)?;
        }
        Op::RemoveController { .. } => {
            if m.managed(block) {
                return Err("A manager-controlled controller cannot be removed.".into());
            }
            let prop =
                match owners(m, block).as_slice() {
                    [] => return Err("No property lists this controller.".into()),
                    [p] => *p,
                    _ => return Err(
                        "This controller is shared by several properties: it cannot be removed."
                            .into(),
                    ),
                };
            whole_chain(m, prop)?;
            let k = prop.ctrls.iter().position(|&c| c == block).unwrap();
            let c = &m.ctrls[&block];
            match k {
                0 => patch(doc, prop.block, ctrl_ref_at(doc, prop), c.next)?,
                _ => patch(doc, prop.ctrls[k - 1], 0, c.next)?,
            }
            doc.patch(block, 4, &(c.flags & !0x08).to_le_bytes())
                .map_err(|e| e.to_string())?;
        }
        _ => return Err("not a controller op".into()),
    }
    Ok(Outcome::Applied)
}

/// The key count of `keys` when they fit a Float (`float`) or Point3 controller, else Err.
fn key_count(keys: &KeyData, float: bool) -> Result<usize, String> {
    match keys {
        KeyData::Float(g) if float => Ok(g.keys.len()),
        KeyData::Pos(g) if !float => Ok(g.keys.len()),
        _ => Err("The keys do not match the controller's type.".into()),
    }
}

/// Appends `keys` as NiFloatData / NiPosData and a keyed interpolator over it whose value is the first key's.
fn append_keyed(doc: &mut NifDoc, keys: &KeyData) -> Result<u32, String> {
    let (data_kind, interp_kind, first): (&[u8], &[u8], Vec<u8>) = match keys {
        KeyData::Float(g) => (
            b"NiFloatData",
            b"NiFloatInterpolator",
            g.keys
                .first()
                .map_or(vec![], |k| k.value.to_le_bytes().to_vec()),
        ),
        KeyData::Pos(g) => (
            b"NiPosData",
            b"NiPoint3Interpolator",
            g.keys.first().map_or(vec![], |k| {
                k.value.iter().flat_map(|f| f.to_le_bytes()).collect()
            }),
        ),
        KeyData::Bool(_) => return Err("The keys do not match the controller's type.".into()),
    };
    let d = enc(doc.append(data_kind, encode_data(keys)))?;
    let mut ib = first;
    ib.extend((d as i32).to_le_bytes());
    enc(doc.append(interp_kind, ib))
}

fn enc<T, E: std::fmt::Display>(r: Result<T, E>) -> Result<T, String> {
    r.map_err(|e| e.to_string())
}

/// #INV_FLT#: the Value every vanilla blend interpolator holds (C1 census, 6722 of 6722).
const INV_FLT: u32 = 0xFF7F_FFFF;

/// The first NiControllerManager on the root's controller chain. The whole chain is checked to its end (C-P35 M2,
/// Note CU4, fix round 1): every entry reads, every manager's payload reads, no loop and no link outside -1 and the
/// file; otherwise Err, so no clip op runs on a chain it cannot see whole.
fn root_manager(h: &Header, cur: &[u8], m: &NifModel) -> Result<Option<u32>, String> {
    let Some(net) = m.root_net.as_ref() else {
        return Ok(None);
    };
    let broken = || Err("The root's controller chain cannot be read.".into());
    let (mut c, mut seen, mut first) = (net.controller, vec![], None);
    while c != -1 {
        let Some(t) = u32::try_from(c).ok().and_then(|b| m.ctrls.get(&b)) else {
            return broken();
        };
        if seen.contains(&t.block) {
            return broken(); // a loop
        }
        seen.push(t.block);
        if t.class == "NiControllerManager" {
            if read_manager(h, cur, t.block as usize).is_err() {
                return broken();
            }
            first = first.or(Some(t.block));
        }
        c = t.next;
    }
    Ok(first)
}

/// The raw Name of each listed sequence (None: no name). A listed sequence that does not read is an Err (C-P29).
fn clip_names<'h>(
    h: &'h Header,
    cur: &[u8],
    seqs: &[i32],
) -> Result<Vec<Option<&'h [u8]>>, String> {
    seqs.iter()
        .map(|&s| {
            let i = usize::try_from(s)
                .map_err(|_| format!("The manager lists a null sequence ({s})."))?;
            Ok(h.string(enc(read_sequence_out(h, cur, i))?.name))
        })
        .collect()
}

/// (manager, the manager as read, list position) of the `nth` clip named `name` on the root's manager.
fn find_clip(
    h: &Header,
    cur: &[u8],
    m: &NifModel,
    name: &str,
    nth: u32,
) -> Result<Option<(u32, Manager, usize)>, String> {
    let Some(mb) = root_manager(h, cur, m)? else {
        return Ok(None);
    };
    let man = enc(read_manager(h, cur, mb as usize))?;
    let names = clip_names(h, cur, &man.sequences)?;
    let k = (0..names.len())
        .filter(|&k| names[k].is_some_and(|r| clip_named(r, name)))
        .nth(nth as usize);
    Ok(k.map(|k| (mb, man, k)))
}

/// `find_clip`'s match: the Windows-1252 name the editor shows, or the model's UTF-8 view of it.
fn clip_named(r: &[u8], name: &str) -> bool {
    crate::text::lossy(r) == name || String::from_utf8_lossy(r) == name
}

/// The (name, nth) that Remove and Rename find sequence `seq` by on the root's manager (plan C5-D2): its
/// Windows-1252 name and how many earlier list entries match that name too. None when the root's manager does not
/// list it, it has no name, or the manager or a listed sequence does not read. Parses the header: call it on a click.
pub fn clip_key(b: &[u8], m: &NifModel, seq: u32) -> Option<(String, u32)> {
    let h = Header::parse(b).ok()?;
    let man = read_manager(&h, b, root_manager(&h, b, m).ok()?? as usize).ok()?;
    let names = clip_names(&h, b, &man.sequences).ok()?;
    let k = man.sequences.iter().position(|&s| s == seq as i32)?;
    let name = crate::text::lossy(names[k]?);
    let nth = names[..k]
        .iter()
        .flatten()
        .filter(|r| clip_named(r, &name))
        .count();
    Some((name, nth as u32))
}

/// The bytes of a new clip name (F20): non-empty, Windows-1252, unique on the manager ignoring ASCII case.
fn clip_name_bytes(
    h: &Header,
    cur: &[u8],
    man: Option<&Manager>,
    name: &str,
    skip: Option<usize>,
) -> Result<Vec<u8>, String> {
    let b = win1252(name)?;
    if b.is_empty() {
        return Err("A clip needs a name.".into());
    }
    let names = clip_names(h, cur, man.map_or(&[][..], |m| &m.sequences))?;
    let taken = (0..names.len())
        .filter(|&k| Some(k) != skip)
        .any(|k| names[k].is_some_and(|r| r.eq_ignore_ascii_case(&b)));
    if taken {
        return Err(format!("Another clip is already named {name}."));
    }
    Ok(b)
}

/// The shape a clip track on controller `c` names (C2-D6, F11, F18), after its class and keys pass.
pub(crate) fn track_shape(
    h: &Header,
    m: &NifModel,
    c: u32,
    t: &super::ClipTrack,
) -> Result<u32, String> {
    let ctrl = m.ctrls.get(&c).ok_or("not a readable controller")?;
    let float = match ctrl.class.as_str() {
        LSCF | TESTREF if ctrl.known => true,
        LSCC if ctrl.known => false,
        _ => return Err(format!("A {} cannot be in a clip.", ctrl.class)),
    };
    if key_count(&t.keys, float)? == 0 {
        return Err("A clip track needs at least one key.".into());
    }
    let (prop, alpha) = m
        .lighting
        .values()
        .map(|l| (&l.prop, false))
        .chain(m.alphas.values().map(|a| (&a.prop, true)))
        .find(|(p, _)| p.ctrls.contains(&c))
        .ok_or("No property lists this controller.")?;
    whole_chain(m, prop)?;
    if alpha && prop.users.len() > 1 {
        return Err(format!(
            "This alpha is shared by {} shapes: split it before editing.",
            prop.users.len()
        ));
    }
    let unnamed = || "This shape has no name: a clip cannot find it.".to_string();
    let name = t.at.shape.as_deref().ok_or_else(unnamed)?;
    let two = || format!("Two shapes are named {name}: a clip cannot tell them apart.");
    let users: Vec<&ShapeModel> = prop
        .users
        .iter()
        .filter_map(|&u| m.shape(u))
        .filter(|s| s.name.as_deref() == Some(name))
        .collect();
    let s = match users.as_slice() {
        [s] => *s,
        [] => return Err(format!("No shape named {name} uses this controller.")),
        _ => return Err(two()),
    };
    shape_raw_name(h, m, s, Some(name))?;
    Ok(s.block)
}

/// The raw Name bytes of shape `s`, which a clip binds by (C2-D6, F18): Err when it has none or another shape has the
/// same bytes. `shown` names the shape in the message (the raw bytes decoded when None).
fn shape_raw_name<'h>(
    h: &'h Header,
    m: &NifModel,
    s: &ShapeModel,
    shown: Option<&str>,
) -> Result<&'h [u8], String> {
    let raw = h
        .string(s.net.name)
        .ok_or("This shape has no name: a clip cannot find it.")?;
    if m.shapes
        .iter()
        .filter(|o| h.string(o.net.name) == Some(raw))
        .count()
        > 1
    {
        let name = shown.map_or_else(|| crate::text::lossy(raw), str::to_string);
        return Err(format!(
            "Two shapes are named {name}: a clip cannot tell them apart."
        ));
    }
    Ok(raw)
}

/// AddSequence, RemoveSequence and RenameSequence, anchored on the root (v2 C2). A missing clip is a Conflict.
pub(crate) fn clip_op(
    doc: &mut NifDoc,
    cur: &[u8],
    m: &NifModel,
    root: u32,
    op: &Op,
) -> Result<Outcome, String> {
    let missing = |name: &str| Ok(Outcome::Conflict(format!("No clip named {name}.")));
    match op {
        Op::AddSequence {
            name,
            cycle,
            freq,
            start,
            stop,
            tracks,
            ..
        } => {
            if m.root != Some(root) || m.root_net.is_none() {
                return Err("The root is not a node: a clip needs a node root.".into());
            }
            let mut res = Vec::with_capacity(tracks.len());
            for t in tracks {
                // track anchors re-resolve on every apply; rebase never writes them back (F3)
                let c = match super::resolve(m, &t.at) {
                    Ok((c, _)) => c,
                    Err(e) => return Ok(Outcome::Conflict(e)),
                };
                res.push((c, track_shape(doc.header(), m, c, t)?, t.keys.clone()));
            }
            add_sequence(doc, cur, m, name, *cycle, *freq, *start, *stop, &res)?;
        }
        Op::RemoveSequence { name, nth, .. } => {
            let Some((mgr, mut man, k)) = find_clip(doc.header(), cur, m, name, *nth)? else {
                return missing(name);
            };
            let removed = man.sequences.remove(k);
            if man.sequences.is_empty() {
                unlink_manager(doc, m, root, mgr, &man)?;
            }
            doc.set_block(mgr, enc(encode_manager(&man))?);
            // C-P29: the union over every readable manager after the remove, as read_model counts live sequences
            let h = doc.header();
            let mut listed: Vec<i32> = man.sequences.clone();
            for i in (0..h.sizes.len()).filter(|&i| i != mgr as usize) {
                if h.kind(i) == b"NiControllerManager" {
                    if let Ok(o) = read_manager(h, cur, i) {
                        listed.extend(o.sequences);
                    }
                }
            }
            if let Some(rs) = m.sequences.iter().find(|s| s.block as i32 == removed) {
                release(doc, m, rs, &listed)?;
            }
        }
        Op::RenameSequence {
            name,
            nth,
            new_name,
            ..
        } => {
            let Some((_, man, k)) = find_clip(doc.header(), cur, m, name, *nth)? else {
                return missing(name);
            };
            let b = clip_name_bytes(doc.header(), cur, Some(&man), new_name, Some(k))?;
            let s = enc(doc.string(&b))?;
            patch(doc, man.sequences[k] as u32, 0, s)?;
        }
        _ => return Err("not a clip op".into()),
    }
    Ok(Outcome::Applied)
}

/// C-P36 (Note CU8, C6E): the game crashes on a manager with no sequences, so the emptied manager and the MTTC after
/// it leave the root's chain (its link goes to what followed them); export cleanup (Note CQ) then drops them with the
/// palette. Call before any block is re-emitted: the offsets are the ones `m` was read with.
fn unlink_manager(
    doc: &mut NifDoc,
    m: &NifModel,
    root: u32,
    mgr: u32,
    man: &Manager,
) -> Result<(), String> {
    let net = m
        .root_net
        .as_ref()
        .ok_or("The root is not a node: a clip needs a node root.")?;
    let mut after = man.ctrl.next;
    if let Some(t) = u32::try_from(after).ok().and_then(|b| m.ctrls.get(&b)) {
        if t.class == "NiMultiTargetTransformController" {
            after = t.next;
        }
    }
    if net.controller == mgr as i32 {
        let at = net.controller_at() - doc.header().offsets()[root as usize];
        return patch(doc, root, at, after);
    }
    // root_manager walked this chain (whole, no loop), so the manager is on it; another chain may also link to the
    // manager, so the predecessor is found on the root's own chain (Note CY3)
    let mut c = net.controller;
    loop {
        let t = u32::try_from(c)
            .ok()
            .and_then(|b| m.ctrls.get(&b))
            .ok_or("The root's controller chain cannot be read.")?;
        if t.next == mgr as i32 {
            return patch(doc, t.block, 0, after);
        }
        c = t.next;
    }
}

/// AddSequence's cycle and range refusals; `convert_check` predicts both with the same text (Note CU3).
const BAD_CYCLE: &str = "The cycle must be Loop, Reverse or Clamp.";
const BAD_RANGE: &str = "Stop must be after start.";

/// The controllers ConvertChainToClips would take from property `prop` (chain order), or why it cannot run.
/// Model only, no header parse: the Controllers tab calls it every frame. `shape` is the anchored shape's name.
pub fn convert_check(m: &NifModel, prop: u32, shape: Option<&str>) -> Result<Vec<u32>, String> {
    if m.root_net.is_none() {
        return Err("The root is not a node: a clip needs a node root.".into());
    }
    let (p, alpha) = match (m.lighting.get(&prop), m.alphas.get(&prop)) {
        (Some(l), _) => (&l.prop, false),
        (_, Some(a)) => (&a.prop, true),
        _ => return Err("not a readable property".into()),
    };
    if alpha && p.users.len() > 1 {
        return Err(format!(
            "This alpha is shared by {} shapes: split it before editing.",
            p.users.len()
        ));
    }
    let name = shape.ok_or("This shape has no name: a clip cannot find it.")?;
    if m.shapes
        .iter()
        .filter(|s| s.name.as_deref() == Some(name))
        .count()
        > 1
    {
        return Err(format!(
            "Two shapes are named {name}: a clip cannot tell them apart."
        ));
    }
    // anim::free_tracks_in's filter (known, Active, not managed), plus a clip class and key data (C3-D1)
    let free: Vec<&crate::ctrl::TimeCtrl> = p
        .ctrls
        .iter()
        .filter_map(|c| m.ctrls.get(c))
        .filter(|c| c.known && c.flags & 0x08 != 0 && !m.managed(c.block))
        .filter(|c| [LSCF, LSCC, TESTREF].contains(&c.class.as_str()))
        .filter(|c| {
            c.interp
                .and_then(|i| u32::try_from(i).ok())
                .and_then(|i| m.interps.get(&i))
                .is_some_and(|i| i.data >= 0)
        })
        .collect();
    let Some(first) = free.first() else {
        return Err("This property has no free controllers with keys.".into());
    };
    // a sequence has one cycle, frequency and range, and no phase or Play Backwards (F12)
    let same = |c: &crate::ctrl::TimeCtrl| {
        (c.flags >> 1) & 3 == (first.flags >> 1) & 3
            && c.frequency.to_bits() == first.frequency.to_bits()
            && c.phase == 0.0
            && c.flags & 0x10 == 0
    };
    if !free.iter().all(|c| same(c)) {
        return Err(
            "These controllers run on different timings: they cannot share one clip.".into(),
        );
    }
    // C-P30: the union fold's f32::min/max would hide a NaN or infinite bound behind a finite neighbour
    if !free
        .iter()
        .all(|c| c.start.is_finite() && c.stop.is_finite())
    {
        return Err("This animation has invalid times: it cannot be converted.".into());
    }
    // Note CU3: the checks AddSequence makes on the clip convert_op builds (cycle bits, the union range)
    if (first.flags >> 1) & 3 == 3 {
        return Err(BAD_CYCLE.into());
    }
    let (start, stop) = free
        .iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), c| {
            (a.min(c.start), b.max(c.stop))
        });
    if stop <= start {
        return Err(BAD_RANGE.into());
    }
    Ok(free.iter().map(|c| c.block).collect())
}

/// ConvertChainToClips on property `prop` (resolved from `at`): gathers the tracks and runs C2's AddSequence path.
pub(crate) fn convert_op(
    doc: &mut NifDoc,
    cur: &[u8],
    m: &NifModel,
    at: &Anchor,
    prop: u32,
    op: &Op,
) -> Result<Outcome, String> {
    let Op::ConvertChainToClips { name, .. } = op else {
        return Err("not a convert op".into());
    };
    let ctrls = convert_check(m, prop, at.shape.as_deref())?;
    let (owner, chain) = match m.lighting.get(&prop) {
        Some(l) => (Owner::Shader, &l.prop.ctrls),
        None => (Owner::Alpha, &m.alphas[&prop].prop.ctrls),
    };
    let mut tracks = Vec::with_capacity(ctrls.len());
    for &c in &ctrls {
        let (_, _, _, keys) = ctrl_keys(doc.header(), cur, m, c)?;
        let t = &m.ctrls[&c];
        // nth as the Controllers tab counts it: earlier controllers of the chain with this class and variable
        let i = chain.iter().position(|&b| b == c).unwrap_or(0);
        let nth = chain[..i]
            .iter()
            .filter_map(|b| m.ctrls.get(b))
            .filter(|p| p.class == t.class && p.var == t.var)
            .count() as u32;
        let at = Anchor {
            block: c,
            kind: t.class.clone(),
            shape: at.shape.clone(),
            role: Role::Ctrl {
                owner,
                class: t.class.clone(),
                var: t.var,
                nth,
            },
        };
        tracks.push(ClipTrack { at, keys });
    }
    let t = &m.ctrls[&ctrls[0]];
    let (start, stop) = ctrls
        .iter()
        .map(|c| &m.ctrls[c])
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), c| {
            (a.min(c.start), b.max(c.stop))
        });
    let no_root = "The root is not a node: a clip needs a node root.";
    let root_at = super::anchor_role(m, None, Role::Root).ok_or(no_root)?;
    let add = Op::AddSequence {
        at: root_at,
        name: name.clone(),
        cycle: ((t.flags >> 1) & 3) as u32,
        freq: t.frequency,
        start,
        stop,
        tracks,
    };
    clip_op(doc, cur, m, m.root.ok_or(no_root)?, &add)
}

/// G2: each known controller the removed clip `rs` drove and no sequence in `listed` drives (`rs` too, when another
/// manager still lists it) gets 0x20 cleared, `rs`'s keyed interpolator back (when it is a plain one of the
/// controller's kind), and `rs`'s timing, so its keys play as they did in the clip (C-P29): frequency, start and
/// stop from the sequence, phase 0, cycle bits from its cycle type, Play Backwards cleared (a clip never sets it).
fn release(
    doc: &mut NifDoc,
    m: &NifModel,
    rs: &crate::ctrl::Sequence,
    listed: &[i32],
) -> Result<(), String> {
    let mut fix = vec![];
    {
        let h = doc.header();
        // bindings once per sequence, not per controller x property (Note CY5)
        let driven: std::collections::BTreeSet<u32> = m
            .sequences
            .iter()
            .filter(|s| listed.contains(&(s.block as i32)))
            .flat_map(|s| crate::anim::bound_map(m, h, s).into_keys())
            .collect();
        for (c, ks) in crate::anim::bound_map(m, h, rs) {
            let Some(ctrl) = m.ctrls.get(&c).filter(|c| c.known) else {
                continue;
            };
            if driven.contains(&c) {
                continue;
            }
            let k = ks[0];
            let ip = rs.blocks[k].interp;
            let want: &[u8] = if ctrl.class == LSCC {
                b"NiPoint3Interpolator"
            } else {
                b"NiFloatInterpolator"
            };
            let plain = u32::try_from(ip).is_ok_and(|i| doc.kind(i) == want);
            let flags = (ctrl.flags & !(0x20 | 0x10 | 0x06)) | ((rs.cycle.min(3) as u16) << 1);
            fix.push((c, flags, plain.then_some(ip)));
        }
    }
    for (c, flags, ip) in fix {
        // flags, frequency, phase, start, stop: bytes 4..22 of the NiTimeController base
        let mut t = flags.to_le_bytes().to_vec();
        for f in [rs.frequency, 0.0, rs.start, rs.stop] {
            t.extend(f.to_le_bytes());
        }
        doc.patch(c, 4, &t).map_err(|e| e.to_string())?;
        if let Some(ip) = ip {
            patch(doc, c, BASE_LEN, ip)?;
        }
    }
    Ok(())
}

/// Appends one clip to the root's manager, creating the manager, its MTTC and palette when the root has none.
/// `tracks` are resolved and checked by [`track_shape`]: (controller block, shape block, keys).
#[allow(clippy::too_many_arguments)]
pub(crate) fn add_sequence(
    doc: &mut NifDoc,
    cur: &[u8],
    m: &NifModel,
    name: &str,
    cycle: u32,
    freq: f32,
    start: f32,
    stop: f32,
    tracks: &[(u32, u32, KeyData)],
) -> Result<(), String> {
    let (root, net) = m
        .root
        .zip(m.root_net.as_ref())
        .ok_or("The root is not a node: a clip needs a node root.")?;
    let found = root_manager(doc.header(), cur, m)?;
    let old = match found {
        Some(mb) => Some(enc(read_manager(doc.header(), cur, mb as usize))?),
        None => None,
    };
    let name_b = clip_name_bytes(doc.header(), cur, old.as_ref(), name, None)?;
    if ![freq, start, stop].iter().all(|v| v.is_finite()) {
        return Err("Clip times must be finite numbers.".into());
    }
    if cycle > 2 {
        return Err(BAD_CYCLE.into());
    }
    if stop.partial_cmp(&start) != Some(std::cmp::Ordering::Greater) {
        return Err(BAD_RANGE.into());
    }
    if tracks.is_empty() {
        return Err("A clip needs at least one track.".into());
    }
    if (1..tracks.len()).any(|k| tracks[..k].iter().any(|o| o.0 == tracks[k].0)) {
        return Err("Two tracks of a clip drive the same controller.".into());
    }
    // (shape block, raw name) per track, read before anything is appended
    let shapes: Vec<(u32, Vec<u8>, i32)> = tracks
        .iter()
        .map(|&(_, s, _)| {
            let sm = m.shape(s).ok_or("not a shape")?;
            let raw = doc
                .header()
                .string(sm.net.name)
                .ok_or("This shape has no name: a clip cannot find it.")?;
            Ok((s, raw.to_vec(), sm.net.name))
        })
        .collect::<Result<_, String>>()?;

    // C-P26/C-P29: the MTTC lists the nodes the clips bind; existing targets stay, missing ones are appended
    let (mgr, mut man) = match (found, old) {
        (Some(mb), Some(man)) => {
            ensure_mttc(doc, cur, man.ctrl.next, &shapes)?;
            (mb, man)
        }
        _ => {
            // F21: a placeholder manager, filled by set_block at the end
            let mgr = enc(doc.append(b"NiControllerManager", vec![]))?;
            let base = |next: i32, flags: u16| TimeCtrl {
                block: 0,
                class: String::new(),
                next,
                flags,
                // C-P29 census: all 1051 vanilla managers and their MTTCs run freq 1, phase 0 over the
                // FLT_MAX / -FLT_MAX range, not the clips' range
                frequency: 1.0,
                phase: 0.0,
                start: f32::MAX,
                stop: f32::MIN,
                target: root as i32,
                interp: None,
                var: None,
                known: false,
            };
            let targets = with_shapes(vec![], &shapes);
            let mttc_body = enc(encode_mttc(&base(net.controller, MTTC_FLAGS), &targets))?;
            let mttc = enc(doc.append(b"NiMultiTargetTransformController", mttc_body))?;
            let man = Manager {
                ctrl: base(mttc as i32, MANAGER_FLAGS),
                cumulative: 0,
                sequences: vec![],
                palette: -1,
            };
            (mgr, man)
        }
    };

    man.palette = ensure_palette(doc, cur, man.palette, root as i32, &shapes)?;

    let mut blocks = Vec::with_capacity(tracks.len());
    for ((c, _, keys), (_, _, node)) in tracks.iter().zip(&shapes) {
        let ctrl = &m.ctrls[c];
        let point3 = ctrl.class == LSCC;
        let interp = append_keyed(doc, keys)?;
        let blend = if point3 {
            "NiBlendPoint3Interpolator"
        } else {
            "NiBlendFloatInterpolator"
        };
        let has_blend = ctrl
            .interp
            .and_then(|i| u32::try_from(i).ok())
            .is_some_and(|i| doc.kind(i) == blend.as_bytes());
        if !has_blend {
            let inv = f32::from_bits(INV_FLT);
            let v = if point3 { vec![inv; 3] } else { vec![inv] };
            let bl = enc(doc.append(blend.as_bytes(), enc(encode_blend_interp(blend, &v))?))?;
            patch(doc, *c, BASE_LEN, bl as i32)?;
        }
        doc.patch(*c, 4, &(ctrl.flags | 0x20).to_le_bytes())
            .map_err(|e| e.to_string())?;
        let testref = ctrl.class == TESTREF;
        let ptype: &[u8] = if testref {
            b"NiAlphaProperty"
        } else {
            b"BSLightingShaderProperty"
        };
        let property = enc(doc.string(ptype))?;
        let ctrl_type = enc(doc.string(ctrl.class.as_bytes()))?;
        // F0: TestRef CBs carry a null Controller ID, as vanilla does
        let ctrl_id = match ctrl.var {
            Some(v) if !testref => enc(doc.string(v.to_string().as_bytes()))?,
            _ => -1,
        };
        blocks.push(CtrlBlockOut {
            interp: interp as i32,
            controller: *c as i32,
            priority: 0,
            node: *node,
            property,
            ctrl_type,
            ctrl_id,
            interp_id: -1,
        });
    }
    let s_name = enc(doc.string(&name_b))?;
    let (s_start, s_end) = (enc(doc.string(b"start"))?, enc(doc.string(b"end"))?);
    let tk_body = enc(encode_text_keys(-1, &[(start, s_start), (stop, s_end)]))?;
    let tk = enc(doc.append(b"NiTextKeyExtraData", tk_body))?;
    let seq = SequenceOut {
        name: s_name,
        blocks,
        array_grow_by: 1,
        weight: 1.0,
        text_keys: tk as i32,
        cycle,
        freq,
        start,
        stop,
        manager: mgr as i32,
        accum_root: net.name,
        notes: vec![],
    };
    let seq = enc(doc.append(b"NiControllerSequence", enc(encode_sequence(&seq))?))?;
    man.sequences.push(seq as i32);
    doc.set_block(mgr, enc(encode_manager(&man))?);
    set_bsx_animated(doc, m)?;
    if found.is_none() {
        // after the BSX add, which may re-emit the root's NiObjectNET with one more extra ref (F4: a 4-byte patch)
        let s = net.start - doc.header().offsets()[root as usize];
        let n = doc
            .block(root)
            .get(s + 4..s + 8)
            .map(|x| u32::from_le_bytes(x.try_into().unwrap()) as usize)
            .ok_or("NiObjectNET outside its block")?;
        patch(doc, root, s + 8 + 4 * n, mgr as i32)?;
    }
    Ok(())
}

/// Sets bit 0 (Animated) on the root's first BSXFlags, appending `BSXFlags "BSX"` = 1 to the root when it has none.
fn set_bsx_animated(doc: &mut NifDoc, m: &NifModel) -> Result<(), String> {
    let (root, net) = m
        .root
        .zip(m.root_net.as_ref())
        .ok_or("The root is not a node: the BSX Animated bit cannot be set.")?;
    // the first BSXFlags whatever its name, as validate::root_bsx picks it
    let found = m.extras_of(net).into_iter().find_map(|e| match e.value {
        Some(ExtraValue::Bsx(v)) if e.kind == "BSXFlags" => Some((e.block, v)),
        _ => None,
    });
    match found {
        Some((_, v)) if v & 1 == 1 => Ok(()),
        Some((x, v)) => doc
            .patch(x, 4, &(v | 1).to_le_bytes())
            .map_err(|e| e.to_string()),
        None => {
            let nm = enc(doc.string(b"BSX"))?;
            let (_, body) = encode_extra(nm, &ExtraValue::Bsx(1), -1);
            let x = enc(doc.append(b"BSXFlags", body))?;
            set_extra_list(
                doc,
                root,
                net,
                &[net.extra.clone(), vec![x as i32]].concat(),
            )
        }
    }
}

fn win1252(s: &str) -> Result<Vec<u8>, String> {
    let (b, _, bad) = encoding_rs::WINDOWS_1252.encode(s);
    if bad {
        return Err(format!("{s:?} cannot be written in Windows-1252"));
    }
    Ok(b.into_owned())
}

/// Re-emits `owner`'s NiObjectNET prefix with `extra` as its extra-data list.
fn set_extra_list(
    doc: &mut NifDoc,
    owner: u32,
    net: &ObjectNet,
    extra: &[i32],
) -> Result<(), String> {
    let base = doc.header().offsets()[owner as usize];
    let body = doc.block(owner);
    let (s, e) = (net.start - base, net.end - base);
    let (head, tail) = body
        .get(..s)
        .zip(body.get(e..))
        .ok_or("NiObjectNET outside its block")?;
    let mut out = head.to_vec();
    out.extend(encode_net(net.name, extra, net.controller));
    out.extend(tail);
    doc.set_block(owner, out);
    Ok(())
}

/// AddExtra (anchored on a shape or the root), EditExtra and RemoveExtra (anchored on the extra block).
pub(crate) fn extra_op(
    doc: &mut NifDoc,
    m: &NifModel,
    at: &Anchor,
    block: u32,
    op: &Op,
) -> Result<Outcome, String> {
    match op {
        Op::AddExtra {
            kind, name, value, ..
        } => {
            let net = match at.role {
                Role::Root => m.root_net.as_ref().ok_or("the root is not a node")?,
                Role::Shape => &m.shape(block).ok_or("not a shape")?.net,
                _ => return Err("extra data is added to a shape or the root".into()),
            };
            if !EDITABLE.contains(&kind.as_str()) {
                return Err(format!("{kind} extra data is read-only"));
            }
            let nm = enc(doc.string(&win1252(name)?))?;
            let si = match value {
                ExtraValue::Str(s) => enc(doc.string(&win1252(s)?))?,
                _ => -1,
            };
            let (k, body) = encode_extra(nm, value, si);
            if k != kind {
                return Err(format!("a {k} value cannot make a {kind}"));
            }
            let new = enc(doc.append(k.as_bytes(), body))?;
            let mut extra = net.extra.clone();
            extra.push(new as i32);
            set_extra_list(doc, block, net, &extra)?;
        }
        Op::EditExtra { value, base, .. } => {
            let x = m.extras.get(&block).ok_or("not readable extra data")?;
            if x.value.as_ref() != Some(base) {
                return Ok(Outcome::Conflict(format!(
                    "changed in the source: was {base:?}, now {:?}",
                    x.value
                )));
            }
            let si = match value {
                ExtraValue::Str(s) => enc(doc.string(&win1252(s)?))?,
                _ => -1,
            };
            let (k, body) = encode_extra(-1, value, si);
            if k != x.kind {
                return Err(format!("a {k} value cannot replace a {}", x.kind));
            }
            doc.patch(block, 4, &body[4..]).map_err(|e| e.to_string())?;
        }
        Op::RemoveExtra { .. } => {
            let owned = |n: &ObjectNet| n.extra.contains(&(block as i32));
            let (owner, net) = match at.role {
                Role::Extra { on_root: true, .. } => m.root.zip(m.root_net.as_ref()),
                _ => m
                    .shapes
                    .iter()
                    .filter(|s| s.name.as_deref() == at.shape.as_deref())
                    .find(|s| owned(&s.net))
                    .map(|s| (s.block, &s.net)),
            }
            .filter(|(_, n)| owned(n))
            .ok_or("no owner lists that extra data")?;
            let mut extra = net.extra.clone();
            let i = extra.iter().position(|&e| e == block as i32).unwrap();
            extra.remove(i);
            set_extra_list(doc, owner, net, &extra)?;
        }
        _ => return Err("not an extra-data op".into()),
    }
    Ok(Outcome::Applied)
}

/// SetTexturePath on the shape `shape`: edits its set in place when it has one user, else on a fresh copy that
/// only this shape's BSLSP points at. A stale `base` is a Conflict.
pub(crate) fn texture_path_op(
    doc: &mut NifDoc,
    m: &NifModel,
    shape: &ShapeModel,
    op: &Op,
) -> Result<Outcome, String> {
    let Op::SetTexturePath {
        slot, path, base, ..
    } = op
    else {
        return Err("not a texture path op".into());
    };
    let lsp = shape
        .shader
        .and_then(|s| m.lighting.get(&s))
        .ok_or("the shape has no readable shader property")?;
    if lsp.prop.users.len() > 1 {
        return Err(format!(
            "This shader is shared by {} shapes: a texture path edit would change all of them.",
            lsp.prop.users.len()
        ));
    }
    let set = lsp
        .texture_set
        .and_then(|t| m.texsets.get(&t))
        .ok_or("the shader property has no readable texture set")?;
    let n = set.raw.slots.len();
    let old = set
        .raw
        .slots
        .get(*slot as usize)
        .ok_or(format!("slot {slot} is past the set's {n} slots"))?;
    // validate's decode: no BOM sniffing (Note GA3)
    let now = crate::text::lossy(old);
    if now != *base {
        return Ok(Outcome::Conflict(format!(
            "changed in the source: was {base:?}, now {now:?}"
        )));
    }
    let mut slots = set.raw.slots.clone();
    slots[*slot as usize] = win1252(path)?;
    let mut target = set.raw.block;
    if set.users.len() > 1 {
        target = copy(doc, target)?;
        // texture-set ref sits 24 bytes after the NiObjectNET (01-format §1.2)
        let at = lsp.prop.net.end - doc.header().offsets()[lsp.prop.block as usize] + 24;
        patch(doc, lsp.prop.block, at, target as i32)?;
    }
    doc.set_block(target, encode_texture_set(&slots));
    Ok(Outcome::Applied)
}
