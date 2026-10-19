//! Task 7 (item 8, AG4, AG46-AG55): Textures column filters, sort and tile size.
mod support;
use eframe::egui::{self, Key, Modifiers};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use engine::codes::Code;
use engine::config::{Backend, Format, Subtype};
use engine::policy::SourceStamp;
use sdc_gui::model::items::{ItemStatus, SetItem};
use sdc_gui::model::texture_view::{ColumnPrefs, SetFilter, SortKey};
use sdc_gui::model::AppModel;
use sdc_gui::settings::*;
use std::sync::Arc;
use support::*;

/// `n` textures cloned from a planned one (so each has a job): key `t/t00`..., name the same.
fn many(dir: &std::path::Path, n: usize) -> AppModel {
    let mut m = opened(dir);
    let base = m.items.iter().find(|i| i.job.is_some()).unwrap().clone();
    m.items = (0..n)
        .map(|n| {
            let mut i = base.clone();
            i.key = format!("t/t{n:05}");
            i.name = format!("t{n:05}.png");
            i.status = ItemStatus::Ready;
            i.warnings = vec![];
            i.set_key = None;
            i
        })
        .collect();
    m.sets = vec![];
    m.invalidate_views();
    m
}

fn job(m: &mut AppModel, n: usize) -> &mut engine::policy::ResolvedJob {
    m.items[n].job.as_mut().unwrap()
}

fn shown(m: &AppModel) -> Vec<String> {
    m.visible_items()
        .iter()
        .map(|&i| m.items[i].key.clone())
        .collect()
}

fn keys(v: &[usize]) -> Vec<String> {
    v.iter().map(|n| format!("t/t{n:05}")).collect()
}

/// Eight textures: subtypes, formats, backends and set membership spread over them.
fn mixed(dir: &std::path::Path) -> AppModel {
    let mut m = many(dir, 8);
    for n in 0..8 {
        let (sub, fmt, be) = match n % 4 {
            0 => (Subtype::Diffuse, Format::Bc7, Backend::Bc7e),
            1 => (Subtype::Normal, Format::Bc5, Backend::Rgbcx),
            2 => (Subtype::Diffuse, Format::Bc7, Backend::Nvtt),
            _ => (Subtype::Height, Format::R8, Backend::Uncompressed),
        };
        let p = &mut job(&mut m, n).profile;
        p.subtype = sub;
        p.format = fmt;
        p.encoder.backend = be;
    }
    // t0 and t1 share a set; t2 is alone in its own.
    m.items[0].set_key = Some("s0".into());
    m.items[1].set_key = Some("s0".into());
    m.items[2].set_key = Some("s1".into());
    let set = |key: &str, members: &[usize]| SetItem {
        key: key.into(),
        members: members.iter().map(|n| format!("t/t{n:05}")).collect(),
        recipes: vec![],
        pbr: false,
        shader: None,
        warnings: vec![],
    };
    m.sets = vec![set("s0", &[0, 1]), set("s1", &[2])];
    m.invalidate_views();
    m
}

#[test]
fn each_new_filter_alone_and_combined_with_and() {
    let d = tempfile::tempdir().unwrap();
    let mut m = mixed(d.path());
    assert_eq!(m.visible_items().len(), 8);
    m.filter.subtype = Some("diffuse".into());
    assert_eq!(shown(&m), keys(&[0, 2, 4, 6]));
    m.filter.subtype = None;
    m.filter.format = Some("bc5".into());
    assert_eq!(shown(&m), keys(&[1, 5]));
    m.filter.format = None;
    m.filter.encoder = Some("nvtt".into());
    assert_eq!(shown(&m), keys(&[2, 6]));
    m.filter.encoder = None;
    m.filter.set = SetFilter::In;
    assert_eq!(shown(&m), keys(&[0, 1]), "a set of more than itself");
    m.filter.set = SetFilter::NotIn;
    assert_eq!(shown(&m), keys(&[2, 3, 4, 5, 6, 7]));
    // AND with each other, with a toggle and with the search.
    m.filter.set = SetFilter::Any;
    m.filter.subtype = Some("diffuse".into());
    m.filter.encoder = Some("nvtt".into());
    assert_eq!(shown(&m), keys(&[2, 6]));
    m.filter.format = Some("bc5".into());
    assert!(shown(&m).is_empty());
    m.filter.format = Some("bc7".into());
    m.filter.not_in_set = true;
    assert_eq!(shown(&m), keys(&[2, 6]));
    m.search = "t00006".into();
    assert_eq!(shown(&m), keys(&[6]));
}

#[test]
fn ctrl_a_with_every_texture_hidden_keeps_the_selection() {
    // A select-all over nothing never deselects.
    let d = tempfile::tempdir().unwrap();
    let mut m = mixed(d.path());
    m.click_item("t/t00000", false, false);
    m.click_item("t/t00001", true, false);
    let before = (m.selection.clone(), m.selected_items());
    m.search = "zzz".into();
    assert!(m.visible_items().is_empty());
    m.select_all_visible();
    assert_eq!((m.selection.clone(), m.selected_items()), before);
    assert_eq!(before.1.len(), 2);
}

#[test]
fn a_texture_with_no_job_matches_no_combo_value() {
    let d = tempfile::tempdir().unwrap();
    let mut m = mixed(d.path());
    m.items[3].job = None;
    m.items[3].status = ItemStatus::Failed("x".into());
    m.invalidate_views();
    m.filter.subtype = Some("height".into());
    assert_eq!(shown(&m), keys(&[7]));
}

#[test]
fn the_combo_options_list_only_what_is_in_the_folder() {
    let d = tempfile::tempdir().unwrap();
    let mut m = mixed(d.path());
    let o = m.filter_options();
    assert_eq!(o.subtypes, ["diffuse", "height", "normal"]);
    assert_eq!(o.formats, ["bc5", "bc7", "r8"]);
    assert_eq!(
        o.encoders,
        ["bc7e", "rgbcx", "nvtt", "uncompressed"],
        "in the order the filter lists them"
    );
    for n in 0..8 {
        job(&mut m, n).profile.encoder.backend = Backend::Bc7e;
        job(&mut m, n).profile.format = Format::Bc7;
        job(&mut m, n).profile.subtype = Subtype::Diffuse;
    }
    m.invalidate_views();
    let o = m.filter_options();
    assert_eq!(o.subtypes, ["diffuse"]);
    assert_eq!(o.formats, ["bc7"]);
    assert_eq!(o.encoders, ["bc7e"]);
}

#[test]
fn sorts_by_name_size_and_status_with_the_name_as_tiebreak() {
    let d = tempfile::tempdir().unwrap();
    let mut m = many(d.path(), 6);
    let size = |n: u64| {
        Some(SourceStamp {
            size: n,
            mtime_ns: 0,
        })
    };
    for (n, s) in [(0, 10), (1, 500), (2, 10), (3, 500), (4, 70), (5, 10)] {
        job(&mut m, n).source = size(s);
    }
    // 0 ok, 1 warning, 2 undecided, 3 error, 4 failed, 5 ok.
    m.items[1].warnings = vec![engine::codes::Warning::new(Code::DimNpot, "w")];
    m.items[2].status = ItemStatus::NeedsDecision;
    m.items[3].status = ItemStatus::Error(vec![Code::DimNpot]);
    m.items[4].status = ItemStatus::Failed("boom".into());
    m.invalidate_views();
    assert_eq!(m.prefs.sort, SortKey::Name, "the default");
    assert_eq!(shown(&m), keys(&[0, 1, 2, 3, 4, 5]));
    m.prefs.sort = SortKey::Size;
    assert_eq!(shown(&m), keys(&[1, 3, 4, 0, 2, 5]), "largest first");
    m.prefs.sort = SortKey::Status;
    assert_eq!(
        shown(&m),
        keys(&[3, 4, 1, 2, 0, 5]),
        "errors, warnings, undecided, ok"
    );
}

#[test]
fn keys_and_ranges_follow_the_sorted_order() {
    let d = tempfile::tempdir().unwrap();
    let mut m = many(d.path(), 6);
    for (n, s) in [(0, 10), (1, 500), (2, 40), (3, 300), (4, 70), (5, 20)] {
        job(&mut m, n).source = Some(SourceStamp {
            size: s,
            mtime_ns: 0,
        });
    }
    m.invalidate_views();
    m.prefs.sort = SortKey::Size; // 1, 3, 4, 2, 5, 0
    m.nav.cols = 2;
    m.grid_move(1, false);
    assert_eq!(m.selected_items(), keys(&[1]));
    m.grid_move(1, false);
    assert_eq!(
        m.selected_items(),
        keys(&[3]),
        "right: the next in size order"
    );
    m.grid_move(2, false);
    assert_eq!(m.selected_items(), keys(&[2]), "down a row of two");
    // Shift-click range from the active one in the sorted order.
    m.click_item("t/t00001", false, false);
    m.click_item("t/t00004", false, true);
    assert_eq!(m.selected_items(), keys(&[1, 3, 4]));
    // A selection survives a filter and a sort change, and the hidden ones are counted.
    m.prefs.sort = SortKey::Name;
    m.filter.encoder = Some("nvtt".into());
    assert_eq!(m.selected_items(), keys(&[1, 3, 4]));
    assert_eq!(
        m.selection_summary(false).as_deref(),
        Some("3 selected (3 hidden by filter)")
    );
}

fn app_harness(m: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(egui::vec2(1500.0, 900.0))
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

#[test]
fn the_column_count_follows_the_tile_size() {
    let d = tempfile::tempdir().unwrap();
    let mut m = many(d.path(), 40);
    m.prefs.tile = 64.0;
    let mut h = app_harness(m);
    h.run();
    h.run();
    let small = h.state().nav.cols;
    h.state_mut().prefs.tile = 192.0;
    h.run();
    h.run();
    let big = h.state().nav.cols;
    assert!(
        small > big && big >= 1,
        "{small} tiles a row at 64, {big} at 192"
    );
    assert_eq!(
        h.state().grid_row_step(),
        big as isize,
        "Up and Down use it"
    );
}

#[test]
fn an_empty_filter_says_so_and_clear_filters_keeps_the_sort_and_the_tile() {
    let d = tempfile::tempdir().unwrap();
    let mut m = mixed(d.path());
    m.prefs.sort = SortKey::Size;
    m.prefs.tile = 128.0;
    m.filter.format = Some("bc5".into());
    m.filter.set = SetFilter::In;
    m.filter.failed = true;
    m.search = "t".into();
    assert!(shown(&m).is_empty());
    assert_eq!(m.hidden_by_filters(), 8);
    let mut h = app_harness(m);
    h.run();
    h.run();
    h.get_by_label("No textures match the filters (8 hidden)");
    h.get_by_label("Clear filters").click();
    h.run();
    let m = h.state();
    assert_eq!(m.visible_items().len(), 8);
    assert!(!m.filters_active());
    assert_eq!(
        (m.filter.format.clone(), m.filter.set),
        (None, SetFilter::Any)
    );
    assert!(!m.filter.failed && m.search.is_empty());
    assert_eq!((m.prefs.sort, m.prefs.tile), (SortKey::Size, 128.0));
}

#[test]
fn no_filter_no_message() {
    let d = tempfile::tempdir().unwrap();
    let mut h = app_harness(mixed(d.path()));
    h.run();
    assert_eq!(h.query_all_by_label("Clear filters").count(), 0);
}

/// The Textures panel's rect: the mips "Filter" field and Task 3.4's Sets button also match "Filter" (K15).
fn textures_rect(h: &Harness<'static, AppModel>) -> egui::Rect {
    egui::containers::panel::PanelState::load(&h.ctx, egui::Id::new("textures"))
        .expect("the Textures panel is drawn")
        .outer_rect
}

fn in_textures<'a>(
    h: &'a Harness<'static, AppModel>,
    label: &'a str,
) -> Vec<egui_kittest::Node<'a>> {
    let r = textures_rect(h);
    h.query_all_by_label_contains(label)
        .filter(|n| r.contains_rect(n.rect()))
        .collect()
}

fn filter_button<'a>(h: &'a Harness<'static, AppModel>) -> egui_kittest::Node<'a> {
    let mut v: Vec<_> = in_textures(h, "Filter")
        .into_iter()
        .filter(|n| n.accesskit_node().role() == egui::accesskit::Role::Button)
        .collect();
    assert_eq!(v.len(), 1, "one Filter button in the Textures column");
    v.remove(0)
}

/// The filter combos drawn now: a combo's accessible text is its value ("Sub-type: Any").
fn filter_combos(h: &Harness<'static, AppModel>) -> Vec<String> {
    let names = [
        "Sub-type",
        "Output format",
        "Encoder",
        "Set membership",
        "Sort",
    ];
    h.query_all_by_role(egui::accesskit::Role::ComboBox)
        .filter_map(|n| n.accesskit_node().value())
        .filter(|v| names.iter().any(|c| v.starts_with(c)))
        .collect()
}

#[test]
fn the_filter_pills_show_their_counts() {
    let d = tempfile::tempdir().unwrap();
    let mut m = mixed(d.path());
    m.items[3].status = ItemStatus::Failed("x".into());
    m.items[4].status = ItemStatus::NeedsDecision;
    m.items[5].status = ItemStatus::NeedsDecision;
    m.items[6].warnings = vec![engine::codes::Warning::new(
        Code::LossReencode,
        "re-encoded",
    )];
    m.invalidate_views();
    let c = m.filter_counts();
    assert_eq!((c.needs_decision, c.failed, c.not_in_set), (2, 1, 6));
    assert_eq!(c.warnings, 1, "{c:?}");
    let mut h = app_harness(m);
    h.run();
    h.run();
    for l in [
        "Needs decision 2",
        "Warnings 1",
        "Failed 1",
        "Not in a set 6",
    ] {
        h.get_by_label(l);
    }
    // A pill toggles its filter and hides, never deselects.
    h.get_by_label("Failed 1").click();
    h.run();
    assert!(h.state().filter.failed);
    assert_eq!(h.state().visible_items().len(), 1);
    h.get_by_label("Failed 1").click();
    h.run();
    assert!(!h.state().filter.failed);
}

#[test]
fn the_filter_popup_holds_the_combos_and_the_tile_buttons_step_by_16() {
    let d = tempfile::tempdir().unwrap();
    let mut h = app_harness(mixed(d.path()));
    h.run();
    h.run();
    assert!(filter_combos(&h).is_empty(), "the combos are folded away");
    assert_eq!(
        filter_button(&h).accesskit_node().label().as_deref(),
        Some("Filter ▾")
    );
    filter_button(&h).hover();
    h.run();
    filter_button(&h).click();
    h.run();
    h.run();
    let combos = filter_combos(&h);
    for c in [
        "Sub-type",
        "Output format",
        "Encoder",
        "Set membership",
        "Sort",
    ] {
        assert!(combos.iter().any(|v| v.starts_with(c)), "{c} in {combos:?}");
    }
    h.get_by_label("Larger tiles").click();
    h.run();
    assert_eq!(h.state().prefs.tile, ColumnPrefs::TILE_DEFAULT + 16.0);
    h.get_by_label("Smaller tiles").click();
    h.run();
    assert_eq!(h.state().prefs.tile, ColumnPrefs::TILE_DEFAULT);
    // Each button is disabled at its end of the range.
    let (lo, hi) = ColumnPrefs::TILE_RANGE;
    h.state_mut().prefs.tile = hi;
    h.run();
    assert!(h
        .get_by_label("Larger tiles")
        .accesskit_node()
        .is_disabled());
    assert!(!h
        .get_by_label("Smaller tiles")
        .accesskit_node()
        .is_disabled());
    h.state_mut().prefs.tile = lo;
    h.run();
    assert!(h
        .get_by_label("Smaller tiles")
        .accesskit_node()
        .is_disabled());
    // A saved size off the grid loads as it is and steps from there, clamped.
    h.state_mut().prefs.tile = 100.0;
    h.run();
    h.get_by_label("Larger tiles").click();
    h.run();
    assert_eq!(h.state().prefs.tile, 116.0);
    h.state_mut().prefs.tile = 185.0;
    h.run();
    h.get_by_label("Larger tiles").click();
    h.run();
    assert_eq!(h.state().prefs.tile, hi);
}

#[test]
fn the_filter_button_counts_the_combo_filters_but_not_the_sort() {
    let d = tempfile::tempdir().unwrap();
    let mut m = mixed(d.path());
    m.prefs.sort = SortKey::Size;
    m.filter.format = Some("bc7".into());
    m.filter.set = SetFilter::In;
    let mut h = app_harness(m);
    h.run();
    h.run();
    assert_eq!(
        filter_button(&h).accesskit_node().label().as_deref(),
        Some("Filter (2) ▾")
    );
}

#[test]
fn the_filter_rows_take_at_most_two_lines_at_the_default_width() {
    let d = tempfile::tempdir().unwrap();
    let mut m = mixed(d.path());
    m.filter.format = Some("bc7".into());
    let mut h = app_harness(m);
    h.run();
    h.run();
    let r = textures_rect(&h);
    assert!(
        (r.width() - 380.0).abs() < 2.0,
        "default column width {}",
        r.width()
    );
    let pill = h.get_by_label_contains("Needs decision").rect();
    let search = h
        .query_all_by_role(egui::accesskit::Role::TextInput)
        .find(|n| r.contains_rect(n.rect()))
        .expect("the search box")
        .rect();
    let sp = h.ctx.global_style().spacing.clone();
    let span = search.top() - pill.top();
    assert!(
        span < 3.0 * sp.interact_size.y + sp.item_spacing.y,
        "pills to search box span {span}"
    );
}

#[test]
fn filters_sort_and_tile_round_trip_through_gui_toml() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("sdc/gui.toml");
    let mut s = GuiSettings::default();
    s.layout.filter_subtype = Some("normal".into());
    s.layout.filter_format = Some("bc5".into());
    s.layout.filter_encoder = Some("rgbcx".into());
    s.layout.filter_set = "not-in".into();
    s.layout.sort = "status".into();
    s.layout.tile = 150.0;
    s.save(&p).unwrap();
    let (back, err) = GuiSettings::load(&p);
    assert_eq!(err, None);
    assert_eq!(back, s);
    // Into the model and back out as the app does.
    let mut m = AppModel::new(back, None, session());
    sdc_gui::layout::restore_model(
        &m.settings.layout.clone(),
        &mut m.filter,
        &mut m.prefs,
        &mut m.sets_filter,
        &mut sdc_gui::model::view::ViewState::default(),
    );
    assert_eq!(m.filter.subtype.as_deref(), Some("normal"));
    assert_eq!(m.filter.set, SetFilter::NotIn);
    assert_eq!((m.prefs.sort, m.prefs.tile), (SortKey::Status, 150.0));
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
    assert_eq!(cur.sort, "status");
    assert_eq!(cur.filter_encoder.as_deref(), Some("rgbcx"));
    assert_eq!(cur.tile, 150.0);
}

#[test]
fn a_bad_saved_value_goes_back_to_its_default() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("gui.toml");
    std::fs::write(
        &p,
        "[layout]\nsort = \"colour\"\ntile = 9000.0\nfilter_set = 4\nfilter_format = 7\n",
    )
    .unwrap();
    let (s, err) = GuiSettings::load(&p);
    assert_eq!(err, None);
    assert_eq!(s.layout.sort, "name");
    assert_eq!(s.layout.tile, 96.0);
    assert_eq!(s.layout.filter_set, "any");
    assert_eq!(s.layout.filter_format, None);
}

#[test]
fn the_index_list_is_built_once_per_change_at_10k() {
    let d = tempfile::tempdir().unwrap();
    let mut m = many(d.path(), 10_000);
    let b0 = m.visible_builds();
    let a = m.visible_shared();
    for _ in 0..50 {
        let again = m.visible_shared();
        assert!(Arc::ptr_eq(&a, &again));
    }
    assert_eq!(m.visible_builds(), b0 + 1, "50 frames, one build");
    m.filter.warnings = true;
    m.visible_shared();
    m.visible_shared();
    assert_eq!(m.visible_builds(), b0 + 2, "a filter change rebuilds once");
    m.prefs.sort = SortKey::Size;
    m.visible_shared();
    assert_eq!(m.visible_builds(), b0 + 3, "so does the sort");
    m.invalidate_views();
    m.visible_shared();
    assert_eq!(m.visible_builds(), b0 + 4, "and a new plan");
    // The tile size is not part of the list.
    m.prefs.tile = 150.0;
    m.visible_shared();
    assert_eq!(m.visible_builds(), b0 + 4);
    let _ = (Key::A, Modifiers::NONE);
}
