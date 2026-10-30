//! The bottom panel (layout v2 A1, A9): Files and Messages tabs, its placement under the columns.
mod support;
use eframe::egui::{self, containers::panel::PanelState, Id};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use sdc_gui::model::bottom::BottomTab;
use sdc_gui::model::AppModel;
use sdc_gui::ui::columns::{SETS_MIN, TEXTURES_MIN};
use sdc_gui::ui::set_preview::CENTRE_MIN;
use std::time::Duration;
use support::*;

fn harness_at(model: AppModel, size: egui::Vec2) -> Harness<'static, AppModel> {
    let h = Harness::builder().with_size(size).build_ui_state(
        |ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model),
        model,
    );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

fn harness(model: AppModel) -> Harness<'static, AppModel> {
    harness_at(model, egui::vec2(1400.0, 900.0))
}

fn outer(h: &Harness<'static, AppModel>, id: &str) -> egui::Rect {
    PanelState::load(&h.ctx, Id::new(id)).unwrap().outer_rect
}

#[test]
fn the_panel_starts_collapsed_with_its_tabs_and_toggling_messages_opens_and_closes_it() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened(d.path()));
    h.run();
    h.get_by_label("Files");
    // R14: only the panel's Messages tab (the header's button is gone).
    assert_eq!(h.query_all_by_label("Messages").count(), 1);
    assert!(!h.state().bottom.open);
    h.state_mut().toggle_tab(BottomTab::Messages);
    h.run();
    assert!(h.state().messages.is_open());
    h.get_by_label_contains("messages"); // "0 messages"
    h.state_mut().toggle_tab(BottomTab::Messages);
    h.run();
    assert!(!h.state().bottom.open && !h.state().messages.is_open());
}

/// Task 3.1 review: the Ctrl+M hint the header's Messages button carried is on the tab's hover.
#[test]
fn the_messages_tab_hover_names_its_shortcut() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened(d.path()));
    h.ctx.global_style_mut(|s| {
        s.interaction.tooltip_delay = 0.0;
        s.interaction.show_tooltips_only_when_still = false;
    });
    h.run();
    let want = "Notes and results of this session (Ctrl+M)";
    assert!(h.query_by_label(want).is_none());
    h.get_by_label("Messages").hover();
    h.run();
    h.run();
    h.get_by_label(want);
}

#[test]
fn an_export_opens_the_files_tab() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    decide_all(&mut m);
    m.export_clicked();
    assert!(m.wait_idle(Duration::from_secs(120)));
    assert!(m.bottom.open && m.bottom.tab == BottomTab::Files);
}

#[test]
fn the_panel_spans_the_window_under_the_columns() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened(d.path()));
    h.state_mut().show_tab(BottomTab::Files);
    h.run_steps(6);
    // The harness insets the root ui by 8 px; the header spans that width, so the panel must match it.
    let (p, t, top) = (
        outer(&h, "bottom"),
        outer(&h, "textures"),
        outer(&h, "header"),
    );
    assert!(
        (p.left() - top.left()).abs() <= 1.0 && (p.right() - top.right()).abs() <= 1.0,
        "panel {p:?} vs header {top:?}"
    );
    assert!(p.width() > 1300.0, "{p:?}");
    assert!(
        t.bottom() <= p.top() + 0.5,
        "textures {t:?} reaches into {p:?}"
    );
}

#[test]
fn at_1280x800_with_the_panel_open_the_columns_keep_their_minimums() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness_at(opened(d.path()), egui::vec2(1280.0, 800.0));
    h.state_mut().show_tab(BottomTab::Files);
    h.run_steps(6);
    let (t, s) = (outer(&h, "textures"), outer(&h, "sets"));
    assert!(t.width() >= TEXTURES_MIN - 0.5, "textures {}", t.width());
    assert!(s.width() >= SETS_MIN - 0.5, "sets {}", s.width());
    assert!(
        1280.0 - s.right() >= CENTRE_MIN - 0.5,
        "centre {}",
        1280.0 - s.right()
    );
}

// ---- Task 1.6: the Problems tab, click-throughs and green dots ----

/// Every item of a PBR project carries exactly `ws` as its warnings (copy of set_editor_ui.rs, K16).
fn with_only_warnings(ws: &[engine::codes::Warning]) -> (tempfile::TempDir, AppModel) {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    for i in m.items.iter_mut() {
        i.warnings = vec![];
    }
    for s in m.sets.iter_mut() {
        s.warnings.clear();
    }
    let key = rock01(&m).members[0].clone();
    m.items.iter_mut().find(|i| i.key == key).unwrap().warnings = ws.to_vec();
    m.invalidate_views();
    (d, m)
}

fn rock01(m: &AppModel) -> sdc_gui::model::sets::SetView {
    sdc_gui::model::sets::set_views(m)
        .into_iter()
        .find(|v| v.id == "landscape/rock01")
        .unwrap()
}

/// `n` undecided textures cloned from a planned one (textures_filter.rs::many, K16).
fn many_undecided(dir: &std::path::Path, n: usize) -> AppModel {
    use engine::codes::Code;
    use sdc_gui::model::items::{Decision, ItemStatus};
    let mut m = opened(dir);
    let base = m.items.iter().find(|i| i.job.is_some()).unwrap().clone();
    m.items = (0..n)
        .map(|n| {
            let mut i = base.clone();
            i.key = format!("t/t{n:05}");
            i.name = format!("t{n:05}.png");
            i.status = ItemStatus::NeedsDecision;
            i.decisions = vec![Decision::Acknowledge(Code::DimNpot)];
            i.warnings = vec![];
            i.set_key = None;
            i
        })
        .collect();
    m.sets = vec![];
    m.invalidate_views();
    m
}

fn idle(h: &mut Harness<'static, AppModel>) {
    h.run();
    assert!(h.state_mut().wait_idle(Duration::from_secs(60)));
    h.run();
}

#[test]
fn the_need_a_decision_count_opens_problems_filtered_to_undecided() {
    use sdc_gui::model::bottom::ProblemsFilter;
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened(d.path()));
    h.run();
    h.get_by_label_contains("need a decision").click();
    h.run();
    assert_eq!(h.state().bottom.tab, BottomTab::Problems);
    assert_eq!(h.state().bottom.filter, Some(ProblemsFilter::Undecided));
    h.get_by_label("Undecided only ×");
}

#[test]
fn a_set_badge_opens_problems_filtered_to_its_set() {
    use engine::codes::{Code, Warning};
    use sdc_gui::model::bottom::ProblemsFilter;
    let (_d, m) = with_only_warnings(&[Warning::new(Code::LossReencode, "re-encoded")]);
    let key = rock01(&m).key;
    let mut h = harness(m);
    h.run();
    h.get_by_label("⚠ 1").click();
    h.run();
    assert_eq!(h.state().bottom.tab, BottomTab::Problems);
    assert_eq!(h.state().bottom.filter, Some(ProblemsFilter::Set(key)));
    h.get_by_label("Set: landscape/rock01 ×");
}

#[test]
fn a_problems_row_selects_its_texture_and_decides_in_place() {
    use engine::codes::Code;
    use sdc_gui::model::items::Decision;
    use sdc_gui::model::Selection;
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    // The alpha question only follows a sub-type.
    m.decide(
        "landscape/leaf.png",
        sdc_gui::model::Answer::Subtype(engine::config::Subtype::Diffuse),
    );
    assert!(m.wait_idle(Duration::from_secs(60)));
    let mut h = harness(m);
    h.state_mut().open_problems(None);
    h.run();
    h.get_by_label("landscape/odd.png").click();
    h.run();
    assert!(matches!(&h.state().selection, Selection::Item(k) if k == "landscape/odd.png"));
    // Rows follow the item order, so odd is the last row with an Acknowledge.
    let n = h.query_all_by_label("Acknowledge").count();
    assert!(n >= 1);
    h.query_all_by_label("Acknowledge").last().unwrap().click();
    idle(&mut h);
    let has = |h: &Harness<'static, AppModel>, k: &str, f: fn(&Decision) -> bool| {
        h.state().item(k).unwrap().decisions.iter().any(f)
    };
    let ack = |d: &Decision| matches!(d, Decision::Acknowledge(Code::DimNpot));
    assert!(!has(&h, "landscape/odd.png", ack), "odd lost its question");
    let alpha = |d: &Decision| matches!(d, Decision::AlphaMeaning);
    assert!(has(&h, "landscape/leaf.png", alpha));
    h.get_by_label("Decide…").click();
    h.run();
    h.get_by_label("Cutout").click();
    idle(&mut h);
    assert!(
        !has(&h, "landscape/leaf.png", alpha),
        "leaf lost its alpha question"
    );
}

#[test]
fn a_read_only_project_disables_the_fix_buttons_with_the_reason() {
    let d = tempfile::tempdir().unwrap();
    let root = mod_folder(d.path());
    std::fs::write(root.join("sdc.toml"), "target = [\n").unwrap();
    let settings = sdc_gui::settings::GuiSettings {
        export_folder: Some(d.path().join("export")),
        ..Default::default()
    };
    let mut m = AppModel::new(settings, None, session());
    m.accept = engine::policy::AcceptSuggestions::None;
    m.open_folder(root);
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.open_problems(None);
    let mut h = harness(m);
    h.run();
    let mut seen = 0;
    for label in ["Set type…", "Decide…", "Acknowledge"] {
        for n in h.query_all_by_label(label) {
            seen += 1;
            assert!(n.accesskit_node().is_disabled(), "{label} is enabled");
        }
    }
    assert!(seen >= 3, "only {seen} fix buttons found");
    assert!(h
        .state()
        .fix_off()
        .unwrap()
        .contains("sdc.toml is read-only"));
}

/// A mod folder with one short normal (decodes to length 0.3, R != G) and a model opened on it.
fn short_normal_model(dir: &std::path::Path, toml: Option<&str>) -> AppModel {
    let root = mod_folder(dir);
    image::ImageBuffer::from_fn(64, 64, |_, _| image::Rgb([153u8, 140, 153]))
        .save(root.join("landscape/x_n.png"))
        .unwrap();
    if let Some(t) = toml {
        std::fs::write(root.join("sdc.toml"), t).unwrap();
    }
    let settings = sdc_gui::settings::GuiSettings {
        export_folder: Some(dir.join("export")),
        ..Default::default()
    };
    let mut m = AppModel::new(settings, None, session());
    m.accept = engine::policy::AcceptSuggestions::None;
    m.open_folder(root);
    assert!(m.wait_idle(Duration::from_secs(60)));
    choose_export(&mut m, dir.join("export"));
    m.open_problems(None);
    m
}

fn z_row_code(m: &AppModel, code: engine::codes::Code) -> bool {
    sdc_gui::model::bottom::problems(m).iter().any(|p| {
        p.code == Some(code)
            && matches!(&p.at, sdc_gui::model::bottom::At::Texture(k) if k == "landscape/x_n.png")
    })
}

#[test]
fn the_z_row_offers_rebuild_z_and_it_clears_the_row() {
    use engine::codes::Code;
    let d = tempfile::tempdir().unwrap();
    let mut m = short_normal_model(d.path(), None);
    m.decide(
        "landscape/x_n.png",
        sdc_gui::model::Answer::Subtype(engine::config::Subtype::Normal),
    );
    m.decide(
        "landscape/x_n.png",
        sdc_gui::model::Answer::Green(engine::config::GreenConvention::Directx),
    );
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(z_row_code(&m, Code::ChanNormalLength));
    let mut h = harness(m);
    h.run();
    h.get_by_label(sdc_gui::ui::problems::REBUILD_Z).click();
    idle(&mut h);
    let v = h
        .state()
        .project
        .as_ref()
        .unwrap()
        .file_value("landscape/x_n.png", "channels.reconstruct_z");
    assert_eq!(v, Some(serde_json::json!(true)));
    assert!(!z_row_code(h.state(), Code::ChanNormalLength));
    assert!(!z_row_code(h.state(), Code::SugContradiction));
}

/// Note DP4: the last export repeats the live warning on an `At::File` row; only the live row is listed.
#[test]
fn an_exported_short_normal_still_shows_one_z_row() {
    use engine::codes::Code;
    use sdc_gui::model::bottom::{problems, At};
    let d = tempfile::tempdir().unwrap();
    let mut m = short_normal_model(d.path(), None);
    decide_all(&mut m);
    m.decide(
        "landscape/x_n.png",
        sdc_gui::model::Answer::Subtype(engine::config::Subtype::Normal),
    );
    m.decide(
        "landscape/x_n.png",
        sdc_gui::model::Answer::Green(engine::config::GreenConvention::Directx),
    );
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.export_clicked();
    assert!(m.wait_idle(Duration::from_secs(120)));
    let z = |m: &AppModel| {
        problems(m)
            .iter()
            .filter(|p| p.code == Some(Code::ChanNormalLength))
            .map(|p| p.at.clone())
            .collect::<Vec<_>>()
    };
    let rows = &m.export.rows;
    assert!(
        rows.iter()
            .any(|r| r.warnings.iter().any(|w| w.code == Code::ChanNormalLength)),
        "the export row carries the warning"
    );
    assert_eq!(z(&m), vec![At::Texture("landscape/x_n.png".into())]);
    m.open_problems(None);
    let mut h = harness(m);
    h.run();
    h.get_by_label(sdc_gui::ui::problems::REBUILD_Z).click();
    idle(&mut h);
    assert!(!z(h.state()).contains(&At::Texture("landscape/x_n.png".into())));
}

#[test]
fn rebuild_z_is_disabled_on_a_read_only_sdc_toml() {
    let d = tempfile::tempdir().unwrap();
    let mut m = short_normal_model(d.path(), Some("target = [\n"));
    // Read-only means no decision can be made, so the row is put on the item directly.
    let w = engine::codes::Warning::new(engine::codes::Code::ChanNormalLength, "not unit length");
    let i = m
        .items
        .iter_mut()
        .find(|i| i.key == "landscape/x_n.png")
        .unwrap();
    i.status = sdc_gui::model::items::ItemStatus::Ready;
    i.warnings = vec![w];
    m.invalidate_views();
    let mut h = harness(m);
    h.run();
    let b = h.get_by_label(sdc_gui::ui::problems::REBUILD_Z);
    assert!(b.accesskit_node().is_disabled());
    h.ctx.global_style_mut(|s| {
        s.interaction.tooltip_delay = 0.0;
        s.interaction.show_tooltips_only_when_still = false;
    });
    b.hover();
    h.run();
    h.run();
    h.get_by_label_contains("sdc.toml is read-only");
}

#[test]
fn the_problems_tab_draws_only_the_rows_on_screen() {
    let d = tempfile::tempdir().unwrap();
    let mut m = many_undecided(d.path(), 2000);
    m.open_problems(None);
    let mut h = harness(m);
    h.run();
    let shown = h.query_all_by_label_contains("t/t0").count();
    assert!((1..100).contains(&shown), "{shown} problem links");
    // One build, and the filtered list is not rebuilt per frame.
    let builds = sdc_gui::model::bottom::problems_builds(h.state());
    let filters = sdc_gui::model::bottom::filtered_builds(h.state());
    assert!(filters >= 1);
    h.run_steps(3);
    assert_eq!(sdc_gui::model::bottom::problems_builds(h.state()), builds);
    assert_eq!(sdc_gui::model::bottom::filtered_builds(h.state()), filters);
}

/// What the columns should draw: a dot per ready tile and per ready set.
fn ready_dots(m: &AppModel) -> (usize, usize) {
    use sdc_gui::model::bottom::{set_ready, tile_ready};
    let tiles = m.items.iter().filter(|i| tile_ready(i)).count();
    let sets = sdc_gui::model::sets::set_views(m)
        .iter()
        .filter(|v| set_ready(m, v))
        .count();
    (tiles, sets)
}

#[test]
fn a_ready_set_and_tile_show_a_green_dot() {
    use sdc_gui::model::bottom::{set_ready, tile_ready};
    let d = tempfile::tempdir().unwrap();
    let m = opened(d.path());
    let v = rock01(&m);
    assert!(!set_ready(&m, &v), "undecided members");
    assert!(!tile_ready(m.item("landscape/rock01.png").unwrap()));
    let (tiles, sets) = ready_dots(&m);
    let mut h = harness(m);
    h.run();
    assert_eq!(h.query_all_by_label("ready").count(), tiles + sets);
    decide_all(h.state_mut());
    // The rest of the fixture warns (neutral defaults, shader notes); a clean project is what turns green.
    for i in h.state_mut().items.iter_mut() {
        i.warnings.clear();
    }
    for st in h.state_mut().sets.iter_mut() {
        st.warnings.clear();
    }
    h.state_mut().invalidate_views();
    h.run();
    let (tiles, sets) = ready_dots(h.state());
    assert!(set_ready(h.state(), &rock01(h.state())));
    assert!(tiles > 0 && sets > 0, "tiles {tiles}, sets {sets}");
    assert_eq!(h.query_all_by_label("ready").count(), tiles + sets);
}

#[test]
fn the_other_warnings_link_in_review_opens_the_problems_tab() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.open_review();
    let mut h = harness(m);
    h.run();
    assert_eq!(h.state().bottom.tab, BottomTab::Review);
    h.get_by_label_contains("in Problems (do not block)")
        .click();
    h.run();
    assert!(h.state().bottom.open && h.state().bottom.tab == BottomTab::Problems);
    assert!(h.state().bottom.filter.is_none());
}

#[test]
/// Note FB1 (was the A12 dot): the header Export is clean only with a folder open and chosen and nothing undecided.
fn the_header_export_is_clean_only_with_a_folder_and_nothing_undecided() {
    let m = AppModel::new(Default::default(), None, session());
    assert!(!m.export_is_clean(), "no folder");
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    assert!(!m.export_is_clean(), "textures are undecided");
    decide_all(&mut m);
    // K3: the fixture's flat, alpha-less rock01_normal raises CHK-PLASTIC; this test is about the other rows.
    m.set_checks(sdc_gui::settings::ChecksSettings {
        plastic: false,
        ..Default::default()
    });
    m.poll();
    for i in m.items.iter_mut() {
        i.warnings.clear();
    }
    for st in m.sets.iter_mut() {
        st.warnings.clear();
    }
    m.invalidate_views();
    {
        assert!(sdc_gui::model::bottom::problems(&m).is_empty());
        assert!(m.export_is_clean());
    }
    // Nothing to export (an empty project, or items cleared while the plan runs) is never "ready".
    m.items.clear();
    m.invalidate_views();
    assert!(!m.export_is_clean(), "nothing to export");
    assert!(!m.header_export_text().starts_with("Export "));
}

#[test]
fn the_set_editor_points_to_problems_instead_of_listing_codes() {
    use sdc_gui::model::bottom::ProblemsFilter;
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let key = rock01(&m).key;
    m.select_set(&key);
    let mut h = harness(m);
    h.run();
    assert!(h
        .query_all_by_label_contains("IN-GREEN-UNSTATED:")
        .next()
        .is_none());
    h.get_by_label_contains("in this set — Show in Problems")
        .click();
    h.run();
    assert_eq!(h.state().bottom.filter, Some(ProblemsFilter::Set(key)));
}

#[test]
fn the_status_dot_names_its_state() {
    use sdc_gui::ui::status::{dot, Status};
    let mut h = Harness::new_ui(|ui| {
        for s in [Status::Blocks, Status::Warning, Status::Ready] {
            dot(ui, s);
        }
    });
    h.run();
    for l in ["blocks export", "warning", "ready"] {
        h.get_by_label(l);
    }
}

/// Final review I2: at 1280x800 a long pitfall row cuts its sentence (full text on hover) and keeps its buttons
/// inside the panel.
#[test]
fn at_1280x800_a_long_problems_row_keeps_its_buttons_on_screen() {
    use serde_json::json;
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    m.set_json_value("landscape/rock01", "match8", Some(json!(r"x\leaf_s.dds")));
    m.set_json_value("landscape/rock01", "alphaprop_thresh", Some(json!(128)));
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.open_problems(None);
    let mut h = harness_at(m, egui::vec2(1280.0, 800.0));
    idle(&mut h);
    let panel = outer(&h, "bottom");
    let mut seen = 0;
    for label in ["Go to set", "Set type…", "Decide…", "Acknowledge"] {
        for n in h.query_all_by_label(label) {
            seen += 1;
            assert!(
                panel.contains_rect(n.rect()),
                "{label} at {:?} is outside the panel {panel:?}",
                n.rect()
            );
        }
    }
    assert!(seen >= 3, "only {seen} buttons");
    let long = sdc_gui::model::pbr_export::export_asks(h.state())
        .into_iter()
        .map(|a| a.text)
        .max_by_key(|t| t.len())
        .unwrap();
    h.get_by_label(&long).hover();
    h.run();
    // The row is the only node with the sentence until the hover adds the tooltip.
    assert!(
        h.query_all_by_label(&long).count() > 1,
        "the hover shows the sentence"
    );
}

/// `problems()` calls over one settled frame, the bottom panel closed (Problems' rows would add their own).
fn lookups_per_frame(m: AppModel) -> u64 {
    use sdc_gui::model::bottom::problem_lookups;
    // K8: the height catch-up takes a frame per newly exposed card.
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_max_steps(16)
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    assert!(!h.state().bottom.open);
    let before = problem_lookups(h.state());
    h.step();
    problem_lookups(h.state()) - before
}

#[test]
fn set_cards_do_not_look_up_problems_per_card() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let small = lookups_per_frame(opened(a.path()));
    let big = lookups_per_frame(many_sets(b.path(), 500));
    assert_eq!(
        small, big,
        "3 sets: {small} lookups a frame, 500 sets: {big}"
    );
}

/// What `export_files` counts, without the cache.
fn uncached_export_files(m: &AppModel) -> usize {
    use sdc_gui::model::items::ItemStatus;
    m.items
        .iter()
        .filter(|i| {
            i.job.is_some()
                && !matches!(
                    i.status,
                    ItemStatus::NeedsDecision | ItemStatus::PackedInto(_) | ItemStatus::Skipped(_)
                )
        })
        .count()
}

#[test]
fn export_files_follows_a_decision_without_a_frame_of_lag() {
    use sdc_gui::model::bottom::export_files_builds;
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    assert!(m.undecided() > 0);
    // Review reads `export_files` every frame (its button text and gate).
    m.open_review();
    let mut h = harness(m);
    h.run();
    let builds = export_files_builds(h.state());
    assert!(builds >= 1);
    h.run_steps(3);
    assert_eq!(
        export_files_builds(h.state()),
        builds,
        "cached across frames"
    );
    let before = h.state().export_files();
    assert!(h.state().header_export_text().starts_with("Review "));
    decide_all(h.state_mut());
    // The text follows the last decision with no frame in between.
    assert!(
        h.state().header_export_text().starts_with("Export "),
        "{}",
        h.state().header_export_text()
    );
    h.run();
    assert_eq!(export_files_builds(h.state()), builds + 1);
    let m = h.state();
    let files = uncached_export_files(m);
    assert!(
        files > before,
        "the decisions add files ({before} -> {files})"
    );
    assert_eq!(m.export_files(), files);
    assert!(
        m.header_export_text().ends_with(&format!("{files} files")),
        "{}",
        m.header_export_text()
    );
    // Undo until a texture is undecided again: the count drops on the next frame too.
    for _ in 0..7 {
        h.state_mut().undo();
        assert!(h.state_mut().wait_idle(Duration::from_secs(60)));
        if uncached_export_files(h.state()) < files {
            break;
        }
    }
    h.run();
    let m = h.state();
    assert_eq!(m.export_files(), uncached_export_files(m));
    assert!(m.export_files() < files, "undo removes a file");
    // Directly, with no frame between: `export_files` must notice the new view key itself.
    h.state_mut().items.clear();
    h.state_mut().invalidate_views();
    assert_eq!(h.state().export_files(), 0);
}

#[test]
fn a_ready_set_that_gains_a_warning_loses_its_green_dot() {
    use sdc_gui::model::bottom::set_ready;
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    decide_all(&mut m);
    for i in m.items.iter_mut() {
        i.warnings.clear();
    }
    for st in m.sets.iter_mut() {
        st.warnings.clear();
    }
    m.invalidate_views();
    let mut h = harness(m);
    h.run();
    let ready = h.query_all_by_label("ready").count();
    assert!(set_ready(h.state(), &rock01(h.state())));
    // A warning on a member: the cached flagged keys must follow the new problems build.
    let w = engine::codes::Warning::new(engine::codes::Code::LossReencode, "re-encoded");
    let key = rock01(h.state()).members[0].clone();
    h.state_mut()
        .items
        .iter_mut()
        .find(|i| i.key == key)
        .unwrap()
        .warnings
        .push(w);
    h.state_mut().invalidate_views();
    h.run();
    assert!(!set_ready(h.state(), &rock01(h.state())));
    assert!(h.query_all_by_label("ready").count() < ready);
}

/// Owner 2026-10-20 ("1 yes"): "Checks {n}" narrows Problems to the artist checks' rows, "Checks only ×" clears it.
#[test]
fn the_checks_filter_shows_only_check_rows() {
    use engine::codes::{Code, Warning};
    use sdc_gui::model::bottom::{problems, Level, ProblemsFilter};
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    decide_all(&mut m);
    // Only the plastic check: the fixture's flat, alpha-less rock01_normal raises it.
    let plastic = sdc_gui::settings::ChecksSettings {
        range: false,
        plastic: true,
        low_detail: false,
        green: false,
        parallax: false,
        uv: false,
        drop_top_mip: false,
    };
    m.set_checks(plastic);
    m.poll();
    for i in m.items.iter_mut() {
        i.warnings.clear();
    }
    for st in m.sets.iter_mut() {
        st.warnings.clear();
    }
    m.items
        .iter_mut()
        .find(|i| i.key == "landscape/rock01.png")
        .unwrap()
        .warnings = vec![Warning::new(Code::ChanNotGray, "a plain warning row")];
    m.invalidate_views();
    let levels: Vec<Level> = problems(&m).iter().map(|p| p.level).collect();
    assert_eq!(
        levels,
        vec![Level::Warning, Level::Check],
        "{:?}",
        problems(&m)
    );
    // Note FB8: a row shows its code's sentence.
    let check = problems(&m)[1].shown().to_string();
    let plain = sdc_gui::model::code_text::code_sentence(Code::ChanNotGray);
    m.open_problems(None);
    let mut h = harness(m);
    h.run();
    let shown = |h: &Harness<'static, AppModel>, t: &str| h.query_all_by_label(t).count();
    assert_eq!(shown(&h, plain), 1);
    assert_eq!(shown(&h, &check), 1);
    h.get_by_label("Checks 1").click();
    h.run();
    assert_eq!(h.state().bottom.filter, Some(ProblemsFilter::Checks));
    assert_eq!(shown(&h, plain), 0);
    assert_eq!(shown(&h, &check), 1);
    assert!(
        h.query_by_label("Checks 1").is_none(),
        "the chip replaces the button"
    );
    h.get_by_label("Checks only ×").click();
    h.run();
    assert_eq!(h.state().bottom.filter, None);
    assert_eq!(shown(&h, plain), 1);
    assert_eq!(shown(&h, &check), 1);
    // With no check rows there is no Checks button.
    h.state_mut().set_checks(sdc_gui::settings::ChecksSettings {
        plastic: false,
        ..plastic
    });
    h.state_mut().poll();
    h.run();
    assert_eq!(shown(&h, &check), 0);
    assert!(h.query_by_label_contains("Checks").is_none());
}
