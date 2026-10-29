//! The Meshes view (plan A Task 12): per-mod NIF list with PGPatcher-aware badges and texture slots.
use eframe::egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use modscan::plugin_store::{MeshUse, MissingRef, ShapeSwap};
use modscan::plugins::PluginStats;
use modscan::scan::Report;
use modscan::store::{ModRow, NifRow, ShapeView, TexResolution};
use sdc_gui::model::meshes::{Banner, FileRefs, MeshesDialog, MeshesModel, NifFilter, ScanTarget};
use sdc_gui::settings::MeshesSettings;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A harness over `m`, with the system fonts as the app has them (see `ui_smoke.rs`).
fn harness(m: MeshesModel) -> Harness<'static, MeshesModel> {
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 800.0))
        .build_ui_state(
            |ui, m: &mut MeshesModel| sdc_gui::ui::meshes::show(ui, m),
            m,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

fn mod_row(id: i64, name: &str, kind: i32, derived: bool) -> ModRow {
    ModRow {
        id,
        name: name.into(),
        kind,
        priority: 0,
        derived,
        nifs: 1,
    }
}

fn nif(copy_id: i64, path: &str) -> NifRow {
    NifRow {
        copy_id,
        path: path.into(),
        wins: true,
        overridden_by: None,
        n_copies: 1,
        derived: false,
        derive_check: None,
        le_mesh: false,
        problem: None,
        patched_in_game: false,
    }
}

fn model_with(banner: Option<Banner>) -> MeshesModel {
    let mut m = MeshesModel::empty();
    m.mods = vec![mod_row(1, "A", 1, false)];
    // The mod is open: its NIFs are listed under it.
    m.selected_mod = Some(1);
    m.banner = banner;
    m
}

#[test]
fn running_banner_offers_retry_and_skip() {
    let mut h = harness(model_with(Some(Banner::PgpRunning)));
    h.run();
    h.get_by_label("PGPatcher is running — scan paused.");
    h.get_by_label("Retry");
    h.get_by_label("Scan without PGPatcher output").click();
    h.run();
    assert!(h.state_mut().take_request().is_some_and(|r| r.skip_derived));
    h.get_by_label("Retry").click();
    h.run();
    assert!(h
        .state_mut()
        .take_request()
        .is_some_and(|r| !r.skip_derived));
}

#[test]
fn mod_list_and_badges() {
    let mut m = model_with(None);
    m.nifs = Arc::new(vec![NifRow {
        wins: false,
        n_copies: 2,
        derive_check: Some("stale_changed".into()),
        patched_in_game: true,
        ..nif(1, "meshes/x.nif")
    }]);
    let mut h = harness(m);
    h.run();
    // Note GC: the picked mod in the picker, and in the row's Mod column.
    picker(&h, "A");
    h.get_by_role_and_label(Role::Label, "A");
    // Review C17: the row says the path is under meshes\ (a dimmed prefix), so !ube/… reads as a path.
    h.get_by_label("meshes/x.nif");
    h.get_by_label("Patched by PGPatcher (in game)");
    h.get_by_label("PGPatcher patch out of date");
}

/// Note GC16 (review fix 3): in a row too narrow for every chip, the warnings come first and stay in sight, and
/// hovering the chips lists every one in full (rule 3.4), the mod's own chip too.
#[test]
fn a_patched_stale_row_keeps_its_warning_in_sight_and_its_hover_lists_every_chip() {
    use sdc_gui::model::meshes::{DERIVED_CHIP, STALE_CHIP};
    let mut m = MeshesModel::empty();
    m.mods = vec![mod_row(1, "PGPatcher output", 1, true)];
    let row = NifRow {
        wins: false,
        patched_in_game: true,
        le_mesh: true,
        derive_check: Some("stale_changed".into()),
        ..nif(1, "meshes/x.nif")
    };
    m.winners = Arc::new(vec![(row, "PGPatcher output".to_string())]);
    let mut h = harness(m);
    h.run();
    // A label cut by its cell is elided ("…"), so each warning's rect must hold its whole shown text: the short
    // "Stale patch" at least (its full name is the AccessKit name).
    let font = eframe::egui::TextStyle::Small.resolve(&h.ctx.global_style());
    let width = |text: &str| {
        h.ctx.fonts_mut(|f| {
            f.layout_no_wrap(text.into(), font.clone(), eframe::egui::Color32::WHITE)
                .size()
                .x
        })
    };
    for (name, shown) in [(STALE_CHIP, "Stale patch"), ("LE", "LE")] {
        let r = h.get_by_role_and_label(Role::Label, name).rect();
        assert!(r.width() + 0.5 >= width(shown), "{name} is cut by its cell: {r:?}");
    }
    h.get_by_role_and_label(Role::Label, "LE").hover();
    h.run();
    for text in [
        "Patched by PGPatcher (in game)",
        STALE_CHIP,
        "LE",
        DERIVED_CHIP,
    ] {
        assert_eq!(
            h.query_all_by_label(text).count(),
            2,
            "{text}: the chip and its line on hover"
        );
    }
}

#[test]
fn filters_narrow_the_nif_list_and_le_and_problem_chips_show() {
    let mut m = model_with(None);
    m.nifs = Arc::new(vec![
        nif(1, "meshes/a.nif"),
        NifRow {
            wins: false,
            n_copies: 3,
            overridden_by: Some("B".into()),
            ..nif(2, "meshes/b.nif")
        },
        NifRow {
            le_mesh: true,
            problem: Some("block 3: bad ref".into()),
            ..nif(3, "meshes/c.nif")
        },
    ]);
    let mut h = harness(m);
    h.run();
    h.get_by_label("LE");
    h.get_by_label("Problem");
    h.get_by_label("Conflicted").click();
    h.run();
    assert_eq!(h.state().filter, NifFilter::Conflicted);
    assert!(h.query_by_label("meshes/a.nif").is_none());
    h.get_by_label("meshes/b.nif");
    h.get_by_label("Problems").click();
    h.run();
    assert!(h.query_by_label("meshes/b.nif").is_none());
    h.get_by_label("meshes/c.nif");
    h.get_by_label_contains("1 of 3");
}

fn shape(textures: Vec<Option<&str>>) -> ShapeView {
    let n = textures.len();
    ShapeView {
        block: 4,
        name: Some("Body".into()),
        kind: "BSTriShape".into(),
        shader_kind: 1,
        shader_type: Some(1),
        slsf1: Some(0x8000_0101),
        slsf2: Some(0x21),
        textures: textures.into_iter().map(|t| t.map(String::from)).collect(),
        raw: vec![None; n],
        alpha_flags: None,
        alpha_threshold: None,
        params: None,
        skinned: true,
        vertex_desc: None,
        num_vertices: Some(120),
        num_triangles: Some(80),
        index3d: Some(0),
    }
}

#[test]
fn shapes_show_shader_type_flags_and_slot_resolution() {
    let mut m = model_with(None);
    m.nifs = Arc::new(vec![NifRow {
        le_mesh: true,
        ..nif(7, "meshes/x.nif")
    }]);
    m.selected_nif = Some(7);
    let mut s = shape(vec![Some("textures/a.dds"), Some("textures/b_n.dds"), None]);
    s.raw[1] = Some(modscan::store::RawSlot {
        raw: "C:\\Game\\Data\\Textures\\b_n.dds".into(),
        flags: nif::path::PathFlags {
            rebased: true,
            ..Default::default()
        },
    });
    m.shapes = vec![s];
    m.shapes_col = true;
    m.resolved.insert(
        "textures/a.dds".into(),
        TexResolution::Found {
            origin: "A".into(),
            n_copies: 2,
        },
    );
    m.resolved
        .insert("textures/b_n.dds".into(), TexResolution::Missing);
    let mut h = harness(m);
    h.run();
    h.get_by_label("LE mesh: optimise it for SSE, e.g. with Cathedral Assets Optimizer.");
    h.get_by_label_contains("Body");
    h.get_by_label_contains("Environment Map (1)");
    // Meshes UI T2: flags, counts, origins and raw paths sit in Details, closed by default.
    assert!(h.query_by_label_contains("SLSF1 0x80000101").is_none());
    h.get_by_label("Details of NIF block 4").click();
    h.run();
    h.get_by_label_contains("SLSF1 0x80000101");
    h.get_by_label_contains("SLSF2 0x00000021");
    h.get_by_label_contains("120 vertices");
    h.get_by_label("textures/a.dds");
    h.get_by_label_contains("→ A (2 copies)");
    h.get_by_label_contains("→ missing");
    h.get_by_label_contains("C:\\Game\\Data\\Textures\\b_n.dds");
    h.get_by_label("(1 empty slot)");
    // Clicking a texture asks who else uses it (no store here: an empty answer).
    h.get_by_label("textures/a.dds").click();
    h.run();
    assert_eq!(
        h.state().tex_users.as_ref().map(|u| u.0.as_str()),
        Some("textures/a.dds")
    );
}

#[test]
fn incomplete_and_stale_banners_use_the_spec_text() {
    assert_eq!(
        Banner::PgpIncomplete("PGP".into()).text(),
        "PGPatcher output 'PGP' looks incomplete (no ParallaxGen_Diff.json). Re-run PGPatcher."
    );
    let mut h = harness(model_with(Some(Banner::PgpStale(3))));
    h.run();
    h.get_by_label("PGPatcher output is out of date for 3 meshes.");
    h.get_by_label("Show list");
}

#[test]
fn mo2_first_run_dialog_saves_the_answer() {
    let mut m = MeshesModel::empty();
    m.activate_with(Some((PathBuf::from("C:\\MO2\\Acherus"), "Nexus".into())));
    assert!(matches!(m.dialog, Some(MeshesDialog::Mo2 { .. })));
    let mut h = harness(m);
    h.run();
    h.get_by_label(
        "Opened from Mod Organizer 2 instance 'C:\\MO2\\Acherus', profile 'Nexus'. Use it?",
    );
    h.get_by_label("Choose instance…");
    h.get_by_label("Open a folder instead");
    // egui hit-tests against the previous frame: let the pointer settle on the button first (as in ui_smoke.rs).
    h.get_by_label("Use").hover();
    h.run();
    h.get_by_label("Use").click();
    h.run();
    let m = h.state_mut();
    assert!(m.dialog.is_none());
    let s = m.take_settings().expect("the answer is saved");
    assert_eq!(s.instance, Some(PathBuf::from("C:\\MO2\\Acherus")));
    assert_eq!(s.profile.as_deref(), Some("Nexus"));
    assert!(matches!(&m.target, Some(ScanTarget::Mo2 { profile, .. }) if profile == "Nexus"));
}

#[test]
fn a_saved_instance_skips_the_dialog() {
    let saved = MeshesSettings {
        instance: Some(PathBuf::from("C:\\MO2\\Acherus")),
        profile: Some("Nexus".into()),
        folder: None,
    };
    let mut m = MeshesModel::new(None, saved);
    m.activate_with(Some((PathBuf::from("C:\\MO2\\Acherus"), "Nexus".into())));
    assert!(m.dialog.is_none());
    assert!(m.target.is_some());
}

// ---- Through the worker and a real store ----

fn put(root: &Path, rel: &str, bytes: Option<&[u8]>) {
    let fixture = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../nif/tests/fixtures/TestNifFile_Skinned_SE.nif"
    ))
    .unwrap();
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

fn wait(m: &mut MeshesModel) {
    wait_for(m, Duration::from_secs(60));
}

/// Until the scan and the list read it starts (Note GC) have ended.
fn wait_for(m: &mut MeshesModel, limit: Duration) {
    let t = Instant::now();
    m.poll();
    while m.scanning() || m.rows_loading() {
        assert!(t.elapsed() < limit, "scan did not end");
        std::thread::sleep(Duration::from_millis(10));
        m.poll();
    }
}

#[test]
fn a_scan_through_the_worker_fills_mods_nifs_and_both_sides_of_a_patched_mesh() {
    let d = instance("+PGP\n+A\n");
    put(d.path(), "mods/A/meshes/x.nif", None);
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
    let db = d.path().join("store/scan.db");
    let mut m = scan_model(db);
    m.activate_with(None);
    m.use_instance(d.path().to_path_buf(), "Default".into());
    m.request_scan(false);
    let req = m.take_request().unwrap();
    m.start(req);
    assert!(m.scanning());
    wait(&mut m);
    assert_eq!(m.error, None);
    assert!(m.report.is_some());
    assert_eq!(m.banner, None);
    let pgp = m.mods.iter().find(|r| r.name == "PGP").unwrap();
    assert!(pgp.derived);
    let a_id = m.mods.iter().find(|r| r.name == "A").unwrap().id;
    m.select_mod(a_id);
    assert_eq!(m.nifs.len(), 1);
    assert_eq!(m.nifs[0].status_label(), "Patched by PGPatcher (in game)");
    let copy = m.nifs[0].copy_id;
    m.select_nif(copy);
    let pair = m
        .pair
        .expect("a patched mesh has an in-game and a source copy");
    assert_eq!(pair.source, copy);
    assert_ne!(pair.game, copy);
    assert!(!m.view_source);
    assert_eq!(m.shown_copy(), Some(pair.game));
    assert!(!m.shapes.is_empty());
    m.set_view_source(true);
    assert_eq!(m.shown_copy(), Some(copy));
    assert!(!m.shapes.is_empty());

    // The same through the UI: the toggle is there for a mesh with a derived copy.
    let mut h = harness(m);
    h.run();
    h.get_by_label("In game");
    h.get_by_label("Source");
}

#[test]
fn a_running_pgpatcher_is_a_banner_and_skip_scans_without_its_output() {
    let d = instance("+PGP\n+A\n");
    put(d.path(), "mods/A/meshes/x.nif", None);
    put(d.path(), "mods/PGP/meshes/x.nif", None);
    put(d.path(), "mods/PGP/PGPatcher.esp", Some(b"e"));
    put(d.path(), "mods/PGP/ParallaxGen_Diff.json", Some(b"{}"));
    let db = d.path().join("scan.db");
    let mut m = scan_model(db);
    m.procs = || vec!["pgpatcher.exe".to_string()];
    m.activate_with(None);
    m.use_instance(d.path().to_path_buf(), "Default".into());
    m.request_scan(false);
    let req = m.take_request().unwrap();
    m.start(req);
    wait(&mut m);
    assert_eq!(m.banner, Some(Banner::PgpRunning));
    m.request_scan(true);
    let req = m.take_request().unwrap();
    m.start(req);
    wait(&mut m);
    assert_eq!(m.error, None);
    assert_eq!(m.banner, None);
    assert!(m.mods.iter().all(|r| r.name != "PGP"));
}

#[test]
fn stale_output_raises_the_stale_banner_with_its_list() {
    let d = instance("+PGP\n+A\n");
    put(d.path(), "mods/A/meshes/x.nif", None);
    put(d.path(), "mods/PGP/meshes/x.nif", None);
    put(d.path(), "mods/PGP/PGPatcher.esp", Some(b"e"));
    put(
        d.path(),
        "mods/PGP/ParallaxGen_Diff.json",
        Some(br#"{"meshes\\x.nif":{"crc32original":1,"crc32patched":2}}"#),
    );
    let mut m = scan_model(d.path().join("scan.db"));
    m.activate_with(None);
    m.use_instance(d.path().to_path_buf(), "Default".into());
    m.request_scan(false);
    let req = m.take_request().unwrap();
    m.start(req);
    wait(&mut m);
    assert_eq!(m.banner, Some(Banner::PgpStale(1)));
    m.show_stale_list();
    assert_eq!(
        m.stale_list.as_deref(),
        Some(&[("meshes/x.nif".to_string(), "PGP".to_string())][..])
    );
}

#[test]
fn a_database_error_is_shown_not_swallowed() {
    let d = tempfile::tempdir().unwrap();
    // A directory where the database file should be: the store cannot open.
    let db = d.path().join("scan.db");
    std::fs::create_dir_all(&db).unwrap();
    let mut m = scan_model(db);
    m.activate_with(None);
    assert!(m.error.is_some());
    let mut h = harness(m);
    h.run();
    h.get_by_label_contains("Scan database");
}

#[test]
fn the_header_toggles_between_textures_and_meshes() {
    use sdc_gui::model::AppModel;
    use sdc_gui::settings::GuiSettings;
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 800.0))
        .build_ui_state(
            |ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m),
            AppModel::placeholder(GuiSettings::default()),
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    h.get_by_label("Textures view");
    assert!(!sdc_gui::ui::header::meshes_view_active(&h.ctx));
    h.get_by_label("Meshes view").hover();
    h.run();
    h.get_by_label("Meshes view").click();
    h.run();
    assert!(sdc_gui::ui::header::meshes_view_active(&h.ctx));
}

#[test]
fn meshes_settings_round_trip_in_gui_toml() {
    use sdc_gui::settings::GuiSettings;
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("gui.toml");
    let mut s = GuiSettings::default();
    s.meshes.instance = Some(PathBuf::from(r"C:\MO2\Acherus"));
    s.meshes.profile = Some("Nexus".into());
    s.save(&p).unwrap();
    let text = std::fs::read_to_string(&p).unwrap();
    assert!(text.contains("[meshes]"), "{text}");
    assert_eq!(GuiSettings::load(&p), (s, None));
    // An older file without [meshes] still loads.
    std::fs::write(&p, "preview_max = 2048\n").unwrap();
    assert_eq!(GuiSettings::load(&p).0.meshes, MeshesSettings::default());
}

/// Headless stand-in for the owner's manual check (plan step 5): scans the owner's MO2 instance read-only through
/// the model and the worker, with the database in %TEMP%, and prints what the view would show. No asserts on the
/// machine's data. `cargo test --release -p sdc-gui --test meshes_view owner_meshes_view -- --ignored --nocapture`
#[test]
#[ignore]
fn owner_meshes_view() {
    let Some(root) = std::env::var_os("SDC_ACHERUS").map(PathBuf::from) else {
        println!("skipped: set SDC_ACHERUS");
        return;
    };
    if !root.join("ModOrganizer.ini").is_file() {
        println!("owner_meshes_view: {} not found, skipping", root.display());
        return;
    }
    let db = std::env::temp_dir().join("sdc-meshes-view-check.db");
    for ext in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{ext}", db.display()));
    }
    let mut m = scan_model(db);
    m.activate_with(None);
    m.choose_instance(root);
    println!("target: {:?}", m.target);
    let t = Instant::now();
    m.request_scan(false);
    let req = m.take_request().unwrap();
    m.start(req);
    wait_for(&mut m, Duration::from_secs(900));
    println!("scan: {:.1} s wall", t.elapsed().as_secs_f64());
    println!("status: {}", m.status);
    println!("error: {:?}", m.error);
    println!("banner: {:?}", m.banner.as_ref().map(Banner::text));
    if let Some(r) = &m.report {
        println!("phase_ms: {:?}", r.phase_ms);
        println!("notes: {:?}; missing mods: {:?}", r.notes, r.missing_mods);
    }
    let kinds = |k: i32| m.mods.iter().filter(|r| r.kind == k).count();
    println!(
        "mods list: {} rows (data {}, mods {}, overwrite {}, separators {}, bsa {}); derived chips: {:?}",
        m.mods.len(),
        kinds(0),
        kinds(1),
        kinds(2),
        kinds(3),
        kinds(4),
        m.mods.iter().filter(|r| r.derived).map(|r| r.name.as_str()).collect::<Vec<_>>()
    );
    let Some(id) = m
        .mods
        .iter()
        .find(|r| r.name.eq_ignore_ascii_case("Assorted mesh fixes"))
        .map(|r| r.id)
    else {
        println!("no mod named 'Assorted mesh fixes'");
        return;
    };
    let t = Instant::now();
    m.select_mod(id);
    println!(
        "select_mod: {} NIFs in {} ms",
        m.nifs.len(),
        t.elapsed().as_millis()
    );
    let mut labels: std::collections::BTreeMap<&str, usize> = Default::default();
    for n in m.nifs.iter() {
        *labels.entry(n.status_label()).or_default() += 1;
    }
    let stale = m
        .nifs
        .iter()
        .filter(|n| {
            n.derive_check
                .as_deref()
                .is_some_and(|c| c.starts_with("stale"))
        })
        .count();
    let le = m.nifs.iter().filter(|n| n.le_mesh).count();
    let problems = m.nifs.iter().filter(|n| n.problem.is_some()).count();
    println!("badges: {labels:?}; stale chips {stale}; LE {le}; problems {problems}");
    if let Some(n) = m.nifs.iter().find(|n| n.patched_in_game).cloned() {
        let t = Instant::now();
        m.select_nif(n.copy_id);
        println!(
            "patched example {}: pair {:?}, {} shapes in game, {} ms",
            n.path,
            m.pair,
            m.shapes.len(),
            t.elapsed().as_millis()
        );
        m.set_view_source(true);
        println!("  {} shapes in source", m.shapes.len());
        if let Some(s) = m.shapes.first() {
            for t in s.textures.iter().flatten() {
                println!("  slot path {t} -> {:?}", m.resolved.get(t));
            }
        }
    }
    println!("error after reads: {:?}", m.error);
}

fn derived_model() -> MeshesModel {
    let mut m = MeshesModel::empty();
    m.mods = vec![
        mod_row(2, "ParallaxGen_Output", 1, true),
        mod_row(1, "Assorted mesh fixes", 1, false),
    ];
    m
}

#[test]
fn select_mod_is_a_no_op_for_a_derived_mod() {
    let mut m = derived_model();
    m.nifs = Arc::new(vec![nif(9, "meshes/keep.nif")]);
    m.select_mod(2);
    assert_eq!(m.selected_mod, None);
    assert_eq!(m.nifs.len(), 1, "no store read, nothing cleared");
}

// ---- The flat list (Note GC) ----

use sdc_gui::model::meshes::display_path;

#[test]
fn display_path_drops_a_leading_meshes_folder_only() {
    assert_eq!(display_path("meshes/a/b.nif"), "a/b.nif");
    assert_eq!(display_path("Meshes\\a\\b.nif"), "a\\b.nif");
    assert_eq!(display_path("MESHES/x.nif"), "x.nif");
    assert_eq!(display_path("meshes"), "meshes");
    assert_eq!(display_path("meshesx/a.nif"), "meshesx/a.nif");
    assert_eq!(display_path("a/meshes/b.nif"), "a/meshes/b.nif");
    assert_eq!(display_path("Ünï/ä.nif"), "Ünï/ä.nif");
    assert_eq!(display_path(""), "");
}

#[test]
fn the_path_filter_and_the_chips_combine() {
    let mut m = model_with(None);
    m.nifs = Arc::new(vec![
        nif(1, "meshes/Armor/iron.nif"),
        NifRow {
            wins: false,
            n_copies: 2,
            ..nif(2, "meshes/armor/steel.nif")
        },
        nif(3, "meshes/rock/big.nif"),
    ]);
    assert_eq!(*m.visible_nifs(), [0, 1, 2]);
    m.search = "ARMOR\\".into();
    assert_eq!(*m.visible_nifs(), [0, 1]);
    m.search = "meshes/armor/s".into();
    assert_eq!(*m.visible_nifs(), [1]);
    m.search = "armor".into();
    m.filter = NifFilter::Wins;
    assert_eq!(*m.visible_nifs(), [0]);
    m.filter = NifFilter::All;
    let mut h = harness(m);
    h.run();
    h.get_by_label_contains("2 of 3 NIFs");
    h.get_by_label("meshes/Armor/iron.nif");
    assert!(h.query_by_label("meshes/rock/big.nif").is_none());
}

/// B (highest) and A both have rock.nif; A also has tree/oak.nif; the separator owns B.
fn scanned() -> (tempfile::TempDir, MeshesModel) {
    scanned_with(None)
}

/// `scanned`, with `oak` as tree/oak.nif's bytes (else the same fixture as rock.nif).
fn scanned_with(oak: Option<&[u8]>) -> (tempfile::TempDir, MeshesModel) {
    let d = instance("+B\n-Sep_separator\n+A\n");
    std::fs::create_dir_all(d.path().join("mods/B")).unwrap();
    put(d.path(), "mods/A/meshes/rock.nif", None);
    put(d.path(), "mods/A/meshes/tree/oak.nif", oak);
    put(d.path(), "mods/B/meshes/rock.nif", None);
    let mut m = scan_model(d.path().join("scan.db"));
    m.procs = || vec![];
    m.activate_with(None);
    m.use_instance(d.path().to_path_buf(), "Default".into());
    m.request_scan(false);
    let req = m.take_request().unwrap();
    m.start(req);
    wait(&mut m);
    assert_eq!(m.error, None);
    (d, m)
}

/// The visible rows as (path, mod).
fn rows_of(m: &mut MeshesModel) -> Vec<(String, String)> {
    m.visible_nifs()
        .iter()
        .map(|&i| {
            let (n, o) = m.nif_at(i);
            (n.path.clone(), o.to_string())
        })
        .collect()
}

/// Another scan's lock on `scan.lock` (a Windows sharing lock): a scan started now ends with Busy, not completed.
#[cfg(windows)]
fn hold_scan_lock(dir: &Path) -> std::fs::File {
    use std::os::windows::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(dir.join("scan.lock"))
        .unwrap()
}

/// Starts a scan; the caller ends it (`wait`).
fn start_scan(m: &mut MeshesModel) {
    m.request_scan(false);
    let req = m.take_request().unwrap();
    m.start(req);
    assert!(m.scanning());
}

/// Note GC14 (review fix 1): a row read still out when a scan starts is dropped, since it may read the origins the
/// scan disabled; nothing is selected mid-scan; a scan that does not complete keeps the lists from before and selects
/// again the NIF the reload was holding.
#[cfg(windows)]
#[test]
fn a_row_read_held_across_a_scan_start_is_dropped_and_an_unfinished_scan_keeps_the_lists() {
    let (d, mut m) = scanned();
    let before = rows_of(&mut m);
    let rock = m.nif_at(0).0.copy_id;
    m.select_nif(rock);
    m.reload();
    let read = m.hold_rows_read();
    let lock = hold_scan_lock(d.path());
    start_scan(&mut m);
    // The held read answers mid-scan with what a store with every origin disabled reads.
    let _ = read.send(Ok((vec![], vec![])));
    m.poll();
    assert_eq!(
        rows_of(&mut m),
        before,
        "a read of the disabled store is dropped"
    );
    wait(&mut m);
    drop(lock);
    assert!(!m.scan_completed && m.error.is_some(), "{:?}", m.error);
    assert_eq!(rows_of(&mut m), before);
    assert_eq!(
        m.selected_nif,
        Some(rock),
        "the NIF the reload held is selected again"
    );
}

/// Note GC15 (review fix 2): "Show in Meshes" during a scan, or after one that did not complete, reads no users from
/// a store whose origins are disabled: the chip waits and cuts nothing, through the unfinished scan, until a scan
/// completes.
#[cfg(windows)]
#[test]
fn the_texture_chip_waits_for_a_completed_scan_before_it_cuts_the_list() {
    let other = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../nif/tests/fixtures/TestNifFile_DeepGraph_SE.nif"
    ))
    .unwrap();
    let (d, mut m) = scanned_with(Some(&other));
    let all = rows_of(&mut m);
    let pending = |m: &MeshesModel| m.tex_filter.as_ref().is_some_and(|(_, u)| u.is_none());
    // What `begin_scan` does to the store; the held lock ends the scan unfinished.
    let s = modscan::store::Store::open(&d.path().join("scan.db")).unwrap();
    s.conn()
        .execute("UPDATE origin SET enabled = 0", [])
        .unwrap();
    let lock = hold_scan_lock(d.path());
    start_scan(&mut m);
    m.show_texture("textures/white.dds".into());
    assert_eq!(rows_of(&mut m), all, "no false empty list mid-scan");
    assert!(pending(&m));
    wait(&mut m);
    drop(lock);
    assert!(!m.scan_completed);
    assert_eq!(
        rows_of(&mut m),
        all,
        "still waiting after the unfinished scan"
    );
    let mut h = harness(m);
    h.run();
    h.get_by_label("Uses textures/white.dds ✕");
    h.get_by_label_contains("after a completed scan");
    let mut m = h.into_state();
    start_scan(&mut m);
    wait(&mut m);
    assert!(m.scan_completed);
    assert!(!pending(&m));
    assert_eq!(rows_of(&mut m), [row("meshes/rock.nif", "B")]);
}

/// Note GC test 8 (rules 3.1, 3.4, 3.6): at 1100 px a long path is cut in its own column, Path | Mod | chips keep
/// their order, and nothing of the row reaches past the column's edge.
#[test]
fn at_1100_px_a_long_row_truncates_and_stays_in_its_columns() {
    use eframe::egui::{self, containers::panel::PanelState};
    let long = format!("meshes/{}rock.nif", "a_very_long_folder_name/".repeat(12));
    let name = "A mod with a long name that keeps going past its column";
    let mut m = MeshesModel::empty();
    m.mods = vec![mod_row(1, name, modscan::store::KIND_BSA, false)];
    let row = NifRow {
        le_mesh: true,
        n_copies: 3,
        problem: Some("bad block".into()),
        ..nif(1, &long)
    };
    let rows = Arc::new(vec![(row, name.to_string())]);
    m.winners = Arc::new(vec![(nif(1, "meshes/rock.nif"), "A".to_string())]);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1100.0, 800.0))
        .build_ui_state(
            |ui, m: &mut MeshesModel| sdc_gui::ui::meshes::show(ui, m),
            m,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    let rect = |h: &Harness<'static, MeshesModel>| {
        PanelState::load(&h.ctx, egui::Id::new("meshes-tree"))
            .unwrap()
            .outer_rect
    };
    // A short row first: the long one must not widen the column (a panel grows to fit what it holds).
    let panel = rect(&h);
    h.state_mut().winners = rows;
    h.run();
    assert_eq!(rect(&h), panel, "the long row widened the column");
    // The full path stays the AccessKit name (rule 3.4).
    let path = h.get_by_label(&long).rect();
    let origin = h.get_by_role_and_label(Role::Label, name).rect();
    let le = h.get_by_role_and_label(Role::Label, "LE").rect();
    assert!(
        path.max.x <= origin.min.x,
        "path {path:?} under the mod {origin:?}"
    );
    assert!(
        origin.max.x <= le.min.x,
        "mod {origin:?} under the chips {le:?}"
    );
    // Unclipped rects: a chip pushed past the edge would be cut by the scroll area and look fine to a clipped check.
    let band = path.y_range();
    let past = h.ctx.viewport(|v| {
        (v.this_pass.widgets.layers().flat_map(|(_, ws)| ws))
            .filter(|w| band.contains(w.rect.center().y) && w.rect.max.x > panel.max.x)
            .map(|w| w.rect)
            .collect::<Vec<_>>()
    });
    assert!(
        past.is_empty(),
        "past the column edge {}: {past:?}",
        panel.max.x
    );
}

/// Note GC test 7 (rules 9.1, 9.2): with 10k rows the visible list is built again only when one of its inputs changes,
/// not every frame.
#[test]
fn the_visible_list_is_rebuilt_only_when_an_input_changes() {
    let mut m = model_with(None);
    m.selected_mod = None;
    let rows = (0..10_000).map(|i| (nif(i, &format!("meshes/m/{i:05}.nif")), "A".to_string()));
    m.winners = Arc::new(rows.collect());
    let mut h = harness(m);
    h.run();
    h.get_by_label_contains("10000 of 10000 NIFs");
    let builds = |h: &Harness<'static, MeshesModel>| h.state().row_builds;
    let before = builds(&h);
    h.run_steps(3);
    assert_eq!(builds(&h), before, "idle frames build nothing");
    // Each input builds once more: the search, a chip, the texture chip, a reload's rows, the selection is not one.
    h.state_mut().search = "m/0001".into();
    h.run_steps(3);
    assert_eq!(builds(&h), before + 1, "the search");
    h.get_by_label_contains("10 of 10000 NIFs");
    h.state_mut().filter = NifFilter::Problems;
    h.run_steps(3);
    assert_eq!(builds(&h), before + 2, "a chip");
    h.state_mut().filter = NifFilter::All;
    let users = std::collections::HashSet::from(["meshes/m/00010.nif".to_string()]);
    h.state_mut().tex_filter = Some(("textures/a.dds".into(), Some(Arc::new(users))));
    h.run_steps(3);
    assert_eq!(
        builds(&h),
        before + 3,
        "the chip back and the texture chip: one frame"
    );
    h.get_by_label("1 of 10000 NIFs");
    let rows = (*h.state().winners).clone();
    h.state_mut().winners = Arc::new(rows);
    h.run_steps(3);
    assert_eq!(builds(&h), before + 4, "new rows");
    h.state_mut().selected_nif = Some(3);
    h.run_steps(3);
    assert_eq!(builds(&h), before + 4, "the selection");
}

fn row(path: &str, origin: &str) -> (String, String) {
    (path.into(), origin.into())
}

/// The mod picker showing `value` (rule 10.8: a combo's accessible text is its value).
fn picker<'h>(h: &'h Harness<'static, MeshesModel>, value: &'h str) -> egui_kittest::Node<'h> {
    h.get_by(move |n| n.role() == Role::ComboBox && n.value().is_some_and(|v| v == value))
}

/// Note GC test 1: by default the list has one row per path, the game-winning copy, with that copy's mod.
#[test]
fn the_list_has_one_row_per_path_with_the_winning_mod() {
    let (_d, mut m) = scanned();
    assert_eq!(
        rows_of(&mut m),
        [row("meshes/rock.nif", "B"), row("meshes/tree/oak.nif", "A")]
    );
    let mut h = harness(m);
    h.run();
    h.get_by_label("meshes/rock.nif");
    h.get_by_label("meshes/tree/oak.nif");
    h.get_by_label("B");
    h.get_by_label_contains("2 of 2 NIFs");
    // A row click opens the copy in the detail pane.
    h.get_by_label("meshes/rock.nif").click();
    h.run();
    let sel = h.state().selected_row().cloned().unwrap();
    assert_eq!((sel.path.as_str(), sel.wins), ("meshes/rock.nif", true));
    assert!(!h.state().shapes.is_empty());
}

/// Note GC test 2: the search matches a path fragment or a mod name; with no match the list is empty and the
/// selection and its details stay (rule 6.1).
#[test]
fn the_search_matches_a_path_or_a_mod_and_never_deselects() {
    let (_d, mut m) = scanned();
    m.search = "TREE\\".into();
    assert_eq!(rows_of(&mut m), [row("meshes/tree/oak.nif", "A")]);
    let first = m.visible_nifs()[0];
    let oak = m.nif_at(first).0.copy_id;
    m.select_nif(oak);
    m.search = " b ".into();
    assert_eq!(rows_of(&mut m), [row("meshes/rock.nif", "B")], "a mod name");
    m.search = "meshes/rock".into();
    assert_eq!(rows_of(&mut m), [row("meshes/rock.nif", "B")]);
    m.search = "nothing".into();
    assert!(rows_of(&mut m).is_empty());
    assert_eq!(m.selected_nif, Some(oak));
    assert_eq!(
        m.selected_row().map(|r| r.path.as_str()),
        Some("meshes/tree/oak.nif")
    );
    assert!(!m.shapes.is_empty());
    let mut h = harness(m);
    h.run();
    h.get_by_label_contains("0 of 2 NIFs");
    // The detail pane still shows the hidden NIF.
    h.get_by_label("tree/oak.nif");
}

/// Note GC test 3: the mod picker offers "Any mod" and the mods that hold NIFs (no PGPatcher-derived mod); picking
/// A lists both of A's copies, the losing rock.nif too.
#[test]
fn the_mod_picker_lists_mods_with_nifs_and_picking_one_lists_its_copies() {
    let (_d, mut m) = scanned();
    let pgp = mod_row(99, "PGPatcher output", 1, true);
    m.mods.push(pgp);
    let names: Vec<&str> = m.picker_mods().map(|r| r.name.as_str()).collect();
    assert_eq!(
        names,
        ["B", "A"],
        "no Overwrite, separator, Data or derived mod"
    );
    let mut h = harness(m);
    h.run();
    picker(&h, "Any mod").click();
    h.run();
    for name in ["B", "A"] {
        h.get_by_role_and_label(Role::Button, name);
    }
    for gone in ["overwrite", "Sep_separator", "PGPatcher output"] {
        assert!(h.query_by_label(gone).is_none(), "{gone} is offered");
    }
    h.get_by_role_and_label(Role::Button, "A").click();
    h.run();
    let a = h.state().mods.iter().find(|r| r.name == "A").unwrap().id;
    assert_eq!(h.state().selected_mod, Some(a));
    let rows = rows_of(h.state_mut());
    assert_eq!(
        rows,
        [row("meshes/rock.nif", "A"), row("meshes/tree/oak.nif", "A")]
    );
    let rock = h.state().nif_at(0).0.clone();
    assert!(!rock.wins, "A's rock.nif loses to B's");
    h.get_by_role_and_label(Role::Label, "Overridden"); // its status chip, not the filter chip
                                                        // "Any mod" brings the winners back.
    picker(&h, "A").click();
    h.run();
    h.get_by_role_and_label(Role::Button, "Any mod").click();
    h.run();
    assert_eq!(h.state().selected_mod, None);
    assert_eq!(
        rows_of(h.state_mut()),
        [row("meshes/rock.nif", "B"), row("meshes/tree/oak.nif", "A")]
    );
}

/// Note GC test 4: in "Any mod", Overridden lists the losing copies, each with its own mod; Wins is hidden there
/// (every row wins) and comes back with a picked mod.
#[test]
fn overridden_in_any_mod_lists_the_losing_copies_and_wins_is_hidden() {
    let (_d, mut m) = scanned();
    m.filter = NifFilter::Overridden;
    assert_eq!(rows_of(&mut m), [row("meshes/rock.nif", "A")]);
    let first = m.visible_nifs()[0];
    assert!(!m.nif_at(first).0.wins);
    let mut h = harness(m);
    h.run();
    assert!(h.query_by_role_and_label(Role::Button, "Wins").is_none());
    h.get_by_role_and_label(Role::Button, "Overridden");
    h.get_by_label("meshes/rock.nif");
    h.get_by_role_and_label(Role::Label, "A");
    let a = h.state().mods.iter().find(|r| r.name == "A").unwrap().id;
    h.state_mut().select_mod(a);
    h.run();
    h.get_by_role_and_label(Role::Button, "Wins").click();
    h.run();
    assert_eq!(rows_of(h.state_mut()), [row("meshes/tree/oak.nif", "A")]);
    // Back in "Any mod" the hidden Wins chip is not left on.
    h.state_mut().any_mod();
    assert_eq!(h.state().filter, NifFilter::All);
}

/// Note GC test 5: the texture chip cuts the list to the meshes that use the texture, with the search, the mod and the
/// chips; ✕ brings the list back. Neither deselects (rule 6.1).
#[test]
fn the_texture_chip_cuts_the_list_to_its_users_and_never_deselects() {
    let other = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../nif/tests/fixtures/TestNifFile_DeepGraph_SE.nif"
    ))
    .unwrap();
    let (_d, mut m) = scanned_with(Some(&other));
    let all = [row("meshes/rock.nif", "B"), row("meshes/tree/oak.nif", "A")];
    assert_eq!(rows_of(&mut m), all);
    let (rock, oak) = (m.nif_at(0).0.copy_id, m.nif_at(1).0.copy_id);
    let texs = |m: &MeshesModel| -> Vec<String> {
        let t = m.shapes.iter().flat_map(|s| s.textures.iter().flatten());
        t.cloned().collect()
    };
    m.select_nif(oak);
    let oak_texs = texs(&m);
    m.select_nif(rock);
    let rock_texs = texs(&m);
    let tex = rock_texs.iter().find(|t| !oak_texs.contains(t));
    let tex = tex.expect("the fixtures use different textures").clone();
    m.select_nif(oak);
    m.set_tex_filter(Some(tex.clone()));
    assert_eq!(rows_of(&mut m), [row("meshes/rock.nif", "B")]);
    assert_eq!(m.selected_nif, Some(oak), "the hidden NIF stays selected");
    assert!(!m.shapes.is_empty());
    // It combines with the search and the chips.
    m.search = "oak".into();
    assert!(rows_of(&mut m).is_empty());
    m.search.clear();
    m.filter = NifFilter::Problems;
    assert!(rows_of(&mut m).is_empty());
    m.filter = NifFilter::All;
    let mut h = harness(m);
    h.run();
    // Rule 6.2: the hidden selection is said, not dropped.
    h.get_by_label("1 of 2 NIFs (the selected NIF is hidden by the filters)");
    let chip = format!("Uses {tex} ✕");
    h.get_by_label(&chip).click();
    h.run();
    assert!(h.state().tex_filter.is_none());
    assert!(h.query_by_label(&chip).is_none());
    assert_eq!(rows_of(h.state_mut()), all);
    assert_eq!(h.state().selected_nif, Some(oak));
}

/// Note GC design 5: Problems means a problem or an LE mesh.
#[test]
fn problems_lists_le_meshes_too() {
    let le = NifRow {
        le_mesh: true,
        ..nif(1, "meshes/le.nif")
    };
    assert!(NifFilter::Problems.matches(&le));
    assert!(!NifFilter::Problems.matches(&nif(2, "meshes/ok.nif")));
}

/// Note GC rule 6.1: picking a mod hides rows and never deselects.
#[test]
fn picking_a_mod_keeps_the_selected_nif() {
    let (_d, mut m) = scanned();
    let rock = m.nif_at(0).0.copy_id; // B's winning copy
    m.select_nif(rock);
    let a = m.mods.iter().find(|r| r.name == "A").unwrap().id;
    m.select_mod(a);
    assert_eq!(m.selected_nif, Some(rock));
    assert_eq!(m.selected_row().map(|r| r.copy_id), Some(rock));
    assert!(!m.shapes.is_empty());
}

/// Note GC rule 1.7: "Reading the scan…" until the rows a reload asked for land.
#[test]
fn the_list_says_it_is_reading_until_the_rows_land() {
    let (_d, mut m) = scanned();
    m.reload();
    assert!(m.rows_loading(), "a reload starts the read");
    let rows = (m.winners.to_vec(), m.losers.to_vec());
    let tx = m.hold_rows_read(); // the read answers when the test says so
    let mut h = harness(m);
    h.run_steps(2); // a read in flight keeps repainting
    h.get_by_label("Reading the scan…");
    assert!(h.query_by_label("meshes/rock.nif").is_none());
    tx.send(Ok(rows)).unwrap();
    h.run();
    assert!(!h.state().rows_loading());
    assert!(h.query_by_label("Reading the scan…").is_none());
    h.get_by_label("meshes/rock.nif");
}

fn stats(
    n: usize,
    cached: usize,
    records: usize,
    ms: u64,
    problems: &[(&str, &str)],
) -> PluginStats {
    PluginStats {
        n,
        cached,
        records,
        ms,
        problems: problems
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect(),
        sub_ms: vec![],
    }
}

/// esp spec §4.3: one plugin line, then parse errors and missing masters by plugin.
#[test]
fn scan_details_show_the_plugin_line_and_problems() {
    let mut m = model_with(None);
    m.report = Some(Report {
        plugins: Some(stats(
            3,
            2,
            120,
            45,
            &[("M.esp", "missing master Missing.esm")],
        )),
        ..Default::default()
    });
    let mut h = harness(m);
    h.run();
    h.get_by_label("Scan details").click();
    h.run();
    h.get_by_label("Plugins: 3 (2 cached), 120 records, 45 ms");
    h.get_by_label("Plugin M.esp: missing master Missing.esm");
}

#[test]
fn scan_details_show_an_empty_plugin_pass() {
    let mut m = model_with(None);
    m.report = Some(Report {
        plugins: Some(PluginStats::default()),
        ..Default::default()
    });
    let mut h = harness(m);
    h.run();
    h.get_by_label("Scan details").click();
    h.run();
    h.get_by_label("Plugins: 0 (0 cached), 0 records, 0 ms");
}

/// esp spec §4.2: records that use the mesh, and per re-textured shape `shape → TXST EDID`. No PGPatcher warning.
#[test]
fn plugin_overrides_list_records_and_shape_swaps() {
    let mut m = model_with(None);
    m.nifs = Arc::new(vec![nif(1, "meshes/x.nif")]);
    m.selected_nif = Some(1);
    m.plugin_uses = vec![MeshUse {
        formid: 0x0001_2345,
        edid: Some("RockStat".into()),
        sig: "STAT".into(),
        plugin: "Skyrim.esm".into(),
        model_slot: 0,
        n_overrides: 2,
        swaps: vec![ShapeSwap {
            shape_name: "Shape0".into(),
            txst_formid: 0xABC,
            txst_edid: Some("DirtTXST".into()),
            index: 0,
        }],
    }];
    let mut h = harness(m);
    h.run();
    h.get_by_label("Plugin overrides (1)").click();
    h.run();
    h.get_by_label("RockStat [00012345] STAT — Skyrim.esm · overridden 2×");
    h.get_by_label("Shape0 → DirtTXST [00000ABC]");
}

/// A swap whose TXST master is missing says so instead of showing FormID 00000000.
#[test]
fn an_unresolved_swap_target_is_labelled() {
    let mut m = model_with(None);
    m.nifs = Arc::new(vec![nif(1, "meshes/x.nif")]);
    m.selected_nif = Some(1);
    m.plugin_uses = vec![MeshUse {
        formid: 1,
        edid: None,
        sig: "STAT".into(),
        plugin: "Mod.esp".into(),
        model_slot: 0,
        n_overrides: 0,
        swaps: vec![ShapeSwap {
            shape_name: "Shape0".into(),
            txst_formid: 0,
            txst_edid: None,
            index: 0,
        }],
    }];
    let mut h = harness(m);
    h.run();
    h.get_by_label("Plugin overrides (1)").click();
    h.run();
    h.get_by_label("Shape0 → unresolved TXST");
}

#[test]
fn plugin_overrides_say_when_no_record_uses_the_mesh() {
    let mut m = model_with(None);
    m.nifs = Arc::new(vec![nif(1, "meshes/x.nif")]);
    m.selected_nif = Some(1);
    let mut h = harness(m);
    h.run();
    h.get_by_label("Plugin overrides (0)").click();
    h.run();
    h.get_by_label("No plugin record of the latest scan uses this mesh.");
}

/// esp spec §4.4: counts, filter, copy; no actions.
#[test]
fn plugin_file_references_list_missing_and_unused_with_a_filter() {
    let mut m = model_with(None);
    m.file_refs = Some(FileRefs {
        missing: vec![MissingRef {
            path: "textures/missing/nothere.dds".into(),
            n_records: 6,
            records: vec!["GhostTXST [00000AB0] TXST — Skyrim.esm".into()],
        }],
        unused: vec![
            "textures/unused/lonely.dds".into(),
            "textures/other/x.dds".into(),
        ],
    });
    let mut h = harness(m);
    h.run();
    h.get_by_label("Plugin file references…").click();
    h.run();
    h.get_by_label(
        "textures/missing/nothere.dds ← GhostTXST [00000AB0] TXST — Skyrim.esm; … 5 more",
    );
    h.get_by_label("Copy");
    h.get_by_label("Unused (2)").click();
    h.run();
    h.get_by_label("textures/unused/lonely.dds");
    h.state_mut().refs_filter = "LONELY".into();
    h.run();
    h.get_by_label("textures/unused/lonely.dds");
    assert!(h.query_by_label("textures/other/x.dds").is_none());
    assert!(
        h.query_by_label_contains("Delete").is_none()
            && h.query_by_label_contains("Move").is_none()
    );
}

#[test]
fn plugin_file_references_when_nothing_is_listed() {
    let mut m = model_with(None);
    m.file_refs = Some(FileRefs::default());
    let mut h = harness(m);
    h.run();
    h.get_by_label("Plugin file references…").click();
    h.run();
    h.get_by_label("Missing (0)");
    h.get_by_label("Nothing listed.");
}

/// Ruling R6: the timing log gets one `plugins` line per finished scan.
#[test]
fn a_finished_scan_logs_the_plugin_timing() {
    use sdc_gui::model::meshes::log_plugins;
    let r = Report {
        plugins: Some(stats(3, 2, 120, 45, &[])),
        ..Default::default()
    };
    let ((), ev) = render3d::perf_log::capture(|| log_plugins(&r));
    assert_eq!(ev.len(), 1, "{ev:?}");
    assert_eq!(ev[0]["event"], "plugins");
    assert_eq!(
        (
            ev[0]["n"].as_u64(),
            ev[0]["cached"].as_u64(),
            ev[0]["records"].as_u64(),
            ev[0]["ms"].as_u64()
        ),
        (Some(3), Some(2), Some(120), Some(45))
    );
    let ((), none) = render3d::perf_log::capture(|| log_plugins(&Report::default()));
    assert!(none.is_empty());
}

/// Review I1: no read while a scan runs; a started scan drops the cached lists.
#[test]
fn file_references_are_not_read_during_a_scan_and_a_started_scan_drops_them() {
    let (_d, mut m) = scanned();
    m.load_file_refs();
    assert!(m.file_refs.is_some());
    m.request_scan(false);
    let req = m.take_request().unwrap();
    m.start(req);
    assert!(m.scanning());
    assert!(
        m.file_refs.is_none(),
        "the old lists are dropped when a scan starts"
    );
    m.load_file_refs();
    assert!(m.file_refs.is_none(), "no read while the scan runs");
    wait(&mut m);
    m.load_file_refs();
    assert!(m.file_refs.is_some());
}

/// Review M1: a failed shape read leaves no stale plugin list.
#[test]
fn a_failed_shape_read_clears_the_plugin_overrides() {
    let (d, mut m) = scanned();
    let a_id = m.mods.iter().find(|r| r.name == "A").unwrap().id;
    m.select_mod(a_id);
    let copy = m.nifs[0].copy_id;
    m.select_nif(copy);
    m.plugin_uses = vec![MeshUse {
        formid: 1,
        edid: None,
        sig: "STAT".into(),
        plugin: "P.esp".into(),
        model_slot: 0,
        n_overrides: 0,
        swaps: vec![],
    }];
    rusqlite::Connection::open(d.path().join("scan.db"))
        .unwrap()
        .execute_batch("DROP TABLE shape")
        .unwrap();
    m.set_view_source(true);
    assert!(m.error.is_some());
    assert!(m.plugin_uses.is_empty());
}

/// Review M4: after a Folder scan (plugins not read) Unused says so instead of implying plugins were checked.
#[test]
fn unused_says_plugins_were_not_read_after_a_folder_scan() {
    let note = sdc_gui::model::meshes::PLUGINS_NOT_READ_NOTE;
    let run = |plugins: Option<PluginStats>| {
        let mut m = model_with(None);
        m.file_refs = Some(FileRefs::default());
        m.report = Some(Report {
            plugins,
            ..Default::default()
        });
        m.refs_unused = true;
        let mut h = harness(m);
        h.run();
        h.get_by_label("Plugin file references…").click();
        h.run();
        h.query_by_label(note).is_some()
    };
    assert!(run(None));
    assert!(!run(Some(stats(1, 1, 1, 1, &[]))));
}

/// Trace 2026-10-22 #6: `unused_textures` leaves LOD textures out (modscan lod_skip.rs); the note must not say they
/// are listed.
#[test]
fn unused_note_says_lod_textures_are_left_out() {
    let note = sdc_gui::model::meshes::UNUSED_NOTE;
    assert!(note.contains("LOD textures are left out"), "{note}");
    assert!(!note.contains("LOD, interface"), "{note}");
}

/// Review M5: the list is cut at 200; the rest is counted.
#[test]
fn plugin_overrides_say_how_many_more_are_cut() {
    let mut m = model_with(None);
    m.nifs = Arc::new(vec![nif(1, "meshes/x.nif")]);
    m.selected_nif = Some(1);
    m.plugin_uses = (0..205)
        .map(|i| MeshUse {
            formid: i,
            edid: None,
            sig: "STAT".into(),
            plugin: "Skyrim.esm".into(),
            model_slot: 0,
            n_overrides: 0,
            swaps: vec![],
        })
        .collect();
    let mut h = harness(m);
    h.run();
    h.get_by_label("Plugin overrides (205)").click();
    h.run();
    h.get_by_label("… and 5 more");
}

/// Long plugin-reference rows wrap inside their column instead of widening the panels.
fn long_line() -> String {
    format!(
        "textures/clutter/{}.dds",
        "journalralis19to23_n ".repeat(20).trim().replace(' ', "_")
    )
}

fn x_of(h: &Harness<'static, MeshesModel>, label: &str) -> f32 {
    h.get_by_label(label).rect().min.x
}

#[test]
fn long_file_ref_rows_do_not_widen_the_layout() {
    let mut m = model_with(None);
    m.nifs = Arc::new(vec![nif(1, "meshes/x.nif")]);
    let mut h = harness(m);
    h.run();
    let mod_x =
        |h: &Harness<'static, MeshesModel>| h.get_by_role_and_label(Role::Label, "A").rect().min.x;
    let (mods_x, nifs_x) = (mod_x(&h), x_of(&h, "meshes/x.nif"));
    let long = long_line();
    h.state_mut().file_refs = Some(FileRefs {
        missing: vec![MissingRef {
            path: long.clone(),
            n_records: 3,
            records: vec!["BCS_JournalRalis19 [2F056C6B] TXST — Book Covers Skyrim.esp".repeat(4)],
        }],
        unused: vec![],
    });
    h.run();
    h.get_by_label("Plugin file references…").click();
    h.run();
    h.run();
    assert_eq!((mod_x(&h), x_of(&h, "meshes/x.nif")), (mods_x, nifs_x));
    let row = h.get_by_label_contains(&long);
    assert!(
        row.rect().max.x <= 1400.0,
        "row runs off screen: {:?}",
        row.rect()
    );
    assert!(
        row.rect().height() > 30.0,
        "row did not wrap: {:?}",
        row.rect()
    );
}

#[test]
fn file_ref_rows_are_capped_and_overrides_wrap() {
    let mut m = model_with(None);
    m.file_refs = Some(FileRefs {
        missing: vec![],
        unused: (0..1500).map(|i| format!("textures/u/{i}.dds")).collect(),
    });
    m.refs_unused = true;
    let mut h = harness(m);
    h.run();
    h.get_by_label("Plugin file references…").click();
    h.run();
    h.get_by_label("… and 500 more — narrow with the filter");
}

#[test]
fn long_override_labels_wrap_inside_the_column() {
    let mut m = model_with(None);
    m.nifs = Arc::new(vec![nif(1, "meshes/x.nif")]);
    m.selected_nif = Some(1);
    m.plugin_uses = vec![MeshUse {
        formid: 1,
        edid: Some("Rock".into()),
        sig: "STAT".into(),
        plugin: "Mod.esp".into(),
        model_slot: 0,
        n_overrides: 0,
        swaps: vec![ShapeSwap {
            shape_name: "S".repeat(600),
            txst_formid: 2,
            txst_edid: Some("T".into()),
            index: 0,
        }],
    }];
    let mut h = harness(m);
    h.run();
    h.get_by_label("Plugin overrides (1)").click();
    h.run();
    h.run();
    let r = h.get_by_label_contains("SSSSSSSSSS").rect();
    assert!(r.max.x <= 1400.0 && r.height() > 30.0, "{r:?}");
}

fn refs_model() -> MeshesModel {
    let mut m = model_with(None);
    m.nifs = Arc::new(vec![nif(1, "meshes/x.nif")]);
    m.file_refs = Some(FileRefs {
        missing: vec![MissingRef {
            path: "textures/a.dds".into(),
            n_records: 1,
            records: vec!["R [00000001] TXST — A.esp".into()],
        }],
        unused: vec![],
    });
    m
}

#[test]
fn opening_plugin_file_references_does_not_move_the_panels() {
    let mut h = harness(refs_model());
    h.run();
    let before = (
        h.get_by_role_and_label(Role::Label, "A").rect(),
        h.get_by_label("meshes/x.nif").rect(),
    );
    h.get_by_label("Plugin file references…").click();
    h.run();
    h.run();
    assert_eq!(
        (
            h.get_by_role_and_label(Role::Label, "A").rect(),
            h.get_by_label("meshes/x.nif").rect()
        ),
        before
    );
}

#[test]
fn a_long_file_ref_row_wraps_inside_the_window() {
    let mut m = refs_model();
    m.file_refs.as_mut().unwrap().missing[0].path = format!("textures/{}.dds", "q".repeat(400));
    let mut h = harness(m);
    h.run();
    h.get_by_label("Plugin file references…").click();
    h.run();
    h.run();
    let win = h.get_by_label("Plugin file references").rect();
    let row = h.get_by_label_contains("qqqqqqqqqq").rect();
    assert!(row.height() > 30.0, "row did not wrap: {row:?}");
    assert!(row.max.x <= 1400.0, "{row:?} {win:?}");
}

/// Filtering spec §3 / ruling R12: inert records are left out of "Plugin overrides" and its count, shown dimmed with
/// their reason behind "Show inert (N)".
#[test]
fn plugin_overrides_hide_inert_records_until_asked() {
    let mut m = model_with(None);
    m.nifs = Arc::new(vec![nif(1, "meshes/x.nif")]);
    m.selected_nif = Some(1);
    let used = |formid: u32, edid: &str| MeshUse {
        formid,
        edid: Some(edid.into()),
        sig: "STAT".into(),
        plugin: "Skyrim.esm".into(),
        model_slot: 0,
        n_overrides: 0,
        swaps: vec![],
    };
    m.plugin_uses = vec![used(1, "Placed"), used(2, "Lonely")];
    m.plugin_inert = [(2u32, "not placed")].into_iter().collect();
    let mut h = harness(m);
    h.run();
    h.get_by_label("Plugin overrides (1)").click();
    h.run();
    h.get_by_label("Placed [00000001] STAT — Skyrim.esm");
    assert!(h.query_by_label_contains("Lonely").is_none());
    h.get_by_label("Show inert (1)").click();
    h.run();
    h.get_by_label("Lonely [00000002] STAT — Skyrim.esm · not placed");
}

/// Ruling R12: Missing lists live paths; paths only inert records name come dimmed behind "Show inert (N)".
#[test]
fn missing_files_of_inert_records_show_on_request() {
    let mut m = model_with(None);
    m.file_refs = Some(FileRefs {
        missing: vec![MissingRef {
            path: "textures/live.dds".into(),
            n_records: 1,
            records: vec!["A [00000001] TXST — S.esm".into()],
        }],
        unused: vec![],
    });
    m.missing_inert = vec![MissingRef {
        path: "meshes/lonely.nif".into(),
        n_records: 1,
        records: vec!["Lonely [00000002] STAT — S.esm (not placed)".into()],
    }];
    let mut h = harness(m);
    h.run();
    h.get_by_label("Plugin file references…").click();
    h.run();
    h.get_by_label("Missing (1)");
    assert!(h.query_by_label_contains("lonely.nif").is_none());
    h.get_by_label("Show inert (1)").click();
    h.run();
    h.get_by_label("meshes/lonely.nif ← Lonely [00000002] STAT — S.esm (not placed)");
}

fn missing_ref(path: String) -> MissingRef {
    MissingRef {
        path,
        n_records: 1,
        records: vec!["R [00000001] STAT — S.esm".into()],
    }
}

fn budget_model(live: usize, inert: usize) -> MeshesModel {
    let mut m = model_with(None);
    m.file_refs = Some(FileRefs {
        missing: (0..live)
            .map(|i| missing_ref(format!("textures/live{i:04}.dds")))
            .collect(),
        unused: vec!["textures/unused/u.dds".into()],
    });
    m.missing_inert = (0..inert)
        .map(|i| missing_ref(format!("meshes/inert{i:04}.nif")))
        .collect();
    m.show_inert_refs = true;
    m
}

fn rows_with(h: &Harness<'static, MeshesModel>, needle: &str) -> usize {
    h.query_all_by_label_contains(needle).count()
}

/// Review I2: one 1000-row budget over live then inert rows; the remainder is reported.
#[test]
fn missing_rows_share_one_budget_and_report_the_remainder() {
    let mut h = harness(budget_model(900, 300));
    h.run();
    h.get_by_label("Plugin file references…").click();
    h.run();
    assert_eq!(rows_with(&h, "textures/live"), 900);
    assert_eq!(rows_with(&h, "meshes/inert"), 100);
    h.get_by_label("… and 200 more — narrow with the filter");
}

#[test]
fn inert_only_overflow_is_reported() {
    let mut h = harness(budget_model(0, 1200));
    h.run();
    h.get_by_label("Plugin file references…").click();
    h.run();
    assert_eq!(rows_with(&h, "meshes/inert"), 1000);
    h.get_by_label("… and 200 more — narrow with the filter");
}

/// Review M3: switching tab shows the new tab's rows at once, with no stale inert rows.
#[test]
fn switching_to_unused_drops_the_inert_rows_in_the_same_frame() {
    let mut h = harness(budget_model(1, 1));
    h.run();
    h.get_by_label("Plugin file references…").click();
    h.run();
    h.get_by_label_contains("meshes/inert0000.nif");
    h.get_by_label("Unused (1)").click();
    h.step();
    assert!(h.query_by_label_contains("meshes/inert").is_none());
    h.get_by_label("textures/unused/u.dds");
}

/// Review I1: Show inert does not follow the selection to another NIF, nor survive a rescan.
#[test]
fn show_inert_resets_on_nif_change_and_on_scan() {
    let mut m = model_with(None);
    m.nifs = Arc::new(vec![nif(1, "meshes/a.nif"), nif(2, "meshes/b.nif")]);
    m.select_nif(1);
    m.show_inert_uses = true;
    m.select_nif(2);
    assert!(!m.show_inert_uses);

    let (_d, mut m) = scanned();
    (m.show_inert_uses, m.show_inert_refs) = (true, true);
    m.request_scan(false);
    let req = m.take_request().unwrap();
    m.start(req);
    assert!(!m.show_inert_uses && !m.show_inert_refs, "scan start");
    (m.show_inert_uses, m.show_inert_refs) = (true, true);
    wait(&mut m);
    assert!(!m.show_inert_uses && !m.show_inert_refs, "scan end");
}

/// Final review M4: the inert Missing lines are built once per filter, not per frame.
#[test]
fn inert_ref_lines_are_cached_until_the_filter_changes() {
    let mut m = budget_model(0, 3);
    let a = m.refs_inert_lines();
    assert!(std::sync::Arc::ptr_eq(&a, &m.refs_inert_lines()));
    m.refs_filter = "inert0001".into();
    let c = m.refs_inert_lines();
    assert!(!std::sync::Arc::ptr_eq(&a, &c));
    assert_eq!(c.len(), 1);
}

/// Re-review (M4): a reload that replaces the inert paths shows the new lines although the filter is unchanged.
#[test]
fn reloading_file_references_replaces_the_cached_inert_lines() {
    let (_d, mut m) = scanned();
    m.missing_inert = vec![missing_ref("meshes/old.nif".into())];
    assert_eq!(m.refs_inert_lines().len(), 1);
    m.load_file_refs(); // the store has no inert paths
    assert!(m.missing_inert.is_empty());
    assert!(m.refs_inert_lines().is_empty(), "stale cached lines");
}

fn shapes_model(shapes: Vec<ShapeView>) -> MeshesModel {
    let mut m = model_with(None);
    m.nifs = Arc::new(vec![nif(7, "meshes/x.nif")]);
    m.selected_nif = Some(7);
    m.shapes = shapes;
    m.shapes_col = true;
    m
}

#[test]
fn shapes_toggle_swaps_the_column_and_is_off_with_no_selection() {
    let mut m = model_with(None);
    m.nifs = Arc::new(vec![nif(7, "meshes/x.nif")]);
    let mut h = harness(m);
    h.run();
    // No mesh selected: disabled.
    assert!(h.get_by_label("Shapes").accesskit_node().is_disabled());
    h.state_mut().selected_nif = Some(7);
    h.state_mut().shapes = vec![shape(vec![None])];
    h.run();
    assert_eq!(
        h.query_all_by_label("x.nif").count() + h.query_all_by_label("meshes/x.nif").count(),
        2,
        "heading and list row"
    );
    h.get_by_label("Shapes").click();
    h.run();
    assert!(h.state().shapes_col);
    h.get_by_label_contains("#0 Body");
    assert_eq!(h.query_all_by_label("x.nif").count(), 1, "heading only");
    // The detail pane has no Shapes | 3D selector any more.
    assert!(h.query_by_label("3D").is_none());
    h.get_by_label("Shapes").click();
    h.run();
    assert_eq!(
        h.query_all_by_label("x.nif").count() + h.query_all_by_label("meshes/x.nif").count(),
        2,
        "heading and list row"
    );
    // The selection clearing while on falls back to the list.
    h.state_mut().shapes_col = true;
    h.state_mut().selected_nif = None;
    h.run();
    assert!(!h.state().shapes_col);
}

#[test]
fn shape_cards_show_the_pgpatcher_3d_index_and_the_block_in_the_hover() {
    let a = ShapeView {
        block: 12,
        name: Some("First".into()),
        index3d: Some(1),
        ..shape(vec![None])
    };
    let b = ShapeView {
        block: 20,
        name: Some("Second".into()),
        index3d: Some(0),
        ..shape(vec![None])
    };
    let c = ShapeView {
        block: 30,
        name: Some("Orphan".into()),
        index3d: None,
        ..shape(vec![None])
    };
    let mut h = harness(shapes_model(vec![a, b, c]));
    h.run();
    h.get_by_label_contains("#1 First");
    h.get_by_label_contains("#0 Second");
    h.get_by_label_contains("#– (not in scene tree) Orphan");
    assert!(
        h.query_by_label_contains("#12").is_none(),
        "the block is not the number"
    );
    h.get_by_label_contains("#1 First").hover();
    h.run();
    h.get_by_label_contains("NIF block 12\nPGPatcher");
    h.get_by_label_contains("PGPatcher 3D index");
}

#[test]
fn shader_line_is_the_bare_type_name_and_slots_are_indented() {
    let mut h = harness(shapes_model(vec![shape(vec![Some("textures/a.dds")])]));
    h.run();
    assert!(h.query_by_label_contains("Lighting:").is_none());
    h.get_by_label("Environment Map (1)");
    let head = h.get_by_label_contains("#0 Body").rect().min.x;
    let slot = h.get_by_label_contains("slot 0").rect().min.x;
    assert!(slot > head + 4.0, "slot x {slot} vs header x {head}");
}

#[test]
fn slsf_expander_lists_set_bits_by_name() {
    let mut h = harness(shapes_model(vec![shape(vec![None])]));
    h.run();
    h.get_by_label("Details of NIF block 4").click();
    h.run();
    assert!(h.query_by_label("Specular (0)").is_none());
    h.get_by_label_contains("SLSF1 0x80000101").click();
    h.run();
    h.get_by_label("Specular (0)");
    h.get_by_label("Receive_Shadows (8)");
}

/// Meshes UI T2: a card is framed with a full-width header button ("Select shape #N name"); Shader, Textures and
/// Alpha show, a shape without alpha says so, and a header click outside edit mode selects the card.
#[test]
fn shape_cards_are_selectable_with_four_facts_and_closed_details() {
    let a = ShapeView {
        block: 12,
        name: Some("First".into()),
        index3d: Some(1),
        ..shape(vec![Some("textures/a.dds"), None, None])
    };
    let b = ShapeView {
        block: 20,
        name: Some("Second".into()),
        index3d: Some(0),
        alpha_flags: Some(0x12ED),
        alpha_threshold: Some(5),
        ..shape(vec![None])
    };
    let mut h = harness(shapes_model(vec![a, b]));
    h.run();
    let first = h.get_by_role_and_label(Role::Button, "Select shape #1 First");
    let second = h
        .get_by_role_and_label(Role::Button, "Select shape #0 Second")
        .rect();
    let first_rect = first.rect();
    assert!(first_rect.width() > 300.0, "full width: {first_rect:?}");
    assert!(first_rect.max.y < second.min.y);
    h.get_by_label("No NiAlphaProperty");
    h.get_by_label("Alpha: blend Src Alpha → Inv Src Alpha · test Greater, threshold 5");
    h.get_by_label("(2 empty slots)");
    assert!(
        h.query_by_label_contains("120 vertices").is_none(),
        "Details closed"
    );
    // The scan has no controller data: no Controllers line outside edit mode (T2 review Minor 6).
    assert!(h.query_by_label("Controllers:").is_none());
    assert_eq!(
        h.query_all_by_label_contains("Details of NIF block")
            .count(),
        2
    );
    h.get_by_role_and_label(Role::Button, "Select shape #1 First")
        .click();
    h.run();
    assert_eq!(h.state().card_sel, Some((12, Some("First".into()))));
}

#[test]
fn flag_bits_name_every_bit_and_alpha_reads_in_words() {
    use sdc_gui::model::meshes::{alpha_line, flag_bits, SLSF1_NAMES};
    assert_eq!(
        flag_bits(&SLSF1_NAMES, 0x8000_0101),
        ["Specular (0)", "Receive_Shadows (8)", "ZBuffer_Test (31)"]
    );
    // A bit past the table.
    assert_eq!(flag_bits(&["A"], 0b110), ["bit 1", "bit 2"]);
    assert_eq!(
        alpha_line(0x12ED, 5),
        "Alpha: blend Src Alpha → Inv Src Alpha · test Greater, threshold 5"
    );
    assert_eq!(
        alpha_line(0x12ED & !0x200, 5),
        "Alpha: blend Src Alpha → Inv Src Alpha"
    );
    assert_eq!(alpha_line(0x12EC, 7), "Alpha: test Greater, threshold 7");
    assert_eq!(alpha_line(0x0000, 7), "Alpha: none");
}

/// A model on a scan database whose host has no PGPatcher running, whatever the machine runs.
fn scan_model(db: PathBuf) -> MeshesModel {
    let mut m = MeshesModel::new(Some(db), MeshesSettings::default());
    m.procs = sdc_gui::model::meshes::no_procs;
    m
}

#[test]
fn a_running_pgpatcher_pauses_the_scan() {
    let d = instance("+A\n");
    put(d.path(), "mods/A/meshes/x.nif", None);
    let mut m = scan_model(d.path().join("scan.db"));
    m.procs = || vec!["pgpatcher.exe".to_string()];
    m.activate_with(None);
    m.use_instance(d.path().to_path_buf(), "Default".into());
    m.request_scan(false);
    let req = m.take_request().unwrap();
    m.start(req);
    wait(&mut m);
    assert_eq!(m.status, "Scan paused: PGPatcher is running.");
    assert_eq!(m.banner, Some(Banner::PgpRunning));
}

/// Trace-debug #40: a mod that turned PGPatcher-derived since the last scan is closed by the reload.
#[test]
fn reload_closes_a_mod_that_became_derived() {
    let (d, mut m) = scanned();
    let b = m.mods.iter().find(|r| r.name == "B").unwrap().id;
    m.select_mod(b);
    let copy = m.nifs[0].copy_id;
    m.select_nif(copy);
    put(d.path(), "mods/B/PGPatcher.esp", Some(b"e"));
    m.request_scan(false);
    let req = m.take_request().unwrap();
    m.start(req);
    wait(&mut m);
    assert_eq!(m.error, None);
    assert!(m.mods.iter().any(|r| r.id == b && r.derived));
    assert_eq!(m.selected_mod, None);
    assert!(m.nifs.is_empty());
    assert_eq!(m.selected_nif, None);
}

/// Trace-debug #41: only a scan that returned a result counts as completed (the app re-reads the Textures view's
/// mesh evidence then); a paused, cancelled or failed one leaves the store unreadable or unchanged.
#[test]
fn cancelled_or_failed_scan_does_not_mark_a_completed_scan() {
    let (_d, mut m) = scanned();
    assert!(m.scan_completed);
    m.procs = || vec!["pgpatcher.exe".to_string()];
    m.request_scan(false);
    let req = m.take_request().unwrap();
    m.start(req);
    wait(&mut m);
    assert_eq!(m.banner, Some(Banner::PgpRunning));
    assert!(!m.scan_completed);
}

/// Trace-debug 2026-10-22 #5 (Note CW2): a scan that stops before `begin_scan` (PGPatcher running) leaves the store as
/// it was, so the Used-by lists and UV coverage come back; a cancelled scan (store disabled) still keeps them off.
#[test]
fn a_scan_that_stops_before_begin_scan_keeps_the_store_settled() {
    let session = std::sync::Arc::new(
        engine::session::Session::new(encoders_sys::Bc1Model::Ideal).expect("an encoder session"),
    );
    let mut app =
        sdc_gui::model::AppModel::new(sdc_gui::settings::GuiSettings::default(), None, session);
    let (_d, mut m) = scanned();
    let run_scan = |app: &mut sdc_gui::model::AppModel, m: &mut MeshesModel| {
        m.request_scan(false);
        let req = m.take_request().unwrap();
        m.start(req);
        app.scan_started();
        wait(m);
        app.scan_finished(m.scan_completed, m.store_untouched);
    };
    m.procs = || vec!["pgpatcher.exe".to_string()];
    run_scan(&mut app, &mut m);
    assert_eq!(m.banner, Some(Banner::PgpRunning));
    assert!(!app.scan_store_unsettled, "the store is as it was");
    // Unsettled before the scan (an earlier cancel): it stays so.
    app.scan_store_unsettled = true;
    run_scan(&mut app, &mut m);
    assert!(app.scan_store_unsettled);

    // Another scan holds scan.lock (a Windows sharing lock): Busy means that scan is mid-flight with every origin
    // disabled, so the flag stays set whatever it was (not like PgpRunning).
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        m.procs = sdc_gui::model::meshes::no_procs;
        let lock = _d.path().join("scan.lock");
        let _held = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0)
            .open(lock)
            .unwrap();
        app.scan_store_unsettled = false;
        run_scan(&mut app, &mut m);
        assert!(
            m.error
                .as_deref()
                .is_some_and(|e| e.contains("already running")),
            "{:?}",
            m.error
        );
        assert!(
            !m.store_untouched && app.scan_store_unsettled,
            "Busy is not an untouched store"
        );
    }
}
#[test]
fn the_path_filter_folds_ascii_only_like_stored_paths() {
    let mut m = model_with(None);
    m.nifs = Arc::new(vec![
        nif(1, "meshes/Éclair/a.nif"),
        nif(2, "meshes/eclair/b.nif"),
    ]);
    m.search = "Éclair".into();
    assert_eq!(*m.visible_nifs(), [0]);
    m.search = "éclair".into();
    assert!(m.visible_nifs().is_empty(), "É and é are different paths");
}
