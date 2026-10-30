//! Review C1, C21 and C14, owner amendment C (Note FB2): the set editor is one scrolling column. The slot rows sit on
//! top at full width, the Material / Advanced tabs and groups below them, so nothing is ever drawn over the rows and
//! no sideways scroll holds them; the set name and the slot label are each shown once.
mod support;
use eframe::egui::{self, accesskit::Role};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use sdc_gui::model::AppModel;
use sdc_gui::ui::State;
use support::*;

type H = Harness<'static, (AppModel, State)>;

fn harness(m: AppModel, w: f32) -> H {
    let h = Harness::builder()
        .with_size(egui::vec2(w, 800.0))
        .build_ui_state(
            |ui, (m, s): &mut (AppModel, State)| sdc_gui::ui::draw_with(ui, m, s),
            (m, State::default()),
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

fn rock_set(dir: &std::path::Path) -> (AppModel, String) {
    let mut m = pbr_opened_plain(dir);
    let v = sdc_gui::model::sets::views(&m)
        .iter()
        .find(|v| v.key.ends_with("landscape/rock01"))
        .map(|v| (v.key.clone(), v.id.clone()))
        .expect("the rock01 set");
    m.select_set(&v.0);
    (m, v.1)
}

/// At every width, with either tab open: each slot-row widget is above the tabs and the first group, starts at the
/// tabs' left edge, and ends inside the centre column (no sideways scroll, nothing beside the rows).
#[test]
fn set_editor_is_one_column() {
    for advanced in [false, true] {
        for w in [1024.0, 1100.0, 1280.0, 1536.0] {
            let d = tempfile::tempdir().unwrap();
            let (mut m, _) = rock_set(d.path());
            m.settings.advanced_open = advanced;
            let mut h = harness(m, w);
            h.run();
            let at = format!("{w} px, advanced {advanced}");
            // The centre panel's rect grows with its content; what the user sees ends at the window.
            let full = sdc_gui::ui::centre_rect(&h.ctx).expect("the centre is drawn");
            let centre = full.intersect(h.ctx.content_rect());
            let tab = |t: &str| h.get_by_role_and_label(Role::Button, t).rect();
            let (material, adv) = (tab("Material"), tab("Advanced"));
            // The first thing under the tabs: the Material group, or the Advanced body's first section.
            let first = if advanced {
                h.get_by_label_contains("Preset inspector").rect()
            } else {
                h.query_all_by_label("Material")
                    .find(|n| n.accesskit_node().role() != Role::Button)
                    .expect("the Material group")
                    .rect()
            };
            let top = material.min.y.min(adv.min.y).min(first.min.y);
            let (diffuse, cmds) = (
                slot_rect(&h, "Diffuse"),
                h.get_by_role_and_label(Role::Button, "Slot commands…")
                    .rect(),
            );
            // Between the first row and Slot commands… (the rows' y-range, in either layout).
            let in_rows = |r: &egui::Rect| {
                r.min.x >= full.min.x && r.min.y >= diffuse.min.y - 30.0 && r.max.y <= cmds.max.y
            };
            let mut rows = vec![diffuse, slot_rect(&h, "Normal _n"), cmds];
            for l in [
                "Diffuse thumbnail",
                "Match on this",
                "Opaque",
                "Assign…",
                "ready",
                "warning",
                "blocks export",
            ] {
                rows.extend(h.query_all_by_label(l).map(|n| n.rect()).filter(in_rows));
            }
            // The format, recipe and channel source combos and the fill values.
            for role in [Role::ComboBox, Role::SpinButton] {
                rows.extend(h.query_all_by_role(role).map(|n| n.rect()).filter(in_rows));
            }
            assert!(rows.len() > 6, "{at}: found the row widgets {rows:?}");
            for r in &rows {
                assert!(
                    r.max.y <= top,
                    "{at}: row widget {r:?} is not above the tabs and groups (top {top})"
                );
                assert!(
                    r.max.x <= centre.max.x,
                    "{at}: row widget {r:?} reaches past the centre {centre:?}"
                );
            }
            let left = slot_rect(&h, "Diffuse").min.x;
            assert!(
                (left - material.min.x).abs() <= 2.0,
                "{at}: rows start at {left}, the tabs at {}",
                material.min.x
            );
        }
    }
}

/// C14: the name box names the set; no label below it repeats the name.
#[test]
fn the_set_name_is_shown_once() {
    let d = tempfile::tempdir().unwrap();
    let (m, id) = rock_set(d.path());
    let mut h = harness(m, 1400.0);
    h.run();
    let centre = sdc_gui::ui::centre_rect(&h.ctx).expect("the centre is drawn");
    let labels: Vec<_> = h
        .query_all_by_label(&id)
        .filter(|n| n.accesskit_node().role() == Role::Label && centre.contains(n.rect().center()))
        .map(|n| n.rect())
        .collect();
    assert!(
        labels.is_empty(),
        "{id:?} repeated as a label at {labels:?}"
    );
}

/// C14: a feature's state shows once, in its own group's header ("off"); the Features group, whose ticks are the
/// state, has no count beside its title.
#[test]
fn the_features_group_has_no_header_value() {
    let d = tempfile::tempdir().unwrap();
    let (m, _) = rock_set(d.path());
    let mut h = harness(m, 1400.0);
    h.run();
    let t = h.get_by_label("Features").rect();
    let beside: Vec<_> = h
        .query_all_by_role(Role::Label)
        .filter(|n| n.rect().min.x > t.max.x && (n.rect().center().y - t.center().y).abs() < 2.0)
        .filter_map(|n| n.value())
        .collect();
    assert!(beside.is_empty(), "Features header value {beside:?}");
}

/// C14: the Assign… menu's header is the slot label, once ("Emissive / Skin (slot 3)", not "Slot 3: Emissive / Skin
/// (slot 3)").
#[test]
fn the_assign_menu_header_is_the_slot_label() {
    let d = tempfile::tempdir().unwrap();
    let (m, _) = rock_set(d.path());
    let mut h = harness(m, 1400.0);
    h.run();
    let row = slot_rect(&h, "Emissive / Skin (slot 3)");
    h.query_all_by_label("Assign…")
        .find(|n| (n.rect().center().y - row.center().y).abs() < 20.0)
        .expect("Assign… on the slot 3 row")
        .click();
    h.run();
    assert_eq!(
        h.query_all_by_label_contains("Slot 3:").count(),
        0,
        "the header repeats the slot number"
    );
    // The row's own label and the menu's header.
    assert_eq!(h.query_all_by_label("Emissive / Skin (slot 3)").count(), 2);
}
