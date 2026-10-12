//! Live material edits on WARP with the stand-in shader. `STANDIN_FLAT` makes pixels = 0.25·diffuse +
//! EmitColor (+ 0.5 green when PBRFlags ≠ 0), so cbuffer and texture edits show without lighting saturation.
mod common;
use common::{cache_files, channel, covered, dds_rgba8, fixture, renderer, FLAT};
use nif::material::MaterialState;
use nif::props::Alpha;
use render3d::scene::Scene;
use render3d::{pbr_flags, Bound, EditKind, Renderer, ShapeId, ShapeStatus};

const CYL1: ShapeId = ShapeId(9);

fn material(r: &Renderer, id: ShapeId) -> MaterialState {
    r.shapes()
        .into_iter()
        .find(|s| s.id == id)
        .unwrap()
        .material
}

fn status(r: &Renderer, id: ShapeId) -> ShapeStatus {
    r.shapes().into_iter().find(|s| s.id == id).unwrap().status
}

fn furniture(cache: &std::path::Path) -> Renderer {
    let mut r = renderer(cache, FLAT);
    let rep = r.load(&fixture("TestNifFile_Furniture_Col_SE.nif"), &|_| None);
    assert_eq!(rep.shapes, 2, "{rep:?}");
    r
}

#[test]
fn unknown_id_is_an_error() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = furniture(cache.path());
    let e = r.set_material(ShapeId(9999), MaterialState::builtin_default(), &|_| None);
    assert!(e.is_err());
}

/// 03 T-E1: emissive + UV scale on a vanilla shape → Cbuffer, next frame differs, no compile.
#[test]
fn cbuffer_edit_changes_pixels_without_a_compile() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = furniture(cache.path());
    let before = r.render(&Scene::neutral(), 64, 64).unwrap();
    let files = cache_files(cache.path());
    let mut m = material(&r, CYL1);
    m.slsf1 |= 1 << 22; // Own_Emit: vanilla EmitColor = colour × multiple (no define reads it)
    m.params.emissive_color = [1.0, 0.0, 0.0];
    m.params.emissive_multiple = 0.5;
    m.params.uv_scale = [2.0, 2.0];
    assert_eq!(
        r.set_material(CYL1, m.clone(), &|_| None),
        Ok(EditKind::Cbuffer)
    );
    let after = r.render(&Scene::neutral(), 64, 64).unwrap();
    assert_eq!(cache_files(cache.path()), files, "nothing was compiled");
    assert!(
        channel(&after, 0) > channel(&before, 0) + 1000,
        "red rose: {} → {}",
        channel(&before, 0),
        channel(&after, 0)
    );
    assert_eq!(channel(&after, 2), channel(&before, 2), "blue untouched");
    assert_eq!(material(&r, CYL1), m);
    assert_eq!(status(&r, CYL1), ShapeStatus::Drawing);
}

#[test]
fn texture_edit_rebinds_without_a_reload() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = furniture(cache.path());
    let before = r.render(&Scene::neutral(), 64, 64).unwrap();
    let red = dds_rgba8(1, 1, &[[255, 0, 0, 255]]);
    let resolve = |p: &str| (p == "textures/standin/red.dds").then(|| red.clone());
    let mut m = material(&r, CYL1);
    m.set_slot_raw(0, "Textures\\Standin\\Red.dds");
    assert_eq!(r.set_material(CYL1, m, &resolve), Ok(EditKind::Textures));
    let after = r.render(&Scene::neutral(), 64, 64).unwrap();
    // White default (0.25 grey) → red texel (0.25, 0, 0) on cylinder_1.
    assert_eq!(channel(&after, 0), channel(&before, 0));
    assert!(channel(&after, 2) < channel(&before, 2));
}

#[test]
fn missing_texture_is_asked_for_again() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = furniture(cache.path());
    let mut m = material(&r, CYL1);
    m.set_slot_raw(0, "textures\\standin\\late.dds");
    assert_eq!(
        r.set_material(CYL1, m.clone(), &|_| None),
        Ok(EditKind::Textures)
    );
    let grey = r.render(&Scene::neutral(), 64, 64).unwrap();
    let red = dds_rgba8(1, 1, &[[255, 0, 0, 255]]);
    let resolve = |p: &str| (p == "textures/standin/late.dds").then(|| red.clone());
    // Touch another slot path so the edit is a texture pass; the path that was missing is asked for again.
    m.set_slot_raw(7, "textures\\standin\\other.dds");
    assert_eq!(r.set_material(CYL1, m, &resolve), Ok(EditKind::Textures));
    let red_px = r.render(&Scene::neutral(), 64, 64).unwrap();
    assert!(channel(&red_px, 2) < channel(&grey, 2));
}

#[test]
fn permutation_edit_compiles_synchronously() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = furniture(cache.path());
    r.render(&Scene::neutral(), 64, 64).unwrap();
    let files = cache_files(cache.path());
    let mut m = material(&r, CYL1);
    m.alpha = Some(Alpha {
        flags: 0x200,
        threshold: 0,
    }); // test only: adds DO_ALPHA_TEST
    assert_eq!(
        r.set_material(CYL1, m, &|_| None),
        Ok(EditKind::Permutation)
    );
    assert!(
        cache_files(cache.path()) > files,
        "a new pixel shader was compiled"
    );
    assert_eq!(status(&r, CYL1), ShapeStatus::Drawing);
    assert!(covered(&r.render(&Scene::neutral(), 64, 64).unwrap()) > 0);
}

/// 03 T-E4 / Review Focus #3, P1-A form: a permutation that fails to compile keeps the old draw on screen.
#[test]
fn failing_permutation_keeps_old_draw() {
    let cache = tempfile::tempdir().unwrap();
    // No STANDIN_OPAQUE_OK: any permutation without DO_ALPHA_TEST fails to compile.
    let mut r = renderer(cache.path(), &["STANDIN_FLAT"]);
    r.load(&fixture("TestNifFile_DeepGraph_SE.nif"), &|_| None);
    let peg = ShapeId(103); // PegLeft01:9, has an NiAlphaProperty
    assert_eq!(status(&r, peg), ShapeStatus::Drawing);
    let before = r.render(&Scene::neutral(), 96, 96).unwrap();
    let good = material(&r, peg);
    let mut bad = good.clone();
    bad.alpha = None;
    assert_eq!(
        r.set_material(peg, bad.clone(), &|_| None),
        Ok(EditKind::Permutation)
    );
    assert!(
        matches!(status(&r, peg), ShapeStatus::Failed { error, showing_previous: true } if error.starts_with("ps:")),
        "{:?}",
        status(&r, peg)
    );
    assert_eq!(material(&r, peg), bad, "the wanted material is reported");
    assert_eq!(
        r.render(&Scene::neutral(), 96, 96).unwrap(),
        before,
        "old draw stays"
    );
    assert_eq!(
        status(&r, ShapeId(107)),
        ShapeStatus::Drawing,
        "others unaffected"
    );
    // Back to the good material: rebuilt, same pixels.
    assert_eq!(
        r.set_material(peg, good, &|_| None),
        Ok(EditKind::Permutation)
    );
    assert_eq!(status(&r, peg), ShapeStatus::Drawing);
    assert_eq!(r.render(&Scene::neutral(), 96, 96).unwrap(), before);
}

#[test]
fn hidden_shape_is_not_drawn_and_comes_back() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = furniture(cache.path());
    let before = r.render(&Scene::neutral(), 64, 64).unwrap();
    let shown = material(&r, CYL1);
    let mut hidden = shown.clone();
    hidden.hidden = true;
    assert_eq!(
        r.set_material(CYL1, hidden, &|_| None),
        Ok(EditKind::Cbuffer)
    );
    assert_eq!(status(&r, CYL1), ShapeStatus::Hidden);
    let gone = r.render(&Scene::neutral(), 64, 64).unwrap();
    assert_ne!(gone, before);
    assert!(channel(&gone, 0) < channel(&before, 0));
    assert_eq!(
        r.set_material(CYL1, shown, &|_| None),
        Ok(EditKind::Cbuffer)
    );
    assert_eq!(status(&r, CYL1), ShapeStatus::Drawing);
    assert_eq!(r.render(&Scene::neutral(), 64, 64).unwrap(), before);
}

/// pgpsim can switch vertex colours on for a mesh without a colour stream (JSON `vertex_colors: true`): the VC
/// permutation must build and read white, not fail or read garbage.
#[test]
fn vertex_colours_without_a_colour_stream_read_white() {
    let (f, stride, bytes) = render3d::mesh::stream_for(
        &nif::geom::Mesh {
            positions: vec![[0.0; 3]; 3],
            ..Default::default()
        },
        "COLOR",
        0,
    );
    assert_eq!(stride, 4);
    assert_eq!(
        f,
        windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM
    );
    assert!(bytes.iter().all(|b| *b == 255));

    let cache = tempfile::tempdir().unwrap();
    let mut r = furniture(cache.path());
    let before = r.render(&Scene::neutral(), 64, 64).unwrap();
    let mut m = material(&r, CYL1);
    assert!(
        !m.has_vertex_colors,
        "cylinder_1 has no colour stream (vertex attributes 0x1B)"
    );
    m.has_vertex_colors = true;
    m.slsf2 |= 1 << 5; // Vertex_Colors: with has_vertex_colors → VC define (VS)
    assert_eq!(
        r.set_material(CYL1, m, &|_| None),
        Ok(EditKind::Permutation)
    );
    assert_eq!(status(&r, CYL1), ShapeStatus::Drawing);
    assert_eq!(
        r.render(&Scene::neutral(), 64, 64).unwrap(),
        before,
        "white vertex colour"
    );
}

/// The packed PBRFlags reach the shader: Rim on a PBR shape is a cbuffer edit (Subsurface = 16) and turns the
/// stand-in's flag channel on.
#[test]
fn pbr_flags_reach_the_shader() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = furniture(cache.path());
    let mut m = material(&r, CYL1);
    m.slsf2 &= !((1 << 12) | (1 << 24) | (1 << 25) | (1 << 26) | (1 << 27));
    m.slsf2 |= 1 << 23;
    m.params.emissive_color = [0.0; 3]; // D1 would add the NIF emissive colour under PBR
    assert_eq!(
        r.set_material(CYL1, m.clone(), &|_| None),
        Ok(EditKind::Permutation)
    );
    assert_eq!(pbr_flags(&m, &Bound::default()), 0);
    let plain = r.render(&Scene::neutral(), 64, 64).unwrap();
    m.slsf2 |= 1 << 26;
    assert_eq!(pbr_flags(&m, &Bound::default()), 16);
    assert_eq!(r.set_material(CYL1, m, &|_| None), Ok(EditKind::Cbuffer));
    let subsurface = r.render(&Scene::neutral(), 64, 64).unwrap();
    assert!(
        channel(&subsurface, 1) > channel(&plain, 1) + 1000,
        "green {} → {}",
        channel(&plain, 1),
        channel(&subsurface, 1)
    );
    assert_eq!(channel(&subsurface, 2), channel(&plain, 2));
}
