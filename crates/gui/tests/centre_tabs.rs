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
    assert_in_centre(&h, "Pop out Presets");
    assert_in_centre(&h, "Pick a preset, or New.");
    assert_eq!(windows(&h), 0, "no floating Presets window");
}

/// Final review M4 (Note FB7): the Advanced drawer is hidden while a docked view is in front.
#[test]
fn the_advanced_drawer_is_hidden_while_a_docked_view_is_in_front() {
    use egui::containers::panel::PanelState;
    let drawn = |h: &H| {
        ["advanced", "advanced-strip"]
            .iter()
            .any(|id| PanelState::load(&h.ctx, egui::Id::new(id)).is_some())
    };
    let d = tempfile::tempdir().unwrap();
    let h = harness(pbr_opened(d.path()));
    assert!(drawn(&h), "the drawer is there with the selection in front");
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.open_presets();
    let h = harness(m);
    assert_eq!(h.state().centre_front(), CentreTab::Presets);
    assert!(!drawn(&h), "no drawer under a docked Presets tab");
}

#[test]
fn json_editor_opens_as_a_centre_tab() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(pbr_opened(d.path()));
    // The same menu item as before: Export to ▾ → PGPatcher JSON editor….
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
    assert_in_centre(&h, &format!("Pop out {WINDOW_TITLE}"));
    assert_eq!(windows(&h), 0, "no floating PGPatcher JSON window");
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
        let title = match tab {
            CentreTab::Presets => "Presets",
            _ => WINDOW_TITLE,
        };
        let (pop, dock) = (format!("Pop out {title}"), format!("Dock {title}"));
        let mut h = harness(m);
        h.run();
        assert_eq!(windows(&h), 0, "{tab:?} starts docked");
        h.get_by_label(&pop).click();
        h.run();
        h.run();
        assert!(popped(h.state()), "{tab:?} popped out");
        assert_eq!(
            h.state().centre_front(),
            CentreTab::Selection,
            "the centre shows the selection again"
        );
        assert!(h.query_by_label(&pop).is_none());
        assert_eq!(windows(&h), 1, "{tab:?} is in a viewport of its own");
        h.get_by_label(&dock).click();
        h.run();
        h.run();
        assert!(!popped(h.state()), "{tab:?} docked");
        assert_eq!(h.state().centre_front(), tab);
        assert_eq!(windows(&h), 0);
        assert_in_centre(&h, &pop);
    }
}

/// Rule 8.4: with Presets docked and the JSON editor popped out, the two views' controls keep distinct names.
#[test]
fn docked_and_popped_controls_have_distinct_names() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.json_editor_popped = true;
    m.open_json_editor();
    m.open_presets();
    let mut h = harness(m);
    h.run();
    assert_eq!(windows(&h), 1, "the JSON editor is popped out");
    let mut names: Vec<String> = h
        .query_all_by_role(Role::Button)
        .filter_map(|n| n.accesskit_node().label())
        .filter(|l| {
            ["Close", "Pop out", "Dock"]
                .iter()
                .any(|p| l.starts_with(p))
        })
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "Close PGPatcher JSON",
            "Close Presets",
            "Dock PGPatcher JSON",
            "Pop out Presets"
        ],
        "one of each, no two alike"
    );
}

/// A click on a set in the Sets column brings the selection to the front, even the set already selected.
#[test]
fn a_set_click_brings_the_selection_to_the_front() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    let rock = m
        .sets
        .iter()
        .find(|s| s.key.ends_with("landscape/rock01"))
        .expect("the rock set")
        .key
        .clone();
    m.select_set(&rock);
    m.open_presets();
    let mut h = harness(m);
    h.run();
    assert_eq!(h.state().centre_front(), CentreTab::Presets);
    assert!(h
        .query_by_role_and_label(Role::Button, "Slot commands…")
        .is_none());
    h.query_all_by_label("landscape/rock01")
        .find(|n| in_sets_column(&h, n.rect()))
        .expect("the rock set's card")
        .click();
    h.run();
    h.run();
    assert_eq!(h.state().centre_front(), CentreTab::Selection);
    assert!(h.state().presets_open, "Presets stays open as a tab");
    assert_in_centre(&h, "Slot commands…");
}

/// The strip takes one auto id drawn or not, so the widgets after it keep their ids when a view opens or closes.
#[test]
fn the_tab_strip_keeps_later_auto_ids() {
    let next_id = |open: bool| {
        let mut m = AppModel::placeholder(sdc_gui::settings::GuiSettings::default());
        if open {
            m.open_presets();
        }
        let id = std::cell::Cell::new(None);
        let mut h = Harness::builder()
            .with_size(egui::vec2(800.0, 400.0))
            .build_ui_state(
                |ui, m: &mut AppModel| {
                    sdc_gui::ui::centre_tabs::strip(ui, m);
                    id.set(Some(ui.next_auto_id()));
                },
                m,
            );
        h.run();
        id.get().expect("drawn")
    };
    assert_eq!(next_id(false), next_id(true));
}

/// A leaf with two edited fields, the drawer open: the texture panel's rows carry a hint and a ↺ each.
fn edited_leaf(dir: &std::path::Path) -> AppModel {
    let mut m = opened(dir);
    m.settings.advanced_open = true;
    m.select_item("landscape/leaf.png");
    m.set_field(
        "landscape/leaf.png",
        "alpha.threshold",
        serde_json::json!(90),
    );
    m.set_field("landscape/leaf.png", "resize", serde_json::json!("half"));
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    m
}

fn sized(width: f32, m: AppModel) -> H {
    let mut h = Harness::builder()
        .with_size(egui::vec2(width, 1000.0))
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    h
}

/// Note F?12 (A9): a row's ↺ is laid out before its hint, so it stays inside the centre at every width with the
/// drawer open (the kittest stands in for screenshots at 1100, 1280 and 1600 px).
#[test]
fn the_reset_button_stays_on_the_panel() {
    for width in [1100.0f32, 1280.0, 1600.0] {
        let d = tempfile::tempdir().unwrap();
        let h = sized(width, edited_leaf(d.path()));
        let c = sdc_gui::ui::centre_rect(&h.ctx).expect("the centre is drawn");
        let drawer = egui::containers::panel::PanelState::load(&h.ctx, egui::Id::new("advanced"))
            .expect("the drawer is drawn")
            .outer_rect;
        assert!(
            drawer.max.x <= width + 0.5,
            "{width}: the drawer {drawer:?} leaves the window"
        );
        // On-screen ↺ buttons (rows scrolled out below the panel are not drawn), split by which panel they sit in.
        let (mut centre, mut in_drawer) = (vec![], vec![]);
        for n in h.query_all_by_label("↺") {
            let r = n.rect();
            if r.max.y > c.max.y {
                continue;
            }
            if r.min.x < drawer.min.x {
                centre.push(r)
            } else {
                in_drawer.push(r)
            }
        }
        assert_eq!(
            centre.len(),
            1,
            "{width}: the edited Alpha row's ↺ is in the centre"
        );
        for r in &centre {
            assert!(
                c.expand(0.5).contains_rect(*r),
                "{width}: ↺ {r:?} leaves the centre {c:?}"
            );
        }
        for r in &in_drawer {
            assert!(
                drawer.expand(0.5).contains_rect(*r),
                "{width}: ↺ {r:?} leaves the drawer {drawer:?}"
            );
        }
    }
}
/// A clipped hint keeps its whole text in the AccessKit name and on one tooltip, never two.
#[test]
fn a_clipped_hint_keeps_its_text_on_hover() {
    let text = "from this texture's settings";
    // The hint column is the drawer's (the centre's rows are compact and leave it blank).
    let visible = |h: &H| {
        let drawer = egui::containers::panel::PanelState::load(&h.ctx, egui::Id::new("advanced"))
            .expect("the drawer is drawn")
            .outer_rect;
        let hints = h
            .query_all_by_label(text)
            .map(|n| n.rect())
            .filter(|r| r.min.x >= drawer.min.x && r.max.y <= drawer.max.y)
            .collect::<Vec<_>>();
        (drawer, hints)
    };
    let d = tempfile::tempdir().unwrap();
    let wide = sized(1600.0, edited_leaf(d.path()));
    let natural = visible(&wide).1[0].width();
    let d = tempfile::tempdir().unwrap();
    let mut h = sized(900.0, edited_leaf(d.path()));
    let (drawer, before) = visible(&h);
    assert!(!before.is_empty(), "the edited rows show their hint");
    for r in &before {
        assert!(
            drawer.expand(0.5).contains_rect(*r),
            "hint {r:?} leaves the drawer {drawer:?}"
        );
    }
    assert!(
        before.iter().any(|r| r.width() < natural - 5.0),
        "no hint is clipped: {before:?} vs {natural}"
    );
    let all = h.query_all_by_label(text).count();
    h.get_all_by_label(text)
        .find(|n| n.rect() == before[0])
        .unwrap()
        .hover();
    h.run();
    // The tooltip may be shifted left of the drawer to fit the window, so count every node with the text.
    assert_eq!(
        h.query_all_by_label(text).count(),
        all + 1,
        "one tooltip, not two"
    );
}
