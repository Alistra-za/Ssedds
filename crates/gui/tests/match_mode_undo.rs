//! Trace-debug #56: switching a JSON editor match pill's mode (exact <-> path contains) is one undo step.
mod support;
use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use sdc_gui::model::AppModel;
use sdc_gui::ui::State;
use serde_json::json;
use support::*;

const ROCK_SET: &str = "landscape/rock01";

type H = Harness<'static, (AppModel, State)>;

fn editor(m: AppModel) -> H {
    let h = Harness::builder()
        .with_size(egui::vec2(1600.0, 900.0))
        .build_ui_state(
            |ui, (m, s): &mut (AppModel, State)| sdc_gui::ui::draw_with(ui, m, s),
            (m, State::default()),
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

/// Open a slot 1 pill's mode drop-down (the last one when `last`, else the first) and pick "path contains".
fn to_contains(h: &mut H, last: bool) {
    {
        let mut combos = h.query_all(
            egui_kittest::kittest::by()
                .role(egui::accesskit::Role::ComboBox)
                .value("exact · slot 1 diffuse"),
        );
        let c = if last { combos.last() } else { combos.next() };
        c.expect("the mode drop-down").click();
    }
    h.run();
    h.query_all_by_label("path contains")
        .last()
        .expect("the option")
        .click();
    h.run();
    h.run();
}

fn rock(m: &AppModel) -> Vec<(String, serde_json::Value)> {
    m.sets_doc
        .as_ref()
        .unwrap()
        .entries()
        .iter()
        .find(|e| e.id == ROCK_SET)
        .map(|e| e.settings.clone().into_iter().collect())
        .unwrap_or_default()
}

#[test]
fn section_mode_switch_is_one_undo_step() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.add_section("aaa_one");
    m.json_editor_open = true;
    let steps = m.history.undo.len();
    let mut h = editor(m);
    h.run();
    to_contains(&mut h, true);
    let s = h.state().0.sections()[0].clone();
    assert_eq!(s.get("path_contains"), Some(&json!("aaa_one")));
    assert_eq!(s.get("match1"), None);
    assert_eq!(h.state().0.history.undo.len(), steps + 1);
    h.state_mut().0.undo();
    let s = h.state().0.sections()[0].clone();
    assert_eq!(s.get("match1"), Some(&json!("aaa_one")));
    assert_eq!(s.get("path_contains"), None);
}

#[test]
fn set_mode_switch_is_one_undo_step() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.json_editor_open = true;
    let (steps, before) = (m.history.undo.len(), rock(&m));
    let mut h = editor(m);
    h.run();
    to_contains(&mut h, false);
    let after = rock(&h.state().0);
    assert!(after.iter().any(|(k, _)| k == "path_contains"), "{after:?}");
    assert_eq!(h.state().0.history.undo.len(), steps + 1);
    h.state_mut().0.undo();
    assert_eq!(rock(&h.state().0), before);
}
