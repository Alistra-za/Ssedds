//! Structural ops: shader type re-emit and alpha add/remove/split (spec §5, plan A9). Removal is unlink: the
//! block stays as an orphan. New blocks are appended; refs inside opaque blocks are never rewritten.
use super::{Anchor, Op, Outcome, Role};
use crate::ctrl::BASE_LEN;
use crate::extra::{encode_extra, ExtraValue, EDITABLE};
use crate::header::Header;
use crate::keys::{encode_data, read_data, KeyData, KeyGroup};
use crate::model::{NifModel, ShapeModel};
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
        return Ok((i as u32, floats, None, empty));
    }
    if ip.data < -1 || ip.data as usize >= h.sizes.len() {
        return Err("The interpolator's data ref is outside the file.".into());
    }
    let d = ip.data as usize;
    let keys = read_data(h, b, d).map_err(|e| e.to_string())?;
    match (&keys, floats) {
        (KeyData::Float(_), 1) | (KeyData::Pos(_), 3) => {
            Ok((i as u32, floats, Some(d as u32), keys))
        }
        _ => Err(mismatch()),
    }
}

/// SetKeys with clip None on the controller `ctrl`. Ok(Conflict) for a stale base.
pub(crate) fn set_keys(
    doc: &mut NifDoc,
    cur: &[u8],
    m: &NifModel,
    ctrl: u32,
    op: &Op,
) -> Result<Outcome, String> {
    let Op::SetKeys { keys, base, .. } = op else {
        return Err("not a key op".into());
    };
    let (i, floats, data, now) = ctrl_keys(doc.header(), cur, m, ctrl)?;
    let kind: &[u8] = match (keys, &now) {
        (KeyData::Float(_), KeyData::Float(_)) => b"NiFloatData",
        (KeyData::Pos(_), KeyData::Pos(_)) => b"NiPosData",
        _ => return Err("The keys do not match the interpolator's type.".into()),
    };
    // bytes, not KeyData ==: NaN != NaN (F19)
    if encode_data(base) != encode_data(&now) {
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
    if interp_users > 1 {
        let ni = copy(doc, i);
        patch(doc, ctrl, BASE_LEN, ni as i32)?;
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
