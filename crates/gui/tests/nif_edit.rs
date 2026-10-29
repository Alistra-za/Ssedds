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

use nif::keys::{Key, KeyData, KeyGroup};

fn fkey(time: f32, value: f32, tbc: Option<[f32; 3]>) -> Key<f32> {
    Key {
        time,
        value,
        forward: None,
        backward: None,
        tbc,
    }
}

#[test]
fn key_bits_survive_the_document() {
    let mut d = doc();
    let op = Op::SetKeys {
        at: sample_op_anchor(),
        clip: None,
        clip_name: None,
        keys: KeyData::Float(KeyGroup {
            key_type: Some(1),
            keys: vec![fkey(-0.0, f32::from_bits(0x7FC0_0001), None)],
        }),
        base: KeyData::Float(KeyGroup {
            key_type: None,
            keys: vec![],
        }),
    };
    d.ops.push(DocOp {
        op,
        status: OpStatus::Applied,
    });
    let back = EditDoc::from_text(&d.to_text()).unwrap();
    match &back.ops[0].op {
        Op::SetKeys {
            keys: KeyData::Float(g),
            ..
        } => {
            assert_eq!(g.keys[0].time.to_bits(), 0x8000_0000);
            assert_eq!(g.keys[0].value.to_bits(), 0x7FC0_0001);
        }
        o => panic!("{o:?}"),
    }
}

fn sample_op_anchor() -> Anchor {
    match sample_op() {
        Op::SetField { at, .. } => at,
        _ => unreachable!(),
    }
}

#[test]
fn doc_and_export_paths_follow_the_spec() {
    let m = Path::new(r"C:\I\mods\sdc NIF Edits");
    assert_eq!(
        doc_path(m, "meshes/clutter/foo.nif").unwrap(),
        Path::new(
            r"C:\I\mods\sdc NIF Edits\.sdc\nif-edits\meshes\clutter\foo.nif.sdc-nifedit.toml"
        )
    );
    assert_eq!(
        export_path(m, "meshes/clutter/foo.nif").unwrap(),
        Path::new(r"C:\I\mods\sdc NIF Edits\meshes\clutter\foo.nif")
    );
}

const ESCAPING: [&str; 5] = [
    r"..\x.nif",
    r"C:\x.nif",
    r"\x.nif",
    r"meshes\..\..\x.nif",
    "meshes/a:b.nif",
];

/// Final review I3: a data path from a crafted BSA must not lead a write out of the edit mod.
#[test]
fn a_data_path_that_leaves_the_edit_mod_is_refused() {
    let m = Path::new(r"C:\I\mods\sdc NIF Edits");
    for p in ESCAPING {
        let want = format!("This NIF's path cannot be used for an edit: {p}");
        assert_eq!(doc_path(m, p), Err(want.clone()), "{p}");
        assert_eq!(export_path(m, p), Err(want), "{p}");
    }
}

#[test]
fn an_edit_on_an_escaping_path_does_not_open() {
    let d = tempfile::tempdir().unwrap();
    let edit_mod = d.path().join("mods").join(EDIT_MOD);
    for p in ESCAPING {
        let r = NifEdit::open(
            fixture("TestNifFile_Skinned_SE.nif"),
            copy("A", 1, false),
            p,
            Some(&edit_mod),
        );
        assert_eq!(
            r.err(),
            Some(format!("This NIF's path cannot be used for an edit: {p}")),
            "{p}"
        );
    }
    assert!(!edit_mod.exists(), "nothing written");
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

/// B-P41 (final review minor 6): AddController's frequency, start and stop are stored as bits, like key floats.
#[test]
fn add_controller_times_round_trip_as_bits() {
    let op = Op::AddController {
        at: Anchor {
            block: 3,
            kind: "BSLightingShaderProperty".into(),
            shape: Some("S".into()),
            role: Role::Shader,
        },
        class: "BSLightingShaderPropertyFloatController".into(),
        var: Some(9),
        keys: KeyData::Float(KeyGroup {
            key_type: Some(1),
            keys: vec![],
        }),
        flags: 0x48,
        freq: f32::from_bits(0xFFC0_0001), // a NaN with a payload and the sign bit
        start: -0.0,
        stop: f32::INFINITY,
    };
    let mut d = doc();
    d.ops = vec![DocOp {
        op,
        status: OpStatus::Applied,
    }];
    let back = EditDoc::from_text(&d.to_text()).unwrap();
    let Op::AddController {
        freq, start, stop, ..
    } = &back.ops[0].op
    else {
        panic!("{:?}", back.ops)
    };
    assert_eq!(
        [freq, start, stop].map(|f| f.to_bits()),
        [0xFFC0_0001, 0x8000_0000, f32::INFINITY.to_bits()]
    );
    // A document saved before B-P41 holds plain numbers: it still opens.
    let t = d.to_text();
    assert!(t.contains("freq = 4290772993"), "{t}");
    let old = EditDoc::from_text(&t.replace("freq = 4290772993", "freq = 1.5")).unwrap();
    assert!(matches!(old.ops[0].op, Op::AddController { freq, .. } if freq == 1.5));
}

/// v2 C2: the three clip ops survive the edit document; AddSequence's times keep their bits (a NaN stop too).
#[test]
fn clip_ops_survive_the_edit_document() {
    use nif::edit::{ClipTrack, Owner};
    let root = Anchor {
        block: 0,
        kind: "NiNode".into(),
        shape: None,
        role: Role::Root,
    };
    let track = |var: u32, block: u32| ClipTrack {
        at: Anchor {
            block,
            kind: "BSLightingShaderPropertyFloatController".into(),
            shape: Some("S".into()),
            role: Role::Ctrl {
                owner: Owner::Shader,
                class: "BSLightingShaderPropertyFloatController".into(),
                var: Some(var),
                nth: 0,
            },
        },
        keys: KeyData::Float(KeyGroup {
            key_type: Some(1),
            keys: vec![Key {
                time: 0.0,
                value: 0.5,
                forward: None,
                backward: None,
                tbc: None,
            }],
        }),
    };
    let ops = [
        Op::AddSequence {
            at: root.clone(),
            name: "Open".into(),
            cycle: 2,
            freq: 1.0,
            start: -0.0,
            stop: f32::from_bits(0xFFC0_0001),
            tracks: vec![track(11, 4), track(20, 5)],
        },
        Op::RemoveSequence {
            at: root.clone(),
            name: "Idle".into(),
            nth: 1,
        },
        Op::RenameSequence {
            at: root,
            name: "Open".into(),
            nth: 0,
            new_name: "Close".into(),
        },
    ];
    let mut d = doc();
    d.ops = ops
        .iter()
        .map(|op| DocOp {
            op: op.clone(),
            status: OpStatus::Applied,
        })
        .collect();
    let back = EditDoc::from_text(&d.to_text()).unwrap();
    let Op::AddSequence { start, stop, .. } = &back.ops[0].op else {
        panic!("{:?}", back.ops)
    };
    assert_eq!(
        [start.to_bits(), stop.to_bits()],
        [0x8000_0000, 0xFFC0_0001]
    );
    // NaN != NaN: compare the rest with the stop made equal
    let mut back = back;
    if let Op::AddSequence { stop, .. } = &mut back.ops[0].op {
        *stop = 1.0;
    }
    if let Op::AddSequence { stop, .. } = &mut d.ops[0].op {
        *stop = 1.0;
    }
    assert_eq!(back, d);
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
        Op::SetKeys {
            at: ctrl.clone(),
            clip: Some(2),
            clip_name: Some("Open".into()),
            keys: KeyData::Float(KeyGroup {
                key_type: Some(1),
                keys: vec![fkey(-0.0, 1e38, None), fkey(1.0, -0.0, None)],
            }),
            base: KeyData::Pos(KeyGroup {
                key_type: Some(2),
                keys: vec![Key {
                    time: 0.5,
                    value: [1.0, -2.0, 3.0],
                    forward: Some([0.1, 0.2, 0.3]),
                    backward: Some([-0.1, -0.2, -0.3]),
                    tbc: None,
                }],
            }),
        },
        Op::SetKeys {
            at: ctrl.clone(),
            clip: None,
            clip_name: None,
            keys: KeyData::Float(KeyGroup {
                key_type: Some(3),
                keys: vec![fkey(0.0, 2.0, Some([0.5, -0.25, 1.0]))],
            }),
            base: KeyData::Bool(KeyGroup {
                key_type: None,
                keys: vec![],
            }),
        },
        Op::AddController {
            at: shader.clone(),
            class: "BSLightingShaderPropertyFloatController".into(),
            var: Some(9),
            keys: KeyData::Float(KeyGroup {
                key_type: Some(1),
                keys: vec![fkey(0.0, 1.0, None), fkey(2.0, -0.0, None)],
            }),
            flags: 0x48,
            freq: 1.0,
            start: 0.0,
            stop: 2.0,
        },
        Op::AddController {
            at: shader.clone(),
            class: "BSNiAlphaPropertyTestRefController".into(),
            var: None,
            keys: KeyData::Pos(KeyGroup {
                key_type: Some(1),
                keys: vec![Key {
                    time: 0.0,
                    value: [1.0, 0.5, 0.25],
                    forward: None,
                    backward: None,
                    tbc: None,
                }],
            }),
            flags: 0x48,
            freq: 1.0,
            start: 0.0,
            stop: 2.0,
        },
        Op::RemoveController { at: ctrl.clone() },
        Op::ConvertChainToClips {
            at: shader.clone(),
            name: "Caf\u{e9}".into(),
        },
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

// ---- v2 A16: the edit session, source resolution and the Parsed preview ----

mod support;
use modscan::store::PathCopy;
use render3d::locate::Fingerprint;
use sdc_gui::model::meshes::nif_edit::*;
use sdc_gui::model::meshes::MeshesModel;
use sdc_gui::model::viewport3d::{Content, V3Cmd};
use sdc_gui::settings::MeshesSettings;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

fn copy(origin: &str, prio: i32, derived: bool) -> PathCopy {
    PathCopy {
        copy_id: prio as i64,
        origin: origin.into(),
        kind: modscan::store::KIND_MOD,
        root: format!(r"C:\I\mods\{origin}"),
        priority: prio,
        derived,
    }
}

#[test]
fn pick_source_skips_the_edit_mod_and_pgpatcher_output() {
    let m = Path::new(r"C:\I\mods\sdc NIF Edits");
    let c = [
        copy("PG output", 9, true),
        copy(EDIT_MOD, 8, false),
        copy("A", 3, false),
    ];
    assert_eq!(pick_source(&c, m).map(|c| c.origin.as_str()), Some("A"));
    assert!(pick_source(&c[..2], m).is_none());
}

/// Final review M2: Windows folder names ignore case, so `SDC NIF Edits` is still the edit mod.
#[test]
fn the_edit_mod_is_matched_ignoring_case() {
    let m = Path::new(r"C:\I\mods\sdc NIF Edits");
    let other = "SDC NIF Edits";
    let c = [copy(other, 8, false), copy("A", 3, false)];
    assert_eq!(pick_source(&c, m).map(|c| c.origin.as_str()), Some("A"));
    let mods = [
        mod_row("PGP", 9, true),
        mod_row(other, 5, false),
        mod_row("A", 3, false),
    ];
    let copies = [copy("PGP", 9, true), c[0].clone(), c[1].clone()];
    assert_eq!(placement(&copies, &mods, EDIT_MOD), Placement::Ok);
}

/// Trace 2: MO2 roots use `/` after the instance folder, the export folder picker uses `\`.
#[test]
fn the_edit_mod_is_matched_across_separator_spelling() {
    let m = Path::new(r"X:\Instance\mods\sdc NIF Edits");
    let mut edit = copy("sdc NIF Edits", 8, false);
    edit.root = r"X:\Instance/mods\sdc NIF Edits".into();
    let mut a = copy("A", 3, false);
    a.root = r"X:\Instance/mods\A".into();
    let c = [edit.clone(), a];
    assert_eq!(pick_source(&c, m).map(|c| c.origin.as_str()), Some("A"));
    edit.root = r"X:\Instance/mods\sdc NIF Edits\".into();
    assert!(pick_source(&[edit], m).is_none());
}

#[test]
fn generated_winner_warns() {
    assert_eq!(
        generated_warning("DynDOLOD Output").as_deref(),
        Some("DynDOLOD Output is generated by a tool: its next run will overwrite this edit.")
    );
    assert!(generated_warning("A").is_none());
}

#[test]
fn slot_labels_follow_shader_type_and_flags() {
    assert_eq!(edit_slot_label(5, 0, 0, 2), "Skin");
    assert_eq!(edit_slot_label(6, 0, 0, 2), "Hair");
    assert_eq!(edit_slot_label(0, 0, 1 << 6, 2), "Glow");
    assert_eq!(edit_slot_label(11, 0, 0, 6), "Inner layer");
    assert_eq!(edit_slot_label(1, 0, 0, 4), "Env cube");
    assert_eq!(edit_slot_label(0, 0, 1 << 23, 5), "RMAOS");
}

/// Trace 2026-10-28 #1 (fix round 1): 2000 zero-size BSTriShapes that all name one 100 000-byte string (0x80, which
/// decodes to three bytes). The shape cards keep the one shared name, not a copy per card.
#[test]
fn shape_cards_share_one_name_across_many_shapes() {
    let mut b = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    b.extend(0x1402_0007u32.to_le_bytes());
    b.push(1);
    b.extend(12u32.to_le_bytes());
    b.extend(2000u32.to_le_bytes());
    b.extend(100u32.to_le_bytes());
    b.extend([1u8, 0, 1, 0, 1, 0]); // author, process and export script
    b.extend(1u16.to_le_bytes());
    b.extend(10u32.to_le_bytes());
    b.extend(b"BSTriShape");
    b.extend([0u8; 2 * 2000]); // type indices
    b.extend([0u8; 4 * 2000]); // sizes: every block starts at the footer
    b.extend(31u32.to_le_bytes());
    b.extend(100_000u32.to_le_bytes());
    for k in 0..30 {
        b.extend(1u32.to_le_bytes());
        b.push(b'a' + k as u8);
    }
    b.extend(100_000u32.to_le_bytes());
    b.extend(vec![0x80u8; 100_000]);
    b.extend(0u32.to_le_bytes()); // groups
    b.extend(30u32.to_le_bytes()); // footer: 30 roots, read as name index 30
    b.extend([0u8; 4 * 30]);
    let cards = sdc_gui::model::meshes::nif_edit::shape_cards(&b);
    assert_eq!(cards.len(), 2000);
    let ptrs: std::collections::BTreeSet<_> = cards
        .iter()
        .map(|c| c.name.as_deref().map(str::as_ptr))
        .collect();
    assert_eq!(ptrs.len(), 1, "{} distinct name copies", ptrs.len());
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../nif/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn put(root: &Path, rel: &str, bytes: Option<&[u8]>) {
    let fixture = fixture("TestNifFile_Skinned_SE.nif");
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes.unwrap_or(&fixture)).unwrap();
}

/// An MO2 instance with profile-local INIs (no real Documents folder is read), as `modscan`'s tests build it.
fn instance(modlist: &str) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    let game = r.join("Game");
    std::fs::create_dir_all(game.join("Data")).unwrap();
    std::fs::write(
        r.join("ModOrganizer.ini"),
        format!(
            "[General]\ngamePath=@ByteArray({})\nselected_profile=@ByteArray(Default)\n",
            game.display().to_string().replace('\\', "\\\\")
        ),
    )
    .unwrap();
    for m in ["A", "PGP"] {
        std::fs::create_dir_all(r.join("mods").join(m)).unwrap();
    }
    std::fs::create_dir_all(r.join("overwrite")).unwrap();
    std::fs::create_dir_all(r.join("profiles/Default")).unwrap();
    std::fs::write(r.join("profiles/Default/modlist.txt"), modlist).unwrap();
    std::fs::write(
        r.join("profiles/Default/settings.ini"),
        "[General]\nLocalSettings=true\n",
    )
    .unwrap();
    d
}

/// Mods A (x.nif, a loose texture no shape uses) and PGP (PGPatcher's patch of x.nif); an LE x.nif in A alone.
fn files(le: bool) -> tempfile::TempDir {
    files_with(le, "+PGP\n+A\n")
}

fn files_with(le: bool, modlist: &str) -> tempfile::TempDir {
    let d = instance(modlist);
    let le_bytes = fixture("TestNifFile_Animated_LE.nif");
    put(d.path(), "mods/A/meshes/x.nif", le.then_some(&le_bytes[..]));
    put(d.path(), "mods/A/textures/new.dds", Some(b"dds"));
    if !le {
        let a = std::fs::read(d.path().join("mods/A/meshes/x.nif")).unwrap();
        put(d.path(), "mods/PGP/meshes/x.nif", None);
        put(d.path(), "mods/PGP/PGPatcher.esp", Some(b"e"));
        let diff = format!(
            r#"{{"meshes\\x.nif":{{"crc32original":{},"crc32patched":1}}}}"#,
            crc32fast::hash(&a)
        );
        put(
            d.path(),
            "mods/PGP/ParallaxGen_Diff.json",
            Some(diff.as_bytes()),
        );
    }
    d
}

/// Scans `root` into `m`'s store and selects A's x.nif.
fn scan_and_select(m: &mut MeshesModel, root: &Path) {
    m.activate_with(None);
    m.use_instance(root.to_path_buf(), "Default".into());
    m.request_scan(false);
    let req = m.take_request().unwrap();
    m.start(req);
    let t = Instant::now();
    m.poll();
    while m.scanning() || m.v3.locating() {
        assert!(t.elapsed() < Duration::from_secs(60), "scan did not end");
        std::thread::sleep(Duration::from_millis(10));
        m.poll();
    }
    assert_eq!(m.error, None);
    let a = m.mods.iter().find(|r| r.name == "A").unwrap().id;
    m.select_mod(a);
    let c = m.nifs[0].copy_id;
    m.select_nif(c);
}

fn session(le: bool) -> (tempfile::TempDir, MeshesModel) {
    let d = files(le);
    let mut m = MeshesModel::new(Some(d.path().join("scan.db")), MeshesSettings::default());
    m.procs = sdc_gui::model::meshes::no_procs;
    scan_and_select(&mut m, d.path());
    (d, m)
}

/// Confirms shaders and stands in for the render thread.
fn attach(m: &mut MeshesModel, root: &Path) -> Receiver<V3Cmd> {
    m.v3.on_located(Ok(Some(Fingerprint {
        root: root.join("CS"),
        owner_mod: None,
        hash: "h".into(),
    })));
    m.v3.attach_test_thread()
}

fn loads(cmds: &Receiver<V3Cmd>) -> Vec<V3Cmd> {
    cmds.try_iter()
        .filter(|c| {
            matches!(
                c,
                V3Cmd::Load { .. } | V3Cmd::LoadParsed { .. } | V3Cmd::LoadBuiltin { .. }
            )
        })
        .collect()
}

/// (shape block, its BSLightingShaderProperty block) of the first lit shape.
fn lit_shape(e: &NifEdit) -> (u32, u32) {
    e.model
        .shapes
        .iter()
        .find_map(|s| {
            s.shader
                .filter(|b| e.model.lighting.contains_key(b))
                .map(|b| (s.block, b))
        })
        .expect("a lit shape")
}

#[test]
fn open_edit_loads_the_source_and_previews_parsed() {
    let (_d, mut m) = session(false);
    m.open_edit();
    assert_eq!(m.error, None);
    let e = m.edit.as_ref().expect("an edit session");
    assert_eq!(e.source_copy.origin, "A");
    assert!(!e.model.shapes.is_empty());
    assert!(!e.le_mesh);
    assert_eq!(e.source_changed, None);
    assert!(matches!(m.v3.content(), Some(Content::Parsed { .. })));
    assert!(m.v3.nif_view);
    m.close_edit();
    assert!(m.edit.is_none());
    assert!(m.v3.is_mesh());
    assert!(!m.v3.nif_view);
}

#[test]
fn glossiness_edit_reaches_the_viewport_without_a_reload() {
    let (d, mut m) = session(false);
    let cmds = attach(&mut m, d.path());
    m.open_edit();
    m.v3.request_frame(64, 64);
    assert_eq!(loads(&cmds).len(), 1, "the first LoadParsed");
    let e = m.edit.as_ref().unwrap();
    let (shape, shader) = lit_shape(e);
    let cur = e.model.lighting[&shader].lighting.params.glossiness;
    assert_ne!(cur, 12.5);
    let at = nif::edit::anchor_role(&e.model, Some(shape), Role::Shader).unwrap();
    m.edit_push(
        vec![Op::SetField {
            at,
            field: Field::Glossiness,
            value: Value::F32(12.5f32.to_bits()),
            base: Value::F32(cur.to_bits()),
        }],
        None,
    );
    assert_eq!(m.error, None);
    let got: Vec<V3Cmd> = cmds.try_iter().collect();
    assert!(got.iter().any(|c| matches!(
        c,
        V3Cmd::SetMaterial { material, .. } if material.params.glossiness == 12.5
    )));
    assert!(!got.iter().any(|c| matches!(c, V3Cmd::LoadParsed { .. })));
    m.v3.request_frame(64, 64);
    assert!(loads(&cmds).is_empty(), "no reload");
    let e = m.edit.as_ref().unwrap();
    assert_eq!(e.doc.ops.len(), 1);
    assert_eq!(e.doc.ops[0].status, OpStatus::Applied);
    let saved = load(&e.doc_path).unwrap().expect("saved after the push");
    assert_eq!(saved.ops.len(), 1);
    assert!(m.undo_edit());
    assert!(m.edit.as_ref().unwrap().doc.ops.is_empty());
    assert!(m.redo_edit());
    assert_eq!(m.edit.as_ref().unwrap().doc.ops.len(), 1);
}

#[test]
fn new_texture_path_reloads_the_same_view() {
    let (d, mut m) = session(false);
    let cmds = attach(&mut m, d.path());
    m.open_edit();
    m.v3.request_frame(64, 64);
    let first = loads(&cmds);
    let Some(V3Cmd::LoadParsed { textures, .. }) = first.first() else {
        panic!("LoadParsed first")
    };
    assert!(!textures.contains_key("textures/new.dds"));
    let e = m.edit.as_ref().unwrap();
    let (shape, shader) = lit_shape(e);
    let set = e.model.lighting[&shader].texture_set.unwrap();
    let base = String::from_utf8_lossy(&e.model.texsets[&set].raw.slots[0]).into_owned();
    m.edit_push(
        vec![Op::SetTexturePath {
            at: nif::edit::anchor_shape(&e.model, shape),
            slot: 0,
            path: r"textures\new.dds".into(),
            base,
        }],
        None,
    );
    assert_eq!(m.error, None);
    assert_eq!(
        m.edit.as_ref().unwrap().doc.ops[0].status,
        OpStatus::Applied
    );
    m.v3.request_frame(64, 64);
    let again = loads(&cmds);
    let Some(V3Cmd::LoadParsed { textures, .. }) = again.first() else {
        panic!("a new LoadParsed")
    };
    assert!(textures.contains_key("textures/new.dds"));
}

#[test]
fn le_file_opens_without_preview() {
    let (d, mut m) = session(true);
    let cmds = attach(&mut m, d.path());
    m.open_edit();
    m.v3.request_frame(64, 64);
    let e = m.edit.as_ref().expect("an LE NIF opens read-only");
    assert!(e.le_mesh);
    assert!(!e.model.shapes.is_empty());
    assert!(loads(&cmds).is_empty());
    assert!(!m.v3.nif_view);
}

/// Correction 2026-10-20: `SdcApp::new` replaces `meshes.v3`, so the flag must come from `open_edit`.
#[test]
fn an_edited_nif_keeps_its_failed_shader_message_in_the_app() {
    use eframe::egui;
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;
    use render3d::{ShapeInfo, ShapeStatus};
    use sdc_gui::app::SdcApp;
    app_data();
    let d = files(false);
    let model = sdc_gui::model::AppModel::new(
        sdc_gui::settings::GuiSettings::default(),
        None,
        support::session(),
    );
    let mut app = SdcApp::new(model);
    app.meshes.procs = sdc_gui::model::meshes::no_procs;
    app.meshes.store_path = Some(d.path().join("scan.db"));
    scan_and_select(&mut app.meshes, d.path());
    app.meshes.open_edit();
    assert_eq!(app.meshes.error, None);
    let (id, material) = app.meshes.edit.as_ref().unwrap().materials()[0].clone();
    app.meshes.v3.report = Some(render3d::LoadReport::default());
    app.meshes.v3.shapes = vec![ShapeInfo {
        id,
        name: "Edited".into(),
        material,
        status: ShapeStatus::Failed {
            error: "boom".into(),
            showing_previous: false,
        },
    }];
    let mut frame = eframe::Frame::_new_kittest();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(
            move |ui, app: &mut SdcApp| eframe::App::ui(app, ui, &mut frame),
            app,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    sdc_gui::ui::header::set_meshes_view(&h.ctx, true);
    h.run_steps(4);
    h.get_by_label("Edited: shader failed: boom");
}

// ---- A16 review: a slider drag is one op ----

/// One drag tick on the first lit shape's glossiness, based on the value the panel shows now.
fn tick(m: &mut MeshesModel, v: f32) {
    let e = m.edit.as_ref().unwrap();
    let (shape, shader) = lit_shape(e);
    let cur = e.model.lighting[&shader].lighting.params.glossiness;
    let op = Op::SetField {
        at: nif::edit::anchor_role(&e.model, Some(shape), Role::Shader).unwrap(),
        field: Field::Glossiness,
        value: Value::F32(v.to_bits()),
        base: Value::F32(cur.to_bits()),
    };
    let key = nif::edit::merge_key(&op);
    m.edit_push(vec![op], Some(key));
}

fn op_values(m: &MeshesModel) -> Vec<(f32, f32)> {
    m.edit
        .as_ref()
        .unwrap()
        .doc
        .ops
        .iter()
        .map(|d| match &d.op {
            Op::SetField {
                value: Value::F32(v),
                base: Value::F32(b),
                ..
            } => (f32::from_bits(*b), f32::from_bits(*v)),
            o => panic!("{o:?}"),
        })
        .collect()
}

#[test]
fn a_slider_drag_is_one_op_with_the_original_base() {
    let (_d, mut m) = session(false);
    m.open_edit();
    let e = m.edit.as_ref().unwrap();
    let orig = e.model.lighting[&lit_shape(e).1].lighting.params.glossiness;
    for v in [orig + 1.0, orig + 2.0, 40.5] {
        tick(&mut m, v);
    }
    assert_eq!(op_values(&m), [(orig, 40.5)]);
    // Undo is one step back to before the drag.
    assert!(m.undo_edit());
    assert!(op_values(&m).is_empty());
    assert!(m.redo_edit());
    assert_eq!(op_values(&m), [(orig, 40.5)]);
    // After the drag ends, the next drag on the same field is a new op based on the committed value.
    m.edit.as_mut().unwrap().flush();
    for v in [50.5, 60.5] {
        tick(&mut m, v);
    }
    assert_eq!(op_values(&m), [(orig, 40.5), (40.5, 60.5)]);
    let e = m.edit.as_mut().unwrap();
    assert!(e.doc.ops.iter().all(|d| d.status == OpStatus::Applied));
    e.revert(1);
    assert_eq!(e.doc.ops[0].status, OpStatus::Applied);
    e.revert(0);
    assert_eq!(
        e.patched, e.source,
        "reverting the drag restores the source bytes"
    );
}

#[test]
fn a_failed_reopen_returns_the_view_to_the_mesh() {
    let (_d, mut m) = session(false);
    m.open_edit();
    assert!(matches!(m.v3.content(), Some(Content::Parsed { .. })));
    let p = m.edit.as_ref().unwrap().doc_path.clone();
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, "not = [a document").unwrap();
    m.open_edit();
    assert!(m.edit.is_none());
    assert!(m.error.is_some());
    assert!(m.v3.is_mesh());
}

/// One non-merged SetField on the first lit shape's shader.
fn set_lit(m: &mut MeshesModel, field: Field, v: f32) {
    let e = m.edit.as_ref().unwrap();
    let (shape, shader) = lit_shape(e);
    let p = &e.model.lighting[&shader].lighting.params;
    let cur = match field {
        Field::Glossiness => p.glossiness,
        _ => p.specular_strength,
    };
    let at = nif::edit::anchor_role(&e.model, Some(shape), Role::Shader).unwrap();
    m.edit_push(
        vec![Op::SetField {
            at,
            field,
            value: Value::F32(v.to_bits()),
            base: Value::F32(cur.to_bits()),
        }],
        None,
    );
}

/// v2 B8 (supersedes P13's In game half): In game (simulated) keeps the session and shows every edit.
#[test]
fn in_game_and_back_keeps_the_session_and_shows_every_edit() {
    let (_d, mut m) = session(false);
    assert!(m.pair.is_some(), "x.nif has a PGPatcher copy");
    m.open_edit();
    assert!(m.view_source, "the edit preview is the Source side");
    set_lit(&mut m, Field::Glossiness, 12.5);
    m.set_view_source(false);
    assert!(!m.v3.is_mesh(), "In game is the simulated edit preview");
    assert!(m.v3.nif_view);
    assert!(m.edit.as_ref().unwrap().in_game.is_some());
    set_lit(&mut m, Field::SpecularStrength, 3.5);
    assert_eq!(m.edit.as_ref().unwrap().doc.ops.len(), 2);
    let Some(Content::Parsed { nif, .. }) = m.v3.content() else {
        panic!("In game shows the edit preview")
    };
    let p = &nif.shapes[0].material.params;
    assert_eq!(
        (p.glossiness, p.specular_strength),
        (12.5, 3.5),
        "no JSON: in game too"
    );
    let (shape, _) = lit_shape(m.edit.as_ref().unwrap());
    m.select_edit(Selection::Shape(shape));
    wait_pg(&mut m);
    let mut h = view(m);
    h.run_steps(4);
    h.get_by_label("In game (simulated)");
    h.get_by_label(NO_MATCH);
    let m = h.state_mut();
    m.set_view_source(true);
    assert!(m.v3.nif_view);
    let Some(Content::Parsed { nif, .. }) = m.v3.content() else {
        panic!("Source shows the edit preview again")
    };
    let p = &nif.shapes[0].material.params;
    assert_eq!((p.glossiness, p.specular_strength), (12.5, 3.5));
    h.run_steps(4);
    assert!(!h.state().edit_preview_hidden());
    assert!(h.query_by_label(EDIT_HIDDEN).is_none());
}

// ---- v2 A17: the edit panel, selection, Shader and Alpha tabs ----

#[path = "../../nif/tests/builder/mod.rs"]
mod builder;
use builder::scenes;
use eframe::egui;
use egui::accesskit::Role as AkRole;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use nif::edit::validate::Target;
use nif::extra::ExtraValue;

/// The Meshes view alone, as `meshes_view.rs` draws it.
fn view(m: MeshesModel) -> Harness<'static, MeshesModel> {
    let h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(
            |ui, m: &mut MeshesModel| sdc_gui::ui::meshes::show(ui, m),
            m,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

/// A read-only edit session on `bytes` without a scan (no store, no preview).
fn direct(bytes: Vec<u8>) -> MeshesModel {
    let mut m = MeshesModel::empty();
    m.edit = Some(NifEdit::open(bytes, copy("A", 1, false), "meshes/x.nif", None).unwrap());
    m
}

fn shape_named(e: &NifEdit, name: &str) -> u32 {
    e.model
        .shapes
        .iter()
        .find(|s| s.name.as_deref() == Some(name))
        .unwrap()
        .block
}

fn select(m: &mut MeshesModel, name: &str) -> u32 {
    let b = shape_named(m.edit.as_ref().unwrap(), name);
    m.select_edit(Selection::Shape(b));
    b
}

fn ops(m: &MeshesModel) -> Vec<Op> {
    m.edit.as_ref().unwrap().doc.ops().to_vec()
}

#[test]
fn edit_toggle_opens_the_panel_with_four_tabs() {
    let (_d, m) = session(false);
    let mut h = view(m);
    h.run_steps(4);
    assert!(h.query_by_label("Controllers tab").is_none());
    h.get_by_label("Edit").click();
    h.run_steps(4);
    assert!(h.state().edit.is_some(), "Edit opens a session");
    assert!(
        h.state().shapes_col,
        "Edit shows the shape cards to pick from"
    );
    for l in ["Shader", "Alpha", "Controllers", "Extra data"] {
        h.get_by_label(&format!("{l} tab"));
    }
    h.get_by_label("Source: A");
    h.get_by_label("Edit").click();
    h.run_steps(4);
    assert!(h.state().edit.is_none(), "Edit again closes it");
    assert!(h.query_by_label("Controllers tab").is_none());
}

#[test]
fn clicking_a_card_selects_the_shape_and_root_card_is_first() {
    let (_d, mut m) = session(false);
    m.open_edit();
    m.shapes_col = true;
    let s = m.shapes[0].clone();
    let card = format!(
        "Select shape {} {}",
        s.index3d
            .map_or("#– (not in scene tree)".into(), |i| format!("#{i}")),
        s.name.as_deref().unwrap_or("(unnamed)"),
    );
    let mut h = view(m);
    h.run_steps(4);
    let root = h.get_by_label("Root").rect();
    let first = h.get_by_label(&card).rect();
    assert!(root.max.y <= first.min.y, "Root is the first card");
    h.get_by_label(&card).click();
    h.run_steps(4);
    assert_eq!(
        h.state().edit.as_ref().unwrap().selection,
        Selection::Shape(s.block)
    );
    h.get_by_label("Root").click();
    h.run_steps(4);
    assert_eq!(h.state().edit.as_ref().unwrap().selection, Selection::Root);
}

/// Meshes UI T2: a card picked outside edit mode is where Edit opens; the Shapes button is locked on while editing.
#[test]
fn edit_opens_on_the_card_picked_before_and_locks_the_shapes_column() {
    let (_d, mut m) = session(false);
    m.shapes_col = true;
    let s = m.shapes.last().unwrap().clone();
    let label = format!(
        "Select shape {} {}",
        s.index3d
            .map_or("#– (not in scene tree)".into(), |i| format!("#{i}")),
        s.name.as_deref().unwrap_or("(unnamed)"),
    );
    let mut h = view(m);
    h.run_steps(4);
    h.get_by_role_and_label(AkRole::Button, &label).click();
    h.run_steps(4);
    assert!(
        h.state().edit.is_none(),
        "a pick outside edit mode opens nothing"
    );
    h.get_by_label("Edit").click();
    h.run_steps(4);
    assert_eq!(
        h.state().edit.as_ref().unwrap().selection,
        Selection::Shape(s.block)
    );
    let shapes = h.get_by_role_and_label(AkRole::Button, "Shapes");
    assert!(
        shapes.accesskit_node().is_disabled(),
        "locked while editing"
    );
    shapes.click();
    h.run_steps(4);
    assert!(h.state().shapes_col);
}

/// T2 review Minor 2: a card picked inside Edit is the card outlined after Edit closes and where the next Edit opens.
#[test]
fn a_pick_inside_edit_carries_to_the_next_edit() {
    let (_d, mut m) = session(false);
    m.open_edit();
    let e = m.edit.as_ref().unwrap();
    let last = e.model.shapes.last().unwrap();
    let (block, name) = (last.block, last.name.as_deref().map(str::to_string));
    let mut h = view(m);
    h.run_steps(4);
    let tail = format!(" {}", name.as_deref().unwrap());
    h.query_all_by_role(AkRole::Button)
        .find(|n| {
            n.accesskit_node()
                .label()
                .is_some_and(|l| l.starts_with("Select shape") && l.ends_with(&tail))
        })
        .expect("the last shape's header")
        .click();
    h.run_steps(4);
    assert_eq!(
        h.state().edit.as_ref().unwrap().selection,
        Selection::Shape(block)
    );
    assert_eq!(h.state().card_sel, Some((block, name.clone())));
    let m = h.state_mut();
    m.close_edit();
    m.open_edit();
    assert_eq!(m.edit.as_ref().unwrap().selection, Selection::Shape(block));
}

/// T2 review Minor 1: Root stays selected across an export-folder switch (the reopen would pick a shape).
#[test]
fn root_stays_selected_across_an_export_folder_switch() {
    let (d, mut m) = export_session();
    m.card_sel = Some((
        m.shapes[0].block,
        m.shapes[0].name.as_deref().map(str::to_string),
    ));
    m.open_edit();
    assert!(matches!(
        m.edit.as_ref().unwrap().selection,
        Selection::Shape(_)
    ));
    m.select_edit(Selection::Root);
    m.set_nif_export_root(Some(d.path().join(r"mods\MyOut")));
    assert!(m.edit.is_some());
    assert_eq!(m.edit.as_ref().unwrap().selection, Selection::Root);
}

/// T2 review Minor 6: a clip-driven controller reads "(clip)", a shape without controllers "none", and a shape whose
/// shader is not a lighting shader keeps its alpha controllers (Minor 3).
#[test]
fn controllers_lines_mark_clips_say_none_and_keep_alpha_on_other_shaders() {
    let mut h = view(direct(scenes::managed_clip()));
    h.run_steps(2);
    h.get_by_label_contains("(clip)");
    let mut h = view(direct(two_named(false)));
    h.run_steps(2);
    assert_eq!(h.query_all_by_label("none").count(), 2);
    let mut n = Nif::new(100);
    let s = n.string("S");
    n.block("BSTriShape", scenes::bstri_named(s, 1, 2));
    n.block(
        "BSWaterShaderProperty",
        W::default().raw(&nif::net::encode_net(-1, &[], -1)).0,
    );
    n.block(
        "NiAlphaProperty",
        W::default()
            .raw(&nif::net::encode_net(-1, &[], 3))
            .u16(0x00ED)
            .u8(128)
            .0,
    );
    n.block(
        "BSNiAlphaPropertyTestRefController",
        scenes::ctrl_body(-1, 0x48, 2, W::default().i32(4)),
    );
    n.block("NiFloatInterpolator", W::default().f(1.0).i32(5).0);
    n.block(
        "NiFloatData",
        W::default().u32(2).u32(1).fs(&[0.0, 0.0, 1.0, 1.0]).0,
    );
    let mut h = view(direct(n.bytes()));
    h.run_steps(2);
    h.get_by_label("1: Alpha test threshold");
}

/// Meshes UI T2: a NIF with one shape opens Edit on that shape, not on Root.
#[test]
fn a_one_shape_nif_opens_edit_on_its_shape() {
    let mut n = builder::Nif::new(100);
    let a = n.string("Only");
    n.block("BSTriShape", one_tri(a, 1));
    n.block("BSLightingShaderProperty", scenes::lsp(0, -1));
    let bytes = n.bytes();
    let d = files_pair(&bytes, &bytes);
    let mut m = MeshesModel::new(Some(d.path().join("scan.db")), MeshesSettings::default());
    m.procs = sdc_gui::model::meshes::no_procs;
    scan_and_select(&mut m, d.path());
    m.open_edit();
    assert_eq!(m.error, None);
    let e = m.edit.as_ref().unwrap();
    assert_eq!(e.selection, Selection::Shape(shape_named(e, "Only")));
}

/// Meshes UI T2: in edit mode a card lists its controllers from the edited model.
#[test]
fn edit_cards_list_their_controllers() {
    let m = direct(ctrl_scene(0x82));
    let mut h = view(m);
    h.run_steps(2);
    h.get_by_label("2: Emissive multiple, U offset");
}

/// A BSTriShape with one triangle (positions only), so the preview draws it.
fn one_tri(name: i32, shader: i32) -> Vec<u8> {
    scenes::avobj(scenes::net(name))
        .fs(&[0.0; 4])
        .i32(-1)
        .i32(shader)
        .i32(-1)
        .u64((1 << 44) | 4) // Vertex, 16-byte records
        .u16(1)
        .u16(3)
        .u32(3 * 16 + 6)
        .fs(&[0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0])
        .u16(0)
        .u16(1)
        .u16(2)
        .u32(0) // particle data
        .0
}

/// BSTriShapes "A" and "B" (0, 1), or "B" and "A" when `swap`, each with its own BSLSP (2, 3).
fn two_named(swap: bool) -> Vec<u8> {
    let mut n = builder::Nif::new(100);
    let (a, b) = (n.string("A"), n.string("B"));
    let (x, y) = if swap { (b, a) } else { (a, b) };
    n.block("BSTriShape", one_tri(x, 2));
    n.block("BSTriShape", one_tri(y, 3));
    n.block("BSLightingShaderProperty", scenes::lsp(0, -1));
    n.block("BSLightingShaderProperty", scenes::lsp(0, -1));
    n.bytes()
}

/// x.nif in mod A and a PGPatcher copy in PGP with different bytes (its blocks numbered differently).
fn files_pair(a: &[u8], pgp: &[u8]) -> tempfile::TempDir {
    let d = instance("+PGP\n+A\n");
    put(d.path(), "mods/A/meshes/x.nif", Some(a));
    put(d.path(), "mods/PGP/meshes/x.nif", Some(pgp));
    put(d.path(), "mods/PGP/PGPatcher.esp", Some(b"e"));
    let diff = format!(
        r#"{{"meshes\\x.nif":{{"crc32original":{},"crc32patched":1}}}}"#,
        crc32fast::hash(a)
    );
    put(
        d.path(),
        "mods/PGP/ParallaxGen_Diff.json",
        Some(diff.as_bytes()),
    );
    d
}

/// Final review I1: the cards, their selection and Solo follow the edited copy, not the PGPatcher copy on show.
#[test]
fn edit_cards_follow_the_session_not_the_shown_pgpatcher_copy() {
    let d = files_pair(&two_named(false), &two_named(true));
    let mut m = MeshesModel::new(Some(d.path().join("scan.db")), MeshesSettings::default());
    m.procs = sdc_gui::model::meshes::no_procs;
    scan_and_select(&mut m, d.path());
    assert!(m.pair.is_some(), "x.nif has a PGPatcher copy");
    m.open_edit();
    assert_eq!(m.error, None);
    m.shapes_col = true;
    let a = shape_named(m.edit.as_ref().unwrap(), "A");
    let shown_a = m
        .shapes
        .iter()
        .find(|s| s.name.as_deref() == Some("A"))
        .unwrap()
        .block;
    assert_ne!(a, shown_a, "the copies number A differently");
    let mut h = view(m);
    h.run_steps(4);
    h.get_by_label_contains("tree) A").click();
    h.run_steps(4);
    assert_eq!(
        h.state().edit.as_ref().unwrap().selection,
        Selection::Shape(a)
    );
    // T4 review Minor 5: the pick is kept in the shown copy's numbering, so the outline finds A after Edit closes.
    assert_eq!(h.state().card_sel, Some((shown_a, Some("A".into()))));
    h.get_by_label("Show only this shape").click();
    h.run_steps(4);
    let mats = h.state().edit.as_ref().unwrap().materials();
    assert_eq!(mats.len(), 2);
    for (id, mat) in mats {
        assert_eq!(mat.hidden, id.0 != a, "shape block {}", id.0);
    }
}

/// Meshes UI T1: the edit-mode cards are built from the edited bytes; untouched, they equal the scan's cards.
#[test]
fn edit_cards_equal_the_scan_before_any_edit() {
    let (d, mut m) = session(false);
    m.open_edit();
    let e = m.edit.as_ref().unwrap();
    let scanned = modscan::store::Store::open_read_only(&d.path().join("scan.db"))
        .unwrap()
        .shapes_of(e.source_copy.copy_id)
        .unwrap();
    assert!(!scanned.is_empty());
    assert_eq!(e.cards, scanned);
}

/// Review Minor 6 (bug 1, rename half): when the source on disk renames its shapes, the cards follow the new names
/// with their full content, not a title only.
#[test]
fn edit_cards_follow_a_source_that_renamed_its_shapes() {
    let mut m = direct(two_named(false));
    let e = m.edit.as_mut().unwrap();
    assert_eq!(e.cards[0].name.as_deref(), Some("A"));
    e.reload_source(two_named(true)).unwrap();
    let names: Vec<_> = e.cards.iter().map(|c| c.name.as_deref()).collect();
    assert_eq!(names, [Some("B"), Some("A")]);
    assert!(
        e.cards.iter().all(|c| c.shader_kind == 1),
        "full cards: {:?}",
        e.cards
    );
}

/// Meshes UI T1 (bug 1): a card shows the edited shader type, flags, alpha and texture path, not the scanned ones.
#[test]
fn edit_cards_show_the_edited_values() {
    use sdc_gui::model::meshes::{alpha_line, shader_type_name};
    let (_d, mut m) = session(false);
    m.open_edit();
    m.shapes_col = true;
    let e = m.edit.as_ref().unwrap();
    let (shape, shader) = lit_shape(e);
    let l = &e.model.lighting[&shader].lighting;
    let ty = l.shader_type;
    let new_ty = if ty == 1 { 0 } else { 1 };
    let at = nif::edit::anchor_role(&e.model, Some(shape), Role::Shader).unwrap();
    let set_block = e.model.lighting[&shader].texture_set.unwrap();
    let alpha = e.model.shape(shape).unwrap().alpha;
    let slot0 = String::from_utf8_lossy(&e.model.texsets[&set_block].raw.slots[0]).into_owned();
    let push = |m: &mut MeshesModel, op: Op| m.edit_push(vec![op], None);
    push(
        &mut m,
        Op::SetField {
            at: at.clone(),
            field: Field::ShaderType,
            value: Value::U32(new_ty),
            base: Value::U32(ty),
        },
    );
    // After the type change: it may have changed the flags too (Note CS).
    let f1 = m.edit.as_ref().unwrap().model.lighting[&shader]
        .lighting
        .slsf1;
    push(
        &mut m,
        Op::SetBits {
            at: at.clone(),
            field: Field::Slsf1,
            mask: 1 << 12,
            value: !f1 & (1 << 12),
            base: f1 & (1 << 12),
        },
    );
    let on_shape = nif::edit::anchor_shape(&m.edit.as_ref().unwrap().model, shape);
    push(
        &mut m,
        Op::SetTexturePath {
            at: on_shape,
            slot: 0,
            path: r"Textures\Edited\Card.dds".into(),
            base: slot0,
        },
    );
    let want_alpha = match alpha {
        Some(a) => {
            let e = m.edit.as_ref().unwrap();
            let p = &e.model.alphas[&a].alpha;
            let at = nif::edit::anchor_role(&e.model, Some(shape), Role::Alpha).unwrap();
            let (flags, thr) = (p.flags, p.threshold);
            push(
                &mut m,
                Op::SetBits {
                    at,
                    field: Field::AlphaFlags,
                    mask: 1,
                    value: !flags as u32 & 1,
                    base: flags as u32 & 1,
                },
            );
            alpha_line(flags ^ 1, thr)
        }
        None => {
            let e = m.edit.as_ref().unwrap();
            let at = nif::edit::anchor_shape(&e.model, shape);
            push(
                &mut m,
                Op::AddAlpha {
                    at,
                    flags: 0x00ED,
                    threshold: 128,
                },
            );
            alpha_line(0x00ED, 128)
        }
    };
    let e = m.edit.as_ref().unwrap();
    assert!(
        e.doc.ops.iter().all(|d| d.status == OpStatus::Applied),
        "{:?}",
        e.outcomes
    );
    let f1_now = e.model.lighting[&shader].lighting.slsf1;
    let mut h = view(m);
    h.run_steps(4);
    h.get_by_label(&shader_type_name(new_ty));
    h.get_by_label(&format!("Details of NIF block {shape}"))
        .click();
    h.run_steps(2);
    h.get_by_label(&format!("SLSF1 0x{f1_now:08X}"));
    h.get_by_label("textures/edited/card.dds");
    h.get_by_label(&want_alpha);
}

#[test]
fn typing_glossiness_records_one_op_and_one_undo_step() {
    let (_d, mut m) = session(false);
    m.open_edit();
    let (shape, shader) = lit_shape(m.edit.as_ref().unwrap());
    m.select_edit(Selection::Shape(shape));
    let mut h = view(m);
    h.run_steps(4);
    h.get_by_label("Glossiness").focus();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    h.get_by_label("Glossiness").type_text("12.5");
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
    let m = h.state_mut();
    let e = m.edit.as_ref().unwrap();
    assert_eq!(e.model.lighting[&shader].lighting.params.glossiness, 12.5);
    assert_eq!(op_values(m).len(), 1, "typing is one op");
    // A drag: two ticks with the panel's merge key, then the drag ends.
    tick(m, 20.5);
    tick(m, 30.5);
    m.edit.as_mut().unwrap().flush();
    assert_eq!(op_values(m).len(), 2, "the drag is one more op");
    assert!(m.undo_edit());
    assert_eq!(op_values(m).len(), 1, "one undo step undoes the drag");
    assert!(m.undo_edit());
    assert!(op_values(m).is_empty());
    assert!(!m.edit.as_ref().unwrap().history.can_undo());
}

#[test]
fn shared_alpha_banner_and_split_on_edit() {
    let mut m = direct(scenes::shared_alpha_and_set());
    let a = select(&mut m, "A");
    let mut h = view(m);
    h.run_steps(2);
    h.get_by_label("Alpha tab").click();
    h.run_steps(2);
    h.get_by_label("Shared by 2 shapes: editing will split.");
    h.get_by_label("Blend").click();
    h.run_steps(2);
    let m = h.state_mut();
    let got = ops(m);
    assert_eq!(got.len(), 2, "{got:?}");
    assert!(matches!(got[0], Op::SplitAlpha { .. }));
    assert!(matches!(
        got[1],
        Op::SetBits {
            field: Field::AlphaFlags,
            mask: 1,
            ..
        }
    ));
    let e = m.edit.as_ref().unwrap();
    assert!(
        e.doc.ops.iter().all(|d| d.status == OpStatus::Applied),
        "{:?}",
        e.outcomes
    );
    let b = shape_named(e, "B");
    let (aa, ab) = (
        e.model.shape(a).unwrap().alpha.unwrap(),
        e.model.shape(b).unwrap().alpha.unwrap(),
    );
    assert_ne!(aa, ab, "A has its own alpha now");
    assert_eq!(e.model.alphas[&aa].alpha.flags, 0x00EC);
    assert_eq!(e.model.alphas[&ab].alpha.flags, 0x00ED, "B is untouched");
    assert!(m.undo_edit());
    assert!(ops(m).is_empty(), "split and edit are one undo step");
}

/// Note FA10: the Alpha threshold slider's second tick sees an unshared alpha and pushes only the SetField; the
/// merged drag must keep the SplitAlpha that started it, or re-applying hits the still-shared alpha (Conflict).
#[test]
fn shared_alpha_threshold_drag_keeps_split() {
    let mut m = direct(scenes::shared_alpha_and_set());
    let a = select(&mut m, "A");
    let (a_name, on_shape, alpha_end, thr0) = {
        let e = m.edit.as_ref().unwrap();
        let alpha = e.model.shape(a).unwrap().alpha.unwrap();
        (
            e.model.shape(a).unwrap().name.as_deref().map(String::from),
            nif::edit::anchor_shape(&e.model, a),
            nif::header::Header::parse(&e.patched).unwrap().sizes.len() as u32,
            e.model.alphas[&alpha].alpha.threshold,
        )
    };
    let set = |at: Anchor, v: u8, base: u8| Op::SetField {
        at,
        field: Field::AlphaThreshold,
        value: Value::U8(v),
        base: Value::U8(base),
    };
    // Tick 1 as the slider builds it for a shared alpha: the SplitAlpha, and the field on the block the split appends.
    let appended = Anchor {
        block: alpha_end,
        kind: "NiAlphaProperty".into(),
        shape: a_name.clone(),
        role: Role::Alpha,
    };
    let key = nif::edit::merge_key(&set(appended.clone(), 0, 0));
    let tick1 = vec![
        Op::SplitAlpha {
            at: on_shape.clone(),
        },
        set(appended, 100, thr0),
    ];
    m.edit_push(tick1, Some(key.clone()));
    // Tick 2 and 3: the alpha is A's own now, so the slider pushes the SetField alone.
    let push_alone = |m: &mut MeshesModel, v: u8| {
        let e = m.edit.as_ref().unwrap();
        let at = nif::edit::anchor_role(&e.model, Some(a), Role::Alpha).unwrap();
        let cur = e.model.alphas[&at.block].alpha.threshold;
        assert_eq!(nif::edit::merge_key(&set(at.clone(), 0, 0)), key);
        m.edit_push(vec![set(at, v, cur)], Some(key.clone()));
    };
    for (i, v) in [150u8, 200].into_iter().enumerate() {
        push_alone(&mut m, v);
        let e = m.edit.as_ref().unwrap();
        let got = ops(&m);
        assert_eq!(got.len(), 2, "tick {}: {got:?}", i + 2);
        // the carried split is A's, and the drag slice is both ops (Split + Set), not just the tail op
        assert!(
            matches!(&got[0], Op::SplitAlpha { at } if at.shape == a_name && at.block == on_shape.block),
            "{got:?}"
        );
        assert!(
            matches!(&got[1], Op::SetField { value: Value::U8(x), base: Value::U8(b), .. } if *x == v && *b == thr0),
            "tick {} keeps the drag's base: {got:?}",
            i + 2
        );
        assert_eq!(e.drag_ops(&key).len(), 2);
        assert!(
            !e.doc.ops.iter().any(|d| d.status == OpStatus::Conflict),
            "tick {}: {:?}",
            i + 2,
            e.outcomes
        );
        assert!(e.doc.ops.iter().all(|d| d.status == OpStatus::Applied));
    }
    let e = m.edit.as_mut().unwrap();
    e.flush();
    let e = m.edit.as_ref().unwrap();
    let b = shape_named(e, "B");
    let (aa, ab) = (
        e.model.shape(a).unwrap().alpha.unwrap(),
        e.model.shape(b).unwrap().alpha.unwrap(),
    );
    assert_ne!(aa, ab, "A has its own alpha");
    assert_eq!(e.model.alphas[&aa].alpha.threshold, 200, "the last tick");
    assert_eq!(e.model.alphas[&ab].alpha.threshold, thr0, "B is untouched");
    assert!(m.undo_edit());
    assert!(ops(&m).is_empty(), "one undo step undoes the whole drag");
    assert!(!m.edit.as_ref().unwrap().history.can_undo());
}

#[test]
fn shared_set_banner_and_texture_edit_keeps_other_shape() {
    let mut m = direct(scenes::shared_alpha_and_set());
    select(&mut m, "A");
    let mut h = view(m);
    h.run_steps(2);
    h.get_by_label("Shared by 2 shapes: editing makes a copy for this shape.");
    let path = r"textures\a.dds";
    let find = |h: &Harness<'static, MeshesModel>| {
        h.query_all_by_role(AkRole::TextInput)
            .find(|n| n.value().as_deref() == Some(path))
            .unwrap_or_else(|| panic!("no box shows {path}"))
            .focus()
    };
    find(&h);
    h.run_steps(1);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    h.query_all_by_role(AkRole::TextInput)
        .find(|n| n.value().as_deref() == Some(path))
        .unwrap()
        .type_text(r"textures\b.dds");
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    let e = h.state().edit.as_ref().unwrap();
    assert_eq!(e.doc.ops.len(), 1);
    assert_eq!(e.doc.ops[0].status, OpStatus::Applied, "{:?}", e.outcomes);
    let slot0 = |name: &str| {
        let s = e.model.shape(shape_named(e, name)).unwrap();
        let set = e.model.lighting[&s.shader.unwrap()].texture_set.unwrap();
        String::from_utf8_lossy(&e.model.texsets[&set].raw.slots[0]).into_owned()
    };
    assert_eq!(slot0("A"), r"textures\b.dds");
    assert_eq!(slot0("B"), r"textures\a.dds");
}

#[test]
fn shared_shader_notice_shows_on_the_shader_tab() {
    let mut m = direct(scenes::two_shapes_one_shader());
    select(&mut m, "A");
    let mut h = view(m);
    h.run_steps(2);
    h.get_by_label("Shared by 2 shapes: shader edits change all of them.");
}

#[test]
fn skinned_and_pbr_bits_are_greyed() {
    let mut m = direct(scenes::shared_alpha_and_set());
    let a = select(&mut m, "A");
    let mut h = view(m);
    h.run_steps(2);
    open_flags(&mut h); // Meshes UI T6: the flag lists start closed
    assert!(h.get_by_label("Skinned").accesskit_node().is_disabled());
    assert!(h
        .get_by_label("Unused01 (TruePBR)")
        .accesskit_node()
        .is_disabled());
    assert!(!h.get_by_label("Specular").accesskit_node().is_disabled());
    assert!(h.query_by_label("Specular level").is_none());
    h.get_by_label("Glossiness");
    // A PBR shape: Glossiness and Specular Strength read as CS uses them.
    let m = h.state_mut();
    let e = m.edit.as_ref().unwrap();
    let at = nif::edit::anchor_role(&e.model, Some(a), Role::Shader).unwrap();
    let shader = e.model.shape(a).unwrap().shader.unwrap();
    let base = e.model.lighting[&shader].lighting.slsf2;
    m.edit_push(
        vec![Op::SetBits {
            at,
            field: Field::Slsf2,
            mask: 1 << 23,
            value: 1 << 23,
            base,
        }],
        None,
    );
    h.run_steps(2);
    h.get_by_label("Specular level");
    h.get_by_label("Roughness scale");
    assert!(h.query_by_label("Glossiness").is_none());
}

#[test]
fn own_emit_badge_shows_for_emissive_without_own_emit() {
    let mut m = direct(scenes::shared_alpha_and_set());
    let a = select(&mut m, "A");
    let mut h = view(m);
    h.run_steps(2);
    assert!(h
        .query_by_label("No effect in game: Own_Emit is off")
        .is_none());
    let m = h.state_mut();
    let e = m.edit.as_ref().unwrap();
    let at = nif::edit::anchor_role(&e.model, Some(a), Role::Shader).unwrap();
    m.edit_push(
        vec![Op::SetField {
            at,
            field: Field::EmissiveColor(0),
            value: Value::F32(1.0f32.to_bits()),
            base: Value::F32(0.0f32.to_bits()),
        }],
        None,
    );
    let e = m.edit.as_ref().unwrap();
    assert!(e.warnings.iter().any(|w| matches!(
        w.target,
        Target::Field {
            field: Field::EmissiveColor(0),
            ..
        }
    )));
    h.run_steps(2);
    h.get_by_label("No effect in game: Own_Emit is off");
}

/// Ruling change C4-D7 (replaces `a_shared_alpha_that_cannot_split_is_not_edited`): a shared alpha with a clip
/// controller splits on edit like any shared alpha.
#[test]
fn a_shared_alpha_with_a_clip_controller_splits_on_edit() {
    let mut m = direct(scenes::shared_alpha_with_managed_ctrl());
    let a = select(&mut m, "A");
    let e = m.edit.as_ref().unwrap();
    let b = shape_named(e, "B");
    let (alpha_a, before) = {
        let ab = e.model.shape(b).unwrap().alpha.unwrap();
        (
            e.model.shape(a).unwrap().alpha.unwrap(),
            e.model.alphas[&ab].alpha.flags,
        )
    };
    let mut h = view(m);
    h.run_steps(2);
    h.get_by_label("Alpha tab").click();
    h.run_steps(2);
    assert!(h
        .query_by_label("Shared alpha with a clip controller: splitting needs Phase C.")
        .is_none());
    h.get_by_label("Shared by 2 shapes: editing will split.");
    h.get_by_label("Blend").click();
    h.run_steps(2);
    let m = h.state();
    let got = ops(m);
    assert!(
        matches!(got.as_slice(), [Op::SplitAlpha { .. }, Op::SetBits { .. }]),
        "{got:?}"
    );
    let e = m.edit.as_ref().unwrap();
    assert!(
        e.outcomes.iter().all(|o| matches!(
            o,
            nif::edit::Outcome::Applied | nif::edit::Outcome::ReAnchored { .. }
        )),
        "{:?}",
        e.outcomes
    );
    let ab = e.model.shape(b).unwrap().alpha.unwrap();
    assert_eq!(e.model.alphas[&ab].alpha.flags, before, "B is untouched");
    let aa = e.model.shape(a).unwrap().alpha.unwrap();
    assert_ne!(aa, alpha_a, "A has its own alpha now");
    assert_ne!(e.model.alphas[&aa].alpha.flags, before);
}

/// C4-D7: the Controllers tab offers Split alpha on a clip-shared alpha, and Add stays enabled.
#[test]
fn controllers_tab_offers_split_on_a_clip_shared_alpha() {
    let mut m = direct(scenes::shared_alpha_with_managed_ctrl());
    let a = select(&mut m, "A");
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    h.get_by_label("Shared by 2 shapes: split the alpha to edit its controllers.");
    assert!(!h
        .get_by_label("Add controller to alpha property")
        .accesskit_node()
        .is_disabled());
    h.get_by_label("Split alpha").click();
    h.run_steps(3);
    let m = h.state();
    let got = ops(m);
    assert!(matches!(got.as_slice(), [Op::SplitAlpha { .. }]), "{got:?}");
    let e = m.edit.as_ref().unwrap();
    assert_eq!(e.outcomes, vec![nif::edit::Outcome::Applied]);
    let aa = e.model.shape(a).unwrap().alpha.unwrap();
    assert_eq!(e.model.alphas[&aa].prop.users.len(), 1);
}

/// `shared_alpha_and_set` with shape A's emissive [0.8, 0.6, −0.0] and specular [0.8, 0.6, 0.4]: colours the egui
/// picker's HSV round trip does not give back bit for bit.
fn odd_colours() -> Vec<u8> {
    let b = scenes::shared_alpha_and_set();
    let m = nif::model::read_model(&b).unwrap();
    let a = m
        .shapes
        .iter()
        .find(|s| s.name.as_deref() == Some("A"))
        .unwrap()
        .block;
    let at = nif::edit::anchor_role(&m, Some(a), Role::Shader).unwrap();
    let lp = &m.lighting[&m.shape(a).unwrap().shader.unwrap()]
        .lighting
        .params;
    let mut ops = vec![];
    for (field, now, new) in [
        (
            Field::EmissiveColor as fn(u8) -> Field,
            lp.emissive_color,
            [0.8f32, 0.6, -0.0],
        ),
        (Field::SpecularColor, lp.specular_color, [0.8, 0.6, 0.4]),
    ] {
        for i in 0..3 {
            ops.push(Op::SetField {
                at: at.clone(),
                field: field(i as u8),
                value: Value::F32(new[i].to_bits()),
                base: Value::F32(now[i].to_bits()),
            });
        }
    }
    let r = nif::edit::apply(&b, &ops).unwrap();
    assert!(r.outcomes.iter().all(|o| *o == nif::edit::Outcome::Applied));
    r.bytes
}

/// A17 review: drawing a colour row never records an edit; a real change keeps the other channels' bits.
#[test]
fn drawing_a_colour_records_nothing_and_a_change_keeps_the_other_channels() {
    let mut m = direct(odd_colours());
    let a = select(&mut m, "A");
    let mut h = view(m);
    h.run_steps(8);
    assert!(ops(h.state()).is_empty(), "{:?}", ops(h.state()));
    assert!(!h.state().edit.as_ref().unwrap().history.can_undo());
    h.get_by_label("Specular colour B").focus();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    h.get_by_label("Specular colour B").type_text("0.25");
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
    let e = h.state().edit.as_ref().unwrap();
    assert!(!e.doc.ops.is_empty());
    let l = &e.model.lighting[&e.model.shape(a).unwrap().shader.unwrap()]
        .lighting
        .params;
    let bits = |c: [f32; 3]| c.map(f32::to_bits);
    assert_eq!(bits(l.specular_color), bits([0.8, 0.6, 0.25]));
    assert_eq!(
        bits(l.emissive_color),
        bits([0.8, 0.6, -0.0]),
        "emissive untouched"
    );
}

/// A17 review: every control carries its name and its own role (the row names are painted).
#[test]
fn edit_controls_have_names_and_roles() {
    let mut m = direct(scenes::shared_alpha_and_set());
    select(&mut m, "A");
    let mut h = view(m);
    h.run_steps(2);
    h.get_by_role_and_label(AkRole::ComboBox, "Shader type");
    h.get_by_role_and_label(AkRole::ColorWell, "Specular colour");
    h.get_by_role_and_label(AkRole::TextInput, "Diffuse path (slot 0)");
    h.get_by_label("Alpha tab").click();
    h.run_steps(2);
    for l in ["Source", "Destination", "Function"] {
        h.get_by_role_and_label(AkRole::ComboBox, l);
    }
    h.get_by_role_and_label(AkRole::Slider, "Threshold");
}

/// A colour change is three channel ops; a drag of the picker merges them as three ops with the first bases.
#[test]
fn a_colour_drag_is_three_ops_with_the_original_bases() {
    let mut m = direct(scenes::shared_alpha_and_set());
    let a = select(&mut m, "A");
    let e = m.edit.as_ref().unwrap();
    let at = nif::edit::anchor_role(&e.model, Some(a), Role::Shader).unwrap();
    let push = |m: &mut MeshesModel, v: f32, base: f32| {
        let ops = (0..3)
            .map(|i| Op::SetField {
                at: at.clone(),
                field: Field::SpecularColor(i),
                value: Value::F32(v.to_bits()),
                base: Value::F32(base.to_bits()),
            })
            .collect();
        m.edit_push(ops, Some("Color|x|Specular colour".into()));
    };
    push(&mut m, 0.5, 1.0);
    push(&mut m, 0.25, 0.5);
    let got = ops(&m);
    assert_eq!(got.len(), 3, "{got:?}");
    for op in &got {
        let Op::SetField { value, base, .. } = op else {
            panic!("{op:?}")
        };
        assert_eq!(*value, Value::F32(0.25f32.to_bits()));
        assert_eq!(*base, Value::F32(1.0f32.to_bits()), "the drag's first base");
    }
    assert!(m.undo_edit());
    assert!(ops(&m).is_empty(), "one undo step");
}

/// Layout v2: the edit panel sits beside the 3D view under the shared header, above the open bottom panel.
#[test]
fn the_edit_panel_fits_beside_the_3d_view_with_the_header_and_bottom_panel() {
    use egui::containers::panel::PanelState;
    use sdc_gui::app::SdcApp;
    app_data();
    let d = files(false);
    let model = sdc_gui::model::AppModel::new(
        sdc_gui::settings::GuiSettings::default(),
        None,
        support::session(),
    );
    let mut app = SdcApp::new(model);
    app.meshes.procs = sdc_gui::model::meshes::no_procs;
    app.meshes.store_path = Some(d.path().join("scan.db"));
    scan_and_select(&mut app.meshes, d.path());
    app.meshes.open_edit();
    let (shape, _) = lit_shape(app.meshes.edit.as_ref().unwrap());
    app.meshes.select_edit(Selection::Shape(shape));
    app.model.show_tab(sdc_gui::model::bottom::BottomTab::Files);
    let mut frame = eframe::Frame::_new_kittest();
    let (w, hgt) = (1280.0, 800.0);
    let mut h = Harness::builder()
        .with_size(egui::vec2(w, hgt))
        .build_ui_state(
            move |ui, app: &mut SdcApp| eframe::App::ui(app, ui, &mut frame),
            app,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    sdc_gui::ui::header::set_meshes_view(&h.ctx, true);
    h.run_steps(6);
    // The shared header and the open bottom panel are still there.
    for l in ["Save", "Export", "MO2…", "Files"] {
        h.get_by_label(l);
    }
    let panel = |id: &str| {
        PanelState::load(&h.ctx, egui::Id::new(id))
            .unwrap()
            .outer_rect
    };
    let (edit, tree, bottom) = (panel("meshes-edit"), panel("meshes-tree"), panel("bottom"));
    assert!(
        edit.max.x <= w + 0.5,
        "edit panel past the window: {edit:?}"
    );
    assert!(
        tree.max.x <= edit.min.x,
        "tree {tree:?} under edit {edit:?}"
    );
    assert!(
        edit.max.y <= bottom.min.y + 0.5,
        "edit {edit:?} over bottom {bottom:?}"
    );
    let frame3d = h
        .get_by_label(sdc_gui::model::viewport3d::FRAME_LABEL)
        .rect();
    assert!(
        frame3d.max.x <= edit.min.x,
        "3D {frame3d:?} under edit {edit:?}"
    );
    assert!(frame3d.min.x >= tree.max.x);
    assert!(
        frame3d.width() > 100.0 && frame3d.height() > 100.0,
        "{frame3d:?}"
    );
    let folder = h.get_by_label("Shader folder…").rect();
    assert!(
        folder.max.x <= edit.min.x,
        "3D toolbar pushed under the panel"
    );
    for l in [
        "Shader tab",
        "Alpha tab",
        "Controllers tab",
        "Extra data tab",
        "Undo NIF edit",
        "Redo NIF edit",
    ] {
        let r = h.get_by_label(l).rect();
        assert!(
            edit.contains(r.center()),
            "{l} {r:?} outside the panel {edit:?}"
        );
    }
    // Meshes UI T3: Export NIF sits in a footer under the tabs, inside the panel, with its blocked reason.
    let footer = panel("meshes-edit-export");
    let export = h.get(export_nif()).rect();
    let tabs = h.get_by_label("Extra data tab").rect();
    assert!(
        edit.contains(footer.min) && edit.contains(footer.max - egui::vec2(0.5, 0.5)),
        "footer {footer:?} outside {edit:?}"
    );
    assert!(
        footer.contains(export.center()),
        "{export:?} not in {footer:?}"
    );
    assert!(
        export.min.y > tabs.max.y,
        "Export NIF {export:?} above the tabs {tabs:?}"
    );
    // Minor 5: an untouched session is idle, not blocked: weak text, no red.
    h.get_by_label("No edits to export.");
    assert!(h.query_by_label("Blocked: No edits to export.").is_none());
}

// ---- v2 A18: Controllers (scalars) and Extra data tabs ----

use builder::{Nif, W};

/// Rulings B-P28, B-P29: an authored TBC key, decreasing or non-finite key times block export (the `validate_ops`
/// wiring).
#[test]
fn authored_key_errors_block_export() {
    let b = scenes::keyed_pair(scenes::Share::No, [1, 2]);
    let base = nif::keys::read_data(&nif::header::Header::parse(&b).unwrap(), &b, 6).unwrap();
    for (key_type, times, want) in [
        (
            3,
            [0.0, 1.0],
            "Only linear and quadratic keys can be authored",
        ),
        (1, [1.0, 0.0], "Key times must not decrease"),
        (1, [0.0, f32::NAN], "Key values must be finite numbers"),
    ] {
        let d = tempfile::tempdir().unwrap();
        let edit_mod = d.path().join("mods").join(EDIT_MOD);
        let mut e = NifEdit::open(
            b.clone(),
            copy("A", 1, false),
            "meshes/x.nif",
            Some(&edit_mod),
        )
        .unwrap();
        let role = Role::Ctrl {
            owner: nif::edit::Owner::Shader,
            class: "BSLightingShaderPropertyFloatController".into(),
            var: Some(11),
            nth: 0,
        };
        let tbc = (key_type == 3).then_some([0.5, -0.25, 1.0]);
        let op = Op::SetKeys {
            at: nif::edit::anchor_role(&e.model, Some(0), role).unwrap(),
            clip: None,
            clip_name: None,
            keys: KeyData::Float(KeyGroup {
                key_type: Some(key_type),
                keys: times.iter().map(|&t| fkey(t, 1.0, tbc)).collect(),
            }),
            base: base.clone(),
        };
        e.push(vec![op], None);
        assert_eq!(e.export_block().as_deref(), Some(want));
    }
}

/// Shape "S" (0) -> BSLSP (1) -> free float controllers var 11 (2, interp 4) and var 20 (3, interp 5); a root NiNode
/// (12) listing BSXFlags `bsx` (6), a BSInvMarker, a multi-line string, an int, a bool and a negative-zero float.
fn ctrl_scene(bsx: u32) -> Vec<u8> {
    ctrl_scene_vars(bsx, [11, 20])
}

fn ctrl_scene_vars(bsx: u32, vars: [u32; 2]) -> Vec<u8> {
    let mut n = Nif::new(100);
    let (s, r) = (n.string("S"), n.string("Root"));
    n.block("BSTriShape", scenes::bstri_named(s, 1, -1));
    n.block("BSLightingShaderProperty", scenes::lsp_body(0, &[], 2, &[]));
    for (next, var, interp) in [(3, vars[0], 4), (-1, vars[1], 5)] {
        let tail = W::default().i32(interp).u32(var);
        n.block(
            "BSLightingShaderPropertyFloatController",
            scenes::ctrl_body(next, 0x48, 1, tail),
        );
    }
    n.block("NiFloatInterpolator", W::default().f(0.5).i32(-1).0);
    n.block("NiFloatInterpolator", W::default().f(0.25).i32(-1).0);
    let names = ["BSX", "INV", "Tag", "Count", "PG_IGNORE", "Scale"].map(|x| n.string(x));
    let val = n.string("a\r\nb");
    n.block("BSXFlags", W::default().i32(names[0]).u32(bsx).0);
    n.block(
        "BSInvMarker",
        W::default()
            .i32(names[1])
            .u16(4712)
            .u16(6283)
            .u16(0)
            .f(1.0)
            .0,
    );
    n.block("NiStringExtraData", W::default().i32(names[2]).i32(val).0);
    n.block("NiIntegerExtraData", W::default().i32(names[3]).u32(7).0);
    n.block("NiBooleanExtraData", W::default().i32(names[4]).u8(1).0);
    n.block("NiFloatExtraData", W::default().i32(names[5]).f(-0.0).0);
    let mut w = W::default().i32(r).u32(6);
    for e in 6..12 {
        w = w.i32(e);
    }
    n.block("NiNode", scenes::avobj(w.i32(-1)).u32(0).u32(0).0);
    n.roots = vec![12];
    n.bytes()
}

fn tab(h: &mut Harness<'static, MeshesModel>, name: &str) {
    h.get_by_label(&format!("{name} tab")).click();
    h.run_steps(3);
}

/// Meshes UI T4: opens the Controllers tab's "Clips (…)" section (closed by default) unless it is open.
fn open_clips(h: &mut Harness<'static, MeshesModel>) {
    if h.query_by_label("New clip name").is_some() {
        return;
    }
    if h.query_by_label_contains("Clips (").is_none() {
        return;
    }
    // The section ends the tab, often below the fold: scroll it into view before the click.
    h.get_by_label_contains("Clips (").scroll_to_me();
    h.run_steps(12);
    h.get_by_label_contains("Clips (").click();
    h.run_steps(3);
}

fn edit_number(h: &mut Harness<'static, MeshesModel>, label: &str, text: &str) {
    if h.query_by_label(label).is_none() {
        open_clips(h); // Meshes UI T4: the New clip fields sit in the closed Clips section
    }
    h.get_by_label(label).focus();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    h.get_by_label(label).type_text(text);
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
}

#[test]
fn controller_rows_show_variable_cycle_and_edit_frequency() {
    let mut m = direct(ctrl_scene(0x82));
    select(&mut m, "S");
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    h.get_by_label("Emissive multiple");
    h.get_by_label("U offset");
    h.get_by_role_and_label(AkRole::ComboBox, "Emissive multiple cycle");
    h.get_by_role_and_label(AkRole::ComboBox, "Emissive multiple variable");
    h.get_by_role_and_label(AkRole::CheckBox, "Emissive multiple active");
    assert!(ops(h.state()).is_empty(), "drawing records nothing");
    edit_number(&mut h, "Emissive multiple frequency", "2");
    let got = ops(h.state());
    assert_eq!(got.len(), 1, "{got:?}");
    let Op::SetField {
        at,
        field,
        value,
        base,
    } = &got[0]
    else {
        panic!("{got:?}")
    };
    assert_eq!(*field, Field::Frequency);
    assert_eq!(*value, Value::F32(2.0f32.to_bits()));
    assert_eq!(*base, Value::F32(1.0f32.to_bits()));
    assert!(
        matches!(
            &at.role,
            Role::Ctrl {
                var: Some(11),
                nth: 0,
                ..
            }
        ),
        "{at:?}"
    );
    let e = h.state().edit.as_ref().unwrap();
    assert_eq!(e.model.ctrls[&2].frequency, 2.0);
    assert_eq!(e.model.ctrls[&3].frequency, 1.0, "the other row untouched");
    // The interpolator value goes through the controller's anchor.
    edit_number(&mut h, "U offset value", "0.75");
    let e = h.state().edit.as_ref().unwrap();
    assert_eq!(e.model.interps[&5].value, vec![0.75], "{:?}", e.outcomes);
}

/// The managed-clip scene in a direct session with its shape selected, on the Controllers tab (Clips closed).
fn clip_view() -> Harness<'static, MeshesModel> {
    let mut m = direct(scenes::managed_clip());
    let b = m.edit.as_ref().unwrap().model.shapes[0].block;
    m.select_edit(Selection::Shape(b));
    let mut h = view(m);
    h.run_steps(2);
    h.get_by_label("Controllers tab").click();
    h.run_steps(3);
    h
}

/// Meshes UI T4: Preview stays at the top; one closed "Clips (N · showing X)" section at the end holds Rename,
/// Remove, Convert to clips and New clip; a managed controller names its clip in one weak line.
#[test]
fn clip_controls_sit_in_one_closed_section_under_the_chains() {
    let mut h = clip_view();
    let preview = h
        .get_by_role_and_label(AkRole::ComboBox, "Preview animation")
        .rect();
    let shader = h.get_by_label("Shader property").rect();
    assert!(preview.max.y <= shader.min.y, "Preview on top");
    let header = h.get_by_label("Clips (2 · showing free chains)").rect();
    assert!(
        header.min.y > shader.max.y,
        "the section is after the chains"
    );
    for l in [
        "Clip name",
        "New clip name",
        "Convert shader controllers to clips",
    ] {
        assert!(
            h.query_by_label(l).is_none(),
            "{l} is inside the closed section"
        );
    }
    h.get_by_label_contains("In clip A · ");
    h.get_by_label("Clips (2 · showing free chains)").click();
    h.run_steps(3);
    // T4 review Minor 6: the order inside the section, and the Convert row's label.
    let y = |h: &Harness<'static, MeshesModel>, l: &str| h.get_by_label(l).rect().min.y;
    let (rename, convert, new) = (
        y(&h, "Clip name"),
        y(&h, "Convert to clips: shader property"),
        y(&h, "New clip name"),
    );
    assert!(
        rename < convert && convert < new,
        "{rename} {convert} {new}"
    );
    h.get_by_label("Convert shader controllers to clips");
}

/// T4 review Minor 1: a refusal left from an earlier file state does not open the section.
#[test]
fn a_stale_clip_refusal_does_not_open_the_clips_section() {
    let mut h = clip_view();
    let id = egui::Id::new("meshes-edit-clip").with("refused");
    // Revision 0 is never a state's (`next_key` starts at 1).
    h.ctx
        .data_mut(|d| d.insert_temp(id, ("0|None".to_string(), "old".to_string())));
    h.run_steps(3);
    assert!(
        h.query_by_label("Clip name").is_none(),
        "opened on a stale refusal"
    );
}

/// T4 review Minor 2: on a shape with no shader and no alpha the Clips section (Rename, Remove) is still there.
#[test]
fn the_clips_section_shows_on_a_shape_without_properties() {
    let mut m = direct(scenes::managed_clip_and_bare_shape());
    m.select_edit(Selection::Shape(12));
    let mut h = view(m);
    h.run_steps(2);
    h.get_by_label("Controllers tab").click();
    h.run_steps(3);
    h.get_by_label("This shape has no shader or alpha property.");
    h.get_by_label("Clips (2 · showing free chains)").click();
    h.run_steps(3);
    h.get_by_label("Clip name");
}

/// Meshes UI T4: a clip refusal stored while the section is closed opens it, so the red line is seen. The refusal is
/// stored as the Rename row stores it (its id, and inputs = state revision | picked clip).
#[test]
fn a_clip_refusal_opens_the_clips_section() {
    let mut h = clip_view();
    let (rev, a) = {
        let e = h.state().edit.as_ref().unwrap();
        (e.rev, e.model.sequences[0].block)
    };
    h.state_mut().edit_with(|e| e.set_playing(Playing::Clip(a)));
    h.run_steps(3);
    let rev = h.state().edit.as_ref().unwrap().rev.max(rev);
    assert!(h.query_by_label("Clip name").is_none(), "closed");
    let id = egui::Id::new("meshes-edit-clip").with("refused");
    let inputs = format!("{rev}|{:?}", Some(a));
    h.ctx
        .data_mut(|d| d.insert_temp(id, (inputs, "Type a name for the clip.".to_string())));
    h.run_steps(3);
    h.get_by_label("Clip name");
    h.get_by_label("Type a name for the clip.");
    // Closed by hand, it stays closed while that refusal stands.
    // T4 review Minor 6: the title names the clip the Preview shows.
    h.get_by_label("Clips (2 · showing A)").click();
    h.run_steps(3);
    assert!(h.query_by_label("Clip name").is_none());
}

/// R8 lifted (v2 B7, E9: replaces `managed_variable_combo_is_disabled`): the Variable combo of a managed controller
/// is enabled; a pick rewrites the clip's ID.
#[test]
fn managed_variable_change_rewrites_clip_ids() {
    let mut m = MeshesModel::empty();
    m.edit = Some(
        NifEdit::open(
            scenes::managed_clip(),
            copy("A", 1, false),
            "meshes/x.nif",
            None,
        )
        .unwrap(),
    );
    let b = m.edit.as_ref().unwrap().model.shapes[0].block;
    m.select_edit(Selection::Shape(b));
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    h.get_by_label("Managed by a clip");
    let var = h.get_by_role_and_label(AkRole::ComboBox, "Emissive multiple variable");
    assert!(!var.accesskit_node().is_disabled());
    var.click();
    h.run_steps(3);
    h.get_by_role_and_label(AkRole::Button, "U offset (20)")
        .click();
    h.run_steps(3);
    let got = ops(h.state());
    assert!(
        matches!(
            &got[..],
            [Op::SetField {
                field: Field::Variable,
                value: Value::U32(20),
                base: Value::U32(11),
                ..
            }]
        ),
        "{got:?}"
    );
    let e = h.state().edit.as_ref().unwrap();
    assert_eq!(e.outcomes, vec![nif::edit::Outcome::Applied]);
    assert_eq!(
        e.model.sequences[0].blocks[0].ctrl_id.as_deref(),
        Some(&b"20"[..])
    );
    assert_eq!(e.model.sequences[0].blocks[1].ctrl_id, None);
}

#[test]
fn bsx_bits_toggle_and_inv_marker_round_trips() {
    let mut m = direct(ctrl_scene(0x82));
    select(&mut m, "S");
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Extra data");
    // Animated is ticked and locked: a free controller animates this model (D9).
    let animated = h.get_by_role_and_label(AkRole::CheckBox, "Animated");
    assert!(animated.accesskit_node().is_disabled());
    assert_eq!(
        animated.accesskit_node().toggled(),
        Some(egui::accesskit::Toggled::True)
    );
    h.get_by_role_and_label(AkRole::CheckBox, "Havok").click();
    h.run_steps(3);
    let got = ops(h.state());
    assert_eq!(got.len(), 1, "{got:?}");
    let Op::EditExtra { value, base, at } = &got[0] else {
        panic!("{got:?}")
    };
    assert_eq!(*value, ExtraValue::Bsx(0x82 & !2));
    assert_eq!(*base, ExtraValue::Bsx(0x82));
    assert!(
        matches!(&at.role, Role::Extra { on_root: true, .. }),
        "{at:?}"
    );
    edit_number(&mut h, "Rotation X", "90");
    let e = h.state().edit.as_ref().unwrap();
    let inv = e.model.extras.values().find(|x| x.kind == "BSInvMarker");
    let Some(ExtraValue::Inv { rot, zoom }) = inv.and_then(|x| x.value.clone()) else {
        panic!("{inv:?}")
    };
    assert_eq!(rot, [1571, 6283, 0]);
    assert_eq!(zoom, 1.0f32.to_bits());
}

/// Animated is not locked when no property has a free controller, and ticking it writes `old | 1`.
#[test]
fn animated_ticks_when_nothing_is_animated() {
    let mut m = direct(scenes::emit_scene("BSX", 0x82, true));
    m.select_edit(Selection::Root);
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Extra data");
    h.get_by_role_and_label(AkRole::CheckBox, "Animated")
        .click();
    h.run_steps(3);
    let got = ops(h.state());
    let Some(Op::EditExtra { value, .. }) = got.first() else {
        panic!("{got:?}")
    };
    assert_eq!(*value, ExtraValue::Bsx(0x83));
}

#[test]
fn add_and_remove_string_extra_on_a_shape() {
    let mut m = direct(ctrl_scene(0x82));
    let s = select(&mut m, "S");
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Extra data");
    h.get_by_role_and_label(AkRole::ComboBox, "Add type (shape)")
        .click();
    h.run_steps(3);
    h.get_by_role_and_label(AkRole::Button, "NiStringExtraData")
        .click();
    h.run_steps(3);
    h.get_by_role_and_label(AkRole::TextInput, "Name (shape)")
        .focus();
    h.run_steps(2);
    h.get_by_role_and_label(AkRole::TextInput, "Name (shape)")
        .type_text("Note");
    h.run_steps(2);
    h.get_by_label("Add to shape").click();
    h.run_steps(4);
    let e = h.state().edit.as_ref().unwrap();
    let net = &e.model.shape(s).unwrap().net;
    let added = e.model.extras_of(net);
    assert_eq!(added.len(), 1, "{:?}", e.outcomes);
    assert_eq!(added[0].kind, "NiStringExtraData");
    assert_eq!(added[0].name.as_deref(), Some(&b"Note"[..]));
    assert!(matches!(ops(h.state())[0], Op::AddExtra { .. }));
    h.get_by_label("Remove Note (shape)").click();
    h.run_steps(4);
    let e = h.state().edit.as_ref().unwrap();
    assert!(e.model.extras_of(&e.model.shape(s).unwrap().net).is_empty());
    assert!(matches!(
        ops(h.state()).last(),
        Some(Op::RemoveExtra { .. })
    ));
    // The root's extras were never touched.
    assert_eq!(
        e.model.extras_of(e.model.root_net.as_ref().unwrap()).len(),
        6
    );
}

/// Opens the Known picker of the shape (`root` false) or root list and clicks `entry`.
fn pick_known(h: &mut Harness<'static, MeshesModel>, root: bool, entry: &str) {
    let combo = if root {
        "Known extra data"
    } else {
        "Known extra data (shape)"
    };
    h.get_by_role_and_label(AkRole::ComboBox, combo)
        .scroll_to_me();
    h.run_steps(12);
    h.get_by_role_and_label(AkRole::ComboBox, combo).click();
    h.run_steps(3);
    h.get_by_role_and_label(AkRole::Button, entry).click();
    h.run_steps(3);
}

fn known_entry_disabled(h: &mut Harness<'static, MeshesModel>, root: bool, entry: &str) -> bool {
    let combo = if root {
        "Known extra data"
    } else {
        "Known extra data (shape)"
    };
    h.get_by_role_and_label(AkRole::ComboBox, combo)
        .scroll_to_me();
    h.run_steps(12);
    h.get_by_role_and_label(AkRole::ComboBox, combo).click();
    h.run_steps(3);
    let off = h
        .get_by_role_and_label(AkRole::Button, entry)
        .accesskit_node()
        .is_disabled();
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    off
}

/// Meshes UI T5: the shape's Known picker adds AnisotropicAlphaMaterial as NiIntegerExtraData 3; root-only entries
/// are off there ("Goes on the root").
#[test]
fn known_picker_adds_anisotropic_alpha_on_a_shape() {
    let mut m = direct(ctrl_scene(0x82));
    let s = select(&mut m, "S");
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Extra data");
    assert!(
        known_entry_disabled(&mut h, false, "BSX"),
        "BSX goes on the root"
    );
    // Owner, 2026-10-22: BODYTRI is not offered (BodySlide / Outfit Studio writes it).
    h.get_by_role_and_label(AkRole::ComboBox, "Known extra data (shape)")
        .click();
    h.run_steps(3);
    assert!(h
        .query_by_role_and_label(AkRole::Button, "BODYTRI")
        .is_none());
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    pick_known(&mut h, false, "AnisotropicAlphaMaterial");
    h.get_by_label("Add to shape").click();
    h.run_steps(4);
    let e = h.state().edit.as_ref().unwrap();
    let added = e.model.extras_of(&e.model.shape(s).unwrap().net);
    assert_eq!(added.len(), 1, "{:?}", e.outcomes);
    assert!(
        matches!(
            &ops(h.state())[0],
            Op::AddExtra { kind, name, value: ExtraValue::Int(3), .. }
                if kind == "NiIntegerExtraData" && name == "AnisotropicAlphaMaterial"
        ),
        "{:?}",
        ops(h.state())
    );
}

/// Meshes UI T5: on the root, BSX and INV are off when present ("Already on this root"); HDT SMP's Add waits for a
/// path; Prn is offered marked unconfirmed (owner decision 1).
#[test]
fn known_picker_on_the_root_offers_hdt_with_a_path_and_marks_prn() {
    let mut m = direct(ctrl_scene(0x82));
    select(&mut m, "S");
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Extra data");
    assert!(
        known_entry_disabled(&mut h, true, "BSX"),
        "BSX is already there"
    );
    assert!(
        known_entry_disabled(&mut h, true, "AnisotropicAlphaMaterial"),
        "goes on a shape"
    );
    assert!(!known_entry_disabled(&mut h, true, "Prn (unconfirmed)"));
    pick_known(&mut h, true, "HDT Skinned Mesh Physics Object");
    assert!(
        h.get_by_label("Add to root").accesskit_node().is_disabled(),
        "needs a path"
    );
    h.get_by_role_and_label(AkRole::TextInput, "Value").focus();
    h.run_steps(2);
    h.get_by_role_and_label(AkRole::TextInput, "Value")
        .type_text(r"SKSE\Plugins\hdtSkinnedMeshConfigs\x.xml");
    h.run_steps(2);
    h.get_by_label("Add to root").click();
    h.run_steps(4);
    let got = ops(h.state());
    assert!(
        matches!(
            &got[..],
            [Op::AddExtra { kind, name, value: ExtraValue::Str(p), .. }]
                if kind == "NiStringExtraData"
                    && name == "HDT Skinned Mesh Physics Object"
                    && &**p == r"SKSE\Plugins\hdtSkinnedMeshConfigs\x.xml"
        ),
        "{got:?}"
    );
}

/// Meshes UI T5 (bug 3): an extra-data row shows its own warnings, here "BSXFlags must be named BSX".
#[test]
fn an_extra_row_shows_its_warnings() {
    let mut m = direct(ctrl_scene(0x82));
    let at = nif::edit::anchor_role(&m.edit.as_ref().unwrap().model, None, Role::Root).unwrap();
    m.edit_push(
        vec![Op::AddExtra {
            at,
            kind: "BSXFlags".into(),
            name: "BSX2".into(),
            value: ExtraValue::Bsx(0),
        }],
        None,
    );
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Extra data");
    // T5 review Minor 6: under its own row (after BSX2, before the root's Add row), once.
    let w = h.get_by_label("BSXFlags must be named BSX").rect();
    let row = h.get_by_label("BSX2").rect();
    let add = h
        .get_by_role_and_label(AkRole::ComboBox, "Known extra data")
        .rect();
    assert!(
        row.max.y <= w.min.y && w.max.y <= add.min.y,
        "{row:?} {w:?} {add:?}"
    );
}

/// T5 review Minors 2-4: Prn needs a value, a typed value is trimmed, and a duplicate is found ignoring ASCII case
/// (the SMP reader compares the HDT name that way).
#[test]
fn known_values_are_required_trimmed_and_duplicates_ignore_case() {
    let mut m = direct(ctrl_scene(0x82));
    select(&mut m, "S");
    let at = nif::edit::anchor_role(&m.edit.as_ref().unwrap().model, None, Role::Root).unwrap();
    m.edit_push(
        vec![Op::AddExtra {
            at,
            kind: "NiStringExtraData".into(),
            name: "hdt skinned mesh physics object".into(),
            value: ExtraValue::Str("x.xml".into()),
        }],
        None,
    );
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Extra data");
    assert!(
        known_entry_disabled(&mut h, true, "HDT Skinned Mesh Physics Object"),
        "the same name in lower case is already there"
    );
    pick_known(&mut h, true, "Prn (unconfirmed)");
    assert!(
        h.get_by_label("Add to root").accesskit_node().is_disabled(),
        "Prn needs a node name"
    );
    h.get_by_role_and_label(AkRole::TextInput, "Value").focus();
    h.run_steps(2);
    h.get_by_role_and_label(AkRole::TextInput, "Value")
        .type_text("  WeaponBack  ");
    h.run_steps(2);
    h.get_by_label("Add to root").click();
    h.run_steps(4);
    let got = ops(h.state());
    assert!(
        matches!(got.last(), Some(Op::AddExtra { name, value: ExtraValue::Str(v), .. }) if name == "Prn" && &**v == "WeaponBack"),
        "{got:?}"
    );
}

/// T5 review Minor 6: after a Known Add the picker is back on Custom… and the entry is off ("Already on this
/// shape"); an existing row of a Known entry shows its tooltip on the name.
#[test]
fn a_known_add_falls_back_to_custom_and_rows_show_the_tip() {
    let mut m = direct(ctrl_scene(0x82));
    select(&mut m, "S");
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Extra data");
    pick_known(&mut h, false, "AnisotropicAlphaMaterial");
    h.get_by_label("Add to shape").click();
    h.run_steps(4);
    assert_eq!(
        h.get_by_role_and_label(AkRole::ComboBox, "Known extra data (shape)")
            .accesskit_node()
            .value()
            .as_deref(),
        Some("Custom…")
    );
    assert!(known_entry_disabled(
        &mut h,
        false,
        "AnisotropicAlphaMaterial"
    ));
    h.get_by_label("AnisotropicAlphaMaterial").hover();
    h.run_steps(30);
    h.get_by_label_contains("Community Shaders see-through look");
}

/// Drawing both tabs records nothing, whatever the values' bits (negative zero, CRLF).
#[test]
fn drawing_the_controller_and_extra_tabs_records_nothing() {
    let mut m = direct(ctrl_scene(0x83));
    select(&mut m, "S");
    let mut h = view(m);
    h.run_steps(2);
    for t in ["Controllers", "Extra data", "Controllers", "Extra data"] {
        tab(&mut h, t);
        h.run_steps(6);
    }
    assert!(ops(h.state()).is_empty(), "{:?}", ops(h.state()));
    assert!(!h.state().edit.as_ref().unwrap().history.can_undo());
}

/// P9: the value of an interpolator that two controllers share is greyed, and no edit reaches it.
#[test]
fn a_shared_interpolator_value_is_greyed() {
    let mut m = direct(scenes::shader_ctrl(true));
    select(&mut m, "S");
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    let v = h.get_by_label("Emissive multiple value");
    assert!(v.accesskit_node().is_disabled());
    assert!(ops(h.state()).is_empty());
    // The controller's own fields stay editable.
    let f = h.get_by_label("Emissive multiple frequency");
    assert!(!f.accesskit_node().is_disabled());
}

/// The name being typed in the Add row belongs to the shape it was typed for.
#[test]
fn the_add_row_name_does_not_follow_to_another_shape() {
    let mut m = direct(scenes::shared_alpha_and_set());
    select(&mut m, "A");
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Extra data");
    let name = |h: &Harness<'static, MeshesModel>| {
        h.get_by_role_and_label(AkRole::TextInput, "Name (shape)")
            .accesskit_node()
            .value()
            .map(|s| s.to_string())
    };
    h.get_by_role_and_label(AkRole::TextInput, "Name (shape)")
        .focus();
    h.run_steps(2);
    h.get_by_role_and_label(AkRole::TextInput, "Name (shape)")
        .type_text("Foo");
    h.run_steps(2);
    assert_eq!(name(&h).as_deref(), Some("BSXFoo"));
    let b = shape_named(h.state().edit.as_ref().unwrap(), "B");
    h.state_mut().select_edit(Selection::Shape(b));
    h.run_steps(3);
    assert_eq!(name(&h).as_deref(), Some("BSX"));
}

// ---- v2 A19: export, placement check, guard and pending list ----

use sdc_gui::model::meshes::export::*;

fn mod_row(name: &str, prio: i32, derived: bool) -> modscan::store::ModRow {
    modscan::store::ModRow {
        id: prio as i64,
        name: name.into(),
        kind: modscan::store::KIND_MOD,
        priority: prio,
        derived,
    }
}

#[test]
fn placement_rules() {
    let mods = [
        mod_row("PGP", 9, true),
        mod_row("B", 7, false),
        mod_row(EDIT_MOD, 5, false),
        mod_row("A", 3, false),
    ];
    let ok = [
        copy("PGP", 9, true),
        copy(EDIT_MOD, 5, false),
        copy("A", 3, false),
    ];
    assert_eq!(placement(&ok, &mods, EDIT_MOD), Placement::Ok);
    let below = [
        copy("PGP", 9, true),
        copy("B", 7, false),
        copy("A", 3, false),
    ];
    assert_eq!(
        placement(&below, &mods, EDIT_MOD),
        Placement::Below("B".into())
    );
    let low_pg = [mod_row("PGP", 4, true), mod_row(EDIT_MOD, 5, false)];
    let c = [copy("PGP", 4, true), copy("A", 3, false)];
    assert_eq!(
        placement(&c, &low_pg, EDIT_MOD),
        Placement::AbovePg("PGP".into())
    );
    assert_eq!(placement(&ok, &mods[..2], EDIT_MOD), Placement::NotEnabled);
    assert_eq!(Placement::Ok.text(EDIT_MOD), "Placement OK.");
    assert_eq!(
        Placement::NotEnabled.text(EDIT_MOD),
        "The edit mod is not enabled: enable 'sdc NIF Edits' in MO2."
    );
    assert_eq!(
        Placement::NotScanned.text(EDIT_MOD),
        "Rescan to check where 'sdc NIF Edits' sits."
    );
    assert_eq!(
        Placement::Below("B".into()).text(EDIT_MOD),
        "The edit mod is below B: your edit loses. Drag 'sdc NIF Edits' below B in MO2."
    );
    assert_eq!(
        Placement::AbovePg("PGP".into()).text(EDIT_MOD),
        "The edit mod is above PGP: PGPatcher output will not include your edit until you drag 'sdc NIF Edits' above PGP in MO2."
    );
}

/// `files(false)` plus A's y.nif, with `sdc NIF Edits` (an empty folder) between PGP and A; scanned, x.nif selected.
fn export_session() -> (tempfile::TempDir, MeshesModel) {
    let d = files_with(false, "+PGP\n+sdc NIF Edits\n+A\n");
    std::fs::create_dir_all(d.path().join("mods").join(EDIT_MOD)).unwrap();
    put(d.path(), "mods/A/meshes/y.nif", None);
    let mut m = MeshesModel::new(Some(d.path().join("scan.db")), MeshesSettings::default());
    m.procs = sdc_gui::model::meshes::no_procs;
    scan_and_select(&mut m, d.path());
    let x = m.nifs.iter().find(|n| n.path.ends_with("x.nif")).unwrap();
    m.select_nif(x.copy_id);
    (d, m)
}

fn exported(d: &tempfile::TempDir) -> std::path::PathBuf {
    d.path()
        .join("mods")
        .join(EDIT_MOD)
        .join("meshes")
        .join("x.nif")
}

/// Sets the first lit shape's glossiness to `v` (one op).
fn gloss(m: &mut MeshesModel, v: f32) {
    let e = m.edit.as_ref().unwrap();
    let (shape, shader) = lit_shape(e);
    let cur = e.model.lighting[&shader].lighting.params.glossiness;
    let op = Op::SetField {
        at: nif::edit::anchor_role(&e.model, Some(shape), Role::Shader).unwrap(),
        field: Field::Glossiness,
        value: Value::F32(v.to_bits()),
        base: Value::F32(cur.to_bits()),
    };
    m.edit_push(vec![op], None);
}

#[test]
fn export_writes_the_patched_nif_and_records_its_hash() {
    let (d, mut m) = export_session();
    m.open_edit();
    assert_eq!(m.error, None);
    assert_eq!(
        m.edit.as_ref().unwrap().placement,
        Some(Placement::Ok),
        "the edit mod sits between A and PGP"
    );
    gloss(&mut m, 12.5);
    assert_eq!(
        m.export_edit(None),
        ExportOutcome::Written {
            rerun_pgpatcher: true
        }
    );
    let e = m.edit.as_ref().unwrap();
    let want = nif::edit::apply(&e.source, &e.doc.ops()).unwrap().bytes;
    assert_ne!(want, *e.source);
    assert_eq!(std::fs::read(exported(&d)).unwrap(), want);
    let saved = load(&e.doc_path).unwrap().unwrap();
    assert_eq!(
        saved.export.sha256.as_deref(),
        Some(sha256_hex(&want).as_str())
    );
    assert!(saved.export.at.is_some());
    assert!(!saved.pending());
    assert!(!e.doc.pending());
}

/// Adds a float controller on the first lit shape's shader (`remove`: and removes it again, leaving three orphans).
fn add_then_remove_ctrl(m: &mut MeshesModel, remove: bool) {
    const LSCF: &str = "BSLightingShaderPropertyFloatController";
    let e = m.edit.as_ref().unwrap();
    let (shape, _) = lit_shape(e);
    let key = |time, value| Key {
        time,
        value,
        forward: None,
        backward: None,
        tbc: None,
    };
    let add = Op::AddController {
        at: nif::edit::anchor_role(&e.model, Some(shape), Role::Shader).unwrap(),
        class: LSCF.into(),
        var: Some(11),
        keys: KeyData::Float(KeyGroup {
            key_type: Some(1),
            keys: vec![key(0.0, 1.0), key(1.0, 2.0)],
        }),
        flags: 0x48,
        freq: 1.0,
        start: 0.0,
        stop: 1.0,
    };
    m.edit_push(vec![add], None);
    if remove {
        let e = m.edit.as_ref().unwrap();
        let (shape, shader) = lit_shape(e);
        let nth = e.model.lighting[&shader]
            .prop
            .ctrls
            .iter()
            .filter(|c| e.model.ctrls[c].class == LSCF && e.model.ctrls[c].var == Some(11))
            .count() as u32
            - 1;
        let role = Role::Ctrl {
            owner: nif::edit::Owner::Shader,
            class: LSCF.into(),
            var: Some(11),
            nth,
        };
        let at = nif::edit::anchor_role(&e.model, Some(shape), role).unwrap();
        m.edit_push(vec![Op::RemoveController { at }], None);
    }
}

#[test]
fn export_drops_orphan_blocks_and_records_the_written_hash() {
    let (d, mut m) = export_session();
    m.open_edit();
    add_then_remove_ctrl(&mut m, true);
    assert_eq!(m.edit.as_ref().unwrap().blocked, None);
    assert!(matches!(m.export_edit(None), ExportOutcome::Written { .. }));
    let e = m.edit.as_ref().unwrap();
    let n = nif::header::Header::parse(&e.source).unwrap().sizes.len();
    let blocks = |b: &[u8]| nif::header::Header::parse(b).unwrap().sizes.len();
    let written = std::fs::read(exported(&d)).unwrap();
    assert!(blocks(&e.patched) >= n + 3, "the session keeps the orphans");
    assert!(
        blocks(&written) <= n + 1,
        "at most the root's new BSXFlags stays"
    );
    assert_eq!(
        written,
        nif::edit::compact(&e.patched, n as u32).unwrap(),
        "the export is the compacted patch"
    );
    assert_eq!(
        load(&e.doc_path).unwrap().unwrap().export.sha256.as_deref(),
        Some(sha256_hex(&written).as_str())
    );
    assert_eq!(e.cleanup_note, None);
    // the recorded hash is the file's: the next export replaces it without asking (and clears an old cleanup line)
    add_then_remove_ctrl(&mut m, false);
    m.edit.as_mut().unwrap().cleanup_note = Some("Exported without cleanup: x.".into());
    assert!(matches!(m.export_edit(None), ExportOutcome::Written { .. }));
    assert_eq!(m.edit.as_ref().unwrap().cleanup_note, None);
    let e = m.edit.as_ref().unwrap();
    let written = std::fs::read(exported(&d)).unwrap();
    assert_eq!(written, nif::edit::compact(&e.patched, n as u32).unwrap());
    assert!(blocks(&written) < blocks(&e.patched));
}

/// Note CQ5: when the cleanup cannot read the file, the export writes the patch as it is and says why.
#[test]
fn export_without_cleanup_writes_the_patch_and_says_why() {
    // scenes::animated(Some(1), 0) plus a source NiFloatData (4) nothing names, whose key count runs past it
    let mut b = builder::Nif::new(100);
    let (r, x, s) = (b.string("Root"), b.string("BSX"), b.string("S"));
    let net = builder::W::default().i32(r).u32(1).i32(1).i32(-1);
    b.block("NiNode", scenes::avobj(net).u32(1).i32(2).u32(0).0);
    b.block("BSXFlags", builder::W::default().i32(x).u32(1).0);
    b.block("BSTriShape", scenes::bstri_named(s, 3, -1));
    b.block(
        "BSLightingShaderProperty",
        scenes::lsp_body(0, &[], -1, &[]),
    );
    b.block("NiFloatData", builder::W::default().u32(u32::MAX).u32(1).0);
    let src = b.bytes();
    let d = files_with(false, "+PGP\n+sdc NIF Edits\n+A\n");
    std::fs::create_dir_all(d.path().join("mods").join(EDIT_MOD)).unwrap();
    put(d.path(), "mods/A/meshes/x.nif", Some(&src));
    let mut m = MeshesModel::new(Some(d.path().join("scan.db")), MeshesSettings::default());
    m.procs = sdc_gui::model::meshes::no_procs;
    scan_and_select(&mut m, d.path());
    m.open_edit();
    assert_eq!(m.error, None);
    add_then_remove_ctrl(&mut m, true);
    assert!(matches!(m.export_edit(None), ExportOutcome::Written { .. }));
    let e = m.edit.as_ref().unwrap();
    let why = nif::edit::compact(&e.patched, 5).unwrap_err();
    assert_eq!(std::fs::read(exported(&d)).unwrap(), *e.patched);
    assert_eq!(
        load(&e.doc_path).unwrap().unwrap().export.sha256.as_deref(),
        Some(sha256_hex(&e.patched).as_str())
    );
    let line = format!("Exported without cleanup: {why}.");
    assert_eq!(e.cleanup_note.as_deref(), Some(line.as_str()));
    let mut h = view(m);
    h.run_steps(4);
    h.get_by_label(&line);
}

#[test]
fn hand_edited_export_asks_before_overwrite() {
    let (d, mut m) = export_session();
    m.open_edit();
    gloss(&mut m, 12.5);
    assert!(matches!(m.export_edit(None), ExportOutcome::Written { .. }));
    let p = exported(&d);
    std::fs::write(&p, b"junk").unwrap();
    // Overwrite without having been asked is asked first.
    let ExportOutcome::Guard { diff } = m.export_edit(Some(GuardChoice::Overwrite)) else {
        panic!("a hand-edited file asks first")
    };
    assert!(!diff.is_empty());
    assert_eq!(std::fs::read(&p).unwrap(), b"junk");
    assert!(matches!(m.export_edit(None), ExportOutcome::Guard { .. }));
    assert_eq!(
        m.export_edit(Some(GuardChoice::KeepTheirs)),
        ExportOutcome::KeptTheirs
    );
    assert_eq!(std::fs::read(&p).unwrap(), b"junk");
    assert!(matches!(m.export_edit(None), ExportOutcome::Guard { .. }));
    assert!(matches!(
        m.export_edit(Some(GuardChoice::Overwrite)),
        ExportOutcome::Written { .. }
    ));
    let e = m.edit.as_ref().unwrap();
    assert_eq!(std::fs::read(&p).unwrap(), *e.patched);
    // Keep theirs never writes, even when the file is gone by the time it is answered.
    std::fs::remove_file(&p).unwrap();
    assert_eq!(
        m.export_edit(Some(GuardChoice::KeepTheirs)),
        ExportOutcome::KeptTheirs
    );
    assert!(!p.exists());
}

#[test]
fn conflicting_doc_blocks_export() {
    let (d, mut m) = export_session();
    m.open_edit();
    gloss(&mut m, 12.5);
    // The source's own glossiness changes on disk to another value.
    let e = m.edit.as_ref().unwrap();
    let (shape, shader) = lit_shape(e);
    let src = nif::model::read_model(&e.source).unwrap();
    let cur = src.lighting[&shader].lighting.params.glossiness;
    assert_ne!(cur, 7.0);
    let theirs = nif::edit::apply(
        &e.source,
        &[Op::SetField {
            at: nif::edit::anchor_role(&e.model, Some(shape), Role::Shader).unwrap(),
            field: Field::Glossiness,
            value: Value::F32(7.0f32.to_bits()),
            base: Value::F32(cur.to_bits()),
        }],
    )
    .unwrap()
    .bytes;
    assert_ne!(theirs, *e.source);
    std::fs::write(d.path().join("mods/A/meshes/x.nif"), &theirs).unwrap();
    assert_eq!(
        m.export_edit(None),
        ExportOutcome::Blocked("Resolve the conflicts first.".into())
    );
    assert!(!exported(&d).exists());
    let e = m.edit.as_ref().unwrap();
    assert_eq!(*e.source, theirs);
    assert!(e.doc.has_conflicts());
}

#[test]
fn a_refused_edit_blocks_export() {
    let (d, mut m) = export_session();
    m.open_edit();
    let e = m.edit.as_ref().unwrap();
    let (shape, shader) = lit_shape(e);
    let set = e.model.lighting[&shader].texture_set.unwrap();
    let base = String::from_utf8_lossy(&e.model.texsets[&set].raw.slots[0]).into_owned();
    let op = Op::SetTexturePath {
        at: nif::edit::anchor_shape(&e.model, shape),
        slot: 0,
        path: "textures\\\u{4e2d}.dds".into(),
        base,
    };
    m.edit_push(vec![op.clone()], None);
    assert!(matches!(
        m.edit.as_ref().unwrap().outcomes[0],
        nif::edit::Outcome::Refused(_)
    ));
    assert_eq!(
        m.export_edit(None),
        ExportOutcome::Blocked("1 edit cannot be applied.".into())
    );
    m.edit_push(vec![op], None);
    assert_eq!(
        m.export_edit(None),
        ExportOutcome::Blocked("2 edits cannot be applied.".into())
    );
    assert!(!exported(&d).exists());
}

#[test]
fn export_refuses_bytes_that_do_not_read_back() {
    let (d, mut m) = export_session();
    m.open_edit();
    gloss(&mut m, 12.5);
    let e = m.edit.as_mut().unwrap();
    let mut bad = (*e.patched).clone();
    bad.truncate(bad.len() - 3);
    e.patched = std::sync::Arc::new(bad);
    let out = m.export_edit(None);
    assert!(
        matches!(&out, ExportOutcome::Blocked(s) if s.starts_with("The written file would not read back: ")),
        "{out:?}"
    );
    assert!(!exported(&d).exists());
}

#[test]
fn an_unreadable_export_file_blocks_and_writes_nothing() {
    let (d, mut m) = export_session();
    m.open_edit();
    gloss(&mut m, 12.5);
    std::fs::create_dir_all(exported(&d)).unwrap();
    for choice in [None, Some(GuardChoice::Overwrite)] {
        let out = m.export_edit(choice);
        assert!(
            matches!(&out, ExportOutcome::Blocked(s) if s.starts_with("Cannot read ")),
            "{out:?}"
        );
    }
    assert!(exported(&d).is_dir());
    let doc = load(&m.edit.as_ref().unwrap().doc_path).unwrap().unwrap();
    assert!(doc.export.sha256.is_none());
}

fn badge_of(m: &MeshesModel, name: &str) -> Option<DocBadge> {
    m.pending_docs()
        .into_iter()
        .find(|(k, _)| k.ends_with(name))
        .map(|(_, b)| b)
}

#[test]
fn pending_list_and_chips() {
    let (d, mut m) = export_session();
    let y = m.nifs.iter().find(|n| n.path.ends_with("y.nif")).unwrap();
    m.select_nif(y.copy_id);
    m.open_edit();
    gloss(&mut m, 30.5);
    let x = m.nifs.iter().find(|n| n.path.ends_with("x.nif")).unwrap();
    m.select_nif(x.copy_id);
    m.open_edit();
    gloss(&mut m, 12.5);
    assert!(matches!(m.export_edit(None), ExportOutcome::Written { .. }));
    assert_eq!(badge_of(&m, "x.nif"), Some(DocBadge::Exported));
    assert_eq!(badge_of(&m, "y.nif"), Some(DocBadge::Pending));
    // A new session on the same store reads the chips from the edit documents, without a scan or an open edit.
    let mut fresh = MeshesModel::new(
        Some(d.path().join("scan.db")),
        MeshesSettings {
            instance: Some(d.path().to_path_buf()),
            profile: Some("Default".into()),
            folder: None,
        },
    );
    fresh.procs = sdc_gui::model::meshes::no_procs;
    fresh.activate_with(None);
    let chips = |k: &str| {
        fresh
            .doc_badges
            .iter()
            .find(|(p, _)| p.ends_with(k))
            .map(|(_, b)| *b)
    };
    assert_eq!(chips("x.nif"), Some(DocBadge::Exported));
    assert_eq!(chips("y.nif"), Some(DocBadge::Pending));
    // Meshes UI T2: the column is locked on the shapes while Edit is open; the chips are in the NIF list.
    m.close_edit();
    let mut h = view(m);
    h.run_steps(4);
    h.get_by_label("Edited");
    h.get_by_label("Edit pending");
    assert!(h.query_by_label("Edit conflict").is_none());
}

#[test]
fn undo_after_export_makes_it_pending_again() {
    let (d, mut m) = export_session();
    m.open_edit();
    gloss(&mut m, 12.5);
    assert!(matches!(m.export_edit(None), ExportOutcome::Written { .. }));
    assert_eq!(badge_of(&m, "x.nif"), Some(DocBadge::Exported));
    assert!(m.undo_edit());
    let e = m.edit.as_ref().unwrap();
    assert!(e.doc.pending(), "the exported file still has the edit");
    assert!(
        e.doc.export.sha256.is_some(),
        "undo keeps the export record"
    );
    assert_eq!(badge_of(&m, "x.nif"), Some(DocBadge::Pending));
    // Our own file is replaced without asking.
    assert!(matches!(m.export_edit(None), ExportOutcome::Written { .. }));
    let e = m.edit.as_ref().unwrap();
    assert_eq!(std::fs::read(exported(&d)).unwrap(), *e.source);
}

#[test]
fn export_button_and_guard_dialog_enter_keeps_theirs() {
    let (d, mut m) = export_session();
    m.open_edit();
    gloss(&mut m, 12.5);
    let Op::SetField { at, .. } = &ops(&m)[0] else {
        unreachable!()
    };
    let revert = format!("Revert 1: {}: Glossiness", at.shape.as_deref().unwrap());
    let mut h = view(m);
    h.run_steps(3);
    h.get_by_label(&writes_line(&d));
    h.get_by_label("Placement OK.");
    // Meshes UI T3: the pending list is closed while no row is red.
    assert!(h.query_by_label(&revert).is_none());
    h.get_by_label("Pending edits (1)").click();
    h.run_steps(3);
    h.get_by_role_and_label(AkRole::Button, &revert);
    h.get_by_label("Ready");
    h.get_by_label("Export NIF · 1 edit");
    h.get(export_nif()).click();
    h.run_steps(3);
    let p = exported(&d);
    assert!(p.is_file(), "the click exported");
    h.get_by_label("Re-run PGPatcher so its output includes this edit.");
    h.get_by_label("Export NIF again");
    std::fs::write(&p, b"junk").unwrap();
    h.get(export_nif()).click();
    h.run_steps(3);
    h.get_by_label("This file changed since sdc exported it.");
    h.get_by_role_and_label(AkRole::Button, "Diff").click();
    h.run_steps(3);
    h.get_by_role_and_label(AkRole::Button, "Overwrite");
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert!(h
        .query_by_label("This file changed since sdc exported it.")
        .is_none());
    assert_eq!(std::fs::read(&p).unwrap(), b"junk", "Enter keeps theirs");
    assert!(h.state().edit.as_ref().unwrap().guard.is_none());
}

/// Meshes UI T3: the Export NIF button; its name is its text ("Export NIF · 1 edit", "Export NIF again").
fn export_nif() -> egui_kittest::kittest::By<'static> {
    egui_kittest::kittest::By::new()
        .role(AkRole::Button)
        .label_contains("Export NIF")
}

/// Meshes UI T3: the export block's destination line ("Writes … to …"), whole in its accessible name.
fn writes_line(d: &tempfile::TempDir) -> String {
    format!("Writes meshes\\x.nif to {}", exported(d).display())
}

/// Meshes UI T3 (bug 2): Enter or Space on a focused Export NIF or Overwrite writes nothing; a click still does.
#[test]
fn enter_or_space_on_focused_export_or_overwrite_writes_nothing() {
    let (d, mut m) = export_session();
    m.open_edit();
    gloss(&mut m, 12.5);
    let mut h = view(m);
    h.run_steps(3);
    let p = exported(&d);
    for key in [egui::Key::Enter, egui::Key::Space] {
        h.get(export_nif()).focus();
        h.run_steps(2);
        h.key_press(key);
        h.run_steps(3);
        assert!(!p.exists(), "{key:?} on a focused Export NIF exported");
    }
    h.get(export_nif()).click();
    h.run_steps(3);
    assert!(p.is_file(), "the click exported");
    std::fs::write(&p, b"junk").unwrap();
    h.get(export_nif()).click();
    h.run_steps(3);
    for key in [egui::Key::Enter, egui::Key::Space] {
        h.get_by_role_and_label(AkRole::Button, "Overwrite").focus();
        h.run_steps(2);
        h.key_press(key);
        h.run_steps(3);
        assert_eq!(std::fs::read(&p).unwrap(), b"junk", "{key:?} overwrote");
        assert!(
            h.state().edit.as_ref().unwrap().guard.is_some(),
            "{key:?} closed the guard"
        );
    }
    h.get_by_role_and_label(AkRole::Button, "Overwrite").click();
    h.run_steps(3);
    assert_ne!(std::fs::read(&p).unwrap(), b"junk", "the click overwrote");
}

/// A pending-list row (a label; its Revert button carries the same words).
fn row(text: &str) -> egui_kittest::kittest::By<'_> {
    egui_kittest::kittest::By::new()
        .role(AkRole::Label)
        .label_contains(text)
}

/// Meshes UI T3: a red row opens "Pending edits (N)" by itself, the block says why it is blocked, and the row is
/// named in words, not as a Rust field (`Slsf1`).
#[test]
fn a_red_row_opens_the_pending_list_and_rows_read_as_words() {
    let (_d, mut m) = export_session();
    m.open_edit();
    let e = m.edit.as_ref().unwrap();
    let (shape, shader) = lit_shape(e);
    let f1 = e.model.lighting[&shader].lighting.slsf1;
    let at = nif::edit::anchor_role(&e.model, Some(shape), Role::Shader).unwrap();
    let set = e.model.lighting[&shader].texture_set.unwrap();
    let base = String::from_utf8_lossy(&e.model.texsets[&set].raw.slots[0]).into_owned();
    let on_shape = nif::edit::anchor_shape(&e.model, shape);
    m.edit_push(
        vec![Op::SetBits {
            at,
            field: Field::Slsf1,
            mask: 1 << 12,
            value: !f1 & (1 << 12),
            base: f1 & (1 << 12),
        }],
        None,
    );
    // A path outside Windows-1252 is refused: a red row.
    m.edit_push(
        vec![Op::SetTexturePath {
            at: on_shape,
            slot: 0,
            path: "textures\\\u{4e2d}.dds".into(),
            base,
        }],
        None,
    );
    let mut h = view(m);
    h.run_steps(3);
    h.get_by_label("Pending edits (2)");
    h.get_by_label("Blocked: 1 edit cannot be applied.");
    h.get(row(": SLSF1 flags"));
    assert!(h.query_by_label_contains("Slsf1").is_none());
    h.get_by_label_contains(": Texture slot 0: ");
    // Closed by hand, Show opens it again.
    h.get_by_label("Pending edits (2)").click();
    h.run_steps(3);
    assert!(h.query(row(": SLSF1 flags")).is_none());
    h.get_by_role_and_label(AkRole::Button, "Show pending edits")
        .click();
    h.run_steps(3);
    h.get(row(": SLSF1 flags"));
}

/// Meshes UI T3: without an export folder the block stays and says why it cannot export.
#[test]
fn a_read_only_session_shows_the_no_export_folder_reason() {
    let m = direct(scenes::shared_alpha_and_set());
    let mut h = view(m);
    h.run_steps(3);
    h.get_by_label(&format!("Blocked: {NO_EXPORT_FOLDER}"));
    assert!(h.get(export_nif()).accesskit_node().is_disabled());
}

// ---- v2 A19 review ----

#[test]
fn a_foreign_file_sdc_never_exported_has_its_own_guard_title() {
    let (d, mut m) = export_session();
    m.open_edit();
    gloss(&mut m, 12.5);
    let p = exported(&d);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, b"theirs").unwrap();
    assert!(matches!(m.export_edit(None), ExportOutcome::Guard { .. }));
    let mut h = view(m);
    h.run_steps(3);
    h.get_by_label("A file sdc did not write is already at this path.");
    assert!(h
        .query_by_label("This file changed since sdc exported it.")
        .is_none());
    h.get_by_role_and_label(AkRole::Button, "Overwrite");
    h.get_by_role_and_label(AkRole::Button, "Keep theirs");
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert_eq!(std::fs::read(&p).unwrap(), b"theirs");
}

#[test]
fn pending_walk_does_not_follow_directory_links() {
    let d = tempfile::tempdir().unwrap();
    let edits = d.path().join(".sdc").join("nif-edits");
    let sub = edits.join("meshes");
    std::fs::create_dir_all(&sub).unwrap();
    let mut doc = doc();
    doc.ops.push(DocOp {
        op: sample_op(),
        status: OpStatus::Applied,
    });
    save(&sub.join("x.nif.sdc-nifedit.toml"), &doc).unwrap();
    // A junction back to its own parent: a walk that follows it never ends (or lists the doc twice).
    let link = sub.join("loop");
    let ok = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(&edits)
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok && link.is_dir(), "the junction was made");
    let found = pending_docs(d.path());
    assert_eq!(found, vec![("meshes/x.nif".to_string(), DocBadge::Pending)]);
    std::fs::remove_dir(&link).unwrap(); // the junction only, never its target
}

#[test]
fn a_path_typed_without_enter_is_exported_by_the_export_click() {
    let (d, mut m) = export_session();
    m.open_edit();
    let (shape, _) = lit_shape(m.edit.as_ref().unwrap());
    m.select_edit(Selection::Shape(shape));
    gloss(&mut m, 12.5);
    let mut h = view(m);
    h.run_steps(3);
    let field = "Diffuse path (slot 0)";
    h.get_by_role_and_label(AkRole::TextInput, field).focus();
    h.run_steps(1);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    h.get_by_role_and_label(AkRole::TextInput, field)
        .type_text("textures\\typed.dds");
    h.run_steps(1);
    h.get(export_nif()).click();
    h.run_steps(4);
    let bytes = std::fs::read(exported(&d)).expect("exported");
    let needle = b"textures\\typed.dds";
    assert!(
        bytes.windows(needle.len()).any(|w| w == needle),
        "the typed path is in the exported file"
    );
    let e = h.state().edit.as_ref().unwrap();
    assert!(!e.doc.pending(), "the export includes the typed edit");
}

// ---- v2 A20: the shared Undo / Redo act on the NIF history while an edit is open in the Meshes view ----

/// One app-data redirect for every `SdcApp` test of this binary (as meshes_header.rs): the variables are process-wide,
/// so a test's own temp dir could be deleted under another test.
fn app_data() {
    static D: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    D.get_or_init(|| {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("appdata");
        std::fs::create_dir_all(&a).unwrap();
        std::env::set_var("LOCALAPPDATA", &a);
        std::env::set_var("APPDATA", &a);
        d
    });
}

type AppH = Harness<'static, sdc_gui::app::SdcApp>;

/// The app in the Meshes view with a Textures history step and an open NIF edit with `ops` NIF undo steps.
fn both_histories(ops: usize) -> (tempfile::TempDir, tempfile::TempDir, AppH) {
    app_data();
    let t = tempfile::tempdir().unwrap();
    let model = support::pbr_opened(&t.path().join("mod"));
    assert!(model.can_undo() && !model.can_redo());
    let (d, h) = in_the_meshes_view(model, ops);
    (t, d, h)
}

fn in_the_meshes_view(model: sdc_gui::model::AppModel, ops: usize) -> (tempfile::TempDir, AppH) {
    let d = files(false);
    let mut app = sdc_gui::app::SdcApp::new(model);
    app.meshes.procs = sdc_gui::model::meshes::no_procs;
    app.meshes.store_path = Some(d.path().join("scan.db"));
    scan_and_select(&mut app.meshes, d.path());
    // As the app's first frame sets it: the edit opens in the Textures export folder (Note CI29).
    let root = (app.model.settings.export_folder.is_some() || app.model.root.is_some())
        .then(|| app.model.export_root());
    app.meshes.set_nif_export_root(root);
    app.meshes.open_edit();
    assert_eq!(app.meshes.error, None);
    for i in 0..ops {
        tick(&mut app.meshes, 40.5 + i as f32);
        app.meshes.edit.as_mut().unwrap().flush();
    }
    let mut frame = eframe::Frame::_new_kittest();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(
            move |ui, app: &mut sdc_gui::app::SdcApp| eframe::App::ui(app, ui, &mut frame),
            app,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    sdc_gui::ui::header::set_meshes_view(&h.ctx, true);
    h.run_steps(4);
    (d, h)
}

/// Beyond the brief's list: with no Textures history, Ctrl+Z and the header still undo the NIF step.
#[test]
fn ctrl_z_undoes_a_nif_edit_with_an_empty_textures_history() {
    app_data();
    let t = tempfile::tempdir().unwrap();
    let model = support::opened(&t.path().join("mod"));
    assert!(!model.can_undo());
    let (_d, mut h) = in_the_meshes_view(model, 2);
    assert!(!header_button(&h, "Undo").accesskit_node().is_disabled());
    press(&mut h, egui::Modifiers::COMMAND, egui::Key::Z);
    assert_eq!(nif(&h), (1, true));
    header_button(&h, "Undo").click();
    h.run_steps(4);
    assert_eq!(nif(&h), (0, true));
    assert!(!h.state().model.can_undo() && !h.state().model.can_redo());
}

/// (NIF ops, NIF can redo).
fn nif(h: &AppH) -> (usize, bool) {
    let e = h.state().meshes.edit.as_ref().unwrap();
    (e.doc.ops.len(), e.history.can_redo())
}

/// (can undo, can redo, the undo label) of the Textures history.
fn textures(h: &AppH) -> (bool, bool, Option<String>) {
    let m = &h.state().model;
    (m.can_undo(), m.can_redo(), m.undo_label().map(String::from))
}

fn header_button<'a>(h: &'a AppH, name: &'a str) -> egui_kittest::Node<'a> {
    h.get_by_role_and_label(AkRole::Button, name)
}

fn press(h: &mut AppH, mods: egui::Modifiers, key: egui::Key) {
    h.key_press_modifiers(mods, key);
    h.run_steps(4);
}

/// Closes the edit: the shared Undo / Redo go back to the Textures history.
fn close(h: &mut AppH) {
    h.state_mut().meshes.close_edit();
    h.run_steps(4);
}

#[test]
fn header_undo_follows_the_nif_history_while_editing() {
    let (_t, _d, mut h) = both_histories(1);
    let tex = textures(&h);
    header_button(&h, "Undo").click();
    h.run_steps(4);
    assert_eq!(nif(&h), (0, true), "the NIF op is undone");
    assert_eq!(textures(&h), tex, "the Textures history is untouched");
    header_button(&h, "Redo").click();
    h.run_steps(4);
    assert_eq!(nif(&h), (1, false));
    assert_eq!(textures(&h), tex);
    close(&mut h);
    header_button(&h, "Undo").click();
    h.run_steps(4);
    assert!(
        h.state().model.can_redo(),
        "without an edit Undo is the Textures one"
    );
}

#[test]
fn ctrl_z_and_ctrl_y_follow_the_nif_history_while_editing() {
    use egui::{Key, Modifiers};
    let (_t, _d, mut h) = both_histories(2);
    let tex = textures(&h);
    press(&mut h, Modifiers::COMMAND, Key::Z);
    assert_eq!(nif(&h), (1, true));
    press(&mut h, Modifiers::COMMAND, Key::Z);
    assert_eq!(nif(&h), (0, true));
    press(&mut h, Modifiers::COMMAND, Key::Y);
    assert_eq!(nif(&h), (1, true));
    press(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    assert_eq!(nif(&h), (2, false));
    assert_eq!(textures(&h), tex, "the Textures history is untouched");
    close(&mut h);
    press(&mut h, Modifiers::COMMAND, Key::Z);
    assert!(
        h.state().model.can_redo(),
        "without an edit Ctrl+Z is the Textures one"
    );
}

/// Ctrl+K, "Undo", Enter.
fn palette_undo(h: &mut AppH) {
    press(h, egui::Modifiers::COMMAND, egui::Key::K);
    h.event(egui::Event::Text("Undo".into()));
    h.run_steps(2);
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
    assert!(
        h.state().model.nav.palette.is_none(),
        "the palette ran a pick"
    );
}

#[test]
fn the_palette_undo_follows_the_nif_history_while_editing() {
    let (_t, _d, mut h) = both_histories(1);
    let tex = textures(&h);
    palette_undo(&mut h);
    assert!(
        sdc_gui::ui::header::meshes_view_active(&h.ctx),
        "the pick was the action, not a texture"
    );
    assert_eq!(nif(&h), (0, true));
    assert_eq!(textures(&h), tex);
    close(&mut h);
    palette_undo(&mut h);
    assert!(h.state().model.can_redo());
}

#[test]
fn header_undo_enabled_state_and_hover_follow_the_nif_history() {
    let (_t, _d, mut h) = both_histories(0);
    assert!(
        header_button(&h, "Undo").accesskit_node().is_disabled(),
        "no NIF step to undo, though the Textures history has one"
    );
    header_button(&h, "Undo").hover();
    h.run_steps(30);
    h.get_by_label("Nothing to undo");
    tick(&mut h.state_mut().meshes, 40.5);
    h.state_mut().meshes.edit.as_mut().unwrap().flush();
    h.run_steps(4);
    assert!(!header_button(&h, "Undo").accesskit_node().is_disabled());
    assert!(header_button(&h, "Redo").accesskit_node().is_disabled());
    header_button(&h, "Undo").hover();
    h.run_steps(30);
    h.get_by_label_contains("Undo the last NIF edit");
    header_button(&h, "Undo").click();
    h.run_steps(4);
    header_button(&h, "Redo").hover();
    h.run_steps(30);
    h.get_by_label_contains("Redo the last NIF edit");
}

/// Review: in the Textures view the header Undo and Ctrl+Z act on Textures although a NIF edit is open.
#[test]
fn the_textures_view_undoes_textures_with_a_nif_edit_open() {
    let (_t, _d, mut h) = both_histories(1);
    sdc_gui::ui::header::set_meshes_view(&h.ctx, false);
    h.run_steps(4);
    header_button(&h, "Undo").click();
    h.run_steps(4);
    assert!(h.state().model.can_redo(), "the header undid Textures");
    assert_eq!(nif(&h), (1, false), "the NIF history is untouched");
    h.state_mut().model.redo();
    h.run_steps(4);
    assert!(!h.state().model.can_redo());
    press(&mut h, egui::Modifiers::COMMAND, egui::Key::Z);
    assert!(h.state().model.can_redo(), "Ctrl+Z undid Textures");
    assert_eq!(nif(&h), (1, false));
}

#[test]
fn ctrl_z_does_nothing_in_a_text_field_a_popup_or_a_dialog() {
    use egui::{Key, Modifiers};
    let (_t, _d, mut h) = both_histories(1);
    // Meshes UI T2: with Edit open the mod filter is hidden (Shapes locked): a texture path field instead.
    let (shape, _) = lit_shape(h.state().meshes.edit.as_ref().unwrap());
    h.state_mut().meshes.select_edit(Selection::Shape(shape));
    h.run_steps(2);
    let tex = textures(&h);
    // A focused text field.
    h.query_all_by_role(AkRole::TextInput)
        .next()
        .expect("a text field")
        .focus();
    h.run_steps(2);
    press(&mut h, Modifiers::COMMAND, Key::Z);
    assert_eq!(nif(&h), (1, false), "not with a text field focused");
    assert_eq!(textures(&h), tex);
    h.key_press(Key::Escape);
    h.run_steps(4);
    // An open popup (the header's Open menu).
    header_button(&h, "Open").click();
    h.run_steps(4);
    assert!(egui::Popup::is_any_open(&h.ctx), "the menu is open");
    press(&mut h, Modifiers::COMMAND, Key::Z);
    assert_eq!(nif(&h), (1, false), "not with a popup open");
    assert_eq!(textures(&h), tex);
    h.key_press(Key::Escape);
    h.run_steps(4);
    // A shared dialog.
    h.state_mut().model.dialog = Some(sdc_gui::model::Dialog::Error("x".into()));
    h.run_steps(4);
    press(&mut h, Modifiers::COMMAND, Key::Z);
    assert_eq!(nif(&h), (1, false), "not with a dialog open");
    assert_eq!(textures(&h), tex);
    // The guard: with nothing in the way, Ctrl+Z undoes the NIF step.
    h.state_mut().model.dialog = None;
    h.run_steps(4);
    assert!(!egui::Popup::is_any_open(&h.ctx));
    press(&mut h, Modifiers::COMMAND, Key::Z);
    assert_eq!(nif(&h), (0, true));
    assert_eq!(textures(&h), tex);
}

/// Sets `mask` bits of SLSF2 on shape A's shader (the edit panel redraws on the next run).
fn set_slsf2(h: &mut Harness<'static, MeshesModel>, a: u32, mask: u32) {
    let m = h.state_mut();
    let e = m.edit.as_ref().unwrap();
    let at = nif::edit::anchor_role(&e.model, Some(a), Role::Shader).unwrap();
    let shader = e.model.shape(a).unwrap().shader.unwrap();
    let base = e.model.lighting[&shader].lighting.slsf2;
    m.edit_push(
        vec![Op::SetBits {
            at,
            field: Field::Slsf2,
            mask,
            value: mask,
            base,
        }],
        None,
    );
    h.run_steps(2);
}

#[test]
fn pbr_shape_labels_slsf2_bits_and_non_pbr_does_not() {
    let mut m = direct(scenes::shared_alpha_and_set());
    let a = select(&mut m, "A");
    let mut h = view(m);
    h.run_steps(2);
    open_flags(&mut h); // Meshes UI T6: the flag lists start closed
    assert!(h.query_all_by_label_contains("PBR:").next().is_none());
    set_slsf2(&mut h, a, (1 << 23) | (1 << 24) | (1 << 25));
    h.get_by_label("PBR: Two-layer coat");
    h.get_by_label("PBR: Inner-layer parallax");
    // The checkbox keeps its name and adds the PBR meaning.
    h.get_by_role_and_label(
        egui::accesskit::Role::CheckBox,
        "Multi_Layer_Parallax (PBR: Two-layer coat)",
    );
}

#[test]
fn slsf2_warning_shows_under_the_flags() {
    let mut m = direct(scenes::shared_alpha_and_set());
    let a = select(&mut m, "A");
    let mut h = view(m);
    h.run_steps(2);
    open_flags(&mut h); // Meshes UI T6: the flag lists start closed
    set_slsf2(&mut h, a, 1 << 12);
    h.get_by_label("Fit_Slope (SLSF2 12) does nothing without the PBR flag.");
}

// ---- v2 P14 (owner, 2026-10-20): the NIF export follows the Textures export folder ----

/// `files_with(false, modlist)` with `mods\MyOut` made before the scan (so a listed MyOut gets its mod row); scanned,
/// x.nif selected, the export root set to `root(instance)`.
fn session_into(
    modlist: &str,
    root: impl Fn(&Path) -> std::path::PathBuf,
) -> (tempfile::TempDir, MeshesModel) {
    let d = files_with(false, modlist);
    std::fs::create_dir_all(d.path().join("mods").join("MyOut")).unwrap();
    let mut m = MeshesModel::new(Some(d.path().join("scan.db")), MeshesSettings::default());
    m.procs = sdc_gui::model::meshes::no_procs;
    scan_and_select(&mut m, d.path());
    m.set_nif_export_root(Some(root(d.path())));
    (d, m)
}

fn nif_doc(root: &Path) -> std::path::PathBuf {
    root.join(r".sdc\nif-edits\meshes\x.nif.sdc-nifedit.toml")
}

#[test]
fn export_goes_to_the_textures_export_folder_and_checks_that_mod() {
    let (d, mut m) = session_into("+PGP\n+MyOut\n+A\n", |r| r.join(r"mods\MyOut"));
    let out = d.path().join(r"mods\MyOut");
    m.open_edit();
    assert_eq!(m.error, None);
    assert_eq!(m.edit.as_ref().unwrap().placement, Some(Placement::Ok));
    gloss(&mut m, 12.5);
    assert!(matches!(m.export_edit(None), ExportOutcome::Written { .. }));
    assert!(out.join(r"meshes\x.nif").is_file());
    assert!(nif_doc(&out).is_file());
    assert!(!d.path().join("mods").join(EDIT_MOD).exists());
    assert_eq!(badge_of(&m, "x.nif"), Some(DocBadge::Exported));
}

#[test]
fn placement_names_the_export_folder_mod() {
    let (_d, mut m) = session_into("+PGP\n+A\n+MyOut\n", |r| r.join(r"mods\MyOut"));
    m.open_edit();
    let p = m.edit.as_ref().unwrap().placement.clone().unwrap();
    assert_eq!(p, Placement::Below("A".into()));
    assert_eq!(
        p.text("MyOut"),
        "The edit mod is below A: your edit loses. Drag 'MyOut' below A in MO2."
    );
}

#[test]
fn without_an_export_folder_the_fallback_mod_is_used() {
    let (d, m) = export_session();
    assert_eq!(m.nif_export_root, None);
    assert_eq!(m.edit_mod(), Some(d.path().join("mods").join(EDIT_MOD)));
}

#[test]
fn an_export_folder_outside_the_instance_is_not_checked_and_still_exports() {
    let o = tempfile::tempdir().unwrap();
    let out = o.path().join("out");
    let (_d, mut m) = session_into("+PGP\n+A\n", |_| out.clone());
    m.open_edit();
    let p = m.edit.as_ref().unwrap().placement.clone();
    assert_eq!(p, Some(Placement::NotInMo2));
    assert_eq!(
        Placement::NotInMo2.text("out"),
        "The export folder is not an MO2 mod here: load order not checked."
    );
    gloss(&mut m, 12.5);
    assert!(matches!(m.export_edit(None), ExportOutcome::Written { .. }));
    assert!(out.join(r"meshes\x.nif").is_file());
    assert!(nif_doc(&out).is_file());
}

#[test]
fn changing_the_export_folder_saves_the_edit_and_opens_that_folders_edit() {
    let (d, mut m) = export_session();
    let out = d.path().join(r"mods\MyOut");
    let old = d.path().join("mods").join(EDIT_MOD);
    // MyOut already has an edit of x.nif (one op, 7.5).
    m.set_nif_export_root(Some(out.clone()));
    m.open_edit();
    gloss(&mut m, 7.5);
    m.close_edit();
    let theirs = std::fs::read(nif_doc(&out)).unwrap();
    // Back to the fallback: an edit there, a shape selected and a drag in progress (not saved yet).
    m.set_nif_export_root(None);
    m.open_edit();
    assert_eq!(m.edit.as_ref().unwrap().doc.ops.len(), 0);
    gloss(&mut m, 12.5);
    let shape = lit_shape(m.edit.as_ref().unwrap()).0;
    m.select_edit(Selection::Shape(shape));
    tick(&mut m, 40.5);
    m.set_nif_export_root(Some(out.clone()));
    let e = m.edit.as_ref().unwrap();
    assert_eq!(e.doc_path, nif_doc(&out));
    assert_eq!(e.export_path, out.join(r"meshes\x.nif"));
    assert_eq!(op_values(&m).len(), 1, "MyOut's edit is loaded");
    assert_eq!(op_values(&m)[0].1, 7.5);
    assert_eq!(
        e.selection,
        Selection::Shape(shape),
        "the selection is kept"
    );
    assert_eq!(
        std::fs::read(nif_doc(&out)).unwrap(),
        theirs,
        "not overwritten"
    );
    let saved = load(&nif_doc(&old)).unwrap().unwrap();
    assert_eq!(
        saved.ops.len(),
        2,
        "the old folder keeps the session's edits and the drag"
    );
    assert_eq!(badge_of(&m, "x.nif"), Some(DocBadge::Pending));
}

#[test]
fn the_destination_line_shows_the_real_path() {
    let (d, mut m) = export_session();
    m.open_edit();
    gloss(&mut m, 12.5);
    let full = writes_line(&d);
    let mut h = view(m);
    h.run_steps(3);
    h.get_by_label(&full);
    h.get_by_label(
        "The source file is never changed. Change the folder with Export to ▾ in the header.",
    );
}

#[test]
fn the_app_passes_the_textures_export_folder_to_the_meshes_view() {
    app_data();
    let t = tempfile::tempdir().unwrap();
    let model = support::opened(&t.path().join("mod"));
    let first = model.export_root();
    let (_d, mut h) = in_the_meshes_view(model, 1);
    assert_eq!(
        h.state().meshes.nif_export_root.as_deref(),
        Some(first.as_path())
    );
    // Export to ▾ changes: the open edit is saved where it was and reopened from the new folder.
    let want = t.path().join("other");
    h.state_mut().model.settings.export_folder = Some(want.clone());
    assert_eq!(h.state().model.export_root(), want);
    h.run_steps(2);
    let m = &h.state().meshes;
    assert_eq!(m.nif_export_root.as_deref(), Some(want.as_path()));
    let e = m.edit.as_ref().unwrap();
    assert!(e.doc_path.starts_with(&want));
    assert!(e.doc.ops.is_empty(), "the new folder has no edit of x.nif");
    assert_eq!(
        load(&nif_doc(&first)).unwrap().unwrap().ops.len(),
        1,
        "saved where it was"
    );
}

// ---- v2 B3: the clock, the per-frame cbuffer path ----

use render3d::{EditKind, ShapeId};
use sdc_gui::model::viewport3d::V3Reply;

/// `bytes` with a free BSLightingShaderPropertyFloatController var 20 (U offset), Loop + Active (0x48), start 0,
/// stop 1, LINEAR keys (0, 0) → (1, 1), put first in the first lit shape's controller chain.
fn with_uv_ctrl(bytes: &[u8]) -> Vec<u8> {
    with_uv_ctrl_timed(bytes, 0x48, [1.0, 0.0, 0.0, 1.0])
}

/// [`with_uv_ctrl`] with the controller's `flags` and (frequency, phase, start, stop).
fn with_uv_ctrl_timed(bytes: &[u8], flags: u16, timing: [f32; 4]) -> Vec<u8> {
    let m = nif::model::read_model(bytes).unwrap();
    let lsp = m
        .shapes
        .iter()
        .find_map(|s| s.shader.filter(|b| m.lighting.contains_key(b)))
        .unwrap();
    let net = m.lighting[&lsp].prop.net.clone();
    let mut doc = nif::write::NifDoc::open(bytes).unwrap();
    let at = net.controller_at() - doc.header().offsets()[lsp as usize];
    let key = |time: f32, value: f32| Key {
        time,
        value,
        forward: None,
        backward: None,
        tbc: None,
    };
    let keys = KeyData::Float(KeyGroup {
        key_type: Some(1),
        keys: vec![key(0.0, 0.0), key(1.0, 1.0)],
    });
    let data = doc
        .append(b"NiFloatData", nif::keys::encode_data(&keys))
        .unwrap();
    let interp = doc
        .append(
            b"NiFloatInterpolator",
            builder::W::default().f(0.0).i32(data as i32).0,
        )
        .unwrap();
    let ctrl = doc
        .append(
            b"BSLightingShaderPropertyFloatController",
            builder::W::default()
                .i32(net.controller)
                .u16(flags)
                .fs(&timing)
                .i32(lsp as i32)
                .i32(interp as i32)
                .u32(20)
                .0,
        )
        .unwrap();
    doc.patch(lsp, at, &(ctrl as i32).to_le_bytes()).unwrap();
    let out = doc.bytes();
    let back = nif::model::read_model(&out).unwrap();
    assert!(back.lighting[&lsp].prop.ctrls.contains(&ctrl));
    assert!(back.ctrls[&ctrl].known, "{:?}", back.ctrls[&ctrl]);
    out
}

/// A scanned session whose x.nif has `with_uv_ctrl`, the edit open, the first load sent and answered.
fn playing_session() -> (tempfile::TempDir, MeshesModel, Receiver<V3Cmd>) {
    session_on(&with_uv_ctrl(&fixture("TestNifFile_Skinned_SE.nif")))
}

/// [`playing_session`] on the given x.nif bytes.
fn session_on(nif: &[u8]) -> (tempfile::TempDir, MeshesModel, Receiver<V3Cmd>) {
    let d = files(false);
    put(d.path(), "mods/A/meshes/x.nif", Some(nif));
    let mut m = MeshesModel::new(Some(d.path().join("scan.db")), MeshesSettings::default());
    m.procs = sdc_gui::model::meshes::no_procs;
    scan_and_select(&mut m, d.path());
    let cmds = attach(&mut m, d.path());
    m.open_edit();
    assert_eq!(m.error, None);
    m.tick_anim(0.0); // makes the clock
    m.v3.request_frame(64, 64);
    let Some(V3Cmd::LoadParsed { id, nif, .. }) = cmds
        .try_iter()
        .find(|c| matches!(c, V3Cmd::LoadParsed { .. }))
    else {
        panic!("the first LoadParsed")
    };
    m.v3.handle(V3Reply::Loaded {
        id,
        report: render3d::LoadReport {
            shapes: nif.shapes.len(),
            ..Default::default()
        },
        bounds: Some(([0.0; 3], 1.0)),
    });
    cmds.try_iter().for_each(drop);
    (d, m, cmds)
}

/// Review Focus 5: 300 playing ticks send only SetMaterial and Render (no load, no shader, feature or deferred
/// command), and every material differs from the static one only in cbuffer fields.
#[test]
fn playback_is_cbuffer_only() {
    let (_d, mut m, cmds) = playing_session();
    let c = m.v3.clock.expect("an animated session has a clock");
    assert_eq!((c.t, c.playing, c.range), (0.0, false, (0.0, 1.0)));
    let animated = m.edit.as_ref().unwrap().frame_materials(0.0);
    assert!(!animated.is_empty());
    let base: std::collections::BTreeMap<ShapeId, nif::material::MaterialState> =
        m.edit.as_ref().unwrap().materials().into_iter().collect();
    m.v3.clock.as_mut().unwrap().play();
    let (mut sets, mut renders) = (vec![], vec![]);
    for _ in 0..300 {
        m.tick_anim(1.0 / 60.0);
        m.v3.request_frame(64, 64);
        for c in cmds.try_iter() {
            match c {
                V3Cmd::SetMaterial { id, material, .. } => sets.push((id, material)),
                V3Cmd::Render { time, .. } => renders.push(time),
                _ => panic!("only SetMaterial and Render during playback"),
            }
        }
    }
    assert_eq!(renders.len(), 300, "one Render per tick");
    assert_eq!(
        sets.len(),
        300 * animated.len(),
        "one SetMaterial per animated shape per tick"
    );
    assert!(
        renders.iter().all(|t| (0.0..=1.0).contains(t)),
        "looping stays in range"
    );
    let mut cbuffer = 0;
    for (id, mat) in &sets {
        let k = render3d::classify(&base[id], mat, &[], false);
        assert!(
            matches!(k, EditKind::Cbuffer | EditKind::Same),
            "{id:?}: {k:?}"
        );
        cbuffer += (k == EditKind::Cbuffer) as usize;
        let mut back = mat.clone();
        back.params.uv_offset[0] = base[id].params.uv_offset[0];
        assert_eq!(&back, &base[id], "only U offset is animated");
    }
    assert!(cbuffer > 0);
    // The 30th tick is t = 0.5 (dt 1/60 < MAX_DT): U offset follows the LINEAR curve.
    assert!((sets[29 * animated.len()].1.params.uv_offset[0] - renders[29]).abs() < 1e-5);
    assert!((renders[29] - 0.5).abs() < 1e-4);
}

#[test]
fn scrub_sets_time_and_pauses() {
    let (_d, mut m, cmds) = playing_session();
    m.v3.clock.as_mut().unwrap().play();
    m.tick_anim(0.02);
    m.v3.request_frame(64, 64);
    cmds.try_iter().for_each(drop);
    m.v3.clock.as_mut().unwrap().scrub(0.25);
    m.tick_anim(0.02);
    let c = m.v3.clock.unwrap();
    assert_eq!(
        (c.t, c.playing),
        (0.25, false),
        "a paused clock does not move"
    );
    m.v3.request_frame(64, 64);
    let got: Vec<V3Cmd> = cmds.try_iter().collect();
    assert!(got.iter().any(
        |c| matches!(c, V3Cmd::SetMaterial { material, .. } if material.params.uv_offset[0] == 0.25)
    ));
    assert!(got
        .iter()
        .any(|c| matches!(c, V3Cmd::Render { time, .. } if *time == 0.25)));
    m.tick_anim(0.02);
    m.v3.request_frame(64, 64);
    assert!(
        cmds.try_recv().is_err(),
        "paused and shown: nothing more is sent"
    );
}

/// F6 and F21: In game pauses the clock and sends nothing; closing the session drops the clock.
#[test]
fn the_clock_pauses_in_game_and_goes_with_the_session() {
    let (_d, mut m, cmds) = playing_session();
    m.v3.clock.as_mut().unwrap().play();
    m.set_view_source(false);
    assert!(m.v3.nif_view, "In game (simulated) is the edit preview too");
    assert!(m.v3.clock.is_some());
    m.tick_anim(0.02);
    assert!(!m.v3.clock.unwrap().playing);
    assert!(!cmds
        .try_iter()
        .any(|c| matches!(c, V3Cmd::SetMaterial { .. })));
    m.close_edit();
    assert_eq!(m.v3.clock, None);
}

/// The controls under the 3D view drive the clock.
#[test]
fn play_and_pause_buttons_drive_the_clock() {
    let (_d, m, _cmds) = playing_session();
    let mut h = view(m);
    h.run_steps(2);
    h.get_by_label("Play").click();
    h.run_steps(3);
    let c = h.state().v3.clock.unwrap();
    assert!(c.playing && c.t > 0.0, "{c:?}");
    h.get_by_label("Pause").click();
    h.run_steps(2);
    assert!(!h.state().v3.clock.unwrap().playing);
    h.get_by_label("Loop");
    h.get_by_role_and_label(AkRole::Slider, "Time");
}

/// Rule 3.1: at 1100, 1280 and 1600 px the clock rows (Play, Loop, Speed, the Time slider and its text) stay
/// inside the centre column.
#[test]
fn the_clock_row_fits_the_centre_column() {
    for w in [1100.0, 1280.0, 1600.0] {
        let (_d, m, _cmds) = playing_session();
        let mut h = Harness::builder()
            .with_size(egui::vec2(w, 800.0))
            .build_ui_state(
                |ui, m: &mut MeshesModel| sdc_gui::ui::meshes::show(ui, m),
                m,
            );
        sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
        h.run_steps(3);
        let rect = |id: &str| {
            egui::containers::panel::PanelState::load(&h.ctx, egui::Id::new(id))
                .unwrap_or_else(|| panic!("{id} is drawn"))
                .outer_rect
        };
        rect("meshes-3d-anim");
        // A panel grows to its content, so the bound is the centre column: between the tree and the edit panel.
        let (lo, hi) = (rect("meshes-tree").max.x, rect("meshes-edit").min.x);
        let mut nodes = h.query_all_by_label("Time").collect::<Vec<_>>();
        nodes.push(h.get_by_label("Play"));
        nodes.push(h.get_by_label("Loop"));
        // v2 B8: the edit-mode toggle above them, too.
        nodes.push(h.get_by_label("In game (simulated)"));
        nodes.push(h.get_by_label("Edited source"));
        let speed: Vec<_> = h.query_all_by_label("Speed").collect();
        assert!(!speed.is_empty(), "the Speed label and its value");
        nodes.extend(speed);
        assert!(nodes.len() >= 4);
        for n in nodes {
            let r = n.rect();
            assert!(
                r.max.x <= hi + 0.5 && r.min.x >= lo - 0.5,
                "{w} px: {r:?} outside {lo}..{hi}"
            );
        }
    }
}

// ---- v2 B3 fix round 1 (B-P33): the clock range is in scene time ----

/// The U offset sent for clock time `t` (scrubbed, then one tick).
fn u_at(m: &mut MeshesModel, cmds: &Receiver<V3Cmd>, t: f32) -> f32 {
    cmds.try_iter().for_each(drop);
    m.v3.clock.as_mut().unwrap().scrub(t);
    m.tick_anim(0.0);
    cmds.try_iter()
        .find_map(|c| match c {
            V3Cmd::SetMaterial { material, .. } => Some(material.params.uv_offset[0]),
            _ => None,
        })
        .expect("a SetMaterial")
}

/// A 0-1 controller at frequency 0.5 takes 2 s of scene time: the clock plays the whole curve.
#[test]
fn half_speed_plays_the_whole_curve() {
    let nif = with_uv_ctrl_timed(
        &fixture("TestNifFile_Skinned_SE.nif"),
        0x48,
        [0.5, 0.0, 0.0, 1.0],
    );
    let (_d, mut m, cmds) = session_on(&nif);
    assert_eq!(m.v3.clock.unwrap().range, (0.0, 2.0));
    assert!((u_at(&mut m, &cmds, 1.9) - 0.95).abs() < 1e-5);
}

/// A frequency-1 Reverse (ping-pong) controller over 0-1 needs 2 s for both legs: the clock reaches the way back.
#[test]
fn ping_pong_reaches_its_reverse_leg() {
    let nif = with_uv_ctrl_timed(
        &fixture("TestNifFile_Skinned_SE.nif"),
        0x48 | (1 << 1),
        [1.0, 0.0, 0.0, 1.0],
    );
    let (_d, mut m, cmds) = session_on(&nif);
    assert_eq!(m.v3.clock.unwrap().range, (0.0, 2.0));
    assert!((u_at(&mut m, &cmds, 0.5) - 0.5).abs() < 1e-5);
    assert!(
        (u_at(&mut m, &cmds, 1.75) - 0.25).abs() < 1e-5,
        "the reverse leg"
    );
}

/// B-P33 Important 2: a controller whose stop is +inf or NaN gets no clock and no slider, and the panel says why.
#[test]
fn invalid_controller_times_turn_playback_off() {
    for stop in [f32::INFINITY, f32::NAN] {
        let nif = with_uv_ctrl_timed(
            &fixture("TestNifFile_Skinned_SE.nif"),
            0x48,
            [1.0, 0.0, 0.0, stop],
        );
        let (_d, m, _cmds) = session_on(&nif);
        assert_eq!(m.v3.clock, None, "stop {stop}");
        assert!(m.edit.as_ref().unwrap().anim_invalid());
        let mut h = view(m);
        h.run_steps(2);
        h.get_by_label(ANIM_INVALID);
        assert!(h.query_by_label("Play").is_none(), "stop {stop}");
        assert_eq!(h.query_all_by_label("Time").count(), 0, "stop {stop}");
        assert_eq!(h.state().v3.clock, None);
    }
}

// ---- v2 B6: the Controllers tab (curve, key table, scrub, add and remove) ----

use nif::keys::encode_data;

fn float_keys(m: &MeshesModel, ctrl: u32) -> KeyGroup<f32> {
    match &m.edit.as_ref().unwrap().keys[&(ctrl, None)] {
        Ok(KeyData::Float(g)) => g.clone(),
        k => panic!("{k:?}"),
    }
}

/// Press on the node labelled `label`, move `dy` px up in three steps, release.
fn drag_up(h: &mut Harness<'static, MeshesModel>, label: &str, dy: f32) {
    h.get_by_label(label).scroll_to_me();
    h.run_steps(2);
    let c = h.get_by_label(label).rect().center();
    drag_from(h, c, dy);
}

/// Press at `c`, move `dy` px up in three steps, release.
fn drag_from(h: &mut Harness<'static, MeshesModel>, c: egui::Pos2, dy: f32) {
    h.hover_at(c);
    h.run_steps(1);
    h.drag_at(c);
    h.run_steps(1);
    for k in 1..=3 {
        h.hover_at(c - egui::vec2(0.0, dy * k as f32 / 3.0)); // the first step is past max_click_dist (6 px)
        h.run_steps(1);
    }
    h.drop_at(c - egui::vec2(0.0, dy));
    h.run_steps(3);
}

/// A handle drag is one SetKeys with the base it started from, one undo step; the release flushes, so the next
/// drag is a second op.
#[test]
fn dragging_a_key_records_one_undo_step() {
    let b = scenes::keyed_pair(scenes::Share::No, [1, 2]); // ctrl 2: var 11, LINEAR (0,0) (1,1), data 6
    let orig = nif::keys::read_data(&nif::header::Header::parse(&b).unwrap(), &b, 6).unwrap();
    let mut m = direct(b);
    select(&mut m, "S");
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    assert!(
        ops(h.state()).is_empty(),
        "drawing the curve records nothing"
    );
    drag_up(&mut h, "Emissive multiple key 2", 30.0);
    let got = ops(h.state());
    assert_eq!(got.len(), 1, "{got:?}");
    let Op::SetKeys {
        clip, keys, base, ..
    } = &got[0]
    else {
        panic!("{got:?}")
    };
    assert_eq!(*clip, None);
    assert_eq!(
        encode_data(base),
        encode_data(&orig),
        "the op keeps the keys the drag started from"
    );
    let g = float_keys(h.state(), 2);
    assert_eq!(
        &KeyData::Float(g.clone()),
        keys,
        "the patched file has the op's keys"
    );
    assert_eq!(
        (g.keys[0].time, g.keys[0].value),
        (0.0, 0.0),
        "the other key is untouched"
    );
    assert_eq!(g.keys[1].time, 1.0, "a vertical drag keeps the time");
    assert!(g.keys[1].value > 1.0, "dragged up: {}", g.keys[1].value);
    assert_eq!(
        h.state().edit.as_ref().unwrap().outcomes,
        vec![nif::edit::Outcome::Applied]
    );
    // The release flushed: a second drag is a second op. (Before any undo: `restore` also ends a drag, so a drag
    // after an undo/redo would be a new op even without the release's flush.)
    drag_up(&mut h, "Emissive multiple key 2", 30.0);
    assert_eq!(ops(h.state()).len(), 2, "{:?}", ops(h.state()));
    // One undo step per drag, back to the source.
    assert!(h.state_mut().undo_edit());
    assert_eq!(ops(h.state()).len(), 1);
    assert!(h.state_mut().undo_edit());
    assert!(ops(h.state()).is_empty());
    assert!(!h.state().edit.as_ref().unwrap().history.can_undo());
}

/// A typed key value is stored with its exact bits; a typed decreasing time is kept (never clamped) and its
/// validate_ops error shows under the controller.
#[test]
fn key_table_edit_sets_exact_value() {
    let mut m = direct(scenes::keyed_pair(scenes::Share::No, [1, 2]));
    select(&mut m, "S");
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    edit_number(&mut h, "Emissive multiple key 2 value", "0.3");
    let got = ops(h.state());
    assert_eq!(got.len(), 1, "{got:?}");
    let g = float_keys(h.state(), 2);
    assert_eq!(g.keys[1].value.to_bits(), 0.3f32.to_bits());
    assert_eq!(g.keys[1].time, 1.0);
    // The bytes on file: data block 6 (unshared, re-emitted in place, B4).
    let e = h.state().edit.as_ref().unwrap();
    let on_file = nif::keys::read_data(
        &nif::header::Header::parse(&e.patched).unwrap(),
        &e.patched,
        6,
    )
    .unwrap();
    assert_eq!(encode_data(&on_file), encode_data(&KeyData::Float(g)));
    // A QUADRATIC tangent (ctrl 3, var 20).
    edit_number(&mut h, "U offset key 1 backward", "-2.5");
    let KeyData::Float(q) = h.state().edit.as_ref().unwrap().keys[&(3, None)]
        .clone()
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(
        q.keys[0].backward.map(f32::to_bits),
        Some((-2.5f32).to_bits())
    );
    // Decreasing time: kept as typed, Error shown, no clamp.
    edit_number(&mut h, "Emissive multiple key 2 time", "-1");
    assert_eq!(float_keys(h.state(), 2).keys[1].time, -1.0);
    h.get_by_label("Key times must not decrease");
    let e = h.state().edit.as_ref().unwrap();
    assert!(
        e.op_warnings
            .iter()
            .any(|w| w.target == Target::Block { block: 2 }),
        "{:?}",
        e.op_warnings
    );
    // 3 typed edits; the time edit also moves the stop to the last key (B-P44) in the same step.
    assert_eq!(ops(h.state()).len(), 4, "each typed edit is its own step");
}

/// D9: Add controller appends a free controller and sets BSX bit 0; the Extra data tab then shows Animated ticked
/// and locked. Remove unlinks it; the bit stays (add-only) and is no longer locked.
#[test]
fn add_controller_ticks_and_locks_bsx_animated() {
    let mut m = direct(scenes::emit_scene("BSX", 0x82, true)); // S -> BSLSP 2, no controllers; root BSX 0x82
    select(&mut m, "S");
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Extra data");
    assert!(!h
        .get_by_role_and_label(AkRole::CheckBox, "Animated")
        .accesskit_node()
        .is_disabled());
    tab(&mut h, "Controllers");
    h.get_by_label("No controllers.");
    h.get_by_role_and_label(AkRole::ComboBox, "Add controller variable")
        .click();
    h.run_steps(3);
    h.get_by_role_and_label(AkRole::Button, "Emissive multiple (11)")
        .click();
    h.run_steps(3);
    h.get_by_label("Add controller to shader property").click();
    h.run_steps(4);
    let got = ops(h.state());
    let e = h.state().edit.as_ref().unwrap();
    let v = e
        .model
        .lighting
        .values()
        .next()
        .unwrap()
        .lighting
        .params
        .emissive_multiple;
    let Some(Op::AddController {
        class,
        var,
        keys,
        flags,
        freq,
        start,
        stop,
        ..
    }) = got.first()
    else {
        panic!("{got:?}")
    };
    assert_eq!(
        (class.as_str(), *var),
        ("BSLightingShaderPropertyFloatController", Some(11))
    );
    assert_eq!((*flags, *freq, *start, *stop), (0x48, 1.0, 0.0, 1.0));
    let KeyData::Float(g) = keys else {
        panic!("{keys:?}")
    };
    assert_eq!(g.key_type, Some(1));
    assert_eq!(
        g.keys.iter().map(|k| (k.time, k.value)).collect::<Vec<_>>(),
        [(0.0, v), (1.0, v)]
    );
    assert_eq!(e.outcomes, vec![nif::edit::Outcome::Applied]);
    assert_eq!(
        e.model.lighting.values().next().unwrap().prop.ctrls.len(),
        1
    );
    let bsx = e
        .model
        .extras
        .values()
        .find(|x| x.kind == "BSXFlags")
        .and_then(|x| x.value.clone());
    assert_eq!(bsx, Some(ExtraValue::Bsx(0x83)));
    tab(&mut h, "Extra data");
    let a = h.get_by_role_and_label(AkRole::CheckBox, "Animated");
    assert!(
        a.accesskit_node().is_disabled(),
        "locked while a free controller exists"
    );
    assert_eq!(
        a.accesskit_node().toggled(),
        Some(egui::accesskit::Toggled::True)
    );
    tab(&mut h, "Controllers");
    h.get_by_label("Remove Emissive multiple controller")
        .click();
    h.run_steps(4);
    assert!(matches!(
        ops(h.state()).last(),
        Some(Op::RemoveController { .. })
    ));
    assert!(h
        .state()
        .edit
        .as_ref()
        .unwrap()
        .model
        .lighting
        .values()
        .next()
        .unwrap()
        .prop
        .ctrls
        .is_empty());
    tab(&mut h, "Extra data");
    let a = h.get_by_role_and_label(AkRole::CheckBox, "Animated");
    assert!(
        !a.accesskit_node().is_disabled(),
        "no free controller: unlocked"
    );
    assert_eq!(
        a.accesskit_node().toggled(),
        Some(egui::accesskit::Toggled::True),
        "D9 is add-only"
    );
}

/// A click on the curve moves the clock (B3) to that time and pauses it; the next frame sends the material at
/// that time and renders with it.
#[test]
fn scrub_moves_the_preview_time() {
    let (_d, mut m, cmds) = playing_session();
    let (shape, _) = lit_shape(m.edit.as_ref().unwrap());
    m.select_edit(Selection::Shape(shape));
    m.v3.clock.as_mut().unwrap().play();
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    h.get_by_label("U offset curve").scroll_to_me();
    h.run_steps(2);
    let r = h.get_by_label("U offset curve").rect();
    // curve.rs PAD = 6 px; the var-20 controller spans [0, 1] (start 0, stop 1, keys at 0 and 1).
    let p = egui::pos2(r.left() + 6.0 + 0.25 * (r.width() - 12.0), r.top() + 10.0);
    cmds.try_iter().for_each(drop);
    h.hover_at(p);
    h.run_steps(1);
    h.drag_at(p);
    h.run_steps(1);
    h.drop_at(p);
    h.run_steps(3);
    let c = h.state().v3.clock.unwrap();
    assert!(!c.playing, "a scrub pauses");
    assert!((c.t - 0.25).abs() < 0.01, "{c:?}");
    let sent: Vec<V3Cmd> = cmds.try_iter().collect();
    assert!(
        sent.iter().any(|x| matches!(x, V3Cmd::SetMaterial { material, .. } if material.params.uv_offset[0] == c.t)),
        "the animated material at the scrubbed time"
    );
    assert!(sent
        .iter()
        .any(|x| matches!(x, V3Cmd::Render { time, .. } if *time == c.t)));
}

/// Removing a playing shape's only controller sends its file material once: the viewport must not keep the last
/// animated frame.
#[test]
fn a_removed_controller_restores_the_file_material() {
    let (_d, mut m, cmds) = playing_session();
    let (shape, lsp) = lit_shape(m.edit.as_ref().unwrap());
    let file_u = m.edit.as_ref().unwrap().model.lighting[&lsp]
        .lighting
        .params
        .uv_offset[0];
    m.v3.clock.as_mut().unwrap().scrub(0.5);
    m.tick_anim(0.0);
    assert!(cmds.try_iter().any(
        |c| matches!(c, V3Cmd::SetMaterial { material, .. } if material.params.uv_offset[0] == 0.5)
    ));
    let e = m.edit.as_ref().unwrap();
    let ctrl = e.model.lighting[&lsp].prop.ctrls[0];
    let c = &e.model.ctrls[&ctrl];
    let at = Anchor {
        block: ctrl,
        kind: c.class.clone(),
        shape: e
            .model
            .shape(shape)
            .and_then(|s| s.name.as_deref().map(str::to_string)),
        role: Role::Ctrl {
            owner: nif::edit::Owner::Shader,
            class: c.class.clone(),
            var: c.var,
            nth: 0,
        },
    };
    m.edit_push(vec![Op::RemoveController { at }], None);
    assert_eq!(
        m.edit.as_ref().unwrap().outcomes.last(),
        Some(&nif::edit::Outcome::Applied)
    );
    cmds.try_iter().for_each(drop);
    m.tick_anim(0.0);
    let sent: Vec<V3Cmd> = cmds.try_iter().collect();
    assert!(
        sent.iter().any(|c| matches!(c, V3Cmd::SetMaterial { id, material, .. } if id.0 == shape && material.params.uv_offset[0] == file_u)),
        "no file material in {} commands (V3Cmd is not Debug)",
        sent.len()
    );
    assert_eq!(m.v3.clock, None, "nothing animated: no clock");
}

// ---- v2 B6 fix round 1 (B-P35) ----

/// Replaces the keys of shape "S"'s float controller `var` (one undo step), before any drawing.
fn set_float_keys(m: &mut MeshesModel, var: u32, keys: KeyData) {
    let e = m.edit.as_ref().unwrap();
    let role = Role::Ctrl {
        owner: nif::edit::Owner::Shader,
        class: "BSLightingShaderPropertyFloatController".into(),
        var: Some(var),
        nth: 0,
    };
    let at = nif::edit::anchor_role(&e.model, Some(shape_named(e, "S")), role).unwrap();
    let base = e.keys[&(at.block, None)].clone().unwrap();
    m.edit_push(
        vec![Op::SetKeys {
            at,
            clip: None,
            clip_name: None,
            keys,
            base,
        }],
        None,
    );
}

fn lin2(a: f32, b: f32) -> KeyData {
    KeyData::Float(KeyGroup {
        key_type: Some(1),
        keys: vec![fkey(0.0, a, None), fkey(1.0, b, None)],
    })
}

/// B-P35 I1: the clip-shared guard is the alpha's (D9); a shared shader with a managed controller still adds.
#[test]
fn a_shared_shader_with_a_managed_controller_keeps_add_enabled() {
    // A and B share BSLSP 3; the added U offset controller has bit 0x20 (managed).
    let b = with_uv_ctrl_timed(
        &scenes::shared_alpha_with_testref_chain(),
        0x68,
        [1.0, 0.0, 0.0, 1.0],
    );
    let mut m = direct(b);
    select(&mut m, "A");
    let e = m.edit.as_ref().unwrap();
    let lsp = e.model.lighting.values().next().unwrap();
    assert!(lsp.prop.users.len() > 1 && lsp.prop.ctrls.iter().any(|&c| e.model.managed(c)));
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    let add = h.get_by_label("Add controller to shader property");
    assert!(!add.accesskit_node().is_disabled());
}

/// B-P35 I2: a tangent handle that projects outside the graph cannot start a drag there.
#[test]
fn a_drag_where_a_tangent_handle_projects_outside_the_graph_changes_no_keys() {
    let mut m = direct(scenes::keyed_pair(scenes::Share::No, [1, 2]));
    select(&mut m, "S");
    let KeyData::Float(mut q) = m.edit.as_ref().unwrap().keys[&(3, None)].clone().unwrap() else {
        panic!()
    };
    q.keys[0].backward = Some(100.0); // ctrl 3: QUADRATIC (0,0) -> (1,1)
    set_float_keys(&mut m, 20, KeyData::Float(q));
    let mut h = view(m);
    // Tall enough that nothing scrolls: an animated scroll_to_me would move the content under the drag.
    h.set_size(egui::vec2(1400.0, 1400.0));
    h.run_steps(2);
    tab(&mut h, "Controllers");
    h.run_steps(2);
    let graph = h.get_by_label("U offset curve").rect();
    let p = h
        .get_by_label("U offset key 1 backward handle")
        .rect()
        .center();
    assert!(
        p.y < graph.top() && p.y > 150.0,
        "the handle projects above the graph, on screen: {p:?} {graph:?}"
    );
    let before = (
        ops(h.state()),
        h.state().edit.as_ref().unwrap().keys.clone(),
    );
    drag_from(&mut h, p, 20.0);
    let e = h.state().edit.as_ref().unwrap();
    assert_eq!(ops(h.state()), before.0, "no op");
    assert_eq!(e.keys, before.1, "no key changed");
}

/// B-P35 I3: a flat curve at 1e8 maps to finite handle positions inside the graph.
#[test]
fn a_flat_curve_at_1e8_has_finite_handles() {
    let mut m = direct(scenes::keyed_pair(scenes::Share::No, [1, 2]));
    select(&mut m, "S");
    set_float_keys(&mut m, 11, lin2(1e8, 1e8));
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    let graph = h.get_by_label("Emissive multiple curve").rect();
    for k in ["Emissive multiple key 1", "Emissive multiple key 2"] {
        let c = h.get_by_label(k).rect().center();
        assert!(
            c.x.is_finite() && c.y.is_finite() && graph.contains(c),
            "{k}: {c:?} in {graph:?}"
        );
    }
}

/// B-P35 I3: keys the graph cannot map (an infinite value) show a line instead of the graph; the table stays.
#[test]
fn an_infinite_key_shows_the_out_of_range_line() {
    let mut m = direct(scenes::keyed_pair(scenes::Share::No, [1, 2]));
    select(&mut m, "S");
    set_float_keys(&mut m, 11, lin2(0.0, f32::INFINITY));
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    h.get_by_label("These keys are out of range for the graph.");
    assert!(h.query_by_label("Emissive multiple curve").is_none());
    h.get_by_label("U offset curve");
    h.get_by_label("Emissive multiple key 2 value");
}

/// Adds an LSCC "Emissive colour" (var 1, LINEAR keys `k1` at 0 and `k2` at 1) to `shape`'s shader as the last
/// op; returns the new controller's block.
fn add_colour_ctrl(m: &mut MeshesModel, shape: u32, k1: [f32; 3], k2: [f32; 3]) -> u32 {
    let e = m.edit.as_ref().unwrap();
    let at = nif::edit::anchor_role(&e.model, Some(shape), Role::Shader).unwrap();
    let key = |time: f32, value: [f32; 3]| Key {
        time,
        value,
        forward: None,
        backward: None,
        tbc: None,
    };
    m.edit_push(
        vec![Op::AddController {
            at: at.clone(),
            class: "BSLightingShaderPropertyColorController".into(),
            var: Some(1),
            keys: KeyData::Pos(KeyGroup {
                key_type: Some(1),
                keys: vec![key(0.0, k1), key(1.0, k2)],
            }),
            flags: 0x48,
            freq: 1.0,
            start: 0.0,
            stop: 1.0,
        }],
        None,
    );
    let e = m.edit.as_ref().unwrap();
    assert!(e.outcomes.iter().all(|o| *o == nif::edit::Outcome::Applied));
    let ctrls = &e.model.lighting[&at.block].prop.ctrls;
    *ctrls.last().unwrap()
}

/// `emit_scene` with an LSCC on S (see `add_colour_ctrl`).
fn colour_scene(k1: [f32; 3], k2: [f32; 3]) -> (MeshesModel, u32) {
    let mut m = direct(scenes::emit_scene("BSX", 0x82, true));
    let s = select(&mut m, "S");
    let ctrl = add_colour_ctrl(&mut m, s, k1, k2);
    (m, ctrl)
}

/// C5b: a colour curve has a colour strip under its graph (the graph keeps its height); a float curve has none.
#[test]
fn colour_strip_shows_on_colour_curves_only() {
    let (m, _) = colour_scene([0.2, 0.4, 0.6], [0.5, 0.5, 0.5]);
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    let graph = h.get_by_label("Emissive colour curve").rect();
    let strip = h.get_by_label("Emissive colour colour strip").rect();
    assert!(strip.top() >= graph.bottom(), "{strip:?} under {graph:?}");
    assert_eq!(graph.height(), 120.0, "the graph is unchanged");

    let mut h = view(clip_scene());
    h.run_steps(2);
    tab(&mut h, "Controllers");
    h.get_by_label("Emissive multiple curve");
    h.get_by_label("U offset curve");
    assert!(h.query_by_label("Emissive multiple colour strip").is_none());
    assert!(h.query_by_label("U offset colour strip").is_none());
}

/// C5b-D5: a click or drag on the colour strip scrubs like the graph.
#[test]
fn clicking_the_colour_strip_scrubs() {
    let (_d, mut m, _cmds) = playing_session();
    let (shape, _) = lit_shape(m.edit.as_ref().unwrap());
    add_colour_ctrl(&mut m, shape, [0.2, 0.4, 0.6], [0.8, 0.6, 0.4]);
    m.select_edit(Selection::Shape(shape));
    m.tick_anim(0.0);
    m.v3.clock.as_mut().unwrap().play();
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    // the strip sits further down than the scrub test's curve: wait for the scroll to settle
    reveal(&mut h, "Emissive colour colour strip");
    let r = h.get_by_label("Emissive colour colour strip").rect();
    // curve.rs PAD = 6 px; the colour controller spans [0, 1].
    let p = egui::pos2(r.left() + 6.0 + 0.25 * (r.width() - 12.0), r.center().y);
    h.hover_at(p);
    h.run_steps(1);
    h.drag_at(p);
    h.run_steps(1);
    h.drop_at(p);
    h.run_steps(3);
    let c = h.state().v3.clock.unwrap();
    assert!(!c.playing, "a scrub pauses");
    assert!((c.t - 0.25).abs() < 0.01, "{c:?}");
}

/// C5b screenshots: the colour strip with an HDR key at 1100 and 1600 px, saved to `SDC_C5B_SHOTS` (default: the
/// temp dir). Needs a GPU: `cargo test -p sdc-gui --test nif_edit -- --ignored render_the_colour_strip`.
#[test]
#[ignore]
fn render_the_colour_strip() {
    let out = std::env::var("SDC_C5B_SHOTS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    for w in [1100.0, 1600.0] {
        let (m, _) = colour_scene([2.0, 0.5, 0.0], [0.2, 0.4, 0.6]);
        let mut h = Harness::builder()
            .with_size(egui::vec2(w, 1000.0))
            .wgpu()
            .build_ui_state(
                |ui, m: &mut MeshesModel| sdc_gui::ui::meshes::show(ui, m),
                m,
            );
        sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
        h.run_steps(2);
        tab(&mut h, "Controllers");
        reveal(&mut h, "Emissive colour colour strip");
        let img = h.render().expect("render");
        img.save(out.join(format!("c5b-strip-{w}.png")))
            .expect("save png");
    }
}

fn pos_keys(m: &MeshesModel, ctrl: u32) -> KeyGroup<[f32; 3]> {
    match &m.edit.as_ref().unwrap().keys[&(ctrl, None)] {
        Ok(KeyData::Pos(g)) => g.clone(),
        k => panic!("{k:?}"),
    }
}

/// B-P35 minor: a colour key's handle moves its own channel only.
#[test]
fn dragging_one_colour_channel_changes_only_it() {
    let (m, ctrl) = colour_scene([0.2, 0.4, 0.6], [0.8, 0.6, 0.4]);
    let pos = |m: &MeshesModel| pos_keys(m, ctrl);
    let before = pos(&m);
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    drag_up(&mut h, "Emissive colour key 2 G", 30.0);
    assert_eq!(ops(h.state()).len(), 2, "{:?}", ops(h.state()));
    let after = pos(h.state());
    let bits = |k: &Key<[f32; 3]>| (k.time.to_bits(), k.value.map(f32::to_bits));
    assert_eq!(
        bits(&after.keys[0]),
        bits(&before.keys[0]),
        "key 1 untouched"
    );
    assert_eq!(after.keys[1].time.to_bits(), before.keys[1].time.to_bits());
    assert_eq!(
        after.keys[1].value[0].to_bits(),
        before.keys[1].value[0].to_bits(),
        "R"
    );
    assert_eq!(
        after.keys[1].value[2].to_bits(),
        before.keys[1].value[2].to_bits(),
        "B"
    );
    assert!(
        after.keys[1].value[1] > 0.6,
        "G dragged up: {:?}",
        after.keys[1]
    );
}

/// Sets the popup DragValue (sRGB byte) of the open colour picker at `index` (0 R, 1 G, 2 B) to `byte`.
fn type_picker_byte(h: &mut Harness<'static, MeshesModel>, index: usize, byte: u8) {
    // The picker's own R/G/B DragValues are the only unnamed ones on screen.
    let mut spins: Vec<_> = h
        .query_all(egui_kittest::kittest::by().role(AkRole::SpinButton))
        .filter(|n| n.accesskit_node().label().is_none())
        .map(|n| (n.rect().min.x, n.accesskit_node().id()))
        .collect();
    assert_eq!(spins.len(), 3, "the picker's R, G and B");
    spins.sort_by(|a, b| a.0.total_cmp(&b.0));
    let id = spins[index].1;
    h.get(egui_kittest::kittest::by().predicate(move |n| n.id() == id))
        .focus();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    h.get(egui_kittest::kittest::by().predicate(move |n| n.id() == id))
        .type_text(&byte.to_string());
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
}

/// Owner follow-up: a colour key row has a picker swatch. A picker change is one merged SetKeys on that key (the
/// same path as an R/G/B drag) until the popup closes; channels the picker did not change keep their exact bits.
#[test]
fn key_table_colour_picker_sets_the_key_colour() {
    let (m, ctrl) = colour_scene([0.2, 0.4, 0.6], [0.8, 0.6, 0.4]);
    let before = pos_keys(&m, ctrl);
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    let sw = "Emissive colour key 2 colour";
    let n = h.get_by_role_and_label(AkRole::ColorWell, sw);
    assert!(!n.accesskit_node().is_disabled());
    n.click();
    h.run_steps(3);
    type_picker_byte(&mut h, 1, 51); // G
    type_picker_byte(&mut h, 1, 102); // G again, same popup: merged
    h.key_press(egui::Key::Escape); // closes the popup: the step ends
    h.run_steps(3);
    let got = ops(h.state());
    assert_eq!(got.len(), 2, "AddController + one SetKeys: {got:?}");
    let Op::SetKeys { clip, base, .. } = &got[1] else {
        panic!("{got:?}")
    };
    assert_eq!(*clip, None);
    assert_eq!(
        encode_data(base),
        encode_data(&KeyData::Pos(before.clone())),
        "the step keeps the keys it started from"
    );
    let after = pos_keys(h.state(), ctrl);
    let bits = |k: &Key<[f32; 3]>| (k.time.to_bits(), k.value.map(f32::to_bits));
    assert_eq!(
        bits(&after.keys[0]),
        bits(&before.keys[0]),
        "key 1 untouched"
    );
    let (b, a) = (&before.keys[1], &after.keys[1]);
    assert_eq!(a.time.to_bits(), b.time.to_bits());
    assert_eq!(a.value[0].to_bits(), b.value[0].to_bits(), "R");
    assert_eq!(a.value[2].to_bits(), b.value[2].to_bits(), "B");
    // C-P17': the key stores sRGB, so the picker's byte 102 is the stored value itself (0.4), not its linear.
    assert!(
        (a.value[1] * 255.0 - 102.0).abs() < 0.01,
        "G: {}",
        a.value[1]
    );
    // The popup closed and flushed: a second picker change is a second step.
    h.get_by_role_and_label(AkRole::ColorWell, sw).click();
    h.run_steps(3);
    type_picker_byte(&mut h, 2, 51); // B
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert_eq!(ops(h.state()).len(), 3, "{:?}", ops(h.state()));
    assert!(h.state_mut().undo_edit());
    assert!(h.state_mut().undo_edit());
    assert_eq!(ops(h.state()).len(), 1, "one undo step per popup");
    assert_eq!(bits(&pos_keys(h.state(), ctrl).keys[1]), bits(b));
}

/// The open colour picker's R, G and B bytes (its own unnamed DragValues), left to right.
fn picker_bytes(h: &Harness<'static, MeshesModel>) -> Vec<f64> {
    let mut spins: Vec<_> = h
        .query_all(egui_kittest::kittest::by().role(AkRole::SpinButton))
        .filter(|n| n.accesskit_node().label().is_none())
        .map(|n| (n.rect().min.x, n.accesskit_node().numeric_value().unwrap()))
        .collect();
    spins.sort_by(|a, b| a.0.total_cmp(&b.0));
    spins.into_iter().map(|s| s.1).collect()
}

/// sRGB 0.5 is byte 127.5 (ecolor rounds it to 127); as linear it would show 188.
fn assert_srgb_half(bytes: &[f64]) {
    assert_eq!(bytes.len(), 3, "{bytes:?}");
    assert!(
        bytes.iter().all(|b| (127.0..=128.0).contains(b)),
        "{bytes:?}"
    );
}

/// C-P17': NIF colours are sRGB, so a key of 0.5 shows sRGB byte ~128 in the picker (not 188, its value as linear).
#[test]
fn key_table_colour_picker_shows_the_key_as_srgb() {
    let (m, _) = colour_scene([0.2, 0.4, 0.6], [0.5, 0.5, 0.5]);
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    h.get_by_role_and_label(AkRole::ColorWell, "Emissive colour key 2 colour")
        .click();
    h.run_steps(3);
    assert_srgb_half(&picker_bytes(&h));
}

fn specular(m: &MeshesModel, a: u32) -> [f32; 3] {
    let e = m.edit.as_ref().unwrap();
    e.model.lighting[&e.model.shape(a).unwrap().shader.unwrap()]
        .lighting
        .params
        .specular_color
}

/// `shared_alpha_and_set` with shape A selected and its Specular colour set to `v`; returns A.
fn specular_scene(v: [f32; 3]) -> (MeshesModel, u32) {
    let mut m = direct(scenes::shared_alpha_and_set());
    let a = select(&mut m, "A");
    let e = m.edit.as_ref().unwrap();
    let at = nif::edit::anchor_role(&e.model, Some(a), Role::Shader).unwrap();
    let base = specular(&m, a);
    let ops = (0..3usize)
        .map(|i| Op::SetField {
            at: at.clone(),
            field: Field::SpecularColor(i as u8),
            value: Value::F32(v[i].to_bits()),
            base: Value::F32(base[i].to_bits()),
        })
        .collect();
    m.edit_push(ops, None);
    (m, a)
}

/// C-P25: a Shader tab colour outside 0-1 (or NaN) gets a disabled swatch with the reason on hover, like the key
/// swatch; the picker would clamp it.
#[test]
fn shader_colour_picker_is_disabled_outside_0_1() {
    for v in [[2.0, 0.5, 0.5], [0.5, -0.25, 0.5], [0.5, 0.5, f32::NAN]] {
        let (m, _) = specular_scene(v);
        let mut h = view(m);
        h.run_steps(2);
        let sw = h.get_by_role_and_label(AkRole::ColorWell, "Specular colour");
        assert!(sw.accesskit_node().is_disabled(), "{v:?}");
        let c = sw.rect().center();
        h.hover_at(c);
        h.run_steps(3);
        h.get_by_label("Values outside 0-1: edit R, G and B directly.");
    }
}

/// C-P17': the Shader tab's colour picker shows the stored colour as sRGB and stores what is picked as sRGB; the
/// channels the picker did not change keep their exact bits, and the R/G/B fields show the raw stored floats.
#[test]
fn shader_colour_picker_shows_and_stores_srgb() {
    let (m, a) = specular_scene([0.5; 3]);
    let spec = |m: &MeshesModel| specular(m, a);
    let mut h = view(m);
    h.run_steps(2);
    h.get_by_role_and_label(AkRole::ColorWell, "Specular colour")
        .click();
    h.run_steps(3);
    assert_srgb_half(&picker_bytes(&h));
    type_picker_byte(&mut h, 1, 102); // G
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    let c = spec(h.state());
    assert_eq!(c[0].to_bits(), 0.5f32.to_bits(), "R untouched");
    assert_eq!(c[2].to_bits(), 0.5f32.to_bits(), "B untouched");
    assert!(
        (c[1] * 255.0 - 102.0).abs() < 0.01,
        "G stored as sRGB: {}",
        c[1]
    );
    let g = h.get_by_label("Specular colour G");
    assert_eq!(g.accesskit_node().numeric_value(), Some(c[1] as f64));
}

/// Owner follow-up: a key outside 0-1 (an HDR emissive colour) gets a disabled swatch with the reason on hover; the
/// picker would clamp it. The in-range key on the same table keeps its picker.
#[test]
fn key_table_colour_picker_is_disabled_for_hdr_values() {
    let (m, _) = colour_scene([2.0, 0.5, 0.5], [0.8, 0.6, 0.4]);
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    let hdr = h.get_by_role_and_label(AkRole::ColorWell, "Emissive colour key 1 colour");
    assert!(hdr.accesskit_node().is_disabled());
    let c = hdr.rect().center();
    let ok = h.get_by_role_and_label(AkRole::ColorWell, "Emissive colour key 2 colour");
    assert!(!ok.accesskit_node().is_disabled());
    h.hover_at(c);
    h.run();
    h.get_by_label("Values outside 0-1: edit R, G and B directly.");
}

/// B-P35 minor (D11): TBC, XYZ and CONST keys are drawn, but their handles, table cells and buttons are disabled
/// and a drag records nothing.
/// Review minor: NaN and negative channels disable the swatch too (never clamped by the picker).
#[test]
fn key_table_colour_picker_is_disabled_for_nan_and_negative_values() {
    let (m, _) = colour_scene([f32::NAN, 0.5, 0.5], [0.5, -0.25, 0.5]);
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    for k in [1, 2] {
        let n = format!("Emissive colour key {k} colour");
        let sw = h.get_by_role_and_label(AkRole::ColorWell, &n);
        assert!(sw.accesskit_node().is_disabled(), "{n}");
    }
}

/// Review minor (D11): a TBC, XYZ or CONST colour key's swatch is disabled like its cells, though in range.
#[test]
fn key_table_colour_picker_is_disabled_for_tbc_xyz_and_const_keys() {
    for t in [3, 4, 5] {
        let (mut m, ctrl) = colour_scene([0.2, 0.4, 0.6], [0.8, 0.6, 0.4]);
        let role = Role::Ctrl {
            owner: nif::edit::Owner::Shader,
            class: "BSLightingShaderPropertyColorController".into(),
            var: Some(1),
            nth: 0,
        };
        let e = m.edit.as_ref().unwrap();
        let at = nif::edit::anchor_role(&e.model, Some(shape_named(e, "S")), role).unwrap();
        let base = pos_keys(&m, ctrl);
        let mut keys = base.clone();
        keys.key_type = Some(t);
        for k in &mut keys.keys {
            k.tbc = (t == 3).then_some([0.0; 3]);
        }
        m.edit_push(
            vec![Op::SetKeys {
                at,
                clip: None,
                clip_name: None,
                keys: KeyData::Pos(keys),
                base: KeyData::Pos(base),
            }],
            None,
        );
        assert_eq!(
            pos_keys(&m, ctrl).key_type,
            Some(t),
            "{:?}",
            m.edit.as_ref().unwrap().outcomes
        );
        let mut h = view(m);
        h.run_steps(2);
        tab(&mut h, "Controllers");
        h.get_by_label("These keys are previewed, not edited.");
        let sw = h.get_by_role_and_label(AkRole::ColorWell, "Emissive colour key 2 colour");
        assert!(sw.accesskit_node().is_disabled(), "type {t}");
    }
}

#[test]
fn tbc_xyz_and_const_keys_are_drawn_not_edited() {
    for t in [3, 4, 5] {
        let mut m = direct(scenes::keyed_pair(scenes::Share::No, [t, 1]));
        select(&mut m, "S");
        let mut h = view(m);
        h.run_steps(2);
        tab(&mut h, "Controllers");
        h.get_by_label("Emissive multiple curve");
        h.get_by_label("These keys are previewed, not edited.");
        assert!(
            h.query_by_label("Emissive multiple interpolation")
                .is_none(),
            "type {t}"
        );
        for l in [
            "Emissive multiple key 1",
            "Emissive multiple key 1 time",
            "Emissive multiple key 1 value",
            "Remove Emissive multiple key 1",
            "Add key to Emissive multiple",
        ] {
            assert!(
                h.get_by_label(l).accesskit_node().is_disabled(),
                "type {t}: {l}"
            );
        }
        drag_up(&mut h, "Emissive multiple key 2", 30.0);
        assert!(ops(h.state()).is_empty(), "type {t}: {:?}", ops(h.state()));
    }
}

// ---- v2 B7: managed clips (the Preview selector, the clip list, clip key edits) ----

/// `with_uv_ctrl(bytes)` plus, first in the same chain, a manager-controlled LSCF var 11 (Emissive multiple, flags
/// 0x68, a NiBlendFloatInterpolator), and a NiControllerSequence "Glow" whose one CB names it by ref with a keyed
/// NiFloatInterpolator LINEAR (0, 0) -> (1, 4); Loop, frequency 1, start 0, stop 1; text keys "start" at 0, "end"
/// at 1. Returns the bytes and the sequence block.
fn with_clip(bytes: &[u8]) -> (Vec<u8>, u32) {
    let keys = KeyData::Float(KeyGroup {
        key_type: Some(1),
        keys: [(0.0, 0.0), (1.0, 4.0)]
            .map(|(time, value)| Key {
                time,
                value,
                forward: None,
                backward: None,
                tbc: None,
            })
            .into(),
    });
    with_clip_at(bytes, true, 0.0, Some(keys), [1.0, 0.0, 1.0])
}

/// [`with_clip`] with the CB bound by ref or (ref -1) by strings only, the CB interpolator's value, its keys (None:
/// data -1, the empty group) and the sequence's (frequency, start, stop).
fn with_clip_at(
    bytes: &[u8],
    by_ref: bool,
    value: f32,
    keys: Option<KeyData>,
    timing: [f32; 3],
) -> (Vec<u8>, u32) {
    let m = nif::model::read_model(bytes).unwrap();
    let (shape, lsp) = m
        .shapes
        .iter()
        .find_map(|s| {
            s.shader
                .filter(|b| m.lighting.contains_key(b))
                .map(|b| (s, b))
        })
        .unwrap();
    let net = m.lighting[&lsp].prop.net.clone();
    let mut doc = nif::write::NifDoc::open(bytes).unwrap();
    let at = net.controller_at() - doc.header().offsets()[lsp as usize];
    let blend = doc
        .append(
            b"NiBlendFloatInterpolator",
            builder::W::default().u8(1).u8(0).f(0.0).f(0.0).0,
        )
        .unwrap();
    let ctrl = doc
        .append(
            b"BSLightingShaderPropertyFloatController",
            scenes::ctrl_body(
                net.controller,
                0x68,
                lsp as i32,
                builder::W::default().i32(blend as i32).u32(11),
            ),
        )
        .unwrap();
    doc.patch(lsp, at, &(ctrl as i32).to_le_bytes()).unwrap();
    let data = keys.map_or(-1, |k| {
        doc.append(b"NiFloatData", nif::keys::encode_data(&k))
            .unwrap() as i32
    });
    let interp = doc
        .append(
            b"NiFloatInterpolator",
            builder::W::default().f(value).i32(data).0,
        )
        .unwrap();
    let s = |doc: &mut nif::write::NifDoc, x: &[u8]| doc.string(x).unwrap();
    let (start, end) = (s(&mut doc, b"start"), s(&mut doc, b"end"));
    let tk = doc
        .append(
            b"NiTextKeyExtraData",
            builder::W::default()
                .i32(-1)
                .u32(2)
                .f(0.0)
                .i32(start)
                .f(1.0)
                .i32(end)
                .0,
        )
        .unwrap();
    let (name, prop, ctype, cid) = (
        s(&mut doc, b"Glow"),
        s(&mut doc, b"BSLightingShaderProperty"),
        s(&mut doc, b"BSLightingShaderPropertyFloatController"),
        s(&mut doc, b"11"),
    );
    let seq = builder::W::default()
        .i32(name)
        .u32(1)
        .u32(0)
        .i32(interp as i32)
        .i32(if by_ref { ctrl as i32 } else { -1 })
        .u8(0)
        .i32(shape.net.name)
        .i32(prop)
        .i32(ctype)
        .i32(cid)
        .i32(-1)
        .f(1.0)
        .i32(tk as i32)
        .u32(0)
        .fs(&timing)
        .i32(-1)
        .i32(-1)
        .u16(0)
        .0;
    let seq = doc.append(b"NiControllerSequence", seq).unwrap();
    let out = doc.bytes();
    let back = nif::model::read_model(&out).unwrap();
    assert!(back.problems.is_empty(), "{:?}", back.problems);
    assert!(back.managed(ctrl));
    assert_eq!(back.lighting[&lsp].prop.ctrls[0], ctrl);
    assert_eq!(back.sequences.len(), 1);
    assert_eq!(back.sequences[0].text_keys.len(), 2);
    assert_eq!(
        nif::anim::clips_of(&back, &out, ctrl),
        vec![seq],
        "the CB binds"
    );
    (out, seq)
}

/// G6, D4: Free runs the free chain and keeps the managed emissive at the property value; selecting the clip runs
/// the CB interpolator and pauses the free chain; back to Free swaps back. The UI selector and the clip's key table
/// drive the same model.
#[test]
fn clip_preview_swaps_interpolators_only_while_selected() {
    let (nif, seq) = with_clip(&with_uv_ctrl(&fixture("TestNifFile_Skinned_SE.nif")));
    let (_d, mut m, cmds) = session_on(&nif);
    let (shape, lsp) = lit_shape(m.edit.as_ref().unwrap());
    let base = m.edit.as_ref().unwrap().model.lighting[&lsp]
        .lighting
        .params
        .clone();
    let at = |m: &MeshesModel, t: f32| {
        let f = m.edit.as_ref().unwrap().frame_materials(t);
        f.into_iter()
            .find(|(id, _)| id.0 == shape)
            .unwrap()
            .1
            .params
    };
    let p = at(&m, 0.5);
    assert_eq!(
        (p.uv_offset[0], p.emissive_multiple),
        (0.5, base.emissive_multiple),
        "Free"
    );
    m.edit_with(|e| e.set_playing(Playing::Clip(seq)));
    let p = at(&m, 0.5);
    assert_eq!(
        (p.uv_offset[0], p.emissive_multiple),
        (base.uv_offset[0], 2.0),
        "the clip"
    );
    assert_eq!(
        m.edit.as_ref().unwrap().anim_range(),
        Some((0.0, 1.0)),
        "the clip's start and stop"
    );
    m.edit_with(|e| e.set_playing(Playing::Free));
    let p = at(&m, 0.5);
    assert_eq!(
        (p.uv_offset[0], p.emissive_multiple),
        (0.5, base.emissive_multiple),
        "Free again"
    );
    // The UI.
    m.select_edit(Selection::Shape(shape));
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    // Meshes UI T4: the clip is one weak line under its controller.
    h.get_by_label("In clip Glow · Loop · 0 to 1 s");
    h.get_by_label("Text keys: start at 0 s, end at 1 s");
    h.get_by_label("Pick this clip in Preview to see and edit its keys.");
    h.get_by_role_and_label(AkRole::ComboBox, "Preview animation")
        .click();
    h.run_steps(3);
    h.get_by_role_and_label(AkRole::Button, "Glow").click();
    h.run_steps(3);
    assert_eq!(h.state().edit.as_ref().unwrap().playing, Playing::Clip(seq));
    // The clip's own key table edits that clip (SetKeys with clip Some).
    edit_number(&mut h, "Emissive multiple in Glow key 2 value", "3");
    let last = ops(h.state()).pop();
    assert!(
        matches!(last, Some(Op::SetKeys { clip: Some(s), .. }) if s == seq),
        "{last:?}"
    );
    assert_eq!(
        h.state().edit.as_ref().unwrap().outcomes.last(),
        Some(&nif::edit::Outcome::Applied)
    );
    assert_eq!(
        at(h.state(), 0.5).emissive_multiple,
        1.5,
        "the edited clip curve: (0,0) -> (1,3)"
    );
    h.get_by_role_and_label(AkRole::ComboBox, "Preview animation")
        .click();
    h.run_steps(3);
    h.get_by_role_and_label(AkRole::Button, "Free chains")
        .click();
    h.run_steps(3);
    assert_eq!(h.state().edit.as_ref().unwrap().playing, Playing::Free);
    drop(cmds);
}

// ---- v2 B7 fix round 1 (B-P36) ----

const IDLE_FIRST: [f32; 3] = [0.9, 0.3, 0.1];
const IDLE_LAST: [f32; 3] = [0.1, 0.2, 0.3];

/// [`playing_session`] with an LSCC (key 1 `IDLE_FIRST`, key 2 `IDLE_LAST`, 0..1) added to the lit shape, then the
/// shader chain (U offset and the LSCC) converted into one clip `name`. Returns the session, the shape and the
/// sequence block.
fn idle_session(name: &str) -> (tempfile::TempDir, MeshesModel, Receiver<V3Cmd>, u32, u32) {
    let (d, mut m, cmds) = playing_session();
    let (shape, lsp) = lit_shape(m.edit.as_ref().unwrap());
    add_colour_ctrl(&mut m, shape, IDLE_FIRST, IDLE_LAST);
    let e = m.edit.as_ref().unwrap();
    let at = nif::edit::anchor_role(&e.model, Some(shape), Role::Shader).unwrap();
    m.edit_push(
        vec![Op::ConvertChainToClips {
            at,
            name: name.into(),
        }],
        None,
    );
    let e = m.edit.as_ref().unwrap();
    assert_eq!(e.outcomes.last(), Some(&nif::edit::Outcome::Applied));
    assert_eq!(e.playing, Playing::Free);
    assert_ne!(
        e.model.lighting[&lsp].lighting.params.emissive_color,
        IDLE_FIRST
    );
    let seq = e.model.sequences[0].block;
    (d, m, cmds, shape, seq)
}

/// `shape`'s emissive colour in the preview at clock time `t`.
fn emissive_at(m: &MeshesModel, shape: u32, t: f32) -> [f32; 3] {
    m.edit
        .as_ref()
        .unwrap()
        .frame_materials(t)
        .into_iter()
        .find(|(id, _)| id.0 == shape)
        .unwrap()
        .1
        .params
        .emissive_color
}

/// C-P32 (IDLE-NM, the C5 in-game check): with no clip playing, a managed controller previews its clip's first key,
/// not the property value; the clip itself still plays its keys. C-P34 (b): with only held values the range has
/// zero length, so there is no clock (no controls), and the held values still reach the viewport.
#[test]
fn an_idle_clip_previews_its_first_key() {
    let (_d, mut m, cmds, shape, seq) = idle_session("Glow");
    for t in [0.0, 0.5, 1.0] {
        assert_eq!(emissive_at(&m, shape, t), IDLE_FIRST, "Free at {t}");
    }
    assert_eq!(m.edit.as_ref().unwrap().anim_range(), None, "held only");
    cmds.try_iter().for_each(drop);
    m.tick_anim(0.0);
    assert!(m.v3.clock.is_none(), "no clock, no controls");
    assert!(
        cmds.try_iter().any(|c| matches!(c, V3Cmd::SetMaterial { material, .. } if material.params.emissive_color == IDLE_FIRST)),
        "the held value is shown"
    );
    m.edit_with(|e| e.set_playing(Playing::Clip(seq)));
    assert_eq!(emissive_at(&m, shape, 1.0), IDLE_LAST, "the clip");
}

/// C-P34 (a), AUTO-SI: under Free chains the SpecialIdle clip plays by itself, with its own timing, and the clock
/// spans it.
#[test]
fn a_special_idle_clip_plays_under_free_chains() {
    let (_d, mut m, _cmds, shape, _) = idle_session(nif::edit::CONVERT_NAME);
    assert_eq!(emissive_at(&m, shape, 0.0), IDLE_FIRST);
    assert_eq!(emissive_at(&m, shape, 1.0), IDLE_LAST);
    let mid = emissive_at(&m, shape, 0.5);
    assert!((mid[0] - 0.5).abs() < 1e-6, "{mid:?}");
    assert_eq!(m.edit.as_ref().unwrap().anim_range(), Some((0.0, 1.0)));
    m.tick_anim(0.0);
    assert_eq!(m.v3.clock.unwrap().range, (0.0, 1.0));
}

/// The managed controller with_clip puts first in the lit shape's chain.
fn clip_ctrl(e: &NifEdit) -> u32 {
    let (_, lsp) = lit_shape(e);
    e.model.lighting[&lsp].prop.ctrls[0]
}

/// Review I2: a source reload that removes the selected clip's sequence returns the preview to the free chains, so
/// they still play (the selector is gone with the last sequence).
#[test]
fn losing_the_selected_clip_returns_to_the_free_chains() {
    let plain = with_uv_ctrl(&fixture("TestNifFile_Skinned_SE.nif"));
    let (nif, seq) = with_clip(&plain);
    let (_d, mut m, _cmds) = session_on(&nif);
    m.edit_with(|e| e.set_playing(Playing::Clip(seq)));
    m.edit_with(|e| e.reload_source(plain.clone()).unwrap());
    let e = m.edit.as_ref().unwrap();
    assert!(e.model.sequences.is_empty());
    assert_eq!(e.playing, Playing::Free);
    assert_eq!(
        e.anim_range(),
        Some((0.0, 1.0)),
        "the free U offset chain plays"
    );
    m.tick_anim(0.0);
    assert!(m.v3.clock.is_some());
}

/// Review I3: Add key on a string-bound clip with an empty key group starts at its CB interpolator's value (5).
#[test]
fn add_key_on_a_string_bound_empty_clip_starts_at_its_interpolator_value() {
    let plain = with_uv_ctrl(&fixture("TestNifFile_Skinned_SE.nif"));
    let (nif, seq) = with_clip_at(&plain, false, 5.0, None, [1.0, 0.0, 1.0]);
    let mut m = direct(nif);
    m.edit_with(|e| e.set_playing(Playing::Clip(seq)));
    let e = m.edit.as_ref().unwrap();
    let ctrl = clip_ctrl(e);
    assert!(
        matches!(&e.keys[&(ctrl, Some(seq))], Ok(KeyData::Float(g)) if g.keys.is_empty()),
        "{:?}",
        e.keys.get(&(ctrl, Some(seq)))
    );
    let (shape, _) = lit_shape(e);
    m.select_edit(Selection::Shape(shape));
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    h.get_by_role_and_label(AkRole::Button, "Add key to Emissive multiple in Glow")
        .click();
    h.run_steps(3);
    let last = ops(h.state()).pop();
    let Some(Op::SetKeys {
        clip: Some(s),
        clip_name,
        keys: KeyData::Float(g),
        ..
    }) = &last
    else {
        panic!("{last:?}")
    };
    assert_eq!(*s, seq);
    assert_eq!(
        clip_name.as_deref(),
        Some("Glow"),
        "C-P35 I1: the edit names its clip"
    );
    assert_eq!(g.keys.len(), 1);
    assert_eq!((g.keys[0].time, g.keys[0].value), (0.0, 5.0));
}

/// Review I4: every combo of the Controllers tab (Preview, the Variable combos, Add controller) ends inside the edit
/// panel at 1100 px.
#[test]
fn controllers_tab_combos_fit_the_edit_panel() {
    let (nif, _) = with_clip(&with_uv_ctrl(&fixture("TestNifFile_Skinned_SE.nif")));
    let (_d, mut m, _cmds) = session_on(&nif);
    let (shape, _) = lit_shape(m.edit.as_ref().unwrap());
    m.select_edit(Selection::Shape(shape));
    // A colour controller: its key rows (with the picker swatch) must fit too.
    add_colour_ctrl(&mut m, shape, [0.2, 0.4, 0.6], [0.8, 0.6, 0.4]);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1100.0, 1600.0))
        .build_ui_state(
            |ui, m: &mut MeshesModel| sdc_gui::ui::meshes::show(ui, m),
            m,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run_steps(2);
    tab(&mut h, "Controllers");
    h.run_steps(3);
    let panel = egui::containers::panel::PanelState::load(&h.ctx, egui::Id::new("meshes-edit"))
        .expect("the edit panel")
        .outer_rect;
    let combos: Vec<_> = h
        .query_all(egui_kittest::kittest::by().role(AkRole::ComboBox))
        .filter(|n| n.rect().min.x >= panel.min.x)
        .collect();
    let names: Vec<String> = combos
        .iter()
        .map(|n| n.accesskit_node().label().unwrap_or_default())
        .collect();
    for want in [
        "Preview animation",
        "Emissive multiple variable",
        "U offset variable",
        "Add controller variable",
    ] {
        assert!(names.iter().any(|n| n == want), "{want} in {names:?}");
    }
    for n in &combos {
        let r = n.rect();
        assert!(
            r.max.x <= panel.max.x + 0.5,
            "{:?}: {r:?} past the edit panel's right edge {}",
            n.accesskit_node().label(),
            panel.max.x
        );
    }
    // The colour key rows: swatch and Remove end inside the panel (AccessKit rects are not clipped).
    for (role, name) in [
        (AkRole::ColorWell, "Emissive colour key 2 colour"),
        (AkRole::Button, "Remove Emissive colour key 2"),
    ] {
        let r = h.get_by_role_and_label(role, name).rect();
        assert!(
            r.max.x <= panel.max.x + 0.5,
            "{name}: {r:?} past the edit panel's right edge {}",
            panel.max.x
        );
    }
}

/// Review minor: a clip whose timing differs from the free chain (0 to 3 s) sets the clock's range when picked;
/// Free sets it back.
#[test]
fn picking_a_clip_sets_the_clock_range_to_its_timing() {
    let plain = with_uv_ctrl(&fixture("TestNifFile_Skinned_SE.nif"));
    let key = |time: f32, value: f32| Key {
        time,
        value,
        forward: None,
        backward: None,
        tbc: None,
    };
    let keys = KeyData::Float(KeyGroup {
        key_type: Some(1),
        keys: vec![key(0.0, 0.0), key(3.0, 3.0)],
    });
    let (nif, seq) = with_clip_at(&plain, true, 0.0, Some(keys), [1.0, 0.0, 3.0]);
    let (_d, mut m, _cmds) = session_on(&nif);
    assert_eq!(m.v3.clock.unwrap().range, (0.0, 1.0), "Free");
    m.edit_with(|e| e.set_playing(Playing::Clip(seq)));
    m.tick_anim(0.0);
    assert_eq!(m.v3.clock.unwrap().range, (0.0, 3.0), "the clip");
    m.edit_with(|e| e.set_playing(Playing::Free));
    m.tick_anim(0.0);
    assert_eq!(m.v3.clock.unwrap().range, (0.0, 1.0), "Free again");
}

// ---- v2 B8: In game (simulated) through pgpsim ----

use sdc_gui::model::meshes::pg_entries::*;
use sdc_gui::model::meshes::ScanTarget;

/// Mods (highest first) PGP, A, Low. A: x.nif, the PBR textures of `white`, PBRNIFPatcher\x.json (object form, BOM,
/// roughness 0.5 from `default`, a `smooth_angle` the preview does not show) and bad.json (no `default`: PGPatcher
/// skips the file). Low: a lower copy of the same path (hidden by A's) and a.json (sorts first, but Low is below A).
fn pg_files() -> tempfile::TempDir {
    let d = files_with(false, "+PGP\n+A\n+Low\n");
    for t in ["white", "white_n", "white_rmaos"] {
        put(
            d.path(),
            &format!("mods/A/textures/pbr/{t}.dds"),
            Some(b"dds"),
        );
    }
    put_x_json(d.path(), 0.5);
    put(
        d.path(),
        "mods/A/PBRNIFPatcher/bad.json",
        Some(br#"{"entries": [{"match_diffuse": "white", "roughness_scale": 0.7}]}"#),
    );
    put(
        d.path(),
        "mods/Low/PBRNifPatcher/x.json",
        Some(br#"[{"match_diffuse": "white", "roughness_scale": 0.1}]"#),
    );
    put(
        d.path(),
        "mods/Low/PBRNifPatcher/a.json",
        Some(br#"[{"match_diffuse": "white", "roughness_scale": 0.9}]"#),
    );
    d
}

/// A's x.json with `roughness` as its default roughness_scale.
fn put_x_json(root: &Path, roughness: f32) {
    let x = format!(
        "\u{feff}{{\"$schema\": \"x\", \"default\": {{\"roughness_scale\": {roughness}}}, \"entries\": [{{\"match_diffuse\": \"white\", \"smooth_angle\": 60}}]}}"
    );
    put(root, "mods/A/PBRNIFPatcher/x.json", Some(x.as_bytes()));
}

fn pg_session() -> (tempfile::TempDir, MeshesModel) {
    let d = pg_files();
    let m = pg_session_in(d.path());
    (d, m)
}

/// Scans `root`, selects A's x.nif and opens an edit session on it.
fn pg_session_in(root: &Path) -> MeshesModel {
    let mut m = MeshesModel::new(Some(root.join("scan.db")), MeshesSettings::default());
    m.procs = sdc_gui::model::meshes::no_procs;
    scan_and_select(&mut m, root);
    m.open_edit();
    assert_eq!(m.error, None);
    m
}

/// Polls until the PGPatcher JSON read lands (fix F2: it runs on a worker thread).
fn wait_pg(m: &mut MeshesModel) {
    let t = Instant::now();
    m.poll();
    while m.pg_reading() {
        assert!(
            t.elapsed() < Duration::from_secs(30),
            "the JSON read did not end"
        );
        std::thread::sleep(Duration::from_millis(5));
        m.poll();
    }
}

/// In game for `m`: switch, then wait for the JSON.
fn in_game(m: &mut MeshesModel) {
    m.set_view_source(false);
    wait_pg(m);
}

/// The Parsed content's material of shape block `b`.
fn shown(m: &MeshesModel, b: u32) -> nif::material::MaterialState {
    let Some(Content::Parsed { nif, .. }) = m.v3.content() else {
        panic!("the edit preview shows")
    };
    nif.shapes
        .iter()
        .find(|s| s.id.0 == b)
        .expect("the shape")
        .material
        .clone()
}

/// The In game lines of shape block `b`.
fn notes(m: &MeshesModel, b: u32) -> Vec<String> {
    let rows = m.edit.as_ref().unwrap().in_game.clone().expect("In game");
    rows.into_iter().find(|(id, _, _)| id.0 == b).unwrap().2
}

#[test]
fn in_game_mode_applies_pgpatcher_json_from_mods() {
    let d = pg_files();
    let inst = modscan::mo2::Instance::load(d.path(), Some("Default")).unwrap();
    let files: Vec<String> = pg_entries(&inst)
        .unwrap()
        .into_iter()
        .map(|f| f.file)
        .collect();
    assert_eq!(files, [r"PBRNIFPatcher\x.json", r"PBRNifPatcher\a.json"]);
    drop(d);

    let (_d, mut m) = pg_session();
    assert!(m.view_source, "the edit opens on Edited source");
    let e = m.edit.as_ref().unwrap();
    let (shape, shader) = lit_shape(e);
    assert_eq!(shown(&m, shape).slots[0].raw, r"textures\white.dds");
    let cur = e.model.lighting[&shader].lighting.params.uv_offset[0];
    let at = nif::edit::anchor_role(&e.model, Some(shape), Role::Shader).unwrap();
    m.edit_push(
        vec![Op::SetField {
            at,
            field: Field::UvOffset(0),
            value: Value::F32(0.25f32.to_bits()),
            base: Value::F32(cur.to_bits()),
        }],
        None,
    );
    m.select_edit(Selection::Shape(shape));
    let mut h = view(m);
    h.run_steps(4);
    h.get_by_label("In game (simulated)").click();
    h.run_steps(4);
    wait_pg(h.state_mut());
    h.run_steps(4);
    h.get_by_label(r"In game: PGPatcher applies PBRNIFPatcher\x.json.");
    h.get_by_label("Not previewed: smooth_angle.");
    h.get_by_label(IN_GAME_SCOPE);
    let m = h.state();
    assert!(!m.view_source);
    assert!(m.v3.nif_view);
    assert!(m.edit.as_ref().unwrap().in_game.is_some());
    let g = shown(m, shape);
    assert_eq!(g.slots[0].raw, r"textures\pbr\white.dds");
    assert_ne!(g.slsf2 & nif::material::SLSF2_TRUE_PBR, 0);
    assert_eq!(
        g.params.specular_strength, 0.5,
        "A's x.json, through its default"
    );
    assert_eq!(g.params.uv_offset[0], 0.25, "the edit survives the sim");

    h.get_by_label("Edited source").click();
    h.run_steps(4);
    let m = h.state();
    assert!(m.view_source);
    assert!(m.edit.as_ref().unwrap().in_game.is_none());
    let s = shown(m, shape);
    assert_eq!(s.slots[0].raw, r"textures\white.dds");
    assert_eq!(s.params.uv_offset[0], 0.25);
}

/// Fix F2: the JSON is read on a worker ("Reading PGPatcher JSON…" until it lands) and kept across switches.
#[test]
fn a_second_switch_to_in_game_does_not_read_the_json_again() {
    let (_d, mut m) = pg_session();
    let (shape, _) = lit_shape(m.edit.as_ref().unwrap());
    m.select_edit(Selection::Shape(shape));
    m.set_view_source(false);
    assert!(m.pg_reading(), "the read runs on a worker");
    assert!(notes(&m, shape).is_empty(), "no line before the JSON lands");
    let mut h = view(m);
    h.run_steps(2);
    h.get_by_label(READING_JSON);
    wait_pg(h.state_mut());
    h.run_steps(3);
    assert!(h.query_by_label(READING_JSON).is_none());
    h.get_by_label(r"In game: PGPatcher applies PBRNIFPatcher\x.json.");
    let m = h.state_mut();
    let reads = m.pg_reads;
    m.set_view_source(true);
    m.set_view_source(false);
    assert!(!m.pg_reading());
    assert_eq!(m.pg_reads, reads, "a second switch does not read again");
    assert_eq!(
        notes(m, shape)[0],
        r"In game: PGPatcher applies PBRNIFPatcher\x.json."
    );
}

/// `n` BSTriShapes, each with its own BSLSP and a texture set naming `textures\white.dds`.
fn lit_shapes(n: u32) -> Vec<u8> {
    let mut b = builder::Nif::new(100);
    let names: Vec<i32> = (0..n).map(|i| b.string(&format!("S{i}"))).collect();
    for (i, name) in names.iter().enumerate() {
        b.block("BSTriShape", one_tri(*name, (n as usize + i) as i32));
    }
    for i in 0..n {
        b.block(
            "BSLightingShaderProperty",
            scenes::lsp(0, (2 * n + i) as i32),
        );
    }
    for _ in 0..n {
        b.block(
            "BSShaderTextureSet",
            scenes::texset(&["textures\\white.dds"]),
        );
    }
    b.bytes()
}

/// Fix F1: rows are cached on the source material, the JSON generation and the scan. A drag tick In game on a
/// 20-shape mesh re-simulates the edited shape only; selection and switching back simulate nothing.
#[test]
fn a_drag_tick_in_game_resimulates_only_the_edited_shape() {
    let d = pg_files();
    put(d.path(), "mods/A/meshes/x.nif", Some(&lit_shapes(20)));
    let mut m = pg_session_in(d.path());
    in_game(&mut m);
    let e = m.edit.as_ref().unwrap();
    assert_eq!(e.sim_runs, 20, "one simulation per lit shape");
    let shape = e.model.shapes[3].block;
    let shader = e.model.shapes[3].shader.unwrap();
    let at = nif::edit::anchor_role(&e.model, Some(shape), Role::Shader).unwrap();
    for (i, v) in [0.1f32, 0.2].into_iter().enumerate() {
        m.edit_push(
            vec![Op::SetField {
                at: at.clone(),
                field: Field::UvOffset(0),
                value: Value::F32(v.to_bits()),
                base: Value::F32(0f32.to_bits()),
            }],
            Some(format!("uv-{shader}")),
        );
        assert_eq!(m.error, None);
        assert_eq!(m.edit.as_ref().unwrap().sim_runs, 21 + i, "tick {i}");
    }
    assert_eq!(shown(&m, shape).params.uv_offset[0], 0.2);
    assert_eq!(shown(&m, shape).slots[0].raw, r"textures\pbr\white.dds");
    m.select_edit(Selection::Shape(shape));
    m.set_view_source(true);
    m.set_view_source(false);
    assert_eq!(m.edit.as_ref().unwrap().sim_runs, 22, "nothing else");
}

/// H4 (fix F6): a file PGPatcher rejects for a shape (a missing texture) lets the next file win; when every file
/// is rejected, the first skip line shows.
#[test]
fn a_skipped_file_lets_the_next_one_win() {
    for renamed in [true, false] {
        let d = pg_files();
        std::fs::remove_file(d.path().join("mods/A/textures/pbr/white_rmaos.dds")).unwrap();
        if renamed {
            for t in ["other", "other_n", "other_rmaos"] {
                put(
                    d.path(),
                    &format!("mods/Low/textures/pbr/{t}.dds"),
                    Some(b"dds"),
                );
            }
            put(
                d.path(),
                "mods/Low/PBRNifPatcher/a.json",
                Some(br#"[{"match_diffuse": "white", "rename": "other", "roughness_scale": 0.9}]"#),
            );
        }
        let mut m = pg_session_in(d.path());
        let (shape, _) = lit_shape(m.edit.as_ref().unwrap());
        in_game(&mut m);
        let s = shown(&m, shape);
        if renamed {
            assert_eq!(
                notes(&m, shape),
                [r"In game: PGPatcher applies PBRNifPatcher\a.json."]
            );
            assert_eq!(s.slots[0].raw, r"textures\pbr\other.dds");
            assert_eq!(s.params.specular_strength, 0.9);
        } else {
            let n = notes(&m, shape);
            assert_eq!(n.len(), 1);
            assert!(
                n[0].starts_with(r"In game: PBRNIFPatcher\x.json is not applied ("),
                "{n:?}"
            );
            assert!(is_warning(&n[0]), "the skip line is amber");
            assert_eq!(s.slots[0].raw, r"textures\white.dds");
        }
    }
}

/// PG_IGNORE (value true) on the shape or its shader: In game keeps the edited material and says why; value 0
/// does not count.
#[test]
fn pg_ignore_shape_is_not_simulated() {
    for (role, v, ignored) in [(Role::Shape, 1u8, true), (Role::Shape, 0, false)] {
        let (_d, mut m) = pg_session();
        let e = m.edit.as_ref().unwrap();
        let (shape, _) = lit_shape(e);
        let at = nif::edit::anchor_role(&e.model, Some(shape), role.clone()).unwrap();
        m.edit_push(
            vec![Op::AddExtra {
                at,
                kind: "NiBooleanExtraData".into(),
                name: "PG_IGNORE".into(),
                value: ExtraValue::Bool(v),
            }],
            None,
        );
        assert_eq!(m.error, None);
        let edited = m.edit.as_ref().unwrap().materials();
        in_game(&mut m);
        let s = shown(&m, shape);
        if ignored {
            let rows = m.edit.as_ref().unwrap().in_game.clone().unwrap();
            let (_, mat, notes) = rows.iter().find(|(id, _, _)| id.0 == shape).unwrap();
            let before = &edited.iter().find(|(id, _)| id.0 == shape).unwrap().1;
            assert_eq!(mat, before, "{role:?}");
            assert_eq!(notes, &vec![PG_IGNORED.to_string()]);
            assert_eq!(s.slots[0].raw, r"textures\white.dds");
            assert_eq!(s.slsf2 & nif::material::SLSF2_TRUE_PBR, 0);
        } else {
            assert_eq!(
                s.slots[0].raw, r"textures\pbr\white.dds",
                "value 0 is simulated"
            );
        }
    }
}

/// One BSTriShape (0) whose BSLSP (1, texture set 2 naming `textures\white.dds`) carries NiBooleanExtraData
/// PG_IGNORE = `v` (3). The editor adds extra data to shapes and the root only, so the shader's is built in.
fn shader_pg_ignore(v: u8) -> Vec<u8> {
    let mut n = builder::Nif::new(100);
    let (s, pg) = (n.string("S"), n.string("PG_IGNORE"));
    n.block("BSTriShape", one_tri(s, 1));
    let lsp = scenes::lsp(0, 2);
    // shader type, name | extra count 1, the extra | controller and the rest
    let mut body = lsp[..8].to_vec();
    body.extend(W::default().u32(1).i32(3).0);
    body.extend(&lsp[12..]);
    n.block("BSLightingShaderProperty", body);
    n.block(
        "BSShaderTextureSet",
        scenes::texset(&["textures\\white.dds"]),
    );
    n.block("NiBooleanExtraData", W::default().i32(pg).u8(v).0);
    n.bytes()
}

/// PG_IGNORE on the shader counts as on the shape (fix F6); value 0 does not.
#[test]
fn pg_ignore_on_the_shader_is_not_simulated() {
    for (v, ignored) in [(1u8, true), (0, false)] {
        let d = pg_files();
        put(d.path(), "mods/A/meshes/x.nif", Some(&shader_pg_ignore(v)));
        let mut m = pg_session_in(d.path());
        let e = m.edit.as_ref().unwrap();
        let (shape, shader) = lit_shape(e);
        assert_eq!(
            e.model.extras_of(&e.model.lighting[&shader].prop.net).len(),
            1,
            "the fixture's shader extra"
        );
        in_game(&mut m);
        let s = shown(&m, shape);
        if ignored {
            assert_eq!(notes(&m, shape), [PG_IGNORED]);
            assert_eq!(s.slots[0].raw, r"textures\white.dds");
        } else {
            assert_eq!(s.slots[0].raw, r"textures\pbr\white.dds");
        }
    }
}

/// A PG_IGNORE pushed while In game shows takes effect at once (the cache keeps the check outside).
#[test]
fn pg_ignore_pushed_in_game_takes_effect() {
    let (_d, mut m) = pg_session();
    let (shape, _) = lit_shape(m.edit.as_ref().unwrap());
    in_game(&mut m);
    assert_eq!(shown(&m, shape).slots[0].raw, r"textures\pbr\white.dds");
    let e = m.edit.as_ref().unwrap();
    let at = nif::edit::anchor_role(&e.model, Some(shape), Role::Shape).unwrap();
    m.edit_push(
        vec![Op::AddExtra {
            at,
            kind: "NiBooleanExtraData".into(),
            name: "PG_IGNORE".into(),
            value: ExtraValue::Bool(1),
        }],
        None,
    );
    assert_eq!(notes(&m, shape), [PG_IGNORED]);
    assert_eq!(shown(&m, shape).slots[0].raw, r"textures\white.dds");
}

/// A folder scan reads no JSON and says so (fix F6).
#[test]
fn a_folder_scan_says_in_game_needs_an_instance() {
    let (d, mut m) = pg_session();
    let (shape, _) = lit_shape(m.edit.as_ref().unwrap());
    m.target = Some(ScanTarget::Folder(d.path().into()));
    m.select_edit(Selection::Shape(shape));
    in_game(&mut m);
    assert_eq!(notes(&m, shape), [NO_MATCH]);
    let mut h = view(m);
    h.run_steps(4);
    h.get_by_label(NO_INSTANCE);
    h.get_by_label(NO_MATCH);
    assert!(h.query_by_label(IN_GAME_SCOPE).is_none());
}

/// Fix F3: an instance that cannot be read says so, in place of the scope line; no shape claims "no match".
#[test]
fn an_unreadable_instance_has_its_own_line() {
    let (d, mut m) = pg_session();
    let (shape, _) = lit_shape(m.edit.as_ref().unwrap());
    std::fs::remove_file(d.path().join("ModOrganizer.ini")).unwrap();
    m.select_edit(Selection::Shape(shape));
    in_game(&mut m);
    assert!(notes(&m, shape).is_empty());
    let mut h = view(m);
    h.run_steps(4);
    h.get_by_label_contains(INSTANCE_UNREADABLE);
    assert!(h.query_by_label(IN_GAME_SCOPE).is_none());
    assert!(h.query_by_label(NO_MATCH).is_none());
}

/// Fix F5: a rescan re-reads the JSON (a session reopened after it sees the new file).
#[test]
fn a_rescan_reads_the_json_again() {
    let (d, mut m) = pg_session();
    let (shape, _) = lit_shape(m.edit.as_ref().unwrap());
    in_game(&mut m);
    assert_eq!(shown(&m, shape).params.specular_strength, 0.5);
    put_x_json(d.path(), 0.3);
    m.set_view_source(true);
    in_game(&mut m);
    assert_eq!(
        shown(&m, shape).params.specular_strength,
        0.5,
        "not watched"
    );
    scan_and_select(&mut m, d.path());
    m.open_edit();
    in_game(&mut m);
    assert_eq!(shown(&m, shape).params.specular_strength, 0.3);
}

/// Fix F9: a mesh PGPatcher already made PBR (no entry matches, slot 5 is an existing `_rmaos`) says so.
#[test]
fn an_already_pbr_shape_says_so() {
    let (_d, mut m) = pg_session();
    let e = m.edit.as_ref().unwrap();
    let (shape, shader) = lit_shape(e);
    let set = e.model.lighting[&shader].texture_set.unwrap();
    let raw = |i: usize| {
        e.model.texsets[&set]
            .raw
            .slots
            .get(i)
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .unwrap_or_default()
    };
    let at = nif::edit::anchor_shape(&e.model, shape);
    let ops = vec![
        Op::SetTexturePath {
            at: at.clone(),
            slot: 0,
            path: r"textures\stone.dds".into(),
            base: raw(0),
        },
        Op::SetTexturePath {
            at,
            slot: 5,
            path: r"textures\pbr\white_rmaos.dds".into(),
            base: raw(5),
        },
    ];
    m.edit_push(ops, None);
    assert!(
        m.edit
            .as_ref()
            .unwrap()
            .doc
            .ops
            .iter()
            .all(|o| o.status == OpStatus::Applied),
        "{:?}",
        m.edit.as_ref().unwrap().outcomes
    );
    in_game(&mut m);
    assert_eq!(notes(&m, shape), [ALREADY_PBR]);
}

/// Owner data (B8 brief, not a gate): the JSON read, then per shape the candidate scan (once per slot change) and
/// one simulation over the candidates (each change), for a shape that matches an owner entry.
#[test]
#[ignore]
fn in_game_cost_on_the_owner_json() {
    let Ok(root) = std::env::var("SDC_ACHERUS") else {
        println!("skipped: set SDC_ACHERUS");
        return;
    };
    let inst = modscan::mo2::Instance::load(Path::new(&root), Some("Nexus")).unwrap();
    let t = Instant::now();
    let files = pg_entries(&inst).unwrap();
    let read = t.elapsed();
    let entries: usize = files.iter().map(|f| f.entries.len()).sum();
    let e = NifEdit::open(
        fixture("TestNifFile_Skinned_SE.nif"),
        copy("A", 1, false),
        "meshes/x.nif",
        None,
    )
    .unwrap();
    let mut mat = e.materials()[0].1.clone();
    // A diffuse the owner's JSON names: the last file's first string match_diffuse.
    let md = files
        .iter()
        .rev()
        .flat_map(|f| &f.entries)
        .find_map(|e| e.get("match_diffuse")?.as_str().map(String::from))
        .expect("a match_diffuse");
    mat.set_slot_raw(0, &format!("textures{md}.dds"));
    for (label, m) in [("no match", e.materials()[0].1.clone()), ("match", mat)] {
        let t = Instant::now();
        let (c, tested) = candidates(&files, r"meshes\x.nif", &m.slots);
        let scan = t.elapsed();
        let t = Instant::now();
        let (_, notes) = simulate_shape(&files, &c, r"meshes\x.nif", &m, &|_| true);
        println!(
            "{label}: {} files, {entries} entries: pg_entries {read:?}; candidates {scan:?} ({} files, {tested} \
             entries tested); simulate {:?} ({notes:?})",
            files.len(),
            c.len(),
            t.elapsed()
        );
    }
    // Fix round 2: on the owner's JSON the index gives the full scan's matches, for every match_diffuse there.
    let t = Instant::now();
    let mut shapes = 0;
    for md in files
        .iter()
        .flat_map(|f| &f.entries)
        .filter_map(|e| e.get("match_diffuse")?.as_str())
    {
        let mut m = e.materials()[0].1.clone();
        m.set_slot_raw(0, &format!("textures{md}.dds"));
        let scan: Vec<(usize, Vec<usize>)> = files
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let hit = (0..f.entries.len())
                    .filter(|&j| pgpsim::shape_matches(&f.entries[j], r"meshes\x.nif", &m.slots))
                    .collect::<Vec<_>>();
                (i, hit)
            })
            .filter(|(_, h)| !h.is_empty())
            .collect();
        assert_eq!(
            candidates(&files, r"meshes\x.nif", &m.slots).0,
            scan,
            "{md}"
        );
        shapes += 1;
    }
    println!(
        "index = full scan for {shapes} owner diffuse paths ({:?})",
        t.elapsed()
    );
}

/// Fix round 2 (B-P38): the first In game build of a 20-shape mesh against a 5000-entry JSON tests only each
/// shape's indexed candidates, never every entry, on the UI thread.
#[test]
fn the_first_in_game_build_tests_only_indexed_entries() {
    let d = pg_files();
    put(d.path(), "mods/A/meshes/x.nif", Some(&lit_shapes(20)));
    let mut big: Vec<String> = (0..5000)
        .map(|i| format!("{{\"match_diffuse\": \"set{i}\\\\tex{i}\"}}"))
        .collect();
    big.push("{\"match_diffuse\": \"white\", \"roughness_scale\": 0.2}".into());
    let json = format!("[{}]", big.join(","));
    put(
        d.path(),
        "mods/A/PBRNIFPatcher/big.json",
        Some(json.as_bytes()),
    );
    let mut m = pg_session_in(d.path());
    in_game(&mut m);
    let e = m.edit.as_ref().unwrap();
    assert_eq!(e.sim_runs, 20);
    // per shape: one candidate in each of big.json, x.json and Low's a.json (4 files read, bad.json dropped)
    assert_eq!(e.match_tests, 20 * 3, "a full scan would test 20 x 5003");
    let shape = e.model.shapes[0].block;
    assert_eq!(
        notes(&m, shape)[0],
        r"In game: PGPatcher applies PBRNIFPatcher\big.json."
    );
}

#[test]
fn controllers_tab_names_pbr_variables_as_the_shader_tab_does() {
    let mut m = direct(ctrl_scene_vars(0x82, [9, 10]));
    let s = select(&mut m, "S");
    // Tall, so the Add controller row is on screen.
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 1800.0))
        .build_ui_state(
            |ui, m: &mut MeshesModel| sdc_gui::ui::meshes::show(ui, m),
            m,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run_steps(2);
    tab(&mut h, "Controllers");
    h.get_by_role_and_label(AkRole::ComboBox, "Glossiness variable");
    h.get_by_role_and_label(AkRole::ComboBox, "Specular strength variable");
    set_slsf2(&mut h, s, 1 << 23);
    assert!(h.query_by_label("Glossiness").is_none());
    h.get_by_role_and_label(AkRole::ComboBox, "Specular level variable");
    h.get_by_role_and_label(AkRole::ComboBox, "Roughness scale variable");
    // The Add controller list offers the PBR names too.
    h.get_by_role_and_label(AkRole::ComboBox, "Add controller variable")
        .click();
    h.run_steps(3);
    h.get_by_role_and_label(AkRole::Button, "Specular level (9)");
    h.get_by_role_and_label(AkRole::Button, "Roughness scale (10)");
}

#[test]
fn controllers_tab_names_lscc_specular_colour_by_pbr_material() {
    let cases = [
        (1u32 << 23, "Specular colour (no effect on PBR)"),
        ((1 << 23) | (1 << 24), "Coat colour"),
        ((1 << 23) | (1 << 26), "Subsurface colour"),
        (1 << 24, "Specular colour"), // not PBR: bits 24 and 26 mean nothing
    ];
    for (bits, want) in cases {
        let mut m = direct(ctrl_scene(0x82));
        let s = select(&mut m, "S");
        let e = m.edit.as_ref().unwrap();
        let at = nif::edit::anchor_role(&e.model, Some(s), Role::Shader).unwrap();
        let key = |time: f32, value: [f32; 3]| Key {
            time,
            value,
            forward: None,
            backward: None,
            tbc: None,
        };
        m.edit_push(
            vec![Op::AddController {
                at,
                class: "BSLightingShaderPropertyColorController".into(),
                var: Some(0),
                keys: KeyData::Pos(KeyGroup {
                    key_type: Some(1),
                    keys: vec![key(0.0, [0.2, 0.4, 0.6]), key(1.0, [0.8, 0.6, 0.4])],
                }),
                flags: 0x48,
                freq: 1.0,
                start: 0.0,
                stop: 1.0,
            }],
            None,
        );
        let mut h = view(m);
        h.run_steps(2);
        set_slsf2(&mut h, s, bits);
        tab(&mut h, "Controllers");
        h.get_by_role_and_label(AkRole::ComboBox, &format!("{want} variable"));
    }
}

// ---- B-P39: guards from the B9 in-game controller check ----

const ENV_PBR: &str = "Env map scale crashes the game on PBR materials.";

/// An exportable session on `b` (temp edit mod).
fn exportable(b: Vec<u8>) -> (tempfile::TempDir, NifEdit) {
    let d = tempfile::tempdir().unwrap();
    let edit_mod = d.path().join("mods").join(EDIT_MOD);
    let e = NifEdit::open(b, copy("A", 1, false), "meshes/x.nif", Some(&edit_mod)).unwrap();
    (d, e)
}

fn pbr_on(e: &NifEdit) -> Op {
    let at = nif::edit::anchor_role(&e.model, Some(0), Role::Shader).unwrap();
    let base = e.model.lighting[&1].lighting.slsf2;
    Op::SetBits {
        at,
        field: Field::Slsf2,
        mask: 1 << 23,
        value: 1 << 23,
        base,
    }
}

#[test]
fn env_map_scale_on_pbr_blocks_export_after_a_switch_and_in_the_source() {
    // The shape turns PBR after the var-8 controller exists.
    let (_d, mut e) = exportable(ctrl_scene_vars(0x82, [8, 20]));
    assert!(e.warnings.iter().all(|w| w.text != ENV_PBR));
    let op = pbr_on(&e);
    e.push(vec![op], None);
    assert_eq!(e.export_block().as_deref(), Some(ENV_PBR));
    // A file that already has it: an unrelated edit still cannot export.
    let (_d2, mut e2) = exportable(e.patched.to_vec());
    let at = nif::edit::anchor_role(&e2.model, Some(0), Role::Shader).unwrap();
    e2.push(
        vec![Op::SetField {
            at,
            field: Field::Glossiness,
            value: Value::F32(50f32.to_bits()),
            base: Value::F32(80f32.to_bits()),
        }],
        None,
    );
    assert_eq!(e2.export_block().as_deref(), Some(ENV_PBR));
}

#[test]
fn controllers_tab_shows_the_b9_guards() {
    // Var 0 (refraction) and var 10 on a shape whose Specular flag is off.
    let mut m = direct(ctrl_scene_vars(0x82, [0, 10]));
    select(&mut m, "S");
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    h.get_by_label("Refraction strength has no visible effect in game.");
    h.get_by_label("This controller has no effect: Specular is off when the mesh loads.");
    // Two controllers on one variable.
    let mut m = direct(ctrl_scene_vars(0x82, [11, 11]));
    select(&mut m, "S");
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    h.get_by_label("Two controllers drive Emissive multiple: only the last one plays.");
    // Var 8 on PBR: the red error under the controller.
    let mut m = direct(ctrl_scene_vars(0x82, [8, 20]));
    let s = select(&mut m, "S");
    let mut h = view(m);
    h.run_steps(2);
    set_slsf2(&mut h, s, 1 << 23);
    tab(&mut h, "Controllers");
    h.get_by_label(ENV_PBR);
}

#[test]
fn add_controller_offers_no_env_map_scale_on_pbr() {
    let mut m = direct(ctrl_scene(0x82));
    let s = select(&mut m, "S");
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 1800.0))
        .build_ui_state(
            |ui, m: &mut MeshesModel| sdc_gui::ui::meshes::show(ui, m),
            m,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run_steps(2);
    tab(&mut h, "Controllers");
    h.get_by_role_and_label(AkRole::ComboBox, "Add controller variable")
        .click();
    h.run_steps(3);
    let env = h.get_by_label("Env map scale (8)");
    assert!(!env.accesskit_node().is_disabled());
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    set_slsf2(&mut h, s, 1 << 23);
    h.get_by_role_and_label(AkRole::ComboBox, "Add controller variable")
        .click();
    h.run_steps(3);
    let env = h.get_by_label("Env map scale (8)");
    assert!(env.accesskit_node().is_disabled());
    env.hover();
    h.run_steps(30);
    h.get_by_label(ENV_PBR);
}

/// B-P40: the Variable combo of an existing controller offers no env map scale on PBR either.
#[test]
fn variable_combo_offers_no_env_map_scale_on_pbr() {
    let mut m = direct(ctrl_scene(0x82));
    let s = select(&mut m, "S");
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    h.get_by_role_and_label(AkRole::ComboBox, "Emissive multiple variable")
        .click();
    h.run_steps(3);
    assert!(!h
        .get_by_label("Env map scale (8)")
        .accesskit_node()
        .is_disabled());
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    set_slsf2(&mut h, s, 1 << 23);
    h.get_by_role_and_label(AkRole::ComboBox, "Emissive multiple variable")
        .click();
    h.run_steps(3);
    let env = h.get_by_label("Env map scale (8)");
    assert!(env.accesskit_node().is_disabled());
    env.hover();
    h.run_steps(30);
    h.get_by_label(ENV_PBR);
    // A controller that already has var 8 still shows it, with its error.
    let mut m = direct(ctrl_scene_vars(0x82, [8, 20]));
    let s = select(&mut m, "S");
    let mut h = view(m);
    h.run_steps(2);
    set_slsf2(&mut h, s, 1 << 23);
    tab(&mut h, "Controllers");
    h.get_by_role_and_label(AkRole::ComboBox, "Env map scale variable");
    h.get_by_label(ENV_PBR);
}

// ---- B-P41: the var-8 crash through PGPatcher (final review I1) ----

const PG_ENV: &str =
    "PGPatcher makes this shape PBR: its Env map scale controller will crash the game.";

/// x.nif is not PBR, so the editor adds var 8; A's x.json makes the shape PBR in game. In game shows a red line, the
/// edit view a Warning from the cached simulation, and export stays open (PGPatcher's output depends on play-time JSON).
#[test]
fn env_map_scale_warns_when_pgpatcher_makes_the_shape_pbr() {
    let (_d, mut m) = pg_session();
    let e = m.edit.as_ref().unwrap();
    let (shape, _) = lit_shape(e);
    let at = nif::edit::anchor_role(&e.model, Some(shape), Role::Shader).unwrap();
    m.edit_push(
        vec![Op::AddController {
            at,
            class: "BSLightingShaderPropertyFloatController".into(),
            var: Some(8),
            keys: KeyData::Float(KeyGroup {
                key_type: Some(1),
                keys: vec![fkey(0.0, 1.0, None), fkey(1.0, 0.5, None)],
            }),
            flags: 0x48,
            freq: 1.0,
            start: 0.0,
            stop: 1.0,
        }],
        None,
    );
    m.select_edit(Selection::Shape(shape));
    let e = m.edit.as_ref().unwrap();
    assert!(e.doc.ops.iter().all(|d| d.status == OpStatus::Applied));
    assert!(
        notes_or_none(&m, shape).is_none(),
        "Edited source: no In game rows"
    );
    in_game(&mut m);
    let n = notes(&m, shape);
    assert!(n.iter().any(|l| l == PG_ENV), "{n:?}");
    // Back on Edited source: the Warning under the controller comes from the cached simulation.
    let runs = m.edit.as_ref().unwrap().sim_runs;
    m.set_view_source(true);
    let e = m.edit.as_ref().unwrap();
    assert_eq!(e.sim_runs, runs, "no new simulation");
    assert_eq!(e.blocked, None, "export is not blocked");
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    h.get_by_label(PG_ENV);
    // Without the controller, the Warning goes.
    let m = h.state_mut();
    let role = Role::Ctrl {
        owner: nif::edit::Owner::Shader,
        class: "BSLightingShaderPropertyFloatController".into(),
        var: Some(8),
        nth: 0,
    };
    let at = nif::edit::anchor_role(&m.edit.as_ref().unwrap().model, Some(shape), role).unwrap();
    m.edit_push(vec![Op::RemoveController { at }], None);
    h.run_steps(3);
    assert!(h.query_by_label(PG_ENV).is_none());
}

/// Note CU7 (Phase B parked): a var-8 controller added after the In game simulation was cached still gets the In
/// game red line and the Edited source Warning, from the cache.
#[test]
fn env_map_scale_added_after_the_simulation_is_cached_still_warns() {
    let (_d, mut m) = pg_session();
    in_game(&mut m);
    m.set_view_source(true);
    let e = m.edit.as_ref().unwrap();
    assert!(e.sim_runs > 0, "the simulation is cached");
    let (shape, _) = lit_shape(e);
    let at = nif::edit::anchor_role(&e.model, Some(shape), Role::Shader).unwrap();
    m.edit_push(
        vec![Op::AddController {
            at,
            class: "BSLightingShaderPropertyFloatController".into(),
            var: Some(8),
            keys: KeyData::Float(KeyGroup {
                key_type: Some(1),
                keys: vec![fkey(0.0, 1.0, None), fkey(1.0, 0.5, None)],
            }),
            flags: 0x48,
            freq: 1.0,
            start: 0.0,
            stop: 1.0,
        }],
        None,
    );
    let e = m.edit.as_ref().unwrap();
    assert!(
        e.pg_warnings.iter().any(|w| w.text == PG_ENV),
        "{:?}",
        e.pg_warnings
    );
    in_game(&mut m);
    let n = notes(&m, shape);
    assert!(n.iter().any(|l| l == PG_ENV), "{n:?}");
}

fn notes_or_none(m: &MeshesModel, b: u32) -> Option<Vec<String>> {
    let rows = m.edit.as_ref().unwrap().in_game.clone()?;
    rows.into_iter().find(|(id, _, _)| id.0 == b).map(|r| r.2)
}

// ---- B-P41: key-drag cost on a many-controller owner NIF (final review minor 5) ----

/// One key-drag tick (`push` with the drag's merge key: `reapply`) on the owner NIF with the most lighting controllers a
/// scan of the loose meshes\effects, actors and magic folders found (2026-10-21): vanilla fxtg09nocturnalbirds.nif,
/// 608 controllers and 2 sequences. The loose extract of it is disabled, so it is read from the vanilla mesh BSAs
/// (the winning copy). Release build; not a gate.
#[test]
#[ignore]
fn key_drag_tick_cost_on_a_many_controller_owner_nif() {
    let Ok(root) = std::env::var("SDC_ACHERUS") else {
        println!("skipped: set SDC_ACHERUS");
        return;
    };
    const PATH: &str = r"meshes\effects\fxtg09nocturnalbirds.nif";
    let data = Path::new(&root).join("Stock Game").join("Data");
    let bytes = ["Skyrim - Meshes0.bsa", "Skyrim - Meshes1.bsa"]
        .iter()
        .find_map(|n| {
            let b = vfs::bsa::Bsa::open(&data.join(n)).ok()?;
            let e = b
                .entries
                .iter()
                .find(|e| e.norm.replace('/', "\\").eq_ignore_ascii_case(PATH))?;
            b.read(e).ok()
        })
        .expect("the NIF in the vanilla mesh BSAs");
    let mut e = NifEdit::open(bytes, copy("A", 1, false), "meshes/x.nif", None).unwrap();
    // The first controller (free or in a clip) with two float or colour keys or more, on a shape's shader or alpha.
    let (at, clip, keys) = e
        .keys
        .iter()
        .filter_map(|((c, clip), k)| match k {
            Ok(k @ KeyData::Float(g)) if g.keys.len() >= 2 => Some((*c, *clip, k.clone())),
            Ok(k @ KeyData::Pos(g)) if g.keys.len() >= 2 => Some((*c, *clip, k.clone())),
            _ => None,
        })
        .find_map(|(c, clip, k)| {
            let m = &e.model;
            let (s, owner, prop) = m.shapes.iter().find_map(|s| {
                let lit = s.shader.and_then(|b| m.lighting.get(&b)).map(|l| &l.prop);
                let alpha = s.alpha.and_then(|b| m.alphas.get(&b)).map(|a| &a.prop);
                [
                    (nif::edit::Owner::Shader, lit),
                    (nif::edit::Owner::Alpha, alpha),
                ]
                .into_iter()
                .find_map(|(o, p)| p.filter(|p| p.ctrls.contains(&c)).map(|p| (s, o, p)))
            })?;
            let cm = &m.ctrls[&c];
            let i = prop.ctrls.iter().position(|&b| b == c)?;
            let nth = prop.ctrls[..i]
                .iter()
                .filter_map(|b| m.ctrls.get(b))
                .filter(|p| p.class == cm.class && p.var == cm.var)
                .count() as u32;
            let at = Anchor {
                block: c,
                kind: cm.class.clone(),
                shape: s.name.as_deref().map(str::to_string),
                role: Role::Ctrl {
                    owner,
                    class: cm.class.clone(),
                    var: cm.var,
                    nth,
                },
            };
            Some((at, clip, k))
        })
        .expect("a keyed controller");
    let tick = |v: f32| {
        let mut k = keys.clone();
        match &mut k {
            KeyData::Float(g) => g.keys[1].value = v,
            KeyData::Pos(g) => g.keys[1].value[0] = v,
            KeyData::Bool(_) => unreachable!(),
        }
        Op::SetKeys {
            at: at.clone(),
            clip,
            clip_name: None,
            keys: k,
            base: keys.clone(),
        }
    };
    let merge = Some(nif::edit::merge_key(&tick(0.0)));
    e.push(vec![tick(0.0)], merge.clone());
    let n = 20;
    let t = Instant::now();
    for i in 1..=n {
        e.push(vec![tick(i as f32 * 0.01)], merge.clone());
    }
    let per = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
    assert_eq!(e.doc.ops.len(), 1, "one drag, one op");
    assert_eq!(e.outcomes, [nif::edit::Outcome::Applied]);
    println!(
        "{} controllers, {} sequences, {} shapes, clip {clip:?}: {per:.2} ms per drag tick",
        e.model.ctrls.len(),
        e.model.sequences.len(),
        e.model.shapes.len()
    );
}

// ---- Key fixes: stale key errors (Note CM33), Add key after the last key (B-P43), stop follows keys (B-P44) ----

fn ctrl_times(m: &MeshesModel, ctrl: u32) -> (f32, f32) {
    let c = &m.edit.as_ref().unwrap().model.ctrls[&ctrl];
    (c.start, c.stop)
}

/// keyed_pair's ctrl 2 (Emissive multiple: LINEAR (0, 0) (1, 1), start 0, stop 1) in the Controllers tab.
fn keyed_view() -> Harness<'static, MeshesModel> {
    let mut m = direct(scenes::keyed_pair(scenes::Share::No, [1, 2]));
    select(&mut m, "S");
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    h
}

/// A scanned session (export folder, real shapes, the clock) on `with_uv_ctrl`, in the Controllers tab of the
/// shape whose chain starts with the U offset controller (LINEAR (0, 0) (1, 1), start 0, stop 1).
fn uv_view() -> (tempfile::TempDir, Harness<'static, MeshesModel>) {
    let (d, mut m, _cmds) = session_on(&with_uv_ctrl(&fixture("TestNifFile_Skinned_SE.nif")));
    let (shape, _) = lit_shape(m.edit.as_ref().unwrap());
    m.select_edit(Selection::Shape(shape));
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    (d, h)
}

fn click_add_key(h: &mut Harness<'static, MeshesModel>) {
    h.get_by_role_and_label(AkRole::Button, "Add key to Emissive multiple")
        .click();
    h.run_steps(3);
}

/// A SetKeys that a later SetKeys replaced is not validated: fixing the times clears the error everywhere.
#[test]
fn stale_key_order_error_clears_when_keys_are_fixed() {
    const ERR: &str = "Key times must not decrease";
    let (_d, mut h) = uv_view();
    edit_number(&mut h, "U offset key 2 time", "-1");
    assert!(h.query_all_by_label(ERR).count() > 0);
    assert_eq!(
        h.state().edit.as_ref().unwrap().export_block().as_deref(),
        Some(ERR)
    );
    edit_number(&mut h, "U offset key 2 time", "2");
    h.run_steps(2);
    assert_eq!(h.query_all_by_label(ERR).count(), 0);
    let e = h.state().edit.as_ref().unwrap();
    assert!(e.op_warnings.is_empty(), "{:?}", e.op_warnings);
    assert_ne!(e.export_block().as_deref(), Some(ERR));
}

/// B-P43: Add key appends at the last time plus the average spacing, with the last key's value.
#[test]
fn add_key_appends_after_the_last_key() {
    let mut h = keyed_view();
    for _ in 0..3 {
        click_add_key(&mut h);
    }
    let g = float_keys(h.state(), 2);
    let got: Vec<(f32, f32)> = g.keys.iter().map(|k| (k.time, k.value)).collect();
    assert_eq!(
        got,
        [(0.0, 0.0), (1.0, 1.0), (2.0, 1.0), (3.0, 1.0), (4.0, 1.0)]
    );
}

/// B-P43/B-P44: past the stop, the same undo step moves the stop to the new key.
#[test]
fn add_key_past_stop_raises_stop_in_one_step() {
    let mut h = keyed_view();
    click_add_key(&mut h);
    assert_eq!(ctrl_times(h.state(), 2), (0.0, 2.0));
    let got = ops(h.state());
    assert_eq!(got.len(), 2, "{got:?}");
    assert!(
        matches!(&got[1], Op::SetField { field: Field::StopTime, value: Value::F32(v), .. } if f32::from_bits(*v) == 2.0),
        "{got:?}"
    );
    assert!(h.state_mut().undo_edit());
    assert!(ops(h.state()).is_empty());
    assert_eq!(ctrl_times(h.state(), 2), (0.0, 1.0));
    assert_eq!(float_keys(h.state(), 2).keys.len(), 2);
}

/// B-P43: the old rule (the cursor, or half-way between the last two keys) gave 1 on (0, 1, 1) and disabled Add
/// key with "A key is already at this time."; now the key lands after the last one.
#[test]
fn add_key_never_duplicates_a_time() {
    let mut h = keyed_view();
    {
        let m = h.state_mut();
        let e = m.edit.as_ref().unwrap();
        let mut g = float_keys(m, 2);
        let base = KeyData::Float(g.clone());
        g.keys.push(g.keys[1].clone());
        let role = Role::Ctrl {
            owner: nif::edit::Owner::Shader,
            class: "BSLightingShaderPropertyFloatController".into(),
            var: Some(11),
            nth: 0,
        };
        let at = nif::edit::anchor_role(&e.model, Some(0), role).unwrap();
        let op = Op::SetKeys {
            at,
            clip: None,
            clip_name: None,
            keys: KeyData::Float(g),
            base,
        };
        m.edit_push(vec![op], None);
    }
    h.run_steps(2);
    assert!(h.query_by_label("A key is already at this time.").is_none());
    click_add_key(&mut h);
    let t: Vec<f32> = float_keys(h.state(), 2)
        .keys
        .iter()
        .map(|k| k.time)
        .collect();
    assert_eq!(t.len(), 4, "{t:?}");
    assert!(t[3] > t[2], "{t:?}");
    assert!(t[..3].iter().all(|&x| x != t[3]), "{t:?}");
}

/// B-P44: dragging the last key moves the stop with it; one undo reverts both.
#[test]
fn stop_follows_the_last_key_on_drag() {
    let mut h = keyed_view();
    // The key 2 handle (t 1, the right end) dragged left: a time drag of several ticks, one merged step.
    let c = h.get_by_label("Emissive multiple key 2").rect().center();
    h.hover_at(c);
    h.run_steps(1);
    h.drag_at(c);
    h.run_steps(1);
    for k in 1..=3 {
        h.hover_at(c - egui::vec2(40.0 * k as f32 / 3.0, 0.0));
        h.run_steps(1);
    }
    h.drop_at(c - egui::vec2(40.0, 0.0));
    h.run_steps(3);
    let last = float_keys(h.state(), 2).keys[1].time;
    assert!(last < 1.0, "{last} {:?}", ops(h.state()));
    assert_eq!(ctrl_times(h.state(), 2), (0.0, last));
    assert_eq!(ops(h.state()).len(), 2, "{:?}", ops(h.state()));
    assert!(h.state_mut().undo_edit());
    assert!(ops(h.state()).is_empty());
    assert_eq!(ctrl_times(h.state(), 2), (0.0, 1.0));
}

/// B-P44: removing the last key brings the stop back to the new last key; a first key moved before the start
/// pulls the start with it.
#[test]
fn remove_last_key_shrinks_stop() {
    let mut h = keyed_view();
    click_add_key(&mut h);
    assert_eq!(ctrl_times(h.state(), 2).1, 2.0);
    h.get_by_label("Remove Emissive multiple key 3").click();
    h.run_steps(3);
    assert_eq!(ctrl_times(h.state(), 2), (0.0, 1.0));
    edit_number(&mut h, "Emissive multiple key 1 time", "-0.5");
    assert_eq!(ctrl_times(h.state(), 2), (-0.5, 1.0));
}

/// B-P44: a clip's keys never move its sequence's start and stop.
#[test]
fn clip_stop_is_not_changed() {
    let plain = with_uv_ctrl(&fixture("TestNifFile_Skinned_SE.nif"));
    let (nif, seq) = with_clip(&plain);
    let mut m = direct(nif);
    m.edit_with(|e| e.set_playing(Playing::Clip(seq)));
    let (shape, _) = lit_shape(m.edit.as_ref().unwrap());
    m.select_edit(Selection::Shape(shape));
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    h.get_by_role_and_label(AkRole::Button, "Add key to Emissive multiple in Glow")
        .click();
    h.run_steps(3);
    let got = ops(h.state());
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(matches!(&got[0], Op::SetKeys { clip: Some(_), .. }));
    let e = h.state().edit.as_ref().unwrap();
    let s = e.model.sequences.iter().find(|s| s.block == seq).unwrap();
    assert_eq!((s.start, s.stop), (0.0, 1.0));
}

/// The owner's "end time stayed at 1": a typed Stop is stored, and the clock's range follows it without a reopen.
#[test]
fn typed_stop_is_stored_and_moves_the_clock_range() {
    let (_d, mut h) = uv_view();
    edit_number(&mut h, "U offset stop", "3");
    let e = h.state().edit.as_ref().unwrap();
    let stops: Vec<f32> = e.model.ctrls.values().map(|c| c.stop).collect();
    assert!(stops.contains(&3.0), "{stops:?}");
    assert_eq!(e.anim_range(), Some((0.0, 3.0)));
    h.state_mut().tick_anim(0.0);
    assert_eq!(h.state().v3.clock.map(|c| c.range), Some((0.0, 3.0)));
}

/// B-P44: the stop op comes and goes during one drag (the last key passes the stop and comes back); the drag stays
/// one undo step, the stop op keeps the stop from before the drag as its base and leaves when it is not needed.
#[test]
fn a_drag_whose_stop_op_comes_and_goes_is_one_step() {
    let mut m = direct(scenes::keyed_pair(scenes::Share::No, [1, 2]));
    let base = KeyData::Float(float_keys(&m, 2));
    let e = m.edit.as_mut().unwrap();
    let role = Role::Ctrl {
        owner: nif::edit::Owner::Shader,
        class: "BSLightingShaderPropertyFloatController".into(),
        var: Some(11),
        nth: 0,
    };
    let at = nif::edit::anchor_role(&e.model, Some(0), role).unwrap();
    let keys = |t: f32| {
        let KeyData::Float(mut g) = base.clone() else {
            unreachable!()
        };
        g.keys[1].time = t;
        Op::SetKeys {
            at: at.clone(),
            clip: None,
            clip_name: None,
            keys: KeyData::Float(g),
            base: base.clone(),
        }
    };
    let stop = |to: f32, from: f32| Op::SetField {
        at: at.clone(),
        field: Field::StopTime,
        value: Value::F32(to.to_bits()),
        base: Value::F32(from.to_bits()),
    };
    let merge = Some(nif::edit::merge_key(&keys(1.0)));
    e.push(vec![keys(1.0)], merge.clone());
    e.push(vec![keys(1.5), stop(1.5, 1.0)], merge.clone());
    // The UI reads the base from the drag's op: here a later tick names the current stop, which push replaces.
    e.push(vec![keys(2.0), stop(2.0, 1.5)], merge.clone());
    let got: Vec<Op> = e.doc.ops().to_vec();
    assert_eq!(got.len(), 2, "{got:?}");
    assert_eq!(got[1], stop(2.0, 1.0), "the base from before the drag");
    assert_eq!(e.model.ctrls[&2].stop, 2.0);
    e.push(vec![keys(1.0)], merge.clone());
    assert_eq!(e.doc.ops().len(), 1, "{:?}", e.doc.ops());
    assert_eq!(e.model.ctrls[&2].stop, 1.0);
    e.flush();
    assert!(e.undo());
    assert!(e.doc.ops().is_empty());
    assert!(!e.history.can_undo());
}

/// keyed_view with ctrl 2's keys replaced by LINEAR keys at `times` (value 1), as one pushed SetKeys.
fn keyed_view_at(times: &[f32]) -> Harness<'static, MeshesModel> {
    let mut h = keyed_view();
    let m = h.state_mut();
    let e = m.edit.as_ref().unwrap();
    let g = float_keys(m, 2);
    let base = KeyData::Float(g.clone());
    let keys = times
        .iter()
        .map(|&time| Key {
            time,
            value: 1.0,
            forward: None,
            backward: None,
            tbc: None,
        })
        .collect();
    let role = Role::Ctrl {
        owner: nif::edit::Owner::Shader,
        class: "BSLightingShaderPropertyFloatController".into(),
        var: Some(11),
        nth: 0,
    };
    let op = Op::SetKeys {
        at: nif::edit::anchor_role(&e.model, Some(0), role).unwrap(),
        clip: None,
        clip_name: None,
        keys: KeyData::Float(KeyGroup {
            key_type: g.key_type,
            keys,
        }),
        base,
    };
    m.edit_push(vec![op], None);
    h.run_steps(2);
    h
}

/// B-P45: at 2^21 a 0.1 step rounds back to the last time; Add key takes the next f32 above it instead.
#[test]
fn add_key_past_a_large_time_still_increases() {
    let mut h = keyed_view_at(&[2097152.0]);
    click_add_key(&mut h);
    click_add_key(&mut h);
    let t: Vec<f32> = float_keys(h.state(), 2)
        .keys
        .iter()
        .map(|k| k.time)
        .collect();
    assert_eq!(t.len(), 3, "{t:?}");
    assert!(t.windows(2).all(|w| w[1] > w[0]), "{t:?}");
    assert!(t.iter().all(|x| x.is_finite()), "{t:?}");
}

/// B-P45: no finite time is above f32::MAX: Add key is off and says why on hover.
#[test]
fn add_key_is_off_when_no_later_time_exists() {
    let mut h = keyed_view_at(&[0.0, f32::MAX]);
    let b = h.get_by_role_and_label(AkRole::Button, "Add key to Emissive multiple");
    assert!(b.accesskit_node().is_disabled());
    b.hover();
    h.run();
    h.get_by_label("No later time is possible for a new key.");
    assert_eq!(float_keys(h.state(), 2).keys.len(), 2);
}

// ---- v2 C5: clips in the Controllers tab ----

const CONVERT_SHADER: &str = "Convert shader controllers to clips";

/// `animated(Some(1), 2)` with "S" selected: free LSCF var 11 "Emissive multiple" (4) and var 20 "U offset" (5).
fn clip_scene() -> MeshesModel {
    let mut m = direct(scenes::animated(Some(1), 2));
    select(&mut m, "S");
    m
}

/// Converts S's shader chain into the clip "SpecialIdle"; returns the sequence block.
fn convert(m: &mut MeshesModel) -> u32 {
    let e = m.edit.as_ref().unwrap();
    let s = shape_named(e, "S");
    let at = nif::edit::anchor_role(&e.model, Some(s), Role::Shader).unwrap();
    m.edit_push(
        vec![Op::ConvertChainToClips {
            at,
            name: nif::edit::CONVERT_NAME.into(),
        }],
        None,
    );
    let e = m.edit.as_ref().unwrap();
    assert_eq!(e.outcomes, vec![nif::edit::Outcome::Applied]);
    e.model.sequences[0].block
}

/// `label` is disabled, and hovering it shows `why`.
fn disabled_with(h: &mut Harness<'static, MeshesModel>, label: &str, why: &str) {
    reveal(h, label);
    let n = h.get_by_label(label);
    assert!(n.accesskit_node().is_disabled(), "{label} is enabled");
    let c = n.rect().center();
    h.hover_at(c);
    h.run_steps(3);
    h.get_by_label(why);
}

/// Scrolls `label` into the panel's view and clicks it (the clip rows sit below the fold at 900 px).
fn click(h: &mut Harness<'static, MeshesModel>, label: &str) {
    reveal(h, label);
    h.get_by_label(label).click();
    h.run_steps(3);
}

/// `scroll_to_me` animates: step until the node has not moved for three frames (at most 60).
fn reveal(h: &mut Harness<'static, MeshesModel>, label: &str) {
    if h.query_by_label(label).is_none() {
        open_clips(h); // Meshes UI T4: clip controls sit in the closed Clips section
    }
    h.get_by_label(label).scroll_to_me();
    let (mut last, mut still) = (h.get_by_label(label).rect(), 0);
    for _ in 0..60 {
        h.run_steps(1);
        let r = h.get_by_label(label).rect();
        still = if r == last { still + 1 } else { 0 };
        if still == 3 {
            break;
        }
        last = r;
    }
}

fn enabled(h: &Harness<'static, MeshesModel>, label: &str) -> bool {
    !h.get_by_label(label).accesskit_node().is_disabled()
}

fn clip_names(m: &MeshesModel) -> Vec<Option<String>> {
    let e = m.edit.as_ref().unwrap();
    e.model
        .sequences
        .iter()
        .map(|s| s.name.as_deref().map(str::to_string))
        .collect()
}

fn managed(m: &MeshesModel, c: u32) -> bool {
    m.edit.as_ref().unwrap().model.managed(c)
}

#[test]
fn convert_button_makes_one_undo_step_and_lists_the_clip() {
    let mut h = view(clip_scene());
    h.run_steps(2);
    tab(&mut h, "Controllers");
    open_clips(&mut h);
    assert!(enabled(&h, CONVERT_SHADER));
    click(&mut h, CONVERT_SHADER);
    h.run_steps(3);
    let got = ops(h.state());
    assert!(
        matches!(got.as_slice(), [Op::ConvertChainToClips { name, .. }] if name == "SpecialIdle"),
        "{got:?}"
    );
    let e = h.state().edit.as_ref().unwrap();
    assert_eq!(e.outcomes, vec![nif::edit::Outcome::Applied]);
    assert_eq!(clip_names(h.state()), vec![Some("SpecialIdle".into())]);
    h.get_by_label("Preview animation");
    assert!(managed(h.state(), 4) && managed(h.state(), 5));
    h.state_mut().undo_edit();
    h.run_steps(3);
    assert!(ops(h.state()).is_empty());
    assert!(clip_names(h.state()).is_empty());
    assert!(h.query_by_label("Preview animation").is_none());
    assert!(
        !h.state().edit.as_ref().unwrap().history.can_undo(),
        "one step"
    );
    assert!(enabled(&h, CONVERT_SHADER));
}

#[test]
fn rename_clip_updates_the_selector() {
    let mut m = clip_scene();
    let seq = convert(&mut m);
    m.edit_with(|e| e.set_playing(Playing::Clip(seq)));
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    open_clips(&mut h);
    edit_number(&mut h, "Clip name", "Open");
    let got = ops(h.state());
    assert!(
        matches!(got.last(), Some(Op::RenameSequence { name, nth: 0, new_name, .. })
            if name == "SpecialIdle" && new_name == "Open"),
        "{got:?}"
    );
    let e = h.state().edit.as_ref().unwrap();
    assert_eq!(e.outcomes.last(), Some(&nif::edit::Outcome::Applied));
    assert_eq!(e.playing, Playing::Clip(seq), "the same block stays shown");
    assert_eq!(
        h.get_by_label("Preview animation").accesskit_node().value(),
        Some("Open".into())
    );
}

#[test]
fn remove_clip_returns_to_free_chains_when_none_left() {
    let mut m = clip_scene();
    let seq = convert(&mut m);
    m.edit_with(|e| e.set_playing(Playing::Clip(seq)));
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    open_clips(&mut h);
    click(&mut h, "Remove clip SpecialIdle");
    h.run_steps(3);
    let got = ops(h.state());
    assert!(
        matches!(got.last(), Some(Op::RemoveSequence { name, nth: 0, .. }) if name == "SpecialIdle"),
        "{got:?}"
    );
    let e = h.state().edit.as_ref().unwrap();
    assert_eq!(e.outcomes.last(), Some(&nif::edit::Outcome::Applied));
    assert!(e.model.sequences.is_empty());
    assert_eq!(e.playing, Playing::Free);
    // The scene's shape has no geometry, so the clock has no range here (anim_range() is None before the convert
    // too); the free chains' own timing is back instead.
    assert_eq!(e.anim_range(), clip_scene().edit.unwrap().anim_range());
    for c in [4, 5] {
        let t = &e.model.ctrls[&c];
        assert_eq!((t.start, t.stop, t.flags & 0x20), (0.0, 1.0, 0), "{c}");
    }
    assert!(h.query_by_label("Preview animation").is_none());
    assert!(!managed(h.state(), 4) && !managed(h.state(), 5));
    assert!(enabled(&h, CONVERT_SHADER));
}

#[test]
fn clip_actions_are_disabled_on_free_chains() {
    let mut m = clip_scene();
    convert(&mut m);
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    open_clips(&mut h);
    assert!(h.get_by_label("Clip name").accesskit_node().is_disabled());
    disabled_with(
        &mut h,
        "Remove clip",
        "Pick a clip in Preview to rename or remove it.",
    );
}

#[test]
fn convert_is_disabled_with_a_reason() {
    // (a) two timings
    let mut m = clip_scene();
    let at = Anchor {
        block: 5,
        kind: "BSLightingShaderPropertyFloatController".into(),
        shape: Some("S".into()),
        role: Role::Ctrl {
            owner: nif::edit::Owner::Shader,
            class: "BSLightingShaderPropertyFloatController".into(),
            var: Some(20),
            nth: 0,
        },
    };
    m.edit_push(
        vec![Op::SetField {
            at,
            field: Field::Frequency,
            value: Value::F32(2.0f32.to_bits()),
            base: Value::F32(1.0f32.to_bits()),
        }],
        None,
    );
    assert_eq!(
        m.edit.as_ref().unwrap().outcomes,
        vec![nif::edit::Outcome::Applied]
    );
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    open_clips(&mut h);
    disabled_with(
        &mut h,
        CONVERT_SHADER,
        "These controllers run on different timings: they cannot share one clip.",
    );
    // (b) the root is the shape
    let mut m = direct(scenes::shader_ctrl(false));
    select(&mut m, "S");
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    open_clips(&mut h);
    disabled_with(
        &mut h,
        CONVERT_SHADER,
        "The root is not a node: a clip needs a node root.",
    );
    // (c) a shared alpha
    let mut m = direct(scenes::shared_alpha_with_testref_chain());
    select(&mut m, "A");
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    open_clips(&mut h);
    disabled_with(
        &mut h,
        "Convert alpha controllers to clips",
        "This alpha is shared by 2 shapes: split it before editing.",
    );
}

#[test]
fn new_clip_drives_the_picked_controllers() {
    let mut h = view(clip_scene());
    h.run_steps(2);
    tab(&mut h, "Controllers");
    open_clips(&mut h);
    disabled_with(&mut h, "Add clip", "Type a name for the clip.");
    edit_number(&mut h, "New clip name", "Open");
    disabled_with(&mut h, "Add clip", "Tick at least one controller.");
    let keys = h.state().edit.as_ref().unwrap().keys[&(5, None)].clone();
    click(&mut h, "Add U offset to the new clip");
    h.run_steps(3);
    click(&mut h, "Add clip");
    h.run_steps(3);
    let got = ops(h.state());
    let Some(Op::AddSequence {
        name,
        cycle,
        start,
        stop,
        tracks,
        ..
    }) = got.last()
    else {
        panic!("{got:?}")
    };
    assert_eq!(
        (name.as_str(), *cycle, *start, *stop),
        ("Open", 2, 0.0, 1.0)
    );
    assert_eq!(tracks.len(), 1, "{tracks:?}");
    assert_eq!(tracks[0].at.block, 5);
    assert_eq!(Ok(tracks[0].keys.clone()), keys);
    let e = h.state().edit.as_ref().unwrap();
    assert_eq!(e.outcomes.last(), Some(&nif::edit::Outcome::Applied));
    assert!(managed(h.state(), 5) && !managed(h.state(), 4));
}

/// C-P35 I2: New clip on a controller whose own keys are TBC starts flat, so the clip never blocks Export NIF.
#[test]
fn new_clip_on_tbc_keys_starts_flat_without_an_error() {
    let b = scenes::animated(Some(1), 2);
    let m = nif::model::read_model(&b).unwrap();
    let role = Role::Ctrl {
        owner: nif::edit::Owner::Shader,
        class: "BSLightingShaderPropertyFloatController".into(),
        var: Some(20),
        nth: 0,
    };
    let s = m
        .shapes
        .iter()
        .find(|s| s.name.as_deref() == Some("S"))
        .unwrap()
        .block;
    let at = nif::edit::anchor_role(&m, Some(s), role).unwrap();
    let Ok(KeyData::Float(base)) = nif::edit::keys_of(&b, &m, 5, None) else {
        panic!("float keys")
    };
    let mut tbc = base.clone();
    tbc.key_type = Some(3);
    for k in &mut tbc.keys {
        k.tbc = Some([0.0; 3]);
    }
    let op = Op::SetKeys {
        at,
        clip: None,
        clip_name: None,
        keys: KeyData::Float(tbc),
        base: KeyData::Float(base),
    };
    let r = nif::edit::apply(&b, &[op]).unwrap();
    assert_eq!(r.outcomes, vec![nif::edit::Outcome::Applied]);
    let mut m = direct(r.bytes);
    select(&mut m, "S");
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    open_clips(&mut h);
    edit_number(&mut h, "New clip name", "Open");
    let tick = "Add U offset to the new clip";
    reveal(&mut h, tick);
    let c = h.get_by_label(tick).rect().center();
    h.hover_at(c);
    h.run_steps(3);
    h.get_by_label("Its keys are not linear or quadratic: the clip starts flat.");
    click(&mut h, tick);
    h.run_steps(3);
    click(&mut h, "Add clip");
    h.run_steps(3);
    let got = ops(h.state());
    let Some(Op::AddSequence { tracks, .. }) = got.last() else {
        panic!("{got:?}")
    };
    assert!(
        matches!(&tracks[0].keys, KeyData::Float(g) if g.key_type == Some(1)),
        "{:?}",
        tracks[0].keys
    );
    let e = h.state().edit.as_ref().unwrap();
    assert_eq!(e.outcomes.last(), Some(&nif::edit::Outcome::Applied));
    let errors: Vec<_> = e
        .op_warnings
        .iter()
        .filter(|w| w.level == nif::edit::validate::Level::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
}

/// C-P35 M6: the New clip ticks belong to one file and shape; opening another file clears them.
#[test]
fn new_clip_ticks_clear_when_the_file_changes() {
    let mut h = view(clip_scene());
    h.run_steps(2);
    tab(&mut h, "Controllers");
    open_clips(&mut h);
    let tick = "Add U offset to the new clip";
    click(&mut h, tick);
    h.run_steps(3);
    let on = |h: &Harness<'static, MeshesModel>| {
        h.get_by_label(tick).accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
    };
    assert!(on(&h), "ticked");
    // Same path, same bytes, same shape block: only the mod differs.
    let mut other = MeshesModel::empty();
    other.edit = Some(
        NifEdit::open(
            scenes::animated(Some(1), 2),
            copy("B", 2, false),
            "meshes/x.nif",
            None,
        )
        .unwrap(),
    );
    select(&mut other, "S");
    *h.state_mut() = other;
    h.run_steps(3);
    tab(&mut h, "Controllers");
    open_clips(&mut h);
    reveal(&mut h, tick);
    assert!(!on(&h), "another file starts with nothing ticked");
}

#[test]
fn a_refused_clip_name_records_nothing_and_says_why() {
    let mut m = clip_scene();
    convert(&mut m);
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    open_clips(&mut h);
    edit_number(&mut h, "New clip name", "specialidle");
    click(&mut h, "Add Emissive multiple to the new clip");
    click(&mut h, "Add clip");
    h.run_steps(3);
    assert_eq!(ops(h.state()).len(), 1, "nothing recorded");
    h.get_by_label("Another clip is already named specialidle.");
}

#[test]
fn clips_ui_fits_the_panel_at_1100() {
    let mut m = clip_scene();
    let seq = convert(&mut m);
    m.edit_with(|e| e.set_playing(Playing::Clip(seq)));
    let mut h = Harness::builder()
        .with_size(egui::vec2(1100.0, 800.0))
        .build_ui_state(
            |ui, m: &mut MeshesModel| sdc_gui::ui::meshes::show(ui, m),
            m,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run_steps(2);
    tab(&mut h, "Controllers");
    open_clips(&mut h);
    let panel = egui::containers::panel::PanelState::load(&h.ctx, egui::Id::new("meshes-edit"))
        .expect("the edit panel")
        .outer_rect;
    for l in [
        "Clip name",
        "Remove clip SpecialIdle",
        CONVERT_SHADER,
        "Add clip",
    ] {
        let r = h.get_by_label(l).rect();
        assert!(
            r.max.x <= panel.max.x + 0.5,
            "{l}: {r:?} past the edit panel's right edge {}",
            panel.max.x
        );
    }
}

/// `clip_scene` converted, with the clip picked in Preview and the Controllers tab open.
fn playing_clip_view() -> Harness<'static, MeshesModel> {
    let mut m = clip_scene();
    let seq = convert(&mut m);
    m.edit_with(|e| e.set_playing(Playing::Clip(seq)));
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    open_clips(&mut h);
    h
}

/// C-P33 (rule 5.2): Enter on a focused Remove clip button removes nothing; a mouse click still does.
#[test]
fn enter_never_removes_a_clip() {
    let mut h = playing_clip_view();
    reveal(&mut h, "Remove clip SpecialIdle");
    h.get_by_label("Remove clip SpecialIdle").focus();
    h.run_steps(2);
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert_eq!(ops(h.state()).len(), 1, "{:?}", ops(h.state()));
    assert_eq!(clip_names(h.state()), vec![Some("SpecialIdle".into())]);
    click(&mut h, "Remove clip SpecialIdle");
    assert!(
        matches!(ops(h.state()).last(), Some(Op::RemoveSequence { .. })),
        "a click still removes"
    );
}

/// C-P33 (C5-D3): a rename refusal clears as soon as the name changes; the change alone records nothing.
#[test]
fn a_rename_refusal_clears_when_the_name_changes() {
    let mut h = playing_clip_view();
    h.get_by_label("Clip name").focus();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.key_press(egui::Key::Backspace);
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    h.get_by_label("Type a name for the clip.");
    h.get_by_label("Clip name").focus();
    h.run_steps(2);
    h.get_by_label("Clip name").type_text("Open");
    h.run_steps(3);
    assert!(h.query_by_label("Type a name for the clip.").is_none());
    assert_eq!(ops(h.state()).len(), 1, "typing records nothing");
}

/// C-P33: a clip with a null Name says so instead of "not on the root's manager".
#[test]
fn a_nameless_clip_says_it_has_no_name() {
    let mut m = clip_scene();
    let seq = convert(&mut m);
    let mut b = m.edit.as_ref().unwrap().patched.to_vec();
    let at = nif::header::Header::parse(&b).unwrap().offsets()[seq as usize];
    b[at..at + 4].copy_from_slice(&(-1i32).to_le_bytes()); // the sequence's Name index
    let mut m = direct(b);
    select(&mut m, "S");
    m.edit_with(|e| e.set_playing(Playing::Clip(seq)));
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    click(&mut h, &format!("Remove clip Clip {seq}"));
    assert!(ops(h.state()).is_empty());
    h.get_by_label("This clip has no name: it cannot be renamed or removed here.");
}

// ---- Note CU: parked NIF editor fixes ----

/// `clip_scene` with two added clips, "Open" on U offset (5) then "Close" on Emissive multiple (4); returns the
/// sequence blocks (Open, Close).
fn two_clips() -> (MeshesModel, u32, u32) {
    two_clips_named("Open", "Close")
}

/// `two_clips` with the clips named `first` (on U offset) and `second` (on Emissive multiple).
fn two_clips_named(first: &str, second: &str) -> (MeshesModel, u32, u32) {
    let mut m = clip_scene();
    let e = m.edit.as_ref().unwrap();
    let s = shape_named(e, "S");
    let root = nif::edit::anchor_role(&e.model, None, Role::Root).unwrap();
    let track = |var: u32| {
        let role = Role::Ctrl {
            owner: nif::edit::Owner::Shader,
            class: "BSLightingShaderPropertyFloatController".into(),
            var: Some(var),
            nth: 0,
        };
        nif::edit::ClipTrack {
            at: nif::edit::anchor_role(&e.model, Some(s), role).unwrap(),
            keys: KeyData::Float(KeyGroup {
                key_type: Some(1),
                keys: vec![fkey(0.0, 0.0, None), fkey(1.0, 1.0, None)],
            }),
        }
    };
    let add = |name: &str, var: u32| Op::AddSequence {
        at: root.clone(),
        name: name.into(),
        cycle: 2,
        freq: 1.0,
        start: 0.0,
        stop: 1.0,
        tracks: vec![track(var)],
    };
    let ops = vec![add(first, 20), add(second, 11)];
    for op in ops {
        m.edit_push(vec![op], None);
    }
    let e = m.edit.as_ref().unwrap();
    assert!(e.outcomes.iter().all(|o| *o == nif::edit::Outcome::Applied));
    // appended in order: the first clip has the lower block
    let mut blocks: Vec<u32> = e.model.sequences.iter().map(|q| q.block).collect();
    blocks.sort();
    (m, blocks[0], blocks[1])
}

/// Fix round 1 (review I2): clip identity is the Windows-1252 name, so "é" and "ö" never pass for each other, and a
/// rename to "Café" keeps the Preview on that clip.
#[test]
fn preview_tells_non_ascii_clip_names_apart() {
    let (mut m, first, _) = two_clips_named("é", "ö");
    m.edit_with(|e| e.set_playing(Playing::Clip(first)));
    assert_eq!(shown_clip(&m), Some(Some("é".into())));
    m.edit_with(|e| e.revert(0));
    assert_eq!(shown_clip(&m), None, "Free chains, not ö");
    // rename the picked clip to Café
    let (mut m, first, _) = two_clips_named("Open", "Close");
    m.edit_with(|e| e.set_playing(Playing::Clip(first)));
    let e = m.edit.as_ref().unwrap();
    let (name, nth) = nif::edit::clip_key(&e.patched, &e.model, first).unwrap();
    let at = nif::edit::anchor_role(&e.model, None, Role::Root).unwrap();
    m.edit_push(
        vec![Op::RenameSequence {
            at,
            name,
            nth,
            new_name: "Café".into(),
        }],
        None,
    );
    assert_eq!(shown_clip(&m), Some(Some("Café".into())));
}

/// The name of the clip the Preview shows, None on Free chains.
fn shown_clip(m: &MeshesModel) -> Option<Option<String>> {
    let e = m.edit.as_ref().unwrap();
    match e.playing {
        Playing::Free => None,
        Playing::Clip(b) => Some(
            e.model
                .sequences
                .iter()
                .find(|s| s.block == b)
                .and_then(|s| s.name.as_deref().map(str::to_string)),
        ),
    }
}

/// Note CU8 (C-P36): removing the last clip takes the manager off the root's chain; undo puts it back.
#[test]
fn removing_the_last_clip_unlinks_the_manager_and_undo_restores_it() {
    let mut m = clip_scene();
    convert(&mut m);
    let root_ctrl = |m: &MeshesModel| {
        let e = m.edit.as_ref().unwrap();
        let c = e.model.root_net.as_ref().unwrap().controller;
        let h = nif::header::Header::parse(&e.patched).unwrap();
        (c >= 0).then(|| String::from_utf8_lossy(h.kind(c as usize)).into_owned())
    };
    assert_eq!(root_ctrl(&m).as_deref(), Some("NiControllerManager"));
    let at = nif::edit::anchor_role(&m.edit.as_ref().unwrap().model, None, Role::Root).unwrap();
    m.edit_push(
        vec![Op::RemoveSequence {
            at,
            name: nif::edit::CONVERT_NAME.into(),
            nth: 0,
        }],
        None,
    );
    let e = m.edit.as_ref().unwrap();
    assert!(e.outcomes.iter().all(|o| *o == nif::edit::Outcome::Applied));
    assert_eq!(root_ctrl(&m), None, "no manager on the root");
    assert!(m
        .edit
        .as_ref()
        .unwrap()
        .warnings
        .iter()
        .all(|w| w.text != nif::edit::validate::EMPTY_MANAGER));
    m.undo_edit();
    assert_eq!(root_ctrl(&m).as_deref(), Some("NiControllerManager"));
}

/// Note CU1: a revert that moves another clip onto the picked clip's block never shows that other clip.
#[test]
fn preview_never_switches_clips_after_a_revert() {
    // Open picked, its AddSequence reverted: Close now sits on Open's block
    let (mut m, open, _) = two_clips();
    m.edit_with(|e| e.set_playing(Playing::Clip(open)));
    m.edit_with(|e| e.revert(0));
    assert_eq!(
        m.edit.as_ref().unwrap().model.sequences[0].name.as_deref(),
        Some("Close")
    );
    assert_eq!(shown_clip(&m), None, "Free chains, not Close");
    // Close picked, Open's AddSequence reverted: Free, or Close itself
    let (mut m, _, close) = two_clips();
    m.edit_with(|e| e.set_playing(Playing::Clip(close)));
    m.edit_with(|e| e.revert(0));
    let got = shown_clip(&m);
    assert!(
        got.is_none() || got == Some(Some("Close".into())),
        "{got:?}"
    );
    // undo brings Open back on its block: the Preview does not jump to it
    m.undo_edit();
    let got = shown_clip(&m);
    assert!(
        got.is_none() || got == Some(Some("Close".into())),
        "{got:?}"
    );
}

/// Note CU2: every change of the edited bytes gives a new revision, so a refusal line never comes back on a later
/// state (the `Arc` address could be reused).
#[test]
fn every_apply_gives_a_new_revision() {
    let mut m = clip_scene();
    let mut seen = vec![m.edit.as_ref().unwrap().rev];
    let mut note = |m: &MeshesModel| {
        let r = m.edit.as_ref().unwrap().rev;
        assert!(!seen.contains(&r), "{r} seen before: {seen:?}");
        seen.push(r);
    };
    convert(&mut m);
    note(&m);
    m.undo_edit();
    note(&m);
    m.redo_edit();
    note(&m);
    m.edit_with(|e| e.revert(0));
    note(&m);
    m.undo_edit();
    note(&m);
}

/// Note CU2: a refused New clip line goes on undo and stays gone on redo.
#[test]
fn a_refusal_line_goes_on_undo_and_stays_gone() {
    let mut m = clip_scene();
    convert(&mut m);
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    edit_number(&mut h, "New clip name", "specialidle");
    click(&mut h, "Add Emissive multiple to the new clip");
    click(&mut h, "Add clip");
    h.run_steps(3);
    let line = "Another clip is already named specialidle.";
    h.get_by_label(line);
    h.state_mut().undo_edit();
    h.run_steps(3);
    assert!(h.query_by_label(line).is_none(), "gone after undo");
    h.state_mut().redo_edit();
    h.run_steps(3);
    assert!(h.query_by_label(line).is_none(), "still gone after redo");
}

/// Note CU3: Convert to clips is off, with AddSequence's reason, for cycle bits 3 and an empty range.
#[test]
fn convert_is_disabled_for_cycle_3_and_an_empty_range() {
    let patch = |b: &mut Vec<u8>, off: usize, v: &[u8]| {
        let offs = nif::header::Header::parse(b).unwrap().offsets().to_vec();
        for c in [4, 5] {
            b[offs[c] + off..offs[c] + off + v.len()].copy_from_slice(v);
        }
    };
    let mut cycle3 = scenes::animated(Some(1), 2);
    patch(&mut cycle3, 4, &0x4Eu16.to_le_bytes());
    let mut empty = scenes::animated(Some(1), 2);
    patch(&mut empty, 18, &0f32.to_le_bytes());
    for (b, why) in [
        (cycle3, "The cycle must be Loop, Reverse or Clamp."),
        (empty, "Stop must be after start."),
    ] {
        let mut m = direct(b);
        select(&mut m, "S");
        let mut h = view(m);
        h.run_steps(2);
        tab(&mut h, "Controllers");
        disabled_with(&mut h, CONVERT_SHADER, why);
    }
}

/// Note CU6 (C-P35 M6, shape half): the New clip ticks clear when another shape is selected.
#[test]
fn new_clip_ticks_clear_when_the_shape_changes() {
    let mut m = direct(scenes::two_animated());
    select(&mut m, "S");
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Controllers");
    let tick = "Add Emissive multiple to the new clip";
    let on = |h: &Harness<'static, MeshesModel>| {
        h.get_by_label(tick).accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
    };
    click(&mut h, tick);
    assert!(on(&h), "ticked on S");
    let t = shape_named(h.state().edit.as_ref().unwrap(), "T");
    h.state_mut().select_edit(Selection::Shape(t));
    h.run_steps(3);
    tab(&mut h, "Controllers");
    reveal(&mut h, tick);
    assert!(!on(&h), "T starts with nothing ticked");
    let s = shape_named(h.state().edit.as_ref().unwrap(), "S");
    h.state_mut().select_edit(Selection::Shape(s));
    h.run_steps(3);
    tab(&mut h, "Controllers");
    reveal(&mut h, tick);
    assert!(!on(&h), "back on S, the old tick is gone");
}

/// C5 screenshots (egui-ui-rules step 5): the Controllers tab with a clip picked at 1100, 1280 and 1600 px, saved
/// to `SDC_C5_SHOTS` (default: the temp dir). Needs a GPU:
/// `cargo test -p sdc-gui --test nif_edit -- --ignored render_the_clips_tab`.
#[test]
#[ignore]
fn render_the_clips_tab() {
    let out = std::env::var("SDC_C5_SHOTS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    for w in [1100.0, 1280.0, 1600.0] {
        let mut m = clip_scene();
        let seq = convert(&mut m);
        m.edit_with(|e| e.set_playing(Playing::Clip(seq)));
        let mut h = Harness::builder()
            .with_size(egui::vec2(w, 1000.0))
            .wgpu()
            .build_ui_state(
                |ui, m: &mut MeshesModel| sdc_gui::ui::meshes::show(ui, m),
                m,
            );
        sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
        h.run_steps(2);
        tab(&mut h, "Controllers");
        for (part, label) in [("top", "Clip name"), ("bottom", "Add clip")] {
            reveal(&mut h, label);
            let img = h.render().expect("render");
            img.save(out.join(format!("c5-clips-{w}-{part}.png")))
                .expect("save png");
        }
    }
}

// ---- Note CS: SLSF1 bits that need a shader type (owner) ----

const SKIN_TINT: &str =
    "Skin tint flag needs the Skin Tint shader type: the game crashes otherwise.";
const FACEGEN_DETAIL: &str =
    "FaceGen detail flag needs the Face Tint or Skin Tint shader type: the mesh renders black otherwise.";

/// Shape "S" (0) on a BSLSP (1) of `shader_type` with SLSF1 = Zbuffer_Test | `bits`.
fn typed(shader_type: u32, bits: u32) -> Vec<u8> {
    let trailer: &[f32] = if shader_type == 5 { &[1.0; 3] } else { &[] };
    let mut b = scenes::lsp_shape(shader_type, trailer);
    let h = nif::header::Header::parse(&b).unwrap();
    let (rel, _) = nif::fields::locate(&h, &b, 1, Field::Slsf1).unwrap();
    let at = h.offsets()[1] + rel;
    b[at..at + 4].copy_from_slice(&((1u32 << 31) | bits).to_le_bytes());
    b
}

fn slsf1(m: &MeshesModel) -> u32 {
    m.edit.as_ref().unwrap().model.lighting[&1].lighting.slsf1
}

#[test]
fn a_type_bound_flag_shows_its_error_and_clears_in_one_undo_step() {
    let mut m = direct(typed(0, 1 << 21));
    select(&mut m, "S");
    let mut h = view(m);
    h.run_steps(2);
    // Meshes UI T6: a flag list with a warning starts open (its Fix in sight).
    h.get_by_label(SKIN_TINT);
    h.get(
        egui_kittest::kittest::By::new()
            .role(AkRole::Button)
            .label_contains("Clear the flag: "),
    )
    .scroll_to_me();
    h.run();
    h.get(
        egui_kittest::kittest::By::new()
            .role(AkRole::Button)
            .label_contains("Clear the flag: "),
    )
    .click();
    h.run_steps(2);
    assert!(h.query_by_label(SKIN_TINT).is_none());
    let m = h.state_mut();
    assert_eq!(ops(m).len(), 1);
    assert_eq!(slsf1(m), 1 << 31);
    assert!(m.undo_edit());
    assert!(ops(m).is_empty(), "the fix is one undo step");
    // Face Tint without the detail flag offers "Set the flag".
    let mut m = direct(typed(4, 0));
    select(&mut m, "S");
    let mut h = view(m);
    h.run_steps(2);
    h.get(
        egui_kittest::kittest::By::new()
            .role(AkRole::Button)
            .label_contains("Set the flag: "),
    )
    .scroll_to_me();
    h.run();
    h.get(
        egui_kittest::kittest::By::new()
            .role(AkRole::Button)
            .label_contains("Set the flag: "),
    )
    .click();
    h.run_steps(2);
    assert_eq!(slsf1(h.state()), (1 << 31) | (1 << 10));
}

#[test]
fn a_shader_type_change_clears_the_flag_in_the_same_undo_step() {
    let mut m = direct(typed(5, 1 << 21));
    select(&mut m, "S");
    let mut h = view(m);
    h.run_steps(2);
    h.get_by_role_and_label(AkRole::ComboBox, "Shader type")
        .click();
    h.run_steps(3);
    h.get_by_label("Default (0)").click();
    h.run_steps(3);
    h.get_by_label("Skin tint flag cleared: it needs the Skin Tint shader type.");
    assert!(h.query_by_label(SKIN_TINT).is_none());
    let m = h.state_mut();
    let got = ops(m);
    assert_eq!(got.len(), 2, "{got:?}");
    assert_eq!(slsf1(m), 1 << 31);
    assert_eq!(
        m.edit.as_ref().unwrap().model.lighting[&1]
            .lighting
            .shader_type,
        0
    );
    assert!(m.undo_edit());
    assert!(ops(m).is_empty(), "type and flag are one undo step");
    assert!(m.edit.as_ref().unwrap().notice.is_none());
}

#[test]
fn a_type_change_off_11_clears_the_mlp_flag_in_the_same_undo_step() {
    // Note DJ: type 11 with SLSF2 24, changed to Environment Map
    let mut b = scenes::lsp_shape(11, &[5.0, 1.0, 1.0, 1.0, 1.0]);
    let hd = nif::header::Header::parse(&b).unwrap();
    let (rel, _) = nif::fields::locate(&hd, &b, 1, Field::Slsf2).unwrap();
    let at = hd.offsets()[1] + rel;
    b[at..at + 4].copy_from_slice(&((1u32 << 24) | 1).to_le_bytes());
    let mut m = direct(b);
    select(&mut m, "S");
    let mut h = view(m);
    h.run_steps(2);
    h.get_by_role_and_label(AkRole::ComboBox, "Shader type")
        .click();
    h.run_steps(3);
    h.get_by_label("Environment Map (1)").click();
    h.run_steps(3);
    h.get_by_label("Multi-layer parallax flag cleared: it needs shader type 11.");
    let m = h.state_mut();
    let got = ops(m);
    assert_eq!(got.len(), 2, "{got:?}");
    let l = &m.edit.as_ref().unwrap().model.lighting[&1].lighting;
    assert_eq!((l.shader_type, l.slsf2), (1, 1));
    assert!(m.undo_edit());
    assert!(ops(m).is_empty(), "type and flag are one undo step");
}
#[test]
fn flags_that_need_another_type_are_greyed_with_the_reason() {
    let mut m = direct(typed(0, 0));
    select(&mut m, "S");
    let mut h = view(m);
    h.run_steps(2);
    open_flags(&mut h); // Meshes UI T6: the flag lists start closed
    let skin = h.get_by_label("FaceGen_RGB_Tint");
    assert!(skin.accesskit_node().is_disabled());
    // Note DK
    assert!(h
        .get_by_label("Hair_Soft_Lighting")
        .accesskit_node()
        .is_disabled());
    assert!(h
        .get_by_label("Facegen_Detail_Map")
        .accesskit_node()
        .is_disabled());
    h.get_by_label("FaceGen_RGB_Tint").scroll_to_me();
    h.run();
    h.get_by_label("FaceGen_RGB_Tint").hover();
    h.run_steps(30);
    h.get_by_label("Needs the Skin Tint shader type.");
    // On its own type the flag is editable.
    let mut m = direct(typed(5, 1 << 21));
    select(&mut m, "S");
    let mut h = view(m);
    h.run_steps(2);
    open_flags(&mut h);
    assert!(!h
        .get_by_label("FaceGen_RGB_Tint")
        .accesskit_node()
        .is_disabled());
    // CS3: Skin Tint may carry the detail flag too.
    assert!(!h
        .get_by_label("Facegen_Detail_Map")
        .accesskit_node()
        .is_disabled());
}

#[test]
fn a_type_bound_flag_blocks_export_even_in_an_untouched_source() {
    let (_d, mut e) = exportable(typed(0, 1 << 10));
    let at = nif::edit::anchor_role(&e.model, Some(0), Role::Shader).unwrap();
    e.push(
        vec![Op::SetField {
            at,
            field: Field::Glossiness,
            value: Value::F32(50f32.to_bits()),
            base: Value::F32(80f32.to_bits()),
        }],
        None,
    );
    assert_eq!(e.export_block().as_deref(), Some(FACEGEN_DETAIL));
}

// ---- Meshes UI T5b: heel height (Note CZ19) ----

/// The heel scene on the Extra data tab with shape `name` selected and the heel entry picked.
fn heel_view(name: &str, sibling: Option<&str>) -> Harness<'static, MeshesModel> {
    let mut m = direct(scenes::heel_scene());
    select(&mut m, name);
    m.edit.as_mut().unwrap().weight_sibling = sibling.map(String::from);
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Extra data");
    pick_known(&mut h, false, "Heel height (RaceMenu)");
    h
}

/// The extra-data ops of the last push, as (kind or "edit", name, value).
fn heel_ops(h: &Harness<'static, MeshesModel>) -> Vec<Op> {
    ops(h.state())
}

/// A shape with an SDTA and no HH_OFFSET: Set adds HH_OFFSET and moves the SDTA's NPC Z with it (one undo step); an
/// earlier shape (A) already has heel data, so the row says it wins.
#[test]
fn heel_height_adds_hh_offset_and_moves_the_sdta() {
    let mut h = heel_view("B", None);
    h.get_by_label("An earlier shape's heel height wins.");
    let v = h.get_by_role_and_label(AkRole::SpinButton, "Heel height (shape)");
    assert_eq!(
        v.accesskit_node().numeric_value(),
        Some(5.0),
        "starts at the SDTA's Z"
    );
    edit_number(&mut h, "Heel height (shape)", "8.5");
    h.get_by_role_and_label(AkRole::Button, "Set heel height")
        .click();
    h.run_steps(4);
    let got = heel_ops(&h);
    assert_eq!(got.len(), 2, "{got:?}");
    assert!(
        matches!(&got[0], Op::AddExtra { kind, name, value: ExtraValue::Float(b), .. }
            if kind == "NiFloatExtraData" && name == "HH_OFFSET" && f32::from_bits(*b) == 8.5),
        "{got:?}"
    );
    let Op::EditExtra {
        value: ExtraValue::Str(s),
        ..
    } = &got[1]
    else {
        panic!("{got:?}")
    };
    assert_eq!(sdta_npc_z(s), Some(8.5));
    // Final review Minor 3: both ops apply, and the patched bytes carry HH_OFFSET = 8.5 and the SDTA's 8.5 on B.
    let e = h.state().edit.as_ref().unwrap();
    assert!(
        e.doc.ops.iter().all(|d| d.status == OpStatus::Applied),
        "{:?}",
        e.outcomes
    );
    let pm = nif::model::read_model(&e.patched).unwrap();
    let b = pm
        .shapes
        .iter()
        .find(|s| s.name.as_deref() == Some("B"))
        .unwrap();
    let extras = pm.extras_of(&b.net);
    let hh = extras
        .iter()
        .find(|x| x.name.as_deref() == Some(&b"HH_OFFSET"[..]))
        .unwrap();
    assert_eq!(hh.value, Some(ExtraValue::Float(8.5f32.to_bits())));
    let sdta = extras
        .iter()
        .find(|x| x.name.as_deref() == Some(&b"SDTA"[..]))
        .unwrap();
    let Some(ExtraValue::Str(written)) = &sdta.value else {
        panic!("{sdta:?}")
    };
    assert_eq!(sdta_npc_z(written), Some(8.5));
    let m = h.state_mut();
    assert!(m.undo_edit());
    assert!(ops(m).is_empty(), "one undo step");
}

/// A shape with HH_OFFSET: Set edits it in place (a second one is ignored by the game) and adds no SDTA.
#[test]
fn heel_height_edits_an_existing_hh_offset() {
    let mut h = heel_view("A", None);
    assert!(h
        .query_by_label("An earlier shape's heel height wins.")
        .is_none());
    edit_number(&mut h, "Heel height (shape)", "9");
    h.get_by_role_and_label(AkRole::Button, "Set heel height")
        .click();
    h.run_steps(4);
    let got = heel_ops(&h);
    assert!(
        matches!(&got[..], [Op::EditExtra { value: ExtraValue::Float(b), base: ExtraValue::Float(was), .. }]
            if f32::from_bits(*b) == 9.0 && f32::from_bits(*was) == 7.0),
        "{got:?}"
    );
}

/// A shape with no heel data: HH_OFFSET only, or HH_OFFSET and a new SDTA when the box is ticked; the weight-file
/// sibling is named.
#[test]
fn heel_height_on_a_bare_shape_adds_hh_and_an_optional_sdta() {
    let mut h = heel_view("C", Some("meshes/x_1.nif"));
    h.get_by_label("An earlier shape's heel height wins.");
    h.get_by_label("Apply the same value to the other weight file (meshes\\x_1.nif).");
    h.get_by_label("Also write SDTA (OStim and others read it)")
        .click();
    h.run_steps(2);
    edit_number(&mut h, "Heel height (shape)", "6");
    h.get_by_role_and_label(AkRole::Button, "Set heel height")
        .click();
    h.run_steps(4);
    let got = heel_ops(&h);
    assert_eq!(got.len(), 2, "{got:?}");
    let Op::AddExtra {
        kind,
        name,
        value: ExtraValue::Str(s),
        ..
    } = &got[1]
    else {
        panic!("{got:?}")
    };
    assert_eq!(
        (kind.as_str(), name.as_str()),
        ("NiStringExtraData", "SDTA")
    );
    assert_eq!(sdta_npc_z(s), Some(6.0));
}

/// On the root the entry is off, and the root's own SDTA row says the game ignores it there.
#[test]
fn heel_height_is_off_on_the_root_and_root_heel_data_warns() {
    let mut m = direct(scenes::heel_scene());
    select(&mut m, "C");
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Extra data");
    assert!(known_entry_disabled(&mut h, true, "Heel height (RaceMenu)"));
    h.get_by_label("The game ignores heel height here. Put it on a shape.");
}

/// Calculate fills the field from the lowest bind-pose vertex (`heel_z_of`); on a shape with no geometry it says so.
#[test]
fn heel_height_calculate_fills_the_field() {
    let bytes = fixture("TestNifFile_Skinned_SE.nif");
    let mut m = direct(bytes.clone());
    let e = m.edit.as_ref().unwrap();
    let (shape, _) = lit_shape(e);
    let want = heel_z_of(&bytes, shape).expect("a measurable shape");
    m.select_edit(Selection::Shape(shape));
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Extra data");
    pick_known(&mut h, false, "Heel height (RaceMenu)");
    h.get_by_role_and_label(AkRole::Button, "Calculate").click();
    h.run_steps(3);
    let v = h.get_by_role_and_label(AkRole::SpinButton, "Heel height (shape)");
    assert_eq!(v.accesskit_node().numeric_value(), Some(want as f64));
    let mut h = heel_view("C", None);
    h.get_by_role_and_label(AkRole::Button, "Calculate").click();
    h.run_steps(3);
    h.get_by_label("This shape has no geometry to measure: type the height.");
}

/// The weight-file sibling of a data path: `_0` ↔ `_1` before `.nif`, any case; None for other names.
#[test]
fn weight_sibling_names_the_other_weight_file() {
    assert_eq!(
        weight_sibling_of("meshes/armor/boots_0.nif").as_deref(),
        Some("meshes/armor/boots_1.nif")
    );
    assert_eq!(
        weight_sibling_of("meshes/armor/Boots_1.NIF").as_deref(),
        Some("meshes/armor/Boots_0.NIF")
    );
    assert_eq!(weight_sibling_of("meshes/armor/boots.nif"), None);
    assert_eq!(weight_sibling_of("meshes/armor/boots_10.nif"), None);
}

// ---- T5b review fix round 1 ----

fn heel_field(h: &Harness<'static, MeshesModel>) -> Option<f64> {
    h.get_by_role_and_label(AkRole::SpinButton, "Heel height (shape)")
        .accesskit_node()
        .numeric_value()
}

/// Important 1: after Set and an undo the field shows the restored value, and a typed value or a heel pick never
/// carries to the same block of another session.
#[test]
fn heel_field_follows_undo_and_stays_in_its_session() {
    let mut h = heel_view("A", None);
    edit_number(&mut h, "Heel height (shape)", "9");
    h.get_by_role_and_label(AkRole::Button, "Set heel height")
        .click();
    h.run_steps(4);
    assert_eq!(heel_field(&h), Some(9.0));
    assert!(h.state_mut().undo_edit());
    h.run_steps(4);
    assert_eq!(heel_field(&h), Some(7.0), "the restored HH_OFFSET");
    edit_number(&mut h, "Heel height (shape)", "4");
    // Another session on a file with the same block numbers: no heel pick, no typed 4.
    let m = h.state_mut();
    m.edit = Some(
        NifEdit::open(
            scenes::heel_scene(),
            copy("A", 1, false),
            "meshes/y.nif",
            None,
        )
        .unwrap(),
    );
    let a = shape_named(m.edit.as_ref().unwrap(), "A");
    m.select_edit(Selection::Shape(a));
    h.run_steps(4);
    assert_eq!(
        h.get_by_role_and_label(AkRole::ComboBox, "Known extra data (shape)")
            .accesskit_node()
            .value()
            .as_deref(),
        Some("Custom…")
    );
    pick_known(&mut h, false, "Heel height (RaceMenu)");
    assert_eq!(heel_field(&h), Some(7.0));
}

/// Important 2: HH_OFFSET and the SDTA's NPC Z get the same number (2.25, not 2.25 and 2.3).
#[test]
fn heel_hh_and_sdta_get_the_same_value() {
    let mut h = heel_view("B", None);
    edit_number(&mut h, "Heel height (shape)", "2.25");
    h.get_by_role_and_label(AkRole::Button, "Set heel height")
        .click();
    h.run_steps(4);
    let got = heel_ops(&h);
    let (
        Op::AddExtra {
            value: ExtraValue::Float(b),
            ..
        },
        Op::EditExtra {
            value: ExtraValue::Str(s),
            ..
        },
    ) = (&got[0], &got[1])
    else {
        panic!("{got:?}")
    };
    assert_eq!(f32::from_bits(*b), 2.25);
    assert_eq!(sdta_npc_z(s), Some(2.25));
}

/// Minor 5: an SDTA whose NPC Z already equals the height is left alone (no reformatting edit).
#[test]
fn heel_leaves_an_sdta_that_already_has_the_height() {
    let mut h = heel_view("B", None);
    assert_eq!(heel_field(&h), Some(5.0));
    h.get_by_role_and_label(AkRole::Button, "Set heel height")
        .click();
    h.run_steps(4);
    let got = heel_ops(&h);
    assert!(matches!(&got[..], [Op::AddExtra { .. }]), "{got:?}");
}

/// Minors 1 and 6: the "no geometry" note stays in its session; a negative height says it lowers the actor.
#[test]
fn heel_notes_for_no_geometry_and_a_negative_height() {
    let mut h = heel_view("C", None);
    h.get_by_role_and_label(AkRole::Button, "Calculate").click();
    h.run_steps(3);
    h.get_by_label("This shape has no geometry to measure: type the height.");
    edit_number(&mut h, "Heel height (shape)", "-1");
    h.get_by_label("A negative height lowers the actor.");
    let m = h.state_mut();
    m.edit = Some(
        NifEdit::open(
            scenes::heel_scene(),
            copy("A", 1, false),
            "meshes/y.nif",
            None,
        )
        .unwrap(),
    );
    let c = shape_named(m.edit.as_ref().unwrap(), "C");
    m.select_edit(Selection::Shape(c));
    h.run_steps(4);
    pick_known(&mut h, false, "Heel height (RaceMenu)");
    assert!(h
        .query_by_label("This shape has no geometry to measure: type the height.")
        .is_none());
}

/// Minor 3: an HH_OFFSET the editor cannot set (another type) blocks Set with a note instead of adding a second one.
#[test]
fn heel_refuses_an_hh_offset_of_another_type() {
    let mut m = direct(scenes::heel_scene());
    let c = select(&mut m, "C");
    let at = nif::edit::anchor_shape(&m.edit.as_ref().unwrap().model, c);
    m.edit_push(
        vec![Op::AddExtra {
            at,
            kind: "NiStringExtraData".into(),
            name: "HH_OFFSET".into(),
            value: ExtraValue::Str("7".into()),
        }],
        None,
    );
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Extra data");
    pick_known(&mut h, false, "Heel height (RaceMenu)");
    h.get_by_label(HEEL_HH_UNSET_TEXT);
    assert!(h
        .get_by_role_and_label(AkRole::Button, "Set heel height")
        .accesskit_node()
        .is_disabled());
}

/// The note for an HH_OFFSET that is not a readable float.
const HEEL_HH_UNSET_TEXT: &str =
    "This shape's HH_OFFSET is not a number the editor can set: remove that row first.";

/// T5b re-review gap: the heel row's "earlier shape" note counts only shapes the root reaches. Worn's own HH_OFFSET
/// is removed first; the only earlier heel data is then on the unlinked "Orphan" shape, so no note.
#[test]
fn heel_row_ignores_an_earlier_orphan_shape() {
    let mut m = direct(scenes::heel_orphan_scene());
    let worn = select(&mut m, "Worn");
    let at = Anchor {
        block: 4,
        kind: "NiFloatExtraData".into(),
        shape: Some("Worn".into()),
        role: Role::Extra {
            on_root: false,
            name: "HH_OFFSET".into(),
            kind: "NiFloatExtraData".into(),
            nth: 0,
        },
    };
    m.edit_push(vec![Op::RemoveExtra { at }], None);
    let e = m.edit.as_ref().unwrap();
    assert!(
        e.model
            .extras_of(&e.model.shape(worn).unwrap().net)
            .is_empty(),
        "{:?}",
        e.outcomes
    );
    let mut h = view(m);
    h.run_steps(2);
    tab(&mut h, "Extra data");
    pick_known(&mut h, false, "Heel height (RaceMenu)");
    assert!(h
        .query_by_label("An earlier shape's heel height wins.")
        .is_none());
}

// ---- Meshes UI T6: panel orientation (Note CZ21) ----

/// The panel names what the tabs act on, under the tab strip: the selected shape and its type, or the root.
#[test]
fn the_panel_says_which_shape_it_edits() {
    let mut m = direct(ctrl_scene(0x82));
    select(&mut m, "S");
    let mut h = view(m);
    h.run_steps(2);
    let line = h.get_by_label("Editing: S (BSTriShape)").rect();
    let tabs = h.get_by_label("Extra data tab").rect();
    assert!(
        line.min.y >= tabs.max.y,
        "under the tab strip: {line:?} {tabs:?}"
    );
    h.state_mut().select_edit(Selection::Root);
    h.run_steps(2);
    h.get_by_label("Editing: Root (NiNode)");
}

/// The Shader tab puts the texture slots first, and the SLSF1 / SLSF2 flag lists start closed.
#[test]
fn shader_tab_shows_slots_first_and_flags_closed() {
    let mut m = direct(scenes::shared_alpha_and_set());
    select(&mut m, "A");
    let mut h = view(m);
    h.run_steps(2);
    let slot = h
        .get_by_role_and_label(AkRole::TextInput, "Diffuse path (slot 0)")
        .rect();
    let ty = h
        .get_by_role_and_label(AkRole::ComboBox, "Shader type")
        .rect();
    assert!(slot.max.y <= ty.min.y, "slots first: {slot:?} {ty:?}");
    assert!(
        h.query_by_label("Specular").is_none(),
        "SLSF1 starts closed"
    );
    h.get_by_label_contains("SLSF1 0x").click();
    h.run_steps(3);
    h.get_by_label("Specular");
}

/// Meshes UI T6: opens the Shader tab's SLSF1 and SLSF2 flag lists (closed by default). T6 review Minor 4: a list
/// already open (its first flag is drawn) is left open.
fn open_flags(h: &mut Harness<'static, MeshesModel>) {
    use nif::material::{SLSF1_NAMES, SLSF2_NAMES};
    for (flags, first) in [("SLSF1 0x", SLSF1_NAMES[0]), ("SLSF2 0x", SLSF2_NAMES[0])] {
        if h.query_by_role_and_label(AkRole::CheckBox, first).is_some() {
            continue;
        }
        h.get_by_label_contains(flags).scroll_to_me();
        h.run_steps(12);
        h.get_by_label_contains(flags).click();
        h.run_steps(3);
    }
}

// ---- Meshes UI T7: rule fixes (Note CZ22) ----

/// The app in the Meshes view at `w` × `hgt` with x.nif's edit open on its first lit shape; `gpu` renders with wgpu.
fn app_edit(w: f32, hgt: f32, gpu: bool) -> (tempfile::TempDir, AppH) {
    use sdc_gui::app::SdcApp;
    app_data();
    let d = files(false);
    let model = sdc_gui::model::AppModel::new(
        sdc_gui::settings::GuiSettings::default(),
        None,
        support::session(),
    );
    let mut app = SdcApp::new(model);
    app.meshes.procs = sdc_gui::model::meshes::no_procs;
    app.meshes.store_path = Some(d.path().join("scan.db"));
    scan_and_select(&mut app.meshes, d.path());
    app.meshes.open_edit();
    let (shape, _) = lit_shape(app.meshes.edit.as_ref().unwrap());
    app.meshes.select_edit(Selection::Shape(shape));
    let mut frame = eframe::Frame::_new_kittest();
    let b = Harness::builder().with_size(egui::vec2(w, hgt));
    let b = if gpu { b.wgpu() } else { b };
    let mut h = b.build_ui_state(
        move |ui, app: &mut SdcApp| eframe::App::ui(app, ui, &mut frame),
        app,
    );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    sdc_gui::ui::header::set_meshes_view(&h.ctx, true);
    h.run_steps(6);
    (d, h)
}

fn panel_rect(h: &AppH, id: &str) -> egui::Rect {
    egui::containers::panel::PanelState::load(&h.ctx, egui::Id::new(id))
        .unwrap()
        .outer_rect
}

/// T7: at 1100 px with an edit open the left column gives way, so the 3D view keeps its 240 px minimum.
#[test]
fn the_3d_view_keeps_its_minimum_at_1100_with_an_edit_open() {
    let (_d, h) = app_edit(1100.0, 800.0, false);
    let (tree, edit) = (panel_rect(&h, "meshes-tree"), panel_rect(&h, "meshes-edit"));
    let frame3d = h
        .get_by_label(sdc_gui::model::viewport3d::FRAME_LABEL)
        .rect();
    assert!(
        frame3d.min.x >= tree.max.x && frame3d.max.x <= edit.min.x,
        "{tree:?} {frame3d:?} {edit:?}"
    );
    assert!(
        frame3d.width() >= 240.0 - 1.0,
        "3D view {frame3d:?} at 1100 px"
    );
    // T7 review Minor 1 (controller ruling): the column defaults to ~420 px and is capped near 40% of the width, so
    // the 3D view sits above its minimum at 1100 and gets the slack at 1280.
    assert!(frame3d.width() > 260.0, "3D view {frame3d:?} at 1100 px");
    assert!(tree.width() <= 0.4 * 1100.0 + 1.0, "tree {tree:?}");
    let (_d, h) = app_edit(1280.0, 800.0, false);
    let frame3d = h
        .get_by_label(sdc_gui::model::viewport3d::FRAME_LABEL)
        .rect();
    assert!(frame3d.width() > 380.0, "3D view {frame3d:?} at 1280 px");
}

/// T5 open item, checked in T7: the Known combo with the longest entry picked stays inside the edit panel at 1100 px.
#[test]
fn the_known_combo_fits_the_edit_panel_at_1100() {
    let (_d, mut h) = app_edit(1100.0, 800.0, false);
    h.get_by_label("Extra data tab").click();
    h.run_steps(4);
    h.get_by_role_and_label(AkRole::ComboBox, "Known extra data")
        .scroll_to_me();
    h.run_steps(12);
    h.get_by_role_and_label(AkRole::ComboBox, "Known extra data")
        .click();
    h.run_steps(4);
    h.get_by_role_and_label(AkRole::Button, "HDT Skinned Mesh Physics Object")
        .click();
    h.run_steps(4);
    let edit = panel_rect(&h, "meshes-edit");
    let combo = h
        .get_by_role_and_label(AkRole::ComboBox, "Known extra data")
        .rect();
    assert!(
        combo.max.x <= edit.max.x + 0.5,
        "combo {combo:?} past the panel {edit:?}"
    );
}

/// T7: the edit tabs are named as tabs, distinct from the fields and headings with the same words.
#[test]
fn the_edit_tabs_are_named_as_tabs() {
    let mut h = view(direct(ctrl_scene(0x82)));
    h.run_steps(2);
    for t in [
        "Shader tab",
        "Alpha tab",
        "Controllers tab",
        "Extra data tab",
    ] {
        h.get_by_label(t);
    }
}

/// T7: a repeated fix button is named after its warning; Take source / Keep mine after their edit.
#[test]
fn repeated_fix_and_conflict_buttons_have_unique_names() {
    let mut m = direct(typed(0, 1 << 21));
    select(&mut m, "S");
    let mut h = view(m);
    h.run_steps(2);
    h.get_by_role_and_label(
        AkRole::Button,
        &format!("Clear the flag: {SKIN_TINT} (SLSF1 flags)"),
    );
    // T7 review Minor 5: two fix buttons on one tab have two different names.
    let mut m = direct(typed(0, (1 << 21) | (1 << 10)));
    select(&mut m, "S");
    let mut h = view(m);
    h.run_steps(2);
    let names: Vec<String> = h
        .query_all_by_role(AkRole::Button)
        .filter_map(|n| n.accesskit_node().label())
        .filter(|l| l.starts_with("Clear the flag: "))
        .collect();
    assert_eq!(names.len(), 2, "{names:?}");
    assert_ne!(names[0], names[1]);
    // A slot path outside Data: its "Not relative to Data" warning carries a fix.
    let mut m = direct(scenes::shared_alpha_and_set());
    let a = select(&mut m, "A");
    let e = m.edit.as_ref().unwrap();
    let set = e.model.lighting[&e.model.shape(a).unwrap().shader.unwrap()]
        .texture_set
        .unwrap();
    let base = String::from_utf8_lossy(&e.model.texsets[&set].raw.slots[0]).into_owned();
    let at = nif::edit::anchor_shape(&e.model, a);
    m.edit_push(
        vec![Op::SetTexturePath {
            at,
            slot: 0,
            path: r"C:\Game\Data\textures\a.dds".into(),
            base,
        }],
        None,
    );
    let mut h = view(m);
    h.run_steps(2);
    h.get_by_role_and_label(AkRole::Button, "Fix slot 0: Not relative to Data");
    // A conflict: the source's own glossiness changes on disk under the edit.
    let (d, mut m) = export_session();
    m.open_edit();
    gloss(&mut m, 12.5);
    let e = m.edit.as_ref().unwrap();
    let (shape, shader) = lit_shape(e);
    let cur = nif::model::read_model(&e.source).unwrap().lighting[&shader]
        .lighting
        .params
        .glossiness;
    let theirs = nif::edit::apply(
        &e.source,
        &[Op::SetField {
            at: nif::edit::anchor_role(&e.model, Some(shape), Role::Shader).unwrap(),
            field: Field::Glossiness,
            value: Value::F32(7.0f32.to_bits()),
            base: Value::F32(cur.to_bits()),
        }],
    )
    .unwrap()
    .bytes;
    std::fs::write(d.path().join("mods/A/meshes/x.nif"), &theirs).unwrap();
    let _ = m.export_edit(None);
    assert!(m.edit.as_ref().unwrap().doc.has_conflicts());
    let mut h = view(m);
    h.run_steps(3);
    h.get(
        egui_kittest::kittest::By::new()
            .role(AkRole::Button)
            .label_contains("Take source 1: "),
    );
    h.get(
        egui_kittest::kittest::By::new()
            .role(AkRole::Button)
            .label_contains("Keep mine 1: "),
    );
}

/// T6 review Minor 3: the Editing line for an unnamed shape and for a selection that is not in the file.
#[test]
fn the_editing_line_names_unnamed_and_missing_shapes() {
    let mut m = direct(scenes::managed_clip_and_bare_shape());
    m.select_edit(Selection::Shape(12));
    let mut h = view(m);
    h.run_steps(2);
    h.get_by_label("Editing: (unnamed) (BSTriShape)");
    h.state_mut().select_edit(Selection::Shape(999));
    h.run_steps(2);
    h.get_by_label("Editing: block 999 (not in this NIF)");
}

/// T7 screenshots (GPU, run with the render_ gate): the Meshes page with an edit open at 1100, 1280 and 1600 px, saved
/// to %TEMP% (never the repo) for the report.
#[test]
fn render_meshes_page_with_an_edit_at_three_widths() {
    for w in [1100.0f32, 1280.0, 1600.0] {
        let (_d, mut h) = app_edit(w, 900.0, true);
        h.run_steps(6);
        let img = h.render().expect("render");
        let out = std::env::temp_dir().join(format!("tessalith-meshes-edit-{w}.png"));
        img.save(&out).expect("save png");
        println!("screenshot: {}", out.display());
    }
}

// ---- Meshes UI T8: the Controllers line outside Edit (Note CZ25) ----

/// Steps the view until `label` shows (the controller summary is read off the UI thread), at most ~5 s.
fn wait_for_label(h: &mut Harness<'static, MeshesModel>, label: &str) -> bool {
    for _ in 0..100 {
        h.run_steps(1);
        if h.query_by_label(label).is_some() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

/// T8: a selected NIF's cards show their Controllers line without Edit, read from the file once.
#[test]
fn cards_show_controllers_without_edit() {
    let bytes = ctrl_scene(0x82);
    let d = files_pair(&bytes, &bytes);
    let mut m = MeshesModel::new(Some(d.path().join("scan.db")), MeshesSettings::default());
    m.procs = sdc_gui::model::meshes::no_procs;
    scan_and_select(&mut m, d.path());
    m.shapes_col = true;
    let mut h = view(m);
    assert!(
        wait_for_label(&mut h, "2: Emissive multiple, U offset"),
        "no Controllers line outside Edit"
    );
    assert!(h.state().edit.is_none());
}

/// T8: a NIF whose file cannot be read any more (gone since the scan) still shows its cards, with no Controllers line
/// and no error.
#[test]
fn cards_stay_when_the_controller_read_fails() {
    let bytes = ctrl_scene(0x82);
    let d = files_pair(&bytes, &bytes);
    let mut m = MeshesModel::new(Some(d.path().join("scan.db")), MeshesSettings::default());
    m.procs = sdc_gui::model::meshes::no_procs;
    // The scan reads the good bytes; both copies then turn into junk before the NIF is selected (and read again).
    m.activate_with(None);
    m.use_instance(d.path().to_path_buf(), "Default".into());
    m.request_scan(false);
    let req = m.take_request().unwrap();
    m.start(req);
    let t = Instant::now();
    m.poll();
    while m.scanning() || m.v3.locating() {
        assert!(t.elapsed() < Duration::from_secs(60), "scan did not end");
        std::thread::sleep(Duration::from_millis(10));
        m.poll();
    }
    for rel in ["mods/A/meshes/x.nif", "mods/PGP/meshes/x.nif"] {
        std::fs::write(d.path().join(rel), b"not a nif").unwrap();
    }
    let a = m.mods.iter().find(|r| r.name == "A").unwrap().id;
    m.select_mod(a);
    let c = m.nifs[0].copy_id;
    m.select_nif(c);
    assert!(!m.shapes.is_empty(), "the cards come from the scan");
    m.shapes_col = true;
    let mut h = view(m);
    for _ in 0..20 {
        h.run_steps(1);
        std::thread::sleep(Duration::from_millis(25));
    }
    h.get_by_label_contains("Select shape");
    assert!(h.query_by_label("Controllers:").is_none());
    assert_eq!(h.state().error, None);
}

// ---- Meshes UI final-review fixes (Note CZ24) ----

/// Final review Minor 1: the flag lists' open state belongs to one session, so a second NIF whose shader has the
/// same block number still opens a list with a warning.
#[test]
fn a_flag_warning_opens_its_list_in_the_next_session() {
    let mut m = direct(typed(0, 0));
    select(&mut m, "S");
    let mut h = view(m);
    h.run_steps(2);
    assert!(h.query_by_label("Specular").is_none(), "closed, no warning");
    let m = h.state_mut();
    m.edit =
        Some(NifEdit::open(typed(0, 1 << 21), copy("A", 1, false), "meshes/y.nif", None).unwrap());
    let s = shape_named(m.edit.as_ref().unwrap(), "S");
    m.select_edit(Selection::Shape(s));
    h.run_steps(3);
    h.get(
        egui_kittest::kittest::By::new()
            .role(AkRole::Button)
            .label_contains("Clear the flag: "),
    );
}

/// Final review Minor 2: a Root pick inside Edit is where the next Edit opens.
#[test]
fn a_root_pick_inside_edit_carries_to_the_next_edit() {
    let (_d, mut m) = session(false);
    m.open_edit();
    let shape = lit_shape(m.edit.as_ref().unwrap()).0;
    m.select_edit(Selection::Shape(shape));
    m.shapes_col = true;
    let mut h = view(m);
    h.run_steps(4);
    let name = h
        .state()
        .edit
        .as_ref()
        .unwrap()
        .model
        .shape(shape)
        .unwrap()
        .name
        .clone();
    let tail = format!(" {}", name.unwrap());
    h.query_all_by_role(AkRole::Button)
        .find(|n| {
            n.accesskit_node()
                .label()
                .is_some_and(|l| l.starts_with("Select shape") && l.ends_with(&tail))
        })
        .unwrap()
        .click();
    h.run_steps(3);
    h.get_by_label("Root").click();
    h.run_steps(3);
    let m = h.state_mut();
    m.close_edit();
    m.open_edit();
    assert_eq!(m.edit.as_ref().unwrap().selection, Selection::Root);
}

/// Final review Minor 8: closing Edit puts the column back where it was (Mods and meshes).
#[test]
fn closing_edit_restores_the_mods_and_meshes_column() {
    let (_d, mut m) = session(false);
    m.shapes_col = false;
    let mut h = view(m);
    h.run_steps(4);
    h.get_by_label("Edit").click();
    h.run_steps(4);
    assert!(h.state().shapes_col, "Shapes while editing");
    h.get_by_label("Edit").click();
    h.run_steps(4);
    assert!(h.state().edit.is_none());
    assert!(!h.state().shapes_col, "back on Mods and meshes");
}

/// Note GB3: `ExtraValue::Str` holds an `Arc<str>`, which is written to the edit document as the plain string it
/// was when it held a `String`, and read back as one.
#[test]
fn string_extra_value_keeps_its_document_shape() {
    use nif::extra::ExtraValue;
    let mut d = doc();
    d.ops.push(DocOp {
        op: Op::EditExtra {
            at: Anchor {
                block: 4,
                kind: "NiStringExtraData".into(),
                shape: None,
                role: Role::Extra {
                    on_root: true,
                    name: "SDTA".into(),
                    kind: "NiStringExtraData".into(),
                    nth: 0,
                },
            },
            value: ExtraValue::Str("[1, 2]".into()),
            base: ExtraValue::Str("".into()),
        },
        status: OpStatus::Applied,
    });
    let t = d.to_text();
    assert!(t.contains("\nStr = \"[1, 2]\"\n"), "{t}");
    assert!(t.contains("\nStr = \"\"\n"), "{t}");
    assert_eq!(EditDoc::from_text(&t).unwrap(), d);
}
