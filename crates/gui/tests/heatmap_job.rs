//! Task 8 (item 15): the error heatmap is made off the UI thread, keyed by (result, mip, channel, full scale).
mod support;
use engine::config::Format;
use engine::image::Rgba32F;
use engine::session::{PreviewResult, SourcePreview};
use sdc_gui::model::preview::{EncodedState, PreviewOut};
use sdc_gui::model::AppModel;
use sdc_gui::settings::GuiSettings;
use std::sync::Arc;
use std::time::{Duration, Instant};

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
                dims: vec![(2, 1), (1, 1)],
                levels: vec![
                    src,
                    Rgba32F {
                        width: 1,
                        height: 1,
                        px: vec![[0.5, 0.5, 0.5, 1.0]],
                    },
                ],
            },
            format: Format::Bc7,
            srgb: true,
            levels: vec![],
            validation: Default::default(),
            warnings: vec![],
            normal_len: vec![],
        },
        decoded: vec![
            vec![[0.5, 0.5, 0.5, 1.0], [0.53, 0.5, 0.5, 1.0]],
            vec![[0.6, 0.5, 0.5, 1.0]],
        ],
    };
    m.preview.encoded = EncodedState::Done(Arc::new(out));
    m.view.heatmap = true;
    m
}

fn settle(m: &mut AppModel) {
    let end = Instant::now() + Duration::from_secs(30);
    while m.heatmap_pending() {
        assert!(Instant::now() < end, "the heatmap job did not land");
        m.poll();
        std::thread::yield_now();
    }
}

#[test]
fn the_frame_call_starts_a_job_and_the_result_arrives_through_poll() {
    let mut m = model_with_result();
    m.heatmap_frame();
    // Only `poll` takes worker results, so nothing can have landed yet: the frame did not wait for the job.
    assert!(m.heatmap_image().is_none());
    assert!(m.heatmap_pending(), "the app keeps frames coming for it");
    // View-only work: it does not lock edits the way `busy()` does.
    assert!(!m.busy());
    settle(&mut m);
    let img = m.heatmap_image().expect("landed");
    assert_eq!((img.width, img.height), (2, 1));
    assert_eq!(&img.rgba[..3], &[0, 0, 0]);
    // The same key again starts nothing.
    m.heatmap_frame();
    assert!(!m.heatmap_pending());
}

#[test]
fn a_result_for_an_older_full_scale_is_shown_and_the_current_one_follows() {
    let mut m = model_with_result();
    m.heatmap_frame();
    // The slider moves while the job runs: the result is for the same texture, mip and channel, so it is shown.
    m.view.full_scale = 0.02;
    m.heatmap_frame();
    settle(&mut m);
    let early = m.heatmap_image().expect("never blank during a drag");
    assert!(m.heatmap_updating(), "but it is not the current one");
    m.heatmap_frame();
    settle(&mut m);
    let img = m.heatmap_image().expect("the current one landed");
    assert!(!Arc::ptr_eq(&early, &img));
    assert!(!m.heatmap_updating());
    // 0.03 off at full scale 0.02 is white.
    assert_eq!(&img.rgba[4..7], &[255, 255, 255]);
}

#[test]
fn a_result_for_another_encode_is_dropped() {
    let mut m = model_with_result();
    m.heatmap_frame();
    let other = model_with_result().preview.encoded;
    m.preview.encoded = other;
    m.heatmap_frame();
    settle(&mut m);
    assert!(m.heatmap_image().is_none(), "the stale result was dropped");
}

#[test]
fn a_mip_change_falls_back_to_the_plain_image_until_the_new_heatmap_lands() {
    let mut m = model_with_result();
    m.heatmap_frame();
    settle(&mut m);
    assert!(m.heatmap_image().is_some());
    assert!(!m.heatmap_updating());
    m.view.mip = 1;
    m.heatmap_frame();
    assert!(
        m.heatmap_image().is_none(),
        "not the old mip stretched over the new one"
    );
    assert!(m.heatmap_updating());
    settle(&mut m);
    let img = m.heatmap_image().expect("landed");
    assert_eq!((img.width, img.height), (1, 1));
    assert!(!m.heatmap_updating());
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
    assert!(!m.heatmap_pending());
    let mut m = AppModel::new(GuiSettings::default(), None, support::session());
    m.view.heatmap = true;
    m.heatmap_frame();
    assert!(!m.heatmap_pending());
    assert!(m.heatmap_image().is_none());
}
