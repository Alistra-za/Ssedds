//! Layout v2 A8: schema keys read as words (key on hover), and the shared framed group.
mod support;
use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use engine::config::{Subtype, Target};
use image::{ImageBuffer, Rgb, Rgba};
use sdc_gui::model::labels::readable;
use sdc_gui::model::sets::{set_views, SetView};
use sdc_gui::model::{Answer, AppModel};
use sdc_gui::settings::GuiSettings;
use std::path::Path;
use std::time::Duration;
use support::*;

#[test]
fn schema_keys_read_as_words() {
    assert_eq!(readable("specular_enabled"), "Specular enabled");
    assert_eq!(readable("environment_map_scale"), "Environment map scale");
    assert_eq!(readable("SkinMainRoughness"), "Skin main roughness");
    assert_eq!(
        readable("PhysicalSecondRoughnessMultiplier"),
        "Physical second roughness multiplier"
    );
    assert_eq!(readable("dynamic_cubemap"), "Dynamic cubemap");
    assert_eq!(readable("inner_uv_scale"), "Inner UV scale");
    // A1: SSS and F0 are initialisms.
    assert_eq!(readable("sssWidth"), "SSS width");
    assert_eq!(readable("UseSSS"), "Use SSS");
    assert_eq!(readable("FuzzF0"), "Fuzz F0");
    assert_eq!(readable("F0"), "F0");
}

// Copied from skin_card.rs.
fn skin_opened(dir: &Path) -> AppModel {
    let mut m = opened(dir);
    m.decide("landscape/rock01.png", Answer::Subtype(Subtype::SkinAlbedo));
    assert!(m.wait_idle(Duration::from_secs(60)));
    m
}

// Copied from cm_json.rs, reduced to the one complex `_m` set.
fn fixture(dir: &Path) -> AppModel {
    let r = dir.join("src/m");
    std::fs::create_dir_all(&r).unwrap();
    let rgb = |x: u32, y: u32| [(x * 4) as u8, (y * 4) as u8, 90u8];
    ImageBuffer::from_fn(64, 64, |x, y| Rgb(rgb(x, y)))
        .save(r.join("iron.png"))
        .unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| {
        let [a, b, c] = rgb(x, y);
        Rgba([a, b, c, (40 + x * 2) as u8])
    })
    .save(r.join("iron_m.png"))
    .unwrap();
    let settings = GuiSettings {
        export_folder: Some(dir.join("export")),
        ..GuiSettings::default()
    };
    let mut m = AppModel::new(settings, None, session());
    m.accept = engine::policy::AcceptSuggestions::High;
    m.open_folder(dir.join("src"));
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.target, Target::Cs);
    m
}

fn view(m: &AppModel, key_end: &str) -> SetView {
    set_views(m)
        .into_iter()
        .find(|v| v.key.ends_with(key_end))
        .unwrap_or_else(|| panic!("no set {key_end}"))
}

fn harness(m: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(egui::vec2(1400.0, 2400.0))
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

#[test]
fn the_skin_card_shows_labels_with_the_key_on_hover() {
    let d = tempfile::tempdir().unwrap();
    let mut m = skin_opened(d.path());
    let k = view(&m, "rock01").key;
    m.select_set(&k);
    let mut h = harness(m);
    h.run();
    h.get_by_label("Skin main roughness").hover();
    h.run();
    h.get_by_label("SkinMainRoughness");
    // Only the tooltip carries the raw key; no row label does.
    assert_eq!(h.query_all_by_label("SkinMainRoughness").count(), 1);
    h.get_by_label("SSS width");
}

#[test]
fn the_complex_material_card_shows_labels() {
    use sdc_gui::ui::cm_card::SECTION;
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path());
    let k = view(&m, "m/iron").key;
    m.select_set(&k);
    let mut h = harness(m);
    h.run();
    h.get_by_label(SECTION).click();
    h.run();
    h.get_by_label("Specular enabled");
    assert!(h.query_by_label("specular_enabled").is_none());
}

#[test]
fn a_group_shows_its_value_in_the_header_and_folds() {
    let mut h = Harness::builder()
        .with_size(egui::vec2(400.0, 300.0))
        .build_ui(|ui| {
            sdc_gui::ui::group::group(ui, "g", "Coat", Some("on".into()), false, |ui| {
                ui.label("inside")
            });
        });
    h.run();
    h.get_by_label("Coat");
    h.get_by_label("on");
    assert!(h.query_by_label("inside").is_none());
    h.get_by_label("Coat").click();
    h.run();
    h.get_by_label("inside");
    h.get_by_label("Coat").click();
    h.run();
    assert!(h.query_by_label("inside").is_none());
}
