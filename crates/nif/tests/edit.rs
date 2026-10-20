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

#[test]
fn structural_ops_are_not_supported_yet() {
    let b = scenes::alpha_shape(0x12EC);
    let m = read_model(&b).unwrap();
    let r = apply(
        &b,
        &[Op::SplitAlpha {
            at: anchor_shape(&m, 0),
        }],
    )
    .unwrap();
    assert_eq!(
        r.outcomes,
        vec![Outcome::Refused("not supported yet".into())]
    );
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
