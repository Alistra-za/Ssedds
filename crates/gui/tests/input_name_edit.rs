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
    harness_dt(0.25)
}

fn harness_dt(dt: f32) -> (tempfile::TempDir, H) {
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
        .with_step_dt(dt)
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

/// Trace-debug #11: a click on another set in the Sets column commits the typed name first (AO1). Move, press and
/// release land in separate frames, as a real click does.
#[test]
fn a_click_on_another_set_commits_the_typed_name_first() {
    let (_d, mut h) = harness();
    let steps = h.state().0.history.undo.len();
    let rock = h.state().0.selection.clone();
    focus(&mut h);
    clear_and_type(&mut h, "rock02");
    let p = h.get_by_label("landscape/leaf").rect().center();
    h.input_mut().events.push(egui::Event::PointerMoved(p));
    h.run();
    for pressed in [true, false] {
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.run();
    }
    settle(&mut h);
    let now = &h.state().0.selection;
    assert!(
        matches!(now, sdc_gui::model::Selection::Set(_)) && *now != rock,
        "the click selected leaf: {now:?}"
    );
    assert_eq!(h.state().0.history.undo.len(), steps + 1, "one undo step");
    assert_eq!(
        h.state().0.input_name_of("landscape/rock02").as_deref(),
        Some("textures/landscape/rock02")
    );
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

/// Note BH1: the set's name shows once, as the input name box; no heading repeats it beside the box.
#[test]
fn the_set_header_shows_the_name_only_in_the_box() {
    let (_d, h) = harness();
    let edit = h
        .get(
            by().role(Role::TextInput)
                .label(sdc_gui::ui::input_name::HINT),
        )
        .rect();
    // Only the Sets column's row is labelled with the set's path; nothing on the box's row is.
    assert!(h.query_all_by_label(ROCK_SET).count() >= 1);
    let on_row = h
        .query_all_by_label(ROCK_SET)
        .filter(|n| (n.rect().center().y - edit.center().y).abs() < 12.0)
        .count();
    assert_eq!(on_row, 0);
}

/// Note BH2: the lock handle is an accessible slider; dragging it moves the lock to the nearest separator.
#[test]
fn the_lock_icon_drags_to_a_separator() {
    let (_d, mut h) = harness();
    assert_eq!(free(&h), "rock01");
    let handle = h.get_by_label(sdc_gui::ui::input_name::LOCK_LABEL);
    assert_eq!(handle.accesskit_node().role(), Role::Slider);
    let from = handle.rect().center();
    // BH13: the padlock sits on the box's lower edge, on the track under the text, not over it.
    let edit = h
        .get(
            by().role(Role::TextInput)
                .label(sdc_gui::ui::input_name::HINT),
        )
        .rect();
    assert!(from.y >= edit.bottom() - 0.5, "{from:?} vs {edit:?}");
    // The first stop: the box's left edge.
    let to = egui::pos2(
        h.get(
            by().role(Role::TextInput)
                .label(sdc_gui::ui::input_name::HINT),
        )
        .rect()
        .left()
            - 200.0,
        from.y,
    );
    h.input_mut().events.push(egui::Event::PointerMoved(from));
    h.run();
    h.input_mut().events.push(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.run();
    for i in 1..=4 {
        h.input_mut()
            .events
            .push(egui::Event::PointerMoved(from.lerp(to, i as f32 / 4.0)));
        h.run();
    }
    h.input_mut().events.push(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run();
    h.run();
    assert_eq!(free(&h), "textures/landscape/rock01");
}

/// Trace-debug #54: Save clicked while the input name box has focus saves nothing and says why (as Ctrl+S, AG23);
/// the name commits on the focus loss.
#[test]
fn header_save_click_with_focused_field_does_not_save_stale_value() {
    let (_d, mut h) = harness();
    // Another unsaved edit, so Save is enabled.
    h.state_mut()
        .0
        .set_json_value(ROCK_SET, "specular_level", Some(serde_json::json!(0.3)));
    h.run();
    let file = h.state().0.sets_doc.as_ref().unwrap().path().to_path_buf();
    focus(&mut h);
    clear_and_type(&mut h, "rock02");
    let p = h.query_all_by_label("Save").next().unwrap().rect().center();
    h.input_mut().events.push(egui::Event::PointerMoved(p));
    h.run();
    for pressed in [true, false] {
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.run();
    }
    settle(&mut h);
    let on_disk = std::fs::read_to_string(&file).unwrap_or_default();
    assert!(!on_disk.contains("specular_level"), "nothing was saved");
    assert!(
        h.state()
            .0
            .messages
            .entries()
            .any(|e| e.text == "Save: leave the text field first"),
        "the log says why"
    );
    assert_eq!(
        h.state().0.input_name_of("landscape/rock02").as_deref(),
        Some("textures/landscape/rock02")
    );
}

/// Export clicked while the input name box has focus saves nothing and says why, as the header Save does (#54,
/// Note BS14): Export saves first, and the focused value commits only on the focus loss.
#[test]
fn header_export_click_with_focused_field_does_not_save_stale_value() {
    let (_d, mut h) = harness();
    h.state_mut()
        .0
        .set_json_value(ROCK_SET, "specular_level", Some(serde_json::json!(0.3)));
    h.run();
    let file = h.state().0.sets_doc.as_ref().unwrap().path().to_path_buf();
    focus(&mut h);
    clear_and_type(&mut h, "rock02");
    let p = h
        .query_all_by_label("Export")
        .next()
        .unwrap()
        .rect()
        .center();
    h.input_mut().events.push(egui::Event::PointerMoved(p));
    h.run();
    for pressed in [true, false] {
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.run();
    }
    settle(&mut h);
    let on_disk = std::fs::read_to_string(&file).unwrap_or_default();
    assert!(!on_disk.contains("specular_level"), "nothing was saved");
    assert!(h.state().0.export.running.is_none(), "no export started");
    assert!(
        h.state()
            .0
            .messages
            .entries()
            .any(|e| e.text == "Export: leave the text field first"),
        "the log says why"
    );
    assert_eq!(
        h.state().0.input_name_of("landscape/rock02").as_deref(),
        Some("textures/landscape/rock02")
    );
}

/// The Review counterpart (layout v2 Task 1.4 fix 1): Review's Export clicked while the input name box has focus
/// saves nothing and says why, as the header Export does (#54, Note BS14).
#[test]
fn review_export_click_with_focused_field_does_not_save_stale_value() {
    let (_d, mut h) = harness();
    h.state_mut()
        .0
        .set_json_value(ROCK_SET, "specular_level", Some(serde_json::json!(0.3)));
    h.state_mut().0.open_review();
    h.run();
    let file = h.state().0.sets_doc.as_ref().unwrap().path().to_path_buf();
    let text = h.state().0.review_button_text();
    assert!(h.state().0.review_gate().is_ok());
    focus(&mut h);
    clear_and_type(&mut h, "rock02");
    let p = h.get_by_label(&text).rect().center();
    h.input_mut().events.push(egui::Event::PointerMoved(p));
    h.run();
    for pressed in [true, false] {
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.run();
    }
    settle(&mut h);
    let on_disk = std::fs::read_to_string(&file).unwrap_or_default();
    assert!(!on_disk.contains("specular_level"), "nothing was saved");
    assert!(h.state().0.export.running.is_none(), "no export started");
    assert!(h.state().0.export.summary.is_none(), "no export ran");
    assert!(
        h.state()
            .0
            .messages
            .entries()
            .any(|e| e.text == "Export: leave the text field first"),
        "the log says why"
    );
}

/// Trace-debug wave 2 #5: the project's own rows name outputs, so they carry no source extension (`leaf.png` is
/// offered as `textures/landscape/leaf`, never `textures/landscape/leaf.png`).
#[test]
fn own_output_rows_have_no_source_extension() {
    let d = tempfile::tempdir().unwrap();
    let m = pbr_opened(d.path());
    let known = m.known_textures(None);
    let names = m.input_name_candidates();
    for list in [&known, &names] {
        assert!(!list.iter().any(|p| p.ends_with(".png")), "{list:?}");
    }
    assert!(
        names.iter().any(|p| p == "textures/landscape/leaf"),
        "{names:?}"
    );
}

/// Alpha of the text shape that contains `needle`: the fade an Area applies reaches its fallback colour. The popup
/// is a Foreground Area, painted after everything else, so the last match is the list row.
fn row_alpha(h: &H, needle: &str) -> Option<u8> {
    fn find(s: &egui::Shape, needle: &str) -> Option<u8> {
        match s {
            egui::Shape::Text(t) if t.galley.text().contains(needle) => Some(t.fallback_color.a()),
            egui::Shape::Vec(v) => v.iter().find_map(|s| find(s, needle)),
            _ => None,
        }
    }
    h.output()
        .shapes
        .iter()
        .filter_map(|c| find(&c.shape, needle))
        .next_back()
}

/// Note CF3: the live path list is typed open, so it appears at its resting opacity on its first visible frame.
#[test]
fn the_path_list_shows_at_once_when_typing_reopens_it() {
    let (_d, mut h) = harness_dt(0.02);
    // kittest zeroes the stock animation time, which would turn every Area fade off.
    h.ctx.global_style_mut(|s| s.animation_time = 0.2);
    focus(&mut h);
    clear_and_type(&mut h, "rocks");
    h.run_steps(20);
    let rest = row_alpha(&h, BIG).expect("the list is open");
    key(&mut h, Key::Escape, Modifiers::NONE);
    h.run_steps(20);
    assert!(h.query_by_label(BIG).is_none(), "closed");
    h.input_mut().events.push(egui::Event::Text("/".into()));
    for _ in 0..6 {
        h.step();
        if let Some(first) = row_alpha(&h, BIG) {
            assert_eq!(first, rest, "the list faded in");
            return;
        }
    }
    panic!("the list never reopened");
}
