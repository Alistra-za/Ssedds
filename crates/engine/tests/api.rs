use engine::api::*;
use engine::codes::Code;
use engine::config::Target;
use engine::events::*;
use engine::manifest::Manifest;
use engine::overrides::parse_set;
use engine::policy::AcceptSuggestions;
use image::{ImageBuffer, Rgb, Rgba};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn fixtures(dir: &Path) {
    let t = dir.join("textures/armor");
    std::fs::create_dir_all(&t).unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| {
        Rgba([128 + (x as u8 % 8), 128, 250, 100 + y as u8])
    })
    .save(t.join("iron_n.png"))
    .unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Rgb([x as u8 * 4, y as u8 * 4, 90]))
        .save(t.join("iron.png"))
        .unwrap();
    ImageBuffer::from_fn(64, 64, |x, _| {
        Rgba([200u8, 30, 30, if x < 32 { 255 } else { 0 }])
    })
    .save(t.join("leaf.png"))
    .unwrap();
}

fn kind(e: &EngineEvent) -> String {
    serde_json::to_value(&e.kind).unwrap()["kind"]
        .as_str()
        .unwrap()
        .to_string()
}

fn kinds(ev: &[EngineEvent], name: &str) -> usize {
    ev.iter().filter(|e| kind(e) == name).count()
}

fn request(dir: &Path) -> BatchRequest {
    let mut r = BatchRequest::new(vec![dir.join("textures")], dir.join("out"), Target::Cs);
    r.recursive = true;
    r.accept = AcceptSuggestions::High;
    r.overrides = vec![
        parse_set("file:**/iron.png.subtype=diffuse").unwrap(),
        parse_set("file:**/leaf.png.subtype=diffuse").unwrap(),
    ];
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

fn files_with_ext(root: &Path, ext: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .map(|e| e.into_path())
        .filter(|p| p.extension().is_some_and(|x| x == ext))
        .collect();
    v.sort();
    v
}

#[test]
fn converts_skips_unresolved_and_reruns_as_up_to_date() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let (s, ev) = run(request(d.path()));
    // leaf.png: diffuse with varying alpha -> alpha meaning unconfirmed -> skipped
    assert_eq!((s.converted, s.skipped, s.failed), (2, 1, 0), "{ev:#?}");
    assert_eq!(s.exit_code(), 1);
    assert!(d.path().join("out/armor/iron_n.dds").exists());
    assert!(!d.path().join("out/armor/leaf.dds").exists());
    assert_eq!(kinds(&ev, "file_done"), 2);
    assert_eq!(kinds(&ev, "batch_done"), 1);
    assert!(ev.windows(2).all(|w| w[0].seq < w[1].seq));
    let skipped: Vec<_> = ev.iter().filter(|e| kind(e) == "file_skipped").collect();
    assert_eq!(skipped.len(), 1);
    assert!(skipped[0].path.as_deref().unwrap().ends_with("leaf.png"));
    assert!(matches!(
        skipped[0].kind,
        EventKind::FileSkipped {
            code: Some(Code::SugLowConfidence),
            ..
        }
    ));
    let m = Manifest::at(&d.path().join("out")).load().unwrap();
    assert_eq!(m.len(), 2);
    assert!(m.contains_key("armor/iron_n.dds"), "{:?}", m.keys());

    let mut req = request(d.path());
    req.overrides
        .push(parse_set("file:**/leaf.png.alpha.policy=cutout").unwrap());
    let (s, _) = run(req);
    assert_eq!((s.converted, s.up_to_date, s.skipped), (1, 2, 0));
    assert_eq!(s.exit_code(), 0);
}

#[test]
fn events_follow_the_documented_order() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let (_, ev) = run(request(d.path()));
    let names: Vec<String> = ev.iter().map(kind).collect();
    assert_eq!(names.first().map(String::as_str), Some("batch_started"));
    assert_eq!(names.last().map(String::as_str), Some("batch_done"));
    let first = |k: &str| names.iter().position(|n| n == k).unwrap();
    let last = |k: &str| names.iter().rposition(|n| n == k).unwrap();
    assert!(last("file_discovered") < first("set_resolved"));
    assert!(last("set_resolved") < first("file_planned"));
    assert!(last("file_planned") < first("file_started"));
    // Per file: started, progress, done.
    for path in ev
        .iter()
        .filter(|e| kind(e) == "file_done")
        .map(|e| e.path.clone())
    {
        let of: Vec<&str> = ev
            .iter()
            .filter(|e| e.path == path)
            .map(|e| match &e.kind {
                EventKind::FileStarted => "started",
                EventKind::Progress { .. } => "progress",
                EventKind::FileDone { .. } => "done",
                _ => "other",
            })
            .filter(|k| *k != "other")
            .collect();
        assert_eq!(of.first(), Some(&"started"), "{of:?}");
        assert_eq!(of.last(), Some(&"done"), "{of:?}");
        assert!(of.contains(&"progress"));
    }
}

#[test]
fn dry_run_writes_nothing_and_plans_every_file() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let mut req = request(d.path());
    req.dry_run = true;
    let (_, ev) = run(req);
    assert_eq!(kinds(&ev, "file_planned"), 3);
    assert_eq!(kinds(&ev, "file_started"), 0);
    assert!(!d.path().join("out").exists());
}

#[test]
fn bad_override_value_is_a_usage_error_and_cancel_cancels() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let mut req = request(d.path());
    req.overrides.push(parse_set("format=bc9").unwrap());
    let rep = Reporter::new(Arc::new(NullSink), "x");
    assert!(matches!(
        Engine::new()
            .unwrap()
            .plan(req, &rep, &CancelToken::default()),
        Err(EngineError::Usage(_))
    ));

    let e = Engine::new().unwrap();
    let plan = e
        .plan(request(d.path()), &rep, &CancelToken::default())
        .unwrap();
    let c = CancelToken::default();
    c.cancel();
    let s = e.run(plan, &rep, &c).unwrap();
    assert_eq!(s.cancelled.len(), 2);
    assert_eq!(s.exit_code(), 1);
    assert!(files_with_ext(&d.path().join("out"), "tmp").is_empty());
    assert!(files_with_ext(&d.path().join("out"), "dds").is_empty());
}

#[test]
fn duplicates_and_dds_inputs_are_skipped_with_codes() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let t = d.path().join("textures/armor");
    ImageBuffer::from_fn(4, 4, |_, _| Rgb([1u8, 2, 3]))
        .save(t.join("iron_n.tga"))
        .unwrap();
    std::fs::write(t.join("old.dds"), b"DDS ").unwrap();
    let rep = Reporter::new(Arc::new(NullSink), "x");
    let plan = Engine::new()
        .unwrap()
        .plan(request(d.path()), &rep, &CancelToken::default())
        .unwrap();
    let codes: Vec<_> = plan.skipped.iter().map(|s| s.code).collect();
    assert_eq!(
        codes
            .iter()
            .filter(|c| **c == Some(Code::InDuplicate))
            .count(),
        2
    );
    assert!(codes.contains(&Some(Code::InDdsSkipped)));
}

/// A 512x512 tangent-space normal (`_n`, BC5 under `cs`): 128 block rows, so the encode bands really run in
/// parallel.
fn big_normal(dir: &Path) {
    ImageBuffer::from_fn(512, 512, |x, y| {
        let (u, v) = ((x % 64) as f32 / 63.0 - 0.5, (y % 48) as f32 / 47.0 - 0.5);
        Rgb([(128.0 + 100.0 * u) as u8, (128.0 + 100.0 * v) as u8, 230])
    })
    .save(dir.join("textures/armor/plate_n.png"))
    .unwrap();
}

/// Same inputs with one worker and with every core: byte-identical outputs, all validated.
#[test]
fn output_bytes_do_not_depend_on_the_thread_count() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    big_normal(d.path());
    let mut req = request(d.path());
    req.overrides
        .push(parse_set("file:**/leaf.png.alpha.policy=cutout").unwrap());
    let outs = [1usize, 0].map(|jobs| {
        let mut r = req.clone();
        r.jobs = jobs;
        r.output_root = d.path().join(format!("out{jobs}"));
        let (s, ev) = run(r);
        assert_eq!(s.converted, 4, "{ev:#?}");
        assert_eq!(s.validation_failed, 0, "{ev:#?}");
        assert_eq!(s.exit_code(), 0, "{s:?}");
        files_with_ext(&d.path().join(format!("out{jobs}")), "dds")
    });
    assert_eq!(outs[0].len(), 4);
    for (a, b) in outs[0].iter().zip(&outs[1]) {
        assert_eq!(a.file_name(), b.file_name());
        assert_eq!(
            std::fs::read(a).unwrap(),
            std::fs::read(b).unwrap(),
            "{a:?}"
        );
    }
}

/// The most files between `file_started` and their `file_done`/`file_failed` at any point of the event stream.
fn peak_in_flight(ev: &[EngineEvent]) -> usize {
    let mut open = std::collections::BTreeSet::new();
    let mut peak = 0;
    for e in ev {
        match &e.kind {
            EventKind::FileStarted => {
                open.insert(e.path.clone());
                peak = peak.max(open.len());
            }
            EventKind::FileDone { .. } | EventKind::FileFailed { .. } => {
                open.remove(&e.path);
            }
            _ => {}
        }
    }
    peak
}

/// A budget smaller than one file admits files one at a time and still finishes.
#[test]
fn tiny_memory_budget_still_converts_everything() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let mut req = request(d.path());
    req.mem_budget = 1;
    req.jobs = 4;
    let (s, ev) = run(req);
    assert_eq!((s.converted, s.failed), (2, 0));
    assert_eq!(peak_in_flight(&ev), 1);
}

/// M1: a worker waiting inside a file's band-parallel encode may pick up another admitted file, so admission also
/// caps unfinished files at the pool size; nesting can then never go deeper than the thread count.
#[test]
fn files_in_flight_never_exceed_the_worker_count() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    big_normal(d.path());
    let t = d.path().join("textures/armor");
    for i in 0..12 {
        ImageBuffer::from_fn(128, 128, |x, y| {
            Rgb([(x * 2) as u8, (y * 2) as u8, (i * 20) as u8])
        })
        .save(t.join(format!("cloth{i}_n.png")))
        .unwrap();
    }
    let mut req = request(d.path());
    req.jobs = 2;
    let (s, ev) = run(req);
    assert_eq!((s.converted, s.failed), (15, 0), "{ev:#?}");
    assert!(peak_in_flight(&ev) <= 2, "peak {}", peak_in_flight(&ev));
}

/// Codex-B I1: planning loads and analyses through the same byte admission as conversion, in a plan and in a dry
/// run. Eight 256x256 inputs on four workers: a budget below one file admits them one at a time (the peak is one
/// file's estimate, not the sum), and a budget of 2.5 files never holds more than two.
#[test]
fn planning_admits_loads_through_the_memory_budget() {
    let d = tempfile::tempdir().unwrap();
    let t = d.path().join("textures/armor");
    std::fs::create_dir_all(&t).unwrap();
    for i in 0..8u32 {
        ImageBuffer::from_fn(256, 256, |x, y| Rgb([x as u8, y as u8, (i * 30) as u8]))
            .save(t.join(format!("wall{i}.png")))
            .unwrap();
    }
    // The conversion estimator: float RGBA base with its mip chain, counted three times.
    let one = 256u64 * 256 * 16 * 134 / 100 * 3;
    for dry_run in [false, true] {
        for (budget, max_files) in [(1, 1), (one * 5 / 2, 2)] {
            let mut req = request(d.path());
            req.jobs = 4;
            req.mem_budget = budget;
            req.dry_run = dry_run;
            let sink = Arc::new(VecSink::default());
            let rep = Reporter::new(sink.clone(), new_batch_id());
            let plan = Engine::new()
                .unwrap()
                .plan(req, &rep, &CancelToken::default())
                .unwrap();
            assert_eq!(plan.jobs.len(), 8, "{:#?}", sink.take());
            let peak = plan.peak_admitted_bytes;
            let what = format!("dry_run {dry_run}, budget {budget}: peak {peak}");
            assert!(peak >= one, "{what}");
            assert!(peak <= one * max_files, "{what}");
            if max_files == 1 {
                assert_eq!(peak, one, "{what}");
            }
        }
    }
}

/// I1: a failure after the DDS is written (here the manifest append) removes the output and fails the file, so
/// the next run does not keep an unrecorded output as OUT-UNKNOWN-EXISTING.
#[test]
fn a_failed_manifest_append_removes_the_output() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let manifest = Manifest::at(&d.path().join("out"));
    std::fs::create_dir_all(manifest.path().parent().unwrap()).unwrap();
    std::fs::write(manifest.path(), b"").unwrap();
    let set_readonly = |ro: bool| {
        let mut perm = std::fs::metadata(manifest.path()).unwrap().permissions();
        perm.set_readonly(ro);
        std::fs::set_permissions(manifest.path(), perm).unwrap();
    };
    set_readonly(true);
    let mut req = request(d.path());
    req.sidecar = true;
    let (s, ev) = run(req);
    set_readonly(false);
    assert_eq!((s.converted, s.failed), (0, 2), "{ev:#?}");
    assert_eq!(s.exit_code(), 1);
    assert!(
        files_with_ext(&d.path().join("out"), "dds").is_empty(),
        "{:?}",
        files_with_ext(&d.path().join("out"), "dds")
    );
    assert!(
        files_with_ext(&d.path().join("out"), "json").is_empty(),
        "sidecars are removed too"
    );
    assert_eq!(kinds(&ev, "file_done"), 0);
    let failed: Vec<_> = ev
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::FileFailed { stage, error } => Some((*stage, error.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(failed.len(), 2);
    assert!(
        failed
            .iter()
            .all(|(st, err)| *st == Stage::Write && err.contains("manifest")),
        "{failed:?}"
    );

    // The rerun converts both instead of keeping unrecorded outputs.
    let (s, _) = run(request(d.path()));
    assert_eq!((s.converted, s.failed), (2, 0));
}

/// Trace-debug wave 2 #15: the cleanup after a failed manifest append removes only a sidecar this run wrote; a
/// hand-made `.dds.json` kept by Note BV3 stays.
#[test]
fn a_failed_manifest_append_keeps_a_foreign_sidecar() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let foreign = d.path().join("out/armor/iron.dds.json");
    std::fs::create_dir_all(foreign.parent().unwrap()).unwrap();
    std::fs::write(&foreign, "{\"mine\": true}").unwrap();
    let manifest = Manifest::at(&d.path().join("out"));
    std::fs::create_dir_all(manifest.path().parent().unwrap()).unwrap();
    std::fs::write(manifest.path(), b"").unwrap();
    let set_readonly = |ro: bool| {
        let mut perm = std::fs::metadata(manifest.path()).unwrap().permissions();
        perm.set_readonly(ro);
        std::fs::set_permissions(manifest.path(), perm).unwrap();
    };
    set_readonly(true);
    let mut req = request(d.path());
    req.sidecar = true;
    let (s, ev) = run(req);
    set_readonly(false);
    assert_eq!((s.converted, s.failed), (0, 2), "{ev:#?}");
    assert_eq!(
        std::fs::read_to_string(&foreign).unwrap(),
        "{\"mine\": true}",
        "the foreign sidecar is kept"
    );
    assert_eq!(
        files_with_ext(&d.path().join("out"), "json").len(),
        1,
        "only the foreign sidecar is left"
    );
}

/// Audit 5 (DECISIONS Note Y1): an output replaced after it was recorded is re-encoded, not kept as up to date.
#[test]
fn a_replaced_output_is_reencoded() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    run(request(d.path()));
    let out = d.path().join("out/armor/iron.dds");
    let good = std::fs::read(&out).unwrap();
    std::fs::write(&out, &good[..good.len() / 2]).unwrap();
    let (s, _) = run(request(d.path()));
    assert_eq!((s.converted, s.up_to_date), (1, 1));
    assert_eq!(std::fs::read(&out).unwrap(), good);
}

/// Note Y1: a record from before the stamp re-encodes once, then is up to date.
#[test]
fn a_record_without_an_output_stamp_is_stale_once() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    run(request(d.path()));
    let m = Manifest::at(&d.path().join("out"));
    let text = std::fs::read_to_string(m.path()).unwrap();
    let legacy: String = text
        .lines()
        .map(|l| {
            let mut v: serde_json::Value = serde_json::from_str(l).unwrap();
            assert!(v.as_object_mut().unwrap().remove("output_stamp").is_some());
            format!("{v}\n")
        })
        .collect();
    std::fs::write(m.path(), legacy).unwrap();
    let (s, _) = run(request(d.path()));
    assert_eq!((s.converted, s.up_to_date), (2, 0));
    let (s, _) = run(request(d.path()));
    assert_eq!((s.converted, s.up_to_date), (0, 2));
}

/// Audit 15 (Note Y2): `--sidecar` on a re-run writes the missing sidecar of an up-to-date output from its record.
#[test]
fn sidecar_is_written_for_an_up_to_date_output() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    run(request(d.path()));
    let side = d.path().join("out/armor/iron.dds.json");
    assert!(!side.exists());
    let mut req = request(d.path());
    req.sidecar = true;
    let (s, _) = run(req);
    assert_eq!((s.converted, s.up_to_date), (0, 2));
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&side).unwrap()).unwrap();
    assert_eq!(v["output"], "armor/iron.dds");
}

/// I2: every record carries the batch BC1 decoder model.
#[test]
fn records_carry_the_bc1_model() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let mut req = request(d.path());
    req.overrides
        .push(parse_set("file:**/iron.png.format=bc1").unwrap());
    let (s, ev) = run(req);
    assert_eq!(s.converted, 2, "{ev:#?}");
    let m = Manifest::at(&d.path().join("out")).load().unwrap();
    for key in ["armor/iron.dds", "armor/iron_n.dds"] {
        assert_eq!(m[key].raw["bc1_model"], "ideal", "{key}");
    }
    assert_eq!(m["armor/iron.dds"].raw["profile"]["format"], "bc1");
}

/// Spec §10: an error-severity resolve code fails the file at Resolve (not a skip).
#[test]
fn a_resolve_error_fails_the_file_at_resolve() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let mut req = request(d.path());
    req.overrides
        .push(parse_set("file:**/iron_n.png.encoder.perceptual=true").unwrap());
    let (s, ev) = run(req);
    assert_eq!((s.converted, s.failed, s.skipped), (1, 1, 1), "{ev:#?}");
    assert_eq!(s.exit_code(), 1);
    let e = ev
        .iter()
        .find(|e| matches!(e.kind, EventKind::FileFailed { .. }))
        .unwrap();
    assert!(e.path.as_deref().unwrap().ends_with("iron_n.png"));
    assert!(e.set.is_some());
    match &e.kind {
        EventKind::FileFailed { stage, error } => {
            assert_eq!(*stage, Stage::Resolve);
            assert!(error.contains("FMT-PERCEPTUAL-DATA"), "{error}");
        }
        _ => unreachable!(),
    }
    assert!(!d.path().join("out/armor/iron_n.dds").exists());
}

/// Note CP: `--set encoder.bc7_weights=…` reaches the profile; ENC-WEIGHTS-IGNORED when perceptual wins, on a
/// non-BC7 format, or on another backend; never when the field is unset.
#[test]
fn bc7_weights_set_resolves_and_warns_where_it_has_no_effect() {
    use engine::config::{Bc7Weights, Format};
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let try_plan = |sets: &[&str]| {
        let mut req = request(d.path());
        req.overrides = sets.iter().map(|s| parse_set(s).unwrap()).collect();
        Engine::new().unwrap().plan(
            req,
            &Reporter::new(Arc::new(NullSink), "t"),
            &CancelToken::default(),
        )
    };
    let plan = |sets: &[&str]| try_plan(sets).unwrap();
    let iron = |p: &Plan| {
        p.jobs
            .iter()
            .find(|j| j.input.ends_with("iron.png"))
            .unwrap()
            .clone()
    };
    let ignored = |j: &engine::policy::ResolvedJob| {
        j.warnings.iter().any(|w| w.code == Code::EncWeightsIgnored)
    };
    let base = iron(&plan(&[]));
    assert_eq!(base.profile.format, Format::Bc7);
    assert_eq!(base.profile.encoder.bc7_weights, None);
    assert!(!ignored(&base));
    let set = iron(&plan(&["encoder.bc7_weights=alpha8"]));
    assert_eq!(set.profile.encoder.bc7_weights, Some(Bc7Weights::Alpha8));
    assert!(!ignored(&set), "{:?}", set.warnings);
    for extra in [
        "file:**/iron.png.encoder.perceptual=true",
        "file:**/iron.png.format=bc1",
        "file:**/iron.png.encoder.backend=nvtt",
    ] {
        let j = iron(&plan(&["encoder.bc7_weights=rmaos", extra]));
        assert!(ignored(&j), "{extra}: {:?}", j.warnings);
    }
    // Fix round 1: `linear` overrides an inherited weighting and is never "ignored".
    for extra in ["", "file:**/iron.png.format=bc1"] {
        let sets: Vec<&str> = [
            "encoder.bc7_weights=alpha8",
            "file:**/iron.png.encoder.bc7_weights=linear",
            extra,
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
        let j = iron(&plan(&sets));
        assert_eq!(j.profile.encoder.bc7_weights, Some(Bc7Weights::Linear));
        assert!(!ignored(&j), "{extra}: {:?}", j.warnings);
    }
    // M3: an unknown value is refused, naming the key and the valid values.
    let text = match try_plan(&["encoder.bc7_weights=alpha4"]) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("alpha4 planned"),
    };
    for want in ["alpha4", "encoder.bc7_weights", "linear", "alpha8", "rmaos"] {
        assert!(text.contains(want), "{want}: {text}");
    }
}

/// `--on-existing skip` keeps an existing output untouched and reports it as skipped, with its set (M9).
#[test]
fn on_existing_skip_keeps_existing_outputs() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let out = d.path().join("out/armor");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("iron.dds"), b"keep me").unwrap();
    let mut req = request(d.path());
    req.on_existing = OnExisting::Skip;
    let (s, ev) = run(req);
    assert_eq!((s.converted, s.skipped), (1, 2), "{ev:#?}");
    // iron.dds is kept on purpose; leaf.png is unresolved, which still makes the batch partial.
    assert_eq!(s.skipped_intentional, 1);
    assert_eq!(s.exit_code(), 1);
    assert_eq!(std::fs::read(out.join("iron.dds")).unwrap(), b"keep me");
    let skipped: Vec<_> = ev.iter().filter(|e| kind(e) == "file_skipped").collect();
    assert_eq!(skipped.len(), 2);
    for e in skipped {
        assert!(
            e.set
                .as_deref()
                .is_some_and(|s| s.ends_with("/iron") || s.ends_with("/leaf")),
            "{e:?}"
        );
    }
}

/// M9: every skip event carries the set key of its file.
#[test]
fn skip_events_carry_the_set_key() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let t = d.path().join("textures/armor");
    ImageBuffer::from_fn(4, 4, |_, _| Rgb([1u8, 2, 3]))
        .save(t.join("iron_n.tga"))
        .unwrap();
    std::fs::write(t.join("old.dds"), b"DDS ").unwrap();
    let out = d.path().join("out/armor");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("iron.dds"), b"not ours").unwrap();
    let (_, ev) = run(request(d.path()));
    let skipped: Vec<_> = ev.iter().filter(|e| kind(e) == "file_skipped").collect();
    // iron_n.png + iron_n.tga (IN-DUPLICATE), old.dds, leaf.png (unresolved), iron.png (OUT-UNKNOWN-EXISTING)
    assert_eq!(skipped.len(), 5, "{skipped:#?}");
    for e in &skipped {
        assert!(e.set.is_some(), "{e:?}");
    }
    let unknown: Vec<_> = ev
        .iter()
        .filter(|e| matches!(&e.kind, EventKind::Warning(w) if w.code == Code::OutUnknownExisting))
        .collect();
    assert_eq!(unknown.len(), 1);
    assert!(unknown[0].set.is_some());
}

#[test]
fn existing_output_without_record_is_kept_unless_overwrite() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let out = d.path().join("out/armor");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("iron.dds"), b"not ours").unwrap();
    let (s, ev) = run(request(d.path()));
    assert_eq!((s.converted, s.skipped), (1, 2));
    assert!(ev.iter().any(|e| matches!(
        e.kind,
        EventKind::FileSkipped {
            code: Some(Code::OutUnknownExisting),
            ..
        }
    )));
    assert_eq!(std::fs::read(out.join("iron.dds")).unwrap(), b"not ours");

    let mut req = request(d.path());
    req.on_existing = OnExisting::Overwrite;
    let (s, _) = run(req);
    assert_eq!(s.converted, 2);
    assert_ne!(std::fs::read(out.join("iron.dds")).unwrap(), b"not ours");
}

#[test]
fn changed_source_is_reencoded_and_sidecar_is_written() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let mut req = request(d.path());
    req.sidecar = true;
    run(req.clone());
    assert!(d.path().join("out/armor/iron.dds.json").exists());
    ImageBuffer::from_fn(64, 64, |x, y| Rgb([y as u8 * 4, x as u8 * 4, 9]))
        .save(d.path().join("textures/armor/iron.png"))
        .unwrap();
    let (s, _) = run(req);
    assert_eq!((s.converted, s.up_to_date), (1, 1));
    assert_eq!(Manifest::at(&d.path().join("out")).load().unwrap().len(), 2);
}

/// Spec §6.4 (M2.5): a lone raw roughness map packs into `_rmaos`; a raw map no recipe uses is reported but does
/// not make the batch partial (DECISIONS Note M4).
#[test]
fn raw_maps_pack_and_unused_ones_do_not_count_as_skipped() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    std::fs::remove_file(d.path().join("textures/armor/leaf.png")).unwrap();
    for name in ["stone_roughness.png", "stone_opacity.png"] {
        ImageBuffer::from_fn(64, 64, |x, _| Rgb([x as u8, x as u8, x as u8]))
            .save(d.path().join("textures/armor").join(name))
            .unwrap();
    }
    let (s, ev) = run(request(d.path()));
    assert_eq!((s.converted, s.skipped), (3, 0), "{ev:#?}");
    assert_eq!(s.exit_code(), 0);
    assert!(d.path().join("out/armor/stone_rmaos.dds").exists());
    assert!(ev.iter().any(|e| matches!(
        e.kind,
        EventKind::FileSkipped {
            code: Some(Code::PackUnusedInput),
            ..
        }
    )));
}

/// Carry 2: without `--target`, the project config decides and is recorded as the target's origin.
#[test]
fn target_comes_from_the_project_config_when_not_given() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    std::fs::write(d.path().join("textures/sdc.toml"), "target = \"vanilla\"\n").unwrap();
    let mut req = request(d.path());
    req.target = None;
    let rep = Reporter::new(Arc::new(NullSink), "x");
    let plan = Engine::new()
        .unwrap()
        .plan(req, &rep, &CancelToken::default())
        .unwrap();
    assert_eq!(plan.target, Target::Vanilla);
    assert_eq!(plan.preset_id, "vanilla-quality");
    for j in &plan.jobs {
        assert_eq!(j.profile.target, Target::Vanilla);
        assert_eq!(j.provenance["target"].to_string(), "project_config");
    }

    let plan = Engine::new()
        .unwrap()
        .plan(request(d.path()), &rep, &CancelToken::default())
        .unwrap();
    assert_eq!(plan.target, Target::Cs);
    assert!(plan
        .jobs
        .iter()
        .all(|j| j.provenance["target"].to_string() == "user"));
}

/// Carry 3: an invalid alias preset is a config error (exit 2); an aliased file records its alias preset.
#[test]
fn suffix_alias_preset_is_recorded_in_the_manifest() {
    let d = tempfile::tempdir().unwrap();
    let t = d.path().join("textures/armor");
    std::fs::create_dir_all(&t).unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Rgb([200, x as u8 * 2, 40 + y as u8]))
        .save(t.join("iron_rmaosultrahigh.png"))
        .unwrap();
    let ultra = d.path().join("ultra.toml");
    std::fs::write(
        &ultra,
        "schema_version = 1\nid = \"ultra\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n\
         [suffix_aliases]\nrmaosultrahigh = \"rmaos\"\n[subtypes.rmaos]\nmips = { levels = 1 }\n",
    )
    .unwrap();
    let bad = d.path().join("bad.toml");
    std::fs::write(
        &bad,
        "schema_version = 1\nid = \"bad\"\nversion = \"1.0.0\"\n[suffix_aliases]\nn = \"rmaos\"\n",
    )
    .unwrap();
    let mut req = BatchRequest::new(
        vec![d.path().join("textures")],
        d.path().join("out"),
        Target::Cs,
    );
    req.recursive = true;
    req.accept = AcceptSuggestions::High;
    req.preset_files = vec![bad];
    let rep = Reporter::new(Arc::new(NullSink), "x");
    let err = Engine::new()
        .unwrap()
        .plan(req.clone(), &rep, &CancelToken::default())
        .err()
        .unwrap();
    assert!(matches!(err, EngineError::Config(_)), "{err:?}");
    assert_eq!(err.exit_code(), 2);

    req.preset_files = vec![ultra];
    let (s, ev) = run(req);
    assert_eq!(s.converted, 1, "{ev:#?}");
    assert!(t.exists());
    assert!(d.path().join("out/armor/iron_rmaos.dds").exists());
    let m = Manifest::at(&d.path().join("out")).load().unwrap();
    let e = &m["armor/iron_rmaos.dds"];
    let p = e.preset.as_ref().unwrap();
    assert_eq!(p.id, "ultra");
    assert_eq!(p.alias_chain, ["sdc-base", "cs-quality", "ultra"]);
    assert_eq!(e.raw["provenance"]["mips.levels"], "preset:ultra");
}

/// Cancels the batch when the first file starts: the file in flight stops at a band boundary and nothing is written.
struct CancelOnStart(CancelToken, VecSink);

impl EventSink for CancelOnStart {
    fn emit(&self, ev: &EngineEvent) {
        if matches!(ev.kind, EventKind::FileStarted) {
            self.0.cancel();
        }
        self.1.emit(ev);
    }
}

#[test]
fn cancel_during_a_file_leaves_no_output_and_lists_every_unfinished_file() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let mut req = request(d.path());
    req.jobs = 1;
    let cancel = CancelToken::default();
    let sink = Arc::new(CancelOnStart(cancel.clone(), VecSink::default()));
    let rep = Reporter::new(sink.clone(), "c");
    let e = Engine::new().unwrap();
    let plan = e.plan(req, &rep, &CancelToken::default()).unwrap();
    let s = e.run(plan, &rep, &cancel).unwrap();
    assert_eq!(s.cancelled.len(), 2, "{s:?}");
    assert_eq!(s.converted, 0);
    assert_eq!(s.exit_code(), 1);
    let out = d.path().join("out");
    assert!(files_with_ext(&out, "dds").is_empty());
    assert!(files_with_ext(&out, "tmp").is_empty());
    let ev = sink.1.take();
    match &ev.last().unwrap().kind {
        EventKind::BatchDone { cancelled, .. } => assert_eq!(cancelled.len(), 2),
        k => panic!("{k:?}"),
    }
}

/// Sets an engine test-hook variable for one test and removes it when dropped, panic or not (review m2). Each hook
/// test uses its own variable and a file name no other test has, so tests running in parallel never see it.
struct HookVar(&'static str);

impl HookVar {
    fn set(name: &'static str, value: &str) -> HookVar {
        std::env::set_var(name, value);
        HookVar(name)
    }
}

impl Drop for HookVar {
    fn drop(&mut self) {
        std::env::remove_var(self.0);
    }
}

#[test]
fn a_panicking_file_fails_alone() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    std::fs::copy(
        d.path().join("textures/armor/iron.png"),
        d.path().join("textures/armor/boom_panic_hook.png"),
    )
    .unwrap();
    let _hook = HookVar::set("SDC_TEST_PANIC_ON", "boom_panic_hook.png");
    let mut req = request(d.path());
    req.overrides
        .push(parse_set("file:**/boom_panic_hook.png.subtype=diffuse").unwrap());
    let (s, ev) = run(req);
    assert_eq!((s.converted, s.failed), (2, 1), "{ev:#?}");
    // M10: the stage is the one the file had entered when it panicked (the hook fires before loading).
    assert!(
        ev.iter().any(|e| matches!(
            &e.kind,
            EventKind::FileFailed { error, stage: Stage::Load } if error.contains("test hook")
        )),
        "{ev:#?}"
    );
}

/// Final review I1: a panic while planning (Load, Analyse or Resolve) fails that file alone at the stage it had
/// entered; the other files are still planned and converted, and the batch is partial (exit 1).
#[test]
fn a_file_panicking_while_planning_fails_alone() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    std::fs::copy(
        d.path().join("textures/armor/iron.png"),
        d.path().join("textures/armor/boom_plan_hook.png"),
    )
    .unwrap();
    let _hook = HookVar::set("SDC_TEST_PANIC_ON_PLAN", "boom_plan_hook.png@analyse");
    let mut req = request(d.path());
    req.overrides
        .push(parse_set("file:**/boom_plan_hook.png.subtype=diffuse").unwrap());
    let (s, ev) = run(req);
    assert_eq!((s.converted, s.failed), (2, 1), "{ev:#?}");
    assert_eq!(s.exit_code(), 1);
    let failed: Vec<_> = ev
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::FileFailed { stage, error } => Some((e.path.clone(), *stage, error.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(failed.len(), 1, "{failed:?}");
    let (path, stage, error) = &failed[0];
    assert!(path.as_deref().unwrap().ends_with("boom_plan_hook.png"));
    // The stage is the one the file had entered: Analyse, not the Load it started with.
    assert_eq!(*stage, Stage::Analyse);
    assert!(
        error.contains("internal error") && error.contains("test hook"),
        "{error}"
    );
    assert_eq!(kinds(&ev, "batch_done"), 1);
}

/// Final review I7: decisions made at plan time apply only to the file that was analysed. A source rewritten
/// between `plan` and `run` fails at Load ("source changed since planning"); the other files still convert.
#[test]
fn a_source_changed_after_planning_fails_at_load() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "drift");
    let e = Engine::new().unwrap();
    let plan = e
        .plan(request(d.path()), &rep, &CancelToken::default())
        .unwrap();
    // Re-exported with real alpha and another size, so the byte size changes whatever the timestamp resolution.
    ImageBuffer::from_fn(32, 32, |x, _| Rgba([x as u8 * 8, 0, 0, x as u8 * 8]))
        .save(d.path().join("textures/armor/iron.png"))
        .unwrap();
    let s = e.run(plan, &rep, &CancelToken::default()).unwrap();
    let ev = sink.take();
    assert_eq!((s.converted, s.failed), (1, 1), "{ev:#?}");
    assert_eq!(s.exit_code(), 1);
    assert!(
        ev.iter().any(
            |e| e.path.as_deref().is_some_and(|p| p.ends_with("iron.png"))
                && matches!(
                    &e.kind,
                    EventKind::FileFailed { stage: Stage::Load, error }
                        if error.contains("source changed since planning; re-run")
                )
        ),
        "{ev:#?}"
    );
    assert!(!d.path().join("out/armor/iron.dds").exists());
    assert!(d.path().join("out/armor/iron_n.dds").exists());
}

#[test]
fn nonfinite_source_samples_raise_a_warning() {
    let d = tempfile::tempdir().unwrap();
    let t = d.path().join("textures");
    std::fs::create_dir_all(&t).unwrap();
    let img = ImageBuffer::from_fn(8, 8, |x, y| {
        Rgb([if x == 0 && y == 0 { f32::NAN } else { 0.5 }, 0.25, 0.75])
    });
    image::DynamicImage::ImageRgb32F(img)
        .save(t.join("hot.tif"))
        .unwrap();
    let mut req = BatchRequest::new(vec![t], d.path().join("out"), Target::Cs);
    req.overrides = vec![parse_set("subtype=diffuse").unwrap()];
    let (s, ev) = run(req);
    assert_eq!(s.converted, 1, "{ev:#?}");
    let w = ev
        .iter()
        .find_map(|e| match &e.kind {
            EventKind::Warning(w) if w.code == Code::InNonfinite => Some(w.clone()),
            _ => None,
        })
        .expect("IN-NONFINITE warning");
    assert_eq!(w.data["samples"], 1);
    let n = ev
        .iter()
        .filter(|e| matches!(&e.kind, EventKind::Warning(w) if w.code == Code::InNonfinite))
        .count();
    assert_eq!(n, 1, "reported once, at plan time");
}

/// DECISIONS S9: `--dry-run` (plan only) reports IN-NONFINITE; alpha samples count only when alpha is kept.
#[test]
fn the_plan_reports_nonfinite_samples_before_any_output() {
    let d = tempfile::tempdir().unwrap();
    let t = d.path().join("textures");
    std::fs::create_dir_all(&t).unwrap();
    let img = ImageBuffer::from_fn(8, 8, |x, y| {
        let bad = x == 0 && y == 0;
        Rgba([
            if bad { f32::NAN } else { 0.5 },
            0.25,
            0.75,
            if bad { f32::NAN } else { 1.0 },
        ])
    });
    image::DynamicImage::ImageRgba32F(img)
        .save(t.join("hot.tif"))
        .unwrap();
    for (policy, want) in [("remove", 1u64), ("data", 2)] {
        let mut req = BatchRequest::new(vec![t.clone()], d.path().join("out"), Target::Cs);
        req.overrides = vec![
            parse_set("subtype=generic_data").unwrap(),
            parse_set(&format!("alpha.policy={policy}")).unwrap(),
        ];
        let plan = Engine::new()
            .unwrap()
            .plan(
                req,
                &Reporter::new(Arc::new(NullSink), "t"),
                &CancelToken::default(),
            )
            .unwrap();
        let w = plan.jobs[0]
            .warnings
            .iter()
            .find(|w| w.code == Code::InNonfinite)
            .expect("IN-NONFINITE at plan");
        assert_eq!(w.data["samples"], want, "{policy}");
        assert!(!d.path().join("out/hot.dds").exists());
    }
}

#[test]
fn a_dds_next_to_a_same_stem_image_is_skipped_and_the_image_still_plans() {
    // Spec §2 (DDS input is secondary), DECISIONS Note I: a DDS has no output of its own, so it cannot
    // collide with the image that maps to the same path.
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let t = d.path().join("textures/armor");
    std::fs::write(t.join("iron.dds"), b"DDS ").unwrap();
    std::fs::write(t.join("IRON_N.DDS"), b"DDS ").unwrap();
    let rep = Reporter::new(Arc::new(NullSink), "x");
    let plan = Engine::new()
        .unwrap()
        .plan(request(d.path()), &rep, &CancelToken::default())
        .unwrap();
    let skipped: Vec<(String, Option<Code>)> = plan
        .skipped
        .iter()
        .map(|s| {
            (
                s.path.file_name().unwrap().to_string_lossy().to_lowercase(),
                s.code,
            )
        })
        .collect();
    assert!(
        !skipped.iter().any(|(_, c)| *c == Some(Code::InDuplicate)),
        "{skipped:?}"
    );
    for dds in ["iron.dds", "iron_n.dds"] {
        assert!(
            skipped.contains(&(dds.into(), Some(Code::InDdsSkipped))),
            "{skipped:?}"
        );
    }
    let planned: Vec<String> = plan
        .jobs
        .iter()
        .map(|j| j.input.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert!(
        planned.contains(&"iron.png".to_string()) && planned.contains(&"iron_n.png".to_string()),
        "{planned:?}"
    );
}

/// `request` with leaf.png given a cutout policy, so every file is ready and an exit 1 can only come from a cancel.
fn request_all_ready(dir: &Path) -> BatchRequest {
    let mut r = request(dir);
    r.overrides
        .push(parse_set("file:**/leaf.png.alpha.policy=cutout").unwrap());
    r
}

fn cancelled_names(s: &BatchSummary) -> Vec<String> {
    s.cancelled
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

/// Codex-CLI 4: a Ctrl-C that lands after planning is not reported as a successful dry run.
#[test]
fn a_cancelled_dry_run_lists_its_pending_files_and_exits_1() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let mut req = request_all_ready(d.path());
    req.dry_run = true;
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "c");
    let e = Engine::new().unwrap();
    let plan = e.plan(req, &rep, &CancelToken::default()).unwrap();
    let cancel = CancelToken::default();
    cancel.cancel();
    let s = e.run(plan, &rep, &cancel).unwrap();
    assert_eq!(
        cancelled_names(&s),
        ["iron.png", "iron_n.png", "leaf.png"],
        "{s:?}"
    );
    assert_eq!(s.exit_code(), 1);
    match &sink.take().last().unwrap().kind {
        EventKind::BatchDone { cancelled, .. } => assert_eq!(cancelled.len(), 3),
        k => panic!("{k:?}"),
    }
}

/// Codex-CLI 4: when every ready file would be up to date, a cancel is still recorded (the files were pending
/// when it arrived).
#[test]
fn a_cancel_is_recorded_when_every_ready_file_is_up_to_date() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let (s, _) = run(request_all_ready(d.path()));
    assert_eq!((s.converted, s.exit_code()), (3, 0), "{s:?}");
    let (s, _) = run(request_all_ready(d.path()));
    assert_eq!((s.up_to_date, s.exit_code()), (3, 0), "{s:?}");

    let rep = Reporter::new(Arc::new(NullSink), "c");
    let e = Engine::new().unwrap();
    let plan = e
        .plan(request_all_ready(d.path()), &rep, &CancelToken::default())
        .unwrap();
    let cancel = CancelToken::default();
    cancel.cancel();
    let s = e.run(plan, &rep, &cancel).unwrap();
    assert_eq!(s.up_to_date, 0, "{s:?}");
    assert_eq!(cancelled_names(&s).len(), 3, "{s:?}");
    assert_eq!(s.exit_code(), 1);
}

/// Codex-CLI 4: a batch cancelled before or during planning loads nothing more, and a dry run of it lists every
/// file it did not plan as cancelled (exit 1) instead of reporting success.
#[test]
fn a_dry_run_cancelled_while_planning_lists_the_unplanned_files_and_exits_1() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let mut req = request_all_ready(d.path());
    req.dry_run = true;
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "c");
    let e = Engine::new().unwrap();
    let cancel = CancelToken::default();
    cancel.cancel();
    let plan = e.plan(req, &rep, &cancel).unwrap();
    assert!(plan.jobs.is_empty());
    assert_eq!(plan.cancelled.len(), 3);
    let s = e.run(plan, &rep, &cancel).unwrap();
    assert_eq!(
        cancelled_names(&s),
        ["iron.png", "iron_n.png", "leaf.png"],
        "{s:?}"
    );
    assert_eq!(s.exit_code(), 1);
    let ev = sink.take();
    assert_eq!(kinds(&ev, "file_planned"), 0);
    match &ev.last().unwrap().kind {
        EventKind::BatchDone { cancelled, .. } => assert_eq!(cancelled.len(), 3),
        k => panic!("{k:?}"),
    }
    // A real run of a plan cancelled while planning writes nothing and also exits 1.
    let plan = e.plan(request_all_ready(d.path()), &rep, &cancel).unwrap();
    let s = e.run(plan, &rep, &cancel).unwrap();
    assert_eq!((s.converted, s.cancelled.len(), s.exit_code()), (0, 3, 1));
    assert!(files_with_ext(&d.path().join("out"), "dds").is_empty());
}

/// F1 (Track C): Preprocess step 6 runs in the real convert path (`preprocess_with`), so a 48x40 diffuse with
/// `resize = pot-down` is written as a 32x32 DDS. DIM-NPOT is acknowledged here; Task 5 clears it for resize.
#[test]
fn a_real_convert_with_pot_down_writes_a_power_of_two_dds() {
    let d = tempfile::tempdir().unwrap();
    let t = d.path().join("textures");
    std::fs::create_dir_all(&t).unwrap();
    ImageBuffer::from_fn(48, 40, |x, y| Rgb([x as u8 * 5, y as u8 * 6, 90]))
        .save(t.join("odd.png"))
        .unwrap();
    let mut req = BatchRequest::new(vec![t], d.path().join("out"), Target::Cs);
    req.overrides = vec![
        parse_set("file:**/odd.png.subtype=diffuse").unwrap(),
        parse_set("resize=pot-down").unwrap(),
    ];
    req.allow.insert(Code::DimNpot);
    let (s, ev) = run(req);
    assert_eq!((s.converted, s.failed), (1, 0), "{ev:#?}");
    let f = engine::dds::read_dds(&std::fs::read(d.path().join("out/odd.dds")).unwrap()).unwrap();
    assert_eq!((f.width, f.height, f.mip_count), (32, 32, 6));
}

/// Trace-debug #22 (Note BQ): a dry run reads the manifest without compacting it, so a big stale manifest is left
/// as it is (a converting process on the same root may be appending to it).
#[test]
fn a_dry_run_in_place_does_not_rewrite_a_big_stale_manifest() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    // A DDS input makes the run read the manifest for its IN-DDS-SKIPPED summary.
    std::fs::write(d.path().join("textures/armor/old.dds"), b"DDS ").unwrap();
    let m = Manifest::at(&d.path().join("out"));
    std::fs::create_dir_all(m.path().parent().unwrap()).unwrap();
    let line = b"{\"schema\":1,\"output\":\"a.dds\"}\n";
    let n = engine::manifest::COMPACT_MIN_BYTES as usize / line.len() + 1;
    std::fs::write(m.path(), line.repeat(n)).unwrap();
    let mut req = request(d.path());
    req.dry_run = true;
    run(req);
    assert_eq!(
        std::fs::metadata(m.path()).unwrap().len(),
        (line.len() * n) as u64,
        "the dry run rewrote the manifest"
    );
}

/// Trace-debug #27 (Note BQ): an output re-encoded without `--sidecar` loses its old sidecar, which described the
/// previous source; a later `--sidecar` run then writes a fresh one.
#[test]
fn stale_sidecar_is_not_kept_after_a_run_without_sidecar() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let side = d.path().join("out/armor/iron.dds.json");
    let mut with = request(d.path());
    with.sidecar = true;
    run(with.clone());
    let first = std::fs::read_to_string(&side).unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Rgb([y as u8 * 4, x as u8 * 4, 9]))
        .save(d.path().join("textures/armor/iron.png"))
        .unwrap();
    let (s, _) = run(request(d.path()));
    assert_eq!(s.converted, 1);
    assert!(!side.exists(), "the stale sidecar was kept");
    run(with);
    assert_ne!(std::fs::read_to_string(&side).unwrap(), first);
    // A `.dds.json` sdc did not write stays.
    std::fs::write(&side, "{\"mine\": true}").unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Rgb([x as u8, y as u8, 99]))
        .save(d.path().join("textures/armor/iron.png"))
        .unwrap();
    let (s, _) = run(request(d.path()));
    assert_eq!(s.converted, 1);
    assert_eq!(std::fs::read_to_string(&side).unwrap(), "{\"mine\": true}");
}

/// Review M2 (owner, 2026-10-16; Note BQ10): a stale sidecar sdc wrote but cannot remove (held open without
/// delete sharing here) is a warning, OUT-SIDECAR-KEPT; the new DDS is kept and the file counts as converted.
#[cfg(windows)]
#[test]
fn an_unremovable_stale_sidecar_is_a_warning_not_a_failure() {
    use std::os::windows::fs::OpenOptionsExt;
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let side = d.path().join("out/armor/iron.dds.json");
    let mut with = request(d.path());
    with.sidecar = true;
    run(with);
    // FILE_SHARE_READ only: sdc can read the record, but not delete it while this handle is open.
    let hold = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&side)
        .unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Rgb([y as u8 * 4, x as u8 * 4, 9]))
        .save(d.path().join("textures/armor/iron.png"))
        .unwrap();
    let (s, ev) = run(request(d.path()));
    drop(hold);
    assert_eq!((s.converted, s.failed), (1, 0));
    assert!(d.path().join("out/armor/iron.dds").exists());
    assert!(side.exists());
    assert!(ev
        .iter()
        .any(|e| matches!(&e.kind, EventKind::Warning(w) if w.code == Code::OutSidecarKept)));
}

/// Audit 08d-6 (Note BV3): `--sidecar` never replaces a `.dds.json` sdc did not write; it is kept with
/// OUT-SIDECAR-KEPT and the DDS stays.
#[test]
fn sidecar_keeps_a_foreign_dds_json() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let side = d.path().join("out/armor/iron.dds.json");
    std::fs::create_dir_all(side.parent().unwrap()).unwrap();
    std::fs::write(&side, "{\"mine\": true}").unwrap();
    let mut with = request(d.path());
    with.sidecar = true;
    let (s, ev) = run(with);
    assert_eq!(s.failed, 0, "{ev:?}");
    assert!(d.path().join("out/armor/iron.dds").exists());
    assert_eq!(std::fs::read_to_string(&side).unwrap(), "{\"mine\": true}");
    assert!(ev
        .iter()
        .any(|e| matches!(&e.kind, EventKind::Warning(w) if w.code == Code::OutSidecarKept)));
}

/// Audit 08d-5 (Note BV4): a sidecar that cannot be written (held open without delete sharing here) is a warning,
/// OUT-SIDECAR-KEPT; the new DDS, already in place of the old one, is kept and recorded.
#[cfg(windows)]
#[test]
fn a_sidecar_write_failure_keeps_the_dds() {
    use std::os::windows::fs::OpenOptionsExt;
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let side = d.path().join("out/armor/iron.dds.json");
    let mut with = request(d.path());
    with.sidecar = true;
    run(with.clone());
    let hold = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&side)
        .unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Rgb([y as u8 * 4, x as u8 * 4, 9]))
        .save(d.path().join("textures/armor/iron.png"))
        .unwrap();
    let (s, ev) = run(with);
    drop(hold);
    assert_eq!(s.failed, 0, "{ev:?}");
    assert!(d.path().join("out/armor/iron.dds").exists());
    assert!(ev
        .iter()
        .any(|e| matches!(&e.kind, EventKind::Warning(w) if w.code == Code::OutSidecarKept)));
}

/// Review m10 (Note BV4): after a failed sidecar write leaves the previous output's sidecar, the next
/// `if-changed --sidecar` run rewrites it from the current record.
#[cfg(windows)]
#[test]
fn a_stale_sdc_sidecar_is_rewritten_on_the_next_run() {
    use std::os::windows::fs::OpenOptionsExt;
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let side = d.path().join("out/armor/iron.dds.json");
    let mut with = request(d.path());
    with.sidecar = true;
    run(with.clone());
    let hold = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&side)
        .unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Rgb([y as u8 * 4, x as u8 * 4, 9]))
        .save(d.path().join("textures/armor/iron.png"))
        .unwrap();
    run(with.clone());
    drop(hold);
    let (s, ev) = run(with);
    assert_eq!(s.failed, 0, "{ev:?}");
    let m = Manifest::at(&d.path().join("out")).load().unwrap();
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&side).unwrap()).unwrap();
    assert_eq!(v, m["armor/iron.dds"].raw);
}
