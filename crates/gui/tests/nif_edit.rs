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
    let d = instance("+PGP\n+A\n");
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
    let tmp = tempfile::tempdir().unwrap();
    // As meshes_header.rs: nothing reaches the real sdc folders.
    let a = tmp.path().join("appdata");
    std::fs::create_dir_all(&a).unwrap();
    std::env::set_var("LOCALAPPDATA", &a);
    std::env::set_var("APPDATA", &a);
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
