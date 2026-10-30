//! Note FB5 (owner amendment E): a crash's `.recovery` file restores itself on the next open, as an unsaved change
//! with one Undo step and a toast. Only a user's save deletes the file. No banner, except beside a document that
//! is read-only. Every folder here is a temp dir.
mod support;
use sdc_gui::crash_log::recovery_path;
use sdc_gui::model::messages::ToastAction;
use sdc_gui::model::AppModel;
use sdc_gui::settings::GuiSettings;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use support::*;

const ORIGINAL: &str = "target = \"vanilla\"\n";
const CRASHED: &str = "target = \"cs\"\n";
const TOAST: &str = "Restored unsaved edits from the crash";

fn age(p: &Path, secs: u64) {
    std::fs::File::options()
        .write(true)
        .open(p)
        .unwrap()
        .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
        .unwrap();
}

/// `sdc.toml` = `doc` (older), `sdc.toml.recovery` = `rec` (newer), then the folder is opened.
fn crashed(dir: &Path, doc: &str, rec: &str) -> (AppModel, PathBuf, PathBuf) {
    let root = mod_folder(dir);
    let file = root.join("sdc.toml");
    std::fs::write(&file, doc).unwrap();
    age(&file, 86_400);
    let rec_file = recovery_path(&file);
    std::fs::write(&rec_file, rec).unwrap();
    (open(root), file, rec_file)
}

fn open(root: PathBuf) -> AppModel {
    let settings = GuiSettings {
        export_folder: Some(root.join("export")),
        ..GuiSettings::default()
    };
    let mut m = AppModel::new(settings, None, session());
    // These pin the recovery file's text; Note FC1's recording of the High types is pinned in auto_accept.rs.
    m.accept = engine::policy::AcceptSuggestions::None;
    m.open_folder(root);
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
    m
}

fn text(m: &AppModel) -> String {
    m.project.as_ref().unwrap().text()
}

fn restore_toast(m: &AppModel) -> bool {
    m.messages.toasts().iter().any(|t| t.text == TOAST)
}

#[test]
fn crash_edits_come_back_on_open() {
    let d = tempfile::tempdir().unwrap();
    let (m, file, rec) = crashed(d.path(), ORIGINAL, CRASHED);
    assert_eq!(text(&m), CRASHED);
    assert!(m.is_dirty());
    assert!(restore_toast(&m), "{:?}", m.messages.toasts());
    let t = m
        .messages
        .toasts()
        .iter()
        .find(|t| t.text == TOAST)
        .unwrap();
    assert_eq!(t.action, Some(ToastAction::UndoCrashRestore));
    assert!(
        !m.banner.as_deref().unwrap_or("").contains(".recovery"),
        "{:?}",
        m.banner
    );
    assert!(rec.exists(), "the file stays until the user saves");
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        ORIGINAL,
        "the original is untouched"
    );
    assert_eq!(m.undo_label(), Some("Restore crash edits"));
}

#[test]
fn saving_deletes_the_recovery_file() {
    let d = tempfile::tempdir().unwrap();
    let (mut m, file, rec) = crashed(d.path(), ORIGINAL, CRASHED);
    assert!(rec.exists());
    m.save().unwrap();
    assert!(!rec.exists());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), CRASHED);
    assert!(!m.is_dirty());
}

#[test]
fn toast_undo_reverts_and_deletes_the_file() {
    let d = tempfile::tempdir().unwrap();
    let (mut m, _, rec) = crashed(d.path(), ORIGINAL, CRASHED);
    m.undo_crash_restore();
    assert_eq!(text(&m), ORIGINAL);
    assert!(!m.is_dirty(), "back at the saved text");
    assert!(!rec.exists(), "the user rejected the recovered edits");
}

const SETS_SAVED: &str = "# saved\n";
const SETS_CRASHED: &str = "# crashed\n";

/// Both documents saved (older) and each with a `.recovery` file (newer); then the folder is opened.
fn crashed_both(dir: &Path, doc: &str, rec: &str, sets_rec: &str) -> (AppModel, PathBuf, PathBuf) {
    let root = mod_folder(dir);
    let sets = root.join("sdc-project.toml");
    std::fs::write(&sets, SETS_SAVED).unwrap();
    age(&sets, 86_400);
    let sets_rec_file = recovery_path(&sets);
    std::fs::write(&sets_rec_file, sets_rec).unwrap();
    let file = root.join("sdc.toml");
    std::fs::write(&file, doc).unwrap();
    age(&file, 86_400);
    let rec_file = recovery_path(&file);
    std::fs::write(&rec_file, rec).unwrap();
    (open(root), rec_file, sets_rec_file)
}

fn sets_text(m: &AppModel) -> String {
    m.sets_doc.as_ref().unwrap().text()
}

/// Both documents restore in one Undo step, and the toast's Undo takes both back and deletes both files.
#[test]
fn both_documents_restore_as_one_step_and_the_toast_undo_deletes_both_files() {
    let d = tempfile::tempdir().unwrap();
    let (mut m, rec, sets_rec) = crashed_both(d.path(), ORIGINAL, CRASHED, SETS_CRASHED);
    assert_eq!(
        (text(&m).as_str(), sets_text(&m).as_str()),
        (CRASHED, SETS_CRASHED)
    );
    assert_eq!(m.undo_label(), Some("Restore crash edits"));
    m.undo_crash_restore();
    assert_eq!(
        (text(&m).as_str(), sets_text(&m).as_str()),
        (ORIGINAL, SETS_SAVED)
    );
    assert!(!m.is_dirty());
    assert!(!m.can_undo(), "it was the only step");
    assert!(!rec.exists() && !sets_rec.exists());
}

/// A recovery file that does not parse restores nothing: the toast's Undo must not delete a file whose edits were
/// never applied.
#[test]
fn an_unparseable_recovery_file_survives_the_toast_undo() {
    let d = tempfile::tempdir().unwrap();
    let (mut m, rec, sets_rec) = crashed_both(d.path(), ORIGINAL, CRASHED, "target = = cut off");
    assert_eq!(text(&m), CRASHED, "the valid one is restored");
    assert_eq!(sets_text(&m), SETS_SAVED);
    assert!(restore_toast(&m));
    m.undo_crash_restore();
    assert_eq!(text(&m), ORIGINAL);
    assert!(!rec.exists(), "the restored file goes");
    assert_eq!(
        std::fs::read_to_string(&sets_rec).unwrap(),
        "target = = cut off"
    );
}

/// A read-only `sdc.toml` keeps its file; the sets document still restores.
#[test]
fn a_read_only_project_does_not_stop_the_sets_restore() {
    let d = tempfile::tempdir().unwrap();
    let (mut m, rec, sets_rec) =
        crashed_both(d.path(), "target = = broken\n", CRASHED, SETS_CRASHED);
    assert!(m.project.as_ref().unwrap().read_only());
    assert_eq!(sets_text(&m), SETS_CRASHED);
    assert!(restore_toast(&m));
    assert_eq!(std::fs::read_to_string(&rec).unwrap(), CRASHED, "untouched");
    m.undo_crash_restore();
    assert_eq!(sets_text(&m), SETS_SAVED);
    assert!(!sets_rec.exists());
    assert_eq!(
        std::fs::read_to_string(&rec).unwrap(),
        CRASHED,
        "still untouched"
    );
}

/// The toast's own `Undo` button does the same through the drawn toast, and the toast goes.
#[test]
fn the_drawn_toast_has_an_undo_button_that_rejects_the_recovery() {
    use eframe::egui;
    use egui_kittest::kittest::Queryable;
    let d = tempfile::tempdir().unwrap();
    let (m, _, rec) = crashed(d.path(), ORIGINAL, CRASHED);
    let t0 = std::time::Instant::now();
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(800.0, 600.0))
        .with_step_dt(0.05)
        .build_ui_state(
            move |ui, m: &mut AppModel| sdc_gui::ui::messages::draw_at(ui.ctx(), m, t0),
            m,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    for _ in 0..6 {
        h.step();
    }
    h.get_by_label(TOAST);
    h.get_by_label("Undo the restored crash edits").click();
    for _ in 0..3 {
        h.step();
    }
    assert_eq!(text(h.state()), ORIGINAL);
    assert!(!rec.exists());
    assert!(h.query_by_label(TOAST).is_none(), "the toast went");
}

#[test]
fn toast_undo_after_another_edit_leaves_everything() {
    let d = tempfile::tempdir().unwrap();
    let (mut m, _, rec) = crashed(d.path(), ORIGINAL, CRASHED);
    m.set_target(engine::config::Target::Vanilla);
    let after = text(&m);
    m.undo_crash_restore();
    assert_eq!(text(&m), after, "the step on top is not the restore");
    assert!(rec.exists());
    assert!(
        m.messages
            .toasts()
            .iter()
            .any(|t| t.text.contains("already undone or edited since")),
        "it says why nothing happened"
    );
}

#[test]
fn ctrl_z_reverts_but_keeps_the_file() {
    let d = tempfile::tempdir().unwrap();
    let (mut m, file, rec) = crashed(d.path(), ORIGINAL, CRASHED);
    m.undo();
    assert_eq!(text(&m), ORIGINAL);
    assert!(!m.is_dirty());
    assert!(rec.exists(), "a plain undo keeps the file");
    // The next open restores it again.
    let m = open(file.parent().unwrap().to_path_buf());
    assert_eq!(text(&m), CRASHED);
}

/// Review Focus 2: nothing to restore means nothing is restored, no toast, and the file goes quietly.
#[test]
fn empty_or_equal_recovery_files_are_deleted_silently() {
    for (name, rec) in [
        ("empty", ""),
        ("whitespace", " \r\n\t\n"),
        ("equal with CRLF", "target = \"vanilla\"\r\n"),
        ("equal", ORIGINAL),
    ] {
        let d = tempfile::tempdir().unwrap();
        let (m, _, rec_file) = crashed(d.path(), ORIGINAL, rec);
        assert!(!rec_file.exists(), "{name}: deleted");
        assert_eq!(text(&m), ORIGINAL, "{name}");
        assert!(!m.is_dirty(), "{name}");
        assert!(!restore_toast(&m), "{name}: no toast");
        assert!(m.undo_label().is_none(), "{name}: no step");
    }
}

/// Review Focus 3: a document that does not parse cannot take the edits; the file is not touched, and the banner
/// says where they are. Once the file is fixed and reloaded they come back.
#[test]
fn a_read_only_document_keeps_its_recovery_file() {
    let d = tempfile::tempdir().unwrap();
    let (mut m, file, rec) = crashed(d.path(), "target = = broken\n", CRASHED);
    assert!(m.project.as_ref().unwrap().read_only());
    assert_eq!(std::fs::read_to_string(&rec).unwrap(), CRASHED);
    assert!(!restore_toast(&m));
    let banner = m.banner.clone().unwrap_or_default();
    assert!(
        banner.contains(
            "Unsaved edits from a crash are in sdc.toml.recovery; they come back when sdc.toml is fixed and reloaded."
        ),
        "{banner:?}"
    );
    // The owner fixes the file by hand (so it is newer than the recovery file now) and reloads.
    std::fs::write(&file, ORIGINAL).unwrap();
    m.reload_documents();
    assert_eq!(text(&m), CRASHED);
    assert!(m.is_dirty());
    assert!(restore_toast(&m));
    assert!(
        !m.banner.as_deref().unwrap_or("").contains(".recovery"),
        "{:?}",
        m.banner
    );
}

/// A recovery file older than its document is a stale leftover: it is neither restored nor deleted.
#[test]
fn an_older_recovery_file_is_left_alone() {
    let d = tempfile::tempdir().unwrap();
    let root = mod_folder(d.path());
    let file = root.join("sdc.toml");
    let rec = recovery_path(&file);
    std::fs::write(&rec, CRASHED).unwrap();
    age(&rec, 1_000);
    std::fs::write(&file, ORIGINAL).unwrap();
    let m = open(root);
    assert_eq!(text(&m), ORIGINAL);
    assert!(rec.exists());
    assert!(!restore_toast(&m));
}
