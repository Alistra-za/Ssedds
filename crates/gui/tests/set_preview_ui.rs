//! The set preview panel (P1-A Task 15): its states without a GPU (no render thread starts, the shaders are never
//! confirmed): no scan DB, plain textures (D3), no matching mesh → built-in, the Skip badge, the JSON line.
mod support;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use nif::material::MaterialState;
use render3d::builtin::Builtin;
use render3d::{ShapeId, ShapeInfo, ShapeStatus};
use sdc_gui::model::pbr_export::json_entries;
use sdc_gui::model::set_preview::*;
use sdc_gui::model::viewport3d::{Content, FRAME_LABEL};
use sdc_gui::model::AppModel;
use std::sync::Arc;
use std::time::{Duration, Instant};
use support::*;

type H = Harness<'static, (AppModel, SetPreview)>;

fn harness(m: AppModel, s: SetPreview) -> H {
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(520.0, 1000.0))
        .build_ui_state(
            |ui, (m, s): &mut (AppModel, SetPreview)| sdc_gui::ui::set_preview::draw(ui, m, s),
            (m, s),
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    // The tech lines these tests read are in the Details footer, collapsed by default (AG132).
    sdc_gui::ui::viewport3d::set_details_open(&h.ctx, sdc_gui::ui::viewport3d::SET_DETAILS, true);
    h
}

/// Draw and poll until nothing is in flight (what the app does every frame).
fn settle(h: &mut H) {
    let t = Instant::now();
    loop {
        h.run_steps(4);
        {
            let (m, s) = h.state_mut();
            s.poll(&*m);
        }
        if !h.state().1.busy() {
            h.run_steps(4);
            return;
        }
        assert!(
            t.elapsed() < Duration::from_secs(90),
            "the panel did not settle"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn rock_selected(d: &std::path::Path) -> AppModel {
    let mut m = pbr_opened(d);
    let key = json_entries(&m)
        .into_iter()
        .find(|(v, _)| v.id == "landscape/rock01")
        .unwrap()
        .0
        .key;
    m.select_set(&key);
    // The 3D panel shows in the 3D view of the set (the 2D | 3D switch).
    m.settings.set_view_3d = true;
    m
}

#[test]
fn without_a_scan_db_the_panel_says_so_and_shows_the_simulated_sphere() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(
        rock_selected(d.path()),
        SetPreview::new(None, None, None, DEFAULT_CAP),
    );
    settle(&mut h);
    h.get_by_label(FRAME_LABEL);
    h.get_by_label(NO_SCAN);
    h.get_by_label(JSON_WINS);
    h.get_by_label(&cap_text(2048));
    h.get_by_label_contains("Diffuse: fresh");
    h.get_by_label("Built-in shape: patched as PGPatcher would");
    h.get_by_label_contains("Off in this preview");
    // One mesh combo, always shown (no scan DB here): Auto, then the built-ins after a separator.
    h.get_by_role(eframe::egui::accesskit::Role::ComboBox)
        .click();
    h.run_steps(4);
    h.get_by_label("Auto");
    for (_, name) in BUILTINS {
        h.get_by_label(name);
    }
    assert!(h.query_by_label(NEEDS_CS).is_none());
}

#[test]
fn a_set_the_simulation_cannot_preview_shows_plain_textures_and_the_notice() {
    let d = tempfile::tempdir().unwrap();
    let mut m = rock_selected(d.path());
    m.set_target(engine::config::Target::Vanilla);
    assert!(m.wait_idle(Duration::from_secs(60)));
    let mut h = harness(m, SetPreview::new(None, None, None, DEFAULT_CAP));
    settle(&mut h);
    h.get_by_label(NEEDS_CS);
    assert!(h.query_by_label(JSON_WINS).is_none());
    h.get_by_label("Built-in shape: plain textures, no PGPatcher simulation");
    let content = h.state().1.v3.content().cloned();
    assert!(
        matches!(&content, Some(Content::Builtin { material, .. }) if !material.is_pbr()),
        "{content:?}"
    );
}

#[test]
fn no_matching_mesh_falls_back_to_a_built_in_shape_the_user_can_change() {
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("scan.db");
    drop(modscan::store::Store::open(&db).unwrap()); // an empty v3 database
    let mut h = harness(
        rock_selected(d.path()),
        SetPreview::new(None, None, Some(db), DEFAULT_CAP),
    );
    settle(&mut h);
    h.get_by_label(NO_MATCH);
    assert!(matches!(
        h.state().1.v3.content(),
        Some(Content::Builtin {
            shape: Builtin::Sphere,
            ..
        })
    ));
    pick_mesh(&mut h, "Cube");
    settle(&mut h);
    assert!(matches!(
        h.state().1.v3.content(),
        Some(Content::Builtin {
            shape: Builtin::Cube,
            ..
        })
    ));
}

#[test]
fn a_skipped_shape_gets_the_red_line_and_patched_shapes_list_their_badges() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(
        rock_selected(d.path()),
        SetPreview::new(None, None, None, DEFAULT_CAP),
    );
    settle(&mut h);
    h.state_mut().1.rows = vec![
        ShapeRow {
            id: ShapeId(4),
            name: "Bottle".into(),
            outcome: Outcome::Skip(r"textures\pbr\xmead\reserve_rmaos.dds missing".into()),
            material: MaterialState::builtin_default(),
        },
        ShapeRow {
            id: ShapeId(5),
            name: "Label".into(),
            outcome: Outcome::Patched {
                badges: vec!["smooth_angle"],
            },
            material: MaterialState::builtin_default(),
        },
    ];
    h.run_steps(4);
    h.get_by_label(r"Bottle: will be skipped: textures\pbr\xmead\reserve_rmaos.dds missing");
    h.get_by_label("Label: patched as PGPatcher would");
    h.get_by_label(&badge_text("smooth_angle"));
}

#[test]
fn nothing_but_a_hint_without_a_followed_set() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.selection = sdc_gui::model::Selection::None;
    let mut h = harness(m, SetPreview::new(None, None, None, DEFAULT_CAP));
    h.run_steps(4);
    h.get_by_label("Select a set to preview it in 3D.");
    assert!(!sdc_gui::ui::set_preview::wanted(&h.state().0));
}

/// Open the mesh ComboBox and pick `name` from its list.
fn pick_mesh(h: &mut H, name: &str) {
    h.get_by_role(eframe::egui::accesskit::Role::ComboBox)
        .click();
    h.run_steps(4);
    h.get_all_by_label(name).last().unwrap().click();
    h.run_steps(4);
}

fn plain_rock() -> (tempfile::TempDir, H) {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(
        rock_selected(d.path()),
        SetPreview::new(None, None, None, DEFAULT_CAP),
    );
    settle(&mut h);
    (d, h)
}

#[test]
fn a_shader_that_failed_to_compile_is_listed_per_shape() {
    let (_d, mut h) = plain_rock();
    h.state_mut().1.v3.shapes = vec![
        ShapeInfo {
            id: ShapeId(1),
            name: "Bottle".into(),
            material: MaterialState::builtin_default(),
            status: ShapeStatus::Failed {
                error: "X3000 bad register".into(),
                showing_previous: false,
            },
        },
        ShapeInfo {
            id: ShapeId(2),
            name: "Fine".into(),
            material: MaterialState::builtin_default(),
            status: ShapeStatus::Drawing,
        },
    ];
    h.run_steps(4);
    h.get_by_label("Bottle: shader failed: X3000 bad register");
    assert!(h.query_by_label_contains("Fine: shader failed").is_none());
}

#[test]
fn the_emissive_approximation_label_shows_only_with_an_emissive_material() {
    let (_d, mut h) = plain_rock();
    h.run_steps(4);
    assert!(h
        .query_by_label(
            "External emittance: in game the cell or weather can change this glow colour"
        )
        .is_none());
    let mut material = MaterialState::builtin_default();
    material.slsf1 |= 1 << 29;
    h.state_mut().1.rows = vec![ShapeRow {
        id: ShapeId(4),
        name: "Lamp".into(),
        outcome: Outcome::Plain,
        material,
    }];
    h.run_steps(4);
    h.get_by_label("External emittance: in game the cell or weather can change this glow colour");
}

#[test]
fn the_mode_toggle_switches_encoded_and_source() {
    let (_d, mut h) = plain_rock();
    assert_eq!(h.state().1.mode(), PreviewMode::Encoded);
    h.get_by_label(MODE_LABEL_ENCODED);
    h.get_by_label(MODE_LABEL_SOURCE).click();
    h.run_steps(4);
    assert_eq!(h.state().1.mode(), PreviewMode::Source);
    h.get_by_label(MODE_LABEL_ENCODED).click();
    h.run_steps(4);
    assert_eq!(h.state().1.mode(), PreviewMode::Encoded);
}

#[test]
fn a_member_chip_shows_its_last_encode_time_even_while_stale() {
    let (_d, mut h) = plain_rock();
    h.get_by_label_contains("Diffuse: fresh (");
    {
        let mem = h
            .state_mut()
            .1
            .enc
            .members
            .values_mut()
            .next()
            .expect("a member");
        let e = Arc::new(Encoded {
            id: 1,
            dds: Arc::new(vec![]),
            ms: 42.0,
        });
        mem.state = EncState::Stale(Some(e));
    }
    h.run_steps(4);
    h.get_by_label_contains("stale · last 42 ms");
}

#[test]
fn every_built_in_shape_is_selectable() {
    let (_d, mut h) = plain_rock();
    for (b, name) in BUILTINS {
        pick_mesh(&mut h, name);
        settle(&mut h);
        assert!(
            matches!(h.state().1.v3.content(), Some(Content::Builtin { shape, .. }) if *shape == b),
            "{name}"
        );
    }
    assert!(BUILTINS.iter().any(|(_, n)| *n == "Material ball"));
}

// ---- Task 16: hooks ----

#[test]
fn the_cap_setting_defaults_to_2048_and_round_trips() {
    let d = tempfile::tempdir().unwrap();
    let mut s = sdc_gui::settings::GuiSettings::default();
    assert_eq!(s.render3d.set_preview_max, 0);
    assert_eq!(SetPreview::from_settings(&s).enc.cap, DEFAULT_CAP);
    s.render3d.set_preview_max = 1024;
    assert_eq!(SetPreview::from_settings(&s).enc.cap, 1024);
    assert!(SetPreview::from_settings(&s).enabled);
    let p = d.path().join("gui.toml");
    s.save(&p).unwrap();
    assert_eq!(sdc_gui::settings::GuiSettings::load(&p), (s, None));
}

#[test]
fn the_textures_view_shows_the_panel_for_a_set_only_when_the_app_enables_it() {
    use sdc_gui::ui::State;
    let d = tempfile::tempdir().unwrap();
    let m = rock_selected(d.path());
    let state = State {
        set3d: SetPreview::new(None, None, None, DEFAULT_CAP),
        ..State::default()
    };
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1800.0, 900.0))
        .build_ui_state(
            |ui, (m, s): &mut (AppModel, State)| sdc_gui::ui::draw_with(ui, m, s),
            (m, state),
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    // No poll in this harness: the 3D view may still be preparing.
    let shown = |h: &Harness<'static, (AppModel, sdc_gui::ui::State)>| {
        h.query_by_label(FRAME_LABEL).is_some()
            || h.query_by_label(sdc_gui::ui::set_preview::PREPARING)
                .is_some()
    };
    assert!(shown(&h));
    h.state_mut().0.selection = sdc_gui::model::Selection::None;
    h.run();
    assert!(!shown(&h));
    // `ui::draw` (every existing kittest) uses `State::default()`: the panel stays off.
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 900.0))
        .build_ui_state(
            |ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m),
            rock_selected(d.path()),
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    assert!(h.query_by_label(FRAME_LABEL).is_none());
    assert!(h
        .query_by_label(sdc_gui::ui::set_preview::PREPARING)
        .is_none());
}

const EMISSIVE_TEXT: &str =
    "External emittance: in game the cell or weather can change this glow colour";

#[test]
fn the_emissive_label_ignores_a_hidden_material_and_follows_the_viewport_shapes() {
    let (_d, mut h) = plain_rock();
    let mut material = MaterialState::builtin_default();
    material.slsf1 |= 1 << 29;
    let mut hidden = material.clone();
    hidden.hidden = true;
    h.state_mut().1.rows = vec![ShapeRow {
        id: ShapeId(4),
        name: "Lamp".into(),
        outcome: Outcome::Plain,
        material: hidden,
    }];
    h.run_steps(4);
    assert!(h.query_by_label(EMISSIVE_TEXT).is_none());
    h.state_mut().1.rows.clear();
    h.state_mut().1.v3.shapes = vec![ShapeInfo {
        id: ShapeId(1),
        name: "Lamp".into(),
        material,
        status: ShapeStatus::Drawing,
    }];
    h.run_steps(4);
    h.get_by_label(EMISSIVE_TEXT);
}

#[test]
fn hiding_the_panel_during_work_lets_the_app_go_idle_and_the_scene_lands_on_re_show() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(
        rock_selected(d.path()),
        SetPreview::new(None, None, None, DEFAULT_CAP),
    );
    h.run_steps(2);
    {
        let (m, s) = h.state_mut();
        s.poll(&*m);
    }
    assert!(h.state().1.busy(), "work is in flight");
    // Hidden: no draw. The worker finishes; nothing may keep the app repainting.
    let t = Instant::now();
    while h.state().1.busy() {
        let (m, s) = h.state_mut();
        s.poll(&*m);
        assert!(
            t.elapsed() < Duration::from_secs(30),
            "busy() stuck while hidden"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(h.state().1.v3.content().is_none());
    settle(&mut h);
    assert!(h.state().1.v3.content().is_some());
}

#[test]
fn an_old_gui_toml_without_the_cap_gives_the_default_and_out_of_range_values_clamp() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("gui.toml");
    std::fs::write(
        &p,
        "[render3d]
",
    )
    .unwrap();
    let (s, _) = sdc_gui::settings::GuiSettings::load(&p);
    assert_eq!(s.render3d.set_preview_max, 0);
    assert_eq!(SetPreview::from_settings(&s).enc.cap, DEFAULT_CAP);
    let cap = |n: u32| {
        let mut s = sdc_gui::settings::GuiSettings::default();
        s.render3d.set_preview_max = n;
        SetPreview::from_settings(&s).enc.cap
    };
    assert_eq!((cap(1), cap(255), cap(256)), (256, 256, 256));
    assert_eq!((cap(8192), cap(8193), cap(u32::MAX)), (8192, 8192, 8192));
}

#[test]
fn a_shader_still_compiling_is_listed_and_a_kept_look_is_said() {
    let (_d, mut h) = plain_rock();
    h.state_mut().1.v3.shapes = vec![
        ShapeInfo {
            id: ShapeId(1),
            name: "Bottle".into(),
            material: MaterialState::builtin_default(),
            status: ShapeStatus::Pending {
                showing_previous: true,
            },
        },
        ShapeInfo {
            id: ShapeId(2),
            name: "Cork".into(),
            material: MaterialState::builtin_default(),
            status: ShapeStatus::Failed {
                error: "X3000 bad register".into(),
                showing_previous: true,
            },
        },
    ];
    h.run_steps(4);
    h.get_by_label("Bottle: compiling…");
    h.get_by_label("Cork: shader failed: X3000 bad register (previous look kept)");
}

#[test]
fn the_mode_toggle_still_calls_set_mode_and_puts_ab_in_use() {
    let (_d, mut h) = plain_rock();
    assert!(!h.state().1.enc.ab_active());
    h.get_by_label(MODE_LABEL_SOURCE).click();
    h.run_steps(4);
    assert_eq!(h.state().1.mode(), PreviewMode::Source);
    assert_eq!(h.state().1.enc.mode(), PreviewMode::Source);
    assert!(
        h.state().1.enc.ab_active(),
        "the first flip puts A/B in use"
    );
    h.get_by_label(MODE_LABEL_ENCODED).click();
    h.run_steps(4);
    assert_eq!(h.state().1.mode(), PreviewMode::Encoded);
}
#[test]
fn the_timings_section_shows_the_log_path_and_a_copy_button() {
    let log_dir = tempfile::tempdir().unwrap();
    let _log = render3d::perf_log::start_in(log_dir.path()).expect("a log");
    let (_d, mut h) = plain_rock();
    // Details scrolls when it is taller than its cap.
    h.get_by_label("Timings").scroll_to_me();
    h.run();
    h.get_by_label("Timings").click();
    h.run();
    h.get_by_label_contains("Timing log:");
    h.get_by_label("Copy log path");
}
