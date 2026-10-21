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

/// Final review M1: an extra whose name has a byte >= 0x80 anchors under the Windows-1252 name the panel shows.
#[test]
fn a_non_ascii_extra_name_anchors_as_the_panel_names_it() {
    let mut n = builder::Nif::new(100);
    let nm = n.string("Countè");
    n.block("NiIntegerExtraData", builder::W::default().i32(nm).u32(7).0);
    let r = n.string("Root");
    let root = builder::W::default().i32(r).u32(1).i32(0).i32(-1);
    n.block("NiNode", scenes::avobj(root).u32(0).u32(0).0);
    n.roots = vec![1];
    let b = n.bytes();
    let m = read_model(&b).unwrap();
    let shown = nif::text::lossy("Countè".as_bytes());
    let at = anchor_role(&m, None, root_extra(&shown, "NiIntegerExtraData"))
        .expect("anchors by the shown name");
    let r = apply(&b, &[Op::RemoveExtra { at }]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
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

#[test]
fn shared_shader_texture_edit_is_refused() {
    let b = scenes::two_shapes_one_shader();
    let m = read_model(&b).unwrap();
    let r = apply(&b, &[path_op(&m, 0, 0, "textures\\x.dds")]).unwrap();
    assert_eq!(
        r.outcomes,
        vec![Outcome::Refused(
            "This shader is shared by 2 shapes: a texture path edit would change all of them."
                .into()
        )]
    );
    assert_eq!(r.bytes, b);
}

#[test]
fn texture_edit_without_shader_or_set_is_refused() {
    for b in [scenes::alpha_shape(0x12EC), scenes::reordered(false, "S")] {
        let m = read_model(&b).unwrap();
        let op = Op::SetTexturePath {
            at: anchor_shape(&m, m.shapes[0].block),
            slot: 0,
            path: "x".into(),
            base: String::new(),
        };
        let r = apply(&b, &[op]).unwrap();
        assert!(
            matches!(r.outcomes[0], Outcome::Refused(_)),
            "{:?}",
            r.outcomes
        );
        assert_eq!(r.bytes, b);
    }
}

/// The Skinned fixture with its glossiness set to `to` (the "source changed" file).
fn with_gloss(b: &[u8], at: &Anchor, to: f32) -> Vec<u8> {
    let base = current(b, at.block, Field::Glossiness).as_f32().unwrap();
    apply(b, &[gloss(at, to, base)]).unwrap().bytes
}

#[test]
fn unchanged_field_in_a_changed_source_applies() {
    let b = read("TestNifFile_Skinned_SE.nif");
    let at = shader_anchor(&b);
    let op = Op::SetField {
        at: at.clone(),
        field: Field::SpecularStrength,
        value: Value::F32(2.0f32.to_bits()),
        base: current(&b, at.block, Field::SpecularStrength),
    };
    let new_src = with_gloss(&b, &at, 50.0); // a different field changed
    let (ops, outcomes) = rebase(&new_src, std::slice::from_ref(&op)).unwrap();
    assert_eq!(outcomes, vec![Outcome::Applied]);
    assert_eq!(ops, vec![op]);
}

#[test]
fn same_field_changed_in_source_is_a_conflict_and_keep_mine_resolves_it() {
    let b = read("TestNifFile_Skinned_SE.nif");
    let at = shader_anchor(&b);
    let base = current(&b, at.block, Field::Glossiness).as_f32().unwrap();
    let ops = [gloss(&at, 12.5, base)];
    let new_src = with_gloss(&b, &at, 50.0);
    assert!(matches!(
        apply(&new_src, &ops).unwrap().outcomes[0],
        Outcome::Conflict(_)
    ));
    let kept = keep_mine(&new_src, &ops, 0).unwrap();
    assert_eq!(kept, vec![gloss(&at, 12.5, 50.0)]);
    let r = apply(&new_src, &kept).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    assert_eq!(
        current(&r.bytes, at.block, Field::Glossiness),
        Value::F32(12.5f32.to_bits())
    );
}

#[test]
fn keep_mine_sets_base_for_set_bits_and_leaves_missing_anchors_alone() {
    let b = scenes::alpha_shape(0x12EC);
    let at = alpha_anchor(&b);
    let bits = |base| Op::SetBits {
        at: at.clone(),
        field: Field::AlphaFlags,
        mask: 1,
        value: 0,
        base,
    };
    let stale = [bits(1)]; // the file has bit 0 clear
    assert!(matches!(
        apply(&b, &stale).unwrap().outcomes[0],
        Outcome::Conflict(_)
    ));
    let kept = keep_mine(&b, &stale, 0).unwrap();
    assert_eq!(kept, vec![bits(0x12EC)]);
    assert_eq!(apply(&b, &kept).unwrap().outcomes, vec![Outcome::Applied]);
    // an anchor that no longer resolves (unnamed shape, wrong block) comes back unchanged
    let mut gone = at.clone();
    gone.block = 77;
    gone.shape = None;
    let lost = [Op::SetBits {
        at: gone,
        field: Field::AlphaFlags,
        mask: 1,
        value: 0,
        base: 1,
    }];
    assert_eq!(keep_mine(&b, &lost, 0).unwrap(), lost.to_vec());
}

#[test]
fn keep_mine_reads_the_current_texture_path_and_extra() {
    use nif::extra::ExtraValue::Str;
    let b = scenes::one_shape_set(&["textures\\a.dds", "textures\\a_n.dds"]);
    let m = read_model(&b).unwrap();
    let Op::SetTexturePath { at, slot, path, .. } = path_op(&m, 0, 1, "textures\\z.dds") else {
        unreachable!()
    };
    let stale = Op::SetTexturePath {
        at,
        slot,
        path,
        base: "textures\\old.dds".into(),
    };
    let kept = keep_mine(&b, &[stale], 0).unwrap();
    assert!(matches!(&kept[0], Op::SetTexturePath { base, .. } if base == "textures\\a_n.dds"));
    assert_eq!(apply(&b, &kept).unwrap().outcomes, vec![Outcome::Applied]);

    let b = scenes::root_extras();
    let m = read_model(&b).unwrap();
    let at = anchor_role(&m, None, root_extra("BODYTRI", "NiStringExtraData")).unwrap();
    let stale = Op::EditExtra {
        at,
        value: Str("new".into()),
        base: Str("other".into()),
    };
    let kept = keep_mine(&b, &[stale], 0).unwrap();
    assert!(matches!(&kept[0], Op::EditExtra { base: Str(s), .. } if s == "a\r\nb"));
    assert_eq!(apply(&b, &kept).unwrap().outcomes, vec![Outcome::Applied]);
}

#[test]
fn take_source_drops_the_op() {
    let at = Anchor {
        block: 0,
        kind: String::new(),
        shape: None,
        role: Role::Shape,
    };
    let ops = [
        gloss(&at, 1.0, 2.0),
        gloss(&at, 3.0, 4.0),
        gloss(&at, 5.0, 6.0),
    ];
    assert_eq!(take_source(&ops, 1), vec![ops[0].clone(), ops[2].clone()]);
}

#[test]
fn rebase_updates_reanchored_blocks() {
    let b = scenes::alpha_shape(0x12EC);
    let mut at = alpha_anchor(&b);
    let real = at.block;
    at.block = 99; // the block moved in the new source
    let op = Op::SetField {
        at,
        field: Field::AlphaThreshold,
        value: Value::U8(10),
        base: Value::U8(128),
    };
    let (ops, outcomes) = rebase(&b, &[op]).unwrap();
    assert_eq!(outcomes, vec![Outcome::ReAnchored { block: real }]);
    assert!(matches!(&ops[0], Op::SetField { at, .. } if at.block == real));
    assert_eq!(rebase(&b, &ops).unwrap().1, vec![Outcome::Applied]); // a second run is plain Applied
}

/// Ruling P8 (A17 review): an alpha field edit never lands on an alpha that several shapes share.
#[test]
fn an_alpha_edit_on_a_shared_alpha_is_a_conflict() {
    let b = scenes::shared_alpha_and_set();
    let m = read_model(&b).unwrap();
    let a = shape_named(&m, "A");
    let set = |at: Anchor| Op::SetBits {
        at,
        field: Field::AlphaFlags,
        mask: 1,
        value: 0,
        base: 0x00ED,
    };
    let shared = anchor_role(&m, Some(a), Role::Alpha).unwrap();
    let r = apply(&b, &[set(shared.clone())]).unwrap();
    assert_eq!(
        r.outcomes,
        vec![Outcome::Conflict(
            "This alpha is shared by 2 shapes: split it before editing.".into()
        )]
    );
    assert_eq!(r.bytes, b);
    // Split first: the edit goes to A's own copy, the block the split appends.
    let end = Header::parse(&b).unwrap().sizes.len() as u32;
    let own = Anchor {
        block: end,
        ..shared
    };
    let r = apply(
        &b,
        &[
            Op::SplitAlpha {
                at: anchor_shape(&m, a),
            },
            set(own),
        ],
    )
    .unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied, Outcome::Applied]);
}

fn ctrl_anchor(b: &[u8], owner: Owner, class: &str, var: Option<u32>, shape: &str) -> Anchor {
    let m = read_model(b).unwrap();
    let s = m.shapes.iter().find(|s| s.name.as_deref() == Some(shape));
    let role = Role::Ctrl {
        owner,
        class: class.into(),
        var,
        nth: 0,
    };
    anchor_role(&m, s.map(|s| s.block), role).unwrap()
}

fn interp_op(at: &Anchor, to: f32, base: f32) -> Op {
    Op::SetField {
        at: at.clone(),
        field: Field::InterpValue(0),
        value: Value::F32(to.to_bits()),
        base: Value::F32(base.to_bits()),
    }
}

const LSCF: &str = "BSLightingShaderPropertyFloatController";

#[test]
fn interp_value_through_a_ctrl_anchor_patches_the_interpolator_only() {
    let b = scenes::shader_ctrl(false);
    let at = ctrl_anchor(&b, Owner::Shader, LSCF, Some(11), "S");
    let r = apply(&b, &[interp_op(&at, 0.75, 0.5)]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    let h = Header::parse(&b).unwrap();
    let (rel, _) = locate(&h, &b, 3, Field::InterpValue(0)).unwrap();
    let off = h.offsets()[3] + rel;
    let ch = changed_ranges(&b, &r.bytes);
    assert!(
        !ch.is_empty() && ch.iter().all(|c| c.start >= off && c.end <= off + 4),
        "{ch:?}"
    );
    assert_eq!(read_model(&r.bytes).unwrap().interps[&3].value, vec![0.75]);
}

#[test]
fn interp_value_with_a_stale_base_conflicts_on_the_interpolator() {
    let b = scenes::shader_ctrl(false);
    let at = ctrl_anchor(&b, Owner::Shader, LSCF, Some(11), "S");
    let r = apply(&b, &[interp_op(&at, 0.75, 0.25)]).unwrap();
    assert!(
        matches!(&r.outcomes[0], Outcome::Conflict(s) if s.contains("was F32")),
        "{:?}",
        r.outcomes
    );
    assert_eq!(r.bytes, b);
}

#[test]
fn keep_mine_reads_the_interp_value_base_from_the_interpolator() {
    let b = scenes::shader_ctrl(false);
    let at = ctrl_anchor(&b, Owner::Shader, LSCF, Some(11), "S");
    let ops = [interp_op(&at, 0.75, 0.25)];
    let kept = keep_mine(&b, &ops, 0).unwrap();
    let Op::SetField { base, .. } = &kept[0] else {
        panic!()
    };
    assert_eq!(*base, Value::F32(0.5f32.to_bits()));
    assert_eq!(apply(&b, &kept).unwrap().outcomes, vec![Outcome::Applied]);
}

/// P9: an interpolator that two controllers reference is not edited through either.
#[test]
fn a_shared_interpolator_is_a_conflict() {
    let b = scenes::shader_ctrl(true);
    let at = ctrl_anchor(&b, Owner::Shader, LSCF, Some(11), "S");
    let r = apply(&b, &[interp_op(&at, 0.75, 0.5)]).unwrap();
    assert_eq!(
        r.outcomes,
        vec![Outcome::Conflict(
            "This interpolator is shared by 2 controllers: editing it would change all of them."
                .into()
        )]
    );
    assert_eq!(r.bytes, b);
}

/// P8 extended: a controller field edit whose alpha has become shared needs the split first.
#[test]
fn a_controller_edit_on_a_shared_alpha_is_a_conflict() {
    let b = scenes::shared_alpha_with_testref_chain();
    let class = "BSNiAlphaPropertyTestRefController";
    let at = ctrl_anchor(&b, Owner::Alpha, class, None, "A");
    let op = Op::SetField {
        at,
        field: Field::Frequency,
        value: Value::F32(2.0f32.to_bits()),
        base: Value::F32(1.0f32.to_bits()),
    };
    let r = apply(&b, &[op]).unwrap();
    assert_eq!(
        r.outcomes,
        vec![Outcome::Conflict(
            "This alpha is shared by 2 shapes: split it before editing.".into()
        )]
    );
    assert_eq!(r.bytes, b);
}

// ---- v2 B4: SetKeys ----

use nif::keys::{encode_data, Key, KeyData, KeyGroup};
use scenes::Share;

fn data_of(b: &[u8], d: usize) -> KeyData {
    nif::keys::read_data(&Header::parse(b).unwrap(), b, d).unwrap()
}

fn key(time: f32, value: f32) -> Key<f32> {
    Key {
        time,
        value,
        forward: None,
        backward: None,
        tbc: None,
    }
}

/// Float LINEAR keys.
fn lin(k: &[(f32, f32)]) -> KeyData {
    KeyData::Float(KeyGroup {
        key_type: Some(1),
        keys: k.iter().map(|&(t, v)| key(t, v)).collect(),
    })
}

fn empty_float() -> KeyData {
    KeyData::Float(KeyGroup {
        key_type: None,
        keys: vec![],
    })
}

fn set_keys(at: &Anchor, keys: KeyData, base: KeyData) -> Op {
    Op::SetKeys {
        at: at.clone(),
        clip: None,
        keys,
        base,
    }
}

fn nblocks(b: &[u8]) -> usize {
    Header::parse(b).unwrap().sizes.len()
}

fn kind_of(b: &[u8], k: usize) -> Vec<u8> {
    Header::parse(b).unwrap().kind(k).to_vec()
}

fn with_i32(mut v: Vec<u8>, at: usize, x: i32) -> Vec<u8> {
    v[at..at + 4].copy_from_slice(&x.to_le_bytes());
    v
}

fn same_blocks(a: &[u8], b: &[u8], blocks: impl IntoIterator<Item = usize>) {
    for k in blocks {
        assert_eq!(block_bytes_of(a, k), block_bytes_of(b, k), "block {k}");
    }
}

#[test]
fn set_keys_reemits_only_the_data_block() {
    let b = scenes::keyed_pair(Share::No, [1, 2]);
    let at = ctrl_anchor(&b, Owner::Shader, LSCF, Some(11), "S");
    let keys = lin(&[(0.0, 0.0), (0.5, 2.0), (1.0, 1.0)]);
    let r = apply(&b, &[set_keys(&at, keys.clone(), data_of(&b, 6))]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    assert_eq!(nblocks(&r.bytes), 8);
    assert_eq!(
        nif::write::block_diff(&b, &r.bytes).unwrap(),
        vec!["block 6 NiFloatData: 24 → 32 bytes".to_string()]
    );
    assert_eq!(block_bytes_of(&r.bytes, 6), encode_data(&keys));
    same_blocks(&b, &r.bytes, (0..8).filter(|&k| k != 6));
    assert_eq!(
        Header::parse(&b).unwrap().strings,
        Header::parse(&r.bytes).unwrap().strings
    );

    // same size: only the 4 value bytes of key 1 change
    let base = data_of(&b, 6);
    let mut same = base.clone();
    if let KeyData::Float(g) = &mut same {
        g.keys[1].value = 0.75;
    }
    let r = apply(&b, &[set_keys(&at, same, base)]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    let off = Header::parse(&b).unwrap().offsets()[6] + 20;
    // 1.0 -> 0.75 (0x3F80_0000 -> 0x3F40_0000) differs in one of the 4 bytes
    let ch = changed_ranges(&b, &r.bytes);
    assert!(
        !ch.is_empty() && ch.iter().all(|c| c.start >= off && c.end <= off + 4),
        "{ch:?}"
    );
    assert_eq!(r.bytes[off..off + 4], 0.75f32.to_le_bytes());
    same_blocks(&b, &r.bytes, [7]);
}

#[test]
fn shared_data_key_edit_copies() {
    // interps 4 and 5 share data 6
    let b = scenes::keyed_pair(Share::Data, [1, 1]);
    let at = ctrl_anchor(&b, Owner::Shader, LSCF, Some(11), "S");
    let keys = lin(&[(0.0, 0.0), (0.5, 2.0), (1.0, 1.0)]);
    let r = apply(&b, &[set_keys(&at, keys.clone(), data_of(&b, 6))]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    assert_eq!(nblocks(&r.bytes), 9);
    assert_eq!(kind_of(&r.bytes, 8), b"NiFloatData");
    assert_eq!(block_bytes_of(&r.bytes, 8), encode_data(&keys));
    assert_eq!(
        block_bytes_of(&r.bytes, 4),
        with_i32(block_bytes_of(&b, 4), 4, 8)
    );
    same_blocks(&b, &r.bytes, (0..8).filter(|&k| k != 4));
    assert_eq!(data_of(&r.bytes, 6), data_of(&b, 6));
    assert_eq!(read_model(&r.bytes).unwrap().interps[&5].data, 6);

    // ctrls 2 and 3 share interp 4 -> data 6
    let b = scenes::keyed_pair(Share::Interp, [1, 1]);
    let at = ctrl_anchor(&b, Owner::Shader, LSCF, Some(11), "S");
    let op = set_keys(&at, keys.clone(), data_of(&b, 6));
    let r = apply(&b, std::slice::from_ref(&op)).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    assert_eq!(nblocks(&r.bytes), 10);
    assert_eq!(kind_of(&r.bytes, 8), b"NiFloatInterpolator");
    assert_eq!(
        block_bytes_of(&r.bytes, 8),
        with_i32(block_bytes_of(&b, 4), 4, 9)
    );
    assert_eq!(kind_of(&r.bytes, 9), b"NiFloatData");
    assert_eq!(block_bytes_of(&r.bytes, 9), encode_data(&keys));
    assert_eq!(
        block_bytes_of(&r.bytes, 2),
        with_i32(block_bytes_of(&b, 2), 26, 8)
    );
    same_blocks(&b, &r.bytes, (0..8).filter(|&k| k != 2));
    let m = read_model(&r.bytes).unwrap();
    assert_eq!(m.ctrls[&2].interp, Some(8));
    assert_eq!(m.ctrls[&3].interp, Some(4));
    assert_eq!(m.interps[&4].data, 6);

    // the second edit finds the copy unshared and edits it in place
    let keys2 = lin(&[(0.0, 1.0), (1.0, 0.0)]);
    let op2 = set_keys(&at, keys2.clone(), keys);
    let r = apply(&b, &[op, op2]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied, Outcome::Applied]);
    assert_eq!(nblocks(&r.bytes), 10);
    assert_eq!(block_bytes_of(&r.bytes, 9), encode_data(&keys2));
}

#[test]
fn tbc_curve_is_preserved_when_untouched() {
    // ctrl 2's data 6 is TBC
    let b = scenes::keyed_pair(Share::No, [3, 1]);
    let at20 = ctrl_anchor(&b, Owner::Shader, LSCF, Some(20), "S");
    let op = set_keys(
        &at20,
        lin(&[(0.0, 0.0), (0.5, 2.0), (1.0, 1.0)]),
        data_of(&b, 7),
    );
    let r = apply(&b, std::slice::from_ref(&op)).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    same_blocks(&b, &r.bytes, [6]);
    let KeyData::Float(g) = data_of(&r.bytes, 6) else {
        panic!("not float")
    };
    assert_eq!(g.key_type, Some(3));
    assert!(g.keys.iter().all(|k| k.tbc == Some([0.5, -0.25, 1.0])));
    assert!(validate::validate_ops(&[op]).is_empty());

    // keep mine reads the TBC curve as the base and carries it
    let at11 = ctrl_anchor(&b, Owner::Shader, LSCF, Some(11), "S");
    let stale = set_keys(&at11, lin(&[(0.0, 3.0)]), lin(&[(0.0, 9.0)]));
    let kept = keep_mine(&b, &[stale], 0).unwrap();
    match &kept[0] {
        Op::SetKeys { base, .. } => assert_eq!(*base, data_of(&b, 6)),
        o => panic!("{o:?}"),
    }
    assert_eq!(apply(&b, &kept).unwrap().outcomes, vec![Outcome::Applied]);
}

#[test]
fn set_keys_appends_data_when_missing() {
    // interp 3 has data -1
    let b = scenes::shader_ctrl(false);
    let at = ctrl_anchor(&b, Owner::Shader, LSCF, Some(11), "S");
    let keys = lin(&[(0.0, 0.0), (1.0, 1.0)]);
    let r = apply(&b, &[set_keys(&at, keys.clone(), empty_float())]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    assert_eq!(nblocks(&r.bytes), 5);
    assert_eq!(kind_of(&r.bytes, 4), b"NiFloatData");
    assert_eq!(block_bytes_of(&r.bytes, 4), encode_data(&keys));
    let mut want = 0.5f32.to_le_bytes().to_vec();
    want.extend(4i32.to_le_bytes());
    assert_eq!(block_bytes_of(&r.bytes, 3), want);
    same_blocks(&b, &r.bytes, 0..3);

    let r = apply(&b, &[set_keys(&at, keys.clone(), lin(&[(0.0, 0.0)]))]).unwrap();
    assert_eq!(
        r.outcomes,
        vec![Outcome::Conflict(
            "changed in the source: the keys differ from the edit's base".into()
        )]
    );
    assert_eq!(r.bytes, b);
}

#[test]
fn set_keys_refusals() {
    let refused = |s: &str| vec![Outcome::Refused(s.into())];
    let b = scenes::keyed_pair(Share::No, [1, 2]);
    let at = ctrl_anchor(&b, Owner::Shader, LSCF, Some(11), "S");
    let clip = |at: &Anchor, c| Op::SetKeys {
        at: at.clone(),
        clip: Some(c),
        keys: lin(&[(0.0, 1.0)]),
        base: data_of(&b, 6),
    };
    let mut lost = at.clone();
    lost.block = 99;
    lost.shape = Some("nope".into());
    // v2 B7 (E9): clip edits are implemented; a missing sequence is Refused, a lost anchor a Conflict.
    let r = apply(&b, &[clip(&at, 0)]).unwrap();
    assert_eq!(r.outcomes, refused("This clip is not in this file."));
    assert_eq!(r.bytes, b);
    let r = apply(&b, &[clip(&lost, 0)]).unwrap();
    assert!(
        matches!(r.outcomes[0], Outcome::Conflict(_)),
        "{:?}",
        r.outcomes
    );
    assert_eq!(r.bytes, b);
    // A CB with controller -1 and null strings binds nothing.
    let ks = scenes::keyed_with_sequence(Some(4));
    let kat = ctrl_anchor(&ks, Owner::Shader, LSCF, Some(11), "S");
    let op = Op::SetKeys {
        at: kat,
        clip: Some(8),
        keys: lin(&[(0.0, 1.0)]),
        base: data_of(&ks, 6),
    };
    let r = apply(&ks, &[op]).unwrap();
    assert_eq!(
        r.outcomes,
        refused("This clip does not animate this controller.")
    );
    assert_eq!(r.bytes, ks);

    let pos = KeyData::Pos(KeyGroup {
        key_type: Some(1),
        keys: vec![Key {
            time: 0.0,
            value: [1.0, 2.0, 3.0],
            forward: None,
            backward: None,
            tbc: None,
        }],
    });
    let r = apply(&b, &[set_keys(&at, pos, data_of(&b, 6))]).unwrap();
    assert_eq!(
        r.outcomes,
        refused("The keys do not match the interpolator's type.")
    );
    assert_eq!(r.bytes, b);

    let mc = scenes::managed_clip();
    let m = read_model(&mc).unwrap();
    let role = Role::Ctrl {
        owner: Owner::Shader,
        class: LSCF.into(),
        var: Some(11),
        nth: 0,
    };
    let mat = anchor_role(&m, Some(m.shapes[0].block), role).unwrap();
    assert_eq!(mat.block, 7);
    let r = apply(&mc, &[set_keys(&mat, lin(&[(0.0, 1.0)]), empty_float())]).unwrap();
    assert_eq!(
        r.outcomes,
        refused("A NiBlendFloatInterpolator has no keys to edit.")
    );
    assert_eq!(r.bytes, mc);
    // v2 B7 (E4): A's two CBs naming different interpolators for one controller are Refused.
    let clip_a = |keys| Op::SetKeys {
        at: mat.clone(),
        clip: Some(2),
        keys,
        base: empty_float(),
    };
    let mut two = mc.clone();
    let seq_a = Header::parse(&mc).unwrap().offsets()[2];
    let cb1 = seq_a + 12 + 29;
    two[cb1..cb1 + 4].copy_from_slice(&11i32.to_le_bytes());
    let r = apply(&two, &[clip_a(lin(&[(0.0, 1.0)]))]).unwrap();
    assert_eq!(
        r.outcomes,
        refused(
            "This clip has several interpolators for this controller: editing them needs Phase C."
        )
    );
    assert_eq!(r.bytes, two);
    // Unpatched: both CBs name interpolator 10 (2 refs, 2 users: no copy); its data -1 gets a new NiFloatData.
    let r = apply(&mc, &[clip_a(lin(&[(0.0, 1.0)]))]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    let m2 = read_model(&r.bytes).unwrap();
    assert_eq!(
        (
            m2.sequences[0].blocks[0].interp,
            m2.sequences[0].blocks[1].interp
        ),
        (10, 10)
    );
    assert_eq!(nblocks(&r.bytes), 13);
    assert_eq!(m2.interps[&10].data, 12);
    assert_eq!(
        encode_data(&data_of(&r.bytes, 12)),
        encode_data(&lin(&[(0.0, 1.0)]))
    );

    let sa = scenes::shared_alpha_with_testref_chain();
    let ta = ctrl_anchor(
        &sa,
        Owner::Alpha,
        "BSNiAlphaPropertyTestRefController",
        None,
        "A",
    );
    let r = apply(&sa, &[set_keys(&ta, lin(&[(0.0, 1.0)]), empty_float())]).unwrap();
    assert_eq!(
        r.outcomes,
        vec![Outcome::Conflict(
            "This alpha is shared by 2 shapes: split it before editing.".into()
        )]
    );
    assert_eq!(r.bytes, sa);

    assert_eq!(
        merge_key(&set_keys(&at, empty_float(), empty_float())),
        "SetKeys|2|"
    );
    assert_eq!(merge_key(&clip(&at, 3)), "SetKeys|2|3");
}

/// v2 B6: keys_of is the base SetKeys checks: the data block's keys, the empty group for no data, an error for a
/// blend interpolator; v2 B7: a clip's CB interpolator keys, and an error for a missing clip.
#[test]
fn keys_of_is_the_set_keys_base() {
    let b = scenes::keyed_pair(Share::No, [1, 2]);
    let m = read_model(&b).unwrap();
    assert_eq!(nif::edit::keys_of(&b, &m, 2, None), Ok(data_of(&b, 6)));
    assert_eq!(nif::edit::keys_of(&b, &m, 3, None), Ok(data_of(&b, 7)));
    let c = scenes::shader_ctrl(false); // interpolator 3 has data -1
    assert_eq!(
        nif::edit::keys_of(&c, &read_model(&c).unwrap(), 2, None),
        Ok(empty_float())
    );
    let mc = scenes::managed_clip();
    let mm = read_model(&mc).unwrap();
    assert_eq!(
        nif::edit::keys_of(&mc, &mm, 7, None),
        Err("A NiBlendFloatInterpolator has no keys to edit.".into())
    );
    // v2 B7 (E9): clips are implemented.
    assert_eq!(
        nif::edit::keys_of(&b, &m, 2, Some(0)),
        Err("This clip is not in this file.".into())
    );
    let mp = scenes::managed_pair();
    let mpm = read_model(&mp).unwrap();
    assert_eq!(
        nif::edit::keys_of(&mp, &mpm, 5, Some(1)),
        Ok(data_of(&mp, 13))
    );
}

/// Review B4 minor 1: a sequence's controlled block counts as a user of the interpolator and its data.
#[test]
fn sequence_users_make_key_edits_copy() {
    let keys = lin(&[(0.0, 0.0), (0.5, 2.0), (1.0, 1.0)]);

    // controlled block 0 of sequence 8 shares ctrl 2's interpolator 4
    let b = scenes::keyed_with_sequence(Some(4));
    assert_eq!(read_model(&b).unwrap().sequences[0].blocks[0].interp, 4);
    let at = ctrl_anchor(&b, Owner::Shader, LSCF, Some(11), "S");
    let r = apply(&b, &[set_keys(&at, keys.clone(), data_of(&b, 6))]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    assert_eq!(nblocks(&r.bytes), 11);
    assert_eq!(kind_of(&r.bytes, 9), b"NiFloatInterpolator");
    assert_eq!(
        block_bytes_of(&r.bytes, 9),
        with_i32(block_bytes_of(&b, 4), 4, 10)
    );
    assert_eq!(block_bytes_of(&r.bytes, 10), encode_data(&keys));
    assert_eq!(
        block_bytes_of(&r.bytes, 2),
        with_i32(block_bytes_of(&b, 2), 26, 9)
    );
    same_blocks(&b, &r.bytes, (0..9).filter(|&k| k != 2));
    let m = read_model(&r.bytes).unwrap();
    assert_eq!(m.ctrls[&2].interp, Some(9));
    assert_eq!(m.sequences[0].blocks[0].interp, 4);
    assert_eq!(m.interps[&4].data, 6);

    // sequence-only interpolator 9 shares ctrl 2's data 6
    let b = scenes::keyed_with_sequence(None);
    assert_eq!(read_model(&b).unwrap().interps[&9].data, 6);
    let at = ctrl_anchor(&b, Owner::Shader, LSCF, Some(11), "S");
    let r = apply(&b, &[set_keys(&at, keys.clone(), data_of(&b, 6))]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    assert_eq!(nblocks(&r.bytes), 11);
    assert_eq!(kind_of(&r.bytes, 10), b"NiFloatData");
    assert_eq!(block_bytes_of(&r.bytes, 10), encode_data(&keys));
    assert_eq!(
        block_bytes_of(&r.bytes, 4),
        with_i32(block_bytes_of(&b, 4), 4, 10)
    );
    same_blocks(&b, &r.bytes, (0..10).filter(|&k| k != 4));
    let m = read_model(&r.bytes).unwrap();
    assert_eq!(m.sequences[0].blocks[0].interp, 9);
    assert_eq!(m.interps[&9].data, 6);
    assert_eq!(data_of(&r.bytes, 6), data_of(&b, 6));
}

// ---- B5: AddController / RemoveController (D9) ----

const LSCC: &str = "BSLightingShaderPropertyColorController";
const TESTREF: &str = "BSNiAlphaPropertyTestRefController";
/// Block-relative controller ref of a `lsp_body` BSLSP with no extra data: shader type, name, extra count.
const LSP_CTRL_AT: usize = 12;

fn add(at: Anchor, class: &str, var: Option<u32>, keys: KeyData, stop: f32) -> Op {
    Op::AddController {
        at,
        class: class.into(),
        var,
        keys,
        flags: 0x48,
        freq: 1.0,
        start: 0.0,
        stop,
    }
}

fn prop_anchor(b: &[u8], role: Role) -> Anchor {
    let m = read_model(b).unwrap();
    anchor_role(&m, Some(shape_named(&m, "S")), role).unwrap()
}

/// The root's first BSXFlags value.
fn bsx_of(b: &[u8]) -> u32 {
    let m = read_model(b).unwrap();
    m.extras_of(m.root_net.as_ref().unwrap())
        .into_iter()
        .find_map(|e| match e.value {
            Some(nif::extra::ExtraValue::Bsx(v)) if e.kind == "BSXFlags" => Some(v),
            _ => None,
        })
        .unwrap()
}

/// The 30- or 34-byte controller the add writes (next -1, phase 0, start 0, freq 1).
fn new_ctrl(flags: u16, stop: f32, target: i32, interp: i32, var: Option<u32>) -> Vec<u8> {
    let mut v = (-1i32).to_le_bytes().to_vec();
    v.extend(flags.to_le_bytes());
    for f in [1.0f32, 0.0, 0.0, stop] {
        v.extend(f.to_le_bytes());
    }
    v.extend(target.to_le_bytes());
    v.extend(interp.to_le_bytes());
    v.extend(var.map(u32::to_le_bytes).into_iter().flatten());
    v
}

fn with_flags(mut v: Vec<u8>, flags: u16) -> Vec<u8> {
    v[4..6].copy_from_slice(&flags.to_le_bytes());
    v
}

fn pos_keys(k: &[(f32, [f32; 3])]) -> KeyData {
    KeyData::Pos(KeyGroup {
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

fn lscf_role(var: u32) -> Role {
    Role::Ctrl {
        owner: Owner::Shader,
        class: LSCF.into(),
        var: Some(var),
        nth: 0,
    }
}

#[test]
fn add_float_controller_links_at_chain_end_and_sets_bsx() {
    let b = scenes::animated(Some(0x80), 2);
    let keys = lin(&[(0.0, 10.0), (2.0, 50.0)]);
    let op = add(
        prop_anchor(&b, Role::Shader),
        LSCF,
        Some(9),
        keys.clone(),
        2.0,
    );
    let r = apply(&b, &[op]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    assert_eq!(nblocks(&r.bytes), 13);
    assert_eq!(kind_of(&r.bytes, 10), b"NiFloatData");
    assert_eq!(kind_of(&r.bytes, 11), b"NiFloatInterpolator");
    assert_eq!(kind_of(&r.bytes, 12), LSCF.as_bytes());
    assert_eq!(block_bytes_of(&r.bytes, 10), encode_data(&keys));
    let mut interp = 10.0f32.to_le_bytes().to_vec();
    interp.extend(10i32.to_le_bytes());
    assert_eq!(block_bytes_of(&r.bytes, 11), interp);
    let c = new_ctrl(0x48, 2.0, 3, 11, Some(9));
    assert_eq!(c.len(), 34);
    assert_eq!(block_bytes_of(&r.bytes, 12), c);
    assert_eq!(
        block_bytes_of(&r.bytes, 5),
        with_i32(block_bytes_of(&b, 5), 0, 12)
    );
    assert_eq!(
        block_bytes_of(&r.bytes, 1),
        with_i32(block_bytes_of(&b, 1), 4, 0x81)
    );
    same_blocks(&b, &r.bytes, (0..10).filter(|&k| k != 1 && k != 5));
    let m = read_model(&r.bytes).unwrap();
    assert_eq!(m.lighting[&3].prop.ctrls, vec![4, 5, 12]);
    assert!(m.problems.is_empty(), "{:?}", m.problems);
    assert!(!m.managed(12));
    assert_eq!(anchor_role(&m, Some(2), lscf_role(9)).unwrap().block, 12);

    // empty chain: the property's own controller ref; bit 0 already set leaves the BSX block alone
    let b = scenes::animated(Some(1), 0);
    let op = add(
        prop_anchor(&b, Role::Shader),
        LSCF,
        Some(9),
        keys.clone(),
        2.0,
    );
    let r = apply(&b, &[op]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    assert_eq!(nblocks(&r.bytes), 7);
    assert_eq!(
        block_bytes_of(&r.bytes, 3),
        with_i32(block_bytes_of(&b, 3), LSP_CTRL_AT, 6)
    );
    same_blocks(&b, &r.bytes, [0, 1, 2]);
    assert_eq!(
        read_model(&r.bytes).unwrap().lighting[&3].prop.ctrls,
        vec![6]
    );

    // colour controller: Pos data and a Point3 interpolator
    let pos = pos_keys(&[(0.0, [1.0, 0.5, 0.25]), (1.0, [0.0, 0.0, 0.0])]);
    let op = add(
        prop_anchor(&b, Role::Shader),
        LSCC,
        Some(1),
        pos.clone(),
        1.0,
    );
    let r = apply(&b, &[op]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    assert_eq!(kind_of(&r.bytes, 4), b"NiPosData");
    assert_eq!(kind_of(&r.bytes, 5), b"NiPoint3Interpolator");
    assert_eq!(kind_of(&r.bytes, 6), LSCC.as_bytes());
    assert_eq!(block_bytes_of(&r.bytes, 4), encode_data(&pos));
    let mut interp: Vec<u8> = [1.0f32, 0.5, 0.25]
        .iter()
        .flat_map(|f| f.to_le_bytes())
        .collect();
    interp.extend(4i32.to_le_bytes());
    assert_eq!(block_bytes_of(&r.bytes, 5), interp);
    assert_eq!(
        block_bytes_of(&r.bytes, 6),
        new_ctrl(0x48, 1.0, 3, 5, Some(1))
    );
    let m = read_model(&r.bytes).unwrap();
    assert!(m.problems.is_empty(), "{:?}", m.problems);
    assert_eq!(m.ctrls[&6].var, Some(1));
}

#[test]
fn add_creates_bsx_when_missing() {
    use nif::extra::ExtraValue;
    let b = scenes::animated(None, 1);
    let (h0, m0) = (Header::parse(&b).unwrap(), read_model(&b).unwrap());
    let op = add(
        prop_anchor(&b, Role::Shader),
        LSCF,
        Some(11),
        lin(&[(0.0, 1.0)]),
        0.0,
    );
    let r = apply(&b, &[op]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    assert_eq!(nblocks(&r.bytes), 11);
    assert_eq!(kind_of(&r.bytes, 10), b"BSXFlags");
    // the existing "BSX" string (index 1) is reused
    let h1 = Header::parse(&r.bytes).unwrap();
    assert_eq!(h1.strings, h0.strings);
    let mut bsx = 1i32.to_le_bytes().to_vec();
    bsx.extend(1u32.to_le_bytes());
    assert_eq!(block_bytes_of(&r.bytes, 10), bsx);
    let net = m0.root_net.clone().unwrap();
    let old = block_bytes_of(&b, 0);
    let (s, e) = (net.start - h0.offsets()[0], net.end - h0.offsets()[0]);
    let mut want = old[..s].to_vec();
    want.extend(nif::net::encode_net(net.name, &[10], net.controller));
    want.extend(&old[e..]);
    assert_eq!(block_bytes_of(&r.bytes, 0), want);
    same_blocks(&b, &r.bytes, 1..4);
    let m = read_model(&r.bytes).unwrap();
    let x: Vec<_> = m
        .extras_of(m.root_net.as_ref().unwrap())
        .into_iter()
        .filter(|e| e.kind == "BSXFlags")
        .collect();
    assert_eq!(x.len(), 1);
    assert_eq!(x[0].name.as_deref(), Some(&b"BSX"[..]));
    assert_eq!(x[0].value, Some(ExtraValue::Bsx(1)));

    // a BSTriShape root has no extra list to hold BSXFlags
    let b = scenes::shader_ctrl(false);
    let op = add(
        prop_anchor(&b, Role::Shader),
        LSCF,
        Some(9),
        lin(&[(0.0, 1.0)]),
        0.0,
    );
    let r = apply(&b, &[op]).unwrap();
    assert_eq!(
        r.outcomes,
        vec![Outcome::Refused(
            "The root is not a node: the BSX Animated bit cannot be set.".into()
        )]
    );
    assert_eq!(r.bytes, b);
}

#[test]
fn remove_middle_controller_relinks_chain() {
    let b = scenes::animated(Some(1), 3);
    let at = ctrl_anchor(&b, Owner::Shader, LSCF, Some(20), "S");
    assert_eq!(at.block, 5);
    let r = apply(&b, &[Op::RemoveController { at }]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    assert_eq!(nblocks(&r.bytes), 13);
    assert_eq!(
        block_bytes_of(&r.bytes, 4),
        with_i32(block_bytes_of(&b, 4), 0, 6)
    );
    // only the Active bit goes; the removed controller keeps its next (6)
    assert_eq!(
        block_bytes_of(&r.bytes, 5),
        with_flags(block_bytes_of(&b, 5), 0x40)
    );
    same_blocks(&b, &r.bytes, (0..13).filter(|&k| k != 4 && k != 5));
    assert_eq!(
        read_model(&r.bytes).unwrap().lighting[&3].prop.ctrls,
        vec![4, 6]
    );

    // the first: the property's ref moves to the second
    let at = ctrl_anchor(&b, Owner::Shader, LSCF, Some(11), "S");
    let r = apply(&b, &[Op::RemoveController { at }]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    assert_eq!(
        block_bytes_of(&r.bytes, 3),
        with_i32(block_bytes_of(&b, 3), LSP_CTRL_AT, 5)
    );
    assert_eq!(
        block_bytes_of(&r.bytes, 4),
        with_flags(block_bytes_of(&b, 4), 0x40)
    );
    same_blocks(&b, &r.bytes, (0..13).filter(|&k| k != 3 && k != 4));

    let mc = scenes::managed_clip();
    let m = read_model(&mc).unwrap();
    let at = anchor_role(&m, Some(m.shapes[0].block), lscf_role(11)).unwrap();
    assert_eq!(at.block, 7);
    let r = apply(&mc, &[Op::RemoveController { at }]).unwrap();
    assert_eq!(
        r.outcomes,
        vec![Outcome::Refused(
            "A manager-controlled controller cannot be removed.".into()
        )]
    );
    assert_eq!(r.bytes, mc);
}

/// Review Focus 2: removal never clears BSX bit 0 and leaves the interpolator and data as orphans.
#[test]
fn remove_last_controller_keeps_bsx_and_clears_active() {
    let b = scenes::animated(Some(1), 1);
    let at = ctrl_anchor(&b, Owner::Shader, LSCF, Some(11), "S");
    let r = apply(&b, &[Op::RemoveController { at }]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    assert_eq!(
        block_bytes_of(&r.bytes, 3),
        with_i32(block_bytes_of(&b, 3), LSP_CTRL_AT, -1)
    );
    assert_eq!(
        block_bytes_of(&r.bytes, 4),
        with_flags(block_bytes_of(&b, 4), 0x40)
    );
    same_blocks(&b, &r.bytes, [0, 1, 2, 5, 6]);
    assert_eq!(bsx_of(&r.bytes), 1);
    assert!(read_model(&r.bytes).unwrap().lighting[&3]
        .prop
        .ctrls
        .is_empty());

    let b = scenes::animated(Some(1), 2);
    let at = ctrl_anchor(&b, Owner::Shader, LSCF, Some(20), "S");
    let r = apply(&b, &[Op::RemoveController { at }]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    assert_eq!(
        block_bytes_of(&r.bytes, 4),
        with_i32(block_bytes_of(&b, 4), 0, -1)
    );

    let b = scenes::animated(Some(0), 0);
    let rm = Op::RemoveController {
        at: Anchor {
            block: 6,
            kind: LSCF.into(),
            shape: Some("S".into()),
            role: lscf_role(11),
        },
    };
    let op = add(
        prop_anchor(&b, Role::Shader),
        LSCF,
        Some(11),
        lin(&[(0.0, 1.0)]),
        0.0,
    );
    let r = apply(&b, &[op, rm]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied, Outcome::Applied]);
    assert_eq!(bsx_of(&r.bytes), 1);
    assert!(read_model(&r.bytes).unwrap().lighting[&3]
        .prop
        .ctrls
        .is_empty());
}

#[test]
fn add_on_shared_alpha_splits_first() {
    let b = scenes::shared_alpha_with_testref_chain();
    let m = read_model(&b).unwrap();
    let a = shape_named(&m, "A");
    let at_alpha = anchor_role(&m, Some(a), Role::Alpha).unwrap();
    assert_eq!(at_alpha.block, 4);
    let keys = lin(&[(0.0, 0.0), (1.0, 255.0)]);
    let ops = [
        Op::SplitAlpha {
            at: anchor_shape(&m, a),
        },
        add(at_alpha.clone(), TESTREF, None, keys.clone(), 1.0),
    ];
    let r = apply(&b, &ops).unwrap();
    assert_eq!(
        r.outcomes,
        vec![Outcome::Applied, Outcome::ReAnchored { block: 8 }]
    );
    assert_eq!(nblocks(&r.bytes), 16);
    assert_eq!(kind_of(&r.bytes, 12), b"NiFloatData");
    assert_eq!(kind_of(&r.bytes, 13), b"NiFloatInterpolator");
    assert_eq!(kind_of(&r.bytes, 14), TESTREF.as_bytes());
    assert_eq!(kind_of(&r.bytes, 15), b"BSXFlags");
    let c = new_ctrl(0x48, 1.0, 8, 13, None);
    assert_eq!(c.len(), 30);
    assert_eq!(block_bytes_of(&r.bytes, 14), c);
    let m = read_model(&r.bytes).unwrap();
    assert_eq!(m.alphas[&8].prop.ctrls, vec![9, 14]);
    assert_eq!(m.alphas[&4].prop.ctrls, vec![5]);
    assert_eq!(m.ctrls[&5].next, -1);
    assert!(m.problems.is_empty(), "{:?}", m.problems);
    same_blocks(&b, &r.bytes, 2..8);
    assert_eq!(bsx_of(&r.bytes), 1);

    let r = apply(&b, &[add(at_alpha, TESTREF, None, keys, 1.0)]).unwrap();
    assert_eq!(
        r.outcomes,
        vec![Outcome::Conflict(
            "This alpha is shared by 2 shapes: split it before editing.".into()
        )]
    );
    assert_eq!(r.bytes, b);
}

#[test]
fn add_controller_refusals() {
    let b = scenes::animated(Some(1), 1);
    let shader = prop_anchor(&b, Role::Shader);
    let one = lin(&[(0.0, 1.0)]);
    let check = |b: &[u8], op: Op, text: &str| {
        let r = apply(b, &[op]).unwrap();
        assert_eq!(r.outcomes, vec![Outcome::Refused(text.into())]);
        assert_eq!(r.bytes, b);
    };
    check(
        &b,
        add(shader.clone(), "NiAlphaController", None, one.clone(), 0.0),
        "NiAlphaController cannot be added.",
    );
    check(
        &b,
        add(shader.clone(), LSCF, Some(3), one.clone(), 0.0),
        &format!("{LSCF} does not take variable Some(3)."),
    );
    check(
        &b,
        add(shader.clone(), LSCF, None, one.clone(), 0.0),
        &format!("{LSCF} does not take variable None."),
    );
    // alpha_shape's root is a BSTriShape: the class and role checks run before the root check
    let al = scenes::alpha_shape(0x12EC);
    check(
        &al,
        add(alpha_anchor(&al), TESTREF, Some(0), one.clone(), 0.0),
        &format!("{TESTREF} does not take variable Some(0)."),
    );
    check(
        &al,
        add(alpha_anchor(&al), LSCF, Some(9), one.clone(), 0.0),
        &format!("{LSCF} goes on a shader property."),
    );
    check(
        &b,
        add(shader.clone(), TESTREF, None, one.clone(), 0.0),
        &format!("{TESTREF} goes on an alpha property."),
    );
    check(
        &b,
        add(
            shader.clone(),
            LSCF,
            Some(9),
            pos_keys(&[(0.0, [1.0; 3])]),
            0.0,
        ),
        "The keys do not match the controller's type.",
    );
    check(
        &b,
        add(shader.clone(), LSCC, Some(0), one.clone(), 0.0),
        "The keys do not match the controller's type.",
    );
    check(
        &b,
        add(shader.clone(), LSCF, Some(9), lin(&[]), 0.0),
        "A new controller needs at least one key.",
    );
    // ctrl 4's next points at block 99: linking after it would orphan the unread tail
    let mut broken = b.clone();
    let at = Header::parse(&b).unwrap().offsets()[4];
    broken[at..at + 4].copy_from_slice(&99i32.to_le_bytes());
    check(
        &broken,
        add(shader.clone(), LSCF, Some(9), one.clone(), 0.0),
        "This controller chain is broken: it cannot be edited.",
    );

    assert_eq!(
        merge_key(&add(shader, LSCF, Some(9), one, 0.0)),
        "AddController|3|BSLightingShaderPropertyFloatController/Some(9)"
    );
    let ctrl5 = Anchor {
        block: 5,
        kind: LSCF.into(),
        shape: Some("S".into()),
        role: lscf_role(20),
    };
    assert_eq!(
        merge_key(&Op::RemoveController { at: ctrl5 }),
        "RemoveController|5|"
    );
}

/// B-P29: the keys an AddController authors get the same checks as SetKeys.
#[test]
fn authored_controller_keys_are_validated() {
    use nif::edit::validate::{validate_ops, Level, Target};
    let b = scenes::animated(Some(1), 0);
    let shader = prop_anchor(&b, Role::Shader);
    let w = validate_ops(&[add(
        shader.clone(),
        LSCF,
        Some(9),
        lin(&[(0.0, 1.0), (f32::NAN, 2.0), (-1.0, 0.0)]),
        0.0,
    )]);
    assert_eq!(w.len(), 1, "{w:?}");
    assert_eq!(w[0].level, Level::Error);
    assert_eq!(w[0].target, Target::Block { block: 3 });
    assert_eq!(w[0].text, "Key values must be finite numbers");
    let w = validate_ops(&[add(
        shader,
        LSCF,
        Some(9),
        lin(&[(1.0, 1.0), (0.0, 2.0)]),
        0.0,
    )]);
    assert_eq!(w.len(), 1, "{w:?}");
    assert_eq!(w[0].text, "Key times must not decrease");
}

// ---- B5 fix round 1 (B-P31) ----

const BROKEN: &str = "This controller chain is broken: it cannot be edited.";

/// `b` with block `k`'s Next Controller (offset 0) set to `next`.
fn with_next(b: &[u8], k: usize, next: i32) -> Vec<u8> {
    let mut v = b.to_vec();
    let at = Header::parse(b).unwrap().offsets()[k];
    v[at..at + 4].copy_from_slice(&next.to_le_bytes());
    v
}

fn refused_unchanged(b: &[u8], op: Op, text: &str) {
    let r = apply(b, &[op]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Refused(text.into())]);
    assert_eq!(r.bytes, b);
}

#[test]
fn remove_refuses_a_controller_on_two_properties() {
    let b = scenes::two_props_one_ctrl();
    let m = read_model(&b).unwrap();
    assert_eq!(m.lighting[&2].prop.ctrls, vec![4]);
    assert_eq!(m.lighting[&3].prop.ctrls, vec![4]);
    for shape in ["S", "T"] {
        let at = ctrl_anchor(&b, Owner::Shader, LSCF, Some(11), shape);
        assert_eq!(at.block, 4);
        refused_unchanged(
            &b,
            Op::RemoveController { at },
            "This controller is shared by several properties: it cannot be removed.",
        );
    }
}

#[test]
fn broken_chains_refuse_remove_and_add() {
    let one = lin(&[(0.0, 1.0)]);
    // self-loop: ctrl 4 -> 4
    let b = with_next(&scenes::animated(Some(1), 1), 4, 4);
    let at = ctrl_anchor(&b, Owner::Shader, LSCF, Some(11), "S");
    refused_unchanged(&b, Op::RemoveController { at }, BROKEN);
    let shader = prop_anchor(&b, Role::Shader);
    refused_unchanged(&b, add(shader, LSCF, Some(9), one.clone(), 0.0), BROKEN);

    // two-link loop: 4 -> 5 -> 4
    let b = with_next(&scenes::animated(Some(1), 2), 5, 4);
    for var in [11, 20] {
        let at = ctrl_anchor(&b, Owner::Shader, LSCF, Some(var), "S");
        refused_unchanged(&b, Op::RemoveController { at }, BROKEN);
    }
    let shader = prop_anchor(&b, Role::Shader);
    refused_unchanged(&b, add(shader, LSCF, Some(9), one.clone(), 0.0), BROKEN);

    // next out of range: 4 -> 99
    let b = with_next(&scenes::animated(Some(1), 1), 4, 99);
    let at = ctrl_anchor(&b, Owner::Shader, LSCF, Some(11), "S");
    refused_unchanged(&b, Op::RemoveController { at }, BROKEN);
}

// ---- v2 B7: clip key edits and the Controller ID rewrite (R8 lifted) ----

/// SetKeys on clip A edits only A's CB interpolator data for that controller; B, the other controller, the blend
/// interpolators and every other block keep their bytes. A shared CB interpolator is copied for A.
#[test]
fn clip_key_edit_changes_only_that_clips_data() {
    let b = scenes::managed_pair();
    let at = ctrl_anchor(&b, Owner::Shader, LSCF, Some(11), "S"); // ctrl 5
    assert_eq!(at.block, 5);
    let keys = lin(&[(0.0, 0.0), (1.0, 5.0)]);
    let op = Op::SetKeys {
        at: at.clone(),
        clip: Some(1),
        keys: keys.clone(),
        base: data_of(&b, 13),
    };
    let r = apply(&b, std::slice::from_ref(&op)).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    let h = Header::parse(&b).unwrap();
    same_blocks(&b, &r.bytes, (0..h.sizes.len()).filter(|&i| i != 13));
    assert_eq!(
        encode_data(&data_of(&r.bytes, 13)),
        encode_data(&keys),
        "A's var-11 data, re-emitted in place"
    );
    let m = read_model(&r.bytes).unwrap();
    let a = nif::anim::clip_tracks(&m, &r.bytes, 1, 4);
    let b_ = nif::anim::clip_tracks(&m, &r.bytes, 2, 4);
    assert_eq!(
        a.iter().find(|t| t.ctrl == 5).unwrap().keys,
        Some(keys.clone())
    );
    assert_eq!(
        b_.iter().find(|t| t.ctrl == 5).unwrap().keys,
        Some(data_of(&b, 15)),
        "clip B unchanged"
    );
    // Stale base: Conflict; Keep mine takes the current keys; then it applies.
    let stale = Op::SetKeys {
        at: at.clone(),
        clip: Some(1),
        keys: keys.clone(),
        base: lin(&[(0.0, 9.0)]),
    };
    assert!(matches!(
        apply(&b, std::slice::from_ref(&stale)).unwrap().outcomes[0],
        Outcome::Conflict(_)
    ));
    let fixed = nif::edit::keep_mine(&b, &[stale], 0).unwrap();
    assert_eq!(apply(&b, &fixed).unwrap().outcomes, vec![Outcome::Applied]);
    // B's var-11 CB shares A's interpolator 9: A's edit copies it (E5).
    let mut s = b.clone();
    let seq_b = h.offsets()[2];
    s[seq_b + 12..seq_b + 16].copy_from_slice(&9i32.to_le_bytes());
    let r = apply(&s, &[op]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    let m = read_model(&r.bytes).unwrap();
    let (sa, sb) = (&m.sequences[0], &m.sequences[1]);
    assert_eq!(sa.blocks[0].interp, 18, "A's CB names the copy");
    assert_eq!(sb.blocks[0].interp, 9, "B still names the original");
    assert_eq!(m.interps[&18].data, 19);
    assert_eq!(encode_data(&data_of(&r.bytes, 19)), encode_data(&keys));
    assert_eq!(
        data_of(&r.bytes, 13),
        data_of(&b, 13),
        "the original data is untouched"
    );
}

/// R8 lifted: Variable 11 -> 12 on managed ctrl 5 rewrites the ID of A's by-ref CB and of B's string-bound CB to
/// one appended "12"; ctrl 6's CBs and B's CB on node "T" keep theirs. Only 4 bytes per CB and the controller's
/// variable change, plus the header's string table.
#[test]
fn variable_change_rewrites_cb_ids_by_ref_and_by_string() {
    let mut b = scenes::managed_pair();
    let h = Header::parse(&b).unwrap();
    let seq_b = h.offsets()[2];
    b[seq_b + 16..seq_b + 20].copy_from_slice(&(-1i32).to_le_bytes()); // B's CB 0: bound by strings only
    let strings = h.strings.len();
    let at = ctrl_anchor(&b, Owner::Shader, LSCF, Some(11), "S");
    let op = Op::SetField {
        at,
        field: Field::Variable,
        value: Value::U32(12),
        base: Value::U32(11),
    };
    let r = apply(&b, &[op]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    let m = read_model(&r.bytes).unwrap();
    assert_eq!(m.ctrls[&5].var, Some(12));
    let id = |s: usize, k: usize| m.sequences[s].blocks[k].ctrl_id.clone();
    assert_eq!(id(0, 0), Some(b"12".to_vec()), "A, by ref");
    assert_eq!(id(1, 0), Some(b"12".to_vec()), "B, by strings");
    assert_eq!(
        (id(0, 1), id(1, 1)),
        (Some(b"20".to_vec()), Some(b"20".to_vec())),
        "ctrl 6's CBs"
    );
    assert_eq!(id(1, 2), Some(b"11".to_vec()), "node T binds nothing");
    let h2 = Header::parse(&r.bytes).unwrap();
    assert_eq!(
        h2.strings.len(),
        strings + 1,
        "one string appended, shared by both CBs"
    );
    // Locality: the controller's variable and each rewritten CB's Controller ID index.
    let n = h.sizes.len();
    same_blocks(&b, &r.bytes, (0..n).filter(|i| ![1, 2, 5].contains(i)));
    let id_at = 12 + 21;
    for (blk, allowed) in [(1, id_at..id_at + 4), (2, id_at..id_at + 4), (5, 30..34)] {
        for c in changed_ranges(&block_bytes_of(&b, blk), &block_bytes_of(&r.bytes, blk)) {
            assert!(
                allowed.start <= c.start && c.end <= allowed.end,
                "block {blk}: {c:?}"
            );
        }
    }
    // The preview still binds both CBs to ctrl 5 with the new ID.
    assert!(nif::anim::clip_tracks(&m, &r.bytes, 2, 4)
        .iter()
        .any(|t| t.ctrl == 5));
}

/// E2/E3 on managed_clip (all three CBs name ctrl 7 by ref; IDs "11", null, ""): 11 -> 20 rewrites only "11";
/// null stays -1 and "" keeps its own index. 20 -> 11 then reuses the existing "11" entry.
#[test]
fn null_and_empty_cb_ids_stay_distinct() {
    let b = scenes::managed_clip();
    let h = Header::parse(&b).unwrap();
    let raw_id = |bytes: &[u8], seq: usize, k: usize| {
        let o = Header::parse(bytes).unwrap().offsets()[seq] + 12 + k * 29 + 21;
        i32::from_le_bytes(bytes[o..o + 4].try_into().unwrap())
    };
    let empty_idx = raw_id(&b, 3, 0);
    // managed_clip's BSTriShape is unnamed: anchor as set_keys_refusals does.
    let anchor = |bytes: &[u8], var: u32| {
        let m = read_model(bytes).unwrap();
        let role = Role::Ctrl {
            owner: Owner::Shader,
            class: LSCF.into(),
            var: Some(var),
            nth: 0,
        };
        anchor_role(&m, Some(m.shapes[0].block), role).unwrap()
    };
    let at = anchor(&b, 11);
    let set = |at: &Anchor, v, o| Op::SetField {
        at: at.clone(),
        field: Field::Variable,
        value: Value::U32(v),
        base: Value::U32(o),
    };
    let r = apply(&b, &[set(&at, 20, 11)]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    let m = read_model(&r.bytes).unwrap();
    assert_eq!(m.sequences[0].blocks[0].ctrl_id, Some(b"20".to_vec()));
    assert_eq!(m.sequences[0].blocks[1].ctrl_id, None, "null stays null");
    assert_eq!(raw_id(&r.bytes, 2, 1), -1);
    assert_eq!(
        m.sequences[1].blocks[0].ctrl_id,
        Some(vec![]),
        "empty stays empty"
    );
    assert_eq!(raw_id(&r.bytes, 3, 0), empty_idx, "with its own index");
    let n = Header::parse(&r.bytes).unwrap().strings.len();
    assert_eq!(n, h.strings.len() + 1);
    // Back: "11" exists, so no new entry; A's CB 0 names the original "11" index again.
    let at2 = anchor(&r.bytes, 20);
    let back = apply(&r.bytes, &[set(&at2, 11, 20)]).unwrap();
    assert_eq!(back.outcomes, vec![Outcome::Applied]);
    assert_eq!(Header::parse(&back.bytes).unwrap().strings.len(), n);
    assert_eq!(raw_id(&back.bytes, 2, 0), raw_id(&b, 2, 0));
}

/// B-P39 (B9 I11-P08): env map scale on a PBR material crashed the game; the add is refused. Not PBR, it applies.
#[test]
fn add_env_map_scale_is_refused_on_pbr() {
    let b = scenes::animated(Some(0x80), 2);
    let at = prop_anchor(&b, Role::Shader);
    let Value::U32(slsf2) = current(&b, 3, Field::Slsf2) else {
        panic!("slsf2")
    };
    let pbr = Op::SetField {
        at: at.clone(),
        field: Field::Slsf2,
        value: Value::U32(slsf2 | 1 << 23),
        base: Value::U32(slsf2),
    };
    let keys = lin(&[(0.0, 1.0), (1.0, 2.0)]);
    let r = apply(
        &b,
        &[pbr, add(at.clone(), LSCF, Some(8), keys.clone(), 1.0)],
    )
    .unwrap();
    assert_eq!(
        r.outcomes[1],
        Outcome::Refused("Env map scale crashes the game on PBR materials.".into())
    );
    let r = apply(&b, &[add(at, LSCF, Some(8), keys, 1.0)]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
}

// ---- v2 C2: AddSequence, RemoveSequence, RenameSequence ----

use nif::clips::{read_mttc, read_palette, read_sequence_out, read_text_keys};
use nif::ctrl::read_manager;

fn root_anchor(b: &[u8]) -> Anchor {
    anchor_role(&read_model(b).unwrap(), None, Role::Root).unwrap()
}

/// A track on `animated`'s LSCF of variable `var` (shape "S", block 2).
fn track(b: &[u8], var: u32, keys: KeyData) -> ClipTrack {
    let at = anchor_role(&read_model(b).unwrap(), Some(2), lscf_role(var)).unwrap();
    ClipTrack { at, keys }
}

fn add_seq(at: Anchor, name: &str, tracks: Vec<ClipTrack>) -> Op {
    Op::AddSequence {
        at,
        name: name.into(),
        cycle: 2,
        freq: 1.0,
        start: 0.0,
        stop: 1.0,
        tracks,
    }
}

fn remove_seq(at: Anchor, name: &str, nth: u32) -> Op {
    Op::RemoveSequence {
        at,
        name: name.into(),
        nth,
    }
}

fn rename_seq(at: Anchor, name: &str, nth: u32, new_name: &str) -> Op {
    Op::RenameSequence {
        at,
        name: name.into(),
        nth,
        new_name: new_name.into(),
    }
}

fn open_keys() -> KeyData {
    lin(&[(0.0, 0.0), (1.0, 1.0)])
}

fn root_ctrl(b: &[u8]) -> usize {
    read_model(b).unwrap().root_net.unwrap().controller as usize
}

fn count_kind(b: &[u8], kind: &str) -> usize {
    let h = Header::parse(b).unwrap();
    (0..h.sizes.len())
        .filter(|&i| h.kind(i) == kind.as_bytes())
        .count()
}

/// The names of the root manager's sequences, in list order.
fn clip_names(b: &[u8]) -> Vec<String> {
    let h = Header::parse(b).unwrap();
    let man = read_manager(&h, b, root_ctrl(b)).unwrap();
    man.sequences
        .iter()
        .map(|&s| {
            let n = read_sequence_out(&h, b, s as usize).unwrap().name;
            String::from_utf8_lossy(h.string(n).unwrap()).into_owned()
        })
        .collect()
}

/// `managed_clip()` with its BSTriShape (5) named "shape" (string 3), as the palette and CBs name it.
fn named_managed_clip() -> Vec<u8> {
    let mut b = scenes::managed_clip();
    let at = Header::parse(&b).unwrap().offsets()[5];
    b[at..at + 4].copy_from_slice(&3i32.to_le_bytes());
    b
}

fn refused(b: &[u8], op: Op) -> String {
    let r = apply(b, &[op]).unwrap();
    assert_eq!(r.bytes, b, "{:?}", r.outcomes);
    match &r.outcomes[0] {
        Outcome::Refused(s) => s.clone(),
        o => panic!("{o:?}"),
    }
}

#[test]
fn add_first_sequence_creates_manager_palette_and_mttc() {
    use nif::clips::{MANAGER_FLAGS, MTTC_FLAGS};
    let b = scenes::animated(Some(1), 2);
    let tracks = vec![track(&b, 11, open_keys()), track(&b, 20, open_keys())];
    let r = apply(&b, &[add_seq(root_anchor(&b), "Open", tracks)]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    let o = &r.bytes;
    let h = Header::parse(o).unwrap();
    let mgr = root_ctrl(o);
    assert_eq!(h.kind(mgr), b"NiControllerManager");
    let man = read_manager(&h, o, mgr).unwrap();
    assert_eq!(
        (man.ctrl.flags, man.ctrl.target, man.cumulative),
        (MANAGER_FLAGS, 0, 0)
    );
    let (mttc, targets) = read_mttc(&h, o, man.ctrl.next as usize).unwrap();
    assert_eq!((mttc.next, mttc.flags, mttc.target), (-1, MTTC_FLAGS, 0));
    // C-P29: vanilla managers and MTTCs (1051 of 1051) run freq 1, phase 0, start FLT_MAX, stop -FLT_MAX
    for c in [&man.ctrl, &mttc] {
        assert_eq!(
            (c.frequency, c.phase, c.start, c.stop),
            (1.0, 0.0, f32::MAX, f32::MIN)
        );
    }
    assert_eq!(targets, [2], "the shape the clip binds (C-P26)");
    assert_eq!(man.sequences.len(), 1);
    let pal = read_palette(&h, o, man.palette as usize).unwrap();
    assert_eq!(pal, (0, vec![(b"S".to_vec(), 2)]));
    let seq = man.sequences[0] as usize;
    let s = read_sequence_out(&h, o, seq).unwrap();
    let st = |i: i32| h.string(i).map(<[u8]>::to_vec);
    assert_eq!(st(s.name), Some(b"Open".to_vec()));
    let root_name = read_model(&b).unwrap().root_net.unwrap().name;
    assert_eq!(
        (s.cycle, s.freq, s.start, s.stop, s.manager, s.accum_root),
        (2, 1.0, 0.0, 1.0, mgr as i32, root_name)
    );
    assert_eq!((s.weight, s.array_grow_by, s.notes.len()), (1.0, 1, 0));
    let (_, tk) = read_text_keys(&h, o, s.text_keys as usize).unwrap();
    let tk: Vec<_> = tk.iter().map(|&(t, v)| (t, st(v).unwrap())).collect();
    assert_eq!(tk, [(0.0, b"start".to_vec()), (1.0, b"end".to_vec())]);
    let m = read_model(o).unwrap();
    assert!(m.problems.is_empty(), "{:?}", m.problems);
    assert_eq!(s.blocks.len(), 2);
    for (cb, (ctrl, id)) in s.blocks.iter().zip([(4, "11"), (5, "20")]) {
        assert_eq!(cb.controller, ctrl);
        assert_eq!(st(cb.node), Some(b"S".to_vec()));
        assert_eq!(st(cb.property), Some(b"BSLightingShaderProperty".to_vec()));
        assert_eq!(st(cb.ctrl_type), Some(LSCF.as_bytes().to_vec()));
        assert_eq!(st(cb.ctrl_id), Some(id.as_bytes().to_vec()));
        assert_eq!((cb.interp_id, cb.priority), (-1, 0));
        assert_eq!(h.kind(cb.interp as usize), b"NiFloatInterpolator");
        assert!(cb.interp as usize >= 10, "a fresh interpolator");
        let d = m.interps[&(cb.interp as u32)].data as usize;
        assert!(d >= 10 && h.kind(d) == b"NiFloatData");
        assert_eq!(data_of(o, d), open_keys());
        let c = &m.ctrls[&(ctrl as u32)];
        assert_eq!(c.flags, 0x48 | 0x20);
        let bl = nif::ctrl::read_blend_interp(&h, o, c.interp.unwrap() as usize).unwrap();
        assert_eq!(bl.class, "NiBlendFloatInterpolator");
        assert_eq!((bl.flags, bl.array_size), (1, 2));
        assert_eq!(bl.value[0].to_bits(), 0xFF7F_FFFF);
    }
    assert_eq!(bsx_of(o) & 1, 1);
    same_blocks(&b, o, (1..10).filter(|k| ![4, 5].contains(k)));
    assert_eq!(
        changed_ranges(&block_bytes_of(&b, 0), &block_bytes_of(o, 0)).len(),
        1,
        "only the root's Controller ref"
    );
    assert!(m.managed(4) && m.managed(5));
    assert_eq!(m.sequences.len(), 1);
}

#[test]
fn add_second_sequence_reuses_manager() {
    let b = scenes::animated(Some(1), 2);
    let root = root_anchor(&b);
    let first = add_seq(
        root.clone(),
        "Open",
        vec![track(&b, 11, open_keys()), track(&b, 20, open_keys())],
    );
    let second = add_seq(root, "Close", vec![track(&b, 11, lin(&[(0.0, 1.0)]))]);
    let one = apply(&b, std::slice::from_ref(&first)).unwrap().bytes;
    let r = apply(&b, &[first, second]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied, Outcome::Applied]);
    let o = &r.bytes;
    assert_eq!(clip_names(o), ["Open", "Close"]);
    assert_eq!(count_kind(o, "NiControllerManager"), 1);
    assert_eq!(count_kind(o, "NiMultiTargetTransformController"), 1);
    assert_eq!(count_kind(o, "NiDefaultAVObjectPalette"), 1);
    let pal = |b: &[u8]| {
        let h = Header::parse(b).unwrap();
        let p = read_manager(&h, b, root_ctrl(b)).unwrap().palette;
        read_palette(&h, b, p as usize).unwrap()
    };
    assert_eq!(pal(o), pal(&one));
    let interp = |b: &[u8]| read_model(b).unwrap().ctrls[&4].interp;
    assert_eq!(interp(o), interp(&one), "the blend interpolator is kept");
}

#[test]
fn add_on_managed_clip_file_joins_its_manager() {
    let b = named_managed_clip();
    let m = read_model(&b).unwrap();
    let t = ClipTrack {
        at: anchor_role(&m, Some(5), lscf_role(11)).unwrap(),
        keys: open_keys(),
    };
    assert_eq!(t.at.block, 7);
    let r = apply(&b, &[add_seq(root_anchor(&b), "C", vec![t])]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    let o = &r.bytes;
    let h = Header::parse(o).unwrap();
    let man = read_manager(&h, o, 1).unwrap();
    assert_eq!(man.sequences[..2], [2, 3]);
    assert_eq!(man.sequences.len(), 3);
    assert_eq!(h.kind(man.sequences[2] as usize), b"NiControllerSequence");
    assert_eq!(man.ctrl.next, 9);
    assert_eq!(read_mttc(&h, o, 9).unwrap().0.next, -1);
    assert_eq!(count_kind(o, "NiControllerManager"), 1);
    assert_eq!(clip_names(o), ["A", "B", "C"]);
    assert_eq!(read_model(o).unwrap().ctrls[&7].interp, Some(8));
}

#[test]
fn add_refuses_bad_tracks() {
    let b = scenes::animated(Some(1), 2);
    let root = root_anchor(&b);
    let t11 = || track(&b, 11, open_keys());
    refused(&b, add_seq(root.clone(), "X", vec![]));
    refused(&b, add_seq(root.clone(), "X", vec![t11(), t11()]));
    let pos = pos_keys(&[(0.0, [0.0; 3])]);
    refused(&b, add_seq(root.clone(), "X", vec![track(&b, 11, pos)]));
    refused(
        &b,
        add_seq(root.clone(), "X", vec![track(&b, 11, lin(&[]))]),
    );
    refused(&b, add_seq(root.clone(), "", vec![t11()]));
    let mut nan = add_seq(root.clone(), "X", vec![t11()]);
    if let Op::AddSequence { stop, .. } = &mut nan {
        *stop = f32::NAN;
    }
    refused(&b, nan);

    let mc = named_managed_clip();
    let mm = read_model(&mc).unwrap();
    let t7 = || ClipTrack {
        at: anchor_role(&mm, Some(5), lscf_role(11)).unwrap(),
        keys: open_keys(),
    };
    refused(&mc, add_seq(root_anchor(&mc), "A", vec![t7()]));
    refused(&mc, add_seq(root_anchor(&mc), "a", vec![t7()]));
    // the shape the track drives has no name
    let un = scenes::managed_clip();
    let t = ClipTrack {
        at: anchor_role(&read_model(&un).unwrap(), Some(5), lscf_role(11)).unwrap(),
        keys: open_keys(),
    };
    assert_eq!(
        refused(&un, add_seq(root_anchor(&un), "C", vec![t])),
        "This shape has no name: a clip cannot find it."
    );

    let sa = scenes::shared_alpha_with_testref_chain();
    let role = Role::Ctrl {
        owner: Owner::Alpha,
        class: TESTREF.into(),
        var: None,
        nth: 0,
    };
    let t = ClipTrack {
        at: anchor_role(&read_model(&sa).unwrap(), Some(1), role).unwrap(),
        keys: open_keys(),
    };
    assert_eq!(
        refused(&sa, add_seq(root_anchor(&sa), "X", vec![t])),
        "This alpha is shared by 2 shapes: split it before editing."
    );

    let fc = scenes::free_chain();
    let t = ClipTrack {
        at: Anchor {
            block: 1,
            kind: LSCF.into(),
            shape: None,
            role: lscf_role(11),
        },
        keys: open_keys(),
    };
    assert_eq!(
        refused(&fc, add_seq(root_anchor(&fc), "X", vec![t])),
        "The root is not a node: a clip needs a node root."
    );
}

#[test]
fn testref_track_writes_a_null_controller_id() {
    let b = scenes::shared_alpha_with_testref_chain();
    let m = read_model(&b).unwrap();
    let role = Role::Ctrl {
        owner: Owner::Alpha,
        class: TESTREF.into(),
        var: None,
        nth: 0,
    };
    let t = ClipTrack {
        at: anchor_role(&m, Some(1), role).unwrap(),
        keys: open_keys(),
    };
    let split = Op::SplitAlpha {
        at: anchor_shape(&m, 1),
    };
    let r = apply(&b, &[split, add_seq(root_anchor(&b), "Fade", vec![t])]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied, Outcome::Applied]);
    let o = &r.bytes;
    let h = Header::parse(o).unwrap();
    let man = read_manager(&h, o, root_ctrl(o)).unwrap();
    let s = read_sequence_out(&h, o, man.sequences[0] as usize).unwrap();
    let cb = &s.blocks[0];
    assert_eq!(cb.ctrl_id, -1);
    assert_eq!(h.string(cb.property), Some(&b"NiAlphaProperty"[..]));
    assert_eq!(h.string(cb.node), Some(&b"A"[..]));
    let m2 = read_model(o).unwrap();
    let c = &m2.ctrls[&(cb.controller as u32)];
    assert!(c.block != 5 && c.class == TESTREF, "the split copy");
    assert_eq!(
        h.kind(c.interp.unwrap() as usize),
        b"NiBlendFloatInterpolator"
    );
    assert!(m2.problems.is_empty(), "{:?}", m2.problems);
}

#[test]
fn remove_sequence_count_follows_list() {
    let b = scenes::managed_clip();
    let r = apply(&b, &[remove_seq(root_anchor(&b), "A", 0)]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    let o = &r.bytes;
    let h = Header::parse(o).unwrap();
    assert_eq!(read_manager(&h, o, 1).unwrap().sequences, [3]);
    same_blocks(&b, o, (0..12).filter(|&k| k != 1));
    let m = read_model(o).unwrap();
    assert_eq!(m.sequences.iter().map(|s| s.block).collect::<Vec<_>>(), [3]);
}

#[test]
fn remove_sequence_keeps_shared_blocks_of_other_clips() {
    let b = scenes::animated(Some(1), 2);
    let root = root_anchor(&b);
    let close_keys = lin(&[(0.0, 1.0), (1.0, 0.5)]);
    let adds = [
        add_seq(root.clone(), "Open", vec![track(&b, 11, open_keys())]),
        add_seq(
            root.clone(),
            "Close",
            vec![track(&b, 11, close_keys.clone())],
        ),
    ];
    let two = apply(&b, &adds).unwrap().bytes;
    let r = apply(
        &b,
        &[
            adds[0].clone(),
            adds[1].clone(),
            remove_seq(root, "Open", 0),
        ],
    )
    .unwrap();
    assert_eq!(r.outcomes[2], Outcome::Applied);
    let o = &r.bytes;
    assert_eq!(clip_names(o), ["Close"]);
    let (m2, m) = (read_model(&two).unwrap(), read_model(o).unwrap());
    assert_eq!(m.ctrls[&4].flags, 0x68);
    assert_eq!(m.ctrls[&4].interp, m2.ctrls[&4].interp);
    let close = m.sequences[0].block;
    let ci = m.sequences[0].blocks[0].interp as usize;
    let cd = m.interps[&(ci as u32)].data as usize;
    same_blocks(&two, o, [ci, cd]);
    let t = nif::anim::clip_tracks(&m, o, close, 3);
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].keys, Some(close_keys));
}

#[test]
fn remove_last_clip_clears_0x20() {
    let b = scenes::animated(Some(1), 2);
    let root = root_anchor(&b);
    let add = add_seq(root.clone(), "Open", vec![track(&b, 11, open_keys())]);
    let one = apply(&b, std::slice::from_ref(&add)).unwrap().bytes;
    let open_interp = read_model(&one).unwrap().sequences[0].blocks[0].interp;
    let r = apply(&b, &[add, remove_seq(root, "Open", 0)]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied, Outcome::Applied]);
    let o = &r.bytes;
    let m = read_model(o).unwrap();
    assert_eq!(
        m.ctrls[&4].flags,
        0x48 | 0x04,
        "the clip's clamp cycle (C-P29)"
    );
    assert_eq!(m.ctrls[&4].interp, Some(open_interp));
    let h = Header::parse(o).unwrap();
    assert_eq!(h.kind(open_interp as usize), b"NiFloatInterpolator");
    let d = m.interps[&(open_interp as u32)].data as usize;
    assert_eq!(data_of(o, d), open_keys());
    assert!(!m.managed(4));
    assert!(m.sequences.is_empty());
    let free = nif::anim::free_tracks(&m, o, 3);
    assert!(free.iter().any(|t| t.ctrl == 4), "{free:?}");
    assert_eq!(h.kind(root_ctrl(o)), b"NiControllerManager");
    assert!(read_manager(&h, o, root_ctrl(o))
        .unwrap()
        .sequences
        .is_empty());
}

#[test]
fn remove_or_rename_a_missing_clip_is_a_conflict() {
    let no = Outcome::Conflict("No clip named Nope.".into());
    for b in [scenes::managed_clip(), scenes::animated(Some(1), 2)] {
        let root = root_anchor(&b);
        let r = apply(
            &b,
            &[
                remove_seq(root.clone(), "Nope", 0),
                rename_seq(root, "Nope", 0, "X"),
            ],
        )
        .unwrap();
        assert_eq!(r.outcomes, vec![no.clone(), no.clone()]);
        assert_eq!(r.bytes, b);
    }
    // "A" exists once: nth 1 is missing too
    let b = scenes::managed_clip();
    let r = apply(&b, &[remove_seq(root_anchor(&b), "A", 1)]).unwrap();
    assert_eq!(
        r.outcomes,
        vec![Outcome::Conflict("No clip named A.".into())]
    );
}

#[test]
fn rename_refuses_duplicates() {
    let b = scenes::managed_clip();
    let root = root_anchor(&b);
    refused(&b, rename_seq(root.clone(), "B", 0, "A"));
    refused(&b, rename_seq(root.clone(), "B", 0, "a"));
    refused(&b, rename_seq(root.clone(), "B", 0, ""));
    let r = apply(&b, &[rename_seq(root, "B", 0, "Idle")]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    let o = &r.bytes;
    for k in 0..12 {
        let (x, y) = (block_bytes_of(&b, k), block_bytes_of(o, k));
        if k == 3 {
            assert_eq!(x[4..], y[4..], "only the 4-byte Name");
            let idx = i32::from_le_bytes(y[..4].try_into().unwrap());
            assert_eq!(Header::parse(o).unwrap().string(idx), Some(&b"Idle"[..]));
        } else {
            assert_eq!(x, y, "block {k}");
        }
    }
    let (hb, ho) = (Header::parse(&b).unwrap(), Header::parse(o).unwrap());
    assert_eq!(ho.strings.len(), hb.strings.len() + 1);
    assert_eq!(clip_names(o), ["A", "Idle"]);
}

#[test]
fn nth_picks_the_second_clip_of_the_same_name() {
    let mut b = scenes::managed_clip();
    let at = Header::parse(&b).unwrap().offsets()[3];
    b[at..at + 4].copy_from_slice(&1i32.to_le_bytes()); // B's name -> "A" (string 1)
    assert_eq!(clip_names(&b), ["A", "A"]);
    let r = apply(&b, &[remove_seq(root_anchor(&b), "A", 1)]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    let h = Header::parse(&r.bytes).unwrap();
    assert_eq!(read_manager(&h, &r.bytes, 1).unwrap().sequences, [2]);
}

// ---- v2 C2 fix round 1 (C-P29) ----

/// The root manager re-emitted with `seqs` as its list (bypassing the editor).
fn with_list(b: &[u8], seqs: Vec<i32>) -> Vec<u8> {
    let h = Header::parse(b).unwrap();
    let mgr = root_ctrl(b);
    let mut man = read_manager(&h, b, mgr).unwrap();
    man.sequences = seqs;
    let mut doc = nif::write::NifDoc::open(b).unwrap();
    doc.set_block(mgr as u32, nif::clips::encode_manager(&man).unwrap());
    doc.bytes()
}

fn mttc_targets(b: &[u8]) -> Vec<i32> {
    let h = Header::parse(b).unwrap();
    let man = read_manager(&h, b, root_ctrl(b)).unwrap();
    read_mttc(&h, b, man.ctrl.next as usize).unwrap().1
}

/// I1: a later clip that binds another shape adds it to the MTTC's targets; an existing target is not repeated.
#[test]
fn mttc_targets_union_across_clips() {
    let b = scenes::two_animated();
    let m = read_model(&b).unwrap();
    let on = |shape: u32| ClipTrack {
        at: anchor_role(&m, Some(shape), lscf_role(11)).unwrap(),
        keys: open_keys(),
    };
    let root = root_anchor(&b);
    let ops = [
        add_seq(root.clone(), "A", vec![on(2)]),
        add_seq(root.clone(), "B", vec![on(3)]),
        add_seq(root, "C", vec![on(2), on(3)]),
    ];
    let r = apply(&b, &ops[..2]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied; 2]);
    assert_eq!(mttc_targets(&r.bytes), [2, 3]);
    let r = apply(&b, &ops).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied; 3]);
    assert_eq!(mttc_targets(&r.bytes), [2, 3]);
    assert_eq!(count_kind(&r.bytes, "NiMultiTargetTransformController"), 1);
}

/// Value of a track at scene time `t`, as the preview plays it.
fn play(tr: &nif::anim::Track, t: f32) -> f32 {
    let ct = nif::anim::ctrl_time(t, tr.freq, tr.phase, tr.start, tr.stop, tr.flags);
    match &tr.keys {
        Some(KeyData::Float(g)) => nif::anim::sample_f32(g, ct).unwrap(),
        k => panic!("{k:?}"),
    }
}

/// I2: a controller released by Remove takes the clip's timing with its keys, so they play as they did in the clip.
#[test]
fn released_controller_takes_the_clip_timing() {
    let b = scenes::animated(Some(1), 1); // controller 4 runs 0-1, loop
    let keys = lin(&[(10.0, 0.0), (20.0, 1.0)]);
    let root = root_anchor(&b);
    let add = Op::AddSequence {
        at: root.clone(),
        name: "Open".into(),
        cycle: 2,
        freq: 1.0,
        start: 10.0,
        stop: 20.0,
        tracks: vec![track(&b, 11, keys)],
    };
    let one = apply(&b, std::slice::from_ref(&add)).unwrap().bytes;
    let m1 = read_model(&one).unwrap();
    let clip = &nif::anim::clip_tracks(&m1, &one, m1.sequences[0].block, 3)[0];
    let r = apply(&b, &[add, remove_seq(root, "Open", 0)]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied; 2]);
    let m = read_model(&r.bytes).unwrap();
    let c = &m.ctrls[&4];
    assert_eq!(
        (c.frequency, c.phase, c.start, c.stop),
        (1.0, 0.0, 10.0, 20.0)
    );
    assert_eq!(c.flags, 0x48 | 0x04, "clamp from the clip's cycle type");
    let free = nif::anim::free_tracks(&m, &r.bytes, 3);
    let tr = free.iter().find(|t| t.ctrl == 4).unwrap();
    for t in [10.0, 15.0, 20.0, 25.0] {
        assert_eq!(play(tr, t), play(clip, t), "at {t}");
    }
    assert_eq!(play(tr, 15.0), 0.5);
}

/// I3: a sequence the manager lists that does not read refuses every clip op; it is not an absent name.
#[test]
fn unreadable_listed_sequence_refuses() {
    let b = with_list(&named_managed_clip(), vec![2, 3, 99]);
    let root = root_anchor(&b);
    let m = read_model(&b).unwrap();
    let t = ClipTrack {
        at: anchor_role(&m, Some(5), lscf_role(11)).unwrap(),
        keys: open_keys(),
    };
    refused(&b, remove_seq(root.clone(), "A", 0));
    refused(&b, remove_seq(root.clone(), "Nope", 0));
    refused(&b, rename_seq(root.clone(), "A", 0, "Z"));
    refused(&b, add_seq(root, "C", vec![t]));
}

/// I4: a sequence another manager still lists keeps its controllers manager-controlled after a Remove.
#[test]
fn another_manager_keeps_the_clip_live() {
    let b = scenes::animated(Some(1), 1);
    let root = root_anchor(&b);
    let one = apply(
        &b,
        &[add_seq(
            root.clone(),
            "Open",
            vec![track(&b, 11, open_keys())],
        )],
    )
    .unwrap()
    .bytes;
    let h = Header::parse(&one).unwrap();
    let man = read_manager(&h, &one, root_ctrl(&one)).unwrap();
    let mut doc = nif::write::NifDoc::open(&one).unwrap();
    let second = nif::ctrl::Manager {
        palette: -1,
        ..man.clone()
    };
    doc.append(
        b"NiControllerManager",
        nif::clips::encode_manager(&second).unwrap(),
    );
    let two = doc.bytes();
    let before = read_model(&two).unwrap().ctrls[&4].clone();
    let r = apply(&two, &[remove_seq(root, "Open", 0)]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    let m = read_model(&r.bytes).unwrap();
    assert_eq!(
        m.ctrls[&4], before,
        "flags 0x20 and the blend interpolator stay"
    );
    assert_eq!(m.sequences.len(), 1, "the other manager lists it");
    let h = Header::parse(&r.bytes).unwrap();
    assert!(read_manager(&h, &r.bytes, root_ctrl(&r.bytes))
        .unwrap()
        .sequences
        .is_empty());
}

/// Minor: an LSCC (Point3) track round trip: Point3 keyed and blend interpolators, all three values, and back.
#[test]
fn colour_track_round_trip() {
    let b = scenes::animated(Some(1), 0);
    let ctrl_keys = pos_keys(&[(0.0, [0.0; 3])]);
    let shader = prop_anchor(&b, Role::Shader);
    let a = apply(&b, &[add(shader, LSCC, Some(1), ctrl_keys, 1.0)])
        .unwrap()
        .bytes;
    let m = read_model(&a).unwrap();
    let role = Role::Ctrl {
        owner: Owner::Shader,
        class: LSCC.into(),
        var: Some(1),
        nth: 0,
    };
    let at = anchor_role(&m, Some(2), role).unwrap();
    let c = at.block;
    let keys = pos_keys(&[(0.0, [0.1, 0.2, 0.3]), (1.0, [0.4, 0.5, 0.6])]);
    let root = root_anchor(&a);
    let addc = add_seq(
        root.clone(),
        "Glow",
        vec![ClipTrack {
            at,
            keys: keys.clone(),
        }],
    );
    let one = apply(&a, std::slice::from_ref(&addc)).unwrap();
    assert_eq!(one.outcomes, vec![Outcome::Applied]);
    let o = &one.bytes;
    let (h, m) = (Header::parse(o).unwrap(), read_model(o).unwrap());
    let s = read_sequence_out(&h, o, m.sequences[0].block as usize).unwrap();
    let cb = &s.blocks[0];
    assert_eq!(h.string(cb.ctrl_id), Some(&b"1"[..]));
    assert_eq!(h.string(cb.ctrl_type), Some(LSCC.as_bytes()));
    assert_eq!(h.kind(cb.interp as usize), b"NiPoint3Interpolator");
    let ip = &m.interps[&(cb.interp as u32)];
    assert_eq!(ip.value, [0.1, 0.2, 0.3]);
    assert_eq!(h.kind(ip.data as usize), b"NiPosData");
    assert_eq!(data_of(o, ip.data as usize), keys);
    let bl = nif::ctrl::read_blend_interp(&h, o, m.ctrls[&c].interp.unwrap() as usize).unwrap();
    assert_eq!(bl.class, "NiBlendPoint3Interpolator");
    assert!(bl.value.iter().all(|v| v.to_bits() == 0xFF7F_FFFF) && bl.value.len() == 3);
    assert_eq!(m.ctrls[&c].flags, 0x68);

    let r = apply(&a, &[addc, remove_seq(root, "Glow", 0)]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied; 2]);
    let m = read_model(&r.bytes).unwrap();
    assert_eq!(m.ctrls[&c].interp, Some(cb.interp));
    assert_eq!(m.ctrls[&c].flags, 0x48 | 0x04);
    let t = nif::anim::free_tracks(&m, &r.bytes, 3);
    assert_eq!(t.iter().find(|t| t.ctrl == c).unwrap().keys, Some(keys));
}

// ---- v2 C3: ConvertChainToClips ----

const TIMINGS: &str = "These controllers run on different timings: they cannot share one clip.";
const NO_FREE: &str = "This property has no free controllers with keys.";

/// `b` with `v` written at body offset `off` of block `k`.
fn patched(b: &[u8], k: usize, off: usize, v: &[u8]) -> Vec<u8> {
    let mut o = b.to_vec();
    let at = Header::parse(b).unwrap().offsets()[k] + off;
    o[at..at + v.len()].copy_from_slice(v);
    o
}

/// `animated`'s shader anchor (shape "S" 2, BSLSP 3).
fn conv_at(b: &[u8]) -> Anchor {
    anchor_role(&read_model(b).unwrap(), Some(2), Role::Shader).unwrap()
}

fn conv(at: &Anchor, name: &str) -> Op {
    Op::ConvertChainToClips {
        at: at.clone(),
        name: name.into(),
    }
}

/// The root manager's only sequence block.
fn only_seq(b: &[u8]) -> usize {
    let h = Header::parse(b).unwrap();
    let man = read_manager(&h, b, root_ctrl(b)).unwrap();
    assert_eq!(man.sequences.len(), 1);
    man.sequences[0] as usize
}

#[test]
fn convert_moves_keys_into_one_clip_and_sets_0x20() {
    let b = scenes::animated(Some(1), 2);
    let m = read_model(&b).unwrap();
    let r = apply(&b, &[conv(&conv_at(&b), CONVERT_NAME)]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    let o = &r.bytes;
    let h = Header::parse(o).unwrap();
    let m2 = read_model(o).unwrap();
    assert!(m2.problems.is_empty(), "{:?}", m2.problems);
    assert_eq!(m2.sequences.len(), 1);
    let seq = only_seq(o);
    let s = read_sequence_out(&h, o, seq).unwrap();
    assert_eq!(h.string(s.name), Some(&b"SpecialIdle"[..]));
    assert_eq!((s.cycle, s.freq, s.start, s.stop), (0, 1.0, 0.0, 1.0));
    let ctrls: Vec<i32> = s.blocks.iter().map(|cb| cb.controller).collect();
    assert_eq!(ctrls, [4, 5], "chain order");
    for (cb, src) in s.blocks.iter().zip([8, 9]) {
        assert!(cb.interp >= 10, "a fresh interpolator");
        assert_eq!(h.kind(cb.interp as usize), b"NiFloatInterpolator");
        let d = m2.interps[&(cb.interp as u32)].data as usize;
        assert_eq!(data_of(o, d), data_of(&b, src));
    }
    for c in [4, 5] {
        assert_eq!(m2.ctrls[&c].flags, 0x48 | 0x20);
    }
    assert!(nif::anim::free_tracks(&m2, o, 3).is_empty());
    let clip: Vec<_> = nif::anim::clip_tracks(&m2, o, seq as u32, 3)
        .into_iter()
        .map(|t| t.keys)
        .collect();
    let free: Vec<_> = nif::anim::free_tracks(&m, &b, 3)
        .into_iter()
        .map(|t| t.keys)
        .collect();
    assert_eq!(clip.len(), 2);
    assert_eq!(clip, free);
    same_blocks(&b, o, [6, 7, 8, 9]);
}

#[test]
fn convert_equals_add_sequence() {
    let b = scenes::animated(Some(1), 2);
    let m = read_model(&b).unwrap();
    let ctrl = |var| anchor_role(&m, Some(2), lscf_role(var)).unwrap();
    let add = Op::AddSequence {
        at: anchor_role(&m, None, Role::Root).unwrap(),
        name: "X".into(),
        cycle: 0,
        freq: 1.0,
        start: 0.0,
        stop: 1.0,
        tracks: vec![
            ClipTrack {
                at: ctrl(11),
                keys: data_of(&b, 8),
            },
            ClipTrack {
                at: ctrl(20),
                keys: data_of(&b, 9),
            },
        ],
    };
    let a = apply(&b, &[add]).unwrap();
    let c = apply(&b, &[conv(&conv_at(&b), "X")]).unwrap();
    assert_eq!(a.outcomes, vec![Outcome::Applied]);
    assert_eq!(c.outcomes, vec![Outcome::Applied]);
    assert_eq!(c.bytes, a.bytes);
}

#[test]
fn convert_range_is_the_union() {
    let b = scenes::animated(Some(1), 2);
    let b = patched(&b, 5, 14, &0.5f32.to_le_bytes());
    let b = patched(&b, 5, 18, &3.0f32.to_le_bytes());
    let r = apply(&b, &[conv(&conv_at(&b), "X")]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    let o = &r.bytes;
    let h = Header::parse(o).unwrap();
    let s = read_sequence_out(&h, o, only_seq(o)).unwrap();
    assert_eq!((s.start, s.stop), (0.0, 3.0));
    let m2 = read_model(o).unwrap();
    for (cb, src) in s.blocks.iter().zip([8, 9]) {
        let d = m2.interps[&(cb.interp as u32)].data as usize;
        assert_eq!(data_of(o, d), data_of(&b, src), "keys are not rescaled");
    }
}

/// `animated(Some(1), 2)` variants whose timings cannot share one clip.
fn mixed_timings() -> Vec<Vec<u8>> {
    let b = scenes::animated(Some(1), 2);
    vec![
        patched(&b, 5, 4, &(0x48u16 | 0x04).to_le_bytes()),
        patched(&b, 5, 6, &2.0f32.to_le_bytes()),
        patched(&b, 5, 10, &0.5f32.to_le_bytes()),
        patched(&b, 5, 4, &(0x48u16 | 0x10).to_le_bytes()),
        patched(&b, 4, 10, &f32::NAN.to_le_bytes()),
    ]
}

#[test]
fn convert_refuses_mixed_timings() {
    for b in mixed_timings() {
        refused_unchanged(&b, conv(&conv_at(&b), "X"), TIMINGS);
    }
    let b = patched(
        &scenes::animated(Some(1), 2),
        4,
        10,
        &(-0.0f32).to_le_bytes(),
    );
    let r = apply(&b, &[conv(&conv_at(&b), "X")]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied], "-0.0 is phase 0");
}

#[test]
fn convert_joins_an_existing_manager() {
    let b = named_managed_clip();
    let m = read_model(&b).unwrap();
    let at = anchor_role(&m, Some(5), Role::Shader).unwrap();
    assert_eq!(at.block, 6);
    let r = apply(
        &b,
        &[
            add(at.clone(), LSCF, Some(20), open_keys(), 1.0),
            conv(&at, "C"),
        ],
    )
    .unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied, Outcome::Applied]);
    let o = &r.bytes;
    let h = Header::parse(o).unwrap();
    let man = read_manager(&h, o, 1).unwrap();
    assert_eq!(man.sequences.len(), 3);
    assert_eq!(man.sequences[..2], [2, 3]);
    assert_eq!(count_kind(o, "NiControllerManager"), 1);
    assert_eq!(count_kind(o, "NiMultiTargetTransformController"), 1);
    // C-P29: the MTTC (still block 9) gains the shape the new clip binds
    assert_eq!(read_mttc(&h, o, 9).unwrap().1, [5]);
    let s = read_sequence_out(&h, o, man.sequences[2] as usize).unwrap();
    assert_eq!(s.blocks.len(), 1);
    let m2 = read_model(o).unwrap();
    let added = *m2.lighting[&6].prop.ctrls.last().unwrap();
    assert_ne!(added, 7);
    assert_eq!(
        s.blocks[0].controller, added as i32,
        "7 is managed: it stays out"
    );
}

/// (file, shape, property, role) cases refused for want of free keyed controllers.
fn without_free() -> Vec<(Vec<u8>, u32, u32, Role)> {
    let a = scenes::animated(Some(1), 2);
    let inactive = patched(&a, 4, 4, &0x40u16.to_le_bytes());
    let both = patched(&inactive, 7, 4, &(-1i32).to_le_bytes());
    vec![
        (scenes::animated(Some(1), 0), 2, 3, Role::Shader),
        (named_managed_clip(), 5, 6, Role::Shader),
        (both, 2, 3, Role::Shader),
    ]
}

#[test]
fn convert_on_property_without_free_controllers_is_refused() {
    for (b, shape, _, role) in without_free() {
        let at = anchor_role(&read_model(&b).unwrap(), Some(shape), role).unwrap();
        refused_unchanged(&b, conv(&at, "X"), NO_FREE);
    }
}

#[test]
fn convert_skips_keyless_and_inactive_controllers() {
    let b = patched(&scenes::animated(Some(1), 2), 7, 4, &(-1i32).to_le_bytes());
    let r = apply(&b, &[conv(&conv_at(&b), "X")]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Applied]);
    let o = &r.bytes;
    let h = Header::parse(o).unwrap();
    let s = read_sequence_out(&h, o, only_seq(o)).unwrap();
    let ctrls: Vec<i32> = s.blocks.iter().map(|cb| cb.controller).collect();
    assert_eq!(ctrls, [4]);
    let m2 = read_model(o).unwrap();
    assert_eq!(m2.ctrls[&5].flags, 0x48);
    assert_eq!(m2.ctrls[&5].interp, Some(7));
}

const SHARED: &str = "This alpha is shared by 2 shapes: split it before editing.";

#[test]
fn convert_on_a_shared_alpha_is_a_conflict() {
    let b = scenes::shared_alpha_with_testref_chain();
    let m = read_model(&b).unwrap();
    let at = anchor_role(&m, Some(1), Role::Alpha).unwrap();
    assert_eq!(at.block, 4);
    let r = apply(&b, &[conv(&at, "X")]).unwrap();
    assert_eq!(r.outcomes, vec![Outcome::Conflict(SHARED.into())]);
    assert_eq!(r.bytes, b);
    assert_eq!(convert_check(&m, 4, Some("A")), Err(SHARED.into()));
}

#[test]
fn convert_refuses_a_root_that_is_not_a_node() {
    let b = scenes::free_chain();
    let m = read_model(&b).unwrap();
    let at = Anchor {
        block: 0,
        kind: "BSLightingShaderProperty".into(),
        shape: None,
        role: Role::Shader,
    };
    let r = apply(&b, &[conv(&at, "X")]).unwrap();
    assert!(
        matches!(r.outcomes[0], Outcome::Conflict(_) | Outcome::Refused(_)),
        "{:?}",
        r.outcomes
    );
    assert_eq!(r.bytes, b);
    assert_eq!(
        convert_check(&m, 0, Some("S")),
        Err("The root is not a node: a clip needs a node root.".into())
    );
}

#[test]
fn convert_then_undo_is_identity() {
    let b = scenes::animated(Some(1), 2);
    assert_ne!(apply(&b, &[conv(&conv_at(&b), "X")]).unwrap().bytes, b);
    assert_eq!(apply(&b, &[]).unwrap().bytes, b);
}

#[test]
fn convert_check_matches_the_op() {
    let mut cases = vec![(scenes::animated(Some(1), 2), 2, 3, Role::Shader)];
    cases.extend(mixed_timings().into_iter().map(|b| (b, 2, 3, Role::Shader)));
    cases.extend(without_free());
    cases.push((scenes::shared_alpha_with_testref_chain(), 1, 4, Role::Alpha));
    for (b, shape, prop, role) in cases {
        let m = read_model(&b).unwrap();
        let at = anchor_role(&m, Some(shape), role).unwrap();
        assert_eq!(at.block, prop);
        let check = convert_check(&m, prop, at.shape.as_deref());
        let r = apply(&b, &[conv(&at, "X")]).unwrap();
        match (&check, &r.outcomes[0]) {
            (Ok(_), Outcome::Applied) => {}
            (Err(e), Outcome::Refused(t) | Outcome::Conflict(t)) => assert_eq!(e, t),
            (c, o) => panic!("check {c:?}, op {o:?}"),
        }
    }
    let b = scenes::animated(Some(1), 2);
    let m = read_model(&b).unwrap();
    assert_eq!(convert_check(&m, 3, Some("S")), Ok(vec![4, 5]));
}
