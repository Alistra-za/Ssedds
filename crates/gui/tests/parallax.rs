//! DECISIONS AG18 (owner, 2026-10-14): "Enable parallax" on the complex `_m`. Off writes alpha 0 (complex material on,
//! parallax off); a source without alpha cannot turn it on.
mod support;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use engine::config::{Subtype, Target};
use image::{ImageBuffer, Rgb, Rgba};
use sdc_gui::model::fields::{field_hidden, field_view};
use sdc_gui::model::AppModel;
use sdc_gui::settings::GuiSettings;
use serde_json::json;
use std::path::Path;
use std::time::Duration;
use support::session;

const IRON_M: &str = "m/iron_m.png";
const BOLT_M: &str = "m/bolt_m.png";

/// `iron`: a diffuse and an `_m` with a varying (height) alpha. `bolt`: a diffuse and an RGB-only `_m`.
fn fixture(dir: &Path) -> AppModel {
    let r = dir.join("m");
    std::fs::create_dir_all(&r).unwrap();
    let rgb = |x: u32, y: u32| [(x * 4) as u8, (y * 4) as u8, 90u8];
    for base in ["iron", "bolt"] {
        ImageBuffer::from_fn(64, 64, |x, y| Rgb(rgb(x, y)))
            .save(r.join(format!("{base}.png")))
            .unwrap();
    }
    ImageBuffer::from_fn(64, 64, |x, y| {
        let [a, b, c] = rgb(x, y);
        Rgba([a, b, c, (40 + x * 2) as u8])
    })
    .save(r.join("iron_m.png"))
    .unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Rgb(rgb(x, y)))
        .save(r.join("bolt_m.png"))
        .unwrap();
    let settings = GuiSettings {
        export_folder: Some(dir.join("export")),
        ..GuiSettings::default()
    };
    let mut m = AppModel::new(settings, None, session());
    m.accept = engine::policy::AcceptSuggestions::High;
    m.open_folder(dir.to_path_buf());
    idle(&mut m);
    m
}

fn idle(m: &mut AppModel) {
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
}

fn parallax(m: &AppModel, key: &str) -> Option<bool> {
    m.item(key)
        .unwrap()
        .job
        .as_ref()
        .unwrap()
        .profile
        .alpha
        .parallax
}

fn editor(mut m: AppModel, set: &str) -> Harness<'static, AppModel> {
    let key = m
        .sets
        .iter()
        .find(|s| s.key.ends_with(set))
        .expect("the set")
        .key
        .clone();
    m.select_set(&key);
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

fn settle(h: &mut Harness<'static, AppModel>) {
    h.run();
    h.state_mut().wait_idle(Duration::from_secs(60));
    h.run();
}

#[test]
fn the_cpm_card_checkbox_turns_parallax_off_in_one_undo_step() {
    let d = tempfile::tempdir().unwrap();
    let m = fixture(d.path());
    assert_eq!(
        m.item(IRON_M)
            .unwrap()
            .job
            .as_ref()
            .unwrap()
            .profile
            .subtype,
        Subtype::ComplexMaterial
    );
    assert_eq!(parallax(&m, IRON_M), Some(true), "varying alpha: on");
    let mut h = editor(m, "m/iron");
    h.run();
    h.get_by_label("Enable parallax").click();
    settle(&mut h);
    let m = h.state_mut();
    assert_eq!(
        m.project
            .as_ref()
            .unwrap()
            .file_value(IRON_M, "alpha.parallax"),
        Some(json!(false))
    );
    assert_eq!(parallax(m, IRON_M), Some(false));
    m.undo();
    idle(m);
    assert_eq!(
        m.project
            .as_ref()
            .unwrap()
            .file_value(IRON_M, "alpha.parallax"),
        None,
        "one undo step takes the toggle back"
    );
    assert_eq!(parallax(m, IRON_M), Some(true));
}

#[test]
fn the_settings_row_shows_only_for_complex_material_and_locks_without_source_alpha() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path());
    let job = m.item(IRON_M).unwrap().job.clone().unwrap();
    let v = field_view(m.target, &job, "alpha.parallax").expect("a parallax row");
    assert_eq!(
        (v.label, v.value.clone(), v.lock),
        ("Enable parallax", json!(true), None)
    );
    assert!(!field_hidden(&job.profile, "alpha.parallax"));
    let diffuse = m.item("m/iron.png").unwrap().job.clone().unwrap();
    assert!(field_hidden(&diffuse.profile, "alpha.parallax"));

    m.set_field(BOLT_M, "subtype", json!(Subtype::ComplexMaterial));
    idle(&mut m);
    let job = m.item(BOLT_M).unwrap().job.clone().unwrap();
    assert_eq!(job.profile.alpha.parallax, Some(false), "no alpha: off");
    let v = field_view(Target::Cs, &job, "alpha.parallax").unwrap();
    assert!(
        v.lock.is_some_and(|w| w.contains("no alpha")),
        "{:?}",
        v.lock
    );
}
