use nif::edit::{Anchor, Op, Role};
use nif::fields::{Field, Value};
use sdc_gui::model::meshes::edit_doc::*;
use std::path::Path;

fn sample_op() -> Op {
    Op::SetField {
        at: Anchor {
            block: 3,
            kind: "BSLightingShaderProperty".into(),
            shape: Some("Shape".into()),
            role: Role::Shader,
        },
        field: Field::ShaderType,
        value: Value::U32(1),
        base: Value::U32(0),
    }
}

fn doc() -> EditDoc {
    EditDoc::new(SourceId {
        origin: "A".into(),
        data_path: "meshes/x.nif".into(),
        archive: None,
        size: 3,
        sha256: "ab".into(),
    })
}

#[test]
fn edit_doc_round_trips_and_tracks_pending() {
    let mut d = doc();
    assert!(!d.pending());
    d.ops.push(DocOp {
        op: sample_op(),
        status: OpStatus::Applied,
    });
    assert!(d.pending());
    let t = d.to_text();
    assert_eq!(EditDoc::from_text(&t).unwrap(), d);
    d.export.ops_hash = Some(d.ops_hash());
    assert!(!d.pending());
    assert!(!d.has_conflicts());
    d.ops[0].status = OpStatus::Conflict;
    assert!(d.has_conflicts());
    assert_eq!(d.ops().len(), 1);
}

#[test]
fn negative_zero_survives_the_document() {
    let mut d = doc();
    let op = Op::SetField {
        at: Anchor {
            block: 3,
            kind: "BSLightingShaderProperty".into(),
            shape: None,
            role: Role::Shader,
        },
        field: Field::UvOffset(0),
        value: Value::F32((-0.0f32).to_bits()),
        base: Value::F32(0.0f32.to_bits()),
    };
    d.ops.push(DocOp {
        op,
        status: OpStatus::Applied,
    });
    let back = EditDoc::from_text(&d.to_text()).unwrap();
    match &back.ops[0].op {
        Op::SetField {
            value: Value::F32(b),
            ..
        } => assert_eq!(*b, 0x8000_0000),
        o => panic!("{o:?}"),
    }
}

#[test]
fn doc_and_export_paths_follow_the_spec() {
    let m = Path::new(r"C:\I\mods\sdc NIF Edits");
    assert_eq!(
        doc_path(m, "meshes/clutter/foo.nif"),
        Path::new(
            r"C:\I\mods\sdc NIF Edits\.sdc\nif-edits\meshes\clutter\foo.nif.sdc-nifedit.toml"
        )
    );
    assert_eq!(
        export_path(m, "meshes/clutter/foo.nif"),
        Path::new(r"C:\I\mods\sdc NIF Edits\meshes\clutter\foo.nif")
    );
}

#[test]
fn save_and_load_use_a_temp_dir() {
    let dir = std::env::temp_dir().join(format!("sdc-nifedit-{}", std::process::id()));
    let p = dir.join("sub").join("a.toml");
    assert_eq!(load(&p).unwrap(), None);
    let mut d = doc();
    d.ops.push(DocOp {
        op: sample_op(),
        status: OpStatus::Applied,
    });
    save(&p, &d).unwrap();
    assert_eq!(load(&p).unwrap(), Some(d));
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn history_merges_drags_caps_at_200_and_redoes() {
    let mut h = NifHistory::default();
    assert!(!h.can_undo() && !h.can_redo());
    h.record("a".into(), Some("drag".into()));
    h.record("b".into(), Some("drag".into()));
    assert_eq!(h.undo("now".into()), Some("a".into()));
    assert!(!h.can_undo() && h.can_redo());
    assert_eq!(h.redo("a".into()), Some("now".into()));

    let mut h = NifHistory::default();
    h.record("a".into(), Some("drag".into()));
    h.break_merge();
    h.record("b".into(), Some("drag".into()));
    assert_eq!(h.undo("c".into()), Some("b".into()));
    assert_eq!(h.undo("b".into()), Some("a".into()));
    assert_eq!(h.undo("a".into()), None);

    h.record("x".into(), None);
    assert!(!h.can_redo());

    let mut h = NifHistory::default();
    for i in 0..NifHistory::LIMIT + 5 {
        h.record(i.to_string(), None);
    }
    let mut n = 0;
    let mut last = String::new();
    while let Some(s) = h.undo("cur".into()) {
        n += 1;
        last = s;
    }
    assert_eq!(n, NifHistory::LIMIT);
    assert_eq!(last, "5");
}

#[test]
fn every_op_variant_round_trips() {
    use nif::edit::Owner;
    use nif::extra::ExtraValue as E;
    let shader = Anchor {
        block: 3,
        kind: "BSLightingShaderProperty".into(),
        shape: Some("S".into()),
        role: Role::Shader,
    };
    let ctrl = Anchor {
        block: 9,
        kind: "BSLightingShaderPropertyFloatController".into(),
        shape: Some("S".into()),
        role: Role::Ctrl {
            owner: Owner::Shader,
            class: "BSLightingShaderPropertyFloatController".into(),
            var: Some(2),
            nth: 1,
        },
    };
    let extra = Anchor {
        block: 4,
        kind: "NiStringExtraData".into(),
        shape: None,
        role: Role::Extra {
            on_root: true,
            name: "n".into(),
            kind: "NiStringExtraData".into(),
            nth: 0,
        },
    };
    let mut ops = vec![
        sample_op(),
        Op::SetBits {
            at: shader.clone(),
            field: Field::Slsf1,
            mask: 0xF0,
            value: 0x30,
            base: 0x10,
        },
        Op::SetTexturePath {
            at: shader.clone(),
            slot: 1,
            path: "a\\b.dds".into(),
            base: "".into(),
        },
        Op::SplitAlpha { at: shader.clone() },
        Op::AddAlpha {
            at: shader.clone(),
            flags: 4845,
            threshold: 128,
        },
        Op::RemoveAlpha { at: ctrl.clone() },
        Op::EditExtra {
            at: extra.clone(),
            value: E::Int(1),
            base: E::Int(2),
        },
        Op::RemoveExtra { at: extra.clone() },
    ];
    for v in [
        E::Bsx(7),
        E::Str("a\r\nb".into()),
        E::Int(5),
        E::Bool(1),
        E::Float((-0.0f32).to_bits()),
        E::Inv {
            rot: [1, 2, 3],
            zoom: 9,
        },
    ] {
        ops.push(Op::AddExtra {
            at: extra.clone(),
            kind: "k".into(),
            name: "n".into(),
            value: v,
        });
    }
    let mut d = doc();
    d.ops = ops
        .into_iter()
        .map(|op| DocOp {
            op,
            status: OpStatus::Applied,
        })
        .collect();
    assert_eq!(EditDoc::from_text(&d.to_text()).unwrap(), d);
    assert_eq!(EditDoc::from_text(&doc().to_text()).unwrap(), doc());
}

#[test]
fn bad_documents_are_refused() {
    let dir = std::env::temp_dir().join(format!("sdc-nifedit-bad-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("bad.toml");
    std::fs::write(&p, "not = [valid").unwrap();
    assert!(load(&p).is_err());
    std::fs::remove_dir_all(&dir).ok();

    let t = doc().to_text();
    let no_ops: String = t
        .lines()
        .filter(|l| !l.starts_with("ops"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(EditDoc::from_text(&no_ops).is_err());
    let v2 = t.replace("version = 1", "version = 2");
    assert!(EditDoc::from_text(&v2).unwrap_err().contains("newer sdc"));

    let mut a = doc();
    a.ops.push(DocOp {
        op: sample_op(),
        status: OpStatus::Applied,
    });
    let mut b = a.clone();
    if let Op::SetField { value, .. } = &mut b.ops[0].op {
        *value = Value::U32(2);
    }
    assert_ne!(a.ops_hash(), b.ops_hash());
}
