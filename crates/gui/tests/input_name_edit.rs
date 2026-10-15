//! Note AO: the set header's input name text box: gray Tab completion, the path lock and the commit keys.
mod support;
use eframe::egui::{self, accesskit::Role, Key, Modifiers};
use egui_kittest::kittest::{by, NodeT as _, Queryable};
use egui_kittest::Harness;
use sdc_gui::model::input_name::{completion, lock_cut, separator_stops, tab_part};
use sdc_gui::model::known_paths::KnownPaths;
use sdc_gui::model::pbr_export::json_entries;
use sdc_gui::model::AppModel;
use sdc_gui::ui::State;
use std::time::Duration;
use support::*;

const ROCK_SET: &str = "landscape/rock01";

fn paths(p: &[&str]) -> Vec<String> {
    p.iter().map(|s| s.to_string()).collect()
}

#[test]
fn the_ghost_completes_a_prefix_and_tab_takes_it_to_the_next_separator() {
    let list = paths(&[
        "textures/architecture/whiterun/wall",
        "textures/landscape/rock02",
        "textures/landscape/rocks/big",
    ]);
    // Case-insensitive, `\` read as `/`; the first match in list order.
    assert_eq!(
        completion(&list, "Textures\\Land").as_deref(),
        Some("scape/rock02")
    );
    assert_eq!(tab_part(&list, "textures/land").as_deref(), Some("scape/"));
    // Tab again continues; one candidate left completes the whole path.
    assert_eq!(
        tab_part(&list, "textures/landscape/").as_deref(),
        Some("rock02")
    );
    assert_eq!(
        tab_part(&list, "textures/ar").as_deref(),
        Some("chitecture/whiterun/wall")
    );
    // No candidate, or the text is already a whole path: no ghost.
    assert_eq!(completion(&list, "meshes/"), None);
    assert_eq!(completion(&list, "textures/landscape/rock02"), None);
    assert_eq!(tab_part(&list, "meshes/"), None);
}

#[test]
fn the_lock_stops_are_the_separators() {
    assert_eq!(separator_stops("textures\\landscape/rock01"), vec![8, 18]);
    assert!(separator_stops("rock01").is_empty());
    // Lock n keeps everything up to the n-th separator.
    assert_eq!(lock_cut("textures/landscape/rock01", 0), 0);
    assert_eq!(lock_cut("textures/landscape/rock01", 1), 9);
    assert_eq!(lock_cut("textures/landscape/rock01", 2), 19);
    assert_eq!(lock_cut("textures/landscape/rock01", 9), 19);
}

type H = Harness<'static, (AppModel, State)>;

fn harness() -> (tempfile::TempDir, H) {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.set_known(KnownPaths {
        textures: [
            ("textures/landscape/rock02.dds".to_string(), 1u16),
            ("textures/landscape/rocks/big.dds".to_string(), 1u16),
            ("textures/landscape/rocks/small.dds".to_string(), 1u16),
            ("textures/other/thing.dds".to_string(), 1u16),
        ]
        .into(),
        mesh_dirs: vec![],
    });
    let key = json_entries(&m)
        .into_iter()
        .find(|(v, _)| v.id == ROCK_SET)
        .unwrap()
        .0
        .key;
    m.select_set(&key);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1600.0, 900.0))
        .build_ui_state(
            |ui, (m, s): &mut (AppModel, State)| sdc_gui::ui::draw_with(ui, m, s),
            (m, State::default()),
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    (d, h)
}

/// The free (editable) part of the box: the only text input whose value is under `textures/landscape/`.
fn free(h: &H) -> String {
    h.get(
        by().role(Role::TextInput)
            .label(sdc_gui::ui::input_name::HINT),
    )
    .accesskit_node()
    .value()
    .unwrap_or_default()
}

fn focus(h: &mut H) {
    h.get(
        by().role(Role::TextInput)
            .label(sdc_gui::ui::input_name::HINT),
    )
    .click();
    h.run();
    // The click puts the cursor where it lands; start from the end.
    key(h, Key::End, Modifiers::NONE);
}

fn key(h: &mut H, key: Key, modifiers: Modifiers) {
    h.input_mut().events.push(egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    });
    h.run();
}

fn clear_and_type(h: &mut H, s: &str) {
    for _ in 0..12 {
        key(h, Key::Backspace, Modifiers::NONE);
    }
    h.input_mut().events.push(egui::Event::Text(s.into()));
    h.run();
}

fn ctrl() -> Modifiers {
    Modifiers {
        ctrl: true,
        command: true,
        ..Modifiers::NONE
    }
}

#[test]
fn the_lock_defaults_to_the_folder_and_ctrl_arrows_move_it() {
    let (_d, mut h) = harness();
    // The folder is locked (drawn as a label), the file name is free.
    assert_eq!(free(&h), "rock01");
    h.get_by_label("textures/landscape/");
    focus(&mut h);
    key(&mut h, Key::ArrowLeft, ctrl());
    assert_eq!(free(&h), "landscape/rock01");
    key(&mut h, Key::ArrowLeft, ctrl());
    key(&mut h, Key::ArrowLeft, ctrl());
    assert_eq!(free(&h), "textures/landscape/rock01", "no lock left of 0");
    key(&mut h, Key::ArrowRight, ctrl());
    key(&mut h, Key::ArrowRight, ctrl());
    key(&mut h, Key::ArrowRight, ctrl());
    assert_eq!(free(&h), "rock01", "no lock right of the last separator");
}

#[test]
fn the_dropdown_lists_only_paths_under_the_lock() {
    let (_d, mut h) = harness();
    h.get_by_label(sdc_gui::ui::input_name::DROPDOWN).click();
    h.run();
    h.get_by_label("Read this set's textures as");
    h.get_by_label("textures/landscape/rock02");
    assert!(h.query_by_label("textures/other/thing").is_none());
}

#[test]
fn tab_accepts_the_ghost_and_with_no_ghost_moves_focus() {
    let (_d, mut h) = harness();
    focus(&mut h);
    clear_and_type(&mut h, "rocks");
    key(&mut h, Key::Tab, Modifiers::NONE);
    assert_eq!(free(&h), "rocks/");
    key(&mut h, Key::Tab, Modifiers::NONE);
    assert_eq!(free(&h), "rocks/big");
    // Nothing to complete: Tab leaves the box; a|b is refused, so the box reverts. The ghost never commits.
    let steps = h.state().0.history.undo.len();
    clear_and_type(&mut h, "a|b");
    key(&mut h, Key::Tab, Modifiers::NONE);
    assert_eq!(free(&h), "rock01");
    assert_eq!(h.state().0.history.undo.len(), steps);
    // Right arrow at the end takes the whole ghost.
    focus(&mut h);
    clear_and_type(&mut h, "rocks");
    key(&mut h, Key::End, Modifiers::NONE);
    key(&mut h, Key::ArrowRight, Modifiers::NONE);
    assert_eq!(free(&h), "rocks/big");
}

#[test]
fn enter_commits_one_step_esc_reverts_and_invalid_text_does_not_commit() {
    let (_d, mut h) = harness();
    let steps = h.state().0.history.undo.len();
    focus(&mut h);
    clear_and_type(&mut h, "rock02");
    // The first Esc closes the live list, the second reverts (AO4).
    key(&mut h, Key::Escape, Modifiers::NONE);
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert_eq!(free(&h), "rock01", "Esc reverts");
    assert_eq!(h.state().0.history.undo.len(), steps);
    // leaf.png already has that name: invalid, shown red with its reason, and never committed.
    focus(&mut h);
    clear_and_type(&mut h, "leaf");
    let why = h
        .get(
            by().role(Role::TextInput)
                .label(sdc_gui::ui::input_name::HINT),
        )
        .accesskit_node()
        .description()
        .unwrap_or_default();
    assert!(why.contains("already has this name"), "{why}");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(h.state().0.history.undo.len(), steps);
    assert_eq!(free(&h), "rock01");
    focus(&mut h);
    clear_and_type(&mut h, "rock02");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(h.state().0.history.undo.len(), steps + 1, "one undo step");
    assert!(h.state_mut().0.wait_idle(Duration::from_secs(120)));
    assert_eq!(
        h.state().0.input_name_of("landscape/rock02").as_deref(),
        Some("textures/landscape/rock02")
    );
}

/// Note AO8: paths under `textures\pbr\` are PGPatcher's outputs, not input names: no row, no ghost.
#[test]
fn pbr_output_paths_are_never_offered() {
    let (_d, mut h) = harness();
    h.state_mut().0.set_known(KnownPaths {
        textures: [
            ("textures/pbr/landscape/rock02.dds".to_string(), 1u16),
            ("textures/landscape/rock02.dds".to_string(), 1u16),
        ]
        .into(),
        mesh_dirs: vec![],
    });
    let list = h.state().0.input_name_candidates();
    assert!(list.iter().any(|p| p == "textures/landscape/rock02"));
    assert!(
        !list
            .iter()
            .any(|p| p.to_lowercase().starts_with("textures/pbr/")),
        "{list:?}"
    );
    assert_eq!(completion(&list, "Textures\\PBR"), None);
    // Unlocked to `textures/`, the list still has no pbr row.
    h.run();
    focus(&mut h);
    key(&mut h, Key::ArrowLeft, ctrl());
    key(&mut h, Key::Escape, Modifiers::NONE);
    h.get_by_label(sdc_gui::ui::input_name::DROPDOWN).click();
    h.run();
    h.get_by_label("textures/landscape/rock02");
    assert!(h.query_by_label("textures/pbr/landscape/rock02").is_none());
}

const BIG: &str = "textures/landscape/rocks/big";
const SMALL: &str = "textures/landscape/rocks/small";

fn click_at(h: &mut H, p: egui::Pos2) {
    for pressed in [true, false] {
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
    h.run();
}

fn settle(h: &mut H) {
    assert!(h.state_mut().0.wait_idle(Duration::from_secs(120)));
    h.run();
}

/// Note AO4: typing shows a live list under the box; Down then Enter commits the highlighted row.
#[test]
fn typing_filters_the_live_list_and_down_enter_commits_its_row() {
    let (_d, mut h) = harness();
    let steps = h.state().0.history.undo.len();
    focus(&mut h);
    clear_and_type(&mut h, "rocks");
    h.get_by_label(BIG);
    h.get_by_label(SMALL);
    assert!(h.query_by_label("textures/landscape/rock02").is_none());
    assert!(
        h.query_by_label("Read this set's textures as").is_none(),
        "not the ▾ list"
    );
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    key(&mut h, Key::ArrowUp, Modifiers::NONE);
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(h.state().0.history.undo.len(), steps + 1, "one undo step");
    settle(&mut h);
    assert_eq!(
        h.state().0.input_name_of("landscape/rocks/big").as_deref(),
        Some(BIG)
    );
    assert!(h.query_by_label(SMALL).is_none(), "the list closed");
}

/// Note AO4: a click on a live row commits that row, not the half-typed text the click's focus loss would.
#[test]
fn a_click_on_a_live_row_commits_that_row() {
    let (_d, mut h) = harness();
    let steps = h.state().0.history.undo.len();
    focus(&mut h);
    clear_and_type(&mut h, "rocks");
    let p = h.get_by_label(SMALL).rect().center();
    click_at(&mut h, p);
    h.run();
    assert_eq!(h.state().0.history.undo.len(), steps + 1, "one undo step");
    settle(&mut h);
    assert_eq!(
        h.state()
            .0
            .input_name_of("landscape/rocks/small")
            .as_deref(),
        Some(SMALL)
    );
}

/// Note AO4: the first Esc closes the live list and keeps the text; the second reverts.
#[test]
fn esc_closes_the_live_list_then_reverts() {
    let (_d, mut h) = harness();
    focus(&mut h);
    clear_and_type(&mut h, "rocks");
    h.get_by_label(BIG);
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(h.query_by_label(BIG).is_none(), "closed");
    assert_eq!(free(&h), "rocks");
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert_eq!(free(&h), "rock01");
}

fn push_key(h: &mut H, key: Key) {
    h.input_mut().events.push(egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
}

/// Review fix: the ghost and Tab's hold follow the text in the frame it changes, with no settling run.
#[test]
fn the_ghost_follows_a_keystroke_by_the_next_frame() {
    let (_d, mut h) = harness();
    focus(&mut h);
    // "rocks/biz" has no candidate: no ghost, so Tab would leave the box.
    clear_and_type(&mut h, "rocks/biz");
    push_key(&mut h, Key::Backspace);
    h.step();
    // "rocks/bi" completes to "rocks/big": the very next frame's Tab is held and takes it.
    push_key(&mut h, Key::Tab);
    h.step();
    h.step();
    assert_eq!(free(&h), "rocks/big");
}

/// Review fix: a pasted full path that starts with the locked part replaces the free part, not doubled after the lock.
#[test]
fn a_pasted_full_path_drops_the_locked_part() {
    let (_d, mut h) = harness();
    focus(&mut h);
    clear_and_type(&mut h, "");
    h.input_mut()
        .events
        .push(egui::Event::Paste("Textures\\Landscape\\rock02".into()));
    h.run();
    assert_eq!(free(&h), "rock02");
}

/// Review fix: a row commits only when the press and the release are both on it.
#[test]
fn a_press_dragged_off_a_live_row_commits_nothing() {
    let (_d, mut h) = harness();
    let steps = h.state().0.history.undo.len();
    focus(&mut h);
    clear_and_type(&mut h, "rocks");
    let p = h.get_by_label(SMALL).rect().center();
    h.input_mut().events.push(egui::Event::PointerButton {
        pos: p,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.run();
    let away = egui::pos2(1500.0, 300.0);
    h.input_mut().events.push(egui::Event::PointerMoved(away));
    h.input_mut().events.push(egui::Event::PointerButton {
        pos: away,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run();
    h.run();
    assert_eq!(h.state().0.history.undo.len(), steps, "nothing committed");
    // A drag is no click: the box keeps its text and focus. Enter then takes the typed text, not the pressed row.
    assert_eq!(free(&h), "rocks");
    key(&mut h, Key::Enter, Modifiers::NONE);
    settle(&mut h);
    assert!(h.state().0.input_name_of("landscape/rocks/small").is_none());
}
