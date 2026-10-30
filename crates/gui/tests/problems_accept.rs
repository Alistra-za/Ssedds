//! Owner amendment A (Note FB9): a Problems row with a suggestion accepts it in one click, Enter or Space; ▾ opens the
//! type picker; "Accept all suggestions (n)" sits at the top. Every accept edits the in-memory document only.
mod support;
use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use engine::config::Subtype;
use sdc_gui::model::bottom::{filtered, problems, At, Level, Problem, ProblemsFilter};
use sdc_gui::model::{Answer, AppModel};
use sdc_gui::ui::problems::row_id;
use serde_json::json;
use std::time::Duration;
use support::*;

/// A lone `_s` texture: no PBR evidence, so the engine suggests subsurface tint (Low), and its key sorts last.
const TINT: &str = "skin/x_s.png";

fn harness(m: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

fn idle(h: &mut Harness<'static, AppModel>) {
    h.run();
    assert!(h.state_mut().wait_idle(Duration::from_secs(60)));
    h.run();
}

/// `opened` plus `skin/x_s.png`, Problems open on the undecided rows.
fn with_tint(dir: &std::path::Path) -> AppModel {
    let skin = dir.join("textures/skin");
    std::fs::create_dir_all(&skin).unwrap();
    image::ImageBuffer::from_fn(64, 64, |x, y| {
        image::Rgb([(x * 4) as u8, (y * 4) as u8, 60u8])
    })
    .save(skin.join("x_s.png"))
    .unwrap();
    let mut m = opened(dir);
    assert_eq!(
        m.subtype_question(TINT),
        Some(Some(Subtype::SubsurfaceTint))
    );
    m.open_problems(Some(ProblemsFilter::Undecided));
    m
}

fn subtype(m: &AppModel, key: &str) -> Option<serde_json::Value> {
    m.project.as_ref().unwrap().file_value(key, "subtype")
}

fn decide_row(m: &AppModel, key: &str) -> bool {
    problems(m)
        .iter()
        .any(|p| p.level == Level::Decide && p.at == At::Texture(key.into()))
}

fn rows(m: &AppModel) -> Vec<Problem> {
    filtered(m, Some(&ProblemsFilter::Undecided)).to_vec()
}

fn key_of(p: &Problem) -> &str {
    match &p.at {
        At::Texture(k) => k,
        other => panic!("{other:?}"),
    }
}

/// Focus the row of `key` as a keyboard user would land on it (AccessKit focus).
fn focus_row(h: &mut Harness<'static, AppModel>, key: &str) {
    let p = rows(h.state())
        .into_iter()
        .find(|p| key_of(p) == key)
        .unwrap();
    h.ctx.memory_mut(|m| m.request_focus(row_id(&p)));
    h.run();
    assert_eq!(h.ctx.memory(|m| m.focused()), Some(row_id(&p)), "{key}");
}

fn focused_row(h: &Harness<'static, AppModel>) -> Option<String> {
    let f = h.ctx.memory(|m| m.focused())?;
    rows(h.state())
        .into_iter()
        .find(|p| row_id(p) == f)
        .map(|p| key_of(&p).to_string())
}

#[test]
fn one_click_accepts_the_suggestion() {
    let d = tempfile::tempdir().unwrap();
    let m = with_tint(d.path());
    let toml = d.path().join("textures/sdc.toml");
    let on_disk = std::fs::read_to_string(&toml).ok();
    let mut h = harness(m);
    h.run();
    h.get_by_label("Set type: subsurface tint ✓").click();
    idle(&mut h);
    assert_eq!(subtype(h.state(), TINT), Some(json!("subsurface_tint")));
    assert!(!decide_row(h.state(), TINT), "the row leaves the list");
    assert!(h.query_by_label("Set type: subsurface tint ✓").is_none());
    // One undo step, in memory: the file waits for Save.
    assert!(h.state().undo_label().is_some());
    assert!(h.state().is_dirty());
    assert_eq!(std::fs::read_to_string(&toml).ok(), on_disk);
}

#[test]
fn the_arrow_opens_the_picker_for_another_type() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(with_tint(d.path()));
    h.run();
    let main = h.get_by_label("Set type: subsurface tint ✓").rect();
    // ▾ is named "Set type…": it is what the plain button was, the picker.
    let arrow = h
        .query_all_by_label("Set type…")
        .find(|n| (n.rect().center().y - main.center().y).abs() < 2.0)
        .expect("the row's ▾");
    assert!(
        arrow.rect().left() >= main.right() - 1.0,
        "▾ follows the suggestion"
    );
    arrow.click();
    h.run();
    h.run();
    h.get_by_label("subsurface tint (suggested)");
    // The list scrolls: narrow it so the entry is on screen, as a user would.
    h.query_all_by_role(egui::accesskit::Role::TextInput)
        .find(|n| n.is_focused())
        .expect("the search box has focus")
        .type_text("back");
    h.run();
    h.get_by_label("backlight tint").click();
    idle(&mut h);
    assert_eq!(subtype(h.state(), TINT), Some(json!("backlight_tint")));
}

#[test]
fn accept_all_at_the_top_shows_the_count() {
    let d = tempfile::tempdir().unwrap();
    let m = with_tint(d.path());
    let n = m
        .undecided_keys()
        .iter()
        .filter(|k| matches!(m.subtype_question(k), Some(Some(_))))
        .count();
    assert!(n >= 2, "{n}");
    let toml = d.path().join("textures/sdc.toml");
    let on_disk = std::fs::read(&toml).ok();
    let mut h = harness(m);
    h.run();
    let all = h
        .get_by_label(&format!("Accept all suggestions ({n})"))
        .rect();
    for b in h.query_all_by_label_contains("Set type: ") {
        assert!(
            all.bottom() <= b.rect().top(),
            "{all:?} not above {:?}",
            b.rect()
        );
    }
    h.get_by_label(&format!("Accept all suggestions ({n})"))
        .click();
    idle(&mut h);
    let m = h.state();
    assert!(m
        .undecided_keys()
        .iter()
        .all(|k| !matches!(m.subtype_question(k), Some(Some(_)))));
    assert_eq!(m.undo_label(), Some("Accept all suggestions"));
    assert_eq!(subtype(m, TINT), Some(json!("subsurface_tint")));
    assert!(h
        .query_by_label_contains("Accept all suggestions")
        .is_none());
    assert!(m.is_dirty());
    assert_eq!(std::fs::read(&toml).ok(), on_disk, "only Save writes");
}

/// Review C7 round 1: with a texture selected, Enter on a focused row accepts it and does not also open the
/// preview (the Grid's Enter); the file waits for Save.
#[test]
fn enter_on_a_row_does_not_open_the_preview() {
    let d = tempfile::tempdir().unwrap();
    let mut m = with_tint(d.path());
    m.select_item("landscape/rock01.png");
    let toml = d.path().join("textures/sdc.toml");
    let on_disk = std::fs::read(&toml).ok();
    let mut h = harness(m);
    h.run();
    focus_row(&mut h, TINT);
    h.key_press(egui::Key::Enter);
    h.run();
    idle(&mut h);
    assert_eq!(subtype(h.state(), TINT), Some(json!("subsurface_tint")));
    assert_eq!(h.state().expanded, None, "Enter opened the preview");
    assert_eq!(std::fs::read(&toml).ok(), on_disk, "only Save writes");
}

/// Review C7 round 1: leaving the tab forgets the focused row, so a row answered meanwhile does not pull focus into
/// the list on the way back.
#[test]
fn coming_back_to_problems_does_not_take_focus() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(with_tint(d.path()));
    h.run();
    focus_row(&mut h, TINT);
    h.state_mut()
        .toggle_tab(sdc_gui::model::bottom::BottomTab::Messages);
    h.run();
    h.state_mut()
        .decide(TINT, Answer::Subtype(Subtype::SubsurfaceTint));
    idle(&mut h);
    h.state_mut().open_problems(Some(ProblemsFilter::Undecided));
    h.run();
    h.run();
    assert_eq!(focused_row(&h), None);
}

/// Review C7 round 1: Tab reaches a Problems row, as Note FB9 says.
#[test]
fn tab_reaches_a_problems_row() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(with_tint(d.path()));
    h.run();
    let mut reached = false;
    for _ in 0..200 {
        h.key_press(egui::Key::Tab);
        h.run();
        if focused_row(&h).is_some() {
            reached = true;
            break;
        }
    }
    assert!(reached, "Tab never focused a row");
}

#[test]
fn enter_accepts_and_moves_to_the_next_row() {
    let d = tempfile::tempdir().unwrap();
    let m = with_tint(d.path());
    let list = rows(&m);
    let i = list
        .iter()
        .position(|p| p.suggested.is_some())
        .expect("a row with a suggestion");
    assert!(i + 2 < list.len(), "{list:?}");
    let (first, second, third) = (
        key_of(&list[i]).to_string(),
        key_of(&list[i + 1]).to_string(),
        key_of(&list[i + 2]).to_string(),
    );
    let want = list[i].suggested.unwrap();
    let mut h = harness(m);
    h.run();
    focus_row(&mut h, &first);
    h.key_press(egui::Key::Enter);
    h.run();
    assert_eq!(focused_row(&h).as_deref(), Some(second.as_str()));
    idle(&mut h);
    assert_eq!(subtype(h.state(), &first), Some(json!(want)));
    assert_eq!(focused_row(&h).as_deref(), Some(second.as_str()));
    // Space does the same.
    let want = rows(h.state())
        .into_iter()
        .find(|p| key_of(p) == second)
        .and_then(|p| p.suggested)
        .expect("the second row has a suggestion");
    h.key_press(egui::Key::Space);
    h.run();
    assert_eq!(focused_row(&h).as_deref(), Some(third.as_str()));
    idle(&mut h);
    assert_eq!(subtype(h.state(), &second), Some(json!(want)));
}

#[test]
fn enter_on_the_last_row_accepts_and_keeps_focus_on_the_list() {
    let d = tempfile::tempdir().unwrap();
    let m = with_tint(d.path());
    assert_eq!(
        key_of(rows(&m).last().unwrap()),
        TINT,
        "x_s is the last row"
    );
    let mut h = harness(m);
    h.run();
    focus_row(&mut h, TINT);
    h.key_press(egui::Key::Enter);
    h.run();
    assert_eq!(focused_row(&h).as_deref(), Some(TINT), "no wrap to the top");
    idle(&mut h);
    assert_eq!(subtype(h.state(), TINT), Some(json!("subsurface_tint")));
    assert!(!decide_row(h.state(), TINT));
    h.run();
    let last = rows(h.state()).last().map(|p| key_of(p).to_string());
    assert!(last.is_some());
    assert_eq!(focused_row(&h), last, "focus stays on the list");
}

#[test]
fn a_row_without_a_suggestion_ignores_enter_and_space() {
    let d = tempfile::tempdir().unwrap();
    let mut m = with_tint(d.path());
    // leaf.png's sub-type decided: it asks only what its alpha means, which has no suggestion.
    let leaf = "landscape/leaf.png";
    m.decide(leaf, Answer::Subtype(Subtype::Diffuse));
    assert!(m.wait_idle(Duration::from_secs(60)));
    let p = rows(&m).into_iter().find(|p| key_of(p) == leaf).unwrap();
    assert_eq!(p.suggested, None);
    let undo = m.undo_label().map(str::to_string);
    let mut h = harness(m);
    h.run();
    focus_row(&mut h, leaf);
    for k in [egui::Key::Enter, egui::Key::Space] {
        h.key_press(k);
        h.run();
        assert!(!h.state().busy(), "{k:?} started a plan");
        assert_eq!(h.state().undo_label().map(str::to_string), undo, "{k:?}");
        assert_eq!(focused_row(&h).as_deref(), Some(leaf), "{k:?} moved focus");
    }
    assert!(decide_row(h.state(), leaf));
}
