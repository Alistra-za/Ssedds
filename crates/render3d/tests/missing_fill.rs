//! `Renderer::set_missing_texture` (GUI suggestions item 3, owner D3): the fill goes to colour registers only (t0, and
//! t4 when it reads a cube); data maps keep their neutral defaults and `Bound` is not changed. WARP + stand-in.
mod common;
use common::{channel, dds_rgba8, fixture, renderer, FLAT};
use render3d::compile::SrvKind;
use render3d::device::Gpu;
use render3d::scene::Scene;
use render3d::{Builtin, Renderer, ShapeId, Variant};
use std::path::Path;

const CYL1: ShapeId = ShapeId(9);
const MAGENTA: [u8; 4] = [255, 0, 255, 255];
const DIFFUSE: &str = "textures/standin/missing_d.dds";

/// cylinder_1 of the furniture fixture with `slots` set (none resolves) and `shader_type`; the default lit stand-in
/// (no FLAT/TBN) reads both t0 and t1.
fn cylinder(
    cache: &Path,
    features: &[&str],
    fill: Option<[u8; 4]>,
    shader_type: u32,
    slots: &[(usize, &str)],
) -> Renderer {
    let mut r = renderer(cache, features);
    r.set_missing_texture(fill);
    let rep = r.load(&fixture("TestNifFile_Furniture_Col_SE.nif"), &|_| None);
    assert_eq!(rep.shapes, 2, "{rep:?}");
    let mut m = r
        .shapes()
        .into_iter()
        .find(|s| s.id == CYL1)
        .unwrap()
        .material;
    m.shader_type = shader_type;
    for (i, p) in slots {
        m.set_slot_raw(*i, p);
    }
    assert!(r.set_material(CYL1, m, &|_| None).is_ok());
    r
}

#[test]
fn a_missing_diffuse_shows_the_fill_and_a_missing_normal_stays_flat() {
    let cache = tempfile::tempdir().unwrap();
    let r = cylinder(
        cache.path(),
        &["STANDIN_OPAQUE_OK"],
        Some(MAGENTA),
        0,
        &[
            (0, r"textures\standin\missing_d.dds"),
            (1, r"textures\standin\missing_n.dds"),
        ],
    );
    assert_eq!(r.draw_default_at(CYL1, 0), Some((SrvKind::Tex2D, MAGENTA)));
    assert_eq!(
        r.draw_default_at(CYL1, 1),
        Some((SrvKind::Tex2D, [128, 128, 255, 255]))
    );
    assert_eq!(
        r.draw_bound(CYL1),
        Some(vec![]),
        "a fill is not a real texture"
    );
}

#[test]
fn the_fill_shows_in_pixels_and_none_keeps_todays_white() {
    let cache = tempfile::tempdir().unwrap();
    let slots = [(0, r"textures\standin\missing_d.dds")];
    let mut filled = cylinder(cache.path(), FLAT, Some(MAGENTA), 0, &slots);
    let cache2 = tempfile::tempdir().unwrap();
    let mut plain = cylinder(cache2.path(), FLAT, None, 0, &slots);
    assert_eq!(
        plain.draw_default_at(CYL1, 0),
        Some((SrvKind::Tex2D, [255, 255, 255, 255]))
    );
    let (a, b) = (
        filled.render(&Scene::neutral(), 64, 64).unwrap(),
        plain.render(&Scene::neutral(), 64, 64).unwrap(),
    );
    assert!(
        channel(&a, 1) + 1000 < channel(&b, 1),
        "magenta has no green: {} vs {}",
        channel(&a, 1),
        channel(&b, 1)
    );
}

#[test]
fn a_facegen_shape_without_rfaos_keeps_t71_black() {
    let cache = tempfile::tempdir().unwrap();
    let r = cylinder(
        cache.path(),
        &["STANDIN_OPAQUE_OK", "STANDIN_FLAT", "STANDIN_SKIN"],
        Some(MAGENTA),
        4,
        &[
            (1, r"textures\standin\head_N.dds"),
            (2, r"textures\standin\head_sk.dds"),
            (7, r"Textures\Standin\Light\head_s.dds"),
        ],
    );
    assert_eq!(
        r.draw_default_at(CYL1, 71),
        Some((SrvKind::Tex2D, [0, 0, 0, 0])),
        "RFAOS guesses never fill"
    );
}

#[test]
fn a_dropped_swap_of_a_missing_path_falls_back_to_the_fill() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = cylinder(
        cache.path(),
        FLAT,
        Some(MAGENTA),
        0,
        &[(0, r"textures\standin\missing_d.dds")],
    );
    // A material edit evicts texture-less cache entries; the missing provenance must outlive that.
    let mut m = r
        .shapes()
        .into_iter()
        .find(|s| s.id == CYL1)
        .unwrap()
        .material;
    m.params.uv_scale = [2.0, 2.0];
    assert!(r.set_material(CYL1, m, &|_| None).is_ok());
    assert_eq!(r.draw_default_at(CYL1, 0), Some((SrvKind::Tex2D, MAGENTA)));
    r.replace_texture(
        DIFFUSE,
        &dds_rgba8(1, 1, &[[255, 0, 0, 255]]),
        Variant::Source,
    )
    .unwrap();
    assert_eq!(r.draw_default_at(CYL1, 0), None, "a real texture now");
    assert_eq!(r.draw_bound(CYL1), Some(vec![0]));
    r.replace_texture(DIFFUSE, &[], Variant::Source).unwrap();
    assert_eq!(r.draw_default_at(CYL1, 0), Some((SrvKind::Tex2D, MAGENTA)));
    assert_eq!(r.draw_bound(CYL1), Some(vec![]));
}

#[test]
fn the_fill_survives_a_device_rebuild() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), FLAT);
    r.set_missing_texture(Some(MAGENTA));
    r.rebuild_device_on(Gpu::new_warp().unwrap()).unwrap();
    let mut m = nif::material::MaterialState::builtin_default();
    m.alpha = Some(nif::props::Alpha {
        flags: 0x200,
        threshold: 0,
    });
    m.set_slot_raw(0, r"textures\standin\missing_d.dds");
    r.load_builtin(Builtin::Sphere, m, &|_| None);
    assert_eq!(
        r.draw_default_at(ShapeId(0), 0),
        Some((SrvKind::Tex2D, MAGENTA))
    );
}

/// Opt-in: an env-mapped shape (type 1) whose cube map is missing gets a cube of the fill at t4 on the installed CS
/// (the stand-in reflects no TextureCube). `SDC_CS_SHADERS=…\Shaders cargo test -p render3d --release --test missing_fill -- --ignored`
#[test]
#[ignore]
fn installed_cs_fills_a_missing_env_cube_with_a_cube() {
    let root = std::path::PathBuf::from(
        std::env::var("SDC_CS_SHADERS").expect("set SDC_CS_SHADERS to the CS Shaders folder"),
    );
    let fp = render3d::locate::fingerprint(&root, None).unwrap();
    let cache = tempfile::tempdir().unwrap();
    let mut r =
        Renderer::with_gpu(Gpu::new_warp().unwrap(), fp, cache.path().to_path_buf()).unwrap();
    r.set_missing_texture(Some(MAGENTA));
    let mut m = nif::material::MaterialState::builtin_default();
    m.shader_type = 1;
    m.slsf1 |= 1 << 7; // Environment_Mapping
    m.set_slot_raw(4, r"textures\cubemaps\sdc_missing_e.dds");
    let rep = r.load_builtin(Builtin::Sphere, m, &|_| None);
    assert!(rep.compile_errors.is_empty(), "{rep:?}");
    assert_eq!(
        r.draw_default_at(ShapeId(0), 4),
        Some((SrvKind::Cube, MAGENTA))
    );
}
