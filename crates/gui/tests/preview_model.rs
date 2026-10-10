mod support;
use engine::config::Target;
use sdc_gui::model::preview::*;
use sdc_gui::model::*;
use std::time::{Duration, Instant};
use support::*;

fn settle(m: &mut AppModel) {
    assert!(m.wait_idle(Duration::from_secs(120)));
}

#[test]
fn encoding_is_off_by_default_and_encode_now_encodes_once() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    assert!(!m.preview.live);
    m.expand("landscape/rock01_rmaos");
    settle(&mut m);
    assert!(matches!(m.preview.source, SourceState::Ready(_)));
    assert!(matches!(m.preview.encoded, EncodedState::NotEncoded));
    m.tick(Instant::now() + Duration::from_secs(5));
    settle(&mut m);
    assert!(
        matches!(m.preview.encoded, EncodedState::NotEncoded),
        "live off: nothing encodes by itself"
    );
    m.encode_now();
    settle(&mut m);
    let EncodedState::Done(out) = &m.preview.encoded else {
        panic!("not done")
    };
    assert_eq!(out.result.source.first_level, 0);
    assert_eq!(out.decoded.len(), out.result.levels.len());
}

#[test]
fn an_edit_makes_the_result_stale_and_live_re_encodes_after_the_debounce() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.expand("landscape/rock01_rmaos");
    m.encode_now();
    settle(&mut m);
    m.set_field(
        "landscape/rock01_rmaos",
        "encoder.effort",
        serde_json::json!("fast"),
    );
    settle(&mut m);
    assert!(matches!(m.preview.encoded, EncodedState::Stale(_)));
    m.set_live(true);
    let t0 = Instant::now();
    m.tick(t0);
    assert!(
        !matches!(m.preview.encoded, EncodedState::Encoding { .. }),
        "debounced"
    );
    m.tick(t0 + DEBOUNCE + Duration::from_millis(1));
    assert!(matches!(
        m.preview.encoded,
        EncodedState::Encoding { previous: Some(_) }
    ));
    settle(&mut m);
    assert!(matches!(m.preview.encoded, EncodedState::Done(_)));
}

#[test]
fn mode_change_cancels_and_drops_a_preview_of_a_vanished_job() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let set = m
        .item("landscape/rock01_rmaos")
        .unwrap()
        .set_key
        .clone()
        .unwrap();
    m.expand("landscape/rock01_rmaos");
    m.encode_now();
    // Switch target while the encode may still be running: cpm packs rock01_m instead.
    m.set_target(Target::Cpm);
    settle(&mut m);
    assert!(m.item("landscape/rock01_rmaos").is_none());
    assert_eq!(m.expanded, None);
    assert_eq!(m.preview.item, None);
    assert!(matches!(m.preview.encoded, EncodedState::NotEncoded));
    assert_eq!(m.selection, Selection::Set(set));
}

#[test]
fn the_size_limit_skips_large_mips() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.settings.preview_max = 16;
    m.expand("landscape/rock01_rmaos");
    m.encode_now();
    settle(&mut m);
    let EncodedState::Done(out) = &m.preview.encoded else {
        panic!("not done")
    };
    assert_eq!(out.result.source.first_level, 2, "64 and 32 are above 16");
}

#[test]
fn items_without_a_job_preview_nothing() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.expand("landscape/rock01_roughness.png");
    assert_eq!(m.preview.notice.as_deref(), Some("Nothing to preview"));
    assert!(!m.busy(), "nothing was started");
    // An unresolved job previews its current settings.
    m.expand("landscape/leaf.png");
    assert_eq!(
        m.preview.notice.as_deref(),
        Some("Preview of the suggested settings")
    );
    settle(&mut m);
    assert!(matches!(m.preview.source, SourceState::Ready(_)));
}

#[test]
fn a_dds_input_previews_its_own_bytes() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.export_clicked();
    m.proceed_anyway();
    settle(&mut m);
    let dds = walk(&d.path().join("export"))
        .into_iter()
        .find(|p| p.extension().is_some_and(|e| e == "dds"))
        .expect("the export wrote a .dds");
    let root = d.path().join("textures/landscape");
    std::fs::copy(&dds, root.join("stored.dds")).unwrap();
    m.open_folder(d.path().join("textures"));
    settle(&mut m);
    m.expand("landscape/stored.dds");
    settle(&mut m);
    let EncodedState::Done(out) = &m.preview.encoded else {
        panic!("not done: {:?}", m.preview.encoded)
    };
    assert!(!out.result.levels.is_empty());
    assert_eq!(out.decoded.len(), out.result.levels.len());
    assert!(out.result.source.levels.is_empty(), "no source side");
    assert_eq!(
        m.preview.notice.as_deref(),
        Some("DDS input: no source image")
    );
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = vec![];
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

#[test]
fn a_result_for_a_collapsed_texture_is_dropped() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.expand("landscape/rock01_rmaos");
    m.encode_now();
    m.collapse();
    assert!(!m.busy(), "collapsing cancels the work");
    std::thread::sleep(Duration::from_millis(500));
    m.poll();
    assert_eq!(m.preview.item, None);
    assert!(matches!(m.preview.encoded, EncodedState::NotEncoded));
}

#[test]
fn live_encode_is_saved_and_encodes_on_expand() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let path = d.path().join("gui.toml");
    m.settings_path = Some(path.clone());
    m.set_live(true);
    assert!(sdc_gui::settings::GuiSettings::load(&path).0.live_encode);
    m.expand("landscape/rock01_rmaos");
    settle(&mut m);
    assert!(matches!(m.preview.encoded, EncodedState::NotEncoded));
    let t0 = Instant::now();
    m.tick(t0);
    m.tick(t0 + DEBOUNCE);
    assert!(matches!(m.preview.encoded, EncodedState::Encoding { .. }));
    settle(&mut m);
    assert!(matches!(m.preview.encoded, EncodedState::Done(_)));
}
