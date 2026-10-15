//! Task 11 (item 13, DECISIONS AG91-AG95): progress while a folder opens, Cancel, stale results, the scan row ETA.
mod support;

use sdc_gui::model::export::{ETA_MIN_FILES, ETA_MIN_TIME};
use sdc_gui::model::meshes::MeshesModel;
use sdc_gui::model::{plan_bar_text, AppModel};
use sdc_gui::settings::GuiSettings;
use sdc_gui::worker::{PlanFailure, WorkerMsg};
use std::time::{Duration, Instant};
use support::{mod_folder, opened, session};

fn progress(m: &mut AppModel, generation: u64, done: usize, total: usize) {
    m.sender()
        .send(WorkerMsg::PlanProgress {
            generation,
            done,
            total,
        })
        .unwrap();
    m.poll();
}

#[test]
fn progress_events_update_done_and_total() {
    let mut m = AppModel::placeholder(GuiSettings::default());
    let g = m.generation();
    progress(&mut m, g, 0, 5000);
    assert_eq!(m.plan_progress, Some((0, 5000)));
    progress(&mut m, g, 1234, 5000);
    assert_eq!(m.plan_progress, Some((1234, 5000)));
    assert_eq!(m.status_line, "Analysing 1,234 / 5,000 files…");
}

#[test]
fn the_bar_text_has_thousands_separators() {
    assert_eq!(plan_bar_text(1234, 5000), "Analysing 1,234 / 5,000 files…");
    assert_eq!(plan_bar_text(0, 7), "Analysing 0 / 7 files…");
    assert_eq!(
        plan_bar_text(999, 1_234_567),
        "Analysing 999 / 1,234,567 files…"
    );
}

#[test]
fn a_stale_generation_is_dropped() {
    let mut m = AppModel::placeholder(GuiSettings::default());
    let g = m.generation();
    progress(&mut m, g + 7, 3, 10);
    assert_eq!(m.plan_progress, None);
    assert!(m.status_line.is_empty());
}

#[test]
fn cancel_with_a_previous_folder_keeps_it() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let first = m.root.clone().unwrap();
    let items = m.items.len();
    assert!(items > 0);
    let od = tempfile::tempdir().unwrap();
    let other = mod_folder(od.path());
    let recent = m.settings.recent.clone();
    m.open_folder(other.clone());
    assert_eq!(m.settings.recent[0], other);
    assert!(m.can_cancel_open());
    assert_eq!(
        m.cancel_open_hover(),
        "Stops opening and re-opens the previous folder (selection and undo history are reset)"
    );
    let cancelled = m.generation();
    m.cancel_open();
    assert!(!m.can_cancel_open());
    assert_eq!(m.root.as_ref(), Some(&first));
    assert_eq!(
        m.settings.recent, recent,
        "the cancelled folder leaves Recent"
    );
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.items.len(), items);
    // A result of the cancelled generation (here a failure, which would raise a dialog) is dropped.
    progress(&mut m, cancelled, 1, 2);
    m.sender()
        .send(WorkerMsg::Planned {
            generation: cancelled,
            result: Err(PlanFailure {
                message: "stale".into(),
                config: false,
            }),
        })
        .unwrap();
    m.poll();
    assert_eq!(m.dialog, None);
    assert!(!m.status_line.contains("stale"));
    assert_eq!(m.root.as_ref(), Some(&first));
    assert_eq!(m.items.len(), items);
    assert_eq!(m.plan_progress, None);
}

#[test]
fn cancel_with_no_previous_folder_shows_the_empty_state_note() {
    let d = tempfile::tempdir().unwrap();
    let root = mod_folder(d.path());
    let mut m = AppModel::new(GuiSettings::default(), None, session());
    m.open_folder(root);
    assert_eq!(m.cancel_open_hover(), "Stops opening; no folder stays open");
    m.cancel_open();
    assert!(m.settings.recent.is_empty());
    assert_eq!(m.root, None);
    assert!(m.project.is_none());
    assert!(!m.busy());
    assert_eq!(m.empty_hint(), "Opening cancelled");
    // The cancelled worker still reports; its generation is stale and nothing lands.
    std::thread::sleep(Duration::from_millis(500));
    m.poll();
    assert_eq!(m.root, None);
    assert!(m.items.is_empty());
    assert_eq!(m.plan_progress, None);
}

#[test]
fn a_fresh_window_has_the_plain_hint() {
    let m = AppModel::placeholder(GuiSettings::default());
    assert_eq!(m.empty_hint(), "Open a folder of source textures to begin.");
}

#[test]
fn editing_is_disabled_while_planning() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    assert!(!m.busy());
    m.replan();
    assert!(m.busy(), "settings, bulk edit and export gate on busy()");
    assert!(
        !m.can_cancel_open(),
        "a re-plan of the open folder is not an open"
    );
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.plan_progress, None, "the bar goes when the plan lands");
}

#[test]
fn scan_eta_follows_the_pace_rules() {
    let mut m = MeshesModel::empty();
    let t0 = Instant::now();
    m.scan_progressed("reading", 0, 100, t0);
    m.scan_progressed("reading", 1, 100, t0 + Duration::from_millis(100));
    assert_eq!(m.scan_eta(t0 + Duration::from_millis(100)), None);
    let t = t0 + Duration::from_millis(500);
    m.scan_progressed("reading", ETA_MIN_FILES, 100, t);
    assert!(m.scan_eta(t).is_some(), "enough files have finished");
    // A new phase starts its own average.
    let t = t + Duration::from_millis(10);
    m.scan_progressed("indexing", 1, 50, t);
    assert_eq!(m.scan_eta(t), None);
    // ... until enough time has passed.
    let later = t + ETA_MIN_TIME;
    m.scan_progressed("indexing", 2, 50, later);
    assert!(m.scan_eta(later).is_some());
    assert!(m
        .scan_line(later)
        .unwrap()
        .starts_with("indexing 2/50 · ETA "));
}
