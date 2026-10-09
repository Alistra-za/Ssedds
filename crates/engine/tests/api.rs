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
    let plan = e.plan(req, &rep).unwrap();
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
        Engine::new().unwrap().plan(req, &rep),
        Err(EngineError::Usage(_))
    ));

    let e = Engine::new().unwrap();
    let plan = e.plan(request(d.path()), &rep).unwrap();
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
        .plan(request(d.path()), &rep)
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

/// Same inputs with one worker and with every core: byte-identical outputs.
#[test]
fn output_bytes_do_not_depend_on_the_thread_count() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let mut req = request(d.path());
    req.overrides
        .push(parse_set("file:**/leaf.png.alpha.policy=cutout").unwrap());
    let outs = [1usize, 0].map(|jobs| {
        let mut r = req.clone();
        r.jobs = jobs;
        r.output_root = d.path().join(format!("out{jobs}"));
        let (s, _) = run(r);
        assert_eq!(s.converted, 3);
        files_with_ext(&d.path().join(format!("out{jobs}")), "dds")
    });
    assert_eq!(outs[0].len(), 3);
    for (a, b) in outs[0].iter().zip(&outs[1]) {
        assert_eq!(a.file_name(), b.file_name());
        assert_eq!(
            std::fs::read(a).unwrap(),
            std::fs::read(b).unwrap(),
            "{a:?}"
        );
    }
}

/// A budget smaller than one file admits files one at a time and still finishes.
#[test]
fn tiny_memory_budget_still_converts_everything() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let mut req = request(d.path());
    req.mem_budget = 1;
    req.jobs = 4;
    let (s, _) = run(req);
    assert_eq!((s.converted, s.failed), (2, 0));
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

/// Spec §4.4: a raw packing input is reported as skipped but does not make the batch partial.
#[test]
fn packing_inputs_are_reported_but_do_not_count_as_skipped() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    std::fs::remove_file(d.path().join("textures/armor/leaf.png")).unwrap();
    ImageBuffer::from_fn(64, 64, |x, _| Rgb([x as u8, x as u8, x as u8]))
        .save(d.path().join("textures/armor/stone_roughness.png"))
        .unwrap();
    let (s, ev) = run(request(d.path()));
    assert_eq!((s.converted, s.skipped), (2, 0), "{ev:#?}");
    assert_eq!(s.exit_code(), 0);
    assert!(ev.iter().any(|e| matches!(
        e.kind,
        EventKind::FileSkipped {
            code: Some(Code::InPackingInput),
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
    let plan = Engine::new().unwrap().plan(req, &rep).unwrap();
    assert_eq!(plan.target, Target::Vanilla);
    assert_eq!(plan.preset_id, "vanilla-quality");
    for j in &plan.jobs {
        assert_eq!(j.profile.target, Target::Vanilla);
        assert_eq!(j.provenance["target"].to_string(), "project_config");
    }

    let plan = Engine::new()
        .unwrap()
        .plan(request(d.path()), &rep)
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
        .plan(req.clone(), &rep)
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
    let plan = e.plan(req, &rep).unwrap();
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

#[test]
fn a_panicking_file_fails_alone() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    std::fs::copy(
        d.path().join("textures/armor/iron.png"),
        d.path().join("textures/armor/boom_panic_hook.png"),
    )
    .unwrap();
    // Only this test's file name matches, so other tests in the binary are unaffected.
    std::env::set_var("SDC_TEST_PANIC_ON", "boom_panic_hook.png");
    let mut req = request(d.path());
    req.overrides
        .push(parse_set("file:**/boom_panic_hook.png.subtype=diffuse").unwrap());
    let (s, ev) = run(req);
    assert_eq!((s.converted, s.failed), (2, 1), "{ev:#?}");
    assert!(ev.iter().any(|e| matches!(
        &e.kind,
        EventKind::FileFailed { error, .. } if error.contains("test hook")
    )));
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
}
