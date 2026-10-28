//! `SdcApp::new` must not read `%LOCALAPPDATA%` (hard rule: tests never touch it): the scan DB path comes from
//! `model.scan_db`, which only `main` sets.
mod support;
use sdc_gui::app::SdcApp;

#[test]
fn sdc_app_new_takes_the_scan_db_from_the_model_not_the_environment() {
    let d = tempfile::tempdir().unwrap();
    std::env::set_var("LOCALAPPDATA", d.path());
    let app = SdcApp::new(support::opened(d.path()));
    assert_eq!(app.meshes.store_path, None);
    assert_eq!(app.state.set3d.store_path, None);

    let mut m = support::opened(d.path());
    m.scan_db = Some(d.path().join("x.db"));
    let app = SdcApp::new(m);
    assert_eq!(app.meshes.store_path, Some(d.path().join("x.db")));
    assert_eq!(app.state.set3d.store_path, Some(d.path().join("x.db")));
}

#[test]
fn sdc_app_new_creates_no_analysis_cache() {
    let d = tempfile::tempdir().unwrap();
    std::env::set_var("LOCALAPPDATA", d.path());
    let _app = SdcApp::new(support::opened(d.path()));
    assert!(!d.path().join("sdc").join("analysis-cache").exists());
}
