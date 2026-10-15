//! Task 14b (DECISIONS AG121-AG125): per-key drag speed and display precision for the PGPatcher numbers.
use sdc_gui::pgp::schema::{spec, KeyKind, KEYS};
use sdc_gui::pgp::settings::GLINT_DEFAULTS;

fn drag(key: &str) -> sdc_gui::pgp::schema::DragSpec {
    spec(key).unwrap().drag
}

#[test]
fn every_numeric_key_has_a_drag_spec() {
    for k in KEYS {
        if let KeyKind::Number(_) = k.kind {
            let d = k.drag;
            assert!(d.speed > 0.0, "{} has no drag speed", k.key);
            assert!(d.decimals <= 4, "{}", k.key);
        }
    }
    assert_eq!(
        KEYS.iter()
            .filter(|k| matches!(k.kind, KeyKind::Number(_)))
            .count(),
        21
    );
}

#[test]
fn the_most_mis_tuned_keys_get_the_report_values() {
    let d = drag("glint.density_randomization");
    assert_eq!((d.speed, d.decimals, d.log), (1.0, 0, true));
    let d = drag("smooth_angle");
    assert_eq!((d.speed, d.decimals, d.log), (0.5, 1, false));
    let d = drag("specular_level");
    assert_eq!((d.speed, d.decimals, d.log), (0.001, 3, true));
    let d = drag("glint.log_microfacet_density");
    assert_eq!((d.speed, d.decimals, d.log), (0.1, 1, false));
    assert_eq!(drag("alphaprop_flags").speed, 16.0);
    assert_eq!(drag("alphaprop_thresh").speed, 1.0);
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
    assert_eq!(drag("specular_level").format(0.123_456), "0.123");
    assert_eq!(drag("displacement_scale").format(1.0), "1.0");
    assert_eq!(drag("smooth_angle").format(75.0), "75.0");
    assert_eq!(drag("smooth_angle").format(72.26), "72.3");
    assert_eq!(drag("glint.density_randomization").format(300.0), "300");
    assert_eq!(drag("glint.density_randomization").format(4.525), "5");
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
