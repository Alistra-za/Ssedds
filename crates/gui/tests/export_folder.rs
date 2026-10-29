//! Note FB1 (review C2, amendment B): a project never inherits another project's export folder from `gui.toml`; the
//! header's `Export to:` chip is drawn only once a folder is chosen, and its Export button reads `Review {n} files`
//! until Review would write without skipping anything.
mod support;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use sdc_gui::model::picker::{PickerBackend, PickerSpec};
use sdc_gui::model::AppModel;
use sdc_gui::settings::GuiSettings;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use support::*;

const NO_FOLDER: &str = "No export folder chosen";

fn idle(m: &mut AppModel) {
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
}

/// The mod fixture opened with `gui.toml` holding another mod's export folder and no per-project choice.
fn fresh(dir: &std::path::Path) -> AppModel {
    let settings = GuiSettings {
        export_folder: Some(dir.join("other-mod")),
        ..GuiSettings::default()
    };
    let mut m = AppModel::new(settings, None, session());
    m.accept = engine::policy::AcceptSuggestions::None;
    m.open_folder(mod_folder(dir));
    idle(&mut m);
    m
}

#[test]
fn a_fresh_project_does_not_inherit_the_gui_toml_export_folder() {
    let d = tempfile::tempdir().unwrap();
    let m = fresh(d.path());
    assert_eq!(m.export_folder_chosen(), None);
    assert_eq!(m.export_root(), m.root.clone().unwrap().join("sdc-out"));
    assert_eq!(m.review_gate(), Err(NO_FOLDER.to_string()));
}

#[test]
fn picking_a_folder_clears_the_blocker() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fresh(d.path());
    let out = d.path().join("out");
    m.set_export_folder(out.clone());
    idle(&mut m);
    assert_eq!(m.export_folder_chosen(), Some(out.clone()));
    assert_eq!(m.export_root(), out);
    assert_ne!(m.review_gate(), Err(NO_FOLDER.to_string()));
}

/// Review Focus 1: with no usable project file the pick lives in `gui.toml` (Note BV6), so it counts as chosen, or the
/// user could never export.
#[test]
fn without_a_usable_project_file_gui_toml_still_counts() {
    let d = tempfile::tempdir().unwrap();
    let other = d.path().join("other-mod");
    // No folder open at all.
    let m = AppModel::new(
        GuiSettings {
            export_folder: Some(other.clone()),
            ..GuiSettings::default()
        },
        None,
        session(),
    );
    assert_eq!(m.export_folder_chosen(), Some(other.clone()));

    // A project file that does not parse: read-only, so the folder is picked into gui.toml.
    let root = mod_folder(d.path());
    std::fs::write(root.join("sdc-project.toml"), "target = [\n").unwrap();
    let mut m = AppModel::new(
        GuiSettings {
            export_folder: Some(other.clone()),
            ..GuiSettings::default()
        },
        None,
        session(),
    );
    m.open_folder(root);
    idle(&mut m);
    assert!(m.sets_doc.as_ref().unwrap().read_only());
    assert_eq!(m.export_folder_chosen(), Some(other));
    let out = d.path().join("out");
    m.set_export_folder(out.clone());
    assert_eq!(m.settings.export_folder, Some(out.clone()));
    assert_eq!(m.export_folder_chosen(), Some(out));
}

#[test]
fn header_export_reads_review_until_clean() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fresh(d.path());
    m.set_export_folder(d.path().join("out"));
    idle(&mut m);
    // The fixture has textures that need a decision.
    assert!(m.undecided() > 0);
    assert!(
        m.header_export_text().starts_with("Review "),
        "{}",
        m.header_export_text()
    );
    assert!(!m.export_is_clean());
    decide_all(&mut m);
    // K3: the fixture's flat `_normal` raises CHK-PLASTIC, which is not about this test.
    m.set_checks(sdc_gui::settings::ChecksSettings {
        plastic: false,
        ..Default::default()
    });
    idle(&mut m);
    assert_eq!(m.undecided(), 0);
    let n = m.export_files();
    assert_eq!(
        m.header_export_text(),
        format!("Export {n} file{}", if n == 1 { "" } else { "s" })
    );
    assert!(m.export_is_clean());
}

#[test]
fn without_a_chosen_folder_the_text_is_review_even_when_everything_is_decided() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fresh(d.path());
    decide_all(&mut m);
    assert_eq!(m.undecided(), 0);
    assert!(m.header_export_text().starts_with("Review "));
    assert!(!m.export_is_clean());
}

fn harness(m: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

#[test]
fn the_header_has_no_export_to_chip_until_a_folder_is_chosen() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(fresh(d.path()));
    h.run();
    assert_eq!(h.query_all_by_label_contains("Export to:").count(), 0);
    assert_eq!(h.query_all_by_label_contains("not set").count(), 0);
    // The one choice: Review leads with it.
    let m = h.state_mut();
    m.set_export_folder(d.path().join("out"));
    idle(m);
    h.run();
    assert_eq!(h.query_all_by_label_contains("Export to:").count(), 1);
}

/// The picker backend that records where the dialog was asked to start.
struct Recorder(std::sync::Mutex<Vec<Option<PathBuf>>>, PathBuf);

impl PickerBackend for Recorder {
    fn pick(&self, spec: &PickerSpec) -> Option<PathBuf> {
        self.0.lock().unwrap().push(spec.start.clone());
        Some(self.1.clone())
    }
}

#[test]
fn review_lists_the_blocker_and_its_choose_button_picks_the_folder() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fresh(d.path());
    let out = d.path().join("picked");
    let rec = Arc::new(Recorder(Default::default(), out.clone()));
    m.picker.set_backend(rec.clone());
    m.open_review();
    let mut h = harness(m);
    h.run();
    // The footer repeats the gate's reason; the Blockers column has its own row.
    assert!(h.query_all_by_label(NO_FOLDER).count() >= 1);
    h.get_by_label("Choose the export folder…").click();
    h.run();
    let end = std::time::Instant::now() + Duration::from_secs(10);
    while h.state().picker.busy() {
        assert!(std::time::Instant::now() < end, "the dialog never closed");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(h.state_mut().poll());
    idle(h.state_mut());
    assert_eq!(h.state().export_folder_chosen(), Some(out));
    // The dialog opened where gui.toml last pointed.
    assert_eq!(
        rec.0.lock().unwrap().clone(),
        vec![Some(d.path().join("other-mod"))]
    );
    h.run();
    assert_eq!(h.query_all_by_label(NO_FOLDER).count(), 0);
}

/// A clean project with no folder chosen does not write into `sdc-out`: the click opens Review instead.
#[test]
fn exporting_without_a_chosen_folder_opens_review_and_writes_nothing() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fresh(d.path());
    decide_all(&mut m);
    m.export_clicked();
    assert!(m.review.is_some());
    assert!(!m.root.clone().unwrap().join("sdc-out").exists());
}

/// Note FB1 (NIF team GD5): NIF edits need a chosen folder too; they never go to `<root>\sdc-out`.
#[test]
fn nif_edits_need_a_chosen_folder() {
    let d = tempfile::tempdir().unwrap();
    // gui.toml names a folder but no project is open.
    let m = AppModel::new(
        GuiSettings {
            export_folder: Some(d.path().join("other-mod")),
            ..GuiSettings::default()
        },
        None,
        session(),
    );
    assert_eq!(m.nif_export_root(), Some(d.path().join("other-mod")));
    // A fresh project does not inherit it, so there is no root (not `<root>\sdc-out`).
    let mut m = fresh(d.path());
    assert_eq!(m.nif_export_root(), None);
    let x = d.path().join("x");
    m.set_export_folder(x.clone());
    idle(&mut m);
    assert_eq!(m.nif_export_root(), Some(x));
}

/// An empty project has nothing to write, so its Export is never accent "ready" (A12 withheld it too).
#[test]
fn an_empty_project_is_not_clean() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("textures");
    std::fs::create_dir_all(&root).unwrap();
    let mut m = AppModel::new(GuiSettings::default(), None, session());
    m.open_folder(root);
    idle(&mut m);
    m.set_export_folder(d.path().join("out"));
    idle(&mut m);
    assert_eq!(m.export_files(), 0);
    assert!(!m.export_is_clean());
    assert_eq!(m.header_export_text(), "Review 0 files");
}
