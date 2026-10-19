//! Task 14 (item 20, DECISIONS AG112-AG114): the toasts (fake clock), the Messages log and its action.
mod support;
use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use sdc_gui::model::messages::{Messages, Severity, FADE, FADE_OUT, FADE_WARN, LOG_CAP, MAX_SHOWN};
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

/// Note CF2: the exit fade is 200 ms (it runs on egui time; see the toast paint tests below).
#[test]
fn the_exit_fade_is_200_ms() {
    assert_eq!(FADE_OUT, Duration::from_millis(200));
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
    assert!(m.bottom.open && m.messages.is_open());
    actions::run("messages", &mut m, &egui::Context::default());
    assert!(!m.bottom.open && !m.messages.is_open());
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
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::messages::tab(ui, m), m);
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

/// R14 (K9): the unread count is on the bottom panel's Messages tab (the header button is gone).
#[test]
fn the_messages_tab_shows_the_unread_count_and_opens_the_panel() {
    let mut m = AppModel::new(
        sdc_gui::settings::GuiSettings::default(),
        None,
        support::session(),
    );
    m.messages.log(Severity::Info, "a");
    m.messages.log(Severity::Info, "b");
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200.0, 700.0))
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    h.get_by_label("Messages (2)").click();
    h.run();
    assert!(h.state().bottom.open && h.state().messages.is_open());
    assert_eq!(h.state().messages.unread(), 0);
    h.get_by_label("Messages");
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

// ---- Note CF2: toasts fade whole, enter with a rise, and repaint only while moving

type ToastHarness = Harness<'static, (AppModel, f32)>;

/// A `draw_at` harness at 0.05 s steps; `at` is the model clock in seconds after `t0`.
fn toast_harness(first: &str) -> ToastHarness {
    let mut m = AppModel::new(
        sdc_gui::settings::GuiSettings::default(),
        None,
        support::session(),
    );
    m.messages.notify(Severity::Info, first);
    let t0 = Instant::now();
    let h = Harness::builder()
        .with_size(egui::vec2(800.0, 600.0))
        .with_step_dt(0.05)
        .build_ui_state(
            move |ui, (m, at): &mut (AppModel, f32)| {
                sdc_gui::ui::messages::draw_at(ui.ctx(), m, t0 + Duration::from_secs_f32(*at))
            },
            (m, 0.0),
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

/// The model's first tick at `at = 0`, then 10 steps at `at = 1.0`: the entry has settled.
fn settle(h: &mut ToastHarness) {
    h.step();
    h.state_mut().1 = 1.0;
    steps(h, 10);
}

fn steps(h: &mut ToastHarness, n: usize) {
    for _ in 0..n {
        h.step();
    }
}

/// What the last frame painted for the toast `text`.
#[derive(Debug, Clone, Copy)]
struct Paint {
    /// The popup frame's fill alpha (0-1).
    frame: f32,
    /// The text's alpha (0-1).
    text: f32,
    /// The text galley's top.
    y: f32,
    /// The text's painted rect.
    rect: egui::Rect,
}

fn paint(h: &ToastHarness, text: &str) -> Option<Paint> {
    fn walk<'a>(s: &'a egui::Shape, out: &mut Vec<&'a egui::Shape>) {
        match s {
            egui::Shape::Vec(v) => v.iter().for_each(|x| walk(x, out)),
            s => out.push(s),
        }
    }
    let mut all = vec![];
    for c in &h.output().shapes {
        walk(&c.shape, &mut all);
    }
    let t = all.iter().find_map(|s| match s {
        egui::Shape::Text(t) if t.galley.text() == text => Some(t),
        _ => None,
    })?;
    let rect = t.galley.rect.translate(t.pos.to_vec2());
    // A label's glyphs carry PLACEHOLDER; the painted colour is the fallback.
    let v = t.galley.rows[0].row.visuals.mesh.vertices[0].color;
    let text_a = if v == egui::Color32::PLACEHOLDER {
        t.fallback_color
    } else {
        v
    }
    .a() as f32
        / 255.0;
    // The popup frame: the smallest unblurred rect around the text (the shadow is blurred).
    let frame = all
        .iter()
        .filter_map(|s| match s {
            egui::Shape::Rect(r) if r.blur_width == 0.0 && r.rect.contains_rect(rect) => Some(r),
            _ => None,
        })
        .min_by(|a, b| a.rect.area().total_cmp(&b.rect.area()))?;
    Some(Paint {
        frame: frame.fill.a() as f32 / 255.0,
        text: text_a,
        y: t.pos.y,
        rect,
    })
}

/// K2: the last step asked for an immediate frame.
fn asks_for_a_frame(h: &ToastHarness) -> bool {
    h.output().viewport_output[&egui::ViewportId::ROOT].repaint_delay == Duration::ZERO
}

/// 01#1: the frame and its shadow fade with the text, not only the text.
#[test]
fn a_toast_fades_whole_over_its_last_200_ms() {
    let mut h = toast_harness("Saved");
    settle(&mut h);
    let rest = paint(&h, "Saved").expect("drawn");
    h.state_mut().1 = 3.85;
    steps(&mut h, 2);
    let p = paint(&h, "Saved").expect("still drawn");
    assert!(p.frame < rest.frame - 0.1, "{p:?} vs {rest:?}");
    let (f, t) = (p.frame / rest.frame, p.text / rest.text);
    assert!((f - t).abs() < 0.03, "frame x{f}, text x{t}");
}

/// "Saved"'s y below "Ready" (same Area). The anchored Area places itself from last frame's size, so on a new
/// toast's first frame the whole stack sits one toast low; measuring against the other toast removes that.
fn below_ready(h: &ToastHarness) -> f32 {
    paint(h, "Saved").expect("Saved drawn").y - paint(h, "Ready").expect("Ready drawn").y
}

/// K4: the first toast warms the Area, so the first frame of "Saved" is a visible one.
#[test]
fn a_new_toast_rises_8pt_and_fades_in() {
    let mut h = toast_harness("Ready");
    settle(&mut h);
    let rest_frame = paint(&h, "Ready").expect("drawn").frame;
    h.state_mut().0.messages.notify(Severity::Info, "Saved");
    h.step();
    let (first, first_dy) = (
        paint(&h, "Saved").expect("drawn on its first frame"),
        below_ready(&h),
    );
    assert!(asks_for_a_frame(&h), "the entry asks for frames");
    steps(&mut h, 4);
    let (five, five_dy) = (paint(&h, "Saved").unwrap(), below_ready(&h));
    steps(&mut h, 10);
    let rest_dy = below_ready(&h);
    eprintln!(
        "rise on frame 1: {} pt, alpha {first:?}",
        first_dy - rest_dy
    );
    assert!(first_dy >= rest_dy + 4.0, "{first_dy} vs {rest_dy}");
    assert!(first.frame < rest_frame, "{first:?}");
    assert!((five_dy - rest_dy).abs() <= 0.5, "{five_dy} vs {rest_dy}");
    assert_eq!(five.frame, rest_frame);
    assert_eq!(paint(&h, "Saved").unwrap().frame, rest_frame);
}

#[test]
fn a_resting_toast_asks_for_no_frames_until_its_fade() {
    let mut h = toast_harness("Saved");
    h.step();
    h.state_mut().1 = 0.25;
    steps(&mut h, 10);
    let d = h.output().viewport_output[&egui::ViewportId::ROOT].repaint_delay;
    assert!(d != Duration::ZERO, "{d:?}");
    assert!(
        (Duration::from_secs_f32(3.4)..=Duration::from_secs_f32(3.8)).contains(&d),
        "{d:?}"
    );
}

/// Review Focus 2: the entry runs on egui time, so a pointer resting on it (which freezes the model clock) does
/// not leave it half-visible.
#[test]
fn a_hovered_toast_reaches_full_opacity_during_its_entry() {
    let mut h = toast_harness("Ready");
    settle(&mut h);
    let rest_frame = paint(&h, "Ready").unwrap().frame;
    h.state_mut().0.messages.notify(Severity::Info, "Saved");
    h.step();
    let r = paint(&h, "Saved").unwrap().rect;
    h.hover_at(r.center());
    steps(&mut h, 5);
    assert_eq!(paint(&h, "Saved").unwrap().frame, rest_frame);
}

/// K14: the `|| hovered` target bites inside the exit window: a hovered toast fades back up and stays.
#[test]
fn a_hovered_toast_in_its_fade_comes_back_to_full_opacity() {
    let mut h = toast_harness("Saved");
    settle(&mut h);
    let rest = paint(&h, "Saved").unwrap();
    h.state_mut().1 = 3.9;
    h.step();
    assert!(
        paint(&h, "Saved").unwrap().frame < rest.frame,
        "the fade began"
    );
    h.hover_at(rest.rect.center());
    steps(&mut h, 5);
    assert_eq!(paint(&h, "Saved").expect("held").frame, rest.frame);
}

#[test]
fn reduced_motion_toasts_fade_without_rising() {
    let mut h = toast_harness("Ready");
    settle(&mut h);
    let rest_frame = paint(&h, "Ready").unwrap().frame;
    sdc_gui::ui::motion::set_reduce_motion(&h.ctx, true);
    h.state_mut().0.messages.notify(Severity::Info, "Saved");
    h.step();
    let (first, first_dy) = (paint(&h, "Saved").expect("drawn"), below_ready(&h));
    assert!(first.frame < rest_frame, "it still fades: {first:?}");
    steps(&mut h, 2);
    let three = paint(&h, "Saved").unwrap();
    steps(&mut h, 10);
    let rest_dy = below_ready(&h);
    assert!((first_dy - rest_dy).abs() <= 0.5, "{first_dy} vs {rest_dy}");
    assert_eq!(three.frame, rest_frame);
}

/// Review Focus 5. From 0.75 of the way in, 2 steps finish a 0.12 s fade but not a 0.2 s one (alpha 0.0625).
#[test]
fn turning_on_reduce_motion_mid_fade_ends_the_fade_sooner() {
    let mut h = toast_harness("Saved");
    settle(&mut h);
    h.state_mut().1 = 3.9;
    h.step();
    sdc_gui::ui::motion::set_reduce_motion(&h.ctx, true);
    steps(&mut h, 2);
    let a = paint(&h, "Saved").map_or(0.0, |p| p.frame);
    assert_eq!(a, 0.0);
    h.state_mut().1 = 4.2;
    steps(&mut h, 2);
    assert!(h.query_by_label("Saved").is_none());
}
