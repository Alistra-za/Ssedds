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
    let mut owned = OwnedFiles::load(&pbr);
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
        OwnedFiles::load(&pbr).write(&foreign, b"ours"),
        Err(engine::dds::Kept::Foreign)
    ));
    assert_eq!(std::fs::read(&foreign).unwrap(), b"mine");
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
