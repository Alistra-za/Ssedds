mod builder;
use builder::scenes;
use nif::edit::*;
use nif::fields::{locate, Field, Value};
use nif::header::Header;
use nif::model::read_model;
use std::ops::Range;

fn read(f: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/fixtures/{f}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// Runs of differing bytes between two files of the same length.
fn changed_ranges(a: &[u8], b: &[u8]) -> Vec<Range<usize>> {
    assert_eq!(a.len(), b.len(), "length changed");
    let mut v: Vec<Range<usize>> = vec![];
    for i in (0..a.len()).filter(|&i| a[i] != b[i]) {
        match v.last_mut() {
            Some(r) if r.end == i => r.end += 1,
            _ => v.push(i..i + 1),
        }
    }
    v
}

fn current(b: &[u8], block: u32, f: Field) -> Value {
    locate(&Header::parse(b).unwrap(), b, block as usize, f)
        .unwrap()
        .1
}

fn alpha_anchor(b: &[u8]) -> Anchor {
    let m = read_model(b).unwrap();
    anchor_role(&m, Some(m.shapes[0].block), Role::Alpha).unwrap()
}

fn shader_anchor(b: &[u8]) -> Anchor {
    let m = read_model(b).unwrap();
    anchor_role(&m, Some(m.shapes[0].block), Role::Shader).unwrap()
}

fn gloss(at: &Anchor, to: f32, base: f32) -> Op {
    Op::SetField {
        at: at.clone(),
        field: Field::Glossiness,
        value: Value::F32(to.to_bits()),
        base: Value::F32(base.to_bits()),
    }
}

#[test]
fn no_ops_is_identity() {
    let b = read("TestNifFile_Skinned_SE.nif");
    assert_eq!(apply(&b, &[]).unwrap().bytes, b);
}

#[test]
fn apply_errs_only_when_src_does_not_open() {
    assert!(apply(b"not a nif", &[]).is_err());
}

#[test]
fn set_field_patches_one_float_exactly() {
    let b = read("TestNifFile_Skinned_SE.nif");
    let m = read_model(&b).unwrap();
    let s = m.shapes[0].block;
    let at = anchor_role(&m, Some(s), Role::Shader).unwrap();
    let base = current(&b, at.block, Field::Glossiness);
    let r = apply(
        &b,
        &[Op::SetField {
            at,
            field: Field::Glossiness,
            value: Value::F32(12.5f32.to_bits()),
            base,
        }],
    )
    .unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    assert_eq!(changed_ranges(&b, &r.bytes).len(), 1); // one 4-byte run
}

#[test]
fn set_bits_keeps_hidden_alpha_sub_fields() {
    // alpha flags 0x12EC (blend off; src 6, dst 7 hidden): mask 1 value 1 -> 0x12ED; then mask 0x1E value 0 (src ONE) -> 0x12E1
    let b = scenes::alpha_shape(0x12EC);
    let at = alpha_anchor(&b);
    let ops = |mask, value, base| Op::SetBits {
        at: at.clone(),
        field: Field::AlphaFlags,
        mask,
        value,
        base,
    };
    let r = apply(&b, &[ops(1, 1, 0x12EC)]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    assert_eq!(
        current(&r.bytes, at.block, Field::AlphaFlags),
        Value::U16(0x12ED)
    );
    let r = apply(&r.bytes, &[ops(0x1E, 0, 0x12ED)]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    assert_eq!(
        current(&r.bytes, at.block, Field::AlphaFlags),
        Value::U16(0x12E1)
    );
    // both at once give the same bytes, and a base differing only outside the mask is not a conflict
    let both = apply(&b, &[ops(1, 1, 0xFFFF_FFEC), ops(0x1E, 0, 0x12ED)]).unwrap();
    assert_eq!(both.outcomes, vec![Outcome::Applied; 2]);
    assert_eq!(both.bytes, r.bytes);
}

#[test]
fn set_bits_conflicts_when_masked_bits_changed() {
    let b = scenes::alpha_shape(0x12EC);
    let at = alpha_anchor(&b);
    let op = Op::SetBits {
        at,
        field: Field::AlphaFlags,
        mask: 1,
        value: 0,
        base: 1, // the file has bit 0 clear
    };
    let r = apply(&b, &[op]).unwrap();
    assert!(matches!(r.outcomes[0], Outcome::Conflict(_)));
    assert_eq!(r.bytes, b);
}

#[test]
fn stale_base_is_a_conflict_and_later_ops_still_apply() {
    let b = scenes::alpha_shape(0x12EC);
    let at = alpha_anchor(&b);
    let thr = |to, base| Op::SetField {
        at: at.clone(),
        field: Field::AlphaThreshold,
        value: Value::U8(to),
        base: Value::U8(base),
    };
    let r = apply(&b, &[thr(10, 99), thr(20, 128)]).unwrap(); // the file holds 128
    assert_eq!(
        r.outcomes[0],
        Outcome::Conflict("changed in the source: was U8(99), now U8(128)".into())
    );
    assert_eq!(r.outcomes[1], Outcome::Applied);
    assert_eq!(
        current(&r.bytes, at.block, Field::AlphaThreshold),
        Value::U8(20)
    );
    assert_eq!(changed_ranges(&b, &r.bytes).len(), 1);
}

#[test]
fn variable_change_on_managed_controller_is_refused() {
    let b = scenes::managed_clip();
    let m = read_model(&b).unwrap();
    let shape = m.shapes[0].block;
    let at = anchor_role(
        &m,
        Some(shape),
        Role::Ctrl {
            owner: Owner::Shader,
            class: "BSLightingShaderPropertyFloatController".into(),
            var: Some(11),
            nth: 0,
        },
    )
    .unwrap();
    assert!(m.managed(at.block));
    let op = Op::SetField {
        at,
        field: Field::Variable,
        value: Value::U32(20),
        base: Value::U32(11),
    };
    let r = apply(&b, &[op]).unwrap();
    assert!(
        matches!(r.outcomes[0], Outcome::Refused(_)),
        "{:?}",
        r.outcomes
    );
    assert_eq!(r.bytes, b);
}

#[test]
fn reordered_blocks_reanchor_by_role() {
    let (b1, b2) = (scenes::reordered(false, "S"), scenes::reordered(true, "S"));
    let at = shader_anchor(&b1);
    assert_eq!(at.block, 1);
    let r = apply(&b2, &[gloss(&at, 12.5, 80.0)]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::ReAnchored { block: 0 }]);
    assert_eq!(
        current(&r.bytes, 0, Field::Glossiness),
        Value::F32(12.5f32.to_bits())
    );
    assert_eq!(changed_ranges(&b2, &r.bytes).len(), 1);
    // and on its own file the recorded index still wins
    let r = apply(&b1, &[gloss(&at, 12.5, 80.0)]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
}

#[test]
fn renamed_shape_is_a_conflict_not_a_misapply() {
    let at = shader_anchor(&scenes::reordered(false, "S"));
    let renamed = scenes::reordered(false, "T");
    let r = apply(&renamed, &[gloss(&at, 12.5, 80.0)]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Conflict("missing".into())]);
    assert_eq!(r.bytes, renamed);
}

#[test]
fn duplicate_shape_names_without_index_match_are_ambiguous() {
    let b = scenes::dup_names();
    let m = read_model(&b).unwrap();
    let at = |block| Anchor {
        block,
        kind: "BSLightingShaderProperty".into(),
        shape: Some("A".into()),
        role: Role::Shader,
    };
    assert_eq!(resolve(&m, &at(3)), Ok((3, false))); // index matches one of them
    assert_eq!(
        resolve(&m, &at(9)),
        Err("ambiguous: 2 candidates".to_string())
    );
    let sh = Anchor {
        block: 9,
        kind: "BSTriShape".into(),
        shape: Some("A".into()),
        role: Role::Shape,
    };
    assert_eq!(resolve(&m, &sh), Err("ambiguous: 2 shapes named A".into()));
    let r = apply(&b, &[gloss(&at(9), 1.0, 80.0)]).unwrap();
    assert!(matches!(&r.outcomes[0], Outcome::Conflict(s) if s.starts_with("ambiguous")));
    assert_eq!(r.bytes, b);
}

fn shape_named(m: &nif::model::NifModel, name: &str) -> u32 {
    m.shapes
        .iter()
        .find(|s| s.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no shape {name}"))
        .block
}

fn block_bytes_of(b: &[u8], k: usize) -> Vec<u8> {
    let h = Header::parse(b).unwrap();
    b[h.offsets()[k]..h.offsets()[k] + h.sizes[k] as usize].to_vec()
}

fn type_op(b: &[u8], to: u32) -> (Op, u32) {
    let at = shader_anchor(b);
    let base = current(b, at.block, Field::ShaderType);
    let blk = at.block;
    let op = Op::SetField {
        at,
        field: Field::ShaderType,
        value: Value::U32(to),
        base,
    };
    (op, blk)
}

fn trailer(b: &[u8], block: u32, n: u8) -> f32 {
    current(b, block, Field::Trailer(n)).as_f32().unwrap()
}

#[test]
fn trailer_defaults_table() {
    assert_eq!(trailer_defaults(7), vec![4.0, 1.0]);
    assert_eq!(trailer_defaults(11), vec![5.0, 0.25, 1.0, 1.0, 1.0]);
    assert_eq!(trailer_defaults(16)[0], 1.3);
    assert!(trailer_defaults(0).is_empty() && trailer_defaults(3).is_empty());
}

#[test]
fn shader_type_change_rewrites_only_the_trailer_and_size() {
    let b = scenes::lsp_shape(5, &[0.25, 0.5, 0.75]); // the fixtures are all type 0
    let (op, blk) = type_op(&b, 1);
    assert_eq!(current(&b, blk, Field::ShaderType), Value::U32(5));
    let r = apply(&b, &[op]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    let (old, new) = (
        block_bytes_of(&b, blk as usize),
        block_bytes_of(&r.bytes, blk as usize),
    );
    assert_eq!(new.len(), old.len() - 8);
    assert_eq!(new[..4], 1u32.to_le_bytes());
    assert_eq!(new[4..new.len() - 4], old[4..new.len() - 4]);
    assert_eq!(trailer(&r.bytes, blk, 0), 1.0);
    assert!(read_model(&r.bytes).unwrap().problems.is_empty());
    for k in (0..Header::parse(&b).unwrap().sizes.len()).filter(|&k| k as u32 != blk) {
        assert_eq!(block_bytes_of(&b, k), block_bytes_of(&r.bytes, k), "{k}");
    }
}

#[test]
fn shader_type_5_to_6_keeps_the_tint_floats() {
    let b = scenes::lsp_shape(5, &[0.25, 0.5, 0.75]);
    let (op, blk) = type_op(&b, 6);
    let r = apply(&b, &[op]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    let s = Header::parse(&b).unwrap().offsets()[blk as usize];
    assert_eq!(changed_ranges(&b, &r.bytes), vec![s..s + 1]); // 5 -> 6: one byte
    let got: Vec<f32> = (0..3).map(|n| trailer(&r.bytes, blk, n)).collect();
    assert_eq!(got, vec![0.25, 0.5, 0.75]);
    // a different trailer length takes the defaults
    let (op, _) = type_op(&r.bytes, 7);
    let r = apply(&r.bytes, &[op]).unwrap();
    assert_eq!(
        (trailer(&r.bytes, blk, 0), trailer(&r.bytes, blk, 1)),
        (4.0, 1.0)
    );
}

#[test]
fn shader_type_20_is_refused() {
    let b = read("TestNifFile_Skinned_SE.nif");
    for t in [20, 21, 1000] {
        let (op, _) = type_op(&b, t);
        let r = apply(&b, &[op]).unwrap();
        assert_eq!(
            r.outcomes,
            vec![Outcome::Refused(
                "shader type 20 is not a Skyrim type".into()
            )]
        );
        assert_eq!(r.bytes, b);
    }
}

#[test]
fn shader_type_stale_base_is_a_conflict() {
    let b = read("TestNifFile_Skinned_SE.nif");
    let (mut op, _) = type_op(&b, 1);
    if let Op::SetField { base, .. } = &mut op {
        *base = Value::U32(9);
    }
    let r = apply(&b, &[op]).unwrap();
    assert!(matches!(r.outcomes[0], Outcome::Conflict(_)));
    assert_eq!(r.bytes, b);
}

#[test]
fn extra_ops_are_not_supported_yet() {
    let b = scenes::alpha_shape(0x12EC);
    let m = read_model(&b).unwrap();
    let r = apply(
        &b,
        &[Op::RemoveExtra {
            at: anchor_shape(&m, 0),
        }],
    )
    .unwrap();
    assert_eq!(
        r.outcomes,
        vec![Outcome::Refused("not supported yet".into())]
    );
}

fn split(m: &nif::model::NifModel, name: &str) -> Op {
    Op::SplitAlpha {
        at: anchor_shape(m, shape_named(m, name)),
    }
}

#[test]
fn split_gives_one_shape_its_own_alpha_and_chain() {
    let b = scenes::shared_alpha_with_testref_chain();
    let m = read_model(&b).unwrap();
    let a = shape_named(&m, "A");
    let r = apply(&b, &[split(&m, "A")]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    let m2 = read_model(&r.bytes).unwrap();
    assert!(m2.problems.is_empty(), "{:?}", m2.problems);
    let (a2, b2) = (shape_named(&m2, "A"), shape_named(&m2, "B"));
    assert_ne!(m2.shape(a2).unwrap().alpha, m2.shape(b2).unwrap().alpha);
    assert_eq!(
        m2.shape(b2).unwrap().alpha,
        m.shape(shape_named(&m, "B")).unwrap().alpha
    );
    let new = m2.shape(a2).unwrap().alpha.unwrap();
    let chain = &m2.alphas[&new].prop.ctrls;
    assert_eq!(chain.len(), 1);
    let c = &m2.ctrls[&chain[0]];
    assert_eq!(c.target, new as i32);
    assert_eq!(c.next, -1);
    // its own interpolator and data, distinct from the originals (6, 7)
    let i = c.interp.unwrap();
    assert!(i > 7);
    let i = &m2.interps[&(i as u32)];
    assert!(i.data > 7);
    let h = Header::parse(&r.bytes).unwrap();
    assert_eq!(h.kind(i.data as usize), b"NiFloatData");
    assert_eq!(
        block_bytes_of(&r.bytes, i.data as usize),
        block_bytes_of(&b, 7)
    );
    // the original alpha keeps its chain and now has one user
    let old = &m2.alphas[&m.shape(shape_named(&m, "B")).unwrap().alpha.unwrap()];
    assert_eq!((old.prop.ctrls.clone(), old.prop.users.len()), (vec![5], 1));
    let old_len = Header::parse(&b).unwrap().sizes.len();
    for k in 0..old_len {
        if k as u32 != a {
            assert_eq!(block_bytes_of(&r.bytes, k), block_bytes_of(&b, k), "{k}");
        }
    }
}

#[test]
fn split_copies_a_two_controller_chain_with_a_shared_interpolator() {
    let b = scenes::shared_alpha_two_ctrl_chain();
    let m = read_model(&b).unwrap();
    let r = apply(&b, &[split(&m, "A")]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    let m2 = read_model(&r.bytes).unwrap();
    assert!(m2.problems.is_empty(), "{:?}", m2.problems);
    let new = m2.shape(shape_named(&m2, "A")).unwrap().alpha.unwrap();
    let chain = &m2.alphas[&new].prop.ctrls;
    assert_eq!(chain.len(), 2);
    let (c1, c2) = (&m2.ctrls[&chain[0]], &m2.ctrls[&chain[1]]);
    assert_eq!((c1.next, c2.next), (chain[1] as i32, -1));
    assert_eq!((c1.target, c2.target), (new as i32, new as i32));
    let (i1, i2) = (c1.interp.unwrap(), c2.interp.unwrap());
    assert!(i1 > 8 && i2 > 8 && i1 != i2); // one copy per controller
    assert_ne!(m2.interps[&(i1 as u32)].data, m2.interps[&(i2 as u32)].data);
    for k in 0..Header::parse(&b).unwrap().sizes.len() {
        if k as u32 != shape_named(&m, "A") {
            assert_eq!(block_bytes_of(&r.bytes, k), block_bytes_of(&b, k), "{k}");
        }
    }
}

#[test]
fn split_does_not_copy_a_data_ref_past_the_source_blocks() {
    let mut b = scenes::shared_alpha_two_ctrl_chain();
    let h = Header::parse(&b).unwrap();
    let n = h.sizes.len() as u32;
    let at = h.offsets()[7] + 4; // the interpolator's data ref
    b[at..at + 4].copy_from_slice(&n.to_le_bytes());
    let m = read_model(&b).unwrap();
    let r = apply(&b, &[split(&m, "A")]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    // alpha + 2 controllers + 2 interpolators, no data copies; the hostile ref is kept as is
    assert_eq!(Header::parse(&r.bytes).unwrap().sizes.len() as u32, n + 5);
}

#[test]
fn split_with_one_user_is_a_no_op() {
    let b = scenes::alpha_shape(0x12EC);
    let m = read_model(&b).unwrap();
    let r = apply(
        &b,
        &[Op::SplitAlpha {
            at: anchor_shape(&m, 0),
        }],
    )
    .unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    assert_eq!(r.bytes, b);
}

#[test]
fn split_with_a_managed_controller_is_refused() {
    let b = scenes::shared_alpha_with_managed_ctrl();
    let m = read_model(&b).unwrap();
    let r = apply(&b, &[split(&m, "A")]).unwrap();
    assert_eq!(
        r.outcomes,
        vec![Outcome::Refused(
            "Shared alpha with a clip controller: splitting needs Phase C.".into()
        )]
    );
    assert_eq!(r.bytes, b);
}

#[test]
fn split_then_undo_restores_source_bytes() {
    // undo = re-apply the remaining ops (none) to the pristine source
    let b = scenes::shared_alpha_with_testref_chain();
    let m = read_model(&b).unwrap();
    let r = apply(&b, &[split(&m, "A")]).unwrap();
    assert_ne!(r.bytes, b);
    assert_eq!(apply(&b, &[]).unwrap().bytes, b);
    // splitting again is a no-op: A is the only user of its new alpha
    let again = apply(&r.bytes, &[split(&m, "A")]).unwrap();
    assert_eq!(again.outcomes, vec![Outcome::Applied]);
    assert_eq!(again.bytes, r.bytes);
}

#[test]
fn add_then_remove_alpha_leaves_an_orphan_and_ref_minus_one() {
    let b = scenes::reordered(false, "S");
    let m = read_model(&b).unwrap();
    let at = anchor_shape(&m, 0);
    let add = Op::AddAlpha {
        at: at.clone(),
        flags: 4844,
        threshold: 128,
    };
    let r = apply(&b, std::slice::from_ref(&add)).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    let m2 = read_model(&r.bytes).unwrap();
    let n = Header::parse(&b).unwrap().sizes.len() as u32;
    assert_eq!(m2.shape(0).unwrap().alpha, Some(n));
    assert_eq!(current(&r.bytes, n, Field::AlphaFlags), Value::U16(4844));
    assert_eq!(current(&r.bytes, n, Field::AlphaThreshold), Value::U8(128));
    assert_eq!(m2.alphas[&n].prop.users, vec![0]);
    // a second add is refused
    let r2 = apply(&r.bytes, &[add]).unwrap();
    assert!(matches!(r2.outcomes[0], Outcome::Refused(_)));
    assert_eq!(r2.bytes, r.bytes);
    let rm = apply(&r.bytes, &[Op::RemoveAlpha { at }]).unwrap();
    assert_eq!(rm.outcomes, vec![Outcome::Applied]);
    let m3 = read_model(&rm.bytes).unwrap();
    assert_eq!(m3.shape(0).unwrap().alpha, None);
    let at_ref = m3.shapes[0].at.alpha;
    assert_eq!(rm.bytes[at_ref..at_ref + 4], (-1i32).to_le_bytes());
    // the orphan stays
    assert!(m3.alphas.contains_key(&n) && m3.alphas[&n].prop.users.is_empty());
    // nothing to remove the second time
    let again = Op::RemoveAlpha {
        at: anchor_shape(&m3, 0),
    };
    let rm2 = apply(&rm.bytes, &[again]).unwrap();
    assert!(matches!(rm2.outcomes[0], Outcome::Refused(_)));
}

#[test]
fn anchors_for_every_scene_resolve_to_themselves() {
    for b in scenes::all() {
        let m = read_model(&b).unwrap();
        for s in &m.shapes {
            for role in [Role::Shape, Role::Shader, Role::Alpha, Role::TextureSet] {
                if let Some(a) = anchor_role(&m, Some(s.block), role) {
                    // an unnamed or duplicated shape name still resolves by its own index
                    assert_eq!(resolve(&m, &a), Ok((a.block, false)), "{a:?}");
                }
            }
        }
    }
}

#[test]
fn merge_key_names_variant_block_and_field() {
    let b = scenes::alpha_shape(0x12EC);
    let at = alpha_anchor(&b);
    let op = Op::SetBits {
        at,
        field: Field::AlphaFlags,
        mask: 1,
        value: 1,
        base: 0,
    };
    assert_eq!(merge_key(&op), "SetBits|1|AlphaFlags");
}
