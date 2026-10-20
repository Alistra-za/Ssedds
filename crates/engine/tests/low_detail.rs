//! v2 artist checks, check 3 (fake resolution) at convert time and the per-file `drop_top_mip` field
//! (DECISIONS Note CI).
use engine::api::*;
use engine::codes::Code;
use engine::config::Target;
use engine::events::*;
use engine::manifest::Manifest;
use engine::overrides::parse_set;
use engine::policy::AcceptSuggestions;
use image::{ImageBuffer, Rgb, RgbImage};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// `n`² RGB white noise from a fixed LCG.
fn noise(n: u32, seed: u64) -> RgbImage {
    let mut s = seed;
    ImageBuffer::from_fn(n, n, |_, _| {
        let mut b = || {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (s >> 56) as u8
        };
        Rgb([b(), b(), b()])
    })
}

/// `in/noise.png` (native 1024² noise) and `in/up.png` (512² noise upscaled 2x, nearest).
fn fixtures(dir: &Path, seed: u64) {
    let i = dir.join("in");
    std::fs::create_dir_all(&i).unwrap();
    noise(1024, 1).save(i.join("noise.png")).unwrap();
    upscaled(seed).save(i.join("up.png")).unwrap();
}

fn upscaled(seed: u64) -> RgbImage {
    let small = noise(512, seed);
    ImageBuffer::from_fn(1024, 1024, |x, y| *small.get_pixel(x / 2, y / 2))
}

fn req(dir: &Path, out: &str, mode: LowDetailMode) -> BatchRequest {
    let mut r = BatchRequest::new(vec![dir.join("in")], dir.join(out), Target::Cs);
    r.recursive = true;
    r.accept = AcceptSuggestions::High;
    r.overrides = vec![
        parse_set("subtype=diffuse").unwrap(),
        parse_set("encoder.effort=ultrafast").unwrap(),
    ];
    r.low_detail = mode;
    r
}

fn run(req: BatchRequest) -> (BatchSummary, Vec<EngineEvent>) {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), new_batch_id());
    let e = Engine::new().unwrap();
    let plan = e.plan(req, &rep, &CancelToken::default()).unwrap();
    let s = e.run(plan, &rep, &CancelToken::default()).unwrap();
    (s, sink.take())
}

fn is_for(e: &EngineEvent, name: &str) -> bool {
    e.job.as_deref() == Some(name)
}

/// The CHK-LOW-DETAIL warnings for job `name`.
fn low_detail(ev: &[EngineEvent], name: &str) -> Vec<engine::codes::Warning> {
    ev.iter()
        .filter(|e| is_for(e, name))
        .filter_map(|e| match &e.kind {
            EventKind::Warning(w) if w.code == Code::ChkLowDetail => Some(w.clone()),
            _ => None,
        })
        .collect()
}

/// The `file_skipped` (kind, code, reason) for job `name`.
fn skips(ev: &[EngineEvent], name: &str) -> Vec<(SkipKind, Option<Code>, String)> {
    ev.iter()
        .filter(|e| is_for(e, name))
        .filter_map(|e| match &e.kind {
            EventKind::FileSkipped { kind, code, reason } => Some((*kind, *code, reason.clone())),
            _ => None,
        })
        .collect()
}

/// A DDS file's (width, height, level bytes).
fn dds(p: &Path) -> (u32, u32, Vec<Vec<u8>>) {
    let b = std::fs::read(p).unwrap();
    let f = engine::dds::read_dds(&b).unwrap();
    let levels = f
        .levels
        .iter()
        .map(|l| f.data[l.2.clone()].to_vec())
        .collect();
    (f.width, f.height, levels)
}

fn mtime(p: &Path) -> std::time::SystemTime {
    std::fs::metadata(p).unwrap().modified().unwrap()
}

#[test]
fn hold_skips_a_flagged_texture_intentionally_and_writes_the_rest() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path(), 2);
    let (s, ev) = run(req(d.path(), "out", LowDetailMode::Hold));
    let w = low_detail(&ev, "up.dds");
    assert_eq!(w.len(), 1, "{ev:?}");
    assert_eq!(w[0].data["width"], 1024);
    assert_eq!(w[0].data["height"], 1024);
    assert!(w[0].data["ratio"].as_f64().unwrap() < engine::checks::LOW_DETAIL_RATIO);
    assert!(
        w[0].message
            .contains("a 1024x1024 texture carries about 512x512 of detail"),
        "{}",
        w[0].message
    );
    assert_eq!(
        skips(&ev, "up.dds"),
        vec![(
            SkipKind::LowDetail,
            Some(Code::ChkLowDetail),
            "held (low detail)".to_string()
        )]
    );
    assert!(!d.path().join("out/up.dds").exists());
    assert!(d.path().join("out/noise.dds").exists());
    assert!(low_detail(&ev, "noise.dds").is_empty());
    assert_eq!((s.converted, s.skipped, s.skipped_intentional), (1, 1, 1));
    assert_eq!(s.exit_code(), 0);
}

#[test]
fn a_held_texture_with_an_earlier_output_says_previous_output_kept() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path(), 2);
    run(req(d.path(), "out", LowDetailMode::Off));
    let up = d.path().join("out/up.dds");
    let before = std::fs::read(&up).unwrap();
    // A different upscale: the source changed, so this is the stale (encode) path, not the measure-only one.
    upscaled(3).save(d.path().join("in/up.png")).unwrap();
    let (s, ev) = run(req(d.path(), "out", LowDetailMode::Hold));
    assert_eq!(low_detail(&ev, "up.dds").len(), 1);
    assert_eq!(
        skips(&ev, "up.dds"),
        vec![(
            SkipKind::LowDetail,
            Some(Code::ChkLowDetail),
            "held (low detail); previous output kept".to_string()
        )]
    );
    assert_eq!(std::fs::read(&up).unwrap(), before);
    assert_eq!(s.exit_code(), 0);
}

#[test]
fn hold_flags_an_up_to_date_texture_again() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path(), 2);
    run(req(d.path(), "out", LowDetailMode::Off));
    let (up, noise) = (d.path().join("out/up.dds"), d.path().join("out/noise.dds"));
    let (t_up, t_noise) = (mtime(&up), mtime(&noise));
    let (s, ev) = run(req(d.path(), "out", LowDetailMode::Hold));
    assert_eq!(low_detail(&ev, "up.dds").len(), 1);
    assert_eq!(
        skips(&ev, "up.dds"),
        vec![(
            SkipKind::LowDetail,
            Some(Code::ChkLowDetail),
            "held (low detail); previous output kept".to_string()
        )]
    );
    assert_eq!(
        skips(&ev, "noise.dds"),
        vec![(SkipKind::UpToDate, None, "up to date".to_string())]
    );
    assert!(low_detail(&ev, "noise.dds").is_empty());
    assert_eq!((mtime(&up), mtime(&noise)), (t_up, t_noise));
    assert_eq!((s.converted, s.up_to_date, s.skipped), (0, 1, 1));
    assert_eq!(s.exit_code(), 0);
}

#[test]
fn warn_writes_the_flagged_texture_and_lists_the_warning() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path(), 2);
    let (s, ev) = run(req(d.path(), "out", LowDetailMode::Warn));
    assert_eq!(low_detail(&ev, "up.dds").len(), 1);
    assert!(low_detail(&ev, "noise.dds").is_empty());
    assert!(skips(&ev, "up.dds").is_empty());
    let (w, h, _) = dds(&d.path().join("out/up.dds"));
    assert_eq!((w, h), (1024, 1024));
    assert_eq!(s.converted, 2);
}

fn records(root: &Path) -> Vec<serde_json::Value> {
    let m = Manifest::at(root);
    let mut v: Vec<serde_json::Value> = std::fs::read_to_string(m.path())
        .unwrap()
        .lines()
        .map(|l| {
            let mut r: serde_json::Value = serde_json::from_str(l).unwrap();
            r["ts"] = serde_json::Value::Null;
            r["output_stamp"] = serde_json::Value::Null;
            r
        })
        .collect();
    v.sort_by_key(|r| r["output"].to_string());
    v
}

fn dds_files(root: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "dds"))
        .collect();
    v.sort();
    v
}

#[test]
fn warn_and_off_write_identical_bytes_and_off_is_the_default() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path(), 2);
    let def = BatchRequest::new(vec![], PathBuf::new(), Target::Cs);
    assert_eq!(def.low_detail, LowDetailMode::Off);
    let (_, off_ev) = run(req(d.path(), "off", LowDetailMode::Off));
    assert!(low_detail(&off_ev, "up.dds").is_empty());
    run(req(d.path(), "warn", LowDetailMode::Warn));
    let (off, warn) = (d.path().join("off"), d.path().join("warn"));
    let names = |v: Vec<PathBuf>| -> Vec<_> {
        v.iter()
            .map(|p| p.file_name().unwrap().to_owned())
            .collect()
    };
    assert_eq!(names(dds_files(&off)), names(dds_files(&warn)));
    assert_eq!(dds_files(&off).len(), 2);
    for p in dds_files(&off) {
        let q = warn.join(p.file_name().unwrap());
        assert!(
            std::fs::read(&p).unwrap() == std::fs::read(&q).unwrap(),
            "{p:?}"
        );
    }
    assert_eq!(records(&off), records(&warn));
}

#[test]
fn drop_top_mip_writes_mip_1_as_the_top_level() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path(), 2);
    run(req(d.path(), "full", LowDetailMode::Off));
    std::fs::write(
        d.path().join("in/sdc.toml"),
        "[files.\"up.png\"]\ndrop_top_mip = true\n",
    )
    .unwrap();
    run(req(d.path(), "half", LowDetailMode::Off));
    let (_, _, full) = dds(&d.path().join("full/up.dds"));
    let (w, h, half) = dds(&d.path().join("half/up.dds"));
    assert_eq!((w, h), (512, 512));
    assert_eq!(half.len() + 1, full.len());
    for (n, l) in half.iter().enumerate() {
        assert!(l == &full[n + 1], "level {n}");
    }
    // The other file is untouched by the entry.
    assert_eq!(
        std::fs::read(d.path().join("full/noise.dds")).unwrap(),
        std::fs::read(d.path().join("half/noise.dds")).unwrap()
    );
    // At its default the field is absent from the serialised profile, so the profile hash holds.
    let p = engine::presets::PresetRegistry::builtin()
        .unwrap()
        .profile("cs-quality", Target::Cs, engine::config::Subtype::Diffuse)
        .unwrap();
    assert!(!p.drop_top_mip);
    assert!(serde_json::to_value(&p).unwrap()["drop_top_mip"].is_null());
}

/// The variance a `_n` gives its set's `_rmaos` is measured on the full chain: dropping the `_n`'s top level leaves
/// the bake unchanged.
#[test]
fn drop_top_mip_on_an_n_leaves_its_rmaos_bake_unchanged() {
    let d = tempfile::tempdir().unwrap();
    let i = d.path().join("in/pbr");
    std::fs::create_dir_all(&i).unwrap();
    ImageBuffer::from_fn(64, 64, |x, _| {
        Rgb([if x % 2 == 0 { 204u8 } else { 51 }, 128, 230])
    })
    .save(i.join("iron_n.png"))
    .unwrap();
    ImageBuffer::from_fn(64, 64, |x, _| image::Rgba([(x * 4) as u8, 0, 200, 255]))
        .save(i.join("iron_rmaos.png"))
        .unwrap();
    let r = |out: &str, drop: bool| {
        let mut r = BatchRequest::new(vec![d.path().join("in")], d.path().join(out), Target::Cs);
        r.recursive = true;
        r.accept = AcceptSuggestions::High;
        if drop {
            r.overrides = vec![parse_set("file:**/iron_n.png.drop_top_mip=true").unwrap()];
        }
        r
    };
    let (s, ev) = run(r("a", false));
    assert_eq!(s.converted, 2, "{ev:?}");
    run(r("b", true));
    let n = |o: &str| dds(&d.path().join(o).join("pbr/iron_n.dds")).0;
    assert_eq!((n("a"), n("b")), (64, 32));
    assert_eq!(
        std::fs::read(d.path().join("a/pbr/iron_rmaos.dds")).unwrap(),
        std::fs::read(d.path().join("b/pbr/iron_rmaos.dds")).unwrap()
    );
}
