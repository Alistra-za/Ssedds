//! The MATO projected layer (Note GG): eligibility, defines, the captured constants (writer level, no GPU), the
//! unresolved-texture list (WARP stand-in), and `render_projected_*` on the installed CS (`SDC_CS_SHADERS`).
mod common;

use nif::material::MaterialState;
use render3d::cbuffer::CbWriter;
use render3d::compile::{CbLayout, CbVar, VarKind};
use render3d::material::{perm_material, projected_eligible, write_projected};
use render3d::permutation::{defines, Stage};
use render3d::scene::{Camera, DirLight, Scene};
use render3d::{Glint, ProjectedLayer, Renderer, ShapeId, ShapeStatus};

const DIFFUSE: &str = "textures/effects/projecteddiffuse.dds";

/// The ash draws' layer (mato-capture-results.md).
fn ash() -> ProjectedLayer {
    ProjectedLayer {
        falloff_scale: 0.4,
        falloff_bias: 0.5,
        noise_uv_scale: 20.0,
        colour: [1.0, 1.0, 1.0],
        base_color_scale: [0.66, 0.63, 0.63],
        roughness: 0.9,
        specular_level: 0.04,
        glint: None,
    }
}

/// The snow draws' layer: projected glint on.
fn snow() -> ProjectedLayer {
    ProjectedLayer {
        falloff_scale: 0.35,
        falloff_bias: 0.4,
        noise_uv_scale: 48.0,
        colour: [1.0, 1.0, 1.0],
        base_color_scale: [1.0, 1.0, 1.0],
        roughness: 0.5,
        specular_level: 0.04,
        glint: Some(Glint {
            screen_space_scale: 1.0,
            log_microfacet_density: 13.0,
            microfacet_roughness: 1.0,
            density_randomization: 5.0,
        }),
    }
}

fn pbr() -> MaterialState {
    let mut m = MaterialState::builtin_default();
    m.slsf2 |= 1 << 23;
    m
}

#[test]
fn eligible_shapes() {
    assert!(projected_eligible(&pbr()));
    assert!(projected_eligible(&MaterialState::builtin_default()));
    let mut skinned = pbr();
    skinned.skinned = true;
    assert!(!projected_eligible(&skinned));
    for t in [3, 4, 5, 11, 14] {
        let mut m = MaterialState::builtin_default();
        m.shader_type = t;
        assert!(!projected_eligible(&m), "type {t}");
    }
    let mut mlp = pbr();
    mlp.slsf2 |= 1 << 24;
    assert!(!projected_eligible(&mlp));
}

fn names(d: &render3d::permutation::Defines) -> Vec<&str> {
    d.0.iter().map(|x| x.0.as_str()).collect()
}

#[test]
fn defines_match_capture() {
    let mut m = pbr();
    m.has_vertex_colors = true;
    let today_ps = defines(&perm_material(&m), Stage::Pixel, &[], true);
    let today_vs = defines(&perm_material(&m), Stage::Vertex, &[], true);
    let mut pm = perm_material(&m);
    assert!(!pm.projected && !pm.projected_glint);
    assert_eq!(defines(&pm, Stage::Pixel, &[], true), today_ps);
    assert_eq!(defines(&pm, Stage::Vertex, &[], true), today_vs);
    assert!(!names(&today_ps).contains(&"PROJECTED_UV"));
    // Ash (descriptor 8019): DEFERRED + PROJECTED_UV + TRUE_PBR + VC, no GLINT.
    pm.projected = true;
    let ps = defines(&pm, Stage::Pixel, &[], true);
    for d in ["DEFERRED", "PROJECTED_UV", "TRUE_PBR", "VC"] {
        assert!(names(&ps).contains(&d), "{d}: {ps:?}");
    }
    assert!(!names(&ps).contains(&"GLINT"), "{ps:?}");
    let vs = defines(&pm, Stage::Vertex, &[], true);
    assert!(names(&vs).contains(&"PROJECTED_UV"), "{vs:?}");
    assert!(names(&vs).contains(&"VC"), "{vs:?}");
    // Snow (18019): also GLINT.
    pm.projected_glint = true;
    assert!(names(&defines(&pm, Stage::Pixel, &[], true)).contains(&"GLINT"));
    // R2: no vertex colours, no VC (the VS input layout follows the mesh).
    let mut plain = perm_material(&pbr());
    plain.projected = true;
    for st in [Stage::Vertex, Stage::Pixel] {
        let d = defines(&plain, st, &[], false);
        assert!(names(&d).contains(&"PROJECTED_UV") && !names(&d).contains(&"VC"));
    }
}

/// A synthetic layout with every field `write_projected` writes, one 16-byte register each (TextureProj: three).
const FIELDS: &[(&str, u32)] = &[
    ("ProjectedUVParams", 16),
    ("ProjectedUVParams2", 16),
    ("ProjectedUVParams3", 16),
    ("MaterialObjectRGBScale", 12),
    ("ParallaxOccData", 16),
    ("SparkleParams", 16),
    ("PBRFlags", 4),
    ("TextureProj", 48),
];

fn offset(name: &str) -> usize {
    let i = FIELDS.iter().position(|f| f.0 == name).unwrap();
    FIELDS[..i]
        .iter()
        .map(|f| f.1.div_ceil(16) as usize * 16)
        .sum()
}

fn writer() -> CbWriter {
    let vars = FIELDS
        .iter()
        .map(|(n, s)| CbVar {
            name: n.to_string(),
            offset: offset(n) as u32,
            size: *s,
            kind: VarKind::Float,
        })
        .collect();
    CbWriter::new(&CbLayout {
        name: "PerGeometry".into(),
        slot: 2,
        size: (offset("TextureProj") + 48) as u32,
        vars,
    })
}

fn floats(w: &CbWriter, name: &str) -> Vec<f32> {
    let (at, n) = (offset(name), FIELDS.iter().find(|f| f.0 == name).unwrap().1);
    w.bytes()[at..at + n as usize]
        .chunks(4)
        .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
        .collect()
}

fn flags(w: &CbWriter) -> u32 {
    let at = offset("PBRFlags");
    u32::from_le_bytes(w.bytes()[at..at + 4].try_into().unwrap())
}

#[test]
fn constants_from_layer() {
    let mut w = writer();
    write_projected(&mut w, &ash(), true);
    assert_eq!(floats(&w, "ProjectedUVParams"), [0.4, 0.0, 0.05, 0.5]);
    assert_eq!(floats(&w, "ProjectedUVParams2"), [1.0, 1.0, 1.0, 1.0]);
    assert_eq!(floats(&w, "ProjectedUVParams3"), [0.2, 0.8, 0.0, 1.0]);
    assert_eq!(floats(&w, "MaterialObjectRGBScale"), [0.66, 0.63, 0.63]);
    assert_eq!(floats(&w, "ParallaxOccData"), [0.9, 0.04, 0.0, 0.0]);
    assert_eq!(flags(&w) & (1 << 12), 0);
    assert_eq!(floats(&w, "SparkleParams"), [0.0; 4]);
    assert_eq!(
        floats(&w, "TextureProj"),
        [0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0]
    );
    // Not PBR: the PBR-only fields stay as they were.
    let mut v = writer();
    write_projected(&mut v, &ash(), false);
    assert_eq!(floats(&v, "ProjectedUVParams"), [0.4, 0.0, 0.05, 0.5]);
    assert_eq!(floats(&v, "MaterialObjectRGBScale"), [0.0; 3]);
    assert_eq!(floats(&v, "ParallaxOccData"), [0.0; 4]);
}

#[test]
fn snow_glint_constants() {
    let mut w = writer();
    w.set_u32("PBRFlags", &[4]);
    write_projected(&mut w, &snow(), true);
    assert_eq!(floats(&w, "SparkleParams"), [1.0, 27.0, 1.0, 5.0]);
    assert_eq!(flags(&w), 4 | (1 << 12), "ORed into the material's flags");
    assert_eq!(
        floats(&w, "ProjectedUVParams"),
        [0.35, 0.0, 1.0 / 48.0, 0.4]
    );
    // A vanilla shape gets no glint.
    let mut v = writer();
    write_projected(&mut v, &snow(), false);
    assert_eq!(flags(&v), 0);
    assert_eq!(floats(&v, "SparkleParams"), [0.0; 4]);
}

#[test]
fn zero_noise_scale_is_finite() {
    for scale in [0.0, -3.0] {
        let mut w = writer();
        let l = ProjectedLayer {
            noise_uv_scale: scale,
            ..snow()
        };
        write_projected(&mut w, &l, true);
        assert_eq!(floats(&w, "ProjectedUVParams")[2], 0.0);
        for (n, _) in FIELDS.iter().filter(|f| f.0 != "PBRFlags") {
            assert!(floats(&w, n).iter().all(|x| x.is_finite()), "{n}");
        }
    }
}

/// R1: the paths `set_projected_layer` could not resolve; empty once the layer is off.
#[test]
fn projected_missing_lists_unresolved_paths() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = common::renderer(cache.path(), common::FLAT);
    assert!(r.projected_missing().is_empty());
    let white = common::dds_rgba8(1, 1, &[[255; 4]]);
    let only_diffuse = |p: &str| (p == DIFFUSE).then(|| white.clone());
    r.set_projected_layer(Some(ash()), &only_diffuse);
    assert_eq!(
        r.projected_missing(),
        [
            "textures/effects/projectednormal.dds",
            "textures/effects/projectednormaldetail.dds",
            "textures/effects/projectednoise.dds"
        ]
    );
    // Undecodable bytes count as unresolved: the register shows its fallback either way.
    r.set_projected_layer(None, &|_| None);
    assert!(r.projected_missing().is_empty());
    r.set_projected_layer(Some(ash()), &|_| Some(vec![1, 2, 3]));
    assert_eq!(r.projected_missing().len(), 4);
    r.set_projected_layer(None, &|_| None);
    assert!(r.projected_missing().is_empty());
}

// ---- render_projected_*: the installed CS (`SDC_CS_SHADERS`); run alone with --test-threads=1. ----

fn cs_renderer(cache: &std::path::Path) -> Option<Renderer> {
    let Ok(shaders) = std::env::var("SDC_CS_SHADERS") else {
        println!("skipped: set SDC_CS_SHADERS");
        return None;
    };
    let fp = render3d::locate::fingerprint(std::path::Path::new(&shaders), None).unwrap();
    Some(Renderer::new(fp, cache.to_path_buf()).unwrap())
}

/// The built-in plane turned to face +Z (x, y, z) → (x, −z, y): normals, tangents and bitangents together, so the
/// tangent frame keeps its handedness. Identity world, as a NIF static at the origin.
fn up_quad(id: u32, m: MaterialState) -> render3d::ParsedShape {
    let mut mesh = render3d::builtin_mesh(render3d::Builtin::Plane);
    let rot = |v: &mut [f32; 3]| *v = [v[0], -v[2], v[1]];
    mesh.positions.iter_mut().for_each(rot);
    mesh.normals.iter_mut().for_each(rot);
    mesh.tangents.iter_mut().for_each(rot);
    mesh.bitangents.iter_mut().for_each(rot);
    render3d::ParsedShape {
        id: ShapeId(id),
        name: format!("quad{id}").into(),
        mesh: std::sync::Arc::new(mesh),
        world: common::IDENT,
        material: m,
    }
}

/// Type 0 PBR, mid-grey base colour at slot 0, an RMAOS at slot 5 that matches the ash layer's roughness 0.9 and
/// specular 0.04 (so the layer changes only the colour), depth test and write on. Glossiness is CS's specular-level
/// scale on PBR (`rawRMAOS.w *= PBRParams1.z`, Lighting.hlsl:1439): the vanilla default 80 would make it 3.2 and the
/// diffuse black.
fn grey_pbr() -> MaterialState {
    let mut m = pbr();
    m.params.glossiness = 1.0;
    m.slsf1 |= 1 << 31;
    m.slsf2 |= 1;
    m.set_slot_raw(0, r"textures\test\grey.dds");
    m.set_slot_raw(5, r"textures\test\grey_rmaos.dds");
    m
}

fn grey_textures(p: &str) -> Option<Vec<u8>> {
    match p {
        "textures/test/grey.dds" => Some(common::dds_rgba8(1, 1, &[[128, 128, 128, 255]])),
        "textures/test/grey_rmaos.dds" => Some(common::dds_rgba8(1, 1, &[[230, 0, 255, 10]])),
        _ => None,
    }
}

/// Looking down at the origin from above, white light from above, grey ambient: an unprojected grey quad reads
/// grey, so any red-over-blue comes from the layer.
fn top_scene() -> Scene {
    Scene {
        camera: Camera::Orbit {
            target: [0.0; 3],
            yaw: render3d::scene::AUTO_YAW,
            pitch: 1.3,
            distance: 120.0,
        },
        dir_light: DirLight {
            direction: [0.0, 0.0, 1.0],
            color: [2.0; 3],
        },
        ambient6: [[0.2; 3]; 6],
        ..Scene::neutral()
    }
}

fn centre(px: &[u8], w: u32, h: u32) -> [u8; 4] {
    let at = ((h / 2 * w + w / 2) * 4) as usize;
    px[at..at + 4].try_into().unwrap()
}

#[test]
fn render_projected_tints_up_faces() {
    let cache = tempfile::tempdir().unwrap();
    let Some(mut r) = cs_renderer(cache.path()) else {
        return;
    };
    let parsed = render3d::ParsedNif {
        shapes: vec![up_quad(0, grey_pbr())],
        ..Default::default()
    };
    let rep = r.load_parsed(&parsed, &grey_textures);
    assert!(
        rep.errors.is_empty() && rep.compile_errors.is_empty(),
        "{rep:?}"
    );
    let (w, h) = (128, 128);
    let off = centre(&r.render(&top_scene(), w, h).unwrap(), w, h);
    let white = common::dds_rgba8(1, 1, &[[255; 4]]);
    r.set_projected_layer(Some(ash()), &|p| (p == DIFFUSE).then(|| white.clone()));
    assert_eq!(r.shapes()[0].status, ShapeStatus::Drawing);
    assert_eq!(r.projected_missing().len(), 3);
    let on = centre(&r.render(&top_scene(), w, h).unwrap(), w, h);
    println!("off {off:?} on {on:?}");
    assert!(
        (off[0] as i32 - off[2] as i32).abs() <= 2,
        "grey without the layer: {off:?}"
    );
    assert!(
        on[0] > off[0],
        "brighter toward white × scale: {off:?} → {on:?}"
    );
    assert!(on[0] > on[2], "red over blue (0.66 vs 0.63): {on:?}");
    assert_eq!(
        r.draw_cb_floats(ShapeId(0), "MaterialObjectRGBScale"),
        Some(vec![0.66, 0.63, 0.63])
    );
    // A load keeps the layer and its textures; the fallbacks are not load-report missing textures (R1 lists them).
    let rep = r.load_parsed(&parsed, &grey_textures);
    assert!(rep.missing_textures.is_empty(), "{rep:?}");
    assert_eq!(centre(&r.render(&top_scene(), w, h).unwrap(), w, h), on);
    // Off again: the old look returns.
    r.set_projected_layer(None, &|_| None);
    assert_eq!(centre(&r.render(&top_scene(), w, h).unwrap(), w, h), off);
}

#[test]
fn render_projected_skips_skinned() {
    let cache = tempfile::tempdir().unwrap();
    let Some(mut r) = cs_renderer(cache.path()) else {
        return;
    };
    let mut parsed = render3d::parse(&common::fixture("TestNifFile_Skinned_SE.nif"));
    let skinned: Vec<ShapeId> = parsed
        .shapes
        .iter()
        .filter(|s| s.material.skinned)
        .map(|s| s.id)
        .collect();
    assert!(!skinned.is_empty());
    parsed.shapes.push(up_quad(9999, grey_pbr()));
    r.set_projected_layer(Some(ash()), &|_| None);
    let rep = r.load_parsed(&parsed, &grey_textures);
    assert!(rep.compile_errors.is_empty(), "{rep:?}");
    for id in &skinned {
        let s = r.shapes().into_iter().find(|s| s.id == *id).unwrap();
        assert_eq!(s.status, ShapeStatus::Drawing, "{}", s.name);
        let (ps, ..) = r.draw_diag(*id).unwrap();
        assert!(!ps.iter().any(|(k, _)| k == "PROJECTED_UV"), "{ps:?}");
    }
    let (ps, ..) = r.draw_diag(ShapeId(9999)).unwrap();
    assert!(ps.iter().any(|(k, _)| k == "PROJECTED_UV"), "{ps:?}");
    r.render(&top_scene(), 64, 64).unwrap();
}

#[test]
fn render_projected_value_edit_is_cbuffer_only() {
    let cache = tempfile::tempdir().unwrap();
    let Some(mut r) = cs_renderer(cache.path()) else {
        return;
    };
    let parsed = render3d::ParsedNif {
        shapes: vec![up_quad(0, grey_pbr())],
        ..Default::default()
    };
    r.set_projected_layer(Some(ash()), &|_| None);
    let rep = r.load_parsed(&parsed, &grey_textures);
    assert!(rep.compile_errors.is_empty(), "{rep:?}");
    r.set_async_compile(true);
    let files = common::cache_files(cache.path());
    r.set_projected_layer(
        Some(ProjectedLayer {
            roughness: 0.3,
            ..ash()
        }),
        &|_| None,
    );
    assert!(r.take_jobs().is_empty(), "no compile");
    assert_eq!(common::cache_files(cache.path()), files);
    assert_eq!(r.shapes()[0].status, ShapeStatus::Drawing);
    assert_eq!(
        r.draw_cb_floats(ShapeId(0), "ParallaxOccData").unwrap()[0],
        0.3
    );
    // Glint on is a permutation (GLINT): a job, the old draw stays on screen.
    r.set_projected_layer(Some(snow()), &|_| None);
    assert_eq!(r.take_jobs().len(), 1);
    assert_eq!(
        r.shapes()[0].status,
        ShapeStatus::Pending {
            showing_previous: true
        }
    );
}
