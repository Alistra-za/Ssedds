//! The render path on WARP with the repo-owned stand-in `Lighting.hlsl` (`fixtures/standin`), so a gate run
//! covers permutation → compile → reflection → cbuffers → default textures → draw → readback without the
//! user's Community Shaders install. Not ignored.
use render3d::device::Gpu;
use render3d::scene::Scene;
use render3d::Renderer;
use std::path::PathBuf;

fn standin_root() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/standin"
    ))
}

/// `TestNifFile_DeepGraph_SE.nif`: 27 Lighting shapes; all but `SkullPeg01:19` have an NiAlphaProperty
/// (→ DO_ALPHA_TEST). The stand-in's PS `#error`s without DO_ALPHA_TEST, so exactly that shape fails.
/// Every fixture shape carries SPECULAR, so SPECULAR cannot be the failing key.
#[test]
fn standin_renders_on_warp_with_one_failing_permutation() {
    let root = standin_root();
    let fp = render3d::locate::fingerprint(&root, None).unwrap();
    let cache = tempfile::tempdir().unwrap();
    let mut r =
        Renderer::with_gpu(Gpu::new_warp().unwrap(), fp, cache.path().to_path_buf()).unwrap();
    let nif = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../nif/tests/fixtures/TestNifFile_DeepGraph_SE.nif"
    ))
    .unwrap();
    let lighting = nif::scan::scan(&nif)
        .unwrap()
        .shapes
        .iter()
        .filter(|s| matches!(s.shader, nif::scan::Shader::Lighting(..)))
        .count();
    assert!(lighting > 2, "fixture has several Lighting shapes");

    let rep = r.load(&nif, &|_| None);
    assert!(rep.errors.is_empty(), "{:?}", rep.errors);
    assert_eq!(rep.compile_errors.len(), 1, "{:?}", rep.compile_errors);
    assert!(
        rep.compile_errors[0].starts_with("SkullPeg01:19: ps:"),
        "{:?}",
        rep.compile_errors
    );
    assert_eq!(rep.shapes, lighting - 1);
    // No resolver: every texture the PS reads is missing and gets a 1×1 default.
    for t in [
        "textures/trap/tripwiretrap01.dds",
        "textures/trap/tripwiretrap01_n.dds",
    ] {
        assert!(rep.missing_textures.iter().any(|m| m == t), "{rep:?}");
    }

    let (w, h) = (96, 96);
    let px = r.render(&Scene::neutral(), w, h).unwrap();
    assert_eq!(px.len(), (w * h * 4) as usize);
    let covered = px
        .chunks(4)
        .filter(|p| p[0] as u32 + p[1] as u32 + p[2] as u32 > 30)
        .count();
    assert!(covered > 0, "frame is not empty");
    assert!(px.chunks(4).all(|p| p[3] == 255), "display alpha is 255");
}

/// Opaque shapes (no NiAlphaProperty, no blending): the stand-in PS writes alpha 0.5, which lands in the
/// render target as is. The display frame must still be opaque — the UI uploads it unmultiplied, so a
/// diffuse alpha below 1 would show the panel through an opaque mesh.
#[test]
fn opaque_shapes_read_back_with_alpha_255() {
    let fp = render3d::locate::fingerprint(&standin_root(), None).unwrap();
    let cache = tempfile::tempdir().unwrap();
    let mut r =
        Renderer::with_gpu(Gpu::new_warp().unwrap(), fp, cache.path().to_path_buf()).unwrap();
    r.set_features(vec!["STANDIN_OPAQUE_OK".into()]);
    let nif = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../nif/tests/fixtures/TestNifFile_Furniture_Col_SE.nif"
    ))
    .unwrap();
    let rep = r.load(&nif, &|_| None);
    assert!(
        rep.compile_errors.is_empty() && rep.errors.is_empty() && rep.shapes == 2,
        "{rep:?}"
    );
    for srgb in [false, true] {
        r.set_encode_srgb(srgb);
        let px = r.render(&Scene::neutral(), 64, 64).unwrap();
        let covered = px
            .chunks(4)
            .filter(|p| p[0] as u32 + p[1] as u32 + p[2] as u32 > 30)
            .count();
        assert!(covered > 0, "frame is not empty");
        assert!(
            px.chunks(4).all(|p| p[3] == 255),
            "display alpha is 255 (srgb {srgb})"
        );
    }
}
