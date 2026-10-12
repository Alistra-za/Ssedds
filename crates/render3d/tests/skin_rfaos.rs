//! P1-C Task 8: the skin RFAOS lookup (pre-October CS rule, 02-skin-rfaos.md §0) on WARP with the stand-in shader.
//! `STANDIN_SKIN`: pixels = 0.25·diffuse + skinMaterialFlags.x × t71.
mod common;
use common::{cache_files, channel, dds_rgba8, fixture, renderer};
use nif::material::MaterialState;
use render3d::cs_settings::CsSettings;
use render3d::scene::Scene;
use render3d::{Builtin, Renderer, ShapeId};
use std::cell::RefCell;
use std::path::{Path, PathBuf};

const SKIN: &[&str] = &["STANDIN_OPAQUE_OK", "STANDIN_FLAT", "STANDIN_SKIN"];
const CYL1: ShapeId = ShapeId(9);
const FROM_S: &str = "textures/standin/light/head_rfaos.dds";
const FROM_N: &str = "textures/standin/head_rfaos.dds";

fn frame(r: &mut Renderer) -> Vec<u8> {
    r.render(&Scene::neutral(), 64, 64).unwrap()
}

fn flags(r: &Renderer, id: ShapeId) -> Vec<f32> {
    r.draw_cb_floats(id, "skinMaterialFlags")
        .expect("skinMaterialFlags reflected")
}

/// cylinder_1 of the furniture fixture as a FaceGen (type 4) shape with `_n`, `_sk` and `_s` slots, its RFAOS
/// candidates resolved by `files`.
fn skin(cache: &Path, files: &[(&str, [u8; 4])], shader_type: u32) -> Renderer {
    let mut r = renderer(cache, SKIN);
    let rep = r.load(&fixture("TestNifFile_Furniture_Col_SE.nif"), &|_| None);
    assert_eq!(rep.shapes, 2, "{rep:?}");
    let mut m: MaterialState = r
        .shapes()
        .into_iter()
        .find(|s| s.id == CYL1)
        .unwrap()
        .material;
    m.shader_type = shader_type;
    m.set_slot_raw(1, r"textures\standin\head_N.dds");
    m.set_slot_raw(2, r"textures\standin\head_sk.dds");
    m.set_slot_raw(7, r"Textures\Standin\Light\head_s.dds");
    let files: Vec<(String, Vec<u8>)> = files
        .iter()
        .map(|(p, c)| (p.to_string(), dds_rgba8(1, 1, &[*c])))
        .collect();
    let resolve = |p: &str| files.iter().find(|f| f.0 == p).map(|f| f.1.clone());
    assert!(r.set_material(CYL1, m, &resolve).is_ok());
    r
}

const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];

#[test]
fn the_specular_derived_rfaos_wins_over_the_normal_derived() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = skin(cache.path(), &[(FROM_S, RED), (FROM_N, GREEN)], 4);
    // Found: (1, 0, 0, w), w = 1 because slot 2 (`_sk`) is named (02-skin-rfaos.md §0.5).
    assert_eq!(flags(&r, CYL1), [1.0, 0.0, 0.0, 1.0]);
    let px = frame(&mut r);
    assert!(
        channel(&px, 0) > channel(&px, 1) + 1000,
        "t71 is the _s-derived (red) map"
    );
}

#[test]
fn the_normal_derived_rfaos_is_the_fallback() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = skin(cache.path(), &[(FROM_N, GREEN)], 5);
    assert_eq!(flags(&r, CYL1), [1.0, 0.0, 0.0, 1.0]);
    let px = frame(&mut r);
    assert!(
        channel(&px, 1) > channel(&px, 0) + 1000,
        "t71 is the _n-derived (green) map"
    );
}

#[test]
fn no_rfaos_keeps_x_off_and_the_black_default() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = skin(cache.path(), &[], 4);
    // Not found: (0, 0, 0, w) (02-skin-rfaos.md §0.5); t71 stays the black default.
    assert_eq!(flags(&r, CYL1), [0.0, 0.0, 0.0, 1.0]);
    let px = frame(&mut r);
    assert_eq!(channel(&px, 0), channel(&px, 1), "nothing added at t71");
    let mut m = MaterialState::builtin_default();
    m.shader_type = 4;
    m.set_slot_raw(1, r"textures\standin\head_n.dds");
    m.set_slot_raw(7, r"textures\standin\light\head_s.dds");
    let rep = r.load_builtin(Builtin::Sphere, m, &|_| None);
    assert_eq!(
        rep.missing_textures,
        [
            "textures/standin/head_n.dds",
            "textures/standin/light/head_s.dds"
        ],
        "a derived RFAOS guess is not a missing texture"
    );
}

#[test]
fn a_non_skin_shape_gets_no_rfaos_and_no_flags() {
    let cache = tempfile::tempdir().unwrap();
    let r = skin(cache.path(), &[(FROM_S, RED)], 0);
    assert_eq!(flags(&r, CYL1), [0.0; 4]);
}

/// A value-only settings change rewrites the static block (FeatureData's zero `skinMaterialFlags` included) and
/// compiles nothing; the per-shape flags survive it.
#[test]
fn the_flags_survive_a_value_only_settings_change() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = skin(cache.path(), &[(FROM_S, RED)], 4);
    let ambient = |a: f64| {
        let merged = serde_json::json!({"Linear Lighting": {"ambientMult": a}});
        CsSettings {
            hash: format!("ambient {a}"),
            merged: merged.as_object().unwrap().clone(),
            ..CsSettings::default()
        }
    };
    r.set_settings(ambient(1.0)); // the settings defines join the stand-in hooks: a recompile
    assert_eq!(flags(&r, CYL1), [1.0, 0.0, 0.0, 1.0]);
    let (files, draw) = (cache_files(cache.path()), r.draw_identity(CYL1));
    r.set_settings(ambient(0.5));
    assert_eq!(cache_files(cache.path()), files, "nothing was compiled");
    assert_eq!(
        r.draw_identity(CYL1),
        draw,
        "rewritten in place, not rebuilt"
    );
    assert_eq!(flags(&r, CYL1), [1.0, 0.0, 0.0, 1.0]);
}

/// Owner data, read-only: the Arch head resolves `light/head_rfaos.dds` from its `_s` slot (the `_n`-derived root
/// `head_rfaos.dds` does not exist). `SDC_ACHERUS` = the MO2 instance root.
#[test]
#[ignore]
fn arch_head_resolves_the_light_rfaos() {
    let root = PathBuf::from(std::env::var("SDC_ACHERUS").expect("SDC_ACHERUS"));
    let mods = root.join("mods");
    let arch = mods.join("Arch Resources");
    let mut roots = vec![arch.clone()];
    roots.extend(
        std::fs::read_dir(&mods)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir() && *p != arch),
    );
    let asked = RefCell::new(vec![]);
    let resolve = |norm: &str| {
        let hit = roots
            .iter()
            .find_map(|r| std::fs::read(r.join(norm.replace('/', "\\"))).ok());
        asked.borrow_mut().push((norm.to_string(), hit.is_some()));
        hit
    };
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), SKIN);
    let nif = std::fs::read(arch.join(r"meshes\arch\head\Arch_head.nif")).unwrap();
    let rep = r.load(&nif, &resolve);
    println!("{rep:?}");
    let rfaos: Vec<_> = asked
        .borrow()
        .iter()
        .filter(|(p, _)| p.ends_with("_rfaos.dds"))
        .cloned()
        .collect();
    println!("rfaos lookups: {rfaos:?}");
    assert_eq!(
        rfaos.iter().find(|(_, hit)| *hit).map(|(p, _)| p.as_str()),
        Some("textures/arch/skin/light/head_rfaos.dds")
    );
    let heads: Vec<_> = r
        .shapes()
        .into_iter()
        .filter(|s| matches!(s.material.shader_type, 4 | 5))
        .collect();
    assert!(!heads.is_empty());
    for s in heads {
        let f = flags(&r, s.id);
        println!(
            "{} type {} skinMaterialFlags {f:?}",
            s.name, s.material.shader_type
        );
        assert_eq!(f[0], 1.0);
    }
}
