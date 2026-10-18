//! Layout v2 C7: the app keeps its header in the Meshes view. Its own binary: the app-data redirect is process-wide
//! and must not race the other header tests.
mod support;
use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use sdc_gui::app::SdcApp;
use std::path::Path;

/// As ui_shots: `%LOCALAPPDATA%` and `%APPDATA%` point into `tmp`, so nothing reaches the real sdc folders.
fn redirect_app_data(tmp: &Path) {
    let a = tmp.join("appdata");
    std::fs::create_dir_all(&a).unwrap();
    std::env::set_var("LOCALAPPDATA", &a);
    std::env::set_var("APPDATA", &a);
    let store = sdc_gui::model::meshes::default_store_path().unwrap();
    assert!(store.starts_with(&a), "scan.db not redirected: {store:?}");
}

#[test]
fn the_meshes_view_keeps_the_header() {
    let tmp = tempfile::tempdir().unwrap();
    redirect_app_data(tmp.path());
    let mut app = SdcApp::new(support::pbr_opened(&tmp.path().join("mod")));
    app.meshes.procs = sdc_gui::model::meshes::no_procs;
    let mut frame = eframe::Frame::_new_kittest();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(
            move |ui, app: &mut SdcApp| eframe::App::ui(app, ui, &mut frame),
            app,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    sdc_gui::ui::header::set_meshes_view(&h.ctx, true);
    h.run_steps(4);
    assert!(sdc_gui::ui::header::meshes_view_active(&h.ctx));
    for l in ["Undo", "Save", "Export", "Meshes view", "MO2…"] {
        h.get_by_label(l);
    }
    // K3: the bottom panel's strip is drawn too, under the header.
    h.get_by_label("Files");
}
