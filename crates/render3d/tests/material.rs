//! CS's TruePBR load remap (01 §3.1-3.4) and the material cbuffer fields, writer-level (no GPU).
use nif::material::MaterialState;
use nif::props::LightingTrailer;
use render3d::cbuffer::CbWriter;
use render3d::compile::{CbLayout, CbVar, VarKind};
use render3d::material::{
    pbr_flags, perm_material, remapped_glint, write_material, Bound, PBR_COAT_NORMAL,
    PBR_COLORED_COAT, PBR_FUZZ, PBR_GLINT, PBR_HAIR_MARSCHNER, PBR_HAS_DISPLACEMENT,
    PBR_HAS_EMISSIVE, PBR_HAS_FEATURES0, PBR_HAS_FEATURES1, PBR_INTERLAYER_PARALLAX,
    PBR_SUBSURFACE, PBR_TWO_LAYER,
};
use render3d::permutation::{defines, Stage};
use render3d::scene::srgb_to_linear;

/// `builtin_default` with the TruePBR bit and the given SLSF2 bits.
fn pbr(bits: &[u32]) -> MaterialState {
    let mut m = MaterialState::builtin_default();
    m.slsf2 |= 1 << 23;
    for b in bits {
        m.slsf2 |= 1 << b;
    }
    m
}

fn bound(regs: &[u32]) -> Bound {
    Bound {
        regs: regs.iter().copied().collect(),
    }
}

/// Shader type 11 with the MultiLayer trailer floats f0..f3 (envmap strength 0.5).
fn mlp(m: &mut MaterialState, f: [f32; 4]) {
    m.shader_type = 11;
    m.params.trailer = LightingTrailer::MultiLayer {
        inner_thickness: f[0],
        refraction_scale: f[1],
        inner_texture_scale: [f[2], f[3]],
        envmap_strength: 0.5,
    };
}

#[test]
fn pbr_flags_eye_moisture_is_428() {
    // Capture oracle (answers.md Q4 table + Q1.3): Arch_Eye_Moisture, type 11, SLSF2 bits 4,7,15,23,24,25,27.
    // ev6721 binds t9 and t12 (plus the always-bound t0/t1/t5): 256 + 128 + 32 + 8 + 4 = 428.
    let mut m = pbr(&[4, 7, 15, 24, 25, 27]);
    m.shader_type = 11;
    assert_eq!(pbr_flags(&m, &bound(&[0, 1, 5, 9, 12])), 428);
    // ev6595 also binds the displacement map at t4: 430.
    assert_eq!(pbr_flags(&m, &bound(&[0, 1, 4, 5, 9, 12])), 430);
}

#[test]
fn pbr_flags_arch_eye_is_20() {
    // answers.md Q4 table: Arch_Eye type 0, SLSF2 8,15,23,26 → 20 = Features0 + Subsurface.
    assert_eq!(pbr_flags(&pbr(&[8, 15, 26]), &bound(&[0, 1, 5, 12])), 20);
}

#[test]
fn pbr_flags_branch_table() {
    let none = bound(&[]);
    let rows: &[(&[u32], u32, &str)] = &[
        (&[], 0, "plain"),
        (&[27], PBR_HAIR_MARSCHNER, "hair"),
        (
            &[27, 26, 25, 12],
            PBR_HAIR_MARSCHNER,
            "hair excludes subsurface/fuzz/glint",
        ),
        (&[26], PBR_SUBSURFACE, "subsurface"),
        (&[25], PBR_FUZZ, "fuzz"),
        (&[25, 12], PBR_FUZZ, "fuzz excludes glint"),
        (&[12], PBR_GLINT, "glint, also a leaked Fit_Slope"),
        (
            &[26, 25],
            PBR_SUBSURFACE | PBR_FUZZ,
            "subsurface is independent of fuzz",
        ),
        (&[26, 12], PBR_SUBSURFACE | PBR_GLINT, "subsurface + glint"),
        (&[24], PBR_TWO_LAYER, "two layer"),
        (
            &[24, 25, 27, 30],
            PBR_TWO_LAYER | PBR_INTERLAYER_PARALLAX | PBR_COAT_NORMAL | PBR_COLORED_COAT,
            "coat cave",
        ),
        (
            &[24, 26, 12],
            PBR_TWO_LAYER,
            "two layer excludes subsurface/glint",
        ),
        (&[21], 0, "SLSF2.21 is not glint"),
        (&[30], 0, "Effect_Lighting alone"),
    ];
    for (bits, want, what) in rows {
        assert_eq!(pbr_flags(&pbr(bits), &none), *want, "{what}");
    }
}

#[test]
fn has_bits_follow_bound_registers_only() {
    let m = pbr(&[]);
    // Non-empty slot paths are not enough: a Has* bit needs the register bound to a real texture (01 §3.3).
    let mut with_paths = m.clone();
    for i in [2, 3, 6, 7] {
        with_paths.set_slot_raw(i, "textures\\x.dds");
    }
    assert_eq!(pbr_flags(&with_paths, &bound(&[])), 0);
    assert_eq!(pbr_flags(&m, &bound(&[6])), PBR_HAS_EMISSIVE);
    assert_eq!(pbr_flags(&m, &bound(&[4])), PBR_HAS_DISPLACEMENT);
    assert_eq!(pbr_flags(&m, &bound(&[12])), PBR_HAS_FEATURES0);
    assert_eq!(pbr_flags(&m, &bound(&[9])), PBR_HAS_FEATURES1);
    assert_eq!(pbr_flags(&m, &bound(&[0, 1, 2, 3, 5, 7, 8])), 0);
    // 01 §5.2 classes: emissive + parallax = 3; the coat cave with t4/t9/t12 resolved = 494.
    assert_eq!(pbr_flags(&m, &bound(&[0, 1, 4, 5, 6])), 3);
    let mut cave = pbr(&[24, 25, 27, 30]);
    cave.shader_type = 11;
    assert_eq!(
        pbr_flags(&cave, &bound(&[0, 1, 4, 5, 9, 12])),
        32 + 128 + 256 + 64 + 2 + 8 + 4
    );
    // Not PBR: always 0, whatever is bound.
    let mut v = MaterialState::builtin_default();
    v.slsf2 |= 1 << 24;
    assert_eq!(pbr_flags(&v, &bound(&[4, 6, 9, 12])), 0);
}

#[test]
fn glint_define_matches_the_glint_flag() {
    let rows: [&[u32]; 8] = [
        &[],
        &[12],
        &[12, 25],
        &[12, 24],
        &[12, 27],
        &[12, 26],
        &[21],
        &[21, 12],
    ];
    for bits in rows {
        let m = pbr(bits);
        let flag = pbr_flags(&m, &Bound::default()) & PBR_GLINT != 0;
        assert_eq!(remapped_glint(m.slsf2), flag, "{bits:?}");
        let d = defines(&perm_material(&m), Stage::Pixel, &[], false);
        assert_eq!(d.0.iter().any(|(k, _)| k == "GLINT"), flag, "{bits:?}");
        assert!(!d.0.iter().any(|(k, _)| k == "ANISO_LIGHTING"), "{bits:?}");
    }
}

/// A synthetic PerMaterial layout, one 16-byte register per field.
const FIELDS: &[(&str, u32)] = &[
    ("TexcoordOffset", 16),
    ("SpecularColor", 16),
    ("PBRFlags", 4),
    ("PBRParams1", 12),
    ("PBRParams2", 16),
    ("MultiLayerParallaxData", 16),
    ("EnvmapData", 16),
    ("EmitColor", 12),
    ("emissiveMult", 4),
];

fn writer() -> CbWriter {
    let vars = FIELDS
        .iter()
        .enumerate()
        .map(|(i, (n, s))| CbVar {
            name: n.to_string(),
            offset: 16 * i as u32,
            size: *s,
            kind: VarKind::Float,
        })
        .collect();
    CbWriter::new(&CbLayout {
        name: "PerMaterial".into(),
        slot: 1,
        size: 16 * FIELDS.len() as u32,
        vars,
    })
}

fn floats(w: &CbWriter, name: &str) -> Vec<f32> {
    let i = FIELDS.iter().position(|f| f.0 == name).unwrap();
    let at = 16 * i;
    w.bytes()[at..at + FIELDS[i].1 as usize]
        .chunks(4)
        .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
        .collect()
}

fn flags(w: &CbWriter) -> u32 {
    let at = 16 * FIELDS.iter().position(|f| f.0 == "PBRFlags").unwrap();
    u32::from_le_bytes(w.bytes()[at..at + 4].try_into().unwrap())
}

fn written(m: &MaterialState, regs: &[u32], linear_lighting: bool) -> CbWriter {
    let mut w = writer();
    write_material(&mut w, m, &bound(regs), linear_lighting, None);
    w
}

#[test]
fn pbr_params_and_mlpd_follow_the_cs_branches() {
    let mut plain = pbr(&[]);
    plain.params.specular_color = [0.9, 0.7, 0.8];
    plain.params.lighting_effect_1 = 0.85;
    plain.params.specular_strength = 0.5;
    plain.params.lighting_effect_2 = 0.2;
    plain.params.glossiness = 0.08;
    let w = written(&plain, &[], false);
    assert_eq!(floats(&w, "PBRParams1"), [0.5, 0.2, 0.08]);
    assert_eq!(
        floats(&w, "PBRParams2"),
        [0.0; 4],
        "not written outside TwoLayer/Subsurface"
    );
    assert_eq!(floats(&w, "MultiLayerParallaxData"), [0.0; 4]);
    assert_eq!(
        floats(&w, "SpecularColor"),
        [0.0; 4],
        "no SPECULAR under PBR"
    );
    assert_eq!(flags(&w), 0);

    let mut ss = plain.clone();
    ss.slsf2 |= 1 << 26;
    let w = written(&ss, &[12], false);
    assert_eq!(flags(&w), PBR_SUBSURFACE | PBR_HAS_FEATURES0);
    assert_eq!(
        floats(&w, "PBRParams2"),
        [0.9, 0.7, 0.8, 0.85],
        "colour raw, not linearised"
    );
    assert_eq!(floats(&w, "MultiLayerParallaxData"), [0.0; 4]);

    let mut coat = plain.clone();
    coat.slsf2 |= 1 << 24;
    mlp(&mut coat, [0.3, 0.018, 1.0, 1.0]);
    let w = written(&coat, &[], false);
    assert_eq!(floats(&w, "MultiLayerParallaxData"), [0.3, 0.018, 0.0, 0.0]);
    assert_eq!(floats(&w, "PBRParams2"), [0.9, 0.7, 0.8, 0.85]);

    let mut fuzz = plain.clone();
    fuzz.slsf2 |= 1 << 25;
    mlp(&mut fuzz, [0.13, 0.13, 0.13, 1.0]);
    let w = written(&fuzz, &[], false);
    assert_eq!(
        floats(&w, "MultiLayerParallaxData"),
        [0.13, 0.13, 0.13, 1.0]
    );
    assert_eq!(floats(&w, "PBRParams2"), [0.0; 4]);

    let mut glint = plain.clone();
    glint.slsf2 |= 1 << 12;
    mlp(&mut glint, [4.0, 15.0, 1.0, 5.0]);
    let w = written(&glint, &[], false);
    assert_eq!(
        floats(&w, "MultiLayerParallaxData"),
        [4.0, 25.0, 1.0, 5.0],
        "y = 40 − L"
    );

    // Leaked Fit_Slope on type 0: CS zeroes the MLP floats, so glint with zero parameters → (0, 40, 0, 0).
    let mut leak = plain.clone();
    leak.slsf2 |= 1 << 12;
    let w = written(&leak, &[], false);
    assert_eq!(flags(&w), PBR_GLINT);
    assert_eq!(floats(&w, "MultiLayerParallaxData"), [0.0, 40.0, 0.0, 0.0]);

    // PBR never writes P0's raw trailer: a PBR type-11 shape outside the MLP branches leaves MLPD and
    // EnvmapData at 0.
    let mut t11 = plain.clone();
    mlp(&mut t11, [1.0, 2.0, 3.0, 4.0]);
    let w = written(&t11, &[5], false);
    assert_eq!(floats(&w, "EnvmapData"), [0.0; 4]);
    assert_eq!(floats(&w, "MultiLayerParallaxData"), [0.0; 4]);
}

#[test]
fn vanilla_trailer_is_unchanged_from_p0() {
    let mut v = MaterialState::builtin_default();
    mlp(&mut v, [1.0, 2.0, 3.0, 4.0]);
    let w = written(&v, &[5], false);
    assert_eq!(floats(&w, "MultiLayerParallaxData"), [1.0, 2.0, 3.0, 4.0]);
    assert_eq!(
        floats(&w, "EnvmapData"),
        [0.5, 1.0, 0.0, 0.0],
        "envmap strength, t5 bound"
    );
    assert_eq!(floats(&w, "SpecularColor"), [1.0, 1.0, 1.0, 80.0]);
    assert_eq!(flags(&w), 0);
    assert_eq!(floats(&w, "PBRParams1"), [0.0; 3]);
}

#[test]
fn edit_rewrites_from_zero() {
    // 03 T-E2: a PBR → vanilla rewrite leaves no PBR bytes behind.
    let ss = pbr(&[26]);
    let mut w = writer();
    write_material(&mut w, &ss, &bound(&[12]), false, None);
    assert_ne!(floats(&w, "PBRParams2"), [0.0; 4]);
    assert_ne!(flags(&w), 0);
    w.zero();
    assert!(w.bytes().iter().all(|b| *b == 0));
    write_material(
        &mut w,
        &MaterialState::builtin_default(),
        &bound(&[]),
        false,
        None,
    );
    for n in ["PBRParams1", "PBRParams2", "MultiLayerParallaxData"] {
        assert!(floats(&w, n).iter().all(|v| *v == 0.0), "{n}");
    }
    assert_eq!(flags(&w), 0);
    assert_eq!(floats(&w, "SpecularColor"), [1.0, 1.0, 1.0, 80.0]);
}

#[test]
fn emissive_d1() {
    // PBR (spec D1, captures C1/frame6171): sRGB-to-linear(NIF colour) × multiple with LL on; raw × multiple without.
    let mut p = pbr(&[]);
    p.params.emissive_color = [0.5, 0.25, 1.0];
    p.params.emissive_multiple = 2.0;
    assert_eq!(
        floats(&written(&p, &[], false), "EmitColor"),
        [1.0, 0.5, 2.0]
    );
    let want = [0.5f32, 0.25, 1.0].map(|c| srgb_to_linear(c) * 2.0);
    assert_eq!(
        floats(&written(&p, &[], true), "EmitColor"),
        want,
        "linearised with LL"
    );
    assert_eq!(floats(&written(&p, &[], false), "emissiveMult"), [2.0]);
    // Vanilla keeps P0's rule: only Own_Emit (SLSF1.22) shapes get the NIF colour.
    let mut v = MaterialState::builtin_default();
    v.params = p.params.clone();
    assert_eq!(floats(&written(&v, &[], false), "EmitColor"), [0.0; 3]);
    v.slsf1 |= 1 << 22;
    assert_eq!(
        floats(&written(&v, &[], false), "EmitColor"),
        [1.0, 0.5, 2.0]
    );
}

#[test]
fn classify_table() {
    use nif::props::Alpha;
    use render3d::material::{classify, EditKind};
    let f = ["ENABLE_LL"];
    let base = MaterialState::builtin_default();
    let with = |edit: &dyn Fn(&mut MaterialState)| {
        let mut m = base.clone();
        edit(&mut m);
        m
    };
    assert_eq!(classify(&base, &base.clone(), &f, false), EditKind::Same);
    let uv = with(&|m| m.params.uv_scale = [2.0, 2.0]);
    assert_eq!(classify(&base, &uv, &f, false), EditKind::Cbuffer);
    let emit = with(&|m| m.params.emissive_color = [1.0, 0.0, 0.0]);
    assert_eq!(classify(&base, &emit, &f, false), EditKind::Cbuffer);
    let hidden = with(&|m| m.hidden = true);
    assert_eq!(classify(&base, &hidden, &f, false), EditKind::Cbuffer);
    // Adding the alpha property toggles DO_ALPHA_TEST; flipping blend on an existing one does not.
    let test_only = with(&|m| {
        m.alpha = Some(Alpha {
            flags: 0x200,
            threshold: 128,
        })
    });
    assert_eq!(
        classify(&base, &test_only, &f, false),
        EditKind::Permutation
    );
    let mut blended = test_only.clone();
    blended.alpha = Some(Alpha {
        flags: 0x201,
        threshold: 128,
    });
    assert_eq!(classify(&test_only, &blended, &f, false), EditKind::Cbuffer);
    // Deferred mode: opaque ↔ blended moves the shape between DEFERRED and forward (D3); a plain edit does not.
    assert_eq!(
        classify(&test_only, &blended, &f, true),
        EditKind::Permutation
    );
    assert_eq!(classify(&base, &uv, &f, true), EditKind::Cbuffer);
    // Specular strength 0 ↔ > 0 toggles SPECULAR on vanilla shapes.
    let no_spec = with(&|m| m.params.specular_strength = 0.0);
    assert_eq!(classify(&base, &no_spec, &f, false), EditKind::Permutation);
    // A new slot path rebinds; a raw-only change (same canonical path) does not.
    let tex = with(&|m| m.set_slot_raw(0, "textures\\a.dds"));
    assert_eq!(classify(&base, &tex, &f, false), EditKind::Textures);
    let mut same_norm = tex.clone();
    same_norm.set_slot_raw(0, "Textures/A.DDS");
    assert_eq!(classify(&tex, &same_norm, &f, false), EditKind::Cbuffer);
    // Glowmap (type 2) moves slot 2 from t12 to t6 without a define: a binding change.
    let mut glow_src = base.clone();
    glow_src.set_slot_raw(2, "textures\\g.dds");
    let mut glow = glow_src.clone();
    glow.shader_type = 2;
    assert_eq!(classify(&glow_src, &glow, &f, false), EditKind::Textures);
    // PBR on/off is TRUE_PBR; under PBR, roughness and the shader type (technique 0) are cbuffer edits,
    // Rim (→ Subsurface) too, but Fit_Slope toggles GLINT.
    let pbr = with(&|m| m.slsf2 |= 1 << 23);
    assert_eq!(classify(&base, &pbr, &f, false), EditKind::Permutation);
    let mut rough = pbr.clone();
    rough.params.specular_strength = 0.25;
    assert_eq!(classify(&pbr, &rough, &f, false), EditKind::Cbuffer);
    let mut t11 = pbr.clone();
    t11.shader_type = 11;
    assert_eq!(classify(&pbr, &t11, &f, false), EditKind::Cbuffer);
    let mut rim = pbr.clone();
    rim.slsf2 |= 1 << 26;
    assert_eq!(classify(&pbr, &rim, &f, false), EditKind::Cbuffer);
    let mut glint = pbr.clone();
    glint.slsf2 |= 1 << 12;
    assert_eq!(classify(&pbr, &glint, &f, false), EditKind::Permutation);
}

use nif::props::Alpha;
use render3d::material::{engine_blend, pipe, verified_blend, Pipe};

#[test]
fn engine_blend_follows_g3() {
    assert_eq!(engine_blend(6, 7), ((6, 7), false));
    assert_eq!(engine_blend(0, 0), ((6, 0), true));
    assert_eq!(engine_blend(1, 2), ((4, 1), true));
    assert_eq!(engine_blend(13, 7), ((0, 7), false));
    assert_eq!(engine_blend(3, 9), ((3, 9), false));
    assert!(!verified_blend(3, 9));
    assert!(verified_blend(6, 7) && verified_blend(0, 0) && verified_blend(1, 2));
}

#[test]
fn pipe_reads_alpha_and_shader_flags() {
    let mut m = MaterialState::builtin_default();
    m.alpha = Some(Alpha {
        flags: 0x00ED,
        threshold: 0,
    }); // blend on, 6/7
    m.slsf1 |= 1 << 31;
    m.slsf2 |= 1; // z test, z write
    assert_eq!(
        pipe(&m),
        Pipe {
            blend: Some((6, 7)),
            mapped: false,
            cull_back: true,
            z_test: true,
            z_write: true
        }
    );
    m.slsf2 |= 1 << 4; // Double_Sided
    assert!(!pipe(&m).cull_back);
    m.alpha = Some(Alpha {
        flags: 0x12EC,
        threshold: 128,
    }); // blend off
    assert_eq!(pipe(&m).blend, None);
}

#[test]
fn pbr_bit_meaning_agrees_with_pbr_flags() {
    use render3d::pbr_bit_meaning;
    const BITS: [u32; 6] = [12, 24, 25, 26, 27, 30];
    // Meaning -> the pbr_flags bit it must set (None: "No effect with PBR").
    let feature = |s: &str| match s {
        "Two-layer coat" => PBR_TWO_LAYER,
        "Inner-layer parallax" => PBR_INTERLAYER_PARALLAX,
        "Fuzz" => PBR_FUZZ,
        "Coat normal" => PBR_COAT_NORMAL,
        "Hair (Marschner)" => PBR_HAIR_MARSCHNER,
        "Coloured coat" => PBR_COLORED_COAT,
        "Subsurface" => PBR_SUBSURFACE,
        "Glint" => PBR_GLINT,
        "No effect with PBR" => 0,
        other => panic!("{other}"),
    };
    for mask in 0..64u32 {
        let on: Vec<u32> = (0..6)
            .filter(|i| mask >> i & 1 != 0)
            .map(|i| BITS[i])
            .collect();
        let m = pbr(&on);
        let flags = pbr_flags(&m, &Bound::default());
        let mut labelled = 0;
        for b in BITS {
            let want = pbr_bit_meaning(m.slsf2, b);
            if m.slsf2 >> b & 1 == 0 {
                continue;
            }
            let f = feature(want.unwrap());
            labelled |= f;
            assert!(f == 0 || flags & f != 0, "{on:?} bit {b}");
        }
        assert_eq!(flags, labelled, "{on:?}: a feature has no labelling bit");
    }
    assert_eq!(pbr_bit_meaning(1 << 24, 24), None, "not PBR");
    assert_eq!(pbr_bit_meaning(1 << 23, 5), None);
}

/// v2 D6c: the stand-in's VS-only TexcoordOffset (b1) is read from the vertex stage; the pixel-stage hook has no such
/// field.
#[test]
fn vs_cb_floats_reads_the_vertex_stage() {
    #[path = "common/mod.rs"]
    mod common;
    let mut s = common::plane_shape(0, common::IDENT, [0.0; 3], 1);
    s.material.params.uv_offset = [0.25, 0.5];
    s.material.params.uv_scale = [2.0, 4.0];
    let cache = tempfile::tempdir().unwrap();
    let r = common::planes(cache.path(), vec![s]);
    let id = render3d::ShapeId(0);
    assert_eq!(
        r.draw_vs_cb_floats(id, "TexcoordOffset"),
        Some(vec![0.25, 0.5, 2.0, 4.0])
    );
    assert_eq!(r.draw_cb_floats(id, "TexcoordOffset"), None);
    assert_eq!(
        r.draw_vs_cb_floats(render3d::ShapeId(9), "TexcoordOffset"),
        None
    );
}
