//! GUI tweaks (spec `2026-10-14-sdc-gui-tweaks-design.md`): the input name picker's model, delete sections and their
//! JSON, delete hits, the column checkboxes and arrows, the 2D | 3D switch and the TruePBR JSON editor window.
mod support;
use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use sdc_gui::model::pbr_export::json_entries;
use sdc_gui::model::sections::{delete_hits, written};
use sdc_gui::model::set_preview::{SetPreview, DEFAULT_CAP};
use sdc_gui::model::viewport3d::FRAME_LABEL;
use sdc_gui::model::AppModel;
use sdc_gui::pgp::writer::{entry_node, EntryInput};
use sdc_gui::sets_doc::SetsDoc;
use sdc_gui::ui::set_preview::{PREPARING, SETTINGS_BUTTON};
use sdc_gui::ui::State;
use serde_json::json;
use std::time::Duration;
use support::*;

const ROCK_SET: &str = "landscape/rock01";

fn idle(m: &mut AppModel) {
    assert!(
        m.wait_idle(Duration::from_secs(120)),
        "planning did not finish"
    );
}

fn rock_key(m: &AppModel) -> String {
    json_entries(m)
        .into_iter()
        .find(|(v, _)| v.id == ROCK_SET)
        .expect("the rock set has an entry")
        .0
        .key
}

#[test]
fn an_input_name_aliases_every_member_and_the_set_keeps_its_settings() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.set_json_value(ROCK_SET, "specular_level", Some(json!(0.5)));
    assert_eq!(
        m.input_name_of(ROCK_SET).as_deref(),
        Some("textures/landscape/rock01")
    );
    let plan = m
        .input_name_plan(ROCK_SET, "textures/landscape/rock02.dds")
        .unwrap();
    let new_of = |k: &str| {
        plan.rows
            .iter()
            .find(|r| r.key == k)
            .map(|r| r.new.clone())
            .unwrap_or_else(|| panic!("no row {k} in {:?}", plan.rows))
    };
    assert_eq!(new_of("landscape/rock01.png"), "landscape/rock02.png");
    assert_eq!(
        new_of("landscape/rock01_normal.png"),
        "landscape/rock02_normal.png"
    );
    assert_eq!(
        plan.retarget,
        Some((ROCK_SET.to_string(), "landscape/rock02".to_string()))
    );
    let steps = m.history.undo.len();
    m.set_input_name(ROCK_SET, "textures/landscape/rock02")
        .unwrap();
    assert_eq!(m.history.undo.len(), steps + 1, "one undo step");
    idle(&mut m);
    let v = json_entries(&m)
        .into_iter()
        .find(|(v, _)| v.id == "landscape/rock02")
        .expect("the set under its new name")
        .0;
    assert_eq!(v.settings.get("specular_level"), Some(&json!(0.5)));
    assert!(d.path().join("textures/landscape/rock01.png").exists());
    m.undo();
    idle(&mut m);
    assert_eq!(
        m.input_name_of(ROCK_SET).as_deref(),
        Some("textures/landscape/rock01")
    );
}

#[test]
fn an_input_name_outside_the_project_or_the_same_name_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let m = pbr_opened(d.path());
    let e = m.input_name_plan(ROCK_SET, "meshes/rock02").unwrap_err();
    assert!(e.contains("textures/"), "{e}");
    let e = m
        .input_name_plan(ROCK_SET, "textures/landscape/rock01")
        .unwrap_err();
    assert!(e.contains("already"), "{e}");
    // leaf.png already has that name.
    let e = m
        .input_name_plan(ROCK_SET, "textures/landscape/leaf")
        .unwrap_err();
    assert!(e.contains("already has this name"), "{e}");
}

#[test]
fn sections_round_trip_in_sdc_project_toml() {
    let d = tempfile::tempdir().unwrap();
    let mut s = SetsDoc::open(d.path());
    let mut keys = sdc_gui::pgp::settings::Settings::new();
    keys.insert("delete".into(), json!(true));
    keys.insert("match1".into(), json!("landscape\\rock01"));
    s.add_section(&keys);
    s.set_section_value(0, "nif_filter", Some(json!("architecture\\whiterun")));
    s.save(false).unwrap();
    let text = std::fs::read_to_string(d.path().join("sdc-project.toml")).unwrap();
    assert!(text.contains("[[pgp_sections]]"), "{text}");
    let s = SetsDoc::open(d.path());
    let sec = &s.sections()[0];
    assert_eq!(sec["match1"], json!("landscape\\rock01"));
    assert_eq!(sec["nif_filter"], json!("architecture\\whiterun"));
    assert!(s.entries().is_empty(), "a section is not a set");
    let mut s = s;
    s.remove_section(0);
    assert!(s.sections().is_empty());
    assert!(!s.text().contains("pgp_sections"));
}

#[test]
fn retarget_moves_an_automatic_sets_entry_and_a_renamed_ones_source() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("sdc-project.toml"),
        "[[sets]]\nid = \"a/one\"\nparallax = true\n\n[[sets]]\nid = \"Shown\"\nrenamed_from = \"b/two\"\n\n\
         [[sets]]\nid = \"c/three\"\nborrow = { slot6 = \"a/one\" }\n",
    )
    .unwrap();
    let mut s = SetsDoc::open(d.path());
    s.retarget_auto("a/one", "a/uno");
    s.retarget_auto("b/two", "b/dos");
    let e = s.entries();
    assert!(e.iter().any(|e| e.id == "a/uno"
        && e.renamed_from.is_none()
        && e.settings.contains_key("parallax")));
    assert!(e
        .iter()
        .any(|e| e.id == "Shown" && e.renamed_from.as_deref() == Some("b/dos")));
    let c = e.iter().find(|e| e.id == "c/three").unwrap();
    assert_eq!(c.borrow.get(&6).map(String::as_str), Some("a/uno"));
}

#[test]
fn a_delete_section_is_written_last_with_only_its_keys() {
    let mut s = sdc_gui::pgp::settings::Settings::new();
    s.insert("match2".into(), json!("landscape/rock01_n"));
    s.insert("nif_filter".into(), json!("architecture/whiterun"));
    s.insert("delete".into(), json!(true));
    s.insert("parallax".into(), json!(true));
    assert!(written(&s));
    let v = entry_node(&EntryInput {
        settings: s.clone(),
        section: true,
        ..EntryInput::default()
    })
    .to_value();
    assert_eq!(
        v,
        json!({"match2": "landscape\\rock01_n.dds", "nif_filter": "architecture\\whiterun", "delete": true})
    );
    s.remove("delete");
    let v = entry_node(&EntryInput {
        settings: s.clone(),
        section: true,
        ..EntryInput::default()
    })
    .to_value();
    assert_eq!(v["delete"], json!(true), "a section always deletes");
    let mut only_mesh = sdc_gui::pgp::settings::Settings::new();
    only_mesh.insert("nif_filter".into(), json!("x"));
    only_mesh.insert("delete".into(), json!(true));
    assert!(!written(&only_mesh), "a mesh filter alone matches nothing");
}

#[test]
fn a_delete_section_that_matches_a_sets_texture_is_a_hit_and_goes_in_the_export() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.add_section("landscape\\rock01");
    let hits = delete_hits(&m);
    assert!(
        hits.iter().any(|h| h.section == 0 && h.set_id == ROCK_SET),
        "{hits:?}"
    );
    m.set_section_value(0, "match1", Some(json!("landscape\\elsewhere")));
    assert!(delete_hits(&m).is_empty());
    m.set_section_value(0, "path_contains", Some(json!("rock0")));
    assert!(delete_hits(&m).iter().any(|h| h.set_id == ROCK_SET));
    let p = sdc_gui::model::pbr_export::prepare(&m).expect("a JSON to write");
    let (id, deps, e) = p.entries.last().unwrap();
    assert_eq!(id, "delete section 1");
    assert!(deps.is_empty() && e.section);
    m.undo();
    m.undo();
    m.undo();
    assert!(m.sections().is_empty(), "each edit is a step");
}

type H = Harness<'static, (AppModel, State)>;

fn harness(m: AppModel, preview: bool) -> H {
    let state = State {
        set3d: if preview {
            SetPreview::new(None, None, None, DEFAULT_CAP)
        } else {
            SetPreview::default()
        },
        ..State::default()
    };
    let h = Harness::builder()
        .with_size(egui::vec2(1600.0, 900.0))
        .build_ui_state(
            |ui, (m, s): &mut (AppModel, State)| sdc_gui::ui::draw_with(ui, m, s),
            (m, state),
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

#[test]
fn the_header_checkboxes_hide_columns_and_the_arrows_collapse_and_expand_them() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened(d.path()), false);
    h.run();
    h.get_by_label("Textures column").click();
    h.run();
    assert!(h.state().0.settings.textures_hidden);
    assert!(h.query_by_label("Textures").is_none());
    h.get_by_label("Textures column").click();
    h.run();
    assert!(!h.state().0.settings.textures_hidden);
    // The Textures heading's arrow is the first ◀.
    h.query_all_by_label("◀").next().unwrap().click();
    h.run();
    assert!(h.state().0.settings.textures_collapsed);
    let strip = h.get_by_label("Textures");
    assert!(strip.rect().width() < 30.0);
    strip.click();
    h.run();
    assert!(!h.state().0.settings.textures_collapsed);
}

#[test]
fn the_2d_3d_switch_swaps_the_slot_cards_for_the_preview_and_keeps_the_truepbr_settings() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    let key = rock_key(&m);
    m.select_set(&key);
    let mut h = harness(m, true);
    h.run();
    // One "Match on this" per slot card.
    assert!(h.query_all_by_label("Match on this").count() >= 1);
    h.get_by_label("Specular level");
    assert!(h.query_by_label(FRAME_LABEL).is_none());
    h.get_by_label("3D").click();
    h.run();
    assert!(h.state().0.settings.set_view_3d);
    // The 3D view (no poll in this harness, so it may still be preparing).
    assert!(h.query_by_label(FRAME_LABEL).is_some() || h.query_by_label(PREPARING).is_some());
    assert!(h.query_by_label("Match on this").is_none());
    // AG132: the TruePBR settings are in their window.
    assert!(h.query_by_label("Specular level").is_none());
    h.get_by_label(SETTINGS_BUTTON).click();
    h.run();
    h.get_by_label("Specular level");
    h.query_all_by_label("2D").next().unwrap().click();
    h.run();
    assert!(!h.state().0.settings.set_view_3d);
    assert!(h.query_all_by_label("Match on this").count() >= 1);
}

#[test]
fn the_json_editor_shows_each_set_and_adds_a_delete_section_that_crosses_out_the_set() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.json_editor_open = true;
    let mut h = harness(m, false);
    h.run();
    h.get_by_label(sdc_gui::ui::json_editor::WINDOW_TITLE);
    assert!(
        h.query_all_by_label("Show").count() >= 1,
        "the rock set's pill"
    );
    h.get_by_label("+ Delete section").click();
    h.run();
    assert_eq!(h.state().0.sections().len(), 1);
    h.get_by_label("Delete section 1");
    h.get_by_label_contains("matches no texture");
    h.state_mut()
        .0
        .set_section_value(0, "match1", Some(json!("landscape\\rock01")));
    h.run();
    h.get_by_label_contains("Delete section 1 deletes the shapes this set is for");
    h.get_by_label("Close").click();
    h.run();
    assert!(!h.state().0.json_editor_open);
}

/// PBR grass (Task 5 fix round 1): the JSON editor's "+ key" menu offers no key grass ignores on a pbr_grass set,
/// since it would be stored, never shown, and come back on a switch to pbr; a pbr set still offers it.
#[test]
fn the_json_editor_add_menu_hides_grass_ignored_keys_on_a_pbr_grass_set() {
    let offers_emissive = |grass: bool| {
        let d = tempfile::tempdir().unwrap();
        let mut m = pbr_opened(d.path());
        if grass {
            m.set_shader(ROCK_SET, Some(engine::shader::Shader::PbrGrass));
            idle(&mut m);
        }
        m.json_editor_open = true;
        let mut h = harness(m, false);
        h.run();
        let before = h.query_all_by_label("Emissive").count();
        h.query_all_by_label("+ key").next().unwrap().click();
        h.run();
        h.query_all_by_label("Emissive").count() > before
    };
    assert!(offers_emissive(false), "a pbr set offers emissive");
    assert!(!offers_emissive(true), "a pbr_grass set does not");
}
