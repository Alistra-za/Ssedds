//! Texture hot-swap and the Source/Encoded variants (03 §2, tests T-H1..T-H4) on WARP with the stand-in shader.
//! `STANDIN_FLAT`: pixels = 0.25·diffuse + EmitColor (+ 0.5 green when PBRFlags ≠ 0).
mod common;
use common::{
    cache_files, channel, dds_cube_rgba8, dds_rgba8, dds_rgba8_dx10, fixture, renderer, FLAT,
};
use nif::material::MaterialState;
use render3d::scene::Scene;
use render3d::{parse, run_job, EditKind, Renderer, ShapeId, ShapeStatus, Variant};
use std::path::Path;

const CYL1: ShapeId = ShapeId(9);
const PATH: &str = "textures/standin/swap.dds";
const EMIT: &str = "textures/standin/emit.dds";

fn material(r: &Renderer) -> MaterialState {
    r.shapes()
        .into_iter()
        .find(|s| s.id == CYL1)
        .unwrap()
        .material
}

fn frame(r: &mut Renderer) -> Vec<u8> {
    r.render(&Scene::neutral(), 64, 64).unwrap()
}

/// The furniture NIF with cylinder_1's diffuse naming `PATH`, resolved to `bytes` (loaded as the Source variant).
fn with_texture(cache: &Path, bytes: Vec<u8>) -> Renderer {
    let mut r = renderer(cache, FLAT);
    let rep = r.load(&fixture("TestNifFile_Furniture_Col_SE.nif"), &|_| None);
    assert_eq!(rep.shapes, 2, "{rep:?}");
    let mut m = material(&r);
    m.set_slot_raw(0, "Textures\\Standin\\Swap.dds");
    let resolve = |p: &str| (p == PATH).then(|| bytes.clone());
    assert_eq!(r.set_material(CYL1, m, &resolve), Ok(EditKind::Textures));
    r
}

/// T-H1: new bytes for a bound path show on the next frame; the mesh buffers are the same objects and nothing is
/// compiled.
#[test]
fn hot_swap_replaces_the_srv_without_a_reload() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = with_texture(cache.path(), dds_rgba8(1, 1, &[[255, 0, 0, 255]]));
    let before = frame(&mut r);
    let files = cache_files(cache.path());
    let mesh = r.draw_identity(CYL1).expect("a draw");
    let touched = r
        .replace_texture(PATH, &dds_rgba8(1, 1, &[[0, 255, 0, 255]]), Variant::Source)
        .unwrap();
    assert_eq!(touched, vec![(CYL1, 0)]);
    let after = frame(&mut r);
    assert!(
        channel(&after, 1) > channel(&before, 1) + 1000,
        "green rose: {} → {}",
        channel(&before, 1),
        channel(&after, 1)
    );
    assert!(channel(&after, 0) < channel(&before, 0), "red fell");
    assert_eq!(r.draw_identity(CYL1), Some(mesh), "no reload: same buffers");
    assert_eq!(cache_files(cache.path()), files, "nothing was compiled");
    // A path no shape reads is stored, touches nothing, and cannot be flipped to a copy it lacks.
    let unused = "textures/standin/unused.dds";
    let px = dds_rgba8(1, 1, &[[1, 2, 3, 255]]);
    assert!(r
        .replace_texture(unused, &px, Variant::Source)
        .unwrap()
        .is_empty());
    assert!(!r.set_texture_variant(unused, Variant::Encoded));
}

/// T-H2: a cube into a 2D slot, or bytes that do not decode, change nothing.
#[test]
fn hot_swap_kind_mismatch_keeps_the_old_binding() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = with_texture(cache.path(), dds_rgba8(1, 1, &[[255, 0, 0, 255]]));
    let before = frame(&mut r);
    let e = r
        .replace_texture(PATH, &dds_cube_rgba8([0, 0, 255, 255]), Variant::Source)
        .unwrap_err();
    assert!(e.contains("Cube"), "{e}");
    assert_eq!(frame(&mut r), before, "the old texture stays");
    assert!(r
        .replace_texture(PATH, b"not a dds", Variant::Source)
        .is_err());
    assert_eq!(frame(&mut r), before);
    // Nothing was stored for the refused cube: there is no second copy to flip to.
    assert!(!r.set_texture_variant(PATH, Variant::Encoded));
}

/// T-H3: the same texels as UNORM (Source) and UNORM_SRGB (Encoded): the sRGB view linearises, so Encoded is
/// darker; flipping back is byte-identical; an inactive copy changes nothing.
#[test]
fn ab_toggle_is_instant_and_the_srgb_header_shows() {
    let cache = tempfile::tempdir().unwrap();
    let grey = [[128, 128, 128, 255]];
    let mut r = with_texture(cache.path(), dds_rgba8(1, 1, &grey));
    let source = frame(&mut r);
    let touched = r
        .replace_texture(PATH, &dds_rgba8_dx10(1, 1, &grey, true), Variant::Encoded)
        .unwrap();
    assert!(touched.is_empty(), "stored, not shown: Source is active");
    assert_eq!(frame(&mut r), source, "an inactive copy changes nothing");
    assert!(r.set_texture_variant(PATH, Variant::Encoded));
    let encoded = frame(&mut r);
    assert!(
        channel(&encoded, 0) < channel(&source, 0),
        "sRGB-tagged grey is darker: {} vs {}",
        channel(&encoded, 0),
        channel(&source, 0)
    );
    assert!(
        !r.set_texture_variant(PATH, Variant::Encoded),
        "already active"
    );
    assert!(r.set_texture_variant(PATH, Variant::Source));
    assert_eq!(frame(&mut r), source, "flipping back is byte-identical");
    // Dropping the inactive copy keeps what is shown; dropping the active one falls back to the other.
    r.replace_texture(PATH, &[], Variant::Encoded).unwrap();
    assert_eq!(frame(&mut r), source);
    assert!(!r.set_texture_variant(PATH, Variant::Encoded), "dropped");
}

/// T-H4: a TruePBR emissive map (slot 2 → t6) swapped in sets HasEmissive (the stand-in's green channel) and
/// dropping it clears the flag again.
#[test]
fn swapping_an_emissive_map_in_and_out_flips_has_emissive() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), FLAT);
    r.load(&fixture("TestNifFile_Furniture_Col_SE.nif"), &|_| None);
    let mut m = material(&r);
    m.slsf2 &= !((1 << 12) | (1 << 24) | (1 << 25) | (1 << 26) | (1 << 27));
    m.slsf2 |= 1 << 23; // TruePBR
    m.params.emissive_color = [0.0; 3]; // D1 would add the NIF emissive colour under PBR
    m.set_slot_raw(2, "Textures\\Standin\\Emit.dds");
    assert_eq!(
        r.set_material(CYL1, m, &|_| None),
        Ok(EditKind::Permutation)
    );
    let absent = frame(&mut r);
    let white = dds_rgba8(1, 1, &[[255, 255, 255, 255]]);
    let touched = r.replace_texture(EMIT, &white, Variant::Encoded).unwrap();
    assert_eq!(touched, vec![(CYL1, 6)]);
    let present = frame(&mut r);
    assert!(
        channel(&present, 1) > channel(&absent, 1) + 1000,
        "green (HasEmissive): {} → {}",
        channel(&absent, 1),
        channel(&present, 1)
    );
    assert_eq!(
        channel(&present, 2),
        channel(&absent, 2),
        "only the flag moved"
    );
    assert_eq!(
        r.replace_texture(EMIT, &[], Variant::Encoded).unwrap(),
        vec![(CYL1, 6)]
    );
    assert_eq!(frame(&mut r), absent, "the flag is clear again");
}

/// The resolver's bytes count as `set_load_variant`'s variant, so a later flip can reach the other copy.
#[test]
fn the_load_variant_labels_what_the_resolver_returned() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), FLAT);
    r.set_load_variant(Variant::Encoded);
    r.load(&fixture("TestNifFile_Furniture_Col_SE.nif"), &|_| None);
    let mut m = material(&r);
    m.set_slot_raw(0, "Textures\\Standin\\Swap.dds");
    let red = dds_rgba8(1, 1, &[[255, 0, 0, 255]]);
    let resolve = |p: &str| (p == PATH).then(|| red.clone());
    r.set_material(CYL1, m, &resolve).unwrap();
    let shown = frame(&mut r);
    let green = dds_rgba8(1, 1, &[[0, 255, 0, 255]]);
    assert!(
        r.replace_texture(PATH, &green, Variant::Source)
            .unwrap()
            .is_empty(),
        "the resolver's red is the Encoded copy and stays shown"
    );
    assert_eq!(frame(&mut r), shown);
    assert!(r.set_texture_variant(PATH, Variant::Source));
    assert!(channel(&frame(&mut r), 1) > channel(&shown, 1) + 1000);
}

/// R1: a shape whose permutation edit is withheld (Pending, previous draw kept) still draws its old draw, so a swap
/// must rebind it too.
#[test]
fn a_pending_shape_showing_its_previous_draw_receives_the_swap() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), FLAT);
    r.set_async_compile(true);
    r.load_parsed(
        &parse(&fixture("TestNifFile_Furniture_Col_SE.nif")),
        &|_| None,
    );
    for j in r.take_jobs() {
        r.apply_compiled(run_job(j));
    }
    let red = dds_rgba8(1, 1, &[[255, 0, 0, 255]]);
    let resolve = |p: &str| (p == PATH).then(|| red.clone());
    let mut m = material(&r);
    m.set_slot_raw(0, "Textures\\Standin\\Swap.dds");
    assert_eq!(
        r.set_material(CYL1, m.clone(), &resolve),
        Ok(EditKind::Textures)
    );
    // TruePBR: a new program, whose job is taken and never applied.
    m.slsf2 &= !((1 << 12) | (1 << 24) | (1 << 25) | (1 << 26) | (1 << 27));
    m.slsf2 |= (1 << 23) | (1 << 26);
    m.params.emissive_color = [0.0; 3];
    assert_eq!(r.set_material(CYL1, m, &resolve), Ok(EditKind::Permutation));
    let _withheld = r.take_jobs();
    let status = r
        .shapes()
        .into_iter()
        .find(|s| s.id == CYL1)
        .unwrap()
        .status;
    assert_eq!(
        status,
        ShapeStatus::Pending {
            showing_previous: true
        }
    );
    let before = frame(&mut r);
    let touched = r
        .replace_texture(PATH, &dds_rgba8(1, 1, &[[0, 255, 0, 255]]), Variant::Source)
        .unwrap();
    assert_eq!(touched, vec![(CYL1, 0)]);
    let after = frame(&mut r);
    assert!(
        channel(&after, 1) > channel(&before, 1) + 1000,
        "the pending shape's previous draw got the new texture"
    );
}
