//! `Renderer::override_slot` (GUI suggestions item 4) on WARP with the stand-in. `STANDIN_FLAT`: pixels =
//! 0.25·diffuse, so the slot-0 texture shows in the colour.
mod common;
use common::{cache_files, channel, dds_cube_rgba8, dds_rgba8, fixture, renderer, FLAT};
use nif::material::MaterialState;
use render3d::device::Gpu;
use render3d::scene::Scene;
use render3d::{parse, EditKind, Renderer, ShapeId, Variant};
use std::path::Path;

const CYL1: ShapeId = ShapeId(9);
const PATH: &str = "textures/standin/swap.dds";

fn frame(r: &mut Renderer) -> Vec<u8> {
    r.render(&Scene::neutral(), 64, 64).unwrap()
}

fn green() -> Vec<u8> {
    dds_rgba8(1, 1, &[[0, 255, 0, 255]])
}

/// The furniture NIF with cylinder_1's slot 0 naming `PATH`, resolved to red (the Source variant).
fn furniture(cache: &Path) -> Renderer {
    let mut r = renderer(cache, FLAT);
    let rep = r.load(&fixture("TestNifFile_Furniture_Col_SE.nif"), &|_| None);
    assert_eq!(rep.shapes, 2, "{rep:?}");
    let mut m: MaterialState = r
        .shapes()
        .into_iter()
        .find(|s| s.id == CYL1)
        .unwrap()
        .material;
    m.set_slot_raw(0, r"Textures\Standin\Swap.dds");
    let red = dds_rgba8(1, 1, &[[255, 0, 0, 255]]);
    assert_eq!(
        r.set_material(CYL1, m, &|p| (p == PATH).then(|| red.clone())),
        Ok(EditKind::Textures)
    );
    r
}

#[test]
fn an_override_shows_now_and_none_restores_without_a_load() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = furniture(cache.path());
    let before = frame(&mut r);
    let (files, mesh) = (cache_files(cache.path()), r.draw_identity(CYL1));
    let touched = r.override_slot(0, Some(&green())).unwrap();
    assert!(touched.contains(&(CYL1, 0)), "{touched:?}");
    let on = frame(&mut r);
    assert!(
        channel(&on, 1) > channel(&before, 1) + 1000,
        "the checker (green) shows"
    );
    assert!(r.override_slot(0, None).unwrap().contains(&(CYL1, 0)));
    assert_eq!(frame(&mut r), before, "the real texture is back");
    assert_eq!(r.draw_identity(CYL1), mesh, "no reload: same buffers");
    assert_eq!(cache_files(cache.path()), files, "nothing was compiled");
}

#[test]
fn the_override_survives_a_load_and_covers_an_empty_slot() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), FLAT);
    r.override_slot(0, Some(&green())).unwrap();
    r.load(&fixture("TestNifFile_Furniture_Col_SE.nif"), &|_| None);
    let mut m: MaterialState = r
        .shapes()
        .into_iter()
        .find(|s| s.id == CYL1)
        .unwrap()
        .material;
    m.set_slot_raw(0, "");
    r.set_material(CYL1, m, &|_| None).unwrap();
    let f = frame(&mut r);
    assert!(
        channel(&f, 1) > channel(&f, 0) + 1000,
        "green over every slot 0, empty or not"
    );
}

#[test]
fn a_variant_flip_under_an_override_shows_once_it_is_cleared() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = furniture(cache.path());
    r.replace_texture(
        PATH,
        &dds_rgba8(1, 1, &[[0, 0, 255, 255]]),
        Variant::Encoded,
    )
    .unwrap();
    r.override_slot(0, Some(&green())).unwrap();
    let covered = frame(&mut r);
    r.set_texture_variant(PATH, Variant::Encoded);
    assert_eq!(
        frame(&mut r),
        covered,
        "the flip does not clobber the override"
    );
    r.override_slot(0, None).unwrap();
    let f = frame(&mut r);
    assert!(
        channel(&f, 2) > channel(&f, 0) + 1000,
        "the flipped (blue) variant shows"
    );
}

#[test]
fn a_bad_override_is_an_error_and_a_cube_for_a_2d_register_is_a_warning() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = furniture(cache.path());
    let before = frame(&mut r);
    assert!(r.override_slot(0, Some(b"not a dds")).is_err());
    assert_eq!(frame(&mut r), before, "nothing changed");
    assert_eq!(
        r.override_slot(0, Some(&dds_cube_rgba8([0, 255, 0, 255]))),
        Ok(vec![]),
        "t0 reads 2D"
    );
    assert_eq!(frame(&mut r), before);
    let rep = r.load(&fixture("TestNifFile_Furniture_Col_SE.nif"), &|_| None);
    assert!(
        rep.warnings.iter().any(|w| w.contains("slot 0 override")),
        "{:?}",
        rep.warnings
    );
}

#[test]
fn an_override_set_while_shapes_compile_lands_with_their_program() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), FLAT);
    r.set_async_compile(true);
    let rep = r.load_parsed(
        &parse(&fixture("TestNifFile_Furniture_Col_SE.nif")),
        &|_| None,
    );
    assert!(rep.pending > 0, "{rep:?}");
    assert_eq!(
        r.override_slot(0, Some(&green())),
        Ok(vec![]),
        "no draw yet"
    );
    r.settle();
    let f = frame(&mut r);
    assert!(channel(&f, 1) > channel(&f, 0) + 1000);
}

#[test]
fn the_override_survives_a_device_rebuild() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), FLAT);
    r.override_slot(0, Some(&green())).unwrap();
    r.rebuild_device_on(Gpu::new_warp().unwrap()).unwrap();
    r.load(&fixture("TestNifFile_Furniture_Col_SE.nif"), &|_| None);
    let f = frame(&mut r);
    assert!(channel(&f, 1) > channel(&f, 0) + 1000);
}
