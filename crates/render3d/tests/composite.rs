//! Deferred composite (v2 Phase D, D4): after the opaque DEFERRED draws, CS's DeferredCompositeCS runs into RT0
//! (MainRW), then blended shapes draw forward on top. The stand-in composite adds Specular x NormalRoughness.x under
//! STANDIN_COMPOSITE_ADD and is a passthrough otherwise.
mod common;
use common::{cache_files, plane_shape, renderer, renderer_at, standin_root, FLAT, IDENT};
use nif::props::Alpha;
use render3d::composite::Composite;
use render3d::scene::Scene;
use render3d::{EditKind, Renderer, ShapeId};

/// The stand-in's RT4 (Specular) marker; its RT2.x (NormalRoughness) is 1.
const M4: [f32; 3] = [0.5, 0.25, 0.125];
const ADD: &str = "STANDIN_COMPOSITE_ADD";

fn feats(extra: &[&'static str]) -> Vec<&'static str> {
    FLAT.iter().copied().chain(extra.iter().copied()).collect()
}

fn px(v: &[f32], w: usize, x: usize, y: usize) -> [f32; 4] {
    v[(y * w + x) * 4..][..4].try_into().unwrap()
}

fn bits(p: [f32; 4]) -> [u32; 4] {
    p.map(f32::to_bits)
}

/// `base + M4` as RT0 (R16G16B16A16_FLOAT) stores it, alpha 1.
fn plus_m4(base: [f32; 4]) -> [f32; 4] {
    let f = |a: f32, b: f32| half::f16::from_f32(a + b).to_f32();
    [f(base[0], M4[0]), f(base[1], M4[1]), f(base[2], M4[2]), 1.0]
}

fn loaded(
    cache: &std::path::Path,
    features: &[&str],
    shapes: Vec<render3d::ParsedShape>,
) -> Renderer {
    let mut r = renderer(cache, features);
    load(&mut r, shapes);
    r
}

fn load(r: &mut Renderer, shapes: Vec<render3d::ParsedShape>) {
    let parsed = render3d::ParsedNif {
        shapes,
        ..Default::default()
    };
    let rep = r.load_parsed(&parsed, &|_| None);
    assert!(
        rep.errors.is_empty() && rep.compile_errors.is_empty(),
        "{rep:?}"
    );
}

fn opaque() -> render3d::ParsedShape {
    plane_shape(0, IDENT, [0.25, 0.125, 0.0], 1)
}

fn blended(id: u32) -> render3d::ParsedShape {
    let mut s = plane_shape(id, IDENT, [0.0, 0.0, 1.0], 1);
    s.material.alpha = Some(Alpha {
        flags: 0x00ED,
        threshold: 0,
    });
    s
}

fn has_deferred(r: &mut Renderer, id: u32) -> bool {
    r.draw_diag(ShapeId(id))
        .unwrap()
        .0
        .iter()
        .any(|(k, _)| k == "DEFERRED")
}

#[test]
fn composite_runs_on_warp_with_stub_inputs() {
    let gpu = render3d::device::Gpu::new_warp().unwrap();
    assert!(
        Composite::typed_uav_load(&gpu),
        "WARP reads RGBA16F through a UAV"
    );
    let cache = tempfile::tempdir().unwrap();
    let mut r = loaded(cache.path(), &feats(&[ADD]), vec![opaque()]);
    r.render(&Scene::neutral(), 64, 64).unwrap();
    let fwd = r.read_mrt(0).unwrap();
    let (base, corner) = (px(&fwd, 64, 32, 32), px(&fwd, 64, 0, 0));
    assert_ne!(base, corner, "the plane covers the centre");
    r.set_deferred(true).unwrap();
    assert_eq!(r.deferred_error(), None);
    r.render(&Scene::neutral(), 64, 64).unwrap();
    let rt0 = r.read_mrt(0).unwrap();
    // b5 BufferDim written (else every thread exits), t0/t2 bound, t4 unbound reads 0.
    assert_eq!(px(&rt0, 64, 32, 32), plus_m4(base));
    assert_eq!(
        bits(px(&rt0, 64, 0, 0)),
        bits(corner),
        "no deferred draw: unchanged"
    );
    assert_eq!(r.composite_builds(), 1);
    assert_eq!(r.bound_cs_views(), 0, "the dispatch unbinds what it bound");
}

/// Opaque o (deferred) and blended b (forward, alpha 0.5: the stand-in returns base.a x 0.5) overlap at the centre:
/// the composite adds s to o before b blends, so D = lerp(o + s, b, a) = F + (1 - a)s, F the forward frame.
#[test]
fn blended_shapes_draw_forward_after_the_composite() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = loaded(cache.path(), &feats(&[ADD]), vec![opaque(), blended(1)]);
    r.render(&Scene::neutral(), 64, 64).unwrap();
    let fwd = r.read_mrt(0).unwrap();
    r.set_deferred(true).unwrap();
    r.render(&Scene::neutral(), 64, 64).unwrap();
    assert!(!has_deferred(&mut r, 1), "blended stays forward");
    let d = r.read_mrt(0).unwrap();
    let (f, got) = (px(&fwd, 64, 32, 32), px(&d, 64, 32, 32));
    let a = 0.5;
    for k in 0..3 {
        let want = f[k] + (1.0 - a) * M4[k];
        assert!((got[k] - want).abs() <= 2e-3, "ch {k}: {got:?} vs {want}");
    }
    let after = (0..3)
        .map(|k| (got[k] - (f[k] + M4[k])).abs())
        .fold(0.0, f32::max);
    assert!(after > 0.02, "composited after the blend: {got:?} vs {f:?}");
    // Where no DEFERRED draw wrote (RT7 = 0), the frame is the forward one.
    let rt7 = r.read_mrt(7).unwrap();
    for p in 0..64 * 64 {
        if rt7[p * 4] == 0.0 {
            assert_eq!(
                bits(px(&d, 64, p % 64, p / 64)),
                bits(px(&fwd, 64, p % 64, p / 64)),
                "pixel {p}"
            );
        }
    }
}

/// A stand-in copy whose composite is `composite` (None = no file).
fn shaders_with(composite: Option<&str>) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::copy(
        standin_root().join("Lighting.hlsl"),
        d.path().join("Lighting.hlsl"),
    )
    .unwrap();
    if let Some(c) = composite {
        std::fs::write(d.path().join("DeferredCompositeCS.hlsl"), c).unwrap();
    }
    d
}

#[test]
fn composite_compile_failure_falls_back_to_forward() {
    const P: &str = "Deferred preview unavailable: ";
    // (i) the composite does not compile.
    let shaders = shaders_with(Some("this is not hlsl"));
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer_at(shaders.path(), cache.path(), FLAT);
    load(&mut r, vec![opaque()]);
    let a = r.render(&Scene::neutral(), 64, 64).unwrap();
    assert!(a.chunks(4).any(|p| p != &a[..4]), "the plane shows");
    let e = r.set_deferred(true).unwrap_err();
    assert!(e.starts_with(P) && e.len() > P.len(), "{e}");
    assert!(!r.deferred());
    assert_eq!(r.deferred_error(), Some(e.as_str()));
    assert_eq!(r.render(&Scene::neutral(), 64, 64).unwrap(), a);

    // (ii) no composite file.
    let shaders = shaders_with(None);
    let mut r = renderer_at(shaders.path(), cache.path(), FLAT);
    load(&mut r, vec![opaque()]);
    let e = r.set_deferred(true).unwrap_err();
    assert!(
        e.starts_with(P) && e.contains("DeferredCompositeCS.hlsl"),
        "{e}"
    );
    assert!(!r.deferred());

    // (iii) late: the composite breaks after deferred mode is on; render falls back, never Err.
    let mut r = loaded(cache.path(), &feats(&[ADD]), vec![opaque()]);
    r.set_deferred(true).unwrap();
    r.render(&Scene::neutral(), 64, 64).unwrap();
    let broken = feats(&[ADD, "STANDIN_COMPOSITE_BROKEN"]);
    r.set_features(broken.iter().map(|s| s.to_string()).collect());
    let f = r.render(&Scene::neutral(), 64, 64).unwrap();
    assert!(!r.deferred());
    let e = r.deferred_error().unwrap();
    assert!(
        e.starts_with(P) && e.contains("STANDIN_COMPOSITE_BROKEN"),
        "{e}"
    );
    assert_eq!(r.target_dims().len(), 2, "forward target");
    let fresh = loaded(cache.path(), &broken, vec![opaque()]).render(&Scene::neutral(), 64, 64);
    assert_eq!(f, fresh.unwrap());
}

#[test]
fn cbuffer_edit_in_deferred_mode_compiles_nothing() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = loaded(cache.path(), &feats(&[ADD]), vec![opaque()]);
    r.set_deferred(true).unwrap();
    r.render(&Scene::neutral(), 64, 64).unwrap();
    let files = cache_files(cache.path());
    let mut edited = opaque();
    edited.material.params.emissive_color = [0.0, 0.25, 0.5];
    assert_eq!(
        r.set_material(ShapeId(0), edited.material.clone(), &|_| None),
        Ok(EditKind::Cbuffer)
    );
    for _ in 0..3 {
        r.render(&Scene::neutral(), 64, 64).unwrap();
    }
    assert_eq!(cache_files(cache.path()), files);
    assert_eq!(r.composite_builds(), 1);
    let mut fwd = loaded(cache.path(), &feats(&[ADD]), vec![edited]);
    fwd.render(&Scene::neutral(), 64, 64).unwrap();
    let new = px(&fwd.read_mrt(0).unwrap(), 64, 32, 32);
    assert_eq!(px(&r.read_mrt(0).unwrap(), 64, 32, 32), plus_m4(new));
}

/// Opt-in step-1 record: the installed composite compiles with CSHADER + ENABLE_LL; prints its reflection.
/// `SDC_CS_SHADERS=…\Shaders cargo test -p render3d --test composite -- --ignored --nocapture`
#[test]
#[ignore]
fn installed_composite_reflects() {
    let Ok(shaders) = std::env::var("SDC_CS_SHADERS") else {
        println!("skipped: set SDC_CS_SHADERS");
        return;
    };
    let gpu = render3d::device::Gpu::new().unwrap();
    println!(
        "adapter {} typed load {}",
        gpu.adapter,
        Composite::typed_uav_load(&gpu)
    );
    let src = render3d::source::SourceSet::new(shaders.into());
    let cache = tempfile::tempdir().unwrap();
    let t = std::time::Instant::now();
    let c = Composite::new(
        &gpu,
        &src,
        &["CSHADER".into(), "ENABLE_LL".into()],
        cache.path(),
    )
    .unwrap();
    println!("built in {:?}", t.elapsed());
    let r = &c.reflection;
    println!("srvs {:?}", r.srvs);
    println!("uavs {:?}", r.uavs);
    println!(
        "cbuffers {:?}",
        r.cbuffers
            .iter()
            .map(|c| (&c.name, c.slot, c.size))
            .collect::<Vec<_>>()
    );
    println!("threads {:?}; unbound {:?}", r.threads, c.unbound);
    let srvs: Vec<(&str, u32)> = r.srvs.iter().map(|s| (s.0.as_str(), s.1)).collect();
    assert_eq!(
        srvs,
        [
            ("SpecularTexture", 0),
            ("NormalRoughnessTexture", 2),
            ("DepthTexture", 4)
        ]
    );
    assert_eq!(r.threads, [8, 8, 1]);
}

/// D-P12: a late composite failure with async compilation on still gives a complete forward frame at once: the
/// forward programs are rebuilt synchronously, and no DEFERRED program (tinted here) draws into the forward target.
#[test]
fn late_composite_failure_in_async_mode_draws_forward_at_once() {
    let cache = tempfile::tempdir().unwrap();
    let tint = feats(&[ADD, "STANDIN_DEFERRED_TINT"]);
    let mut r = loaded(cache.path(), &tint, vec![opaque()]);
    r.set_deferred(true).unwrap();
    r.render(&Scene::neutral(), 64, 64).unwrap();
    r.set_async_compile(true);
    let broken = feats(&[ADD, "STANDIN_DEFERRED_TINT", "STANDIN_COMPOSITE_BROKEN"]);
    r.set_features(broken.iter().map(|s| s.to_string()).collect());
    let f = r.render(&Scene::neutral(), 64, 64).unwrap();
    assert!(!r.deferred());
    assert!(r.deferred_error().is_some());
    let fresh = loaded(cache.path(), &broken, vec![opaque()]).render(&Scene::neutral(), 64, 64);
    let fresh = fresh.unwrap();
    let diff = f
        .chunks(4)
        .zip(fresh.chunks(4))
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(diff, 0, "pixels that differ from a fresh forward frame");
}

/// D-P13: toggling deferred off in async mode keeps the deferred frame (target, composite) while any draw still holds
/// a DEFERRED program, so opaque shapes never vanish; the frame goes forward once every program has landed.
#[test]
fn toggle_off_in_async_mode_keeps_the_deferred_frame_until_forward_lands() {
    let cache = tempfile::tempdir().unwrap();
    let tint = feats(&[ADD, "STANDIN_DEFERRED_TINT"]);
    // Deferred before the load: only DEFERRED programs exist, so the forward ones must compile after the toggle.
    let mut r = renderer(cache.path(), &tint);
    r.set_deferred(true).unwrap();
    load(&mut r, vec![opaque()]);
    let d = r.render(&Scene::neutral(), 64, 64).unwrap();
    r.set_async_compile(true);
    r.set_deferred(false).unwrap();
    assert_eq!(r.render(&Scene::neutral(), 64, 64).unwrap(), d);
    assert_eq!(r.target_dims().len(), 10);
    for j in r.take_jobs() {
        r.apply_compiled(render3d::run_job(j));
    }
    let f = r.render(&Scene::neutral(), 64, 64).unwrap();
    assert_eq!(r.target_dims().len(), 2);
    let fresh = loaded(cache.path(), &tint, vec![opaque()]).render(&Scene::neutral(), 64, 64);
    assert!(
        f == fresh.unwrap(),
        "the forward frame after the programs land"
    );
    assert!(
        f != d,
        "the tint shows the deferred frame was the deferred one"
    );
}
