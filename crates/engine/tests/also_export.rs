//! Track A Task 7 (DECISIONS Note CM): `also::run_also` writes `<name> PBR`, `<name> Vanilla` and `<name> CM` side by
//! side, as three engine runs. Fixtures are generated: a PBR set `textures/pbr/arm/steel`, a plain set
//! `textures/clutter/cup`, a PBR landscape set `textures/landscape/dirt` and a complex-material set
//! `textures/clutter/plate` (a raw `_height`: cs packs a `_p`, vanilla a complex `_m`, Note AZ2). The byte-identity test
//! with a non-default BC1 model is `also_export_bc1.rs` (the model is process-global).
mod also_support;
use also_support::*;
use engine::also::*;
use engine::api::*;
use engine::config::Target;
use engine::events::*;
use engine::manifest::{sha256_file, Manifest, ManifestRecord};
use engine::overrides::parse_set;
use engine::presets::{AlsoTarget, ExportConfig};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

fn set(v: &[&str]) -> BTreeSet<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn phase(s: &AlsoSummary, p: Phase) -> &BatchSummary {
    &s.phases.iter().find(|(q, _)| *q == p).expect("phase ran").1
}

fn keys_of(ev: &[EngineEvent], f: impl Fn(&EventKind) -> bool) -> Vec<String> {
    ev.iter()
        .filter(|e| f(&e.kind))
        .filter_map(|e| e.job.clone())
        .collect()
}

#[test]
fn a_vanilla_mode_project_is_refused() {
    let (_d, m, loc) = project(cup);
    let mut req = request(&m, &loc);
    req.target = Some(Target::Vanilla);
    match also(req, &spec(&loc, BOTH)).0 {
        Err(EngineError::Usage(msg)) => assert!(msg.contains("cs mode"), "{msg}"),
        other => panic!("{other:?}"),
    }
    for f in ["Mod PBR", "Mod Vanilla", "Mod CM"] {
        assert!(!loc.join(f).exists(), "{f}");
    }
}

#[test]
fn a_location_containing_the_opened_folder_is_allowed() {
    let (_d, m, loc) = project(cup);
    let s = also(request(&m, &loc), &spec(&loc, BOTH)).0.unwrap();
    assert_eq!(
        s.folders,
        [
            loc.join("Mod PBR"),
            loc.join("Mod Vanilla"),
            loc.join("Mod CM")
        ]
    );
    assert!(loc.join("Mod PBR/textures/clutter/cup.dds").exists());
    assert!(loc.join("Mod Vanilla/textures/clutter/cup.dds").exists());

    let mut sp = spec(&loc, BOTH);
    sp.cfg.names.pbr = Some("Mod".into());
    match also(request(&m, &loc), &sp).0 {
        Err(EngineError::Usage(msg)) => assert!(msg.contains("opened folder"), "{msg}"),
        other => panic!("{other:?}"),
    }
}

/// Removes `SDC_TEST_PANIC_ON` when the PBR phase ends (its `batch_done`, the first one), so Vanilla encodes the file.
struct ClearHookOnPbrDone(VecSink);

impl EventSink for ClearHookOnPbrDone {
    fn emit(&self, ev: &EngineEvent) {
        if matches!(ev.kind, EventKind::BatchDone { .. }) {
            std::env::remove_var("SDC_TEST_PANIC_ON");
        }
        self.0.emit(ev);
    }
}

struct HookVar(&'static str);

impl Drop for HookVar {
    fn drop(&mut self) {
        std::env::remove_var(self.0);
    }
}

#[test]
fn a_failed_pbr_encode_is_encoded_again_in_vanilla_not_copied() {
    let (_d, m, loc) = project(|m| {
        cup(m);
        gradient(&m.join("textures/clutter/cup_panic_hook.png"));
    });
    let key = "textures/clutter/cup_panic_hook.dds";
    let (pbr, van) = (loc.join("Mod PBR"), loc.join("Mod Vanilla"));
    // A clean export copies it; then its source changes and the PBR encode fails, leaving a stale PBR file and record.
    also(request(&m, &loc), &spec(&loc, &[AlsoTarget::Vanilla]))
        .0
        .unwrap();
    assert!(record(&van, key).raw.get("copied_from").is_some());
    png(
        &m.join("textures/clutter/cup_panic_hook.png"),
        16,
        16,
        [250, 10, 10, 255],
    );
    std::env::set_var("SDC_TEST_PANIC_ON", "cup_panic_hook.png");
    let _hook = HookVar("SDC_TEST_PANIC_ON");
    let sink = Arc::new(ClearHookOnPbrDone(VecSink::default()));
    let s = also_with(
        request(&m, &loc),
        &spec(&loc, &[AlsoTarget::Vanilla]),
        sink,
        &CancelToken::default(),
    )
    .unwrap();
    assert_eq!(phase(&s, Phase::Pbr).failed, 1, "{s:#?}");
    assert!(pbr.join(key).exists(), "the stale PBR file stays");
    let r = record(&van, key);
    assert!(r.raw.get("copied_from").is_none(), "encoded, not copied");
    assert_ne!(
        std::fs::read(pbr.join(key)).unwrap(),
        std::fs::read(van.join(key)).unwrap()
    );
    assert_eq!(phase(&s, Phase::Vanilla).converted, 1, "{s:#?}");
    assert_eq!(s.copied, 0, "cup and cup_n are up to date");
}

#[test]
fn the_cm_summary_counts_only_derived_items() {
    let (_d, m, loc) = project(|m| {
        steel(m);
        cup(m);
        std::fs::write(m.join("textures/clutter/broken.png"), b"not a png").unwrap();
    });
    let s = also(request(&m, &loc), &spec(&loc, BOTH)).0.unwrap();
    assert_eq!(phase(&s, Phase::Pbr).failed, 1, "{s:#?}");
    assert_eq!(phase(&s, Phase::Vanilla).failed, 1, "{s:#?}");
    assert_eq!(phase(&s, Phase::Cm).failed, 0, "{s:#?}");
    assert_eq!(s.combined().failed, 2);
    assert_eq!(s.exit_code(), 1);
}

#[test]
fn three_folders_side_by_side_with_their_own_manifests() {
    let (_d, m, loc) = project(|m| {
        steel(m);
        cup(m);
    });
    let s = also(request(&m, &loc), &spec(&loc, BOTH)).0.unwrap();
    assert_eq!(s.exit_code(), 0, "{s:#?}");
    for f in ["Mod PBR", "Mod Vanilla", "Mod CM"] {
        assert!(loc.join(f).join("_sdc/manifest.jsonl").exists(), "{f}");
    }
    assert_eq!(
        dds_files(&loc.join("Mod Vanilla")),
        set(&[
            "textures/arm/steel.dds",
            "textures/arm/steel_n.dds",
            "textures/clutter/cup.dds",
            "textures/clutter/cup_n.dds"
        ])
    );
    assert_eq!(
        dds_files(&loc.join("Mod CM")),
        set(&["textures/arm/steel_m.dds"])
    );
}

#[test]
fn plain_sets_are_copied_and_their_bytes_match_the_pbr_folder() {
    let (_d, m, loc) = project(|m| {
        steel(m);
        cup(m);
    });
    let s = also(request(&m, &loc), &spec(&loc, BOTH)).0.unwrap();
    assert_eq!(s.copied, 2, "{s:#?}");
    let (pbr, van) = (loc.join("Mod PBR"), loc.join("Mod Vanilla"));
    for f in ["textures/clutter/cup.dds", "textures/clutter/cup_n.dds"] {
        assert_eq!(
            std::fs::read(pbr.join(f)).unwrap(),
            std::fs::read(van.join(f)).unwrap()
        );
        let (twin, copy) = (record(&pbr, f), record(&van, f));
        assert_eq!(
            copy.raw["copied_from"],
            serde_json::to_value(twin.output_stamp).unwrap()
        );
        assert_eq!(
            copy.output_stamp,
            engine::policy::SourceStamp::of(&van.join(f))
        );
        assert_eq!(copy.source_sha256, twin.source_sha256);
    }
}

#[test]
fn a_set_whose_vanilla_plan_differs_is_encoded_not_copied() {
    let (_d, m, loc) = project(plate);
    let s = also(request(&m, &loc), &spec(&loc, &[AlsoTarget::Vanilla]))
        .0
        .unwrap();
    assert_eq!(s.exit_code(), 0, "{s:#?}");
    let (pbr, van) = (loc.join("Mod PBR"), loc.join("Mod Vanilla"));
    assert!(pbr.join("textures/clutter/plate_p.dds").exists());
    let m_rec = record(&van, "textures/clutter/plate_m.dds");
    assert!(
        m_rec.raw.get("copied_from").is_none(),
        "the complex _m is encoded"
    );
    assert!(record(&van, "textures/clutter/plate.dds")
        .raw
        .get("copied_from")
        .is_some());
    assert!(!van.join("textures/clutter/plate_p.dds").exists());
    assert_eq!(s.copied, 1);
}

#[test]
fn a_second_export_is_all_up_to_date() {
    let (_d, m, loc) = project(|m| {
        steel(m);
        cup(m);
    });
    also(request(&m, &loc), &spec(&loc, BOTH)).0.unwrap();
    let (s, ev) = also(request(&m, &loc), &spec(&loc, BOTH));
    let s = s.unwrap();
    for (p, b) in &s.phases {
        assert_eq!(b.converted, 0, "{p:?}: {b:#?}");
    }
    assert_eq!(s.copied, 0);
    assert_eq!(s.exit_code(), 0);
    let up = keys_of(&ev, |k| {
        matches!(
            k,
            EventKind::FileSkipped {
                kind: SkipKind::UpToDate,
                ..
            }
        )
    });
    assert!(
        up.contains(&"Mod Vanilla/textures/clutter/cup.dds".to_string()),
        "{up:?}"
    );
}

#[test]
fn a_changed_bc1_model_re_copies_the_plain_sets() {
    let (_d, m, loc) = project(cup);
    let mut req = request(&m, &loc);
    req.overrides
        .push(parse_set("file:**/cup.png.format=bc1").unwrap());
    also(req.clone(), &spec(&loc, &[AlsoTarget::Vanilla]))
        .0
        .unwrap();
    // The model is process-global (one per test binary), so the earlier export is made to look like an Nvidia one:
    // its PBR record says so, and this Ideal export must re-encode cup.dds and copy it again.
    let pbr = loc.join("Mod PBR");
    let mut rec: ManifestRecord =
        serde_json::from_value(record(&pbr, "textures/clutter/cup.dds").raw).unwrap();
    rec.bc1_model = Some("nvidia".into());
    Manifest::at(&pbr).append(&rec).unwrap();
    let s = also(req, &spec(&loc, &[AlsoTarget::Vanilla])).0.unwrap();
    assert_eq!(phase(&s, Phase::Pbr).converted, 1, "{s:#?}");
    assert_eq!(s.copied, 1, "{s:#?}");
    let f = "textures/clutter/cup.dds";
    assert_eq!(
        std::fs::read(loc.join("Mod PBR").join(f)).unwrap(),
        std::fs::read(loc.join("Mod Vanilla").join(f)).unwrap()
    );
}

#[test]
fn a_landscape_p_is_copied_into_vanilla_when_identical() {
    let (_d, m, loc) = project(dirt);
    let s = also(request(&m, &loc), &spec(&loc, BOTH)).0.unwrap();
    assert_eq!(s.exit_code(), 0, "{s:#?}");
    let f = "textures/landscape/dirt_p.dds";
    let van = loc.join("Mod Vanilla");
    assert_eq!(
        std::fs::read(loc.join("Mod PBR").join(f)).unwrap(),
        std::fs::read(van.join(f)).unwrap()
    );
    assert!(record(&van, f).raw.get("copied_from").is_some());
    assert!(!loc.join("Mod CM").join(f).exists());
}

#[test]
fn cm_alone_also_writes_vanilla_and_says_why() {
    let (_d, m, loc) = project(|m| {
        steel(m);
        cup(m);
    });
    let s = also(request(&m, &loc), &spec(&loc, &[AlsoTarget::Cm]))
        .0
        .unwrap();
    assert!(loc.join("Mod Vanilla/textures/arm/steel.dds").exists());
    assert!(loc.join("Mod CM/textures/arm/steel_m.dds").exists());
    assert!(
        s.notes
            .iter()
            .any(|n| n.contains("CM installs on top of Vanilla")),
        "{:?}",
        s.notes
    );
}

#[test]
fn a_project_without_pbr_sets_writes_no_cm_folder() {
    let (_d, m, loc) = project(cup);
    let s = also(request(&m, &loc), &spec(&loc, BOTH)).0.unwrap();
    assert!(!loc.join("Mod CM").exists());
    assert!(s.phases.iter().all(|(p, _)| *p != Phase::Cm));
    assert!(
        s.notes
            .contains(&"no PBR sets: Vanilla holds copies only; no CM folder written".to_string()),
        "{:?}",
        s.notes
    );
    let r = std::fs::read_to_string(loc.join("Mod conversion-report.md")).unwrap();
    assert!(!r.contains("Install the CM folder"), "{r}");
}

#[test]
fn vanilla_alone_writes_two_folders() {
    let (_d, m, loc) = project(|m| {
        steel(m);
        cup(m);
    });
    let s = also(request(&m, &loc), &spec(&loc, &[AlsoTarget::Vanilla]))
        .0
        .unwrap();
    assert!(loc.join("Mod PBR").exists() && loc.join("Mod Vanilla").exists());
    assert!(!loc.join("Mod CM").exists());
    assert_eq!(s.phases.len(), 2);
}

#[test]
fn the_owned_file_record_is_in_the_pbr_folder() {
    let (_d, m, loc) = project(cup);
    also(request(&m, &loc), &spec(&loc, &[AlsoTarget::Vanilla]))
        .0
        .unwrap();
    let pbr = loc.join("Mod PBR");
    assert!(pbr.join("_sdc/also.json").exists());
    assert!(!loc.join("_sdc").exists());

    // A crash between the rename and the save leaves the new SHA pending: the file is still Tessalith's.
    let sums = loc.join("Mod SHA256SUMS.txt");
    std::fs::write(&sums, b"old").unwrap();
    let key = std::path::absolute(&sums)
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/")
        .to_lowercase();
    let pending = sha256_file(&sums).unwrap();
    std::fs::write(
        pbr.join("_sdc/also.json"),
        serde_json::to_vec(&serde_json::json!({ key.clone(): { "pending": pending } })).unwrap(),
    )
    .unwrap();
    let mut owned = OwnedFiles::load(&pbr).unwrap();
    owned.write(&sums, b"new").unwrap();
    assert_eq!(std::fs::read(&sums).unwrap(), b"new");
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(pbr.join("_sdc/also.json")).unwrap()).unwrap();
    assert_eq!(saved[&key]["sha256"], sha256_file(&sums).unwrap());
    assert!(saved[&key].get("pending").is_none());

    // A file nobody recorded is kept.
    let foreign = loc.join("Mod conversion-report.md");
    std::fs::write(&foreign, b"mine").unwrap();
    assert!(matches!(
        OwnedFiles::load(&pbr).unwrap().write(&foreign, b"ours"),
        Err(OwnedWriteError::Kept(engine::dds::Kept::Foreign))
    ));
    assert_eq!(std::fs::read(&foreign).unwrap(), b"mine");
}

/// Review M5: a write that fails says so ("could not be written"), not "could not be read".
#[test]
fn an_owned_write_that_fails_says_could_not_be_written() {
    let d = tempfile::tempdir().unwrap();
    let pbr = d.path().join("Mod PBR");
    std::fs::write(d.path().join("blocker"), b"a file, not a folder").unwrap();
    let target = d.path().join("blocker/sums.txt");
    let e = OwnedFiles::load(&pbr)
        .unwrap()
        .write(&target, b"x")
        .unwrap_err();
    assert!(matches!(e, OwnedWriteError::Io(_)), "{e:?}");
    let note = e.note(&target);
    assert!(note.contains("could not be written"), "{note}");
}

/// Review M4: a corrupt `also.json` is left as it is (never saved back as `{}`), with a warning and a note.
#[test]
fn a_corrupt_owned_record_is_kept_and_reported() {
    let (_d, m, loc) = project(cup);
    let rec = loc.join("Mod PBR/_sdc/also.json");
    std::fs::create_dir_all(rec.parent().unwrap()).unwrap();
    std::fs::write(&rec, b"{\"c:/x/mod sha256sums.txt\": {\"sha256\": \"ab").unwrap();
    assert!(OwnedFiles::load(&loc.join("Mod PBR")).is_err());
    let (s, ev) = also(request(&m, &loc), &spec(&loc, &[AlsoTarget::Vanilla]));
    let s = s.unwrap();
    assert_eq!(
        std::fs::read(&rec).unwrap(),
        b"{\"c:/x/mod sha256sums.txt\": {\"sha256\": \"ab"
    );
    assert!(
        s.notes.iter().any(|n| n.contains("also.json")),
        "{:?}",
        s.notes
    );
    assert!(ev.iter().any(|e| matches!(&e.kind,
        EventKind::Warning(w) if w.code == engine::codes::Code::AlsoOwnedUnreadable)));
}

/// Review M1: an error of a later phase keeps its class: recorded with its phase, its exit code wins, it is not a
/// cancel, and a warning event names the phase.
#[test]
fn a_later_phase_error_keeps_its_exit_code_and_is_reported() {
    let (_d, m, loc) = project(cup);
    // The Vanilla folder's name is taken by a file: its run cannot create the output root (Fatal, exit 3).
    std::fs::create_dir_all(&loc).unwrap();
    std::fs::write(loc.join("Mod Vanilla"), b"a file").unwrap();
    let (s, ev) = also(request(&m, &loc), &spec(&loc, BOTH));
    let s = s.unwrap();
    assert_eq!(s.cancelled_after, None);
    let (phase, _, code) = s.error.clone().expect("the error is kept");
    assert_eq!((phase, code), (Phase::Vanilla, 3));
    assert_eq!(s.exit_code(), 3);
    assert!(ev.iter().any(|e| matches!(&e.kind,
        EventKind::Warning(w) if w.code == engine::codes::Code::AlsoPhaseFailed && w.message.contains("Vanilla"))));
}

/// Review M2: the copies' events come before the Vanilla phase's `batch_done`.
#[test]
fn copy_events_come_before_the_vanilla_batch_done() {
    let (_d, m, loc) = project(|m| {
        steel(m);
        cup(m);
    });
    let (s, ev) = also(request(&m, &loc), &spec(&loc, &[AlsoTarget::Vanilla]));
    assert_eq!(s.unwrap().copied, 2);
    let copy = ev
        .iter()
        .position(|e| {
            e.job.as_deref() == Some("Mod Vanilla/textures/clutter/cup.dds")
                && matches!(e.kind, EventKind::FileDone { .. })
        })
        .unwrap();
    let done = ev
        .iter()
        .position(|e| e.batch == ev[copy].batch && matches!(e.kind, EventKind::BatchDone { .. }))
        .unwrap();
    assert!(copy < done, "copy {copy}, batch_done {done}");
}

/// Review M7: a batch-level planning warning (here an unmatched `[files]` key) appears once, not once per phase.
#[test]
fn batch_level_warnings_appear_once() {
    let (_d, m, loc) = project(|m| {
        steel(m);
        cup(m);
    });
    std::fs::write(
        m.join("sdc.toml"),
        "[files.\"textures/nothing.png\"]\nalpha.threshold = 90\n",
    )
    .unwrap();
    let (s, ev) = also(request(&m, &loc), &spec(&loc, BOTH));
    assert_eq!(s.unwrap().phases.len(), 3);
    let n = ev
        .iter()
        .filter(|e| {
            matches!(&e.kind,
            EventKind::Warning(w) if w.code == engine::codes::Code::CfgFileUnmatched)
        })
        .count();
    assert_eq!(n, 1);
}

/// Review M3: `same_identity` holds for a cs job and its vanilla clone, and fails when any compared field differs.
#[test]
fn same_identity_fails_when_any_compared_field_differs() {
    use engine::config::{Format, Target};
    use engine::policy::{JobStatus, ResolvedJob};
    let (_d, m, loc) = project(|m| {
        steel(m);
        cup(m);
        plate(m);
    });
    let (cs_root, van_root) = (loc.join("Mod PBR"), loc.join("Mod Vanilla"));
    let e = Engine::new().unwrap();
    let rep = Reporter::new(Arc::new(NullSink), "t");
    let cs = e
        .plan(request(&m, &cs_root), &rep, &CancelToken::default())
        .unwrap();
    let job = |p: &engine::api::Plan, tail: &str| -> ResolvedJob {
        p.jobs
            .iter()
            .find(|j| {
                j.output
                    .to_string_lossy()
                    .replace('\\', "/")
                    .ends_with(tail)
            })
            .unwrap_or_else(|| panic!("{tail}"))
            .clone()
    };
    let cup = job(&cs, "textures/clutter/cup.dds");
    let packed = job(&cs, "textures/clutter/plate_p.dds");
    let mut vreq = request(&m, &van_root);
    vreq.target = Some(Target::Vanilla);
    vreq.derive = Some(engine::derive::DeriveRequest {
        mode: engine::derive::DeriveMode::Vanilla,
        params: Default::default(),
        sets: derive_sets(&cs, &BTreeSet::new()).0,
        drop: vec![],
        scanned: false,
    });
    let vp = e.plan(vreq, &rep, &CancelToken::default()).unwrap();
    let derived = job(&vp, "textures/arm/steel.dds");
    assert!(derived.derive.is_some());

    let mut van = cup.clone();
    van.profile.target = Target::Vanilla;
    van.output = van_root.join("textures/clutter/cup.dds");
    assert!(same_identity(&cup, &cs_root, &van, &van_root));
    type Flip = Box<dyn Fn(&mut ResolvedJob)>;
    let flips: Vec<(&str, Flip)> = vec![
        ("format", Box::new(|j| j.profile.format = Format::Bc1)),
        (
            "input",
            Box::new(|j| j.input = j.input.with_file_name("other.png")),
        ),
        (
            "output key",
            Box::new(|j| j.output = j.output.with_file_name("other.dds")),
        ),
        (
            "status",
            Box::new(|j| j.status = JobStatus::Blocked { codes: vec![] }),
        ),
        (
            "rnm",
            Box::new(|j| {
                j.rnm = Some(engine::rnm::RnmJob {
                    layers: vec![],
                    ao: None,
                })
            }),
        ),
        (
            "bake",
            Box::new(|j| {
                j.bake = Some(engine::bake::BakeJob {
                    normal: j.input.clone(),
                    normal_profile: j.profile.clone(),
                    shift: 0,
                    stamp: None,
                    rnm: None,
                })
            }),
        ),
    ];
    for (what, flip) in &flips {
        let mut v = van.clone();
        flip(&mut v);
        assert!(!same_identity(&cup, &cs_root, &v, &van_root), "{what}");
    }
    let mut v = van.clone();
    v.pack = packed.pack.clone();
    assert!(!same_identity(&cup, &cs_root, &v, &van_root), "pack");
    let mut v = van.clone();
    v.derive = derived.derive.clone();
    assert!(!same_identity(&cup, &cs_root, &v, &van_root), "derive");
    let mut c = cup.clone();
    c.status = JobStatus::Blocked { codes: vec![] };
    assert!(!same_identity(&c, &cs_root, &van, &van_root), "cs status");
}

#[test]
fn an_unknown_file_in_the_vanilla_folder_is_kept() {
    let (_d, m, loc) = project(cup);
    let mine = loc.join("Mod Vanilla/textures/clutter/cup.dds");
    std::fs::create_dir_all(mine.parent().unwrap()).unwrap();
    std::fs::write(&mine, b"mine").unwrap();
    let (s, ev) = also(request(&m, &loc), &spec(&loc, &[AlsoTarget::Vanilla]));
    let s = s.unwrap();
    assert_eq!(std::fs::read(&mine).unwrap(), b"mine");
    assert_eq!(s.copied, 1, "cup_n only");
    let key = "Mod Vanilla/textures/clutter/cup.dds";
    assert!(ev.iter().any(|e| e.job.as_deref() == Some(key)
        && matches!(&e.kind, EventKind::Warning(w) if w.code == engine::codes::Code::OutUnknownExisting)));
    assert!(keys_of(&ev, |k| matches!(
        k,
        EventKind::FileSkipped {
            kind: SkipKind::UnknownExisting,
            ..
        }
    ))
    .contains(&key.to_string()));
    assert_eq!(s.exit_code(), 1);
}

#[test]
fn the_opened_folder_is_refused_as_a_location_and_a_location_inside_it_is_not_discovered() {
    let (_d, m, _loc) = project(cup);
    match also(request(&m, &m), &spec(&m, BOTH)).0 {
        Err(EngineError::Usage(msg)) => assert!(msg.contains("opened folder"), "{msg}"),
        other => panic!("{other:?}"),
    }
    assert!(!m.join("Mod PBR").exists());

    let inside = m.join("out");
    also(request(&m, &inside), &spec(&inside, BOTH)).0.unwrap();
    assert!(inside.join("Mod Vanilla/textures/clutter/cup.dds").exists());
    let (s, ev) = also(request(&m, &inside), &spec(&inside, BOTH));
    assert_eq!(s.unwrap().exit_code(), 0);
    let dds_skip = ev.iter().any(|e| match &e.kind {
        EventKind::Warning(w) => w.code == engine::codes::Code::InDdsSkipped,
        EventKind::FileSkipped { code, .. } => *code == Some(engine::codes::Code::InDdsSkipped),
        _ => false,
    });
    assert!(!dds_skip);
    assert!(ev
        .iter()
        .all(|e| !e.path.as_deref().is_some_and(|p| p.contains("/out/"))));
}

#[test]
fn a_skipped_set_is_copied_nowhere_and_noted() {
    let (_d, m, loc) = project(|m| {
        steel(m);
        cup(m);
    });
    let mut sp = spec(&loc, BOTH);
    sp.cfg.skip = vec!["textures/pbr/arm/steel".into()];
    let s = also(request(&m, &loc), &sp).0.unwrap();
    assert!(loc.join("Mod PBR/textures/pbr/arm/steel.dds").exists());
    for f in ["Mod Vanilla", "Mod CM"] {
        let files = dds_files(&loc.join(f));
        assert!(files.iter().all(|f| !f.contains("steel")), "{f}: {files:?}");
    }
    assert_eq!(
        s.sets,
        vec![SetNote {
            id: "textures/pbr/arm/steel".into(),
            why: "skipped (Skip vanilla/CM for this set)".into()
        }]
    );
}

#[test]
fn vanilla_and_cm_job_keys_carry_their_folder_prefix() {
    let (_d, m, loc) = project(|m| {
        steel(m);
        cup(m);
    });
    let (s, ev) = also(request(&m, &loc), &spec(&loc, BOTH));
    s.unwrap();
    let done = keys_of(&ev, |k| matches!(k, EventKind::FileDone { .. }));
    for k in [
        "textures/clutter/cup.dds",
        "Mod Vanilla/textures/clutter/cup.dds",
        "Mod Vanilla/textures/arm/steel.dds",
        "Mod CM/textures/arm/steel_m.dds",
    ] {
        assert!(done.contains(&k.to_string()), "{k}: {done:?}");
    }
    // The CM phase keeps only the derived items: no event of the plain set.
    assert!(ev
        .iter()
        .filter_map(|e| e.job.as_deref())
        .all(|k| !k.starts_with("Mod CM/textures/clutter")));
}

#[test]
fn a_cs_only_set_override_fails_before_anything_is_written() {
    let (_d, m, loc) = project(cup);
    let mut req = request(&m, &loc);
    req.overrides.push(parse_set("format=bc6h_uf16").unwrap());
    let e = also(req, &spec(&loc, BOTH)).0.unwrap_err();
    assert_eq!(e.exit_code(), 2, "{e}");
    for f in ["Mod PBR", "Mod Vanilla", "Mod CM"] {
        assert!(!loc.join(f).exists(), "{f}");
    }
}

/// Cancels on the PBR phase's `batch_done`.
struct CancelOnPbrDone(CancelToken);

impl EventSink for CancelOnPbrDone {
    fn emit(&self, ev: &EngineEvent) {
        if matches!(ev.kind, EventKind::BatchDone { .. }) {
            self.0.cancel();
        }
    }
}

#[test]
fn cancel_after_the_pbr_phase_stops_the_rest() {
    let (_d, m, loc) = project(|m| {
        steel(m);
        cup(m);
    });
    let cancel = CancelToken::default();
    let s = also_with(
        request(&m, &loc),
        &spec(&loc, BOTH),
        Arc::new(CancelOnPbrDone(cancel.clone())),
        &cancel,
    )
    .unwrap();
    assert_eq!(s.cancelled_after, Some(Phase::Pbr));
    assert_eq!(s.phases.len(), 1);
    assert!(!loc.join("Mod Vanilla").exists());
    assert_eq!(s.exit_code(), 1);
}

// ---- the folder-name rule (Task 6 review: pairwise with the real project name, Windows names) ----

#[test]
fn a_custom_name_equal_to_another_folders_default_is_refused() {
    let (_d, m, loc) = project(cup);
    let mut sp = spec(&loc, BOTH);
    sp.cfg.names.cm = Some("mod vanilla".into());
    match also(request(&m, &loc), &sp).0 {
        Err(EngineError::Config(msg)) => assert!(msg.contains("export.names.cm"), "{msg}"),
        other => panic!("{other:?}"),
    }
    assert!(!loc.join("Mod PBR").exists());
}

#[test]
fn windows_illegal_and_reserved_folder_names_are_refused_by_key() {
    for (key, bad) in [
        ("pbr", "a:b"),
        ("vanilla", "what?"),
        ("cm", "x|y"),
        ("pbr", "CON"),
        ("vanilla", "nul.txt"),
        ("cm", "Com1"),
        ("cm", "LPT9"),
    ] {
        let mut c = ExportConfig::default();
        match key {
            "pbr" => c.names.pbr = Some(bad.into()),
            "vanilla" => c.names.vanilla = Some(bad.into()),
            _ => c.names.cm = Some(bad.into()),
        }
        let e = c.check().unwrap_err();
        assert!(e.contains(&format!("export.names.{key}")), "{bad}: {e}");
    }
    let mut c = ExportConfig::default();
    c.names.cm = Some("CONSOLE".into());
    c.check().unwrap();
}

#[test]
fn check_location_rules() {
    let d = tempfile::tempdir().unwrap();
    let m = d.path().join("mods/Mod");
    let loc = d.path().join("mods");
    let folders = |l: &Path| [l.join("Mod PBR"), l.join("Mod Vanilla"), l.join("Mod CM")];
    check_location(std::slice::from_ref(&m), &loc, &folders(&loc)).unwrap();
    assert!(check_location(std::slice::from_ref(&m), &m, &folders(&m)).is_err());
    let mut f = folders(&loc);
    f[1] = d.path().join("mods");
    assert!(check_location(std::slice::from_ref(&m), &loc, &f).is_err());
    let inside = m.join("out");
    check_location(std::slice::from_ref(&m), &inside, &folders(&inside)).unwrap();
    assert!(export_folder_problem(&m, &loc).is_some());
    assert!(export_folder_problem(&m, &m.join("x")).is_none());
}

#[test]
fn prefix_sink_prefixes_job_keys_only() {
    let inner = Arc::new(VecSink::default());
    let rep = Reporter::new(
        Arc::new(PrefixSink::new("Mod CM/".into(), inner.clone())),
        "b",
    );
    rep.emit_job(Some("textures/a.dds"), None, None, EventKind::FileStarted);
    rep.emit(None, None, EventKind::FileStarted);
    let ev = inner.take();
    assert_eq!(ev[0].job.as_deref(), Some("Mod CM/textures/a.dds"));
    assert_eq!(ev[1].job, None);
}

#[test]
fn derive_sets_skip_by_id() {
    let (_d, m, loc) = project(|m| {
        steel(m);
        cup(m);
    });
    let r = request(&m, &loc.join("Mod PBR"));
    let p = Engine::new()
        .unwrap()
        .plan(
            r,
            &Reporter::new(Arc::new(NullSink), "t"),
            &CancelToken::default(),
        )
        .unwrap();
    let (sets, drop, notes) = derive_sets(&p, &BTreeSet::new());
    assert_eq!(sets.len(), 1);
    assert_eq!(sets[0].id, "textures/pbr/arm/steel");
    assert!(drop.is_empty() && notes.is_empty());
    let (sets, drop, notes) = derive_sets(&p, &set(&["Textures/PBR/Arm/Steel"]));
    assert!(sets.is_empty());
    assert_eq!(drop.len(), 3);
    assert_eq!(notes.len(), 1);
}

// ---- Task 8: zips and SHA-256 lines (C8, TA17, TA21, TA23) ----

fn zip_spec(loc: &Path, also: &[AlsoTarget]) -> AlsoSpec {
    let mut sp = spec(loc, also);
    sp.cfg.zip = true;
    sp
}

/// The entry names of a zip, from its central directory.
fn zip_names(path: &Path) -> Vec<String> {
    let z = std::fs::read(path).unwrap();
    let u16le = |at: usize| u16::from_le_bytes([z[at], z[at + 1]]) as usize;
    let u32le = |at: usize| u32::from_le_bytes(z[at..at + 4].try_into().unwrap()) as usize;
    let eocd = z.len() - 22;
    let mut at = u32le(eocd + 16);
    (0..u16le(eocd + 10))
        .map(|_| {
            let (n, x, c) = (u16le(at + 28), u16le(at + 30), u16le(at + 32));
            let name = String::from_utf8(z[at + 46..at + 46 + n].to_vec()).unwrap();
            at += 46 + n + x + c;
            name
        })
        .collect()
}

fn sums(loc: &Path) -> Vec<String> {
    std::fs::read_to_string(loc.join("Mod SHA256SUMS.txt"))
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect()
}

fn zips_in(loc: &Path) -> Vec<String> {
    std::fs::read_dir(loc)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".zip"))
        .collect()
}

#[test]
fn zip_on_writes_a_zip_per_folder_and_sha256sums() {
    let (_d, m, loc) = project(|m| {
        steel(m);
        cup(m);
    });
    let s = also(request(&m, &loc), &zip_spec(&loc, BOTH)).0.unwrap();
    let names = ["Mod PBR.zip", "Mod Vanilla.zip", "Mod CM.zip"];
    assert_eq!(s.zips, names.map(|n| loc.join(n)).to_vec(), "{:?}", s.notes);
    let want: Vec<String> = names
        .iter()
        .map(|n| format!("{}  {n}", sha256_file(&loc.join(n)).unwrap()))
        .collect();
    assert_eq!(sums(&loc), want);
    let raw = std::fs::read(loc.join("Mod SHA256SUMS.txt")).unwrap();
    assert!(raw.is_ascii() && !raw.contains(&b'\r'));
    assert!(zip_names(&loc.join("Mod CM.zip")).contains(&"Mod CM/textures/arm/steel_m.dds".into()));
    assert!(zip_names(&loc.join("Mod PBR.zip"))
        .iter()
        .all(|n| !n.contains("_sdc") && n.starts_with("Mod PBR/")));

    // A second, all up-to-date export zips the same files to the same bytes, over its own zips.
    let s2 = also(request(&m, &loc), &zip_spec(&loc, BOTH)).0.unwrap();
    assert_eq!(s2.zips.len(), 3, "{:?}", s2.notes);
    assert_eq!(sums(&loc), want);
}

#[test]
fn cancel_after_the_pbr_phase_writes_no_zip() {
    let (_d, m, loc) = project(|m| {
        steel(m);
        cup(m);
    });
    let cancel = CancelToken::default();
    let s = also_with(
        request(&m, &loc),
        &zip_spec(&loc, BOTH),
        Arc::new(CancelOnPbrDone(cancel.clone())),
        &cancel,
    )
    .unwrap();
    assert!(zips_in(&loc).is_empty());
    assert!(s.zips.is_empty());
    assert!(!loc.join("Mod SHA256SUMS.txt").exists());
    assert!(
        s.notes
            .contains(&"zip skipped: the export did not finish".to_string()),
        "{:?}",
        s.notes
    );
}

/// Final review I2: a file written into the PBR folder between the phases and the zips (the GUI's JSON) and passed
/// as `extra` is zipped, with no "not zipped" note; one not passed is noted. A path outside the PBR folder is ignored.
#[test]
fn extra_files_written_after_the_phases_are_zipped() {
    let (_d, m, loc) = project(steel);
    let sp = zip_spec(&loc, &[AlsoTarget::Vanilla]);
    let start = || {
        start_also(
            &Engine::new().unwrap(),
            &engine::session::AnalysisCache::default(),
            request(&m, &loc),
            &sp,
            Arc::new(VecSink::default()),
            &CancelToken::default(),
        )
        .unwrap()
    };
    let json = loc.join("Mod PBR/PBRNIFPatcher/Mod.json");
    let note = "Mod PBR/PBRNIFPatcher/Mod.json: not zipped: not written by this export";
    let run = start();
    std::fs::create_dir_all(json.parent().unwrap()).unwrap();
    std::fs::write(&json, b"{}").unwrap();
    let outside = loc.join("elsewhere.json");
    let s = finish_also(run, &sp, &[json.clone(), outside], &CancelToken::default());
    assert!(zip_names(&loc.join("Mod PBR.zip")).contains(&"Mod PBR/PBRNIFPatcher/Mod.json".into()));
    assert!(!s.notes.iter().any(|n| n == note), "{:?}", s.notes);
    assert_eq!(s.exit_code(), 0, "{:?}", s.notes);
    let s = finish_also(start(), &sp, &[], &CancelToken::default());
    assert!(s.notes.iter().any(|n| n == note), "{:?}", s.notes);
    assert!(!zip_names(&loc.join("Mod PBR.zip")).contains(&"Mod PBR/PBRNIFPatcher/Mod.json".into()));
}

/// Final review M1: a cancel after the phases (while the GUI writes its JSON, or zips) leaves the earlier zip as it
/// was, and no temporary file.
#[test]
fn a_cancel_before_the_zips_keeps_the_old_zip() {
    let (_d, m, loc) = project(steel);
    let sp = zip_spec(&loc, &[AlsoTarget::Vanilla]);
    also(request(&m, &loc), &sp).0.unwrap();
    let old = std::fs::read(loc.join("Mod PBR.zip")).unwrap();
    std::fs::write(m.join("textures/pbr/arm/steel.png"), b"").unwrap();
    png(
        &m.join("textures/pbr/arm/steel.png"),
        16,
        16,
        [10, 20, 30, 255],
    );
    let cancel = CancelToken::default();
    let run = start_also(
        &Engine::new().unwrap(),
        &engine::session::AnalysisCache::default(),
        request(&m, &loc),
        &sp,
        Arc::new(VecSink::default()),
        &cancel,
    )
    .unwrap();
    cancel.cancel();
    let s = finish_also(run, &sp, &[], &cancel);
    assert!(s.notes.contains(&ZIP_SKIPPED.to_string()), "{:?}", s.notes);
    assert_eq!(std::fs::read(loc.join("Mod PBR.zip")).unwrap(), old);
    assert!(zips_in(&loc).len() == 2);
    assert!(std::fs::read_dir(&loc).unwrap().all(|e| !e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .ends_with(".tmp")));
    assert_eq!(s.exit_code(), 1);
}

#[test]
fn a_zip_tessalith_did_not_write_is_kept() {
    let (_d, m, loc) = project(|m| {
        steel(m);
        cup(m);
    });
    std::fs::write(loc.join("Mod PBR.zip"), b"mine").unwrap();
    let (s, ev) = also(request(&m, &loc), &zip_spec(&loc, BOTH));
    let s = s.unwrap();
    assert_eq!(std::fs::read(loc.join("Mod PBR.zip")).unwrap(), b"mine");
    // Review M1: a zip not written is a partial export, with a warning.
    assert_eq!(s.exit_code(), 1, "{s:#?}");
    assert!(ev.iter().any(|e| matches!(&e.kind,
        EventKind::Warning(w) if w.code == engine::codes::Code::AlsoZipFailed && w.message.contains("Mod PBR.zip"))));
    assert!(
        s.notes.iter().any(|n| n.contains("Mod PBR.zip")),
        "{:?}",
        s.notes
    );
    assert_eq!(s.zips.len(), 2);
    let lines = sums(&loc);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines.iter().all(|l| !l.ends_with("Mod PBR.zip")));
}

#[test]
fn the_zip_holds_only_this_runs_outputs() {
    let (_d, m, loc) = project(|m| {
        steel(m);
        cup(m);
    });
    let mut sp = spec(&loc, BOTH);
    sp.cfg.legacy.keep_metal_brightness = true;
    also(request(&m, &loc), &sp).0.unwrap();
    let stale = "Mod CM/textures/arm/steel.dds";
    assert!(loc.join(stale).exists(), "the keep-metal CM diffuse");
    let unknown = "Mod Vanilla/textures/x.dds";
    std::fs::write(loc.join(unknown), b"mine").unwrap();

    let s = also(request(&m, &loc), &zip_spec(&loc, BOTH)).0.unwrap();
    assert_eq!(std::fs::read(loc.join(unknown)).unwrap(), b"mine");
    let van = zip_names(&loc.join("Mod Vanilla.zip"));
    let cm = zip_names(&loc.join("Mod CM.zip"));
    assert!(!van.contains(&unknown.to_string()), "{van:?}");
    assert!(van.contains(&"Mod Vanilla/textures/clutter/cup.dds".to_string()));
    assert_eq!(cm, ["Mod CM/textures/arm/steel_m.dds"]);
    for f in [unknown, stale] {
        let want = format!("{f}: not zipped: not written by this export");
        assert!(s.notes.contains(&want), "{want}: {:?}", s.notes);
    }
}

/// Final review I3: with zip off, a file an earlier export left (the CM diffuse of Keep metal brightness, now off) is
/// kept and reported, and the install text says the CM folder holds such files.
#[test]
fn an_earlier_exports_cm_diffuse_is_reported_with_zip_off() {
    let (_d, m, loc) = project(steel);
    let mut sp = spec(&loc, BOTH);
    sp.cfg.legacy.keep_metal_brightness = true;
    also(request(&m, &loc), &sp).0.unwrap();
    let stale = "Mod CM/textures/arm/steel.dds";
    assert!(loc.join(stale).exists(), "the keep-metal CM diffuse");
    sp.cfg.legacy.keep_metal_brightness = false;
    let s = also(request(&m, &loc), &sp).0.unwrap();
    assert!(loc.join(stale).exists(), "never deleted");
    let want = format!("{stale}: {NOT_WRITTEN}");
    assert!(s.notes.contains(&want), "{want}: {:?}", s.notes);
    assert!(
        !s.notes.iter().any(|n| n.contains("steel_m.dds")),
        "{:?}",
        s.notes
    );
    let r = report(&loc, "Mod");
    assert!(r.contains(&want), "{r}");
    assert!(r.contains(cm_lines(&sp.cfg, false, true)[2]), "{r}");
}

/// Task 13 M3 / final review I3: the CM text names the complex-grass stacks when a PBR grass set is exported.
#[test]
fn the_cm_text_names_the_grass_stacks() {
    let mut c = ExportConfig::default();
    assert_eq!(
        cm_lines(&c, false, false),
        [
            "Install the CM folder on top of the Vanilla folder.",
            "It holds only the `_m` files."
        ]
    );
    assert!(cm_lines(&c, true, false)[1].contains("complex-grass"));
    c.legacy.keep_metal_brightness = true;
    assert_eq!(cm_lines(&c, true, false).len(), 1);
    let (_d, m, loc) = project(|m| {
        let g = m.join("textures/landscape/grass");
        png(&g.join("g.png"), 16, 16, [90, 140, 60, 255]);
        png(&g.join("g_n.png"), 16, 16, [128, 128, 255, 255]);
        png(&g.join("g_rmaos.png"), 16, 16, [128, 0, 255, 255]);
    });
    let mut req = request(&m, &loc);
    req.gras_all_diffuses
        .insert("textures/landscape/grass/g".to_string());
    also(req, &spec(&loc, BOTH)).0.unwrap();
    let r = report(&loc, "Mod");
    assert!(r.contains("complex-grass"), "{r}");
}

#[test]
fn a_failed_file_does_not_block_the_zips() {
    let (_d, m, loc) = project(|m| {
        steel(m);
        cup(m);
        let g = m.join("textures/landscape/grass");
        png(&g.join("g.png"), 24, 24, [90, 140, 60, 255]);
        png(&g.join("g_n.png"), 24, 24, [128, 128, 255, 255]);
        png(&g.join("g_rmaos.png"), 24, 24, [128, 0, 255, 255]);
    });
    let mut req = request(&m, &loc);
    req.gras_all_diffuses
        .insert("textures/landscape/grass/g".to_string());
    let (s, ev) = also(req, &zip_spec(&loc, BOTH));
    let s = s.unwrap();
    let grass_shape = ev.iter().any(|e| match &e.kind {
        EventKind::Warning(w) => w.code == engine::codes::Code::GrassShape,
        EventKind::FileSkipped { code, .. } => *code == Some(engine::codes::Code::GrassShape),
        EventKind::FileFailed { error, .. } => error.contains("GRASS-SHAPE"),
        _ => false,
    });
    assert!(grass_shape, "{s:#?}");
    assert!(s.combined().failed > 0, "{s:#?}");
    assert_eq!(s.zips.len(), 3, "{:?}", s.notes);
    assert_eq!(sums(&loc).len(), 3);
}

#[test]
fn vanilla_alone_writes_two_sums_lines() {
    let (_d, m, loc) = project(|m| {
        steel(m);
        cup(m);
    });
    also(request(&m, &loc), &zip_spec(&loc, &[AlsoTarget::Vanilla]))
        .0
        .unwrap();
    let lines = sums(&loc);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].ends_with("  Mod PBR.zip") && lines[1].ends_with("  Mod Vanilla.zip"));
}

/// Carried from Task 7 (CM43): with `ALSO-OWNED-UNREADABLE` raised, no zip and no sums file is written.
#[test]
fn a_corrupt_owned_record_writes_no_zip_or_sums() {
    let (_d, m, loc) = project(cup);
    let rec = loc.join("Mod PBR/_sdc/also.json");
    std::fs::create_dir_all(rec.parent().unwrap()).unwrap();
    std::fs::write(&rec, b"{ not json").unwrap();
    let s = also(request(&m, &loc), &zip_spec(&loc, &[AlsoTarget::Vanilla]))
        .0
        .unwrap();
    assert!(zips_in(&loc).is_empty());
    assert!(s.zips.is_empty());
    assert!(!loc.join("Mod SHA256SUMS.txt").exists());
    assert_eq!(std::fs::read(&rec).unwrap(), b"{ not json");
    assert_eq!(s.exit_code(), 1, "no zip was written");
}

/// Review M2: a dry run writes no zip and says nothing about zips.
#[test]
fn a_dry_run_writes_no_zip() {
    let (_d, m, loc) = project(cup);
    let mut req = request(&m, &loc);
    req.dry_run = true;
    let s = also(req, &zip_spec(&loc, &[AlsoTarget::Vanilla]))
        .0
        .unwrap();
    assert!(!loc.exists() || zips_in(&loc).is_empty());
    assert!(s.zips.is_empty());
    assert!(!loc.join("Mod SHA256SUMS.txt").exists());
    assert!(s.notes.iter().all(|n| !n.contains("zip")), "{:?}", s.notes);
}

/// Review M2: a phase that returned `Err` stops the zips.
#[test]
fn a_phase_error_writes_no_zip() {
    let (_d, m, loc) = project(cup);
    std::fs::write(loc.join("Mod Vanilla"), b"a file").unwrap();
    let s = also(request(&m, &loc), &zip_spec(&loc, BOTH)).0.unwrap();
    assert!(s.error.is_some());
    assert!(zips_in(&loc).is_empty());
    assert!(!loc.join("Mod SHA256SUMS.txt").exists());
    assert!(
        s.notes
            .contains(&"zip skipped: the export did not finish".to_string()),
        "{:?}",
        s.notes
    );
}

/// Breaks `also.json` when the Vanilla phase ends (the second `batch_done`), after the start check read it.
struct BreakRecordOnVanillaDone(std::path::PathBuf, std::sync::atomic::AtomicU32);

impl EventSink for BreakRecordOnVanillaDone {
    fn emit(&self, ev: &EngineEvent) {
        if matches!(ev.kind, EventKind::BatchDone { .. })
            && self.1.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 1
        {
            std::fs::write(&self.0, b"broken").unwrap();
        }
    }
}

/// Review M5: an owned record that cannot be read at the zip step, and was not reported yet, is warned about.
#[test]
fn an_owned_record_unreadable_at_the_zip_step_is_reported() {
    let (_d, m, loc) = project(cup);
    let rec = loc.join("Mod PBR/_sdc/also.json");
    let sink = Arc::new(VecSink::default());
    let tee: Arc<dyn EventSink> = Arc::new(Tee(
        sink.clone(),
        Arc::new(BreakRecordOnVanillaDone(rec.clone(), Default::default())),
    ));
    let s = also_with(
        request(&m, &loc),
        &zip_spec(&loc, &[AlsoTarget::Vanilla]),
        tee,
        &CancelToken::default(),
    )
    .unwrap();
    assert!(zips_in(&loc).is_empty());
    assert_eq!(std::fs::read(&rec).unwrap(), b"broken");
    let warned = sink
        .take()
        .iter()
        .filter(|e| {
            matches!(&e.kind,
            EventKind::Warning(w) if w.code == engine::codes::Code::AlsoOwnedUnreadable)
        })
        .count();
    assert_eq!(warned, 1);
    assert!(
        s.notes.iter().any(|n| n.contains("also.json")),
        "{:?}",
        s.notes
    );
    assert_eq!(s.exit_code(), 1);
}

struct Tee(Arc<VecSink>, Arc<dyn EventSink>);

impl EventSink for Tee {
    fn emit(&self, ev: &EngineEvent) {
        self.0.emit(ev);
        self.1.emit(ev);
    }
}

/// Review M5: a corrupt record found at the start is warned about once, not again at the zip step.
#[test]
fn a_corrupt_owned_record_is_warned_once_with_zip_on() {
    let (_d, m, loc) = project(cup);
    let rec = loc.join("Mod PBR/_sdc/also.json");
    std::fs::create_dir_all(rec.parent().unwrap()).unwrap();
    std::fs::write(&rec, b"{ not json").unwrap();
    let (s, ev) = also(request(&m, &loc), &zip_spec(&loc, &[AlsoTarget::Vanilla]));
    s.unwrap();
    let warned = ev
        .iter()
        .filter(|e| {
            matches!(&e.kind,
            EventKind::Warning(w) if w.code == engine::codes::Code::AlsoOwnedUnreadable)
        })
        .count();
    assert_eq!(warned, 1);
}

// ---- Task 9: the conversion report (C5, TA16, TA21, TA26-TA28, TA30) ----

fn report(loc: &Path, project: &str) -> String {
    std::fs::read_to_string(loc.join(format!("{project} conversion-report.md"))).unwrap()
}

fn fake_dds(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b"DDS not a real texture").unwrap();
}

/// A DDS-only PBR set `textures/pbr/arm/iron` (DERIVE-DDS-SOURCE).
fn iron(m: &Path) {
    for f in ["iron.dds", "iron_n.dds", "iron_rmaos.dds"] {
        fake_dds(&m.join("textures/pbr/arm").join(f));
    }
}

fn envmap_request(m: &Path, loc: &Path) -> BatchRequest {
    let mut req = request(m, loc);
    req.mesh_shaders = std::collections::BTreeMap::from([(
        "textures/arm/steel".to_string(),
        vec![engine::shader::Shader::Envmap],
    )]);
    req
}

#[test]
fn the_report_lists_suggestions_skips_and_settings() {
    let (_d, m, loc) = project(|m| {
        steel(m);
        iron(m);
        cup(m);
    });
    let mut sp = spec(&loc, BOTH);
    sp.scanned = true;
    let s = also(envmap_request(&m, &loc), &sp).0.unwrap();
    assert_eq!(
        s.report,
        Some(loc.join("Mod conversion-report.md")),
        "{:?}",
        s.notes
    );
    let r = report(&loc, "Mod");
    assert!(
        r.starts_with("<!-- written by Tessalith; regenerated on every Also export -->\n"),
        "{r}"
    );
    assert!(
        r.contains("| textures/pbr/arm/steel | 29.5 | 1 | yes | yes |"),
        "{r}"
    );
    assert!(r.contains("brightness = 0.9"), "{r}");
    let skipped = r
        .lines()
        .find(|l| l.contains("textures/pbr/arm/iron"))
        .unwrap_or_default();
    assert!(skipped.contains("is DDS"), "{r}");
    assert!(
        !r.contains("| textures/pbr/arm/iron |"),
        "a skipped set has no row: {r}"
    );
    for f in ["| Mod PBR |", "| Mod Vanilla |", "| Mod CM |"] {
        assert!(r.contains(f), "{f}: {r}");
    }
    assert!(r.contains("Install the CM folder on top of the Vanilla folder."));
    assert!(r.contains("It holds only the `_m` files."));
    assert!(!r.contains('\r'));
    // The same inputs give the same set table (the counts differ: all up to date the second time).
    also(envmap_request(&m, &loc), &sp).0.unwrap();
    let rows = |r: &str| -> Vec<String> {
        r.lines()
            .filter(|l| l.starts_with("| textures/"))
            .map(str::to_string)
            .collect()
    };
    assert_eq!(rows(&r), rows(&report(&loc, "Mod")));
}

#[test]
fn without_a_scan_the_report_says_why_no_env_m_was_written() {
    let (_d, m, loc) = project(steel);
    also(request(&m, &loc), &spec(&loc, BOTH)).0.unwrap();
    let r = report(&loc, "Mod");
    let row = r
        .lines()
        .find(|l| l.starts_with("| textures/pbr/arm/steel |"))
        .unwrap_or_else(|| panic!("{r}"));
    assert!(row.contains("| yes | no: no scan |"), "{row}");
}

#[test]
fn a_report_tessalith_did_not_write_is_kept() {
    let (_d, m, loc) = project(cup);
    let path = loc.join("Mod conversion-report.md");
    std::fs::write(&path, b"my own notes").unwrap();
    let s = also(request(&m, &loc), &spec(&loc, &[AlsoTarget::Vanilla]))
        .0
        .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"my own notes");
    assert_eq!(s.report, None);
    let note = engine::dds::Kept::Foreign.note(&path);
    assert!(s.notes.contains(&note), "{:?}", s.notes);
    // Fix round 1 (CM51): as a kept zip, a Warning event and a partial exit.
    assert_eq!(s.exit_code(), 1, "{s:#?}");
    let (_, ev) = also(request(&m, &loc), &spec(&loc, &[AlsoTarget::Vanilla]));
    assert!(ev.iter().any(|e| matches!(&e.kind,
        EventKind::Warning(w) if w.code == engine::codes::Code::AlsoReportFailed && w.message.contains("conversion-report.md"))));
}

#[test]
fn two_projects_in_one_location_each_get_their_own_report() {
    let d = tempfile::tempdir().unwrap();
    let loc = d.path().join("mods");
    for p in ["Iron", "Steel"] {
        cup(&loc.join(p));
    }
    for p in ["Iron", "Steel"] {
        let s = also(request(&loc.join(p), &loc), &zip_spec(&loc, BOTH))
            .0
            .unwrap();
        assert!(
            s.notes.iter().all(|n| !n.contains(" kept")),
            "{p}: {:?}",
            s.notes
        );
        assert!(s.report.is_some(), "{p}");
    }
    for (p, other) in [("Iron", "Steel"), ("Steel", "Iron")] {
        let r = report(&loc, p);
        assert!(r.contains(&format!("| {p} PBR |")), "{r}");
        assert!(!r.contains(&format!("{other} PBR")), "{r}");
        assert!(loc.join(format!("{p} SHA256SUMS.txt")).exists());
    }
}

#[test]
fn a_cancelled_export_writes_the_report_with_cancelled_after_pbr() {
    let (_d, m, loc) = project(|m| {
        steel(m);
        cup(m);
    });
    let cancel = CancelToken::default();
    let s = also_with(
        request(&m, &loc),
        &spec(&loc, BOTH),
        Arc::new(CancelOnPbrDone(cancel.clone())),
        &cancel,
    )
    .unwrap();
    assert!(s.report.is_some(), "{:?}", s.notes);
    let r = report(&loc, "Mod");
    assert!(r.contains("Cancelled after PBR"), "{r}");
    assert!(!r.contains("No PBR set was converted"), "{r}");
    assert!(
        !r.contains("Install the CM folder"),
        "no CM folder was written: {r}"
    );
    assert!(r.contains("| Mod PBR |"), "{r}");
    assert!(
        !r.contains("| Mod Vanilla |") && !r.contains("| Mod CM |"),
        "{r}"
    );
}

#[test]
fn the_install_line_is_printed_with_keep_metal_on_too() {
    let (_d, m, loc) = project(steel);
    let mut sp = spec(&loc, BOTH);
    sp.cfg.legacy.keep_metal_brightness = true;
    also(request(&m, &loc), &sp).0.unwrap();
    let r = report(&loc, "Mod");
    assert!(r.contains("on top of the Vanilla folder"), "{r}");
    assert!(!r.contains("only the `_m`"), "{r}");
    assert_eq!(
        cm_lines(&sp.cfg, false, false),
        vec!["Install the CM folder on top of the Vanilla folder."]
    );
    sp.cfg.legacy.keep_metal_brightness = false;
    assert_eq!(cm_lines(&sp.cfg, false, false).len(), 2);
}

/// Carried into Task 9: a member with no recipe role is both a set-level `DERIVE-NOT-CONVERTED` note and a per-file
/// skip; the report lists it once.
#[test]
fn a_member_not_converted_is_listed_once() {
    let (_d, m, loc) = project(|m| {
        steel(m);
        png(
            &m.join("textures/pbr/arm/steel_g.png"),
            16,
            16,
            [10, 10, 10, 255],
        );
    });
    also(request(&m, &loc), &spec(&loc, BOTH)).0.unwrap();
    let r = report(&loc, "Mod");
    assert_eq!(r.matches("steel_g.png").count(), 1, "{r}");
    assert!(
        r.contains("no vanilla or complex-material map uses it"),
        "{r}"
    );
}

/// Carried into Task 9 (CM43): with `ALSO-OWNED-UNREADABLE` raised, no report is written.
#[test]
fn a_corrupt_owned_record_writes_no_report() {
    let (_d, m, loc) = project(cup);
    let rec = loc.join("Mod PBR/_sdc/also.json");
    std::fs::create_dir_all(rec.parent().unwrap()).unwrap();
    std::fs::write(&rec, b"{ not json").unwrap();
    let s = also(request(&m, &loc), &spec(&loc, &[AlsoTarget::Vanilla]))
        .0
        .unwrap();
    assert!(!loc.join("Mod conversion-report.md").exists());
    assert_eq!(s.report, None);
    assert_eq!(std::fs::read(&rec).unwrap(), b"{ not json");
}

#[test]
fn the_report_names_a_failed_phase_and_every_note() {
    let (_d, m, loc) = project(cup);
    std::fs::write(loc.join("Mod Vanilla"), b"a file").unwrap();
    let s = also(request(&m, &loc), &zip_spec(&loc, BOTH)).0.unwrap();
    let r = report(&loc, "Mod");
    assert!(r.contains("Vanilla phase failed"), "{r}");
    assert!(!s.notes.is_empty());
    for n in &s.notes {
        assert!(r.contains(n.as_str()), "{n}: {r}");
    }
    assert!(r.contains("zip skipped: the export did not finish"), "{r}");
}

#[test]
fn the_report_says_toksvig_and_inner_layer_per_set() {
    let (_d, m, loc) = project(steel);
    std::fs::write(
        m.join("sdc.toml"),
        "[files.\"textures/pbr/arm/steel_n.png\"]\nmips.normal = \"renormalize_bake\"\n",
    )
    .unwrap();
    let mut sp = spec(&loc, BOTH);
    sp.inner_layer_sets = set(&["Textures/PBR/Arm/Steel"]);
    also(request(&m, &loc), &sp).0.unwrap();
    let r = report(&loc, "Mod");
    assert!(r.contains("Toksvig not applied"), "{r}");
    assert!(r.contains("inner-layer `_p` copied unchanged"), "{r}");
}

// ---- Task 9 fix round 1 (I1, M4, N1) ----

/// I1: a size mismatch only the CM `_m` meets (a `_p` of another size) reaches the CM run: one `FileFailed`, a failed
/// count and a partial exit.
#[test]
fn a_cm_only_size_mismatch_fails_in_the_cm_phase() {
    let (_d, m, loc) = project(|m| {
        steel(m);
        png(
            &m.join("textures/pbr/arm/steel_p.png"),
            32,
            32,
            [100, 100, 100, 255],
        );
    });
    let (s, ev) = also(request(&m, &loc), &spec(&loc, BOTH));
    let s = s.unwrap();
    assert_eq!(phase(&s, Phase::Vanilla).failed, 0, "{s:#?}");
    assert_eq!(phase(&s, Phase::Cm).failed, 1, "{s:#?}");
    let failed: Vec<String> = keys_of(&ev, |k| matches!(k, EventKind::FileFailed { .. }))
        .into_iter()
        .filter(|k| k.starts_with("Mod CM/"))
        .collect();
    assert_eq!(failed, ["Mod CM/textures/arm/steel_m.dds"], "{ev:#?}");
    // Every CM job key ends once.
    let mut ends = std::collections::BTreeMap::<String, u32>::new();
    for k in keys_of(&ev, |k| {
        matches!(
            k,
            EventKind::FileDone { .. }
                | EventKind::FileSkipped { .. }
                | EventKind::FileFailed { .. }
        )
    }) {
        if k.starts_with("Mod CM/") {
            *ends.entry(k).or_default() += 1;
        }
    }
    assert!(ends.values().all(|&n| n == 1), "{ends:?}");
    assert_eq!(s.exit_code(), 1);
    let r = report(&loc, "Mod");
    assert_eq!(r.matches("CM: PACK-SIZE-MISMATCH").count(), 1, "{r}");
}

/// M4: an unrounded glossiness is printed `{:.1}`, and a scanned mesh without the Environment Map shader says so.
#[test]
fn a_scanned_mesh_without_envmap_and_an_unrounded_glossiness() {
    let (_d, m, loc) = project(|m| {
        let d = m.join("textures/pbr/arm");
        png(&d.join("steel.png"), 16, 16, [200, 200, 200, 255]);
        png(&d.join("steel_n.png"), 16, 16, [128, 128, 255, 255]);
        png(&d.join("steel_rmaos.png"), 16, 16, [100, 255, 200, 255]);
    });
    let mut req = request(&m, &loc);
    req.mesh_shaders = std::collections::BTreeMap::from([(
        "textures/arm/steel".to_string(),
        vec![engine::shader::Shader::Default],
    )]);
    let mut sp = spec(&loc, BOTH);
    sp.scanned = true;
    also(req, &sp).0.unwrap();
    let r = report(&loc, "Mod");
    let g = engine::derive::recipe::nif_glossiness(100.0 / 255.0);
    assert_ne!(format!("{g}"), format!("{g:.1}"));
    let row = format!("| textures/pbr/arm/steel | {g:.1} | 1 | yes | no: mesh not env-map |");
    assert!(r.contains(&row), "{row}: {r}");
}

/// M4, TA28: an RNM detail layer is listed once, with its own reason.
#[test]
fn an_rnm_detail_layer_is_listed_once_with_its_reason() {
    let (_d, m, loc) = project(|m| {
        steel(m);
        png(
            &m.join("textures/pbr/arm/steel_detail.png"),
            16,
            16,
            [128, 128, 255, 255],
        );
    });
    also(request(&m, &loc), &spec(&loc, BOTH)).0.unwrap();
    let r = report(&loc, "Mod");
    assert_eq!(r.matches("steel_detail.png").count(), 1, "{r}");
    assert!(
        r.contains("steel_detail.png is not converted: detail layer not in the vanilla _n"),
        "{r}"
    );
}

/// N1: the brief writes the report after a failure, so a PBR run that fails is recorded with its phase and exit
/// code, and a report is written when it can be (here it cannot: the PBR folder's name is taken by a file).
#[test]
fn a_failed_pbr_run_is_recorded_not_returned() {
    let (_d, m, loc) = project(cup);
    std::fs::write(loc.join("Mod PBR"), b"a file").unwrap();
    let (s, ev) = also(request(&m, &loc), &spec(&loc, BOTH));
    let s = s.unwrap();
    let (p, _, code) = s.error.clone().expect("the error is kept");
    assert_eq!((p, code), (Phase::Pbr, 3));
    assert_eq!(s.exit_code(), 3);
    assert!(!loc.join("Mod Vanilla").exists());
    assert!(ev.iter().any(|e| matches!(&e.kind,
        EventKind::Warning(w) if w.code == engine::codes::Code::AlsoPhaseFailed && w.message.contains("Pbr"))));
}

/// Final review I1: a set's `_curves` member reaches the Vanilla diffuse (spec step 3, CM3), and is not reported as
/// not converted.
#[test]
fn a_curves_member_changes_the_vanilla_diffuse() {
    let diffuse = |curves: bool| {
        let (_d, m, loc) = project(|m| {
            steel(m);
            if curves {
                png(
                    &m.join("textures/pbr/arm/steel_curves.png"),
                    16,
                    16,
                    [240, 240, 240, 255],
                );
            }
        });
        let (s, ev) = also(request(&m, &loc), &spec(&loc, &[AlsoTarget::Vanilla]));
        assert_eq!(s.unwrap().exit_code(), 0);
        let not_converted = ev.iter().any(|e| match &e.kind {
            EventKind::Warning(w) => w.code == engine::codes::Code::DeriveNotConverted,
            EventKind::FileSkipped { code, .. } => {
                *code == Some(engine::codes::Code::DeriveNotConverted)
            }
            _ => false,
        });
        assert!(!not_converted, "{curves}: {ev:#?}");
        std::fs::read(loc.join("Mod Vanilla/textures/arm/steel.dds")).unwrap()
    };
    assert_ne!(diffuse(false), diffuse(true));
}
