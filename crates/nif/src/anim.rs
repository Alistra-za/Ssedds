//! Controller evaluation for the preview (spec §6.1-§6.2): NifSkope's ctrlTime, key sampling and the
//! controller-target → MaterialState mapping. Pure.
use crate::ctrl::{CtrlBlock, Sequence, TimeCtrl};
use crate::header::Header;
use crate::keys::{read_data, Key, KeyData, KeyGroup, KeyType};
use crate::material::MaterialState;
use crate::model::NifModel;
use crate::props::LightingTrailer;

/// NifSkope `Controller::ctrlTime` (glcontroller.cpp:246-290), then Play Backwards (bit 4) as
/// `start + stop - ct`. shortcut: bit 4 is this project's reading of G8 (direction checked, phase not); upgrade
/// if an engine capture shows a different phase.
pub fn ctrl_time(t: f32, freq: f32, phase: f32, start: f32, stop: f32, flags: u16) -> f32 {
    let x0 = freq * t + phase;
    let cycle = (flags >> 1) & 3;
    let d = stop - start;
    let mut ct = if start <= x0 && x0 <= stop {
        x0
    } else if cycle <= 1 {
        if d <= 0.0 {
            start
        } else {
            let x = (x0 - start) / d;
            let y = (x - x.floor()) * d;
            if cycle == 0 || (x.floor().abs() as i32) & 1 == 0 {
                start + y
            } else {
                stop - y
            }
        }
    } else if x0 < start {
        // CLAMP and value 3; NifSkope has no d <= 0 guard here (F14)
        start
    } else if x0 > stop {
        stop
    } else {
        x0
    };
    if flags & 0x10 != 0 {
        ct = start + stop - ct;
    }
    ct
}

fn hermite<const N: usize>(
    v1: [f32; N],
    v2: [f32; N],
    t1: [f32; N],
    t2: [f32; N],
    x: f32,
) -> [f32; N] {
    let (x2, x3) = (x * x, x * x * x);
    std::array::from_fn(|c| {
        v1[c] * (2.0 * x3 - 3.0 * x2 + 1.0)
            + v2[c] * (-2.0 * x3 + 3.0 * x2)
            + t1[c] * (x3 - 2.0 * x2 + x)
            + t2[c] * (x3 - x2)
    })
}

/// Kochanek-Bartels tangent at key `i`, unscaled like QUADRATIC (F17, unverified against the engine).
fn kb<T, const N: usize>(
    k: &[Key<T>],
    arr: &impl Fn(&T) -> [f32; N],
    i: usize,
    out: bool,
) -> [f32; N] {
    let d = |a: usize, b: usize| -> [f32; N] {
        let (pa, pb) = (arr(&k[a].value), arr(&k[b].value));
        std::array::from_fn(|c| pa[c] - pb[c])
    };
    let prev = (i > 0).then(|| d(i, i - 1));
    let next = (i + 1 < k.len()).then(|| d(i + 1, i));
    let (prev, next) = match (prev, next) {
        (Some(p), Some(n)) => (p, n),
        (None, Some(n)) => (n, n),
        (Some(p), None) => (p, p),
        (None, None) => return [0.0; N],
    };
    let [t, b, c] = k[i].tbc.unwrap_or_default();
    let (cp, cn) = if out {
        (1.0 + c, 1.0 - c)
    } else {
        (1.0 - c, 1.0 + c)
    };
    let a = (1.0 - t) * (1.0 + b) * cp / 2.0;
    let e = (1.0 - t) * (1.0 - b) * cn / 2.0;
    std::array::from_fn(|j| a * prev[j] + e * next[j])
}

fn sample<T, const N: usize>(
    g: &KeyGroup<T>,
    t: f32,
    arr: impl Fn(&T) -> [f32; N],
) -> Option<[f32; N]> {
    let k = &g.keys;
    let (first, last) = (k.first()?, k.last()?);
    if t <= first.time {
        return Some(arr(&first.value));
    }
    if t >= last.time {
        return Some(arr(&last.value));
    }
    // the largest i with time <= t (the later key on duplicate times); last.time <= t is false here, so i + 1 exists
    let i = k.iter().rposition(|x| x.time <= t)?;
    let j = i + 1;
    let (vi, vj) = (arr(&k[i].value), arr(&k[j].value));
    let d = k[j].time - k[i].time;
    // exactly on a key, or decreasing times (invalid file): the key itself, never NaN
    if k[i].time == t || d.is_nan() || d <= 0.0 {
        return Some(vi);
    }
    let x = (t - k[i].time) / d;
    let zero = [0.0; N];
    Some(match g.key_type {
        Some(q) if q == KeyType::Quadratic as u32 => hermite(
            vi,
            vj,
            k[i].backward.as_ref().map_or(zero, &arr),
            k[j].forward.as_ref().map_or(zero, &arr),
            x,
        ),
        Some(q) if q == KeyType::Tbc as u32 => {
            hermite(vi, vj, kb(k, &arr, i, true), kb(k, &arr, j, false), x)
        }
        // a true step that holds until the next key (G5; NifSkope switches at x = 0.5, F16)
        Some(q) if q == KeyType::Const as u32 => vi,
        _ => std::array::from_fn(|c| vi[c] + (vj[c] - vi[c]) * x),
    })
}

/// None for 0 keys.
pub fn sample_f32(g: &KeyGroup<f32>, t: f32) -> Option<f32> {
    sample(g, t, |v| [*v]).map(|[v]| v)
}

/// None for 0 keys.
pub fn sample_vec3(g: &KeyGroup<[f32; 3]>, t: f32) -> Option<[f32; 3]> {
    sample(g, t, |v| *v)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Lscf(u32),
    Lscc(u32),
    AlphaRef,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnimValue {
    F32(f32),
    Vec3([f32; 3]),
    None,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub ctrl: u32,
    pub target: Target,
    pub flags: u16,
    pub freq: f32,
    pub phase: f32,
    pub start: f32,
    pub stop: f32,
    /// The interpolator's Value; None for #INV_FLT# / #INV_VEC3#, a null or unreadable interpolator.
    pub value: AnimValue,
    /// The interpolator's key data, when it has a readable data block.
    pub keys: Option<KeyData>,
}

/// #INV_FLT# / first component of #INV_VEC3# (−3.402823466e+38 = f32::MIN, nif.xml:84-85; F24).
const INV: u32 = 0xFF7F_FFFF;

fn target_of(c: &TimeCtrl) -> Option<Target> {
    match (c.class.as_str(), c.var) {
        ("BSLightingShaderPropertyFloatController", Some(v)) => Some(Target::Lscf(v)),
        ("BSLightingShaderPropertyColorController", Some(v)) => Some(Target::Lscc(v)),
        ("BSNiAlphaPropertyTestRefController", _) => Some(Target::AlphaRef),
        _ => None,
    }
}

fn interp_track(m: &NifModel, h: &Header, b: &[u8], r: i32) -> (AnimValue, Option<KeyData>) {
    let Some(i) = u32::try_from(r).ok().and_then(|r| m.interps.get(&r)) else {
        return (AnimValue::None, None);
    };
    let value = match (i.class.as_str(), i.value.as_slice()) {
        ("NiFloatInterpolator", &[v]) if v.to_bits() != INV => AnimValue::F32(v),
        ("NiPoint3Interpolator", &[x, y, z]) if x.to_bits() != INV => AnimValue::Vec3([x, y, z]),
        _ => AnimValue::None,
    };
    let keys = (i.data >= 0)
        .then(|| read_data(h, b, i.data as usize).ok())
        .flatten();
    (value, keys)
}

/// The property's controller chain and its type name as a sequence names it; None for any other block.
fn chain_of(m: &NifModel, prop: u32) -> Option<(&[u32], &[u32], &'static [u8])> {
    if let Some(l) = m.lighting.get(&prop) {
        return Some((&l.prop.ctrls, &l.prop.users, b"BSLightingShaderProperty"));
    }
    let a = m.alphas.get(&prop)?;
    Some((&a.prop.ctrls, &a.prop.users, b"NiAlphaProperty"))
}

/// Tracks of the controllers that run in the free preview: known, Active, not managed (G2, F13). Parses the
/// header and reads key data on each call; call once per edit, never per frame (F23).
pub fn free_tracks(m: &NifModel, b: &[u8], prop: u32) -> Vec<Track> {
    let (Ok(h), Some((chain, _, _))) = (Header::parse(b), chain_of(m, prop)) else {
        return vec![];
    };
    chain
        .iter()
        .filter_map(|&id| {
            let c = m.ctrls.get(&id)?;
            if !c.known || c.flags & 0x08 == 0 || m.managed(id) {
                return None;
            }
            let target = target_of(c)?;
            let (value, keys) = interp_track(m, &h, b, c.interp.unwrap_or(-1));
            Some(Track {
                ctrl: id,
                target,
                flags: c.flags,
                freq: c.frequency,
                phase: c.phase,
                start: c.start,
                stop: c.stop,
                value,
                keys,
            })
        })
        .collect()
}

/// B2's binding (spec §5.5): the controller of `ctrls` (one property's chain) that CB `cb` drives, by its controller
/// ref, else (ref -1 only) by raw node, property type, controller type and Controller ID bytes, first match.
fn bound<'a>(
    cb: &CtrlBlock,
    ctrls: &[&'a TimeCtrl],
    names: &[&[u8]],
    ptype: &[u8],
) -> Option<&'a TimeCtrl> {
    if cb.controller >= 0 {
        ctrls
            .iter()
            .find(|c| c.block as i32 == cb.controller)
            .copied()
    } else if cb.controller == -1 {
        ctrls
            .iter()
            .find(|c| {
                let id = match target_of(c) {
                    Some(Target::Lscf(v) | Target::Lscc(v)) => Some(v.to_string().into_bytes()),
                    _ => None,
                };
                cb.ctrl_type.as_deref() == Some(c.class.as_bytes())
                    && cb.property.as_deref() == Some(ptype)
                    && cb.node.as_deref().is_some_and(|n| names.contains(&n))
                    && cb.ctrl_id == id
            })
            .copied()
    } else {
        None
    }
}

/// The CB indices of `s` that drive controller `ctrl`, over every property whose chain lists it (B7). A by-ref CB
/// naming `ctrl` counts even when no property lists it, so a hostile file cannot hide it from the ID rewrite.
pub(crate) fn bound_cbs(m: &NifModel, h: &Header, s: &Sequence, ctrl: u32) -> Vec<usize> {
    let props = m
        .lighting
        .keys()
        .chain(m.alphas.keys())
        .filter_map(|&p| chain_of(m, p))
        .filter(|(chain, _, _)| chain.contains(&ctrl));
    let mut out: Vec<usize> = (0..s.blocks.len())
        .filter(|&k| s.blocks[k].controller == ctrl as i32)
        .collect();
    for (chain, users, ptype) in props {
        let ctrls: Vec<&TimeCtrl> = chain.iter().filter_map(|id| m.ctrls.get(id)).collect();
        let names: Vec<&[u8]> = users
            .iter()
            .filter_map(|&u| h.string(m.shape(u)?.net.name))
            .collect();
        out.extend((0..s.blocks.len()).filter(|&k| {
            bound(&s.blocks[k], &ctrls, &names, ptype).is_some_and(|c| c.block == ctrl)
        }));
    }
    // a controller on two properties' chains finds a CB twice
    out.sort_unstable();
    out.dedup();
    out
}

/// The sequence blocks with at least one CB driving `ctrl` (the Controllers tab's clip list). Parses the header:
/// call once per change, never per frame.
pub fn clips_of(m: &NifModel, b: &[u8], ctrl: u32) -> Vec<u32> {
    let Ok(h) = Header::parse(b) else {
        return vec![];
    };
    m.sequences
        .iter()
        .filter(|s| !bound_cbs(m, &h, s, ctrl).is_empty())
        .map(|s| s.block)
        .collect()
}

/// Tracks for sequence block `seq` on `prop`: each controlled block binds by controller ref, else (ref -1 only)
/// by node, property type, controller type and controller ID strings (spec §5.5). Timing comes from the sequence;
/// the value and keys from the CB's own interpolator.
pub fn clip_tracks(m: &NifModel, b: &[u8], seq: u32, prop: u32) -> Vec<Track> {
    let (Ok(h), Some(s), Some((chain, users, ptype))) = (
        Header::parse(b),
        m.sequences.iter().find(|s| s.block == seq),
        chain_of(m, prop),
    ) else {
        return vec![];
    };
    let ctrls: Vec<&TimeCtrl> = chain.iter().filter_map(|id| m.ctrls.get(id)).collect();
    // raw string-table bytes: a lossy-decoded name would never match a non-UTF-8 node name
    let names: Vec<&[u8]> = users
        .iter()
        .filter_map(|&u| h.string(m.shape(u)?.net.name))
        .collect();
    s.blocks
        .iter()
        .filter_map(|cb| {
            let c = bound(cb, &ctrls, &names, ptype)?;
            let target = target_of(c)?;
            let (value, keys) = interp_track(m, &h, b, cb.interp);
            Some(Track {
                ctrl: c.block,
                target,
                flags: 0x08 | ((s.cycle.min(3) as u16) << 1),
                freq: s.frequency,
                // sequences have no Phase at 20.2.0.7 (F§7.1)
                phase: 0.0,
                start: s.start,
                stop: s.stop,
                value,
                keys,
            })
        })
        .collect()
}

/// `base` with every track applied at time `t`, in order (a later track on the same target wins). Raw NIF
/// values; sRGB→linear × multiple for EmitColor happens downstream in render3d.
pub fn apply_anim(base: &MaterialState, tracks: &[Track], t: f32) -> MaterialState {
    let mut out = base.clone();
    for tr in tracks {
        let ct = ctrl_time(t, tr.freq, tr.phase, tr.start, tr.stop, tr.flags);
        let v = match &tr.keys {
            Some(KeyData::Float(g)) if !g.keys.is_empty() => sample_f32(g, ct).map(AnimValue::F32),
            Some(KeyData::Pos(g)) if !g.keys.is_empty() => sample_vec3(g, ct).map(AnimValue::Vec3),
            _ => Some(tr.value),
        };
        let p = &mut out.params;
        match (tr.target, v) {
            (Target::Lscf(var), Some(AnimValue::F32(v))) => match var {
                20 => p.uv_offset[0] = v,
                22 => p.uv_offset[1] = v,
                21 => p.uv_scale[0] = v,
                23 => p.uv_scale[1] = v,
                11 => p.emissive_multiple = v,
                9 => p.glossiness = v,
                10 => p.specular_strength = v,
                12 => p.alpha = v,
                8 => {
                    // only the EnvMap trailer has this constant (F3)
                    if let LightingTrailer::EnvMap { scale } = &mut p.trailer {
                        *scale = v;
                    }
                }
                _ => {}
            },
            (Target::Lscc(1), Some(AnimValue::Vec3(v))) => p.emissive_color = v,
            (Target::Lscc(0), Some(AnimValue::Vec3(v))) => p.specular_color = v,
            (Target::AlphaRef, Some(AnimValue::F32(v))) if !v.is_nan() => {
                if let Some(a) = &mut out.alpha {
                    a.threshold = v.round().clamp(0.0, 255.0) as u8;
                }
            }
            _ => {}
        }
    }
    out
}
