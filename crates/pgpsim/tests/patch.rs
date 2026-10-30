//! 01 §2.1-§2.3 on one entry through `simulate`.
mod common;
use common::*;
use nif::props::LightingTrailer;
use pgpsim::{Badge, SimResult};
use serde_json::{json, Value};

fn md() -> Value {
    json!({"match_diffuse": "rock01_d"})
}

fn with(extra: Value) -> Value {
    let mut e = md();
    for (k, v) in extra.as_object().expect("object") {
        e[k] = v.clone();
    }
    e
}

#[test]
fn plain_sdc_entry() {
    let (m, mp, badges) = patched(sim(
        &with(json!({"roughness_scale": 0.25})),
        &sdc_default(),
        &rock(),
    ));
    assert_eq!(mp, MP);
    assert!(badges.is_empty());
    assert_eq!(m.shader_type, 0);
    assert!(m.is_pbr());
    assert_eq!(m.params.glossiness, f(0.04));
    assert_eq!(m.params.specular_strength, f(0.25));
    assert_eq!(m.params.lighting_effect_2, f(1.0));
    let want: Vec<String> = [
        format!("{MP}.dds"),
        format!("{MP}_n.dds"),
        String::new(),
        String::new(),
        String::new(),
        format!("{MP}_rmaos.dds"),
        String::new(),
        String::new(),
        String::new(),
    ]
    .to_vec();
    assert_eq!(raws(&m), want);
    assert_eq!(m.slots[1].norm, "textures/pbr/landscape/rock01_d_n.dds");
}

#[test]
fn always_on_flag_clears() {
    let mut src = rock();
    src.slsf1 = with_bits(0, &[7, 10, 11, 17, 18, 29]);
    src.slsf2 = with_bits(0, &[6, 24, 25, 27]);
    let (m, _, _) = patched(sim(&md(), &sdc_default(), &src));
    assert_eq!(m.slsf1, 0);
    assert_eq!(m.slsf2, 1 << 23);
}

#[test]
fn leaks_without_the_keys_that_clear_them() {
    let mut src = rock();
    src.slsf1 = with_bits(0, &[29]);
    src.slsf2 = with_bits(0, &[5, 12, 26, 30]);
    // No default: no "emissive" (SLSF1.29 leaks), no "subsurface" (Rim leaks), no "vertex_colors" (SLSF2.5
    // leaks); Fit_Slope and Effect_Lighting are never cleared.
    let (m, _, _) = patched(sim(&md(), &Value::Null, &src));
    assert_eq!(m.slsf1, 1 << 29);
    assert_eq!(m.slsf2, with_bits(0, &[5, 12, 23, 26, 30]));
    // sdc's default writes emissive:false and subsurface:false: those two are cleared, Fit_Slope still leaks.
    let (m, _, _) = patched(sim(&md(), &sdc_default(), &src));
    assert_eq!(m.slsf1, 0);
    assert_eq!(m.slsf2, with_bits(0, &[5, 12, 23, 30]));
}

#[test]
fn absent_emissive_and_parallax_keep_the_source_slots() {
    let src = shape(&[
        D,
        N,
        "textures\\landscape\\rock01_g.dds",
        "textures\\landscape\\rock01_p.dds",
    ]);
    let (m, _, _) = patched(sim(&md(), &Value::Null, &src));
    assert_eq!(m.slots[2].raw, "textures\\landscape\\rock01_g.dds");
    assert_eq!(m.slots[3].raw, "textures\\landscape\\rock01_p.dds");
}

#[test]
fn emissive_true_sets_external_emittance_and_glow_slot() {
    let (m, _, _) = patched(sim(
        &with(json!({"emissive": true})),
        &sdc_default(),
        &rock(),
    ));
    assert!(bit(m.slsf1, 29));
    assert_eq!(m.slots[2].raw, format!("{MP}_g.dds"));
}

#[test]
fn vertex_colors_false_drops_attribute_and_flag() {
    let mut src = rock();
    src.has_vertex_colors = true;
    src.slsf2 = with_bits(0, &[5]);
    let (m, _, _) = patched(sim(
        &with(json!({"vertex_colors": false})),
        &sdc_default(),
        &src,
    ));
    assert!(!m.has_vertex_colors);
    assert!(!bit(m.slsf2, 5));
}

#[test]
fn zbuffer_write_false_clears_slsf2_0() {
    let mut src = rock();
    src.slsf2 = 1;
    let (m, _, _) = patched(sim(
        &with(json!({"zbuffer_write": false})),
        &sdc_default(),
        &src,
    ));
    assert!(!bit(m.slsf2, 0));
}

#[test]
fn multilayer_beats_glint_beats_fuzz() {
    let all = with(
        json!({"multilayer": true, "glint": {"screen_space_scale": 4.0}, "fuzz": {"color": [0.5, 0.5, 0.5]}}),
    );
    let (m, _, _) = patched(sim(&all, &sdc_default(), &rock()));
    assert_eq!(m.shader_type, 11);
    assert!(bit(m.slsf2, 24));
    assert!(!bit(m.slsf2, 12));
    assert_eq!(m.params.trailer, mlp(0.0, 1.0, 1.0, 1.0));

    let glint_fuzz =
        with(json!({"glint": {"screen_space_scale": 4.0}, "fuzz": {"color": [0.5, 0.5, 0.5]}}));
    let (m, _, _) = patched(sim(&glint_fuzz, &sdc_default(), &rock()));
    assert_eq!(m.shader_type, 11);
    assert!(bit(m.slsf2, 12));
    assert!(!bit(m.slsf2, 24));
    assert!(!bit(m.slsf2, 25));
    assert_eq!(m.params.trailer, mlp(4.0, 1.0, 1.0, 1.0));

    let fuzz = with(json!({"fuzz": {"color": [0.5, 0.5, 0.5]}}));
    let (m, _, _) = patched(sim(&fuzz, &sdc_default(), &rock()));
    assert_eq!(m.shader_type, 11);
    assert!(bit(m.slsf2, 25));
    assert!(!bit(m.slsf2, 12));
    assert_eq!(m.params.trailer, mlp(0.5, 0.5, 0.5, 1.0));
}

#[test]
fn type_11_keeps_its_floats_and_type_0_drops_the_trailer() {
    let mut src = rock();
    src.shader_type = 11;
    src.params.trailer = LightingTrailer::MultiLayer {
        inner_thickness: 0.3,
        refraction_scale: 0.4,
        inner_texture_scale: [0.5, 0.6],
        envmap_strength: 0.7,
    };
    let (m, _, _) = patched(sim(
        &with(json!({"glint": {"screen_space_scale": 4.0}})),
        &sdc_default(),
        &src,
    ));
    assert_eq!(
        m.params.trailer,
        LightingTrailer::MultiLayer {
            inner_thickness: 4.0,
            refraction_scale: 0.4,
            inner_texture_scale: [0.5, 0.6],
            envmap_strength: 0.7,
        }
    );
    let (m, _, _) = patched(sim(&md(), &sdc_default(), &src));
    assert_eq!(m.shader_type, 0);
    assert_eq!(m.params.trailer, LightingTrailer::None);

    let mut env = rock();
    env.shader_type = 1;
    env.params.trailer = LightingTrailer::EnvMap { scale: 0.5 };
    let (m, _, _) = patched(sim(&md(), &sdc_default(), &env));
    assert_eq!(m.shader_type, 0);
    assert_eq!(m.params.trailer, LightingTrailer::None);
}

#[test]
fn fuzz_defaults_and_values() {
    let (m, _, _) = patched(sim(&with(json!({"fuzz": {}})), &sdc_default(), &rock()));
    assert_eq!(m.params.trailer, mlp(0.0, 0.0, 0.0, 1.0));
    let e = with(json!({"fuzz": {"color": [0.13, 0.13, 0.13], "weight": 0.5, "texture": true}}));
    let (m, _, _) = patched(sim(&e, &sdc_default(), &rock()));
    assert_eq!(m.params.trailer, mlp(0.13, 0.13, 0.13, 0.5));
    assert_eq!(m.slots[6].raw, format!("{MP}_f.dds"));
}

#[test]
fn coat_keys_override_subsurface_fields_only_with_multilayer() {
    let e = with(json!({
        "multilayer": true, "subsurface_color": [0.1, 0.2, 0.3], "subsurface_opacity": 0.5,
        "coat_color": [0.7, 0.8, 0.9], "coat_strength": 0.25, "coat_roughness": 0.6,
        "coat_specular_level": 0.02, "inner_uv_scale": 2.0,
        "coat_diffuse": true, "coat_parallax": true, "coat_normal": true
    }));
    let (m, _, _) = patched(sim(&e, &sdc_default(), &rock()));
    assert_eq!(m.shader_type, 11);
    assert_eq!(m.params.specular_color, [f(0.7), f(0.8), f(0.9)]);
    assert_eq!(m.params.lighting_effect_1, f(0.25));
    assert_eq!(m.params.trailer, mlp(0.6, 0.02, 2.0, 2.0));
    for b in [23, 24, 25, 27, 30] {
        assert!(bit(m.slsf2, b), "SLSF2.{b}");
    }
    assert!(!bit(m.slsf2, 26));
    assert_eq!(m.slots[6].raw, format!("{MP}_cnr.dds"));
    assert_eq!(m.slots[7].raw, format!("{MP}_s.dds"));

    let mut no_ml = e.clone();
    no_ml.as_object_mut().expect("object").remove("multilayer");
    let (m, _, _) = patched(sim(&no_ml, &sdc_default(), &rock()));
    assert_eq!(m.shader_type, 0);
    assert_eq!(m.params.specular_color, [f(0.1), f(0.2), f(0.3)]);
    assert_eq!(m.params.lighting_effect_1, f(0.5));
    assert_eq!(m.params.trailer, LightingTrailer::None);
    for b in [24, 25, 27, 30] {
        assert!(!bit(m.slsf2, b), "SLSF2.{b}");
    }
    assert_eq!(m.slots[6].raw, format!("{MP}_cnr.dds"));
    assert_eq!(m.slots[7].raw, format!("{MP}_s.dds"));
}

#[test]
fn hair_sets_back_lighting_unless_multilayer_coat_normal_clears_it() {
    let (m, _, _) = patched(sim(&with(json!({"hair": true})), &sdc_default(), &rock()));
    assert_eq!(m.shader_type, 0);
    assert!(bit(m.slsf2, 27));
    let e = with(json!({"hair": true, "multilayer": true, "coat_normal": false}));
    let (m, _, _) = patched(sim(&e, &sdc_default(), &rock()));
    assert_eq!(m.shader_type, 11);
    assert!(!bit(m.slsf2, 27));
}

#[test]
fn emissive_color_needs_four_numbers_scale_and_uv_apply() {
    let src = rock();
    let e =
        with(json!({"emissive_color": [0.1, 0.2, 0.3], "emissive_scale": 2.0, "uv_scale": 3.0}));
    let (m, _, _) = patched(sim(&e, &sdc_default(), &src));
    assert_eq!(m.params.emissive_color, src.params.emissive_color);
    assert_eq!(m.params.emissive_multiple, f(2.0));
    assert_eq!(m.params.uv_scale, [f(3.0), f(3.0)]);
    let (m, _, _) = patched(sim(
        &with(json!({"emissive_color": [0.1, 0.2, 0.3, 1.0]})),
        &sdc_default(),
        &src,
    ));
    assert_eq!(m.params.emissive_color, [f(0.1), f(0.2), f(0.3)]);
}

#[test]
fn delete_hides_the_shape_and_skips_the_texture_check() {
    let src = rock();
    let e = with(json!({"delete": true, "roughness_scale": 0.1}));
    let (m, mp, badges) = patched(sim_with(&e, &sdc_default(), &src, &|_| false));
    let mut want = src.clone();
    want.hidden = true;
    assert_eq!(m, want);
    assert_eq!(mp, MP);
    assert!(badges.is_empty());
}

#[test]
fn missing_patched_texture_is_a_skip() {
    let rmaos = format!("{MP}_rmaos.dds");
    let r = sim_with(&md(), &sdc_default(), &rock(), &|p: &str| p != rmaos);
    assert_eq!(r, SimResult::Skip(format!("{rmaos} missing")));
}

#[test]
fn missing_untouched_source_slot_is_a_skip() {
    let x = "textures\\landscape\\rock01_x.dds";
    let src = shape(&[D, N, "", "", "", "", "", "", x]);
    let r = sim_with(&md(), &sdc_default(), &src, &|p: &str| p != x);
    assert_eq!(r, SimResult::Skip(format!("{x} missing")));
}

#[test]
fn badges_for_geometry_keys() {
    let mut src = rock();
    src.has_vertex_colors = true;
    let e = with(
        json!({"smooth_angle": 60, "auto_uv": 2.0, "vertex_color_lum_mult": 0.5, "vertex_color_sat_mult": 0.5}),
    );
    let (_, _, badges) = patched(sim(&e, &sdc_default(), &src));
    assert_eq!(
        badges,
        vec![
            Badge::NotPreviewed("smooth_angle"),
            Badge::NotPreviewed("auto_uv"),
            Badge::NotPreviewed("vertex_color_lum_mult"),
            Badge::NotPreviewed("vertex_color_sat_mult"),
        ]
    );
    // Without vertex colours PGPatcher skips the multipliers, so nothing is missing from the preview.
    let mut plain = rock();
    plain.has_vertex_colors = false;
    let (_, _, badges) = patched(sim(&e, &sdc_default(), &plain));
    assert_eq!(
        badges,
        vec![
            Badge::NotPreviewed("smooth_angle"),
            Badge::NotPreviewed("auto_uv")
        ]
    );
}

/// GE3: 2.0 removed `pbr`; PBR is always on and the key is dropped silently (R2) [Src 2.2.0 TP:623-627].
#[test]
fn pbr_false_is_ignored() {
    let plain = patched(sim(
        &with(json!({"roughness_scale": 0.5})),
        &sdc_default(),
        &rock(),
    ));
    for pbr in [json!(false), json!("false"), json!(0)] {
        let (m, mp, badges) = patched(sim(
            &with(json!({"pbr": pbr, "roughness_scale": 0.5})),
            &sdc_default(),
            &rock(),
        ));
        assert_eq!(mp, MP, "{pbr}");
        assert!(badges.is_empty(), "{pbr}: {badges:?}");
        assert!(m.is_pbr(), "{pbr}");
        assert_eq!(m, plain.0, "{pbr}");
    }
}

/// GE3: 2.0 removed `env_mapping` and `env_map_scale*`: no type 1, no badge (R2; measured T11).
#[test]
fn env_keys_are_ignored() {
    let plain = patched(sim(&md(), &sdc_default(), &rock()));
    for e in [
        json!({"pbr": false, "env_mapping": true, "env_map_scale": 0.5, "env_map_scale_mult": 3.0}),
        json!({"env_mapping": true}),
        json!({"env_mapping": "yes", "env_map_scale": "x"}),
    ] {
        let (m, mp, badges) = patched(sim(&with(e.clone()), &sdc_default(), &rock()));
        assert_eq!(mp, MP, "{e}");
        assert!(badges.is_empty(), "{e}: {badges:?}");
        assert_eq!(m.shader_type, 0, "{e}");
        assert!(!bit(m.slsf1, 7), "{e}");
        assert!(m.is_pbr(), "{e}");
        assert_eq!(m, plain.0, "{e}");
    }
}

#[test]
fn no_match_and_nif_filter() {
    assert_eq!(
        sim(&json!({"match_diffuse": "other"}), &sdc_default(), &rock()),
        SimResult::NoMatch
    );
    assert_eq!(
        sim(
            &with(json!({"nif_filter": "clutter"})),
            &sdc_default(),
            &rock()
        ),
        SimResult::NoMatch
    );
}

/// GE1: 2.x patches an SLSF1.21 shape and clears the bit [Src 2.2.0 TP:1080; measured T7].
#[test]
fn facegen_rgb_tint_is_patched_and_cleared() {
    let mut src = rock();
    src.slsf1 |= 1 << 21;
    let (m, mp, _) = patched(sim(&md(), &sdc_default(), &src));
    assert_eq!(mp, MP);
    assert!(m.is_pbr());
    assert!(!bit(m.slsf1, 21));
}

#[test]
fn wrong_json_type_is_a_skip() {
    let r = sim(&with(json!({"emissive": "yes"})), &sdc_default(), &rock());
    assert!(
        matches!(&r, SimResult::Skip(s) if s.contains("\"emissive\"")),
        "{r:?}"
    );
    let r = sim(
        &with(json!({"fuzz": {"color": [1, 1]}})),
        &sdc_default(),
        &rock(),
    );
    assert!(
        matches!(&r, SimResult::Skip(s) if s.contains("fuzz.color")),
        "{r:?}"
    );
}

// ---- PGPatcher 2.2 alphaprop_* (GUI suggestions item 5) ----

fn alpha_after(entry: Value, source: &nif::material::MaterialState) -> Option<nif::props::Alpha> {
    patched(sim(&with(entry), &sdc_default(), source)).0.alpha
}

fn with_alpha(flags: u16, threshold: u8) -> nif::material::MaterialState {
    let mut m = rock();
    m.alpha = Some(nif::props::Alpha { flags, threshold });
    m
}

#[test]
fn alphaprop_values_in_range_apply_to_an_existing_property() {
    let a = alpha_after(
        json!({"alphaprop_flags": 4845, "alphaprop_thresh": 200}),
        &with_alpha(237, 10),
    );
    assert_eq!(
        a,
        Some(nif::props::Alpha {
            flags: 4845,
            threshold: 200
        })
    );
}

#[test]
fn bad_alphaprop_values_are_ignored_and_never_skip() {
    for (flags, thresh) in [
        (json!(70000), json!(300)),
        (json!(-1), json!(-1)),
        (json!(1.5), json!(2.5)),
        (json!("8"), json!("8")),
        (json!(true), json!(true)),
    ] {
        let r = sim(
            &with(json!({"alphaprop_flags": flags, "alphaprop_thresh": thresh})),
            &sdc_default(),
            &with_alpha(237, 10),
        );
        assert!(
            matches!(r, SimResult::Patched { .. }),
            "{flags} / {thresh}: {r:?}"
        );
        assert_eq!(
            patched(r).0.alpha,
            Some(nif::props::Alpha {
                flags: 237,
                threshold: 10
            }),
            "{flags} / {thresh}"
        );
    }
}

#[test]
fn delete_beats_create() {
    let a = alpha_after(
        json!({"alphaprop_delete": true, "alphaprop_create": true, "alphaprop_thresh": 5}),
        &with_alpha(237, 10),
    );
    assert_eq!(a, None);
}

#[test]
fn create_on_a_shape_without_alpha_uses_niflys_defaults_then_the_given_values() {
    assert_eq!(
        alpha_after(json!({"alphaprop_create": true}), &rock()),
        Some(nif::props::Alpha {
            flags: 4844,
            threshold: 128
        })
    );
    assert_eq!(
        alpha_after(
            json!({"alphaprop_create": true, "alphaprop_thresh": 64}),
            &rock()
        ),
        Some(nif::props::Alpha {
            flags: 4844,
            threshold: 64
        })
    );
}

#[test]
fn without_create_a_shape_without_alpha_stays_without() {
    assert_eq!(
        alpha_after(
            json!({"alphaprop_flags": 4845, "alphaprop_thresh": 64}),
            &rock()
        ),
        None
    );
}
