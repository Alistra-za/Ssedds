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
use sdc_gui::model::viewport3d::Content;
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
    h.get_by_label(PANEL_TITLE);
    h.get_by_label(NO_SCAN);
    h.get_by_label(JSON_WINS);
    h.get_by_label(&cap_text(2048));
    h.get_by_label_contains("Diffuse: fresh");
    h.get_by_label("Built-in shape: patched as PGPatcher would");
    h.get_by_label_contains("Off in this preview");
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
    h.get_by_label("Cube").click();
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
            status: ShapeStatus::Failed("X3000 bad register".into()),
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
        .query_by_label("Emissive colour is approximate (in game the engine supplies it)")
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
    h.get_by_label("Emissive colour is approximate (in game the engine supplies it)");
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
        h.get_by_label(name).click();
        settle(&mut h);
        assert!(
            matches!(h.state().1.v3.content(), Some(Content::Builtin { shape, .. }) if *shape == b),
            "{name}"
        );
    }
    assert!(BUILTINS.iter().any(|(_, n)| *n == "Material ball"));
}
