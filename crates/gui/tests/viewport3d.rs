//! The 3D viewport of the Meshes view (plan B Task B8): panel texts, the CS locate state machine and, with
//! `--ignored`, a real render of the owner's femalebody_1.nif at 1600×900 (frame-time measurement).
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use render3d::locate::{Change, Fingerprint};
use render3d::perf_log;
use sdc_gui::model::meshes::LE_WARNING;
use sdc_gui::model::viewport3d::{self as v3, V3State, Viewport3d};
use sdc_gui::worker::WorkerMsg;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn harness(v: Viewport3d) -> Harness<'static, Viewport3d> {
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(1000.0, 700.0))
        .build_ui_state(
            |ui, v: &mut Viewport3d| sdc_gui::ui::viewport3d::show(ui, v),
            v,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    // The toggle, the footer and the notes these tests read are in the Details footer (AG132).
    sdc_gui::ui::viewport3d::set_details_open(
        &h.ctx,
        sdc_gui::ui::viewport3d::MESHES_DETAILS,
        true,
    );
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
    assert!(
        h.query_by_label_contains("Off in this preview").is_none(),
        "no off line when nothing is off"
    );
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

const BODY: &str = r"meshes\actors\character\character assets\femalebody_1.nif";
const PNG_NAME: &str = "b8_femalebody_1600x900.png";

/// A viewport on the owner's winning femalebody_1.nif and its textures, shaders cached in `cache`, starting forward
/// (`look.deferred` off). None, with the skip line, when SDC_ACHERUS or SDC_CS_SHADERS is unset.
fn femalebody_viewport(cache: &Path) -> Option<Viewport3d> {
    let (Some(instance), Some(cs_shaders)) = (
        std::env::var_os("SDC_ACHERUS"),
        std::env::var_os("SDC_CS_SHADERS"),
    ) else {
        eprintln!("skipped: set SDC_ACHERUS and SDC_CS_SHADERS");
        return None;
    };
    let inst = modscan::mo2::Instance::load(Path::new(&instance), None).unwrap();
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
    let shaders = render3d::locate::fingerprint(Path::new(&cs_shaders), None).unwrap();
    let mut v = Viewport3d::new(Some(shaders.clone()));
    v.on_located(Ok(Some(shaders)));
    v.cache_dir = Some(cache.to_path_buf());
    v.look.deferred = false;
    v.set_mesh(Some(v3::MeshSpec {
        key: 1,
        nif: v3::FileSource::Loose(winner.root.join(BODY)),
        textures,
        le_mesh: false,
        head_part: false,
    }));
    Some(v)
}

/// Polls until nothing is in flight (5 min at most).
fn wait_idle(v: &mut Viewport3d) {
    let t = std::time::Instant::now();
    while v.busy() && t.elapsed().as_secs() < 300 {
        v.poll();
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(!v.busy(), "timed out");
}

/// Renders the winning femalebody_1.nif through the viewport's worker at 1600×900 (forward), prints the frame times
/// and writes the sRGB-encoded frame to %TEMP%. Needs SDC_ACHERUS and SDC_CS_SHADERS. `cargo test -p sdc-gui --release --test viewport3d -- --ignored --nocapture`.
#[test]
#[ignore = "needs the owner's MO2 instance, CS install and a D3D11 device"]
fn femalebody_1600x900_frame_times() {
    let Some(mut v) = femalebody_viewport(&std::env::temp_dir().join("sdc_b8_shader_cache")) else {
        return;
    };
    let wait = wait_idle;
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
    let out = std::env::temp_dir().join(PNG_NAME);
    image::save_buffer(&out, &f.rgba, f.width, f.height, image::ColorType::Rgba8).unwrap();
    println!("wrote {}", out.display());
    // Features off: no ENABLE_LL, gamma-space output, no display encode (for a by-eye check). No reload (P1-C):
    // the thread recompiles in the background and sends refresh frames; take the picture once none came for 5 s.
    let before = v.frame.clone().unwrap().id;
    v.set_features(false);
    let t = std::time::Instant::now();
    let mut last = (before, std::time::Instant::now());
    while (last.0 == before || last.1.elapsed().as_secs() < 5) && t.elapsed().as_secs() < 300 {
        v.poll();
        let id = v.frame.as_ref().unwrap().id;
        if id != last.0 {
            last = (id, std::time::Instant::now());
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_ne!(
        v.frame.as_ref().unwrap().id,
        before,
        "re-rendered after the toggle"
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

/// The central frame of the Meshes view at `layout::DEFAULT_SIZE` (1400×900): 780×800 points at 1.0 ppp (the
/// brief's figure for that layout; not re-measured with a kittest).
const MESHES_DEFAULT: (u32, u32) = (780, 800);

/// Mean ms of 10 timed frames after 5 warm-up orbits: draw + GPU + readback + the `ColorImage` build.
fn mean_frame_ms(v: &mut Viewport3d, (w, h): (u32, u32)) -> f64 {
    for _ in 0..5 {
        v.orbit.drag(-3.0, 0.0);
        v.request_frame(w, h);
        wait_idle(v);
    }
    let mut total = 0.0;
    for i in 0..10 {
        v.orbit.drag(5.0 * (i + 1) as f32, 0.0);
        v.request_frame(w, h);
        wait_idle(v);
        let (f, tm) =
            fresh_sample(v, (w, h)).unwrap_or_else(|e| panic!("sample {i} at {w}x{h}: {e}"));
        let t = Instant::now();
        let img =
            eframe::egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &f.rgba);
        let upload = t.elapsed().as_secs_f64() * 1000.0;
        std::hint::black_box(img);
        total += tm.draw_ms + tm.gpu_ms + tm.readback_ms + upload;
    }
    total / 10.0
}

/// The frame and timing answering the last `Render` sent, at `w x h`: Err when it is stale (an older or refresh
/// frame), the wrong size, or a render error or a notice (a deferred fallback) arrived since the request.
fn fresh_sample(
    v: &Viewport3d,
    (w, h): (u32, u32),
) -> Result<(Arc<v3::Frame>, render3d::FrameTiming), String> {
    if let Some(e) = &v.render_error {
        return Err(format!("render error: {e}"));
    }
    if let Some(n) = &v.notice {
        return Err(format!("notice since the request: {n}"));
    }
    let f = v.frame.clone().ok_or("no frame")?;
    let want = v.render_sent_for_test();
    if f.id != want {
        return Err(format!("stale frame {} (the request was {want})", f.id));
    }
    if (f.width, f.height) != (w, h) {
        return Err(format!("frame {}x{}, asked {w}x{h}", f.width, f.height));
    }
    Ok((f, v.timing.ok_or("no timing")?))
}

#[test]
fn a_perf_sample_takes_only_a_fresh_error_free_frame() {
    let (mut v, _cmds) = rendered();
    let (w, h) = (1, 1); // the size frame_of makes
    v.request_frame(w, h);
    let rid = v.render_sent_for_test();
    v.handle(v3::V3Reply::Frame {
        frame: frame_of(rid, 9),
        timing: Default::default(),
    });
    assert!(fresh_sample(&v, (w, h)).is_ok());
    assert!(fresh_sample(&v, (64, 64)).is_err(), "wrong size");
    // The next render fails: the old frame and timing stay, but they are stale.
    v.orbit.drag(5.0, 0.0);
    v.request_frame(w, h);
    let rid = v.render_sent_for_test();
    v.handle(v3::V3Reply::RenderError {
        id: rid,
        message: "boom".into(),
    });
    assert!(!v.busy());
    let e = fresh_sample(&v, (w, h)).unwrap_err();
    assert!(e.contains("boom"), "{e}");
    // A frame for the request, but a fallback notice arrived meanwhile.
    v.handle(v3::V3Reply::Frame {
        frame: frame_of(rid, 9),
        timing: Default::default(),
    });
    assert!(fresh_sample(&v, (w, h)).is_ok());
    v.handle(v3::V3Reply::Notice(
        "Deferred preview unavailable: x".into(),
    ));
    assert!(
        fresh_sample(&v, (w, h)).is_err(),
        "a notice since the request"
    );
    // A refresh frame (compile-triggered) is not the answer to the request.
    v.notice = None;
    v.handle(v3::V3Reply::Refresh {
        frame: frame_of(1 << 62, 9),
        timing: Default::default(),
    });
    assert!(fresh_sample(&v, (w, h)).is_err(), "a refresh frame");
}

/// v2 D8 perf gate: the deferred toggle never blocks the UI thread (each UI-side call < 16 ms), and deferred frames
/// stay under 33.3 ms (30 fps) at 1600×900 and the Meshes default size. Cold = a fresh shader cache; warm = a new
/// viewport on the same cache.
#[test]
#[ignore = "needs the owner's MO2 instance, CS install and a D3D11 device"]
fn femalebody_deferred_holds_30_fps() {
    let cache = tempfile::tempdir().unwrap();
    let mut last = None;
    for pass in ["cold", "warm"] {
        let Some(mut v) = femalebody_viewport(cache.path()) else {
            return;
        };
        v.request_frame(1600, 900);
        wait_idle(&mut v);
        println!("adapter: {:?}", v.adapter);
        let before = v.frame.as_ref().map(|f| f.id);
        let mut slowest = 0.0f64;
        let mut timed = |f: &mut dyn FnMut()| {
            let t = Instant::now();
            f();
            slowest = slowest.max(t.elapsed().as_secs_f64() * 1000.0);
        };
        let t0 = Instant::now();
        timed(&mut || v.set_deferred(true));
        let mut first = None;
        loop {
            timed(&mut || {
                v.poll();
            });
            timed(&mut || v.request_frame(1600, 900));
            if first.is_none() && v.frame.as_ref().map(|f| f.id) != before {
                first = Some(t0.elapsed());
            }
            if first.is_some() && !v.busy() {
                break;
            }
            assert!(t0.elapsed().as_secs() < 300, "the toggle never settled");
            std::thread::sleep(Duration::from_millis(1));
        }
        let settled = t0.elapsed();
        let statuses: Vec<_> = v.shapes.iter().map(|s| (&s.name, &s.status)).collect();
        println!(
            "{pass} toggle: first new frame {:.1} ms, idle {:.1} ms, slowest UI call {slowest:.2} ms, shapes {statuses:?}",
            first.unwrap().as_secs_f64() * 1000.0,
            settled.as_secs_f64() * 1000.0
        );
        assert_eq!(v.notice, None, "no deferred fallback");
        assert!(
            !v.shapes.is_empty()
                && v.shapes
                    .iter()
                    .all(|s| s.status == render3d::ShapeStatus::Drawing),
            "{statuses:?}"
        );
        assert!(
            slowest < 16.0,
            "{pass}: a UI-side call took {slowest:.2} ms"
        );
        last = Some(v);
    }
    let mut v = last.unwrap();
    for size in [(1600, 900), MESHES_DEFAULT] {
        let deferred = mean_frame_ms(&mut v, size);
        v.set_deferred(false);
        v.request_frame(size.0, size.1);
        wait_idle(&mut v);
        let forward = mean_frame_ms(&mut v, size);
        v.set_deferred(true);
        v.request_frame(size.0, size.1);
        wait_idle(&mut v);
        println!(
            "{}x{}: deferred mean {deferred:.2} ms, forward mean {forward:.2} ms",
            size.0, size.1
        );
        assert!(
            deferred < 33.3,
            "{size:?}: deferred mean {deferred:.2} ms is under 30 fps"
        );
    }
}

// ---- fix round 1 ----

fn spec(key: i64, le_mesh: bool) -> v3::MeshSpec {
    v3::MeshSpec {
        key,
        nif: v3::FileSource::Loose(PathBuf::from(format!("C:/nope/{key}.nif"))),
        textures: Default::default(),
        le_mesh,
        head_part: false,
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
        settings: None,
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
    // A CS update (same folder, new hash) reloads and recompiles; both shapes now fail.
    v.on_located(Ok(Some(fp(d.path(), "CS", "b"))));
    assert!(matches!(cmds.try_recv(), Ok(v3::V3Cmd::SetShaders(_))));
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
    v.handle(v3::V3Reply::Shapes(vec![info(ShapeStatus::Failed {
        error: "compile".into(),
        showing_previous: true,
    })]));
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
        |l| l.bg = [0.5, 0.25, 0.0],
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
    assert_eq!(s.background, Background::Solid([0.0; 3]));
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
    // The HDRI turns with the sun (owner D4): one yaw for both.
    let turned = v3::Look { sun_yaw: 1.25, ..l }.scene(Camera::Auto, None);
    assert_eq!(turned.env_yaw, 1.25);
    assert_eq!(l.scene(Camera::Auto, None).env_yaw, 0.0);
}

#[test]
fn view_section_offers_background_and_lights() {
    let mut h = harness(Viewport3d::with_state(V3State::Ready));
    h.run();
    h.get_by_label(sdc_gui::ui::viewport3d::VIEW_MENU).click();
    h.run();
    h.get_by_label("Solid");
    h.get_by_label("Studio HDRI").click();
    h.run();
    assert!(h.state().look.hdri);
    h.get_by_label("Reset view").click();
    h.run();
    assert_eq!(h.state().look, v3::Look::default());
}

#[test]
fn work_that_starts_after_idle_does_not_inherit_the_old_spinner_timer() {
    use std::time::{Duration, Instant};
    let d = tempfile::tempdir().unwrap();
    let mut v = Viewport3d::with_state(V3State::NotFound);
    let t0 = Instant::now();
    v.choose_folder(d.path().to_path_buf());
    assert!(!v.show_spinner(t0));
    assert!(v.show_spinner(t0 + v3::SPINNER_DELAY));
    let t = Instant::now();
    while v.busy() && t.elapsed().as_secs() < 20 {
        v.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    // Idle, and nothing drew in between (the viewport was hidden); then new work starts.
    v.choose_folder(d.path().to_path_buf());
    assert!(
        !v.show_spinner(t0 + Duration::from_secs(10)),
        "a new piece of work starts its own delay"
    );
}

/// Final review I2: a set-preview reload (`set_content(.., true)`, a new key every push) whose shapes all fail to
/// compile keeps the last frame and lists the errors; new content (keep_view false) still clears.
#[test]
fn a_keep_view_reload_whose_shapes_all_fail_keeps_the_frame_and_lists_the_errors() {
    let (_d, mut v, cmds) = confirmed();
    let id = loaded_parsed(&mut v, &cmds, 1, 1);
    v.request_frame(64, 64);
    let v3::V3Cmd::Render { id: rid, .. } = cmds.try_recv().unwrap() else {
        panic!("render")
    };
    v.handle(v3::V3Reply::Frame {
        frame: Arc::new(v3::Frame {
            width: 64,
            height: 64,
            rgba: vec![9; 64 * 64 * 4],
            id: rid,
        }),
        timing: Default::default(),
    });
    let _ = id;
    let nif = Arc::new(render3d::ParsedNif::default());
    v.set_content(Some(parsed(2, &nif, &[])), true);
    assert!(v.frame.is_some(), "kept while the reload is pending");
    v.request_frame(64, 64);
    let Ok(v3::V3Cmd::LoadParsed { id: id2, .. }) = cmds.try_recv() else {
        panic!("reload")
    };
    loaded(
        &mut v,
        id2,
        render3d::LoadReport {
            compile_errors: vec!["a: ps: X3000".into()],
            ..Default::default()
        },
    );
    assert!(v.frame.is_some(), "the old image stays");
    assert_eq!(v.report.as_ref().unwrap().compile_errors.len(), 1);
    // The shapes come back Failed (no draw after a load): a camera change must not render over the kept frame.
    v.handle(v3::V3Reply::Shapes(vec![info(ShapeStatus::Failed {
        error: "x".into(),
        showing_previous: false,
    })]));
    v.orbit.drag(5.0, 0.0);
    v.request_frame(64, 64);
    assert!(
        cmds.try_recv().is_err(),
        "no Render while the frame is held"
    );
    assert!(v.frame.is_some());
    v.handle(v3::V3Reply::Shapes(vec![info(ShapeStatus::Drawing)]));
    v.orbit.drag(5.0, 0.0);
    v.request_frame(64, 64);
    assert!(matches!(cmds.try_recv(), Ok(v3::V3Cmd::Render { .. })));
    // New content (not a reload) that draws nothing clears it.
    v.set_content(Some(parsed(3, &nif, &[])), false);
    v.request_frame(64, 64);
    let Ok(v3::V3Cmd::LoadParsed { id: id3, .. }) = cmds.try_recv() else {
        panic!("load")
    };
    loaded(
        &mut v,
        id3,
        render3d::LoadReport {
            compile_errors: vec!["b: ps: X3000".into()],
            ..Default::default()
        },
    );
    assert!(v.frame.is_none());
}

/// Final review M-d: a render error for content replaced since is dropped like the other replies.
#[test]
fn a_render_error_for_replaced_content_is_dropped() {
    let (_d, mut v, cmds) = confirmed();
    let nif = Arc::new(render3d::ParsedNif::default());
    v.set_content(Some(parsed(1, &nif, &[])), false);
    v.request_frame(64, 64);
    let _ = cmds.try_recv();
    // Load pending: nothing is loaded for the error to belong to.
    v.handle(v3::V3Reply::RenderError {
        id: 1,
        message: "old".into(),
    });
    assert!(v.render_error.is_none());
}

// ---- P1-B Task 6: background compiles ----

fn load_with(
    v: &mut Viewport3d,
    cmds: &std::sync::mpsc::Receiver<v3::V3Cmd>,
    key: u64,
    keep: bool,
    shapes: usize,
    pending: usize,
) -> u64 {
    let nif = Arc::new(render3d::ParsedNif::default());
    v.set_content(Some(parsed(key, &nif, &[])), keep);
    v.request_frame(64, 64);
    let Ok(v3::V3Cmd::LoadParsed { id, .. }) = cmds.try_recv() else {
        panic!("load")
    };
    loaded(
        v,
        id,
        render3d::LoadReport {
            shapes,
            pending,
            ..Default::default()
        },
    );
    id
}

fn frame_of(id: u64, px: u8) -> Arc<v3::Frame> {
    Arc::new(v3::Frame {
        width: 1,
        height: 1,
        rgba: vec![px; 4],
        id,
    })
}

fn pending(previous: bool) -> ShapeStatus {
    ShapeStatus::Pending {
        showing_previous: previous,
    }
}

/// A first load of an uncached mesh has no drawing shape yet; the camera must still go out so the render
/// thread has something to redraw with when the programs land (03 §1.4).
#[test]
fn a_first_load_with_only_pending_shapes_still_gets_a_camera() {
    let (_d, mut v, cmds) = confirmed();
    load_with(&mut v, &cmds, 1, false, 1, 1);
    v.request_frame(64, 64);
    assert!(
        matches!(cmds.try_recv(), Ok(v3::V3Cmd::Render { .. })),
        "report.shapes counts the pending shape"
    );
    v.handle(v3::V3Reply::Shapes(vec![info(pending(false))]));
    v.orbit.drag(5.0, 0.0);
    v.request_frame(64, 64);
    assert!(matches!(cmds.try_recv(), Ok(v3::V3Cmd::Render { .. })));
}

#[test]
fn pending_shapes_are_busy_so_a_long_compile_shows_the_spinner_and_idle_does_not() {
    use std::time::{Duration, Instant};
    let (_d, mut v, cmds) = confirmed();
    load_with(&mut v, &cmds, 1, false, 1, 1);
    assert!(v.busy(), "report.pending before the first Shapes reply");
    assert_eq!(v.compiling(), 1);
    v.handle(v3::V3Reply::Shapes(vec![info(pending(false))]));
    assert!(v.busy());
    let t0 = Instant::now();
    assert!(!v.show_spinner(t0));
    assert!(!v.show_spinner(t0 + Duration::from_millis(299)));
    assert!(
        v.show_spinner(t0 + v3::SPINNER_DELAY),
        "a compile longer than 300 ms spins"
    );
    v.handle(v3::V3Reply::Shapes(vec![info(ShapeStatus::Drawing)]));
    assert_eq!(v.compiling(), 0);
    assert!(!v.busy());
    assert!(!v.show_spinner(t0 + Duration::from_secs(5)));
    assert!(!v.poll(), "idle: nothing arrives, nothing to repaint for");
}

#[test]
fn a_dead_render_thread_does_not_leave_pending_shapes_busy() {
    let (_d, mut v, cmds) = confirmed();
    load_with(&mut v, &cmds, 1, false, 1, 1);
    v.handle(v3::V3Reply::Shapes(vec![info(pending(false))]));
    assert!(v.busy());
    v.handle(v3::V3Reply::Failed("gone".into()));
    assert!(!v.busy());
}

#[test]
fn a_refresh_frame_replaces_the_image_without_touching_the_render_accounting() {
    let (_d, mut v, cmds) = confirmed();
    load_with(&mut v, &cmds, 1, false, 1, 1);
    v.handle(v3::V3Reply::Shapes(vec![info(pending(false))]));
    v.request_frame(64, 64);
    let Ok(v3::V3Cmd::Render { id: rid, .. }) = cmds.try_recv() else {
        panic!("render")
    };
    v.handle(v3::V3Reply::Frame {
        frame: frame_of(rid, 1),
        timing: Default::default(),
    });
    v.handle(v3::V3Reply::Shapes(vec![info(ShapeStatus::Drawing)]));
    v.handle(v3::V3Reply::Refresh {
        frame: frame_of((1 << 62) + 1, 7),
        timing: Default::default(),
    });
    assert_eq!(v.frame.as_ref().unwrap().rgba, vec![7; 4]);
    // A real render sent now is still in flight: the refresh id did not advance render_done.
    v.orbit.drag(5.0, 0.0);
    v.request_frame(64, 64);
    assert!(v.busy());
}

/// A reload whose programs are all new has no old draws to keep (load_parsed discards them): the old image
/// stays until the new content is ready, not replaced by a half-built one.
#[test]
fn a_reload_with_pending_shapes_holds_the_old_image_until_they_all_resolve() {
    let (_d, mut v, cmds) = confirmed();
    load_with(&mut v, &cmds, 1, false, 1, 0);
    v.handle(v3::V3Reply::Shapes(vec![info(ShapeStatus::Drawing)]));
    v.frame = Some(frame_of(50, 9));
    load_with(&mut v, &cmds, 2, true, 2, 2);
    v.handle(v3::V3Reply::Shapes(vec![
        info(pending(false)),
        info(pending(false)),
    ]));
    assert_eq!(
        v.frame.as_ref().unwrap().rgba,
        vec![9; 4],
        "the old image stays"
    );
    v.orbit.drag(5.0, 0.0);
    v.request_frame(64, 64);
    assert!(cmds.try_recv().is_err(), "no Render over the held image");
    // One shape lands, one still has no program: still held, refreshes ignored.
    v.handle(v3::V3Reply::Shapes(vec![
        info(ShapeStatus::Drawing),
        info(pending(false)),
    ]));
    v.handle(v3::V3Reply::Refresh {
        frame: frame_of((1 << 62) + 1, 7),
        timing: Default::default(),
    });
    assert_eq!(v.frame.as_ref().unwrap().rgba, vec![9; 4]);
    v.request_frame(64, 64);
    assert!(cmds.try_recv().is_err());
    // Everything resolved: released, the camera goes out, refreshes are taken again.
    v.handle(v3::V3Reply::Shapes(vec![
        info(ShapeStatus::Drawing),
        info(ShapeStatus::Drawing),
    ]));
    v.request_frame(64, 64);
    assert!(matches!(cmds.try_recv(), Ok(v3::V3Cmd::Render { .. })));
    v.handle(v3::V3Reply::Refresh {
        frame: frame_of((1 << 62) + 2, 5),
        timing: Default::default(),
    });
    assert_eq!(v.frame.as_ref().unwrap().rgba, vec![5; 4]);
}

/// An edit's shapes keep their old draw in the renderer, so camera moves keep rendering while it compiles.
#[test]
fn an_edit_waiting_for_its_shader_keeps_the_camera_live_on_the_old_draw() {
    let (_d, mut v, cmds) = confirmed();
    let id = load_with(&mut v, &cmds, 1, false, 1, 0);
    v.handle(v3::V3Reply::Shapes(vec![info(ShapeStatus::Drawing)]));
    assert!(v.set_material(ShapeId(0), MaterialState::builtin_default()));
    let _ = cmds.try_recv();
    v.handle(v3::V3Reply::MaterialSet {
        content: id,
        id: ShapeId(0),
        result: Ok(EditKind::Permutation),
        ms: 1.0,
    });
    v.handle(v3::V3Reply::Shapes(vec![info(pending(true))]));
    v.orbit.drag(10.0, 0.0);
    v.request_frame(64, 64);
    let Ok(v3::V3Cmd::Render { id: rid, .. }) = cmds.try_recv() else {
        panic!("render over the old draw")
    };
    v.handle(v3::V3Reply::Frame {
        frame: frame_of(rid, 3),
        timing: Default::default(),
    });
    assert!(v.busy(), "the compile is still running");
    v.handle(v3::V3Reply::Shapes(vec![info(ShapeStatus::Drawing)]));
    assert!(!v.busy());
}

#[test]
fn dropping_the_viewport_tells_the_render_thread_to_quit() {
    let (_d, v, cmds) = confirmed();
    drop(v);
    assert!(matches!(cmds.try_iter().last(), Some(v3::V3Cmd::Quit)));
}

// ---- P1-B Task 6: a real render thread, the repo stand-in shader, the real pool ----

fn standin_root() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../render3d/tests/fixtures/standin"
    ))
}

fn real_viewport(cache: &Path, material: MaterialState) -> Viewport3d {
    let f = render3d::locate::fingerprint(&standin_root(), None).unwrap();
    let mut v = Viewport3d::new(Some(f.clone()));
    v.on_located(Ok(Some(f)));
    v.cache_dir = Some(cache.to_path_buf());
    v.set_content(
        Some(v3::Content::Builtin {
            key: 1,
            shape: Builtin::Sphere,
            material,
            textures: Arc::new(HashMap::new()),
        }),
        false,
    );
    v
}

/// Drives the UI-thread side until nothing is in flight (the way the app's frames do).
fn run_idle(v: &mut Viewport3d) {
    let t = std::time::Instant::now();
    loop {
        v.poll();
        v.request_frame(64, 64);
        if !v.busy() && !v.shapes.is_empty() {
            return;
        }
        assert!(
            t.elapsed().as_secs() < 60,
            "never went idle: {:?}",
            v.shapes
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn a_real_load_compiles_in_the_background_then_draws_and_goes_idle() {
    let cache = tempfile::tempdir().unwrap();
    let mut m = MaterialState::builtin_default();
    m.alpha = Some(nif::props::Alpha {
        flags: 0x200,
        threshold: 0,
    }); // DO_ALPHA_TEST: the stand-in's pixel permutation compiles
    let mut v = real_viewport(cache.path(), m);
    run_idle(&mut v);
    assert_eq!(v.shapes[0].status, ShapeStatus::Drawing);
    // The refresh frame follows the Shapes reply on the same queue; wait for the lit image.
    let lit = |v: &Viewport3d| {
        v.frame.as_ref().is_some_and(|f| {
            f.rgba
                .chunks(4)
                .any(|p| p[0] as u32 + p[1] as u32 + p[2] as u32 > 0)
        })
    };
    let t = std::time::Instant::now();
    while !lit(&v) && t.elapsed().as_secs() < 10 {
        v.poll();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(lit(&v), "the compile-triggered refresh drew the sphere");
    // Idle stays idle: no endless repaints.
    std::thread::sleep(std::time::Duration::from_millis(100));
    v.poll();
    for _ in 0..5 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(!v.poll());
        assert!(!v.busy());
    }
}

/// GUI suggestions item 4: the UV checker goes on and off through render3d's slot override. Neither direction loads
/// the content again: a `Loaded` reply for the loaded id would set `report` (R14).
#[test]
fn the_uv_checker_toggles_without_a_reload() {
    let cache = tempfile::tempdir().unwrap();
    let mut m = MaterialState::builtin_default();
    m.alpha = Some(nif::props::Alpha {
        flags: 0x200,
        threshold: 0,
    }); // DO_ALPHA_TEST: the stand-in's pixel permutation compiles
    let mut v = real_viewport(cache.path(), m);
    run_idle(&mut v);
    v.report = None;
    v.set_debug(v3::DebugView::UvChecker);
    run_idle(&mut v);
    v.set_debug(v3::DebugView::Lit);
    run_idle(&mut v);
    assert!(
        v.report.is_none(),
        "no Loaded reply: nothing was loaded again"
    );
    assert_eq!(
        v.offered(),
        [v3::DebugView::Lit, v3::DebugView::UvChecker],
        "the stand-in has no debug anchor"
    );
    assert!(!v.debug_support());
}

#[test]
fn a_real_compile_failure_is_listed_and_the_panel_goes_idle() {
    let cache = tempfile::tempdir().unwrap();
    let mut m = MaterialState::builtin_default();
    m.alpha = None; // the stand-in's pixel permutation without DO_ALPHA_TEST does not compile
    let mut v = real_viewport(cache.path(), m);
    run_idle(&mut v);
    assert!(matches!(
        &v.shapes[0].status,
        ShapeStatus::Failed { error, showing_previous: false } if error.starts_with("ps:")
    ));
    assert!(
        v.frame.is_some(),
        "the background frame was shown meanwhile"
    );
    assert!(!v.busy());
}

// ---- P1-B Task 8a: texture swaps ----

#[test]
fn texture_swaps_follow_the_load_re_render_and_report_errors() {
    use render3d::Variant;
    let (_d, mut v, cmds) = confirmed();
    let nif = Arc::new(render3d::ParsedNif::default());
    v.set_content(Some(parsed(1, &nif, &[])), false);
    let p = "textures/a.dds";
    let bytes = Arc::new(vec![1u8, 2, 3]);
    assert!(
        !v.swap_texture(p.into(), bytes.clone(), Variant::Encoded),
        "no load sent yet: the caller pushes the scene instead"
    );
    assert!(!v.set_texture_variant(p.into(), Variant::Encoded));
    v.request_frame(64, 64);
    let Ok(v3::V3Cmd::LoadParsed { id, .. }) = cmds.try_recv() else {
        panic!("LoadParsed first (the default load variant is never announced)")
    };
    assert!(v.swap_texture(p.into(), bytes.clone(), Variant::Encoded));
    assert!(v.set_texture_variant(p.into(), Variant::Encoded));
    assert!(matches!(
        cmds.try_recv(),
        Ok(v3::V3Cmd::SwapTexture { path, variant: Variant::Encoded, bytes: b }) if path == p && b == bytes
    ));
    assert!(matches!(
        cmds.try_recv(),
        Ok(v3::V3Cmd::SetTextureVariant { path, variant: Variant::Encoded }) if path == p
    ));
    loaded(
        &mut v,
        id,
        render3d::LoadReport {
            shapes: 1,
            ..Default::default()
        },
    );
    v.request_frame(64, 64);
    assert!(matches!(cmds.try_recv(), Ok(v3::V3Cmd::Render { .. })));
    v.request_frame(64, 64);
    assert!(cmds.try_recv().is_err(), "nothing changed: nothing sent");
    assert!(v.swap_texture(p.into(), bytes, Variant::Source));
    cmds.try_recv().unwrap();
    v.request_frame(64, 64);
    assert!(
        matches!(cmds.try_recv(), Ok(v3::V3Cmd::Render { .. })),
        "a swap draws on the next request"
    );
    // A failed swap is listed; a reply for content replaced since is dropped.
    v.handle(v3::V3Reply::TextureSwap {
        content: id,
        path: p.into(),
        result: Err("boom".into()),
    });
    assert_eq!(v.swap_errors, vec!["textures/a.dds: boom".to_string()]);
    v.handle(v3::V3Reply::TextureSwap {
        content: id + 99,
        path: p.into(),
        result: Err("stale".into()),
    });
    assert_eq!(v.swap_errors.len(), 1);
}

#[test]
fn only_parsed_and_builtin_content_takes_swaps_and_the_load_variant_is_announced_once() {
    use render3d::Variant;
    let (_d, mut v, cmds) = confirmed();
    v.set_mesh(Some(spec(1, false)));
    v.request_frame(64, 64);
    cmds.try_recv().unwrap(); // Load
    assert!(!v.swap_texture("textures/a.dds".into(), Arc::new(vec![1]), Variant::Source));
    let nif = Arc::new(render3d::ParsedNif::default());
    v.set_load_variant(Variant::Encoded);
    v.set_content(Some(parsed(2, &nif, &[])), false);
    v.request_frame(64, 64);
    assert!(matches!(
        cmds.try_recv(),
        Ok(v3::V3Cmd::SetLoadVariant(Variant::Encoded))
    ));
    assert!(matches!(cmds.try_recv(), Ok(v3::V3Cmd::LoadParsed { .. })));
    // Unchanged for the next load: not announced again.
    v.set_content(Some(parsed(3, &nif, &[])), false);
    v.request_frame(64, 64);
    assert!(matches!(cmds.try_recv(), Ok(v3::V3Cmd::LoadParsed { .. })));
}

/// A real render thread on WARP with the repo stand-in: a swap shows in the next frame and is applied again after
/// a reload (here: the features toggle), which loads the content's own bytes again.
#[test]
fn a_texture_swap_reaches_the_render_thread_and_survives_a_reload() {
    use nif::material::MaterialState;
    use render3d::Variant;
    use sdc_gui::model::set_preview::uncompressed_dds;
    const PATH: &str = "textures/ab/t.dds";
    let standin = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../render3d/tests/fixtures/standin"
    ));
    let fp = render3d::locate::fingerprint(&standin, None).unwrap();
    let cache = tempfile::tempdir().unwrap();
    let mut v = Viewport3d::new(Some(fp.clone()));
    v.cache_dir = Some(cache.path().to_path_buf());
    v.on_located(Ok(Some(fp)));
    let tex = |c: [f32; 4]| {
        uncompressed_dds(
            &[engine::image::Rgba32F {
                width: 1,
                height: 1,
                px: vec![c],
            }],
            false,
            false,
        )
        .unwrap()
    };
    let mut material = MaterialState::builtin_default();
    material.set_slot_raw(0, PATH);
    // DO_ALPHA_TEST: the stand-in refuses the other permutations.
    material.alpha = Some(nif::props::Alpha {
        flags: 0x200,
        threshold: 0,
    });
    let red = v3::FileSource::Memory {
        id: 1,
        bytes: Arc::new(tex([1.0, 0.0, 0.0, 1.0])),
    };
    v.set_content(
        Some(v3::Content::Builtin {
            key: 1,
            shape: render3d::builtin::Builtin::Sphere,
            material,
            textures: Arc::new(HashMap::from([(PATH.to_string(), red)])),
        }),
        false,
    );
    let chan = |v: &Viewport3d, c: usize| -> u64 {
        let f = v.frame.as_ref().expect("a frame");
        f.rgba.chunks(4).map(|p| p[c] as u64).sum()
    };
    // The first frame is blank while the programs compile; the compile-triggered refresh draws the lit one.
    // Frame ids: refresh frames use a private range, so a new frame is one with a different id.
    let wait_for = |v: &mut Viewport3d, c: usize, newer_than: u64| {
        let t = std::time::Instant::now();
        loop {
            v.request_frame(64, 64);
            v.poll();
            let ok = v
                .frame
                .as_ref()
                .is_some_and(|f| f.id != newer_than && chan(v, c) > chan(v, 1 - c) + 1000);
            if ok && !v.busy() {
                break;
            }
            assert!(
                t.elapsed().as_secs() < 60,
                "no frame dominated by channel {c}: {:?} {:?} {:?}",
                v.swap_errors,
                v.frame.as_ref().map(|f| (f.id, chan(v, 0), chan(v, 1))),
                v.render_error
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    };
    wait_for(&mut v, 0, 0);
    assert!(v.render_error.is_none(), "{:?}", v.render_error);
    let green = Arc::new(tex([0.0, 1.0, 0.0, 1.0]));
    let id0 = v.frame.as_ref().unwrap().id;
    assert!(v.swap_texture(PATH.into(), green, Variant::Source));
    wait_for(&mut v, 1, id0);
    assert!(v.swap_errors.is_empty(), "{:?}", v.swap_errors);
    let id1 = v.frame.as_ref().unwrap().id;
    // A CS update (same folder, new hash) reloads the content; the features toggle no longer does (P1-C).
    let mut updated = v.shaders.clone().unwrap();
    updated.hash.push('x');
    v.on_located(Ok(Some(updated)));
    // A new frame after the reload that is green again: the swap was applied again.
    wait_for(&mut v, 1, id1);
}

// ---- P1-B Task 9: timing log events ----

fn frame_reply(v: &mut Viewport3d, id: u64) {
    v.handle(v3::V3Reply::Frame {
        frame: Arc::new(v3::Frame {
            width: 2,
            height: 2,
            rgba: vec![0; 16],
            id,
        }),
        timing: Default::default(),
    });
}

fn named<'a>(ev: &'a [serde_json::Value], n: &str) -> Vec<&'a serde_json::Value> {
    ev.iter().filter(|e| e["event"] == n).collect()
}

#[test]
fn ui_gap_is_logged_only_when_work_was_in_flight() {
    let (_d, mut v, cmds) = confirmed();
    let nif = Arc::new(render3d::ParsedNif::default());
    v.set_content(Some(parsed(1, &nif, &[])), false);
    v.request_frame(64, 64);
    let Ok(v3::V3Cmd::LoadParsed { id, .. }) = cmds.try_recv() else {
        panic!("LoadParsed")
    };
    assert!(v.busy());
    let t0 = Instant::now();
    let at = |ms: u64| t0 + Duration::from_millis(ms);
    let ((), ev) = perf_log::capture(|| {
        v.poll_at(at(0));
        v.poll_at(at(150)); // busy before: a freeze
        loaded(
            &mut v,
            id,
            render3d::LoadReport {
                shapes: 1,
                ..Default::default()
            },
        );
        assert!(!v.busy());
        v.poll_at(at(200)); // short gap
        v.poll_at(at(10_000)); // the previous frame found nothing in flight: idle
    });
    let gaps = named(&ev, "ui_gap");
    assert_eq!(gaps.len(), 1, "{ev:?}");
    assert_eq!(gaps[0]["gap_ms"], 150.0);
    assert_eq!(gaps[0]["panel"], "meshes");
}

#[test]
fn the_frame_that_drains_the_last_reply_does_not_arm_the_gap_watch() {
    let (_d, mut v, cmds) = confirmed();
    let nif = Arc::new(render3d::ParsedNif::default());
    v.set_content(Some(parsed(1, &nif, &[])), false);
    v.request_frame(64, 64);
    let Ok(v3::V3Cmd::LoadParsed { id, .. }) = cmds.try_recv() else {
        panic!("LoadParsed")
    };
    let t0 = Instant::now();
    let at = |ms: u64| t0 + Duration::from_millis(ms);
    let ((), ev) = perf_log::capture(|| {
        v.poll_at(at(0));
        v.reply_sender_for_test()
            .send(WorkerMsg::Viewport3d(v3::V3Reply::Loaded {
                id,
                report: render3d::LoadReport {
                    shapes: 1,
                    ..Default::default()
                },
                bounds: Some(([0.0; 3], 1.0)),
            }))
            .unwrap();
        v.poll_at(at(16)); // takes the reply: nothing left in flight, egui stops asking for frames
        assert!(!v.busy());
        v.poll_at(at(2_000)); // the next input, two seconds later: idle, not a freeze
    });
    assert!(named(&ev, "ui_gap").is_empty(), "{ev:?}");

    // Drawing after the drain starts a render; the app re-arms with that, so a blocked next frame is a freeze.
    let ((), ev) = perf_log::capture(|| {
        v.request_frame(64, 64);
        assert!(v.busy());
        v.arm_gap(v.busy());
        v.poll_at(at(2_500));
    });
    let gaps = named(&ev, "ui_gap");
    assert_eq!(gaps.len(), 1, "{ev:?}");
    assert_eq!(gaps[0]["gap_ms"], 500.0);
}

#[test]
fn the_load_edit_swap_and_flip_lines_are_written_when_the_frame_that_shows_them_arrives() {
    use render3d::{EditKind, ShapeId, Variant};
    let (_d, mut v, cmds) = confirmed();
    let nif = Arc::new(render3d::ParsedNif::default());
    v.set_content(Some(parsed(1, &nif, &[])), false);
    let ((), ev) = perf_log::capture(|| {
        v.request_frame(64, 64);
        let Ok(v3::V3Cmd::LoadParsed { id, .. }) = cmds.try_recv() else {
            panic!("LoadParsed")
        };
        loaded(
            &mut v,
            id,
            render3d::LoadReport {
                shapes: 1,
                ..Default::default()
            },
        );
        v.request_frame(64, 64);
        let Ok(v3::V3Cmd::Render { id: r1, .. }) = cmds.try_recv() else {
            panic!("Render")
        };
        frame_reply(&mut v, r1);
    });
    let ff = named(&ev, "first_frame");
    assert_eq!(ff.len(), 1, "{ev:?}");
    assert!(ff[0]["ms_since_load"].as_f64().unwrap() >= 0.0);
    // Commands sent after that frame was requested.
    let r1 = v.render_sent_for_test();
    let cid = v.loaded_for_test().unwrap();
    let ((), early) = perf_log::capture(|| {
        assert!(v.set_material(ShapeId(0), MaterialState::builtin_default()));
        assert!(v.swap_texture(
            "textures/a.dds".into(),
            Arc::new(vec![1, 2, 3]),
            Variant::Encoded
        ));
        assert!(v.set_texture_variant("textures/a.dds".into(), Variant::Source));
        assert!(v.set_texture_variant("textures/b.dds".into(), Variant::Source));
        // The edit's kind arrives with the thread's MaterialSet reply.
        v.handle(v3::V3Reply::MaterialSet {
            content: cid,
            id: ShapeId(0),
            result: Ok(EditKind::Permutation),
            ms: 1.0,
        });
        // A frame requested before the commands cannot show them.
        frame_reply(&mut v, r1);
    });
    assert!(
        named(&early, "edit").is_empty() && named(&early, "texture_swap").is_empty(),
        "{early:?}"
    );
    let ((), late) = perf_log::capture(|| {
        v.request_frame(64, 64);
        // The edit and texture commands come first; the Render that can show them is the last one.
        let r2 = std::iter::from_fn(|| cmds.try_recv().ok())
            .find_map(|c| match c {
                v3::V3Cmd::Render { id, .. } => Some(id),
                _ => None,
            })
            .expect("a Render for the commands");
        // Refresh frames (rendered after a compile result) never answer a command.
        v.handle(v3::V3Reply::Refresh {
            frame: Arc::new(v3::Frame {
                width: 2,
                height: 2,
                rgba: vec![0; 16],
                id: 1 << 62,
            }),
            timing: Default::default(),
        });
        frame_reply(&mut v, r2);
    });
    let edit = named(&late, "edit");
    let swap = named(&late, "texture_swap");
    let flip = named(&late, "ab_flip");
    assert_eq!((edit.len(), swap.len(), flip.len()), (1, 1, 1), "{late:?}");
    assert_eq!(
        (edit[0]["shape"].as_u64(), edit[0]["kind"].as_str()),
        (Some(0), Some("Permutation"))
    );
    assert_eq!(swap[0]["path"], "textures/a.dds");
    assert_eq!(
        (swap[0]["variant"].as_str(), swap[0]["bytes"].as_u64()),
        (Some("Encoded"), Some(3))
    );
    assert_eq!(
        (flip[0]["paths"].as_u64(), flip[0]["variant"].as_str()),
        (Some(2), Some("Source"))
    );
    for e in [edit[0], swap[0], flip[0]] {
        assert!(e["ms_to_frame"].as_f64().unwrap() >= 0.0, "{e}");
    }
}
/// The render thread's lines (`gpu`, `compile`, `load`, `frame_stats` aside) reach the log file.
#[test]
fn a_real_render_thread_writes_gpu_compile_load_and_first_frame_lines() {
    use nif::material::MaterialState;
    let log_dir = tempfile::tempdir().unwrap();
    let log = perf_log::start_in(log_dir.path()).expect("a log");
    let path = log.path().to_path_buf();
    let standin = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../render3d/tests/fixtures/standin"
    ));
    let fp = render3d::locate::fingerprint(&standin, None).unwrap();
    let cache = tempfile::tempdir().unwrap();
    let mut v = Viewport3d::new(Some(fp.clone()));
    v.cache_dir = Some(cache.path().to_path_buf());
    v.on_located(Ok(Some(fp)));
    let mut material = MaterialState::builtin_default();
    material.alpha = Some(nif::props::Alpha {
        flags: 0x200,
        threshold: 0,
    }); // the stand-in refuses the other permutations
    v.set_content(
        Some(v3::Content::Builtin {
            key: 7,
            shape: render3d::builtin::Builtin::Sphere,
            material,
            textures: Arc::new(std::collections::HashMap::new()),
        }),
        false,
    );
    let t = Instant::now();
    loop {
        v.request_frame(64, 64);
        v.poll();
        if !v.busy() {
            break;
        }
        assert!(t.elapsed().as_secs() < 60, "the viewport did not settle");
        std::thread::yield_now();
    }
    assert!(v.frame.is_some());
    drop(log);
    // A pool worker may still hold a temporary handle for an instant: wait for `run_end`.
    let t = Instant::now();
    let text = loop {
        let text = std::fs::read_to_string(&path).unwrap();
        if text.contains("\"run_end\"") || t.elapsed().as_secs() > 10 {
            break text;
        }
        std::thread::yield_now();
    };
    let ev: Vec<serde_json::Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    for want in ["run_start", "gpu", "compile", "load", "first_frame"] {
        assert!(
            ev.iter().any(|e| e["event"] == want),
            "no {want} line in {text}"
        );
    }
    // Other tests of this binary may have render threads writing to the same live log.
    assert!(
        ev.iter()
            .any(|e| e["event"] == "load" && e["content"] == "builtin:Sphere" && e["shapes"] == 1),
        "{text}"
    );
}

// ---- P1-C Task 5: the user's CS settings (T-S4b) ----

use render3d::cs_settings::CsSettings;
use sdc_gui::model::meshes::ScanTarget;

const SETTINGS_REL: &str = "SKSE/Plugins/CommunityShaders/SettingsUser.json";
const GOOD: &str = r#"{"Linear Lighting": {"enableLinearLighting": 1}}"#;
const CHANGED: &str = r#"{"Linear Lighting": {"enableLinearLighting": 1, "glowmapMult": 0.5}}"#;

/// A folder target holding a CS `Shaders/Lighting.hlsl` and `SettingsUser.json` (no real owner files).
fn cs_folder(json: &str) -> (tempfile::TempDir, ScanTarget, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let m = d.path().join("CS");
    std::fs::create_dir_all(m.join("Shaders")).unwrap();
    std::fs::write(m.join("Shaders/Lighting.hlsl"), "// l").unwrap();
    let file = m.join(SETTINGS_REL);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, json).unwrap();
    (d, ScanTarget::Folder(m), file)
}

/// One locate (what start and window focus do), waited for.
fn relocated(v: &mut Viewport3d, target: &ScanTarget) {
    v.locate(Some(target.clone()));
    let t = Instant::now();
    while v.locating() {
        assert!(
            t.elapsed() < Duration::from_secs(20),
            "locate did not answer"
        );
        v.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn settings_sent(cmds: &std::sync::mpsc::Receiver<v3::V3Cmd>) -> Vec<CsSettings> {
    cmds.try_iter()
        .filter_map(|c| match c {
            v3::V3Cmd::SetSettings(s) => Some(s),
            _ => None,
        })
        .collect()
}

#[test]
fn focus_reread_sends_set_settings_only_when_the_hash_changes() {
    let (_d, target, file) = cs_folder(GOOD);
    let mut v = Viewport3d::new(None);
    let cmds = v.attach_test_thread();
    relocated(&mut v, &target); // start
    let first = settings_sent(&cmds);
    assert_eq!(first.len(), 1, "the first read is applied");
    assert!(!first[0].parse_failed, "{:?}", first[0].notes);
    assert_eq!(v.settings().hash, first[0].hash);
    relocated(&mut v, &target); // focus, nothing changed on disk
    assert!(settings_sent(&cmds).is_empty(), "same hash: nothing sent");
    std::fs::write(&file, CHANGED).unwrap();
    relocated(&mut v, &target); // focus after CS saved its settings
    let next = settings_sent(&cmds);
    assert_eq!(next.len(), 1, "a new hash is sent once");
    assert_ne!(next[0].hash, first[0].hash);
    assert_eq!(v.settings().hash, next[0].hash);
}

#[test]
fn a_malformed_settings_file_keeps_the_previous_settings_and_shows_a_note() {
    let (_d, target, file) = cs_folder(GOOD);
    let mut v = Viewport3d::new(None);
    let cmds = v.attach_test_thread();
    relocated(&mut v, &target);
    assert_eq!(settings_sent(&cmds).len(), 1);
    let good = v.settings().clone();
    // The test thread never answers:
    v.handle(v3::V3Reply::Reconfigured(1));
    // CS is half-way through writing the file.
    std::fs::write(&file, r#"{"Linear Lighting": "#).unwrap();
    relocated(&mut v, &target);
    assert!(
        settings_sent(&cmds).is_empty(),
        "a malformed file sends nothing"
    );
    assert_eq!(v.settings(), &good, "the previous settings stay");
    let note = v.notice.clone().expect("a notice");
    assert!(note.starts_with(v3::SETTINGS_KEPT), "{note}");
    // The panel shows it through the existing notice line.
    let mut h = harness(v);
    h.run();
    h.get_by_label_contains(v3::SETTINGS_KEPT);
    let mut v = h.into_state();
    // CS finished writing: the next focus applies the file and the note goes.
    std::fs::write(&file, CHANGED).unwrap();
    relocated(&mut v, &target);
    assert_eq!(settings_sent(&cmds).len(), 1);
    assert!(v.notice.is_none(), "{:?}", v.notice);
}

#[test]
fn the_features_toggle_sends_set_features_and_no_load() {
    let (_d, mut v, cmds) = confirmed();
    let nif = Arc::new(render3d::ParsedNif::default());
    v.set_content(Some(parsed(1, &nif, &[])), false);
    v.request_frame(64, 64);
    let Ok(v3::V3Cmd::LoadParsed { id, .. }) = cmds.try_recv() else {
        panic!("LoadParsed")
    };
    loaded(
        &mut v,
        id,
        render3d::LoadReport {
            shapes: 1,
            ..Default::default()
        },
    );
    v.request_frame(64, 64);
    let Ok(v3::V3Cmd::Render { id: rid, .. }) = cmds.try_recv() else {
        panic!("render")
    };
    frame_reply(&mut v, rid); // answered, so nothing is in flight before the toggle
    v.set_features(false);
    v.request_frame(64, 64);
    let sent: Vec<v3::V3Cmd> = cmds.try_iter().collect();
    assert_eq!(sent.len(), 1, "only the toggle: no load, no render");
    assert!(matches!(sent[0], v3::V3Cmd::SetFeatures(false)));
    assert_eq!(v.loaded_for_test(), Some(id));
    assert!(v.busy(), "awake until the render thread answers the toggle");
    v.handle(v3::V3Reply::Reconfigured(1));
    assert!(!v.busy());
}

#[test]
fn the_panel_stays_awake_until_the_refresh_after_the_last_shapes_is_read() {
    let (_d, mut v, _cmds) = loaded_mesh();
    v.handle(v3::V3Reply::Shapes(vec![info(ShapeStatus::Pending {
        showing_previous: true,
    })]));
    // The worker sends the refresh frame, then the final Shapes: read in separate polls.
    assert!(v.busy());
    v.handle(v3::V3Reply::Refresh {
        frame: Arc::new(v3::Frame {
            width: 2,
            height: 2,
            rgba: vec![0; 16],
            id: 1 << 62,
        }),
        timing: Default::default(),
    });
    assert!(v.busy(), "Shapes still unread");
    v.handle(v3::V3Reply::Shapes(vec![info(ShapeStatus::Drawing)]));
    assert!(!v.busy());
}

#[test]
fn the_off_line_lists_what_the_renderer_leaves_off() {
    assert_eq!(v3::off_line(&[]), None);
    let off = ["DynamicCubemaps", "LODBlending", "ScreenSpaceGI", "NRD"].map(String::from);
    assert_eq!(
        v3::off_line(&off).as_deref(),
        Some("Off in this preview: Dynamic Cubemaps, LOD Blending, Screen Space GI, NRD")
    );
    let mut v = Viewport3d::with_state(V3State::Ready);
    v.handle(v3::V3Reply::OffInPreview(off.to_vec()));
    let mut h = harness(v);
    h.run();
    h.get_by_label_contains("Off in this preview: Dynamic Cubemaps, LOD Blending");
}

// ---- P1-C Task 6: the `settings` timing-log line ----

#[test]
fn applying_settings_writes_a_settings_line() {
    let standin = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../render3d/tests/fixtures/standin"
    ));
    let fp = render3d::locate::fingerprint(&standin, None).unwrap();
    let cache = tempfile::tempdir().unwrap();
    let mut r = render3d::Renderer::with_gpu(
        render3d::device::Gpu::new_warp().unwrap(),
        fp,
        cache.path().to_path_buf(),
    )
    .unwrap();
    let s = CsSettings {
        hash: "0123456789abcdef0123".into(),
        notes: vec!["Info.ini missing".into()],
        ..Default::default()
    };
    let ((), ev) = perf_log::capture(|| v3::apply_settings(&mut r, s));
    let e = named(&ev, "settings");
    assert_eq!(e.len(), 1, "{ev:?}");
    assert_eq!(e[0]["hash"], "0123456789ab");
    assert_eq!(
        e[0]["defines"],
        serde_json::json!(["ENABLE_LL"]),
        "default settings: LL only"
    );
    assert_eq!(e[0]["notes"], 1);
    assert!(e[0]["ms"].as_f64().unwrap() >= 0.0, "{}", e[0]);
}

#[test]
fn a_dismissed_malformed_notice_stays_gone_until_the_failure_changes() {
    let (_d, target, file) = cs_folder(GOOD);
    let mut v = Viewport3d::new(None);
    let _cmds = v.attach_test_thread();
    relocated(&mut v, &target);
    std::fs::write(&file, r#"{"a": "#).unwrap();
    relocated(&mut v, &target);
    assert!(v.notice.is_some());
    v.notice = None; // Dismiss
    relocated(&mut v, &target); // focus, same bad file
    assert!(v.notice.is_none(), "{:?}", v.notice);
    std::fs::write(&file, r#"{"b": "#).unwrap();
    relocated(&mut v, &target); // a different failure shows
    assert!(v.notice.is_some());
    v.notice = None;
    std::fs::write(&file, CHANGED).unwrap();
    relocated(&mut v, &target); // a good read clears it
    std::fs::write(&file, r#"{"a": "#).unwrap();
    relocated(&mut v, &target);
    assert!(
        v.notice.is_some(),
        "the same bad file shows again after a good read"
    );
}

#[test]
fn keep_old_keeps_the_old_settings_and_use_new_applies_the_new_ones() {
    let d = tempfile::tempdir().unwrap();
    let (a, b) = (d.path().join("CS_a"), d.path().join("CS_b"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    for keep in [true, false] {
        let mut v = Viewport3d::new(Some(fp(&a, "CS_a", "h")));
        let cmds = v.attach_test_thread();
        let old = CsSettings::default();
        let new = read_json_settings(CHANGED);
        v.on_settings(old.clone());
        let _ = settings_sent(&cmds);
        v.handle(v3::V3Reply::Located {
            generation: v.locate_generation(),
            result: Ok(Some(fp(&b, "CS_b", "h"))),
            settings: Some(new.clone()),
        });
        assert!(matches!(v.popup, Some(Change::Moved { .. })));
        assert!(
            settings_sent(&cmds).is_empty(),
            "nothing applied while asking"
        );
        assert_eq!(v.settings(), &old);
        if keep {
            v.keep_old();
            assert_eq!(v.settings(), &old);
        } else {
            v.use_new();
            assert_eq!(v.settings(), &new);
            assert_eq!(settings_sent(&cmds).len(), 1);
        }
    }
}

fn read_json_settings(json: &str) -> CsSettings {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("SettingsUser.json");
    std::fs::write(&f, json).unwrap();
    render3d::cs_settings::read_settings(&render3d::cs_settings::SettingsPaths {
        user: Some(f),
        default: None,
        info_ini: None,
        features_dir: None,
    })
}

/// A tempdir MO2 instance (profile `Default`, one mod `A`) under `d`; `overwrite` = its SettingsUser.json, if any.
fn mo2_target(d: &Path, overwrite: Option<&str>) -> ScanTarget {
    let r = d.join("MO2");
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
    std::fs::create_dir_all(r.join("mods/A")).unwrap();
    std::fs::create_dir_all(r.join("overwrite")).unwrap();
    std::fs::create_dir_all(r.join("profiles/Default")).unwrap();
    std::fs::write(r.join("profiles/Default/modlist.txt"), "+A\n").unwrap();
    std::fs::write(
        r.join("profiles/Default/settings.ini"),
        "[General]\nLocalSettings=true\n",
    )
    .unwrap();
    if let Some(json) = overwrite {
        let user = r.join("overwrite").join(SETTINGS_REL);
        std::fs::create_dir_all(user.parent().unwrap()).unwrap();
        std::fs::write(&user, json).unwrap();
    }
    ScanTarget::Mo2 {
        root: r,
        profile: "Default".into(),
    }
}

/// A chosen CS folder (no owner mod) outside the instance; `local` = a SettingsUser.json beside its `Shaders`.
fn chosen_cs(d: &Path, local: Option<&str>) -> Fingerprint {
    let cs = d.join("Elsewhere/CS");
    std::fs::create_dir_all(cs.join("Shaders")).unwrap();
    std::fs::write(cs.join("Shaders/Lighting.hlsl"), "// l").unwrap();
    if let Some(json) = local {
        let user = cs.join(SETTINGS_REL);
        std::fs::create_dir_all(user.parent().unwrap()).unwrap();
        std::fs::write(&user, json).unwrap();
    }
    Fingerprint {
        root: cs.join("Shaders"),
        owner_mod: None,
        hash: "h".into(),
    }
}

/// Task 8 fix round 2: a chosen CS folder (no owner mod) with an MO2 scan target reads the settings through the
/// target's origins (overwrite), not from the folder above `Shaders`.
#[test]
fn a_chosen_cs_folder_reads_the_settings_through_the_mo2_target() {
    let d = tempfile::tempdir().unwrap();
    let target = mo2_target(d.path(), Some(CHANGED));
    let chosen = chosen_cs(d.path(), None);
    let s = v3::read_cs_settings(Some(&target), &chosen);
    assert_eq!(s.hash, read_json_settings(CHANGED).hash, "{:?}", s.notes);
    // No MO2 target: the folder above `Shaders` as before (nothing there).
    let s = v3::read_cs_settings(None, &chosen);
    assert_eq!(s, CsSettings::default());
}

/// Fix round 3 (a): no origin has a settings file → the settings beside `Shaders` are read.
#[test]
fn settings_beside_shaders_are_the_fallback_when_no_origin_has_any() {
    let d = tempfile::tempdir().unwrap();
    let target = mo2_target(d.path(), None);
    let chosen = chosen_cs(d.path(), Some(GOOD));
    let s = v3::read_cs_settings(Some(&target), &chosen);
    assert_eq!(s.hash, read_json_settings(GOOD).hash, "{:?}", s.notes);
}

/// Fix round 3 (b): settings beside `Shaders` and in MO2's overwrite disagree → the overwrite wins.
#[test]
fn an_origin_settings_file_wins_over_the_one_beside_shaders() {
    let d = tempfile::tempdir().unwrap();
    let target = mo2_target(d.path(), Some(CHANGED));
    let chosen = chosen_cs(d.path(), Some(GOOD));
    let s = v3::read_cs_settings(Some(&target), &chosen);
    assert_eq!(s.hash, read_json_settings(CHANGED).hash, "{:?}", s.notes);
}

/// Task 8 fix round 2: no settings files but a Features folder with Skin.ini is a change from the default (the
/// hash covers the loaded features), so it is sent and the renderer compiles CS_SKIN.
#[test]
fn features_found_only_in_the_features_folder_are_sent() {
    let d = tempfile::tempdir().unwrap();
    let features = d.path().join("Features");
    std::fs::create_dir_all(&features).unwrap();
    std::fs::write(features.join("Skin.ini"), "[Info]\nVersion = 1-0-0\n").unwrap();
    let s = render3d::cs_settings::read_settings(&render3d::cs_settings::SettingsPaths {
        features_dir: Some(features),
        ..Default::default()
    });
    let mut v = Viewport3d::new(None);
    let cmds = v.attach_test_thread();
    v.on_settings(s);
    let sent = settings_sent(&cmds);
    assert_eq!(sent.len(), 1, "a features-only read is not the default");
    let mut r = render3d::Renderer::with_gpu(
        render3d::device::Gpu::new_warp().unwrap(),
        render3d::locate::fingerprint(d.path(), None).unwrap(),
        d.path().join("cache"),
    )
    .unwrap();
    r.set_settings(sent[0].clone());
    assert!(
        r.features().iter().any(|f| f == "CS_SKIN"),
        "{:?}",
        r.features()
    );
}

/// Owner data, read-only: the settings the fixed GUI reads for the owner's CS taken as a chosen folder (no owner
/// mod, as gui.toml saves it) with the Meshes MO2 target (profile Nexus). `SDC_ACHERUS` = the MO2 instance root.
#[test]
#[ignore]
fn owner_chosen_folder_settings_come_through_the_mo2_target() {
    let root = PathBuf::from(std::env::var("SDC_ACHERUS").expect("SDC_ACHERUS"));
    let chosen = Fingerprint {
        root: root.join(r"mods\CommunityShaders_AIO-2026-10-03T06-46Z\Shaders"),
        owner_mod: None,
        hash: String::new(),
    };
    let target = ScanTarget::Mo2 {
        root,
        profile: "Nexus".into(),
    };
    let s = v3::read_cs_settings(Some(&target), &chosen);
    let defines = render3d::cs_settings::preview_defines(&s, true);
    println!(
        "hash {} notes {:?}\ndefines {defines:?}",
        &s.hash[..12],
        s.notes
    );
    assert_ne!(s.hash, CsSettings::default().hash);
    // Each feature as the owner's own files say (read directly, CS mod under overwrite): the owner turned Hair
    // Specular off (2026-10-14), so not every feature is on.
    let cs = root_of(&target).join(r"mods\CommunityShaders_AIO-2026-10-03T06-46Z");
    let origins = vec![
        ("cs".to_string(), cs.clone(), 0),
        (
            "overwrite".to_string(),
            root_of(&target).join("overwrite"),
            i32::MAX,
        ),
    ];
    let direct = render3d::cs_settings::read_settings(&render3d::locate::settings_paths(
        Some(&origins),
        &cs.join("Shaders"),
    ));
    assert_eq!(
        defines,
        render3d::cs_settings::preview_defines(&direct, true),
        "the GUI reads what the owner's files enable"
    );
    for (name, define) in render3d::cs_settings::PREVIEW_FEATURES {
        assert_eq!(
            defines.iter().any(|d| d == define),
            owner_enables(&direct, name),
            "{name}"
        );
    }
}

/// Whether the settings as read turn feature `name` on: CS loaded it and its own master switch (Vanilla Fresnel's
/// `Enable`, Hair Specular's `Enabled`) is not 0.
fn owner_enables(s: &CsSettings, name: &str) -> bool {
    let switch = match name {
        "VanillaFresnel" => Some(("Vanilla Fresnel", "Enable")),
        "HairSpecular" => Some(("Hair Specular", "Enabled")),
        _ => None,
    };
    let off = switch.is_some_and(|(section, key)| {
        s.merged
            .get(section)
            .and_then(|v| v.get(key))
            .is_some_and(|v| v.as_f64() == Some(0.0) || v.as_bool() == Some(false))
    });
    s.loaded.contains(name) && !off
}

fn root_of(t: &ScanTarget) -> PathBuf {
    match t {
        ScanTarget::Mo2 { root, .. } => root.clone(),
        _ => unreachable!("an MO2 target"),
    }
}

/// Review I1: "Choose folder…" after a locate with an MO2 target reads through that target (overwrite wins over
/// the settings beside the chosen folder's `Shaders`).
#[test]
fn choose_folder_reads_the_settings_through_the_last_scan_target() {
    let d = tempfile::tempdir().unwrap();
    let target = mo2_target(d.path(), Some(CHANGED));
    let chosen = chosen_cs(d.path(), Some(GOOD));
    let mut v = Viewport3d::new(None);
    let _cmds = v.attach_test_thread();
    relocated(&mut v, &target);
    v.choose_folder(chosen.root.parent().unwrap().to_path_buf());
    let t = Instant::now();
    while v.locating() {
        assert!(
            t.elapsed() < Duration::from_secs(20),
            "choose did not answer"
        );
        v.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(v.settings().hash, read_json_settings(CHANGED).hash);
}

/// P1-D: the coverage line shows under the frame only when the renderer reports one.
#[test]
fn the_coverage_line_shows_only_when_reported() {
    let mut v = Viewport3d::with_state(V3State::Ready);
    v.handle(v3::V3Reply::Coverage(Some(
        "CS settings: 3 fields unset, 1 keys unmatched".into(),
    )));
    let mut h = harness(v);
    h.run();
    h.get_by_label_contains("CS settings: 3 fields unset, 1 keys unmatched");
    let mut h = harness(Viewport3d::with_state(V3State::Ready));
    h.run();
    assert!(h.query_by_label_contains("CS settings:").is_none());
}

/// P1-D: a `Some` -> `None` coverage reply clears the footer line. The render thread send cadence has no harness in
/// these tests, so it is not pinned here.
#[test]
fn the_coverage_line_clears_when_the_renderer_stops_reporting() {
    let mut v = Viewport3d::with_state(V3State::Ready);
    v.handle(v3::V3Reply::Coverage(Some(
        "CS settings: 1 fields unset, 0 keys unmatched".into(),
    )));
    v.handle(v3::V3Reply::Coverage(None));
    let mut h = harness(v);
    h.run();
    assert!(h.query_by_label_contains("CS settings:").is_none());
}

// ---- audit finding 14: recompiles on a loaded mesh ----

fn loaded_mesh() -> (
    tempfile::TempDir,
    Viewport3d,
    std::sync::mpsc::Receiver<v3::V3Cmd>,
) {
    let (d, mut v, cmds) = confirmed();
    v.set_mesh(Some(spec(1, false)));
    v.request_frame(64, 64);
    let Ok(v3::V3Cmd::Load { id, .. }) = cmds.try_recv() else {
        panic!("load")
    };
    loaded(&mut v, id, render3d::LoadReport::default());
    (d, v, cmds)
}

#[test]
fn a_recompile_on_a_loaded_mesh_keeps_the_panel_busy() {
    let (_d, mut v, _cmds) = loaded_mesh();
    v.handle(v3::V3Reply::Shapes(vec![info(ShapeStatus::Drawing)]));
    assert!(!v.busy());
    v.handle(v3::V3Reply::Shapes(vec![info(ShapeStatus::Pending {
        showing_previous: true,
    })]));
    assert!(v.busy());
}

#[test]
fn a_failed_recompile_on_a_loaded_mesh_is_listed() {
    let (_d, mut v, _cmds) = loaded_mesh();
    v.handle(v3::V3Reply::Shapes(vec![info(ShapeStatus::Failed {
        error: "X3000".into(),
        showing_previous: true,
    })]));
    let mut h = harness(v);
    h.run();
    h.get_by_label("S: shader failed: X3000 (previous look kept)");
}

#[test]
fn a_late_ack_after_failed_does_not_overcount_reconfigurations() {
    let (_d, mut v, _cmds) = loaded_mesh();
    v.set_features(false);
    assert!(v.busy());
    v.handle(v3::V3Reply::Failed("device".into()));
    v.handle(v3::V3Reply::Reconfigured(1));
    assert!(!v.busy());
    let _new = v.attach_test_thread();
    v.set_features(true);
    assert!(v.busy(), "the next command is waited for again");
    v.handle(v3::V3Reply::Reconfigured(1));
    assert!(!v.busy());
}

// ---- Task 10 (item 12): pan, sun rotation, debug views, magenta missing slots, Copy image

fn target(o: &v3::Orbit, b: Option<([f32; 3], f32)>) -> [f32; 3] {
    match o.camera(b) {
        render3d::scene::Camera::Orbit { target, .. } => target,
        c => panic!("orbit camera wanted, got {c:?}"),
    }
}

#[test]
fn pan_moves_the_target_in_the_view_plane_scaled_by_distance() {
    let b = Some(([1.0, 2.0, 3.0], 2.0));
    let mut o = v3::Orbit::default();
    assert_eq!(target(&o, b), [1.0, 2.0, 3.0]);
    // The default camera sits on +Y looking towards -Y, so its screen right is world -X. A drag to the right moves
    // the model right, so the target goes to the left of the screen: +X.
    o.pan(10.0, 0.0);
    let t = target(&o, b);
    assert!(t[0] > 1.0, "{t:?}");
    assert!(
        (t[1] - 2.0).abs() < 1e-5 && (t[2] - 3.0).abs() < 1e-5,
        "{t:?}"
    );
    // A drag down moves the model down: the target goes up (+Z at this small pitch).
    let mut d = v3::Orbit::default();
    d.pan(0.0, 10.0);
    let up = target(&d, b);
    assert!(up[2] > 3.0, "{up:?}");
    // The offset stays in the view plane: nothing along the view direction.
    let (y, p) = (d.yaw, d.pitch);
    let view = [p.cos() * y.sin(), -p.cos() * y.cos(), p.sin()];
    let dot = (up[0] - 1.0) * view[0] + (up[1] - 2.0) * view[1] + (up[2] - 3.0) * view[2];
    assert!(dot.abs() < 1e-4, "{dot}");
    // Twice as far away, the same drag moves twice as much.
    let mut far = v3::Orbit::default();
    far.zoom(2.0);
    far.pan(10.0, 0.0);
    let (near_dx, far_dx) = (t[0] - 1.0, target(&far, b)[0] - 1.0);
    assert!((far_dx - 2.0 * near_dx).abs() < 1e-4, "{near_dx} {far_dx}");
    // Reset (double-click, Home, Reset view) recentres.
    o.reset();
    assert_eq!(target(&o, b), [1.0, 2.0, 3.0]);
    assert_eq!(o, v3::Orbit::default());
}

#[test]
fn shift_right_drag_turns_the_sun_about_the_vertical() {
    use render3d::scene::Camera;
    let mut l = v3::Look::default();
    let a = l.scene(Camera::Auto, None).dir_light.direction;
    l.rotate_sun(std::f32::consts::FRAC_PI_2 / 0.01);
    let b = l.scene(Camera::Auto, None).dir_light.direction;
    assert!((a[2] - b[2]).abs() < 1e-5, "the height stays");
    let dot = a[0] * b[0] + a[1] * b[1];
    let len_a = (a[0] * a[0] + a[1] * a[1]).sqrt();
    let len_b = (b[0] * b[0] + b[1] * b[1]).sqrt();
    assert!((len_a - len_b).abs() < 1e-5);
    assert!(dot.abs() < 1e-4, "a quarter turn: {a:?} -> {b:?}");
    // The angle wraps into one turn.
    for _ in 0..10 {
        l.rotate_sun(-300.0);
    }
    assert!(
        (0.0..std::f32::consts::TAU).contains(&l.sun_yaw),
        "{}",
        l.sun_yaw
    );
}

#[test]
fn the_sun_turn_round_trips_through_gui_toml() {
    use sdc_gui::settings::LookSettings;
    let mut l = v3::Look::default();
    l.rotate_sun(123.0);
    let s = LookSettings::of(&l);
    let text = toml::to_string(&s).unwrap();
    assert!(text.contains("sun_yaw"), "{text}");
    let back: LookSettings = toml::from_str(&text).unwrap();
    assert_eq!(v3::Look::from_settings(Some(&back)), l);
    // An old gui.toml without it keeps the default sun.
    let old: LookSettings = toml::from_str("sun = 2.0").unwrap();
    assert_eq!(v3::Look::from_settings(Some(&old)).sun_yaw, 0.0);
}

#[test]
fn the_debug_selector_lists_only_views_the_renderer_can_make() {
    use v3::DebugView;
    // Without render3d's support (no thread yet) only Lit and UV checker are offered.
    assert_eq!(DebugView::ALL.len(), 7);
    let mut h = harness(Viewport3d::with_state(V3State::Ready));
    h.run();
    h.get_by_label(sdc_gui::ui::viewport3d::VIEW_MENU).click();
    h.run();
    h.get_by_label("Lit");
    for missing in ["Albedo", "Normals", "Roughness", "Metallic", "AO"] {
        assert!(h.query_by_label(missing).is_none(), "{missing} is offered");
    }
    h.get_by_label("UV checker").click();
    h.run();
    assert_eq!(h.state().debug(), DebugView::UvChecker);
}

#[test]
fn a_debug_view_change_goes_to_the_render_thread_and_waits_for_its_answer() {
    let (mut v, cmds) = rendered();
    v.set_debug(v3::DebugView::UvChecker);
    assert!(matches!(
        cmds.try_recv(),
        Ok(v3::V3Cmd::SetDebug(v3::DebugView::UvChecker))
    ));
    assert!(v.busy());
    v.handle(v3::V3Reply::Reconfigured(1));
    v.set_debug(v3::DebugView::UvChecker);
    assert!(cmds.try_recv().is_err(), "no change, nothing sent");
}

#[test]
fn the_uv_checker_is_a_checker() {
    let rgba = v3::checker_rgba();
    let n = v3::CHECKER_SIZE as usize;
    assert_eq!(rgba.len(), n * n * 4);
    let px = |x: usize, y: usize| rgba[(y * n + x) * 4..(y * n + x) * 4 + 4].to_vec();
    let cell = n / 8;
    assert_ne!(px(0, 0), px(cell, 0), "neighbours differ");
    assert_ne!(px(0, 0), px(0, cell));
    assert_eq!(px(0, 0), px(cell, cell), "diagonals match");
    assert_eq!(px(0, 0)[3], 255);
    let f = engine::dds::read_dds(&v3::checker_dds()).unwrap();
    assert_eq!((f.width, f.height), (v3::CHECKER_SIZE, v3::CHECKER_SIZE));
}

#[test]
fn missing_textures_are_listed_in_magenta_with_show_in_textures() {
    let mut v = Viewport3d::with_state(V3State::Ready);
    v.report = Some(render3d::LoadReport {
        missing_textures: vec!["textures/a.dds".into(), "textures/b.dds".into()],
        ..Default::default()
    });
    v.missing_items = vec![Some("landscape/a.png".into()), None];
    let mut h = harness(v);
    h.run();
    h.get_by_label("Missing texture: textures/a.dds");
    h.get_by_label("Missing texture: textures/b.dds");
    // Only the path in the open folder has the button.
    h.get_by_label("Open in Textures").click();
    h.run();
    assert_eq!(
        h.state().show_in_textures.as_deref(),
        Some("landscape/a.png")
    );
}

#[test]
fn copy_image_puts_the_last_frame_on_the_clipboard() {
    let mut v = Viewport3d::with_state(V3State::Ready);
    v.copy_image(|_, _, _| panic!("no frame yet"));
    assert_eq!(v.status.take().as_deref(), Some("Nothing to copy yet"));
    let rgba: Vec<u8> = (0..8).collect();
    v.frame = Some(std::sync::Arc::new(v3::Frame {
        width: 2,
        height: 1,
        rgba: rgba.clone(),
        id: 1,
    }));
    let mut got = None;
    v.copy_image(|w, h, b| {
        got = Some((w, h, b.to_vec()));
        Ok(())
    });
    assert_eq!(got, Some((2, 1, rgba)));
    assert_eq!(v.status.take().as_deref(), Some("Copied 3D view (2×1)"));
    v.copy_image(|_, _, _| Err("busy".into()));
    assert_eq!(v.status.take().as_deref(), Some("Copy failed: busy"));
}

#[test]
fn the_3d_keys_reset_cycle_and_are_listed() {
    use eframe::egui::Key;
    use sdc_gui::ui::actions::{action, keys_text, Area};
    for (id, keys) in [
        ("3d-reset", "Home"),
        ("3d-debug", "V"),
        ("3d-copy", "Ctrl+Shift+C"),
    ] {
        let a = action(id).unwrap_or_else(|| panic!("{id}"));
        assert_eq!(a.area, Area::View3d);
        assert_eq!(keys_text(id), keys);
    }
    let mut v = Viewport3d::with_state(V3State::Ready);
    v.orbit.pan(30.0, 5.0);
    let mut h = harness(v);
    h.run();
    h.key_press(Key::Home);
    h.run();
    assert_eq!(h.state().orbit, v3::Orbit::default());
    h.key_press(Key::V);
    h.run();
    assert_eq!(h.state().debug(), v3::DebugView::UvChecker);
    h.key_press(Key::V);
    h.run();
    assert_eq!(h.state().debug(), v3::DebugView::Lit);
}

fn shape_info(id: u32, m: MaterialState) -> ShapeInfo {
    ShapeInfo {
        id: ShapeId(id),
        name: format!("s{id}"),
        material: m,
        status: ShapeStatus::Drawing,
    }
}

fn pbr() -> MaterialState {
    let mut m = MaterialState::builtin_default();
    m.slsf2 |= nif::material::SLSF2_TRUE_PBR;
    m
}

#[test]
fn offered_views_follow_support_and_the_loaded_materials() {
    use v3::DebugView::*;
    let (mut v, _cmds) = rendered();
    v.handle(v3::V3Reply::Shapes(vec![shape_info(
        0,
        MaterialState::builtin_default(),
    )]));
    assert_eq!(v.offered(), [Lit, UvChecker], "no support reported yet");
    v.handle(v3::V3Reply::DebugSupport(true));
    assert_eq!(
        v.offered(),
        [Lit, UvChecker, Albedo, Normals, Roughness, Ao],
        "non-PBR: no Metallic"
    );
    assert_eq!(v.stand_in(Roughness), Some("1 − specular mask"));
    assert_eq!(v.stand_in(Albedo), None);
    let mut face = MaterialState::builtin_default();
    face.shader_type = 4;
    v.handle(v3::V3Reply::Shapes(vec![shape_info(0, face.clone())]));
    assert!(!v.offered().contains(&Ao), "FaceGen: vertex AO is constant");
    v.handle(v3::V3Reply::Shapes(vec![
        shape_info(0, face),
        shape_info(1, pbr()),
    ]));
    assert_eq!(v.offered().len(), 7, "one PBR shape offers everything");
    assert_eq!(Lit.next(&v.offered()), UvChecker);
    assert_eq!(Ao.next(&v.offered()), Lit, "wraps over the offered views");
    assert_eq!(
        Metallic.next(&[Lit, UvChecker]),
        Lit,
        "a view not offered goes to the first"
    );
}

#[test]
fn a_load_that_drops_the_current_view_falls_back_to_lit() {
    let (mut v, cmds) = rendered();
    v.handle(v3::V3Reply::DebugSupport(true));
    v.handle(v3::V3Reply::Shapes(vec![shape_info(0, pbr())]));
    v.set_debug(v3::DebugView::Metallic);
    assert!(matches!(
        cmds.try_recv(),
        Ok(v3::V3Cmd::SetDebug(v3::DebugView::Metallic))
    ));
    v.handle(v3::V3Reply::Reconfigured(1));
    v.handle(v3::V3Reply::Shapes(vec![shape_info(
        0,
        MaterialState::builtin_default(),
    )]));
    assert_eq!(v.debug(), v3::DebugView::Lit);
    assert!(matches!(
        cmds.try_recv(),
        Ok(v3::V3Cmd::SetDebug(v3::DebugView::Lit))
    ));
}

#[test]
fn losing_debug_support_falls_back_to_lit_with_a_status_line() {
    let (mut v, cmds) = rendered();
    v.handle(v3::V3Reply::DebugSupport(true));
    v.handle(v3::V3Reply::Shapes(vec![shape_info(0, pbr())]));
    v.set_debug(v3::DebugView::Albedo);
    let _ = cmds.try_recv();
    v.handle(v3::V3Reply::Reconfigured(1));
    // A CS update without the anchor (SetShaders), or a refused SetDebug.
    v.handle(v3::V3Reply::DebugSupport(false));
    assert_eq!(v.debug(), v3::DebugView::Lit);
    assert_eq!(v.status.as_deref(), Some(v3::DEBUG_UNSUPPORTED));
    assert!(matches!(
        cmds.try_recv(),
        Ok(v3::V3Cmd::SetDebug(v3::DebugView::Lit))
    ));
    assert_eq!(v.offered(), [v3::DebugView::Lit, v3::DebugView::UvChecker]);
}

#[test]
fn the_view_section_lists_the_offered_views_with_the_stand_in_hover() {
    let (mut v, _cmds) = rendered();
    v.handle(v3::V3Reply::DebugSupport(true));
    v.handle(v3::V3Reply::Shapes(vec![shape_info(
        0,
        MaterialState::builtin_default(),
    )]));
    let mut h = harness(v);
    // The harness size asks for a frame no thread answers: busy, so it repaints; step instead of run().
    h.run_steps(4);
    h.get_by_label(sdc_gui::ui::viewport3d::VIEW_MENU).click();
    h.run_steps(4);
    for offered in ["Lit", "UV checker", "Albedo", "Normals", "Roughness", "AO"] {
        h.get_by_label(offered);
    }
    assert!(h.query_by_label("Metallic").is_none());
    h.get_by_label("Roughness").hover();
    h.run_steps(4);
    h.get_by_label_contains("1 − specular mask");
}

/// Audit 3 item 3: two panels both count frame ids from 1; each keeps its own texture.
#[test]
fn two_panels_with_the_same_frame_id_keep_their_own_textures() {
    let frame = |c: u8| {
        Some(std::sync::Arc::new(v3::Frame {
            width: 2,
            height: 2,
            rgba: vec![c; 16],
            id: 1,
        }))
    };
    let mut a = Viewport3d::with_state(V3State::Ready);
    a.panel = "meshes";
    a.frame = frame(10);
    let mut b = Viewport3d::with_state(V3State::Ready);
    b.panel = "set_preview";
    b.frame = frame(200);
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(400.0, 700.0))
        .build_ui_state(
            |ui, (a, b): &mut (Viewport3d, Viewport3d)| {
                sdc_gui::ui::viewport3d::image(ui, a);
                sdc_gui::ui::viewport3d::image(ui, b);
            },
            (a, b),
        );
    h.run();
    let get = |p| {
        h.ctx.data(|d| {
            d.get_temp::<(u64, eframe::egui::TextureHandle)>(
                sdc_gui::ui::viewport3d::frame_cache_id(p),
            )
        })
    };
    let (ta, tb) = (get("meshes").unwrap().1, get("set_preview").unwrap().1);
    assert_ne!(ta.id(), tb.id());
}

// ---- v2 Phase D, D8: the "Accurate (deferred)" toggle ----

#[test]
fn deferred_toggle_sends_one_command_and_rerenders() {
    let (mut v, cmds) = rendered();
    assert!(v.look.deferred, "on by default");
    let rid = v.render_sent_for_test();
    frame_reply(&mut v, rid); // nothing in flight before the toggle
    assert!(!v.busy());
    v.set_deferred(false);
    let sent: Vec<v3::V3Cmd> = cmds.try_iter().collect();
    assert_eq!(sent.len(), 1, "exactly one command");
    assert!(matches!(sent[0], v3::V3Cmd::SetDeferred(false)));
    assert!(v.busy(), "awake until the render thread answers the toggle");
    v.set_deferred(false);
    assert!(cmds.try_recv().is_err(), "the same value sends nothing");
    v.request_frame(64, 64);
    let sent: Vec<v3::V3Cmd> = cmds.try_iter().collect();
    assert_eq!(sent.len(), 1, "a render, no load");
    match &sent[0] {
        v3::V3Cmd::Render { look, .. } => assert!(!look.deferred),
        _ => panic!("a render, no load"),
    }
    let rid = v.render_sent_for_test();
    frame_reply(&mut v, rid);
    v.handle(v3::V3Reply::Reconfigured(1));
    assert!(!v.busy());
}

/// Step-2 choice: `fake_render_thread` hands `ensure_thread` a channel instead of spawning the render loop, so the
/// real `request_frame → ensure_thread → Load` order is what is checked (`attach_test_thread` stays silent).
#[test]
fn a_saved_deferred_choice_is_sent_before_the_first_load() {
    for on in [true, false] {
        let d = tempfile::tempdir().unwrap();
        let mut v = Viewport3d::new(Some(fp(d.path(), "CS", "a")));
        v.on_located(Ok(Some(fp(d.path(), "CS", "a"))));
        v.cache_dir = Some(d.path().to_path_buf());
        v.look.deferred = on;
        let cmds = v.fake_render_thread();
        v.set_mesh(Some(spec(1, false)));
        v.request_frame(64, 64);
        let sent: Vec<v3::V3Cmd> = cmds.try_iter().collect();
        if on {
            assert_eq!(sent.len(), 2, "on: SetDeferred, then Load");
            assert!(matches!(sent[0], v3::V3Cmd::SetDeferred(true)));
            assert!(matches!(sent[1], v3::V3Cmd::Load { .. }));
            assert!(v.busy());
        } else {
            assert_eq!(sent.len(), 1, "off: the thread starts forward, Load only");
            assert!(matches!(sent[0], v3::V3Cmd::Load { .. }));
        }
    }
}

#[test]
fn deferred_toggle_is_in_the_view_menu() {
    let mut v = Viewport3d::with_state(V3State::Ready);
    v.look.sun = 3.0;
    v.look.hdri = true;
    let mut h = harness(v);
    h.run();
    h.get_by_label(sdc_gui::ui::viewport3d::VIEW_MENU).click();
    h.run();
    h.get_by_label(v3::DEFERRED_LABEL).click();
    h.run();
    assert!(!h.state().look.deferred);
    h.get_by_label("Reset view").click();
    h.run();
    assert!(!h.state().look.deferred, "Reset view keeps the toggle");
    assert_eq!(
        h.state().look,
        v3::Look {
            deferred: false,
            ..v3::Look::default()
        }
    );
}

fn alpha_tested() -> MaterialState {
    let mut m = MaterialState::builtin_default();
    m.alpha = Some(nif::props::Alpha {
        flags: 0x200,
        threshold: 0,
    }); // DO_ALPHA_TEST: the stand-in's pixel permutation compiles
    m
}

fn lit(v: &Viewport3d) -> bool {
    v.frame.as_ref().is_some_and(|f| {
        f.rgba
            .chunks(4)
            .any(|p| p[0] as u32 + p[1] as u32 + p[2] as u32 > 0)
    })
}

fn wait_lit(v: &mut Viewport3d) {
    let t = std::time::Instant::now();
    while !lit(v) && t.elapsed().as_secs() < 10 {
        v.poll();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(lit(v), "a lit frame");
}

/// A viewport on a copy of the stand-in whose `DeferredCompositeCS.hlsl` is `composite` (None: the stand-in's).
fn standin_copy(composite: Option<&str>) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for f in ["Lighting.hlsl", "DeferredCompositeCS.hlsl"] {
        std::fs::copy(standin_root().join(f), d.path().join(f)).unwrap();
    }
    if let Some(c) = composite {
        std::fs::write(d.path().join("DeferredCompositeCS.hlsl"), c).unwrap();
    }
    d
}

fn viewport_on(shaders: &Path, cache: &Path) -> Viewport3d {
    let f = render3d::locate::fingerprint(shaders, None).unwrap();
    let mut v = Viewport3d::new(Some(f.clone()));
    v.on_located(Ok(Some(f)));
    v.cache_dir = Some(cache.to_path_buf());
    v.set_content(
        Some(v3::Content::Builtin {
            key: 1,
            shape: Builtin::Sphere,
            material: alpha_tested(),
            textures: Arc::new(HashMap::new()),
        }),
        false,
    );
    v
}

const DEFERRED_NOTICE: &str = "Deferred preview unavailable: ";

#[test]
fn deferred_falls_back_and_shows_the_notice() {
    let shaders = standin_copy(Some("this is not hlsl"));
    let cache = tempfile::tempdir().unwrap();
    let mut v = viewport_on(shaders.path(), cache.path());
    assert!(v.look.deferred);
    run_idle(&mut v);
    let notice = v.notice.clone().expect("the fallback notice");
    assert!(notice.starts_with(DEFERRED_NOTICE), "{notice}");
    assert_eq!(v.shapes[0].status, ShapeStatus::Drawing);
    wait_lit(&mut v);
    assert!(v.look.deferred, "D-P30: the preference stays");
    // D-P33: turning it off says nothing new; turning it on again reports the failure again.
    v.notice = None;
    v.set_deferred(false);
    run_idle(&mut v);
    assert_eq!(v.notice, None, "no notice when the toggle turns off");
    v.set_deferred(true);
    run_idle(&mut v);
    let again = v.notice.clone().expect("the notice is back");
    assert!(again.starts_with(DEFERRED_NOTICE), "{again}");
    let mut h = harness(v);
    h.run_steps(4);
    h.get_by_label(&again);
}

#[test]
fn deferred_on_with_the_standin_draws_and_goes_idle() {
    let shaders = standin_copy(None);
    let cache = tempfile::tempdir().unwrap();
    let mut v = viewport_on(shaders.path(), cache.path());
    run_idle(&mut v);
    assert_eq!(v.notice, None);
    assert_eq!(v.shapes[0].status, ShapeStatus::Drawing);
    wait_lit(&mut v);
    v.report = None;
    let before = v.frame.as_ref().unwrap().id;
    v.set_deferred(false);
    run_idle(&mut v);
    let t = std::time::Instant::now();
    while v.frame.as_ref().unwrap().id == before && t.elapsed().as_secs() < 10 {
        v.poll();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_ne!(v.frame.as_ref().unwrap().id, before, "re-rendered");
    assert!(lit(&v));
    assert_eq!(v.notice, None);
    assert!(
        v.report.is_none(),
        "no Loaded reply: nothing was loaded again"
    );
}
