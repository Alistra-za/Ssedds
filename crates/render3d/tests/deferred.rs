//! Deferred mode (v2 Phase D, D3): opaque draws compile DEFERRED and write CS's 8 render targets; blended draws stay
//! forward into RT0. The stand-in's DEFERRED main writes fixed markers to RT1-7.
mod common;
use common::{cache_files, dds_rgba8, plane_shape, planes, renderer_at, standin_root, FLAT, IDENT};
use nif::props::Alpha;
use render3d::scene::Scene;
use render3d::{DebugOutput, EditKind, Renderer, ShapeId, ShapeStatus};

/// What the stand-in's RT1-7 read back as where it drew (index = RT; RT0 is the lit colour).
const M: [[f32; 4]; 8] = [
    [0.0; 4],
    [0.25, 0.5, 0.0, 1.0],
    [1.0, 0.0, 1.0, 1.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.5, 0.25, 0.125, 1.0],
    [0.25, 0.125, 0.5, 1.0],
    [2.0, 4.0, 0.0625, 1.0],
    [1.0, 0.0, 0.0, 1.0],
];

/// RT1-7 after the clear (0; missing alpha reads 1, R10G10B10A2 keeps its 0 alpha).
fn cleared(rt: usize) -> [f32; 4] {
    if rt == 2 || rt == 3 {
        [0.0; 4]
    } else {
        [0.0, 0.0, 0.0, 1.0]
    }
}

fn px(v: &[f32], w: usize, x: usize, y: usize) -> [f32; 4] {
    v[(y * w + x) * 4..][..4].try_into().unwrap()
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

fn two_planes(cache: &std::path::Path) -> Renderer {
    planes(
        cache,
        vec![plane_shape(0, IDENT, [1.0, 0.0, 0.0], 1), blended(1)],
    )
}

#[test]
fn deferred_draw_writes_eight_targets() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = two_planes(cache.path());
    assert!(!r.deferred());
    r.set_deferred(true).unwrap();
    assert!(r.deferred());
    r.render(&Scene::neutral(), 64, 64).unwrap();
    assert!(has_deferred(&mut r, 0), "opaque shape compiles DEFERRED");
    assert!(!has_deferred(&mut r, 1), "blended shape stays forward");
    let rts: Vec<Vec<f32>> = (0..8).map(|i| r.read_mrt(i).unwrap()).collect();
    for v in &rts {
        assert_eq!(v.len(), 64 * 64 * 4);
    }
    let (mut drawn, mut empty) = (0, 0);
    for p in 0..64 * 64 {
        let at = |i: usize| -> [f32; 4] { rts[i][p * 4..][..4].try_into().unwrap() };
        let rt7 = at(7)[0];
        if rt7 == 1.0 {
            drawn += 1;
            for (i, want) in M.iter().enumerate().skip(1) {
                assert_eq!(at(i), *want, "RT{i} at pixel {p}");
            }
        } else {
            assert_eq!(rt7, 0.0, "RT7 at pixel {p}");
            empty += 1;
            for i in 1..8 {
                assert_eq!(at(i), cleared(i), "RT{i} at pixel {p}");
            }
        }
    }
    assert!(drawn > 0 && empty > 0, "drawn {drawn}, empty {empty}");
    assert!(r.read_mrt(8).is_err());
}

#[test]
fn resize_recreates_every_target() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = two_planes(cache.path());
    r.set_deferred(true).unwrap();
    r.render(&Scene::neutral(), 64, 64).unwrap();
    assert_eq!(r.target_dims(), vec![(64, 64); 10]);
    r.render(&Scene::neutral(), 96, 48).unwrap();
    assert_eq!(r.target_dims(), vec![(96, 48); 10]);
    assert_eq!(px(&r.read_mrt(7).unwrap(), 96, 48, 24), M[7]);
    r.set_deferred(false).unwrap();
    r.render(&Scene::neutral(), 96, 48).unwrap();
    assert_eq!(r.target_dims(), vec![(96, 48); 2]);
    assert!(r.read_mrt(1).is_err());
}

/// D5: the deferred depth is R32_TYPELESS with an R32_FLOAT SRV (the composite's DepthTexture); forward keeps D32.
#[test]
fn deferred_target_has_a_depth_srv() {
    use render3d::device::{Gpu, Target};
    let gpu = Gpu::new_warp().unwrap();
    assert!(Target::new_deferred(&gpu, 8, 8)
        .unwrap()
        .depth_srv
        .is_some());
    assert!(Target::new(&gpu, 8, 8).unwrap().depth_srv.is_none());
}

/// D3D11 keeps the views it has bound alive: recreating the target must unbind them first, or the old textures
/// outlive the Rust drop while the new ones are allocated. `drop_target` is what render's recreation branch calls.
#[test]
fn recreating_the_target_unbinds_the_old_views_first() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = two_planes(cache.path());
    r.set_deferred(true).unwrap();
    r.render(&Scene::neutral(), 64, 64).unwrap();
    assert!(r.bound_rtvs() > 0, "a frame leaves its views bound");
    assert_eq!(r.bound_cs_views(), 0, "the composite unbinds its views");
    r.drop_target();
    assert!(r.target_dims().is_empty(), "the old target is gone");
    assert_eq!(r.bound_rtvs(), 0);
    assert_eq!(r.bound_cs_views(), 0);
    r.render(&Scene::neutral(), 64, 64).unwrap();
    assert_eq!(r.target_dims(), vec![(64, 64); 10]);
}

/// A forward program writes SV_Target1/2 too (CS's forward PS_OUTPUT): drawn after an opaque deferred shape, it must
/// leave that shape's RT1/RT2 untouched, so RT1-7 are never bound for it.
#[test]
fn forward_draws_never_see_rts_one_to_seven() {
    let cache = tempfile::tempdir().unwrap();
    let mut f = FLAT.to_vec();
    f.push("STANDIN_FORWARD_AUX");
    let mut r = common::renderer(cache.path(), &f);
    let parsed = render3d::ParsedNif {
        shapes: vec![plane_shape(0, IDENT, [1.0, 0.0, 0.0], 1), blended(1)],
        ..Default::default()
    };
    let rep = r.load_parsed(&parsed, &|_| None);
    assert!(
        rep.errors.is_empty() && rep.compile_errors.is_empty(),
        "{rep:?}"
    );
    r.set_deferred(true).unwrap();
    r.render(&Scene::neutral(), 64, 64).unwrap();
    assert!(!has_deferred(&mut r, 1));
    let (rt1, rt2) = (r.read_mrt(1).unwrap(), r.read_mrt(2).unwrap());
    assert_eq!(px(&rt1, 64, 32, 32), M[1]);
    assert_eq!(px(&rt2, 64, 32, 32), M[2]);
}

#[test]
fn debug_views_still_render_in_deferred_mode() {
    let shaders = tempfile::tempdir().unwrap();
    let src = std::fs::read_to_string(standin_root().join("Lighting.hlsl")).unwrap();
    assert!(src.contains("return psout ;"));
    std::fs::write(
        shaders.path().join("Lighting.hlsl"),
        src.replace("    return psout ;", "    return psout;"),
    )
    .unwrap();
    std::fs::copy(
        standin_root().join("DeferredCompositeCS.hlsl"),
        shaders.path().join("DeferredCompositeCS.hlsl"),
    )
    .unwrap();
    let cache = tempfile::tempdir().unwrap();
    // The composite adds Specular to RT0 in Lit; the debug colours below prove debug views skip it (v2 D4).
    let mut r = renderer_at(
        shaders.path(),
        cache.path(),
        &[FLAT, &["STANDIN_COMPOSITE_ADD"]].concat(),
    );
    assert!(r.debug_supported());
    let mut s = plane_shape(0, IDENT, [0.0; 3], 1);
    s.material.set_slot_raw(0, r"textures\sdc\d.dds");
    let dds = dds_rgba8(1, 1, &[[64, 128, 192, 255]]);
    let parsed = render3d::ParsedNif {
        shapes: vec![s],
        ..Default::default()
    };
    let rep = r.load_parsed(&parsed, &|p| {
        (p == "textures/sdc/d.dds").then(|| dds.clone())
    });
    assert!(
        rep.errors.is_empty() && rep.compile_errors.is_empty(),
        "{rep:?}"
    );
    r.set_deferred(true).unwrap();
    let lit = r.render(&Scene::neutral(), 64, 64).unwrap();
    let centre = |f: &[u8]| -> [i32; 3] {
        let c = &f[(32 * 64 + 32) * 4..];
        [c[0] as i32, c[1] as i32, c[2] as i32]
    };
    for (d, want) in [
        (DebugOutput::Albedo, [64, 128, 192]),
        (DebugOutput::Roughness, [191; 3]),
        (DebugOutput::Ao, [128; 3]),
    ] {
        r.set_debug_output(d).unwrap();
        let f = r.render(&Scene::neutral(), 64, 64).unwrap();
        let got = centre(&f);
        for k in 0..3 {
            assert!((got[k] - want[k]).abs() <= 1, "{d:?}: {got:?}");
        }
        assert_eq!(px(&r.read_mrt(7).unwrap(), 64, 32, 32), M[7], "{d:?}");
    }
    r.set_debug_output(DebugOutput::Lit).unwrap();
    assert_eq!(r.render(&Scene::neutral(), 64, 64).unwrap(), lit);
    assert!(r.deferred());
}

#[test]
fn toggle_back_to_forward_matches_the_old_frame() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = two_planes(cache.path());
    let a = r.render(&Scene::neutral(), 64, 64).unwrap();
    let files = cache_files(cache.path());
    r.set_deferred(true).unwrap();
    let b = r.render(&Scene::neutral(), 64, 64).unwrap();
    assert_eq!(b, a, "RT0 of the deferred frame is the forward colour");
    let files2 = cache_files(cache.path());
    assert!(files2 > files, "the DEFERRED permutation compiled");
    r.set_deferred(false).unwrap();
    assert_eq!(cache_files(cache.path()), files2, "nothing compiles");
    let c = r.render(&Scene::neutral(), 64, 64).unwrap();
    assert_eq!(c, a);
    assert!(r.read_mrt(1).is_err());
    r.set_deferred(false).unwrap();
    assert_eq!(cache_files(cache.path()), files2);
}

/// D-P36 (F2): a deferred-mode frame with no DEFERRED draw (all blended, or a built-in) is a forward frame: no 8-RT
/// target, no composite, the same pixels as forward.
#[test]
fn a_frame_without_a_deferred_draw_stays_forward() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = planes(cache.path(), vec![blended(1)]);
    let fwd = r.render(&Scene::neutral(), 64, 64).unwrap();
    r.set_deferred(true).unwrap();
    assert_eq!(r.render(&Scene::neutral(), 64, 64).unwrap(), fwd);
    assert_eq!(r.target_dims().len(), 2, "blended only: the forward target");
    assert!(r.read_mrt(1).is_err());

    let mut b = common::renderer(cache.path(), &["STANDIN_OPAQUE_OK"]);
    let m = nif::material::MaterialState::builtin_default();
    let rep = b.load_builtin(render3d::Builtin::Sphere, m, &|_| None);
    assert!(rep.compile_errors.is_empty(), "{rep:?}");
    b.set_deferred(true).unwrap();
    b.render(&Scene::neutral(), 64, 64).unwrap();
    assert_eq!(b.target_dims().len(), 2, "a built-in: the forward target");
}

#[test]
fn deferred_edit_from_opaque_to_blended_repermutes() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = two_planes(cache.path());
    r.set_deferred(true).unwrap();
    let mut m = blended(0).material;
    m.params.emissive_color = [1.0, 0.0, 0.0];
    assert_eq!(
        r.set_material(ShapeId(0), m, &|_| None),
        Ok(EditKind::Permutation)
    );
    assert!(!has_deferred(&mut r, 0));
}

/// Opt-in: the installed CS compiles its DEFERRED permutation for an opaque plane and writes NormalGlossiness.
/// `SDC_CS_SHADERS=…\Shaders cargo test -p render3d --release --test deferred -- --ignored --nocapture`
#[test]
#[ignore]
fn installed_cs_deferred_plane_smoke() {
    let Ok(shaders) = std::env::var("SDC_CS_SHADERS") else {
        println!("skipped: set SDC_CS_SHADERS");
        return;
    };
    let fp = render3d::locate::fingerprint(std::path::Path::new(&shaders), None).unwrap();
    let cache = tempfile::tempdir().unwrap();
    let mut r = Renderer::new(fp, cache.path().to_path_buf()).unwrap();
    println!("adapter: {} (warp: {})", r.gpu().adapter, r.gpu().warp);
    let parsed = render3d::ParsedNif {
        shapes: vec![plane_shape(0, IDENT, [0.0; 3], 1)],
        ..Default::default()
    };
    let rep = r.load_parsed(&parsed, &|_| None);
    assert!(
        rep.errors.is_empty() && rep.compile_errors.is_empty(),
        "{rep:?}"
    );
    let t = std::time::Instant::now();
    r.set_deferred(true).unwrap();
    println!("set_deferred(true) cold: {:?}", t.elapsed());
    assert_eq!(r.deferred_error(), None);
    assert_eq!(r.shapes()[0].status, ShapeStatus::Drawing);
    assert!(has_deferred(&mut r, 0));
    r.render(&Scene::neutral(), 1600, 900).unwrap();
    for _ in 0..5 {
        r.render(&Scene::neutral(), 1600, 900).unwrap();
        println!("timing {:?}", r.last_timing());
    }
    println!(
        "composite builds {}, unbound {:?}",
        r.composite_builds(),
        r.composite().map(|c| &c.unbound)
    );
    // Warm: a fresh renderer on the same disk cache.
    let fp = render3d::locate::fingerprint(std::path::Path::new(&shaders), None).unwrap();
    let mut w = Renderer::new(fp, cache.path().to_path_buf()).unwrap();
    w.load_parsed(&parsed, &|_| None);
    let t = std::time::Instant::now();
    w.set_deferred(true).unwrap();
    println!("set_deferred(true) warm: {:?}", t.elapsed());
    let n = px(&r.read_mrt(2).unwrap(), 1600, 800, 450);
    println!("RT2 centre {n:?}");
    assert!(n.iter().any(|v| *v != 0.0), "NormalGlossiness written");
    println!("coverage {:?}", r.settings_coverage());
}
