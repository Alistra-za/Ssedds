//! M3a Task 7: the session cache re-resolves without reloading (spec §4.4) and makes thumbnails.
use encoders_sys::Bc1Model;
use engine::api::*;
use engine::config::Target;
use engine::events::*;
use engine::image::{Rgba32F, SourceInfo};
use engine::overrides::parse_set;
use engine::session::*;
use image::{ImageBuffer, Luma, Rgb};
use std::path::Path;
use std::sync::Arc;

fn fixture(dir: &Path) {
    let r = dir.join("in");
    std::fs::create_dir_all(&r).unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Rgb([x as u8 * 4, y as u8 * 4, 90]))
        .save(r.join("iron.png"))
        .unwrap();
    for (name, v) in [("iron_roughness.png", 120u8), ("iron_ao.png", 200)] {
        ImageBuffer::from_fn(64, 64, |x, _| Luma([v.wrapping_add(x as u8)]))
            .save(r.join(name))
            .unwrap();
    }
}

fn req(dir: &Path, target: Target) -> BatchRequest {
    let mut r = BatchRequest::new(vec![dir.join("in")], dir.join("out"), target);
    r.overrides = vec![parse_set("file:iron.png.subtype=diffuse").unwrap()];
    r
}

fn plan(s: &Session, r: BatchRequest) -> Plan {
    s.plan(
        r,
        &Reporter::new(Arc::new(NullSink), "t"),
        &CancelToken::default(),
    )
    .unwrap()
}

#[test]
fn replanning_reuses_analyses_and_a_target_change_reloads_only_pack_sources() {
    let d = tempfile::tempdir().unwrap();
    fixture(d.path());
    let s = Session::new(Bc1Model::Ideal).unwrap();
    plan(&s, req(d.path(), Target::Cs));
    // Pass 1 loads the three files; pass 2 assembles _rmaos from its two sources.
    assert_eq!(s.cache().loads(), 5);
    plan(&s, req(d.path(), Target::Cs));
    assert_eq!(s.cache().loads(), 5, "an identical re-plan loads nothing");
    plan(&s, req(d.path(), Target::Cpm));
    // The cpm `_m` packs roughness (as glossiness) and has no AO channel, so the new recipe has one source: only it
    // loads, and the three files' pass-1 analyses are reused.
    assert_eq!(
        s.cache().loads(),
        6,
        "cpm packs a _m from roughness alone: only that source loads"
    );
}

#[test]
fn a_changed_file_reloads_once() {
    let d = tempfile::tempdir().unwrap();
    fixture(d.path());
    let s = Session::new(Bc1Model::Ideal).unwrap();
    plan(&s, req(d.path(), Target::Cs));
    let before = s.cache().loads();
    // A different size is a different stamp, whatever the timestamp resolution.
    ImageBuffer::from_fn(32, 32, |_, _| Rgb([1u8, 2, 3]))
        .save(d.path().join("in/iron.png"))
        .unwrap();
    let p = plan(&s, req(d.path(), Target::Cs));
    assert_eq!(s.cache().loads(), before + 1);
    let iron = p
        .jobs
        .iter()
        .find(|j| j.input.ends_with("iron.png"))
        .unwrap();
    assert_eq!(s.thumbnail(iron).unwrap().width, 32);
}

/// `pack_key` must change when a pack source changes, or the packed analysis would be stale.
#[test]
fn a_changed_pack_source_reassembles_its_recipe() {
    let d = tempfile::tempdir().unwrap();
    fixture(d.path());
    let s = Session::new(Bc1Model::Ideal).unwrap();
    let p = plan(&s, req(d.path(), Target::Cs));
    let stamp = |p: &Plan| p.jobs.iter().find_map(|j| j.pack.as_ref()).unwrap().sources[0].stamp;
    let before_stamp = stamp(&p);
    let before = s.cache().loads();
    ImageBuffer::from_fn(64, 64, |x, y| Luma([((x + y) % 256) as u8]))
        .save(d.path().join("in/iron_roughness.png"))
        .unwrap();
    // A different byte size makes a different stamp whatever the timestamp resolution; if the PNG encoder
    // happens to produce the same size, write 32x32 sources for both maps instead.
    let p = plan(&s, req(d.path(), Target::Cs));
    assert_eq!(
        s.cache().loads(),
        before + 1 + 2,
        "pass 1 reloads the map, pass 2 reassembles from both sources"
    );
    assert_ne!(stamp(&p), before_stamp);
}

#[test]
fn engine_plan_builds_no_thumbnails() {
    let cache = AnalysisCache::default();
    assert!(!cache.thumbnails);
}

#[test]
fn thumbnails_of_float_and_16_bit_sources_are_visible() {
    // Float: linear radiance 4.0 tone-maps below white and well above black.
    let img = Rgba32F::new(512, 256, [4.0, 0.5, 0.05, 1.0]);
    let info = SourceInfo {
        bit_depth: 32,
        channels: 4,
        has_alpha: false,
        is_float: true,
    };
    let t = make_thumbnail(&img, &info);
    assert_eq!((t.width, t.height), (256, 128));
    let px = &t.rgba[..4];
    assert!(px[0] > 150 && px[0] < 255, "{px:?}");
    assert!(px[2] > 0, "{px:?}");
    // 16-bit PNG through the planner: mid-grey stays mid-grey.
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("in")).unwrap();
    ImageBuffer::from_fn(64, 64, |_, _| Rgb([30000u16, 30000, 30000]))
        .save(d.path().join("in/deep.png"))
        .unwrap();
    let s = Session::new(Bc1Model::Ideal).unwrap();
    let mut r = BatchRequest::new(vec![d.path().join("in")], d.path().join("out"), Target::Cs);
    r.overrides = vec![parse_set("file:deep.png.subtype=diffuse").unwrap()];
    let p = plan(&s, r);
    let t = s.thumbnail(&p.jobs[0]).unwrap();
    assert!((100..=140).contains(&t.rgba[0]), "{:?}", &t.rgba[..4]);
}

/// A recipe with a source that has no stamp is never cached (nothing says when that source changed), and a
/// serialisable one is keyed by everything including its stamps.
#[test]
fn a_packed_image_with_an_unstamped_source_is_not_cached() {
    let d = tempfile::tempdir().unwrap();
    fixture(d.path());
    let s = Session::new(Bc1Model::Ideal).unwrap();
    let p = plan(&s, req(d.path(), Target::Cs));
    let pk = p.jobs.iter().find_map(|j| j.pack.clone()).unwrap();
    assert!(pk.sources.iter().all(|s| s.stamp.is_some()));
    let key = pack_cache_key(&pk).expect("a stamped recipe is cached");
    assert_eq!(pack_key(&pk), Some(key));
    let mut unstamped = pk.clone();
    unstamped.sources[1].stamp = None;
    assert_eq!(pack_cache_key(&unstamped), None);
    assert!(pack_key(&unstamped).is_some(), "the plain key still exists");
    // Its thumbnail is not looked up either.
    let mut job = p.jobs.iter().find(|j| j.pack.is_some()).unwrap().clone();
    assert!(s.thumbnail(&job).is_some());
    job.pack = Some(unstamped);
    assert!(s.thumbnail(&job).is_none());
}

/// Audit 8 (DECISIONS Note Y7): thumbnails are shared, not copied, and a plan of another folder drops the first
/// folder's entries.
#[test]
fn a_plan_of_another_folder_drops_the_first_folders_entries() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    fixture(a.path());
    fixture(b.path());
    let s = Session::new(Bc1Model::Ideal).unwrap();
    plan(&s, req(a.path(), Target::Cs));
    let iron_a = a.path().join("in").join("iron.png");
    let t = s.cache().thumbnail_of(&iron_a).unwrap();
    assert!(Arc::ptr_eq(&t, &s.cache().thumbnail_of(&iron_a).unwrap()));
    plan(&s, req(b.path(), Target::Cs));
    assert!(s.cache().thumbnail_of(&iron_a).is_none());
    assert!(s
        .cache()
        .thumbnail_of(&b.path().join("in").join("iron.png"))
        .is_some());
}

/// A cancelled plan saw only part of its folder: it must not evict the entries it did not cover.
#[test]
fn a_cancelled_plan_keeps_the_cached_entries() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    fixture(a.path());
    fixture(b.path());
    let s = Session::new(Bc1Model::Ideal).unwrap();
    plan(&s, req(a.path(), Target::Cs));
    let cancel = CancelToken::default();
    cancel.cancel();
    let p = s
        .plan(
            req(b.path(), Target::Cs),
            &Reporter::new(Arc::new(NullSink), "t"),
            &cancel,
        )
        .unwrap();
    assert!(!p.cancelled.is_empty());
    assert!(s
        .cache()
        .thumbnail_of(&a.path().join("in").join("iron.png"))
        .is_some());
}
