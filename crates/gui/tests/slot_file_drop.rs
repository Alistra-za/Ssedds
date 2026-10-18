//! Item 17 (Task 12): a file dropped from Explorer onto a set editor slot card fills that slot, through the move
//! command; nothing is ever copied.
mod support;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use sdc_gui::model::sets::{set_views, DropOutcome, SlotKind, ONLY_FIRST_FILE};
use sdc_gui::model::AppModel;
use sdc_gui::ui::State;
use std::path::{Path, PathBuf};
use std::time::Duration;
use support::*;

const LEAF: &str = "landscape/leaf";
const ROCK: &str = "landscape/rock01";
const NORMAL: &str = "landscape/rock01_normal.png";

fn idle(m: &mut AppModel) {
    assert!(m.wait_idle(Duration::from_secs(60)));
}

fn file(m: &AppModel, key: &str) -> PathBuf {
    m.root.as_ref().unwrap().join(key)
}

fn members(m: &AppModel, id: &str) -> Vec<String> {
    set_views(m)
        .into_iter()
        .find(|v| v.id == id)
        .unwrap()
        .members
}

fn files_in(dir: &Path) -> usize {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter(|e| e.as_ref().is_ok_and(|e| e.file_type().is_file()))
        .count()
}

#[test]
fn a_file_inside_the_folder_fills_the_slot_as_one_step() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    let before = (m.history.undo.len(), files_in(d.path()));
    let p = file(&m, NORMAL);
    let r = m.drop_file_on_slot(LEAF, SlotKind::Normal, &[p]);
    assert_eq!(r.outcome, DropOutcome::Joined);
    assert_eq!((r.item.as_deref(), r.note), (Some(NORMAL), None));
    idle(&mut m);
    assert!(members(&m, LEAF).contains(&NORMAL.to_string()));
    assert_eq!(
        (m.history.undo.len(), files_in(d.path())),
        (before.0 + 1, before.1)
    );
    m.undo();
    idle(&mut m);
    assert!(!members(&m, LEAF).contains(&NORMAL.to_string()));
}

#[test]
fn a_filled_slot_asks_first_and_a_locked_slot_refuses() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    let leaf = file(&m, "landscape/leaf.png");
    let r = m.drop_file_on_slot(ROCK, SlotKind::Diffuse, &[leaf]);
    assert_eq!(
        r.outcome,
        DropOutcome::AskReplace {
            existing: "landscape/rock01.png".into()
        }
    );
    m.set_slot_lock(LEAF, 2, true).unwrap();
    idle(&mut m);
    let undo = m.history.undo.len();
    let r = m.drop_file_on_slot(LEAF, SlotKind::Normal, &[file(&m, NORMAL)]);
    assert!(
        matches!(&r.outcome, DropOutcome::Refused(why) if why.contains("locked")),
        "{r:?}"
    );
    assert_eq!(m.history.undo.len(), undo);
}

#[test]
fn a_file_outside_the_folder_is_refused_and_never_copied() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    let other = tempfile::tempdir().unwrap();
    let outside = other.path().join("brick_n.png");
    std::fs::copy(file(&m, NORMAL), &outside).unwrap();
    let n = files_in(d.path());
    let r = m.drop_file_on_slot(LEAF, SlotKind::Normal, &[outside]);
    assert_eq!(
        r.outcome,
        DropOutcome::Refused(
            "brick_n.png is outside the open folder; files are never copied".into()
        )
    );
    assert_eq!(files_in(d.path()), n, "nothing was copied in");
}

#[test]
fn several_files_take_the_first_and_a_file_for_another_slot_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    let paths = [file(&m, NORMAL), file(&m, "landscape/leaf.png")];
    let r = m.drop_file_on_slot(LEAF, SlotKind::Normal, &paths);
    assert_eq!(r.note, Some(ONLY_FIRST_FILE));
    assert_eq!(r.item.as_deref(), Some(NORMAL));
    assert_eq!(r.outcome, DropOutcome::Joined);
    assert_eq!(ONLY_FIRST_FILE, "only the first file was used");
    let r = m.drop_file_on_slot(ROCK, SlotKind::Normal, &[file(&m, "landscape/leaf.png")]);
    assert!(
        matches!(&r.outcome, DropOutcome::Refused(why) if why.contains("Diffuse")),
        "{r:?}"
    );
}

#[derive(Debug)]
struct Dropped(PathBuf);

impl eframe::egui::DroppedFile for Dropped {
    fn path(&self) -> &Path {
        &self.0
    }
    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

fn harness(m: AppModel) -> Harness<'static, (AppModel, State)> {
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 900.0))
        .build_ui_state(
            |ui, (model, state): &mut (AppModel, State)| sdc_gui::ui::draw_with(ui, model, state),
            (m, State::default()),
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

fn drop_on(h: &mut Harness<'static, (AppModel, State)>, card: &str, path: PathBuf) {
    // K5: the slot row, not the set panel's Matching row of the same name.
    h.hover_at(slot_rect(h, card).center());
    h.run();
    h.input_mut()
        .dropped_files
        .push(std::sync::Arc::new(Dropped(path)));
    h.run();
}

#[test]
fn a_drop_on_a_slot_card_fills_it_and_an_outside_file_shows_the_error_line() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    let key = set_views(&m)
        .into_iter()
        .find(|v| v.id == LEAF)
        .unwrap()
        .key;
    m.select_set(&key);
    let mut h = harness(m);
    h.run();
    let p = file(&h.state().0, NORMAL);
    drop_on(&mut h, "Normal _n", p);
    idle(&mut h.state_mut().0);
    assert!(members(&h.state().0, LEAF).contains(&NORMAL.to_string()));
    let other = tempfile::tempdir().unwrap();
    let outside = other.path().join("brick.png");
    std::fs::write(&outside, b"x").unwrap();
    h.run();
    drop_on(&mut h, "Normal _n", outside);
    h.run();
    h.get_by_label("brick.png is outside the open folder; files are never copied");
    // A drop that landed on a slot does not also open the file's folder.
    assert_ne!(h.state().0.root.as_deref(), Some(other.path()));
}

#[test]
fn a_replace_asked_by_a_drop_shows_its_prompt_with_the_sets_column_hidden() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.settings.sets_hidden = true;
    let key = set_views(&m)
        .into_iter()
        .find(|v| v.id == ROCK)
        .unwrap()
        .key;
    m.select_set(&key);
    let mut h = harness(m);
    h.run();
    let leaf = file(&h.state().0, "landscape/leaf.png");
    drop_on(&mut h, "Diffuse", leaf);
    h.run();
    h.get_by_label("Replace?");
    h.get_by_label("Cancel").click();
    h.run();
    assert!(h.state().1.drops.pending.is_none());
}

/// Trace-debug #51: a drop above the set editor's scroll area, over a card scrolled out of view, opens the folder.
#[test]
fn drop_on_card_scrolled_out_of_view_opens_folder_not_slot() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    let key = set_views(&m)
        .into_iter()
        .find(|v| v.id == LEAF)
        .unwrap()
        .key;
    m.select_set(&key);
    let mut h = Harness::builder()
        // Layout v2 A3: the rows scroll alone (the material cards are in the panel), so a lower window.
        .with_size(eframe::egui::vec2(1400.0, 240.0))
        .build_ui_state(
            |ui, (model, state): &mut (AppModel, State)| sdc_gui::ui::draw_with(ui, model, state),
            (m, State::default()),
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    let card = slot_rect(&h, "Diffuse");
    // The set's header row, above the scroll area that holds the cards.
    let header = h
        .get(
            egui_kittest::kittest::by()
                .role(eframe::egui::accesskit::Role::TextInput)
                .label(sdc_gui::ui::input_name::HINT),
        )
        .rect();
    let y = header.center().y;
    h.hover_at(eframe::egui::pos2(card.center().x, card.bottom() + 40.0));
    h.event(eframe::egui::Event::MouseWheel {
        unit: eframe::egui::MouseWheelUnit::Point,
        delta: eframe::egui::vec2(0.0, -(card.top() - y + 30.0)),
        phase: eframe::egui::TouchPhase::Move,
        modifiers: eframe::egui::Modifiers::NONE,
    });
    for _ in 0..30 {
        h.step();
    }
    let moved = slot_rect(&h, "Diffuse");
    assert!(
        moved.top() < y,
        "the Diffuse card scrolled up under the header row: {card:?} -> {moved:?}, header {header:?}"
    );
    let other = tempfile::tempdir().unwrap();
    let outside = other.path().join("brick.png");
    std::fs::write(&outside, b"x").unwrap();
    h.hover_at(eframe::egui::pos2(moved.center().x, y));
    h.run();
    h.input_mut()
        .dropped_files
        .push(std::sync::Arc::new(Dropped(outside)));
    h.run();
    assert_eq!(h.state().1.drops.slot_note, None, "no slot card took it");
    // The fixture has unsaved edits, so opening the file's folder asks first.
    assert!(
        matches!(&h.state().0.dialog, Some(sdc_gui::model::Dialog::OpenUnsaved(p)) if p == other.path()),
        "{:?}",
        h.state().0.dialog
    );
}

/// Trace-debug #52: a folder change closes the Sets column's modal and the Slot commands window.
#[test]
fn a_folder_change_closes_the_sets_modals_and_slot_commands() {
    use sdc_gui::ui::sets_column::Pending;
    let d = tempfile::tempdir().unwrap();
    let m = pbr_opened(d.path());
    let mut h = harness(m);
    h.run();
    h.state_mut().1.drops.pending = Some(Pending::ConfirmDelete {
        id: LEAF.into(),
        members: 1,
    });
    h.state_mut().1.slot_cmds.open = Some(LEAF.into());
    h.run();
    let root = h.state().0.root.clone().unwrap();
    h.state_mut().0.open_folder(root);
    idle(&mut h.state_mut().0);
    h.run();
    assert!(h.state().1.drops.pending.is_none());
    assert!(h.state().1.slot_cmds.open.is_none());
}
