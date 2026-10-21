//! DECISIONS Note CP (owner, 2026-10-21): the "BC7 weighting" dropdown replaces the Perceptual check box. One row,
//! two fields: `encoder.perceptual` and `encoder.bc7_weights`.
mod support;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use engine::config::{Backend, Bc7Weights, Format};
use sdc_gui::model::bulk::BulkOp;
use sdc_gui::model::fields::{field_view, hidden_reason, weighting_option_lock, WEIGHTING};
use sdc_gui::model::AppModel;
use serde_json::{json, Value};
use std::time::Duration;
use support::*;

const ROCK: &str = "landscape/rock01.png";
const NORMAL: &str = "landscape/rock01_normal.png";

fn ready(dir: &std::path::Path) -> AppModel {
    let mut m = opened(dir);
    decide_all(&mut m);
    m
}

fn idle(m: &mut AppModel) {
    assert!(m.wait_idle(Duration::from_secs(60)));
}

/// The texture's own `encoder.perceptual` and `encoder.bc7_weights`.
fn own(m: &AppModel, key: &str) -> (Option<Value>, Option<Value>) {
    let p = m.project.as_ref().unwrap();
    (
        p.file_value(key, "encoder.perceptual"),
        p.file_value(key, WEIGHTING),
    )
}

fn row_value(m: &AppModel, key: &str) -> Value {
    let job = m.item(key).unwrap().job.clone().unwrap();
    field_view(m.target, &job, WEIGHTING).unwrap().value
}

fn harness(m: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 1000.0))
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

/// Picking each option in the settings panel writes the two fields as the owner asked: Linear clears both,
/// Perceptual sets perceptual and clears the weights, a weighting clears perceptual and sets the weights.
#[test]
fn picking_each_option_sets_the_two_fields() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
    let p = m.item(ROCK).unwrap().job.clone().unwrap().profile;
    assert_eq!((p.format, p.encoder.backend), (Format::Bc7, Backend::Bc7e));
    m.select_item(ROCK);
    let mut h = harness(m);
    h.run();
    h.get_by_label("BC7 weighting");
    let mut shown = "Linear";
    for (label, value, want) in [
        ("Alpha ×8", "alpha8", (None, Some(json!("alpha8")))),
        ("Perceptual", "perceptual", (Some(json!(true)), None)),
        ("Alpha ×2", "alpha2", (None, Some(json!("alpha2")))),
        ("Alpha ×16", "alpha16", (None, Some(json!("alpha16")))),
        ("RMAOS (R4 G1 B2 A1)", "rmaos", (None, Some(json!("rmaos")))),
        ("Linear", "linear", (None, None)),
    ] {
        // A combo's accessible text is its value.
        h.query_all_by_role(eframe::egui::accesskit::Role::ComboBox)
            .find(|n| n.accesskit_node().value().as_deref() == Some(shown))
            .unwrap_or_else(|| panic!("a combo showing {shown}"))
            .click();
        h.run();
        h.get_by_label(label).click();
        h.run();
        idle(h.state_mut());
        h.run();
        let m = h.state();
        assert_eq!(own(m, ROCK), want, "{label}");
        assert_eq!(row_value(m, ROCK), json!(value), "{label}");
        let e = &m.item(ROCK).unwrap().job.as_ref().unwrap().profile.encoder;
        assert_eq!(e.perceptual, value == "perceptual", "{label}");
        assert_eq!(
            e.bc7_weights,
            serde_json::from_value::<Bc7Weights>(json!(value)).ok(),
            "{label}"
        );
        shown = label;
    }
}

/// The row's reset clears both fields; a texture's own `perceptual = true` marks the row edited.
#[test]
fn the_reset_clears_both_fields() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
    m.set_field(ROCK, WEIGHTING, json!("perceptual"));
    idle(&mut m);
    let job = m.item(ROCK).unwrap().job.clone().unwrap();
    let v = field_view(m.target, &job, WEIGHTING).unwrap();
    assert!(v.edited && v.provenance == "project_file", "{v:?}");
    // A weighting set in sdc.toml under a perceptual set here: perceptual wins, and the reset clears both.
    m.project
        .as_mut()
        .unwrap()
        .set_file_value(ROCK, WEIGHTING, &json!("alpha8"));
    m.clear_field(ROCK, WEIGHTING);
    idle(&mut m);
    assert_eq!(own(&m, ROCK), (None, None));
    assert_eq!(row_value(&m, ROCK), json!("linear"));
}

/// Perceptual keeps the old row's lock (colour without data alpha); the weightings stay open on a normal map. The row
/// shows only for BC7 with bc7e.
#[test]
fn perceptual_keeps_its_lock_and_the_row_is_bc7e_only() {
    let d = tempfile::tempdir().unwrap();
    let m = ready(d.path());
    let n = m.item(NORMAL).unwrap().job.clone().unwrap().profile;
    assert_eq!(n.format, Format::Bc7);
    assert!(weighting_option_lock(m.target, &n, "perceptual").is_some());
    assert_eq!(weighting_option_lock(m.target, &n, "alpha16"), None);
    assert_eq!(hidden_reason(&n, WEIGHTING), None);
    let mut p = n.clone();
    p.encoder.backend = Backend::Nvtt;
    assert_eq!(
        hidden_reason(&p, WEIGHTING),
        Some("used only by the built-in BC7 encoder")
    );
    p.format = Format::Bc3;
    assert_eq!(hidden_reason(&p, WEIGHTING), Some("BC7 only"));
}

/// Bulk: Perceptual skips the normal map with the lock's reason; Alpha ×16 reaches both. Copy settings carries a
/// perceptual pick, which lives in the other field.
#[test]
fn bulk_and_copy_handle_both_fields() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
    let keys = vec![ROCK.to_string(), NORMAL.to_string()];
    m.apply_bulk_edit(&keys, WEIGHTING, BulkOp::Set(json!("perceptual")));
    idle(&mut m);
    assert_eq!(own(&m, ROCK), (Some(json!(true)), None));
    assert_eq!(own(&m, NORMAL), (None, None));
    assert!(
        m.bulk
            .report
            .as_deref()
            .is_some_and(|r| r.contains("1 of 2")),
        "{:?}",
        m.bulk.report
    );
    m.apply_bulk_edit(&keys, WEIGHTING, BulkOp::Set(json!("alpha16")));
    idle(&mut m);
    for k in [ROCK, NORMAL] {
        assert_eq!(own(&m, k), (None, Some(json!("alpha16"))), "{k}");
    }
    m.set_field(ROCK, WEIGHTING, json!("perceptual"));
    idle(&mut m);
    let leaf = "landscape/leaf.png".to_string();
    m.apply_copy_settings(ROCK, std::slice::from_ref(&leaf), &[WEIGHTING.into()]);
    idle(&mut m);
    assert_eq!(own(&m, &leaf), (Some(json!(true)), None));
}
