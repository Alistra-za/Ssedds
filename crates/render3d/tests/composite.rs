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
    // b5 BufferDim written (else every thread exits), t0/t2 bound; t4 (the deferred depth) is bound but reaches only
    // u1 without STANDIN_COMPOSITE_DEPTH, as CS reads depth outside DYNAMIC_CUBEMAPS only for MotionVectorsRW.
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

/// D5 step 0: the installed composite with the A_DC defines reflects exactly what frame246061's composite.json binds
/// (t0, t2, t4 depth, t5, t7 cube; t6 EnvTexture is stripped) and one sampler, s0 LinearSampler.
#[test]
#[ignore]
fn installed_dc_composite_reflects() {
    let Ok(shaders) = std::env::var("SDC_CS_SHADERS") else {
        println!("skipped: set SDC_CS_SHADERS");
        return;
    };
    use render3d::compile::SrvKind;
    let gpu = render3d::device::Gpu::new().unwrap();
    let src = render3d::source::SourceSet::new(shaders.into());
    let cache = tempfile::tempdir().unwrap();
    let defines = [
        "CSHADER",
        "DYNAMIC_CUBEMAPS",
        "ENABLE_LL",
        "CS_GAME_GAMMA=1.600000024",
    ]
    .map(String::from);
    let c = Composite::new(&gpu, &src, &defines, cache.path()).unwrap();
    let r = &c.reflection;
    println!("srvs {:?}\nsamplers {:?}", r.srvs, r.samplers);
    let srvs: Vec<(&str, u32, SrvKind)> = r.srvs.iter().map(|s| (s.0.as_str(), s.1, s.2)).collect();
    assert_eq!(
        srvs,
        [
            ("SpecularTexture", 0, SrvKind::Tex2D),
            ("NormalRoughnessTexture", 2, SrvKind::Tex2D),
            ("DepthTexture", 4, SrvKind::Tex2D),
            ("ReflectanceTexture", 5, SrvKind::Tex2D),
            ("EnvReflectionsTexture", 7, SrvKind::Cube),
        ]
    );
    assert_eq!(r.samplers, [("LinearSampler".to_string(), 0)]);
    assert!(c.needs_env());
    if let Ok(root) = std::env::var("SDC_GOLDEN") {
        let p = std::path::Path::new(&root)
            .join("Skyrim_AE_1.6.1170_frame246061/composite/composite.json");
        let j: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        let bound: Vec<(String, u32)> = j["srvs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| {
                (
                    s["name"].as_str().unwrap().into(),
                    s["bind"].as_u64().unwrap() as u32,
                )
            })
            .collect();
        let refl: Vec<(String, u32)> = r.srvs.iter().map(|s| (s.0.clone(), s.1)).collect();
        assert_eq!(refl, bound, "frame246061 composite.json");
    }
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

/// D-P36 (F4): toggled off in async mode, a shape whose forward compile fails keeps only its DEFERRED draw, which a
/// forward frame never draws: it must not report that its previous draw is on screen.
#[test]
fn a_dropped_deferred_draw_does_not_report_showing_previous() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), &feats(&["STANDIN_FORWARD_BROKEN"]));
    r.set_deferred(true).unwrap();
    load(&mut r, vec![opaque()]);
    r.render(&Scene::neutral(), 64, 64).unwrap();
    r.set_async_compile(true);
    r.set_deferred(false).unwrap();
    for j in r.take_jobs() {
        r.apply_compiled(render3d::run_job(j));
    }
    r.render(&Scene::neutral(), 64, 64).unwrap();
    assert_eq!(r.target_dims().len(), 2, "the frame is forward");
    match &r.shapes()[0].status {
        render3d::ShapeStatus::Failed {
            showing_previous, ..
        } => assert!(!showing_previous, "its DEFERRED draw is not on screen"),
        s => panic!("the forward compile fails: {s:?}"),
    }
}

/// D-P14: toggled off in async mode, then a define change before the forward programs land: the composite is stale,
/// so the forward programs are built at once with the new defines and the frame is complete and forward.
#[test]
fn define_change_during_async_toggle_off_settles_forward_at_once() {
    let cache = tempfile::tempdir().unwrap();
    let tint = feats(&[ADD, "STANDIN_DEFERRED_TINT"]);
    let mut r = renderer(cache.path(), &tint);
    r.set_deferred(true).unwrap();
    load(&mut r, vec![opaque()]);
    r.render(&Scene::neutral(), 64, 64).unwrap();
    r.set_async_compile(true);
    r.set_deferred(false).unwrap();
    let changed = feats(&["STANDIN_DEFERRED_TINT"]);
    r.set_features(changed.iter().map(|s| s.to_string()).collect());
    let f = r.render(&Scene::neutral(), 64, 64).unwrap();
    assert_eq!(r.target_dims().len(), 2);
    let fresh = loaded(cache.path(), &changed, vec![opaque()]).render(&Scene::neutral(), 64, 64);
    let fresh = fresh.unwrap();
    let diff = f
        .chunks(4)
        .zip(fresh.chunks(4))
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(diff, 0, "pixels that differ from a fresh forward frame");
}

const DEPTH: &str = "STANDIN_COMPOSITE_DEPTH";
const DC: &str = "DYNAMIC_CUBEMAPS";
/// The stand-in's RT5 (Reflectance) marker.
const M5: [f32; 3] = [0.25, 0.125, 0.5];

/// The 1×1 mip of face +Z of the studio env cube at yaw 0, as the GPU stores it (RGBA16F).
fn env_last_z() -> [f32; 4] {
    let faces = render3d::hdri::cube_faces(&render3d::hdri::studio().image, 256, 0.0);
    for (k, f) in faces.iter().enumerate() {
        let c = render3d::hdri::mip_chain(f, 256).unwrap();
        eprintln!("DBG face {k}: {:?}", c.last().unwrap()[0]);
    }
    let chain = render3d::hdri::mip_chain(&faces[4], 256).unwrap();
    chain.last().unwrap()[0].map(|v| half::f16::from_f32(v).to_f32())
}

fn f16(v: f32) -> f32 {
    half::f16::from_f32(v).to_f32()
}

/// The deferred depth is bound to DepthTexture: with STANDIN_COMPOSITE_DEPTH a pixel no draw covered (cleared depth
/// 1.0) gains exactly 1.
#[test]
fn composite_reads_the_bound_depth() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = loaded(cache.path(), &feats(&[DEPTH]), vec![opaque()]);
    r.render(&Scene::neutral(), 64, 64).unwrap();
    let corner = px(&r.read_mrt(0).unwrap(), 64, 0, 0);
    r.set_deferred(true).unwrap();
    r.render(&Scene::neutral(), 64, 64).unwrap();
    let got = px(&r.read_mrt(0).unwrap(), 64, 0, 0);
    let want = [
        f16(corner[0] + 1.0),
        f16(corner[1] + 1.0),
        f16(corner[2] + 1.0),
        1.0,
    ];
    assert_eq!(bits(got), bits(want), "{got:?} vs {want:?}");
    assert_eq!(r.bound_cs_views(), 0);
}

/// DYNAMIC_CUBEMAPS: the composite adds RT5 × the env cube's level-15 +Z sample (the HDRI cube, yaw 0).
#[test]
fn dc_composite_adds_reflectance_times_the_env_cube() {
    let cache = tempfile::tempdir().unwrap();
    let mut fwd = loaded(cache.path(), FLAT, vec![opaque()]);
    fwd.render(&Scene::neutral(), 64, 64).unwrap();
    let base = px(&fwd.read_mrt(0).unwrap(), 64, 32, 32);
    let mut r = loaded(cache.path(), &feats(&[DC]), vec![opaque()]);
    r.set_deferred(true).unwrap();
    assert_eq!(r.deferred_error(), None);
    r.render(&Scene::neutral(), 64, 64).unwrap();
    let e = env_last_z();
    let got = px(&r.read_mrt(0).unwrap(), 64, 32, 32);
    let sums: Vec<f32> = (0..3).map(|k| base[k] + M5[k] * e[k]).collect();
    assert_store(got, [sums[0], sums[1], sums[2], 1.0]);
    assert_eq!(r.bound_cs_views(), 0);
}

/// `got` (an RGBA16F store) equals each exact f32 `sum` rounded to a half, bit for bit: the nearest-even half or the half
/// toward zero. D3D11 lets a float-to-f16 store round either way, and WARP rounds toward zero on any inexact sum, not
/// only on ties (Note DL: the mirrored HDRI's env cube gave non-tie sums). The sums must be exact in f32 (a power of two
/// times an f16 plus an f16).
fn assert_store(got: [f32; 4], sum: [f32; 4]) {
    for k in 0..4 {
        let near = f16(sum[k]);
        let ok = got[k].to_bits() == near.to_bits() || got[k] == f16_toward_zero(sum[k]);
        assert!(ok, "ch {k}: {got:?}, exact {:?} -> {near}", sum);
    }
}

/// `v` (positive) rounded to a half toward zero.
fn f16_toward_zero(v: f32) -> f32 {
    let h = half::f16::from_f32(v);
    if h.to_f32() > v {
        half::f16::from_bits(h.to_bits() - 1).to_f32()
    } else {
        h.to_f32()
    }
}

#[test]
fn a_store_may_round_to_nearest_or_toward_zero_and_nothing_else() {
    // 0.5272827: the Note DL D5 R channel; nearest 0.52734375, toward zero 0.52685547 (WARP).
    let sum = [0.527_282_7, 0.0, 0.0, 1.0];
    assert_store([0.527_343_75, 0.0, 0.0, 1.0], sum);
    assert_store([0.526_855_47, 0.0, 0.0, 1.0], sum);
    // two halves below is neither
    assert!(std::panic::catch_unwind(|| assert_store([0.526_367_2, 0.0, 0.0, 1.0], sum)).is_err());
}

/// Views bound to the compute stage of `gpu` (SRVs 0..20 + UAVs 0..8).
fn cs_views(gpu: &render3d::device::Gpu) -> usize {
    use windows::Win32::Graphics::Direct3D11::{
        ID3D11ShaderResourceView, ID3D11UnorderedAccessView,
    };
    let mut s: [Option<ID3D11ShaderResourceView>; 20] = Default::default();
    let mut u: [Option<ID3D11UnorderedAccessView>; 8] = Default::default();
    unsafe {
        gpu.ctx.CSGetShaderResources(0, Some(&mut s));
        gpu.ctx.CSGetUnorderedAccessViews(0, Some(&mut u));
    }
    s.iter().flatten().count() + u.iter().flatten().count()
}

#[test]
fn composite_without_env_input_fails_closed() {
    use render3d::composite::Inputs;
    use render3d::device::{Gpu, Target};
    let gpu = Gpu::new_warp().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let src = render3d::source::SourceSet::new(standin_root());
    let c = Composite::new(&gpu, &src, &["CSHADER".into(), DC.into()], cache.path()).unwrap();
    assert!(c.needs_env());
    let t = Target::new_deferred(&gpu, 16, 16).unwrap();
    let e = c
        .dispatch(
            &gpu,
            &t,
            &Inputs {
                depth: t.depth_srv.as_ref(),
                env: None,
                sampler: None,
            },
        )
        .unwrap_err();
    assert!(e.contains("EnvReflectionsTexture"), "{e}");
    assert_eq!(cs_views(&gpu), 0);
    let plain = Composite::new(&gpu, &src, &["CSHADER".into()], cache.path()).unwrap();
    assert!(!plain.needs_env());
}

/// Built in `render` only when something reads it (the DC composite or a forward env register), once per yaw.
#[test]
fn env_cube_is_built_once_per_yaw() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = loaded(cache.path(), &feats(&[DC]), vec![opaque()]);
    r.set_deferred(true).unwrap();
    let mut s = Scene::neutral();
    r.render(&s, 64, 64).unwrap();
    r.render(&s, 64, 64).unwrap();
    assert_eq!(r.env_builds(), 1);
    s.env_yaw = 0.5;
    let (_, events) = render3d::perf_log::capture(|| r.render(&s, 64, 64).unwrap());
    assert_eq!(r.env_builds(), 2);
    let ev = events
        .iter()
        .find(|e| e["event"] == "env_cube")
        .unwrap_or_else(|| panic!("no env_cube event in {events:?}"));
    println!("env_cube build {} ms", ev["ms"]);
    let mut plain = loaded(cache.path(), &feats(&[ADD]), vec![opaque()]);
    plain.set_deferred(true).unwrap();
    plain.render(&Scene::neutral(), 64, 64).unwrap();
    assert_eq!(plain.env_builds(), 0);
}

/// R3: a forward DC draw reads the env cube at its EnvReflectionsTexture register (the stand-in adds its level-15 +Z
/// sample under DYNAMIC_CUBEMAPS && STANDIN_FLAT, forward only).
#[test]
fn forward_dc_draw_samples_the_env_cube() {
    let cache = tempfile::tempdir().unwrap();
    let mut plain = loaded(cache.path(), FLAT, vec![opaque()]);
    plain.render(&Scene::neutral(), 64, 64).unwrap();
    let base = px(&plain.read_mrt(0).unwrap(), 64, 32, 32);
    let mut r = loaded(cache.path(), &feats(&[DC]), vec![opaque()]);
    r.render(&Scene::neutral(), 64, 64).unwrap();
    let e = env_last_z();
    let got = px(&r.read_mrt(0).unwrap(), 64, 32, 32);
    assert_store(
        got,
        [base[0] + e[0], base[1] + e[1], base[2] + e[2], base[3]],
    );
    assert_eq!(r.env_builds(), 1);
}

/// Review M7: the forward EnvTexture register (CS t31) gets the env cube too (stand-in STANDIN_ENV_T31 samples t31
/// instead of t30).
#[test]
fn forward_dc_draw_samples_env_texture_t31() {
    let cache = tempfile::tempdir().unwrap();
    let mut plain = loaded(cache.path(), FLAT, vec![opaque()]);
    plain.render(&Scene::neutral(), 64, 64).unwrap();
    let base = px(&plain.read_mrt(0).unwrap(), 64, 32, 32);
    let mut r = loaded(
        cache.path(),
        &feats(&[DC, "STANDIN_ENV_T31"]),
        vec![opaque()],
    );
    r.render(&Scene::neutral(), 64, 64).unwrap();
    let e = env_last_z();
    let got = px(&r.read_mrt(0).unwrap(), 64, 32, 32);
    assert_store(
        got,
        [base[0] + e[0], base[1] + e[1], base[2] + e[2], base[3]],
    );
}

/// Review M6: the composite's own sampler goes only to `LinearSampler`; any other reflected sampler is an error.
#[test]
fn composite_with_an_unknown_sampler_fails_closed() {
    let gpu = render3d::device::Gpu::new_warp().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let src = render3d::source::SourceSet::new(standin_root());
    let defines = ["CSHADER", DC, "STANDIN_COMPOSITE_SAMPLER2"].map(String::from);
    let e = Composite::new(&gpu, &src, &defines, cache.path())
        .err()
        .expect("unknown sampler");
    assert!(e.contains("PointSampler"), "{e}");
}

/// Review M4: a failed env-cube build is an Err once, then remembered for its yaw (no rebuild every frame, the frame
/// draws with no reflection); a new yaw tries again. The failure is faked with a cube size that is not a power of two.
#[test]
fn failed_env_cube_build_is_not_retried_for_the_same_yaw() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = loaded(cache.path(), &feats(&[DC]), vec![opaque()]);
    r.set_deferred(true).unwrap();
    r.set_env_size(100);
    let mut s = Scene::neutral();
    let e = r.render(&s, 64, 64).unwrap_err();
    assert!(e.contains("power of two"), "{e}");
    let (_, events) = render3d::perf_log::capture(|| r.render(&s, 64, 64).unwrap());
    assert!(
        events.iter().all(|e| e["event"] != "env_cube"),
        "{events:?}"
    );
    assert_eq!(r.env_builds(), 0);
    s.env_yaw = 0.5;
    assert!(r.render(&s, 64, 64).is_err(), "a new yaw tries again");
    r.set_env_size(render3d::hdri::ENV_SIZE);
    s.env_yaw = 0.25;
    r.render(&s, 64, 64).unwrap();
    assert_eq!(r.env_builds(), 1);
}

/// R2: an unbound env cube (default) is 2×2 black, so Lighting's DC marker test (a 1×1 cube, Lighting.hlsl:2024-2046)
/// never mistakes a missing cube for CS's dynamic cubemap.
#[test]
fn missing_env_cube_is_not_the_dc_marker() {
    use windows::core::Interface;
    use windows::Win32::Graphics::Direct3D11::{ID3D11Texture2D, D3D11_TEXTURE2D_DESC};
    let gpu = render3d::device::Gpu::new_warp().unwrap();
    let v = render3d::textures::default_srv(&gpu, render3d::compile::SrvKind::Cube, [0, 0, 0, 255])
        .unwrap();
    let mut d = D3D11_TEXTURE2D_DESC::default();
    unsafe {
        v.GetResource()
            .unwrap()
            .cast::<ID3D11Texture2D>()
            .unwrap()
            .GetDesc(&mut d)
    };
    assert_eq!((d.Width, d.Height, d.ArraySize, d.MipLevels), (2, 2, 6, 1));
}
