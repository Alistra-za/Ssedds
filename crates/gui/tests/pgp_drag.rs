//! Task 14b (DECISIONS AG121-AG125): per-key drag speed and display precision for the PGPatcher numbers.
use sdc_gui::pgp::schema::{spec, KeyKind, KEYS};
use sdc_gui::pgp::settings::GLINT_DEFAULTS;

fn drag(key: &str) -> sdc_gui::pgp::schema::DragSpec {
    spec(key).unwrap().drag
}

/// The whole table: (key, speed, decimals, log). Decimals are at least as fine as the observed data
/// (pgp-numeric-stats.csv): 0.02776 and 0.3576 are real values, so a drag never rounds them away.
const TABLE: [(&str, f64, usize, bool); 21] = [
    ("smooth_angle", 0.5, 1, false),
    ("auto_uv", 0.002, 3, false),
    ("uv_scale", 0.002, 3, false),
    ("vertex_color_lum_mult", 0.005, 3, false),
    ("vertex_color_sat_mult", 0.005, 2, false),
    ("alphaprop_flags", 16.0, 0, false),
    ("alphaprop_thresh", 1.0, 0, false),
    ("emissive_scale", 0.02, 2, true),
    ("displacement_scale", 0.01, 2, true),
    ("specular_level", 0.001, 5, true),
    ("roughness_scale", 0.01, 2, false),
    ("subsurface_opacity", 0.005, 2, false),
    ("coat_specular_level", 0.001, 4, true),
    ("coat_roughness", 0.005, 2, false),
    ("coat_strength", 0.005, 2, false),
    ("inner_uv_scale", 0.002, 3, false),
    ("glint.screen_space_scale", 0.02, 2, false),
    ("glint.log_microfacet_density", 0.1, 3, false),
    ("glint.microfacet_roughness", 0.005, 3, true),
    ("glint.density_randomization", 1.0, 3, true),
    ("fuzz.weight", 0.005, 2, false),
];

#[test]
fn every_numeric_key_has_its_drag_spec() {
    let numeric: Vec<&str> = KEYS
        .iter()
        .filter(|k| matches!(k.kind, KeyKind::Number(_)))
        .map(|k| k.key)
        .collect();
    assert_eq!(numeric.len(), TABLE.len());
    for (key, speed, decimals, log) in TABLE {
        assert!(numeric.contains(&key), "{key} is not a number key");
        let d = drag(key);
        assert_eq!(
            (d.speed, d.decimals, d.log),
            (speed, decimals, log),
            "{key}"
        );
    }
}

#[test]
fn the_most_mis_tuned_keys_get_the_report_values() {
    assert_eq!(drag("alphaprop_flags").speed, 16.0);
    assert_eq!(drag("alphaprop_thresh").speed, 1.0);
    assert_eq!(drag("smooth_angle").decimals, 1);
}

#[test]
fn a_log_key_speeds_up_with_its_value_and_a_linear_one_does_not() {
    let d = drag("displacement_scale");
    assert_eq!(d.speed_at(0.0), 0.01);
    assert_eq!(d.speed_at(1.0), 0.01);
    assert!(
        (d.speed_at(40.0) - 0.4).abs() < 1e-12,
        "{}",
        d.speed_at(40.0)
    );
    assert_eq!(d.speed_at(-40.0), d.speed_at(40.0));
    assert_eq!(drag("smooth_angle").speed_at(75.0), 0.5);
}

#[test]
fn decimals_are_as_displayed() {
    assert_eq!(drag("specular_level").format(0.04), "0.04");
    assert_eq!(drag("specular_level").format(0.123_456), "0.12346");
    assert_eq!(drag("displacement_scale").format(1.0), "1.0");
    assert_eq!(drag("smooth_angle").format(75.0), "75.0");
    assert_eq!(drag("smooth_angle").format(72.26), "72.3");
    assert_eq!(drag("glint.density_randomization").format(300.0), "300.0");
    assert_eq!(drag("glint.density_randomization").format(4.525), "4.525");
    assert_eq!(drag("specular_level").format(0.02776), "0.02776");
    assert_eq!(drag("coat_specular_level").format(0.3576), "0.3576");
    assert_eq!(
        drag("glint.log_microfacet_density").format(12.935),
        "12.935"
    );
}

/// Note FC13 (AG121): a stored value the box rounds keeps its full value on hover; an exact one needs none.
#[test]
fn a_rounded_value_shows_its_stored_value_on_hover() {
    let d = drag("glint.density_randomization");
    assert_eq!(d.format(4.525_123), "4.525");
    assert_eq!(
        d.stored_hover(4.525_123).as_deref(),
        Some("Stored value: 4.525123")
    );
    assert_eq!(d.stored_hover(4.525), None);
    assert_eq!(d.stored_hover(300.0), None);
    // A whole-number display rounds too.
    assert_eq!(drag("alphaprop_thresh").format(4.525), "5");
    assert!(drag("alphaprop_thresh").stored_hover(4.525).is_some());
}

/// DECISIONS AG123: no GUI default sits outside the observed p1-p99 band (pgp-numeric-stats.csv).
#[test]
fn glint_defaults_sit_inside_the_observed_band() {
    let band = |k: &str| match k {
        "glint.screen_space_scale" => (1.0, 4.0),
        "glint.log_microfacet_density" => (6.0, 24.0),
        "glint.microfacet_roughness" => (0.063, 1.43),
        "glint.density_randomization" => (2.128, 300.0),
        _ => panic!("{k}"),
    };
    for (k, x) in GLINT_DEFAULTS {
        let (lo, hi) = band(k);
        assert!((lo..=hi).contains(&x), "{k} default {x} outside {lo}..{hi}");
    }
    assert_eq!(
        GLINT_DEFAULTS
            .iter()
            .find(|(k, _)| *k == "glint.log_microfacet_density")
            .unwrap()
            .1,
        10.0
    );
}

#[test]
fn the_evidence_revision_moves_only_when_the_evidence_changes() {
    let mut m = sdc_gui::model::AppModel::new(
        sdc_gui::settings::GuiSettings::default(),
        None,
        std::sync::Arc::new(engine::session::Session::new(encoders_sys::Bc1Model::Ideal).unwrap()),
    );
    let t: std::collections::BTreeMap<String, bool> = [("a".to_string(), true)].into();
    let r0 = m.evidence_rev;
    m.set_tree_anim(t.clone());
    assert_eq!(m.evidence_rev, r0 + 1);
    m.set_tree_anim(t);
    assert_eq!(m.evidence_rev, r0 + 1, "the same value is no change");
    m.set_known(Default::default());
    assert_eq!(m.evidence_rev, r0 + 1, "nor is an equal known set");
}
