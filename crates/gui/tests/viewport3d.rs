//! The 3D viewport of the Meshes view (plan B Task B8): panel texts, the CS locate state machine and, with
//! `--ignored`, a real render of the owner's femalebody_1.nif at 1600×900 (frame-time measurement).
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use render3d::locate::{Change, Fingerprint};
use sdc_gui::model::meshes::LE_WARNING;
use sdc_gui::model::viewport3d::{self as v3, V3State, Viewport3d};
use std::path::{Path, PathBuf};

fn harness(v: Viewport3d) -> Harness<'static, Viewport3d> {
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(1000.0, 700.0))
        .build_ui_state(
            |ui, v: &mut Viewport3d| sdc_gui::ui::viewport3d::show(ui, v),
            v,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

fn fp(root: &Path, owner: &str, hash: &str) -> Fingerprint {
    Fingerprint {
        root: root.to_path_buf(),
        owner_mod: Some(owner.into()),
        hash: hash.into(),
    }
}

#[test]
fn not_found_offers_choose_folder() {
    let mut h = harness(Viewport3d::with_state(V3State::NotFound));
    h.run();
    h.get_by_label("Community Shaders not found");
    h.get_by_label("Choose folder…");
}

#[test]
fn moved_popup_has_three_choices_when_old_exists() {
    let d = tempfile::tempdir().unwrap();
    let old = fp(d.path(), "CS_old", "a");
    let new = fp(&d.path().join("n"), "CS_new", "b");
    let mut v = Viewport3d::with_state(V3State::Ready);
    v.popup = Some(Change::Moved { old, new });
    let mut h = harness(v);
    h.run();
    h.get_by_label("Community Shaders moved/updated: CS_old → CS_new. Use the new location?");
    h.get_by_label("Use new");
    h.get_by_label("Choose folder…");
    h.get_by_label("Keep old");
}

#[test]
fn moved_popup_hides_keep_old_when_old_is_gone() {
    let d = tempfile::tempdir().unwrap();
    let old = fp(&d.path().join("gone"), "CS_old", "a");
    let new = fp(d.path(), "CS_new", "b");
    let mut v = Viewport3d::with_state(V3State::Ready);
    v.popup = Some(Change::Moved { old, new });
    let mut h = harness(v);
    h.run();
    h.get_by_label("Use new");
    assert!(h.query_by_label("Keep old").is_none());
}

#[test]
fn le_mesh_message_and_no_frame() {
    let mut v = Viewport3d::with_state(V3State::Ready);
    v.report = Some(render3d::LoadReport {
        le_mesh: true,
        ..Default::default()
    });
    v.frame = Some(std::sync::Arc::new(v3::Frame {
        width: 2,
        height: 2,
        rgba: vec![255; 16],
        id: 1,
    }));
    let mut h = harness(v);
    h.run();
    h.get_by_label(LE_WARNING);
    assert!(h.query_by_label(v3::FRAME_LABEL).is_none());
}

#[test]
fn panel_shows_toggle_footer_and_problems() {
    let mut v = Viewport3d::with_state(V3State::Ready);
    v.report = Some(render3d::LoadReport {
        shapes: 1,
        missing_textures: vec!["textures/a/b.dds".into()],
        compile_errors: vec!["Body: ps: X3000".into()],
        ..Default::default()
    });
    v.frame = Some(std::sync::Arc::new(v3::Frame {
        width: 2,
        height: 2,
        rgba: vec![255; 16],
        id: 1,
    }));
    let mut h = harness(v);
    h.run();
    h.get_by_label("Community Shaders features");
    h.get_by_label("Dynamic Cubemaps: off in preview");
    h.get_by_label("Missing texture: textures/a/b.dds");
    h.get_by_label("Compile error: Body: ps: X3000");
    h.get_by_label(v3::FRAME_LABEL);
}

#[test]
fn features_toggle_flips_the_model() {
    let v = Viewport3d::with_state(V3State::Ready);
    assert!(v.features_on, "on by default");
    let mut h = harness(v);
    h.run();
    h.get_by_label("Community Shaders features").click();
    h.run();
    assert!(!h.state().features_on);
}

// ---- locate state machine (no thread, no GPU) ----

#[test]
fn first_find_is_adopted_silently() {
    let d = tempfile::tempdir().unwrap();
    let mut v = Viewport3d::new(None);
    v.on_located(Ok(Some(fp(d.path(), "CS", "a"))));
    assert!(v.popup.is_none());
    assert_eq!(v.state, V3State::Ready);
    assert_eq!(v.take_shaders_changed().unwrap().hash, "a");
}

#[test]
fn hash_change_is_a_notice_not_a_popup() {
    let d = tempfile::tempdir().unwrap();
    let mut v = Viewport3d::new(Some(fp(d.path(), "CS", "a")));
    v.on_located(Ok(Some(fp(d.path(), "CS", "b"))));
    assert!(v.popup.is_none());
    assert_eq!(v.notice.as_deref(), Some(v3::UPDATED_NOTICE));
    assert_eq!(v.shaders.as_ref().unwrap().hash, "b");
    assert!(v.take_shaders_changed().is_some());
}

#[test]
fn renamed_root_pops_up_keeps_frame_and_keep_old_is_remembered() {
    let d = tempfile::tempdir().unwrap();
    let (a, b) = (d.path().join("CS_a"), d.path().join("CS_b"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let mut v = Viewport3d::new(Some(fp(&a, "CS_a", "h")));
    v.frame = Some(std::sync::Arc::new(v3::Frame {
        width: 1,
        height: 1,
        rgba: vec![1, 2, 3, 255],
        id: 1,
    }));
    v.on_located(Ok(Some(fp(&b, "CS_b", "h"))));
    assert!(matches!(v.popup, Some(Change::Moved { .. })));
    assert!(
        v.frame.is_some(),
        "the last frame stays until the user answers"
    );
    v.keep_old();
    assert!(v.popup.is_none());
    assert_eq!(v.shaders.as_ref().unwrap().root, a);
    // The same new location is not asked about again this session.
    v.on_located(Ok(Some(fp(&b, "CS_b", "h"))));
    assert!(v.popup.is_none());
    assert!(v.take_shaders_changed().is_none());
}

#[test]
fn use_new_adopts_the_new_location() {
    let d = tempfile::tempdir().unwrap();
    let mut v = Viewport3d::new(Some(fp(&d.path().join("a"), "CS_a", "h")));
    v.on_located(Ok(Some(fp(d.path(), "CS_b", "h"))));
    v.use_new();
    assert!(v.popup.is_none());
    assert_eq!(
        v.shaders.as_ref().unwrap().owner_mod.as_deref(),
        Some("CS_b")
    );
    assert!(v.take_shaders_changed().is_some());
}

#[test]
fn not_found_switches_the_panel_but_keeps_the_frame() {
    let d = tempfile::tempdir().unwrap();
    let mut v = Viewport3d::new(Some(fp(d.path(), "CS", "a")));
    v.frame = Some(std::sync::Arc::new(v3::Frame {
        width: 1,
        height: 1,
        rgba: vec![0; 4],
        id: 1,
    }));
    v.on_located(Ok(None));
    assert_eq!(v.state, V3State::NotFound);
    assert!(v.frame.is_some());
}

#[test]
fn locate_finds_cs_in_a_folder_target_and_accepts_shaders_or_mod_root() {
    let d = tempfile::tempdir().unwrap();
    let m = d.path().join("CommunityShaders_AIO");
    std::fs::create_dir_all(m.join("Shaders")).unwrap();
    std::fs::write(m.join("Shaders/Lighting.hlsl"), "// l").unwrap();
    let found = v3::locate_shaders(
        Some(&sdc_gui::model::meshes::ScanTarget::Folder(m.clone())),
        None,
    )
    .unwrap()
    .unwrap();
    assert_eq!(found.root, m.join("Shaders"));
    assert_eq!(found.owner_mod.as_deref(), Some("CommunityShaders_AIO"));
    assert_eq!(v3::chosen_root(&m).unwrap(), m.join("Shaders"));
    assert_eq!(
        v3::chosen_root(&m.join("Shaders")).unwrap(),
        m.join("Shaders")
    );
    assert!(v3::chosen_root(d.path()).is_err());
    // Nothing in the target: a still-valid saved root is fingerprinted again.
    let saved = render3d::locate::fingerprint(&m.join("Shaders"), None).unwrap();
    let again = v3::locate_shaders(None, Some(&saved)).unwrap().unwrap();
    assert_eq!(again, saved);
}

#[test]
fn orbit_starts_at_the_auto_camera_and_double_click_resets() {
    use render3d::scene::{Camera, View};
    let b = Some(([1.0, 2.0, 3.0], 4.0));
    let o = v3::Orbit::default();
    let a = View::new(&Camera::Auto, b, 64, 64);
    let c = View::new(&o.camera(b), b, 64, 64);
    for k in 0..3 {
        assert!(
            (a.eye[k] - c.eye[k]).abs() < 1e-3,
            "{:?} {:?}",
            a.eye,
            c.eye
        );
    }
    let mut o2 = o;
    o2.drag(40.0, -10.0);
    o2.zoom(2.0);
    assert_ne!(o2, o);
    o2.reset();
    assert_eq!(o2, o);
}

// ---- real render (owner's machine) ----

const CS_SHADERS: &str =
    r"C:\_mount\moddingvol\Acherus\mods\CommunityShaders_AIO-2026-10-03T06-46Z\Shaders";
const INSTANCE: &str = r"C:\_mount\moddingvol\Acherus";
const BODY: &str = r"meshes\actors\character\character assets\femalebody_1.nif";
const PNG_OUT: &str = r"F:\ai\dds\nifparseref\plan\b7-renders\b8_femalebody_1600x900.png";

/// Renders the winning femalebody_1.nif through the viewport's worker at 1600×900, prints the frame times and
/// writes the sRGB-encoded frame. `cargo test -p sdc-gui --release --test viewport3d -- --ignored --nocapture`.
#[test]
#[ignore = "needs the owner's MO2 instance, CS install and a D3D11 device"]
fn femalebody_1600x900_frame_times() {
    let inst = modscan::mo2::Instance::load(Path::new(INSTANCE), None).unwrap();
    let (mut origins, _) = modscan::mo2::loose_origins(&inst).unwrap();
    origins.sort_by_key(|o| std::cmp::Reverse(o.priority));
    let winner = origins
        .iter()
        .find(|o| o.root.join(BODY).is_file())
        .expect("a femalebody_1.nif");
    println!("NIF from {}", winner.name);
    let nif = std::fs::read(winner.root.join(BODY)).unwrap();
    let scan = nif::scan::scan(&nif).unwrap();
    let mut textures = std::collections::HashMap::new();
    for s in &scan.shapes {
        if let nif::scan::Shader::Lighting(_, slots) = &s.shader {
            for p in slots.iter().filter(|p| !p.norm.is_empty()) {
                let rel = p.norm.replace('/', "\\");
                if let Some(o) = origins.iter().find(|o| o.root.join(&rel).is_file()) {
                    textures.insert(p.norm.clone(), v3::FileSource::Loose(o.root.join(&rel)));
                }
            }
        }
    }
    let shaders = render3d::locate::fingerprint(Path::new(CS_SHADERS), None).unwrap();
    let mut v = Viewport3d::new(Some(shaders.clone()));
    v.on_located(Ok(Some(shaders)));
    v.cache_dir = Some(std::env::temp_dir().join("sdc_b8_shader_cache"));
    v.set_mesh(Some(v3::MeshSpec {
        key: 1,
        nif: v3::FileSource::Loose(winner.root.join(BODY)),
        textures,
        le_mesh: false,
    }));
    let wait = |v: &mut Viewport3d| {
        let t = std::time::Instant::now();
        while v.busy() && t.elapsed().as_secs() < 300 {
            v.poll();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(!v.busy(), "timed out");
    };
    v.request_frame(1600, 900);
    wait(&mut v);
    println!("report: {:#?}", v.report);
    println!("adapter: {:?}", v.adapter);
    // Untimed warm-up frames (GPU clocks ramp up).
    for _ in 0..5 {
        v.orbit.drag(-3.0, 0.0);
        v.request_frame(1600, 900);
        wait(&mut v);
    }
    let mut times = vec![];
    for i in 0..10 {
        v.orbit.drag(5.0 * (i + 1) as f32, 0.0);
        v.request_frame(1600, 900);
        wait(&mut v);
        let f = v.frame.clone().unwrap();
        let t = std::time::Instant::now();
        let img = eframe::egui::ColorImage::from_rgba_unmultiplied(
            [f.width as usize, f.height as usize],
            &f.rgba,
        );
        let upload_cpu = t.elapsed().as_secs_f64() * 1000.0;
        std::hint::black_box(img);
        times.push((v.timing.unwrap(), upload_cpu));
    }
    for (t, u) in &times {
        println!(
            "draw {:.2} ms · gpu wait {:.2} ms · readback {:.2} ms · ColorImage {:.2} ms",
            t.draw_ms, t.gpu_ms, t.readback_ms, u
        );
    }
    let n = times.len() as f64;
    let avg =
        |f: &dyn Fn(&(render3d::FrameTiming, f64)) -> f64| times.iter().map(f).sum::<f64>() / n;
    println!(
        "AVG draw {:.2} · gpu {:.2} · readback {:.2} · ColorImage {:.2}",
        avg(&|t| t.0.draw_ms),
        avg(&|t| t.0.gpu_ms),
        avg(&|t| t.0.readback_ms),
        avg(&|t| t.1)
    );
    v.orbit.reset();
    v.request_frame(1600, 900);
    wait(&mut v);
    let f = v.frame.clone().unwrap();
    let out = PathBuf::from(PNG_OUT);
    image::save_buffer(&out, &f.rgba, f.width, f.height, image::ColorType::Rgba8).unwrap();
    println!("wrote {}", out.display());
    // Features off: no ENABLE_LL, gamma-space output, no display encode (for a by-eye check).
    v.set_features(false);
    v.request_frame(1600, 900);
    wait(&mut v);
    let before = v.frame.clone().unwrap().id;
    v.request_frame(1600, 900);
    wait(&mut v);
    assert_ne!(
        v.frame.as_ref().unwrap().id,
        before,
        "re-rendered after the reload"
    );
    println!(
        "features off: {:?}",
        v.report
            .as_ref()
            .map(|r| (r.shapes, r.compile_errors.len()))
    );
    let f = v.frame.clone().unwrap();
    let off = std::env::temp_dir().join("b8_femalebody_features_off.png");
    image::save_buffer(&off, &f.rgba, f.width, f.height, image::ColorType::Rgba8).unwrap();
    println!("wrote {}", off.display());
}

#[test]
fn render3d_shaders_round_trip_in_gui_toml() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("sdc/gui.toml");
    for owner in [Some("CommunityShaders_AIO".to_string()), None] {
        let mut s = sdc_gui::settings::GuiSettings::default();
        s.render3d.shaders = Some(Fingerprint {
            root: d.path().join("CS").join("Shaders"),
            owner_mod: owner,
            hash: "ab12".into(),
        });
        s.save(&p).unwrap();
        assert_eq!(sdc_gui::settings::GuiSettings::load(&p), (s, None));
    }
}

// ---- fix round 1 ----

fn spec(key: i64, le_mesh: bool) -> v3::MeshSpec {
    v3::MeshSpec {
        key,
        nif: v3::FileSource::Loose(PathBuf::from(format!("C:/nope/{key}.nif"))),
        textures: Default::default(),
        le_mesh,
    }
}

/// A Render queued when a new Load arrives is dropped by the render thread with no reply: busy must not stick.
#[test]
fn a_render_dropped_by_a_new_load_does_not_keep_busy() {
    let d = tempfile::tempdir().unwrap();
    let mut v = Viewport3d::new(Some(fp(d.path(), "CS", "a")));
    v.on_located(Ok(Some(fp(d.path(), "CS", "a"))));
    let cmds = v.attach_test_thread();
    v.set_mesh(Some(spec(1, false)));
    v.request_frame(64, 64);
    let v3::V3Cmd::Load { id, .. } = cmds.try_recv().unwrap() else {
        panic!("load first")
    };
    v.handle(v3::V3Reply::Loaded {
        id,
        report: render3d::LoadReport {
            shapes: 1,
            ..Default::default()
        },
        bounds: Some(([0.0; 3], 1.0)),
    });
    v.request_frame(64, 64);
    assert!(matches!(cmds.try_recv(), Ok(v3::V3Cmd::Render { .. })));
    assert!(v.busy());
    // The next mesh fails to read: the thread drops the render and answers only the load.
    v.set_mesh(Some(spec(2, false)));
    v.request_frame(64, 64);
    let v3::V3Cmd::Load { id, .. } = cmds.try_recv().unwrap() else {
        panic!("load")
    };
    v.handle(v3::V3Reply::Loaded {
        id,
        report: render3d::LoadReport {
            errors: vec!["NIF: unreadable".into()],
            ..Default::default()
        },
        bounds: None,
    });
    assert!(!v.busy());
    // Same with an LE mesh or a cleared selection after a render was sent.
    v.set_mesh(Some(spec(1, false)));
    v.request_frame(64, 64);
    let v3::V3Cmd::Load { id, .. } = cmds.try_recv().unwrap() else {
        panic!("load")
    };
    v.handle(v3::V3Reply::Loaded {
        id,
        report: render3d::LoadReport {
            shapes: 1,
            ..Default::default()
        },
        bounds: Some(([0.0; 3], 1.0)),
    });
    v.request_frame(64, 64);
    assert!(v.busy());
    v.set_mesh(Some(spec(3, true)));
    assert!(!v.busy());
    v.set_mesh(None);
    assert!(!v.busy());
}

#[test]
fn le_mesh_shows_its_text_without_a_load_even_when_cs_is_missing() {
    let mut v = Viewport3d::with_state(V3State::NotFound);
    let cmds = v.attach_test_thread();
    v.set_mesh(Some(spec(7, true)));
    v.request_frame(64, 64);
    assert!(cmds.try_recv().is_err(), "nothing sent for an LE mesh");
    assert!(v.report.as_ref().is_some_and(|r| r.le_mesh));
    assert!(!v.busy());
    let mut h = harness(v);
    h.run();
    h.get_by_label(LE_WARNING);
    h.get_by_label("Community Shaders not found");
}

#[test]
fn a_locate_error_keeps_the_state_and_says_so() {
    let d = tempfile::tempdir().unwrap();
    let mut v = Viewport3d::new(Some(fp(d.path(), "CS", "a")));
    v.on_located(Err("busy file".into()));
    assert_eq!(v.state, V3State::Ready);
    assert!(v.notice.as_deref().unwrap().contains("busy file"));
    assert!(v.shaders.is_some());
}

/// A locate started before "Choose folder…" must not undo the chosen folder when it answers late.
#[test]
fn a_stale_locate_after_choose_folder_is_ignored() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("Lighting.hlsl"), "// l").unwrap();
    let mut v = Viewport3d::with_state(V3State::NotFound);
    v.locate(None);
    let stale = v.locate_generation();
    v.choose_folder(d.path().to_path_buf());
    let t = std::time::Instant::now();
    while v.busy() && t.elapsed().as_secs() < 20 {
        v.poll();
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(v.state, V3State::Ready);
    assert_eq!(v.shaders.as_ref().unwrap().root, d.path());
    v.handle(v3::V3Reply::Located {
        generation: stale,
        result: Ok(None),
    });
    assert_eq!(v.state, V3State::Ready, "the late answer is ignored");
    assert_eq!(v.shaders.as_ref().unwrap().root, d.path());
}

// ---- final fix wave ----

fn loaded(v: &mut Viewport3d, id: u64, report: render3d::LoadReport) {
    v.handle(v3::V3Reply::Loaded {
        id,
        report,
        bounds: Some(([0.0; 3], 1.0)),
    });
}

/// The saved fingerprint is not used until this session's first locate confirms it: compiling against it
/// before then could store new-source bytecode under the old hash.
#[test]
fn no_render_thread_before_the_saved_shaders_are_confirmed() {
    let d = tempfile::tempdir().unwrap();
    let saved = fp(d.path(), "CS", "a");
    let mut v = Viewport3d::new(Some(saved.clone()));
    let cmds = v.attach_test_thread();
    v.set_mesh(Some(spec(1, false)));
    v.request_frame(64, 64);
    assert!(cmds.try_recv().is_err(), "nothing sent before the locate");
    assert!(!v.busy());
    v.on_located(Ok(Some(saved)));
    v.request_frame(64, 64);
    assert!(matches!(cmds.try_recv(), Ok(v3::V3Cmd::Load { .. })));
}

#[test]
fn shaders_are_confirmed_by_update_use_new_keep_old_or_choose_but_not_by_a_pending_popup() {
    let d = tempfile::tempdir().unwrap();
    let (a, b) = (d.path().join("CS_a"), d.path().join("CS_b"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    // Hash-only update.
    let mut v = Viewport3d::new(Some(fp(&a, "CS_a", "h")));
    assert!(!v.shaders_confirmed());
    v.on_located(Ok(Some(fp(&a, "CS_a", "h2"))));
    assert!(v.shaders_confirmed());
    // Moved: not until the user answers.
    for keep in [false, true] {
        let mut v = Viewport3d::new(Some(fp(&a, "CS_a", "h")));
        v.on_located(Ok(Some(fp(&b, "CS_b", "h"))));
        assert!(v.popup.is_some() && !v.shaders_confirmed());
        if keep {
            v.keep_old();
        } else {
            v.use_new();
        }
        assert!(v.shaders_confirmed());
    }
    // Not found / a locate error: still unconfirmed.
    let mut v = Viewport3d::new(Some(fp(&a, "CS_a", "h")));
    v.on_located(Err("busy".into()));
    assert!(!v.shaders_confirmed());
    v.on_located(Ok(None));
    assert!(!v.shaders_confirmed());
    // First find (nothing saved) is adopted and confirmed.
    let mut v = Viewport3d::new(None);
    v.on_located(Ok(Some(fp(&a, "CS_a", "h"))));
    assert!(v.shaders_confirmed());
}

/// Review Focus #3 (P0 ruling): a reload of the same mesh whose permutations now fail keeps the last frame and
/// lists the compile errors; a new mesh that draws nothing still clears.
#[test]
fn a_reload_with_compile_errors_keeps_the_last_frame() {
    let d = tempfile::tempdir().unwrap();
    let saved = fp(d.path(), "CS", "a");
    let mut v = Viewport3d::new(Some(saved.clone()));
    v.on_located(Ok(Some(saved)));
    let cmds = v.attach_test_thread();
    v.set_mesh(Some(spec(1, false)));
    v.request_frame(64, 64);
    let v3::V3Cmd::Load { id, .. } = cmds.try_recv().unwrap() else {
        panic!("load")
    };
    loaded(
        &mut v,
        id,
        render3d::LoadReport {
            shapes: 2,
            ..Default::default()
        },
    );
    v.request_frame(64, 64);
    let v3::V3Cmd::Render { id: rid, .. } = cmds.try_recv().unwrap() else {
        panic!("render")
    };
    v.handle(v3::V3Reply::Frame {
        frame: std::sync::Arc::new(v3::Frame {
            width: 64,
            height: 64,
            rgba: vec![9; 64 * 64 * 4],
            id: rid,
        }),
        timing: Default::default(),
    });
    // A CS update recompiles; both shapes now fail.
    v.set_features(false);
    assert!(matches!(cmds.try_recv(), Ok(v3::V3Cmd::SetFeatures(false))));
    loaded(
        &mut v,
        id,
        render3d::LoadReport {
            compile_errors: vec!["a: ps: X3000".into(), "b: ps: X3000".into()],
            ..Default::default()
        },
    );
    assert!(v.frame.is_some(), "the last frame stays");
    assert_eq!(v.report.as_ref().unwrap().compile_errors.len(), 2);
    let mut h = harness(v);
    h.run();
    h.get_by_label("Compile error: a: ps: X3000");
    h.get_by_label(v3::FRAME_LABEL);
    let mut v = h.into_state();
    // A different mesh that draws nothing clears the frame as before.
    v.set_mesh(Some(spec(2, false)));
    v.frame = Some(std::sync::Arc::new(v3::Frame {
        width: 1,
        height: 1,
        rgba: vec![9; 4],
        id: 99,
    }));
    v.request_frame(64, 64);
    let v3::V3Cmd::Load { id, .. } = cmds.try_recv().unwrap() else {
        panic!("load")
    };
    loaded(
        &mut v,
        id,
        render3d::LoadReport {
            compile_errors: vec!["c: ps: X3000".into()],
            ..Default::default()
        },
    );
    assert!(v.frame.is_none());
}

// ---- P1-A Task 12: Memory sources, parsed and built-in content, material edits ----

use nif::material::MaterialState;
use render3d::builtin::Builtin;
use render3d::{EditKind, ShapeId, ShapeInfo, ShapeStatus};
use std::collections::HashMap;
use std::sync::Arc;

fn mem(id: u64, b: &[u8]) -> v3::FileSource {
    v3::FileSource::Memory {
        id,
        bytes: Arc::new(b.to_vec()),
    }
}

fn confirmed() -> (
    tempfile::TempDir,
    Viewport3d,
    std::sync::mpsc::Receiver<v3::V3Cmd>,
) {
    let d = tempfile::tempdir().unwrap();
    let saved = fp(d.path(), "CS", "a");
    let mut v = Viewport3d::new(Some(saved.clone()));
    v.on_located(Ok(Some(saved)));
    let cmds = v.attach_test_thread();
    (d, v, cmds)
}

fn parsed(key: u64, nif: &Arc<render3d::ParsedNif>, tex: &[(&str, v3::FileSource)]) -> v3::Content {
    v3::Content::Parsed {
        key,
        nif: nif.clone(),
        textures: Arc::new(
            tex.iter()
                .map(|(k, s)| (k.to_string(), s.clone()))
                .collect(),
        ),
    }
}

#[test]
fn memory_sources_compare_by_id_print_their_length_and_serve_their_bytes() {
    assert_eq!(mem(1, b"DDS a"), mem(1, b"DDS b"));
    assert_ne!(mem(1, b"DDS a"), mem(2, b"DDS a"));
    assert_ne!(mem(1, b"x"), v3::FileSource::Loose(PathBuf::from("x")));
    let dbg = format!("{:?}", mem(7, &[0u8; 4096]));
    assert!(dbg.contains("4096") && dbg.len() < 64, "{dbg}");
    assert_eq!(v3::read_file(&mem(3, b"DDS bytes")).unwrap(), b"DDS bytes");
}

#[test]
fn parsed_content_loads_once_and_material_edits_follow_the_load() {
    let (_d, mut v, cmds) = confirmed();
    let nif = Arc::new(render3d::ParsedNif::default());
    v.set_content(
        Some(parsed(1, &nif, &[("textures/pbr/a.dds", mem(1, b"DDS "))])),
        false,
    );
    assert!(
        !v.set_material(ShapeId(3), MaterialState::builtin_default()),
        "nothing sent yet: the caller puts the material into the content"
    );
    v.request_frame(64, 64);
    let Ok(v3::V3Cmd::LoadParsed { id, textures, .. }) = cmds.try_recv() else {
        panic!("LoadParsed first")
    };
    assert!(textures.contains_key("textures/pbr/a.dds"));
    // Queued behind the load: the thread applies it after the load.
    let mut m = MaterialState::builtin_default();
    m.params.glossiness = 12.0;
    assert!(v.set_material(ShapeId(3), m.clone()));
    assert!(matches!(
        cmds.try_recv(),
        Ok(v3::V3Cmd::SetMaterial { content, id: ShapeId(3), material }) if material == m && content == id
    ));
    assert!(v.busy());
    loaded(
        &mut v,
        id,
        render3d::LoadReport {
            shapes: 1,
            ..Default::default()
        },
    );
    v.handle(v3::V3Reply::Shapes(vec![ShapeInfo {
        id: ShapeId(3),
        name: "Bottle".into(),
        material: m.clone(),
        status: ShapeStatus::Drawing,
    }]));
    assert_eq!(v.shapes.len(), 1);
    v.handle(v3::V3Reply::MaterialSet {
        content: id,
        id: ShapeId(3),
        result: Ok(EditKind::Cbuffer),
        ms: 0.2,
    });
    assert_eq!(v.last_edit, Some((EditKind::Cbuffer, 0.2)));
    assert!(!v.busy());
    v.request_frame(64, 64);
    assert!(matches!(cmds.try_recv(), Ok(v3::V3Cmd::Render { .. })));
    // The same content (same key, same Memory ids) is no reload.
    v.set_content(
        Some(parsed(
            1,
            &nif,
            &[("textures/pbr/a.dds", mem(1, b"other bytes, same id"))],
        )),
        false,
    );
    v.request_frame(64, 64);
    assert!(cmds.try_recv().is_err());
    assert!(matches!(
        v.content(),
        Some(v3::Content::Parsed { key: 1, .. })
    ));
}

#[test]
fn a_keep_view_reload_keeps_frame_and_camera_and_a_new_model_resets_them() {
    let (_d, mut v, cmds) = confirmed();
    let nif = Arc::new(render3d::ParsedNif::default());
    v.set_content(Some(parsed(1, &nif, &[])), false);
    v.request_frame(64, 64);
    let Ok(v3::V3Cmd::LoadParsed { id, .. }) = cmds.try_recv() else {
        panic!("load")
    };
    loaded(
        &mut v,
        id,
        render3d::LoadReport {
            shapes: 1,
            ..Default::default()
        },
    );
    v.frame = Some(Arc::new(v3::Frame {
        width: 1,
        height: 1,
        rgba: vec![9; 4],
        id: 50,
    }));
    v.orbit.drag(40.0, 0.0);
    let yaw = v.orbit.yaw;
    // Re-encoded textures: the same model, a new key and new bytes.
    v.set_content(
        Some(parsed(2, &nif, &[("textures/a.dds", mem(9, b"DDS "))])),
        true,
    );
    assert!(
        v.frame.is_some(),
        "the last frame stays until the reload lands"
    );
    assert_eq!(v.orbit.yaw, yaw);
    v.request_frame(64, 64);
    assert!(matches!(cmds.try_recv(), Ok(v3::V3Cmd::LoadParsed { .. })));
    // A built-in shape is another model.
    v.set_content(
        Some(v3::Content::Builtin {
            key: 3,
            shape: Builtin::Sphere,
            material: MaterialState::builtin_default(),
            textures: Arc::new(HashMap::new()),
        }),
        false,
    );
    assert!(v.frame.is_none());
    assert_ne!(v.orbit.yaw, yaw);
    v.request_frame(64, 64);
    assert!(matches!(
        cmds.try_recv(),
        Ok(v3::V3Cmd::LoadBuiltin {
            shape: Builtin::Sphere,
            ..
        })
    ));
}

#[test]
fn an_le_parsed_nif_is_never_loaded() {
    let (_d, mut v, cmds) = confirmed();
    let nif = Arc::new(render3d::ParsedNif {
        le_mesh: true,
        ..Default::default()
    });
    v.set_content(Some(parsed(1, &nif, &[])), false);
    v.request_frame(64, 64);
    assert!(cmds.try_recv().is_err());
    assert!(v.report.as_ref().is_some_and(|r| r.le_mesh));
    assert!(!v.busy());
}

// ---- Task 12 fix round 1 ----

fn loaded_parsed(
    v: &mut Viewport3d,
    cmds: &std::sync::mpsc::Receiver<v3::V3Cmd>,
    key: u64,
    shapes: usize,
) -> u64 {
    let nif = Arc::new(render3d::ParsedNif::default());
    v.set_content(Some(parsed(key, &nif, &[])), false);
    v.request_frame(64, 64);
    let Ok(v3::V3Cmd::LoadParsed { id, .. }) = cmds.try_recv() else {
        panic!("load")
    };
    loaded(
        v,
        id,
        render3d::LoadReport {
            shapes,
            ..Default::default()
        },
    );
    id
}

fn info(status: ShapeStatus) -> ShapeInfo {
    ShapeInfo {
        id: ShapeId(0),
        name: "S".into(),
        material: MaterialState::builtin_default(),
        status,
    }
}

#[test]
fn a_stale_material_reply_does_not_touch_the_new_contents_edits() {
    let (_d, mut v, cmds) = confirmed();
    let a = loaded_parsed(&mut v, &cmds, 1, 1);
    assert!(v.set_material(ShapeId(0), MaterialState::builtin_default()));
    let _ = cmds.try_recv();
    let b = loaded_parsed(&mut v, &cmds, 2, 1);
    assert_ne!(a, b);
    assert!(v.set_material(ShapeId(0), MaterialState::builtin_default()));
    // A's late reply (an error) must neither free B's pending edit nor show its error.
    v.handle(v3::V3Reply::MaterialSet {
        content: a,
        id: ShapeId(0),
        result: Err("old".into()),
        ms: 1.0,
    });
    assert!(v.busy());
    assert!(v.render_error.is_none());
    v.handle(v3::V3Reply::MaterialSet {
        content: b,
        id: ShapeId(0),
        result: Ok(EditKind::Same),
        ms: 1.0,
    });
    assert!(!v.busy());
}

#[test]
fn a_recovered_shape_is_rendered_although_the_load_drew_nothing() {
    let (_d, mut v, cmds) = confirmed();
    loaded_parsed(&mut v, &cmds, 1, 0);
    v.handle(v3::V3Reply::Shapes(vec![info(ShapeStatus::Hidden)]));
    v.request_frame(64, 64);
    assert!(cmds.try_recv().is_err(), "nothing drawable");
    v.handle(v3::V3Reply::Shapes(vec![info(ShapeStatus::Drawing)]));
    v.request_frame(64, 64);
    assert!(matches!(cmds.try_recv(), Ok(v3::V3Cmd::Render { .. })));
}

#[test]
fn a_dead_worker_does_not_leave_edits_busy() {
    let (_d, mut v, cmds) = confirmed();
    loaded_parsed(&mut v, &cmds, 1, 1);
    assert!(v.set_material(ShapeId(0), MaterialState::builtin_default()));
    assert!(v.busy());
    v.handle(v3::V3Reply::Failed("gone".into()));
    assert!(!v.busy());
}

#[test]
fn a_failed_edit_on_the_only_shape_keeps_the_camera_live() {
    let (_d, mut v, cmds) = confirmed();
    let id = loaded_parsed(&mut v, &cmds, 1, 1);
    v.handle(v3::V3Reply::Shapes(vec![info(ShapeStatus::Drawing)]));
    assert!(v.set_material(ShapeId(0), MaterialState::builtin_default()));
    let _ = cmds.try_recv();
    v.handle(v3::V3Reply::MaterialSet {
        content: id,
        id: ShapeId(0),
        result: Err("compile".into()),
        ms: 1.0,
    });
    v.handle(v3::V3Reply::Shapes(vec![info(ShapeStatus::Failed(
        "compile".into(),
    ))]));
    v.orbit.drag(10.0, 0.0);
    v.request_frame(64, 64);
    assert!(matches!(cmds.try_recv(), Ok(v3::V3Cmd::Render { .. })));
}

#[test]
fn the_spinner_needs_300_ms_of_continuous_work_and_resets_when_idle() {
    use std::time::{Duration, Instant};
    let d = tempfile::tempdir().unwrap();
    let mut v = Viewport3d::with_state(V3State::NotFound);
    let t0 = Instant::now();
    assert!(!v.show_spinner(t0), "idle");
    v.choose_folder(d.path().to_path_buf()); // busy until its answer is polled
    assert!(v.busy());
    assert!(!v.show_spinner(t0), "just started");
    assert!(!v.show_spinner(t0 + Duration::from_millis(299)));
    assert!(v.show_spinner(t0 + v3::SPINNER_DELAY));
    let t = Instant::now();
    while v.busy() && t.elapsed().as_secs() < 20 {
        v.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(!v.busy());
    assert!(!v.show_spinner(t0 + Duration::from_secs(5)), "idle: off");
    // The timer restarted: new work does not inherit the old start.
    v.choose_folder(d.path().to_path_buf());
    let t1 = t0 + Duration::from_secs(10);
    assert!(!v.show_spinner(t1));
    assert!(!v.show_spinner(t1 + Duration::from_millis(100)));
    assert!(v.show_spinner(t1 + Duration::from_millis(300)));
}

// ---- M2: background and light controls ----

/// A viewport with a loaded mesh and its first Render already sent and drained.
fn rendered() -> (Viewport3d, std::sync::mpsc::Receiver<v3::V3Cmd>) {
    let d = tempfile::tempdir().unwrap();
    let mut v = Viewport3d::new(Some(fp(d.path(), "CS", "a")));
    v.on_located(Ok(Some(fp(d.path(), "CS", "a"))));
    let cmds = v.attach_test_thread();
    v.set_mesh(Some(spec(1, false)));
    v.request_frame(64, 64);
    let v3::V3Cmd::Load { id, .. } = cmds.try_recv().unwrap() else {
        panic!("load first")
    };
    v.handle(v3::V3Reply::Loaded {
        id,
        report: render3d::LoadReport {
            shapes: 1,
            ..Default::default()
        },
        bounds: Some(([0.0; 3], 2.0)),
    });
    v.request_frame(64, 64);
    assert!(matches!(cmds.try_recv(), Ok(v3::V3Cmd::Render { .. })));
    (v, cmds)
}

#[test]
fn changing_the_look_sends_a_render_and_no_load() {
    let (mut v, cmds) = rendered();
    // Nothing changed: nothing is sent.
    v.request_frame(64, 64);
    assert!(cmds.try_recv().is_err());
    for change in [
        (|l: &mut v3::Look| l.hdri = true) as fn(&mut v3::Look),
        |l| l.grey = 0.5,
        |l| l.sun = 2.0,
        |l| l.point = 1.5,
        |l| l.radius = 4.0,
    ] {
        change(&mut v.look);
        let want = v.look;
        v.request_frame(64, 64);
        match cmds.try_recv() {
            Ok(v3::V3Cmd::Render { look, .. }) => assert_eq!(look, want),
            Ok(_) => panic!("a look change must only render"),
            Err(e) => panic!("no command for {want:?}: {e}"),
        }
        assert!(cmds.try_recv().is_err(), "exactly one command");
    }
    assert!(v.busy(), "the render is in flight");
}

#[test]
fn the_default_look_is_the_unchanged_scene() {
    use render3d::scene::{Background, Camera, Scene};
    let l = v3::Look::default();
    assert_eq!(l.scene(Camera::Auto, None), Scene::neutral());
    // With bounds the point light is placed but off.
    let s = l.scene(Camera::Auto, Some(([1.0, 2.0, 3.0], 2.0)));
    assert_eq!(s.point_light.brightness, 0.0);
    assert_eq!(s.background, Background::Solid(0.0));
    let on = v3::Look {
        hdri: true,
        sun: 9.0,
        point: 1.0,
        radius: 2.0,
        ..l
    }
    .scene(Camera::Auto, Some(([1.0, 2.0, 3.0], 2.0)));
    assert_eq!(on.background, Background::Hdri);
    assert_eq!(on.sun_brightness, 4.0, "clamped to the slider range");
    assert_eq!(on.point_light.brightness, 1.0);
    assert!(
        (on.point_light.radius - 4.0).abs() < 1e-5,
        "2 x bounds radius 2"
    );
}

#[test]
fn view_section_offers_background_and_lights() {
    let mut h = harness(Viewport3d::with_state(V3State::Ready));
    h.run();
    h.get_by_label("View").click();
    h.run();
    h.get_by_label("Solid");
    h.get_by_label("Studio HDRI").click();
    h.run();
    assert!(h.state().look.hdri);
    h.get_by_label("Reset view").click();
    h.run();
    assert_eq!(h.state().look, v3::Look::default());
}
