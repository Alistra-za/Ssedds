//! DECISIONS Note CP (owner, 2026-10-21): the "BC7 weighting" dropdown replaces the Perceptual check box. One row,
//! two fields: `encoder.perceptual` and `encoder.bc7_weights`.
mod support;
use eframe::egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use engine::codes::Code;
use engine::config::{Backend, Bc7Weights, ColorTag, Format};
use sdc_gui::model::bulk::BulkOp;
use sdc_gui::model::fields::{
    field_view, hidden_reason, weighting_option_lock, WEIGHTING, WEIGHTING_OPTIONS,
};
use sdc_gui::model::AppModel;
use sdc_gui::settings::GuiSettings;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;
use support::*;

const ROCK: &str = "landscape/rock01.png";
const NORMAL: &str = "landscape/rock01_normal.png";

fn ready(dir: &Path) -> AppModel {
    let mut m = opened(dir);
    decide_all(&mut m);
    m
}

/// `ready` with `sdc_toml` written into the project folder first (an inherited layer).
fn ready_with(dir: &Path, sdc_toml: &str) -> AppModel {
    let root = mod_folder(dir);
    std::fs::write(root.join("sdc.toml"), sdc_toml).unwrap();
    let settings = GuiSettings {
        export_folder: Some(dir.join("export")),
        ..GuiSettings::default()
    };
    let mut m = AppModel::new(settings, None, session());
    m.accept = engine::policy::AcceptSuggestions::None;
    m.open_folder(root);
    idle(&mut m);
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

/// What the encoder gets: perceptual, and the weights it applies (`linear` is the plain call, as unset is).
fn encode_params(m: &AppModel, key: &str) -> (bool, Option<Bc7Weights>) {
    let job = m.item(key).unwrap().job.as_ref().unwrap();
    let e = &job.profile.encoder;
    (
        e.perceptual,
        e.bc7_weights.filter(|w| *w != Bc7Weights::Linear),
    )
}

fn ignored(m: &AppModel, key: &str) -> bool {
    m.item(key)
        .unwrap()
        .job
        .as_ref()
        .unwrap()
        .warnings
        .iter()
        .any(|w| w.code == Code::EncWeightsIgnored)
}

fn harness(m: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 1000.0))
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

/// Opens the settings panel's BC7 weighting combo (named "BC7 weighting", M4) and picks `label`.
fn pick(h: &mut Harness<'static, AppModel>, label: &str) {
    let combos: Vec<_> = h
        .query_all_by_role(Role::ComboBox)
        .filter(|n| n.accesskit_node().label().as_deref() == Some("BC7 weighting"))
        .collect();
    assert_eq!(combos.len(), 1, "one named BC7 weighting combo");
    combos[0].click();
    h.run();
    h.get_by_label(label).click();
    h.run();
    idle(h.state_mut());
    h.run();
}

/// The GUI's options are every `Bc7Weights` value (`ALL`, tied to the enum) plus Perceptual, so a new variant is
/// offered and picked by the kittest below.
#[test]
fn the_options_cover_every_weighting() {
    let mut values: Vec<&str> = WEIGHTING_OPTIONS.iter().map(|o| o.0).collect();
    values.retain(|v| *v != "perceptual");
    let all: Vec<Value> = Bc7Weights::ALL.iter().map(|w| json!(w)).collect();
    assert_eq!(values.iter().map(|v| json!(v)).collect::<Vec<_>>(), all);
}

/// Picking each option in the settings panel, with nothing inherited: Linear clears both fields, Perceptual sets
/// perceptual and clears the weights, a weighting clears perceptual and sets the weights.
#[test]
fn picking_each_option_sets_the_two_fields() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
    let p = m.item(ROCK).unwrap().job.clone().unwrap().profile;
    assert_eq!((p.format, p.encoder.backend), (Format::Bc7, Backend::Bc7e));
    m.select_item(ROCK);
    let mut h = harness(m);
    h.run();
    let combo = h.get_by_role_and_label(Role::ComboBox, "BC7 weighting");
    assert_eq!(combo.accesskit_node().value().as_deref(), Some("Linear"));
    // Every option once, Linear last, so each pick changes the value.
    let order = WEIGHTING_OPTIONS
        .iter()
        .filter(|o| o.0 != "linear")
        .chain(WEIGHTING_OPTIONS.iter().filter(|o| o.0 == "linear"));
    for &(value, label, _) in order {
        pick(&mut h, label);
        let want = match value {
            "perceptual" => (Some(json!(true)), None),
            "linear" => (None, None),
            w => (None, Some(json!(w))),
        };
        let m = h.state();
        assert_eq!(own(m, ROCK), want, "{label}");
        assert_eq!(row_value(m, ROCK), json!(value), "{label}");
        let weights = serde_json::from_value::<Bc7Weights>(json!(value))
            .ok()
            .filter(|w| *w != Bc7Weights::Linear);
        assert_eq!(
            encode_params(m, ROCK),
            (value == "perceptual", weights),
            "{label}"
        );
        assert!(!ignored(m, ROCK), "{label}");
    }
}

/// Fix round 1, I1: with an inherited `perceptual = true`, Linear and Alpha x8 override it per file.
#[test]
fn an_inherited_perceptual_is_overridden_per_file() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ready_with(d.path(), "[defaults]\nencoder.perceptual = true\n");
    assert_eq!(encode_params(&m, ROCK), (true, None));
    m.select_item(ROCK);
    let mut h = harness(m);
    h.run();
    pick(&mut h, "Linear");
    let m = h.state();
    assert_eq!(own(m, ROCK), (Some(json!(false)), Some(json!("linear"))));
    assert_eq!(encode_params(m, ROCK), (false, None));
    assert_eq!(row_value(m, ROCK), json!("linear"));
    assert!(!ignored(m, ROCK));
    pick(&mut h, "Alpha ×8");
    let m = h.state();
    assert_eq!(own(m, ROCK), (Some(json!(false)), Some(json!("alpha8"))));
    assert_eq!(encode_params(m, ROCK), (false, Some(Bc7Weights::Alpha8)));
    assert!(!ignored(m, ROCK));
}

/// Fix round 1, I1: with an inherited `bc7_weights = "alpha8"`, Linear writes `linear`, and Alpha x8 then needs no
/// perceptual write of its own.
#[test]
fn an_inherited_weighting_is_overridden_per_file() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ready_with(d.path(), "[defaults]\nencoder.bc7_weights = \"alpha8\"\n");
    assert_eq!(encode_params(&m, ROCK), (false, Some(Bc7Weights::Alpha8)));
    m.select_item(ROCK);
    let mut h = harness(m);
    h.run();
    pick(&mut h, "Linear");
    let m = h.state();
    assert_eq!(own(m, ROCK), (Some(json!(false)), Some(json!("linear"))));
    assert_eq!(encode_params(m, ROCK), (false, None));
    assert_eq!(row_value(m, ROCK), json!("linear"));
    assert!(!ignored(m, ROCK));
    pick(&mut h, "Alpha ×8");
    let m = h.state();
    assert_eq!(encode_params(m, ROCK), (false, Some(Bc7Weights::Alpha8)));
    assert_eq!(own(m, ROCK).1, Some(json!("alpha8")));
    assert!(!ignored(m, ROCK));
}

/// Note FC8 (supersedes CP10's second pick): an own value over a different inherited one hid the inherited value, so
/// Linear cleared the own value and the inherited one showed. One Linear pick now overrides it, in one undo step.
#[test]
fn linear_once() {
    // (inherited layer, the own value written first)
    for (toml, first) in [
        ("[defaults]\nencoder.bc7_weights = \"alpha8\"\n", "alpha16"),
        ("[defaults]\nencoder.perceptual = true\n", "perceptual"),
    ] {
        let d = tempfile::tempdir().unwrap();
        let mut m = ready_with(d.path(), toml);
        m.set_field(ROCK, WEIGHTING, json!(first));
        idle(&mut m);
        assert_eq!(row_value(&m, ROCK), json!(first), "{toml}");
        assert!(own(&m, ROCK) != (None, None), "{toml}: an own value");
        m.select_item(ROCK);
        let steps = m.history.undo.len();
        let mut h = harness(m);
        h.run();
        pick(&mut h, "Linear");
        idle(h.state_mut());
        h.run();
        let m = h.state();
        assert_eq!(row_value(m, ROCK), json!("linear"), "{toml}");
        assert_eq!(encode_params(m, ROCK), (false, None), "{toml}");
        assert_eq!(
            own(m, ROCK),
            (Some(json!(false)), Some(json!("linear"))),
            "{toml}"
        );
        assert_eq!(m.history.undo.len(), steps + 1, "{toml}: one step");
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
    // Both of the texture's own fields set (perceptual wins); the reset clears both.
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
/// shows only for BC7 with bc7e, and (I2) the Advanced drawer's view of it is locked elsewhere, with the same reason.
#[test]
fn perceptual_keeps_its_lock_and_the_row_is_bc7e_only() {
    let d = tempfile::tempdir().unwrap();
    let m = ready(d.path());
    let job = m.item(NORMAL).unwrap().job.clone().unwrap();
    let n = job.profile.clone();
    assert_eq!(n.format, Format::Bc7);
    assert!(weighting_option_lock(m.target, &n, "perceptual").is_some());
    assert_eq!(weighting_option_lock(m.target, &n, "alpha16"), None);
    assert_eq!(hidden_reason(&n, WEIGHTING), None);
    assert_eq!(field_view(m.target, &job, WEIGHTING).unwrap().lock, None);
    let mut j = job.clone();
    j.profile.encoder.backend = Backend::Nvtt;
    let why = Some("used only by the built-in BC7 encoder");
    assert_eq!(hidden_reason(&j.profile, WEIGHTING), why);
    assert_eq!(field_view(m.target, &j, WEIGHTING).unwrap().lock, why);
    j.profile.format = Format::Bc3;
    assert_eq!(hidden_reason(&j.profile, WEIGHTING), Some("BC7 only"));
    assert_eq!(
        field_view(m.target, &j, WEIGHTING).unwrap().lock,
        Some("BC7 only")
    );
}

/// Fix round 1, M1: leaving BC7 drops the texture's own weighting, so no hidden value keeps warning.
#[test]
fn a_format_change_away_from_bc7_drops_the_weighting() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
    m.set_field(ROCK, WEIGHTING, json!("alpha8"));
    idle(&mut m);
    m.set_format(ROCK, Format::Bc7, ColorTag::Srgb);
    idle(&mut m);
    assert_eq!(own(&m, ROCK).1, Some(json!("alpha8")), "BC7 keeps it");
    m.set_format(ROCK, Format::Bc1, ColorTag::Srgb);
    idle(&mut m);
    assert_eq!(own(&m, ROCK).1, None);
    assert!(!ignored(&m, ROCK));
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
