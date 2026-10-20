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
