//! M6 Track F3a D3/D4: map-kind chips over selected sets, and the right-click menus.
mod support;
use engine::config::Subtype;
use sdc_gui::model::bulk::{kind_chips, MenuAction};
use sdc_gui::model::sets::SlotKind;
use sdc_gui::model::*;
use std::time::Duration;
use support::*;

const ROCK: &str = "landscape/rock01.png";
const LEAF: &str = "landscape/leaf.png";
const ODD: &str = "landscape/odd.png";
const NORMAL: &str = "landscape/rock01_normal.png";
const RMAOS: &str = "landscape/rock01_rmaos";

fn set_of(m: &AppModel, item: &str) -> String {
    sdc_gui::model::sets::views(m)
        .iter()
        .find(|v| v.members.iter().any(|k| k == item))
        .expect("the item's set")
        .key
        .clone()
}

fn strings(k: &[&str]) -> Vec<String> {
    k.iter().map(|s| s.to_string()).collect()
}

/// rock01 as a TruePBR set, and leaf decided as a plain diffuse: two sets.
fn two_sets(dir: &std::path::Path) -> (AppModel, String, String) {
    let mut m = pbr_opened(dir);
    m.decide(LEAF, Answer::Subtype(Subtype::Diffuse));
    assert!(m.wait_idle(Duration::from_secs(60)));
    let (rock, leaf) = (set_of(&m, ROCK), set_of(&m, LEAF));
    m.click_set(&rock, false, false);
    m.click_set(&leaf, true, false);
    (m, rock, leaf)
}

#[test]
fn chips_list_the_kinds_present_with_counts_and_set_coverage() {
    let d = tempfile::tempdir().unwrap();
    let (m, rock, _) = two_sets(d.path());
    let rock_members = sdc_gui::model::sets::views(&m)
        .iter()
        .find(|v| v.key == rock)
        .unwrap()
        .members
        .len();
    let chips = kind_chips(&m, &m.selected_sets());
    let got: Vec<(Option<SlotKind>, usize, usize)> = chips
        .iter()
        .map(|c| (c.kind, c.members.len(), c.sets_with))
        .collect();
    assert_eq!(
        got,
        vec![
            (None, rock_members + 1, 2),
            (Some(SlotKind::Diffuse), 2, 2),
            (Some(SlotKind::Normal), 1, 1),
            (Some(SlotKind::Rmaos), 1, 1),
        ]
    );
    assert_eq!(chips[2].label, "Normal _n");
}

#[test]
fn the_picked_chip_decides_the_targets_and_select_members_turns_sets_into_textures() {
    let d = tempfile::tempdir().unwrap();
    let (mut m, _, _) = two_sets(d.path());
    assert!(m.in_bulk());
    m.bulk.kind = Some(SlotKind::Normal);
    assert_eq!(m.bulk_targets(), strings(&[NORMAL]));
    m.bulk.kind = Some(SlotKind::Rmaos);
    assert_eq!(m.bulk_targets(), strings(&[RMAOS]));
    m.select_members_of_kind(Some(SlotKind::Diffuse));
    assert_eq!(m.selected_items(), strings(&[ROCK, LEAF]));
    assert!(m.selected_sets().is_empty());
    assert_eq!(m.selection, Selection::Item(ROCK.into()));
    assert_eq!(m.bulk_targets(), strings(&[ROCK, LEAF]));
}

#[test]
fn right_click_keeps_a_selected_entry_and_replaces_an_unselected_one() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.click_item(ROCK, false, false);
    m.click_item(LEAF, true, false);
    m.right_click_item(ROCK);
    assert_eq!(m.selected_items(), strings(&[ROCK, LEAF]));
    m.right_click_item(ODD);
    assert_eq!(m.selected_items(), strings(&[ODD]));
}

#[test]
fn the_tile_menu_is_a_list_with_counts_and_reasons() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.click_item(ROCK, false, false);
    m.click_item(LEAF, true, false);
    assert_eq!(m.menu_title(false), "2 textures selected");
    let menu = m.item_menu();
    let got: Vec<(MenuAction, &str)> = menu.iter().map(|e| (e.action, e.label.as_str())).collect();
    assert_eq!(
        got,
        vec![
            (MenuAction::EditSettings, "Edit settings for 2 selected"),
            (
                MenuAction::CopyFromActive,
                "Copy settings from leaf.png to 1 selected…"
            ),
            (MenuAction::ResetAll, "Reset all settings for 2 selected…"),
        ]
    );
    assert_eq!(
        menu[1].disabled.as_deref(),
        Some("leaf.png has no settings of its own")
    );
    assert_eq!(menu[2].disabled, None);
    m.run_menu(MenuAction::ResetAll);
    assert_eq!(
        m.dialog,
        Some(Dialog::ConfirmResetAll(strings(&[ROCK, LEAF])))
    );
}

#[test]
fn the_set_menu_offers_reset_over_members_and_one_entry_per_chip() {
    let d = tempfile::tempdir().unwrap();
    let (mut m, rock, leaf) = two_sets(d.path());
    assert_eq!(m.menu_title(true), "2 sets selected");
    let menu = m.set_menu();
    for a in [MenuAction::EditSettings, MenuAction::ResetAll] {
        assert!(menu.iter().any(|e| e.action == a), "{a:?} in the menu");
    }
    let kinds: Vec<MenuAction> = menu
        .iter()
        .map(|e| e.action)
        .filter(|a| matches!(a, MenuAction::SelectMembersOfKind(_)))
        .collect();
    assert_eq!(
        kinds,
        vec![
            MenuAction::SelectMembersOfKind(None),
            MenuAction::SelectMembersOfKind(Some(SlotKind::Diffuse)),
            MenuAction::SelectMembersOfKind(Some(SlotKind::Normal)),
            MenuAction::SelectMembersOfKind(Some(SlotKind::Rmaos)),
        ]
    );
    m.run_menu(MenuAction::ResetAll);
    let Some(Dialog::ConfirmResetAll(keys)) = m.dialog.clone() else {
        panic!("expected the reset confirm, got {:?}", m.dialog);
    };
    for k in [ROCK, NORMAL, RMAOS, LEAF] {
        assert!(keys.iter().any(|x| x == k), "{k} in {keys:?}");
    }
    assert_eq!(
        m.selected_sets(),
        vec![rock, leaf],
        "the menu keeps the selection"
    );
}

#[test]
fn a_picked_kind_that_left_after_a_replan_falls_back_to_all_visibly() {
    let d = tempfile::tempdir().unwrap();
    let (mut m, _, _) = two_sets(d.path());
    m.bulk.kind = Some(SlotKind::Normal);
    assert_eq!(m.bulk_targets(), strings(&[NORMAL]));
    m.decide(NORMAL, Answer::Subtype(Subtype::Diffuse));
    assert!(m.wait_idle(Duration::from_secs(60)));
    let sets = m.selected_sets();
    let shown: Vec<Option<SlotKind>> = kind_chips(&m, &sets).iter().map(|c| c.kind).collect();
    assert!(!shown.contains(&Some(SlotKind::Normal)), "{shown:?}");
    let all = kind_chips(&m, &sets)[0].members.clone();
    assert_eq!(m.bulk_targets(), all, "no more than the chips show");
    assert_eq!(m.bulk.kind, None, "the chip row shows All");
}
