//! Task 8 (item 15): the error heatmap is made off the UI thread, keyed by (result, mip, channel, full scale).
mod support;
use engine::config::Format;
use engine::image::Rgba32F;
use engine::session::{PreviewResult, SourcePreview};
use sdc_gui::model::preview::{EncodedState, PreviewOut};
use sdc_gui::model::AppModel;
use sdc_gui::settings::GuiSettings;
use std::sync::Arc;
use std::time::Duration;

fn model_with_result() -> AppModel {
    let mut m = AppModel::new(GuiSettings::default(), None, support::session());
    let src = Rgba32F {
        width: 2,
        height: 1,
        px: vec![[0.5, 0.5, 0.5, 1.0], [0.5, 0.5, 0.5, 1.0]],
    };
    let out = PreviewOut {
        result: PreviewResult {
            source: SourcePreview {
                first_level: 0,
                dims: vec![(2, 1)],
                levels: vec![src],
            },
            format: Format::Bc7,
            srgb: true,
            levels: vec![],
            validation: Default::default(),
            warnings: vec![],
            normal_len: vec![],
        },
        decoded: vec![vec![[0.5, 0.5, 0.5, 1.0], [0.53, 0.5, 0.5, 1.0]]],
    };
    m.preview.encoded = EncodedState::Done(Arc::new(out));
    m.view.heatmap = true;
    m
}

fn settle(m: &mut AppModel) {
    assert!(m.wait_idle(Duration::from_secs(30)));
}

#[test]
fn the_frame_call_starts_a_job_and_the_result_arrives_through_poll() {
    let mut m = model_with_result();
    m.heatmap_frame();
    // Only `poll` takes worker results, so nothing can have landed yet: the frame did not wait for the job.
    assert!(m.heatmap_image().is_none());
    assert!(m.busy(), "the job counts as work, so frames keep coming");
    settle(&mut m);
    let img = m.heatmap_image().expect("landed");
    assert_eq!((img.width, img.height), (2, 1));
    assert_eq!(&img.rgba[..3], &[0, 0, 0]);
    // The same key again starts nothing.
    m.heatmap_frame();
    assert!(!m.busy());
}

#[test]
fn a_result_for_an_older_key_is_dropped() {
    let mut m = model_with_result();
    m.heatmap_frame();
    // The full scale moves while the job runs: its result no longer matches.
    m.view.full_scale = 0.02;
    m.heatmap_frame();
    settle(&mut m);
    assert!(m.heatmap_image().is_none(), "the stale result was dropped");
    m.heatmap_frame();
    settle(&mut m);
    let img = m.heatmap_image().expect("the current one landed");
    // 0.03 off at full scale 0.02 is white.
    assert_eq!(&img.rgba[4..7], &[255, 255, 255]);
}

#[test]
fn full_scale_changes_the_heatmap_and_the_old_one_stays_until_the_new_lands() {
    let mut m = model_with_result();
    m.heatmap_frame();
    settle(&mut m);
    let first = m.heatmap_image().unwrap();
    m.view.full_scale = 0.5;
    m.heatmap_frame();
    let still = m.heatmap_image().unwrap();
    assert!(
        Arc::ptr_eq(&first, &still),
        "the previous heatmap stays on show"
    );
    settle(&mut m);
    let second = m.heatmap_image().unwrap();
    assert!(!Arc::ptr_eq(&first, &second));
    assert_ne!(first.rgba, second.rgba);
}

#[test]
fn a_heatmap_off_or_without_a_result_starts_nothing() {
    let mut m = model_with_result();
    m.view.heatmap = false;
    m.heatmap_frame();
    assert!(!m.busy());
    let mut m = AppModel::new(GuiSettings::default(), None, support::session());
    m.view.heatmap = true;
    m.heatmap_frame();
    assert!(!m.busy());
    assert!(m.heatmap_image().is_none());
}
