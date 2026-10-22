//! M6 Track F2 Task 8: the RNM section of the settings panel, the normal card's outline, the AO note.
mod support;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use image::{ImageBuffer, Luma, Rgb};
use sdc_gui::model::rnm::{rnm_active, AO_NOTE};
use sdc_gui::model::AppModel;
use sdc_gui::settings::GuiSettings;
use serde_json::json;
use std::path::Path;
use std::time::Duration;
use support::session;

const N: &str = "m/iron_n.png";

fn fixture(dir: &Path, files: &[&str]) -> AppModel {
    let r = dir.join("m");
    std::fs::create_dir_all(&r).unwrap();
    for f in files {
        if *f == "iron.png" {
            ImageBuffer::from_fn(64, 64, |x, y| Rgb([(x * 4) as u8, (y * 4) as u8, 90u8]))
                .save(r.join(f))
                .unwrap();
        } else {
            ImageBuffer::from_fn(64, 64, |x, _| Luma([(x * 3) as u8]))
                .save(r.join(f))
                .unwrap();
        }
    }
    let settings = GuiSettings {
        export_folder: Some(dir.join("export")),
        ..GuiSettings::default()
    };
    let mut m = AppModel::new(settings, None, session());
    m.accept = engine::policy::AcceptSuggestions::High;
    m.open_folder(dir.to_path_buf());
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
    m
}

const FULL: [&str; 4] = [
    "iron.png",
    "iron_n.png",
    "iron_bump.png",
    "iron_detailnorm.png",
];
const BENT: [&str; 4] = ["iron.png", "iron_n.png", "iron_bent.png", "iron_ao.png"];

fn panel(mut m: AppModel) -> Harness<'static, AppModel> {
    m.select_item(N);
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

#[test]
fn rnm_section_lists_layers_in_order() {
    let d = tempfile::tempdir().unwrap();
    let mut h = panel(fixture(d.path(), &FULL));
    h.run();
    assert!(!h.get_all_by_label("RNM").collect::<Vec<_>>().is_empty());
    let (bump, detail) = (h.get_by_label("bump"), h.get_by_label("detail"));
    assert!(
        bump.rect().min.y < detail.rect().min.y,
        "bump is above detail"
    );
}

#[test]
fn moving_a_layer_writes_the_order_and_a_redraw_writes_nothing() {
    let d = tempfile::tempdir().unwrap();
    let mut h = panel(fixture(d.path(), &FULL));
    h.run();
    let before = h.state().project.as_ref().unwrap().text();
    h.run();
    assert_eq!(
        h.state().project.as_ref().unwrap().text(),
        before,
        "a redraw dirtied the project"
    );
    // The first row is bump; its down arrow swaps it with detail.
    h.get_all_by_label("▼").next().unwrap().click();
    h.run();
    h.state_mut().wait_idle(Duration::from_secs(60));
    h.run();
    let v = h
        .state()
        .project
        .as_ref()
        .unwrap()
        .file_value(N, "rnm.order")
        .unwrap();
    assert_eq!(v[0], json!("detail"));
    assert_eq!(v[1], json!("bump"));
}

/// Trace #23: after a strength slider settles, it keeps the new value until the re-plan lands (no snap back).
#[test]
fn the_rnm_strength_slider_does_not_snap_back_while_the_plan_runs() {
    use eframe::egui::accesskit::Role;
    use egui_kittest::kittest::NodeT as _;
    let d = tempfile::tempdir().unwrap();
    let mut h = panel(fixture(d.path(), &FULL));
    h.run();
    let strength = |h: &Harness<'static, AppModel>| -> Vec<f64> {
        h.query_all_by_role(Role::Slider)
            .filter(|n| n.accesskit_node().max_numeric_value() == Some(2.0))
            .filter_map(|n| n.accesskit_node().numeric_value())
            .collect()
    };
    let before = strength(&h);
    assert!(!before.is_empty(), "no RNM strength slider");
    h.query_all_by_role(Role::Slider)
        .find(|n| n.accesskit_node().max_numeric_value() == Some(2.0))
        .unwrap()
        .focus();
    h.key_press(eframe::egui::Key::ArrowRight);
    h.run_steps(1);
    h.run_steps(1);
    assert!(h.state().busy(), "the re-plan should still be running");
    assert_ne!(strength(&h), before, "the slider snapped back");
}

#[test]
fn ao_note_shows_for_bent() {
    let d = tempfile::tempdir().unwrap();
    let mut h = panel(fixture(d.path(), &BENT));
    h.run();
    h.get_all_by_label(AO_NOTE).next().unwrap();
    h.get_all_by_label_contains("iron_ao.png").next().unwrap();
    h.get_by_label("use AO");
}

#[test]
fn normal_card_outline_when_active() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), &FULL);
    assert!(rnm_active(m.item(N).unwrap().job.as_ref().unwrap()));
    let key = m
        .sets
        .iter()
        .find(|s| s.key.ends_with("m/iron"))
        .unwrap()
        .key
        .clone();
    m.select_set(&key);
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    h.get_all_by_label_contains("iron_n").next().unwrap();
    // Hover text is not reliably observable in kittest: the outline's condition is `rnm_active`, asserted above.
    h.get_by_label("Normal _n").hover();
    h.run();
    h.get_by_label_contains("RNM: bump");
}
