//! Task 9 (item 10, DECISIONS AG64-AG71): pin an encode and compare it against the next one, through the model
//! helpers (the preview itself needs a GPU).
mod support;
use sdc_gui::model::preview::*;
use sdc_gui::model::AppModel;
use std::sync::{Arc, Weak};
use std::time::Duration;
use support::*;

const KEY: &str = "landscape/rock01_rmaos";

fn settle(m: &mut AppModel) {
    assert!(m.wait_idle(Duration::from_secs(120)));
    let end = std::time::Instant::now() + Duration::from_secs(60);
    while m.heatmap_pending() {
        assert!(std::time::Instant::now() < end, "heatmap did not land");
        m.poll();
        std::thread::yield_now();
    }
}

fn done(m: &AppModel) -> Arc<PreviewOut> {
    let EncodedState::Done(o) = &m.preview.encoded else {
        panic!("not done")
    };
    o.clone()
}

/// An opened folder with `KEY` expanded, encoded and pinned.
fn pinned(dir: &std::path::Path) -> AppModel {
    let mut m = opened(dir);
    m.expand(KEY);
    m.encode_now();
    settle(&mut m);
    assert!(m.can_pin());
    m.pin_this();
    m
}

fn change_and_reencode(m: &mut AppModel) {
    m.set_field(KEY, "encoder.effort", serde_json::json!("fast"));
    settle(m);
    m.encode_now();
    settle(m);
    assert!(matches!(m.preview.encoded, EncodedState::Done(_)));
}

#[test]
fn a_pin_needs_a_finished_encode() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.expand(KEY);
    settle(&mut m);
    assert!(!m.can_pin(), "nothing encoded yet");
    m.pin_this();
    assert!(m.preview.pin.is_none());
    m.encode_now();
    settle(&mut m);
    m.set_field(KEY, "encoder.effort", serde_json::json!("fast"));
    settle(&mut m);
    assert!(matches!(m.preview.encoded, EncodedState::Stale(_)));
    assert!(!m.can_pin(), "a stale result does not match its settings");
}

#[test]
fn a_setting_change_and_a_re_encode_keep_the_pin() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pinned(d.path());
    let first = done(&m);
    let id = m.preview.pin.as_ref().unwrap().id;
    change_and_reencode(&mut m);
    let pin = m.preview.pin.as_ref().expect("kept");
    assert_eq!(pin.id, id);
    assert!(Arc::ptr_eq(&pin.out, &first));
    assert!(
        !Arc::ptr_eq(&done(&m), &first),
        "the current encode is a new one"
    );
}

#[test]
fn a_texture_change_close_and_folder_open_each_drop_the_pin() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pinned(d.path());
    m.expand("landscape/leaf.png");
    assert!(m.preview.pin.is_none(), "texture change");
    assert!(!m.comparing_pin());

    let mut m = pinned(d.path());
    m.collapse();
    assert!(m.preview.pin.is_none(), "close");

    let mut m = pinned(d.path());
    m.open_folder(d.path().join("textures"));
    assert!(m.preview.pin.is_none(), "folder open");
}

#[test]
fn pinning_again_replaces_the_old_pin_and_dropping_frees_its_buffers() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pinned(d.path());
    let old: Weak<PreviewOut> = Arc::downgrade(&m.preview.pin.as_ref().unwrap().out);
    let old_id = m.preview.pin.as_ref().unwrap().id;
    change_and_reencode(&mut m);
    assert!(old.upgrade().is_some(), "the pin is the only holder");
    let second = done(&m);
    m.pin_this();
    let pin = m.preview.pin.as_ref().unwrap();
    assert_ne!(pin.id, old_id);
    assert!(Arc::ptr_eq(&pin.out, &second));
    assert!(
        old.upgrade().is_none(),
        "the replaced pin's images are released"
    );
    drop(second);

    // Dropping on close releases the rest.
    let weak = Arc::downgrade(&m.preview.pin.as_ref().unwrap().out);
    m.preview.encoded = EncodedState::NotEncoded;
    assert!(weak.upgrade().is_some());
    m.collapse();
    assert!(
        weak.upgrade().is_none(),
        "the pin's images are released with it"
    );
}

#[test]
fn unpin_drops_the_pin_and_the_comparison() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pinned(d.path());
    assert!(m.comparing_pin(), "pinning turns the comparison on");
    m.unpin();
    assert!(m.preview.pin.is_none());
    assert!(!m.comparing_pin());
}

#[test]
fn the_compare_toggle_routes_the_right_images_to_each_side() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pinned(d.path());
    let pin_out = m.preview.pin.as_ref().unwrap().out.clone();
    change_and_reencode(&mut m);
    let cur = done(&m);
    // Pinned vs Current.
    assert!(m.comparing_pin());
    let (left, right) = m.compare_sides(0);
    assert_eq!(left.unwrap().px, pin_out.decoded[0].as_slice());
    assert_eq!(right.unwrap().px, cur.decoded[0].as_slice());
    // Source vs Current.
    m.set_compare_pinned(false);
    assert!(!m.comparing_pin());
    let (left, right) = m.compare_sides(0);
    assert_eq!(left.unwrap().px, cur.result.source.levels[0].px.as_slice());
    assert_eq!(right.unwrap().px, cur.decoded[0].as_slice());
    // The toggle needs a pin.
    m.unpin();
    m.set_compare_pinned(true);
    assert!(!m.comparing_pin());
}

#[test]
fn captions_carry_both_settings_summaries_and_psnrs() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pinned(d.path());
    let pin_out = m.preview.pin.as_ref().unwrap().out.clone();
    change_and_reencode(&mut m);
    let cur = done(&m);
    let (left, right) = m.pin_captions().expect("comparing");
    assert!(left.starts_with("Pinned"), "{left}");
    assert!(right.starts_with("Current"), "{right}");
    assert_ne!(left, right, "the effort differs");
    for (text, out) in [(&left, &pin_out), (&right, &cur)] {
        assert!(
            text.to_lowercase()
                .contains(&out.result.format.id().to_lowercase()),
            "{text}"
        );
        match out.result.validation.min_mip_psnr_db {
            Some(p) => assert!(text.contains(&format!("{p:.1} dB")), "{text}"),
            None => assert!(text.contains("PSNR —"), "{text}"),
        }
        assert!(text.contains("PSNR"), "{text}");
    }
    assert!(right.to_lowercase().contains("fast"), "{right}");
    m.set_compare_pinned(false);
    assert!(m.pin_captions().is_none());
}

#[test]
fn a_mip_the_pin_lacks_shows_a_dash() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pinned(d.path());
    let n = m.preview.pin.as_ref().unwrap().out.decoded.len();
    assert!(n >= 2, "the fixture has a mip chain");
    assert!(m.pin_missing_text(0).is_none());
    // A pin made under a smaller preview limit has fewer mips than the current encode.
    let pin = m.preview.pin.as_mut().unwrap();
    let mut short = PreviewOut {
        result: pin.out.result.clone(),
        decoded: pin.out.decoded[..1].to_vec(),
    };
    short.result.levels.truncate(1);
    pin.out = Arc::new(short);
    assert_eq!(m.pin_missing_text(1), Some("— not in the pinned encode"));
    assert!(m.compare_sides(1).0.is_none());
    assert!(m.compare_sides(1).1.is_some());
    let lines = m.preview_texel(1, (0, 0)).lines();
    assert!(lines.iter().any(|l| l == "Pinned: —"), "{lines:?}");
}

#[test]
fn the_readout_shows_the_pinned_side_while_comparing() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pinned(d.path());
    let lines = m.preview_texel(0, (0, 0)).lines();
    assert!(
        lines.iter().any(|l| l.starts_with("Pinned: R ")),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.starts_with("|Δ| pinned vs encoded")),
        "{lines:?}"
    );
    m.set_compare_pinned(false);
    let lines = m.preview_texel(0, (0, 0)).lines();
    assert!(!lines.iter().any(|l| l.contains("Pinned")), "{lines:?}");
}

#[test]
fn the_heatmap_job_is_keyed_on_the_pin_and_compares_current_with_pinned() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pinned(d.path());
    m.view.heatmap = true;
    m.heatmap_frame();
    settle(&mut m);
    // Pinned == current: no difference anywhere.
    let img = m.heatmap_image().expect("landed");
    assert!(img.rgba.chunks(4).all(|p| p[..3] == [0, 0, 0]));
    assert!(m.heatmap_label().contains("Current vs Pinned"));
    // Pinning again gives a new pin id: the heatmap is made again, and the old one is not shown for it.
    m.pin_this();
    m.heatmap_frame();
    assert!(m.heatmap_updating());
    assert!(m.heatmap_image().is_none());
    settle(&mut m);
    assert!(m.heatmap_image().is_some());
    // Without the comparison the heatmap is the source's again.
    m.set_compare_pinned(false);
    m.heatmap_frame();
    assert!(m.heatmap_image().is_none());
    assert!(m.heatmap_label().contains("Source"));
    settle(&mut m);
    assert!(m.heatmap_image().is_some());
}

#[test]
fn p_is_in_the_actions_table() {
    use sdc_gui::ui::actions::{actions, Area};
    let a = actions()
        .iter()
        .find(|a| a.id == "pin")
        .expect("a pin action");
    assert_eq!(a.area, Area::Preview);
    assert_eq!(a.label, "Pin this result");
    assert_eq!(a.keys.len(), 1);
    assert_eq!(a.keys[0].text(), "P");
    // Off until there is a finished encode.
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.expand(KEY);
    settle(&mut m);
    assert!(!(a.enabled)(&m));
    m.encode_now();
    settle(&mut m);
    assert!((a.enabled)(&m));
}

#[test]
fn unpinning_frees_the_pinned_buffers_even_after_a_heatmap_was_made() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pinned(d.path());
    change_and_reencode(&mut m);
    m.view.heatmap = true;
    m.heatmap_frame();
    settle(&mut m);
    assert!(m.heatmap_image().is_some());
    m.view.heatmap = false;
    let weak = Arc::downgrade(&m.preview.pin.as_ref().unwrap().out);
    m.unpin();
    assert!(
        weak.upgrade().is_none(),
        "the heatmap job kept the pin alive"
    );
    // Pinning again frees the replaced pin too.
    m.pin_this();
    m.view.heatmap = true;
    m.heatmap_frame();
    settle(&mut m);
    m.view.heatmap = false;
    change_and_reencode(&mut m);
    let weak = Arc::downgrade(&m.preview.pin.as_ref().unwrap().out);
    m.pin_this();
    assert!(weak.upgrade().is_none());
}
