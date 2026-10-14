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
}

fn fake() -> (Arc<Fake>, Sender<Option<PathBuf>>) {
    let (tx, rx) = channel();
    (
        Arc::new(Fake {
            answers: Mutex::new(rx),
        }),
        tx,
    )
}

impl PickerBackend for Fake {
    fn pick(&self, _: &PickerSpec) -> Option<PathBuf> {
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
fn a_stale_open_folder_result_is_dropped() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(&d.path().join("a"));
    let a = m.root.clone();
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
    tx.send(Some(c)).unwrap();
    wait_closed(&m.picker);
    m.poll();
    // ... so the answer is for a folder that is gone.
    assert_eq!(m.root, Some(b));
    assert_ne!(m.root, a);
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
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::header::draw(ui, m), m);
    h.run();
    let b = h.get_by_label("Open folder…");
    assert!(b.accesskit_node().is_disabled());
    tx.send(None).unwrap();
    wait_closed(&h.state().picker);
    h.run();
    assert!(!h
        .get_by_label("Open folder…")
        .accesskit_node()
        .is_disabled());
    assert_eq!(BUSY_HOVER, "A file dialog is already open");
}
