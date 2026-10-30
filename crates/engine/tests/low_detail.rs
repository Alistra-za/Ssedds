//! v2 artist checks, check 3 (fake resolution) at convert time and the per-file `drop_top_mip` field
//! (DECISIONS Note CL).
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

/// Note FC2 (amends CL20): Hold measures and records `CHK-LOW-DETAIL`, but writes the full chain, byte for byte
/// what an Off run writes; no file is withheld.
#[test]
fn hold_measures_but_writes_the_full_chain() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path(), 2);
    let (s, ev) = run(req(d.path(), "out", LowDetailMode::Hold));
    run(req(d.path(), "off", LowDetailMode::Off));
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
    assert!(skips(&ev, "up.dds").is_empty(), "{ev:?}");
    let (out, off) = (d.path().join("out"), d.path().join("off"));
    assert_eq!(dds_files(&out).len(), 2);
    for p in dds_files(&off) {
        let q = out.join(p.file_name().unwrap());
        assert!(
            std::fs::read(&p).unwrap() == std::fs::read(&q).unwrap(),
            "{p:?}"
        );
    }
    assert_eq!(records(&off), records(&out));
    assert!(low_detail(&ev, "noise.dds").is_empty());
    assert_eq!((s.converted, s.skipped, s.skipped_intentional), (2, 0, 0));
    assert_eq!(s.exit_code(), 0);
}

/// CL20 (owner answer 2), kept by FC2: an up-to-date output is measured again under Hold and flagged again, and
/// stays up to date (never re-encoded).
#[test]
fn hold_flags_an_up_to_date_texture_again() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path(), 2);
    run(req(d.path(), "out", LowDetailMode::Off));
    let (up, noise) = (d.path().join("out/up.dds"), d.path().join("out/noise.dds"));
    let (t_up, t_noise) = (mtime(&up), mtime(&noise));
    let (s, ev) = run(req(d.path(), "out", LowDetailMode::Hold));
    assert_eq!(low_detail(&ev, "up.dds").len(), 1);
    for n in ["up.dds", "noise.dds"] {
        assert_eq!(
            skips(&ev, n),
            vec![(SkipKind::UpToDate, None, "up to date".to_string())]
        );
    }
    assert!(low_detail(&ev, "noise.dds").is_empty());
    assert_eq!((mtime(&up), mtime(&noise)), (t_up, t_noise));
    assert_eq!((s.converted, s.up_to_date, s.skipped), (0, 2, 0));
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

fn plan_of(r: BatchRequest) -> Plan {
    let rep = Reporter::new(Arc::new(NullSink), "t");
    Engine::new()
        .unwrap()
        .plan(r, &rep, &CancelToken::default())
        .unwrap()
}

fn job_of<'a>(p: &'a Plan, name: &str) -> &'a engine::policy::ResolvedJob {
    p.jobs
        .iter()
        .find(|j| j.input.file_name().is_some_and(|n| n == name))
        .unwrap_or_else(|| panic!("no job for {name}"))
}

/// A request over `in/` with only `sets` (no sub-type override).
fn plain(dir: &Path, out: &str, sets: &[&str]) -> BatchRequest {
    let mut r = BatchRequest::new(vec![dir.join("in")], dir.join(out), Target::Cs);
    r.recursive = true;
    r.accept = AcceptSuggestions::High;
    r.overrides = sets.iter().map(|s| parse_set(s).unwrap()).collect();
    r
}

/// Review I1: the DIM checks read the size actually written, after the dropped top level.
#[test]
fn the_dim_checks_see_the_size_after_drop_top_mip() {
    let d = tempfile::tempdir().unwrap();
    let i = d.path().join("in");
    std::fs::create_dir_all(&i).unwrap();
    ImageBuffer::from_fn(256, 4, |x, y| Rgb([x as u8, y as u8 * 60, 90]))
        .save(i.join("strip.png"))
        .unwrap();
    std::fs::write(
        i.join("sdc.toml"),
        "[files.\"strip.png\"]\nsubtype = \"diffuse\"\ndrop_top_mip = true\n",
    )
    .unwrap();
    let r = plain(d.path(), "out", &[]);
    let p = plan_of(r.clone());
    let j = job_of(&p, "strip.png");
    let small: Vec<_> = j
        .warnings
        .iter()
        .filter(|w| w.code == Code::DimSmall)
        .collect();
    assert_eq!(small.len(), 1, "{:?}", j.warnings);
    assert_eq!(small[0].severity, engine::codes::Severity::Blocking);
    run(r);
    assert!(!d.path().join("out/strip.dds").exists());

    // A 64² `_flow` written at 32².
    std::fs::remove_file(i.join("strip.png")).unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Rgb([x as u8 * 4, y as u8 * 4, 0]))
        .save(i.join("water_flow.png"))
        .unwrap();
    std::fs::write(
        i.join("sdc.toml"),
        "[files.\"water_flow.png\"]\nsubtype = \"flow\"\ndrop_top_mip = true\n",
    )
    .unwrap();
    let p = plan_of(plain(d.path(), "out", &[]));
    let j = job_of(&p, "water_flow.png");
    assert!(
        j.warnings.iter().any(|w| w.code == Code::DimFlowSmall),
        "{:?}",
        j.warnings
    );
    std::fs::write(
        i.join("sdc.toml"),
        "[files.\"water_flow.png\"]\nsubtype = \"flow\"\n",
    )
    .unwrap();
    let p = plan_of(plain(d.path(), "out", &[]));
    assert!(!job_of(&p, "water_flow.png")
        .warnings
        .iter()
        .any(|w| w.code == Code::DimFlowSmall));

    // `resize = half` plus the field: DIM-RESIZED names the written size.
    std::fs::remove_file(i.join("water_flow.png")).unwrap();
    noise(1024, 1).save(i.join("big.png")).unwrap();
    std::fs::write(
        i.join("sdc.toml"),
        "[files.\"big.png\"]\nsubtype = \"diffuse\"\nresize = \"half\"\ndrop_top_mip = true\n",
    )
    .unwrap();
    let r = plain(d.path(), "out2", &["encoder.effort=ultrafast"]);
    let p = plan_of(r.clone());
    let w = job_of(&p, "big.png")
        .warnings
        .iter()
        .find(|w| w.code == Code::DimResized)
        .cloned()
        .expect("DIM-RESIZED");
    assert_eq!(w.data["to"], serde_json::json!([256, 256]), "{}", w.message);
    assert!(w.message.contains("to 256x256"), "{}", w.message);
    run(r);
    assert_eq!(dds(&d.path().join("out2/big.dds")).0, 256);
}

/// Review M2: an explicit `false` hashes as the absent key (still up to date); `true` re-encodes.
#[test]
fn an_explicit_false_stays_up_to_date_and_true_re_encodes() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path(), 2);
    run(req(d.path(), "out", LowDetailMode::Off));
    let up = d.path().join("out/up.dds");
    let t0 = mtime(&up);
    let toml = |v: bool| {
        std::fs::write(
            d.path().join("in/sdc.toml"),
            format!("[files.\"up.png\"]\ndrop_top_mip = {v}\n"),
        )
        .unwrap()
    };
    toml(false);
    let (s, _) = run(req(d.path(), "out", LowDetailMode::Off));
    assert_eq!((s.converted, s.up_to_date), (0, 2));
    assert_eq!(mtime(&up), t0);
    toml(true);
    let (s, _) = run(req(d.path(), "out", LowDetailMode::Off));
    assert_eq!((s.converted, s.up_to_date), (1, 1));
    assert_eq!(dds(&up).0, 512);
}

/// Review M3: on a single-level chain the field does nothing, so it is left out of the profile (and the hash).
#[test]
fn drop_top_mip_on_a_single_level_chain_is_ignored() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path(), 2);
    let mut r = req(d.path(), "out", LowDetailMode::Off);
    r.overrides
        .push(parse_set("file:**/up.png.mips.levels=none").unwrap());
    run(r.clone());
    let up = d.path().join("out/up.dds");
    let t0 = mtime(&up);
    r.overrides
        .push(parse_set("file:**/up.png.drop_top_mip=true").unwrap());
    assert!(!job_of(&plan_of(r.clone()), "up.png").profile.drop_top_mip);
    let (s, _) = run(r);
    assert_eq!((s.converted, s.up_to_date), (0, 2));
    assert_eq!(mtime(&up), t0);
    assert_eq!(dds(&up).0, 1024);
}

/// Review M10: a source re-saved with the same bytes after planning is still measured under Hold (the content was
/// checked by hash), not failed for a changed stamp.
#[test]
fn hold_measures_a_touched_but_identical_source() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path(), 2);
    run(req(d.path(), "out", LowDetailMode::Off));
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), new_batch_id());
    let e = Engine::new().unwrap();
    let plan = e
        .plan(
            req(d.path(), "out", LowDetailMode::Hold),
            &rep,
            &CancelToken::default(),
        )
        .unwrap();
    let src = d.path().join("in/up.png");
    let f = std::fs::OpenOptions::new().write(true).open(&src).unwrap();
    f.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(60))
        .unwrap();
    drop(f);
    let s = e.run(plan, &rep, &CancelToken::default()).unwrap();
    let ev = sink.take();
    assert_eq!(s.failed, 0, "{ev:?}");
    assert_eq!(low_detail(&ev, "up.dds").len(), 1);
    assert_eq!(skips(&ev, "up.dds")[0].0, SkipKind::UpToDate);
    assert_eq!(s.exit_code(), 0);
}
