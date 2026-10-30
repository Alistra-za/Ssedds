//! M6 Track F3a: the selection gestures and the bulk panel, drawn.
mod support;
use eframe::egui::{Key, Modifiers};
use egui_kittest::kittest::{NodeT, Queryable};
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

#[test]
fn esc_closes_the_slot_commands_modal_and_keeps_the_selection() {
    // Review I1: a modal not tied to `model.dialog` owns Esc.
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let key = sdc_gui::model::sets::views(&m)
        .iter()
        .find(|v| !v.key.is_empty())
        .map(|v| v.key.clone())
        .unwrap();
    m.click_set(&key, false, false);
    let mut state = sdc_gui::ui::State::default();
    state.slot_cmds.open = Some(key.clone());
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1280.0, 800.0))
        .build_ui_state(
            |ui, (m, s): &mut (AppModel, sdc_gui::ui::State)| sdc_gui::ui::draw_with(ui, m, s),
            (m, state),
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    h.key_press(Key::Escape);
    h.run();
    assert!(h.state().1.slot_cmds.open.is_none(), "Esc closes the modal");
    assert_eq!(h.state().0.selection, Selection::Set(key));
}

#[test]
fn right_click_on_an_unselected_tile_selects_it_alone() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.click_item(ROCK, false, false);
    m.click_item(LEAF, true, false);
    let mut h = harness(m);
    h.run();
    // Neither selected tile carries this label.
    h.get_by_label("odd.png").click_secondary();
    h.run();
    assert_eq!(
        h.state().selected_items(),
        vec!["landscape/odd.png".to_string()]
    );
}

#[test]
fn ctrl_click_on_a_set_pill_adds_it() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let views = sdc_gui::model::sets::views(&m);
    let keys: Vec<&sdc_gui::model::sets::SetView> =
        views.iter().filter(|v| !v.key.is_empty()).collect();
    let (a, b) = (keys[0].key.clone(), keys[1].key.clone());
    let b_name = keys[1].id.clone();
    m.click_set(&a, false, false);
    let mut h = harness(m);
    h.run();
    h.get_by_label(&b_name).click_modifiers(Modifiers::COMMAND);
    h.run();
    assert_eq!(h.state().selected_sets(), vec![a, b]);
}

#[test]
fn two_selected_textures_show_the_bulk_panel_with_mixed_values() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_field(ROCK, "encoder.effort", serde_json::json!("veryfast"));
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    m.click_item(ROCK, false, false);
    m.click_item(LEAF, true, false);
    let mut h = harness(m);
    h.run();
    h.get_by_label("2 textures selected");
    assert!(h.query_all_by_label("— mixed").count() >= 1);
    // The single panel's heading for the active texture is gone.
    assert!(h.query_by_label("Needs a decision").is_none());
}

#[test]
fn two_selected_sets_show_the_kind_chips() {
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
    h.get_by_label("2 sets selected");
    h.get_by_label_contains("All (").click();
    h.run();
    assert_eq!(h.state().bulk.kind, None);
}

#[test]
fn the_copy_dialog_ticks_every_field_and_check_none_clears_them() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_field(LEAF, "format", serde_json::json!("bc1"));
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    m.click_item(ROCK, false, false);
    m.click_item(LEAF, true, false);
    m.open_copy_settings();
    let mut h = harness(m);
    h.run();
    h.get_by_label("Check none").click();
    h.run();
    assert!(matches!(
        &h.state().dialog,
        Some(Dialog::CopySettings { fields, .. }) if fields.iter().all(|f| !f.1)
    ));
}

#[test]
fn the_copy_dialog_applies_nothing_with_nothing_ticked() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_field(LEAF, "format", serde_json::json!("bc1"));
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    m.click_item(ROCK, false, false);
    m.click_item(LEAF, true, false);
    m.open_copy_settings();
    let mut h = harness(m);
    h.run();
    h.get_by_label("Check none").click();
    h.run();
    let apply = h.get_by_label("Apply");
    assert!(apply.accesskit_node().is_disabled());
    apply.click();
    h.run();
    assert!(matches!(
        &h.state().dialog,
        Some(Dialog::CopySettings { .. })
    ));
}

#[test]
fn a_second_frame_with_nothing_changed_does_not_recompute_the_bulk_view() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.click_item(ROCK, false, false);
    m.click_item(LEAF, true, false);
    let mut h = harness(m);
    h.run();
    let n = h.state().bulk_computes();
    assert!(n >= 1);
    h.run();
    h.run();
    assert_eq!(h.state().bulk_computes(), n);
}

#[test]
fn a_mixed_slider_shows_mixed_and_an_idle_frame_writes_nothing() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    for k in [ROCK, LEAF] {
        m.set_field(k, "alpha.policy", serde_json::json!("cutout"));
    }
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    m.set_field(ROCK, "alpha.threshold", serde_json::json!(100));
    m.set_field(LEAF, "alpha.threshold", serde_json::json!(200));
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    m.click_item(ROCK, false, false);
    m.click_item(LEAF, true, false);
    let before = m.bulk_view().rows.clone();
    let row = before
        .iter()
        .find(|r| r.field == "alpha.threshold")
        .unwrap();
    assert!(row.applies > 0 && row.value.is_none(), "{row:?}");
    let mut h = harness(m);
    h.run();
    h.run();
    assert!(h.query_all_by_value("— mixed").count() >= 1);
    assert_eq!(h.state_mut().bulk_view().rows, before);
}

/// Review C4 (Note FB4): a row that applies to none of the selection shows "— not set", never the word `null`.
#[test]
fn a_bulk_row_that_applies_to_nothing_reads_not_set_never_null() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.click_item(ROCK, false, false);
    m.click_item(LEAF, true, false);
    let rows = m.bulk_view().rows.clone();
    assert!(
        rows.iter().any(|r| r.applies == 0),
        "the fixture has a row that applies to 0"
    );
    let mut h = harness(m);
    h.run();
    h.run();
    h.get_by_label("2 textures selected");
    let shown = |h: &Harness<'static, AppModel>, s: &str| {
        h.query_all_by_label(s).count() + h.query_all_by_value(s).count()
    };
    assert_eq!(shown(&h, "null"), 0, "the word null is never drawn");
    assert!(shown(&h, "— not set") >= 1, "an unset combo says so");
}
