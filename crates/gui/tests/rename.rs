//! M6 Track F3b Task 3: the rename plan (spec §4), Apply as one step and one plan (§7), and the menu entries (§6).
mod support;
use image::{ImageBuffer, Rgb};
use sdc_gui::model::bulk::MenuAction;
use sdc_gui::model::rename::*;
use sdc_gui::model::*;
use std::path::{Path, PathBuf};
use std::time::Duration;
use support::*;

const ROCK: &str = "landscape/rock01.png";
const LEAF: &str = "landscape/leaf.png";
const RMAOS: &str = "landscape/rock01_rmaos";

fn idle(m: &mut AppModel) {
    assert!(
        m.wait_idle(Duration::from_secs(120)),
        "planning did not finish"
    );
}

fn dialog(m: &mut AppModel) -> &mut RenameDialog {
    match m.dialog.as_mut() {
        Some(Dialog::Rename(d)) => d,
        other => panic!("expected the rename dialog, got {other:?}"),
    }
}

fn plan(m: &AppModel) -> RenamePlan {
    m.rename_plan().expect("the rename dialog is open")
}

fn row<'a>(p: &'a RenamePlan, key: &str) -> &'a RenameRow {
    p.rows
        .iter()
        .find(|r| r.key == key)
        .unwrap_or_else(|| panic!("no row {key} in {:?}", p.rows))
}

fn output_of(m: &AppModel, key: &str) -> String {
    m.item(key)
        .and_then(|i| i.job.as_ref())
        .map(|j| j.output.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default()
}

fn set_id_of(m: &AppModel, item: &str) -> String {
    sdc_gui::model::sets::views(m)
        .iter()
        .find(|v| v.members.iter().any(|k| k == item))
        .expect("the item's set")
        .id
        .clone()
}

fn set_key_of(m: &AppModel, item: &str) -> String {
    sdc_gui::model::sets::views(m)
        .iter()
        .find(|v| v.members.iter().any(|k| k == item))
        .expect("the item's set")
        .key
        .clone()
}

#[test]
fn an_input_alias_is_written_read_and_undone_in_one_step() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.click_item(LEAF, false, false);
    m.run_menu(MenuAction::RenameInputs);
    dialog(&mut m)
        .typed
        .insert(LEAF.into(), "landscape/leaf_n".into());
    let p = plan(&m);
    let r = row(&p, LEAF);
    assert_eq!(r.new, "landscape/leaf_n.png", "the real extension is added");
    assert_eq!(r.kind, Some(("colour".into(), "_n".into())));
    assert_eq!(r.output, "landscape/leaf_n.dds");
    assert!(r.warning.as_deref().unwrap_or("").contains("_n"));
    assert!(p.can_apply());
    let (steps, generation) = (m.history.undo.len(), m.generation());
    assert!(m.apply_rename());
    assert_eq!(m.dialog, None);
    assert_eq!(m.history.undo.len(), steps + 1, "one undo step");
    assert_eq!(m.generation(), generation + 1, "one plan");
    idle(&mut m);
    assert_eq!(m.read_as_of(LEAF).as_deref(), Some("landscape/leaf_n.png"));
    assert!(
        output_of(&m, LEAF).ends_with("export/landscape/leaf_n.dds"),
        "{}",
        output_of(&m, LEAF)
    );
    assert!(d.path().join("textures/landscape/leaf.png").exists());
    m.undo();
    idle(&mut m);
    assert_eq!(m.read_as_of(LEAF), None);
    assert!(output_of(&m, LEAF).ends_with("export/landscape/leaf.dds"));
}

#[test]
fn the_recipe_runs_find_regex_prefix_suffix_and_case_on_the_base_only() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.click_item(LEAF, false, false);
    m.open_rename(RenameMode::Inputs);
    dialog(&mut m).recipe = Recipe {
        find: r"^(\w)(\w+)$".into(),
        replace: "${2}$1".into(),
        regex: true,
        prefix: "p_".into(),
        suffix: "_s".into(),
        case: Case::Upper,
    };
    let p = plan(&m);
    assert_eq!(row(&p, LEAF).new, "landscape/P_EAFL_S.png");
    assert_eq!(p.matched(), (1, 1));
    // A literal find that does not match leaves the base as it is.
    dialog(&mut m).recipe = Recipe {
        find: "zzz".into(),
        replace: "y".into(),
        ..Recipe::default()
    };
    let p = plan(&m);
    assert_eq!(row(&p, LEAF).new, LEAF);
    assert_eq!(p.matched(), (0, 1));
    assert!(!p.can_apply(), "nothing changes");
}

#[test]
fn a_suffix_on_the_name_is_never_touched_by_the_recipe() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.click_item("landscape/rock01_normal.png", false, false);
    m.open_rename(RenameMode::Inputs);
    dialog(&mut m).recipe = Recipe {
        prefix: "big_".into(),
        ..Recipe::default()
    };
    let p = plan(&m);
    let r = row(&p, "landscape/rock01_normal.png");
    assert_eq!(r.new, "landscape/big_rock01_normal.png");
    assert_eq!(r.kind, Some(("_normal".into(), "_normal".into())));
    assert_eq!(r.warning, None);
}

#[test]
fn an_invalid_pattern_disables_apply() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.click_item(LEAF, false, false);
    m.open_rename(RenameMode::Inputs);
    dialog(&mut m).recipe = Recipe {
        find: "(".into(),
        regex: true,
        ..Recipe::default()
    };
    let p = plan(&m);
    assert!(p.error.as_deref().unwrap_or("").starts_with("pattern:"));
    assert!(!p.can_apply());
    assert!(!m.apply_rename());
}

#[test]
fn input_names_that_collide_or_windows_refuses_block_apply() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.click_item(LEAF, false, false);
    m.click_item(ROCK, true, false);
    m.open_rename(RenameMode::Inputs);
    for (typed, why) in [
        ("landscape/odd.png", "already has this name"),
        ("landscape/CON.png", "reserves"),
        ("../leaf.png", ".."),
        ("landscape/leaf2.tga", "extension"),
        ("landscape/x?.png", "Windows"),
    ] {
        dialog(&mut m).typed.insert(LEAF.into(), typed.into());
        let p = plan(&m);
        let r = row(&p, LEAF);
        assert!(
            r.problem.as_deref().unwrap_or("").contains(why),
            "{typed}: {:?}",
            r.problem
        );
        assert!(!p.can_apply(), "{typed}");
    }
    // Two rows onto one name: the second is refused.
    let d2 = dialog(&mut m);
    d2.typed.insert(LEAF.into(), "landscape/same.png".into());
    d2.typed.insert(ROCK.into(), "landscape/same.png".into());
    let p = plan(&m);
    let problems = p.rows.iter().filter(|r| r.problem.is_some()).count();
    assert_eq!(problems, 1, "{:?}", p.rows);
    // Excluding the refused row lets the other apply.
    let refused = p
        .rows
        .iter()
        .find(|r| r.problem.is_some())
        .unwrap()
        .key
        .clone();
    dialog(&mut m).exclude.insert(refused);
    assert!(plan(&m).can_apply());
}

#[test]
fn packed_outputs_and_locked_slots_are_skipped_in_inputs_mode() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    let set = set_id_of(&m, ROCK);
    let diffuse = sdc_gui::model::sets::SlotKind::Diffuse.slot_number();
    m.set_slot_lock(&set, diffuse, true).unwrap();
    idle(&mut m);
    m.click_set(&set_key_of(&m, ROCK), false, false);
    m.open_rename(RenameMode::Inputs);
    dialog(&mut m).recipe.prefix = "x_".into();
    let p = plan(&m);
    assert_eq!(row(&p, ROCK).skipped.as_deref(), Some("skipped (locked)"));
    assert!(row(&p, RMAOS)
        .skipped
        .as_deref()
        .unwrap_or("")
        .contains("packed"));
    assert!(!row(&p, ROCK).writes());
    assert!(row(&p, "landscape/rock01_normal.png").writes());
}

#[test]
fn outputs_mode_names_a_set_and_blocks_collisions_and_suffixes() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let leaf = set_id_of(&m, LEAF);
    m.click_item(LEAF, false, false);
    m.run_menu(MenuAction::RenameOutputs);
    assert_eq!(dialog(&mut m).keys, vec![leaf.clone()]);
    dialog(&mut m).typed.insert(leaf.clone(), "rock01".into());
    let p = plan(&m);
    assert!(row(&p, &leaf)
        .problem
        .as_deref()
        .unwrap_or("")
        .contains("already writes"));
    dialog(&mut m).typed.insert(leaf.clone(), "maple_n".into());
    assert!(row(&plan(&m), &leaf)
        .problem
        .as_deref()
        .unwrap_or("")
        .contains("suffix"));
    dialog(&mut m).typed.clear();
    dialog(&mut m).recipe.prefix = "big_".into();
    let p = plan(&m);
    assert_eq!(row(&p, &leaf).old, "leaf");
    assert_eq!(row(&p, &leaf).new, "big_leaf");
    assert_eq!(row(&p, &leaf).output, "landscape/big_leaf.dds");
    let steps = m.history.undo.len();
    assert!(m.apply_rename());
    assert_eq!(m.history.undo.len(), steps + 1);
    idle(&mut m);
    assert_eq!(m.output_name_of(&leaf).as_deref(), Some("big_leaf"));
    assert!(output_of(&m, LEAF).ends_with("export/landscape/big_leaf.dds"));
    assert_eq!(m.last_recipe.prefix, "big_", "the recipe is kept");
}

#[test]
fn clear_input_alias_is_offered_only_when_one_is_selected() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.click_item(LEAF, false, false);
    assert!(!m
        .item_menu()
        .iter()
        .any(|e| e.action == MenuAction::ClearInputAlias));
    m.open_rename(RenameMode::Inputs);
    dialog(&mut m)
        .typed
        .insert(LEAF.into(), "landscape/leaf_n".into());
    assert!(m.apply_rename());
    idle(&mut m);
    assert!(m
        .item_menu()
        .iter()
        .any(|e| e.action == MenuAction::ClearInputAlias));
    m.run_menu(MenuAction::ClearInputAlias);
    idle(&mut m);
    assert_eq!(m.read_as_of(LEAF), None);
}

#[test]
fn settings_stay_on_the_real_file_across_an_alias() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.decide(LEAF, Answer::Subtype(engine::config::Subtype::Diffuse));
    idle(&mut m);
    let before = m.project.as_ref().unwrap().file_value(LEAF, "subtype");
    assert!(before.is_some());
    m.click_item(LEAF, false, false);
    m.open_rename(RenameMode::Inputs);
    dialog(&mut m)
        .typed
        .insert(LEAF.into(), "landscape/maple.png".into());
    assert!(m.apply_rename());
    idle(&mut m);
    let job = m
        .item(LEAF)
        .and_then(|i| i.job.as_ref())
        .expect("still planned under its key");
    assert_eq!(job.profile.subtype, engine::config::Subtype::Diffuse);
    assert_eq!(
        m.project.as_ref().unwrap().file_value(LEAF, "subtype"),
        before
    );
}

/// `n` small colour textures under `textures/many`, each its own set.
fn many(dir: &Path, n: usize) -> PathBuf {
    let r = dir.join("textures/many");
    std::fs::create_dir_all(&r).unwrap();
    for i in 0..n {
        ImageBuffer::from_fn(8, 8, |x, y| Rgb([(x * 30) as u8, (y * 30) as u8, i as u8]))
            .save(r.join(format!("t{i:03}.png")))
            .unwrap();
    }
    dir.join("textures")
}

#[test]
fn renaming_300_outputs_is_one_step_and_one_plan() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened_as_the_app(many(d.path(), 300), d.path().join("export"));
    m.select_all_visible();
    m.open_rename(RenameMode::Outputs);
    assert_eq!(dialog(&mut m).keys.len(), 300);
    dialog(&mut m).recipe.prefix = "x_".into();
    let (steps, generation) = (m.history.undo.len(), m.generation());
    assert!(m.apply_rename());
    assert_eq!(m.history.undo.len(), steps + 1, "one undo step");
    assert_eq!(m.generation(), generation + 1, "one plan");
}

fn pbr_settings_of(m: &AppModel, id: &str) -> Option<serde_json::Value> {
    sdc_gui::model::pbr_export::json_entries(m)
        .into_iter()
        .find(|(v, _)| v.id == id)
        .map(|(v, _)| serde_json::json!(v.settings))
}

/// Trace-debug 2026-10-16 #2: an Inputs rename moves the set's sdc-project.toml entry to the new automatic id, as the
/// input name box does (AB1), in the same undo step.
#[test]
fn rename_inputs_moves_set_entry_with_automatic_id() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    let set = set_id_of(&m, ROCK);
    assert_eq!(set, "landscape/rock01");
    m.set_json_value(&set, "specular_level", Some(serde_json::json!(0.5)));
    idle(&mut m);
    m.click_set(&set_key_of(&m, ROCK), false, false);
    m.open_rename(RenameMode::Inputs);
    dialog(&mut m).recipe.prefix = "steel_".into();
    let steps = m.history.undo.len();
    assert!(m.apply_rename());
    assert_eq!(m.history.undo.len(), steps + 1, "one undo step");
    idle(&mut m);
    assert_eq!(set_id_of(&m, ROCK), "landscape/steel_rock01");
    let s = pbr_settings_of(&m, "landscape/steel_rock01").expect("the set under its new id");
    assert_eq!(s["specular_level"], serde_json::json!(0.5));
    assert!(
        sdc_gui::model::sets::views(&m)
            .iter()
            .all(|v| v.id != "landscape/rock01"),
        "no set is left behind under the old id"
    );
    m.undo();
    idle(&mut m);
    assert_eq!(set_id_of(&m, ROCK), "landscape/rock01");
    let s = pbr_settings_of(&m, "landscape/rock01").expect("the set under its old id");
    assert_eq!(s["specular_level"], serde_json::json!(0.5));
    assert!(sdc_gui::model::sets::views(&m)
        .iter()
        .all(|v| v.id != "landscape/steel_rock01"));
}

/// Review I1: with a read-only sdc.toml the aliases cannot be written, so the set entry must not move either.
#[test]
fn rename_inputs_on_a_read_only_sdc_toml_moves_nothing() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    let set = set_id_of(&m, ROCK);
    m.set_json_value(&set, "specular_level", Some(serde_json::json!(0.5)));
    idle(&mut m);
    m.click_set(&set_key_of(&m, ROCK), false, false);
    m.open_rename(RenameMode::Inputs);
    dialog(&mut m).recipe.prefix = "steel_".into();
    let bad = d.path().join("bad");
    std::fs::create_dir_all(&bad).unwrap();
    std::fs::write(bad.join("sdc.toml"), "this is = = not toml\n").unwrap();
    m.project = Some(sdc_gui::project::ProjectDoc::open(&bad));
    let before = m.sets_doc.as_ref().unwrap().text();
    assert!(!m.apply_rename());
    assert_eq!(m.sets_doc.as_ref().unwrap().text(), before);
}

/// Review M1: when the new id already has an entry, the old one stays where it is and the user is told.
#[test]
fn rename_inputs_onto_an_existing_entry_says_the_entry_was_kept() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    let set = set_id_of(&m, ROCK);
    m.set_json_value(&set, "specular_level", Some(serde_json::json!(0.5)));
    m.set_json_value(
        "landscape/steel_rock01",
        "specular_level",
        Some(serde_json::json!(0.25)),
    );
    idle(&mut m);
    m.click_set(&set_key_of(&m, ROCK), false, false);
    m.open_rename(RenameMode::Inputs);
    dialog(&mut m).recipe.prefix = "steel_".into();
    assert!(m.apply_rename());
    idle(&mut m);
    assert!(
        m.messages
            .entries()
            .any(|e| e.text.contains("landscape/rock01") && e.text.contains("kept")),
        "{:?}",
        m.messages
            .entries()
            .map(|e| e.text.clone())
            .collect::<Vec<_>>()
    );
}
