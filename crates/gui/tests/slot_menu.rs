//! F3a Task 8 (ledger R8): the slot context menu of a set-editor card or a pill member: the Slot commands, From
//! another set, Pin / Unpin (sdc) and the PGPatcher lock (Note AN).
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
                MenuAction::ShowInExplorer(ROCK.into()),
                "Show in Explorer",
                None
            ),
            (
                MenuAction::SlotLock {
                    set: ROCK_SET.into(),
                    n: 1,
                    on: true
                },
                "Pin this slot (always use this file)",
                None
            ),
            (
                MenuAction::PgpLock {
                    set: ROCK_SET.into(),
                    n: 1,
                    on: true
                },
                "Keep the original texture (PGPatcher won't replace it)",
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

/// Note F?11 (D13): From this set on a slot the set has no file for says so; a slot that has one keeps "already".
#[test]
fn an_empty_slot_says_the_set_has_no_file() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    let menu = m.slot_menu(ROCK_SET, SlotKind::Coat);
    assert_eq!(menu[0].label, "From this set (nothing)");
    assert_eq!(
        menu[0].disabled.as_deref(),
        Some("This set has no file for this slot")
    );
    let menu = m.slot_menu(ROCK_SET, SlotKind::Diffuse);
    assert_eq!(menu[0].disabled.as_deref(), Some("already from this set"));
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
    let pin = menu
        .iter()
        .find(|e| matches!(e.action, MenuAction::SlotLock { .. }))
        .unwrap();
    assert_eq!(
        (pin.action.clone(), pin.label.as_str()),
        (lock(false), "Unpin this slot")
    );
    // A refused command comes back as the reason.
    assert_eq!(
        m.run_menu(cmd(ROCK_SET, 2, SlotCommand::Clear)),
        Some("slot locked".to_string())
    );
    assert_eq!(m.run_menu(lock(false)), None);
    assert!(!m.slot_locked(ROCK_SET, 2));
}

#[test]
fn a_vanilla_member_under_textures_gives_its_game_path() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    // leaf.png is in a vanilla set: no planned output path, so the game's own path, `.dds`.
    assert_eq!(
        member_game_path(&m, LEAF).as_deref(),
        Ok(r"textures\landscape\leaf.dds")
    );
    assert_eq!(
        m.run_menu(MenuAction::SlotLink {
            set: ROCK_SET.into(),
            n: 1,
            item: LEAF.into()
        }),
        None
    );
    assert_eq!(
        slot_value(&m, ROCK_SET, 1),
        Some(json!(r"textures\landscape\leaf.dds"))
    );
}

#[test]
fn the_game_path_follows_the_current_prefix_not_the_last_plan() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    m.set_json_value(ROCK_SET, "rename", Some(json!("stone")));
    // Before the new plan lands.
    assert_eq!(
        member_game_path(&m, ROCK).as_deref(),
        Ok(r"textures\pbr\landscape\stone.dds")
    );
}

#[test]
fn a_member_outside_a_textures_folder_is_offered_disabled() {
    use image::{ImageBuffer, Rgb};
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("m");
    std::fs::create_dir_all(&r).unwrap();
    for n in ["rock01.png", "leaf.png"] {
        ImageBuffer::from_fn(64, 64, |x, y| Rgb([(x * 4) as u8, (y * 4) as u8, 90u8]))
            .save(r.join(n))
            .unwrap();
    }
    let settings = sdc_gui::settings::GuiSettings {
        export_folder: Some(d.path().join("export")),
        ..Default::default()
    };
    let mut m = AppModel::new(settings, None, session());
    m.accept = engine::policy::AcceptSuggestions::None;
    m.open_folder(d.path().to_path_buf());
    idle(&mut m);
    const NO_PATH: &str = "no game path (not under a textures folder)";
    assert_eq!(
        member_game_path(&m, "m/rock01.png"),
        Err(NO_PATH.to_string())
    );
    let menu = m.slot_menu("m/leaf", SlotKind::Diffuse);
    let link = menu
        .iter()
        .find(|e| matches!(&e.action, MenuAction::SlotLink { item, .. } if item == "m/rock01.png"))
        .expect("listed");
    assert_eq!(link.disabled.as_deref(), Some(NO_PATH));
    // The submenu itself opens (it has an entry).
    assert!(menu
        .iter()
        .any(|e| e.action == MenuAction::FromAnotherSet && e.disabled.is_none()));
    // Run anyway, it is refused and writes nothing.
    assert_eq!(m.run_menu(link.action.clone()), Some(NO_PATH.to_string()));
    assert_eq!(slot_value(&m, "m/leaf", 1), None);
}

/// Audit 08f-8: a menu pick that is refused says why in the status line (and Messages), wherever the menu was.
#[test]
fn a_refused_menu_pick_says_why() {
    let d = tempfile::tempdir().unwrap();
    let mut m = support::opened(d.path());
    m.menu_picked(sdc_gui::model::bulk::MenuAction::ShowInExplorer(
        "no/such.png".into(),
    ));
    assert!(
        m.status_line.contains("no such texture"),
        "{}",
        m.status_line
    );
}

/// Trace-debug wave 2 #4: a vanilla member's link names the file its export writes (folder from the game path, name
/// from the output), so `moss_normal.png` links as `moss_n.dds` and `moss_albedo.png` as `moss.dds` (Note R9).
#[test]
fn a_vanilla_member_links_to_its_written_output_name() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("textures/landscape");
    std::fs::create_dir_all(&r).unwrap();
    image::ImageBuffer::from_fn(64, 64, |_, _| image::Rgb([128u8, 128, 255]))
        .save(r.join("moss_normal.png"))
        .unwrap();
    image::ImageBuffer::from_fn(64, 64, |x, _| image::Rgb([x as u8 * 4, 90, 40]))
        .save(r.join("moss_albedo.png"))
        .unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    let normal = "landscape/moss_normal.png";
    assert_eq!(
        member_game_path(&m, normal).as_deref(),
        Ok(r"textures\landscape\moss_n.dds")
    );
    assert_eq!(
        member_game_path(&m, "landscape/moss_albedo.png").as_deref(),
        Ok(r"textures\landscape\moss.dds")
    );
    assert_eq!(
        m.run_menu(MenuAction::SlotLink {
            set: ROCK_SET.into(),
            n: 2,
            item: normal.into()
        }),
        None
    );
    assert_eq!(
        slot_value(&m, ROCK_SET, 2),
        Some(json!(r"textures\landscape\moss_n.dds"))
    );
}
