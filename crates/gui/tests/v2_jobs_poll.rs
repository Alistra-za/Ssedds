//! Owner answer 8 (DECISIONS Note CL, Task 6): a v2 worker job keeps `SdcApp` polling through its one `app.rs`
//! line, and the app goes quiet once the job landed. Its own binary: `SdcApp` needs `%APPDATA%` and
//! `%LOCALAPPDATA%` redirected, which is process-wide (preflight K11).
mod support;
use eframe::egui;
use egui_kittest::Harness;
use engine::image::Rgba32F;
use sdc_gui::app::SdcApp;
use sdc_gui::model::checks::RangeRule;
use sdc_gui::model::heatmap::HeatImage;
use sdc_gui::model::overlay::{range_overlay, OverlayKind};
use std::path::Path;
use std::time::Duration;

fn redirect_app_data(tmp: &Path) {
    let a = tmp.join("appdata");
    std::fs::create_dir_all(&a).unwrap();
    std::env::set_var("LOCALAPPDATA", &a);
    std::env::set_var("APPDATA", &a);
    let store = sdc_gui::model::meshes::default_store_path().unwrap();
    assert!(store.starts_with(&a), "scan.db not redirected: {store:?}");
}

fn slow(l: &Rgba32F, rules: &[RangeRule]) -> HeatImage {
    std::thread::sleep(Duration::from_millis(1500));
    range_overlay(l, rules)
}

/// `app.rs`'s `POLL_VISIBLE` (private).
const POLL_VISIBLE: Duration = Duration::from_millis(50);

fn repaint_delay(h: &Harness<'_, SdcApp>) -> Duration {
    h.output().viewport_output[&egui::ViewportId::ROOT].repaint_delay
}

#[test]
fn v2_jobs_keep_the_app_polling_without_greying_advanced() {
    let tmp = tempfile::tempdir().unwrap();
    redirect_app_data(tmp.path());
    let mut model = support::pbr_wall(&tmp.path().join("mod"), 20, [128, 10, 200]);
    model.expand("test/wall.png");
    assert!(model.wait_idle(Duration::from_secs(60)));
    let app = SdcApp::new(model);
    let mut frame = eframe::Frame::_new_kittest();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(
            move |ui, app: &mut SdcApp| eframe::App::ui(app, ui, &mut frame),
            app,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run_steps(10);
    assert!(
        repaint_delay(&h) > POLL_VISIBLE,
        "nothing pending, yet the app asks for frames every {:?}",
        repaint_delay(&h)
    );
    let started =
        h.state_mut()
            .model
            .overlay_for_with("test/wall.png", 0, OverlayKind::Range, slow);
    assert!(started.is_none());
    assert!(h.state().model.v2_jobs_pending());
    assert!(!h.state().model.busy(), "the overlay job greys Advanced");
    h.step();
    assert!(
        repaint_delay(&h) <= POLL_VISIBLE,
        "the overlay job is not polled: {:?}",
        repaint_delay(&h)
    );
    assert!(h.state().model.v2_jobs_pending(), "landed too soon to pin");
    assert!(h.state_mut().model.wait_idle(Duration::from_secs(10)));
    h.run_steps(10);
    assert!(!h.state().model.v2_jobs_pending());
    assert!(
        repaint_delay(&h) > POLL_VISIBLE,
        "still polling after the job landed: {:?}",
        repaint_delay(&h)
    );
}
