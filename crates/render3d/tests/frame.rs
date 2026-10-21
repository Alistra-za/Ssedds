//! 03 §7 per-frame cost: resolved fields write the same bytes and pixels as the by-name writer; an unchanged
//! frame uploads nothing; `draw_ms` is recorded (not asserted).
mod common;
use common::{dds_rgba8, fixture, plane_shape, renderer, FLAT, IDENT};
use nif::material::MaterialState;
use render3d::cbuffer::CbWriter;
use render3d::compile::{CbLayout, CbVar, VarKind};
use render3d::cs_settings::{feature_data, CsSettings};
use render3d::material::{write_material, Bound};
use render3d::scene::{
    rows, write_static, Camera, LightValues, Scene, SceneFields, View, ViewValues, FEATURE_FLOATS,
    FEATURE_UINTS, LIGHT_NAMES, VIEW_NAMES,
};
use render3d::{Renderer, ShapeId};

const STATIC_NAMES: &[&str] = &[
    "EyePosition",
    "FogParam",
    "ColourOutputClamp",
    "AmbientSpecularTintAndFresnelPower",
    "SSRParams",
    "WaterSystemHeight",
    "WaterData",
    "FrameParams",
    "DynamicResolutionParams1",
    "DynamicResolutionParams2",
    "InInterior",
];
const MATERIAL_NAMES: &[&str] = &[
    "TexcoordOffset",
    "SpecularColor",
    "PBRFlags",
    "PBRParams1",
    "PBRParams2",
    "MultiLayerParallaxData",
    "EmitColor",
    "emissiveMult",
    "MaterialData",
    "AlphaTestRefRS",
    "PixelShaderDescriptor",
    "ExtraShaderDescriptor",
    "LightingEffectParams",
    "MaterialObjectRGBScale",
    "Bones",
    "PreviousBones",
];

fn all_names() -> Vec<String> {
    let mut v: Vec<String> = ["World", "PreviousWorld"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    v.extend(
        VIEW_NAMES
            .iter()
            .chain(LIGHT_NAMES.iter())
            .chain(STATIC_NAMES)
            .chain(MATERIAL_NAMES)
            .map(|s| s.to_string()),
    );
    v.extend(FEATURE_FLOATS.iter().map(|(n, _)| n.to_string()));
    v.extend(FEATURE_UINTS.iter().map(|(n, _)| n.to_string()));
    v
}

fn layout(names: &[String], size: u32) -> CbLayout {
    CbLayout {
        name: "Synthetic".into(),
        slot: 0,
        size: size * names.len() as u32,
        vars: names
            .iter()
            .enumerate()
            .map(|(i, n)| CbVar {
                name: n.clone(),
                offset: size * i as u32,
                size,
                kind: if FEATURE_UINTS.iter().any(|(u, _)| n.ends_with(u)) {
                    VarKind::Uint
                } else {
                    VarKind::Float
                },
            })
            .collect(),
    }
}

/// 03 T-F1, writer level: every scene and material field, two layouts (plain names with room for every value;
/// namespaced, reordered, subset, truncated to 32 bytes).
#[test]
fn resolved_fields_write_same_bytes_as_by_name() {
    let names = all_names();
    let mut ns: Vec<String> = names
        .iter()
        .enumerate()
        .filter(|(i, _)| i % 3 != 1)
        .map(|(_, n)| {
            if n.contains('.') {
                format!("SharedData::{n}")
            } else {
                format!("FrameBuffer::{n}")
            }
        })
        .collect();
    ns.reverse();
    let mut scene = Scene::neutral();
    scene.dir_light.direction = [0.3, -0.2, 0.93];
    scene.time = 2.5;
    let view = View::new(
        &Camera::Orbit {
            target: [1.0, 2.0, 3.0],
            yaw: 0.7,
            pitch: 0.3,
            distance: 90.0,
        },
        Some(([1.0, 2.0, 3.0], 20.0)),
        320,
        200,
    );
    let model = [
        [0.0, -2.0, 0.0, 5.0],
        [2.0, 0.0, 0.0, -7.0],
        [0.0, 0.0, 2.0, 11.0],
    ];
    let mut m = MaterialState::builtin_default();
    m.slsf2 |= (1 << 23) | (1 << 26);
    let bound = Bound {
        regs: [12].into_iter().collect(),
    };
    let fv = feature_data(&CsSettings::default(), &Default::default(), true, false);
    for l in [layout(&names, 400), layout(&ns, 32)] {
        let mut by_name = CbWriter::new(&l);
        write_material(&mut by_name, &m, &bound, true);
        scene.write(&mut by_name, &view, &model, &fv);
        let mut resolved = CbWriter::new(&l);
        write_material(&mut resolved, &m, &bound, true);
        write_static(&mut resolved, &fv);
        let f = SceneFields::resolve(&resolved);
        assert!(f.write_lights(&mut resolved, &LightValues::new(&scene)));
        assert!(f.write_view(
            &mut resolved,
            &ViewValues::new(&view),
            &rows(&view.world(&model))
        ));
        assert_eq!(
            by_name.bytes(),
            resolved.bytes(),
            "layout {:?}…",
            &l.vars[..2]
        );
    }
}

fn checker() -> Vec<u8> {
    dds_rgba8(
        2,
        2,
        &[
            [255, 0, 0, 255],
            [0, 0, 255, 255],
            [0, 0, 255, 255],
            [255, 0, 0, 255],
        ],
    )
}

fn same(a: &mut Renderer, b: &mut Renderer, s: &Scene, w: u32, h: u32, what: &str) {
    assert_eq!(
        a.render(s, w, h).unwrap(),
        b.render(s, w, h).unwrap(),
        "{what}"
    );
}

/// 03 §7 acceptance: the resolved / dirty-upload path draws byte-identical pixels to the by-name writer through a
/// first frame, an unchanged frame, a camera move, a light change, a cbuffer edit and a resize.
#[test]
fn pixels_match_the_by_name_writer() {
    let tex = checker();
    let resolve = |_: &str| Some(tex.clone());
    let (ca, cb) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut a = renderer(ca.path(), &["STANDIN_OPAQUE_OK"]);
    let mut b = renderer(cb.path(), &["STANDIN_OPAQUE_OK"]);
    b.set_reference_writer(true);
    let nif = fixture("TestNifFile_DeepGraph_SE.nif");
    assert_eq!(a.load(&nif, &resolve), b.load(&nif, &resolve));
    let mut scene = Scene::neutral();
    same(&mut a, &mut b, &scene, 96, 96, "first frame");
    same(&mut a, &mut b, &scene, 96, 96, "unchanged frame");
    let (c, r) = a.bounds().unwrap();
    scene.camera = Camera::Orbit {
        target: c,
        yaw: 2.0,
        pitch: 0.5,
        distance: 3.0 * r,
    };
    same(&mut a, &mut b, &scene, 96, 96, "camera move");
    scene.dir_light.direction = [-0.6, 0.3, 0.74];
    scene.ambient6[4] = [0.5; 3];
    same(&mut a, &mut b, &scene, 96, 96, "light change");
    let peg = ShapeId(103);
    let mut m = a
        .shapes()
        .into_iter()
        .find(|s| s.id == peg)
        .unwrap()
        .material;
    m.params.uv_scale = [3.0, 3.0];
    for x in [&mut a, &mut b] {
        assert_eq!(
            x.set_material(peg, m.clone(), &resolve),
            Ok(render3d::EditKind::Cbuffer)
        );
    }
    same(&mut a, &mut b, &scene, 96, 96, "cbuffer edit");
    same(&mut a, &mut b, &scene, 64, 48, "resize");
}

#[test]
fn unchanged_frame_uploads_nothing_and_an_edit_uploads_one_shape() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), FLAT);
    let rep = r.load(&fixture("TestNifFile_DeepGraph_SE.nif"), &|_| None);
    assert_eq!(rep.shapes, 27);
    let mut scene = Scene::neutral();
    r.render(&scene, 64, 64).unwrap();
    assert!(r.last_uploads() >= 27, "{}", r.last_uploads());
    r.render(&scene, 64, 64).unwrap();
    assert_eq!(r.last_uploads(), 0, "same camera, same lights");
    let peg = ShapeId(103);
    let mut m = r
        .shapes()
        .into_iter()
        .find(|s| s.id == peg)
        .unwrap()
        .material;
    m.params.emissive_color = [0.2, 0.0, 0.0];
    assert_eq!(
        r.set_material(peg, m, &|_| None),
        Ok(render3d::EditKind::Cbuffer)
    );
    r.render(&scene, 64, 64).unwrap();
    // The stand-in has 4 cbuffers per draw (VS b1/b2, PS b1/b2).
    assert!((1..=4).contains(&r.last_uploads()), "{}", r.last_uploads());
    let (c, rad) = r.bounds().unwrap();
    scene.camera = Camera::Orbit {
        target: c,
        yaw: 1.0,
        pitch: 0.2,
        distance: 3.0 * rad,
    };
    r.render(&scene, 64, 64).unwrap();
    assert!(
        r.last_uploads() >= 27,
        "a camera move rewrites every draw's view fields"
    );
}

/// Recorded, not asserted (03 §7: "draw_ms drops by at least 2× against P0" goes into the phase report).
/// `cargo test -p render3d --test frame record_draw_ms -- --ignored --nocapture`
#[test]
#[ignore = "records draw_ms, asserts nothing"]
fn record_draw_ms_deep_graph() {
    for reference in [true, false] {
        let cache = tempfile::tempdir().unwrap();
        let mut r = renderer(cache.path(), FLAT);
        r.set_reference_writer(reference);
        r.load(&fixture("TestNifFile_DeepGraph_SE.nif"), &|_| None);
        let (c, rad) = r.bounds().unwrap();
        let mut scene = Scene::neutral();
        r.render(&scene, 128, 128).unwrap();
        let (mut moving, mut still) = (0.0, 0.0);
        for i in 0..20 {
            scene.camera = Camera::Orbit {
                target: c,
                yaw: 0.1 * i as f32,
                pitch: 0.3,
                distance: 3.0 * rad,
            };
            r.render(&scene, 128, 128).unwrap();
            moving += r.last_timing().draw_ms;
        }
        for _ in 0..20 {
            r.render(&scene, 128, 128).unwrap();
            still += r.last_timing().draw_ms;
        }
        eprintln!(
            "DeepGraph (27 shapes, WARP) draw_ms per frame, {}: moving camera {:.3}, still camera {:.3}",
            if reference { "by-name writer (P0)" } else { "resolved + dirty uploads" },
            moving / 20.0,
            still / 20.0
        );
    }
}

/// A draw kept across hide / failed un-hide must not come back with the constants of before the hide.
#[test]
fn a_retained_draw_returning_from_hidden_matches_the_by_name_writer() {
    let (ca, cb) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    // No STANDIN_OPAQUE_OK: a material without alpha test fails to compile (edit.rs's failing_permutation).
    let mut a = renderer(ca.path(), &["STANDIN_FLAT"]);
    let mut b = renderer(cb.path(), &["STANDIN_FLAT"]);
    b.set_reference_writer(true);
    let nif = fixture("TestNifFile_DeepGraph_SE.nif");
    a.load(&nif, &|_| None);
    b.load(&nif, &|_| None);
    let peg = ShapeId(103);
    let mut scene = Scene::neutral();
    same(&mut a, &mut b, &scene, 96, 96, "first frame");
    let good = a
        .shapes()
        .into_iter()
        .find(|s| s.id == peg)
        .unwrap()
        .material;
    let mut hidden = good.clone();
    hidden.hidden = true;
    for x in [&mut a, &mut b] {
        x.set_material(peg, hidden.clone(), &|_| None).unwrap();
    }
    let (c, r) = a.bounds().unwrap();
    scene.camera = Camera::Orbit {
        target: c,
        yaw: 2.0,
        pitch: 0.5,
        distance: 3.0 * r,
    };
    scene.dir_light.direction = [-0.6, 0.3, 0.74];
    same(&mut a, &mut b, &scene, 96, 96, "moved while hidden");
    let mut bad = good;
    bad.alpha = None;
    for x in [&mut a, &mut b] {
        x.set_material(peg, bad.clone(), &|_| None).unwrap();
        assert!(matches!(
            x.shapes().into_iter().find(|s| s.id == peg).unwrap().status,
            render3d::ShapeStatus::Failed { .. }
        ));
    }
    same(
        &mut a,
        &mut b,
        &scene,
        96,
        96,
        "un-hidden with a failed rebuild",
    );
    // Pixels can hide it (the peg may be occluded); the retained draw must also have been rewritten.
    assert!(
        a.last_uploads() >= 1,
        "retained draw drew with pre-hide constants"
    );
}

/// v2 B3 (F4): `Scene.time` reaches CS's `Timer` every frame, and a cbuffer edit (which rewrites the static block)
/// does not reset it.
#[test]
fn timer_reaches_the_shader() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(
        cache.path(),
        &["STANDIN_OPAQUE_OK", "STANDIN_FLAT", "STANDIN_TIMER"],
    );
    let plane = plane_shape(1, IDENT, [0.0; 3], 0);
    let base = plane.material.clone();
    let rep = r.load_parsed(
        &render3d::ParsedNif {
            shapes: vec![plane],
            ..Default::default()
        },
        &|_| None,
    );
    assert!(
        rep.errors.is_empty() && rep.compile_errors.is_empty(),
        "{rep:?}"
    );
    let id = ShapeId(1);
    let mut s = Scene::neutral();
    assert_eq!(
        s.time, 1.0,
        "the default is the constant the preview always wrote"
    );
    let a = r.render(&s, 64, 64).unwrap();
    assert_eq!(r.draw_cb_floats(id, "Timer"), Some(vec![1.0]));
    s.time = 0.25;
    let b = r.render(&s, 64, 64).unwrap();
    assert_eq!(r.draw_cb_floats(id, "Timer"), Some(vec![0.25]));
    assert_ne!(a, b, "frac(1.0) = 0 adds nothing; frac(0.25) adds red");
    // A cbuffer edit (glossiness is not drawn by STANDIN_FLAT): the static block is rewritten, Timer must survive.
    let mut m = base;
    m.params.glossiness += 1.0;
    assert_eq!(
        r.set_material(id, m, &|_| None).unwrap(),
        render3d::EditKind::Cbuffer
    );
    let c = r.render(&s, 64, 64).unwrap();
    assert_eq!(
        r.draw_cb_floats(id, "Timer"),
        Some(vec![0.25]),
        "not reset to 1.0 by write_static"
    );
    assert_eq!(b, c, "same time, same pixels");
}
