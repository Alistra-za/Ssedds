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
        path: false,
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
fn two_inputs_renamed_to_one_output_block_apply() {
    let d = tempfile::tempdir().unwrap();
    let r = mod_folder(d.path()).join("landscape");
    ImageBuffer::from_fn(8, 8, |x, y| Rgb([x as u8, y as u8, 9u8]))
        .save(r.join("a.png"))
        .unwrap();
    ImageBuffer::from_fn(8, 8, |x, y| Rgb([y as u8, x as u8, 9u8]))
        .save(r.join("b.tga"))
        .unwrap();
    let mut m = opened(d.path());
    let (a, b) = ("landscape/a.png", "landscape/b.tga");
    m.click_item(a, false, false);
    m.click_item(b, true, false);
    m.open_rename(RenameMode::Inputs);
    let d2 = dialog(&mut m);
    d2.typed.insert(a.into(), "landscape/x.png".into());
    d2.typed.insert(b.into(), "landscape/x.tga".into());
    let p = plan(&m);
    // Different names, one x.dds: the second row is refused.
    let bad: Vec<_> = p.rows.iter().filter(|r| r.problem.is_some()).collect();
    assert_eq!(bad.len(), 1, "{:?}", p.rows);
    assert!(
        bad[0].problem.as_deref().unwrap().contains("x.dds"),
        "{:?}",
        bad[0].problem
    );
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

/// Trace-debug 2026-10-16 #19: an input renamed onto the path another set writes (through its `output_name`) blocks,
/// or both textures are skipped as IN-DUPLICATE.
#[test]
fn an_input_rename_onto_another_sets_output_blocks_apply() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let leaf = set_id_of(&m, LEAF);
    m.set_output_name(&leaf, Some("steel")).unwrap();
    idle(&mut m);
    assert!(output_of(&m, LEAF).ends_with("export/landscape/steel.dds"));
    m.click_item(ROCK, false, false);
    m.open_rename(RenameMode::Inputs);
    dialog(&mut m)
        .typed
        .insert(ROCK.into(), "landscape/steel.png".into());
    let p = plan(&m);
    let r = row(&p, ROCK);
    assert!(
        r.problem
            .as_deref()
            .unwrap_or("")
            .contains("already writes"),
        "{:?}",
        r.problem
    );
    assert!(!p.can_apply());
}

/// Trace-debug 2026-10-16 #35: a case-only rename is previewed and enabled, so Apply stores it as an alias.
#[test]
fn a_case_only_rename_is_stored_as_an_alias() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.click_item(LEAF, false, false);
    m.open_rename(RenameMode::Inputs);
    dialog(&mut m).recipe.case = Case::Upper;
    let p = plan(&m);
    assert_eq!(row(&p, LEAF).new, "landscape/LEAF.png");
    assert!(p.can_apply());
    let steps = m.history.undo.len();
    assert!(m.apply_rename());
    assert_eq!(m.history.undo.len(), steps + 1);
    idle(&mut m);
    assert_eq!(
        m.project.as_ref().unwrap().read_as(LEAF).as_deref(),
        Some("landscape/LEAF.png")
    );
    // Back to its own name: the alias goes.
    m.open_rename(RenameMode::Inputs);
    dialog(&mut m).recipe.case = Case::Lower;
    assert!(m.apply_rename());
    idle(&mut m);
    assert_eq!(m.project.as_ref().unwrap().read_as(LEAF), None);
}

/// Owner 2026-10-19: Whole path runs the recipe over the folders and the base name; the map suffix and the
/// extension stay. Off, Find never sees the folders.
#[test]
fn whole_path_renames_folders_in_bulk() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.click_item(ROCK, false, false);
    m.click_item(LEAF, true, false);
    m.run_menu(MenuAction::RenameInputs);
    dialog(&mut m).recipe.find = "landscape/".into();
    dialog(&mut m).recipe.replace = "stone/".into();
    let p = plan(&m);
    assert_eq!(
        p.matched().0,
        0,
        "base name only: the folder is not searched"
    );
    assert_eq!(row(&p, ROCK).new, ROCK);
    dialog(&mut m).recipe.path = true;
    let p = plan(&m);
    assert_eq!(p.matched(), (2, 2));
    assert_eq!(row(&p, ROCK).new, "stone/rock01.png");
    assert_eq!(row(&p, LEAF).new, "stone/leaf.png");
    assert!(p.can_apply());
    let r = dialog(&mut m);
    r.recipe.find.clear();
    r.recipe.replace.clear();
    r.recipe.prefix = "src/".into();
    r.recipe.suffix = "_old".into();
    let p = plan(&m);
    assert_eq!(
        row(&p, ROCK).new,
        "src/landscape/rock01_old.png",
        "prefix before the folders, suffix before the extension"
    );
}

// ---- Task 2 (Note FA): pins of what the plan and the menu keys give today, before they were made linear.

/// Every field of every row, one line per row (paths with `/`).
fn project(p: &RenamePlan) -> Vec<String> {
    p.rows
        .iter()
        .map(|r| {
            format!(
                "{}|{}|{}|{:?}|{}|{}|{}|{:?}|{:?}|{:?}",
                r.key,
                r.old,
                r.new,
                r.kind,
                r.output,
                r.included,
                r.matched,
                r.problem,
                r.warning,
                r.skipped
            )
            .replace("\\\\", "/")
        })
        .collect()
}

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn rename_keys_on_the_fixture_follow_the_selection() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.select_all_visible();
    assert_eq!(
        m.rename_keys(RenameMode::Inputs),
        strs(&[
            "landscape/leaf.png",
            "landscape/odd.png",
            "landscape/rock01.png",
            "landscape/rock01_normal.png",
            "landscape/rock01_rmaos"
        ])
    );
    assert_eq!(
        m.rename_keys(RenameMode::Outputs),
        strs(&["landscape/leaf", "landscape/odd", "landscape/rock01"])
    );
    // Two textures of one set give that set once; the order is the selection's.
    m.click_item("landscape/rock01_normal.png", false, false);
    m.click_item(ROCK, true, false);
    m.click_item(LEAF, true, false);
    assert_eq!(
        m.rename_keys(RenameMode::Inputs),
        strs(&["landscape/rock01_normal.png", ROCK, LEAF])
    );
    assert_eq!(
        m.rename_keys(RenameMode::Outputs),
        strs(&["landscape/rock01", "landscape/leaf"])
    );
    // Selected sets: Inputs takes their members (a set's own order), Outputs the set ids.
    let (rock_set, leaf_set) = (set_key_of(&m, ROCK), set_key_of(&m, LEAF));
    m.click_set(&leaf_set, false, false);
    m.click_set(&rock_set, true, false);
    assert_eq!(
        m.rename_keys(RenameMode::Inputs),
        strs(&[
            "landscape/leaf.png",
            "landscape/rock01.png",
            "landscape/rock01_ao.png",
            "landscape/rock01_normal.png",
            "landscape/rock01_rmaos",
            "landscape/rock01_roughness.png"
        ])
    );
    assert_eq!(
        m.rename_keys(RenameMode::Outputs),
        strs(&["landscape/leaf", "landscape/rock01"])
    );
}

#[test]
fn rename_keys_skip_textures_that_are_in_no_set_in_outputs_mode() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let leaf_set = set_key_of(&m, LEAF);
    m.sets.retain(|s| s.key != leaf_set);
    m.invalidate_views();
    m.click_item(LEAF, false, false);
    m.click_item(ROCK, true, false);
    assert_eq!(m.rename_keys(RenameMode::Inputs), strs(&[LEAF, ROCK]));
    assert_eq!(
        m.rename_keys(RenameMode::Outputs),
        strs(&["landscape/rock01"])
    );
    m.click_item(LEAF, false, false);
    assert_eq!(m.rename_keys(RenameMode::Outputs), Vec::<String>::new());
}

#[test]
fn rename_keys_over_sets_that_share_members_and_a_texture_in_two_sets() {
    let d = tempfile::tempdir().unwrap();
    let mut m = many_sets(d.path(), 3);
    let keys: Vec<String> = sdc_gui::model::sets::views(&m)
        .iter()
        .map(|v| v.key.clone())
        .collect();
    // Two sets with the same member: Inputs names it once.
    m.click_set(&keys[2], false, false);
    m.click_set(&keys[0], true, false);
    assert_eq!(m.rename_keys(RenameMode::Inputs), strs(&[LEAF]));
    assert_eq!(
        m.rename_keys(RenameMode::Outputs),
        strs(&["landscape/leaf0002", "landscape/leaf0000"])
    );
    // A texture in all three sets: the first set in display order is its set (Outputs); two of them give it once.
    m.click_item(LEAF, false, false);
    assert_eq!(
        m.rename_keys(RenameMode::Outputs),
        strs(&["landscape/leaf0000"])
    );
    m.click_item(ROCK, true, false);
    assert_eq!(m.rename_keys(RenameMode::Inputs), strs(&[LEAF, ROCK]));
    assert_eq!(
        m.rename_keys(RenameMode::Outputs),
        strs(&["landscape/leaf0000"])
    );
}

#[test]
fn aliased_selection_keeps_the_selection_order() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.click_item(LEAF, false, false);
    m.click_item(ROCK, true, false);
    m.open_rename(RenameMode::Inputs);
    let d2 = dialog(&mut m);
    d2.typed.insert(LEAF.into(), "landscape/maple.png".into());
    d2.typed.insert(ROCK.into(), "landscape/stone.png".into());
    assert!(m.apply_rename());
    idle(&mut m);
    m.click_item("landscape/odd.png", false, false);
    m.click_item(ROCK, true, false);
    m.click_item(LEAF, true, false);
    assert_eq!(m.aliased_selection(), strs(&[ROCK, LEAF]));
    m.click_item(LEAF, false, false);
    m.click_item("landscape/odd.png", true, false);
    assert_eq!(m.aliased_selection(), strs(&[LEAF]));
    m.click_item("landscape/odd.png", false, false);
    assert_eq!(m.aliased_selection(), Vec::<String>::new());
}

#[test]
fn the_plan_rows_and_collision_messages_are_pinned() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.select_all_visible();
    m.open_rename(RenameMode::Inputs);
    dialog(&mut m).recipe.prefix = "x_".into();
    assert_eq!(
        project(&plan(&m)),
        strs(&[
            "landscape/leaf.png|landscape/leaf.png|landscape/x_leaf.png|Some((\"colour\", \"colour\"))|landscape/x_leaf.dds|true|true|None|None|None",
            "landscape/odd.png|landscape/odd.png|landscape/x_odd.png|Some((\"colour\", \"colour\"))|landscape/x_odd.dds|true|true|None|None|None",
            "landscape/rock01.png|landscape/rock01.png|landscape/x_rock01.png|Some((\"colour\", \"colour\"))|landscape/x_rock01.dds|true|true|None|None|None",
            "landscape/rock01_normal.png|landscape/rock01_normal.png|landscape/x_rock01_normal.png|Some((\"_normal\", \"_normal\"))|landscape/x_rock01_n.dds|true|true|None|None|None",
            "landscape/rock01_rmaos|landscape/rock01_rmaos|landscape/rock01_rmaos|None||true|false|None|None|Some(\"skipped (a packed output has no file of its own)\")",
        ])
    );
    m.open_rename(RenameMode::Outputs);
    dialog(&mut m).recipe.prefix = "x_".into();
    assert_eq!(
        project(&plan(&m)),
        strs(&[
            "landscape/leaf|leaf|x_leaf|None|landscape/x_leaf.dds|true|true|None|None|None",
            "landscape/odd|odd|x_odd|None|landscape/x_odd.dds|true|true|None|None|None",
            "landscape/rock01|rock01|x_rock01|None|landscape/x_rock01.dds (+2 more)|true|true|None|None|None",
        ])
    );
    // Outputs: two sets onto one name (the second is refused), and a set onto a name a selected set gives up.
    let (leaf, odd) = (set_id_of(&m, LEAF), set_id_of(&m, "landscape/odd.png"));
    let rock = set_id_of(&m, ROCK);
    dialog(&mut m).recipe.prefix.clear();
    let d2 = dialog(&mut m);
    d2.typed.insert(leaf.clone(), "same".into());
    d2.typed.insert(odd.clone(), "same".into());
    d2.typed.insert(rock.clone(), "leaf".into());
    assert_eq!(
        project(&plan(&m)),
        strs(&[
            "landscape/leaf|leaf|same|None|landscape/same.dds|true|true|None|None|None",
            "landscape/odd|odd|same|None|landscape/same.dds|true|true|Some(\"landscape/leaf is renamed onto landscape/same.dds too\")|None|None",
            "landscape/rock01|rock01|leaf|None|landscape/leaf.dds (+2 more)|true|true|None|None|None",
        ])
    );
    // Outputs: a set onto the output of a set that is not renamed.
    m.click_item(ROCK, false, false);
    m.open_rename(RenameMode::Outputs);
    dialog(&mut m).typed.insert(rock.clone(), "LEAF".into());
    assert_eq!(
        project(&plan(&m)),
        strs(&[
            "landscape/rock01|rock01|LEAF|None|landscape/LEAF.dds (+2 more)|true|true|Some(\"landscape/leaf already writes landscape/LEAF.dds\")|None|None",
        ])
    );
    // Inputs: onto a name another file has, two files onto one name, one onto a name another file gives up.
    m.select_all_visible();
    m.open_rename(RenameMode::Inputs);
    let d2 = dialog(&mut m);
    d2.typed.insert(LEAF.into(), "landscape/odd.png".into());
    d2.typed.insert(ROCK.into(), "landscape/same.png".into());
    d2.typed
        .insert("landscape/odd.png".into(), "landscape/same.png".into());
    d2.typed.insert(
        "landscape/rock01_normal.png".into(),
        "landscape/leaf.png".into(),
    );
    assert_eq!(
        project(&plan(&m)),
        strs(&[
            "landscape/leaf.png|landscape/leaf.png|landscape/odd.png|Some((\"colour\", \"colour\"))|landscape/odd.dds|true|true|Some(\"landscape/odd.png already has this name\")|None|None",
            "landscape/odd.png|landscape/odd.png|landscape/same.png|Some((\"colour\", \"colour\"))|landscape/same.dds|true|true|None|None|None",
            "landscape/rock01.png|landscape/rock01.png|landscape/same.png|Some((\"colour\", \"colour\"))|landscape/same.dds|true|true|Some(\"landscape/odd.png is renamed to this name too\")|None|None",
            "landscape/rock01_normal.png|landscape/rock01_normal.png|landscape/leaf.png|Some((\"_normal\", \"colour\"))|landscape/leaf.dds|true|true|Some(\"landscape/leaf.png already has this name\")|Some(\"read as colour now (was _normal)\")|None",
            "landscape/rock01_rmaos|landscape/rock01_rmaos|landscape/rock01_rmaos|None||true|false|None|None|Some(\"skipped (a packed output has no file of its own)\")",
        ])
    );
    // A locked slot skips its texture.
    let rock_diffuse = sdc_gui::model::sets::SlotKind::Diffuse.slot_number();
    m.set_slot_lock(&rock, rock_diffuse, true).unwrap();
    dialog(&mut m).typed.clear();
    dialog(&mut m).recipe.prefix = "y_".into();
    let p = project(&plan(&m));
    assert_eq!(
        p[2],
        "landscape/rock01.png|landscape/rock01.png|landscape/y_rock01.png|Some((\"colour\", \"colour\"))|landscape/y_rock01.dds|true|true|None|None|Some(\"skipped (locked)\")"
    );
    assert_eq!(p[0].matches("skipped").count(), 0);
}

fn rename_menu(
    entries: Vec<sdc_gui::model::bulk::MenuEntry>,
) -> Vec<(MenuAction, String, Option<String>)> {
    entries
        .into_iter()
        .filter(|e| {
            matches!(
                e.action,
                MenuAction::RenameInputs | MenuAction::RenameOutputs | MenuAction::ClearInputAlias
            )
        })
        .map(|e| (e.action, e.label, e.disabled))
        .collect()
}

#[test]
fn the_item_and_set_menus_keep_their_rename_entries_in_order() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.click_item(ROCK, false, false);
    m.click_item(LEAF, true, false);
    let f2 = format!("  {}", sdc_gui::ui::actions::keys_text("rename"));
    assert_eq!(
        rename_menu(m.item_menu()),
        vec![
            (
                MenuAction::RenameInputs,
                format!("Rename inputs…{f2}"),
                None
            ),
            (
                MenuAction::RenameOutputs,
                "Rename outputs…".to_string(),
                None
            ),
        ]
    );
    m.open_rename(RenameMode::Inputs);
    let d2 = dialog(&mut m);
    d2.typed.insert(LEAF.into(), "landscape/maple.png".into());
    assert!(m.apply_rename());
    idle(&mut m);
    m.click_item(ROCK, false, false);
    m.click_item(LEAF, true, false);
    assert_eq!(
        rename_menu(m.item_menu()),
        vec![
            (
                MenuAction::RenameInputs,
                format!("Rename inputs…{f2}"),
                None
            ),
            (
                MenuAction::RenameOutputs,
                "Rename outputs…".to_string(),
                None
            ),
            (
                MenuAction::ClearInputAlias,
                "Clear input alias (1)".to_string(),
                None
            ),
        ]
    );
    let (a, b) = (set_key_of(&m, ROCK), set_key_of(&m, LEAF));
    m.click_set(&a, false, false);
    m.click_set(&b, true, false);
    assert_eq!(
        rename_menu(m.set_menu()),
        vec![
            (
                MenuAction::RenameOutputs,
                format!("Rename outputs…{f2}"),
                None
            ),
            (MenuAction::RenameInputs, "Rename inputs…".to_string(), None),
            (
                MenuAction::ClearInputAlias,
                "Clear input alias (1)".to_string(),
                None
            ),
        ]
    );
}
