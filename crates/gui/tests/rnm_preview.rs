//! Task 14 (item 21, DECISIONS AG115-AG117): the blended-normal thumbnail of the RNM section.
mod support;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use image::{ImageBuffer, Luma, Rgb};
use sdc_gui::model::rnm_preview::{normal_rgba, render, THUMB_MAX};
use sdc_gui::model::AppModel;
use sdc_gui::settings::GuiSettings;
use serde_json::json;
use std::path::Path;
use std::time::{Duration, Instant};

const N: &str = "m/iron_n.png";

fn fixture(dir: &Path) -> AppModel {
    let r = dir.join("m");
    std::fs::create_dir_all(&r).unwrap();
    ImageBuffer::from_fn(256, 128, |x, y| Rgb([(x % 256) as u8, (y * 2) as u8, 90u8]))
        .save(r.join("iron.png"))
        .unwrap();
    ImageBuffer::from_fn(256, 128, |x, y| {
        Rgb([128u8.wrapping_add((x / 8) as u8), 128, 255 - (y / 8) as u8])
    })
    .save(r.join("iron_n.png"))
    .unwrap();
    for f in ["iron_bump.png", "iron_detailnorm.png"] {
        ImageBuffer::from_fn(64, 64, |x, y| Luma([((x * 7 + y * 3) % 256) as u8]))
            .save(r.join(f))
            .unwrap();
    }
    let settings = GuiSettings {
        export_folder: Some(dir.join("export")),
        ..GuiSettings::default()
    };
    let mut m = AppModel::new(settings, None, support::session());
    m.accept = engine::policy::AcceptSuggestions::High;
    m.open_folder(dir.to_path_buf());
    assert!(m.wait_idle(Duration::from_secs(60)), "planning");
    m
}

fn settle(m: &mut AppModel) {
    let end = Instant::now() + Duration::from_secs(30);
    while m.rnm_preview_pending() {
        assert!(Instant::now() < end, "the preview job did not land");
        m.poll();
        std::thread::yield_now();
    }
}

#[test]
fn the_pixel_mapping_is_n_plus_one_over_two() {
    assert_eq!(normal_rgba([0.0, 0.0, 1.0]), [128, 128, 255, 255]);
    assert_eq!(normal_rgba([1.0, 0.0, 0.0]), [255, 128, 128, 255]);
    assert_eq!(normal_rgba([-1.0, -1.0, 0.0]), [0, 0, 128, 255]);
}

#[test]
fn the_thumbnail_is_the_engine_blend_at_128_px() {
    let d = tempfile::tempdir().unwrap();
    let m = fixture(d.path());
    let job = m.item(N).unwrap().job.clone().unwrap();
    assert!(job.active_rnm().is_some(), "the layers are found");
    let t = render(&job).unwrap();
    assert_eq!((t.width, t.height), (THUMB_MAX, THUMB_MAX / 2));
    assert_eq!(t.rgba.len(), (t.width * t.height * 4) as usize);
    assert!(t.rgba.chunks(4).all(|p| p[3] == 255));
    // The blend moved the base: a render with the layers off differs.
    let mut off = job.clone();
    off.profile.rnm.enabled = false;
    assert!(render(&off).is_err(), "no active layers, no blend");
}

#[test]
fn the_job_runs_off_the_ui_thread_and_lands_through_poll() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path());
    m.rnm_preview_frame(N);
    assert!(m.rnm_preview_pending(), "started, not finished");
    assert!(m.rnm_preview_image(N).is_none(), "nothing on show yet");
    settle(&mut m);
    let img = m.rnm_preview_image(N).expect("the picture landed");
    assert!(img.width.max(img.height) <= THUMB_MAX);
    // The same settings start nothing new.
    m.rnm_preview_frame(N);
    assert!(!m.rnm_preview_pending());
    assert_eq!(m.rnm_preview.started, 1);
}

#[test]
fn a_released_slider_and_a_new_order_each_refresh_it() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path());
    m.rnm_preview_frame(N);
    settle(&mut m);
    let before = m.rnm_preview_image(N).unwrap();
    m.set_field(N, "rnm.bump.strength", json!(2.0));
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.rnm_preview_frame(N);
    assert!(m.rnm_preview_pending(), "the strength changed the key");
    settle(&mut m);
    let after = m.rnm_preview_image(N).unwrap();
    assert_ne!(before.rgba, after.rgba, "the stronger bump shows");
    assert_eq!(m.rnm_preview.started, 2);

    m.set_field(
        N,
        "rnm.order",
        json!(["detail", "bump", "bump2", "detail2", "bent"]),
    );
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.rnm_preview_frame(N);
    assert!(m.rnm_preview_pending(), "the order changed the key");
    settle(&mut m);
    assert_eq!(m.rnm_preview.started, 3);
}

#[test]
fn a_result_for_settings_that_changed_meanwhile_is_dropped() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path());
    m.rnm_preview_frame(N);
    // The settings change before the result is read.
    let i = m.items.iter().position(|i| i.key == N).unwrap();
    m.items[i].job.as_mut().unwrap().profile.rnm.bump.strength = Some(1.9);
    m.invalidate_views();
    settle(&mut m);
    assert!(
        m.rnm_preview_image(N).is_none(),
        "the stale result is not shown"
    );
    assert!(m.rnm_preview_failed(N).is_none());
    m.rnm_preview_frame(N);
    assert!(
        m.rnm_preview_pending(),
        "the current settings start a new job"
    );
    settle(&mut m);
    assert!(m.rnm_preview_image(N).is_some());
    assert_eq!(m.rnm_preview.started, 2);
}

/// Note FC13 (AG116): "Preview unavailable" names the reason.
#[test]
fn a_failed_preview_says_why() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path());
    std::fs::remove_file(d.path().join("m/iron_n.png")).unwrap();
    m.rnm_preview_frame(N);
    settle(&mut m);
    let why = m.rnm_preview_failed(N).expect("the blend failed");
    assert!(!why.trim().is_empty(), "a reason");
    m.select_item(N);
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 900.0))
        .build_ui_state(
            |ui, model: &mut AppModel| {
                model.poll();
                sdc_gui::ui::draw(ui, model)
            },
            m,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run_steps(6);
    h.get_by_label(&format!("Preview unavailable: {why}"));
}

#[test]
fn the_section_shows_the_picture() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path());
    m.select_item(N);
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 900.0))
        .build_ui_state(
            |ui, model: &mut AppModel| {
                model.poll();
                sdc_gui::ui::draw(ui, model)
            },
            m,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    h.get_by_label("Blended normal");
    for _ in 0..200 {
        if !h.state().rnm_preview_pending() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
        h.step();
    }
    h.step();
    assert!(h.state().rnm_preview_image(N).is_some());
    assert!(h.query_by_label("Blending…").is_none());
}
