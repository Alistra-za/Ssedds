//! Task 13 (item 11, DECISIONS AG106-AG110): the PGPatcher JSON editor shows its errors on the entries, counts them,
//! walks them with Next error / Prev (F8 / Shift+F8), filters by search, and builds its JSON text once per revision.
mod support;
use eframe::egui::{self, accesskit::Role, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use engine::config::Subtype;
use sdc_gui::model::{Answer, AppModel};
use sdc_gui::ui::{actions, json_editor};
use serde_json::json;
use std::time::Duration;
use support::*;

const ROCK: &str = "landscape/rock01";
const LEAF: &str = "landscape/leaf";
const FOLIAGE: &str = "subsurface foliage is on but the set has no subsurface texture";

fn two_sets(dir: &std::path::Path, leaf: Subtype) -> AppModel {
    let mut m = pbr_opened(dir);
    m.decide("landscape/leaf.png", Answer::Subtype(leaf));
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.json_editor_open = true;
    m
}

/// Both sets get the foliage block; the leaf also gets a pitfall (Delete wins over the other alpha keys).
fn broken(dir: &std::path::Path) -> AppModel {
    let mut m = two_sets(dir, Subtype::PbrAlbedo);
    for id in [ROCK, LEAF] {
        m.set_json_value(id, "subsurface_foliage", Some(json!(true)));
    }
    m.set_json_value(LEAF, "alphaprop_delete", Some(json!(true)));
    m.set_json_value(LEAF, "alphaprop_thresh", Some(json!(10)));
    m
}

fn harness(m: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(egui::vec2(1200.0, 900.0))
        .build_ui_state(|ui, m: &mut AppModel| json_editor::draw(ui.ctx(), m), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    let mut h = h;
    h.run();
    h
}

/// The entries drawn open: each has a "+ key" menu.
fn open_entries(h: &Harness<'static, AppModel>) -> usize {
    h.query_all_by_label("+ key").count()
}

fn collapse_all(h: &mut Harness<'static, AppModel>) {
    while h.query_all_by_label("Collapse").next().is_some() {
        h.query_all_by_label("Collapse").next().unwrap().click();
        h.run();
    }
    assert_eq!(open_entries(h), 0);
}

/// The search box is the first text box of the window.
fn search(h: &mut Harness<'static, AppModel>, text: &str) {
    h.query_all_by_role(Role::TextInput).next().unwrap().click();
    h.run();
    h.query_all_by_role(Role::TextInput)
        .next()
        .unwrap()
        .type_text(text);
    h.run();
}

#[test]
fn an_error_shows_under_its_entry_and_an_unowned_one_in_general() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(broken(d.path()));
    assert_eq!(
        h.query_all_by_label_contains(FOLIAGE).count(),
        2,
        "one under each entry"
    );
    h.get_by_label_contains("Delete wins");
    assert!(
        h.query_by_label("General").is_none(),
        "nothing is unowned yet"
    );
    h.state_mut()
        .sets_doc
        .as_mut()
        .unwrap()
        .set_json_name(Some("a/b"));
    h.run();
    h.get_by_label("General");
    h.get_by_label_contains("JSON name");
}

#[test]
fn the_header_counts_errors_and_warnings() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(broken(d.path()));
    h.get_by_label("2 errors, 1 warning");
    h.state_mut()
        .set_json_value(ROCK, "subsurface_foliage", None);
    h.run();
    h.get_by_label("1 error, 1 warning");
}

#[test]
fn a_skin_set_has_no_entry_and_no_error_here() {
    let d = tempfile::tempdir().unwrap();
    let mut m = two_sets(d.path(), Subtype::SkinAlbedo);
    m.set_json_value(LEAF, "subsurface_foliage", Some(json!(true)));
    m.set_json_value(ROCK, "subsurface_foliage", Some(json!(true)));
    let h = harness(m);
    h.get_by_label("1 error, 0 warnings");
    assert_eq!(h.query_all_by_label_contains(FOLIAGE).count(), 1);
    assert_eq!(open_entries(&h), 1);
}

#[test]
fn next_error_and_prev_wrap_and_open_the_target() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(broken(d.path()));
    collapse_all(&mut h);
    h.get_by_label("Next error").click();
    h.run();
    assert_eq!(open_entries(&h), 1, "the first entry with an error opens");
    h.get_by_label("Next error").click();
    h.run();
    assert_eq!(open_entries(&h), 2, "the second opens");
    h.query_all_by_label("Collapse").next().unwrap().click(); // closes the first again
    h.run();
    assert_eq!(open_entries(&h), 1);
    h.get_by_label("Next error").click();
    h.run();
    assert_eq!(open_entries(&h), 2, "Next wraps to the first and opens it");
    h.query_all_by_label("Collapse").last().unwrap().click(); // closes the second
    h.run();
    h.get_by_label("Prev").click();
    h.run();
    assert_eq!(
        open_entries(&h),
        2,
        "Prev from the first wraps to the last and opens it"
    );
}

#[test]
fn f8_and_shift_f8_walk_the_errors() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(broken(d.path()));
    collapse_all(&mut h);
    h.key_press(Key::F8);
    h.run();
    assert_eq!(open_entries(&h), 1);
    h.key_press_modifiers(Modifiers::SHIFT, Key::F8);
    h.run();
    assert_eq!(open_entries(&h), 2, "Shift+F8 wrapped back to the last");
}

#[test]
fn search_filters_by_name_and_by_path_and_says_how_many() {
    let d = tempfile::tempdir().unwrap();
    let m = broken(d.path());
    let mut h = harness(m);
    h.get_by_label("showing 2 of 2");
    search(&mut h, "LEAF");
    h.get_by_label("showing 1 of 2");
    assert_eq!(open_entries(&h), 1);
    h.get_by_label("2 errors, 1 warning"); // the counts ignore the filter
}

#[test]
fn search_finds_a_path_inside_the_entry() {
    let d = tempfile::tempdir().unwrap();
    let mut m = broken(d.path());
    m.set_json_value(ROCK, "match2", Some(json!("landscape/special_zzz")));
    assert!(m.wait_idle(Duration::from_secs(60)));
    let mut h = harness(m);
    search(&mut h, "SPECIAL_zzz");
    h.get_by_label("showing 1 of 2");
}

#[test]
fn next_error_clears_a_filter_that_hides_the_error() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(broken(d.path()));
    search(&mut h, "zzz-no-such-entry");
    h.get_by_label("showing 0 of 2");
    h.get_by_label("2 errors, 1 warning");
    h.get_by_label("Next error").click();
    h.run();
    h.get_by_label("showing 2 of 2");
    assert_eq!(open_entries(&h), 2);
}

#[test]
fn the_json_text_is_built_once_per_revision() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(two_sets(d.path(), Subtype::PbrAlbedo));
    let before = json_editor::cache_builds();
    for _ in 0..10 {
        h.run();
    }
    assert_eq!(
        json_editor::cache_builds(),
        before,
        "no rebuild without a change"
    );
    h.state_mut()
        .set_json_value(ROCK, "subsurface_foliage", Some(json!(true)));
    for _ in 0..10 {
        h.run();
    }
    assert_eq!(
        json_editor::cache_builds(),
        before + 1,
        "one rebuild after an edit"
    );
}

#[test]
fn f8_is_in_the_actions_table() {
    let keys: Vec<_> = actions::actions()
        .iter()
        .filter(|a| a.area == actions::Area::JsonEditor)
        .flat_map(|a| a.keys.iter().map(|s| s.text()))
        .collect();
    assert_eq!(keys, ["F8", "Shift+F8"]);
}

#[test]
fn a_rescan_refreshes_the_editor_without_an_edit() {
    let d = tempfile::tempdir().unwrap();
    let mut m = two_sets(d.path(), Subtype::PbrAlbedo);
    m.set_json_value(ROCK, "rename", Some(json!("rock09")));
    let mut h = harness(m);
    h.run();
    let before = json_editor::cache_builds();
    h.state_mut()
        .set_tree_anim([("landscape/rock01".to_string(), true)].into());
    h.run();
    assert_eq!(
        json_editor::cache_builds(),
        before + 1,
        "new Tree_Anim values rebuild"
    );
    assert!(h
        .query_by_label_contains("textures/dlc/landscape/rock01")
        .is_none());
    h.state_mut()
        .set_known(sdc_gui::model::known_paths::KnownPaths {
            textures: [("textures/dlc/landscape/rock01.dds".to_string(), 1u16)].into(),
            mesh_dirs: vec![],
        });
    h.run();
    assert_eq!(
        json_editor::cache_builds(),
        before + 2,
        "new known paths rebuild"
    );
    h.get_by_label_contains("textures/dlc/landscape/rock01");
}
