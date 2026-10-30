//! Helper textures Task 7 (Note DY16): helper files are rows in a "Helpers (n)" group under the tiles, shown or hidden
//! by a "Helpers n" pill that is on by default. Settings go through `GuiSettings` values and temp dirs only.
mod support;
use eframe::egui;
use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use image::{ImageBuffer, Luma, Rgb};
use sdc_gui::model::items::ItemKind;
use sdc_gui::model::AppModel;
use sdc_gui::settings::GuiSettings;
use std::path::Path;
use std::time::Duration;
use support::session;

/// A folder with `files` (a diffuse for `<base>.png`, a flat normal for `_n`, grey ramps for the rest) and `toml` as
/// its `sdc.toml`, opened with high-confidence suggestions accepted.
fn fixture(dir: &Path, files: &[&str], toml: &str) -> AppModel {
    for f in files {
        let p = dir.join(f);
        if f.ends_with("_n.png") {
            ImageBuffer::from_fn(64, 64, |_, _| Rgb([128u8, 128, 255]))
                .save(p)
                .unwrap();
        } else if !f.contains('_') {
            ImageBuffer::from_fn(64, 64, |x, y| Rgb([(x * 4) as u8, (y * 4) as u8, 90u8]))
                .save(p)
                .unwrap();
        } else {
            ImageBuffer::from_fn(64, 64, |x, _| Luma([(x * 3) as u8]))
                .save(p)
                .unwrap();
        }
    }
    if !toml.is_empty() {
        std::fs::write(dir.join("sdc.toml"), toml).unwrap();
    }
    let settings = GuiSettings {
        export_folder: Some(dir.join("export")),
        ..GuiSettings::default()
    };
    let mut m = AppModel::new(settings, None, session());
    m.accept = engine::policy::AcceptSuggestions::High;
    m.open_folder(dir.to_path_buf());
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
    m
}

const XSET: [&str; 3] = ["x.png", "x_roughness.png", "x_opacity.png"];

fn harness(m: AppModel, size: egui::Vec2) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(size)
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

fn app(m: AppModel) -> Harness<'static, AppModel> {
    let mut h = harness(m, egui::vec2(1500.0, 900.0));
    h.run();
    h.run();
    h
}

fn textures_rect(h: &Harness<'static, AppModel>) -> egui::Rect {
    egui::containers::panel::PanelState::load(&h.ctx, egui::Id::new("textures"))
        .expect("the Textures panel is drawn")
        .outer_rect
}

/// The helper rows drawn now, by their accessible name.
fn rows(h: &Harness<'static, AppModel>) -> Vec<(String, egui::Rect)> {
    h.query_all_by_role(Role::Button)
        .filter_map(|n| {
            let l = n.accesskit_node().label()?;
            l.starts_with("Helper ").then(|| (l.to_string(), n.rect()))
        })
        .collect()
}

fn row(h: &Harness<'static, AppModel>, file: &str) -> (String, egui::Rect) {
    let prefix = format!("Helper {file}:");
    rows(h)
        .into_iter()
        .find(|r| r.0.starts_with(&prefix))
        .unwrap_or_else(|| panic!("no row for {file} in {:?}", rows(h)))
}

const XSET3: [&str; 4] = ["x.png", "x_roughness.png", "x_opacity.png", "y_opacity.png"];

/// The group header's accessible name.
fn header_name(n: usize) -> String {
    format!("Helpers ({n})")
}

#[test]
fn helpers_group_sits_below_tiles() {
    let d = tempfile::tempdir().unwrap();
    let h = app(fixture(d.path(), &XSET, ""));
    let m = h.state();
    let rough = m
        .items
        .iter()
        .find(|i| i.name == "x_roughness.png")
        .unwrap();
    assert_eq!(rough.kind, ItemKind::Helper);
    let tiles: Vec<String> = m
        .visible_items()
        .iter()
        .map(|&i| m.items[i].name.clone())
        .collect();
    assert!(!tiles.is_empty());
    assert!(
        !tiles
            .iter()
            .any(|t| t.contains("_roughness") || t.contains("_opacity")),
        "no helper is a tile: {tiles:?}"
    );
    let header = h.get_by_label("Helpers (2)").rect();
    let panel = textures_rect(&h);
    for t in &tiles {
        for n in h
            .query_all_by_label(t)
            .filter(|n| panel.contains_rect(n.rect()))
        {
            assert!(
                n.rect().max.y <= header.min.y,
                "tile {t} at {:?} is not above the header {header:?}",
                n.rect()
            );
        }
    }
    assert_eq!(rows(&h).len(), 2);
}

/// Note FB12 (supersedes DY16's pill): the pill row holds only filters; Helpers is a group header.
#[test]
fn helpers_is_not_a_pill() {
    let d = tempfile::tempdir().unwrap();
    let h = app(fixture(d.path(), &XSET, ""));
    assert_eq!(h.state().filter_counts().helpers, 2);
    assert_eq!(
        h.query_all_by_label("Helpers filter").count(),
        0,
        "no Helpers filter pill"
    );
    assert_eq!(h.query_all_by_label_contains("Helpers 2").count(), 0);
    h.get_by_role_and_label(Role::Button, &header_name(2));
}

#[test]
fn helpers_group_collapses() {
    let d = tempfile::tempdir().unwrap();
    let mut h = app(fixture(d.path(), &XSET3, ""));
    let name = header_name(3);
    assert_eq!(rows(&h).len(), 3, "open by default");
    let toggled = |h: &Harness<'static, AppModel>| {
        h.get_by_role_and_label(Role::Button, &name)
            .accesskit_node()
            .toggled()
    };
    assert_eq!(toggled(&h), Some(egui::accesskit::Toggled::True));
    let tiles = h.state().visible_items();
    h.get_by_role_and_label(Role::Button, &name).click();
    h.run();
    h.run();
    assert!(rows(&h).is_empty(), "collapsing hides the rows");
    assert_eq!(toggled(&h), Some(egui::accesskit::Toggled::False));
    assert_eq!(h.state().visible_items(), tiles, "tiles unchanged");
    assert!(!h.state().filters_active(), "collapsing is not a filter");
    assert_eq!(h.state().hidden_by_filters(), 0);
    h.get_by_role_and_label(Role::Button, &name).click();
    h.run();
    h.run();
    assert_eq!(rows(&h).len(), 3, "opening brings them back");
    assert_eq!(toggled(&h), Some(egui::accesskit::Toggled::True));
}

/// Review fix (FB12): the header button fits its line, so the first helper row starts below it.
#[test]
fn the_group_header_does_not_overlap_the_first_row() {
    let d = tempfile::tempdir().unwrap();
    let h = app(fixture(d.path(), &XSET3, ""));
    let header = h
        .get_by_role_and_label(Role::Button, &header_name(3))
        .rect();
    let first = rows(&h)
        .into_iter()
        .map(|r| r.1.min.y)
        .fold(f32::MAX, f32::min);
    assert!(
        header.max.y <= first + 0.5,
        "header {header:?} overlaps the first row at {first}"
    );
}

#[test]
fn no_helpers_no_header() {
    let d = tempfile::tempdir().unwrap();
    let h = app(fixture(d.path(), &["x.png"], ""));
    assert_eq!(h.query_all_by_label_contains("Helpers (").count(), 0);
}

#[test]
fn hand_set_role_chip_is_amber_with_pencil() {
    let d = tempfile::tempdir().unwrap();
    let toml = "[files.\"x_roughness.png\"]\nhelper.role = \"glossiness\"\n";
    let h = app(fixture(d.path(), &XSET, toml));
    let (name, _) = row(&h, "x_roughness.png");
    assert!(name.contains(": Glossiness (set by you)"), "{name}");
    h.get_by_label("✎ Glossiness");
    let amber = h.output().shapes.iter().any(|s| match &s.shape {
        egui::Shape::Text(t) => {
            t.galley.text().starts_with('✎')
                && t.galley
                    .job
                    .sections
                    .iter()
                    .all(|s| s.format.color == sdc_gui::ui::status::AMBER)
        }
        _ => false,
    });
    assert!(amber, "the hand-set chip is painted amber");
    // A suggested role keeps the default chip colour and no pencil.
    let (other, _) = row(&h, "x_opacity.png");
    assert!(!other.contains("(set by you)"), "{other}");
    h.get_by_label("Opacity");
}

#[test]
fn every_row_says_its_target_or_not_used() {
    let d = tempfile::tempdir().unwrap();
    let mut files = XSET.to_vec();
    files.push("y_opacity.png");
    let h = app(fixture(d.path(), &files, ""));
    let (y, _) = row(&h, "y_opacity.png");
    assert!(y.contains("not used:"), "{y}");
    let (x, _) = row(&h, "x_roughness.png");
    assert!(x.contains("fills to x_rmaos R"), "{x}");
    for (n, _) in rows(&h) {
        assert!(
            n.contains("fills to ") || n.contains("not used"),
            "{n} says neither"
        );
    }
}

#[test]
fn layer_files_now_have_rows() {
    let d = tempfile::tempdir().unwrap();
    let mut h = app(fixture(
        d.path(),
        &["x.png", "x_n.png", "x_detailnorm.png"],
        "",
    ));
    let (name, _) = row(&h, "x_detailnorm.png");
    assert!(name.contains("fills to x_n"), "{name}");
    h.state_mut().select_item("x_n.png");
    h.run();
    h.run();
    assert!(h.query_all_by_label("RNM").count() > 0, "the RNM section");
    h.get_by_label("detail");
}

#[test]
fn helper_row_stays_inside_column_at_1100() {
    let d = tempfile::tempdir().unwrap();
    let long = format!("x_{}_roughness.png", "a".repeat(64));
    assert_eq!(long.len(), 80);
    let m = fixture(d.path(), &["x.png", &long], "");
    let mut h = harness(m, egui::vec2(1100.0, 800.0));
    h.run();
    h.run();
    let panel = textures_rect(&h);
    let (name, rect) = row(&h, &long);
    assert!(rect.max.x <= panel.max.x, "row {rect:?} panel {panel:?}");
    // Every widget on the row's line stays inside the column.
    let mut on_row = 0;
    for n in h
        .query_all(egui_kittest::kittest::by())
        .filter(|n| n.accesskit_node().bounding_box().is_some())
    {
        let r = n.rect();
        if r.center().y > rect.min.y && r.center().y < rect.max.y && r.min.x >= panel.min.x {
            on_row += 1;
            assert!(
                r.max.x <= panel.max.x,
                "{:?} ends at {} past {}",
                n.accesskit_node().label(),
                r.max.x,
                panel.max.x
            );
        }
    }
    assert!(on_row >= 3, "row, name, target and chip: {on_row}");
    let name_label = h.get_by_label(&long).rect();
    assert!(
        name_label.width() < panel.width() / 2.0,
        "the name is truncated: {name_label:?}"
    );
    let target = h.get_by_label_contains("→ x_aaaa").rect();
    assert!(target.width() > 10.0 && target.max.x <= panel.max.x);
    assert!(name.contains("fills to"), "{name}");
}

#[test]
fn ten_thousand_helpers_build_once() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), &XSET, "");
    let base = m
        .items
        .iter()
        .find(|i| i.kind == ItemKind::Helper)
        .unwrap()
        .clone();
    m.items.retain(|i| i.kind != ItemKind::Helper);
    m.items.extend((0..10_000).map(|n| {
        let mut i = base.clone();
        i.key = format!("h/h{n:05}_roughness.png");
        i.name = format!("h{n:05}_roughness.png");
        i
    }));
    m.invalidate_views();
    let mut h = app(m);
    assert_eq!(h.state().visible_helpers_shared().len(), 10_000);
    let drawn = sdc_gui::ui::columns::helper_rows_drawn(&h.ctx);
    assert!(drawn > 0 && drawn < 100, "{drawn} rows laid out of 10000");
    let b0 = h.state().visible_builds();
    h.run();
    h.run();
    assert_eq!(h.state().visible_builds(), b0, "no build per frame");
    h.state_mut().search = "h0000".into();
    h.run();
    h.run();
    h.run();
    assert_eq!(h.state().visible_builds(), b0 + 1, "one build per change");
    assert_eq!(h.state().visible_helpers_shared().len(), 10);
}

/// The keys of the helper rows shown, in display order.
fn helper_keys(m: &AppModel) -> Vec<String> {
    m.visible_helpers_shared()
        .iter()
        .map(|&i| m.items[i].key.clone())
        .collect()
}

/// Review fix (DY16): mouse range selection runs over the tiles then the shown helper rows.
#[test]
fn shift_click_between_helpers_selects_the_range() {
    let d = tempfile::tempdir().unwrap();
    let mut files = XSET.to_vec();
    files.push("y_opacity.png");
    let mut m = fixture(d.path(), &files, "");
    let rows = helper_keys(&m);
    assert_eq!(rows.len(), 3, "{rows:?}");
    m.click_item(&rows[0], false, false);
    m.click_item(&rows[2], false, true);
    assert_eq!(m.selected_items(), rows, "A..C in display order");
}

/// Re-review fix (DY16): keyboard ranges stay on the tiles (shortcut: helper rows are mouse-only), even from a
/// helper anchor.
#[test]
fn shift_arrow_from_a_helper_selects_tiles_only() {
    let d = tempfile::tempdir().unwrap();
    let mut files = XSET.to_vec();
    files.push("y_opacity.png");
    let mut m = fixture(d.path(), &files, "");
    let rows = helper_keys(&m);
    let tiles: Vec<String> = m
        .visible_items()
        .iter()
        .map(|&i| m.items[i].key.clone())
        .collect();
    assert!(tiles.len() >= 2, "{tiles:?}");
    m.click_item(&rows[2], false, false);
    m.grid_move(1, true);
    let sel = m.selected_items();
    assert!(!sel.is_empty());
    assert!(
        sel.iter().all(|k| tiles.contains(k)),
        "Shift+Right from a helper selected {sel:?}"
    );
    // From a tile, Shift+Right still extends over tiles.
    m.click_item(&tiles[0], false, false);
    m.grid_move(1, true);
    assert_eq!(m.selected_items(), tiles[..2].to_vec());
}

#[test]
fn ctrl_clicked_visible_helpers_are_not_hidden() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), &XSET, "");
    let rows = helper_keys(&m);
    m.click_item(&rows[0], false, false);
    m.click_item(&rows[1], true, false);
    assert_eq!(m.selection_summary(false).as_deref(), Some("2 selected"));
}

/// Review fix (FB12): a selected helper in a collapsed group stays selected and the heading says where it went.
#[test]
fn a_selected_helper_in_a_collapsed_group_is_named_in_the_hint() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), &XSET, "");
    let rows = helper_keys(&m);
    m.click_item("x.png", false, false);
    m.click_item(&rows[0], true, false);
    assert_eq!(m.selection_summary(false).as_deref(), Some("2 selected"));
    m.filter.hide_helpers = true;
    assert!(m.selected_items().contains(&rows[0]), "still selected");
    assert_eq!(
        m.selection_summary(false).as_deref(),
        Some("2 selected (1 in the collapsed Helpers group)")
    );
    assert_eq!(m.hidden_by_filters(), 0, "a fold is not a filter");
    m.filter.hide_helpers = false;
    assert_eq!(m.selection_summary(false).as_deref(), Some("2 selected"));
}

/// Review fix (FB12): select-all, the arrows and a mouse range never reach rows a collapsed group does not show.
#[test]
fn a_collapsed_group_is_out_of_reach_of_select_all_and_ranges() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), &XSET3, "");
    let rows = helper_keys(&m);
    let tiles: Vec<String> = m
        .visible_items()
        .iter()
        .map(|&i| m.items[i].key.clone())
        .collect();
    m.filter.hide_helpers = true;
    m.click_item(&tiles[0], false, false);
    m.select_all_visible();
    assert!(
        m.selected_items().iter().all(|k| !rows.contains(k)),
        "Ctrl+A selected a hidden helper"
    );
    // A range from a helper anchor to a tile does not sweep the hidden rows.
    m.filter.hide_helpers = false;
    m.click_item(&rows[0], false, false);
    m.filter.hide_helpers = true;
    m.click_item(&tiles[0], false, true);
    assert_eq!(m.selected_items(), vec![tiles[0].clone()]);
}

/// Review fix (FB12): revealing a helper (the palette, a link) opens a collapsed group and draws the row.
#[test]
fn revealing_a_helper_opens_the_group() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), &XSET3, "");
    let key = helper_keys(&m)[1].clone();
    m.filter.hide_helpers = true;
    let mut h = app(m);
    assert!(rows(&h).is_empty());
    h.state_mut()
        .palette_pick(&sdc_gui::model::nav::Pick::Texture(key.clone()));
    assert!(!h.state().filter.hide_helpers, "the group opened");
    h.run();
    h.run();
    assert_eq!(rows(&h).len(), 3);
    assert_eq!(h.state().selected_items(), vec![key]);
}

/// Review pictures of the Textures column at 1100, 1280 and 1600 px (egui-ui-rules step 5). Needs a GPU and
/// `SDC_SHOTS_DIR`: `cargo test -p sdc-gui --test helpers_list -- --ignored shots`.
#[test]
#[ignore]
fn shots() {
    let Ok(out) = std::env::var("SDC_SHOTS_DIR") else {
        return;
    };
    let d = tempfile::tempdir().unwrap();
    let long = format!("x_{}_roughness.png", "a".repeat(64));
    let files = [
        "x.png",
        "x_roughness.png",
        "x_opacity.png",
        "x_n.png",
        "x_detailnorm.png",
        "y_opacity.png",
        &long,
    ];
    let toml = "[files.\"x_roughness.png\"]\nhelper.role = \"glossiness\"\n";
    for (w, hgt) in [(1100.0, 800.0), (1280.0, 800.0), (1600.0, 900.0)] {
        let m = fixture(d.path(), &files, toml);
        let mut h = Harness::builder()
            .with_size(egui::vec2(w, hgt))
            .with_pixels_per_point(1.0)
            .wgpu()
            .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
        sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
        h.run_steps(10);
        let img = h.render().expect("render");
        img.save(Path::new(&out).join(format!("helpers-{w}.png")))
            .expect("save png");
    }
}

/// Review C23 (Note FB13): with 10,000 selected the heading stays one line; the count truncates inside the column.
#[test]
fn textures_heading_stays_one_line_with_ten_thousand_selected() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), &XSET, "");
    let base = m
        .items
        .iter()
        .find(|i| i.kind != ItemKind::Helper)
        .unwrap()
        .clone();
    m.items.retain(|i| i.kind == ItemKind::Helper);
    m.items.extend((0..10_000).map(|n| {
        let mut i = base.clone();
        i.status = sdc_gui::model::items::ItemStatus::NeedsDecision;
        i.key = format!("t/t{n:05}.png");
        i.name = format!("t{n:05}.png");
        i
    }));
    m.invalidate_views();
    m.select_all_visible();
    assert_eq!(
        m.selection_summary(false).as_deref(),
        Some("10000 selected")
    );
    assert_eq!(m.undecided(), 10_000, "the link is shown");
    let mut h = harness(m, egui::vec2(1100.0, 800.0));
    // The user dragged the Textures column narrow.
    h.ctx.data_mut(|d| {
        d.insert_persisted(egui::Id::new("textures").with("chosen"), 220.0f32);
    });
    h.run_steps(4);
    let panel = textures_rect(&h);
    let heading = h.get_by_label("Textures").rect();
    let count = h.get_by_label_contains("10000 selected").rect();
    assert!(
        (heading.center().y - count.center().y).abs() < 2.0,
        "heading {heading:?} and count {count:?} share a line"
    );
    assert!(
        count.max.x <= panel.max.x,
        "count {count:?} inside {panel:?}"
    );
    // The "need a decision" link is on that line too.
    let links: Vec<_> = h.query_all_by_label_contains("need a decision").collect();
    assert!(!links.is_empty(), "the link is drawn");
    for l in links {
        let r = l.rect();
        assert!(
            r.min.y >= heading.min.y - 2.0 && r.max.y <= heading.max.y + 2.0,
            "link {r:?} within the heading's line {heading:?}"
        );
    }
    // The count is cut to fit, and the full text shows on hover.
    let before = h.query_all_by_label_contains("10000 selected").count();
    h.hover_at(count.center());
    h.run_steps(30);
    assert!(
        h.query_all_by_label_contains("10000 selected").count() > before,
        "the full count on hover"
    );
}

#[test]
fn hide_helpers_persists() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("sdc/gui.toml");
    let mut m = AppModel::new(GuiSettings::default(), None, session());
    m.filter.hide_helpers = true;
    let ctx = egui::Context::default();
    let s = GuiSettings {
        layout: sdc_gui::layout::capture(
            &ctx,
            &m.settings.layout,
            &m.filter,
            &m.prefs,
            &m.sets_filter,
            &sdc_gui::model::view::ViewState::default(),
            (&Default::default(), &Default::default()),
        ),
        ..GuiSettings::default()
    };
    assert!(s.layout.filters.iter().any(|f| f == "hide-helpers"));
    s.save(&p).unwrap();
    let (back, err) = GuiSettings::load(&p);
    assert_eq!(err, None);
    let mut m = AppModel::new(back, None, session());
    assert!(!m.filter.hide_helpers);
    sdc_gui::layout::restore_model(
        &m.settings.layout.clone(),
        &mut m.filter,
        &mut m.prefs,
        &mut m.sets_filter,
        &mut sdc_gui::model::view::ViewState::default(),
    );
    assert!(m.filter.hide_helpers);
}
