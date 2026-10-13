//! P1-C Task 8: the skin RFAOS lookup (pre-October CS rule, 02-skin-rfaos.md §0) on WARP with the stand-in shader.
//! `STANDIN_SKIN`: pixels = 0.25·diffuse + skinMaterialFlags.x × t71.
mod common;
use common::{cache_files, channel, dds_rgba8, dds_rgba8_dx10, fixture, renderer};
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
    let files: Vec<(&str, Vec<u8>)> = files
        .iter()
        .map(|(p, c)| (*p, dds_rgba8(1, 1, &[*c])))
        .collect();
    skin_dds(cache, &files, shader_type)
}

/// As [`skin`] with the candidates' DDS bytes given.
fn skin_dds(cache: &Path, files: &[(&str, Vec<u8>)], shader_type: u32) -> Renderer {
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

/// Fix round 1: t71 is sampled linear. The same 128-grey texels tagged `R8G8B8A8_UNORM_SRGB` and `_UNORM` give the
/// same frame (an sRGB view would decode 128 to ~55 and darken the added grey).
#[test]
fn an_srgb_tagged_rfaos_is_sampled_linear() {
    let grey = [128, 128, 128, 255];
    let px = |srgb: bool| {
        let cache = tempfile::tempdir().unwrap();
        let mut r = skin_dds(
            cache.path(),
            &[(FROM_S, dds_rgba8_dx10(1, 1, &[grey], srgb))],
            4,
        );
        assert_eq!(flags(&r, CYL1)[0], 1.0);
        frame(&mut r)
    };
    let (linear, srgb) = (px(false), px(true));
    assert_eq!(channel(&srgb, 0), channel(&linear, 0));
}

/// Diagnosis (owner data, read-only): the Arch head on the INSTALLED CS shaders with the owner's settings, features
/// on. Prints the PS defines, the t71 / b7 reflection, the RFAOS candidate and its binding, the b7 bytes, and renders
/// the head with and without the RFAOS (PNGs in %TEMP%\sdc-rfaos-diag).
#[test]
#[ignore]
fn diag_arch_head_on_installed_cs() {
    let root = PathBuf::from(std::env::var("SDC_ACHERUS").expect("SDC_ACHERUS"));
    let mods = root.join("mods");
    let cs = mods.join("CommunityShaders_AIO-2026-10-03T06-46Z");
    let arch = mods.join("Arch Resources");
    let origins = vec![
        ("CS".to_string(), cs.clone(), 0),
        ("overwrite".to_string(), root.join("overwrite"), i32::MAX),
    ];
    let paths = render3d::locate::settings_paths(Some(&origins), &cs.join("Shaders"));
    println!("settings paths: {paths:?}");
    let settings = render3d::cs_settings::read_settings(&paths);
    let no_origins = render3d::cs_settings::read_settings(&render3d::locate::settings_paths(
        None,
        &cs.join("Shaders"),
    ));
    println!(
        "hash (12): with origins {} / no origins {} / default {}; no-origins loaded {:?}",
        &settings.hash[..12],
        &no_origins.hash[..12],
        &render3d::cs_settings::CsSettings::default().hash[..12],
        no_origins.loaded
    );
    println!(
        "settings: ll={} loaded={:?} notes={:?}",
        settings.linear_lighting, settings.loaded, settings.notes
    );
    let mut roots = vec![arch.clone()];
    roots.extend(
        std::fs::read_dir(&mods)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir() && *p != arch),
    );
    let find = |norm: &str| {
        roots
            .iter()
            .map(|r| r.join(norm.replace('/', "\\")))
            .find(|p| p.is_file())
    };
    let nif = std::fs::read(arch.join(r"meshes\arch\head\Arch_head.nif")).unwrap();
    let out = std::env::temp_dir().join("sdc-rfaos-diag");
    std::fs::create_dir_all(&out).unwrap();
    let cache = tempfile::tempdir().unwrap();
    let fp = render3d::locate::fingerprint(&cs.join("Shaders"), None).unwrap();
    let mut r = Renderer::with_gpu(
        render3d::device::Gpu::new().unwrap(),
        fp,
        cache.path().to_path_buf(),
    )
    .unwrap();
    r.set_settings(settings);
    r.set_features_on(true);
    println!("features: {:?}", r.features());
    for hide in [false, true] {
        let asked = RefCell::new(vec![]);
        let resolve = |norm: &str| {
            if hide && norm.ends_with("_rfaos.dds") {
                asked.borrow_mut().push((norm.to_string(), None));
                return None;
            }
            let p = find(norm);
            asked.borrow_mut().push((norm.to_string(), p.clone()));
            p.and_then(|p| std::fs::read(p).ok())
        };
        let t0 = std::time::Instant::now();
        let rep = r.load(&nif, &resolve);
        println!(
            "\n=== hide_rfaos={hide} load {:.1}s: {rep:?}",
            t0.elapsed().as_secs_f64()
        );
        for (n, p) in asked.borrow().iter() {
            println!("  asked {n} -> {p:?}");
        }
        for s in r.shapes() {
            println!(
                "shape {} '{}' type {} slsf1 {:#x} slsf2 {:#x} status {:?}",
                s.id.0,
                s.name,
                s.material.shader_type,
                s.material.slsf1,
                s.material.slsf2,
                s.status
            );
            for (i, t) in s.material.slots.iter().enumerate() {
                if !t.norm.is_empty() {
                    println!("  slot {i}: {}", t.norm);
                }
            }
            let (defines, refl, srvs) = r.draw_diag(s.id).unwrap();
            println!("  PS defines: {defines:?}");
            match &refl {
                None => println!("  PS reflection: program not at hand"),
                Some(refl) => {
                    println!("  PS srvs: {:?}", refl.srvs);
                    for cb in &refl.cbuffers {
                        if cb.slot == 7 || cb.name.contains("Skin") {
                            println!("  cbuffer {} b{} size {}:", cb.name, cb.slot, cb.size);
                            for v in &cb.vars {
                                println!(
                                    "    {} @{} size {} {:?}",
                                    v.name, v.offset, v.size, v.kind
                                );
                            }
                        }
                    }
                }
            }
            println!("  bound (reg, dxgi): {srvs:?}");
            for var in [
                "skinPerGeometry",
                "skinMaterialFlags",
                "skinPerGeometryProfile.skinParams",
                "skinPerGeometryProfile.skinParams2",
                "skinPerGeometryProfile.physicalParams",
            ] {
                println!("  {var} = {:?}", r.draw_cb_floats(s.id, var));
            }
        }
        let (w, h) = (512, 512);
        let px = r.render(&Scene::neutral(), w, h).unwrap();
        let head: Vec<&[u8]> = px
            .chunks(4)
            .filter(|p| p[0] as u32 + p[1] as u32 + p[2] as u32 > 30)
            .collect();
        let n = head.len().max(1) as f64;
        let mean = |c: usize| head.iter().map(|p| p[c] as f64).sum::<f64>() / n;
        println!(
            "render: {} head px, mean RGB ({:.1}, {:.1}, {:.1})",
            head.len(),
            mean(0),
            mean(1),
            mean(2)
        );
        let png = out.join(format!("arch_head_hide_rfaos_{hide}.png"));
        image::save_buffer(&png, &px, w, h, image::ExtendedColorType::Rgba8).unwrap();
        println!("saved {}", png.display());
    }
    let rfaos = find("textures/arch/skin/light/head_rfaos.dds").unwrap();
    let (img, _, meta) = engine::load::load_image_meta(&rfaos).unwrap();
    let n = img.px.len() as f64;
    let m = |c: usize| img.px.iter().map(|p| p[c] as f64).sum::<f64>() / n;
    println!(
        "\nRFAOS {} {}x{} meta {meta:?}: mean R {:.3} G {:.3} B {:.3} A {:.3}",
        rfaos.display(),
        img.width,
        img.height,
        m(0),
        m(1),
        m(2),
        m(3)
    );
}

/// Diagnosis (owner data, read-only): the Arch scalp (hair, flow map in the backlight slot) on the INSTALLED CS
/// shaders, once with the owner's MO2 settings (CS_HAIR on) and once with the settings the GUI log shows it used
/// (`CsSettings::default()`, ENABLE_LL only). Prints the scalp's flags, slots → registers, PS defines and the
/// written `hairSpecularSettings.Enabled`; PNGs in %TEMP%\sdc-rfaos-diag.
#[test]
#[ignore]
fn diag_arch_scalp_on_installed_cs() {
    let root = PathBuf::from(std::env::var("SDC_ACHERUS").expect("SDC_ACHERUS"));
    let mods = root.join("mods");
    let cs = mods.join("CommunityShaders_AIO-2026-10-03T06-46Z");
    let arch = mods.join("Arch Resources");
    let origins = vec![
        ("CS".to_string(), cs.clone(), 0),
        ("overwrite".to_string(), root.join("overwrite"), i32::MAX),
    ];
    let owner = render3d::cs_settings::read_settings(&render3d::locate::settings_paths(
        Some(&origins),
        &cs.join("Shaders"),
    ));
    let resolve = |norm: &str| std::fs::read(arch.join(norm.replace('/', "\\"))).ok();
    let nif = std::fs::read(arch.join(r"meshes\arch\head\hair\scalp.nif")).unwrap();
    let out = std::env::temp_dir().join("sdc-rfaos-diag");
    std::fs::create_dir_all(&out).unwrap();
    for (label, settings) in [
        ("owner_settings", owner),
        (
            "gui_default_settings",
            render3d::cs_settings::CsSettings::default(),
        ),
    ] {
        let cache = tempfile::tempdir().unwrap();
        let fp = render3d::locate::fingerprint(&cs.join("Shaders"), None).unwrap();
        let mut r = Renderer::with_gpu(
            render3d::device::Gpu::new().unwrap(),
            fp,
            cache.path().to_path_buf(),
        )
        .unwrap();
        r.set_settings(settings);
        r.set_features_on(true);
        let rep = r.load(&nif, &resolve);
        println!("\n=== {label}: features {:?}\n{rep:?}", r.features());
        for s in r.shapes() {
            let m = &s.material;
            println!(
                "shape {} '{}' type {} slsf1 {:#x} slsf2 {:#x} (SLSF2.27 back lighting: {}) pbr {} status {:?}",
                s.id.0,
                s.name,
                m.shader_type,
                m.slsf1,
                m.slsf2,
                m.slsf2 & (1 << 27) != 0,
                m.is_pbr(),
                s.status
            );
            let msn = m.slsf1 & (1 << 12) != 0;
            for (i, t) in m.slots.iter().enumerate() {
                if !t.norm.is_empty() {
                    let reg =
                        render3d::textures::register_for_slot(m.shader_type, m.is_pbr(), msn, i);
                    println!("  slot {i}: {} -> t{reg:?}", t.norm);
                }
            }
            let (defines, refl, srvs) = r.draw_diag(s.id).unwrap();
            println!("  PS defines: {defines:?}");
            println!("  bound (reg, dxgi): {srvs:?}");
            if let Some(refl) = refl {
                println!("  PS srvs: {:?}", refl.srvs);
                for cb in &refl.cbuffers {
                    for v in cb
                        .vars
                        .iter()
                        .filter(|v| v.name.contains("hairSpecularSettings.Enabled"))
                    {
                        println!(
                            "  reflected {} in {} b{} @{} {:?}",
                            v.name, cb.name, cb.slot, v.offset, v.kind
                        );
                    }
                }
            }
            let enabled = r.draw_cb_floats(s.id, "hairSpecularSettings.Enabled");
            println!(
                "  hairSpecularSettings.Enabled written bits: {:?}",
                enabled.map(|v| v.iter().map(|f| f.to_bits()).collect::<Vec<_>>())
            );
        }
        let (w, h) = (512, 512);
        let px = r.render(&Scene::neutral(), w, h).unwrap();
        let hit: Vec<&[u8]> = px
            .chunks(4)
            .filter(|p| p[0] as u32 + p[1] as u32 + p[2] as u32 > 30)
            .collect();
        let n = hit.len().max(1) as f64;
        let mean = |c: usize| hit.iter().map(|p| p[c] as f64).sum::<f64>() / n;
        println!(
            "render: {} px, mean RGB ({:.1}, {:.1}, {:.1})",
            hit.len(),
            mean(0),
            mean(1),
            mean(2)
        );
        // scalp.png = the owner's settings via MO2 origins (what the fixed GUI reads).
        let png = out.join(match label {
            "owner_settings" => "scalp.png".to_string(),
            _ => format!("scalp_{label}.png"),
        });
        image::save_buffer(&png, &px, w, h, image::ExtendedColorType::Rgba8).unwrap();
        println!("saved {}", png.display());
    }
}
