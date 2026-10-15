//! Task 10b: the Advanced skin card (owner, 2026-10-14). Per-set, non-inheritable CS SkinProfile values kept in
//! the set's `skin` table of `sdc-project.toml`; nothing is exported yet (AG79-AG86).
mod support;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use engine::config::Subtype;
use sdc_gui::model::skin::FIELDS;
use sdc_gui::model::{Answer, AppModel};
use sdc_gui::sets_doc::SetsDoc;
use serde_json::json;
use std::time::Duration;
use support::*;

const SKIN: &str = "landscape/rock01";
const TITLE: &str = "Advanced skin (Community Shaders)";

fn skin_opened(dir: &std::path::Path) -> AppModel {
    let mut m = opened(dir);
    m.decide("landscape/rock01.png", Answer::Subtype(Subtype::SkinAlbedo));
    assert!(m.wait_idle(Duration::from_secs(60)));
    m
}

fn set_key(m: &AppModel) -> String {
    sdc_gui::model::sets::set_views(m)
        .into_iter()
        .find(|v| v.id == SKIN)
        .unwrap()
        .key
}

fn harness(m: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 1400.0))
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

#[test]
fn the_field_list_is_the_twenty_cs_keys_without_wetness() {
    let keys: Vec<&str> = FIELDS.iter().map(|f| f.key).collect();
    assert_eq!(
        keys,
        [
            "SkinMainRoughness",
            "SkinSecondRoughness",
            "SkinSpecularTexMultiplier",
            "SecondarySpecularStrength",
            "F0",
            "BaseColorMultiplier",
            "PhysicalMainRoughnessMultiplier",
            "PhysicalSecondRoughnessMultiplier",
            "PhysicalSpecularStrength",
            "ExtraEdgeRoughness",
            "EnableSkinDetail",
            "SkinDetailStrength",
            "SkinDetailTiling",
            "BodyTilingMultiplier",
            "Translucency",
            "sssWidth",
            "UseSSS",
            "FuzzStrength",
            "FuzzRoughness",
            "FuzzF0",
        ]
    );
    assert!(!keys.contains(&"WetResponse"));
}

#[test]
fn the_card_shows_for_a_skin_set_and_not_for_a_plain_one() {
    let d = tempfile::tempdir().unwrap();
    let mut m = skin_opened(d.path());
    let k = set_key(&m);
    m.select_set(&k);
    let mut h = harness(m);
    h.run();
    h.get_by_label_contains(TITLE);
    h.get_by_label_contains("export spec pending");

    let d2 = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d2.path());
    let k = set_key(&m);
    m.select_set(&k);
    let mut h = harness(m);
    h.run();
    assert!(h.query_by_label_contains(TITLE).is_none());
}

#[test]
fn an_unset_field_shows_its_default_and_stores_nothing() {
    let d = tempfile::tempdir().unwrap();
    let mut m = skin_opened(d.path());
    assert!(m.skin_values(SKIN).is_empty());
    let k = set_key(&m);
    m.select_set(&k);
    let mut h = harness(m);
    h.run();
    h.get_by_label("default 0.7");
    h.get_by_label("default 0.0278");
    assert!(h.state().skin_values(SKIN).is_empty());
}

#[test]
fn setting_resetting_and_one_undo_step_each() {
    let d = tempfile::tempdir().unwrap();
    let mut m = skin_opened(d.path());
    m.set_skin_value(SKIN, "F0", Some(json!(0.04)));
    m.set_skin_value(SKIN, "UseSSS", Some(json!(false)));
    assert_eq!(m.skin_values(SKIN)["F0"], json!(0.04));
    m.undo();
    assert!(!m.skin_values(SKIN).contains_key("UseSSS"));
    assert_eq!(m.skin_values(SKIN)["F0"], json!(0.04));
    m.set_skin_value(SKIN, "F0", None);
    assert!(m.skin_values(SKIN).is_empty(), "reset clears it");
    m.undo();
    assert_eq!(m.skin_values(SKIN)["F0"], json!(0.04));
    m.undo();
    assert!(
        m.skin_values(SKIN).is_empty(),
        "the set itself was one step"
    );
}

#[test]
fn the_skin_table_round_trips_and_unknown_keys_survive() {
    let d = tempfile::tempdir().unwrap();
    let mut m = skin_opened(d.path());
    m.set_skin_value(SKIN, "F0", Some(json!(0.04)));
    m.sets_doc
        .as_mut()
        .unwrap()
        .set_value(SKIN, "skin.Future", Some(json!("x")));
    m.set_skin_value(SKIN, "sssWidth", Some(json!(0.3)));
    let root = m.root.clone().unwrap();
    m.sets_doc.as_mut().unwrap().save(true).unwrap();
    let s = SetsDoc::open(&root);
    let e = s.entries().into_iter().find(|e| e.id == SKIN).unwrap();
    assert_eq!(e.skin["F0"], json!(0.04));
    assert_eq!(e.skin["sssWidth"], json!(0.3));
    assert_eq!(e.skin["Future"], json!("x"));
    assert!(
        e.settings.keys().all(|k| !k.starts_with("skin")),
        "skin values are not PGPatcher settings: {:?}",
        e.settings
    );
}

#[test]
fn the_set_button_stores_the_default_and_the_reset_clears_it() {
    let d = tempfile::tempdir().unwrap();
    let mut m = skin_opened(d.path());
    let k = set_key(&m);
    m.select_set(&k);
    let mut h = harness(m);
    h.run();
    h.query_all_by_label("set").next().unwrap().click();
    h.run();
    assert_eq!(h.state().skin_values(SKIN)["SkinMainRoughness"], json!(0.7));
    h.get_by_label("↺").click();
    h.run();
    assert!(h.state().skin_values(SKIN).is_empty());
}
