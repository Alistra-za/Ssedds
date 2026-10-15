//! Debug views (GUI suggestions item 1): the stand-in has no anchor, so only Lit; the installed CS reads RMAOS back
//! as its values (opt-in).
mod common;
use common::{cache_files, dds_rgba8, renderer};
use nif::material::{MaterialState, SLSF2_TRUE_PBR};
use render3d::device::Gpu;
use render3d::scene::Scene;
use render3d::{Builtin, DebugOutput, Renderer, ShapeId};

#[test]
fn the_standin_refuses_debug_views_and_lit_keys_are_unchanged() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), &["STANDIN_OPAQUE_OK"]);
    let rep = r.load_builtin(Builtin::Sphere, MaterialState::builtin_default(), &|_| None);
    assert!(
        rep.compile_errors.is_empty() && rep.errors.is_empty(),
        "{rep:?}"
    );
    let files = cache_files(cache.path());
    assert!(!r.debug_supported());
    assert!(r.set_debug_output(DebugOutput::Albedo).is_err());
    assert_eq!(r.debug_output(), DebugOutput::Lit, "nothing changed");
    assert_eq!(r.set_debug_output(DebugOutput::Lit), Ok(()));
    assert_eq!(
        cache_files(cache.path()),
        files,
        "the Lit programs are memory hits: same keys, nothing compiled"
    );
    let (ps, _, _) = r.draw_diag(ShapeId(0)).unwrap();
    assert!(
        !ps.iter().any(|(k, _)| k.starts_with("SDC_DEBUG")),
        "{ps:?}"
    );
}

/// Opt-in: a TruePBR sphere with roughness scale 1 (PBRParams1.x, L:1439) and a 1×1 RMAOS (64, 128, 192) reads back
/// as those values in Roughness / Metallic / AO (vertexAO 1: no vertex colours, L:269), encode on and off.
/// `SDC_CS_SHADERS=…\Shaders cargo test -p render3d --release --test debug -- --ignored`
#[test]
#[ignore]
fn installed_cs_reads_rmaos_back_as_its_values() {
    let root = std::path::PathBuf::from(
        std::env::var("SDC_CS_SHADERS").expect("set SDC_CS_SHADERS to the CS Shaders folder"),
    );
    let fp = render3d::locate::fingerprint(&root, None).unwrap();
    let cache = tempfile::tempdir().unwrap();
    let mut r =
        Renderer::with_gpu(Gpu::new_warp().unwrap(), fp, cache.path().to_path_buf()).unwrap();
    assert!(r.debug_supported(), "the installed CS has the anchor");
    let mut m = MaterialState::builtin_default();
    m.slsf2 |= SLSF2_TRUE_PBR;
    m.params.specular_strength = 1.0; // the roughness scale
    m.set_slot_raw(5, r"textures\sdc\rmaos.dds");
    let rmaos = dds_rgba8(1, 1, &[[64, 128, 192, 255]]);
    let rep = r.load_builtin(Builtin::Sphere, m, &|p| {
        (p == "textures/sdc/rmaos.dds").then(|| rmaos.clone())
    });
    assert!(
        rep.errors.is_empty() && rep.compile_errors.is_empty(),
        "{rep:?}"
    );
    for encode in [true, false] {
        r.set_encode_srgb(encode);
        for (d, want) in [
            (DebugOutput::Roughness, 64),
            (DebugOutput::Metallic, 128),
            (DebugOutput::Ao, 192),
        ] {
            // set_debug_output always re-permutes (R7), so the PRELINEAR define follows the encode just set.
            r.set_debug_output(d).unwrap();
            let f = r.render(&Scene::neutral(), 64, 64).unwrap();
            let c = &f[(32 * 64 + 32) * 4..][..3];
            for k in 0..3 {
                assert!(
                    (c[k] as i32 - want).abs() <= 2,
                    "{d:?} encode {encode}: {c:?}"
                );
            }
        }
    }
    r.rebuild_device_on(Gpu::new_warp().unwrap()).unwrap();
    assert_eq!(
        r.debug_output(),
        DebugOutput::Ao,
        "kept across a device rebuild"
    );
}
