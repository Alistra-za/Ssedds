//! Layout v2 Task 1.4 (B2; R4, R11): the Review tab in the bottom panel.
mod support;
use eframe::egui::{self, Key, Modifiers};
use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use sdc_gui::model::bottom::BottomTab;
use sdc_gui::model::{pbr_export, AppModel, Selection};
use sdc_gui::settings::GuiSettings;
use serde_json::json;
use std::time::Duration;
use support::*;

const ROCK: &str = "landscape/rock01.png";

fn harness(model: AppModel) -> Harness<'static, AppModel> {
    harness_dt(model, 0.25)
}

fn harness_dt(model: AppModel, dt: f32) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_step_dt(dt)
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(
            |ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model),
            model,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

fn settle(h: &mut Harness<'static, AppModel>) {
    assert!(h.state_mut().wait_idle(Duration::from_secs(120)));
    h.run();
}

/// Click `label` the way a pointer does: scroll it into view and settle on it first, because egui hit-tests against
/// the previous frame and a column scrolls inside the space above the footer.
fn press(h: &mut Harness<'static, AppModel>, label: &str) {
    h.get_by_label(label).scroll_to_me();
    h.run();
    h.get_by_label(label).hover();
    h.run();
    h.get_by_label(label).click();
    h.run();
}

fn disabled(h: &Harness<'static, AppModel>, label: &str) -> bool {
    h.get_by_label(label).accesskit_node().is_disabled()
}

/// `pbr_ready` with `key = value` on rock01, planned, and Export pressed.
fn reviewing(
    dir: &std::path::Path,
    edits: &[(&str, serde_json::Value)],
) -> Harness<'static, AppModel> {
    reviewing_dt(dir, edits, 0.25)
}

fn reviewing_dt(
    dir: &std::path::Path,
    edits: &[(&str, serde_json::Value)],
    dt: f32,
) -> Harness<'static, AppModel> {
    let mut m = pbr_ready(dir);
    for (k, v) in edits {
        m.set_json_value("landscape/rock01", k, Some(v.clone()));
    }
    assert!(m.wait_idle(Duration::from_secs(60)));
    let mut h = harness_dt(m, dt);
    h.run();
    press(&mut h, "Export");
    assert!(h.state().review.is_some());
    h
}

#[test]
fn export_opens_the_review_tab_and_no_modal() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened(d.path()));
    h.run();
    press(&mut h, "Export");
    h.get_by_label("Blockers");
    h.get_by_label("Undecided");
    h.get_by_label("Warnings");
    h.get_by_label("What will be written");
    assert!(
        h.query_by_label("Undecided textures").is_none(),
        "the old modal's heading"
    );
    assert!(h.state().dialog.is_none());
    // The primary button.
    h.get_by_label_contains("undecided)");
    h.get_by_label("Review");
}

#[test]
fn review_export_is_disabled_with_the_blocker_count() {
    let d = tempfile::tempdir().unwrap();
    let mut h = reviewing(d.path(), &[("emissive", json!(true))]);
    let text = h.state().review_button_text();
    assert!(disabled(&h, &text));
    h.get_by_label("Fix 1 blocker first");
    let rock_key = h
        .state()
        .sets
        .iter()
        .find(|s| s.key.ends_with("rock01"))
        .unwrap()
        .key
        .clone();
    press(&mut h, "Go to set");
    assert_eq!(h.state().selection, Selection::Set(rock_key));
    assert!(h.state().review.is_some(), "Review stays open while fixing");
}

#[test]
fn ticking_every_pitfall_enables_export() {
    let d = tempfile::tempdir().unwrap();
    let mut h = reviewing(d.path(), &[("alphaprop_thresh", json!(128))]);
    let text = h.state().review_button_text();
    assert!(disabled(&h, &text));
    h.get_by_label("Acknowledge 1 warning first");
    let asks = pbr_export::export_asks(h.state());
    assert_eq!(asks.len(), 1);
    press(&mut h, &asks[0].text);
    assert!(!disabled(&h, &text));
    press(&mut h, &text);
    settle(&mut h);
    assert!(h.state().export.summary.is_some());
}

/// R4, owner 2026-10-17. A6: both pitfalls are in rock01 (match8 to a missing `_s` path raises two).
#[test]
fn acknowledge_all_ticks_every_pitfall_in_one_click() {
    let d = tempfile::tempdir().unwrap();
    let mut h = reviewing(d.path(), &[("match8", json!(r"x\leaf_s.dds"))]);
    let text = h.state().review_button_text();
    assert!(disabled(&h, &text));
    h.get_by_label("Acknowledge 2 warnings first");
    let asks = pbr_export::export_asks(h.state());
    assert_eq!(asks.len(), 2);
    press(&mut h, "Acknowledge all");
    for a in &asks {
        assert_eq!(
            h.get_by_label(&a.text).accesskit_node().toggled(),
            Some(egui::accesskit::Toggled::True),
            "{}",
            a.text
        );
    }
    assert!(!disabled(&h, &text));
    // A pitfall added after the click is unticked. Another missing match path keeps the same two texts, so the new
    // one is the alpha-flags pitfall.
    h.state_mut()
        .set_json_value("landscape/rock01", "alphaprop_thresh", Some(json!(128)));
    settle(&mut h);
    h.get_by_label("Acknowledge 1 warning first");
}

#[test]
fn deciding_a_row_in_review_keeps_the_columns_usable() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened(d.path()));
    h.run();
    press(&mut h, "Export");
    // One ✓ per row that asks its sub-type, in list order.
    let with_check: Vec<String> = h
        .state()
        .undecided_keys()
        .into_iter()
        .filter(|k| h.state().subtype_question(k).is_some())
        .collect();
    let at = with_check.iter().position(|k| k == ROCK).unwrap();
    let before = h.query_all_by_label("\u{2713}").count();
    assert_eq!(before, with_check.len());
    let node = h.query_all_by_label("\u{2713}").nth(at).unwrap();
    node.hover();
    h.run();
    h.query_all_by_label("\u{2713}").nth(at).unwrap().click();
    h.run();
    settle(&mut h);
    assert!(!h.state().undecided_keys().contains(&ROCK.to_string()));
    assert_eq!(h.query_all_by_label("\u{2713}").count(), before - 1);
    // A Textures tile still takes a click while Review is open.
    h.get_by_label_contains("rock01_roughness.png").click();
    h.run();
    assert!(
        matches!(&h.state().selection, Selection::Item(k) if k.ends_with("rock01_roughness.png")),
        "{:?}",
        h.state().selection
    );
    assert!(h.state().review.is_some());
}

/// A7: Accept all is off, with the reason, when no listed texture has a suggestion.
#[test]
fn accept_all_suggestions_is_off_when_no_texture_has_one() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened(d.path()));
    h.run();
    press(&mut h, "Export");
    assert!(!disabled(&h, "Accept all suggestions"));
    press(&mut h, "Accept all suggestions");
    settle(&mut h);
    // What is left asks its alpha meaning or an acknowledgement, never a sub-type.
    assert!(!h.state().undecided_keys().is_empty());
    assert!(disabled(&h, "Accept all suggestions"));
    h.get_by_label("Accept all suggestions").hover();
    h.run();
    h.get_by_label("No undecided texture has a suggestion");
}

/// Review Focus 3.
#[test]
fn review_export_is_disabled_with_the_reason_while_read_only() {
    let d = tempfile::tempdir().unwrap();
    let root = mod_folder(d.path());
    std::fs::write(root.join("sdc.toml"), "target = [\n").unwrap();
    let settings = GuiSettings {
        export_folder: Some(d.path().join("export")),
        ..GuiSettings::default()
    };
    let mut m = AppModel::new(settings, None, session());
    m.accept = engine::policy::AcceptSuggestions::None;
    m.open_folder(root);
    assert!(m.wait_idle(Duration::from_secs(60)));
    let reason = m.review_gate().unwrap_err();
    assert!(reason.starts_with("sdc.toml is read-only: "), "{reason}");
    let mut h = harness(m);
    h.run();
    // The banner may say it too: Review adds one label beside its button.
    let without = h.query_all_by_label(&reason).count();
    h.state_mut().open_review();
    h.run();
    let text = h.state().review_button_text();
    assert!(disabled(&h, &text));
    assert_eq!(h.query_all_by_label(&reason).count(), without + 1);
}

#[test]
fn close_collapses_the_panel_and_drops_the_tab() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened(d.path()));
    h.run();
    press(&mut h, "Export");
    h.get_by_label("Review");
    press(&mut h, "Close");
    assert!(h.state().review.is_none() && !h.state().bottom.open);
    assert!(h.query_by_label("Review").is_none());
    // Opened on a tab it no longer lists, the panel shows Files, never an empty Review.
    h.state_mut().show_tab(BottomTab::Review);
    h.run();
    h.get_by_label("Failed only");
    assert!(h.query_by_label("What will be written").is_none());
}

#[test]
fn ctrl_k_opens_the_palette_while_review_is_open() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened(d.path()));
    h.run();
    press(&mut h, "Export");
    let fields = |h: &Harness<'static, AppModel>| h.query_all_by_role(Role::TextInput).count();
    let before = fields(&h);
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run();
    assert!(h.state().nav.palette.is_some(), "Ctrl+K opens the palette");
    h.run();
    assert_eq!(
        fields(&h),
        before + 1,
        "the palette's search field is drawn"
    );
    h.key_press(Key::Escape);
    h.run();
    assert!(h.state().nav.palette.is_none());
    assert!(h.state().review.is_some());
}

/// C4, R6: an edited PGPatcher JSON is listed under What will be written with Keep / Overwrite, none picked.
#[test]
fn the_edited_json_offers_keep_and_overwrite() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    export_through(&mut m);
    assert!(m.wait_idle(Duration::from_secs(120)));
    std::fs::write(pgp_json_path(&m), "{ \"edited\": true }\n").unwrap();
    let mut h = harness(m);
    h.run();
    press(&mut h, "Export");
    assert!(h.state().review.is_some());
    h.get_by_label_contains(": edited outside Tessalith");
    h.get_by_label("Keep the edited file");
    h.get_by_label("Overwrite it");
    assert_eq!(h.state().review.as_ref().unwrap().json_answer, None);
    press(&mut h, "Overwrite it");
    let a = h.state().review.as_ref().unwrap().json_answer.clone();
    assert!(a.is_some_and(|a| a.overwrite));
}

/// The clipped (clickable) rect of the widget labelled `label`, from the last pass.
fn hit_rect(h: &Harness<'static, AppModel>, label: &str) -> egui::Rect {
    let rect = h.get_by_label(label).rect();
    h.ctx.viewport(|v| {
        v.this_pass
            .widgets
            .layers()
            .flat_map(|(_, ws)| ws)
            .find(|w| w.rect == rect)
            .unwrap()
            .interact_rect
    })
}

/// Final review I1: the columns scroll inside the space above the footer, so no column widget can take a click
/// meant for Close, Export or the JSON choice.
#[test]
fn the_columns_stay_above_the_footer() {
    let d = tempfile::tempdir().unwrap();
    let mut h = reviewing(
        d.path(),
        &[
            ("match8", json!(r"x\leaf_s.dds")),
            ("alphaprop_thresh", json!(128)),
        ],
    );
    h.set_size(egui::vec2(1400.0, 700.0));
    h.run();
    let footer = h.get_by_label("What will be written").rect().top();
    let mut labels: Vec<String> = pbr_export::export_asks(h.state())
        .into_iter()
        .map(|a| a.text)
        .collect();
    labels.push("Acknowledge all".into());
    for l in &labels {
        let r = hit_rect(&h, l);
        assert!(
            !r.is_positive() || r.bottom() <= footer,
            "{l}: clickable to {} under the footer at {footer}",
            r.bottom()
        );
    }
}

/// The fill of the painted frame around `rect` (the Export button's).
fn fill_at(h: &Harness<'static, AppModel>, rect: egui::Rect) -> egui::Color32 {
    fn flat(s: &egui::Shape, out: &mut Vec<egui::Shape>) {
        match s {
            egui::Shape::Vec(v) => v.iter().for_each(|s| flat(s, out)),
            s => out.push(s.clone()),
        }
    }
    let mut all = vec![];
    for c in &h.output().shapes {
        flat(&c.shape, &mut all);
    }
    all.iter()
        .find_map(|s| match s {
            egui::Shape::Rect(r)
                if r.rect.expand(1.0).contains_rect(rect)
                    && rect.expand(1.0).contains_rect(r.rect) =>
            {
                Some(r.fill)
            }
            _ => None,
        })
        .expect("the Export button's frame")
}

/// Tick the only pitfall with one click and one frame, so the fade is caught at its start.
fn tick_once(h: &mut Harness<'static, AppModel>) {
    let asks = pbr_export::export_asks(h.state());
    h.get_by_label(&asks[0].text).scroll_to_me();
    h.run();
    h.get_by_label(&asks[0].text).hover();
    h.run();
    h.get_by_label(&asks[0].text).click();
    // The click takes a frame or two to reach the model; the next frame is the first the gate passes.
    for _ in 0..6 {
        h.step();
        if h.state().review_gate().is_ok() {
            break;
        }
    }
    h.step();
}

fn between(c: egui::Color32, a: egui::Color32, b: egui::Color32) -> bool {
    let ok = |x: u8, p: u8, q: u8| x > p.min(q) && x < p.max(q);
    c != a
        && c != b
        && (ok(c.r(), a.r(), b.r()) || ok(c.g(), a.g(), b.g()) || ok(c.b(), a.b(), b.b()))
}

/// Note CF11, Review Focus 3: Export fades to the accent when it becomes ready and snaps back at once (K11).
#[test]
fn export_fades_to_the_accent_and_snaps_back() {
    let d = tempfile::tempdir().unwrap();
    let mut h = reviewing_dt(d.path(), &[("alphaprop_thresh", json!(128))], 0.05);
    let text = h.state().review_button_text();
    let accent = h.ctx.global_style().visuals.selection.bg_fill;
    let rest = h.ctx.global_style().visuals.widgets.inactive.weak_bg_fill;
    let off = fill_at(&h, h.get_by_label(&text).rect());
    tick_once(&mut h);
    assert!(!disabled(&h, &text));
    let first = fill_at(&h, h.get_by_label(&text).rect());
    assert!(
        between(first, rest, accent),
        "{first:?} not between {rest:?} and {accent:?}"
    );
    for _ in 0..4 {
        h.step();
    }
    assert_eq!(fill_at(&h, h.get_by_label(&text).rect()), accent);
    // Untick: the gate fails, the state snaps to 0 and the disabled look returns.
    let asks = pbr_export::export_asks(h.state());
    h.get_by_label(&asks[0].text).click();
    h.step();
    h.step();
    assert!(disabled(&h, &text));
    assert_eq!(fill_at(&h, h.get_by_label(&text).rect()), off);
    // Ticking again fades again.
    tick_once(&mut h);
    let again = fill_at(&h, h.get_by_label(&text).rect());
    assert!(between(again, rest, accent), "{again:?}");
}

#[test]
fn reduced_motion_makes_the_export_accent_full_within_3_steps() {
    let d = tempfile::tempdir().unwrap();
    let mut h = reviewing_dt(d.path(), &[("alphaprop_thresh", json!(128))], 0.05);
    h.state_mut().settings.reduce_motion = true;
    let text = h.state().review_button_text();
    let accent = h.ctx.global_style().visuals.selection.bg_fill;
    tick_once(&mut h);
    h.step();
    h.step();
    assert_eq!(fill_at(&h, h.get_by_label(&text).rect()), accent);
}

/// Guard, green from the start (K11): the button is live on the frame the gate passes; only paint animates.
#[test]
fn the_review_export_works_on_the_frame_it_becomes_ready() {
    let d = tempfile::tempdir().unwrap();
    let mut h = reviewing_dt(d.path(), &[("alphaprop_thresh", json!(128))], 0.05);
    let text = h.state().review_button_text();
    tick_once(&mut h);
    h.get_by_label(&text).click();
    h.step();
    assert!(h.state().export.running.is_some() || h.state().export.summary.is_some());
}
