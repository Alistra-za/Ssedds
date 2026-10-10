//! M2.5 Task 6: what `Engine::plan` makes of a set's raw maps (spec §6.4, DECISIONS Note M).
use engine::api::*;
use engine::codes::Code;
use engine::config::*;
use engine::events::*;
use engine::overrides::parse_set;
use engine::policy::*;
use engine::presets::Provenance;
use image::{ImageBuffer, Luma, Rgb, Rgba};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

fn put(dir: &Path, name: &str, w: u32, f: impl Fn(u32, u32) -> u8) {
    let p = dir.join("in/textures/a").join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(w, w, |x, y| Luma([f(x, y)]))
        .save(p)
        .unwrap();
}

fn request(dir: &Path, target: Target, sets: &[&str]) -> BatchRequest {
    let mut req = BatchRequest::new(vec![dir.join("in")], dir.join("out"), target);
    req.recursive = true;
    req.overrides = sets.iter().map(|s| parse_set(s).unwrap()).collect();
    req
}

fn plan_of(req: BatchRequest) -> (Plan, Vec<EngineEvent>) {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "pack-plan");
    let p = Engine::new()
        .unwrap()
        .plan(req, &rep, &CancelToken::default())
        .unwrap();
    (p, sink.take())
}

fn plan(dir: &Path, target: Target, sets: &[&str]) -> (Plan, Vec<EngineEvent>) {
    plan_of(request(dir, target, sets))
}

/// The dry run of a plan: what `convert --dry-run` reports.
fn dry_run(mut p: Plan) -> (BatchSummary, Vec<EngineEvent>) {
    p.request.dry_run = true;
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "pack-plan");
    let s = Engine::new()
        .unwrap()
        .run(p, &rep, &CancelToken::default())
        .unwrap();
    (s, sink.take())
}

fn job<'a>(p: &'a Plan, name: &str) -> &'a ResolvedJob {
    p.jobs
        .iter()
        .find(|j| j.input.file_name().is_some_and(|n| n == name))
        .unwrap_or_else(|| panic!("no job for {name}"))
}

fn codes(j: &ResolvedJob) -> Vec<Code> {
    j.warnings.iter().map(|w| w.code).collect()
}

fn recipes(ev: &[EngineEvent]) -> Vec<RecipeReport> {
    ev.iter()
        .flat_map(|e| match &e.kind {
            EventKind::SetResolved { recipes, .. } => recipes.clone(),
            _ => vec![],
        })
        .collect()
}

#[test]
fn a_lone_raw_map_packs_into_rmaos() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "iron_roughness.png", 64, |x, _| x as u8 * 4);
    let (p, ev) = plan(d.path(), Target::Cs, &[]);
    assert_eq!(p.jobs.len(), 1, "the consumed raw job leaves the plan");
    let j = job(&p, "iron_roughness.png");
    assert!(
        j.output.ends_with("textures/a/iron_rmaos.dds"),
        "{}",
        j.output.display()
    );
    assert_eq!(j.profile.subtype, Subtype::Rmaos);
    assert_eq!(j.status, JobStatus::Ready);
    assert_eq!(j.provenance["subtype"], Provenance::Recipe("rmaos".into()));
    // Like every job of the batch, the recipe job records where the target came from.
    assert!(j.provenance.contains_key("target"), "{:?}", j.provenance);
    assert_eq!(
        j.provenance["pack.r"],
        Provenance::Source("iron_roughness.png:R".into())
    );
    assert_eq!(j.provenance["pack.g"], Provenance::Default("0".into()));
    assert_eq!(j.provenance["pack.b"], Provenance::Default("1".into()));
    assert_eq!(j.provenance["pack.a"], Provenance::Default("1".into()));
    let defaulted: Vec<&str> = j
        .warnings
        .iter()
        .filter(|w| w.code == Code::PackDefaulted)
        .map(|w| w.data["channel"].as_str().unwrap())
        .collect();
    assert_eq!(defaulted, ["G", "B", "A"]);
    let r = recipes(&ev);
    assert_eq!(r.len(), 1);
    assert_eq!(
        (r[0].status, r[0].subtype),
        (RecipeStatus::Run, Subtype::Rmaos)
    );
    assert!(r[0].output.ends_with("iron_rmaos.dds"));
    assert_eq!(
        r[0].channels,
        [
            "source:iron_roughness.png:R",
            "default:0",
            "default:1",
            "default:1"
        ]
    );
}

/// DECISIONS Note M8: a packed job's `input` is its first source in R, G, B, A order, not the first by path
/// (`iron_ao.png` sorts before `iron_roughness.png` but packs into B).
#[test]
fn a_packed_job_input_is_its_first_source_in_channel_order() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "iron_roughness.png", 64, |x, _| x as u8 * 4);
    put(d.path(), "iron_ao.png", 64, |_, y| y as u8 * 4);
    let (p, _) = plan(d.path(), Target::Cs, &[]);
    assert_eq!(p.jobs.len(), 1, "{:?}", p.jobs);
    let j = job(&p, "iron_roughness.png");
    let pk = j.pack.as_ref().unwrap();
    assert_eq!(j.input, pk.sources[0].path);
    let names: Vec<_> = pk
        .sources
        .iter()
        .map(|s| s.path.file_name().unwrap().to_owned())
        .collect();
    assert_eq!(names, ["iron_roughness.png", "iron_ao.png"]);
    assert!(!p.jobs.iter().any(|j| j.input.ends_with("iron_ao.png")));
}

#[test]
fn unused_raw_maps_are_reported_not_converted() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "iron_roughness.png", 64, |x, _| x as u8 * 4);
    put(d.path(), "iron_opacity.png", 64, |_, _| 255);
    let (p, _) = plan(d.path(), Target::Cs, &[]);
    let o = job(&p, "iron_opacity.png");
    assert_eq!(
        o.status,
        JobStatus::PackingInput {
            role: RawMap::Opacity
        }
    );
    assert_eq!(codes(o), [Code::PackUnusedInput]);
    let (s, ev) = dry_run(p);
    assert_eq!((s.skipped, s.exit_code()), (0, 0));
    assert!(ev.iter().any(|e| matches!(
        e.kind,
        EventKind::FileSkipped {
            code: Some(Code::PackUnusedInput),
            ..
        }
    ) && e
        .path
        .as_deref()
        .is_some_and(|p| p.ends_with("iron_opacity.png"))));
}

#[test]
fn packing_can_be_turned_off_per_output() {
    for set in [
        "packing.enabled=false",
        "subtype:rmaos.packing.enabled=false",
        "file:**/iron_rmaos.png.packing.enabled=false",
    ] {
        let d = tempfile::tempdir().unwrap();
        put(d.path(), "iron_roughness.png", 64, |x, _| x as u8 * 4);
        let (p, ev) = plan(d.path(), Target::Cs, &[set]);
        let j = job(&p, "iron_roughness.png");
        assert!(j.pack.is_none(), "{set}");
        assert_eq!(
            j.status,
            JobStatus::PackingInput {
                role: RawMap::Roughness
            },
            "{set}"
        );
        assert_eq!(codes(j), [Code::InPackingInput], "{set}");
        assert!(
            j.warnings[0].message.contains("packing.enabled = false"),
            "{set}"
        );
        assert_eq!(recipes(&ev)[0].status, RecipeStatus::Disabled, "{set}");
    }
}

/// Note M4 (review risk c): raw maps of a disabled recipe stay `IN-PACKING-INPUT`; they are reported but do not
/// count as skipped, so the batch is not partial.
#[test]
fn a_disabled_recipe_is_not_a_partial_batch() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "iron_roughness.png", 64, |x, _| x as u8 * 4);
    put(d.path(), "iron_ao.png", 64, |_, y| y as u8 * 4);
    let (p, _) = plan(d.path(), Target::Cs, &["packing.enabled=false"]);
    assert_eq!(p.jobs.len(), 2, "{:?}", p.jobs);
    let (s, ev) = dry_run(p);
    assert_eq!(
        (s.skipped, s.skipped_intentional, s.failed, s.exit_code()),
        (0, 0, 0, 0)
    );
    let raw_skips = ev
        .iter()
        .filter(|e| {
            matches!(
                e.kind,
                EventKind::FileSkipped {
                    code: Some(Code::InPackingInput),
                    ..
                }
            )
        })
        .count();
    assert_eq!(raw_skips, 2);
}

/// Note M11, ruling W7: a recipe whose sources were planned but whose pass-2 Load the cancel pre-empted has no
/// job, so its report says `failed`/"cancelled", not `run`. One worker thread makes the timing deterministic: the
/// budget admits one file at a time, so `iron_roughness.png` has finished its pass-1 Load before `x.png` is
/// admitted, and the hook cancels just before `zz_cancel_hook.png` (the last file) would load.
#[test]
fn a_recipe_cancelled_before_pass_2_is_reported_failed() {
    struct Hook;
    impl Drop for Hook {
        fn drop(&mut self) {
            std::env::remove_var("SDC_TEST_CANCEL_ON");
        }
    }
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "iron_roughness.png", 64, |x, _| x as u8 * 4);
    put(d.path(), "x.png", 16, |_, _| 128);
    put(d.path(), "zz_cancel_hook.png", 16, |_, _| 128);
    std::env::set_var("SDC_TEST_CANCEL_ON", "zz_cancel_hook.png");
    let _hook = Hook;
    let mut req = request(d.path(), Target::Cs, &[]);
    req.jobs = 1;
    let (p, ev) = plan_of(req);
    assert!(
        p.cancelled
            .iter()
            .any(|c| c.ends_with("iron_roughness.png")),
        "{:?}",
        p.cancelled
    );
    assert!(!p.jobs.iter().any(|j| j.pack.is_some()), "{:?}", p.jobs);
    let r = &recipes(&ev)[0];
    assert_eq!(r.status, RecipeStatus::Failed);
    assert_eq!(r.reason.as_deref(), Some("cancelled"));
}

fn prepacked(dir: &Path) {
    let p = dir.join("in/textures/a/iron_rmaos.png");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(64, 64, |x, _| Rgba([x as u8 * 4, 0, 255, 255]))
        .save(p)
        .unwrap();
}

#[test]
fn a_prepacked_file_wins_by_default() {
    let d = tempfile::tempdir().unwrap();
    prepacked(d.path());
    put(d.path(), "iron_roughness.png", 64, |x, _| x as u8 * 4);
    let (p, ev) = plan(d.path(), Target::Cs, &[]);
    let pre = job(&p, "iron_rmaos.png");
    assert!(pre.pack.is_none());
    assert!(
        codes(pre).contains(&Code::PackConflict),
        "{:?}",
        pre.warnings
    );
    let raw = job(&p, "iron_roughness.png");
    assert_eq!(codes(raw), [Code::PackUnusedInput]);
    assert!(raw.warnings[0].message.contains("iron_rmaos.png"));
    assert_eq!(recipes(&ev)[0].status, RecipeStatus::Conflict);
}

/// DECISIONS Note M9: with `packing.prefer = raw` the pre-packed file is an intentional skip.
#[test]
fn prefer_raw_replaces_the_prepacked_file() {
    let d = tempfile::tempdir().unwrap();
    prepacked(d.path());
    put(d.path(), "iron_roughness.png", 64, |x, _| x as u8 * 4);
    let (p, ev) = plan(d.path(), Target::Cs, &["packing.prefer=raw"]);
    assert!(job(&p, "iron_roughness.png").pack.is_some());
    assert!(!p.jobs.iter().any(|j| j.input.ends_with("iron_rmaos.png")));
    let sk = p
        .skipped
        .iter()
        .find(|s| s.path.ends_with("iron_rmaos.png"))
        .unwrap();
    assert_eq!(sk.code, Some(Code::PackConflict));
    assert_eq!(recipes(&ev)[0].status, RecipeStatus::Conflict);
    let (s, _) = dry_run(p);
    assert_eq!((s.skipped, s.skipped_intentional, s.exit_code()), (1, 1, 0));
}

/// DECISIONS Note M9: a recipe that cannot be packed never costs the user the file they had. With
/// `packing.prefer = raw` and sources of different sizes the pre-packed file stays a job (no `PACK-CONFLICT`
/// skip) and the recipe fails with `PACK-SIZE-MISMATCH`.
#[test]
fn prefer_raw_with_mismatched_sources_keeps_the_prepacked_file() {
    let d = tempfile::tempdir().unwrap();
    prepacked(d.path());
    put(d.path(), "iron_roughness.png", 64, |_, _| 100);
    put(d.path(), "iron_ao.png", 32, |_, _| 200);
    // The kept pre-packed file must convert, not wait on its `_rmaos` suggestion (never auto-decide).
    let mut req = request(d.path(), Target::Cs, &["packing.prefer=raw"]);
    req.accept = AcceptSuggestions::High;
    let (p, ev) = plan_of(req);
    assert!(p.skipped.is_empty(), "{:?}", p.skipped);
    let pre = job(&p, "iron_rmaos.png");
    assert!(pre.pack.is_none());
    assert_eq!(pre.status, JobStatus::Ready, "{:?}", pre.warnings);
    assert_eq!(
        job(&p, "iron_roughness.png").status,
        JobStatus::Error {
            codes: vec![Code::PackSizeMismatch]
        }
    );
    let r = &recipes(&ev)[0];
    assert_eq!(r.status, RecipeStatus::Failed);
    assert!(
        r.reason.as_deref().unwrap().contains("iron_rmaos.png"),
        "{:?}",
        r.reason
    );
    let (s, _) = dry_run(p);
    assert_eq!((s.skipped, s.failed, s.exit_code()), (0, 1, 1));
}

/// Review Focus 1, DECISIONS Note M11.
#[test]
fn a_source_that_fails_to_load_fails_its_recipe() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "iron_roughness.png", 64, |x, _| x as u8 * 4);
    std::fs::write(d.path().join("in/textures/a/iron_ao.png"), b"not a png").unwrap();
    let (p, ev) = plan(d.path(), Target::Cs, &[]);
    assert!(p.failed.iter().any(|f| f.path.ends_with("iron_ao.png")));
    assert!(
        p.jobs.is_empty(),
        "no recipe job and no leftover raw job: {:?}",
        p.jobs
    );
    let r = &recipes(&ev)[0];
    assert_eq!(r.status, RecipeStatus::Failed);
    assert!(r.reason.as_deref().unwrap().contains("iron_ao.png"));
    assert_eq!(dry_run(p).0.exit_code(), 1);
}

#[test]
fn sources_of_different_sizes_fail_the_recipe() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "iron_roughness.png", 64, |_, _| 100);
    put(d.path(), "iron_ao.png", 32, |_, _| 200);
    let (p, ev) = plan(d.path(), Target::Cs, &[]);
    let j = job(&p, "iron_roughness.png");
    assert_eq!(
        j.status,
        JobStatus::Error {
            codes: vec![Code::PackSizeMismatch]
        }
    );
    // Like every job of the batch (ruling W1), and with the recipe as its sub-type's origin.
    assert_eq!(j.provenance["target"], Provenance::User);
    assert_eq!(j.provenance["subtype"], Provenance::Recipe("rmaos".into()));
    let m = &j.warnings[0].message;
    assert!(
        m.contains("iron_roughness.png 64x64") && m.contains("iron_ao.png 32x32"),
        "{m}"
    );
    assert_eq!(recipes(&ev)[0].status, RecipeStatus::Failed);
    let (s, _) = dry_run(p);
    assert_eq!((s.failed, s.exit_code()), (1, 1));
}

/// Review Focus 5.
#[test]
fn a_raw_map_given_a_subtype_leaves_the_recipes() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "iron_roughness.png", 64, |x, _| x as u8 * 4);
    put(d.path(), "iron_height.png", 64, |_, y| y as u8 * 4);
    let (p, ev) = plan(
        d.path(),
        Target::Cs,
        &["file:**/iron_height.png.subtype=height"],
    );
    let h = job(&p, "iron_height.png");
    assert!(h.pack.is_none() && h.output.ends_with("iron_height.dds"));
    assert_eq!(h.profile.subtype, Subtype::Height);
    assert!(job(&p, "iron_roughness.png").pack.is_some());
    let kinds: Vec<Subtype> = recipes(&ev).iter().map(|r| r.subtype).collect();
    assert_eq!(kinds, [Subtype::Rmaos]);
}

/// Spec §6.4 "Set membership", DECISIONS Note M7.
#[test]
fn set_has_rules_see_the_packed_suffix() {
    let d = tempfile::tempdir().unwrap();
    let n = |name: &str| {
        ImageBuffer::from_fn(16, 16, |_, _| Rgb([128u8, 128, 255]))
            .save(d.path().join("in/textures/a").join(name))
            .unwrap()
    };
    put(d.path(), "iron_ao.png", 16, |_, _| 255);
    n("iron_n.png");
    n("steel_n.png");
    let cfg = d.path().join("sdc.toml");
    std::fs::write(
        &cfg,
        "[[rules]]\nmatch = { suffix = \"n\", set_has = \"rmaos\" }\noverrides = { mips = { levels = 3 } }\n",
    )
    .unwrap();
    let mut req = request(d.path(), Target::Cs, &[]);
    req.project_config = Some(cfg);
    let (p, _) = plan_of(req);
    assert_eq!(
        job(&p, "iron_n.png").profile.mips.levels,
        MipLevels::Count(3)
    );
    assert_eq!(
        job(&p, "steel_n.png").profile.mips.levels,
        MipLevels::Keyword(MipKeyword::Full)
    );
}

/// Review Focus 4, DECISIONS Note M8, final review I5.
#[test]
fn file_planned_paths_stay_unique() {
    let d = tempfile::tempdir().unwrap();
    prepacked(d.path());
    put(d.path(), "iron_roughness.png", 64, |x, _| x as u8 * 4);
    put(d.path(), "iron_height.png", 64, |_, y| y as u8 * 4);
    ImageBuffer::from_fn(16, 16, |_, _| Rgb([128u8, 128, 255]))
        .save(d.path().join("in/textures/a/iron_n.png"))
        .unwrap();
    let (p, ev) = plan(d.path(), Target::Cs, &["packing.prefer=raw"]);
    let planned: Vec<String> = ev
        .iter()
        .filter(|e| matches!(e.kind, EventKind::FilePlanned { .. }))
        .map(|e| e.path.clone().unwrap())
        .collect();
    let unique: BTreeSet<&String> = planned.iter().collect();
    assert_eq!(unique.len(), planned.len(), "{planned:?}");
    assert_eq!(
        planned.len(),
        3,
        "iron_n, the _rmaos recipe and the _p recipe: {planned:?}"
    );
    for s in &p.skipped {
        let sp = s.path.to_string_lossy().replace('\\', "/");
        assert!(!planned.contains(&sp), "{sp} is both skipped and planned");
    }
}

/// DECISIONS Note M6: pass 1 has no packed image, so a rule that matches on `alpha` cannot change `packing.*`,
/// whatever its value. Pass 2 still applies it to the real packed image for the other fields.
#[test]
fn alpha_rules_cannot_change_the_recipe() {
    for alpha in ["varying", "none", "constant255"] {
        let d = tempfile::tempdir().unwrap();
        put(d.path(), "iron_roughness.png", 64, |x, _| x as u8 * 4);
        let cfg = d.path().join("sdc.toml");
        std::fs::write(
            &cfg,
            format!(
                "[[rules]]\nmatch = {{ alpha = \"{alpha}\" }}\noverrides = {{ packing = {{ enabled = false }}, channels = {{ grayscale_source = \"luma\" }}, mips = {{ levels = 3 }} }}\n"
            ),
        )
        .unwrap();
        let mut req = request(d.path(), Target::Cs, &[]);
        req.project_config = Some(cfg);
        let (p, ev) = plan_of(req);
        let j = job(&p, "iron_roughness.png");
        let pk = j.pack.as_ref().unwrap_or_else(|| panic!("{alpha}: {j:?}"));
        assert_eq!(recipes(&ev)[0].status, RecipeStatus::Run, "{alpha}");
        assert_eq!(
            pk.gray,
            GraySource::R,
            "{alpha}: grayscale_source stays frozen from pass 1"
        );
        // The packed roughness-only image has a defaulted (constant 255) alpha: only that rule applies in pass 2.
        let expected = if alpha == "constant255" {
            MipLevels::Count(3)
        } else {
            MipLevels::Keyword(MipKeyword::Full)
        };
        assert_eq!(j.profile.mips.levels, expected, "{alpha}");
    }
}
