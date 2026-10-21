//! Structural ops: shader type re-emit and alpha add/remove/split (spec §5, plan A9). Removal is unlink: the
//! block stays as an orphan. New blocks are appended; refs inside opaque blocks are never rewritten.
use super::validate::{ENV_SCALE_PBR, LSCC, LSCF, LSCF_VARS, TESTREF};
use super::{Anchor, Op, Outcome, Role};
use crate::ctrl::{BASE_LEN, CB_LEN};
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
            let new = doc.append(b"NiAlphaProperty", body);
            patch(doc, shape.block, ref_at, new as i32)
        }
        Op::RemoveAlpha { .. } => {
            if raw == -1 {
                return Err("the shape has no alpha property".into());
            }
            patch(doc, shape.block, ref_at, -1)
        }
        Op::SplitAlpha { .. } => {
            let new = split(doc, m, shape)?;
            match new {
                Some(new) => patch(doc, shape.block, ref_at, new as i32),
                None => Ok(()),
            }
        }
        _ => Err("not an alpha op".into()),
    }
}

fn copy(doc: &mut NifDoc, block: u32) -> u32 {
    let (kind, body) = (doc.kind(block).to_vec(), doc.block(block).to_vec());
    doc.append(&kind, body)
}

/// Appends a private copy of the shape's alpha with deep copies of its free controller chain; returns the new
/// alpha block, or None when the alpha has a single user.
fn split(doc: &mut NifDoc, m: &NifModel, shape: &ShapeModel) -> Result<Option<u32>, String> {
    let a = shape
        .alpha
        .and_then(|a| m.alphas.get(&a))
        .ok_or("the shape has no readable alpha property")?;
    if a.prop.users.len() <= 1 {
        return Ok(None);
    }
    if a.prop.ctrls.iter().any(|&c| m.managed(c)) {
        return Err("Shared alpha with a clip controller: splitting needs Phase C.".into());
    }
    let source_blocks = doc.len(); // refs are bounded by the source, not the growing doc
    let new = copy(doc, a.prop.block);
    // (controller copy, Some((interpolator copy, data copy, pose floats))) in chain order
    let mut copies = vec![];
    for c in &a.prop.ctrls {
        let nc = copy(doc, *c);
        let mut ni = None;
        if let Some(i) = m.ctrls[c].interp.and_then(|i| m.interps.get(&(i as u32))) {
            let n = copy(doc, i.block);
            let data = u32::try_from(i.data)
                .ok()
                .filter(|&d| i.known && d < source_blocks);
            ni = Some((n, data.map(|d| copy(doc, d)), i.value.len()));
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
    Ok(Some(new))
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
            "This clip has several interpolators for this controller: editing them needs Phase C."
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
    let idx = doc.string(new.to_string().as_bytes());
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
        keys, base, clip, ..
    } = op
    else {
        return Err("not a key op".into());
    };
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
        let ni = copy(doc, i);
        for &(blk, at) in &t.refs {
            patch(doc, blk, at, ni as i32)?;
        }
        let nd = doc.append(kind, encode_data(keys));
        patch(doc, ni, 4 * floats, nd as i32)?;
    } else if let Some(d) = data.filter(|_| data_users <= 1) {
        doc.set_block(d, encode_data(keys));
    } else {
        let nd = doc.append(kind, encode_data(keys));
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
            let (data_kind, interp_kind, first, n): (&[u8], &[u8], Vec<u8>, usize) = match keys {
                KeyData::Float(g) if float => (
                    b"NiFloatData",
                    b"NiFloatInterpolator",
                    g.keys
                        .first()
                        .map_or(vec![], |k| k.value.to_le_bytes().to_vec()),
                    g.keys.len(),
                ),
                KeyData::Pos(g) if !float => (
                    b"NiPosData",
                    b"NiPoint3Interpolator",
                    g.keys.first().map_or(vec![], |k| {
                        k.value.iter().flat_map(|f| f.to_le_bytes()).collect()
                    }),
                    g.keys.len(),
                ),
                _ => return Err("The keys do not match the controller's type.".into()),
            };
            if n == 0 {
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
            if m.root_net.is_none() {
                return Err("The root is not a node: the BSX Animated bit cannot be set.".into());
            }
            let d = doc.append(data_kind, encode_data(keys));
            let mut ib = first;
            ib.extend((d as i32).to_le_bytes());
            let i = doc.append(interp_kind, ib);
            let mut cb = (-1i32).to_le_bytes().to_vec();
            cb.extend(flags.to_le_bytes());
            for f in [*freq, 0.0, *start, *stop] {
                cb.extend(f.to_le_bytes());
            }
            cb.extend((block as i32).to_le_bytes());
            cb.extend((i as i32).to_le_bytes());
            cb.extend(var.map(u32::to_le_bytes).into_iter().flatten());
            let c = doc.append(class.as_bytes(), cb);
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
            let owners: Vec<&PropModel> = m
                .lighting
                .values()
                .map(|l| &l.prop)
                .chain(m.alphas.values().map(|a| &a.prop))
                .filter(|p| p.ctrls.contains(&block))
                .collect();
            let prop =
                match owners.as_slice() {
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
            let nm = doc.string(b"BSX");
            let (_, body) = encode_extra(nm, &ExtraValue::Bsx(1), -1);
            let x = doc.append(b"BSXFlags", body);
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
            let nm = doc.string(&win1252(name)?);
            let si = match value {
                ExtraValue::Str(s) => doc.string(&win1252(s)?),
                _ => -1,
            };
            let (k, body) = encode_extra(nm, value, si);
            if k != kind {
                return Err(format!("a {k} value cannot make a {kind}"));
            }
            let new = doc.append(k.as_bytes(), body);
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
                ExtraValue::Str(s) => doc.string(&win1252(s)?),
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
                    .filter(|s| s.name == at.shape)
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
    let now = encoding_rs::WINDOWS_1252.decode(old).0;
    if now != *base {
        return Ok(Outcome::Conflict(format!(
            "changed in the source: was {base:?}, now {now:?}"
        )));
    }
    let mut slots = set.raw.slots.clone();
    slots[*slot as usize] = win1252(path)?;
    let mut target = set.raw.block;
    if set.users.len() > 1 {
        target = copy(doc, target);
        // texture-set ref sits 24 bytes after the NiObjectNET (01-format §1.2)
        let at = lsp.prop.net.end - doc.header().offsets()[lsp.prop.block as usize] + 24;
        patch(doc, lsp.prop.block, at, target as i32)?;
    }
    doc.set_block(target, encode_texture_set(&slots));
    Ok(Outcome::Applied)
}
