//! Note FC9 (theme 9): one word per concept. A sweep over the main views: no stray word, the PGPatcher lock is a grey
//! padlock with a caption, and a feature that is on but blocked says so.
mod support;
use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use sdc_gui::model::sets::set_views;
use sdc_gui::model::AppModel;
use serde_json::json;
use std::time::Duration;
use support::*;

const SET: &str = "landscape/rock01";

/// Words the UI no longer uses for a concept that has one name (FC9): the old names of "undecided" and "not ready",
/// the id-with-spaces sub-type label, and the two half-size names Review does not use.
const STRAYS: [&str; 10] = [
    "need a decision",
    "Need a decision",
    "Needs a decision",
    "Needs decision",
    "needs decision",
    "Needs attention",
    "skin rfaos",
    "mip 0",
    "Mip 0",
    "Fake resolution",
];

fn harness(model: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(egui::vec2(1600.0, 900.0))
        .build_ui_state(
            |ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model),
            model,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

fn assert_no_strays(h: &Harness<'static, AppModel>, view: &str) {
    for w in STRAYS {
        assert_eq!(
            h.query_all_by_label_contains(w).count(),
            0,
            "{view}: a label still says {w:?}"
        );
    }
}

#[test]
fn no_stray_words() {
    // The Textures view with an undecided texture selected (the settings box) and the Problems tab open.
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.open_problems(None);
    let key = m
        .items
        .iter()
        .find(|i| i.key.ends_with("rock01_normal.png"))
        .unwrap()
        .key
        .clone();
    m.select_item(&key);
    let mut h = harness(m);
    h.run();
    h.run();
    assert_no_strays(&h, "Textures");
    // The one word, on the count line, the pill, the Problems rows and the settings box.
    for w in ["undecided", "Undecided"] {
        assert!(h.query_all_by_label_contains(w).count() >= 1, "{w}");
    }
    assert!(
        h.query_all_by_label_contains("Undecided: ").count() >= 1,
        "a Problems row reads `Undecided: …`"
    );

    // The Sets column: its pill is `Not ready`.
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.open_problems(None);
    let mut h = harness(m);
    h.run();
    h.run();
    assert_no_strays(&h, "Sets");
    assert!(h.query_all_by_label_contains("Not ready").count() >= 1);
}

/// Note FC9 (AN6): a PGPatcher-locked slot is a grey padlock with the caption `kept by PGPatcher`, not a red cross.
#[test]
fn the_pgpatcher_lock_reads_kept_by_pgpatcher() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    let key = m
        .sets
        .iter()
        .find(|s| s.key.ends_with(SET))
        .unwrap()
        .key
        .clone();
    m.select_set(&key);
    let mut h = harness(m);
    h.run();
    assert_eq!(h.query_all_by_label("kept by PGPatcher").count(), 0);
    h.state_mut()
        .set_json_value(SET, "lock_rmaos", Some(json!(true)));
    assert!(h.state_mut().wait_idle(Duration::from_secs(60)));
    h.run();
    h.run();
    assert!(
        h.query_all_by_label("kept by PGPatcher").count() >= 1,
        "the padlock is named"
    );
    assert!(set_views(h.state()).iter().any(|v| v.id == SET));
}

/// Note FC9 (CC9/CC10): a feature that is on but blocked reads `on, blocked by …`, not `off`.
#[test]
fn a_blocked_feature_reads_on_blocked_by() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    let key = m
        .sets
        .iter()
        .find(|s| s.key.ends_with(SET))
        .unwrap()
        .key
        .clone();
    m.select_set(&key);
    m.set_json_value(SET, "hair", Some(json!(true)));
    m.set_json_value(SET, "multilayer", Some(json!(true)));
    assert!(m.wait_idle(Duration::from_secs(60)));
    let mut h = harness(m);
    h.run();
    h.run();
    h.get_by_label_contains("on, blocked by");
}
