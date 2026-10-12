//! Sun brightness and the one point light, on WARP with the stand-in shader (which reads the same vanilla
//! point-light fields as CS's forward permutation), plus the real-CS check (ignored, needs SDC_ACHERUS).
mod common;
use common::renderer;
use nif::material::MaterialState;
use render3d::scene::{Camera, PointLight, Scene, View};
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

const N: u32 = 64;

fn green(frame: &[u8], x: u32, y: u32) -> i32 {
    frame[((y * N + x) * 4 + 1) as usize] as i32
}

fn total(frame: &[u8]) -> i64 {
    frame.chunks(4).map(|p| p[1] as i64).sum()
}

#[test]
fn defaults_are_the_unchanged_look() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = sphere(cache.path());
    let base = r.render(&Scene::neutral(), N, N).unwrap();
    let mut s = Scene::neutral();
    s.sun_brightness = 1.0;
    s.point_light = PointLight {
        brightness: 0.0,
        ..PointLight::around(r.bounds().unwrap(), 0.0, 2.0)
    };
    assert_eq!(r.render(&s, N, N).unwrap(), base, "point light off, sun x1");
}

#[test]
fn sun_brightness_scales_the_lit_pixels() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = sphere(cache.path());
    let mut s = Scene::neutral();
    let one = r.render(&s, N, N).unwrap();
    s.sun_brightness = 2.0;
    let two = r.render(&s, N, N).unwrap();
    s.sun_brightness = 0.0;
    let zero = r.render(&s, N, N).unwrap();
    assert!(total(&two) > total(&one), "2x is brighter");
    assert!(total(&zero) < total(&one), "0x is darker");
    assert!(total(&two) > total(&one) + 100);
}

#[test]
fn point_light_brightens_the_side_facing_it() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = sphere(cache.path());
    let bounds = r.bounds().unwrap();
    let mut s = Scene::neutral();
    s.dir_light.color = [0.0; 3];
    let off = r.render(&s, N, N).unwrap();
    // 2.4 radii from the centre with 2.2 radii of falloff: the near side is lit, the far side is out of reach.
    s.point_light = PointLight::around(bounds, 2.0, 2.2);
    let on = r.render(&s, N, N).unwrap();
    assert!(total(&on) > total(&off) + 100, "the light adds light");
    // Which half of the screen is the light on?
    let v = View::new(&s.camera, Some(bounds), N, N);
    let p = s.point_light.position;
    let c = [p[0] - v.eye[0], p[1] - v.eye[1], p[2] - v.eye[2], 1.0];
    let clip_x: f32 = (0..4).map(|k| v.view_proj[0][k] * c[k]).sum();
    let clip_w: f32 = (0..4).map(|k| v.view_proj[3][k] * c[k]).sum();
    let light_right = clip_x / clip_w > 0.0;
    let half = |frame: &[u8], right: bool| -> i64 {
        (0..N)
            .flat_map(|y| (0..N).map(move |x| (x, y)))
            .filter(|(x, _)| (*x >= N / 2) == right)
            .map(|(x, y)| green(frame, x, y) as i64)
            .sum()
    };
    let gain = |right| half(&on, right) - half(&off, right);
    assert!(
        gain(light_right) > 2 * gain(!light_right).max(1),
        "gain on the light's side {} vs far side {}",
        gain(light_right),
        gain(!light_right)
    );
}

#[test]
fn point_light_radius_limits_its_reach() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = sphere(cache.path());
    let bounds = r.bounds().unwrap();
    let mut s = Scene::neutral();
    s.dir_light.color = [0.0; 3];
    let off = r.render(&s, N, N).unwrap();
    s.point_light = PointLight::around(bounds, 3.0, 0.2); // reaches 0.2 radii: nothing of the sphere
    assert_eq!(r.render(&s, N, N).unwrap(), off);
}

/// The resolved / dirty-upload path and the by-name writer agree with the new fields, through a light change,
/// a camera move (the point light is camera-relative) and an unchanged frame.
#[test]
fn point_light_pixels_match_the_by_name_writer() {
    let (ca, cb) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (mut a, mut b) = (sphere(ca.path()), sphere(cb.path()));
    b.set_reference_writer(true);
    let bounds = a.bounds().unwrap();
    let mut s = Scene::neutral();
    let mut same = |s: &Scene, what: &str| {
        assert_eq!(
            a.render(s, N, N).unwrap(),
            b.render(s, N, N).unwrap(),
            "{what}"
        );
    };
    same(&s, "first");
    s.point_light = PointLight::around(bounds, 1.5, 3.0);
    same(&s, "point light on");
    same(&s, "unchanged");
    s.camera = Camera::Orbit {
        target: bounds.0,
        yaw: 2.2,
        pitch: 0.4,
        distance: 3.0 * bounds.1,
    };
    same(&s, "camera move");
    s.sun_brightness = 2.5;
    s.point_light.radius *= 0.5;
    same(&s, "sun and radius");
}

/// The same light on the user's real Community Shaders `Lighting.hlsl` (forward permutation, no
/// LIGHT_LIMIT_FIX, so the vanilla `PointLightPosition` loop): a point light must brighten the sphere.
/// `$env:SDC_ACHERUS='C:\_mount\moddingvol\Acherus'; cargo test -p render3d --test lights -- --ignored`
#[test]
#[ignore = "needs SDC_ACHERUS (MO2 instance root) with the CS 2026-10-03 mod"]
fn point_light_brightens_the_real_cs_lighting() {
    use render3d::device::Gpu;
    let root = std::env::var("SDC_ACHERUS").expect("set SDC_ACHERUS to the MO2 instance root");
    let shaders = std::path::PathBuf::from(format!(
        "{root}/mods/CommunityShaders_AIO-2026-10-03T06-46Z/Shaders"
    ));
    let fp = render3d::locate::fingerprint(&shaders, None).unwrap();
    let cache = tempfile::tempdir().unwrap();
    let mut r =
        Renderer::with_gpu(Gpu::new_warp().unwrap(), fp, cache.path().to_path_buf()).unwrap();
    let rep = r.load_builtin(Builtin::Sphere, MaterialState::builtin_default(), &|_| None);
    assert!(
        rep.errors.is_empty() && rep.compile_errors.is_empty(),
        "{rep:?}"
    );
    let mut s = Scene::neutral();
    s.dir_light.color = [0.0; 3];
    let off = r.render(&s, N, N).unwrap();
    s.point_light = PointLight::around(r.bounds().unwrap(), 2.0, 3.0);
    let on = r.render(&s, N, N).unwrap();
    let (t_off, t_on) = (total(&off), total(&on));
    println!("green sum: off {t_off}, on {t_on}");
    assert!(t_on > t_off + 100, "off {t_off}, on {t_on}");
}
