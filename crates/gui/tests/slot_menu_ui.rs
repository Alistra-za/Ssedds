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
    outside_sets(&h, "RMAOS").click_secondary();
    h.run();
    h.get_by_label(PIN).click();
    h.run();
    assert!(h.state().slot_locked(SET, 6));
    h.get_by_label("🔒 RMAOS").click_secondary();
    h.run();
    h.get_by_label("Unpin this slot").click();
    h.run();
    assert!(!h.state().slot_locked(SET, 6));
}

const PIN: &str = "Pin this slot (always use this file)";
const KEEP: &str = "Keep the original texture (PGPatcher won't replace it)";

/// Note F?11 (D14): the slot menu says what pinning and the PGPatcher lock do; the terms ride on hover.
#[test]
fn the_slot_menu_says_pin_and_keep_in_words() {
    let d = tempfile::tempdir().unwrap();
    let mut h = opened_on_the_set(d.path());
    h.ctx.global_style_mut(|s| {
        s.interaction.tooltip_delay = 0.0;
        s.interaction.show_tooltips_only_when_still = false;
    });
    outside_sets(&h, "RMAOS").click_secondary();
    h.run();
    h.get_by_label(PIN);
    h.get_by_label(KEEP);
    for old in ["(sdc)", "(PGPatcher lock)"] {
        assert_eq!(h.query_all_by_label_contains(old).count(), 0, "{old}");
    }
    h.get_by_label(PIN).hover();
    h.run();
    h.run();
    h.get_by_label("Pinned slots refuse other files (sdc.toml pin)");
    h.get_by_label(KEEP).hover();
    h.run();
    h.run();
    h.get_by_label("PGPatcher lock: lock_rmaos in the JSON");
}

#[test]
fn path_from_the_card_menu_opens_the_slot_commands_window_and_it_toggles_the_lock() {
    let d = tempfile::tempdir().unwrap();
    let mut h = opened_on_the_set(d.path());
    outside_sets(&h, "RMAOS").click_secondary();
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
    // Review C15 (Note FB13): eight boxes, each named for what it does and for its slot; the old term is on hover.
    for n in 1..=8 {
        assert_eq!(
            h.query_all_by_label(&format!("Pin slot {n} (always use this file)"))
                .count(),
            1
        );
    }
    let b = h.get_by_label("Pin slot 6 (always use this file)");
    h.hover_at(b.rect().center());
    h.run();
    assert!(
        h.query_by_label_contains("Pinned (locked)").is_some(),
        "the old term (locked) is on hover"
    );
    h.get_by_label("Pin slot 6 (always use this file)").click();
    h.run();
    assert!(h.state().slot_locked(SET, 6));
}

#[test]
fn emptying_the_path_box_leaves_it_empty() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    m.set_slot_command(
        SET,
        6,
        sdc_gui::model::sets::SlotCommand::Path(r"effects\white.dds".into()),
    )
    .unwrap();
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
    outside_sets(&h, "RMAOS").click_secondary();
    h.run();
    h.get_by_label("Path…").click();
    h.run();
    let has_path = |h: &Harness<'static, AppModel>| {
        h.query_all_by_role(eframe::egui::accesskit::Role::TextInput)
            .any(|n| n.value().is_some_and(|v| v.contains("white.dds")))
    };
    assert!(has_path(&h), "the box starts with the stored path");
    let empties = |h: &Harness<'static, AppModel>| {
        h.query_all_by_role(eframe::egui::accesskit::Role::TextInput)
            .filter(|n| n.value().as_deref() == Some(""))
            .count()
    };
    let before = empties(&h);
    let boxed = h
        .query_all_by_role(eframe::egui::accesskit::Role::TextInput)
        .find(|n| n.value().is_some_and(|v| v.contains("white.dds")))
        .unwrap();
    boxed.focus();
    h.run();
    h.key_press_modifiers(eframe::egui::Modifiers::COMMAND, eframe::egui::Key::A);
    h.key_press(eframe::egui::Key::Backspace);
    h.run();
    h.run();
    assert!(!has_path(&h), "an emptied box is not refilled");
    assert_eq!(
        empties(&h),
        before + 1,
        "the box is exactly empty, not left with part of the path"
    );
}
