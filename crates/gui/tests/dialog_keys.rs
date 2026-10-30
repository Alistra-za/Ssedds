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

/// Trace #7: Ctrl+Shift+Z with nothing to redo is Redo (disabled), never Undo.
#[test]
fn ctrl_shift_z_with_nothing_to_redo_does_not_undo() {
    let d = tempfile::tempdir().unwrap();
    let mut h = Harness::builder()
        .with_size(egui::vec2(400.0, 300.0))
        .build_ui_state(
            |ui, m: &mut AppModel| {
                sdc_gui::ui::actions::keys(ui.ctx(), m, false);
            },
            with_a_step(d.path()),
        );
    h.run();
    assert!(h.state().can_undo() && !h.state().can_redo());
    h.key_press_modifiers(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, Key::Z);
    h.run();
    assert!(h.state().can_undo(), "Ctrl+Shift+Z undid");
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

/// Note FC5 (supersedes AG13's confirm): Delete set asks nothing; it is one undo step and a toast whose Undo brings
/// the set back.
#[test]
fn delete_set_has_no_dialog_and_undoes() {
    let d = tempfile::tempdir().unwrap();
    let (m, id) = with_a_created_set(d.path());
    let before = m.history.undo.len();
    let members = sdc_gui::model::sets::set_views(&m)
        .iter()
        .find(|s| s.id == id)
        .unwrap()
        .members
        .len();
    // The toasts are drawn by `app.rs` after `ui::draw`; this harness draws both, as a frame does.
    let mut h = Harness::builder()
        .with_size(egui::vec2(1500.0, 900.0))
        .with_step_dt(0.05)
        .build_ui_state(
            |ui, m: &mut AppModel| {
                sdc_gui::ui::draw(ui, m);
                sdc_gui::ui::messages::draw(ui.ctx(), m);
            },
            m,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    pill(&h, &id).click_secondary();
    h.run();
    h.get_by_label("Delete set").click();
    // `step`, not `run`: the toast keeps asking for frames while it shows.
    h.step();
    assert!(h.state_mut().wait_idle(std::time::Duration::from_secs(60)));
    h.step();
    assert!(h.state().dialog.is_none());
    assert!(
        h.query_by_label_contains("Its textures stay").is_none(),
        "no question"
    );
    assert!(!has_set(h.state(), &id));
    assert_eq!(h.state().history.undo.len(), before + 1);
    let s = if members == 1 { "" } else { "s" };
    for _ in 0..6 {
        h.step();
    }
    h.get_by_label(&format!("Deleted set {id} ({members} member{s})"));
    h.get_by_label("Undo the deleted set").click();
    for _ in 0..3 {
        h.step();
    }
    assert!(h.state_mut().wait_idle(std::time::Duration::from_secs(60)));
    assert!(has_set(h.state(), &id), "the toast's Undo brings it back");
    assert_eq!(h.state().history.undo.len(), before);
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
    h.get_by_label_contains("Undecided").hover();
    h.run();
    h.get_by_label_contains("wait for your answer");
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

// ---- layout v2 A7 (R18): one dialog frame, the Enter default drawn filled, clearer buttons

/// Every rectangle painted in the last frame.
fn painted_rects(h: &Harness<'static, AppModel>) -> Vec<egui::epaint::RectShape> {
    fn walk(s: &egui::Shape, out: &mut Vec<egui::epaint::RectShape>) {
        match s {
            egui::Shape::Rect(r) => out.push(r.clone()),
            egui::Shape::Vec(v) => v.iter().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    let mut out = vec![];
    for c in &h.output().shapes {
        walk(&c.shape, &mut out);
    }
    out
}

/// The dialog's button `label`: the lowest match on screen (the header has a Save of its own).
fn dialog_button<S>(h: &Harness<'static, S>, label: &str) -> egui::Rect {
    h.query_all_by_label(label)
        .map(|n| n.rect())
        .max_by(|a, b| a.center().y.total_cmp(&b.center().y))
        .unwrap_or_else(|| panic!("no {label} button"))
}

#[test]
fn the_enter_default_is_rightmost_and_filled() {
    use sdc_gui::ui::status::RED;
    let d = tempfile::tempdir().unwrap();
    let mut m = with_a_step(d.path());
    m.dialog = Some(Dialog::CloseUnsaved);
    let mut h = harness(m);
    h.run();
    h.run();
    // Note FC5: Cancel is the Enter default; Save and Don't save need a click.
    let (save, discard, cancel) = (
        dialog_button(&h, "Save"),
        dialog_button(&h, "Don't save"),
        dialog_button(&h, "Cancel"),
    );
    assert!(
        cancel.center().x > discard.center().x && discard.center().x > save.center().x,
        "the footer reads Save, Don't save, Cancel: {save:?} {discard:?} {cancel:?}"
    );
    let rects = painted_rects(&h);
    let fill = h.ctx.style_of(h.ctx.theme()).visuals.selection.bg_fill;
    let red = egui::Stroke::new(1.5, RED);
    let filled = |b: egui::Rect| {
        rects
            .iter()
            .any(|r| r.fill == fill && r.rect.contains(b.center()))
    };
    let outlined = |b: egui::Rect| {
        rects
            .iter()
            .any(|r| r.stroke == red && r.rect.contains(b.center()))
    };
    assert!(
        filled(cancel),
        "Cancel, the Enter default, is filled with the accent"
    );
    assert!(
        outlined(discard),
        "Don't save has the red destructive outline"
    );
    assert!(
        !filled(discard) && !filled(save),
        "only the default is filled"
    );
    assert!(
        !outlined(save) && !outlined(cancel),
        "only the destructive one is outlined"
    );
}

/// Two quiet frames, then Enter and one more frame.
fn press_enter<S>(h: &mut Harness<'static, S>) {
    h.run();
    h.run();
    h.key_press(Key::Enter);
    h.run();
}

fn docs_text(root: &std::path::Path) -> String {
    ["sdc.toml", "sdc-project.toml"]
        .iter()
        .map(|f| std::fs::read_to_string(root.join(f)).unwrap_or_default())
        .collect()
}

/// R18 / AG11: the Enter default is drawn, never changed. Each row is a dialog and what its Enter did before A7.
/// presets-discard's row is in presets_window.rs (its harness), set-rename's and fill-names' keys are pinned in
/// set_editor_ui.rs and fill_from_names.rs, the rename dialog's below.
#[test]
fn every_dialogs_enter_still_presses_what_it_pressed() {
    // Close / Open unsaved: `enter_in_unsaved_dialog_cancels` (Note FC5).
    // ReloadUnsaved: Enter cancels.
    let d = tempfile::tempdir().unwrap();
    let mut m = with_a_step(d.path());
    m.dialog = Some(Dialog::ReloadUnsaved);
    let mut h = harness(m);
    press_enter(&mut h);
    assert!(
        h.state().dialog.is_none() && h.state().is_dirty(),
        "ReloadUnsaved: Enter cancelled"
    );
    // ExternalChange: Enter cancels, never Reload or Overwrite.
    let d = tempfile::tempdir().unwrap();
    let mut m = with_a_step(d.path());
    let root = m.root.clone().unwrap();
    let before = docs_text(&root);
    m.dialog = Some(Dialog::ExternalChange(DocKind::Project));
    let mut h = harness(m);
    press_enter(&mut h);
    assert!(h.state().dialog.is_none(), "ExternalChange: closed");
    assert!(
        h.state().is_dirty() && h.state().can_undo(),
        "ExternalChange: the edits are kept"
    );
    assert_eq!(docs_text(&root), before, "ExternalChange: nothing written");
    every_other_dialogs_enter();
}

/// Note FC5 (supersedes AG11/CE3's Enter = Save): Close / Open unsaved: Enter cancels, back to the app.
#[test]
fn enter_in_unsaved_dialog_cancels() {
    let d = tempfile::tempdir().unwrap();
    let mut m = with_a_step(d.path());
    m.dialog = Some(Dialog::CloseUnsaved);
    let mut h = harness(m);
    press_enter(&mut h);
    assert!(
        h.state().dialog.is_none() && h.state().is_dirty(),
        "CloseUnsaved: Enter cancelled"
    );
    let d = tempfile::tempdir().unwrap();
    let other_dir = tempfile::tempdir().unwrap();
    let other = mod_folder(other_dir.path());
    let mut m = with_a_step(d.path());
    let root = m.root.clone().unwrap();
    let before = docs_text(&root);
    m.dialog = Some(Dialog::OpenUnsaved(other.clone()));
    let mut h = harness(m);
    press_enter(&mut h);
    assert!(h.state().dialog.is_none(), "OpenUnsaved: closed");
    assert_eq!(docs_text(&root), before, "OpenUnsaved: Enter saved nothing");
    assert_eq!(
        h.state().root.as_deref(),
        Some(root.as_path()),
        "OpenUnsaved: and opened nothing"
    );
    assert!(h.state().is_dirty(), "OpenUnsaved: the edits are kept");
}

/// The rest of `every_dialogs_enter_still_presses_what_it_pressed`.
fn every_other_dialogs_enter() {
    use sdc_gui::model::AfterExport;
    // ExportRunning: Enter keeps exporting.
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    export_through(&mut m);
    assert!(!m.request_close());
    assert_eq!(m.dialog, Some(Dialog::ExportRunning(AfterExport::Close)));
    let mut h = harness(m);
    press_enter(&mut h);
    assert!(h.state().dialog.is_none(), "ExportRunning: closed");
    assert!(
        !h.state().export.running.as_ref().unwrap().is_cancelled(),
        "ExportRunning: Enter kept exporting"
    );
    assert!(h.state_mut().wait_idle(std::time::Duration::from_secs(120)));
    // (OverwriteJson is gone: Review asks before the export, Note FC5.)
    // Error: Enter is OK.
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.dialog = Some(Dialog::Error("boom".into()));
    let mut h = harness(m);
    press_enter(&mut h);
    assert!(h.state().dialog.is_none(), "Error: Enter closed it");
    // ConfirmResetAll: Enter cancels.
    let d = tempfile::tempdir().unwrap();
    let mut m = with_a_step(d.path());
    let steps = m.history.undo.len();
    m.dialog = Some(Dialog::ConfirmResetAll(vec!["landscape/rock01".into()]));
    let mut h = harness(m);
    press_enter(&mut h);
    assert!(h.state().dialog.is_none(), "ConfirmResetAll: closed");
    assert_eq!(
        h.state().history.undo.len(),
        steps,
        "ConfirmResetAll: nothing reset"
    );
    // CopySettings: Enter does nothing, even with Apply enabled.
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let copy = Dialog::CopySettings {
        from: "landscape/rock01".into(),
        to: vec!["landscape/leaf.png".into()],
        fields: vec![("resize".into(), true)],
    };
    m.dialog = Some(copy.clone());
    let mut h = harness(m);
    press_enter(&mut h);
    assert_eq!(
        h.state().dialog,
        Some(copy),
        "CopySettings: Enter did nothing"
    );
}

/// The sets column's dialogs and Slot commands, which live in `ui::State`.
#[test]
fn the_set_dialogs_enter_still_presses_what_it_pressed() {
    use sdc_gui::model::sets::DropChoice;
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
    let steps = h.state().0.history.undo.len();
    // Enter does nothing on a Replace (K1) or a Choice; Esc cancels both.
    for p in [
        Pending::Replace {
            item: "landscape/leaf.png".into(),
            set: "s".into(),
            existing: "landscape/rock01".into(),
            choice: DropChoice::Move,
        },
        Pending::Choice {
            item: "landscape/leaf.png".into(),
            set: "s".into(),
            from: "t".into(),
        },
    ] {
        h.state_mut().1.drops.pending = Some(p.clone());
        press_enter(&mut h);
        assert_eq!(
            h.state().1.drops.pending,
            Some(p.clone()),
            "Enter did nothing: {p:?}"
        );
        h.key_press(Key::Escape);
        h.run();
        assert_eq!(h.state().1.drops.pending, None, "Esc cancels: {p:?}");
    }
    // (ConfirmDelete is gone: Delete set asks nothing, Note FC5.)
    assert_eq!(h.state().0.history.undo.len(), steps);
    // Slot commands: Enter does nothing; Esc closes it (A2).
    h.state_mut().1.slot_cmds.open = Some("s".into());
    press_enter(&mut h);
    assert_eq!(
        h.state().1.slot_cmds.open.as_deref(),
        Some("s"),
        "Slot commands: Enter did nothing"
    );
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(
        h.state().1.slot_cmds.open,
        None,
        "Slot commands: Esc closed it"
    );
}

/// K4: the rename dialog takes the shared keys. Enter right after typing in Find (the field gives its focus up on
/// that Enter) applies.
#[test]
fn enter_right_after_typing_in_find_applies_the_rename() {
    use egui::accesskit::Role;
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.click_item("landscape/leaf.png", false, false);
    m.last_recipe.replace = "twig".into();
    let mut h = harness(m);
    h.run();
    h.state_mut()
        .open_rename(sdc_gui::model::rename::RenameMode::Inputs);
    h.run();
    h.run();
    // The Find field: the first text box right of the "Find" label, on its row.
    let label = h.get_by_label("Find").rect();
    let find = h
        .query_all_by_role(Role::TextInput)
        .map(|n| n.rect())
        .filter(|r| (r.center().y - label.center().y).abs() < 6.0 && r.min.x > label.max.x)
        .min_by(|a, b| a.min.x.total_cmp(&b.min.x))
        .expect("the Find field");
    h.query_all_by_role(Role::TextInput)
        .find(|n| n.rect() == find)
        .unwrap()
        .focus();
    h.run();
    h.query_all_by_role(Role::TextInput)
        .find(|n| n.rect() == find)
        .unwrap()
        .type_text("leaf");
    h.run();
    h.key_press(Key::Enter);
    h.run();
    assert!(h.state().dialog.is_none(), "Enter applied");
    assert_eq!(
        h.state().read_as_of("landscape/leaf.png").as_deref(),
        Some("landscape/twig.png")
    );
}

/// Note FC5 (supersedes CZ4's "other Destructive buttons unchanged"): Enter or Space on a focused Destructive button
/// in any `dialogs::dialog` does nothing; a click still does. One row per dialog with a Destructive button (the
/// `Role::Destructive` uses in `ui/`): Reload, Changed on disk, Reset all, Delete generated file, Export running,
/// Close / Open unsaved, the sets column's Replace and the presets Discard. The Meshes view's NIF guard dialog
/// (Overwrite) is pinned in `nif_edit::enter_or_space_on_focused_export_or_overwrite_writes_nothing`.
#[test]
fn focused_destructive_buttons_ignore_enter_and_space() {
    use sdc_gui::model::sets::DropChoice;
    use sdc_gui::model::AfterExport;
    use sdc_gui::ui::sets_column::Pending;
    use sdc_gui::ui::State;
    // Focus the dialog's `label` button, press `key`, and give it two frames.
    fn press_on<S>(h: &mut Harness<'static, S>, label: &str, key: Key) {
        h.run();
        h.run();
        let r = dialog_button(h, label);
        h.query_all_by_label(label)
            .find(|n| n.rect() == r)
            .unwrap()
            .focus();
        h.run();
        h.key_press(key);
        h.run();
    }
    for key in [Key::Enter, Key::Space] {
        // Reload: the edits stay.
        let d = tempfile::tempdir().unwrap();
        let mut m = with_a_step(d.path());
        m.dialog = Some(Dialog::ReloadUnsaved);
        let mut h = harness(m);
        press_on(&mut h, "Discard and reload", key);
        assert!(h.state().is_dirty(), "ReloadUnsaved {key:?}: edits kept");
        // Changed on disk: the file stays.
        let d = tempfile::tempdir().unwrap();
        let mut m = with_a_step(d.path());
        let root = m.root.clone().unwrap();
        let before = docs_text(&root);
        m.dialog = Some(Dialog::ExternalChange(DocKind::Project));
        let mut h = harness(m);
        press_on(&mut h, "Overwrite with my changes", key);
        assert_eq!(
            docs_text(&root),
            before,
            "ExternalChange {key:?}: nothing written"
        );
        // Reset all: nothing reset.
        let mut m = with_a_step(d.path());
        let steps = m.history.undo.len();
        m.dialog = Some(Dialog::ConfirmResetAll(vec!["landscape/rock01".into()]));
        let mut h = harness(m);
        press_on(&mut h, "Reset", key);
        assert_eq!(h.state().history.undo.len(), steps, "ResetAll {key:?}");
        // Delete generated file: the file stays.
        let mut m = opened(d.path());
        let f = d.path().join("gen.png");
        std::fs::write(&f, b"x").unwrap();
        m.dialog = Some(Dialog::ConfirmDeleteGenerated(Box::new(
            sdc_gui::model::from_normal::DeleteGenerated {
                key: "gen.png".into(),
                name: "gen.png".into(),
                path: f.clone(),
                body: "Delete gen.png?".into(),
                refused: None,
                seen: None,
            },
        )));
        let mut h = harness(m);
        press_on(&mut h, "Delete gen.png", key);
        assert!(f.exists(), "DeleteGenerated {key:?}: file kept");
        // Export running: the export keeps going.
        let d = tempfile::tempdir().unwrap();
        let mut m = opened(d.path());
        export_through(&mut m);
        assert!(!m.request_close());
        assert_eq!(m.dialog, Some(Dialog::ExportRunning(AfterExport::Close)));
        let mut h = harness(m);
        press_on(&mut h, "Stop the export and close", key);
        assert!(
            !h.state().export.running.as_ref().unwrap().is_cancelled(),
            "ExportRunning {key:?}: not stopped"
        );
        assert!(h.state_mut().wait_idle(std::time::Duration::from_secs(120)));
        // Close / Open unsaved: Don't save drops the edits.
        let d = tempfile::tempdir().unwrap();
        let mut m = with_a_step(d.path());
        m.dialog = Some(Dialog::CloseUnsaved);
        let mut h = harness(m);
        press_on(&mut h, "Don't save", key);
        assert!(h.state().is_dirty(), "CloseUnsaved {key:?}: edits kept");
        let other_dir = tempfile::tempdir().unwrap();
        let other = mod_folder(other_dir.path());
        let mut m = with_a_step(d.path());
        let root = m.root.clone();
        m.dialog = Some(Dialog::OpenUnsaved(other));
        let mut h = harness(m);
        press_on(&mut h, "Don't save", key);
        assert_eq!(h.state().root, root, "OpenUnsaved {key:?}: nothing opened");
        assert!(h.state().is_dirty(), "OpenUnsaved {key:?}: edits kept");
        // Sets column Replace: the slot keeps its texture and the question stays.
        let d = tempfile::tempdir().unwrap();
        let mut h = Harness::builder()
            .with_size(egui::vec2(1500.0, 900.0))
            .build_ui_state(
                |ui, (m, s): &mut (AppModel, State)| sdc_gui::ui::draw_with(ui, m, s),
                (opened(d.path()), State::default()),
            );
        sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
        let steps = h.state().0.history.undo.len();
        let replace = Pending::Replace {
            item: "landscape/leaf.png".into(),
            set: "s".into(),
            existing: "landscape/rock01".into(),
            choice: DropChoice::Move,
        };
        h.state_mut().1.drops.pending = Some(replace.clone());
        h.run();
        h.run();
        let label = h
            .query_all_by_label_contains("Replace ")
            .filter_map(|n| n.accesskit_node().label())
            .next_back()
            .expect("the Replace button");
        press_on(&mut h, &label, key);
        assert_eq!(h.state().0.history.undo.len(), steps, "Replace {key:?}");
        assert_eq!(h.state().1.drops.pending, Some(replace), "Replace {key:?}");
        // Presets Discard: the edits stay.
        let p = tempfile::tempdir().unwrap();
        let mut m =
            sdc_gui::model::AppModel::placeholder(sdc_gui::settings::GuiSettings::default());
        m.user_presets = Some(p.path().to_path_buf());
        m.open_presets();
        m.presets_model().select("cs-quality");
        m.presets_model().duplicate();
        m.presets_model().set_id("edited");
        m.presets_model().set_field(
            engine::config::Subtype::Diffuse,
            "resize",
            serde_json::json!("half"),
        );
        let mut h = harness(m);
        h.run();
        h.get_by_label("New").click();
        h.run();
        h.get_by_label("Discard unsaved changes to edited?");
        press_on(&mut h, "Discard", key);
        assert_eq!(
            h.state_mut().presets_model().id(),
            "edited",
            "presets Discard {key:?}: edits kept"
        );
    }
    // A pointer click still works.
    let d = tempfile::tempdir().unwrap();
    let mut m = with_a_step(d.path());
    m.dialog = Some(Dialog::ReloadUnsaved);
    let mut h = harness(m);
    h.run();
    h.run();
    h.get_by_label("Discard and reload").click();
    h.run();
    assert!(!h.state().is_dirty(), "a click discards");
}

#[test]
fn the_footer_order_is_plain_then_destructive_then_the_default() {
    use sdc_gui::ui::dialogs::{footer_order, Role::*};
    assert_eq!(footer_order(&[Default, Plain, Destructive]), vec![1, 2, 0]);
    assert_eq!(footer_order(&[Destructive, Default]), vec![0, 1]);
    assert_eq!(footer_order(&[Plain, Plain]), vec![0, 1]);
    assert_eq!(
        footer_order(&[Destructive, Plain, Default, Plain]),
        vec![1, 3, 0, 2]
    );
}

/// I1: the modal `area` over 8 frames: its height settles by frame 4, and every node whose label contains one of
/// `parts` lies on screen.
fn assert_dialog_settles_on_screen<S>(h: &mut Harness<'static, S>, area: &str, parts: &[&str]) {
    let id = egui::Id::new(area);
    let mut heights = vec![];
    for _ in 0..8 {
        h.step();
        let r = h
            .ctx
            .memory(|m| m.area_rect(id))
            .expect("the dialog is open");
        heights.push(r.height());
    }
    assert!(
        heights[3..].iter().all(|x| (x - heights[3]).abs() < 0.5),
        "{area}: the height settles: {heights:?}"
    );
    let screen = h.ctx.content_rect();
    for part in parts {
        let rects: Vec<egui::Rect> = h
            .query_all_by_label_contains(part)
            .map(|n| n.rect())
            .collect();
        assert!(!rects.is_empty(), "{area}: no {part:?}");
        for r in rects {
            assert!(
                screen.contains_rect(r),
                "{area}: {part:?} at {r:?} is off {screen:?}"
            );
        }
    }
}

#[test]
fn a_long_texture_name_keeps_the_replace_dialog_on_screen() {
    use sdc_gui::model::sets::DropChoice;
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
    let existing = format!("textures/{}.png", "t".repeat(111));
    h.state_mut().1.drops.pending = Some(Pending::Replace {
        item: "landscape/leaf.png".into(),
        set: "s".into(),
        existing: existing.clone(),
        choice: DropChoice::Move,
    });
    assert_dialog_settles_on_screen(
        &mut h,
        "set-drop",
        &["Replace?", "already has", "Replace t", "Cancel"],
    );
    h.get_by_label_contains(&format!("s already has {existing} in"));
}

/// Owner 2026-10-19: the rename table has no New column (Output shows the result), and Inputs has a Whole path
/// toggle that makes Find see the folders.
#[test]
fn the_rename_dialog_has_no_new_column_and_a_whole_path_toggle() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.click_item("landscape/leaf.png", false, false);
    let mut h = harness(m);
    h.run();
    h.state_mut()
        .open_rename(sdc_gui::model::rename::RenameMode::Inputs);
    h.run();
    assert!(h.query_by_label("New").is_none(), "no New column");
    h.get_by_label("Output");
    h.get_by_label("Whole path").click();
    h.run();
    match &h.state().dialog {
        Some(Dialog::Rename(r)) => assert!(r.recipe.path, "the toggle sets the recipe"),
        other => panic!("expected the rename dialog, got {other:?}"),
    }
    h.state_mut()
        .set_rename_mode(sdc_gui::model::rename::RenameMode::Outputs);
    h.run();
    assert!(
        h.query_by_label("Whole path").is_none(),
        "Outputs renames base names only"
    );
}
