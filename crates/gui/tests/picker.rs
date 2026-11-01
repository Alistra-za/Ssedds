//! Task 6 (item 7; DECISIONS AG40-AG44): file pickers run off the UI thread through one service. Every test uses a
//! fake backend; no real dialog opens.
mod support;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use sdc_gui::model::meshes::{MeshesModel, ScanTarget};
use sdc_gui::model::picker::{Picker, PickerBackend, PickerPurpose, PickerSpec, BUSY_HOVER};
use sdc_gui::model::viewport3d::Viewport3d;
use sdc_gui::model::AppModel;
use sdc_gui::settings::{GuiSettings, MeshesSettings};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use support::*;

/// A dialog that stays open until the test answers it.
struct Fake {
    answers: Mutex<Receiver<Option<PathBuf>>>,
    /// The `start` of every spec the dialog was asked with.
    starts: Mutex<Vec<Option<PathBuf>>>,
}

fn fake() -> (Arc<Fake>, Sender<Option<PathBuf>>) {
    let (tx, rx) = channel();
    (
        Arc::new(Fake {
            answers: Mutex::new(rx),
            starts: Mutex::new(vec![]),
        }),
        tx,
    )
}

impl PickerBackend for Fake {
    fn pick(&self, spec: &PickerSpec) -> Option<PathBuf> {
        self.starts.lock().unwrap().push(spec.start.clone());
        self.answers
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(30))
            .ok()
            .flatten()
    }
}

fn wait_closed(p: &Picker) {
    let end = Instant::now() + Duration::from_secs(10);
    while p.busy() {
        assert!(Instant::now() < end, "the picker never finished");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Open `purpose` on `p`, answer it with `path`, and wait for the dialog thread to finish.
fn ask(p: &Picker, purpose: PickerPurpose, scope: Option<PathBuf>, path: Option<PathBuf>) {
    let (f, tx) = fake();
    p.set_backend(f);
    assert!(p.request(
        &egui::Context::default(),
        purpose,
        scope,
        PickerSpec::folder()
    ));
    tx.send(path).unwrap();
    wait_closed(p);
}

use eframe::egui;

fn model() -> AppModel {
    AppModel::placeholder(GuiSettings::default())
}

#[test]
fn a_second_picker_is_refused_while_one_is_open() {
    let m = model();
    let (f, tx) = fake();
    m.picker.set_backend(f);
    let ctx = egui::Context::default();
    assert!(m.pick(&ctx, PickerPurpose::NvttPath, PickerSpec::folder()));
    assert!(m.picker.busy());
    // Any purpose, from any model that shares the picker, is refused.
    assert!(!m.pick(&ctx, PickerPurpose::OpenFolder, PickerSpec::folder()));
    let meshes = MeshesModel::empty();
    assert!(!meshes
        .picker
        .request(&ctx, PickerPurpose::ScanFolder, None, PickerSpec::folder()));
    tx.send(None).unwrap();
    wait_closed(&m.picker);
    // Free again.
    let (f, tx) = fake();
    m.picker.set_backend(f);
    assert!(m.pick(&ctx, PickerPurpose::NvttPath, PickerSpec::folder()));
    tx.send(None).unwrap();
    wait_closed(&m.picker);
}

#[test]
fn cancel_does_nothing_and_clears_busy() {
    let mut m = model();
    ask(&m.picker, PickerPurpose::NvttPath, None, None);
    assert!(!m.picker.busy());
    assert!(m.poll(), "the landed result is drained");
    assert_eq!(m.settings.nvtt_path, None);
    assert!(m.dialog.is_none());
}

#[test]
fn each_purpose_reaches_its_own_handler() {
    let d = tempfile::tempdir().unwrap();
    let mut m = model();
    let nvc = d.path().join("nvcompress.exe");
    ask(&m.picker, PickerPurpose::NvttPath, None, Some(nvc.clone()));
    m.poll();
    assert_eq!(m.settings.nvtt_path, Some(nvc));

    let out = d.path().join("elsewhere");
    ask(
        &m.picker,
        PickerPurpose::ExportFolder,
        None,
        Some(out.clone()),
    );
    m.poll();
    assert_eq!(m.settings.export_folder, Some(out));

    let report = d.path().join("r.jsonl");
    ask(
        &m.picker,
        PickerPurpose::SaveReport,
        None,
        Some(report.clone()),
    );
    m.poll();
    settle_report(&mut m);
    assert!(report.is_file(), "the report was written");

    let mut m = opened(&d.path().join("a"));
    let b = mod_folder(&d.path().join("b"));
    ask(
        &m.picker,
        PickerPurpose::OpenFolder,
        m.root.clone(),
        Some(b.clone()),
    );
    m.poll();
    assert_eq!(m.root, Some(b));

    // The meshes model takes the scan pickers; the app model leaves them alone.
    let mut meshes = MeshesModel::new(None, MeshesSettings::default());
    let scan = d.path().join("scan");
    ask(
        &meshes.picker,
        PickerPurpose::ScanFolder,
        None,
        Some(scan.clone()),
    );
    assert!(!m.poll() || m.settings.nvtt_path.is_none());
    meshes.poll();
    assert!(matches!(&meshes.target, Some(ScanTarget::Folder(f)) if *f == scan));
    ask(
        &meshes.picker,
        PickerPurpose::ChooseMo2,
        None,
        Some(d.path().join("no-mo2-here")),
    );
    meshes.poll();
    assert!(meshes.error.is_some(), "an MO2 instance was tried");

    // The 3D preview takes the shader folder.
    let mut v = Viewport3d::new(None);
    ask(
        &v.picker,
        PickerPurpose::ShaderFolder,
        None,
        Some(d.path().to_path_buf()),
    );
    assert!(v.poll());
    assert!(v.locating(), "the chosen folder is being checked");
}

#[test]
fn an_open_folder_pick_is_never_stale_but_an_export_folder_pick_is() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(&d.path().join("a"));
    let c = mod_folder(&d.path().join("c"));
    // The picker opens while folder A is open ...
    let (f, tx) = fake();
    m.picker.set_backend(f);
    assert!(m.pick(
        &egui::Context::default(),
        PickerPurpose::OpenFolder,
        PickerSpec::folder()
    ));
    // ... another folder replaces it before the user answers ...
    let b = mod_folder(&d.path().join("b"));
    m.open_folder(b.clone());
    assert!(m.wait_idle(Duration::from_secs(60)));
    tx.send(Some(c.clone())).unwrap();
    wait_closed(&m.picker);
    m.poll();
    // ... and the pick still opens: it is the user's latest choice (AG129).
    assert_eq!(m.root, Some(c));
    assert!(!m.status_line.contains("Picker result ignored"));
    assert!(m.wait_idle(Duration::from_secs(60)));

    // An export folder picked for a folder that is gone is still dropped.
    let (f, tx) = fake();
    m.picker.set_backend(f);
    assert!(m.pick(
        &egui::Context::default(),
        PickerPurpose::ExportFolder,
        PickerSpec::folder()
    ));
    m.open_folder(b.clone());
    assert!(m.wait_idle(Duration::from_secs(60)));
    let before = m.settings.export_folder.clone();
    tx.send(Some(d.path().join("elsewhere"))).unwrap();
    wait_closed(&m.picker);
    m.poll();
    assert_eq!(m.settings.export_folder, before);
    assert!(m.status_line.contains("Picker result ignored"));
    assert!(m
        .messages
        .entries()
        .any(|e| e.text.starts_with("Picker result ignored")));
    assert_eq!(m.messages.toasts().len(), 1, "and it toasts");
}

#[test]
fn app_wide_pickers_are_never_stale() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(&d.path().join("a"));
    let (f, tx) = fake();
    m.picker.set_backend(f);
    assert!(m.pick(
        &egui::Context::default(),
        PickerPurpose::NvttPath,
        PickerSpec::folder()
    ));
    m.open_folder(mod_folder(&d.path().join("b")));
    let nvc = d.path().join("nvcompress.exe");
    tx.send(Some(nvc.clone())).unwrap();
    wait_closed(&m.picker);
    m.poll();
    assert_eq!(m.settings.nvtt_path, Some(nvc));
}

#[test]
fn the_actions_and_buttons_are_disabled_while_a_dialog_is_open() {
    let m = model();
    let open = sdc_gui::ui::actions::actions()
        .iter()
        .find(|a| a.id == "open")
        .unwrap();
    assert!((open.enabled)(&m));
    let (f, tx) = fake();
    m.picker.set_backend(f);
    assert!(m.pick(
        &egui::Context::default(),
        PickerPurpose::NvttPath,
        PickerSpec::folder()
    ));
    assert!(
        !(open.enabled)(&m),
        "the Open folder action reports disabled"
    );
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 200.0))
        .build_ui_state(
            |ui, m: &mut AppModel| sdc_gui::ui::header::draw(ui, m, None),
            m,
        );
    h.run();
    // R14: Open folder… is the first entry of Open ▾.
    h.get_by_label("Open").click();
    h.run();
    let b = h.get_by_label("Open folder…");
    assert!(b.accesskit_node().is_disabled());
    tx.send(None).unwrap();
    wait_closed(&h.state().picker);
    h.run();
    if h.query_by_label("Open folder…").is_none() {
        h.get_by_label("Open").click();
        h.run();
    }
    assert!(!h
        .get_by_label("Open folder…")
        .accesskit_node()
        .is_disabled());
    assert_eq!(BUSY_HOVER, "A file dialog is already open");
}

#[test]
fn a_shader_folder_pick_reaches_the_viewport_that_asked() {
    use sdc_gui::model::set_preview::{SetPreview, DEFAULT_CAP};
    let d = tempfile::tempdir().unwrap();
    let mut meshes = MeshesModel::new(None, MeshesSettings::default());
    let mut set = SetPreview::new(None, None, None, DEFAULT_CAP);
    // The set preview's 3D panel asks.
    ask(
        &set.v3.picker,
        PickerPurpose::ShaderFolder,
        None,
        Some(d.path().to_path_buf()),
    );
    // The Meshes viewport polls first, as in the app, and gets nothing.
    meshes.poll();
    assert!(!meshes.v3.locating());
    assert!(set.v3.poll());
    assert!(set.v3.locating());
}

#[test]
fn the_export_folder_cannot_change_while_an_export_runs() {
    use egui_kittest::kittest::Queryable;
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(&d.path().join("a"));
    m.export.running = Some(Default::default());
    // A dialog opened before the export started lands during it: dropped.
    let before = m.settings.export_folder.clone();
    ask(
        &m.picker,
        PickerPurpose::ExportFolder,
        m.root.clone(),
        Some(d.path().join("elsewhere")),
    );
    m.poll();
    assert_eq!(m.settings.export_folder, before);
    assert!(m.status_line.contains("an export is running"));
    // Export to ▾'s Choose export folder… is off, with the reason (K7).
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
    h.run_steps(2);
    h.get_by_label_contains("Export to: ").click();
    h.run_steps(2);
    let b = h.get_by_label("Choose export folder…");
    assert!(b.accesskit_node().is_disabled());
    b.hover();
    // run(), not run_steps: the tooltip waits out its delay.
    h.run();
    h.get_by_label(sdc_gui::ui::header::EXPORT_RUNNING_HOVER);
}

/// Note FI10: the start folder of the Open folder dialog for a model whose recent list is `recent`.
fn open_folder_start(recent: Vec<PathBuf>) -> Option<PathBuf> {
    let m = AppModel::placeholder(GuiSettings {
        recent,
        ..GuiSettings::default()
    });
    let (f, tx) = fake();
    m.picker.set_backend(f.clone());
    assert!(m.pick(
        &egui::Context::default(),
        PickerPurpose::OpenFolder,
        m.open_folder_spec()
    ));
    tx.send(None).unwrap();
    wait_closed(&m.picker);
    let starts = f.starts.lock().unwrap();
    assert_eq!(starts.len(), 1);
    starts[0].clone()
}

#[test]
fn open_folder_starts_in_the_last_projects_parent() {
    let d = tempfile::tempdir().unwrap();
    let a = d.path().join("a");
    let proj = a.join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    assert_eq!(open_folder_start(vec![proj]), Some(a));
}

#[test]
fn open_folder_with_no_recent_has_no_start() {
    assert_eq!(open_folder_start(vec![]), None);
    // The last project's parent is gone.
    let d = tempfile::tempdir().unwrap();
    assert_eq!(
        open_folder_start(vec![d.path().join("gone").join("proj")]),
        None
    );
}

/// Polls until the Save report worker has answered.
fn settle_report(m: &mut AppModel) {
    let end = std::time::Instant::now() + Duration::from_secs(30);
    while m.report_saving {
        assert!(std::time::Instant::now() < end, "the report write landed");
        m.poll();
        std::thread::yield_now();
    }
}

#[test]
fn save_report_writes_on_a_worker_and_says_so() {
    let d = tempfile::tempdir().unwrap();
    let mut m = model();
    let report = d.path().join("r.md");
    ask(
        &m.picker,
        PickerPurpose::SaveReport,
        None,
        Some(report.clone()),
    );
    m.poll();
    assert!(m.report_saving, "the write runs on a worker");
    settle_report(&mut m);
    assert!(report.is_file());
    assert!(
        m.status_line.starts_with("Saved report: "),
        "{}",
        m.status_line
    );
    assert!(m
        .messages
        .toasts()
        .iter()
        .any(|t| t.text.starts_with("Saved report: ")));
    // A path that cannot be written: the error goes to the status line and a toast, no dialog.
    ask(
        &m.picker,
        PickerPurpose::SaveReport,
        None,
        Some(d.path().to_path_buf()),
    );
    m.poll();
    settle_report(&mut m);
    assert!(
        m.status_line.starts_with("Could not write the report"),
        "{}",
        m.status_line
    );
    assert!(m.dialog.is_none());
}

/// Owner ruling (Note BV6): the export folder is per project. It is saved in that project's sdc-project.toml; another
/// project keeps the app-wide default from gui.toml, which picking no longer changes.
#[test]
fn the_export_folder_is_per_project() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(&d.path().join("a"));
    assert!(m.wait_idle(Duration::from_secs(60)));
    let default = m.settings.export_folder.clone();
    let root_a = m.root.clone().unwrap();
    let out_a = d.path().join("out-a");
    m.set_export_folder(out_a.clone());
    assert_eq!(m.export_root(), out_a);
    assert_eq!(m.settings.export_folder, default, "gui.toml is not changed");
    let saved = std::fs::read_to_string(root_a.join("sdc-project.toml")).unwrap();
    assert!(saved.contains("export_folder"), "{saved}");
    assert!(m.wait_idle(Duration::from_secs(60)));

    let root_b = mod_folder(&d.path().join("b"));
    m.open_folder(root_b);
    assert!(m.wait_idle(Duration::from_secs(60)));
    // Note FB1: project b has none, and the gui.toml default is no longer inherited.
    assert_eq!(m.export_folder_chosen(), None);
    assert_eq!(m.export_root(), m.root.clone().unwrap().join("sdc-out"));

    m.open_folder(root_a);
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.export_root(), out_a);
}
