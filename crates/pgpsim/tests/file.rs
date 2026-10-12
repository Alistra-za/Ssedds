//! 01 §1.4-§1.5 within one file: every matching entry applies in index order; the last one wins.
mod common;
use common::*;
use nif::material::MaterialState;
use pgpsim::{simulate, simulate_file, Badge, SimInput, SimResult};
use serde_json::{json, Value};

fn file(entries: &[Value], src: &MaterialState, exists: &dyn Fn(&str) -> bool) -> SimResult {
    simulate_file(entries, &sdc_default(), NIF, src, exists)
}

#[test]
fn later_entry_wins_and_resets_defaults() {
    let entries = [
        json!({"match_diffuse": "rock01_d", "emissive": true, "roughness_scale": 0.5}),
        json!({"path_contains": "landscape", "roughness_scale": 0.75}),
    ];
    let (m, mp, _) = patched(file(&entries, &rock(), &|_| true));
    assert_eq!(mp, MP);
    assert_eq!(m.params.specular_strength, f(0.75));
    // The second entry carries sdc's emissive:false from `default`.
    assert_eq!(m.slots[2].raw, "");
    assert!(!bit(m.slsf1, 29));
}

#[test]
fn non_matching_entries_are_ignored() {
    let entries = [
        json!({"match_diffuse": "other", "roughness_scale": 0.1}),
        json!({"match_diffuse": "rock01_d", "nif_filter": "clutter", "roughness_scale": 0.2}),
        json!({"match_diffuse": "rock01_d", "roughness_scale": 0.5}),
    ];
    let (m, _, _) = patched(file(&entries, &rock(), &|_| true));
    assert_eq!(m.params.specular_strength, f(0.5));
    assert_eq!(file(&entries[..2], &rock(), &|_| true), SimResult::NoMatch);
}

#[test]
fn a_lock_keeps_the_earlier_entrys_slot() {
    let entries = [
        json!({"match_diffuse": "rock01_d"}),
        json!({"match_diffuse": "rock01_d", "rename": "snow01", "lock_diffuse": true}),
    ];
    let (m, mp, _) = patched(file(&entries, &rock(), &|_| true));
    assert_eq!(mp, "textures\\pbr\\landscape\\snow01");
    assert_eq!(m.slots[0].raw, format!("{MP}.dds"));
    assert_eq!(m.slots[1].raw, "textures\\pbr\\landscape\\snow01_n.dds");
}

#[test]
fn the_texture_check_sees_the_final_slots() {
    let on = json!({"match_diffuse": "rock01_d", "parallax": true});
    let off = json!({"match_diffuse": "rock01_d", "parallax": false});
    let no_height = |p: &str| !p.ends_with("_p.dds");
    let (m, _, _) = patched(file(&[on.clone(), off.clone()], &rock(), &no_height));
    assert_eq!(m.slots[3].raw, "");
    assert_eq!(
        file(&[off, on], &rock(), &no_height),
        SimResult::Skip(format!("{MP}_p.dds missing"))
    );
}

#[test]
fn delete_in_a_later_entry_hides_after_the_earlier_ones_applied() {
    let entries = [
        json!({"match_diffuse": "rock01_d", "roughness_scale": 0.5}),
        json!({"match_diffuse": "rock01_d", "delete": true}),
    ];
    let (m, _, _) = patched(file(&entries, &rock(), &|_| false));
    assert!(m.hidden);
    assert_eq!(m.params.specular_strength, f(0.5));
}

#[test]
fn mlp_floats_persist_across_entries_like_nifly_memory() {
    let entries = [
        json!({"match_diffuse": "rock01_d", "glint": {"screen_space_scale": 4.0}}),
        json!({"match_diffuse": "rock01_d"}),
        json!({"match_diffuse": "rock01_d", "glint": {"log_microfacet_density": 20.0}}),
    ];
    let (m, _, _) = patched(file(&entries, &rock(), &|_| true));
    assert_eq!(m.shader_type, 11);
    assert_eq!(m.params.trailer, mlp(4.0, 20.0, 1.0, 1.0));
}

#[test]
fn badges_are_listed_once() {
    let entries = [
        json!({"match_diffuse": "rock01_d", "smooth_angle": 60}),
        json!({"match_diffuse": "rock01_d", "smooth_angle": 75}),
    ];
    let (_, _, badges) = patched(file(&entries, &rock(), &|_| true));
    assert_eq!(badges, vec![Badge::NotPreviewed("smooth_angle")]);
}

#[test]
fn one_entry_file_equals_simulate() {
    let e = json!({"match_diffuse": "rock01_d", "multilayer": true, "coat_strength": 0.5});
    let src = rock();
    let d = sdc_default();
    let one = simulate(&SimInput {
        entry: &e,
        default: &d,
        nif_path: NIF,
        source: &src,
        exists: &|_| true,
    });
    assert_eq!(
        one,
        simulate_file(std::slice::from_ref(&e), &d, NIF, &src, &|_| true)
    );
}
