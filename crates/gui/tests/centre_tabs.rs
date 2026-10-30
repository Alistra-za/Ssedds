//! Note FB7 (review C6, amendment F): Presets and the TruePBR JSON editor open as centre-column tabs, never as
//! floating windows; Pop out moves a view to an OS window of its own and Dock brings it back.
mod support;
use eframe::egui::{self, accesskit::Role};
use egui_kittest::kittest::{NodeT as _, Queryable};
use egui_kittest::Harness;
use sdc_gui::model::{AppModel, CentreTab};
use sdc_gui::ui::json_editor::{NAME_LABEL, WINDOW_TITLE};
use support::*;

type H = Harness<'static, AppModel>;

fn harness(m: AppModel) -> H {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 1000.0))
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    h
}

/// Floating windows on screen: an `egui::Window` is a Middle-order area, and so is a viewport embedded as one.
fn windows(h: &H) -> usize {
    h.ctx.memory(|m| {
        m.areas()
            .visible_layer_ids()
            .iter()
            .filter(|l| l.order == egui::Order::Middle)
            .count()
    })
}

fn assert_in_centre(h: &H, label: &str) {
    let r = h.get_by_label(label).rect();
    let c = sdc_gui::ui::centre_rect(&h.ctx).expect("the centre is drawn");
    assert!(
        c.expand(0.5).contains_rect(r),
        "{label} {r:?} is outside the centre {c:?}"
    );
}

#[test]
fn presets_opens_as_a_centre_tab() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(pbr_opened(d.path()));
    // The same menu item as before: Preset ▾ → Presets….
    h.query_all_by_role(Role::ComboBox)
        .find(|n| {
            n.accesskit_node()
                .value()
                .or(n.accesskit_node().label())
                .is_some_and(|v| v.starts_with("default ("))
        })
        .expect("the Preset combo")
        .click();
    h.run();
    h.get_by_label("Presets…").click();
    h.run();
    h.run();
    assert!(h.state().presets_open);
    assert_eq!(h.state().centre_front(), CentreTab::Presets);
    assert_in_centre(&h, "Presets tab");
    assert_in_centre(&h, "Pop out");
    assert_in_centre(&h, "Pick a preset, or New.");
    assert_eq!(windows(&h), 0, "no floating Presets window");
}

#[test]
fn json_editor_opens_as_a_centre_tab() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(pbr_opened(d.path()));
    // The same menu item as before: Export to ▾ → TruePBR JSON editor….
    h.get_by_label_contains("Export to: ").click();
    h.run();
    h.get_by_label(sdc_gui::ui::header::JSON_EDITOR).click();
    h.run();
    h.run();
    assert!(h.state().json_editor_open);
    assert_eq!(h.state().centre_front(), CentreTab::Json);
    assert_in_centre(&h, &format!("{WINDOW_TITLE} tab"));
    assert_in_centre(&h, WINDOW_TITLE);
    assert_in_centre(&h, NAME_LABEL);
    assert_in_centre(&h, "Pop out");
    assert_eq!(windows(&h), 0, "no floating TruePBR JSON window");
}

#[test]
fn pop_out_and_dock() {
    for tab in [CentreTab::Presets, CentreTab::Json] {
        let d = tempfile::tempdir().unwrap();
        let mut m = pbr_opened(d.path());
        match tab {
            CentreTab::Presets => m.open_presets(),
            _ => m.open_json_editor(),
        }
        let popped = |m: &AppModel| match tab {
            CentreTab::Presets => m.presets_popped,
            _ => m.json_editor_popped,
        };
        let mut h = harness(m);
        h.run();
        assert_eq!(windows(&h), 0, "{tab:?} starts docked");
        h.get_by_label("Pop out").click();
        h.run();
        h.run();
        assert!(popped(h.state()), "{tab:?} popped out");
        assert_eq!(
            h.state().centre_front(),
            CentreTab::Selection,
            "the centre shows the selection again"
        );
        assert!(h.query_by_label("Pop out").is_none());
        assert_eq!(windows(&h), 1, "{tab:?} is in a viewport of its own");
        h.get_by_label("Dock").click();
        h.run();
        h.run();
        assert!(!popped(h.state()), "{tab:?} docked");
        assert_eq!(h.state().centre_front(), tab);
        assert_eq!(windows(&h), 0);
        assert_in_centre(&h, "Pop out");
    }
}
