//! Layout v2 A3 / C6: every set editor has a Material (JSON set) or Set tab and an Advanced tab, under the slot rows
//! since Note FB2 (one column); the window-edge Advanced strip stays for a texture, the preview, a bulk selection,
//! nothing selected and 3D.
mod support;
use eframe::egui::{self, accesskit::Role, containers::panel::PanelState, Id};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use sdc_gui::model::set_preview::{SetPreview, DEFAULT_CAP};
use sdc_gui::model::AppModel;
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

/// Note FB2: the set editor is one column in the centre; its tabs and groups end at the centre's visible edge (the
/// panel's rect grows with its content, so it is cut at the window).
fn centre(h: &H) -> egui::Rect {
    sdc_gui::ui::centre_rect(&h.ctx)
        .expect("the centre is drawn")
        .intersect(h.ctx.content_rect())
}

/// The set editor's tabs are drawn: its "Advanced" tab in the centre (the drawer's strip is at the window edge).
fn set_tabs(h: &H) -> bool {
    sdc_gui::ui::centre_rect(&h.ctx).is_some_and(|c| {
        h.query_all_by_role(Role::Button).any(|n| {
            n.accesskit_node().label().as_deref() == Some("Advanced")
                && c.contains(n.rect().center())
        })
    })
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

/// Note FB2 (was "the slot rows stay put while the material panel scrolls"): one column scrolls as one, so the rows
/// move with the groups under them.
#[test]
fn the_slot_rows_scroll_with_the_material_groups() {
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
    let dy = target.min.y - moved.min.y;
    assert!(dy > 0.0, "the column scrolled ({target:?} → {moved:?})");
    let now = slot_rect(&h, "Diffuse");
    assert!(
        (diffuse.min.y - now.min.y - dy).abs() < 1.0,
        "the rows moved with it ({diffuse:?} → {now:?}, {dy})"
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
    assert!(!set_tabs(&h));
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
    // K21: the checkbox's label. Note FB2: below the slot rows; scroll to it as a user would.
    h.get_by_label("Multilayer (coat)").scroll_to_me();
    h.run();
    h.get_by_label("Multilayer (coat)").click();
    h.run();
    assert_eq!(header_value(&h, "Coat"), "on");
    h.get_by_label("Coat colour");
}

/// Note FB2 (C14): the button sits on the tab row, after the Advanced tab.
#[test]
fn the_json_editor_button_is_small_and_on_the_tab_row() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    rock_selected(&mut m);
    let mut h = harness(m);
    h.run();
    let tab = h.get_by_role_and_label(Role::Button, "Advanced").rect();
    let b = h.get_by_label("PGPatcher JSON editor…");
    assert!(b.rect().width() < 220.0);
    assert!(
        b.rect().min.x > tab.max.x && tab.y_range().contains(b.rect().center().y),
        "button {:?} beside the tab {tab:?}",
        b.rect()
    );
    b.click();
    h.run();
    assert!(h.state().0.json_editor_open);
}

/// Note DO, then FB2: at 1000 px with a long set name the button stays inside the centre.
#[test]
fn the_json_editor_button_stays_inside_a_narrow_centre() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    rock_selected(&mut m);
    // The screenshot's case: a long name.
    m.set_input_name(
        "landscape/rock01",
        "textures/landscape/kingolafeffigy/kingolafeffigyeffigy01",
    )
    .unwrap();
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    let key = m
        .sets
        .iter()
        .find(|s| s.key.ends_with("kingolafeffigyeffigy01"))
        .expect("the renamed set")
        .key
        .clone();
    m.select_set(&key);
    let mut h = harness_with(m, 1000.0, 800.0, false);
    h.run();
    let p = centre(&h);
    let b = h.get_by_label("PGPatcher JSON editor…").rect();
    assert!(b.max.x <= p.max.x, "button {b:?} inside the centre {p:?}");
}

/// `set_editor_shown` mirrors `centre`'s and `set_editor::draw`'s routing: in every centre state exactly one of the
/// window-edge drawer and the set editor's tabs is drawn (3D: the drawer); with no folder (the start screen), neither. Each
/// case is a fresh harness, so the persisted panel states are this case's own. The last field is (panel, drawer).
#[test]
fn drawer_and_panel_never_both_or_neither() {
    type Setup = fn(&mut AppModel);
    let cases: [(&str, Setup, bool, (bool, bool)); 8] = [
        (
            "expanded texture over a set",
            |m| {
                rock_selected(m);
                m.expanded = Some("landscape/rock01.png".into());
            },
            false,
            (false, true),
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
            (false, true),
        ),
        (
            "nothing",
            |m| m.selection = sdc_gui::model::Selection::None,
            false,
            (false, true),
        ),
        (
            "a texture",
            |m| m.select_item("landscape/leaf.png"),
            false,
            (false, true),
        ),
        // A11: the start screen draws neither.
        (
            "no folder",
            |m| *m = AppModel::placeholder(Default::default()),
            false,
            (false, false),
        ),
        ("a set in 2D", rock_selected, false, (true, false)),
        (
            "a set in 3D",
            |m| {
                rock_selected(m);
                m.settings.set_view_3d = true;
            },
            true,
            (false, true),
        ),
        (
            "a stale set",
            |m| m.selection = sdc_gui::model::Selection::Set("landscape/gone".into()),
            false,
            (false, true),
        ),
    ];
    for (name, setup, three_d, (want_panel, want_drawer)) in cases {
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
        let panel = set_tabs(&h);
        assert_eq!(panel, want_panel, "{name}: the set panel");
        assert_eq!(drawer_drawn(&h), want_drawer, "{name}: the drawer");
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
    assert!(!set_tabs(&h));
}

/// I1 (final review): a group wider than its column (the Coat colour row) widens the scroll content, and egui unions
/// that into every later header row; each header value must still sit inside the centre (Note FB2: one column).
#[test]
fn header_values_stay_inside_the_panel_with_coat_on() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    rock_selected(&mut m);
    let mut h = harness(m);
    h.run();
    h.get_by_label("Multilayer (coat)").scroll_to_me();
    h.run();
    h.get_by_label("Multilayer (coat)").click();
    h.run();
    assert_eq!(header_value(&h, "Coat"), "on");
    let p = centre(&h);
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
