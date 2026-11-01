//! Note FC9 (theme 9): one word per concept. A sweep over the main views: no stray word, the PGPatcher lock is a grey
//! padlock with a caption, and a feature that is on but blocked says so.
mod support;
use eframe::egui;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use sdc_gui::model::AppModel;
use serde_json::json;
use std::time::Duration;
use support::*;

const SET: &str = "landscape/rock01";

/// Words the UI no longer uses for a concept that has one name (FC9): the old names of "undecided" and "not ready",
/// the id-with-spaces sub-type label, and the two half-size names Review does not use.
const STRAYS: [&str; 12] = [
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
    // Note FI11: the pin and the PGPatcher lock are said in words; the terms ride on hover.
    "(sdc)",
    "(PGPatcher lock)",
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

/// Note FI14 (A16): the editor is `PGPatcher JSON` in its title and its text; the add button names the action.
#[test]
fn the_json_editor_has_one_name() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.open_json_editor();
    let mut h = harness(m);
    h.run();
    h.run();
    assert_eq!(h.query_all_by_label_contains("TruePBR JSON").count(), 0);
    h.get_by_label("+ Add a delete section");
    h.get_by_label("Close PGPatcher JSON");
}

/// Note FI14 (A20): before any export the Files tab says so, and shows no counts of nothing.
#[test]
fn the_files_tab_before_export_says_not_exported_yet() {
    use sdc_gui::model::bottom::BottomTab;
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.show_tab(BottomTab::Files);
    let mut h = harness(m);
    h.run();
    h.run();
    h.get_by_label("Not exported yet.");
    assert_eq!(h.query_all_by_label_contains("0 converted").count(), 0);
    assert_eq!(h.query_all_by_label_contains("No files yet").count(), 0);
}

/// Note FI14 (A21): one texture and one set read in the singular.
#[test]
fn one_set_is_singular() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("textures/landscape");
    std::fs::create_dir_all(&root).unwrap();
    image::ImageBuffer::from_fn(64, 64, |_, _| image::Rgb([90u8, 90, 90]))
        .save(root.join("rock01.png"))
        .unwrap();
    let mut m = AppModel::new(sdc_gui::settings::GuiSettings::default(), None, session());
    m.open_folder(d.path().to_path_buf());
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(
        m.status_line.contains("1 texture, 1 set"),
        "{}",
        m.status_line
    );
    assert!(!m.status_line.contains("1 textures"), "{}", m.status_line);
}

/// Note FI14 (A22): the Undecided row's parts are sentences joined with `. `, and the alpha-data acknowledgement says
/// what it does.
#[test]
fn the_alpha_acknowledge_names_its_action() {
    use engine::codes::Code;
    use sdc_gui::model::items::{Decision, ItemStatus};
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let it = m
        .items
        .iter_mut()
        .find(|i| i.key == "landscape/odd.png")
        .unwrap();
    it.decisions = vec![
        Decision::AlphaMeaning,
        Decision::Acknowledge(Code::LossAlphaData),
    ];
    it.status = ItemStatus::NeedsDecision;
    m.invalidate_views();
    m.open_problems(None);
    let mut h = harness(m);
    h.run();
    h.run();
    assert!(h.query_all_by_label("Drop the alpha anyway").count() >= 1);
    assert!(
        h.query_all_by_label_contains("Undecided: what its alpha means. This alpha channel")
            .count()
            >= 1
    );
    for n in h.query_all_by_label_contains("Undecided: ") {
        let l = n.accesskit_node().label().unwrap_or_default().to_string();
        assert!(!l.contains("; "), "{l}");
    }
}

/// Fix round 1: the words that live in places the first sweep never opened.
#[test]
fn advanced_checks_use_low_detail_and_no_mip_zero() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.settings.advanced_open = true;
    let mut h = harness(m);
    h.ctx.global_style_mut(|s| {
        s.interaction.tooltip_delay = 0.0;
        s.interaction.show_tooltips_only_when_still = false;
    });
    h.run();
    assert_eq!(h.query_all_by_label("Low detail check").count(), 1);
    assert_no_strays(&h, "Advanced");
    // The hover is not in the tree until it shows; hover the switch and sweep again.
    h.get_by_label_contains("Measure whether full size adds anything")
        .hover();
    h.run();
    h.run();
    h.get_by_label_contains("full-size level");
    assert_no_strays(&h, "Advanced hover");
}

/// Fix round 1: the sub-type dropdowns offer `Skin RFAOS`, never `skin rfaos`.
fn open_a_combo_and_check(h: &mut Harness<'static, AppModel>, view: &str) {
    use eframe::egui::accesskit::Role;
    let combos: Vec<_> = h
        .query_all_by_role(Role::ComboBox)
        .map(|n| n.rect())
        .collect();
    assert!(!combos.is_empty(), "{view}: a sub-type combo");
    let mut seen = false;
    for r in combos {
        h.input_mut()
            .events
            .push(egui::Event::PointerMoved(r.center()));
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: r.center(),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        });
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: r.center(),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        h.run();
        h.run();
        assert_eq!(
            h.query_all_by_label_contains("skin rfaos").count(),
            0,
            "{view}: lower-case id offered"
        );
        if h.query_all_by_label("Skin RFAOS").count() > 0 {
            seen = true;
            break;
        }
        h.key_press(egui::Key::Escape);
        h.run();
    }
    assert!(seen, "{view}: Skin RFAOS is offered");
}

#[test]
fn the_subtype_dropdowns_offer_skin_rfaos() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.select_item("landscape/rock01_normal.png");
    let mut h = harness(m);
    h.run();
    open_a_combo_and_check(&mut h, "settings box");

    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.open_review();
    let mut h = harness(m);
    h.run();
    h.run();
    open_a_combo_and_check(&mut h, "Review");
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
