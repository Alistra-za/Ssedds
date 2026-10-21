//! Export-time compaction (Note CQ): drops the blocks our edits appended that nothing reaches any more (a removed
//! controller, clip or copy). Never used by `apply` or the preview, so op block numbers stay stable.
//!
//! Every source block stays at its index. An appended block stays when a Ref or Ptr of a kept block names it. Source
//! blocks are scanned only in the kinds an op can patch with a ref (shapes, nodes, properties, controllers,
//! interpolators, managers, sequences, palettes); their other fields only name source blocks. An appended block of
//! a kind listed here is read whole; a reachable one of any other kind, a block that does not read, or a ref past
//! the file stops the compaction (Err), and the caller exports the bytes as they are.
use crate::clips::read_text_keys;
use crate::clips::{read_mttc, read_palette, read_sequence_out};
use crate::ctrl::{
    body, interp_floats, is_ctrl, known_len, read_blend_interp, read_manager, BASE_LEN, CB_LEN,
};
use crate::extra::read_extra;
use crate::graph::NODE_TYPES;
use crate::header::Header;
use crate::keys::read_data;
use crate::net::read_net;
use crate::props::decode_texture_set_raw;
use crate::shape_head::{decode_head_at, ShapeKind};

/// Where a block's refs are.
enum Refs {
    /// Every Ref and Ptr of the block (block-relative offsets).
    All(Vec<usize>),
    /// Source blocks only: the ref fields an op can patch.
    Patchable(Vec<usize>),
    /// No ref enumerator for this kind.
    Unknown,
}

/// Kinds an op appends that hold no refs.
const NO_REFS: [&[u8]; 10] = [
    b"NiFloatData",
    b"NiPosData",
    b"BSShaderTextureSet",
    b"NiTextKeyExtraData",
    b"BSXFlags",
    b"NiStringExtraData",
    b"NiIntegerExtraData",
    b"NiBooleanExtraData",
    b"NiFloatExtraData",
    b"BSInvMarker",
];

fn refs_of(h: &Header, b: &[u8], i: usize, root: Option<usize>) -> Result<Refs, String> {
    let e = |x: crate::NifError| format!("block {i}: {x}");
    let kind = h.kind(i);
    let d = body(h, b, i).map_err(e)?;
    let start = h.offsets[i];
    let need = |at: &[usize]| match at.iter().all(|&a| a + 4 <= d.len()) {
        true => Ok(()),
        false => Err(format!("block {i}: a ref runs past the block")),
    };
    // NiObjectNET extra-data list and controller
    let net = || -> Result<(Vec<usize>, usize), String> {
        let n = read_net(h, b, i).map_err(e)?;
        let mut v: Vec<usize> = (0..n.extra.len()).map(|k| n.extra_at(k) - start).collect();
        v.push(n.controller_at() - start);
        Ok((v, n.end - start))
    };
    let r = if NO_REFS.contains(&kind) {
        // ref-free only when the block reads at its exact size
        match kind {
            b"NiFloatData" | b"NiPosData" => read_data(h, b, i).map(drop),
            b"BSShaderTextureSet" => decode_texture_set_raw(h, b, i).map(drop),
            b"NiTextKeyExtraData" => read_text_keys(h, b, i).map(drop),
            _ => read_extra(h, b, i).map(drop),
        }
        .map_err(e)?;
        Refs::All(vec![])
    } else if let Some(k) = ShapeKind::from_name(kind) {
        let (_, at) = decode_head_at(h, b, i, k).map_err(e)?;
        let mut v = net()?.0;
        v.extend([at.shader - start, at.alpha - start]);
        Refs::Patchable(v)
    } else if NODE_TYPES.iter().any(|t| t.as_bytes() == kind)
        || (Some(i) == root && kind.ends_with(b"Node"))
    {
        // children and effects name source blocks only: no op adds one
        Refs::Patchable(net()?.0)
    } else if kind == b"BSLightingShaderProperty" {
        let (mut v, end) = net()?;
        v.push(end + 24); // texture set (01-format §1.2)
        Refs::Patchable(v)
    } else if kind == b"BSEffectShaderProperty" {
        Refs::Patchable(net()?.0)
    } else if kind == b"NiAlphaProperty" {
        let (v, end) = net()?;
        if end + 3 != d.len() {
            return Err(format!(
                "block {i}: NiAlphaProperty is not 3 bytes past its NiObjectNET"
            ));
        }
        Refs::All(v)
    } else if interp_floats(kind) > 0 {
        let at = 4 * interp_floats(kind);
        if at + 4 != d.len() {
            return Err(format!("block {i}: interpolator size"));
        }
        Refs::All(vec![at])
    } else if kind == b"NiBlendFloatInterpolator" || kind == b"NiBlendPoint3Interpolator" {
        let x = read_blend_interp(h, b, i).map_err(e)?;
        let n = if x.items.is_some() { x.array_size } else { 0 };
        Refs::All((0..n as usize).map(|k| 26 + 17 * k).collect())
    } else if kind == b"NiControllerManager" {
        let m = read_manager(h, b, i).map_err(e)?;
        let at = BASE_LEN + 5;
        let mut v = vec![0, 22];
        v.extend((0..m.sequences.len()).map(|k| at + 4 * k));
        v.push(d.len() - 4);
        Refs::All(v)
    } else if kind == b"NiMultiTargetTransformController" {
        let (_, t) = read_mttc(h, b, i).map_err(e)?;
        let mut v = vec![0, 22];
        v.extend((0..t.len()).map(|k| BASE_LEN + 2 + 4 * k));
        Refs::All(v)
    } else if kind == b"NiDefaultAVObjectPalette" {
        let (_, objs) = read_palette(h, b, i).map_err(e)?;
        let mut v = vec![0];
        let mut at = 8;
        for (name, _) in &objs {
            v.push(at + 4 + name.len());
            at += 8 + name.len();
        }
        Refs::All(v)
    } else if kind == b"NiControllerSequence" {
        let s = read_sequence_out(h, b, i).map_err(e)?;
        let mut v = vec![];
        for k in 0..s.blocks.len() {
            v.extend([12 + k * CB_LEN, 16 + k * CB_LEN]);
        }
        let t = 12 + s.blocks.len() * CB_LEN;
        v.extend([t + 4, t + 24]); // text keys, manager
        v.extend((0..s.notes.len()).map(|k| t + 34 + 4 * k));
        Refs::All(v)
    } else if let Some(n) = known_len(kind) {
        if n != d.len() {
            return Err(format!("block {i}: controller size"));
        }
        Refs::All(vec![0, 22, BASE_LEN]) // next, target, interpolator
    } else if is_ctrl(kind) {
        // ops patch only `next` on a controller of another class
        Refs::Patchable(vec![0, 22])
    } else {
        Refs::Unknown
    };
    if let Refs::All(v) | Refs::Patchable(v) = &r {
        need(v)?;
    }
    Ok(r)
}

fn i32_at(d: &[u8], at: usize) -> i32 {
    i32::from_le_bytes(d[at..at + 4].try_into().unwrap())
}

/// `b` without the appended blocks (index >= `source_blocks`) that no kept block reaches, refs renumbered, unused
/// block types dropped (leftover strings stay). Returns `b` unchanged when nothing is dropped. Err (the caller
/// exports `b` as it is) when a block that must be scanned does not read or has a kind without a ref enumerator.
pub fn compact(b: &[u8], source_blocks: u32) -> Result<Vec<u8>, String> {
    let h = Header::parse(b).map_err(|e| e.to_string())?;
    h.check_file_length(b).map_err(|e| e.to_string())?;
    let (n, src) = (h.sizes.len(), source_blocks as usize);
    if src > n {
        return Err(format!("{src} source blocks but the file has {n}"));
    }
    if src == n {
        return Ok(b.to_vec());
    }
    let roots = h.roots(b).map_err(|e| e.to_string())?;
    let root = roots
        .first()
        .and_then(|&r| usize::try_from(r).ok())
        .filter(|&r| r < n);
    let mut keep = vec![false; n];
    let mut slots: Vec<Vec<usize>> = vec![vec![]; n];
    let mut scanned = vec![false; n];
    let mut todo: Vec<usize> = (0..src).collect();
    // footer roots are refs too
    for &r in &roots {
        if let Ok(r) = usize::try_from(r) {
            if r >= n {
                return Err(format!("root {r} is past the file"));
            }
            todo.push(r);
        }
    }
    for &k in &todo {
        keep[k] = true;
    }
    while let Some(i) = todo.pop() {
        if std::mem::replace(&mut scanned[i], true) {
            continue; // a root listed twice, or a source root
        }
        let v = match refs_of(&h, b, i, root)? {
            Refs::All(v) => v,
            Refs::Patchable(v) if i < src => v,
            Refs::Unknown if i < src => continue,
            _ => {
                let kind = String::from_utf8_lossy(h.kind(i));
                return Err(format!("block {i}: no ref layout for an appended {kind}"));
            }
        };
        let d = &b[h.offsets[i]..];
        for &at in &v {
            let r = i32_at(d, at);
            let Ok(r) = usize::try_from(r) else { continue };
            if r >= n {
                return Err(format!("block {i}: ref {r} is past the file"));
            }
            if !keep[r] {
                keep[r] = true;
                todo.push(r);
            }
        }
        slots[i] = v;
    }
    if keep.iter().all(|&k| k) {
        return Ok(b.to_vec());
    }
    // old index -> new index, kept blocks only
    let mut new = vec![u32::MAX; n];
    let mut next = 0u32;
    for k in (0..n).filter(|&k| keep[k]) {
        new[k] = next;
        next += 1;
    }
    let renum = |r: i32| match usize::try_from(r) {
        Ok(r) if r >= src => new[r] as i32,
        _ => r,
    };
    // one pass over the kept blocks, one over the types (a hostile header lists up to 65535 types)
    let mut used = vec![false; h.block_types.len()];
    for k in (0..n).filter(|&k| keep[k]) {
        used[h.type_index[k] as usize] = true;
    }
    let mut type_new = vec![0u16; used.len()];
    let mut nt = 0u16;
    for t in (0..used.len()).filter(|&t| used[t]) {
        type_new[t] = nt;
        nt += 1;
    }
    let strings_at =
        h.prefix_end + 2 + h.block_types.iter().map(|t| 4 + t.len()).sum::<usize>() + 6 * n;
    let mut out = b[..h.prefix_end].to_vec();
    out[h.nblocks_at..h.nblocks_at + 4].copy_from_slice(&next.to_le_bytes());
    out.extend(nt.to_le_bytes());
    for t in (0..used.len()).filter(|&t| used[t]) {
        out.extend((h.block_types[t].len() as u32).to_le_bytes());
        out.extend(&h.block_types[t]);
    }
    for k in (0..n).filter(|&k| keep[k]) {
        let raw = h.type_index_raw[k];
        out.extend((type_new[(raw & 0x7FFF) as usize] | (raw & 0x8000)).to_le_bytes());
    }
    for k in (0..n).filter(|&k| keep[k]) {
        out.extend(h.sizes[k].to_le_bytes());
    }
    out.extend_from_slice(&b[strings_at..h.data_start]);
    for k in (0..n).filter(|&k| keep[k]) {
        let mut d = b[h.offsets[k]..h.offsets[k] + h.sizes[k] as usize].to_vec();
        for &at in &slots[k] {
            let r = renum(i32_at(&d, at));
            d[at..at + 4].copy_from_slice(&r.to_le_bytes());
        }
        out.extend(d);
    }
    out.extend((roots.len() as u32).to_le_bytes());
    for r in &roots {
        out.extend(renum(*r).to_le_bytes());
    }
    Ok(out)
}
