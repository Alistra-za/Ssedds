//! Layout v2 A3 / C6: every set editor has a right-hand panel with a Material (JSON set) or Set tab and an Advanced
//! tab; the window-edge Advanced strip stays for a texture, the preview, a bulk selection, nothing selected and 3D.
mod support;
use eframe::egui::{self, accesskit::Role, containers::panel::PanelState, Id};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use sdc_gui::model::set_preview::{SetPreview, DEFAULT_CAP};
use sdc_gui::model::AppModel;
use sdc_gui::ui::set_preview::CENTRE_MIN;
use sdc_gui::ui::State;
use support::*;

type H = Harness<'static, (AppModel, State)>;

/// The window-edge drawer: its strip (a Button "Advanced", as the tab) or its open panel, by panel id.
fn drawer_drawn(h: &H) -> bool {
    ["advanced-strip", "advanced"]
        .iter()
        .any(|id| PanelState::load(&h.ctx, Id::new(*id)).is_some())
}

/// Reduced from set_preview_layout.rs `harness_with` (K19): `three_d` builds the enabled (IO-free) set preview.
fn harness_with(m: AppModel, w: f32, h: f32, three_d: bool) -> H {
    let state = State {
        set3d: if three_d {
            SetPreview::new(None, None, None, DEFAULT_CAP)
        } else {
            SetPreview::default()
        },
        ..State::default()
    };
    let h = Harness::builder()
        .with_size(egui::vec2(w, h))
        .build_ui_state(
            |ui, (m, s): &mut (AppModel, State)| sdc_gui::ui::draw_with(ui, m, s),
            (m, state),
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

fn harness(m: AppModel) -> H {
    harness_with(m, 1400.0, 900.0, false)
}

fn rock_selected(m: &mut AppModel) {
    let key = m
        .sets
        .iter()
        .find(|s| s.key.ends_with("landscape/rock01"))
        .unwrap()
        .key
        .clone();
    m.select_set(&key);
}

fn panel(h: &H) -> egui::Rect {
    PanelState::load(&h.ctx, Id::new("set-panel"))
        .expect("the set panel")
        .outer_rect
}

#[test]
fn a_vanilla_set_has_a_set_tab_and_an_advanced_tab_and_no_window_edge_drawer() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    decide_all(&mut m);
    rock_selected(&mut m);
    let mut h = harness(m);
    h.run();
    h.get_by_role_and_label(Role::Button, "Set");
    h.get_by_role_and_label(Role::Button, "Advanced");
    assert!(!drawer_drawn(&h), "no strip at the window edge");
    h.get_by_label("Used by plugins");
}

#[test]
fn a_pbr_set_has_a_material_tab() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    rock_selected(&mut m);
    let mut h = harness(m);
    h.run();
    // K11: the pgp group "Material" shares the label; the tab is the Button.
    h.get_by_role_and_label(Role::Button, "Material");
    h.get_by_label("Features");
    // The PBR Landscape card is wider than the default: it scrolls in the panel, the panel does not grow.
    let w = panel(&h).width();
    assert!(
        (w - sdc_gui::ui::set_editor::SET_PANEL_WIDTH).abs() <= 1.0,
        "panel {w}"
    );
}

#[test]
fn the_advanced_tab_shows_the_drawer_body_and_is_remembered() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    rock_selected(&mut m);
    let mut h = harness(m);
    h.run();
    assert!(h.query_by_label_contains("Preset inspector").is_none());
    h.get_by_role_and_label(Role::Button, "Advanced").click();
    h.run();
    h.get_by_label_contains("Preset inspector");
    // A7: the code's hint, with a set selected.
    h.get_by_label("Select a texture to edit its encoder and header fields.");
    assert!(h.state().0.settings.advanced_open);
    assert!(h.query_by_label("Features").is_none(), "one tab at a time");
}

#[test]
fn the_slot_rows_stay_put_while_the_material_panel_scrolls() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    rock_selected(&mut m);
    let mut h = harness_with(m, 1400.0, 700.0, false);
    h.run();
    let diffuse = slot_rect(&h, "Diffuse");
    let target = h.get_by_label("Advanced TruePBR settings").rect();
    h.get_by_label("Advanced TruePBR settings").scroll_to_me();
    h.run();
    let moved = h.get_by_label("Advanced TruePBR settings").rect();
    assert!(
        moved.min.y < target.min.y,
        "the panel scrolled ({target:?} → {moved:?})"
    );
    assert_eq!(
        slot_rect(&h, "Diffuse"),
        diffuse,
        "the slot rows stayed put"
    );
}

#[test]
fn a_selected_texture_keeps_the_window_edge_advanced_strip() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.select_item("landscape/leaf.png");
    let mut h = harness(m);
    h.run();
    assert!(drawer_drawn(&h), "the strip at the window edge");
    assert!(PanelState::load(&h.ctx, Id::new("set-panel")).is_none());
}

#[test]
fn a_set_in_3d_keeps_the_window_edge_advanced_strip() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    rock_selected(&mut m);
    m.settings.set_view_3d = true;
    let mut h = harness_with(m, 1400.0, 800.0, true);
    // The 3D view never goes idle.
    for _ in 0..6 {
        h.run_steps(3);
        let (m, s) = h.state_mut();
        s.set3d.poll(&*m);
    }
    assert!(drawer_drawn(&h), "the strip at the window edge");
}

/// Layout v2 ruling (row width, moved from set_editor_ui.rs): at 1400 the rows fit beside the panel at its default
/// width, and no row widget reaches under it (no sideways scroll). A narrower centre scrolls them sideways.
#[test]
fn the_slot_rows_stay_in_their_column() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    rock_selected(&mut m);
    let mut h = harness(m);
    h.run();
    let edge = panel(&h).min.x;
    let left = slot_rect(&h, "Diffuse").min.x;
    let mut right = 0.0f32;
    for l in [
        "Diffuse thumbnail",
        "Match on this",
        "Opaque",
        "Assign…",
        "ready",
        "warning",
        "blocks export",
    ] {
        for n in h.query_all_by_label(l) {
            if n.rect().min.x >= left && n.rect().min.x < edge {
                right = right.max(n.rect().max.x);
            }
        }
    }
    for n in h.query_all_by_role(Role::ComboBox) {
        if n.rect().min.x >= left && n.rect().min.x < edge {
            right = right.max(n.rect().max.x);
        }
    }
    assert!(right > left, "found the row widgets");
    assert!(
        right < edge,
        "the rows ({left}..{right}) reach under the panel at {edge}"
    );
}

/// A8: a panel dragged (here: stored) far wider still leaves the rows CENTRE_MIN / 2.
#[test]
fn the_rows_keep_their_minimum_beside_a_wide_panel() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    rock_selected(&mut m);
    let mut h = harness(m);
    let outer_rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1400.0, 900.0));
    h.ctx
        .data_mut(|d| d.insert_persisted(Id::new("set-panel"), PanelState { outer_rect }));
    h.run();
    let edge = panel(&h).min.x;
    let left = slot_rect(&h, "Diffuse").min.x;
    assert!(
        edge - left >= CENTRE_MIN / 2.0,
        "rows {left}..{edge} beside a panel {} wide",
        panel(&h).width()
    );
    // The panel never sits over the rows: the first cell (the slot label) ends left of it; the rest scroll sideways.
    assert!(slot_rect(&h, "Diffuse").max.x < edge);
}

/// A group's title (K11: not the tab, which is a Button).
fn title<'a>(h: &'a H, t: &'a str) -> egui_kittest::Node<'a> {
    h.query_all_by_label(t)
        .find(|n| n.accesskit_node().role() != Role::Button)
        .unwrap_or_else(|| panic!("no group {t}"))
}

/// The value right-aligned in a group's header row ("on"/"off" repeat, so it is found beside its title).
fn header_value(h: &H, t: &str) -> String {
    let r = title(h, t).rect();
    h.query_all_by_role(Role::Label)
        .filter(|n| n.rect().min.x > r.max.x && (n.rect().center().y - r.center().y).abs() < 2.0)
        .find_map(|n| n.value())
        .unwrap_or_else(|| panic!("no header value beside {t}"))
}

#[test]
fn the_material_groups_are_framed_in_order_with_values() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    rock_selected(&mut m);
    let mut h = harness(m);
    h.run();
    let ys: Vec<f32> = [
        "Material",
        "Features",
        "Coat",
        "Subsurface",
        "NiAlphaProperty",
        "Matching",
        "CS PBR records",
        "Advanced TruePBR settings",
    ]
    .iter()
    .map(|t| title(&h, t).rect().min.y)
    .collect();
    assert!(ys.windows(2).all(|w| w[0] < w[1]), "{ys:?}");
    assert_eq!(header_value(&h, "Subsurface"), "Off");
    assert_eq!(header_value(&h, "Matching"), "1 match");
    assert_eq!(header_value(&h, "Coat"), "off");
    assert_eq!(header_value(&h, "NiAlphaProperty"), "off");
    assert_eq!(header_value(&h, "CS PBR records"), "0 records");
}

#[test]
fn turning_multilayer_on_opens_the_coat_group() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    rock_selected(&mut m);
    let mut h = harness(m);
    h.run();
    assert!(h.query_by_label("Coat colour").is_none());
    // K21: the checkbox's label.
    h.get_by_label("Multilayer (coat)").click();
    h.run();
    assert_eq!(header_value(&h, "Coat"), "on");
    h.get_by_label("Coat colour");
}

#[test]
fn the_json_editor_button_is_small_and_in_the_panel_header() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    rock_selected(&mut m);
    let mut h = harness(m);
    h.run();
    let b = h.get_by_label("TruePBR JSON editor…");
    assert!(b.rect().width() < 220.0);
    assert!(b.rect().min.x > panel(&h).min.x, "in the panel");
    b.click();
    h.run();
    assert!(h.state().0.json_editor_open);
}

/// `set_editor_shown` mirrors `centre`'s and `set_editor::draw`'s routing: in every centre state exactly one of the
/// window-edge drawer and the set panel is drawn (3D: the drawer). Each case is a fresh harness, so the persisted
/// panel states are this case's own.
#[test]
fn drawer_and_panel_never_both_or_neither() {
    type Setup = fn(&mut AppModel);
    let cases: [(&str, Setup, bool, bool); 7] = [
        (
            "expanded texture over a set",
            |m| {
                rock_selected(m);
                m.expanded = Some("landscape/rock01.png".into());
            },
            false,
            false,
        ),
        (
            "two sets",
            |m| {
                let keys: Vec<String> = m.sets.iter().map(|s| s.key.clone()).collect();
                m.click_set(&keys[0], false, false);
                m.click_set(&keys[1], true, false);
                assert!(m.in_bulk());
            },
            false,
            false,
        ),
        (
            "nothing",
            |m| m.selection = sdc_gui::model::Selection::None,
            false,
            false,
        ),
        (
            "a texture",
            |m| m.select_item("landscape/leaf.png"),
            false,
            false,
        ),
        ("a set in 2D", rock_selected, false, true),
        (
            "a set in 3D",
            |m| {
                rock_selected(m);
                m.settings.set_view_3d = true;
            },
            true,
            false,
        ),
        (
            "a stale set",
            |m| m.selection = sdc_gui::model::Selection::Set("landscape/gone".into()),
            false,
            false,
        ),
    ];
    for (name, setup, three_d, want_panel) in cases {
        let d = tempfile::tempdir().unwrap();
        let mut m = pbr_opened_plain(d.path());
        setup(&mut m);
        let mut h = harness_with(m, 1400.0, 900.0, three_d);
        if three_d {
            // The 3D view never goes idle.
            for _ in 0..6 {
                h.run_steps(3);
                let (m, s) = h.state_mut();
                s.set3d.poll(&*m);
            }
        } else {
            h.run();
        }
        let panel = PanelState::load(&h.ctx, Id::new("set-panel")).is_some();
        assert_eq!(panel, want_panel, "{name}: the set panel");
        assert_eq!(drawer_drawn(&h), !want_panel, "{name}: the drawer");
    }
}

/// A selection whose set left the plan shows the editor's message, and Advanced stays reachable through the strip.
#[test]
fn a_set_no_longer_in_the_plan_keeps_the_window_edge_advanced_strip() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    m.selection = sdc_gui::model::Selection::Set("landscape/gone".into());
    let mut h = harness(m);
    h.run();
    h.get_by_label("This set is no longer in the plan.");
    assert!(drawer_drawn(&h), "the strip at the window edge");
    assert!(PanelState::load(&h.ctx, Id::new("set-panel")).is_none());
}

/// I1 (final review): a group wider than the panel (the Coat colour row) widens the scroll content, and egui unions
/// that into every later header row; each header value must still sit inside the panel's visible rect.
#[test]
fn header_values_stay_inside_the_panel_with_coat_on() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    rock_selected(&mut m);
    let mut h = harness(m);
    h.run();
    h.get_by_label("Multilayer (coat)").click();
    h.run();
    let p = panel(&h);
    for t in [
        "Coat",
        "Subsurface",
        "Emissive settings",
        "Parallax settings",
        "NiAlphaProperty",
        "Matching",
        "CS PBR records",
    ] {
        let r = title(&h, t).rect();
        let v = h
            .query_all_by_role(Role::Label)
            .find(|n| {
                n.rect().min.x > r.max.x
                    && (n.rect().center().y - r.center().y).abs() < 2.0
                    && n.value().is_some()
            })
            .unwrap_or_else(|| panic!("no header value beside {t}"));
        assert!(
            v.rect().max.x <= p.max.x,
            "{t}: value at {:?}, panel {p:?}",
            v.rect()
        );
    }
}
