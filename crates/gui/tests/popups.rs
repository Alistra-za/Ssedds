//! AG125: a popup that holds a filter box or a list stays open while it is used, and closes on a click outside, Esc
//! or a pick.
mod support;
use eframe::egui;
use egui_kittest::kittest::{by, Queryable};
use egui_kittest::Harness;
use sdc_gui::model::pbr_export::json_entries;
use sdc_gui::model::AppModel;
use sdc_gui::ui::State;
use support::*;

type H = Harness<'static, (AppModel, State)>;

const HEADING: &str = "Read this set's textures as";

fn harness() -> (tempfile::TempDir, H) {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    let key = json_entries(&m)
        .into_iter()
        .find(|(v, _)| v.id == "landscape/rock01")
        .unwrap()
        .0
        .key;
    m.select_set(&key);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1600.0, 900.0))
        .build_ui_state(
            |ui, (m, s): &mut (AppModel, State)| sdc_gui::ui::draw_with(ui, m, s),
            (m, State::default()),
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    h.get_by_label(sdc_gui::ui::input_name::DROPDOWN).click();
    h.run();
    h.get_by_label(HEADING);
    (d, h)
}

fn filter_box(h: &H) -> egui::Rect {
    h.get(by().predicate(|n| n.placeholder() == Some("Filter: words of the path")))
        .rect()
}

fn filter_box_or_value(h: &H) -> egui::Pos2 {
    h.get(by().role(egui::accesskit::Role::TextInput).value("rock"))
        .rect()
        .center()
}

fn click_at(h: &mut H, p: egui::Pos2) {
    for pressed in [true, false] {
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    h.run();
}

#[test]
fn typing_in_the_input_name_filter_keeps_the_popup_open() {
    let (_d, mut h) = harness();
    let r = filter_box(&h);
    click_at(&mut h, r.center());
    h.get_by_label(HEADING);
    h.input_mut().events.push(egui::Event::Text("rock".into()));
    h.run();
    h.get_by_label(HEADING);
    // The typed text reached the box, which now lists only the matches.
    h.get(by().role(egui::accesskit::Role::TextInput).value("rock"));
    // A click on the popup's own text, not a control, keeps it too.
    let heading = h.get_by_label(HEADING).rect().center();
    click_at(&mut h, heading);
    h.get_by_label(HEADING);
    // Esc with the box focused closes it as well.
    let r = filter_box_or_value(&h);
    click_at(&mut h, r);
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.query_by_label(HEADING).is_none(), "Esc in the box");
}

#[test]
fn a_click_outside_or_esc_closes_the_input_name_popup() {
    let (_d, mut h) = harness();
    // A spot on the background (not on a slot card, whose click opens its own window).
    let outside = egui::pos2(1500.0, 300.0);
    assert_eq!(
        h.ctx.layer_id_at(outside).map(|l| l.order),
        Some(egui::Order::Background)
    );
    click_at(&mut h, outside);
    assert!(h.query_by_label(HEADING).is_none(), "click outside");
    h.get_by_label(sdc_gui::ui::input_name::DROPDOWN).click();
    h.run();
    h.get_by_label(HEADING);
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.query_by_label(HEADING).is_none(), "Esc");
}
