//! M4 Task 7: a roughness member with a `BakeJob` decodes its `_n`, widens its roughness, records the `_n` in the
//! manifest, re-encodes when the `_n` or its decode settings change, and fails when the `_n` changed after
//! planning (DECISIONS R4, R12).
use engine::api::*;
use engine::bake::BakeJob;
use engine::config::*;
use engine::dds::read_dds;
use engine::decode::{decode_level, Decoded};
use engine::events::*;
use engine::manifest::{sha256_file, Manifest};
use engine::policy::*;
use engine::presets::PresetRegistry;
use image::{ImageBuffer, Rgb, Rgba};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// A unit-length normal map whose direction changes from texel to texel; another `phase` gives another map.
fn bumpy_n(dir: &Path, w: u32, phase: f32) -> PathBuf {
    let p = dir.join("in/iron_n.png");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(w, w, |x, y| {
        let nx = 0.6 * (x as f32 * 1.3 + y as f32 * 0.7 + phase).sin();
        let ny = 0.6 * (x as f32 * 0.9 - y as f32 * 1.1).cos();
        let nz = (1.0 - nx * nx - ny * ny).sqrt();
        let e = |v: f32| ((v * 0.5 + 0.5) * 255.0).round() as u8;
        Rgb([e(nx), e(ny), e(nz)])
    })
    .save(&p)
    .unwrap();
    p
}

/// A constant roughness of 100/255 (G 0, B 255, A 255).
fn rmaos(dir: &Path, w: u32) -> PathBuf {
    let p = dir.join("in/iron_rmaos.png");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(w, w, |_, _| Rgba([100u8, 0, 255, 255]))
        .save(&p)
        .unwrap();
    p
}

/// The `_rmaos` job as `Engine::plan` builds it for a baking set (stamps taken now). `bake: false` leaves the
/// `BakeJob` off; `flip_green` changes the `_n`'s decode settings.
fn plan(dir: &Path, rough: &Path, normal: &Path, bake: bool, flip_green: bool) -> Plan {
    let registry = PresetRegistry::builtin().unwrap();
    let mut normal_profile = registry
        .profile("cs-quality", Target::Cs, Subtype::PbrNormal)
        .unwrap();
    normal_profile.channels.flip_green = flip_green;
    let mut request = BatchRequest::new(vec![dir.join("in")], dir.join("out"), Target::Cs);
    request.on_existing = OnExisting::IfChanged;
    let job = ResolvedJob {
        input: rough.to_path_buf(),
        output: dir.join("out/iron_rmaos.dds"),
        game_path: "iron_rmaos.png".into(),
        set_key: "out/iron".into(),
        profile: registry
            .profile("cs-quality", Target::Cs, Subtype::Rmaos)
            .unwrap(),
        provenance: Default::default(),
        suggestions: vec![],
        warnings: vec![],
        status: JobStatus::Ready,
        suffix_alias: None,
        source: SourceStamp::of(rough),
        pack: None,
        bake: bake.then(|| BakeJob {
            normal: normal.to_path_buf(),
            normal_profile,
            shift: 0,
            stamp: SourceStamp::of(normal),
        }),
        shader: None,
        rnm: None,
    };
    Plan {
        request,
        registry,
        preset_id: "cs-quality".into(),
        target: Target::Cs,
        jobs: vec![job],
        skipped: vec![],
        failed: vec![],
        peak_admitted_bytes: 0,
        cancelled: vec![],
        shaders: Default::default(),
    }
}

fn run(plan: Plan) -> (BatchSummary, Vec<EngineEvent>) {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "bake-run");
    let s = Engine::new()
        .unwrap()
        .run(plan, &rep, &CancelToken::default())
        .unwrap();
    (s, sink.take())
}

/// Mean decoded R of mip `level` of `out/iron_rmaos.dds`, in codes.
fn mean_r(dir: &Path, level: usize) -> f64 {
    let f = read_dds(&std::fs::read(dir.join("out/iron_rmaos.dds")).unwrap()).unwrap();
    let (w, h, r) = f.levels[level].clone();
    let Decoded::Ldr(px) = decode_level(f.format, w, h, &f.data[r], None).unwrap() else {
        panic!("LDR expected")
    };
    px.iter().map(|p| f64::from(p[0])).sum::<f64>() / px.len() as f64
}

#[test]
fn a_baked_member_is_rougher_and_records_its_normal() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    for (d, bake) in [(&a, true), (&b, false)] {
        let n = bumpy_n(d.path(), 32, 0.0);
        let r = rmaos(d.path(), 32);
        let (s, ev) = run(plan(d.path(), &r, &n, bake, false));
        assert_eq!(s.converted, 1, "{ev:#?}");
    }
    for level in 1..=3 {
        let (baked, plain) = (mean_r(a.path(), level), mean_r(b.path(), level));
        assert!(baked > plain + 2.0, "mip {level}: {baked} vs {plain}");
    }
    // 1:1, so mip 0 has no variance: both encode the same constant.
    assert!((mean_r(a.path(), 0) - mean_r(b.path(), 0)).abs() < 1.0);
    let m = Manifest::at(&a.path().join("out")).load().unwrap();
    let e = m.values().next().unwrap();
    assert_eq!(e.sources.len(), 1, "{:?}", e.sources);
    assert!(e.sources[0].path.ends_with("iron_n.png"));
    assert_eq!(
        e.sources[0].sha256,
        sha256_file(&a.path().join("in/iron_n.png")).unwrap()
    );
    assert!(e.sources[0].bake.is_some());
    let plain = Manifest::at(&b.path().join("out")).load().unwrap();
    assert!(plain.values().next().unwrap().sources.is_empty());
}

#[test]
fn a_two_to_one_normal_bakes_into_mip_0_too() {
    let d = tempfile::tempdir().unwrap();
    let n = bumpy_n(d.path(), 64, 0.0);
    let r = rmaos(d.path(), 32);
    let mut p = plan(d.path(), &r, &n, true, false);
    p.jobs[0].bake.as_mut().unwrap().shift = 1;
    assert_eq!(run(p).0.converted, 1);
    assert!(mean_r(d.path(), 0) > 102.0, "{}", mean_r(d.path(), 0));
}

/// Controller correction to R1: the bake reads the `_n`'s mip 0 as written. A uniformly short flat `_n` (|n| = 0.5,
/// every texel pointing up) adds no roughness under the default `renormalize`, and adds roughness under `keep`.
#[test]
fn a_uniformly_short_flat_normal_adds_roughness_only_under_keep() {
    let mean_mip_1 = |base: Option<NormalBase>| {
        let d = tempfile::tempdir().unwrap();
        let n = d.path().join("in/iron_n.png");
        std::fs::create_dir_all(n.parent().unwrap()).unwrap();
        // (128, 128, 191) decodes to about (0.004, 0.004, 0.498): length 0.5, pointing up.
        ImageBuffer::from_fn(32, 32, |_, _| Rgb([128u8, 128, 191]))
            .save(&n)
            .unwrap();
        let r = rmaos(d.path(), 32);
        let mut p = plan(d.path(), &r, &n, base.is_some(), false);
        if let Some(b) = base {
            p.jobs[0]
                .bake
                .as_mut()
                .unwrap()
                .normal_profile
                .mips
                .normal_base = b;
        }
        assert_eq!(run(p).0.converted, 1);
        mean_r(d.path(), 1)
    };
    let plain = mean_mip_1(None);
    let default = mean_mip_1(Some(NormalBase::Renormalize));
    let keep = mean_mip_1(Some(NormalBase::Keep));
    assert!(
        (default - plain).abs() < 1.0,
        "default {default} vs unbaked {plain}"
    );
    assert!(keep > plain + 2.0, "keep {keep} vs unbaked {plain}");
}

#[test]
fn up_to_date_until_the_normal_or_its_decode_settings_change() {
    let d = tempfile::tempdir().unwrap();
    let n = bumpy_n(d.path(), 32, 0.0);
    let r = rmaos(d.path(), 32);
    assert_eq!(run(plan(d.path(), &r, &n, true, false)).0.converted, 1);
    let (s, _) = run(plan(d.path(), &r, &n, true, false));
    assert_eq!((s.converted, s.up_to_date), (0, 1));
    // The `_n`'s green flip is part of the bake's identity.
    let (s, _) = run(plan(d.path(), &r, &n, true, true));
    assert_eq!((s.converted, s.up_to_date), (1, 0));
    let (s, _) = run(plan(d.path(), &r, &n, true, true));
    assert_eq!((s.converted, s.up_to_date), (0, 1));
    // New `_n` bytes.
    bumpy_n(d.path(), 32, 1.0);
    let (s, _) = run(plan(d.path(), &r, &n, true, true));
    assert_eq!((s.converted, s.up_to_date), (1, 0));
    // How the `_n`'s mip 0 is written (`mips.normal_base`) is part of the bake's identity too (DECISIONS R1, R12).
    let keep = || {
        let mut p = plan(d.path(), &r, &n, true, true);
        p.jobs[0]
            .bake
            .as_mut()
            .unwrap()
            .normal_profile
            .mips
            .normal_base = NormalBase::Keep;
        p
    };
    let (s, _) = run(keep());
    assert_eq!((s.converted, s.up_to_date), (1, 0));
    let (s, _) = run(keep());
    assert_eq!((s.converted, s.up_to_date), (0, 1));
    // The member's own `mips.bake_clamp` (in its profile hash) and its own bytes re-encode as for any job.
    let clamp = || {
        let mut p = keep();
        p.jobs[0].profile.mips.bake_clamp = 0.5;
        p
    };
    let (s, _) = run(clamp());
    assert_eq!((s.converted, s.up_to_date), (1, 0));
    let (s, _) = run(clamp());
    assert_eq!((s.converted, s.up_to_date), (0, 1));
    ImageBuffer::from_fn(32, 32, |_, _| Rgba([140u8, 0, 255, 255]))
        .save(&r)
        .unwrap();
    let (s, _) = run(clamp());
    assert_eq!((s.converted, s.up_to_date), (1, 0));
    // Turning the bake off re-encodes too.
    let (s, _) = run(plan(d.path(), &r, &n, false, true));
    assert_eq!((s.converted, s.up_to_date), (1, 0));
}

/// Task 6 carry: `coat_normal` defaults to `mips.normal = renormalize_bake` for its own self-bake, which must not
/// make it a companion-bake target. Only roughness members widen from a `BakeJob`; one handed to a `_cnr` changes
/// none of its bytes.
#[test]
fn a_bake_job_on_a_coat_normal_changes_nothing() {
    let registry = PresetRegistry::builtin().unwrap();
    let coat = registry
        .profile("cs-quality", Target::Cs, Subtype::CoatNormal)
        .unwrap();
    assert_eq!(coat.mips.normal, NormalMips::RenormalizeBake);
    let bytes = |bake: bool| {
        let d = tempfile::tempdir().unwrap();
        let n = bumpy_n(d.path(), 32, 0.0);
        let cnr = d.path().join("in/iron_cnr.png");
        ImageBuffer::from_fn(32, 32, |x, y| {
            let v = (128 + (x as i32 * 7 - y as i32 * 5).rem_euclid(60) - 30) as u8;
            Rgba([v, 255 - v, 220, 140])
        })
        .save(&cnr)
        .unwrap();
        let mut p = plan(d.path(), &cnr, &n, bake, false);
        p.jobs[0].profile = coat.clone();
        p.jobs[0].output = d.path().join("out/iron_cnr.dds");
        p.jobs[0].game_path = "iron_cnr.png".into();
        let (s, ev) = run(p);
        assert_eq!(s.converted, 1, "{ev:#?}");
        std::fs::read(d.path().join("out/iron_cnr.dds")).unwrap()
    };
    assert_eq!(bytes(true), bytes(false));
}

/// Run a planned baking member after `edit` rewrote its `_n`; the member must fail at Load with this error.
fn fails_after(edit: impl FnOnce(&Path)) -> String {
    let d = tempfile::tempdir().unwrap();
    let n = bumpy_n(d.path(), 32, 0.0);
    let r = rmaos(d.path(), 32);
    let p = plan(d.path(), &r, &n, true, false);
    edit(d.path());
    let (s, ev) = run(p);
    assert_eq!((s.converted, s.failed), (0, 1), "{ev:#?}");
    assert!(!d.path().join("out/iron_rmaos.dds").exists());
    ev.iter()
        .find_map(|e| match &e.kind {
            EventKind::FileFailed {
                stage: Stage::Load,
                error,
            } => Some(error.clone()),
            _ => None,
        })
        .unwrap()
}

/// Review Focus 3: the `_n` is a source of the member (final review I7).
#[test]
fn a_normal_changed_after_planning_fails_the_member() {
    // Another size: the stamp catches it before the shift guard could.
    let err = fails_after(|d| {
        bumpy_n(d, 64, 0.0);
    });
    assert!(
        err.ends_with("iron_n.png: source changed since planning; re-run"),
        "{err}"
    );
}

/// The commonest edit: the `_n` repainted at the same size. Only the stamp (its modification time) can catch it.
#[test]
fn a_normal_repainted_at_the_same_size_after_planning_fails_the_member() {
    let err = fails_after(|d| {
        let n = bumpy_n(d, 32, 1.0);
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(10);
        std::fs::File::options()
            .write(true)
            .open(&n)
            .unwrap()
            .set_modified(later)
            .unwrap();
    });
    assert!(
        err.ends_with("iron_n.png: source changed since planning; re-run"),
        "{err}"
    );
}

/// Final review Minor 2 (DECISIONS R12): the `_n`'s bytes do not depend on whether it bakes, so attaching or
/// detaching the bake (its roughness member leaves the batch and comes back) leaves the `_n` up to date.
#[test]
fn the_n_stays_up_to_date_when_its_bake_turns_on_or_off() {
    let d = tempfile::tempdir().unwrap();
    // Under `textures/pbr/`, so the lone `_n` is still a `pbr_normal` (path-based PBR set).
    let pbr = d.path().join("in/textures/pbr");
    std::fs::create_dir_all(&pbr).unwrap();
    std::fs::rename(bumpy_n(d.path(), 32, 0.0), pbr.join("iron_n.png")).unwrap();
    let r = pbr.join("iron_rmaos.png");
    std::fs::rename(rmaos(d.path(), 32), &r).unwrap();
    let aside = d.path().join("iron_rmaos.png");
    let convert = || {
        let mut req =
            BatchRequest::new(vec![d.path().join("in")], d.path().join("out"), Target::Cs);
        req.recursive = true;
        req.accept = AcceptSuggestions::High;
        req.on_existing = OnExisting::IfChanged;
        let sink = Arc::new(VecSink::default());
        let rep = Reporter::new(sink.clone(), "bake-toggle");
        let e = Engine::new().unwrap();
        let p = e.plan(req, &rep, &CancelToken::default()).unwrap();
        let n = p
            .jobs
            .iter()
            .find(|j| j.input.ends_with("iron_n.png"))
            .expect("the _n has a job");
        assert_eq!(n.profile.subtype, Subtype::PbrNormal);
        e.run(p, &rep, &CancelToken::default()).unwrap()
    };
    let s = convert();
    assert_eq!((s.converted, s.up_to_date), (2, 0));
    std::fs::rename(&r, &aside).unwrap();
    let s = convert();
    assert_eq!(
        (s.converted, s.up_to_date),
        (0, 1),
        "bake off: the _n is unchanged"
    );
    std::fs::rename(&aside, &r).unwrap();
    let s = convert();
    assert_eq!(
        (s.converted, s.up_to_date),
        (0, 2),
        "bake on again: nothing changed"
    );
}

/// S-mips-7 (ruling F7): an NPOT pair with `resize = pot-down` bakes from the resized, renormalised `_n`. The
/// expectation is built through the public API: Preprocess (which resizes), the `_n`'s written mip 0, its length
/// pyramid and the member's chain with that bake.
#[test]
fn a_resized_pair_bakes_from_the_resized_normal() {
    use engine::bake::{LengthPyramid, NormalVariance};
    use engine::mips::build_chain_with;
    use engine::ops::preprocess;
    let d = tempfile::tempdir().unwrap();
    let n = bumpy_n(d.path(), 48, 0.0);
    let r = rmaos(d.path(), 48);
    let mut p = plan(d.path(), &r, &n, true, false);
    let job = &mut p.jobs[0];
    job.profile.resize = ResizeMode::PotDown;
    job.bake.as_mut().unwrap().normal_profile.resize = ResizeMode::PotDown;
    let (member_p, normal_p) = (
        job.profile.clone(),
        job.bake.as_ref().unwrap().normal_profile.clone(),
    );
    let (s, ev) = run(p);
    assert_eq!(s.converted, 1, "{ev:#?}");

    let load = |p: &Path| engine::load::load_image(p).unwrap().0;
    let n_work = preprocess(load(&n), &normal_p);
    assert_eq!((n_work.img.width, n_work.img.height), (32, 32));
    let base = build_chain_with(&n_work, &normal_p, None).swap_remove(0);
    let nv = NormalVariance::new(
        LengthPyramid::build(&base),
        (32, 32),
        member_p.mips.bake_clamp,
    )
    .unwrap();
    let m_work = preprocess(load(&r), &member_p);
    let chain = build_chain_with(&m_work, &member_p, Some(&nv));
    let want =
        chain[1].px.iter().map(|p| f64::from(p[0])).sum::<f64>() / chain[1].px.len() as f64 * 255.0;
    let got = mean_r(d.path(), 1);
    assert!(
        (got - want).abs() < 1.0,
        "level 1 mean R: {got} vs reference {want}"
    );
    assert!(got > 102.0, "the bake adds roughness: {got}");
}
