//! M6 Track F2 Task 10 (owner, 2026-10-12): the roughness variance bake reads the `_n` after its RNM blend, and a
//! layer edit re-bakes the roughness member (DECISIONS Note W18).
use engine::api::*;
use engine::dds::read_dds;
use engine::decode::{decode_level, Decoded};
use engine::events::*;
use image::{ImageBuffer, Rgb, Rgba};
use std::path::Path;
use std::sync::Arc;

mod rnm_support;
use rnm_support::{plan_dir, png};

fn run(plan: Plan) -> BatchSummary {
    let rep = Reporter::new(Arc::new(VecSink::default()), "rnm-bake");
    Engine::new()
        .unwrap()
        .run(plan, &rep, &CancelToken::default())
        .unwrap()
}

/// A flat `_n` and a constant mid roughness `_rmaos`, so the only variance is what the layers add.
fn fixture(dir: &Path) {
    png(dir, "iron.png");
    let w =
        |name: &str, f: &dyn Fn() -> image::RgbaImage| f().save(dir.join("in").join(name)).unwrap();
    w("iron_n.png", &|| {
        ImageBuffer::from_fn(32, 32, |_, _| Rgba([128, 128, 255, 255]))
    });
    w("iron_rmaos.png", &|| {
        ImageBuffer::from_fn(32, 32, |_, _| Rgba([128, 0, 255, 255]))
    });
}

/// A strong ripple normal; `h` changes the bytes.
fn detail(dir: &Path, h: u32) {
    ImageBuffer::from_fn(32, 32, |x, y| {
        Rgb([
            (128.0 + 100.0 * (x as f32 * 1.7 + h as f32).sin()) as u8,
            (128.0 + 100.0 * (y as f32 * 1.3).cos()) as u8,
            200u8,
        ])
    })
    .save(dir.join("in/iron_detailnorm.png"))
    .unwrap();
}

/// Mean decoded R of mip `level` of `out/iron_rmaos.dds`.
fn mean_r(dir: &Path, level: usize) -> f64 {
    let f = read_dds(&std::fs::read(dir.join("out/iron_rmaos.dds")).unwrap()).unwrap();
    let (w, h, r) = f.levels[level].clone();
    let Decoded::Ldr(px) = decode_level(f.format, w, h, &f.data[r], None).unwrap() else {
        panic!("LDR expected")
    };
    px.iter().map(|p| f64::from(p[0])).sum::<f64>() / px.len() as f64
}

fn record(dir: &Path) -> engine::manifest::ManifestEntry {
    engine::manifest::Manifest::at(&dir.join("out"))
        .load()
        .unwrap()["iron_rmaos.dds"]
        .clone()
}

#[test]
fn bake_reads_the_blended_normal() {
    let d = tempfile::tempdir().unwrap();
    fixture(d.path());
    assert_eq!(run(plan_dir(d.path(), vec![])).failed, 0);
    let (flat, before) = (mean_r(d.path(), 2), record(d.path()));
    detail(d.path(), 0);
    let s = run(plan_dir(d.path(), vec![]));
    assert!(s.converted >= 1 && s.failed == 0);
    assert_ne!(
        record(d.path()).profile_sha256.clone() + &format!("{:?}", record(d.path()).sources),
        before.profile_sha256.clone() + &format!("{:?}", before.sources)
    );
    let rough = mean_r(d.path(), 2);
    assert!(
        rough > flat + 2.0,
        "layers must widen the roughness: {rough} vs {flat}"
    );
}

#[test]
fn bake_records_rnm_inputs() {
    let d = tempfile::tempdir().unwrap();
    fixture(d.path());
    detail(d.path(), 0);
    assert_eq!(run(plan_dir(d.path(), vec![])).failed, 0);
    let roles: Vec<_> = record(d.path())
        .sources
        .iter()
        .filter_map(|r| r.rnm.clone())
        .collect();
    assert!(roles.contains(&"bake.detail".to_string()), "{roles:?}");
}

#[test]
fn layer_edit_rebakes() {
    let d = tempfile::tempdir().unwrap();
    fixture(d.path());
    detail(d.path(), 0);
    run(plan_dir(d.path(), vec![]));
    assert_eq!(run(plan_dir(d.path(), vec![])).converted, 0);
    let before = std::fs::read(d.path().join("out/iron_rmaos.dds")).unwrap();
    detail(d.path(), 3);
    let s = run(plan_dir(d.path(), vec![]));
    assert!(
        s.converted >= 2,
        "the _n and the _rmaos both re-encode: {}",
        s.converted
    );
    assert_ne!(
        std::fs::read(d.path().join("out/iron_rmaos.dds")).unwrap(),
        before
    );
}
