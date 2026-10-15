//! AG2: the shared colour field (swatch + hex) and its two sites, the TruePBR colours and the 3D background grey.
mod support;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use sdc_gui::model::viewport3d::{V3State, Viewport3d};
use sdc_gui::model::AppModel;
use sdc_gui::ui::color::{format_hex, parse_hex, u8_to_unit, unit_to_u8};
use serde_json::{json, Value};
use support::*;

#[test]
fn hex_parses_with_or_without_hash_and_trims() {
    assert_eq!(parse_hex("#1A2B3C"), Some([0x1A, 0x2B, 0x3C]));
    assert_eq!(parse_hex("1a2b3c"), Some([0x1A, 0x2B, 0x3C]));
    assert_eq!(parse_hex("  #1a2b3c "), Some([0x1A, 0x2B, 0x3C]));
    assert_eq!(parse_hex("#1a2b3"), None);
    assert_eq!(parse_hex("#1a2b3c4"), None);
    assert_eq!(parse_hex("#GG0000"), None);
    assert_eq!(parse_hex(""), None);
}

#[test]
fn round_trips() {
    for v in [[0, 0, 0], [255, 255, 255], [1, 128, 254]] {
        assert_eq!(parse_hex(&format_hex(v)), Some(v));
    }
    assert_eq!(format_hex([1, 171, 255]), "#01ABFF");
    for b in 0..=255u8 {
        let f = u8_to_unit(b);
        assert_eq!(unit_to_u8(f), b);
        assert!((f - f32::from(b) / 255.0).abs() <= 1.0 / 255.0);
    }
}

fn rock_with_emissive(dir: &std::path::Path) -> AppModel {
    let mut m = pbr_opened(dir);
    let key = m
        .sets
        .iter()
        .find(|s| s.key.ends_with("landscape/rock01"))
        .unwrap()
        .key
        .clone();
    m.select_set(&key);
    m.set_json_value(
        "landscape/rock01",
        "emissive_color",
        Some(json!([10, 20, 30])),
    );
    m.set_json_value("landscape/rock01", "emissive", Some(json!(true)));
    m
}

fn panel(m: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(700.0, 900.0))
        .build_ui_state(
            |ui, m: &mut AppModel| sdc_gui::ui::pgp_fields::panel(ui, m, "landscape/rock01"),
            m,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

fn colour(h: &Harness<'static, AppModel>) -> Option<Value> {
    h.state()
        .sets_doc
        .as_ref()
        .unwrap()
        .entries()
        .into_iter()
        .find(|e| e.id == "landscape/rock01")?
        .settings
        .into_iter()
        .find(|(k, _)| k == "emissive_color")
        .map(|(_, v)| v)
}

fn type_hex<S: 'static>(h: &mut Harness<'static, S>, shown: &str, text: &str) {
    type_hex_then(h, shown, text, eframe::egui::Key::Enter);
}

fn hex_box_shows<S: 'static>(h: &Harness<'static, S>, shown: &str) -> bool {
    h.query_all_by_role(eframe::egui::accesskit::Role::TextInput)
        .any(|n| n.value().as_deref() == Some(shown))
}

fn type_hex_then<S: 'static>(
    h: &mut Harness<'static, S>,
    shown: &str,
    text: &str,
    end: eframe::egui::Key,
) {
    let role = eframe::egui::accesskit::Role::TextInput;
    h.query_all_by_role(role)
        .find(|n| n.value().as_deref() == Some(shown))
        .unwrap_or_else(|| panic!("no hex box shows {shown}"))
        .focus();
    h.run();
    h.key_press_modifiers(eframe::egui::Modifiers::COMMAND, eframe::egui::Key::A);
    h.run();
    h.query_all_by_role(role)
        .find(|n| n.value().as_deref() == Some(shown))
        .unwrap()
        .type_text(text);
    h.run();
    h.key_press(end);
    h.run();
    h.run();
}

#[test]
fn hex_commit_is_one_undo_step_and_reset_works() {
    let d = tempfile::tempdir().unwrap();
    let mut h = panel(rock_with_emissive(d.path()));
    h.run();
    type_hex(&mut h, "#0A141E", "#1A2B3C");
    // The same document value the number boxes write, one step.
    // AG133: a hex commit stores 0-1 floats.
    assert_eq!(
        colour(&h),
        Some(json!([26.0 / 255.0, 43.0 / 255.0, 60.0 / 255.0]))
    );
    h.state_mut().undo();
    assert_eq!(colour(&h), Some(json!([10, 20, 30])), "one step");
    h.run();
    // The reset button on the emissive_color row: the one level with its hex box.
    let hex_y = h
        .query_all_by_role(eframe::egui::accesskit::Role::TextInput)
        .find(|n| n.value().as_deref() == Some("#0A141E"))
        .unwrap()
        .rect()
        .center()
        .y;
    h.get_all_by_label("reset")
        .min_by(|a, b| {
            let d = |n: &egui_kittest::Node<'_>| (n.rect().center().y - hex_y).abs();
            d(a).total_cmp(&d(b))
        })
        .unwrap()
        .click();
    h.run();
    assert_eq!(colour(&h), None);
}

#[test]
fn invalid_hex_commits_nothing_and_reverts() {
    let d = tempfile::tempdir().unwrap();
    let mut h = panel(rock_with_emissive(d.path()));
    h.run();
    type_hex(&mut h, "#0A141E", "#1A2B3");
    assert_eq!(colour(&h), Some(json!([10, 20, 30])));
    assert!(hex_box_shows(&h, "#0A141E"), "the box text reverts");
    // No step was added: the one undo reverts the "emissive" toggle, which was the last real step.
    h.state_mut().undo();
    assert_eq!(colour(&h), Some(json!([10, 20, 30])));
    assert!(!h
        .state()
        .sets_doc
        .as_ref()
        .unwrap()
        .entries()
        .iter()
        .any(|e| e.id == "landscape/rock01" && e.settings.iter().any(|(k, _)| k == "emissive")));
}

#[test]
fn background_grey_renders_a_hex_box() {
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1000.0, 700.0))
        .build_ui_state(
            |ui, v: &mut Viewport3d| sdc_gui::ui::viewport3d::show(ui, v),
            Viewport3d::with_state(V3State::NotFound),
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    h.get_by_label(sdc_gui::ui::viewport3d::VIEW_MENU).click();
    h.run();
    h.query_all_by_role(eframe::egui::accesskit::Role::TextInput)
        .find(|n| n.value().as_deref() == Some("#000000"))
        .expect("grey hex box");
    type_hex(&mut h, "#000000", "#808080");
    assert!((h.state().look.grey - 128.0 / 255.0).abs() < 1e-6);
}

#[test]
fn esc_cancels_a_hex_edit() {
    let d = tempfile::tempdir().unwrap();
    let mut h = panel(rock_with_emissive(d.path()));
    h.run();
    type_hex_then(&mut h, "#0A141E", "#1A2B3C", eframe::egui::Key::Escape);
    assert_eq!(colour(&h), Some(json!([10, 20, 30])), "nothing committed");
    assert!(hex_box_shows(&h, "#0A141E"), "the box text reverts");
}

#[test]
fn a_stale_edit_buffer_is_dropped_when_the_row_goes_away() {
    let d = tempfile::tempdir().unwrap();
    let shown = std::rc::Rc::new(std::cell::Cell::new(true));
    let flag = shown.clone();
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(700.0, 900.0))
        .build_ui_state(
            move |ui, m: &mut AppModel| {
                if flag.get() {
                    sdc_gui::ui::pgp_fields::panel(ui, m, "landscape/rock01");
                }
            },
            rock_with_emissive(d.path()),
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    let role = eframe::egui::accesskit::Role::TextInput;
    h.query_all_by_role(role)
        .find(|n| n.value().as_deref() == Some("#0A141E"))
        .unwrap()
        .focus();
    h.run();
    h.query_all_by_role(role)
        .find(|n| n.value().as_deref() == Some("#0A141E"))
        .unwrap()
        .type_text("1");
    h.run();
    assert!(
        !hex_box_shows(&h, "#0A141E"),
        "a half-typed edit is showing"
    );
    shown.set(false);
    h.run();
    h.run();
    h.run();
    shown.set(true);
    h.run();
    assert!(hex_box_shows(&h, "#0A141E"), "no stale text");
}
