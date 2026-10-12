use render3d::compile::Compiled;
use render3d::permutation::{defines, runtime_bits, Defines, Material, Stage};
use render3d::source::SourceSet;
use std::path::Path;

/// The pre-P1-B call shape (file, root, defines, target, cache, hash): expand `file` under `root`, then
/// `render3d::compile::compile`. The hash argument is ignored: the key is the content now.
fn compile(
    file: &Path,
    root: &Path,
    d: &Defines,
    target: &str,
    cache: &Path,
    _hash: &str,
) -> Result<Compiled, String> {
    let rel = file.strip_prefix(root).unwrap_or(file);
    let src = SourceSet::new(root.to_path_buf()).get(rel)?;
    render3d::compile::compile(&src, d, target, cache)
}

fn m(shader_type: u32, slsf1: u32, slsf2: u32, skinned: bool) -> Material {
    Material {
        shader_type,
        slsf1,
        slsf2,
        skinned,
        alpha_test: false,
        has_vertex_colors: false,
        specular_strength: 1.0,
    }
}
fn names(d: &render3d::permutation::Defines) -> Vec<&str> {
    d.0.iter().map(|x| x.0.as_str()).collect()
}

#[test]
fn define_table() {
    let d = defines(&m(5, 1 << 12, 0, true), Stage::Pixel, &["CS_SKIN"], false);
    for want in [
        "PSHADER",
        "SKINNED",
        "MODELSPACENORMALS",
        "FACEGEN_RGB_TINT",
        "CS_SKIN",
    ] {
        assert!(names(&d).contains(&want), "{want} in {:?}", names(&d));
    }
    let glow_ps = defines(&m(2, 0, 0, false), Stage::Pixel, &[], false);
    assert!(!names(&glow_ps).iter().any(|n| n.contains("GLOW")));
    let mlp = defines(&m(11, 0, 0, false), Stage::Pixel, &[], false);
    assert!(names(&mlp).contains(&"MULTI_LAYER_PARALLAX") && names(&mlp).contains(&"ENVMAP"));
    let pbr = defines(&m(0, 0, 1 << 23, false), Stage::Pixel, &[], false);
    assert!(names(&pbr).contains(&"TRUE_PBR"));
    let pbr_vs = defines(&m(0, 0, 1 << 23, false), Stage::Vertex, &[], false);
    assert!(!names(&pbr_vs).contains(&"TRUE_PBR"));
    let skin_vs = defines(&m(5, 0, 0, true), Stage::Vertex, &[], false);
    assert!(!names(&skin_vs).contains(&"FACEGEN_RGB_TINT"));

    // RB7 (answers.md Q4.2): Specular/Soft/Rim/Back are compile-time in installed CS 1.9.
    let lit = defines(
        &m(4, 1, (1 << 25) | (1 << 26) | (1 << 27), true),
        Stage::Pixel,
        &[],
        false,
    );
    for want in ["SPECULAR", "SOFT_LIGHTING", "RIM_LIGHTING", "BACK_LIGHTING"] {
        assert!(names(&lit).contains(&want), "{want} in {:?}", names(&lit));
    }
    let lit_vs = defines(&m(4, 1, 1 << 25, true), Stage::Vertex, &[], false);
    assert!(!names(&lit_vs).contains(&"SPECULAR") && !names(&lit_vs).contains(&"SOFT_LIGHTING"));
    // Specular strength 0 drops SPECULAR although SLSF1.0 is set (Arch_Mouth capture).
    let mut mouth = m(5, 1, 0, true);
    mouth.specular_strength = 0.0;
    assert!(!names(&defines(&mouth, Stage::Pixel, &[], false)).contains(&"SPECULAR"));
    // Under TruePBR these flags remap to PBRFlags instead (Arch_Eye: SF2.26 -> no Rim).
    let pbr_rim = defines(
        &m(0, 1, (1 << 23) | (1 << 26) | (1 << 25), false),
        Stage::Pixel,
        &[],
        false,
    );
    for no in ["SPECULAR", "SOFT_LIGHTING", "RIM_LIGHTING"] {
        assert!(
            !names(&pbr_rim).contains(&no),
            "{no} in {:?}",
            names(&pbr_rim)
        );
    }
    // Aniso: ANISO_LIGHTING, or GLINT with TruePBR.
    assert!(
        names(&defines(&m(6, 0, 1 << 21, true), Stage::Pixel, &[], false))
            .contains(&"ANISO_LIGHTING")
    );
    // TruePBR: GLINT comes from CS's remapped Glint flag (Fit_Slope 12 without MLP 24 / Soft 25 / Back 27),
    // never from SLSF2.21, which CS clears at load (P1 spec §2.2).
    let pbr_ps = |slsf2: u32| -> Vec<String> {
        names(&defines(
            &m(11, 0, slsf2 | (1 << 23), false),
            Stage::Pixel,
            &[],
            false,
        ))
        .iter()
        .map(|s| s.to_string())
        .collect()
    };
    let has = |v: &[String], n: &str| v.iter().any(|x| x == n);
    assert!(!has(&pbr_ps(1 << 21), "GLINT") && !has(&pbr_ps(1 << 21), "ANISO_LIGHTING"));
    assert!(has(&pbr_ps(1 << 12), "GLINT"));
    assert!(
        has(&pbr_ps((1 << 12) | (1 << 26)), "GLINT"),
        "subsurface + glint"
    );
    for blocker in [24, 25, 27] {
        assert!(
            !has(&pbr_ps((1 << 12) | (1 << blocker)), "GLINT"),
            "{blocker}"
        );
    }
    assert!(
        !names(&defines(&m(0, 0, 1 << 12, false), Stage::Pixel, &[], false)).contains(&"GLINT")
    );
    // DO_ALPHA_TEST whenever the shape has an NiAlphaProperty (blend-only included).
    let mut alpha = m(6, 0, 0, true);
    alpha.alpha_test = true;
    assert!(names(&defines(&alpha, Stage::Pixel, &[], false)).contains(&"DO_ALPHA_TEST"));
    assert!(!names(&defines(&alpha, Stage::Vertex, &[], false)).contains(&"DO_ALPHA_TEST"));
    // HAIR + Decal / Dynamic_Decal → DEPTH_WRITE_DECALS (PS only); not on other types.
    for decal in [26, 27] {
        let hair = m(6, 1 << decal, 0, true);
        assert!(names(&defines(&hair, Stage::Pixel, &[], false)).contains(&"DEPTH_WRITE_DECALS"));
        assert!(!names(&defines(&hair, Stage::Vertex, &[], false)).contains(&"DEPTH_WRITE_DECALS"));
        let not_hair = m(0, 1 << decal, 0, true);
        assert!(
            !names(&defines(&not_hair, Stage::Pixel, &[], false)).contains(&"DEPTH_WRITE_DECALS")
        );
    }
    assert!(
        !names(&defines(&m(6, 0, 0, true), Stage::Pixel, &[], false))
            .contains(&"DEPTH_WRITE_DECALS")
    );
    // TruePBR drops ENVMAP / PARALLAX / MULTI_LAYER_PARALLAX on both stages (Arch_Eye_Moisture capture).
    for st in [1, 3, 11] {
        for stage in [Stage::Pixel, Stage::Vertex] {
            let d = defines(&m(st, 0, 1 << 23, false), stage, &[], false);
            for no in ["ENVMAP", "PARALLAX", "MULTI_LAYER_PARALLAX"] {
                assert!(
                    !names(&d).contains(&no),
                    "type {st} {stage:?}: {no} in {:?}",
                    names(&d)
                );
            }
        }
    }
    // FaceGen keeps its technique under PBR (guarded at Lighting.hlsl:366).
    assert!(
        names(&defines(&m(4, 0, 1 << 23, true), Stage::Pixel, &[], false)).contains(&"FACEGEN")
    );
    // VS VC needs SLSF2.5 and vertex colours present.
    let mut vc = m(0, 0, 1 << 5, false);
    assert!(!names(&defines(&vc, Stage::Vertex, &[], false)).contains(&"VC"));
    vc.has_vertex_colors = true;
    assert!(names(&defines(&vc, Stage::Vertex, &[], false)).contains(&"VC"));
    assert!(!names(&defines(&vc, Stage::Pixel, &[], false)).contains(&"VC"));
}

/// RB7 / answers.md Q4.1: cb4 `PixelShaderDescriptor` = engine PS desc & ~CS compile key. Specular/Soft/Rim/Back
/// are compile-time defines in installed CS 1.9 and never reach cb4; CharacterLight/DefShadow stay 0 in the preview.
/// The Glowmap technique is still read at run time (Lighting.hlsl:2598), so type 2 keeps `2 << 24`.
#[test]
fn runtime_bits_table() {
    let b = runtime_bits(&m(2, 1, (1 << 25) | (1 << 27), false));
    assert_eq!(b, 2 << 24);
    assert_eq!(
        runtime_bits(&m(0, 1, (1 << 25) | (1 << 26) | (1 << 27), true)),
        0
    );
    assert_eq!(runtime_bits(&m(5, 1 << 12, 0, true)), 0);
}

#[test]
fn tiny_shader_compiles_and_reflects() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
    let cache = tempfile::tempdir().unwrap();
    let d = render3d::permutation::Defines(vec![("PSHADER".into(), "1".into())]);
    let c = compile(
        std::path::Path::new(&format!("{dir}/tiny.hlsl")),
        std::path::Path::new(dir),
        &d,
        "ps_5_0",
        cache.path(),
        "h",
    )
    .unwrap();
    let cb = &c.reflection.cbuffers[0];
    assert_eq!(cb.name, "PerMaterial");
    assert_eq!(cb.slot, 1);
    assert!(cb.vars.iter().any(|v| v.name == "Tint"
        && v.offset == 16
        && v.kind == render3d::compile::VarKind::Float));
    use render3d::compile::SrvKind;
    assert_eq!(
        c.reflection.srvs,
        vec![
            ("TexColor".to_string(), 0, SrvKind::Tex2D),
            ("TexEnv".to_string(), 4, SrvKind::Cube)
        ]
    );
    assert_eq!(c.reflection.samplers, vec![("S".to_string(), 0)]);
    assert!(c
        .reflection
        .inputs
        .iter()
        .any(|i| i.0 == "TEXCOORD" && i.1 == 0));
    let again = compile(
        std::path::Path::new(&format!("{dir}/tiny.hlsl")),
        std::path::Path::new(dir),
        &d,
        "ps_5_0",
        cache.path(),
        "h",
    )
    .unwrap();
    assert_eq!(again.bytecode, c.bytecode);
    assert_eq!(again.reflection, c.reflection);
    assert_eq!(std::fs::read_dir(cache.path()).unwrap().count(), 2);
}

#[test]
fn compile_error_is_reported_not_fatal() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("bad.hlsl"),
        "float4 main() : SV_Target { return undefined_symbol; }",
    )
    .unwrap();
    let e = compile(
        &d.path().join("bad.hlsl"),
        d.path(),
        &Default::default(),
        "ps_5_0",
        d.path(),
        "h",
    )
    .unwrap_err();
    assert!(e.contains("undefined_symbol"), "{e}");
}

#[test]
fn cbuffer_writer_sets_by_name() {
    let layout = render3d::compile::CbLayout {
        name: "PerMaterial".into(),
        slot: 1,
        size: 32,
        vars: vec![
            render3d::compile::CbVar {
                name: "Scale".into(),
                offset: 0,
                size: 4,
                kind: render3d::compile::VarKind::Float,
            },
            render3d::compile::CbVar {
                name: "Tint".into(),
                offset: 16,
                size: 12,
                kind: render3d::compile::VarKind::Float,
            },
        ],
    };
    let mut w = render3d::cbuffer::CbWriter::new(&layout);
    assert!(w.set("Tint", &[1.0, 0.5, 0.25]));
    assert!(!w.set("NotInThisBuild", &[1.0]));
    assert_eq!(&w.bytes()[16..20], &1.0f32.to_le_bytes());
    assert_eq!(&w.bytes()[24..28], &0.25f32.to_le_bytes());
}

/// Opt-in: compiles the user's installed CS Lighting.hlsl (Global Constraints path).
/// `SDC_CS_SHADERS=...\Shaders cargo test -p render3d -- --ignored installed_cs`
#[test]
#[ignore]
fn installed_cs() {
    let root = std::path::PathBuf::from(std::env::var("SDC_CS_SHADERS").unwrap());
    let cache = tempfile::tempdir().unwrap();
    for (st, skinned, target, stage) in [
        (0, true, "ps_5_0", Stage::Pixel),
        (5, true, "ps_5_0", Stage::Pixel),
        (0, true, "vs_5_0", Stage::Vertex),
        (1, false, "ps_5_0", Stage::Pixel),
    ] {
        let d = defines(&m(st, 1 << 12, 0, skinned), stage, &[], false);
        let t = std::time::Instant::now();
        let c = compile(
            &root.join("Lighting.hlsl"),
            &root,
            &d,
            target,
            cache.path(),
            "x",
        )
        .unwrap_or_else(|e| panic!("{st}: {e}"));
        println!(
            "type {st} {target}: {} ms, {} cbuffers, {} srvs",
            t.elapsed().as_millis(),
            c.reflection.cbuffers.len(),
            c.reflection.srvs.len()
        );
        assert!(!c.reflection.cbuffers.is_empty());
    }
    // RB7 defines: hair with Specular/Soft/Rim/Back/Aniso + alpha, and TruePBR with Glint.
    let mut hair = m(6, 1, (1 << 21) | (1 << 25) | (1 << 26) | (1 << 27), true);
    hair.alpha_test = true;
    let pbr = m(0, 1, (1 << 21) | (1 << 23), false);
    let mut decal_hair = m(6, (1 << 26) | (1 << 27), 1 << 27, true);
    decal_hair.alpha_test = true;
    let mlp_pbr = m(11, 0, (1 << 23) | (1 << 25) | (1 << 27), false);
    let env_pbr = m(1, 0, 1 << 23, false);
    for (name, mat, stage, target) in [
        ("hair", hair, Stage::Pixel, "ps_5_0"),
        ("pbr+glint", pbr, Stage::Pixel, "ps_5_0"),
        ("hair+decal", decal_hair, Stage::Pixel, "ps_5_0"),
        ("mlp+pbr ps", mlp_pbr, Stage::Pixel, "ps_5_0"),
        ("mlp+pbr vs", mlp_pbr, Stage::Vertex, "vs_5_0"),
        ("envmap+pbr ps", env_pbr, Stage::Pixel, "ps_5_0"),
        ("envmap+pbr vs", env_pbr, Stage::Vertex, "vs_5_0"),
    ] {
        let d = defines(&mat, stage, &[], false);
        let t = std::time::Instant::now();
        let c = compile(
            &root.join("Lighting.hlsl"),
            &root,
            &d,
            target,
            cache.path(),
            "x",
        )
        .unwrap_or_else(|e| panic!("{name}: {e}"));
        println!("{name} {target}: {} ms", t.elapsed().as_millis());
        assert!(!c.reflection.cbuffers.is_empty());
    }
}

#[test]
fn truncated_cache_entry_is_recompiled() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
    let cache = tempfile::tempdir().unwrap();
    let d = render3d::permutation::Defines(vec![("PSHADER".into(), "1".into())]);
    let file = std::path::PathBuf::from(format!("{dir}/tiny.hlsl"));
    let run = || {
        compile(
            &file,
            std::path::Path::new(dir),
            &d,
            "ps_5_0",
            cache.path(),
            "h",
        )
        .unwrap()
    };
    let first = run();
    let cso = std::fs::read_dir(cache.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|x| x == "cso"))
        .unwrap();
    std::fs::write(&cso, &first.bytecode[..first.bytecode.len() / 2]).unwrap();
    let again = run();
    assert_eq!(again.bytecode, first.bytecode);
    assert_eq!(
        std::fs::read(&cso).unwrap(),
        first.bytecode,
        "cache entry rewritten"
    );
    // Garbage of the right length is rejected too (bad DXBC magic).
    std::fs::write(&cso, vec![0u8; first.bytecode.len()]).unwrap();
    assert_eq!(run().bytecode, first.bytecode);
    let names: Vec<_> = std::fs::read_dir(cache.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names.len(), 2, "no temp files left behind: {names:?}");
}

#[test]
fn cache_key_uses_path_relative_to_root() {
    let d = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    for (sub, v) in [("a", "1"), ("b", "0.5")] {
        std::fs::create_dir_all(d.path().join(sub)).unwrap();
        std::fs::write(
            d.path().join(sub).join("x.hlsl"),
            format!("float4 main() : SV_Target {{ return {v}; }}"),
        )
        .unwrap();
    }
    let c = |sub: &str| {
        compile(
            &d.path().join(sub).join("x.hlsl"),
            d.path(),
            &Default::default(),
            "ps_5_0",
            cache.path(),
            "h",
        )
        .unwrap()
        .bytecode
    };
    assert_ne!(c("a"), c("b"));
}

fn feature_kinds_layout(cache: &Path) -> render3d::compile::CbLayout {
    let dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"));
    let c = compile(
        &dir.join("feature_kinds.hlsl"),
        dir,
        &Default::default(),
        "ps_5_0",
        cache,
        "h",
    )
    .unwrap();
    c.reflection
        .cbuffers
        .into_iter()
        .find(|l| l.name.ends_with("FeatureData"))
        .unwrap()
}

/// C6: every reflected variable carries its HLSL scalar type, struct members included; a struct is `Other`.
#[test]
fn reflection_reports_member_kinds() {
    use render3d::compile::VarKind;
    let cache = tempfile::tempdir().unwrap();
    let l = feature_kinds_layout(cache.path());
    assert_eq!(l.slot, 6);
    let kind = |n: &str| l.vars.iter().find(|v| v.name.ends_with(n)).map(|v| v.kind);
    assert_eq!(kind("hairSpecularSettings"), Some(VarKind::Other));
    assert_eq!(kind("hairSpecularSettings.Enabled"), Some(VarKind::Uint));
    assert_eq!(
        kind("hairSpecularSettings.HairGlossiness"),
        Some(VarKind::Float)
    );
    assert_eq!(kind("hairSpecularSettings.Signed"), Some(VarKind::Int));
    assert_eq!(kind("hairSpecularSettings.Flag"), Some(VarKind::Bool));
    assert_eq!(kind("skinParams"), Some(VarKind::Float));
    let w = render3d::cbuffer::CbWriter::new(&l);
    assert_eq!(w.kind("hairSpecularSettings.Enabled"), Some(VarKind::Uint));
    assert_eq!(w.kind("notInThisBuild"), None);
}

/// C6, read back from the writer's bytes: a `uint` member gets the integer's bit pattern (1, not 0x3F800000), a
/// `float` member the f32's, `int` two's complement, `bool` 1; a name the layout lacks is skipped.
#[test]
fn feature_data_writes_reflected_kinds() {
    let cache = tempfile::tempdir().unwrap();
    let l = feature_kinds_layout(cache.path());
    let mut w = render3d::cbuffer::CbWriter::new(&l);
    render3d::cs_settings::write_feature_data(
        &mut w,
        &[
            ("hairSpecularSettings.Enabled".into(), vec![1.0]),
            ("hairSpecularSettings.HairGlossiness".into(), vec![70.0]),
            ("hairSpecularSettings.Signed".into(), vec![-2.0]),
            ("hairSpecularSettings.Flag".into(), vec![1.0]),
            ("skinParams".into(), vec![0.7, 0.35, 1.0, 1.0]),
            ("notInThisBuild".into(), vec![5.0]),
        ],
    );
    let word = |n: &str, i: usize| {
        let at = w.resolve(n).unwrap().off as usize + 4 * i;
        u32::from_le_bytes(w.bytes()[at..at + 4].try_into().unwrap())
    };
    assert_eq!(word("hairSpecularSettings.Enabled", 0), 1);
    assert_eq!(
        word("hairSpecularSettings.HairGlossiness", 0),
        70.0f32.to_bits()
    );
    assert_eq!(word("hairSpecularSettings.Signed", 0), (-2i32) as u32);
    assert_eq!(word("hairSpecularSettings.Flag", 0), 1);
    assert_eq!(word("skinParams", 0), 0.7f32.to_bits());
    assert_eq!(word("skinParams", 3), 1.0f32.to_bits());
}
