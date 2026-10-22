//! Trace #18, #20, #21: the JSON editor's pending match pills (added from "+ key", no value yet).
mod support;
use eframe::egui::{self, accesskit::Role};
use egui_kittest::kittest::{NodeT as _, Queryable};
use egui_kittest::Harness;
use sdc_gui::model::AppModel;
use sdc_gui::ui::State;
use support::*;

type H = Harness<'static, (AppModel, State)>;

fn editor(m: AppModel) -> H {
    let h = Harness::builder()
        .with_size(egui::vec2(1600.0, 1200.0))
        .build_ui_state(
            |ui, (m, s): &mut (AppModel, State)| sdc_gui::ui::draw_with(ui, m, s),
            (m, State::default()),
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    let mut h = h;
    h.run();
    h
}

/// The values of every match-mode drop-down on screen, in order.
fn modes(h: &H) -> Vec<String> {
    h.query_all_by_role(Role::ComboBox)
        .filter_map(|n| n.accesskit_node().value())
        .filter(|v| v.starts_with("exact · slot") || v == "path contains")
        .collect()
}

/// Click the `n`-th "+ key" menu (sets first, then delete sections), then the item.
fn add_key(h: &mut H, n: usize, via_match: bool, item: &str) {
    h.query_all_by_label("+ key").nth(n).unwrap().click();
    h.run();
    if via_match {
        h.query_all_by_label_contains("Match")
            .last()
            .unwrap()
            .click();
        h.run();
    }
    h.query_all_by_label(item).last().unwrap().click();
    h.run();
    h.run();
}

fn pick_mode(h: &mut H, from: &str, to: &str) {
    h.query_all_by_role(Role::ComboBox)
        .find(|n| n.accesskit_node().value().as_deref() == Some(from))
        .expect("the drop-down")
        .click();
    h.run();
    h.query_all_by_label(to).last().unwrap().click();
    h.run();
    h.run();
}

fn rock_has_match2(m: &AppModel) -> bool {
    m.sets_doc
        .as_ref()
        .unwrap()
        .entries()
        .iter()
        .any(|e| e.settings.contains_key("match2"))
}

#[test]
fn exact_slot_without_a_path_shows_a_pill_and_writes_nothing() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.json_editor_open = true;
    let mut h = editor(m);
    let before = modes(&h).len();
    add_key(&mut h, 0, true, "exact · slot 2 normal");
    assert_eq!(modes(&h).len(), before + 1, "no pill for the new slot");
    assert!(!rock_has_match2(&h.state().0), "an empty match2 was stored");
}

#[test]
fn a_mode_change_moves_a_pending_pill() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.add_section("aaa_one");
    m.json_editor_open = true;
    let mut h = editor(m);
    // Section 0 is the second "+ key" (the set's is first).
    add_key(&mut h, 1, false, "path contains");
    assert!(modes(&h).iter().any(|v| v == "path contains"));
    pick_mode(&mut h, "path contains", "exact · slot 2 normal");
    let now = modes(&h);
    assert!(!now.iter().any(|v| v == "path contains"), "{now:?}");
    assert!(
        now.iter().any(|v| v.starts_with("exact · slot 2 normal")),
        "the pill snapped back: {now:?}"
    );
}

#[test]
fn remove_clears_a_sections_pending_pill_and_shifts_the_next() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.add_section("aaa_one");
    m.add_section("bbb_two");
    m.json_editor_open = true;
    let mut h = editor(m);
    add_key(&mut h, 1, false, "path contains"); // pending on section 0
    assert!(modes(&h).iter().any(|v| v == "path contains"));
    h.query_all_by_label("Remove").next().unwrap().click(); // removes section 0
    h.run();
    h.run();
    assert!(
        !modes(&h).iter().any(|v| v == "path contains"),
        "a phantom pill on the section that shifted up"
    );
    // The other way round: pending on section 1 follows it when section 0 goes.
    let d2 = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d2.path());
    m.add_section("aaa_one");
    m.add_section("bbb_two");
    m.json_editor_open = true;
    let mut h = editor(m);
    add_key(&mut h, 2, false, "path contains"); // pending on section 1
    h.query_all_by_label("Remove").next().unwrap().click();
    h.run();
    h.run();
    assert!(
        modes(&h).iter().any(|v| v == "path contains"),
        "the pending pill was lost with its section's index"
    );
}

#[test]
fn a_pending_pill_switched_to_slot_1_stays_visible() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    // Slot 1 turned off (`match1 = ""`) with another match kept, so the set still has an entry and the
    // drop-down offers slot 1.
    m.set_json_value("landscape/rock01", "match1", Some(serde_json::json!("")));
    m.set_json_value(
        "landscape/rock01",
        "match3",
        Some(serde_json::json!("a\\b")),
    );
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    m.json_editor_open = true;
    let mut h = editor(m);
    add_key(&mut h, 0, true, "exact · slot 2 normal");
    pick_mode(&mut h, "exact · slot 2 normal", "exact · slot 1 diffuse");
    let now = modes(&h);
    assert!(
        now.iter().any(|v| v.starts_with("exact · slot 1")),
        "the pill vanished: {now:?}"
    );
}
