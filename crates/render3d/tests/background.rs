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
    assert_eq!(Scene::neutral().background, Background::Solid([0.0; 3]));
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
        Background::Solid(g) => assert!(g.iter().all(|c| (c - 0.214).abs() < 1e-3), "{g:?}"),
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

/// The horizontal camera yaw whose view direction lands on the smoothest unclipped part of the image, and the image
/// mean there (a 13×13 window), so a 1-pixel centre can be compared with it.
fn smooth_azimuth() -> (f32, f32) {
    let img = &hdri::studio().image;
    let at = |yaw: f32| -> Vec<f32> {
        // The camera sits at (sin yaw, -cos yaw, 0) from the target and looks along the opposite.
        let (u, v) = hdri::dir_to_uv([-yaw.sin(), yaw.cos(), 0.0]);
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
        g
    };
    let mut best = (f32::MAX, 0.0f32, 0.0f32);
    for k in 0..72 {
        let yaw = k as f32 * std::f32::consts::TAU / 72.0;
        let g = at(yaw);
        let mean = g.iter().sum::<f32>() / g.len() as f32;
        let var = g.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / g.len() as f32;
        if mean > 0.05 && mean < 0.8 && var < best.0 {
            best = (var, yaw, mean);
        }
    }
    (best.1, best.2)
}

/// The centre pixel of a frame looking along camera yaw `cam_yaw` with the environment turned by `env_yaw`.
fn centre_green(cam_yaw: f32, env_yaw: f32) -> f32 {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), &["STANDIN_OPAQUE_OK"]);
    let mut s = Scene::neutral();
    s.background = Background::Hdri;
    s.env_yaw = env_yaw;
    s.camera = Camera::Orbit {
        target: [0.0; 3],
        yaw: cam_yaw,
        pitch: 0.0,
        distance: 5.0,
    };
    let f = r.render(&s, 64, 64).unwrap();
    // encode_srgb is off here: the shader gives display (sRGB) values.
    render3d::scene::srgb_to_linear(px(&f, 64, 32, 32)[1] as f32 / 255.0)
}

#[test]
fn hdri_background_matches_the_cpu_lookup_of_the_same_direction() {
    let (yaw, mean) = smooth_azimuth();
    let got = centre_green(yaw, 0.0);
    assert!(
        (got - mean).abs() < 0.06,
        "rendered {got} vs image {mean} (yaw {yaw})"
    );
}

/// The sign test, backdrop half: turning the environment by +π/2 (the sun's sense) moves what was straight ahead a
/// quarter turn on, where the camera turned by the same quarter finds it again.
#[test]
fn a_turned_hdri_shows_the_same_spot_a_quarter_turn_on() {
    let (yaw, mean) = smooth_azimuth();
    let q = std::f32::consts::FRAC_PI_2;
    let got = centre_green(yaw + q, q);
    assert!(
        (got - mean).abs() < 0.06,
        "rendered {got} vs image {mean} (yaw {yaw} + π/2)"
    );
}

#[test]
fn an_rgb_background_clears_to_that_colour_with_and_without_encoding() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = sphere(cache.path());
    let mut s = Scene::neutral();
    s.background = Background::rgb([1.0, 0.0, 0.0]);
    for encode in [true, false] {
        r.set_encode_srgb(encode);
        let f = r.render(&s, 64, 64).unwrap();
        assert_eq!(px(&f, 64, 0, 0), [255, 0, 0], "encode {encode}");
    }
    // Clamped per channel before the sRGB decode.
    assert_eq!(
        Background::rgb([0.5, 2.0, -1.0]),
        Background::Solid([render3d::scene::srgb_to_linear(0.5), 1.0, 0.0])
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
