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
    assert!(h.query_by_label("Controllers").is_none());
    h.get_by_label("Edit").click();
    h.run_steps(4);
    assert!(h.state().edit.is_some(), "Edit opens a session");
    assert!(
        h.state().shapes_col,
        "Edit shows the shape cards to pick from"
    );
    for l in ["Shader", "Alpha", "Controllers", "Extra data"] {
        h.get_by_label(l);
    }
    h.get_by_label("Source: A");
    h.get_by_label("Edit").click();
    h.run_steps(4);
    assert!(h.state().edit.is_none(), "Edit again closes it");
    assert!(h.query_by_label("Controllers").is_none());
}

#[test]
fn clicking_a_card_selects_the_shape_and_root_card_is_first() {
    let (_d, mut m) = session(false);
    m.open_edit();
    m.shapes_col = true;
    let s = m.shapes[0].clone();
    let card = format!(
        "{} {} — {}",
        s.index3d
            .map_or("#– (not in scene tree)".into(), |i| format!("#{i}")),
        s.name.as_deref().unwrap_or("(unnamed)"),
        s.kind
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
    h.get_by_label("Alpha").click();
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

/// A shared alpha whose split is refused (a clip controller) is not editable: an edit would land on every user.
#[test]
fn a_shared_alpha_that_cannot_split_is_not_edited() {
    let mut m = direct(scenes::shared_alpha_with_managed_ctrl());
    select(&mut m, "A");
    let e = m.edit.as_ref().unwrap();
    let b = shape_named(e, "B");
    let before = e.model.alphas[&e.model.shape(b).unwrap().alpha.unwrap()]
        .alpha
        .flags;
    let mut h = view(m);
    h.run_steps(2);
    h.get_by_label("Alpha").click();
    h.run_steps(2);
    h.get_by_label("Shared alpha with a clip controller: splitting needs Phase C.");
    if let Some(n) = h.query_by_label("Blend") {
        n.click();
    }
    h.run_steps(2);
    let e = h.state().edit.as_ref().unwrap();
    let after = e.model.alphas[&e.model.shape(b).unwrap().alpha.unwrap()]
        .alpha
        .flags;
    assert_eq!(before, after, "B's alpha changed: {:?}", e.outcomes);
    assert!(e.doc.ops.is_empty(), "{:?}", e.doc.ops);
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
    h.get_by_label("Alpha").click();
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
    let tmp = tempfile::tempdir().unwrap();
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
        "Shader",
        "Alpha",
        "Controllers",
        "Extra data",
        "Undo NIF edit",
        "Redo NIF edit",
    ] {
        let r = h.get_by_label(l).rect();
        assert!(
            edit.contains(r.center()),
            "{l} {r:?} outside the panel {edit:?}"
        );
    }
}

// ---- v2 A18: Controllers (scalars) and Extra data tabs ----

use builder::{Nif, W};

/// Shape "S" (0) -> BSLSP (1) -> free float controllers var 11 (2, interp 4) and var 20 (3, interp 5); a root NiNode
/// (12) listing BSXFlags `bsx` (6), a BSInvMarker, a multi-line string, an int, a bool and a negative-zero float.
fn ctrl_scene(bsx: u32) -> Vec<u8> {
    let mut n = Nif::new(100);
    let (s, r) = (n.string("S"), n.string("Root"));
    n.block("BSTriShape", scenes::bstri_named(s, 1, -1));
    n.block("BSLightingShaderProperty", scenes::lsp_body(0, &[], 2, &[]));
    for (next, var, interp) in [(3, 11, 4), (-1, 20, 5)] {
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
    h.get_by_label(name).click();
    h.run_steps(3);
}

fn edit_number(h: &mut Harness<'static, MeshesModel>, label: &str, text: &str) {
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

#[test]
fn managed_variable_combo_is_disabled() {
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
    assert!(var.accesskit_node().is_disabled());
    var.click();
    h.run_steps(3);
    assert!(ops(h.state()).is_empty(), "{:?}", ops(h.state()));
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
    assert_eq!(Placement::Ok.text(), "Placement OK.");
    assert_eq!(
        Placement::NotEnabled.text(),
        "The edit mod is not enabled: enable 'sdc NIF Edits' in MO2."
    );
    assert_eq!(
        Placement::NotScanned.text(),
        "Rescan to check where 'sdc NIF Edits' sits."
    );
    assert_eq!(
        Placement::Below("B".into()).text(),
        "The edit mod is below B: your edit loses. Drag 'sdc NIF Edits' below B in MO2."
    );
    assert_eq!(
        Placement::AbovePg("PGP".into()).text(),
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
    m.edit_push(vec![op], None);
    assert!(matches!(
        m.edit.as_ref().unwrap().outcomes[0],
        nif::edit::Outcome::Refused(_)
    ));
    assert_eq!(
        m.export_edit(None),
        ExportOutcome::Blocked("1 edits cannot be applied.".into())
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
    h.get_by_label("To: mods\\sdc NIF Edits\\meshes\\x.nif");
    h.get_by_label("Placement OK.");
    h.get_by_role_and_label(AkRole::Button, &revert);
    h.get_by_role_and_label(AkRole::Button, "Export NIF")
        .click();
    h.run_steps(3);
    let p = exported(&d);
    assert!(p.is_file(), "the click exported");
    h.get_by_label("Re-run PGPatcher so its output includes this edit.");
    std::fs::write(&p, b"junk").unwrap();
    h.get_by_role_and_label(AkRole::Button, "Export NIF")
        .click();
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
