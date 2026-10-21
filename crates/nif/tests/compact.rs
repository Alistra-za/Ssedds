//! Task GC (Note CQ): export drops the blocks our edits appended that nothing reaches.
mod builder;
use builder::scenes;
use nif::edit::*;
use nif::header::Header;
use nif::keys::{Key, KeyData, KeyGroup};
use nif::model::{read_model, NifModel};

const LSCF: &str = "BSLightingShaderPropertyFloatController";

fn lin(k: &[(f32, f32)]) -> KeyData {
    KeyData::Float(KeyGroup {
        key_type: Some(1),
        keys: k
            .iter()
            .map(|&(time, value)| Key {
                time,
                value,
                forward: None,
                backward: None,
                tbc: None,
            })
            .collect(),
    })
}

fn nblocks(b: &[u8]) -> usize {
    Header::parse(b).unwrap().sizes.len()
}

fn block_bytes(b: &[u8], k: usize) -> Vec<u8> {
    let h = Header::parse(b).unwrap();
    b[h.offsets[k]..h.offsets[k] + h.sizes[k] as usize].to_vec()
}

fn shader_of(m: &NifModel, name: &str) -> u32 {
    m.shapes
        .iter()
        .find(|s| s.name.as_deref() == Some(name))
        .unwrap()
        .block
}

fn add_ctrl(b: &[u8], keys: KeyData) -> Op {
    let m = read_model(b).unwrap();
    Op::AddController {
        at: anchor_role(&m, Some(shader_of(&m, "S")), Role::Shader).unwrap(),
        class: LSCF.into(),
        var: Some(11),
        keys,
        flags: 0x48,
        freq: 1.0,
        start: 0.0,
        stop: 1.0,
    }
}

fn ctrl_at(b: &[u8]) -> Anchor {
    let m = read_model(b).unwrap();
    let role = Role::Ctrl {
        owner: Owner::Shader,
        class: LSCF.into(),
        var: Some(11),
        nth: 0,
    };
    anchor_role(&m, Some(shader_of(&m, "S")), role).unwrap()
}

fn add_seq(b: &[u8], keys: KeyData) -> Op {
    let m = read_model(b).unwrap();
    Op::AddSequence {
        at: anchor_role(&m, None, Role::Root).unwrap(),
        name: "Clip".into(),
        cycle: 0,
        freq: 1.0,
        start: 0.0,
        stop: 1.0,
        tracks: vec![ClipTrack {
            at: ctrl_at(b),
            keys,
        }],
    }
}

fn remove_seq(b: &[u8]) -> Op {
    let m = read_model(b).unwrap();
    Op::RemoveSequence {
        at: anchor_role(&m, None, Role::Root).unwrap(),
        name: "Clip".into(),
        nth: 0,
    }
}

/// Runs each op on the bytes the ops before it produce, building it from those bytes as the GUI does.
type Step<'a> = &'a dyn Fn(&[u8]) -> Op;

fn run(src: &[u8], steps: &[Step]) -> Vec<u8> {
    let mut ops = vec![];
    let mut cur = src.to_vec();
    for s in steps {
        ops.push(s(&cur));
        let r = apply(src, &ops).unwrap();
        assert!(
            r.outcomes.iter().all(|o| *o == Outcome::Applied),
            "{:?}",
            r.outcomes
        );
        cur = r.bytes;
    }
    cur
}

/// The owner's scenario on `animated(None, 0)`: Root (0), an unlinked BSXFlags (1, a source orphan), shape S (2),
/// its BSLSP (3). Add + remove a controller; add one, clip it, remove the clip and the controller; add one and
/// clip it again.
fn owner_scenario() -> (Vec<u8>, Vec<u8>) {
    let src = scenes::animated(None, 0);
    let k1 = lin(&[(0.0, 1.0), (1.0, 2.0)]);
    let k2 = lin(&[(0.0, 3.0), (1.0, 4.0)]);
    let k3 = lin(&[(0.0, 5.0), (0.5, 6.0), (1.0, 7.0)]);
    let out = run(
        &src,
        &[
            &|b| add_ctrl(b, k1.clone()),
            &|b| Op::RemoveController { at: ctrl_at(b) },
            &|b| add_ctrl(b, k1.clone()),
            &|b| add_seq(b, k2.clone()),
            &remove_seq,
            &|b| Op::RemoveController { at: ctrl_at(b) },
            &|b| add_ctrl(b, k2.clone()),
            &|b| add_seq(b, k3.clone()),
        ],
    );
    (src, out)
}

/// What a reader sees, free of block numbers: shapes, each shader's chain with its keys, the root's extras, and every
/// live clip with its timing, text keys, CB strings, the chain position of the controller it drives and its keys.
fn summary(b: &[u8]) -> Vec<String> {
    let m = read_model(b).unwrap();
    let mut v = vec![];
    let pos = |c: i32| -> String {
        for s in &m.shapes {
            if let Some(l) = s.shader.and_then(|x| m.lighting.get(&x)) {
                if let Some(k) = l.prop.ctrls.iter().position(|&x| x as i32 == c) {
                    return format!("{:?}/shader/{k}", s.name);
                }
            }
        }
        format!("not on a chain ({c})")
    };
    for s in &m.shapes {
        v.push(format!("shape {:?} {:?}", s.name, s.kind));
        if let Some(l) = s.shader.and_then(|x| m.lighting.get(&x)) {
            for c in &l.prop.ctrls {
                let t = &m.ctrls[c];
                v.push(format!(
                    "  ctrl {} {:?} flags {:#x} times {:?} keys {:?}",
                    t.class,
                    t.var,
                    t.flags,
                    (t.frequency, t.phase, t.start, t.stop),
                    keys_of(b, &m, *c, None)
                ));
            }
        }
    }
    for x in m.extras_of(m.root_net.as_ref().unwrap()) {
        v.push(format!("root extra {} {:?} {:?}", x.kind, x.name, x.value));
    }
    for s in &m.sequences {
        v.push(format!(
            "clip {:?} cycle {} {:?} text keys {:?}",
            s.name,
            s.cycle,
            (s.frequency, s.start, s.stop),
            s.text_keys
        ));
        for cb in &s.blocks {
            v.push(format!(
                "  cb {:?} {:?} {:?} {:?} drives {} keys {:?}",
                cb.node,
                cb.property,
                cb.ctrl_type,
                cb.ctrl_id,
                pos(cb.controller),
                keys_of(b, &m, cb.controller as u32, Some(s.block))
            ));
        }
    }
    v
}

#[test]
fn owner_scenario_keeps_only_live_appended_blocks() {
    let (src, out) = owner_scenario();
    assert_eq!(nblocks(&src), 4);
    assert_eq!(nblocks(&out), 27, "the uncompacted export");
    let c = compact(&out, 4).unwrap();
    // source 4 + BSXFlags, manager, MTTC, palette, controller, data, keyed interp, blend, text keys, sequence
    assert_eq!(nblocks(&c), 14);
    let h = Header::parse(&c).unwrap();
    let kinds: Vec<String> = (4..14)
        .map(|i| String::from_utf8_lossy(h.kind(i)).into_owned())
        .collect();
    assert_eq!(
        kinds,
        [
            "BSXFlags",
            "NiControllerManager",
            "NiMultiTargetTransformController",
            "NiDefaultAVObjectPalette",
            LSCF,
            "NiFloatData",
            "NiFloatInterpolator",
            "NiBlendFloatInterpolator",
            "NiTextKeyExtraData",
            "NiControllerSequence",
        ]
    );
    // no unused type stays in the list
    assert_eq!(h.block_types.len(), {
        let mut t = h.type_index.clone();
        t.sort();
        t.dedup();
        t.len()
    });
    let m = read_model(&c).unwrap();
    assert!(m.problems.is_empty(), "{:?}", m.problems);
    assert_eq!(summary(&c), summary(&out));
    // the live clip still drives the shader's controller, which is managed
    let ctrl = m.lighting[&3].prop.ctrls[0];
    assert_eq!(m.sequences.len(), 1);
    assert_eq!(m.sequences[0].blocks[0].controller, ctrl as i32);
    assert!(m.managed(ctrl));
    assert_eq!(
        keys_of(&c, &m, ctrl, Some(m.sequences[0].block)).unwrap(),
        lin(&[(0.0, 5.0), (0.5, 6.0), (1.0, 7.0)])
    );
    // source blocks that name no appended block keep their bytes (the root names the BSX and the manager)
    for k in [1, 2] {
        assert_eq!(block_bytes(&c, k), block_bytes(&out, k), "block {k}");
    }
}

#[test]
fn compaction_is_idempotent() {
    let (_, out) = owner_scenario();
    let c = compact(&out, 4).unwrap();
    assert_eq!(compact(&c, 4).unwrap(), c);
}

#[test]
fn no_appended_orphan_is_byte_identical() {
    let src = scenes::animated(Some(1), 0);
    assert_eq!(compact(&src, 4).unwrap(), src, "no appended blocks");
    let out = run(&src, &[&|b| add_ctrl(b, lin(&[(0.0, 1.0), (1.0, 2.0)]))]);
    assert_eq!(nblocks(&out), 7);
    assert_eq!(compact(&out, 4).unwrap(), out);
}

#[test]
fn a_source_orphan_block_stays() {
    // animated(None, 0)'s BSXFlags (1) is listed by nothing
    let src = scenes::animated(None, 0);
    let out = run(
        &src,
        &[&|b| add_ctrl(b, lin(&[(0.0, 1.0), (1.0, 2.0)])), &|b| {
            Op::RemoveController { at: ctrl_at(b) }
        }],
    );
    let c = compact(&out, 4).unwrap();
    // the controller, its interpolator and data go; the appended BSXFlags the root lists stays
    assert_eq!(nblocks(&c), 5);
    assert_eq!(block_bytes(&c, 1), block_bytes(&src, 1));
    assert_eq!(Header::parse(&c).unwrap().kind(1), b"BSXFlags");
    assert_eq!(Header::parse(&c).unwrap().kind(4), b"BSXFlags");
}

#[test]
fn hostile_input_never_panics_and_refuses() {
    let (_, out) = owner_scenario();
    let h = Header::parse(&out).unwrap();
    // truncated anywhere
    for n in [0, 10, h.data_start, h.offsets[20], out.len() - 1] {
        assert!(compact(&out[..n], 4).is_err(), "truncated at {n}");
    }
    // a garbled controlled-block count in the live sequence
    let seq = h.offsets[26];
    let mut g = out.clone();
    g[seq + 4..seq + 8].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(compact(&g, 4).is_err());
    // a live ref past the file
    let mut g = out.clone();
    g[seq + 12..seq + 16].copy_from_slice(&1000i32.to_le_bytes());
    assert!(compact(&g, 4).is_err());
    // a source count past the file
    assert!(compact(&out, 28).is_err());
    // every single-byte garble of the appended blocks: no panic, either result
    for at in h.offsets[4]..h.footer_at {
        for v in [0x00, 0x7F, 0xFF] {
            let mut g = out.clone();
            g[at] = v;
            let _ = compact(&g, 4);
        }
    }
}

/// The owner's real file (Starsight Eyes eye mesh after add/remove/add/clip/remove-clip/remove/add/clip): its source
/// had 9 blocks.
#[test]
#[ignore]
fn owner_eye_mesh() {
    let Ok(acherus) = std::env::var("SDC_ACHERUS") else {
        println!("skipped: set SDC_ACHERUS");
        return;
    };
    let p = std::path::Path::new(&acherus)
        .join(r"mods\s6\meshes\artsick\eyemod\colour\eyecolourbase.nif");
    let Ok(b) = std::fs::read(&p) else {
        println!("skipped: {} not found", p.display());
        return;
    };
    let c = compact(&b, 9).unwrap();
    println!("{} -> {} blocks", nblocks(&b), nblocks(&c));
    assert!(nblocks(&c) < nblocks(&b));
    assert_eq!(compact(&c, 9).unwrap(), c);
    assert!(read_model(&c).unwrap().problems.is_empty());
}

fn shape_op(b: &[u8], f: fn(Anchor) -> Op) -> Op {
    let m = read_model(b).unwrap();
    let a = m.shapes.iter().find(|s| s.name.as_deref() == Some("A"));
    f(anchor_shape(&m, a.unwrap().block))
}

fn split_a(b: &[u8]) -> Op {
    shape_op(b, |at| Op::SplitAlpha { at })
}

fn remove_alpha_a(b: &[u8]) -> Op {
    shape_op(b, |at| Op::RemoveAlpha { at })
}

#[test]
fn split_copies_are_read_and_kept_while_linked() {
    for src in [
        scenes::shared_alpha_with_testref_chain(),
        scenes::shared_alpha_with_managed_ctrl(),
        scenes::shared_alpha_two_ctrl_chain(),
    ] {
        let n = nblocks(&src) as u32;
        let out = run(&src, &[&split_a]);
        assert!(nblocks(&out) as u32 > n);
        assert_eq!(compact(&out, n).unwrap(), out);
        // removing the split shape's alpha orphans the copy and its chain: the bytes of removing it unsplit
        let out = run(&src, &[&split_a, &remove_alpha_a]);
        let plain = run(&src, &[&remove_alpha_a]);
        assert_eq!(compact(&out, n).unwrap(), plain);
    }
}
