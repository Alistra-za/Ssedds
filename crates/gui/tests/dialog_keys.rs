//! Task 2 (items 2, 4, 23, 24): undo made visible, the dialog keys, the Delete set confirm, the Rename submenu.
mod support;
use eframe::egui::{self, Key};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use sdc_gui::model::{AppModel, Dialog, DocKind};
use serde_json::json;
use support::*;

fn harness(model: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(egui::vec2(1500.0, 900.0))
        .build_ui_state(
            |ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model),
            model,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

/// A model with one undoable step, labelled "hair".
fn with_a_step(dir: &std::path::Path) -> AppModel {
    let mut m = opened(dir);
    m.edit_docs("hair", None, false, |_, s| {
        s.set_value("landscape/rock01", "hair", Some(json!(true)))
    });
    m
}

// ---- item 2: undo made visible

#[test]
fn undo_and_redo_are_disabled_with_no_history() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened(d.path()));
    h.run();
    assert!(h.get_by_label("Undo").accesskit_node().is_disabled());
    assert!(h.get_by_label("Redo").accesskit_node().is_disabled());
}

#[test]
fn the_buttons_enable_with_history_and_their_hover_text_carries_the_label() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(with_a_step(d.path()));
    h.run();
    assert!(!h.get_by_label("Undo").accesskit_node().is_disabled());
    assert!(h.get_by_label("Redo").accesskit_node().is_disabled());
    h.get_by_label("Undo").hover();
    h.run();
    h.get_by_label_contains("Undo: hair");
    h.get_by_label("Undo").click();
    h.run();
    assert!(!h.state().can_undo() && h.state().can_redo());
    assert!(!h.get_by_label("Redo").accesskit_node().is_disabled());
    h.get_by_label("Redo").hover();
    h.run();
    h.get_by_label_contains("Redo: hair");
}

#[test]
fn the_status_line_says_what_was_undone_and_redone() {
    let d = tempfile::tempdir().unwrap();
    let mut m = with_a_step(d.path());
    m.undo();
    assert!(
        m.status_line.starts_with("Undid: hair."),
        "{}",
        m.status_line
    );
    // The plan that follows rewrites the status line; the note stays in front of it.
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    assert!(
        m.status_line.starts_with("Undid: hair."),
        "{}",
        m.status_line
    );
    m.redo();
    assert!(
        m.status_line.starts_with("Redid: hair."),
        "{}",
        m.status_line
    );
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    assert!(
        m.status_line.starts_with("Redid: hair."),
        "{}",
        m.status_line
    );
    // The next edit drops the note.
    m.edit_docs("again", None, false, |_, s| {
        s.set_value("landscape/rock01", "hair", Some(json!(false)))
    });
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    assert!(!m.status_line.contains("Redid"), "{}", m.status_line);
}

#[test]
fn undo_clears_the_encoding_note_and_keeps_the_export_cancelled_note() {
    let d = tempfile::tempdir().unwrap();
    let mut m = with_a_step(d.path());
    m.encoding_note = Some(("one texture".into(), "a.png".into()));
    m.status_note = Some("Export cancelled: edited.".into());
    m.undo();
    assert!(m.encoding_note.is_none());
    assert_eq!(m.status_note.as_deref(), Some("Export cancelled: edited."));
    assert!(
        m.status_line
            .starts_with("Undid: hair. Export cancelled: edited."),
        "{}",
        m.status_line
    );
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    assert!(
        m.status_line.contains("Export cancelled: edited."),
        "{}",
        m.status_line
    );
}

#[test]
fn the_undo_note_stands_in_front_of_the_counts_and_goes_with_the_next_plan_or_after_5_s() {
    let d = tempfile::tempdir().unwrap();
    let mut m = with_a_step(d.path());
    let counts = format!("{} textures, {} sets", m.items.len(), m.sets.len());
    m.undo();
    assert_eq!(m.status_line, format!("Undid: hair. {counts}"));
    // The plan the undo started shows it; the next plan drops it.
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    assert!(
        m.status_line.starts_with("Undid: hair. "),
        "{}",
        m.status_line
    );
    assert!(m.status_line.contains("textures"), "{}", m.status_line);
    m.replan();
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    assert!(!m.status_line.contains("Undid"), "{}", m.status_line);
    assert!(m.status_line.contains("textures"), "{}", m.status_line);
    // Or five seconds after it was set.
    m.redo();
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    m.tick(std::time::Instant::now() + std::time::Duration::from_secs(1));
    assert!(
        m.status_line.starts_with("Redid: hair."),
        "{}",
        m.status_line
    );
    m.tick(std::time::Instant::now() + std::time::Duration::from_secs(6));
    assert!(!m.status_line.contains("Redid"), "{}", m.status_line);
    assert!(m.status_line.contains("textures"), "{}", m.status_line);
}

#[test]
fn the_undo_note_asks_for_a_frame_when_it_runs_out() {
    use std::time::{Duration, Instant};
    let d = tempfile::tempdir().unwrap();
    let mut m = with_a_step(d.path());
    let t0 = Instant::now();
    assert_eq!(m.undo_note_left(t0), None, "no note, no repaint");
    m.undo();
    // An idle window gets a frame when the 5 s are over, so the note goes without waiting for input.
    let left = m
        .undo_note_left(t0 + Duration::from_secs(1))
        .expect("a note");
    assert!(
        left > Duration::from_secs(3) && left <= Duration::from_secs(5),
        "{left:?}"
    );
    let at_end = t0 + Duration::from_secs(1) + left + Duration::from_millis(50);
    m.tick(at_end);
    assert!(!m.status_line.contains("Undid"), "{}", m.status_line);
    assert_eq!(m.undo_note_left(at_end), None);
}

#[test]
fn ctrl_z_waits_while_a_sets_column_box_is_open() {
    let d = tempfile::tempdir().unwrap();
    let mut h = Harness::builder()
        .with_size(egui::vec2(400.0, 300.0))
        .build_ui_state(
            |ui, (m, open): &mut (AppModel, bool)| {
                sdc_gui::ui::actions::keys(ui.ctx(), m, *open);
            },
            (with_a_step(d.path()), true),
        );
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, Key::Z);
    h.run();
    assert!(h.state().0.can_undo(), "Ctrl+Z undid behind a box");
    h.state_mut().1 = false;
    h.key_press_modifiers(egui::Modifiers::COMMAND, Key::Z);
    h.run();
    assert!(!h.state().0.can_undo(), "Ctrl+Z works once the box is gone");
}

#[test]
fn enter_presses_ok_on_the_sets_column_refusal() {
    use sdc_gui::ui::sets_column::Pending;
    use sdc_gui::ui::State;
    let d = tempfile::tempdir().unwrap();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(
            |ui, (m, s): &mut (AppModel, State)| sdc_gui::ui::draw_with(ui, m, s),
            (opened(d.path()), State::default()),
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.state_mut().1.drops.pending = Some(Pending::Refused("no".into()));
    h.run();
    h.run();
    h.key_press(Key::Enter);
    h.run();
    assert!(h.state().1.drops.pending.is_none(), "Enter closed the box");
}

#[test]
fn the_undo_label_after_a_bulk_edit_is_the_bulk_steps_label() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.export_clicked();
    m.accept_all_suggestions();
    assert_eq!(m.undo_label(), Some("Accept all suggestions"));
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    m.undo();
    assert!(
        m.status_line.starts_with("Undid: Accept all suggestions."),
        "{}",
        m.status_line
    );
}

#[test]
fn ctrl_z_and_ctrl_y_work_in_the_meshes_view() {
    let d = tempfile::tempdir().unwrap();
    let mut h = Harness::builder()
        .with_size(egui::vec2(400.0, 300.0))
        .build_ui_state(
            |ui, m: &mut AppModel| {
                sdc_gui::ui::actions::keys(ui.ctx(), m, false);
            },
            with_a_step(d.path()),
        );
    h.ctx
        .data_mut(|d| d.insert_temp(egui::Id::new("sdc-meshes-view"), true));
    assert!(sdc_gui::ui::header::meshes_view_active(&h.ctx));
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, Key::Z);
    h.run();
    assert!(!h.state().can_undo(), "Ctrl+Z undid in the Meshes view");
    h.key_press_modifiers(egui::Modifiers::COMMAND, Key::Y);
    h.run();
    assert!(h.state().can_undo(), "Ctrl+Y redid");
}

// ---- item 4: dialog keys

fn dialog_after(m: AppModel, key: Key, first: bool) -> Option<Dialog> {
    let mut h = harness(m);
    h.run();
    h.run();
    if first {
        h.key_press(key);
    }
    h.run();
    h.state().dialog.clone()
}

#[test]
fn esc_closes_the_dialogs_that_use_the_shared_modal() {
    let d = tempfile::tempdir().unwrap();
    for dialog in [
        Dialog::Error("boom".into()),
        Dialog::CloseUnsaved,
        Dialog::ReloadUnsaved,
        Dialog::ConfirmResetAll(vec![]),
        Dialog::CopySettings {
            from: "a".into(),
            to: vec![],
            fields: vec![],
        },
        Dialog::OverwriteJson(d.path().join("x.json")),
    ] {
        let mut m = opened(d.path());
        m.dialog = Some(dialog.clone());
        assert_eq!(dialog_after(m, Key::Escape, true), None, "{dialog:?}");
    }
}

#[test]
fn enter_never_decides_a_decision_dialog() {
    let d = tempfile::tempdir().unwrap();
    // K7: Copy settings is the decision dialog where Enter does nothing (R18); Review's Enter is Task 1.4's test.
    let dialog = Dialog::CopySettings {
        from: "a".into(),
        to: vec![],
        fields: vec![],
    };
    let mut m = opened(d.path());
    m.dialog = Some(dialog.clone());
    assert_eq!(
        dialog_after(m, Key::Enter, true),
        Some(dialog.clone()),
        "Enter did something in {dialog:?}"
    );
}

#[test]
fn a_dialog_ignores_enter_on_the_frame_it_opens() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened(d.path()));
    h.run();
    h.run();
    h.state_mut().dialog = Some(Dialog::Error("boom".into()));
    h.key_press(Key::Enter);
    h.step();
    assert!(
        h.state().dialog.is_some(),
        "Enter on the opening frame is ignored"
    );
    h.run();
    h.key_press(Key::Enter);
    h.run();
    assert!(
        h.state().dialog.is_none(),
        "a later Enter takes the default"
    );
}

#[test]
fn enter_on_the_external_change_dialog_never_overwrites() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let root = m.root.clone().unwrap();
    m.edit_docs("hair", None, false, |_, s| {
        s.set_value("landscape/rock01", "hair", Some(json!(true)))
    });
    m.save().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    m.edit_docs("hair2", None, false, |_, s| {
        s.set_value("landscape/rock01", "hair", Some(json!(false)))
    });
    std::fs::write(root.join("sdc-project.toml"), "# edited elsewhere\n").unwrap();
    assert!(m.save().is_err());
    assert_eq!(m.dialog, Some(Dialog::ExternalChange(DocKind::Project)));
    let mut h = harness(m);
    h.run();
    h.run();
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(h.state().dialog, None, "Enter closed the dialog");
    assert_eq!(
        std::fs::read_to_string(root.join("sdc-project.toml")).unwrap(),
        "# edited elsewhere\n",
        "the file on disk is untouched"
    );
    assert!(h.state().can_undo(), "the edits in memory are kept");
}

// ---- item 4: Delete set confirm; item 23: the Rename submenu

fn with_a_created_set(dir: &std::path::Path) -> (AppModel, String) {
    let mut m = pbr_opened(dir);
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    let odd = m
        .items
        .iter()
        .find(|i| i.name == "odd.png")
        .unwrap()
        .key
        .clone();
    let id = m.drop_new_set(&odd).unwrap();
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    (m, id)
}

fn pill<'a>(h: &'a Harness<'static, AppModel>, id: &'a str) -> egui_kittest::Node<'a> {
    h.query_all_by_label(id).next().expect("the set pill")
}

fn has_set(m: &AppModel, id: &str) -> bool {
    sdc_gui::model::sets::set_views(m)
        .iter()
        .any(|s| s.id == id)
}

#[test]
fn delete_set_asks_cancel_keeps_it_and_delete_is_one_undo_step() {
    let d = tempfile::tempdir().unwrap();
    let (m, id) = with_a_created_set(d.path());
    let before = m.history.undo.len();
    let mut h = harness(m);
    h.run();
    // Cancel keeps the set.
    pill(&h, &id).click_secondary();
    h.run();
    h.get_by_label("Delete set").click();
    h.run();
    h.get_by_label_contains(&format!("Delete set '{id}'"));
    h.get_by_label_contains("Its textures stay; only the set is removed.");
    assert!(has_set(h.state(), &id), "nothing deleted before the answer");
    h.get_by_label("Cancel").click();
    h.run();
    assert!(has_set(h.state(), &id));
    assert_eq!(h.state().history.undo.len(), before);
    // Esc cancels too.
    pill(&h, &id).click_secondary();
    h.run();
    h.get_by_label("Delete set").click();
    h.run();
    h.run();
    h.key_press(Key::Escape);
    h.run();
    assert!(has_set(h.state(), &id));
    assert!(h.query_by_label_contains("Its textures stay").is_none());
    // Delete removes it, as one step.
    pill(&h, &id).click_secondary();
    h.run();
    h.get_by_label("Delete set").click();
    h.run();
    h.get_by_label("Delete").click();
    h.run();
    assert!(h.state_mut().wait_idle(std::time::Duration::from_secs(60)));
    assert!(!has_set(h.state(), &id));
    assert_eq!(h.state().history.undo.len(), before + 1);
    h.state_mut().undo();
    assert!(h.state_mut().wait_idle(std::time::Duration::from_secs(60)));
    assert!(has_set(h.state(), &id), "one undo brings it back");
}

#[test]
fn a_sets_menu_groups_the_three_renames_in_a_submenu_with_hover_text() {
    let d = tempfile::tempdir().unwrap();
    let (m, id) = with_a_created_set(d.path());
    let mut h = harness(m);
    h.run();
    pill(&h, &id).click_secondary();
    h.run();
    assert!(
        h.query_by_label("Rename outputs…  F2").is_none(),
        "the entries live in the submenu"
    );
    h.get_by_label_contains("Rename").click();
    h.run();
    h.get_by_label("Rename…");
    h.get_by_label("Rename outputs…  F2");
    h.get_by_label("Rename inputs…").hover();
    h.run();
    h.get_by_label_contains("Read the source files under other names");
}

#[test]
fn the_unlabelled_controls_have_hover_text() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened(d.path()));
    h.run();
    h.get_by_label_contains("Needs decision").hover();
    h.run();
    h.get_by_label_contains("still need a decision");
}

/// Trace-debug #10 (review M6): Enter on the Reload question cancels and keeps the edits; only the button reloads.
#[test]
fn reload_unsaved_enter_cancels_and_the_button_discards() {
    let d = tempfile::tempdir().unwrap();
    let mut m = with_a_step(d.path());
    assert!(m.is_dirty());
    m.dialog = Some(Dialog::ReloadUnsaved);
    let mut h = harness(m);
    h.run();
    h.run();
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(h.state().dialog, None, "Enter cancels");
    assert!(h.state().is_dirty(), "the edits are kept");
    assert!(h.state().can_undo());
    h.state_mut().request_reload();
    h.run();
    h.get_by_label("Discard and reload").click();
    h.run();
    assert_eq!(h.state().dialog, None);
    assert!(!h.state().is_dirty(), "the edits are dropped");
    assert!(!h.state().can_undo());
}

/// Trace-debug wave 2 #2: the Enter that picks "Rename" in the Ctrl+K palette must not also apply the rename the
/// dialog opens with (the last recipe). A later Enter applies it.
#[test]
fn the_rename_dialog_ignores_enter_on_the_frame_it_opens() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.click_item("landscape/leaf.png", false, false);
    m.last_recipe.prefix = "new_".into();
    let mut h = harness(m);
    h.run();
    h.run();
    h.state_mut()
        .open_rename(sdc_gui::model::rename::RenameMode::Inputs);
    h.key_press(Key::Enter);
    h.step();
    assert!(
        matches!(h.state().dialog, Some(Dialog::Rename(_))),
        "Enter on the opening frame is ignored"
    );
    assert_eq!(h.state().read_as_of("landscape/leaf.png"), None);
    h.run();
    h.key_press(Key::Enter);
    h.run();
    assert!(h.state().dialog.is_none(), "a later Enter applies");
    assert_eq!(
        h.state().read_as_of("landscape/leaf.png").as_deref(),
        Some("landscape/new_leaf.png")
    );
}

/// N3, R11: the Review tab adds no Enter handler, so Enter with Review open never starts an export.
#[test]
fn enter_with_review_open_never_exports() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened(d.path()));
    h.run();
    h.state_mut().export_clicked();
    h.run();
    assert!(h.state().review.is_some());
    for _ in 0..2 {
        h.key_press(Key::Enter);
        h.run();
    }
    assert!(h.state_mut().wait_idle(std::time::Duration::from_secs(120)));
    assert!(h.state().export.summary.is_none() && h.state().export.running.is_none());
}
