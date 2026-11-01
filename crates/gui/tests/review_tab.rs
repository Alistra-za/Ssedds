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

/// Note FB1: press the header's Export button, which reads "Review n files" or "Export n files".
fn press_export(h: &mut Harness<'static, AppModel>) {
    let label = header_export_label(h);
    press(h, &label);
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
    press_export(&mut h);
    assert!(h.state().review.is_some());
    h
}

#[test]
fn export_opens_the_review_tab_and_no_modal() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened(d.path()));
    h.run();
    press_export(&mut h);
    h.get_by_label_contains("Blockers (");
    h.get_by_label_contains("Undecided (");
    h.get_by_label_contains("Warnings (");
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

/// Review C9 (Note FB4): the headings carry their counts, and Warnings says what it does not block.
#[test]
fn the_review_headings_count_and_the_warnings_column_says_what_does_not_block() {
    use sdc_gui::model::bottom::{problems, Level};
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.open_review();
    // Note FB1: the missing export folder is a blocker, so it counts.
    let blockers =
        pbr_export::export_blocks(&m).len() + usize::from(m.export_folder_chosen().is_none());
    let undecided = m.undecided_keys().len();
    assert_eq!(pbr_export::export_asks(&m).len(), 0, "nothing asks");
    let others = problems(&m)
        .iter()
        .filter(|p| p.level == Level::Warning)
        .count();
    assert!(
        undecided >= 1 && others >= 1,
        "undecided {undecided}, others {others}"
    );
    let mut h = harness(m);
    h.run();
    h.get_by_label(&format!("Blockers ({blockers})"));
    h.get_by_label(&format!("Undecided ({undecided})"));
    h.get_by_label("Warnings (0)");
    h.get_by_label("None that block the export");
    let s = if others == 1 { "" } else { "s" };
    h.get_by_label(&format!("{others} warning{s} in Problems (do not block)"));
    assert!(h.query_by_label_contains("see Problems").is_none());
}

fn nif_row(blocked: Option<&str>) -> sdc_gui::model::meshes::NifReviewRow {
    sdc_gui::model::meshes::NifReviewRow {
        path: r"meshes\x.nif".into(),
        mod_name: "My Mod".into(),
        blocked: blocked.map(String::from),
        foreign: None,
        answer: None,
        compaction: None,
    }
}

/// Note F?7 (4b, O5): a NIF edit waiting only for the export folder is listed under its own footer heading, not warned.
#[test]
fn a_nif_edit_waiting_for_a_folder_is_listed_not_warned() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened_unchosen(d.path());
    assert!(m.export_folder_chosen().is_none());
    m.open_review();
    let blockers =
        pbr_export::export_blocks(&m).len() + usize::from(m.export_folder_chosen().is_none());
    m.set_nif_review_rows(vec![nif_row(Some("No export folder chosen"))]);
    let mut h = harness(m);
    h.run();
    h.get_by_label("NIF edits skipped until an export folder is chosen (1)");
    h.get_by_label(r"meshes\x.nif");
    assert!(h.query_by_label_contains("NIF edits (").is_none());
    h.get_by_label("Warnings (0)");
    h.get_by_label(&format!("Blockers ({blockers})"));
}

/// Note F?7 (4b): any other blocked reason stays a Warning (FC5).
#[test]
fn another_blocked_nif_reason_is_still_a_warning() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.open_review();
    m.set_nif_review_rows(vec![nif_row(Some("Resolve the conflicts first."))]);
    let mut h = harness(m);
    h.run();
    h.get_by_label("Warnings (1)");
    assert!(h
        .query_by_label_contains("until an export folder")
        .is_none());
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
    press_export(&mut h);
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
    // The Textures column still takes a click while Review is open (Note DY16: the raw roughness is a helper row now).
    h.query_all_by_role(eframe::egui::accesskit::Role::Button)
        .find(|n| {
            n.accesskit_node()
                .label()
                .is_some_and(|l| l.starts_with("Helper rock01_roughness.png:"))
        })
        .expect("the roughness helper row")
        .click();
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
    press_export(&mut h);
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

/// Note F?11 (A11): an undecided texture that only asks its alpha meaning is not accepted here; the hover says why.
#[test]
fn accept_all_explains_an_alpha_proposal() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    // The alpha question only follows a sub-type.
    m.decide(
        "landscape/leaf.png",
        sdc_gui::model::Answer::Subtype(engine::config::Subtype::Diffuse),
    );
    assert!(m.wait_idle(Duration::from_secs(60)));
    let mut h = harness(m);
    h.run();
    press_export(&mut h);
    press(&mut h, "Accept all suggestions");
    settle(&mut h);
    let asks_alpha = h.state().undecided_keys().iter().any(|k| {
        let it = h.state().item(k).unwrap();
        it.decisions
            .iter()
            .any(|d| matches!(d, sdc_gui::model::items::Decision::AlphaMeaning))
    });
    assert!(asks_alpha, "a texture still asks its alpha meaning");
    assert!(disabled(&h, "Accept all suggestions"));
    h.get_by_label("Accept all suggestions").hover();
    h.run();
    h.get_by_label("Only type suggestions are accepted here; an alpha meaning needs your answer");
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
    press_export(&mut h);
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
    press_export(&mut h);
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

/// C4, R6: an edited PGPatcher JSON is listed under What will be written; Note FC5: its Keep / Overwrite question is
/// a Blockers row, none picked, and counts until answered.
#[test]
fn the_edited_json_offers_keep_and_overwrite() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    export_through(&mut m);
    assert!(m.wait_idle(Duration::from_secs(120)));
    let p = pgp_json_path(&m);
    std::fs::write(&p, "{ \"edited\": true }\n").unwrap();
    let mut h = harness(m);
    h.run();
    press_export(&mut h);
    assert!(h.state().review.is_some());
    h.get_by_label_contains(": edited outside Tessalith");
    let name = p.file_name().unwrap().to_string_lossy().into_owned();
    h.get_by_label(&format!("{name} was edited outside Tessalith"));
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
    h.run_steps(4);
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

/// Final review M2: one step into the fade, the 0.12 s reduced fade (`cubic_out` 0.80) is nearer the accent than
/// the 0.15 s one (0.70) in an unreduced control, so the test fails if Reduce motion is ignored.
#[test]
fn reduced_motion_brings_the_export_accent_sooner_than_the_control() {
    let one_step_in = |reduce: bool| {
        let d = tempfile::tempdir().unwrap();
        let mut h = reviewing_dt(d.path(), &[("alphaprop_thresh", json!(128))], 0.05);
        h.state_mut().settings.reduce_motion = reduce;
        let text = h.state().review_button_text();
        tick_once(&mut h);
        let accent = h.ctx.global_style().visuals.selection.bg_fill;
        (fill_at(&h, h.get_by_label(&text).rect()), accent)
    };
    let ((reduced, accent), (control, _)) = (one_step_in(true), one_step_in(false));
    let dist = |c: egui::Color32| {
        c.r().abs_diff(accent.r()) as u32
            + c.g().abs_diff(accent.g()) as u32
            + c.b().abs_diff(accent.b()) as u32
    };
    assert!(dist(reduced) < dist(control), "{reduced:?} vs {control:?}");
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

fn review_at(dir: &std::path::Path, size: egui::Vec2) -> Harness<'static, AppModel> {
    let mut m = opened(dir);
    m.open_review();
    let mut h = Harness::builder()
        .with_size(size)
        .build_ui_state(|ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run_steps(8);
    h
}

/// Note FB10: Review opens at 40 % of the window's height while the user has not resized it.
#[test]
fn review_opens_at_least_40_percent_tall() {
    let d = tempfile::tempdir().unwrap();
    let h = review_at(d.path(), egui::vec2(1400.0, 864.0));
    let p = egui::containers::panel::PanelState::load(&h.ctx, egui::Id::new("bottom-review"))
        .expect("the Review panel")
        .outer_rect;
    assert!(p.height() >= 0.4 * 864.0 - 1.0, "{p:?}");
}

/// Note FB10: under 1200 px the columns stack in one scroll area; wider, they sit side by side.
#[test]
fn review_columns_stack_under_1200() {
    let d = tempfile::tempdir().unwrap();
    let pos = |h: &Harness<'static, AppModel>, l: &str| h.get_by_label_contains(l).rect();
    let h = review_at(d.path(), egui::vec2(1100.0, 864.0));
    let (b, u) = (pos(&h, "Blockers ("), pos(&h, "Undecided ("));
    assert!(
        u.top() > b.bottom() - 1.0 && u.left() <= b.left() + 1.0,
        "{b:?} {u:?}"
    );
    let h = review_at(d.path(), egui::vec2(1536.0, 864.0));
    let (b, u) = (pos(&h, "Blockers ("), pos(&h, "Undecided ("));
    assert!(
        u.left() > b.right() && (u.top() - b.top()).abs() < 2.0,
        "{b:?} {u:?}"
    );
}

/// Note FB10 round 1: the stack switch follows the window width: 1199 stacks, 1200 sits side by side.
#[test]
fn review_stacks_at_1199_and_not_at_1200() {
    let d = tempfile::tempdir().unwrap();
    let stacked = |w: f32| {
        let h = review_at(d.path(), egui::vec2(w, 864.0));
        let (b, u) = (
            h.get_by_label_contains("Blockers (").rect(),
            h.get_by_label_contains("Undecided (").rect(),
        );
        u.top() > b.bottom() - 1.0
    };
    assert!(stacked(1199.0));
    assert!(!stacked(1200.0));
}

/// Note FB10 round 1: a Review height the user set survives a visit to another tab.
#[test]
fn a_review_resize_sticks_across_a_tab_switch() {
    use egui::containers::panel::PanelState;
    let d = tempfile::tempdir().unwrap();
    let mut h = review_at(d.path(), egui::vec2(1400.0, 864.0));
    let id = egui::Id::new("bottom-review");
    let height =
        |h: &Harness<'static, AppModel>| PanelState::load(&h.ctx, id).unwrap().outer_rect.height();
    let before = height(&h);
    // Drag the panel's top edge down by 120 px.
    let edge = egui::pos2(
        700.0,
        PanelState::load(&h.ctx, id).unwrap().outer_rect.top(),
    );
    let to = edge + egui::vec2(0.0, 120.0);
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    h.event(egui::Event::PointerMoved(edge));
    h.run_steps(2);
    h.event(button(edge, true));
    h.run_steps(2);
    h.event(egui::Event::PointerMoved(to));
    h.run_steps(2);
    h.event(button(to, false));
    h.run_steps(4);
    let want = height(&h);
    assert!(want < before - 100.0, "dragged: {before} -> {want}");
    h.state_mut().show_tab(BottomTab::Problems);
    h.run_steps(4);
    h.state_mut().show_tab(BottomTab::Review);
    h.run_steps(4);
    assert!((height(&h) - want).abs() < 2.0, "{}", height(&h));
}

/// FC7: one Warning row per re-encode cause, with Acknowledge, project-wide in Problems; the export waits for the
/// ticks as for any other warning. A first export (no manifest) is a line, never an ask.
#[test]
fn review_shows_one_row_per_cause() {
    use engine::config::Subtype;
    use engine::manifest::{RecodeCause, RecodeWhy};
    use sdc_gui::model::bottom::{problems, At, Level};
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    // A fresh export folder: the plan says it is a first export, and nothing asks.
    let first = m.recode.clone();
    assert!(
        matches!(&first[..], [RecodeCause { why: RecodeWhy::FirstExport { .. }, count }] if *count > 0),
        "{first:?}"
    );
    assert!(pbr_export::export_asks(&m).is_empty());
    let normal = RecodeCause {
        why: RecodeWhy::EncodeVersion {
            subtype: Subtype::Normal,
            from: "1.0.0".into(),
            to: "1.1.0".into(),
        },
        count: 2,
    };
    let settings = RecodeCause {
        why: RecodeWhy::ProfileChanged {
            subtype: Subtype::Diffuse,
        },
        count: 3,
    };
    m.recode = [first.clone(), vec![normal, settings]].concat();
    let rows = [
        "2 files re-encode: the normal encoder changed in 1.1.0",
        "3 files re-encode: their settings changed",
    ];
    let asks: Vec<String> = pbr_export::export_asks(&m)
        .into_iter()
        .map(|a| a.text)
        .collect();
    assert_eq!(asks, rows);
    for r in rows {
        let p = problems(&m).iter().find(|p| p.text == r).cloned();
        let p = p.unwrap_or_else(|| panic!("{r} is not a Problems row"));
        assert_eq!((p.level, p.at), (Level::Warning, At::Project));
    }
    m.open_review();
    assert_eq!(
        m.review_gate(),
        Err("Acknowledge 2 warnings first".to_string())
    );
    let mut h = harness(m);
    h.run();
    h.get_by_label(&first[0].line());
    for r in rows {
        h.get_by_role_and_label(Role::CheckBox, r);
    }
    h.get_by_label_contains("Warnings (2");
    h.get_by_label("Acknowledge all").click();
    h.run();
    let acked = &h.state().review.as_ref().unwrap().acknowledged;
    assert!(rows.iter().all(|r| acked.contains(*r)), "{acked:?}");
    assert!(h
        .state()
        .review_gate()
        .err()
        .is_none_or(|e| !e.starts_with("Acknowledge")));
}

/// Note FC12 (supersedes DS26's "no warning"): a portrait photo read as a cube loses its top and bottom; Review says
/// which part is used, as a Warning to acknowledge before the export.
#[test]
fn portrait_face_is_a_row() {
    use engine::config::Subtype;
    use sdc_gui::model::Answer;
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("textures/sky");
    std::fs::create_dir_all(&r).unwrap();
    let night = d.path().join("textures/night");
    std::fs::create_dir_all(&night).unwrap();
    for dir in [&r, &night] {
        image::RgbImage::from_fn(64, 128, |x, y| image::Rgb([x as u8, y as u8, 40]))
            .save(dir.join("photo.png"))
            .unwrap();
    }
    let mut m = AppModel::new(GuiSettings::default(), None, session());
    m.accept = engine::policy::AcceptSuggestions::None;
    m.open_folder(d.path().join("textures"));
    assert!(m.wait_idle(Duration::from_secs(60)));
    choose_export(&mut m, d.path().join("export"));
    for key in ["sky/photo.png", "night/photo.png"] {
        m.decide(key, Answer::Subtype(Subtype::Cubemap));
        assert!(m.wait_idle(Duration::from_secs(60)));
        m.set_field(key, "cubemap.layout", json!("photo"));
        assert!(m.wait_idle(Duration::from_secs(60)));
    }
    const ROW: &str = "Only the middle 64×64 of sky/photo.png is used";
    const ROW2: &str = "Only the middle 64×64 of night/photo.png is used";
    let asks: Vec<String> = pbr_export::export_asks(&m)
        .into_iter()
        .map(|a| a.text)
        .collect();
    // Fix round 1: one row per texture path, so each same-named photo needs its own tick.
    assert!(
        asks.iter().any(|a| a == ROW) && asks.iter().any(|a| a == ROW2),
        "{asks:?}"
    );
    m.open_review();
    let mut h = harness(m);
    h.run();
    assert!(h
        .state()
        .review_gate()
        .is_err_and(|e| e.starts_with("Acknowledge")));
    h.get_by_role_and_label(Role::CheckBox, ROW).click();
    h.run();
    assert!(h
        .state()
        .review
        .as_ref()
        .unwrap()
        .acknowledged
        .contains(ROW));
    assert!(
        h.state()
            .review_gate()
            .is_err_and(|e| e.starts_with("Acknowledge")),
        "the other photo's row still waits"
    );
    // A landscape photo (wider than tall) is used whole: no row.
    let w = image::RgbImage::from_fn(128, 64, |x, y| image::Rgb([x as u8, y as u8, 40]));
    w.save(r.join("photo.png")).unwrap();
    w.save(night.join("photo.png")).unwrap();
    h.state_mut().replan();
    assert!(h.state_mut().wait_idle(Duration::from_secs(60)));
    assert!(pbr_export::export_asks(h.state())
        .iter()
        .all(|a| !a.text.starts_with("Only the middle")));
}

/// Note FC12 (supersedes CN45's skip-with-note): a zip over the ZIP format's limits is a Blockers row before the
/// export, from the engine's estimate of the planned outputs.
#[test]
fn zip_over_the_limit_is_a_blocker_row() {
    use engine::zip::{ZipEstimate, MAX_BYTES, MAX_ENTRIES};
    use sdc_gui::model::review::zip_block;
    let limits = (MAX_ENTRIES, MAX_BYTES);
    assert_eq!(
        zip_block(
            &ZipEstimate {
                entries: 70_000,
                bytes: 5 << 30
            },
            limits
        )
        .as_deref(),
        Some(
            "Zip too large for the ZIP format (70,000 files, 5.0 GiB); untick \"Also write a zip\" or split \
             the export"
        )
    );
    assert_eq!(
        zip_block(
            &ZipEstimate {
                entries: 3,
                bytes: 1 << 20
            },
            limits
        ),
        None
    );
    let at = |entries, bytes| zip_block(&ZipEstimate { entries, bytes }, limits).is_some();
    assert!(
        at(MAX_ENTRIES, 0) && at(0, MAX_BYTES),
        "at a limit is over it"
    );
    assert!(!at(MAX_ENTRIES - 1, MAX_BYTES - 1));
}
