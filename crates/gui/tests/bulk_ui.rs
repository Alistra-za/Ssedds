//! M6 Track F3a: the selection gestures and the bulk panel, drawn.
mod support;
use eframe::egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use sdc_gui::model::{AppModel, Dialog, Selection};
use support::*;

const ROCK: &str = "landscape/rock01.png";
const LEAF: &str = "landscape/leaf.png";

/// A harness over `model`. The crate bundles no fonts, so the harness needs the system fonts, as the app does.
fn harness(model: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(1280.0, 800.0))
        .build_ui_state(
            |ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model),
            model,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

#[test]
fn ctrl_click_on_tiles_selects_both_and_the_heading_counts_them() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened(d.path()));
    h.run();
    h.get_by_label("rock01.png").click();
    h.run();
    h.get_by_label("leaf.png")
        .click_modifiers(Modifiers::COMMAND);
    h.run();
    assert_eq!(
        h.state().selected_items(),
        vec![ROCK.to_string(), LEAF.to_string()]
    );
    assert_eq!(h.query_all_by_label("2 selected").count(), 1);
}

#[test]
fn right_click_on_a_selected_tile_keeps_the_selection_and_opens_the_menu() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.click_item(ROCK, false, false);
    m.click_item(LEAF, true, false);
    let mut h = harness(m);
    h.run();
    // rock01.png is selected but not active, so only its tile carries that label.
    h.get_by_label("rock01.png").click_secondary();
    h.run();
    assert_eq!(h.state().selected_items().len(), 2);
    h.get_by_label("Reset all settings for 2 selected…").click();
    h.run();
    assert!(matches!(h.state().dialog, Some(Dialog::ConfirmResetAll(_))));
}

#[test]
fn ctrl_a_selects_every_visible_texture_and_esc_clears() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened(d.path()));
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run();
    let shown = h.state().visible_items().len();
    assert_eq!(h.state().selected_items().len(), shown);
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(h.state().selection, Selection::None);
}

#[test]
fn keys_do_not_reach_the_selection_behind_a_dialog() {
    // Review Focus 3.
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.click_item(ROCK, false, false);
    m.dialog = Some(Dialog::Error("x".into()));
    let mut h = harness(m);
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(h.state().selection, Selection::Item(ROCK.into()));
    assert_eq!(h.state().selected_items().len(), 1);
}

#[test]
fn two_selected_sets_are_counted_in_the_sets_heading() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let keys: Vec<String> = sdc_gui::model::sets::views(&m)
        .iter()
        .filter(|v| !v.key.is_empty())
        .map(|v| v.key.clone())
        .collect();
    m.click_set(&keys[0], false, false);
    m.click_set(&keys[1], true, false);
    let mut h = harness(m);
    h.run();
    assert_eq!(h.query_all_by_label("2 selected").count(), 1);
}
