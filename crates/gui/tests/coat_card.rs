//! The coat settings sit under the Multilayer checkbox (DECISIONS AJ1-AJ6) and every colour key has the colour field.
mod support;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use sdc_gui::model::AppModel;
use serde_json::{json, Value};
use support::*;

const SET: &str = "landscape/rock01";

fn model(dir: &std::path::Path, edits: &[(&str, Value)]) -> AppModel {
    let mut m = pbr_opened(dir);
    let key = m
        .sets
        .iter()
        .find(|s| s.key.ends_with(SET))
        .unwrap()
        .key
        .clone();
    m.select_set(&key);
    for (k, v) in edits {
        m.set_json_value(SET, k, Some(v.clone()));
    }
    m
}

fn panel(m: AppModel) -> Harness<'static, AppModel> {
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(700.0, 1100.0))
        .build_ui_state(
            |ui, m: &mut AppModel| sdc_gui::ui::pgp_fields::panel(ui, m, SET),
            m,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    h
}

fn stored(h: &Harness<'static, AppModel>, key: &str) -> Option<Value> {
    h.state()
        .sets_doc
        .as_ref()
        .unwrap()
        .entries()
        .into_iter()
        .find(|e| e.id == SET)?
        .settings
        .into_iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v)
}

const INPUT: eframe::egui::accesskit::Role = eframe::egui::accesskit::Role::TextInput;

fn hex_box(h: &Harness<'static, AppModel>, shown: &str) -> bool {
    h.query_all_by_role(INPUT)
        .any(|n| n.value().as_deref() == Some(shown))
}

#[test]
fn the_coat_group_shows_under_multilayer_and_not_in_advanced() {
    let d = tempfile::tempdir().unwrap();
    let mut h = panel(model(d.path(), &[("multilayer", json!(true))]));
    for l in ["Coat settings", "Coat colour", "Inner UV scale"] {
        h.get_by_label(l);
    }
    h.get_by_label("Advanced TruePBR settings").click();
    h.run();
    assert_eq!(h.get_all_by_label("Coat colour").count(), 1);
    assert_eq!(h.get_all_by_label("Inner UV scale").count(), 1);
    assert_eq!(h.get_all_by_label("Coat settings").count(), 1);
}

#[test]
fn the_coat_group_is_hidden_and_values_are_kept_while_multilayer_is_off() {
    let d = tempfile::tempdir().unwrap();
    let mut h = panel(model(
        d.path(),
        &[
            ("multilayer", json!(true)),
            ("coat_color", json!([10, 20, 30])),
            ("inner_uv_scale", json!(2.0)),
        ],
    ));
    h.get_by_label("Multilayer (coat)").click();
    h.run();
    assert!(h.query_by_label("Coat settings").is_none());
    assert!(h.query_by_label("Inner UV scale").is_none());
    h.get_by_label("2 coat values kept, unused while Multilayer is off");
    assert_eq!(stored(&h, "coat_color"), Some(json!([10, 20, 30])));
    assert_eq!(stored(&h, "inner_uv_scale"), Some(json!(2.0)));
    h.get_by_label("Multilayer (coat)").click();
    h.run();
    assert!(h.query_by_label_contains("coat values kept").is_none());
    h.get_by_label("Coat colour");
}

#[test]
fn coat_colour_has_a_hex_box_and_a_commit_is_one_undo_step() {
    let d = tempfile::tempdir().unwrap();
    let mut h = panel(model(
        d.path(),
        &[
            ("coat_color", json!([10, 20, 30])),
            ("multilayer", json!(true)),
        ],
    ));
    assert!(hex_box(&h, "#0A141E"));
    h.query_all_by_role(INPUT)
        .find(|n| n.value().as_deref() == Some("#0A141E"))
        .unwrap()
        .focus();
    h.run();
    h.key_press_modifiers(eframe::egui::Modifiers::COMMAND, eframe::egui::Key::A);
    h.run();
    h.query_all_by_role(INPUT)
        .find(|n| n.value().as_deref() == Some("#0A141E"))
        .unwrap()
        .type_text("#1A2B3C");
    h.run();
    h.key_press(eframe::egui::Key::Enter);
    h.run();
    h.run();
    assert_eq!(stored(&h, "coat_color"), Some(json!([26, 43, 60])));
    h.state_mut().undo();
    assert_eq!(stored(&h, "coat_color"), Some(json!([10, 20, 30])));
}

#[test]
fn every_colour_key_gets_the_colour_field() {
    for (on, key, rgb, shown) in [
        ("emissive", "emissive_color", [10, 20, 30], "#0A141E"),
        ("subsurface", "subsurface_color", [1, 2, 3], "#010203"),
        ("multilayer", "coat_color", [4, 5, 6], "#040506"),
        ("fuzz_on", "fuzz.color", [7, 8, 9], "#070809"),
    ] {
        let d = tempfile::tempdir().unwrap();
        let h = panel(model(d.path(), &[(on, json!(true)), (key, json!(rgb))]));
        assert!(hex_box(&h, shown), "{key} has a hex box");
    }
}

#[test]
fn a_cnr_member_shows_multilayer_auto_and_unticking_stores_an_explicit_off() {
    let d = tempfile::tempdir().unwrap();
    mod_folder(d.path());
    image::ImageBuffer::from_fn(64, 64, |_, _| image::Rgb([128u8, 128, 255]))
        .save(d.path().join("textures/landscape/rock01_cnr.png"))
        .unwrap();
    let mut h = panel(model(d.path(), &[]));
    h.get_by_label("Coat settings");
    assert!(h.query_all_by_label("(auto)").count() >= 2);
    assert_eq!(stored(&h, "multilayer"), None, "nothing is stored");
    h.get_by_label("Multilayer (coat)").click();
    h.run();
    assert_eq!(stored(&h, "multilayer"), Some(json!(false)));
    assert!(h.query_by_label("Coat settings").is_none());
}
