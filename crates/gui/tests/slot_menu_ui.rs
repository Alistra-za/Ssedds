//! F3a Task 8: the slot menu on a set-editor card, drawn.
mod support;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use sdc_gui::model::AppModel;
use support::*;

const SET: &str = "landscape/rock01";

fn harness(model: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(1600.0, 900.0))
        .build_ui_state(
            |ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model),
            model,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

fn opened_on_the_set(d: &std::path::Path) -> Harness<'static, AppModel> {
    let mut m = pbr_opened(d);
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    let key = m
        .sets
        .iter()
        .find(|s| s.key.ends_with(SET))
        .unwrap()
        .key
        .clone();
    m.select_set(&key);
    let mut h = harness(m);
    h.run();
    h
}

#[test]
fn a_card_right_click_locks_and_unlocks_the_slot() {
    let d = tempfile::tempdir().unwrap();
    let mut h = opened_on_the_set(d.path());
    h.get_by_label("RMAOS").click_secondary();
    h.run();
    h.get_by_label("Pin slot (sdc)").click();
    h.run();
    assert!(h.state().slot_locked(SET, 6));
    h.get_by_label("🔒 RMAOS").click_secondary();
    h.run();
    h.get_by_label("Unpin slot (sdc)").click();
    h.run();
    assert!(!h.state().slot_locked(SET, 6));
}

#[test]
fn path_from_the_card_menu_opens_the_slot_commands_window_and_it_toggles_the_lock() {
    let d = tempfile::tempdir().unwrap();
    let mut h = opened_on_the_set(d.path());
    h.get_by_label("RMAOS").click_secondary();
    h.run();
    h.get_by_label("Path…").click();
    h.run();
    assert_eq!(
        h.query_all_by_label("Slot commands: landscape/rock01")
            .count(),
        1
    );
    assert!(
        h.query_all_by_label("Apply").count() >= 1,
        "slot 6 is in Path mode"
    );
    h.get_by_label("Lock slot 6").click();
    h.run();
    assert!(h.state().slot_locked(SET, 6));
}
