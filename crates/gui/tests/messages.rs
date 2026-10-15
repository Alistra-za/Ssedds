//! Task 14 (item 20, DECISIONS AG112-AG114): the toasts (fake clock), the Messages log and its action.
mod support;
use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use sdc_gui::model::messages::{Messages, Severity, FADE, FADE_WARN, LOG_CAP, MAX_SHOWN};
use sdc_gui::model::AppModel;
use sdc_gui::ui::actions;
use std::collections::HashSet;
use std::time::{Duration, Instant};

fn none() -> HashSet<u64> {
    HashSet::new()
}

fn secs(t0: Instant, s: f32) -> Instant {
    t0 + Duration::from_secs_f32(s)
}

fn store() -> Messages {
    let mut m = Messages::default();
    m.clock = || "12:00:00".into();
    m
}

#[test]
fn a_toast_fades_after_four_seconds_and_a_warning_after_eight() {
    let (mut m, t0) = (store(), Instant::now());
    m.notify(Severity::Info, "Saved");
    m.notify(Severity::Warning, "careful");
    assert!(m.tick(t0, &none()));
    assert_eq!(m.toasts().len(), 2);
    assert_eq!(FADE, Duration::from_secs(4));
    assert_eq!(FADE_WARN, Duration::from_secs(8));
    m.tick(secs(t0, 3.9), &none());
    assert_eq!(m.toasts().len(), 2);
    m.tick(secs(t0, 4.1), &none());
    assert_eq!(m.toasts().len(), 1);
    assert_eq!(m.toasts()[0].text, "careful");
    m.tick(secs(t0, 7.9), &none());
    assert_eq!(m.toasts().len(), 1);
    assert!(!m.tick(secs(t0, 8.1), &none()), "nothing left");
    assert!(m.toasts().is_empty());
}

#[test]
fn a_toast_that_waited_for_the_first_tick_still_gets_its_full_time() {
    let (mut m, t0) = (store(), Instant::now());
    m.notify(Severity::Info, "a");
    // The first frame comes long after the notice; the time counts from it.
    m.tick(secs(t0, 100.0), &none());
    m.tick(secs(t0, 103.0), &none());
    assert_eq!(m.toasts().len(), 1);
    m.tick(secs(t0, 104.5), &none());
    assert!(m.toasts().is_empty());
}

#[test]
fn at_most_three_are_on_screen_and_the_rest_queue() {
    let (mut m, t0) = (store(), Instant::now());
    for i in 0..5 {
        m.notify(Severity::Info, format!("n{i}"));
    }
    m.tick(t0, &none());
    assert_eq!(MAX_SHOWN, 3);
    assert_eq!(m.toasts().len(), 3);
    assert_eq!(m.queued(), 2);
    // The first three expire; the queued two come up with their full time.
    m.tick(secs(t0, 4.1), &none());
    let texts: Vec<&str> = m.toasts().iter().map(|t| t.text.as_str()).collect();
    assert_eq!(texts, ["n3", "n4"]);
    assert_eq!(m.queued(), 0);
    m.tick(secs(t0, 8.0), &none());
    assert_eq!(m.toasts().len(), 2, "3.9 s of 4 s used");
    m.tick(secs(t0, 8.2), &none());
    assert!(m.toasts().is_empty());
    assert_eq!(m.len(), 5, "the log kept all of them");
}

#[test]
fn hovering_a_toast_pauses_its_fade() {
    let (mut m, t0) = (store(), Instant::now());
    m.notify(Severity::Info, "one");
    m.notify(Severity::Info, "two");
    m.tick(t0, &none());
    let held: HashSet<u64> = [m.toasts()[0].id].into();
    m.tick(secs(t0, 3.0), &held);
    m.tick(secs(t0, 10.0), &held);
    let texts: Vec<&str> = m.toasts().iter().map(|t| t.text.as_str()).collect();
    assert_eq!(texts, ["one"], "the other one went; the held one waits");
    // Leaving it: the 4 s count on from where it stopped.
    m.tick(secs(t0, 13.0), &none());
    assert_eq!(m.toasts().len(), 1);
    m.tick(secs(t0, 14.1), &none());
    assert!(m.toasts().is_empty());
}

#[test]
fn the_fade_out_dims_the_last_half_second() {
    let (mut m, t0) = (store(), Instant::now());
    m.notify(Severity::Info, "x");
    m.tick(t0, &none());
    assert_eq!(m.toasts()[0].opacity(), 1.0);
    m.tick(secs(t0, 3.75), &none());
    let o = m.toasts()[0].opacity();
    assert!((o - 0.5).abs() < 0.01, "{o}");
}

#[test]
fn the_log_keeps_500_counts_unread_and_clears() {
    let mut m = store();
    for i in 0..LOG_CAP + 20 {
        m.log(Severity::Info, format!("m{i}"));
    }
    assert_eq!(m.len(), LOG_CAP);
    assert_eq!(m.entries().next().unwrap().text, "m20", "the oldest went");
    assert_eq!(m.entries().next().unwrap().time, "12:00:00");
    assert_eq!(m.unread(), LOG_CAP);
    m.set_open(true);
    assert_eq!(m.unread(), 0);
    m.log(Severity::Warning, "while open");
    assert_eq!(m.unread(), 0, "an open panel reads as it goes");
    m.set_open(false);
    m.log(Severity::Error, "closed");
    assert_eq!(m.unread(), 1);
    m.clear();
    assert_eq!((m.len(), m.unread()), (0, 0));
}

#[test]
fn a_log_only_note_raises_no_toast() {
    let mut m = store();
    m.log(Severity::Info, "quiet");
    assert!(m.toasts().is_empty());
    assert_eq!(m.len(), 1);
}

#[test]
fn ctrl_m_is_an_action_in_global_and_toggles_the_panel() {
    let a = actions::action("messages").expect("listed");
    assert_eq!(actions::keys_text("messages"), "Ctrl+M");
    assert_eq!(a.area, actions::Area::Global);
    let mut m = AppModel::new(
        sdc_gui::settings::GuiSettings::default(),
        None,
        support::session(),
    );
    actions::run("messages", &mut m, &egui::Context::default());
    assert!(m.messages.is_open());
    actions::run("messages", &mut m, &egui::Context::default());
    assert!(!m.messages.is_open());
}

#[test]
fn the_panel_lists_newest_first_and_clear_empties_it() {
    let mut m = AppModel::new(
        sdc_gui::settings::GuiSettings::default(),
        None,
        support::session(),
    );
    m.messages.log(Severity::Info, "first note");
    m.messages.log(Severity::Warning, "second note");
    m.messages.set_open(true);
    let mut h = Harness::builder()
        .with_size(egui::vec2(700.0, 500.0))
        .build_ui_state(
            |ui, m: &mut AppModel| sdc_gui::ui::messages::panel(ui.ctx(), m),
            m,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    let first = h.get_by_label("first note").rect().min.y;
    let second = h.get_by_label("second note").rect().min.y;
    assert!(second < first, "newest on top");
    h.get_by_label("2 messages");
    h.get_by_label("Clear").click();
    h.run();
    h.get_by_label("0 messages");
    assert!(h.query_by_label("first note").is_none());
    assert!(h.state().messages.is_empty());
}

#[test]
fn the_header_button_shows_the_unread_count_and_opens_the_panel() {
    let mut m = AppModel::new(
        sdc_gui::settings::GuiSettings::default(),
        None,
        support::session(),
    );
    m.messages.log(Severity::Info, "a");
    m.messages.log(Severity::Info, "b");
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 200.0))
        .build_ui_state(
            |ui, m: &mut AppModel| sdc_gui::ui::messages::button(ui, m),
            m,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    h.get_by_label("Messages (2)").click();
    h.run();
    assert!(h.state().messages.is_open());
    assert_eq!(h.state().messages.unread(), 0);
}

#[test]
fn toasts_draw_at_the_bottom_right_and_are_gone_after_their_time() {
    let mut m = AppModel::new(
        sdc_gui::settings::GuiSettings::default(),
        None,
        support::session(),
    );
    m.messages.notify(Severity::Info, "Saved");
    let t0 = Instant::now();
    let mut h = Harness::builder()
        .with_size(egui::vec2(800.0, 600.0))
        .build_ui_state(
            move |ui, (m, at): &mut (AppModel, f32)| {
                sdc_gui::ui::messages::draw_at(ui.ctx(), m, t0 + Duration::from_secs_f32(*at))
            },
            (m, 0.0),
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.step();
    let r = h.get_by_label("Saved").rect();
    assert!(r.max.x > 600.0 && r.max.y > 450.0, "{r:?}");
    h.state_mut().1 = 4.2;
    h.step();
    h.step();
    assert!(h.query_by_label("Saved").is_none());
}

#[test]
fn saved_undo_and_export_results_raise_toasts() {
    let d = tempfile::tempdir().unwrap();
    let mut m = support::opened(d.path());
    m.set_field(
        "landscape/rock01.png",
        "alpha.policy",
        serde_json::json!("remove"),
    );
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.save().unwrap();
    let toasts = |m: &AppModel| -> Vec<String> {
        m.messages.toasts().iter().map(|t| t.text.clone()).collect()
    };
    assert!(
        toasts(&m).contains(&"Saved".to_string()),
        "{:?}",
        toasts(&m)
    );
    m.undo();
    assert!(
        toasts(&m).iter().any(|t| t.starts_with("Undid: ")),
        "{:?}",
        toasts(&m)
    );
    let s = engine::api::BatchSummary {
        converted: 7,
        skipped: 2,
        failed: 1,
        ..Default::default()
    };
    assert_eq!(
        sdc_gui::model::export_finished_text(&s),
        "Export finished: 7 written, 2 skipped, 1 failed"
    );
}

#[test]
fn twenty_quick_undos_leave_one_queued_undo_toast_and_the_queue_is_capped() {
    let mut m = store();
    for i in 0..20 {
        m.notify(Severity::Info, format!("Undid: step {i}."));
    }
    assert_eq!(m.toasts().len(), MAX_SHOWN);
    assert_eq!(m.queued(), 1, "the newest replaced the waiting one");
    assert_eq!(m.len(), 20, "the log keeps every one");
    let mut m = store();
    for i in 0..30 {
        m.notify(Severity::Info, format!("kind{i}: x"));
    }
    assert_eq!(m.queued(), sdc_gui::model::messages::MAX_QUEUED);
}
