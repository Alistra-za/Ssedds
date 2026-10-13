//! F3a Task 8 (ledger R8): the slot context menu of a set-editor card or a pill member: the Slot commands, From
//! another set, Lock / Unlock.
mod support;
use sdc_gui::model::bulk::{MenuAction, MenuEntry};
use sdc_gui::model::sets::*;
use sdc_gui::model::AppModel;
use serde_json::json;
use std::time::Duration;
use support::*;

const ROCK_SET: &str = "landscape/rock01";
const LEAF_SET: &str = "landscape/leaf";
const ROCK: &str = "landscape/rock01.png";
const LEAF: &str = "landscape/leaf.png";

fn idle(m: &mut AppModel) {
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
}

fn rows(menu: &[MenuEntry]) -> Vec<(MenuAction, &str, Option<&str>)> {
    menu.iter()
        .map(|e| (e.action.clone(), e.label.as_str(), e.disabled.as_deref()))
        .collect()
}

fn slot_value(m: &AppModel, set: &str, n: u8) -> Option<serde_json::Value> {
    set_views(m)
        .into_iter()
        .find(|v| v.id == set)
        .unwrap()
        .settings
        .get(&format!("slot{n}"))
        .cloned()
}

fn cmd(set: &str, n: u8, cmd: SlotCommand) -> MenuAction {
    MenuAction::Slot {
        set: set.into(),
        n,
        cmd,
    }
}

#[test]
fn a_filled_slot_offers_the_slot_commands_other_sets_and_lock() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    let menu = m.slot_menu(ROCK_SET, SlotKind::Diffuse);
    assert_eq!(
        rows(&menu),
        vec![
            (
                cmd(ROCK_SET, 1, SlotCommand::FromSet),
                "From this set (rock01.png)",
                Some("already from this set")
            ),
            (
                MenuAction::SlotPath {
                    set: ROCK_SET.into(),
                    n: 1
                },
                "Path…",
                None
            ),
            (cmd(ROCK_SET, 1, SlotCommand::Clear), "Clear", None),
            (MenuAction::FromAnotherSet, "From another set", None),
            (
                MenuAction::SlotLink {
                    set: ROCK_SET.into(),
                    n: 1,
                    item: LEAF.into()
                },
                "landscape/leaf — leaf.png",
                None
            ),
            (
                MenuAction::SlotLink {
                    set: ROCK_SET.into(),
                    n: 1,
                    item: "landscape/odd.png".into()
                },
                "landscape/odd — odd.png",
                None
            ),
            (
                MenuAction::SlotLock {
                    set: ROCK_SET.into(),
                    n: 1,
                    on: true
                },
                "Lock",
                None
            ),
        ]
    );
    // The menu runs the window's model call: Clear, then From this set, one step each.
    assert_eq!(m.run_menu(cmd(ROCK_SET, 1, SlotCommand::Clear)), None);
    assert_eq!(slot_value(&m, ROCK_SET, 1), Some(json!("")));
    assert_eq!(
        m.slot_menu(ROCK_SET, SlotKind::Diffuse)[2]
            .disabled
            .as_deref(),
        Some("already cleared")
    );
    assert_eq!(m.run_menu(cmd(ROCK_SET, 1, SlotCommand::FromSet)), None);
    assert_eq!(slot_value(&m, ROCK_SET, 1), None);
    m.undo();
    assert_eq!(slot_value(&m, ROCK_SET, 1), Some(json!("")));
}

#[test]
fn an_empty_slot_with_no_match_elsewhere_disables_from_another_set() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    let menu = m.slot_menu(ROCK_SET, SlotKind::Height);
    assert_eq!(menu[0].label, "From this set (nothing)");
    assert_eq!(
        rows(&menu)[3],
        (
            MenuAction::FromAnotherSet,
            "From another set",
            Some("no other set has a Height _p")
        )
    );
    assert!(!menu
        .iter()
        .any(|e| matches!(e.action, MenuAction::SlotLink { .. })));
}

#[test]
fn from_another_set_writes_what_path_writes_with_that_members_game_path() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    let game = member_game_path(&m, ROCK).unwrap();
    assert_eq!(game, r"textures\pbr\landscape\rock01.dds");
    let link = MenuAction::SlotLink {
        set: LEAF_SET.into(),
        n: 1,
        item: ROCK.into(),
    };
    assert!(m
        .slot_menu(LEAF_SET, SlotKind::Diffuse)
        .iter()
        .any(|e| e.action == link && e.label == "landscape/rock01 — rock01.png"));
    assert_eq!(m.run_menu(link), None);
    let linked = slot_value(&m, LEAF_SET, 1);
    m.undo();
    assert_eq!(slot_value(&m, LEAF_SET, 1), None, "one undo step");
    m.set_slot_command(LEAF_SET, 1, SlotCommand::Path(game.clone()))
        .unwrap();
    assert_eq!(slot_value(&m, LEAF_SET, 1), linked);
    assert_eq!(linked, Some(json!(game)));
}

#[test]
fn a_locked_slot_menu_offers_only_unlock() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    let lock = |on| MenuAction::SlotLock {
        set: ROCK_SET.into(),
        n: 2,
        on,
    };
    assert_eq!(m.run_menu(lock(true)), None);
    let menu = m.slot_menu(ROCK_SET, SlotKind::Normal);
    for e in &menu[..4] {
        assert_eq!(e.disabled.as_deref(), Some("slot locked"), "{}", e.label);
    }
    let last = menu.last().unwrap();
    assert_eq!(
        (last.action.clone(), last.label.as_str()),
        (lock(false), "Unlock")
    );
    // A refused command comes back as the reason.
    assert_eq!(
        m.run_menu(cmd(ROCK_SET, 2, SlotCommand::Clear)),
        Some("slot locked".to_string())
    );
    assert_eq!(m.run_menu(lock(false)), None);
    assert!(!m.slot_locked(ROCK_SET, 2));
}
