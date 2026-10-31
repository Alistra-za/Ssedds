//! Layout v2, owner addition (R17): the Sets column's search box, filter pills and shader filter. Filters only hide
//! (Review Focus 5): the selection, the editor and the bulk panel stay.
mod support;
use eframe::egui::{self, accesskit::Role, containers::panel::PanelState, Id};
use egui_kittest::kittest::{by, NodeT, Queryable};
use egui_kittest::Harness;
use engine::shader::Shader;
use sdc_gui::model::sets::views;
use sdc_gui::model::sets_filter::SetsFilter;
use sdc_gui::model::{AppModel, Selection};
use sdc_gui::settings::*;
use std::time::Duration;
use support::*;

fn key_of(m: &AppModel, id_end: &str) -> String {
    views(m)
        .iter()
        .find(|v| v.id.ends_with(id_end))
        .unwrap_or_else(|| panic!("no set {id_end}"))
        .key
        .clone()
}

fn visible_ids(m: &AppModel) -> Vec<String> {
    let v = views(m);
    m.visible_sets().iter().map(|&i| v[i].id.clone()).collect()
}

fn harness(m: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(egui::vec2(1500.0, 900.0))
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

fn sets_rect(h: &Harness<'static, AppModel>) -> egui::Rect {
    PanelState::load(&h.ctx, Id::new("sets"))
        .expect("the Sets panel is drawn")
        .outer_rect
}

/// K15: the Sets column's own "Filter ▾" (the Textures column and the mips field also match "Filter").
fn sets_filter_button<'a>(h: &'a Harness<'static, AppModel>) -> egui_kittest::Node<'a> {
    let r = sets_rect(h);
    let mut v: Vec<_> = h
        .query_all_by_label_contains("Filter")
        .filter(|n| r.contains_rect(n.rect()) && n.accesskit_node().role() == Role::Button)
        .collect();
    assert_eq!(v.len(), 1, "one Filter button in the Sets column");
    v.remove(0)
}

/// The set names drawn in the Sets column.
fn names_in_column(h: &Harness<'static, AppModel>, ids: &[String]) -> usize {
    ids.iter()
        .filter(|id| {
            h.query_all_by_label(id.as_str())
                .any(|n| in_sets_column(h, n.rect()))
        })
        .count()
}

#[test]
fn search_folds_like_set_ids() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    for q in ["LANDSCAPE/ROCK", "  landscape\\Rock ", "/landscape/rock/"] {
        m.sets_filter.search = q.into();
        let v = views(&m);
        assert!(
            m.visible_sets()
                .iter()
                .all(|&i| v[i].id.to_lowercase().contains("landscape/rock")),
            "{q:?}"
        );
        assert_eq!(visible_ids(&m), ["landscape/rock01"], "{q:?}");
    }
    // The key (an absolute export path here) is not searched: only the id and the label.
    m.sets_filter.search = "export".into();
    assert!(m.visible_sets().is_empty());
}

#[test]
fn the_pills_count_and_partition_the_sets() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    // A created set with no members yet (K20): it counts toward no pill.
    m.edit_docs("t", None, true, |_, s| s.create_set("new/empty"));
    assert!(m.wait_idle(Duration::from_secs(60)));
    let all = views(&m);
    assert!(all.iter().any(|v| v.key.is_empty()), "the keyless set");
    let c = m.set_counts();
    let keyed = all.iter().filter(|v| !v.key.is_empty()).count();
    assert_eq!(keyed, 3);
    assert_eq!(c.attention + c.ready, keyed);
    assert!(c.no_json <= keyed);
    // Each pill shows what its count says, and a keyless set passes only an empty filter.
    m.sets_filter.attention = true;
    assert_eq!(m.visible_sets().len(), c.attention);
    m.sets_filter = SetsFilter {
        ready: true,
        ..SetsFilter::default()
    };
    assert_eq!(m.visible_sets().len(), c.ready);
    m.sets_filter = SetsFilter {
        no_json: true,
        ..SetsFilter::default()
    };
    assert_eq!(m.visible_sets().len(), c.no_json);
    m.clear_sets_filter();
    assert_eq!(
        m.visible_sets().len(),
        all.len(),
        "an empty filter shows all"
    );
}

#[test]
fn no_json_and_the_shader_filter() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    // leaf and odd are vanilla Landscape sets (no JSON); rock01 is PBR Landscape with no JSON yet.
    m.sets_filter.shader = Some(Shader::Landscape);
    assert_eq!(visible_ids(&m), ["landscape/leaf", "landscape/odd"]);
    m.sets_filter.shader = Some(Shader::PbrLandscape);
    assert_eq!(visible_ids(&m), ["landscape/rock01"]);
    m.sets_filter.no_json = true;
    assert_eq!(visible_ids(&m), ["landscape/rock01"], "AND with No JSON");
    // A complex-material set that exports its JSON is not "No JSON" (A8).
    m.sets_filter = SetsFilter {
        no_json: true,
        ..SetsFilter::default()
    };
    complex_material(&mut m, "landscape/leaf");
    let leaf = views(&m)
        .iter()
        .find(|v| v.id == "landscape/leaf")
        .cloned()
        .unwrap();
    let exporting =
        sdc_gui::model::cm_json::exports(&m.sets_doc.as_ref().unwrap().entries(), "landscape/leaf");
    assert_eq!(
        sdc_gui::model::sets_filter::writes_json(&m, &leaf),
        exporting
    );
    assert_eq!(
        visible_ids(&m).contains(&"landscape/leaf".to_string()),
        !exporting
    );
}

#[test]
fn the_list_follows_a_new_problem_without_a_view_change() {
    // K17: Ready reads `problems()`, which can change while the views key does not.
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.sets_filter.attention = true;
    let a = m.visible_sets();
    let again = m.visible_sets();
    assert!(std::sync::Arc::ptr_eq(&a, &again), "cached per change");
    m.evidence_rev += 1;
    let b = m.visible_sets();
    assert!(
        !std::sync::Arc::ptr_eq(&a, &b),
        "a new problems list rebuilds it"
    );
    assert_eq!(*a, *b);
}

#[test]
fn a_filter_hides_the_open_set_and_keeps_it_selected() {
    // Review Focus 5.
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let rock = key_of(&m, "landscape/rock01");
    m.select_set(&rock);
    let before = (m.selection.clone(), m.selected_sets());
    m.sets_filter.ready = true; // rock01 needs attention in the fixture
    let v = views(&m);
    assert!(!m.visible_sets().iter().any(|&i| v[i].key == rock));
    assert_eq!((m.selection.clone(), m.selected_sets()), before);
    assert!(!m.is_dirty());
}

#[test]
fn go_to_a_hidden_set_selects_it_and_the_filter_stays() {
    // Review Focus 5.
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let rock = key_of(&m, "landscape/rock01");
    m.sets_filter.ready = true;
    m.select_set(&rock); // what Problems' "Go to set" calls
    assert_eq!(m.selection, Selection::Set(rock.clone()));
    assert!(m.sets_filter.ready);
    let v = views(&m);
    assert!(
        !m.visible_sets().iter().any(|&i| v[i].key == rock),
        "K23: selected and still hidden"
    );
}

/// rock01, leaf, odd in that order: the search "o" hides leaf only.
fn rock_leaf_odd(dir: &std::path::Path) -> (AppModel, [String; 3]) {
    let mut m = opened(dir);
    assert!(m.reorder_set("landscape/rock01", 0));
    assert!(m.wait_idle(Duration::from_secs(60)));
    let ids: Vec<String> = views(&m).iter().map(|v| v.id.clone()).collect();
    assert_eq!(ids, ["landscape/rock01", "landscape/leaf", "landscape/odd"]);
    let k = ["landscape/rock01", "landscape/leaf", "landscape/odd"].map(|i| key_of(&m, i));
    (m, k)
}

#[test]
fn shift_click_ranges_follow_the_visible_sets() {
    // Review Focus 5, R17.
    let d = tempfile::tempdir().unwrap();
    let (mut m, [rock, leaf, odd]) = rock_leaf_odd(d.path());
    m.click_set(&rock, false, false);
    m.click_set(&odd, false, true);
    assert_eq!(m.selected_sets().len(), 3, "unfiltered: all three");
    m.sets_filter.search = "o".into();
    assert_eq!(visible_ids(&m), ["landscape/rock01", "landscape/odd"]);
    m.click_set(&rock, false, false);
    m.click_set(&odd, false, true);
    let mut sel = m.selected_sets();
    sel.sort();
    let mut want = vec![rock, odd];
    want.sort();
    assert_eq!(sel, want, "the hidden leaf is not in the range");
    assert!(!m.selected_sets().contains(&leaf));
}

#[test]
fn ctrl_a_with_every_set_hidden_keeps_the_selection() {
    // A select-all over nothing never deselects.
    let d = tempfile::tempdir().unwrap();
    let (mut m, [rock, leaf, _]) = rock_leaf_odd(d.path());
    m.click_set(&rock, false, false);
    m.click_set(&leaf, true, false);
    let before = (m.selection.clone(), m.selected_sets());
    m.sets_filter.search = "zzz".into();
    assert!(m.visible_sets().is_empty());
    m.select_all_visible();
    assert_eq!((m.selection.clone(), m.selected_sets()), before);
}

#[test]
fn no_false_no_match_while_the_first_plan_runs() {
    // Pills restored from gui.toml, then a folder opens: until the views exist there is no "No sets match" and no
    // Clear filter that would wipe them.
    let d = tempfile::tempdir().unwrap();
    let root = mod_folder(d.path());
    let mut s = GuiSettings {
        export_folder: Some(d.path().join("export")),
        ..GuiSettings::default()
    };
    s.layout.sets_filters = vec!["ready".into()];
    let mut m = AppModel::new(s, None, session());
    sdc_gui::layout::restore_model(
        &m.settings.layout.clone(),
        &mut m.filter,
        &mut m.prefs,
        &mut m.sets_filter,
        &mut sdc_gui::model::view::ViewState::default(),
    );
    assert!(m.sets_filter.ready);
    m.open_folder(root);
    assert!(views(&m).is_empty(), "the plan has not arrived yet");
    let mut h = harness(m);
    h.step();
    assert!(views(h.state()).is_empty());
    assert!(h.query_by_label_contains("No sets match").is_none());
    assert!(h.query_by_label("Clear filter").is_none());
    assert!(h.state_mut().wait_idle(Duration::from_secs(60)));
    h.run();
    assert!(h.state().sets_filter.ready, "the restored pill stays");
    // Nothing is ready in this fixture: now the message is true.
    h.get_by_label("No sets match the filter (3 hidden)");
    h.get_by_label("Clear filter");
}

#[test]
fn ctrl_a_on_sets_selects_the_visible_sets() {
    // K18: as the Textures Ctrl+A ("every visible texture").
    let d = tempfile::tempdir().unwrap();
    let (mut m, [rock, leaf, odd]) = rock_leaf_odd(d.path());
    m.select_set(&rock);
    m.select_all_visible();
    assert_eq!(m.selected_sets().len(), 3);
    m.select_set(&rock);
    m.sets_filter.search = "o".into();
    m.select_all_visible();
    let mut sel = m.selected_sets();
    sel.sort();
    let mut want = vec![rock, odd];
    want.sort();
    assert_eq!(sel, want);
    assert!(!sel.contains(&leaf));
}

#[test]
fn the_selection_summary_counts_the_hidden_sets() {
    let d = tempfile::tempdir().unwrap();
    let (mut m, [rock, leaf, _]) = rock_leaf_odd(d.path());
    m.click_set(&rock, false, false);
    m.click_set(&leaf, true, false);
    assert_eq!(m.selection_summary(true).as_deref(), Some("2 selected"));
    m.sets_filter.search = "o".into();
    assert_eq!(
        m.selection_summary(true).as_deref(),
        Some("2 selected (1 hidden by filter)")
    );
}

/// Note FC13 (CD13): "Go to set" on a set the filter hides keeps the filter and says so in one line, with the way back.
#[test]
fn a_selected_set_the_filter_hides_is_named_with_a_clear_filter_button() {
    let d = tempfile::tempdir().unwrap();
    let (mut m, [rock, leaf, _]) = rock_leaf_odd(d.path());
    m.sets_filter.search = "rock".into();
    assert_eq!(m.selected_set_hidden_by_filter(), None, "nothing selected");
    m.select_set(&rock);
    assert_eq!(m.selected_set_hidden_by_filter(), None, "a shown set");
    m.select_set(&leaf);
    assert_eq!(
        m.selected_set_hidden_by_filter().as_deref(),
        Some("landscape/leaf")
    );
    let mut h = harness(m);
    h.run();
    h.get_by_label("landscape/leaf is hidden by the Sets filter");
    h.get_by_label("Clear filter").click();
    h.run();
    h.run();
    assert!(!h.state().sets_filter_active());
    assert!(h
        .query_by_label_contains("is hidden by the Sets filter")
        .is_none());
    assert_eq!(h.state().selected_sets(), [leaf], "the selection stayed");
}

/// Note FC13 (CD16, supersedes "a new set stays hidden"): a set made while a filter is on is shown until the filter
/// changes, and the column says so.
#[test]
fn a_set_made_under_a_filter_stays_shown_until_the_filter_changes() {
    let d = tempfile::tempdir().unwrap();
    let (mut m, _) = rock_leaf_odd(d.path());
    m.sets_filter.search = "rock".into();
    let odd = m
        .items
        .iter()
        .find(|i| i.name == "odd.png")
        .unwrap()
        .key
        .clone();
    let id = m.drop_new_set(&odd).unwrap();
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(visible_ids(&m).contains(&id), "{id} shown under the filter");
    let mut h = harness(m);
    h.run();
    h.get_by_label("1 new set shown");
    // The next filter change ends it.
    h.state_mut().sets_filter.search = "roc".into();
    h.run();
    h.run();
    assert!(!visible_ids(h.state()).contains(&id));
    assert!(h.query_by_label("1 new set shown").is_none());
    // And the old filter coming back does not bring it back.
    h.state_mut().sets_filter.search = "rock".into();
    h.run();
    assert!(!visible_ids(h.state()).contains(&id));
}

#[test]
fn the_pills_and_shader_persist_and_the_search_does_not() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("sdc/gui.toml");
    let mut s = GuiSettings::default();
    s.layout.sets_filters = vec!["ready".into(), "no-json".into()];
    s.layout.sets_shader = Some("pbr_landscape".into());
    s.save(&p).unwrap();
    let (back, err) = GuiSettings::load(&p);
    assert_eq!(err, None);
    assert_eq!(back, s);
    // Into the model and back out as the app does.
    let mut m = AppModel::new(back, None, session());
    m.sets_filter.search = "rock".into();
    sdc_gui::layout::restore_model(
        &m.settings.layout.clone(),
        &mut m.filter,
        &mut m.prefs,
        &mut m.sets_filter,
        &mut sdc_gui::model::view::ViewState::default(),
    );
    assert_eq!(
        m.sets_filter,
        SetsFilter {
            search: "rock".into(),
            attention: false,
            ready: true,
            no_json: true,
            shader: Some(Shader::PbrLandscape),
        },
        "the search is the model's own"
    );
    m.sets_filter.ready = false;
    m.sets_filter.attention = true;
    let ctx = egui::Context::default();
    let cur = sdc_gui::layout::capture(
        &ctx,
        &m.settings.layout,
        &m.filter,
        &m.prefs,
        &m.sets_filter,
        &sdc_gui::model::view::ViewState::default(),
        (&Default::default(), &Default::default()),
    );
    assert_eq!(cur.sets_filters, ["attention", "no-json"]);
    assert_eq!(cur.sets_shader.as_deref(), Some("pbr_landscape"));
    let mut saved = s.clone();
    saved.layout = cur;
    saved.save(&p).unwrap();
    let text = std::fs::read_to_string(&p).unwrap();
    assert!(!text.contains("rock"), "the search is not saved:\n{text}");
    // A restart's model starts with no search.
    let (back, _) = GuiSettings::load(&p);
    let mut fresh = AppModel::new(back, None, session());
    sdc_gui::layout::restore_model(
        &fresh.settings.layout.clone(),
        &mut fresh.filter,
        &mut fresh.prefs,
        &mut fresh.sets_filter,
        &mut sdc_gui::model::view::ViewState::default(),
    );
    assert_eq!(fresh.sets_filter.search, "");
    assert!(fresh.sets_filter.attention && fresh.sets_filter.no_json);
}

#[test]
fn a_bad_saved_sets_filter_is_dropped_alone() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("gui.toml");
    std::fs::write(
        &p,
        "[layout]\nsets_filters = [\"ready\", \"bogus\", \"ready\", 3]\nsets_shader = \"nope\"\nsort = \"size\"\n",
    )
    .unwrap();
    let (s, err) = GuiSettings::load(&p);
    assert_eq!(err, None);
    assert_eq!(s.layout.sets_filters, ["ready"]);
    assert_eq!(s.layout.sets_shader, None);
    assert_eq!(s.layout.sort, "size", "the rest of the file still loads");
    std::fs::write(&p, "[layout]\nsets_filters = 7\nsets_shader = 7\n").unwrap();
    let (s, err) = GuiSettings::load(&p);
    assert_eq!(err, None);
    assert!(s.layout.sets_filters.is_empty());
    assert_eq!(s.layout.sets_shader, None);
}

#[test]
fn the_column_says_when_nothing_matches_and_clear_brings_all_back() {
    let d = tempfile::tempdir().unwrap();
    let m = opened(d.path());
    let ids: Vec<String> = views(&m).iter().map(|v| v.id.clone()).collect();
    let mut h = harness(m);
    h.run();
    assert_eq!(names_in_column(&h, &ids), 3);
    let r = sets_rect(&h);
    h.get(by().predicate(|n| n.placeholder() == Some("Search sets")))
        .focus();
    h.run();
    let boxes: Vec<_> = h
        .query_all_by_role(Role::TextInput)
        .filter(|n| n.accesskit_node().placeholder() == Some("Search sets"))
        .collect();
    assert_eq!(boxes.len(), 1);
    assert!(r.contains_rect(boxes[0].rect()), "in the Sets column");
    boxes[0].type_text("zzz");
    h.run();
    h.run();
    assert_eq!(h.state().sets_filter.search, "zzz");
    h.get_by_label("No sets match the filter (3 hidden)");
    assert_eq!(names_in_column(&h, &ids), 0);
    h.get_by_label("Clear filter").click();
    h.run();
    h.run();
    assert!(!h.state().sets_filter_active());
    assert_eq!(names_in_column(&h, &ids), 3, "every set is back");
    assert!(h
        .query_by_label("No sets match the filter (3 hidden)")
        .is_none());
}

#[test]
fn the_pills_show_their_counts_and_the_shader_popup_filters() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let c = m.set_counts();
    // A9: a saved shader the Mode does not offer is still listed.
    m.target = engine::config::Target::Vanilla;
    m.sets_filter.shader = Some(Shader::PbrLandscape);
    let mut h = harness(m);
    h.run();
    let r = sets_rect(&h);
    let pill = |h: &Harness<'static, AppModel>, t: &str| {
        let n = h.get_by_label(t);
        assert!(r.contains_rect(n.rect()), "{t} in the Sets column");
    };
    pill(&h, &format!("Not ready {}", c.attention));
    pill(&h, &format!("Ready {}", c.ready));
    pill(&h, &format!("No JSON {}", c.no_json));
    // rock01's card also says "PBR Landscape": the popup adds one more.
    let cards = h.query_all_by_label(Shader::PbrLandscape.label()).count();
    sets_filter_button(&h).click();
    h.run();
    assert_eq!(
        h.query_all_by_label(Shader::PbrLandscape.label()).count(),
        cards + 1
    );
    h.get_by_label(Shader::Landscape.label()).click();
    h.run();
    assert_eq!(h.state().sets_filter.shader, Some(Shader::Landscape));
    h.get_by_label(&format!("Ready {}", c.ready)).click();
    h.run();
    assert!(h.state().sets_filter.ready);
}

#[test]
fn the_set_panel_stays_while_its_set_is_hidden() {
    // K23: filters hide, the editor stays.
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    decide_all(&mut m);
    let rock = key_of(&m, "landscape/rock01");
    m.select_set(&rock);
    m.sets_filter.search = "leaf".into();
    let mut h = harness(m);
    h.run();
    let v = views(h.state());
    assert!(!h.state().visible_sets().iter().any(|&i| v[i].key == rock));
    // Note FB2: the set editor (one column) is drawn, its Slot commands… with it.
    h.get_by_role_and_label(Role::Button, "Slot commands…");
    assert_eq!(h.state().selection, Selection::Set(rock));
}

#[test]
fn the_bulk_panel_stays_while_part_of_its_selection_is_hidden() {
    // K23: two sets selected, one hidden.
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let (rock, leaf) = (key_of(&m, "landscape/rock01"), key_of(&m, "landscape/leaf"));
    m.click_set(&rock, false, false);
    m.click_set(&leaf, true, false);
    m.sets_filter.search = "leaf".into();
    let mut h = harness(m);
    h.run();
    assert!(h.state().in_bulk());
    h.get_by_label("2 sets selected");
    h.get_by_label("2 selected (1 hidden by filter)");
}

/// Drag set `from`'s ≡ handle onto set `onto`'s name, step by step as a pointer would.
fn drag_handle(h: &mut Harness<'static, AppModel>, from: &str, onto: &str) {
    let handle = |h: &Harness<'static, AppModel>, id: &str| {
        let name = h
            .query_all_by_label(id)
            .map(|n| n.rect())
            .find(|r| in_sets_column(h, *r))
            .unwrap_or_else(|| panic!("{id}'s card"));
        h.query_all_by_label("≡")
            .map(|n| n.rect())
            .find(|r| (r.center().y - name.center().y).abs() < 4.0)
            .expect("the card's handle")
            .center()
    };
    let at = handle(h, from);
    let to = h
        .query_all_by_label(onto)
        .map(|n| n.rect())
        .find(|r| in_sets_column(h, *r))
        .unwrap()
        .center();
    let press = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    h.hover_at(at);
    h.run();
    h.event(press(at, true));
    h.run();
    for s in 1..=8 {
        h.event(egui::Event::PointerMoved(at + (to - at) * (s as f32 / 8.0)));
        h.run();
    }
    h.event(press(to, false));
    // A moved set's border flashes (it repaints for a second): steps, not `run`.
    h.run_steps(3);
}

fn order_text(h: &Harness<'static, AppModel>) -> String {
    h.state().sets_doc.as_ref().unwrap().text()
}

/// The control for `reordering_is_off_while_filtered`: with no filter the same drag moves the set. (A test of its own:
/// the drawing state, and so the moved set's flash, is per thread.)
#[test]
fn a_handle_drag_reorders_with_no_filter() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened(d.path()));
    h.run();
    let before = order_text(&h);
    drag_handle(&mut h, "landscape/odd", "landscape/leaf");
    assert_ne!(order_text(&h), before, "the drag reorders with no filter");
}

#[test]
fn reordering_is_off_while_filtered() {
    // A search that hides nothing still turns reordering off.
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.sets_filter.search = "landscape".into();
    let mut h = harness(m);
    h.run();
    assert_eq!(h.state().visible_sets().len(), 3);
    let before = order_text(&h);
    drag_handle(&mut h, "landscape/odd", "landscape/leaf");
    assert_eq!(order_text(&h), before, "no reorder while filtered");
    assert!(!h.state().is_dirty());
}

#[test]
fn move_up_and_down_are_off_while_filtered() {
    // K19.
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.sets_filter.search = "landscape".into();
    let mut h = harness(m);
    h.run();
    let name = h
        .query_all_by_label("landscape/odd")
        .find(|n| in_sets_column(&h, n.rect()))
        .unwrap()
        .rect();
    h.hover_at(name.center());
    h.run();
    h.query_all_by_label("landscape/odd")
        .find(|n| in_sets_column(&h, n.rect()))
        .unwrap()
        .click_secondary();
    h.run();
    for b in ["Move up", "Move down"] {
        assert!(
            h.get_by_label(b).accesskit_node().is_disabled(),
            "{b} is disabled"
        );
    }
}
