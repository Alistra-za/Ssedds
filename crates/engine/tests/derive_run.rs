//! Track A Task 3 (DECISIONS Note CM): a derived job runs the recipe on a PBR set's inputs, encodes the result as a
//! normal output and records every source with the job's identity. Jobs are built by hand from the plan of one
//! colour file (Task 4 makes the planner do this).
use engine::api::*;
use engine::config::*;
use engine::dds::{read_dds, DdsPixelFormat};
use engine::decode::{decode_level, Decoded};
use engine::derive::recipe::LegacyParams;
use engine::derive::{DeriveJob, DeriveKind, DeriveSource};
use engine::events::*;
use engine::manifest::{Manifest, SourceRef};
use engine::pack::ChannelSource;
use engine::policy::*;
use image::{ImageBuffer, Rgba};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn png(dir: &Path, rel: &str, w: u32, h: u32, px: [u8; 4]) -> PathBuf {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_pixel(w, h, Rgba(px)).save(&p).unwrap();
    p
}

fn rmaos_ch(path: &Path, role: RawMap, channel: u8) -> ChannelSource {
    ChannelSource::Source {
        path: path.to_path_buf(),
        role,
        invert: false,
        channel: Some(channel),
    }
}

fn source(path: &Path, role: &str) -> DeriveSource {
    DeriveSource {
        path: path.to_path_buf(),
        role: role.into(),
        stamp: SourceStamp::of(path),
    }
}

/// A derived job of `kind` reading `colour` (albedo, or the normal for `LegacyN`), an optional `normal` (grass) and
/// the `_rmaos` by channel.
fn derive(
    kind: DeriveKind,
    landscape: bool,
    albedo: Option<&Path>,
    normal: Option<&Path>,
    rmaos: &Path,
    params: LegacyParams,
) -> DeriveJob {
    let mut sources = vec![];
    if let Some(a) = albedo {
        sources.push(source(a, "albedo"));
    }
    if let Some(n) = normal {
        sources.push(source(n, "normal"));
    }
    sources.push(source(rmaos, "rough+metal+ao"));
    DeriveJob {
        kind,
        landscape,
        albedo: albedo.map(Path::to_path_buf),
        normal: normal.map(Path::to_path_buf),
        rough: rmaos_ch(rmaos, RawMap::Roughness, 0),
        metal: rmaos_ch(rmaos, RawMap::Metallic, 1),
        ao: rmaos_ch(rmaos, RawMap::Ao, 2),
        height: None,
        curves: None,
        cavity: None,
        gray: GraySource::R,
        params,
        sources,
    }
}

/// The plan of `dj`'s first source alone, its one job turned into `dj` writing `out/<name>`, with the profile of
/// the kind's sub-type under `vanilla`.
fn plan(dir: &Path, dj: DeriveJob, name: &str) -> Plan {
    let first = dj.sources[0].path.clone();
    let mut req = BatchRequest::new(
        vec![first.parent().unwrap().to_path_buf()],
        dir.join("out"),
        Target::Vanilla,
    );
    req.on_existing = OnExisting::IfChanged;
    req.accept = AcceptSuggestions::High;
    let mut plan = Engine::new()
        .unwrap()
        .plan(
            req,
            &Reporter::new(Arc::new(NullSink), "t"),
            &CancelToken::default(),
        )
        .unwrap();
    let mut job = plan
        .jobs
        .drain(..)
        .find(|j| j.input == first)
        .expect("the colour file's job");
    job.profile = plan
        .registry
        .profile(
            &plan.preset_id,
            Target::Vanilla,
            dj.kind.subtype(dj.landscape),
        )
        .unwrap();
    job.output = dir.join("out").join(name);
    job.status = JobStatus::Ready;
    job.derive = Some(dj);
    plan.jobs = vec![job];
    plan
}

fn run(plan: Plan) -> (BatchSummary, Vec<EngineEvent>) {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "derive-run");
    let s = Engine::new()
        .unwrap()
        .run(plan, &rep, &CancelToken::default())
        .unwrap();
    (s, sink.take())
}

/// Mip 0 of `path`, decoded, with its pixel format.
fn mip0(path: &Path) -> (DdsPixelFormat, u32, u32, Vec<[u8; 4]>) {
    let f = read_dds(&std::fs::read(path).unwrap()).unwrap();
    let (w, h, r) = f.levels[0].clone();
    let Decoded::Ldr(px) = decode_level(f.format, w, h, &f.data[r], None).unwrap() else {
        panic!("LDR expected")
    };
    (f.format, w, h, px)
}

fn near(v: u8, want: i32, tol: i32, what: &str) {
    assert!((i32::from(v) - want).abs() <= tol, "{what}: {v} vs {want}");
}

#[test]
fn a_vanilla_diffuse_job_writes_the_recipe_pixels() {
    let d = tempfile::tempdir().unwrap();
    let albedo = png(d.path(), "in/iron_albedo.png", 64, 64, [200, 200, 200, 255]);
    let rmaos = png(d.path(), "maps/iron_rmaos.png", 64, 64, [128, 0, 128, 255]);
    let dj = derive(
        DeriveKind::VanillaDiffuse,
        false,
        Some(&albedo),
        None,
        &rmaos,
        LegacyParams::default(),
    );
    let identity = dj.identity_sha256();
    let (s, ev) = run(plan(d.path(), dj, "iron.dds"));
    assert_eq!(s.converted, 1, "{ev:#?}");
    let (fmt, w, h, px) = mip0(&d.path().join("out/iron.dds"));
    assert_eq!(fmt, DdsPixelFormat::Bc7 { srgb: false });
    assert_eq!((w, h), (64, 64));
    // round(200 · 0.9 · lerp(1, 128/255, 0.5)) = 135.
    for p in &px {
        for &c in &p[..3] {
            near(c, 135, 2, "diffuse");
        }
        assert_eq!(p[3], 255);
    }
    let m = Manifest::at(&d.path().join("out")).load().unwrap();
    let e = m.values().next().unwrap();
    let roles: Vec<_> = e.sources.iter().map(|r| r.derive_role.clone()).collect();
    assert_eq!(
        roles,
        [
            Some("albedo".to_string()),
            Some("rough+metal+ao".to_string())
        ]
    );
    assert!(e.sources[0].path.ends_with("iron_albedo.png"));
    assert!(e.sources[1].path.ends_with("iron_rmaos.png"));
    assert!(e
        .sources
        .iter()
        .all(|r| r.derive.as_deref() == Some(identity.as_str())));
}

#[test]
fn a_legacy_n_keeps_rgb_and_writes_spec_alpha_as_bc7_data() {
    let d = tempfile::tempdir().unwrap();
    let normal = png(d.path(), "in/iron_n.png", 64, 64, [128, 128, 255, 255]);
    // rough 0.5, metal 1, AO 1: 0.66 · 0.5 · 1 · 2 = 0.66 → 168.
    let rmaos = png(
        d.path(),
        "maps/iron_rmaos.png",
        64,
        64,
        [128, 255, 255, 255],
    );
    let dj = derive(
        DeriveKind::LegacyN,
        false,
        None,
        Some(&normal),
        &rmaos,
        LegacyParams::default(),
    );
    let p = plan(d.path(), dj, "iron_n.dds");
    assert_eq!(p.jobs[0].profile.alpha.policy, AlphaPolicy::Data);
    let (s, ev) = run(p);
    assert_eq!(s.converted, 1, "{ev:#?}");
    let (fmt, _, _, px) = mip0(&d.path().join("out/iron_n.dds"));
    assert_eq!(fmt, DdsPixelFormat::Bc7 { srgb: false });
    for q in &px {
        near(q[0], 128, 2, "R");
        near(q[1], 128, 2, "G");
        near(q[2], 255, 2, "B");
        near(q[3], 168, 2, "spec");
    }

    // Landscape: rough 0.9, AO 0.5, metal 0 → 2.5 · 0.66 · 0.1 · 0.5 = 0.0825 → 21 (O5).
    let d = tempfile::tempdir().unwrap();
    let normal = png(d.path(), "in/dirt_n.png", 64, 64, [128, 128, 255, 255]);
    let rmaos = png(d.path(), "maps/dirt_rmaos.png", 64, 64, [230, 0, 128, 255]);
    let dj = derive(
        DeriveKind::LegacyN,
        true,
        None,
        Some(&normal),
        &rmaos,
        LegacyParams::default(),
    );
    let (s, ev) = run(plan(d.path(), dj, "dirt_n.dds"));
    assert_eq!(s.converted, 1, "{ev:#?}");
    let (fmt, _, _, px) = mip0(&d.path().join("out/dirt_n.dds"));
    assert_eq!(fmt, DdsPixelFormat::Bc7 { srgb: false });
    for q in &px {
        near(q[3], 21, 2, "landscape spec");
    }
}

#[test]
fn a_grass_stack_is_twice_as_tall_and_passes_the_grass_corner_test() {
    let stack = |keep: bool| {
        let d = tempfile::tempdir().unwrap();
        let albedo = png(
            d.path(),
            "in/grass_albedo.png",
            64,
            64,
            [200, 200, 200, 255],
        );
        let normal = png(d.path(), "maps/grass_n.png", 64, 64, [128, 128, 255, 255]);
        // Full metal, rough 0.5, AO 1.
        let rmaos = png(
            d.path(),
            "maps/grass_rmaos.png",
            64,
            64,
            [128, 255, 255, 255],
        );
        let params = LegacyParams {
            keep_metal_brightness: keep,
            keep_metal_strength: 1.0,
            ..LegacyParams::default()
        };
        let dj = derive(
            DeriveKind::GrassStack,
            false,
            Some(&albedo),
            Some(&normal),
            &rmaos,
            params,
        );
        let mut p = plan(d.path(), dj, "grass.dds");
        // As planning resolves complex grass (X12); the base `remove` would drop the bottom half's spec alpha too.
        p.jobs[0].profile.alpha.policy = AlphaPolicy::Cutout;
        let (s, ev) = run(p);
        assert_eq!(s.converted, 1, "{ev:#?}");
        assert!(
            !format!("{ev:?}").contains("VAL-GRASS-DETECT"),
            "grass corner test failed: {ev:#?}"
        );
        mip0(&d.path().join("out/grass.dds"))
    };
    let (_, w, h, px) = stack(false);
    assert_eq!((w, h), (64, 128));
    // Top: the vanilla diffuse, 200 · 0.9 · 0.3 (full metal) = 54. Bottom: the normal, spec 0.66 → 168.
    for q in &px[..64 * 64] {
        near(q[0], 54, 2, "top, vanilla flavour");
        assert_eq!(q[3], 255);
    }
    for q in &px[64 * 64..] {
        near(q[0], 128, 2, "bottom R");
        near(q[2], 255, 2, "bottom B");
        near(q[3], 168, 2, "bottom spec");
    }
    // Keep metal brightness at strength 1: not darkened, 200 · 0.9 = 180.
    let (_, _, _, px) = stack(true);
    for q in &px[..64 * 64] {
        near(q[0], 180, 2, "top, keep-metal flavour");
    }
}

#[test]
fn a_second_run_is_up_to_date_and_a_changed_setting_re_encodes() {
    let d = tempfile::tempdir().unwrap();
    let albedo = png(d.path(), "in/iron_albedo.png", 64, 64, [200, 200, 200, 255]);
    let rmaos = png(d.path(), "maps/iron_rmaos.png", 64, 64, [128, 0, 128, 255]);
    let job = |k: f64| {
        let params = LegacyParams {
            brightness: k,
            ..LegacyParams::default()
        };
        derive(
            DeriveKind::VanillaDiffuse,
            false,
            Some(&albedo),
            None,
            &rmaos,
            params,
        )
    };
    assert_ne!(job(0.9).identity_sha256(), job(0.65).identity_sha256());
    assert_eq!(run(plan(d.path(), job(0.9), "iron.dds")).0.converted, 1);
    let (s, _) = run(plan(d.path(), job(0.9), "iron.dds"));
    assert_eq!((s.converted, s.up_to_date), (0, 1));
    let (s, _) = run(plan(d.path(), job(0.65), "iron.dds"));
    assert_eq!((s.converted, s.up_to_date), (1, 0), "a changed setting");
    // New bytes in the `_rmaos` (its SHA-256, not its time, decides).
    png(d.path(), "maps/iron_rmaos.png", 64, 64, [64, 0, 128, 255]);
    let (s, _) = run(plan(d.path(), job(0.65), "iron.dds"));
    assert_eq!((s.converted, s.up_to_date), (1, 0), "a changed _rmaos");
    let (s, _) = run(plan(d.path(), job(0.65), "iron.dds"));
    assert_eq!((s.converted, s.up_to_date), (0, 1));
}

#[test]
fn a_source_changed_after_planning_fails_the_derived_job_by_name() {
    let d = tempfile::tempdir().unwrap();
    let albedo = png(d.path(), "in/iron_albedo.png", 64, 64, [200, 200, 200, 255]);
    let rmaos = png(d.path(), "maps/iron_rmaos.png", 64, 64, [128, 0, 128, 255]);
    let dj = derive(
        DeriveKind::VanillaDiffuse,
        false,
        Some(&albedo),
        None,
        &rmaos,
        LegacyParams::default(),
    );
    let p = plan(d.path(), dj, "iron.dds");
    // A different size, so the stamp differs whatever the file time resolution.
    png(d.path(), "maps/iron_rmaos.png", 32, 32, [1, 2, 3, 255]);
    let (s, ev) = run(p);
    assert_eq!(s.failed, 1, "{ev:#?}");
    let err = ev
        .iter()
        .find_map(|e| match &e.kind {
            EventKind::FileFailed { error, .. } => Some(error.clone()),
            _ => None,
        })
        .unwrap();
    assert!(
        err.contains("iron_rmaos.png: source changed since planning"),
        "{err}"
    );
    assert!(!d.path().join("out/iron.dds").exists());
}

#[test]
fn a_derive_free_job_serialises_without_the_field() {
    let d = tempfile::tempdir().unwrap();
    let albedo = png(d.path(), "in/iron_albedo.png", 16, 16, [200, 200, 200, 255]);
    let req = BatchRequest::new(vec![d.path().join("in")], d.path().join("out"), Target::Cs);
    let plan = Engine::new()
        .unwrap()
        .plan(
            req,
            &Reporter::new(Arc::new(NullSink), "t"),
            &CancelToken::default(),
        )
        .unwrap();
    let job = serde_json::to_value(&plan.jobs[0]).unwrap();
    assert!(job.get("derive").is_none(), "{job}");
    let r = serde_json::to_value(SourceRef::of(&albedo).unwrap()).unwrap();
    assert!(r.get("derive").is_none() && r.get("derive_role").is_none());
}
