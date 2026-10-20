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

fn path_op(m: &nif::model::NifModel, shape: u32, slot: u32, path: &str) -> Op {
    let set = m.lighting[&m.shape(shape).unwrap().shader.unwrap()]
        .texture_set
        .unwrap();
    let base = m.texsets[&set].raw.slots.get(slot as usize).cloned();
    Op::SetTexturePath {
        at: anchor_shape(m, shape),
        slot,
        path: path.into(),
        base: String::from_utf8(base.unwrap_or_default()).unwrap(),
    }
}

#[test]
fn unshared_set_edit_changes_only_that_block_and_its_size() {
    let b = scenes::one_shape_set(&["textures\\a.dds", "textures\\a_n.dds"]);
    let m = read_model(&b).unwrap();
    let op = path_op(&m, 0, 1, "textures\\much\\longer\\a_n.dds");
    let r = apply(&b, &[op]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    let h2 = Header::parse(&r.bytes).unwrap();
    assert_eq!(h2.sizes.len(), 3, "no block appended");
    for k in [0, 2] {
        assert_eq!(block_bytes_of(&r.bytes, k), block_bytes_of(&b, k));
    }
    let m2 = read_model(&r.bytes).unwrap();
    assert!(m2.problems.is_empty());
    assert_eq!(m2.texsets[&1].raw.slots[0], b"textures\\a.dds");
    assert_eq!(
        m2.texsets[&1].raw.slots[1],
        b"textures\\much\\longer\\a_n.dds"
    );
    assert!(h2.sizes[1] > Header::parse(&b).unwrap().sizes[1]);
}

#[test]
fn slot_counts_9_6_4_3_2_are_never_padded() {
    for n in [9usize, 6, 4, 3, 2] {
        let paths: Vec<String> = (0..n).map(|i| format!("textures\\s{i}.dds")).collect();
        let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
        let b = scenes::one_shape_set(&refs);
        let m = read_model(&b).unwrap();
        let r = apply(&b, &[path_op(&m, 0, 0, "textures\\x.dds")]).unwrap();
        let m2 = read_model(&r.bytes).unwrap();
        assert_eq!(m2.texsets[&1].raw.slots.len(), n);
        assert_eq!(m2.texsets[&1].raw.slots[0], b"textures\\x.dds");
        assert_eq!(m2.texsets[&1].raw.slots[1..], m.texsets[&1].raw.slots[1..]);
    }
}

#[test]
fn slot_past_the_count_is_refused() {
    let b = scenes::one_shape_set(&["textures\\a.dds", "textures\\a_n.dds"]);
    let m = read_model(&b).unwrap();
    let op = Op::SetTexturePath {
        at: anchor_shape(&m, 0),
        slot: 2,
        path: "x".into(),
        base: String::new(),
    };
    let r = apply(&b, &[op]).unwrap();
    assert_eq!(
        r.outcomes,
        vec![Outcome::Refused("slot 2 is past the set's 2 slots".into())]
    );
    assert_eq!(r.bytes, b);
}

#[test]
fn stale_base_conflicts_and_unmappable_path_is_refused() {
    let b = scenes::one_shape_set(&["textures\\a.dds"]);
    let m = read_model(&b).unwrap();
    let mut stale = path_op(&m, 0, 0, "textures\\b.dds");
    if let Op::SetTexturePath { base, .. } = &mut stale {
        *base = "textures\\other.dds".into();
    }
    let r = apply(&b, &[stale, path_op(&m, 0, 0, "textures\\\u{4e2d}.dds")]).unwrap();
    assert!(matches!(r.outcomes[0], Outcome::Conflict(_)));
    assert!(matches!(r.outcomes[1], Outcome::Refused(_)));
    assert_eq!(r.bytes, b);
}

#[test]
fn second_edit_on_the_copy_edits_in_place() {
    let b = scenes::shared_alpha_and_set();
    let m = read_model(&b).unwrap();
    let a = shape_named(&m, "A");
    let ops = [
        path_op(&m, a, 0, "textures\\new\\a.dds"),
        Op::SetTexturePath {
            at: anchor_shape(&m, a),
            slot: 0,
            path: "textures\\newer\\a.dds".into(),
            base: "textures\\new\\a.dds".into(),
        },
    ];
    let r = apply(&b, &ops).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied, Outcome::Applied]);
    assert_eq!(
        Header::parse(&r.bytes).unwrap().sizes.len(),
        Header::parse(&b).unwrap().sizes.len() + 1,
        "one appended block only"
    );
    let m2 = read_model(&r.bytes).unwrap();
    let set = m2.lighting[&m2.shape(shape_named(&m2, "A")).unwrap().shader.unwrap()]
        .texture_set
        .unwrap();
    assert_eq!(m2.texsets[&set].raw.slots[0], b"textures\\newer\\a.dds");
    assert_eq!(block_bytes_of(&r.bytes, 3), block_bytes_of(&b, 3));
}

#[test]
fn shared_set_edit_leaves_other_shape_and_undo_is_identity() {
    let b = scenes::shared_alpha_and_set();
    let m = read_model(&b).unwrap();
    let a = shape_named(&m, "A");
    let set = m.lighting[&m.shape(a).unwrap().shader.unwrap()]
        .texture_set
        .unwrap();
    let r = apply(&b, &[path_op(&m, a, 0, "textures\\new\\a.dds")]).unwrap();
    let m2 = read_model(&r.bytes).unwrap();
    assert_eq!(
        block_bytes_of(&r.bytes, set as usize),
        block_bytes_of(&b, set as usize)
    );
    assert_eq!(
        Header::parse(&r.bytes).unwrap().sizes.len(),
        Header::parse(&b).unwrap().sizes.len() + 1
    );
    let a_set = m2.lighting[&m2.shape(shape_named(&m2, "A")).unwrap().shader.unwrap()]
        .texture_set
        .unwrap();
    assert_ne!(a_set, set);
    assert_eq!(m2.texsets[&a_set].raw.slots[0], b"textures\\new\\a.dds");
    assert_eq!(
        m2.texsets[&a_set].raw.slots[1..],
        m.texsets[&set].raw.slots[1..]
    );
    let b_set = m2.lighting[&m2.shape(shape_named(&m2, "B")).unwrap().shader.unwrap()]
        .texture_set
        .unwrap();
    assert_eq!(b_set, set);
    assert_eq!(apply(&b, &[]).unwrap().bytes, b);
}

fn root_extra(name: &str, kind: &str) -> Role {
    Role::Extra {
        on_root: true,
        name: name.into(),
        kind: kind.into(),
        nth: 0,
    }
}

fn add_extra(at: Anchor, kind: &str, name: &str, value: nif::extra::ExtraValue) -> Op {
    Op::AddExtra {
        at,
        kind: kind.into(),
        name: name.into(),
        value,
    }
}

#[test]
fn add_bsx_to_root_reemits_only_the_root_prefix() {
    use nif::extra::ExtraValue;
    let b = read("TestNifFile_Furniture_Col_SE.nif");
    let (h0, m0) = (Header::parse(&b).unwrap(), read_model(&b).unwrap());
    let root = m0.root.unwrap();
    let net = m0.root_net.clone().unwrap();
    let at = anchor_role(&m0, None, Role::Root).unwrap();
    let r = apply(
        &b,
        &[add_extra(at, "BSXFlags", "BSXFlags", ExtraValue::Bsx(0x83))],
    )
    .unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    let h1 = Header::parse(&r.bytes).unwrap();
    let new = h0.sizes.len();
    assert_eq!(h1.sizes.len(), new + 1);
    for k in (0..new).filter(|&k| k != root as usize) {
        assert_eq!(block_bytes_of(&r.bytes, k), block_bytes_of(&b, k), "{k}");
    }
    // strings are append-only
    assert_eq!(h1.strings[..h0.strings.len()], h0.strings[..]);
    let old = block_bytes_of(&b, root as usize);
    let base = h0.offsets()[root as usize];
    let (s, e) = (net.start - base, net.end - base);
    let mut extra = net.extra.clone();
    extra.push(new as i32);
    let mut want = old[..s].to_vec();
    want.extend(nif::net::encode_net(net.name, &extra, net.controller));
    want.extend(&old[e..]);
    assert_eq!(block_bytes_of(&r.bytes, root as usize), want);
    let m1 = read_model(&r.bytes).unwrap();
    let x = &m1.extras[&(new as u32)];
    assert_eq!(x.kind, "BSXFlags");
    assert_eq!(x.value, Some(ExtraValue::Bsx(0x83)));
    assert_eq!(x.name.as_deref(), Some(&b"BSXFlags"[..]));
}

#[test]
fn edit_string_extra_appends_a_string_and_patches_one_index() {
    use nif::extra::ExtraValue::Str;
    let b = scenes::root_extras();
    let m = read_model(&b).unwrap();
    let at = anchor_role(&m, None, root_extra("BODYTRI", "NiStringExtraData")).unwrap();
    let edit = |to: &str, base: &str| Op::EditExtra {
        at: at.clone(),
        value: Str(to.into()),
        base: Str(base.into()),
    };
    let r = apply(&b, &[edit("new text", "a\r\nb")]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    let (h0, h1) = (Header::parse(&b).unwrap(), Header::parse(&r.bytes).unwrap());
    assert_eq!(h1.strings.len(), h0.strings.len() + 1);
    assert_eq!(h1.strings[h0.strings.len()], b"new text");
    let (old, new) = (
        block_bytes_of(&b, at.block as usize),
        block_bytes_of(&r.bytes, at.block as usize),
    );
    assert_eq!(old[..4], new[..4]); // name untouched
    assert_eq!(new[4..], (h0.strings.len() as i32).to_le_bytes());
    let m1 = read_model(&r.bytes).unwrap();
    assert_eq!(m1.extras[&at.block].value, Some(Str("new text".into())));
    // a stale base is a conflict; an unmappable character is refused
    let r = apply(&b, &[edit("x", "other"), edit("\u{65e5}", "a\r\nb")]).unwrap();
    assert!(
        matches!(r.outcomes[0], Outcome::Conflict(_)),
        "{:?}",
        r.outcomes
    );
    assert!(matches!(&r.outcomes[1], Outcome::Refused(s) if s.contains("Windows-1252")));
    assert_eq!(r.bytes, b);
}

#[test]
fn edit_inv_marker_round_trips() {
    use nif::extra::ExtraValue::Inv;
    let b = scenes::root_extras();
    let m = read_model(&b).unwrap();
    let at = anchor_role(&m, None, root_extra("INV", "BSInvMarker")).unwrap();
    let op = Op::EditExtra {
        at,
        value: Inv {
            rot: [100, 200, 300],
            zoom: 2.0f32.to_bits(),
        },
        base: Inv {
            rot: [4712, 6283, 0],
            zoom: 1.0f32.to_bits(),
        },
    };
    let r = apply(&b, &[op]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    assert_eq!(r.bytes.len(), b.len());
    let m1 = read_model(&r.bytes).unwrap();
    let blk = m.root_net.as_ref().unwrap().extra[6] as u32;
    assert_eq!(
        m1.extras[&blk].value,
        Some(Inv {
            rot: [100, 200, 300],
            zoom: 2.0f32.to_bits()
        })
    );
}

#[test]
fn remove_extra_keeps_the_block_as_orphan_and_the_null_entries() {
    let b = scenes::root_extras();
    let m = read_model(&b).unwrap();
    let at = anchor_role(&m, None, root_extra("Count", "NiIntegerExtraData")).unwrap();
    assert_eq!(at.block, 2);
    let r = apply(&b, &[Op::RemoveExtra { at }]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    let m1 = read_model(&r.bytes).unwrap();
    assert_eq!(m1.root_net.unwrap().extra, vec![0, 1, -1, 3, 4, 5, 6]);
    assert!(m1.extras.contains_key(&2)); // orphan stays
    assert_eq!(block_bytes_of(&r.bytes, 2), block_bytes_of(&b, 2));
    assert_eq!(m1.extras.len(), m.extras.len());
}

#[test]
fn add_extra_to_a_shape_keeps_its_geometry_bytes() {
    use nif::extra::ExtraValue::Int;
    let b = scenes::alpha_shape(0x12EC);
    let m = read_model(&b).unwrap();
    let s = &m.shapes[0];
    let at = anchor_role(&m, Some(s.block), Role::Shape).unwrap();
    let r = apply(&b, &[add_extra(at, "NiIntegerExtraData", "Count", Int(5))]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    let (h0, m1) = (Header::parse(&b).unwrap(), read_model(&r.bytes).unwrap());
    assert_eq!(m1.shapes[0].net.extra, vec![2]);
    let e = h0.offsets()[0] + h0.sizes[0] as usize;
    let h1 = Header::parse(&r.bytes).unwrap();
    let e1 = h1.offsets()[0] + h1.sizes[0] as usize;
    assert_eq!(&r.bytes[m1.shapes[0].net.end..e1], &b[s.net.end..e]);
    assert_eq!(block_bytes_of(&r.bytes, 1), block_bytes_of(&b, 1));
}

#[test]
fn unknown_extra_kind_is_refused() {
    use nif::extra::ExtraValue::Int;
    let b = scenes::root_extras();
    let m = read_model(&b).unwrap();
    let at = anchor_role(&m, None, Role::Root).unwrap();
    let r = apply(
        &b,
        &[
            add_extra(at.clone(), "NiStringsExtraData", "X", Int(1)),
            add_extra(at, "NiIntegerExtraData", "\u{65e5}", Int(1)),
        ],
    )
    .unwrap();
    assert!(
        matches!(r.outcomes[0], Outcome::Refused(_)),
        "{:?}",
        r.outcomes
    );
    assert!(matches!(&r.outcomes[1], Outcome::Refused(s) if s.contains("Windows-1252")));
    assert_eq!(r.bytes, b);
}

#[test]
fn root_anchor_carries_the_root_kind_and_resolve_checks_it() {
    let b = scenes::root_extras();
    let m = read_model(&b).unwrap();
    let a = anchor_role(&m, None, Role::Root).unwrap();
    assert_eq!(a.kind, "NiNode");
    assert_eq!(resolve(&m, &a), Ok((7, false)));
    // a root of another type is not the same anchor: it re-anchors, never silently matches
    let swapped = Anchor {
        kind: "BSFadeNode".into(),
        ..a
    };
    assert_eq!(resolve(&m, &swapped), Ok((7, true)));
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
