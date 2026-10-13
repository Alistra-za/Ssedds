//! P1-C T-S6 and C11: the user's CS settings applied to a loaded model, on WARP with the stand-in shader. The
//! test is the pool, as in async_compile.rs: it takes the jobs and runs them with `run_job`.
mod common;
use common::{cache_files, channel, covered, fixture, renderer, FLAT};
use render3d::cs_settings::CsSettings;
use render3d::device::Gpu;
use render3d::scene::Scene;
use render3d::{parse, run_job, Renderer, ShapeStatus};
use std::path::Path;

/// FLAT plus the stand-in's FeatureData hook: the flat colour × `linearLightingSettings.ambientMult`.
const FEATURE_DATA: &[&str] = &["STANDIN_OPAQUE_OK", "STANDIN_FLAT", "STANDIN_FEATURE_DATA"];
const N: u32 = 64;

fn none(_: &str) -> Option<Vec<u8>> {
    None
}

/// Settings as `read_settings` would give them for a SettingsUser.json with only a "Linear Lighting" section.
/// Both `linear_lighting` and the merged key are set, so the test does not depend on which one
/// `preview_defines` reads.
fn settings(ll: bool, ambient: f64) -> CsSettings {
    let merged = serde_json::json!({
        "Linear Lighting": {"enableLinearLighting": i32::from(ll), "ambientMult": ambient}
    });
    CsSettings {
        hash: format!("test ll={ll} ambient={ambient}"),
        linear_lighting: ll,
        merged: merged.as_object().unwrap().clone(),
        ..CsSettings::default()
    }
}

fn statuses(r: &Renderer) -> Vec<ShapeStatus> {
    r.shapes().into_iter().map(|s| s.status).collect()
}

fn rgb(px: &[u8]) -> u64 {
    channel(px, 0) + channel(px, 1) + channel(px, 2)
}

/// Everything queued so far, compiled on this thread and applied. Returns how many jobs there were.
fn drain(r: &mut Renderer) -> usize {
    let jobs = r.take_jobs();
    let n = jobs.len();
    for j in jobs {
        r.apply_compiled(run_job(j));
    }
    n
}

/// The furniture fixture (2 Lighting shapes) with `features` and settings `s`, async, every program compiled.
fn furniture(cache: &Path, features: &[&str], s: CsSettings) -> Renderer {
    let mut r = renderer(cache, features);
    r.set_async_compile(true);
    r.set_settings(s);
    let rep = r.load_parsed(&parse(&fixture("TestNifFile_Furniture_Col_SE.nif")), &none);
    assert!(
        rep.errors.is_empty() && rep.compile_errors.is_empty(),
        "{rep:?}"
    );
    assert!(drain(&mut r) >= 1);
    assert_eq!(statuses(&r), vec![ShapeStatus::Drawing; 2]);
    r
}

/// 03 T-S6: LL off → on is a define change: every shape goes through the permutation path, old draws on screen.
#[test]
fn settings_change_of_define_goes_through_swap() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = furniture(cache.path(), FLAT, settings(false, 1.0));
    assert_eq!(r.features(), FLAT, "LL off: no ENABLE_LL");
    let scene = Scene::neutral();
    // C11 switches the display encode on with LL at once; what must not change meanwhile is the draws, so the
    // reference frame is the old draws read back with the encode on.
    r.set_encode_srgb(true);
    let before = r.render(&scene, N, N).unwrap();
    assert!(covered(&before) > 0);
    r.set_settings(settings(true, 1.0));
    assert!(r.features().iter().any(|f| f == "ENABLE_LL"));
    assert_eq!(
        statuses(&r),
        vec![
            ShapeStatus::Pending {
                showing_previous: true
            };
            2
        ]
    );
    let jobs = r.take_jobs();
    assert!(
        !jobs.is_empty(),
        "the new programs are queued, not compiled inline"
    );
    assert_eq!(
        r.render(&scene, N, N).unwrap(),
        before,
        "the old draws, byte-identical, while the programs compile"
    );
    for j in jobs {
        r.apply_compiled(run_job(j));
    }
    assert_eq!(statuses(&r), vec![ShapeStatus::Drawing; 2]);
    assert!(covered(&r.render(&scene, N, N).unwrap()) > 0);
}

/// C11: same defines, new FeatureData values → a cbuffer edit only: no job, no cache entry, next frame differs.
#[test]
fn a_settings_change_without_define_change_compiles_nothing() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = furniture(cache.path(), FEATURE_DATA, settings(true, 1.0));
    let scene = Scene::neutral();
    let before = r.render(&scene, N, N).unwrap();
    assert_eq!(r.render(&scene, N, N).unwrap(), before);
    assert_eq!(r.last_uploads(), 0, "a steady frame uploads nothing");
    let features = r.features().to_vec();
    let files = cache_files(cache.path());
    r.set_settings(settings(true, 0.5));
    assert_eq!(r.features(), features, "the defines did not change");
    assert!(r.take_jobs().is_empty(), "no compile job");
    assert_eq!(statuses(&r), vec![ShapeStatus::Drawing; 2]);
    let after = r.render(&scene, N, N).unwrap();
    assert!(
        r.last_uploads() > 0,
        "the rewritten static block is uploaded"
    );
    assert_eq!(cache_files(cache.path()), files, "nothing compiled");
    assert!(
        rgb(&after) < rgb(&before),
        "ambientMult 1 → 0.5 darkens: {} → {}",
        rgb(&before),
        rgb(&after)
    );
}

/// C8: loaded features (Info.ini) the preview does not compile; TruePBR (settings only) is never listed.
#[test]
fn off_in_preview_lists_loaded_but_uncompiled_features() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), FLAT);
    let mut s = settings(true, 1.0);
    s.loaded = ["CSEditor", "LightLimitFix", "LinearLighting", "TruePBR"]
        .map(String::from)
        .into();
    r.set_features_on(false);
    r.set_settings(s.clone());
    assert_eq!(r.off_in_preview(), ["LightLimitFix", "LinearLighting"]);
    r.set_features_on(true);
    assert_eq!(r.off_in_preview(), ["LightLimitFix"]);
    assert_eq!(r.settings(), &s);
}

/// C11: a device rebuild keeps the settings, the toggle, the FeatureData values and the sRGB mode.
#[test]
fn rebuild_device_on_keeps_settings() {
    let cache = tempfile::tempdir().unwrap();
    let s = settings(true, 0.5);
    let mut r = furniture(cache.path(), FEATURE_DATA, s.clone());
    r.set_features_on(false); // drops ENABLE_LL: a define change, compiled here
    assert!(drain(&mut r) >= 1);
    assert_eq!(statuses(&r), vec![ShapeStatus::Drawing; 2]);
    let scene = Scene::neutral();
    let before = r.render(&scene, N, N).unwrap();
    let features = r.features().to_vec();
    r.rebuild_device_on(Gpu::new_warp().unwrap()).unwrap();
    assert_eq!(r.settings(), &s);
    assert_eq!(r.features(), features);
    let rep = r.load_parsed(&parse(&fixture("TestNifFile_Furniture_Col_SE.nif")), &none);
    assert_eq!((rep.shapes, rep.pending), (2, 0), "{rep:?}");
    assert_eq!(
        r.render(&scene, N, N).unwrap(),
        before,
        "same FeatureData values and sRGB mode"
    );
    // The toggle survived too: re-applying the same settings leaves ENABLE_LL off and compiles nothing.
    r.set_settings(s);
    assert_eq!(r.features(), features);
    assert!(r.take_jobs().is_empty());
}
