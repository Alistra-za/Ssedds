//! Structural ops: shader type re-emit and alpha add/remove/split (spec §5, plan A9). Removal is unlink: the
//! block stays as an orphan. New blocks are appended; refs inside opaque blocks are never rewritten.
use super::validate::{ENV_SCALE_PBR, LSCC, LSCF, LSCF_VARS, TESTREF};
use super::{Anchor, Op, Outcome, Role};
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
    let d = doc.append(data_kind, encode_data(keys));
    let mut ib = first;
    ib.extend((d as i32).to_le_bytes());
    Ok(doc.append(interp_kind, ib))
}

fn enc<T>(r: Result<T, crate::NifError>) -> Result<T, String> {
    r.map_err(|e| e.to_string())
}

/// #INV_FLT#: the Value every vanilla blend interpolator holds (C1 census, 6722 of 6722).
const INV_FLT: u32 = 0xFF7F_FFFF;

/// The first NiControllerManager on the root's controller chain.
fn root_manager(m: &NifModel) -> Option<u32> {
    let mut c = m.root_net.as_ref()?.controller;
    // bounded: a looping chain is cut by `ctrl::chain`, but stay safe on any `next`
    for _ in 0..=m.ctrls.len() {
        let t = m.ctrls.get(&u32::try_from(c).ok()?)?;
        if t.class == "NiControllerManager" {
            return Some(t.block);
        }
        c = t.next;
    }
    None
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
    let Some(mb) = root_manager(m) else {
        return Ok(None);
    };
    let man = enc(read_manager(h, cur, mb as usize))?;
    // the Windows-1252 name the editor shows, or the model's UTF-8 view of it
    let named = |r: &[u8]| crate::text::lossy(r) == name || String::from_utf8_lossy(r) == name;
    let names = clip_names(h, cur, &man.sequences)?;
    let k = (0..names.len())
        .filter(|&k| names[k].is_some_and(named))
        .nth(nth as usize);
    Ok(k.map(|k| (mb, man, k)))
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
    let raw = h.string(s.net.name).ok_or_else(unnamed)?;
    if m.shapes
        .iter()
        .filter(|o| h.string(o.net.name) == Some(raw))
        .count()
        > 1
    {
        return Err(two());
    }
    Ok(s.block)
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
            let s = doc.string(&b);
            patch(doc, man.sequences[k] as u32, 0, s)?;
        }
        _ => return Err("not a clip op".into()),
    }
    Ok(Outcome::Applied)
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
        for (&c, ctrl) in m.ctrls.iter().filter(|(_, c)| c.known) {
            let Some(&k) = crate::anim::bound_cbs(m, h, rs, c).first() else {
                continue;
            };
            let others = m
                .sequences
                .iter()
                .filter(|s| listed.contains(&(s.block as i32)));
            if others
                .into_iter()
                .any(|s| !crate::anim::bound_cbs(m, h, s, c).is_empty())
            {
                continue;
            }
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
    let found = root_manager(m);
    let old = match found {
        Some(mb) => Some(enc(read_manager(doc.header(), cur, mb as usize))?),
        None => None,
    };
    let name_b = clip_name_bytes(doc.header(), cur, old.as_ref(), name, None)?;
    if ![freq, start, stop].iter().all(|v| v.is_finite()) {
        return Err("Clip times must be finite numbers.".into());
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
    let (scene, mut objs) = match &old {
        Some(man) if man.palette >= 0 => {
            enc(read_palette(doc.header(), cur, man.palette as usize))?
        }
        _ => (root as i32, vec![]),
    };

    // C-P26/C-P29: the MTTC lists the nodes the clips bind; existing targets stay, missing ones are appended
    let with_shapes = |mut targets: Vec<i32>| {
        for (s, _, _) in &shapes {
            if !targets.contains(&(*s as i32)) {
                targets.push(*s as i32);
            }
        }
        targets
    };
    let (mgr, mut man) = match (found, old) {
        (Some(mb), Some(man)) => {
            let mttc = u32::try_from(man.ctrl.next)
                .ok()
                .filter(|&n| doc.kind(n) == b"NiMultiTargetTransformController");
            if let Some(n) = mttc {
                let (base, old_t) = enc(read_mttc(doc.header(), cur, n as usize))?;
                let targets = with_shapes(old_t.clone());
                if targets != old_t {
                    doc.set_block(n, enc(encode_mttc(&base, &targets))?);
                }
            }
            (mb, man)
        }
        _ => {
            // F21: a placeholder manager, filled by set_block at the end
            let mgr = doc.append(b"NiControllerManager", vec![]);
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
            let targets = with_shapes(vec![]);
            let mttc_body = enc(encode_mttc(&base(net.controller, MTTC_FLAGS), &targets))?;
            let mttc = doc.append(b"NiMultiTargetTransformController", mttc_body);
            let man = Manager {
                ctrl: base(mttc as i32, MANAGER_FLAGS),
                cumulative: 0,
                sequences: vec![],
                palette: -1,
            };
            (mgr, man)
        }
    };

    let before = objs.len();
    for (s, raw, _) in &shapes {
        if !objs.iter().any(|(_, p)| *p == *s as i32) {
            objs.push((raw.clone(), *s as i32));
        }
    }
    let pal = enc(encode_palette(scene, &objs))?;
    if man.palette < 0 {
        man.palette = doc.append(b"NiDefaultAVObjectPalette", pal) as i32;
    } else if objs.len() > before {
        doc.set_block(man.palette as u32, pal);
    }

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
            let bl = doc.append(blend.as_bytes(), enc(encode_blend_interp(blend, &v))?);
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
        let property = doc.string(ptype);
        let ctrl_type = doc.string(ctrl.class.as_bytes());
        // F0: TestRef CBs carry a null Controller ID, as vanilla does
        let ctrl_id = match ctrl.var {
            Some(v) if !testref => doc.string(v.to_string().as_bytes()),
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
    let s_name = doc.string(&name_b);
    let (s_start, s_end) = (doc.string(b"start"), doc.string(b"end"));
    let tk_body = enc(encode_text_keys(-1, &[(start, s_start), (stop, s_end)]))?;
    let tk = doc.append(b"NiTextKeyExtraData", tk_body);
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
    let seq = doc.append(b"NiControllerSequence", enc(encode_sequence(&seq))?);
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
