//! M6 Track F3a D1: the multi-selection over textures and sets.
mod support;
use sdc_gui::model::*;
use std::time::Duration;
use support::*;

const ROCK: &str = "landscape/rock01.png";
const LEAF: &str = "landscape/leaf.png";
const ODD: &str = "landscape/odd.png";

fn visible_keys(m: &AppModel) -> Vec<String> {
    m.visible_items()
        .into_iter()
        .map(|i| m.items[i].key.clone())
        .collect()
}

fn strings(k: &[&str]) -> Vec<String> {
    k.iter().map(|s| s.to_string()).collect()
}

#[test]
fn ctrl_click_toggles_and_the_last_added_is_active() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.click_item(ROCK, false, false);
    assert_eq!(m.selected_items(), strings(&[ROCK]));
    assert!(!m.in_bulk());
    m.click_item(LEAF, true, false);
    assert_eq!(m.selected_items(), strings(&[ROCK, LEAF]));
    assert_eq!(m.selection, Selection::Item(LEAF.into()));
    assert!(m.in_bulk());
    assert!(m.is_selected(ROCK, false) && !m.is_selected(ODD, false));
    // Removing the active one hands over to the last remaining key.
    m.click_item(LEAF, true, false);
    assert_eq!(m.selected_items(), strings(&[ROCK]));
    assert_eq!(m.selection, Selection::Item(ROCK.into()));
    assert!(!m.in_bulk());
    m.click_item(ROCK, true, false);
    assert!(m.selected_items().is_empty());
    assert_eq!(m.selection, Selection::None);
}

#[test]
fn shift_click_selects_the_displayed_range_with_filters_applied() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let all = visible_keys(&m);
    assert!(all.len() >= 4, "{all:?}");
    m.click_item(&all[0], false, false);
    m.click_item(&all[3], false, true);
    assert_eq!(m.selected_items(), all[0..=3].to_vec());
    assert_eq!(
        m.selection,
        Selection::Item(all[0].clone()),
        "Shift keeps the active texture"
    );
    m.filter.needs_decision = true;
    let shown = visible_keys(&m);
    assert!(shown.len() >= 2 && shown.len() < all.len(), "{shown:?}");
    m.click_item(&shown[0], false, false);
    m.click_item(shown.last().unwrap(), false, true);
    assert_eq!(m.selected_items(), shown);
}

#[test]
fn ctrl_a_selects_the_visible_textures_and_the_count_says_how_many_are_hidden() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.click_item(ROCK, false, false);
    m.select_all_visible();
    let all = visible_keys(&m);
    assert_eq!(m.selected_items(), all);
    assert_eq!(
        m.selection,
        Selection::Item(ROCK.into()),
        "the active texture stays"
    );
    assert_eq!(
        m.selection_summary(false),
        Some(format!("{} selected", all.len()))
    );
    m.search = "rock01".into();
    let hidden = all.iter().filter(|k| !k.contains("rock01")).count();
    assert!(hidden > 0);
    assert_eq!(
        m.selection_summary(false),
        Some(format!(
            "{} selected ({hidden} hidden by filter)",
            all.len()
        ))
    );
    assert_eq!(m.selection_summary(true), None);
    m.clear_selection();
    assert_eq!(m.selection, Selection::None);
    assert!(m.selected_items().is_empty());
}

#[test]
fn the_selection_survives_a_plan_and_drops_keys_that_are_gone() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.click_item(ROCK, false, false);
    m.click_item(LEAF, true, false);
    m.click_item(ODD, true, false);
    m.replan();
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.selected_items(), strings(&[ROCK, LEAF, ODD]));
    // odd.png, the active one, is deleted: it leaves, and the last remaining key becomes active.
    std::fs::remove_file(d.path().join("textures/landscape/odd.png")).unwrap();
    m.replan();
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.selected_items(), strings(&[ROCK, LEAF]));
    assert_eq!(m.selection, Selection::Item(LEAF.into()));
}

#[test]
fn sets_and_textures_never_mix_and_select_item_ends_the_multi_selection() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let keys: Vec<String> = sdc_gui::model::sets::views(&m)
        .iter()
        .filter(|v| !v.key.is_empty())
        .map(|v| v.key.clone())
        .collect();
    assert!(keys.len() >= 3, "{keys:?}");
    m.click_set(&keys[0], false, false);
    m.click_set(&keys[2], false, true);
    assert_eq!(m.selected_sets(), keys[0..=2].to_vec());
    assert_eq!(m.selection_summary(true), Some("3 selected".into()));
    assert!(m.selected_items().is_empty());
    // A Ctrl+click on a texture while sets are selected starts over with that texture alone.
    m.click_item(ROCK, true, false);
    assert_eq!(m.selected_items(), strings(&[ROCK]));
    assert!(m.selected_sets().is_empty());
    m.click_item(LEAF, true, false);
    assert!(m.in_bulk());
    // Review Focus 2: every single-selection path ends the multi-selection.
    m.select_item(ODD);
    assert_eq!(m.selected_items(), strings(&[ODD]));
    assert!(!m.in_bulk());
}
