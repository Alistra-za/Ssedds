mod builder;
use builder::scenes::{self, ctrl_body, one_block_nif};
use builder::{Nif, W};
use nif::ctrl::{chain, read_ctrl, read_interp, read_manager, read_sequence, TimeCtrl};
use nif::fields::{locate, Field, Value};
use nif::header::Header;
use nif::NifError;

#[test]
fn free_chain_reads_two_controllers_on_one_bslsp() {
    let b = scenes::free_chain();
    let h = Header::parse(&b).unwrap();
    let lsp = scenes::block_of(&h, "BSLightingShaderProperty");
    let net = nif::net::read_net(&h, &b, lsp).unwrap();
    let mut p = vec![];
    let ids = chain(&h, &b, net.controller, &mut p);
    assert!(p.is_empty(), "{p:?}");
    let c: Vec<TimeCtrl> = ids
        .iter()
        .map(|&i| read_ctrl(&h, &b, i as usize).unwrap())
        .collect();
    assert_eq!(
        c.iter().map(|c| c.var).collect::<Vec<_>>(),
        vec![Some(11), Some(20)]
    );
    assert!(c
        .iter()
        .all(|c| c.known && c.target == lsp as i32 && c.flags == 0x48));
    let i = read_interp(&h, &b, c[0].interp.unwrap() as usize).unwrap();
    assert_eq!(
        (i.class.as_str(), i.value.len()),
        ("NiFloatInterpolator", 1)
    );
    assert_eq!((i.value[0], i.data), (0.5, 5));
}

#[test]
fn chain_cycle_is_a_problem_not_a_hang() {
    let mut n = Nif::new(100);
    for next in [1, 0] {
        n.block(
            "NiAlphaController",
            ctrl_body(next, 0x48, -1, W::default().i32(-1)),
        );
    }
    let b = n.bytes();
    let h = Header::parse(&b).unwrap();
    let mut p = vec![];
    assert_eq!(chain(&h, &b, 0, &mut p), vec![0, 1]);
    assert_eq!(p, vec!["controller chain loops at block 0".to_string()]);
}

#[test]
fn chain_stops_at_out_of_range_and_non_controller_blocks() {
    let b = scenes::alpha_testref();
    let h = Header::parse(&b).unwrap();
    for first in [99, 0, 2] {
        let mut p = vec![];
        assert!(chain(&h, &b, first, &mut p).is_empty());
        assert_eq!(p.len(), 1, "{first}: {p:?}");
    }
    let mut p = vec![];
    assert_eq!(chain(&h, &b, 1, &mut p), vec![1]);
    assert!(p.is_empty());
    assert!(chain(&h, &b, -1, &mut p).is_empty() && p.is_empty());
}

#[test]
fn chain_longer_than_256_is_a_problem() {
    let mut n = Nif::new(100);
    for i in 0..300 {
        n.block(
            "NiAlphaController",
            ctrl_body(i + 1, 0x48, -1, W::default().i32(-1)),
        );
    }
    let b = n.bytes();
    let h = Header::parse(&b).unwrap();
    let mut p = vec![];
    assert_eq!(chain(&h, &b, 0, &mut p).len(), 256);
    assert_eq!(p, vec!["controller chain longer than 256".to_string()]);
}

#[test]
fn alpha_testref_controller_is_known_and_has_no_variable() {
    let b = scenes::alpha_testref();
    let h = Header::parse(&b).unwrap();
    let c = read_ctrl(&h, &b, 1).unwrap();
    assert_eq!((c.known, c.interp, c.var), (true, Some(2), None));
    assert!(locate(&h, &b, 1, Field::Variable).is_err());
    // wrong size for a known class is an error, not a guess
    let short = one_block_nif("NiAlphaController", ctrl_body(-1, 0x48, 0, W::default()));
    let hs = Header::parse(&short).unwrap();
    assert!(matches!(
        read_ctrl(&hs, &short, 0),
        Err(NifError::BlockSize { .. })
    ));
}

#[test]
fn managed_clip_sequences_name_their_controllers() {
    let b = scenes::managed_clip();
    let h = Header::parse(&b).unwrap();
    let root = nif::net::read_net(&h, &b, 0).unwrap();
    let mut p = vec![];
    let ids = chain(&h, &b, root.controller, &mut p);
    assert!(p.is_empty(), "{p:?}");
    assert_eq!(ids.len(), 2);
    let m = read_manager(&h, &b, ids[0] as usize).unwrap();
    assert_eq!(m.sequences.len(), 2);
    assert!(!m.ctrl.known);
    let mttc = read_ctrl(&h, &b, ids[1] as usize).unwrap();
    assert_eq!(
        (mttc.class.as_str(), mttc.known),
        ("NiMultiTargetTransformController", false)
    );

    let lsp = scenes::block_of(&h, "BSLightingShaderProperty");
    let fc = nif::net::read_net(&h, &b, lsp).unwrap().controller;
    let seqs: Vec<_> = m
        .sequences
        .iter()
        .map(|&s| read_sequence(&h, &b, s as usize).unwrap())
        .collect();
    assert_eq!(seqs[0].name.as_deref(), Some("A"));
    assert_eq!(seqs[1].name.as_deref(), Some("B"));
    for s in &seqs {
        assert_eq!(s.manager, ids[0] as i32);
        assert_eq!(s.blocks[0].controller, fc);
        assert_eq!(s.blocks[0].node.as_deref(), Some(&b"shape"[..]));
        assert_eq!(
            s.blocks[0].ctrl_type.as_deref(),
            Some(&b"BSLightingShaderPropertyFloatController"[..])
        );
    }
    assert_eq!(seqs[0].blocks[0].ctrl_id, Some(b"11".to_vec()));
    assert_eq!(seqs[0].blocks[1].ctrl_id, None);
    assert_eq!(seqs[1].blocks[0].ctrl_id, Some(vec![]));

    let fcc = read_ctrl(&h, &b, fc as usize).unwrap();
    assert_eq!((fcc.flags, fcc.var), (0x68, Some(11)));
    let bi = read_interp(&h, &b, fcc.interp.unwrap() as usize).unwrap();
    assert_eq!(
        (bi.class.as_str(), bi.known, bi.data),
        ("NiBlendFloatInterpolator", false, -1)
    );
    assert!(bi.value.is_empty());
}

#[test]
fn sequence_count_past_block_is_an_error() {
    let mut body = scenes::sequence_body();
    body[4..8].copy_from_slice(&1_000_000u32.to_le_bytes());
    let b = one_block_nif("NiControllerSequence", body);
    let h = Header::parse(&b).unwrap();
    assert!(matches!(
        read_sequence(&h, &b, 0),
        Err(NifError::Geometry { .. })
    ));
    // a count that fits in the block but disagrees with its size is a size error
    let mut body = scenes::sequence_body();
    body[4..8].copy_from_slice(&0u32.to_le_bytes());
    let b = one_block_nif("NiControllerSequence", body);
    let h = Header::parse(&b).unwrap();
    assert!(matches!(
        read_sequence(&h, &b, 0),
        Err(NifError::BlockSize { .. })
    ));
}

#[test]
fn out_of_range_blocks_and_wrong_classes_are_errors() {
    let b = scenes::free_chain();
    let h = Header::parse(&b).unwrap();
    assert!(read_ctrl(&h, &b, 99).is_err());
    assert!(read_interp(&h, &b, 99).is_err());
    assert!(read_sequence(&h, &b, 99).is_err());
    assert!(read_manager(&h, &b, 99).is_err());
    assert!(read_ctrl(&h, &b, 0).is_err()); // the property
    assert!(read_manager(&h, &b, 1).is_err()); // a controller, not the manager
}

#[test]
fn controller_field_offsets() {
    let b = scenes::free_chain();
    let h = Header::parse(&b).unwrap();
    assert_eq!(
        locate(&h, &b, 1, Field::Frequency).unwrap(),
        (6, Value::F32(1.0f32.to_bits()))
    );
    assert_eq!(
        locate(&h, &b, 1, Field::CtrlFlags).unwrap(),
        (4, Value::U16(0x48))
    );
    assert_eq!(
        locate(&h, &b, 1, Field::Phase).unwrap(),
        (10, Value::F32(0))
    );
    assert_eq!(
        locate(&h, &b, 1, Field::StartTime).unwrap(),
        (14, Value::F32(0))
    );
    assert_eq!(
        locate(&h, &b, 1, Field::StopTime).unwrap(),
        (18, Value::F32(1.0f32.to_bits()))
    );
    assert_eq!(
        locate(&h, &b, 1, Field::Variable).unwrap(),
        (30, Value::U32(11))
    );
    assert_eq!(
        locate(&h, &b, 3, Field::InterpValue(0)).unwrap(),
        (0, Value::F32(0.5f32.to_bits()))
    );
    assert!(locate(&h, &b, 3, Field::InterpValue(1)).is_err());
    assert!(locate(&h, &b, 3, Field::Frequency).is_err());
    assert!(locate(&h, &b, 5, Field::InterpValue(0)).is_err()); // key data is not editable here
    assert!(locate(&h, &b, 99, Field::Frequency).is_err());
    // the manager has a base but no variable
    let m = scenes::managed_clip();
    let hm = Header::parse(&m).unwrap();
    assert!(locate(&hm, &m, 1, Field::StopTime).is_ok());
    assert!(locate(&hm, &m, 1, Field::Variable).is_err());
}

#[test]
fn point3_interpolator_has_three_values() {
    let body = W::default().fs(&[0.1, 0.2, 0.3]).i32(7).0;
    let b = one_block_nif("NiPoint3Interpolator", body);
    let h = Header::parse(&b).unwrap();
    let i = read_interp(&h, &b, 0).unwrap();
    assert_eq!((i.value.clone(), i.data), (vec![0.1, 0.2, 0.3], 7));
    assert_eq!(
        locate(&h, &b, 0, Field::InterpValue(2)).unwrap(),
        (8, Value::F32(0.3f32.to_bits()))
    );
    assert!(locate(&h, &b, 0, Field::InterpValue(3)).is_err());
}

#[test]
fn scenes_all_parse() {
    for b in scenes::all() {
        Header::parse(&b).unwrap().check_file_length(&b).unwrap();
    }
}

#[test]
fn chain_reports_negative_refs_other_than_minus_one() {
    for bad in [-2, i32::MIN] {
        let mut n = Nif::new(100);
        n.block(
            "NiAlphaController",
            ctrl_body(bad, 0x48, -1, W::default().i32(-1)),
        );
        let b = n.bytes();
        let h = Header::parse(&b).unwrap();
        let mut p = vec![];
        assert_eq!(chain(&h, &b, 0, &mut p), vec![0]);
        assert_eq!(p.len(), 1, "{bad}: {p:?}");
    }
}

#[test]
fn sequence_strings_past_file_length_are_an_error() {
    // 200 CBs x 4 refs to one 4 KB string: 3.2 MB of copies from a ~30 KB file
    let mut n = Nif::new(100);
    let s = n.string(&"x".repeat(4096));
    let mut w = W::default().i32(-1).u32(200).u32(0);
    for _ in 0..200 {
        w = w.i32(-1).i32(-1).u8(0).i32(s).i32(s).i32(s).i32(s).i32(-1);
    }
    let body = w
        .f(1.0)
        .i32(-1)
        .u32(0)
        .fs(&[1.0, 0.0, 1.0])
        .i32(-1)
        .i32(-1)
        .u16(0)
        .0;
    n.block("NiControllerSequence", body);
    let b = n.bytes();
    let h = Header::parse(&b).unwrap();
    assert!(matches!(
        read_sequence(&h, &b, 0),
        Err(NifError::Geometry { .. })
    ));
}
