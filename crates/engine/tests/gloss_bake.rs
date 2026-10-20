//! Track C Task 9 (DECISIONS Note CL40): an explicit `mips.normal = renormalize_bake` on a set's `normal` bakes its
//! footprint variance into the set's complex material `_m` gloss (G); off by default, the `_n` itself unchanged.
use encoders_sys::Bc1Model;
use engine::api::*;
use engine::codes::Code;
use engine::config::{NormalMips, Subtype, Target};
use engine::events::*;
use engine::image::Rgba32F;
use engine::manifest::profile_sha256;
use engine::overrides::parse_set;
use engine::policy::{AcceptSuggestions, Provenance, ResolvedJob};
use engine::session::Session;
use image::{DynamicImage, ImageBuffer, Rgb};
use std::path::Path;
use std::sync::Arc;

const CM: &str = "file:**/wall_m.png.subtype=complex_material";
const OPT_IN: &str = "file:**/wall_n.png.mips.normal=renormalize_bake";
const OPT_OUT: &str = "file:**/wall_n.png.mips.normal=renormalize";
/// Keeps the 1024² encodes of the size-mismatch case quick; applies to every run compared.
const FAST: &str = "encoder.effort=ultrafast";
/// G of the `_m`: 0.8.
const GLOSS: u8 = 204;

fn request(dir: &Path, target: Target, sets: &[&str], allow: &[Code]) -> BatchRequest {
    let mut req = BatchRequest::new(vec![dir.join("in")], dir.join("out"), target);
    req.recursive = true;
    req.accept = AcceptSuggestions::High;
    req.overrides = sets.iter().map(|s| parse_set(s).unwrap()).collect();
    req.allow = allow.iter().copied().collect();
    req.on_existing = OnExisting::Overwrite;
    req
}

fn plan(req: BatchRequest) -> Plan {
    let rep = Reporter::new(Arc::new(NullSink), "gloss");
    Engine::new()
        .unwrap()
        .plan(req, &rep, &CancelToken::default())
        .unwrap()
}

/// Plan and run; the events.
fn convert(req: BatchRequest) -> Vec<EngineEvent> {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "gloss");
    let e = Engine::new().unwrap();
    let plan = e.plan(req, &rep, &CancelToken::default()).unwrap();
    let summary = e.run(plan, &rep, &CancelToken::default()).unwrap();
    assert_eq!(summary.failed, 0, "{summary:?}");
    sink.take()
}

fn job<'a>(p: &'a Plan, name: &str) -> &'a ResolvedJob {
    p.jobs
        .iter()
        .find(|j| j.input.ends_with(name))
        .unwrap_or_else(|| panic!("no job for {name}"))
}

fn put(dir: &Path, name: &str, img: DynamicImage) {
    let p = dir.join("in/textures/a").join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    img.save(p).unwrap();
}

fn out(dir: &Path, name: &str) -> Vec<u8> {
    std::fs::read(dir.join("out/textures/a").join(name)).unwrap()
}

/// A tangent-space normal: bumpy where `bumpy(x)`, flat +Z elsewhere.
fn normal(w: u32, h: u32, bumpy: impl Fn(u32) -> bool) -> DynamicImage {
    DynamicImage::ImageRgb8(ImageBuffer::from_fn(w, h, |x, y| {
        if !bumpy(x) {
            return Rgb([128, 128, 255]);
        }
        let nx = 0.6 * (x as f32 * 1.3 + y as f32 * 0.7).sin();
        let ny = 0.6 * (x as f32 * 0.9 - y as f32 * 1.1).cos();
        let nz = (1.0 - nx * nx - ny * ny).sqrt();
        let e = |v: f32| ((v * 0.5 + 0.5) * 255.0).round() as u8;
        Rgb([e(nx), e(ny), e(nz)])
    }))
}

/// A complex material `_m`: metal 0.25, gloss 0.8, B a ramp.
fn material(w: u32, h: u32) -> DynamicImage {
    DynamicImage::ImageRgb8(ImageBuffer::from_fn(w, h, |x, _| {
        Rgb([64, GLOSS, (x % 200) as u8])
    }))
}

fn diffuse(w: u32) -> DynamicImage {
    DynamicImage::ImageRgb8(ImageBuffer::from_fn(w, w, |x, y| {
        Rgb([(x * 3) as u8, (y * 3) as u8, 90])
    }))
}

/// wall.png + wall_n.png + wall_m.png (complex material), 64².
fn wall_set(dir: &Path, bumpy: impl Fn(u32) -> bool) {
    put(dir, "wall.png", diffuse(64));
    put(dir, "wall_n.png", normal(64, 64, bumpy));
    put(dir, "wall_m.png", material(64, 64));
}

fn chain(p: &Plan, name: &str) -> Vec<Rgba32F> {
    let s = Session::new(Bc1Model::Ideal).unwrap();
    let c = s
        .preview_source(job(p, name), 16384, &CancelToken::default())
        .unwrap();
    assert_eq!(c.first_level, 0);
    c.levels
}

#[test]
fn without_the_opt_in_a_complex_set_is_byte_identical() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    wall_set(a.path(), |_| true);
    wall_set(b.path(), |_| true);
    let pa = plan(request(a.path(), Target::Vanilla, &[CM], &[]));
    assert_eq!(
        job(&pa, "wall_m.png").profile.subtype,
        Subtype::ComplexMaterial
    );
    assert_eq!(job(&pa, "wall_n.png").profile.subtype, Subtype::Normal);
    assert!(job(&pa, "wall_m.png").bake.is_none());
    convert(request(a.path(), Target::Vanilla, &[CM], &[]));
    convert(request(b.path(), Target::Vanilla, &[CM, OPT_OUT], &[]));
    for f in ["wall.dds", "wall_n.dds", "wall_m.dds"] {
        assert!(out(a.path(), f) == out(b.path(), f), "{f} differs");
    }
}

#[test]
fn opting_in_lowers_gloss_where_the_normal_is_bumpy_and_keeps_it_where_flat() {
    let d = tempfile::tempdir().unwrap();
    // Left half bumpy, right half flat; the boundary is on a power-of-two column.
    wall_set(d.path(), |x| x < 32);
    let plain = chain(
        &plan(request(d.path(), Target::Vanilla, &[CM], &[])),
        "wall_m.png",
    );
    let p = plan(request(d.path(), Target::Vanilla, &[CM, OPT_IN], &[]));
    let m = job(&p, "wall_m.png");
    let b = m.bake.as_ref().expect("the _m bakes its set's _n");
    assert!(b.normal.ends_with("wall_n.png") && b.shift == 0);
    // The `_n` keeps the user's value and provenance (nothing is rewritten as `set`).
    let n = job(&p, "wall_n.png");
    assert_eq!(n.profile.mips.normal, NormalMips::RenormalizeBake);
    assert_ne!(n.provenance.get("mips.normal"), Some(&Provenance::Set));
    let baked = chain(&p, "wall_m.png");
    assert_eq!(baked.len(), plain.len());
    let mean_g = |l: &Rgba32F, xs: std::ops::Range<u32>| {
        let mut s = 0.0f64;
        let mut k = 0;
        for y in 0..l.height {
            for x in xs.clone() {
                s += f64::from(l.at(x, y)[1]);
                k += 1;
            }
        }
        s / f64::from(k)
    };
    let (u, v) = (&plain[2], &baked[2]);
    let half = u.width / 2;
    assert!(
        mean_g(v, 0..half) < mean_g(u, 0..half) - 5.0 / 255.0,
        "bumpy half: baked {} vs plain {}",
        mean_g(v, 0..half),
        mean_g(u, 0..half)
    );
    // Flat half: bit for bit on every level whose texels lie in one half (the 1×1 level spans both).
    for (level, (u, v)) in plain.iter().zip(&baked).enumerate() {
        if u.width < 2 {
            continue;
        }
        for y in 0..u.height {
            for x in u.width / 2..u.width {
                assert_eq!(
                    u.at(x, y),
                    v.at(x, y),
                    "flat half changed at mip {level} ({x}, {y})"
                );
            }
        }
    }
    // Every other channel is untouched.
    for (u, v) in plain.iter().zip(&baked) {
        for (a, b) in u.px.iter().zip(&v.px) {
            assert_eq!((a[0], a[2], a[3]), (b[0], b[2], b[3]));
        }
    }
}

#[test]
fn the_normal_itself_is_unchanged_by_the_opt_in() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    wall_set(a.path(), |_| true);
    wall_set(b.path(), |_| true);
    let hash = |dir: &Path, sets: &[&str]| {
        profile_sha256(
            &job(
                &plan(request(dir, Target::Vanilla, sets, &[])),
                "wall_n.png",
            )
            .profile,
        )
        .unwrap()
    };
    assert_eq!(hash(a.path(), &[CM]), hash(b.path(), &[CM, OPT_IN]));
    convert(request(a.path(), Target::Vanilla, &[CM], &[]));
    convert(request(b.path(), Target::Vanilla, &[CM, OPT_IN], &[]));
    assert!(out(a.path(), "wall_n.dds") == out(b.path(), "wall_n.dds"));
    // The opt-in run did bake the _m.
    assert!(out(a.path(), "wall_m.dds") != out(b.path(), "wall_m.dds"));
}

#[test]
fn a_size_mismatch_reports_set_bake_ratio_and_does_not_bake() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    for d in [a.path(), b.path()] {
        put(d, "wall_n.png", normal(1024, 1024, |_| true));
        put(d, "wall_m.png", material(768, 768));
    }
    let allow = [Code::DimNpot];
    let p = plan(request(
        b.path(),
        Target::Vanilla,
        &[CM, OPT_IN, FAST],
        &allow,
    ));
    let m = job(&p, "wall_m.png");
    assert!(m.bake.is_none());
    assert!(
        m.warnings.iter().any(|w| w.code == Code::SetBakeRatio),
        "{:?}",
        m.warnings
    );
    convert(request(a.path(), Target::Vanilla, &[CM, FAST], &allow));
    convert(request(
        b.path(),
        Target::Vanilla,
        &[CM, OPT_IN, FAST],
        &allow,
    ));
    assert!(out(a.path(), "wall_m.dds") == out(b.path(), "wall_m.dds"));
}

#[test]
fn a_pbr_set_still_bakes_roughness_only() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "iron.png", diffuse(64));
    put(d.path(), "iron_n.png", normal(64, 64, |_| true));
    put(
        d.path(),
        "iron_rmaos.png",
        DynamicImage::ImageRgb8(ImageBuffer::from_fn(64, 64, |_, _| Rgb([100, 0, 255]))),
    );
    let p = plan(request(d.path(), Target::Cs, &[], &[]));
    let n = job(&p, "iron_n.png");
    assert_eq!(n.profile.subtype, Subtype::PbrNormal);
    assert_eq!(n.profile.mips.normal, NormalMips::RenormalizeBake);
    assert_eq!(n.provenance.get("mips.normal"), Some(&Provenance::Set));
    assert!(job(&p, "iron_rmaos.png").bake.is_some());
    for j in p
        .jobs
        .iter()
        .filter(|j| !j.input.ends_with("iron_rmaos.png"))
    {
        assert!(j.bake.is_none(), "{} bakes", j.input.display());
    }
}
