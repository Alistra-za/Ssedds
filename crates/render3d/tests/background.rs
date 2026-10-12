//! Backgrounds (solid grey, studio HDRI) and the HDRI ambient, on WARP with the stand-in shader.
mod common;
use common::renderer;
use nif::material::MaterialState;
use render3d::hdri;
use render3d::scene::{Background, Camera, Scene};
use render3d::{Builtin, Renderer};

fn sphere(cache: &std::path::Path) -> Renderer {
    let mut r = renderer(cache, &["STANDIN_OPAQUE_OK"]);
    let rep = r.load_builtin(Builtin::Sphere, MaterialState::builtin_default(), &|_| None);
    assert!(
        rep.errors.is_empty() && rep.compile_errors.is_empty(),
        "{rep:?}"
    );
    r
}

fn px(frame: &[u8], w: u32, x: u32, y: u32) -> [u8; 3] {
    let i = ((y * w + x) * 4) as usize;
    [frame[i], frame[i + 1], frame[i + 2]]
}

#[test]
fn default_background_is_black() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = sphere(cache.path());
    let f = r.render(&Scene::neutral(), 64, 64).unwrap();
    assert_eq!(px(&f, 64, 0, 0), [0, 0, 0]);
    assert_eq!(Scene::neutral().background, Background::Solid(0.0));
}

#[test]
fn solid_grey_fills_the_background_and_the_mesh_stays_on_top() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = sphere(cache.path());
    let mut s = Scene::neutral();
    s.background = Background::grey(0.5);
    let f = r.render(&s, 64, 64).unwrap();
    // 50 % perceptual grey is 128/255 whether or not the readback sRGB-encodes (see the next test).
    assert_eq!(px(&f, 64, 0, 0), [128, 128, 128]);
    assert_eq!(px(&f, 64, 63, 63), [128, 128, 128]);
    assert_ne!(px(&f, 64, 32, 32), [128, 128, 128], "the sphere is drawn");
    // A perceptual grey goes through sRGB -> linear: 0.5 sRGB is 0.214 linear.
    match Background::grey(0.5) {
        Background::Solid(g) => assert!((g - 0.214).abs() < 1e-3, "{g}"),
        b => panic!("{b:?}"),
    }
}

/// CS features on = the GUI sRGB-encodes the linear target; off = the target is read as is. The background
/// must look the same in both.
#[test]
fn backgrounds_look_the_same_with_and_without_srgb_encoding() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), &["STANDIN_OPAQUE_OK"]);
    let mut s = Scene::neutral();
    for bg in [Background::grey(0.5), Background::Hdri] {
        s.background = bg;
        r.set_encode_srgb(true);
        let on = r.render(&s, 48, 32).unwrap();
        r.set_encode_srgb(false);
        let off = r.render(&s, 48, 32).unwrap();
        if bg != Background::Hdri {
            assert!((px(&off, 48, 5, 5)[0] as i32 - 128).abs() <= 1, "{bg:?}");
        }
        for (a, b) in on.chunks(4).zip(off.chunks(4)) {
            for k in 0..3 {
                assert!(
                    (a[k] as i32 - b[k] as i32).abs() <= 3,
                    "{bg:?}: {a:?} vs {b:?}"
                );
            }
        }
    }
}

#[test]
fn hdri_background_varies_across_the_frame_and_is_not_black() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), &["STANDIN_OPAQUE_OK"]);
    let mut s = Scene::neutral();
    s.background = Background::Hdri;
    let f = r.render(&s, 96, 64).unwrap();
    let lit = f
        .chunks(4)
        .filter(|p| p[0] as u32 + p[1] as u32 + p[2] as u32 > 0)
        .count();
    assert!(lit > 96 * 64 * 9 / 10, "non-black: {lit} of {}", 96 * 64);
    let mut lo = 255u8;
    let mut hi = 0u8;
    for p in f.chunks(4) {
        lo = lo.min(p[1]);
        hi = hi.max(p[1]);
    }
    assert!(hi - lo > 40, "the image varies: {lo}..{hi}");
}

#[test]
fn hdri_background_matches_the_cpu_lookup_of_the_same_direction() {
    // Looking along a horizontal direction: the centre pixel is the image at dir_to_uv(that direction). Pick the
    // azimuth where the image is smoothest (and unclipped) so the 1-pixel centre can be compared with a mean.
    let img = &hdri::studio().image;
    let at = |yaw: f32| -> (Vec<f32>, [f32; 3]) {
        // The camera sits at (sin yaw, -cos yaw, 0) from the target and looks along the opposite.
        let d = [-yaw.sin(), yaw.cos(), 0.0];
        let (u, v) = hdri::dir_to_uv(d);
        let (cx, cy) = (
            (u * img.width as f32) as i32,
            (v * img.height as f32) as i32,
        );
        let mut g = vec![];
        for dy in -6..=6 {
            for dx in -6..=6 {
                let x = (cx + dx).rem_euclid(img.width as i32) as usize;
                let y = (cy + dy).clamp(0, img.height as i32 - 1) as usize;
                g.push(img.rgb[(y * img.width + x) * 3 + 1]);
            }
        }
        (g, d)
    };
    let mut best = (f32::MAX, 0.0f32);
    for k in 0..72 {
        let yaw = k as f32 * std::f32::consts::TAU / 72.0;
        let (g, _) = at(yaw);
        let mean = g.iter().sum::<f32>() / g.len() as f32;
        let var = g.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / g.len() as f32;
        if mean > 0.05 && mean < 0.8 && var < best.0 {
            best = (var, yaw);
        }
    }
    let (g, _) = at(best.1);
    let mean = g.iter().sum::<f32>() / g.len() as f32;
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), &["STANDIN_OPAQUE_OK"]);
    let mut s = Scene::neutral();
    s.background = Background::Hdri;
    s.camera = Camera::Orbit {
        target: [0.0; 3],
        yaw: best.1,
        pitch: 0.0,
        distance: 5.0,
    };
    let f = r.render(&s, 64, 64).unwrap();
    // encode_srgb is off here: the shader gives display (sRGB) values.
    let got = render3d::scene::srgb_to_linear(px(&f, 64, 32, 32)[1] as f32 / 255.0);
    assert!(
        (got - mean).abs() < 0.06,
        "rendered {got} vs image {mean} (yaw {})",
        best.1
    );
}

#[test]
fn hdri_ambient_lights_the_mesh_and_differs_from_the_neutral_ambient() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = sphere(cache.path());
    let mut s = Scene::neutral();
    s.dir_light.color = [0.0; 3]; // ambient only
    let neutral = r.render(&s, 64, 64).unwrap();
    s.background = Background::Hdri;
    let hdri_frame = r.render(&s, 64, 64).unwrap();
    // The sphere's centre pixel (not background) changes with the ambient.
    assert_ne!(px(&neutral, 64, 32, 32), px(&hdri_frame, 64, 32, 32));
    // The ambient the scene reports is the HDRI's, scaled.
    assert_eq!(s.ambient(), hdri::studio().ambient6);
    assert_ne!(s.ambient(), Scene::neutral().ambient6);
}

#[test]
fn the_mesh_is_drawn_over_the_hdri() {
    let cache = tempfile::tempdir().unwrap();
    let mut with = sphere(cache.path());
    let cache2 = tempfile::tempdir().unwrap();
    let mut without = renderer(cache2.path(), &["STANDIN_OPAQUE_OK"]);
    let mut s = Scene::neutral();
    s.background = Background::Hdri;
    let (a, b) = (
        with.render(&s, 64, 64).unwrap(),
        without.render(&s, 64, 64).unwrap(),
    );
    assert_eq!(px(&a, 64, 0, 0), px(&b, 64, 0, 0), "background corner");
    assert_ne!(px(&a, 64, 32, 32), px(&b, 64, 32, 32), "sphere in front");
}
