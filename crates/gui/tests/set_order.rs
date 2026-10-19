//! Note CF4-CF6: every set card shows its place in the full order, and "Move to…" moves a set by number or name,
//! also while a Sets filter is on (owner decision a, b; R2, R3).
mod support;
use eframe::egui::{
    self,
    accesskit::{Role, Toggled},
    Key,
};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use sdc_gui::model::sets::views;
use sdc_gui::model::AppModel;
use support::*;

fn harness(m: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(egui::vec2(1500.0, 900.0))
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

fn order_text(h: &Harness<'static, AppModel>) -> String {
    h.state().sets_doc.as_ref().unwrap().text()
}

/// The set ids in the full order.
fn ids(m: &AppModel) -> Vec<String> {
    views(m).iter().map(|v| v.id.clone()).collect()
}

/// The fixture's default order: the search "o" hides leaf (position 1) only.
const LEAF_ODD_ROCK: [&str; 3] = ["landscape/leaf", "landscape/odd", "landscape/rock01"];

fn opened_checked(dir: &std::path::Path) -> AppModel {
    let m = opened(dir);
    assert_eq!(ids(&m), LEAF_ODD_ROCK, "the fixture's order");
    m
}

/// The number labels in the Sets column.
fn positions(h: &Harness<'static, AppModel>) -> Vec<String> {
    h.query_all_by_label_contains("Position ")
        .filter(|n| in_sets_column(h, n.rect()))
        // A Label's text is its AccessKit value.
        .filter_map(|n| n.accesskit_node().value())
        .collect()
}

/// Right-click set `id`'s card and click "Move to…", then step once so the dialog's field takes the focus.
fn open_move_to(h: &mut Harness<'static, AppModel>, id: &str) {
    let name = h
        .query_all_by_label(id)
        .find(|n| in_sets_column(h, n.rect()))
        .unwrap_or_else(|| panic!("{id}'s card"))
        .rect();
    h.hover_at(name.center());
    h.run();
    h.query_all_by_label(id)
        .find(|n| in_sets_column(h, n.rect()))
        .unwrap()
        .click_secondary();
    h.run();
    h.get_by_label("Move to…").click();
    h.run();
}

/// K14: the dialog's field, not the Sets search box (also a TextInput), found by its accessible name (M5).
fn field<'a>(h: &'a Harness<'static, AppModel>) -> egui_kittest::Node<'a> {
    let f = h.get_by_role_and_label(Role::TextInput, "Position or set name");
    assert!(f.accesskit_node().is_focused(), "the field has the focus");
    f
}

/// The fixture plus nine created sets: 12 in all, one named "extra/armor_13" (a number in a name).
fn twelve_sets(dir: &std::path::Path) -> AppModel {
    let mut m = opened_checked(dir);
    for i in 4..=12 {
        let id = if i == 12 {
            "extra/armor_13".to_string()
        } else {
            format!("extra/s{i:02}")
        };
        m.edit_docs("t", None, true, move |_, s| s.create_set(&id));
    }
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    assert_eq!(ids(&m).len(), 12);
    m
}

fn type_query(h: &mut Harness<'static, AppModel>, text: &str) {
    field(h).type_text(text);
    h.run();
}

fn dialog_open(h: &Harness<'static, AppModel>) -> bool {
    h.query_by_label("Move landscape/rock01 to…").is_some()
}

/// Enter in the field, then steps (a moved card flashes, so `run` would not settle).
fn enter(h: &mut Harness<'static, AppModel>) {
    h.key_press(Key::Enter);
    h.run_steps(3);
}

#[test]
fn move_to_opens_a_dialog_that_takes_typing() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened_checked(d.path()));
    h.run();
    let before = order_text(&h);
    let name = h
        .query_all_by_label("landscape/rock01")
        .find(|n| in_sets_column(&h, n.rect()))
        .unwrap()
        .rect();
    h.hover_at(name.center());
    h.run();
    h.query_all_by_label("landscape/rock01")
        .find(|n| in_sets_column(&h, n.rect()))
        .unwrap()
        .click_secondary();
    h.run();
    // K14: the click on "Move to…" and an Enter in one frame: the Enter must not answer the dialog it opened (BY2).
    let at = h.get_by_label("Move to…").rect().center();
    h.hover_at(at);
    h.step();
    for pressed in [true, false] {
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        });
    }
    h.input_mut().events.push(egui::Event::Key {
        key: Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    });
    h.step();
    h.run();
    assert!(
        h.query_by_label("Move up").is_none(),
        "the context menu closed"
    );
    assert!(dialog_open(&h), "the dialog is open");
    assert_eq!(
        order_text(&h),
        before,
        "the opening frame's Enter moved nothing"
    );
    type_query(&mut h, "1");
    assert_eq!(field(&h).value().as_deref(), Some("1"));
    assert!(dialog_open(&h));
    assert_eq!(order_text(&h), before);
    assert!(!h.state().is_dirty());
}

#[test]
fn each_card_shows_its_position_in_the_full_order_while_filtered() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened_checked(d.path()));
    h.run();
    assert_eq!(
        positions(&h),
        ["Position 1 of 3", "Position 2 of 3", "Position 3 of 3"]
    );
    h.state_mut().sets_filter.search = "o".into();
    h.run();
    assert_eq!(positions(&h), ["Position 2 of 3", "Position 3 of 3"]);
}

#[test]
fn move_to_by_number_moves_in_the_full_order_while_filtered() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened_checked(d.path());
    let rock = views(&m)[2].key.clone();
    m.select_set(&rock);
    m.sets_filter.search = "o".into();
    let mut h = harness(m);
    h.run();
    let sel = h.state().selected_sets();
    open_move_to(&mut h, "landscape/rock01");
    assert_eq!(
        h.query_all_by_label("(hidden by filter)").count(),
        1,
        "leaf is listed and marked hidden"
    );
    type_query(&mut h, "1");
    enter(&mut h);
    assert!(!dialog_open(&h), "a pick closes the dialog");
    assert_eq!(
        ids(h.state()),
        ["landscape/rock01", "landscape/leaf", "landscape/odd"]
    );
    assert_eq!(h.state().sets_filter.search, "o", "the search is unchanged");
    assert_eq!(h.state().selected_sets(), sel, "the selection is unchanged");
    assert!(h.state().is_dirty());
    h.key_press_modifiers(egui::Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    assert_eq!(ids(h.state()), LEAF_ODD_ROCK, "Ctrl+Z restores the order");
}

#[test]
fn move_to_by_name_takes_the_targets_position() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened_checked(d.path()));
    h.run();
    // Up: rock01 takes leaf's place, position 1.
    open_move_to(&mut h, "landscape/rock01");
    type_query(&mut h, "LEAF");
    enter(&mut h);
    assert_eq!(
        ids(h.state()),
        ["landscape/rock01", "landscape/leaf", "landscape/odd"]
    );
    // Down: rock01 takes odd's place, position 3, and odd shifts up by one (the `reorder_set` rule).
    open_move_to(&mut h, "landscape/rock01");
    type_query(&mut h, "odd");
    enter(&mut h);
    assert_eq!(
        ids(h.state()),
        ["landscape/leaf", "landscape/odd", "landscape/rock01"]
    );
}

#[test]
fn move_to_out_of_range_or_no_match_moves_nothing() {
    // Review Focus 4.
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened_checked(d.path()));
    h.run();
    let before = order_text(&h);
    // I1: digits are a position only, never a name ("0" would match the 0 in "rock01"). Out of range: no row, the
    // hint, and Enter does nothing, so the dialog stays open.
    open_move_to(&mut h, "landscape/rock01");
    for (q, hint, other) in [
        ("0", "Positions are 1 to 3", "Nothing matches"),
        ("00", "Positions are 1 to 3", "Nothing matches"),
        ("99", "Positions are 1 to 3", "Nothing matches"),
        ("zzz", "Nothing matches", "Positions are 1 to 3"),
    ] {
        h.key_press_modifiers(egui::Modifiers::COMMAND, Key::A);
        h.run();
        type_query(&mut h, q);
        assert_eq!(field(&h).value().as_deref(), Some(q));
        assert!(h.query_by_label(hint).is_some(), "{q:?} shows {hint:?}");
        assert!(h.query_by_label(other).is_none(), "{q:?} hides {other:?}");
        let rows = h.query_all_by_label_contains("  landscape/").count();
        assert_eq!(rows, 0, "{q:?}: no row listed");
        enter(&mut h);
        assert_eq!(order_text(&h), before, "{q:?} + Enter moves nothing");
        assert!(!h.state().is_dirty(), "{q:?}");
        assert!(dialog_open(&h), "{q:?}: the dialog stays open after Enter");
    }
    // A name that matches several sets highlights the first in the full order (leaf, position 1).
    h.key_press_modifiers(egui::Modifiers::COMMAND, Key::A);
    h.run();
    type_query(&mut h, "landscape");
    let first = h
        .query_all_by_label_contains("  landscape/")
        .find(|n| n.accesskit_node().toggled() == Some(Toggled::True))
        .and_then(|n| n.accesskit_node().label());
    assert_eq!(first.as_deref(), Some("1  landscape/leaf"), "the highlight");
    assert!(h.query_by_label("Nothing matches").is_none());
    enter(&mut h);
    assert_eq!(
        ids(h.state()),
        ["landscape/rock01", "landscape/leaf", "landscape/odd"]
    );
}

#[test]
fn a_number_past_the_end_never_picks_a_set_named_with_it() {
    // I1, review failure case 2: "13" with 12 sets, one named armor_13.
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(twelve_sets(d.path()));
    h.run();
    let before = ids(h.state());
    // The created sets sort first: s04 is position 2, on screen.
    assert_eq!(before[1], "extra/s04");
    let rows_with_armor = |h: &Harness<'static, AppModel>| {
        h.query_all_by_label_contains("armor_13")
            .filter(|n| !in_sets_column(h, n.rect()))
            .count()
    };
    open_move_to(&mut h, "extra/s04");
    assert_eq!(rows_with_armor(&h), 1, "the control: blank lists armor_13");
    type_query(&mut h, "13");
    assert!(h.query_by_label("Positions are 1 to 12").is_some());
    assert_eq!(rows_with_armor(&h), 0, "no name row for 13");
    enter(&mut h);
    assert_eq!(ids(h.state()), before, "nothing moved");
    assert!(
        h.query_by_label("Move extra/s04 to…").is_some(),
        "the dialog stays open"
    );
}

#[test]
fn the_number_slot_has_one_width_for_one_and_two_digits() {
    // M2: N = 12, so "1" and "12" share a slot and the names line up.
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(twelve_sets(d.path()));
    h.run();
    // Every card, also those scrolled below the panel (AccessKit rects are not clipped).
    let widths: Vec<f32> = h
        .query_all_by_label_contains("Position ")
        .map(|n| n.rect().width())
        .collect();
    assert_eq!(widths.len(), 12, "{widths:?}");
    assert!(
        widths.iter().all(|w| (w - widths[0]).abs() < 0.01),
        "one slot width: {widths:?}"
    );
    let one = h.get_by_label("Position 1 of 12").rect();
    let two = h.get_by_label("Position 12 of 12").rect();
    assert!(
        (one.right() - two.right()).abs() < 0.01,
        "right edges line up"
    );
}

#[test]
fn esc_closes_move_to_with_no_change() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened_checked(d.path()));
    h.run();
    let before = order_text(&h);
    open_move_to(&mut h, "landscape/rock01");
    type_query(&mut h, "1");
    h.key_press(Key::Escape);
    h.run();
    assert!(!dialog_open(&h), "Esc closes the dialog");
    assert_eq!(order_text(&h), before);
    assert!(!h.state().is_dirty());
}

#[test]
fn ctrl_z_does_nothing_while_move_to_is_open() {
    // `actions::keys` holds the global keys while a Sets dialog is pending.
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened_checked(d.path()));
    h.run();
    open_move_to(&mut h, "landscape/rock01");
    type_query(&mut h, "1");
    enter(&mut h);
    let moved = ["landscape/rock01", "landscape/leaf", "landscape/odd"];
    assert_eq!(ids(h.state()), moved);
    open_move_to(&mut h, "landscape/rock01");
    h.key_press_modifiers(egui::Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    assert_eq!(
        ids(h.state()),
        moved,
        "Ctrl+Z is held while the dialog is open"
    );
    assert!(dialog_open(&h));
    // The control: closed, the same Ctrl+Z undoes the move.
    h.key_press(Key::Escape);
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    assert_eq!(ids(h.state()), LEAF_ODD_ROCK);
}

#[test]
fn move_to_is_off_on_a_read_only_project() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened_checked(d.path());
    let root = m.root.clone().unwrap();
    std::fs::write(root.join("sdc-project.toml"), "[[sets]\nbroken").unwrap();
    m.reload_documents();
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    assert!(m.sets_doc.as_ref().unwrap().read_only());
    let mut h = harness(m);
    h.run();
    let name = h
        .query_all_by_label("landscape/rock01")
        .find(|n| in_sets_column(&h, n.rect()))
        .unwrap()
        .rect();
    h.hover_at(name.center());
    h.run();
    h.query_all_by_label("landscape/rock01")
        .find(|n| in_sets_column(&h, n.rect()))
        .unwrap()
        .click_secondary();
    h.run();
    assert!(h.get_by_label("Move to…").accesskit_node().is_disabled());
}
