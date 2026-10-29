//! Packing fills Task 2: per-output fills, the RMAOS/RFAOS choice on the packed card, the Advanced skin follow, the
//! dropdown groups and the retired global fill panel.
mod support;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use engine::config::{PackRecipe, RawMap, Subtype};
use image::{ImageBuffer, Luma, Rgb};
use sdc_gui::model::pack_fills::{fill_rows, FillRow};
use sdc_gui::model::pbr_export::{export_blocks, json_entries};
use sdc_gui::model::sets::{set_views, SlotFill, SlotKind};
use sdc_gui::model::AppModel;
use sdc_gui::settings::GuiSettings;
use serde_json::json;
use std::path::Path;
use std::time::Duration;
use support::session;

const DIFFUSE: &str = "m/rock01.png";
const SK: &str = "m/rock01_sk.png";
const RMAOS: &str = "m/rock01_rmaos";
const RFAOS: &str = "m/rock01_rfaos";

fn grey(dir: &Path, name: &str, v: u8) {
    ImageBuffer::from_fn(64, 64, |x, _| Luma([v.wrapping_add(x as u8)]))
        .save(dir.join(name))
        .unwrap();
}

/// A diffuse, a roughness map and a `_sk`: `_rmaos` with no AO or metallic source.
fn fixture_with(dir: &Path, sdc_toml: &str, second_set: bool) -> AppModel {
    let r = dir.join("m");
    std::fs::create_dir_all(&r).unwrap();
    if !sdc_toml.is_empty() {
        std::fs::write(dir.join("sdc.toml"), sdc_toml).unwrap();
    }
    for base in ["rock01", "rock02"] {
        if base == "rock02" && !second_set {
            continue;
        }
        ImageBuffer::from_fn(64, 64, |x, y| Rgb([(x * 4) as u8, (y * 4) as u8, 90u8]))
            .save(r.join(format!("{base}.png")))
            .unwrap();
        grey(&r, &format!("{base}_roughness.png"), 120);
    }
    grey(&r, "rock01_sk.png", 0);
    let settings = GuiSettings {
        export_folder: Some(dir.join("export")),
        ..GuiSettings::default()
    };
    let mut m = AppModel::new(settings, None, session());
    m.accept = engine::policy::AcceptSuggestions::None;
    m.open_folder(dir.to_path_buf());
    idle(&mut m);
    m
}

fn fixture(dir: &Path) -> AppModel {
    fixture_with(dir, "", false)
}

fn idle(m: &mut AppModel) {
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
}

fn subtype(m: &AppModel, key: &str) -> Option<serde_json::Value> {
    m.project.as_ref().unwrap().file_value(key, "subtype")
}

fn resolved(m: &AppModel, key: &str) -> Subtype {
    m.item(key).unwrap().job.as_ref().unwrap().profile.subtype
}

fn texts(m: &AppModel) -> (String, String) {
    (
        m.project.as_ref().unwrap().text(),
        m.sets_doc.as_ref().unwrap().text(),
    )
}

fn channel(m: &AppModel, key: &str, which: &str) -> String {
    m.item(key).unwrap().job.as_ref().unwrap().provenance[which].to_string()
}

fn roles(m: &AppModel, key: &str) -> Vec<RawMap> {
    fill_rows(m, key)
        .into_iter()
        .map(|r| match r {
            FillRow::FromFile { role, .. } | FillRow::Value { role, .. } => role,
        })
        .collect()
}

fn editor(mut m: AppModel) -> Harness<'static, AppModel> {
    let key = m
        .sets
        .iter()
        .find(|s| s.key.ends_with("m/rock01"))
        .expect("the rock01 set")
        .key
        .clone();
    m.select_set(&key);
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

fn settle(h: &mut Harness<'static, AppModel>) {
    h.run();
    h.state_mut().wait_idle(Duration::from_secs(60));
    h.run();
}

#[test]
fn an_rmaos_card_without_an_ao_source_has_an_ao_row_and_a_set_value_becomes_the_channel() {
    let d = tempfile::tempdir().unwrap();
    let mut h = editor(fixture(d.path()));
    h.run();
    h.get_by_label("AO");
    // Note DY18: a channel with a file names it in its source picker.
    assert_eq!(
        h.get_by_role_and_label(
            eframe::egui::accesskit::Role::ComboBox,
            "roughness channel source"
        )
        .value()
        .as_deref(),
        Some("Source: rock01_roughness.png")
    );
    assert_eq!(
        roles(h.state(), RMAOS),
        vec![
            RawMap::Roughness,
            RawMap::Metallic,
            RawMap::Ao,
            RawMap::Specular
        ]
    );
    h.state_mut().set_pack_fill(RMAOS, RawMap::Ao, Some(200));
    settle(&mut h);
    let m = h.state();
    assert_eq!(
        m.project
            .as_ref()
            .unwrap()
            .file_value(RMAOS, "packing.fill.ao"),
        Some(json!(200)),
        "the fill is the packed output's own key"
    );
    assert_eq!(m.project.as_ref().unwrap().fill(RawMap::Ao), None);
    assert_eq!(channel(m, RMAOS, "pack.b"), "fill:200");
    // The reset removes the key again (the slot rows scroll sideways in a narrow column: bring it into view).
    h.get_by_label("Reset").scroll_to_me();
    h.run();
    h.get_by_label("Reset").click();
    settle(&mut h);
    let m = h.state();
    assert_eq!(
        m.project
            .as_ref()
            .unwrap()
            .file_value(RMAOS, "packing.fill.ao"),
        None
    );
    assert!(!channel(m, RMAOS, "pack.b").starts_with("fill:"));
}

#[test]
fn an_rmaos_card_never_shows_a_fuzz_row() {
    let d = tempfile::tempdir().unwrap();
    let mut h = editor(fixture(d.path()));
    h.run();
    assert!(h.query_by_label("fuzz").is_none());
    assert!(h.query_by_label("opacity").is_none());
    h.get_by_label("metallic");
    assert!(!roles(h.state(), RMAOS).contains(&RawMap::Fuzz));
    // Choosing _rmaos writes no fuzz fill.
    h.state_mut().set_packing_recipe(RMAOS, PackRecipe::Rmaos);
    settle(&mut h);
    let p = h.state().project.as_ref().unwrap();
    assert_eq!(p.file_value(RMAOS, "packing.fill.fuzz"), None);
    assert_eq!(p.file_value(RFAOS, "packing.fill.fuzz"), None);
}

#[test]
fn rfaos_shows_a_fuzz_row_hides_metallic_and_switches_the_inferred_members() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path());
    assert_eq!(resolved(&m, DIFFUSE), Subtype::PbrAlbedo);
    assert_eq!(resolved(&m, SK), Subtype::BacklightTint);
    m.set_pack_fill(RMAOS, RawMap::Ao, Some(77));
    m.set_pack_fill(RMAOS, RawMap::Metallic, Some(9));
    idle(&mut m);
    let before = texts(&m);
    let mut h = editor(m);
    h.run();
    h.state_mut().set_packing_recipe(RMAOS, PackRecipe::Rfaos);
    settle(&mut h);
    h.run();
    h.get_by_label("fuzz");
    assert!(h.query_by_label("metallic").is_none());
    let m = h.state();
    assert_eq!(
        roles(m, RFAOS),
        vec![
            RawMap::Roughness,
            RawMap::Fuzz,
            RawMap::Ao,
            RawMap::Specular
        ]
    );
    // The recipe is on the _rfaos key, the contradicting one is gone, and the fill followed the output.
    let p = m.project.as_ref().unwrap();
    assert_eq!(p.file_value(RFAOS, "packing.recipe"), Some(json!("rfaos")));
    assert_eq!(p.file_value(RMAOS, "packing.recipe"), None);
    assert_eq!(p.file_value(RFAOS, "packing.fill.ao"), Some(json!(77)));
    assert_eq!(p.file_value(RMAOS, "packing.fill.ao"), None);
    assert_eq!(p.file_value(RMAOS, "packing.fill.metallic"), None);
    assert!(m.unmatched_keys.is_empty(), "{:?}", m.unmatched_keys);
    // The inferred members follow the engine's suggestion: nothing was written for them.
    assert_eq!(subtype(m, DIFFUSE), None);
    assert_eq!(subtype(m, SK), None);
    assert_eq!(resolved(m, DIFFUSE), Subtype::SkinAlbedo);
    assert_eq!(resolved(m, SK), Subtype::SkinSk);
    assert!(!m.sets_doc.as_ref().unwrap().text().contains("auto_skin"));
    h.get_by_label_contains("metallic fill removed (RFAOS has no metallic channel)");
    let v = set_views(m);
    let v = v.iter().find(|v| v.id == "m/rock01").unwrap();
    assert!(matches!(
        v.slots.get(&SlotKind::SkinSk),
        Some(SlotFill::Own { .. })
    ));
    // One undo restores both documents.
    h.state_mut().undo();
    settle(&mut h);
    assert_eq!(texts(h.state()), before);
    assert_eq!(resolved(h.state(), DIFFUSE), Subtype::PbrAlbedo);
    assert_eq!(resolved(h.state(), SK), Subtype::BacklightTint);
}

#[test]
fn switching_back_to_rmaos_reverts_the_inferred_members_and_keeps_user_set_ones() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path());
    m.set_packing_recipe(RMAOS, PackRecipe::Rfaos);
    idle(&mut m);
    assert_eq!(resolved(&m, SK), Subtype::SkinSk);
    m.set_field(SK, "subtype", json!("glow"));
    idle(&mut m);
    m.set_packing_recipe(RFAOS, PackRecipe::Rmaos);
    idle(&mut m);
    assert_eq!(resolved(&m, DIFFUSE), Subtype::PbrAlbedo, "inferred again");
    assert_eq!(subtype(&m, DIFFUSE), None);
    assert_eq!(
        subtype(&m, SK),
        Some(json!("glow")),
        "the hand-set one stays"
    );
    let p = m.project.as_ref().unwrap();
    assert_eq!(p.file_value(RMAOS, "packing.recipe"), Some(json!("rmaos")));
    assert_eq!(p.file_value(RFAOS, "packing.recipe"), None);
    assert!(m.item(RMAOS).is_some());
}

#[test]
fn members_the_user_set_are_left_alone() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path());
    m.set_field(DIFFUSE, "subtype", json!("diffuse"));
    m.set_field(SK, "subtype", json!("backlight_tint"));
    idle(&mut m);
    m.set_packing_recipe(RMAOS, PackRecipe::Rfaos);
    idle(&mut m);
    assert_eq!(subtype(&m, DIFFUSE), Some(json!("diffuse")));
    assert_eq!(subtype(&m, SK), Some(json!("backlight_tint")));
    assert_eq!(resolved(&m, DIFFUSE), Subtype::Diffuse);
    assert_eq!(resolved(&m, SK), Subtype::BacklightTint);
}

#[test]
fn a_read_only_project_file_does_not_stop_the_recipe() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("sdc-project.toml"), "[[sets]\nbroken").unwrap();
    let mut m = fixture(d.path());
    assert!(m.sets_doc.as_ref().unwrap().read_only());
    m.set_packing_recipe(RMAOS, PackRecipe::Rfaos);
    idle(&mut m);
    assert_eq!(
        m.project
            .as_ref()
            .unwrap()
            .file_value(RFAOS, "packing.recipe"),
        Some(json!("rfaos"))
    );
    assert_eq!(subtype(&m, DIFFUSE), None, "no member was written");
    assert_eq!(
        resolved(&m, DIFFUSE),
        Subtype::SkinAlbedo,
        "the engine follows"
    );
    assert!(m.item(RFAOS).is_some(), "the recipe took effect");
}

#[test]
fn a_read_only_sdc_toml_disables_the_recipe_and_the_fills_and_says_why() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path());
    let bad = d.path().join("bad");
    std::fs::create_dir_all(&bad).unwrap();
    std::fs::write(bad.join("sdc.toml"), "this is = = not toml\n").unwrap();
    m.project = Some(sdc_gui::project::ProjectDoc::open(&bad));
    assert!(m.project.as_ref().unwrap().read_only());
    // The model refuses too.
    m.set_pack_fill(RMAOS, RawMap::Ao, Some(1));
    m.set_packing_recipe(RMAOS, PackRecipe::Rfaos);
    assert_eq!(
        m.project.as_ref().unwrap().file_value(RMAOS, "packing"),
        None
    );
    let mut h = editor(m);
    h.run();
    h.run();
    let combo = h
        .query_all_by_role(eframe::egui::accesskit::Role::ComboBox)
        .find(|n| {
            n.accesskit_node()
                .value()
                .is_some_and(|t| t.starts_with("RMAOS"))
        })
        .expect("the recipe selector");
    assert!(combo.accesskit_node().is_disabled());
    // The value box on each named fill row is disabled; the controls are found by their row's label.
    for name in ["metallic", "AO", "specular"] {
        let y = h.get_by_label(name).accesskit_node().raw_bounds().unwrap();
        let row: Vec<_> = h
            .query_all_by_role(eframe::egui::accesskit::Role::SpinButton)
            .filter(|n| {
                let b = n.accesskit_node().raw_bounds().unwrap();
                (b.y0 - y.y0).abs() < 6.0
            })
            .collect();
        assert_eq!(row.len(), 1, "one value box on the {name} row");
        assert!(row[0].accesskit_node().is_disabled(), "{name}");
    }
}

#[test]
fn the_recipe_selector_shows_the_chosen_recipe_with_auto() {
    let d = tempfile::tempdir().unwrap();
    let mut h = editor(fixture(d.path()));
    h.run();
    let shows = |h: &Harness<'static, AppModel>, text: &str| {
        h.query_all_by_role(eframe::egui::accesskit::Role::ComboBox)
            .any(|n| n.value().as_deref() == Some(text))
    };
    assert!(shows(&h, "RMAOS (auto)"));
    h.state_mut().set_packing_recipe(RMAOS, PackRecipe::Rfaos);
    settle(&mut h);
    h.run();
    assert!(shows(&h, "RFAOS (skin)"), "explicit: no (auto)");
}

#[test]
fn two_sets_with_different_ao_fills_pack_different_channels() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture_with(d.path(), "", true);
    m.set_pack_fill(RMAOS, RawMap::Ao, Some(10));
    m.set_pack_fill("m/rock02_rmaos", RawMap::Ao, Some(200));
    idle(&mut m);
    assert_eq!(channel(&m, RMAOS, "pack.b"), "fill:10");
    assert_eq!(channel(&m, "m/rock02_rmaos", "pack.b"), "fill:200");
}

#[test]
fn a_legacy_project_wide_fill_is_listed_applies_and_clears() {
    let d = tempfile::tempdir().unwrap();
    let m = fixture_with(d.path(), "[defaults.packing.fill]\nao = 10\n", false);
    assert_eq!(channel(&m, RMAOS, "pack.b"), "fill:10", "it still applies");
    assert_eq!(m.legacy_fills(), vec![(RawMap::Ao, 10)]);
    let mut m = m;
    m.settings.advanced_open = true;
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    h.get_by_label_contains("Project-wide fills (legacy): ao 10");
    assert!(h.query_by_label("Packing fill values").is_none());
    h.get_by_label("Clear").click();
    settle(&mut h);
    let m = h.state();
    assert!(m.legacy_fills().is_empty());
    assert!(!channel(m, RMAOS, "pack.b").starts_with("fill:"));
    assert!(h
        .query_by_label_contains("Project-wide fills (legacy)")
        .is_none());
    // One undo brings it back.
    h.state_mut().undo();
    settle(&mut h);
    assert_eq!(h.state().legacy_fills(), vec![(RawMap::Ao, 10)]);
}

#[test]
fn the_dropdown_has_an_advanced_skin_group_after_pbr_and_other_last() {
    use sdc_gui::ui::settings_panel::{subtype_groups, OTHER_HEADING, PBR_HEADING, SKIN_HEADING};
    let groups = subtype_groups(|_| true);
    assert_eq!(groups[0].0, PBR_HEADING);
    assert_eq!(groups[1].0, SKIN_HEADING);
    assert_eq!(SKIN_HEADING, "Advanced skin (Community Shaders)");
    assert_eq!(
        groups[1].1,
        vec![
            Subtype::SkinAlbedo,
            Subtype::SkinRfaos,
            Subtype::SkinSk,
            Subtype::SkinWet
        ]
    );
    let last = groups.last().unwrap();
    assert_eq!(
        (last.0, last.1.clone()),
        (OTHER_HEADING, vec![Subtype::UiSky, Subtype::Cubemap])
    );
    assert!(groups
        .iter()
        .filter(|g| g.0 != OTHER_HEADING)
        .all(|g| !g.1.contains(&Subtype::UiSky)));
    assert!(!groups[0].1.contains(&Subtype::SkinRfaos));
    let mut all: Vec<Subtype> = groups.iter().flat_map(|(_, v)| v.clone()).collect();
    all.sort_by_key(|s| s.id());
    all.dedup();
    assert_eq!(all.len(), Subtype::ALL.len(), "each exactly once");
}

#[test]
fn the_open_dropdown_draws_the_skin_group_after_pbr_and_other_last() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path());
    m.select_item(DIFFUSE);
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 1200.0))
        .build_ui_state(|ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    h.query_all_by_role(eframe::egui::accesskit::Role::ComboBox)
        .find(|n| n.value().as_deref() == Some("pbr albedo"))
        .expect("the sub-type combo")
        .click();
    h.run();
    h.run();
    let y = |label: &str| {
        h.get_by_label(label)
            .accesskit_node()
            .raw_bounds()
            .unwrap()
            .y0
    };
    assert!(y("PBR (Community Shaders)") < y("Advanced skin (Community Shaders)"));
    assert!(y("Advanced skin (Community Shaders)") < y("skin rfaos"));
    assert!(y("skin rfaos") < y("Colour maps"));
    assert!(y("Other") > y("Grayscale maps"));
    assert!(y("ui sky") > y("Other"));
}

fn note(m: &AppModel, key: &str) -> Option<String> {
    m.dropped_fills.get(key).cloned()
}

#[test]
fn a_dropped_fill_note_does_not_survive_reopening_the_folder() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path());
    m.set_pack_fill(RMAOS, RawMap::Metallic, Some(9));
    idle(&mut m);
    m.set_packing_recipe(RMAOS, PackRecipe::Rfaos);
    idle(&mut m);
    assert!(note(&m, RFAOS).unwrap().contains("metallic fill removed"));
    m.open_folder(d.path().to_path_buf());
    idle(&mut m);
    assert!(m.dropped_fills.is_empty());
}

#[test]
fn only_the_latest_switchs_notes_show() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path());
    m.set_pack_fill(RMAOS, RawMap::Metallic, Some(9));
    m.set_packing_recipe(RMAOS, PackRecipe::Rfaos);
    idle(&mut m);
    assert!(note(&m, RFAOS).is_some());
    m.set_pack_fill(RFAOS, RawMap::Fuzz, Some(30));
    idle(&mut m);
    m.set_packing_recipe(RFAOS, PackRecipe::Rmaos);
    idle(&mut m);
    assert!(note(&m, RFAOS).is_none(), "the old note is gone");
    assert!(note(&m, RMAOS).unwrap().contains("fuzz fill removed"));
    m.set_packing_recipe(RMAOS, PackRecipe::Rfaos);
    idle(&mut m);
    assert!(m.dropped_fills.is_empty(), "{:?}", m.dropped_fills);
}

#[test]
fn a_moved_fill_never_overwrites_the_destination_silently() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path());
    m.set_pack_fill(RMAOS, RawMap::Ao, Some(128));
    m.set_pack_fill(RFAOS, RawMap::Ao, Some(64));
    idle(&mut m);
    m.set_packing_recipe(RMAOS, PackRecipe::Rfaos);
    idle(&mut m);
    let p = m.project.as_ref().unwrap();
    assert_eq!(p.file_value(RFAOS, "packing.fill.ao"), Some(json!(64)));
    assert_eq!(p.file_value(RMAOS, "packing.fill.ao"), None);
    assert_eq!(
        note(&m, RFAOS).unwrap(),
        "ao fill 128 not moved: the _rfaos entry already had 64"
    );
}

#[test]
fn complex_m_has_glossiness_metallic_and_height_rows_and_vanilla_m_none() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path());
    grey(&d.path().join("m"), "rock03_metallic.png", 5);
    m.open_folder(d.path().to_path_buf());
    idle(&mut m);
    m.set_target(engine::config::Target::Vanilla);
    idle(&mut m);
    support::complex_material(&mut m, "m/rock01");
    assert_eq!(
        roles(&m, "m/rock01_m"),
        vec![RawMap::Glossiness, RawMap::Metallic, RawMap::Height]
    );
    assert!(m.item("m/rock03_m").is_some(), "vanilla builds the _m");
    assert!(roles(&m, "m/rock03_m").is_empty());
}

/// Final review I1: with a fuzz source `auto` builds `_rfaos` (Advanced skin); picking RMAOS builds `_rmaos` and the
/// inferred members go back to their PBR sub-types, and picking RFAOS again brings the skin ones back.
#[test]
fn a_fuzz_source_set_switched_to_rmaos_reverts_the_members() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("m")).unwrap();
    grey(&d.path().join("m"), "rock01_fuzz.png", 60);
    let mut m = fixture(d.path());
    assert!(m.item(RFAOS).is_some(), "auto builds _rfaos");
    assert_eq!(resolved(&m, DIFFUSE), Subtype::SkinAlbedo);
    assert_eq!(resolved(&m, SK), Subtype::SkinSk);
    m.set_packing_recipe(RFAOS, PackRecipe::Rmaos);
    idle(&mut m);
    assert!(m.item(RMAOS).is_some(), "the forced recipe builds _rmaos");
    assert_eq!(resolved(&m, DIFFUSE), Subtype::PbrAlbedo);
    assert_eq!(resolved(&m, SK), Subtype::BacklightTint);
    assert_eq!(subtype(&m, DIFFUSE), None);
    assert_eq!(subtype(&m, SK), None);
    m.set_packing_recipe(RMAOS, PackRecipe::Rfaos);
    idle(&mut m);
    assert!(m.item(RFAOS).is_some());
    assert_eq!(resolved(&m, DIFFUSE), Subtype::SkinAlbedo);
    assert_eq!(resolved(&m, SK), Subtype::SkinSk);
}

/// A pre-packed Advanced skin set: `rock01.png`, `rock01_rfaos.png`, `rock01_sk.png`.
fn skin_fixture(dir: &Path) -> AppModel {
    let r = dir.join("m");
    std::fs::create_dir_all(&r).unwrap();
    for name in ["rock01.png", "rock01_rfaos.png"] {
        ImageBuffer::from_fn(64, 64, |x, y| Rgb([(x * 4) as u8, (y * 4) as u8, 90u8]))
            .save(r.join(name))
            .unwrap();
    }
    grey(&r, "rock01_sk.png", 0);
    let settings = GuiSettings {
        export_folder: Some(dir.join("export")),
        ..GuiSettings::default()
    };
    let mut m = AppModel::new(settings, None, session());
    m.accept = engine::policy::AcceptSuggestions::None;
    m.open_folder(dir.to_path_buf());
    idle(&mut m);
    m
}

fn rock01_entry(m: &AppModel) -> Option<sdc_gui::pgp::writer::EntryInput> {
    json_entries(m)
        .into_iter()
        .find(|(v, _)| v.id == "m/rock01")
        .map(|(_, e)| e)
}

/// AG90 (owner, 2026-10-14, replaces final review C1): an Advanced skin set is not a PGPatcher JSON set.
#[test]
fn an_rfaos_set_is_a_skin_set_with_no_json_entry() {
    let d = tempfile::tempdir().unwrap();
    let m = skin_fixture(d.path());
    assert_eq!(resolved(&m, DIFFUSE), Subtype::SkinAlbedo);
    let v = set_views(&m);
    let v = v.iter().find(|v| v.id == "m/rock01").unwrap();
    assert!(!v.pbr && v.skin, "pbr {} skin {}", v.pbr, v.skin);
    assert!(rock01_entry(&m).is_none());
    // The packed file keeps its `_rfaos` name (never `_rmaos`).
    let out = &m
        .item("m/rock01_rfaos.png")
        .unwrap()
        .job
        .as_ref()
        .unwrap()
        .output;
    assert!(
        out.to_string_lossy().ends_with("rock01_rfaos.dds"),
        "{out:?}"
    );
    assert_eq!(export_blocks(&m), vec![]);
}

/// AG90 (owner, 2026-10-14, replaces final review C1): a skin set has no PGPatcher pitfalls or blocks, even with a
/// leftover JSON value.
#[test]
fn a_skin_set_with_emissive_on_and_no_g_is_not_blocked() {
    let d = tempfile::tempdir().unwrap();
    let mut m = skin_fixture(d.path());
    m.set_json_value("m/rock01", "emissive", Some(json!(true)));
    idle(&mut m);
    let b = export_blocks(&m);
    assert!(b.iter().all(|i| i.set_id != "m/rock01"), "{b:?}");
}

/// AG90: picking RFAOS on the card makes the set an Advanced skin set, which leaves the JSON; the badge says so.
#[test]
fn picking_rfaos_takes_the_set_out_of_the_json() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path());
    assert!(rock01_entry(&m).is_some());
    m.set_packing_recipe(RMAOS, PackRecipe::Rfaos);
    idle(&mut m);
    assert_eq!(resolved(&m, DIFFUSE), Subtype::SkinAlbedo);
    assert!(rock01_entry(&m).is_none());
    let mut h = editor(m);
    h.run();
    h.run();
    // Layout v2 A5 (K14): the card's pill says "skin" (its hover the long "Advanced skin (no JSON)").
    h.get_by_label("skin");
}

/// Owner 2026-10-27: making the diffuse Advanced skin switches the set's packed `_rmaos` to `_rfaos`, as one undo step.
#[test]
fn a_diffuse_made_skin_albedo_switches_the_packed_rmaos_to_rfaos() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path());
    assert!(m.item(RMAOS).is_some());
    let before = texts(&m);
    m.set_field(DIFFUSE, "subtype", json!(Subtype::SkinAlbedo));
    idle(&mut m);
    assert_eq!(resolved(&m, DIFFUSE), Subtype::SkinAlbedo);
    assert!(m.item(RFAOS).is_some(), "the packed output is _rfaos");
    assert!(m.item(RMAOS).is_none());
    let p = m.project.as_ref().unwrap();
    assert_eq!(p.file_value(RFAOS, "packing.recipe"), Some(json!("rfaos")));
    m.undo();
    idle(&mut m);
    assert_eq!(texts(&m), before, "one undo step");
    assert!(m.item(RMAOS).is_some());
}

/// Owner 2026-10-27: taking the diffuse off Advanced skin switches the packed `_rfaos` back to `_rmaos`.
#[test]
fn a_diffuse_taken_off_skin_albedo_switches_the_packed_rfaos_back() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path());
    m.set_packing_recipe(RMAOS, PackRecipe::Rfaos);
    idle(&mut m);
    assert_eq!(resolved(&m, DIFFUSE), Subtype::SkinAlbedo);
    assert!(m.item(RFAOS).is_some());
    m.set_field(DIFFUSE, "subtype", json!(Subtype::PbrAlbedo));
    idle(&mut m);
    assert_eq!(resolved(&m, DIFFUSE), Subtype::PbrAlbedo);
    assert!(m.item(RMAOS).is_some(), "the packed output is _rmaos again");
    assert!(m.item(RFAOS).is_none());
    let p = m.project.as_ref().unwrap();
    assert_eq!(p.file_value(RMAOS, "packing.recipe"), Some(json!("rmaos")));
    assert_eq!(p.file_value(RFAOS, "packing.recipe"), None);
}

/// The same edit on a set that cannot build `_rfaos` (metallic only) still sets the sub-type and keeps `_rmaos`.
#[test]
fn a_skin_albedo_diffuse_keeps_rmaos_when_rfaos_cannot_be_built() {
    let d = tempfile::tempdir().unwrap();
    let mut m = metallic_only(d.path());
    m.set_field(DIFFUSE, "subtype", json!(Subtype::SkinAlbedo));
    idle(&mut m);
    assert_eq!(subtype(&m, DIFFUSE), Some(json!(Subtype::SkinAlbedo)));
    assert!(m.item(RMAOS).is_some());
}

/// A metallic-only cs set: `_rmaos` from a metallic map, which `_rfaos` (no metallic channel) cannot be built from.
fn metallic_only(dir: &Path) -> AppModel {
    metallic_only_with(dir, "")
}

/// [`metallic_only`] with more `sdc.toml` text.
fn metallic_only_with(dir: &Path, extra: &str) -> AppModel {
    let r = dir.join("m");
    std::fs::create_dir_all(&r).unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Rgb([(x * 4) as u8, (y * 4) as u8, 90u8]))
        .save(r.join("rock01.png"))
        .unwrap();
    grey(&r, "rock01_metallic.png", 30);
    std::fs::write(
        dir.join("sdc.toml"),
        format!("[files.\"m/rock01_rmaos\".packing.fill]\nao = 99\n{extra}"),
    )
    .unwrap();
    let settings = GuiSettings {
        export_folder: Some(dir.join("export")),
        ..GuiSettings::default()
    };
    let mut m = AppModel::new(settings, None, session());
    m.accept = engine::policy::AcceptSuggestions::None;
    m.open_folder(dir.to_path_buf());
    idle(&mut m);
    m
}

const RFAOS_WHY: &str = "RFAOS needs a roughness, glossiness or AO source";

/// Final review I2: RFAOS cannot be built from a metallic-only set, so the model refuses it with the reason and
/// moves nothing (the fills stay on the planned `_rmaos` key).
#[test]
fn rfaos_on_a_metallic_only_set_is_refused_with_the_reason() {
    let d = tempfile::tempdir().unwrap();
    let mut m = metallic_only(d.path());
    assert!(m.item(RMAOS).is_some());
    assert_eq!(m.rfaos_unbuildable(RMAOS), Some(RFAOS_WHY));
    let before = texts(&m);
    assert_eq!(
        m.set_packing_recipe(RMAOS, PackRecipe::Rfaos),
        Some(RFAOS_WHY.to_string())
    );
    idle(&mut m);
    assert_eq!(texts(&m), before, "nothing written");
    assert!(m.item(RMAOS).is_some());
    assert!(m.unmatched_keys.is_empty(), "{:?}", m.unmatched_keys);
    assert!(m.dropped_fills.is_empty());
    // A set with a roughness source can build it.
    let d = tempfile::tempdir().unwrap();
    let m = fixture(d.path());
    assert_eq!(m.rfaos_unbuildable(RMAOS), None);
}

/// Final review I2: the card offers RFAOS disabled on a metallic-only set.
#[test]
fn the_rfaos_option_is_disabled_on_a_metallic_only_set() {
    let d = tempfile::tempdir().unwrap();
    let mut h = editor(metallic_only(d.path()));
    h.run();
    h.run();
    h.query_all_by_role(eframe::egui::accesskit::Role::ComboBox)
        .find(|n| n.value().as_deref() == Some("RMAOS (auto)"))
        .expect("the recipe selector")
        .click();
    h.run();
    h.run();
    let rfaos = h.get_by_label("RFAOS (skin)");
    assert!(rfaos.accesskit_node().is_disabled());
    rfaos.hover();
    h.run();
    h.run();
    h.get_by_label(RFAOS_WHY);
    // The RMAOS option in the open list (the slot card is labelled "RMAOS" too, but is no button).
    let rmaos = h
        .query_all_by_label("RMAOS")
        .find(|n| n.accesskit_node().role() == eframe::egui::accesskit::Role::Button)
        .expect("the RMAOS option");
    assert!(!rmaos.accesskit_node().is_disabled());
}

/// Final review I2: a hand-written `rfaos` the engine could not follow leaves the card on RMAOS; picking RMAOS
/// still writes it, which clears the stale value.
#[test]
fn picking_rmaos_clears_a_stale_rfaos() {
    let d = tempfile::tempdir().unwrap();
    let m = metallic_only_with(
        d.path(),
        "\n[files.\"m/rock01_rfaos\".packing]\nrecipe = \"rfaos\"\n",
    );
    assert!(m.item(RMAOS).is_some(), "unbuildable: still _rmaos");
    assert_eq!(m.explicit_recipe(RMAOS), PackRecipe::Rfaos);
    let mut h = editor(m);
    h.run();
    h.query_all_by_role(eframe::egui::accesskit::Role::ComboBox)
        .find(|n| n.value().as_deref() == Some("RMAOS"))
        .expect("the recipe selector, no (auto)")
        .click();
    h.run();
    h.run();
    // K11: the Sets column's "RMAOS" chip is a button too.
    h.query_all_by_label("RMAOS")
        .find(|n| {
            n.accesskit_node().role() == eframe::egui::accesskit::Role::Button
                && !support::in_sets_column(&h, n.rect())
        })
        .expect("the RMAOS option")
        .click();
    settle(&mut h);
    let p = h.state().project.as_ref().unwrap();
    assert_eq!(p.file_value(RMAOS, "packing.recipe"), Some(json!("rmaos")));
    assert_eq!(p.file_value(RFAOS, "packing.recipe"), None);
}

/// Final review I2: re-picking the recipe an `auto` card already built writes nothing (the set keeps following
/// `auto`).
#[test]
fn re_picking_the_auto_recipe_writes_nothing() {
    let d = tempfile::tempdir().unwrap();
    let m = fixture(d.path());
    let before = texts(&m);
    let mut h = editor(m);
    h.run();
    h.query_all_by_role(eframe::egui::accesskit::Role::ComboBox)
        .find(|n| n.value().as_deref() == Some("RMAOS (auto)"))
        .expect("the recipe selector")
        .click();
    h.run();
    h.run();
    // K11: the Sets column's "RMAOS" chip is a button too.
    h.query_all_by_label("RMAOS")
        .find(|n| {
            n.accesskit_node().role() == eframe::egui::accesskit::Role::Button
                && !support::in_sets_column(&h, n.rect())
        })
        .expect("the RMAOS option")
        .click();
    settle(&mut h);
    assert_eq!(texts(h.state()), before);
}

#[test]
fn dragging_a_fill_box_and_releasing_stores_the_final_dragged_value() {
    let d = tempfile::tempdir().unwrap();
    let mut h = editor(fixture(d.path()));
    h.run();
    let start = fill_rows(h.state(), RMAOS)
        .into_iter()
        .find_map(|r| match r {
            FillRow::Value {
                role: RawMap::Ao,
                shown,
                ..
            } => Some(shown),
            _ => None,
        })
        .expect("an AO value row");
    // Note DY18: the source picker widens the row, so the box may sit past the column's edge: scroll to it.
    h.get_by_label("AO").scroll_to_me();
    h.run();
    let label = h.get_by_label("AO").accesskit_node().raw_bounds().unwrap();
    let spin = h
        .query_all_by_role(eframe::egui::accesskit::Role::SpinButton)
        .find(|n| (n.accesskit_node().raw_bounds().unwrap().y0 - label.y0).abs() < 6.0)
        .expect("the AO value box");
    spin.scroll_to_me();
    h.run();
    let label = h.get_by_label("AO").accesskit_node().raw_bounds().unwrap();
    let r#box = h
        .query_all_by_role(eframe::egui::accesskit::Role::SpinButton)
        .find(|n| (n.accesskit_node().raw_bounds().unwrap().y0 - label.y0).abs() < 6.0)
        .expect("the AO value box")
        .rect();
    // DragValue speed 1.0: one step per pixel. Drag 40 px left in 10 px moves, then release.
    let p = r#box.center();
    h.hover_at(p);
    h.drag_at(p);
    h.step();
    for i in 1..=4 {
        h.hover_at(p - eframe::egui::vec2(10.0 * i as f32, 0.0));
        h.step();
    }
    h.drop_at(p - eframe::egui::vec2(40.0, 0.0));
    settle(&mut h);
    let want = start.saturating_sub(40);
    assert_ne!(want, start, "the fixture's AO must be able to move down");
    assert_eq!(
        h.state()
            .project
            .as_ref()
            .unwrap()
            .file_value(RMAOS, "packing.fill.ao"),
        Some(json!(want)),
        "the released drag's final value is the fill"
    );
}

/// Note DR: with a `_f` in the set, making the diffuse skin_albedo (Note DQ) fills the RFAOS card's fuzz row from it.
#[test]
fn a_skin_f_is_the_rfaos_cards_fuzz_file() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("m")).unwrap();
    grey(&d.path().join("m"), "rock01_f.png", 200);
    let mut m = fixture(d.path());
    assert!(
        fill_rows(&m, RMAOS).iter().all(|r| !matches!(
            r,
            FillRow::FromFile {
                role: RawMap::Fuzz,
                ..
            }
        )),
        "_f is not a fuzz source before the set is skin"
    );
    m.set_field(DIFFUSE, "subtype", json!(Subtype::SkinAlbedo));
    idle(&mut m);
    assert!(m.item(RFAOS).is_some(), "the packed output is _rfaos");
    let rows = fill_rows(&m, RFAOS);
    assert!(
        rows.iter().any(|r| matches!(r, FillRow::FromFile { role: RawMap::Fuzz, file, .. } if file == "rock01_f.png")),
        "{rows:?}"
    );
}
