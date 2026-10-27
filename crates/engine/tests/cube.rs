//! Cubemap sub-type and its `cubemap` profile section (cubemap support v1, DECISIONS Note DN).
mod cube_support;
use cube_support::*;
use engine::api::*;
use engine::codes::Code;
use engine::config::*;
use engine::cube::layout::{self, cells, detect, import_raw};
use engine::cube::plan::face_tag;
use engine::cube::FACES;
use engine::events::*;
use engine::image::Rgba32F;
use engine::overrides::parse_set;
use engine::policy::{allowed_encodings, AcceptSuggestions, JobStatus, ResolvedJob};
use engine::presets::PresetRegistry;
use engine::shader::Shader;
use std::path::Path;
use std::sync::Arc;

fn builtin_ids(reg: &PresetRegistry) -> Vec<String> {
    let mut ids: Vec<String> = reg.selectable().iter().map(|p| p.id.clone()).collect();
    ids.push("sdc-base".into());
    ids
}

#[test]
fn default_cubemap_section_is_not_serialised() {
    let reg = PresetRegistry::builtin().unwrap();
    for id in builtin_ids(&reg) {
        for t in [Target::Cs, Target::Vanilla] {
            for st in Subtype::ALL {
                let p = reg.profile(&id, t, st).unwrap();
                let v = serde_json::to_value(&p).unwrap();
                assert!(v.get("cubemap").is_none(), "{id}/{t:?}/{st:?}");
            }
        }
    }
}

#[test]
fn cubemap_profile_defaults() {
    let reg = PresetRegistry::builtin().unwrap();
    let p = reg
        .profile("cs-quality", Target::Cs, Subtype::Cubemap)
        .unwrap();
    assert_eq!(p.format, Format::Bc7);
    assert_eq!(
        (p.color.content, p.color.tag),
        (ColorContent::Srgb, ColorTag::Unorm)
    );
    assert_eq!(p.alpha.policy, AlphaPolicy::Remove);
    assert_eq!(p.mips.levels, MipLevels::Keyword(MipKeyword::Full));
    assert_eq!(p.category, Category::Color);
}

#[test]
fn cubemap_encodings_are_unorm_bc1_bc3_bc7_rgba8() {
    for t in [Target::Cs, Target::Vanilla] {
        assert_eq!(
            allowed_encodings(Subtype::Cubemap, t),
            vec![
                (Format::Bc1, ColorTag::Unorm),
                (Format::Bc3, ColorTag::Unorm),
                (Format::Bc7, ColorTag::Unorm),
                (Format::Rgba8, ColorTag::Unorm),
            ],
            "{t:?}"
        );
    }
}

#[test]
fn detect_exact_ratios_only() {
    assert_eq!(detect(1024, 768), Some(CubeLayout::Hcross));
    assert_eq!(detect(768, 1024), Some(CubeLayout::Vcross));
    assert_eq!(detect(1536, 256), Some(CubeLayout::Hstrip));
    assert_eq!(detect(256, 1536), Some(CubeLayout::Vstrip));
    assert_eq!(detect(2048, 1024), Some(CubeLayout::Equirect));
    for (w, h) in [
        (512, 512),
        (1024, 2048),
        (4096, 2049),
        (1025, 768),
        (6145, 1024),
    ] {
        assert_eq!(detect(w, h), None, "{w}x{h}");
    }
}

#[test]
fn every_raw_layout_round_trips_the_arrow_cube() {
    let faces = arrow_faces(64);
    let cases = [
        (CubeLayout::Hcross, hcross(&faces)),
        (CubeLayout::Vcross, vcross(&faces, true)),
        (CubeLayout::Hstrip, hstrip(&faces)),
        (CubeLayout::Vstrip, vstrip(&faces)),
    ];
    for (layout, img) in cases {
        let got = import_raw(&img, layout, &CubemapSpec::default(), true).unwrap();
        assert!(got == faces, "{layout:?}");
        assert_eq!(cells(layout, img.width, img.height).unwrap().0, 64);
    }
}

#[test]
fn vcross_negz_rotate_toggle() {
    let faces = arrow_faces(32);
    let spec = CubemapSpec::default();
    assert!(import_raw(&vcross(&faces, true), CubeLayout::Vcross, &spec, true).unwrap() == faces);
    let spec = CubemapSpec {
        vcross_negz_rotate: false,
        ..CubemapSpec::default()
    };
    assert!(import_raw(&vcross(&faces, false), CubeLayout::Vcross, &spec, true).unwrap() == faces);
    // Left on, the unrotated -Z comes out turned.
    let got = import_raw(
        &vcross(&faces, false),
        CubeLayout::Vcross,
        &CubemapSpec::default(),
        true,
    )
    .unwrap();
    assert!(got[5] == turn180(&faces[5]));
}

#[test]
fn face_rotate_and_flip_correct_a_turned_face() {
    let faces = arrow_faces(32);
    let n = 32;
    // A clockwise quarter turn sends the top (white) row to the right column.
    let mut turned = faces.clone();
    let mut t = Rgba32F::new(n, n, [0.0; 4]);
    for y in 0..n {
        for x in 0..n {
            *t.at_mut(x, y) = faces[2].at(y, n - 1 - x);
        }
    }
    assert_eq!(t.at(n - 1, 5), [1.0; 4]);
    turned[2] = t;
    let mut spec = CubemapSpec::default();
    spec.face_rotate[2] = 270;
    assert!(import_raw(&hstrip(&turned), CubeLayout::Hstrip, &spec, false).unwrap() == faces);

    let mut flipped = faces.clone();
    for y in 0..n {
        for x in 0..n {
            *flipped[3].at_mut(x, y) = faces[3].at(n - 1 - x, y);
        }
    }
    assert!(flipped[3] != faces[3]);
    let mut spec = CubemapSpec::default();
    spec.face_flip[3] = FaceFlip::H;
    assert!(import_raw(&hstrip(&flipped), CubeLayout::Hstrip, &spec, false).unwrap() == faces);
    // And a vertical flip is its own inverse on a v-flipped face.
    let mut vflipped = faces.clone();
    for y in 0..n {
        for x in 0..n {
            *vflipped[0].at_mut(x, y) = faces[0].at(x, n - 1 - y);
        }
    }
    let mut spec = CubemapSpec::default();
    spec.face_flip[0] = FaceFlip::V;
    assert!(import_raw(&hstrip(&vflipped), CubeLayout::Hstrip, &spec, false).unwrap() == faces);
}

#[test]
fn photo_shaped_like_a_cross_is_refused_when_auto() {
    let (w, h) = (64, 48);
    let mut seed = 12345u32;
    let mut img = Rgba32F::new(w, h, [0.0; 4]);
    for p in &mut img.px {
        for c in &mut p[..3] {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            *c = (seed >> 24) as f32 / 255.0;
        }
        p[3] = 1.0;
    }
    let spec = CubemapSpec::default();
    let e = import_raw(&img, CubeLayout::Hcross, &spec, true).unwrap_err();
    assert!(e.contains("cubemap.layout"), "{e}");
    assert!(import_raw(&img, CubeLayout::Hcross, &spec, false).is_ok());
    // Faces is not a single-image layout.
    assert!(import_raw(&img, CubeLayout::Faces, &spec, false).is_err());
}

#[test]
fn face_size_rule() {
    let reg = PresetRegistry::builtin().unwrap();
    let mut p = reg
        .profile("cs-quality", Target::Cs, Subtype::Cubemap)
        .unwrap();
    assert_eq!(layout::face_size(&p, CubeLayout::Hcross, (600, 450)), 128);
    assert_eq!(layout::face_size(&p, CubeLayout::Equirect, (512, 256)), 128);
    assert_eq!(
        layout::face_size(&p, CubeLayout::Equirect, (1000, 500)),
        128
    );
    p.cubemap.face_size = Some(64);
    assert_eq!(layout::face_size(&p, CubeLayout::Hcross, (600, 450)), 64);
}

// ---- Task 13: equirect panoramas, yaw and mirror (DECISIONS DN14) ----

use engine::cube::latlong::{dir_to_uv, uv_to_dir};
use engine::cube::sample::fetch;
use engine::cube::world::{to_cube, UP};
use engine::cube::{cube_dir, dir_to_face, filter, import_dir, Cube};

fn near3(a: [f32; 3], b: [f32; 3]) -> bool {
    (0..3).all(|k| (a[k] - b[k]).abs() < 1e-4)
}

#[test]
fn pano_mapping_matches_the_in_game_check() {
    // results.md "Second check": u 0 west, 0.25 north, 0.5 east, 0.75 south (clockwise seen from above), row 0 up.
    let cases = [
        ((0.0, 0.5), [-1.0, 0.0, 0.0]),
        ((0.25, 0.5), [0.0, 1.0, 0.0]),
        ((0.5, 0.5), [1.0, 0.0, 0.0]),
        ((0.75, 0.5), [0.0, -1.0, 0.0]),
        ((0.3, 0.0), [0.0, 0.0, 1.0]),
    ];
    for ((u, v), want) in cases {
        let d = uv_to_dir(u, v, 0.0);
        assert!(near3(d, want), "({u},{v}) -> {d:?}");
    }
    for (u, v) in [(0.1, 0.3), (0.6, 0.7), (0.9, 0.45)] {
        let (u2, v2) = dir_to_uv(uv_to_dir(u, v, 0.0), 0.0);
        assert!((u - u2).abs() < 1e-4 && (v - v2).abs() < 1e-4, "({u},{v})");
    }
}

/// A `w` x `h` equirect of `c`, one fetch per pixel centre.
fn pano_of(c: &Cube, w: u32, h: u32) -> Rgba32F {
    let mut img = Rgba32F::new(w, h, [0.0; 4]);
    for y in 0..h {
        for x in 0..w {
            let d = uv_to_dir(
                (x as f32 + 0.5) / w as f32,
                (y as f32 + 0.5) / h as f32,
                0.0,
            );
            *img.at_mut(x, y) = fetch(c, to_cube(d));
        }
    }
    img
}

fn rgb_psnr(a: &Rgba32F, b: &Rgba32F) -> f64 {
    let mse =
        a.px.iter()
            .zip(&b.px)
            .flat_map(|(p, q)| (0..3).map(move |k| ((p[k] - q[k]) as f64).powi(2)))
            .sum::<f64>()
            / (3 * a.px.len()) as f64;
    10.0 * (1.0 / mse).log10()
}

#[test]
fn equirect_of_a_cube_round_trips() {
    let arrow = Cube {
        size: 64,
        faces: arrow_faces(64),
    };
    let got = import_dir::equirect(&pano_of(&arrow, 512, 256), 64);
    assert_eq!(got.size, 64);
    let q: Vec<f64> = (0..6)
        .map(|f| rgb_psnr(&got.faces[f], &arrow.faces[f]))
        .collect();
    // DN14: the arrow cube's saturated hard edges (face borders, bands) soften by about a texel through the two
    // bilinear passes, which caps whole-face PSNR near 27-28 dB; away from those edges the round trip is exact.
    assert!(q.iter().all(|&q| q > 26.0), "{q:.2?}");
    let away = |v: u32| v > 2 && v < 61 && !(5..=10).contains(&v);
    let inner = |img: &Rgba32F| {
        let mut o = Rgba32F::new(1, 0, [0.0; 4]);
        for y in (0..64).filter(|&y| away(y)) {
            for x in (0..64).filter(|&x| away(x)) {
                o.px.push(img.at(x, y));
            }
        }
        o
    };
    for f in 0..6 {
        let q = rgb_psnr(&inner(&got.faces[f]), &inner(&arrow.faces[f]));
        assert!(q > 40.0, "face {f} away from edges: {q:.2} dB");
    }
}

/// The centre texel of the face world direction `w` lands on.
fn centre(c: &Cube, w: [f32; 3]) -> [f32; 4] {
    let n = c.size;
    c.faces[dir_to_face(to_cube(w)).0].at(n / 2, n / 2)
}

#[test]
fn equirect_top_row_lands_on_up() {
    let mut pano = Rgba32F::new(256, 128, [0.0, 0.0, 0.0, 1.0]);
    for y in 0..32 {
        for x in 0..256 {
            *pano.at_mut(x, y) = [1.0; 4];
        }
    }
    let c = import_dir::equirect(&pano, 32);
    let up = c.faces[dir_to_face(UP).0].at(16, 16);
    let down = c.faces[dir_to_face(UP.map(|v| -v)).0].at(16, 16);
    assert!(up[0] > 0.99, "{up:?}");
    assert!(down[0] < 0.01, "{down:?}");
}

const EAST: [f32; 3] = [1.0, 0.0, 0.0];
const WEST: [f32; 3] = [-1.0, 0.0, 0.0];
const NORTH: [f32; 3] = [0.0, 1.0, 0.0];
const SOUTH: [f32; 3] = [0.0, -1.0, 0.0];

const UPW: [f32; 3] = [0.0, 0.0, 1.0];
const DOWN: [f32; 3] = [0.0, 0.0, -1.0];

/// A black cube with patches around world east (red), north (green) and up (blue).
fn markers(n: u32) -> Cube {
    let faces = std::array::from_fn(|f| {
        let mut img = Rgba32F::new(n, n, [0.0, 0.0, 0.0, 1.0]);
        for y in 0..n {
            for x in 0..n {
                let d = engine::cube::world::to_world(cube_dir(f, x, y, n));
                let p = img.at_mut(x, y);
                for k in 0..3 {
                    if d[k] > 0.9 {
                        p[k] = 1.0;
                    }
                }
            }
        }
        img
    });
    Cube { size: n, faces }
}

/// Each world direction's face centre is the colour of the marker expected there (black for none).
fn assert_markers(c: &Cube, red: [f32; 3], green: [f32; 3]) {
    for w in [EAST, WEST, NORTH, SOUTH, UPW, DOWN] {
        let want = [w == red, w == green, w == UPW].map(|on| if on { 1.0 } else { 0.0 });
        let got = centre(c, w);
        assert!(
            (0..3).all(|k| (got[k] - want[k]).abs() < 0.1),
            "{w:?}: {got:?}, want {want:?}"
        );
    }
}

#[test]
fn yaw_quarter_turn_moves_east_to_south() {
    assert_markers(&markers(32), EAST, NORTH);
    // Clockwise seen from above: east to south, north to east, up stays up.
    assert_markers(&filter::yaw(&markers(32), 90.0), SOUTH, EAST);
    assert_eq!(filter::yaw(&markers(8), 0.0), markers(8));
}

#[test]
fn mirror_swaps_east_and_west() {
    // North and up stay put.
    assert_markers(&filter::mirror(&markers(32)), WEST, NORTH);
}

/// A smooth field of world direction: no hard edges, so any sub-texel offset in the importer shows.
fn smooth(d: [f32; 3]) -> [f32; 4] {
    [
        0.5 + 0.5 * (6.0 * d[0]).sin(),
        0.5 + 0.5 * (6.0 * d[1]).sin(),
        0.5 + 0.5 * (6.0 * d[2]).sin(),
        1.0,
    ]
}

#[test]
fn equirect_of_a_smooth_field_round_trips() {
    let (w, h, n) = (512, 256, 64);
    let mut pano = Rgba32F::new(w, h, [0.0; 4]);
    for y in 0..h {
        for x in 0..w {
            let d = uv_to_dir(
                (x as f32 + 0.5) / w as f32,
                (y as f32 + 0.5) / h as f32,
                0.0,
            );
            *pano.at_mut(x, y) = smooth(d);
        }
    }
    let got = import_dir::equirect(&pano, n);
    let q: Vec<f64> = (0..6)
        .map(|f| {
            let mut want = Rgba32F::new(n, n, [0.0; 4]);
            for y in 0..n {
                for x in 0..n {
                    let d = engine::cube::world::to_world(cube_dir(f, x, y, n));
                    *want.at_mut(x, y) = smooth(d);
                }
            }
            rgb_psnr(&got.faces[f], &want)
        })
        .collect();
    eprintln!("smooth round trip, per-face PSNR: {q:.2?}");
    assert!(q.iter().all(|&q| q > 35.0), "{q:.2?}");
}

#[test]
fn explicit_equirect_reads_any_size() {
    // 64x33 is a small 4096x2049: unclear when guessed, read as a panorama when set.
    assert_eq!(detect(64, 33), None);
    assert!(layout::fits(CubeLayout::Equirect, 64, 33));
    let d = tempfile::tempdir().unwrap();
    png(
        d.path(),
        "textures/cubemaps/pano.png",
        64,
        33,
        [90, 120, 200],
    );
    let (p, ev) = plan_of(req(
        d.path(),
        &["file:**/pano.png.cubemap.layout=equirect"],
        true,
    ));
    let (s, run_ev) = run(p);
    assert_eq!(s.converted, 1, "{ev:#?} {run_ev:#?}");
}

#[test]
fn default_panorama_is_not_resized() {
    let d = tempfile::tempdir().unwrap();
    png(
        d.path(),
        "textures/cubemaps/pano.png",
        512,
        256,
        [90, 120, 200],
    );
    let (p, ev) = plan_of(req(d.path(), &[], true));
    let j = job(&p, "pano.dds");
    assert_eq!(j.cube.as_ref().unwrap().layout, CubeLayout::Equirect);
    assert!(
        !j.warnings.iter().any(|w| w.code == Code::DimResized),
        "{:?} {ev:#?}",
        j.warnings
    );
    // A non-power-of-two width: the 250 cell is named, not min(w, h).
    let d = tempfile::tempdir().unwrap();
    png(
        d.path(),
        "textures/cubemaps/pano.png",
        1000,
        500,
        [90, 120, 200],
    );
    let p = plan(d.path(), &[]);
    let w = job(&p, "pano.dds")
        .warnings
        .iter()
        .find(|w| w.code == Code::DimResized)
        .expect("DIM-RESIZED");
    assert_eq!(w.message, "face 250 → 128");
}

#[test]
fn near_two_to_one_is_unclear() {
    assert_eq!(detect(4096, 2049), None);
    assert_eq!(detect(4096, 2048), Some(CubeLayout::Equirect));
}

#[test]
fn face_tags_parse_both_spellings() {
    assert_eq!(face_tag("chrome_e_posx"), Some(("chrome_e", 0)));
    assert_eq!(face_tag("chrome_e_NZ"), Some(("chrome_e", 5)));
    assert_eq!(face_tag("chrome_e"), None);
    assert_eq!(face_tag("_px"), None);
}

// ---- Task 7: batch planning of cube jobs (DECISIONS DN8) ----

fn png(dir: &Path, rel: &str, w: u32, h: u32, rgb: [u8; 3]) {
    let p = dir.join("in").join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    image::RgbImage::from_fn(w, h, |x, y| {
        image::Rgb([
            rgb[0],
            rgb[1].wrapping_add((x * 2) as u8),
            rgb[2] ^ (y as u8),
        ])
    })
    .save(p)
    .unwrap();
}

/// The six faces `<prefix>_posx.png` .. `<prefix>_negz.png`, `n` x `n`.
fn faces(dir: &Path, prefix: &str, n: u32) {
    for (k, f) in FACES.iter().enumerate() {
        png(
            dir,
            &format!("{prefix}_{f}.png"),
            n,
            n,
            [40 * k as u8, 90, 200],
        );
    }
}

fn req(dir: &Path, sets: &[&str], accept: bool) -> BatchRequest {
    let mut r = BatchRequest::new(vec![dir.join("in")], dir.join("out"), Target::Cs);
    r.recursive = true;
    r.overrides = sets.iter().map(|s| parse_set(s).unwrap()).collect();
    if accept {
        r.accept = AcceptSuggestions::High;
    }
    r
}

fn plan_of(r: BatchRequest) -> (Plan, Vec<EngineEvent>) {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "cube");
    let p = Engine::new()
        .unwrap()
        .plan(r, &rep, &CancelToken::default())
        .unwrap();
    (p, sink.take())
}

fn plan(dir: &Path, sets: &[&str]) -> Plan {
    plan_of(req(dir, sets, true)).0
}

fn run(p: Plan) -> (BatchSummary, Vec<EngineEvent>) {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "cube");
    let s = Engine::new()
        .unwrap()
        .run(p, &rep, &CancelToken::default())
        .unwrap();
    (s, sink.take())
}

fn name(p: &Path) -> String {
    p.file_name().unwrap().to_string_lossy().into_owned()
}

fn job<'a>(p: &'a Plan, out: &str) -> &'a ResolvedJob {
    p.jobs
        .iter()
        .find(|j| name(&j.output) == out)
        .unwrap_or_else(|| panic!("no job writing {out}: {:#?}", p.jobs))
}

#[test]
fn six_faces_become_one_job() {
    let d = tempfile::tempdir().unwrap();
    faces(d.path(), "textures/a/chrome_e", 32);
    let p = plan(d.path(), &[]);
    assert_eq!(p.jobs.len(), 1, "{:#?}", p.jobs);
    let j = job(&p, "chrome_e.dds");
    assert_eq!(j.status, JobStatus::Ready, "{:#?}", j.warnings);
    assert_eq!(j.game_path, "textures/a/chrome_e.png");
    let cj = j.cube.as_ref().unwrap();
    assert_eq!(cj.layout, CubeLayout::Faces);
    let got: Vec<(String, Option<usize>)> =
        cj.sources.iter().map(|s| (name(&s.path), s.face)).collect();
    let want: Vec<(String, Option<usize>)> = FACES
        .iter()
        .enumerate()
        .map(|(k, f)| (format!("chrome_e_{f}.png"), Some(k)))
        .collect();
    assert_eq!(got, want);
    assert_eq!(j.input, cj.sources[0].path);
    assert!(cj.sources.iter().all(|s| s.stamp.is_some()));
}

#[test]
fn cubemaps_folder_faces_group_without_e() {
    let d = tempfile::tempdir().unwrap();
    faces(d.path(), "textures/cubemaps/sky", 32);
    let p = plan(d.path(), &[]);
    assert_eq!(p.jobs.len(), 1, "{:#?}", p.jobs);
    assert!(job(&p, "sky.dds").cube.is_some());
}

/// The CUBE-FACES message of the one job planned from `d`.
fn fault(d: &Path, sets: &[&str]) -> String {
    let p = plan(d, sets);
    assert_eq!(p.jobs.len(), 1, "{:#?}", p.jobs);
    let j = &p.jobs[0];
    assert_eq!(
        j.status,
        JobStatus::Error {
            codes: vec![Code::CubeFaces]
        }
    );
    j.warnings
        .iter()
        .find(|w| w.code == Code::CubeFaces)
        .unwrap()
        .message
        .clone()
}

/// Review Focus 3.
#[test]
fn broken_face_sets_are_cube_faces_errors() {
    let d = tempfile::tempdir().unwrap();
    faces(d.path(), "textures/a/chrome_e", 32);
    std::fs::remove_file(d.path().join("in/textures/a/chrome_e_negy.png")).unwrap();
    assert!(fault(d.path(), &[]).contains("missing negy"));

    let d = tempfile::tempdir().unwrap();
    faces(d.path(), "textures/a/chrome_e", 32);
    png(d.path(), "textures/a/chrome_e_px.png", 32, 32, [1, 2, 3]);
    assert!(fault(d.path(), &[]).contains("two files for posx"));

    let d = tempfile::tempdir().unwrap();
    faces(d.path(), "textures/a/chrome_e", 32);
    png(d.path(), "textures/a/chrome_e_posy.png", 64, 64, [1, 2, 3]);
    assert!(fault(d.path(), &[]).contains("faces differ in size"));

    let d = tempfile::tempdir().unwrap();
    faces(d.path(), "textures/a/chrome_e", 32);
    png(d.path(), "textures/a/chrome_e_posz.png", 64, 32, [1, 2, 3]);
    assert!(fault(d.path(), &[]).contains("not square"));

    let d = tempfile::tempdir().unwrap();
    faces(d.path(), "textures/a/chrome_e", 32);
    let m = fault(d.path(), &["file:**/chrome_e_negz.png.cubemap.blur=0.5"]);
    assert!(m.contains("faces resolve to different settings"), "{m}");
}

/// Fix round 1: every discovered face file ends with a terminal event, for a good and a broken set.
#[test]
fn every_face_file_gets_a_terminal_event() {
    for broken in [false, true] {
        let d = tempfile::tempdir().unwrap();
        faces(d.path(), "textures/a/chrome_e", 32);
        if broken {
            std::fs::remove_file(d.path().join("in/textures/a/chrome_e_negy.png")).unwrap();
        }
        let (p, plan_ev) = plan_of(req(d.path(), &[], true));
        let (_, run_ev) = run(p);
        let all: Vec<&EngineEvent> = plan_ev.iter().chain(&run_ev).collect();
        // A GUI row is keyed by (path, output key): it ends on a terminal event for that key, or when its path is
        // consumed into another output (a PackedSource skip).
        let row = |e: &EngineEvent| (e.path.clone(), e.job.clone());
        let found: Vec<_> = all
            .iter()
            .filter(|e| matches!(e.kind, EventKind::FileDiscovered { .. }))
            .map(|e| row(e))
            .collect();
        let ended: std::collections::BTreeSet<_> = all
            .iter()
            .filter(|e| {
                matches!(
                    e.kind,
                    EventKind::FileSkipped { .. }
                        | EventKind::FileDone { .. }
                        | EventKind::FileFailed { .. }
                )
            })
            .map(|e| row(e))
            .collect();
        let consumed: std::collections::BTreeSet<_> = all
            .iter()
            .filter(|e| {
                matches!(
                    e.kind,
                    EventKind::FileSkipped {
                        kind: SkipKind::PackedSource,
                        ..
                    }
                )
            })
            .map(|e| e.path.clone())
            .collect();
        assert_eq!(found.len(), if broken { 5 } else { 6 });
        let open: Vec<_> = found
            .iter()
            .filter(|r| !ended.contains(*r) && !consumed.contains(&r.0))
            .collect();
        assert!(
            open.is_empty(),
            "broken={broken}: no terminal event for {open:?}"
        );
    }
}

/// Fix round 1: a merged Blocked status carries the warning of the face that blocks it.
#[test]
fn a_blocking_face_warning_reaches_the_cube_job() {
    let d = tempfile::tempdir().unwrap();
    faces(d.path(), "textures/a/chrome_e", 32);
    let p = d.path().join("in/textures/a/chrome_e_negz.png");
    image::RgbaImage::from_fn(32, 32, |x, _| image::Rgba([9, 9, 9, (x * 8) as u8]))
        .save(&p)
        .unwrap();
    let pl = plan(d.path(), &[]);
    let j = job(&pl, "chrome_e.dds");
    assert!(
        matches!(&j.status, JobStatus::Blocked { codes } if codes.contains(&Code::LossAlphaData)),
        "{:?}",
        j.status
    );
    assert!(j.warnings.iter().any(|w| w.code == Code::LossAlphaData));
}

#[test]
fn face_status_is_merged() {
    let d = tempfile::tempdir().unwrap();
    faces(d.path(), "textures/a/chrome_e", 32);
    let sets: Vec<String> = FACES[..5]
        .iter()
        .map(|f| format!("file:**/chrome_e_{f}.png.subtype=cubemap"))
        .collect();
    let sets: Vec<&str> = sets.iter().map(String::as_str).collect();
    let (p, _) = plan_of(req(d.path(), &sets, false));
    assert_eq!(p.jobs.len(), 1, "{:#?}", p.jobs);
    assert!(
        matches!(job(&p, "chrome_e.dds").status, JobStatus::Unresolved { .. }),
        "{:?}",
        p.jobs[0].status
    );
}

#[test]
fn six_face_cube_fills_envmap_e_role() {
    let d = tempfile::tempdir().unwrap();
    for n in ["chrome", "chrome_n", "chrome_m"] {
        png(
            d.path(),
            &format!("textures/a/{n}.png"),
            32,
            32,
            [128, 128, 255],
        );
    }
    faces(d.path(), "textures/a/chrome_e", 32);
    let (p, ev) = plan_of(req(d.path(), &["set:chrome.shader=envmap"], true));
    assert!(job(&p, "chrome_e.dds").cube.is_some());
    let warnings = ev
        .iter()
        .find_map(|e| match &e.kind {
            EventKind::SetResolved {
                warnings, shader, ..
            } if e
                .set
                .as_deref()
                .is_some_and(|s| s.ends_with("textures/a/chrome")) =>
            {
                Some((shader.clone().map(|s| s.shader), warnings.clone()))
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(warnings.0, Some(Shader::Envmap));
    let codes: Vec<Code> = warnings.1.iter().map(|w| w.code).collect();
    assert!(
        !codes.contains(&Code::ShaderExtraMap) && !codes.contains(&Code::ShaderMissingMap),
        "{:#?}",
        warnings.1
    );
}

#[test]
fn cube_output_collision_is_in_duplicate() {
    let d = tempfile::tempdir().unwrap();
    png(d.path(), "textures/a/chrome_e.png", 128, 96, [9, 9, 9]);
    faces(d.path(), "textures/a/chrome_e", 32);
    let p = plan(d.path(), &[]);
    assert!(p.jobs.is_empty(), "{:#?}", p.jobs);
    let dup: Vec<String> = p
        .skipped
        .iter()
        .filter(|s| s.code == Some(Code::InDuplicate))
        .map(|s| name(&s.path))
        .collect();
    assert!(dup.contains(&"chrome_e.png".to_string()), "{dup:?}");
    assert!(dup.contains(&"chrome_e_posx.png".to_string()), "{dup:?}");
    assert_eq!(dup.len(), 7, "{dup:?}");
}

#[test]
fn single_file_layout_from_aspect_or_key() {
    let d = tempfile::tempdir().unwrap();
    png(d.path(), "textures/cubemaps/x.png", 1024, 768, [9, 9, 9]);
    png(d.path(), "textures/cubemaps/y.png", 512, 512, [9, 9, 9]);
    png(d.path(), "textures/cubemaps/z.png", 512, 512, [9, 9, 9]);
    let p = plan(d.path(), &["file:**/z.png.cubemap.layout=hstrip"]);
    let x = job(&p, "x.dds").cube.as_ref().unwrap();
    assert_eq!(x.layout, CubeLayout::Hcross);
    assert_eq!(x.sources.len(), 1);
    match &job(&p, "y.dds").status {
        JobStatus::Unresolved { reasons, needs } => {
            assert!(
                reasons.iter().any(|r| r.contains("cubemap.layout")),
                "{reasons:?}"
            );
            assert!(needs.is_empty());
        }
        s => panic!("{s:?}"),
    }
    let z = job(&p, "z.dds");
    assert_eq!(
        z.status,
        JobStatus::Error {
            codes: vec![Code::CubeFaces]
        }
    );
    assert!(z
        .warnings
        .iter()
        .any(|w| w.message.contains("layout does not fit 512x512")));
}

#[test]
fn cube_sources_drive_if_changed() {
    let d = tempfile::tempdir().unwrap();
    faces(d.path(), "textures/a/chrome_e", 32);
    let go = || {
        let mut r = req(d.path(), &[], true);
        r.on_existing = OnExisting::IfChanged;
        run(plan_of(r).0).0
    };
    assert_eq!(go().converted, 1);
    let s = go();
    assert_eq!((s.converted, s.up_to_date), (0, 1));
    png(d.path(), "textures/a/chrome_e_negz.png", 32, 32, [7, 7, 7]);
    let s = go();
    assert_eq!((s.converted, s.up_to_date), (1, 0));
}

#[test]
fn cube_stamps_are_checked() {
    let d = tempfile::tempdir().unwrap();
    faces(d.path(), "textures/a/chrome_e", 32);
    let p = plan(d.path(), &[]);
    png(d.path(), "textures/a/chrome_e_negz.png", 64, 64, [7, 7, 7]);
    let (s, ev) = run(p);
    assert_eq!((s.converted, s.failed), (0, 1));
    let err = ev
        .iter()
        .find_map(|e| match &e.kind {
            EventKind::FileFailed { error, .. } => Some(error.clone()),
            _ => None,
        })
        .unwrap();
    assert!(
        err.contains("chrome_e_negz.png") && err.contains("source changed since planning"),
        "{err}"
    );
}

// ---- Task 8: encode a cube job end to end (DECISIONS DN10) ----

/// Review Focus 5: an `.hdr` cross with a highlight above 1.0 and no `cubemap.highlight_clamp` converts, and its
/// events carry LOSS-CLAMP-HDR.
#[test]
fn hdr_cross_without_clamp_warns_end_to_end() {
    let d = tempfile::tempdir().unwrap();
    let mut cross = hcross(&arrow_faces(16));
    // +X sits in cell (2, 1).
    *cross.at_mut(2 * 16 + 8, 16 + 8) = [40.0, 40.0, 40.0, 1.0];
    let img = image::Rgb32FImage::from_fn(cross.width, cross.height, |x, y| {
        let p = cross.at(x, y);
        image::Rgb([p[0], p[1], p[2]])
    });
    let p = d.path().join("in/textures/cubemaps/hot.hdr");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    image::DynamicImage::ImageRgb32F(img).save(&p).unwrap();
    let (plan, plan_ev) = plan_of(req(d.path(), &[], true));
    let (s, run_ev) = run(plan);
    assert_eq!(s.converted, 1, "{plan_ev:#?}");
    assert!(
        plan_ev.iter().chain(&run_ev).any(|e| matches!(
            &e.kind,
            EventKind::Warning(w) if w.code == Code::LossClampHdr
        )),
        "no LOSS-CLAMP-HDR"
    );
}

/// Validation decodes all six faces: a corrupted face 3 mip 0 is named in VAL-MIP-ERROR.
#[test]
fn cube_validation_reads_every_face() {
    use engine::dds::read_dds;
    use engine::decode::{decode_level, Decoded};
    use engine::quantize::EncoderInput;
    use engine::validate::{validate, ValidationInput};
    let d = tempfile::tempdir().unwrap();
    faces(d.path(), "textures/a/chrome_e", 32);
    let p = plan(d.path(), &["format=rgba8"]);
    let profile = job(&p, "chrome_e.dds").profile.clone();
    let (s, _) = run(p);
    assert_eq!(s.converted, 1);
    let mut bytes = std::fs::read(d.path().join("out/textures/a/chrome_e.dds")).unwrap();
    let f = read_dds(&bytes).unwrap();
    // rgba8 is lossless, so the decoded surfaces are the encoder input.
    let inputs: Vec<EncoderInput> = f
        .surfaces()
        .into_iter()
        .map(
            |(w, h, r)| match decode_level(f.format, w, h, &f.data[r], None).unwrap() {
                Decoded::Ldr(px) => EncoderInput::Ldr(px),
                Decoded::Hdr(_) => unreachable!(),
            },
        )
        .collect();
    assert_eq!(inputs.len(), 36);
    let check = |bytes: &[u8]| {
        validate(&ValidationInput {
            bytes,
            profile: &profile,
            encoder_input: Some(&inputs),
            base_coverage: None,
            source_peak: None,
            bc1_model: encoders_sys::Bc1Model::Ideal,
            expected_levels: Some(6),
        })
    };
    assert!(
        check(&bytes).summary.passed,
        "{:#?}",
        check(&bytes).warnings
    );
    let header = bytes.len() - f.data.len();
    let r = f.surfaces()[3 * 6].2.clone();
    for b in &mut bytes[header + r.start..header + r.end] {
        *b = !*b;
    }
    let rep = check(&bytes);
    assert!(!rep.summary.passed);
    let w = rep
        .warnings
        .iter()
        .find(|w| w.code == Code::ValMipError)
        .unwrap_or_else(|| panic!("{:#?}", rep.warnings));
    assert!(w.message.starts_with("face 3 mip 0:"), "{}", w.message);
    // Review M1: the data names the face and mip; `level` stays the surface index.
    assert_eq!(
        (&w.data["level"], &w.data["face"], &w.data["mip"]),
        (
            &serde_json::json!(18),
            &serde_json::json!(3),
            &serde_json::json!(0)
        )
    );
}

/// The preview of a cube job is its +X face alone.
#[test]
fn cube_preview_is_the_pos_x_face() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join("in/textures/a");
    std::fs::create_dir_all(&dir).unwrap();
    for (k, f) in arrow_faces(32).iter().enumerate() {
        png8(f)
            .save(dir.join(format!("chrome_e_{}.png", FACES[k])))
            .unwrap();
    }
    // A face size the 2D path would never produce from 32-px files, so this proves the cube path ran.
    let p = plan(d.path(), &["cubemap.face_size=16"]);
    let j = job(&p, "chrome_e.dds");
    let s = engine::session::Session::new(encoders_sys::Bc1Model::Ideal).unwrap();
    let pv = s.preview(j, 4096, &CancelToken::default()).unwrap();
    assert_eq!(pv.source.first_level, 0);
    assert_eq!(pv.source.dims[0], (16, 16));
    // One face (faces = 1): one 16 -> 1 chain, not six.
    assert_eq!(pv.source.dims.len(), 5, "one face's chain");
    assert_eq!((pv.levels.len(), pv.source.levels.len()), (5, 5));
    // +X is red under its white and black bands.
    let c = pv.source.levels[0].at(10, 10);
    assert!(c[0] > 0.9 && c[1] < 0.1 && c[2] < 0.1, "{c:?}");
}

/// Fix round 1: `face_rotate` corrects a turned file of a six-file cube, as it does a cell of a strip.
#[test]
fn face_rotate_applies_to_six_face_files() {
    use engine::dds::read_dds;
    use engine::decode::{decode_level, Decoded};
    let n = 32;
    let faces = arrow_faces(n);
    let mut files = faces.clone();
    // +Y turned a clockwise quarter: its top (white) row becomes the right column.
    for y in 0..n {
        for x in 0..n {
            *files[2].at_mut(x, y) = faces[2].at(y, n - 1 - x);
        }
    }
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join("in/textures/a");
    std::fs::create_dir_all(&dir).unwrap();
    for (k, f) in files.iter().enumerate() {
        png8(f)
            .save(dir.join(format!("chrome_e_{}.png", FACES[k])))
            .unwrap();
    }
    let p = plan(
        d.path(),
        &["format=rgba8", "cubemap.face_rotate=[0, 0, 270, 0, 0, 0]"],
    );
    let (s, ev) = run(p);
    assert_eq!(s.converted, 1, "{ev:#?}");
    let f =
        read_dds(&std::fs::read(d.path().join("out/textures/a/chrome_e.dds")).unwrap()).unwrap();
    let surf = f.surfaces();
    for (k, face) in faces.iter().enumerate() {
        let (w, h, r) = surf[k * 6].clone();
        let Decoded::Ldr(px) = decode_level(f.format, w, h, &f.data[r], None).unwrap() else {
            unreachable!()
        };
        assert!(px == texels8(face), "face {k} differs");
    }
}

/// DECISIONS DN11: a cube's mips average linear light decoded with the game's 1.6 gamma, not sRGB: 0/255 halves give
/// round(255 * 0.5^(1/1.6)) = 165 (sRGB would give 188).
#[test]
fn cube_mips_average_in_game_gamma() {
    use engine::dds::read_dds;
    use engine::decode::{decode_level, Decoded};
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join("in/textures/a");
    std::fs::create_dir_all(&dir).unwrap();
    for f in FACES {
        image::RgbImage::from_fn(2, 2, |x, y| {
            let v = if (x + y) % 2 == 0 { 255 } else { 0 };
            image::Rgb([v, v, v])
        })
        .save(dir.join(format!("chrome_e_{f}.png")))
        .unwrap();
    }
    let mut r = req(d.path(), &["format=rgba8"], true);
    r.allow = [Code::DimSmall].into_iter().collect();
    let (s, ev) = run(plan_of(r).0);
    assert_eq!(s.converted, 1, "{ev:#?}");
    let f =
        read_dds(&std::fs::read(d.path().join("out/textures/a/chrome_e.dds")).unwrap()).unwrap();
    assert_eq!(f.mip_count, 2);
    for (k, (w, h, r)) in f
        .surfaces()
        .into_iter()
        .enumerate()
        .filter(|(k, _)| k % 2 == 1)
    {
        let Decoded::Ldr(px) = decode_level(f.format, w, h, &f.data[r], None).unwrap() else {
            unreachable!()
        };
        for c in &px[0][..3] {
            assert!(
                (*c as i32 - 165).abs() <= 1,
                "face {} mip 1: {:?}",
                k / 2,
                px[0]
            );
        }
    }
}

/// DECISIONS DN11: a float source is linear already, so it is only re-encoded: linear 0.25 is
/// round(255 * 0.25^(1/1.6)) = 107.
#[test]
fn cube_float_source_is_only_reencoded() {
    use engine::dds::read_dds;
    use engine::decode::{decode_level, Decoded};
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("in/textures/cubemaps/flat.exr");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    image::DynamicImage::ImageRgb32F(image::Rgb32FImage::from_pixel(
        64,
        48,
        image::Rgb([0.25, 0.25, 0.25]),
    ))
    .save(&p)
    .unwrap();
    let (s, ev) = run(plan_of(req(d.path(), &["format=rgba8"], true)).0);
    assert_eq!(s.converted, 1, "{ev:#?}");
    let f =
        read_dds(&std::fs::read(d.path().join("out/textures/cubemaps/flat.dds")).unwrap()).unwrap();
    let per = f.mip_count as usize;
    for (k, (w, h, r)) in f
        .surfaces()
        .into_iter()
        .enumerate()
        .filter(|(k, _)| k % per == 0)
    {
        let Decoded::Ldr(px) = decode_level(f.format, w, h, &f.data[r], None).unwrap() else {
            unreachable!()
        };
        assert!(
            px.iter().all(|p| p[..3] == [107, 107, 107]),
            "face {}: {:?}",
            k / per,
            px[0]
        );
    }
}

// ---- Task 10: cube checks (DECISIONS DN12) ----

/// A RGBA8 cube of `size` with `mips` levels per face (all texels `px`), as written by Task 2's writer.
fn cube_dds(
    size: u32,
    mips: u32,
    srgb: bool,
    hdr: engine::dds::ResolvedHeader,
    px: impl Fn(usize) -> [u8; 4],
) -> engine::dds::DdsFile {
    use engine::dds::*;
    let d = DdsDesc {
        width: size,
        height: size,
        format: Format::Rgba8,
        srgb,
        header: hdr,
    };
    let mut levels = vec![];
    for face in 0..6 {
        let mut s = size;
        for _ in 0..mips {
            levels.push(px(face).repeat((s * s) as usize));
            s = (s / 2).max(1);
        }
    }
    read_dds(&dds_bytes_cube(&d, &levels).unwrap()).unwrap()
}

#[test]
fn cube_checks() {
    use engine::cube::checks::inspect_issues;
    use engine::dds::ResolvedHeader::{Dx10, Legacy};
    let none = |p: &str, f| inspect_issues(p, &f);
    let full = cube_dds(8, 4, false, Legacy, |_| [9, 9, 9, 255]);
    assert!(none("textures/a/x_e.dds", full).is_empty());
    let mipless = cube_dds(8, 1, false, Legacy, |_| [9, 9, 9, 255]);
    assert_eq!(
        none("textures/a/x_e.dds", mipless),
        ["cube has no mips: curved surfaces will sparkle; re-encode it with a full chain"]
    );
    // +Y is face 2: a lit +X alone is the dynamic marker, a lit +Y alone is not.
    let only = |k: usize| {
        cube_dds(1, 1, false, Legacy, move |f| {
            if f == k {
                [0, 0, 8, 255]
            } else {
                [0, 0, 0, 255]
            }
        })
    };
    assert!(none("x_e.dds", only(0)).is_empty());
    assert_eq!(
        none("x_e.dds", only(2)),
        ["1x1 cube with a non-black +Y: Community Shaders treats it as a static, near-black reflection, not a dynamic-cubemap marker"]
    );
    assert_eq!(
        none("x_e.dds", cube_dds(8, 4, true, Dx10, |_| [9, 9, 9, 255])),
        ["cube uses an sRGB format; Community Shaders expects UNORM cubes"]
    );
    // A BC7 sRGB cube (DX10 header) is flagged too.
    {
        use engine::dds::*;
        let d = DdsDesc {
            width: 8,
            height: 8,
            format: Format::Bc7,
            srgb: true,
            header: Dx10,
        };
        let levels: Vec<Vec<u8>> = (0..6)
            .flat_map(|_| [8u32, 4, 2, 1].map(|s| vec![0; level_size(Format::Bc7, s, s)]))
            .collect();
        let f = read_dds(&dds_bytes_cube(&d, &levels).unwrap()).unwrap();
        assert_eq!(
            none("x_e.dds", f),
            ["cube uses an sRGB format; Community Shaders expects UNORM cubes"]
        );
    }
    let flat = engine::dds::read_dds(
        &engine::dds::dds_bytes(
            &engine::dds::DdsDesc {
                width: 4,
                height: 4,
                format: Format::Rgba8,
                srgb: false,
                header: Legacy,
            },
            &[vec![0; 64]],
        )
        .unwrap(),
    )
    .unwrap();
    let want = "2D texture where a cubemap is expected (the game reads _e and textures\\cubemaps\\ files as cubes)";
    for p in [
        "Textures\\X_E.DDS",
        "textures/CubeMaps/sky.dds",
        "a/b_e",
        "F:\\mods\\Textures\\CubeMaps\\foo.dds",
    ] {
        assert_eq!(inspect_issues(p, &flat), [want], "{p}");
    }
    for p in [
        "textures/a/x.dds",
        "textures/a/x_e_n.dds",
        "textures/cubemapsx/x.dds",
        "F:\\mods\\cubemaps\\textures\\armor\\iron_d.dds",
        "cubemaps/x.dds",
    ] {
        assert!(inspect_issues(p, &flat).is_empty(), "{p}");
    }
}

#[test]
fn our_cube_output_has_no_cube_issues() {
    use engine::dds::read_dds;
    let d = tempfile::tempdir().unwrap();
    faces(d.path(), "textures/a/chrome_e", 32);
    let (s, ev) = run(plan(d.path(), &[]));
    assert_eq!(s.converted, 1, "{ev:#?}");
    let f =
        read_dds(&std::fs::read(d.path().join("out/textures/a/chrome_e.dds")).unwrap()).unwrap();
    assert!(f.is_cubemap);
    assert!(engine::cube::checks::inspect_issues("textures/a/chrome_e.dds", &f).is_empty());
}

/// The marker's colour sits on +X and +Y stays black, so any --color reads as a dynamic marker (not a static cube).
#[test]
fn marker_reads_back_as_a_dynamic_marker() {
    use engine::cube::marker::marker_bytes;
    for rgb in [[0, 0, 0], [255, 197, 78]] {
        let f = engine::dds::read_dds(&marker_bytes(rgb, 255)).unwrap();
        assert!(engine::cube::checks::inspect_issues("textures/x_e.dds", &f).is_empty());
    }
}
