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
    let mut v = Viewport3d::new(Some(shaders));
    v.cache_dir = Some(std::env::temp_dir().join("sdc_b8_shader_cache"));
    v.set_mesh(Some(v3::MeshSpec {
        key: 1,
        nif: v3::FileSource::Loose(winner.root.join(BODY)),
        textures,
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
