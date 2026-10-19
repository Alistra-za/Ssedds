//! Task 3 (item 3, AG3, AG17-AG20): the window and layout are remembered in gui.toml.
mod support;
use eframe::egui::{self, pos2, vec2, Rect};
use sdc_gui::layout::{fit_window, initial_window, native_points, SaveDebounce, DEFAULT_SIZE};
use sdc_gui::model::view::{Channel, Layout};
use sdc_gui::model::viewport3d::Look;
use sdc_gui::settings::*;
use std::time::{Duration, Instant};

fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::from_min_size(pos2(x, y), vec2(w, h))
}

fn full() -> LayoutSettings {
    LayoutSettings {
        window: Some(WindowSettings {
            x: 120.0,
            y: 80.0,
            w: 1500.0,
            h: 950.0,
            ppp: 1.5,
            maximized: true,
        }),
        widths: [("textures".to_string(), 410.0), ("sets".to_string(), 275.5)].into(),
        meshes_view: true,
        filters: vec!["warnings".into(), "not-in-set".into()],
        filter_subtype: Some("normal".into()),
        filter_format: None,
        filter_encoder: Some("nvtt".into()),
        filter_set: "in".into(),
        sort: "size".into(),
        tile: 128.0,
        sets_filters: vec!["attention".into(), "no-json".into()],
        sets_shader: Some("pbr_landscape".into()),
        preview_channel: 3,
        preview_layout: "split".into(),
        look: Some(LookSettings {
            hdri: Some(true),
            grey: None,
            bg: Some([0.25; 3]),
            sun: Some(2.0),
            point: Some(1.5),
            radius: Some(3.0),
            sun_yaw: Some(0.5),
        }),
        set_look: None,
        settings_window: Some([300.0, 140.0, 460.0, 600.0]),
        presets_window: Some([80.0, 60.0, 900.0, 700.0]),
        details: vec!["set-3d-details".into()],
    }
}

fn load_text(text: &str) -> (GuiSettings, Option<String>) {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("gui.toml");
    std::fs::write(&p, text).unwrap();
    GuiSettings::load(&p)
}

#[test]
fn the_layout_round_trips_through_gui_toml() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("sdc/gui.toml");
    let s = GuiSettings {
        layout: full(),
        set_view_3d: true,
        ..GuiSettings::default()
    };
    s.save(&p).unwrap();
    let (back, err) = GuiSettings::load(&p);
    assert_eq!(err, None);
    assert_eq!(back, s);
}

#[test]
fn a_missing_or_old_file_gives_todays_defaults() {
    let (s, err) = load_text("preview_max = 2048\nset_view_3d = true\n");
    assert_eq!(err, None);
    assert_eq!(s.layout, LayoutSettings::default());
    assert!(s.set_view_3d, "the 2D | 3D switch is kept (AB4)");
    assert_eq!(s.layout.window, None);
    let w = initial_window(&s.layout, &[rect(0.0, 0.0, 1920.0, 1080.0)]);
    assert_eq!((w.size, w.pos, w.maximized), (DEFAULT_SIZE, None, false));
    assert_eq!(DEFAULT_SIZE, [1400.0, 900.0]);
    assert!(!GuiSettings::default().set_view_3d, "2D stays the default");
}

#[test]
fn a_bad_value_is_ignored_on_its_own() {
    let (s, err) = load_text(
        r#"
preview_max = 2048
[layout]
meshes_view = true
filters = ["warnings", "bogus", 7, "failed"]
preview_channel = 99
preview_layout = "diagonal"
[layout.widths]
textures = 120.0
sets = 300
set-3d = "wide"
nonsense = 500.0
advanced = 1.0e9
[layout.window]
x = 10.0
y = 10.0
w = "wide"
h = 700.0
ppp = 1.0
maximized = false
[layout.look]
grey = 7.0
sun = "bright"
point = 1.0
radius = 0.0
"#,
    );
    assert_eq!(err, None, "no whole-file failure");
    assert_eq!(s.preview_max, 2048, "the rest of the file is read");
    let l = &s.layout;
    assert!(l.meshes_view, "a valid sibling survives");
    assert_eq!(
        l.filters,
        vec!["warnings".to_string(), "failed".to_string()]
    );
    assert_eq!(l.preview_channel, 0, "channel 99 is ignored");
    assert_eq!(l.preview_layout, "side-by-side");
    assert_eq!(
        l.widths,
        [("sets".to_string(), 300.0)].into(),
        "width < 200, text, unknown names and absurd widths are dropped"
    );
    assert_eq!(l.window, None, "a window with a bad field is ignored");
    let look = l.look.expect("the look table is read");
    assert_eq!(look.grey, None, "folded into bg on load");
    assert_eq!(
        look.bg,
        Some([1.0; 3]),
        "the legacy grey, clamped to its range"
    );
    assert_eq!(look.sun, None, "text is ignored");
    assert_eq!(look.point, Some(1.0));
    assert_eq!(look.radius, Some(0.1), "clamped to its range");
}

#[test]
fn a_whole_layout_of_the_wrong_type_is_ignored() {
    let (s, err) = load_text("layout = 5\nlive_encode = true\n");
    assert_eq!(err, None);
    assert!(s.live_encode);
    assert_eq!(s.layout, LayoutSettings::default());
}

#[test]
fn channels_layouts_and_looks_convert_both_ways() {
    for c in [
        Channel::Rgba,
        Channel::R,
        Channel::G,
        Channel::B,
        Channel::A,
        Channel::AlphaMask,
    ] {
        assert_eq!(Channel::from_id(c.shader_id()), Some(c));
    }
    assert_eq!(Channel::from_id(99), None);
    for l in [Layout::SideBySide, Layout::Split] {
        assert_eq!(Layout::from_name(l.name()), Some(l));
    }
    assert_eq!(Layout::from_name("diagonal"), None);
    let look = Look {
        hdri: true,
        bg: [0.5, 0.25, 0.0],
        sun: 2.0,
        point: 1.0,
        radius: 4.0,
        sun_yaw: 1.5,
    };
    assert_eq!(Look::from_settings(Some(&LookSettings::of(&look))), look);
    assert_eq!(Look::from_settings(None), Look::default());
    let partial = LookSettings {
        sun: Some(3.0),
        ..LookSettings::default()
    };
    assert_eq!(
        Look::from_settings(Some(&partial)),
        Look {
            sun: 3.0,
            ..Look::default()
        }
    );
}

// ---- the off-screen clamp

fn mon() -> Rect {
    rect(0.0, 0.0, 1920.0, 1080.0)
}

#[test]
fn a_rect_that_fits_is_kept() {
    let r = rect(100.0, 50.0, 1400.0, 900.0);
    assert_eq!(fit_window(r, &[mon()]), r);
}

#[test]
fn no_overlap_centres_on_the_primary_at_the_saved_size() {
    // Saved on a second monitor that is gone.
    let r = fit_window(rect(3000.0, 100.0, 1400.0, 900.0), &[mon()]);
    assert_eq!(r.size(), vec2(1400.0, 900.0));
    assert_eq!(r.center(), mon().center());
}

#[test]
fn no_overlap_caps_the_size_to_90_percent_of_the_monitor() {
    let r = fit_window(rect(5000.0, 0.0, 3000.0, 2000.0), &[mon()]);
    assert_eq!(r.size(), vec2(1920.0 * 0.9, 1080.0 * 0.9));
    assert_eq!(r.center(), mon().center());
}

#[test]
fn a_10_px_overlap_is_off_screen() {
    // 10 px of the window still touches the monitor's right edge.
    let r = fit_window(rect(1910.0, 100.0, 800.0, 600.0), &[mon()]);
    assert_eq!(r.center(), mon().center());
    // Exactly 64 px on both axes stays.
    let kept = rect(1856.0, 1016.0, 800.0, 600.0);
    assert_eq!(fit_window(kept, &[mon()]), kept);
}

#[test]
fn a_window_bigger_than_the_screen_shrinks_to_it() {
    let r = fit_window(rect(-200.0, -100.0, 3000.0, 1500.0), &[mon()]);
    assert_eq!(r, mon());
}

#[test]
fn any_monitor_counts_and_with_no_monitor_info_the_rect_is_kept() {
    let second = rect(1920.0, 0.0, 1920.0, 1080.0);
    let on_second = rect(2000.0, 100.0, 1400.0, 900.0);
    assert_eq!(fit_window(on_second, &[mon(), second]), on_second);
    assert_eq!(fit_window(on_second, &[]), on_second);
}

#[test]
fn the_initial_window_is_in_points_and_uses_the_saved_scale() {
    let l = LayoutSettings {
        window: Some(WindowSettings {
            x: 100.0,
            y: 50.0,
            w: 1000.0,
            h: 700.0,
            ppp: 2.0,
            maximized: false,
        }),
        ..LayoutSettings::default()
    };
    // The monitors are in pixels: at 2x the window is 2000 x 1400 px, bigger than the 1920 x 1080 monitor.
    let w = initial_window(&l, &[mon()]);
    assert_eq!(w.size, [960.0, 540.0]);
    assert_eq!(w.pos, Some([0.0, 0.0]));
    assert!(!w.maximized);
    // No monitor info: the size is applied, the OS places the window.
    let w = initial_window(&l, &[]);
    assert_eq!((w.size, w.pos), ([1000.0, 700.0], None));
    // Fits: the saved points come back as they were.
    let w = initial_window(&l, &[rect(0.0, 0.0, 4000.0, 3000.0)]);
    assert_eq!((w.size, w.pos), ([1000.0, 700.0], Some([100.0, 50.0])));
}

// ---- the debounce

#[test]
fn two_changes_inside_two_seconds_give_one_write() {
    let t0 = Instant::now();
    let mut d = SaveDebounce::default();
    assert!(!d.due(t0), "nothing changed");
    d.changed(t0);
    d.changed(t0 + Duration::from_millis(900));
    assert!(!d.due(t0 + Duration::from_millis(1900)));
    assert!(d.due(t0 + Duration::from_millis(2000)), "one write");
    assert!(!d.due(t0 + Duration::from_millis(2100)), "and not another");
    d.changed(t0 + Duration::from_secs(5));
    assert!(!d.due(t0 + Duration::from_secs(6)));
    assert!(d.due(t0 + Duration::from_secs(7)));
}

#[test]
fn the_app_writes_a_changed_layout_once_per_two_seconds_and_on_exit() {
    use sdc_gui::app::SdcApp;
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("sdc/gui.toml");
    let model = sdc_gui::model::AppModel::new(
        GuiSettings::default(),
        Some(path.clone()),
        support::session(),
    );
    let mut app = SdcApp::new(model);
    let ctx = egui::Context::default();
    let t0 = Instant::now();
    app.sync_layout_at(&ctx, t0);
    assert!(!path.exists(), "nothing changed, nothing written");
    app.model.filter.warnings = true;
    app.sync_layout_at(&ctx, t0 + Duration::from_millis(100));
    app.model.view.channel = Channel::G;
    app.model.view.layout = Layout::Split;
    app.sync_layout_at(&ctx, t0 + Duration::from_millis(1500));
    assert!(!path.exists(), "inside the 2 s");
    app.sync_layout_at(&ctx, t0 + Duration::from_millis(2200));
    let (s, _) = GuiSettings::load(&path);
    assert_eq!(s.layout.filters, vec!["warnings".to_string()]);
    assert_eq!(s.layout.preview_channel, Channel::G.shader_id() as u8);
    assert_eq!(s.layout.preview_layout, "split");
    // A change after that waits for its own 2 s, but the exit writes it.
    app.model.filter.failed = true;
    app.sync_layout_at(&ctx, t0 + Duration::from_millis(2300));
    assert_eq!(GuiSettings::load(&path).0.layout.filters.len(), 1);
    eframe::App::on_exit(&mut app);
    assert_eq!(GuiSettings::load(&path).0.layout.filters.len(), 2);
}

#[test]
fn a_new_app_starts_from_the_saved_layout() {
    use sdc_gui::app::SdcApp;
    let settings = GuiSettings {
        layout: LayoutSettings {
            set_look: Some(LookSettings {
                sun: Some(0.5),
                ..LookSettings::default()
            }),
            ..full()
        },
        ..GuiSettings::default()
    };
    let model = sdc_gui::model::AppModel::placeholder(settings);
    let mut app = SdcApp::new(model);
    assert!(app.model.filter.warnings && app.model.filter.not_in_set);
    assert!(!app.model.filter.failed && !app.model.filter.needs_decision);
    let s = &app.model.sets_filter;
    assert!(s.attention && s.no_json && !s.ready);
    assert_eq!(s.shader, Some(engine::shader::Shader::PbrLandscape));
    assert_eq!(app.model.view.channel, Channel::B);
    assert_eq!(app.model.view.layout, Layout::Split);
    assert_eq!(
        app.meshes.v3.look,
        Look {
            hdri: true,
            bg: [0.25; 3],
            sun: 2.0,
            point: 1.5,
            radius: 3.0,
            sun_yaw: 0.5
        }
    );
    assert_eq!(app.state.set3d.v3.look.sun, 0.5);
    let ctx = egui::Context::default();
    app.restore_ctx(&ctx);
    assert!(sdc_gui::ui::header::meshes_view_active(&ctx));
    // The same layout comes back out of the context, the partial set look included.
    app.sync_layout_at(&ctx, Instant::now());
    let mut want = full();
    want.set_look = Some(LookSettings {
        sun: Some(0.5),
        ..LookSettings::default()
    });
    assert_eq!(app.model.settings.layout, want);
}

#[test]
fn saved_column_widths_come_back_in_the_real_panels() {
    use eframe::egui::containers::panel::PanelState;
    use sdc_gui::ui::State;
    let d = tempfile::tempdir().unwrap();
    let layout = LayoutSettings {
        widths: [("textures".to_string(), 520.0), ("sets".to_string(), 333.0)].into(),
        ..LayoutSettings::default()
    };
    let h = egui_kittest::Harness::builder()
        .with_size(vec2(1800.0, 900.0))
        .build_ui_state(
            |ui, (m, s): &mut (sdc_gui::model::AppModel, State)| sdc_gui::ui::draw_with(ui, m, s),
            (support::opened(d.path()), State::default()),
        );
    sdc_gui::layout::restore_ctx(&h.ctx, &layout);
    let mut h = h;
    h.run();
    h.run();
    let w = |id: &str| PanelState::load(&h.ctx, egui::Id::new(id)).map(|s| s.outer_rect.width());
    assert_eq!(w("textures"), Some(520.0));
    assert_eq!(w("sets"), Some(333.0));
}

#[test]
fn the_monitor_list_has_positive_areas() {
    for m in sdc_gui::layout::monitors() {
        assert!(m.width() > 0.0 && m.height() > 0.0, "{m:?}");
    }
}

// ---- Task 3 review carry-over (Task 4)

#[test]
fn a_start_that_changes_nothing_writes_nothing() {
    use sdc_gui::app::SdcApp;
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("sdc/gui.toml");
    let model = sdc_gui::model::AppModel::new(
        GuiSettings::default(),
        Some(path.clone()),
        support::session(),
    );
    let mut app = SdcApp::new(model);
    let ctx = egui::Context::default();
    // The first frame reads parts the file did not have (here a panel width): that is the starting point, not a change.
    sdc_gui::layout::restore_ctx(
        &ctx,
        &LayoutSettings {
            widths: [("textures".to_string(), 500.0)].into(),
            ..LayoutSettings::default()
        },
    );
    let t0 = Instant::now();
    app.sync_layout_at(&ctx, t0);
    assert_eq!(
        app.model.settings.layout.widths.get("textures"),
        Some(&500.0)
    );
    app.sync_layout_at(&ctx, t0 + Duration::from_secs(3));
    assert!(
        !path.exists(),
        "a start with nothing changed wrote gui.toml"
    );
    eframe::App::on_exit(&mut app);
    assert!(!path.exists(), "and so did the exit");
    // A real change still writes.
    app.model.filter.failed = true;
    app.sync_layout_at(&ctx, t0 + Duration::from_secs(4));
    app.sync_layout_at(&ctx, t0 + Duration::from_secs(7));
    assert_eq!(
        GuiSettings::load(&path).0.layout.filters,
        vec!["failed".to_string()]
    );
}

#[test]
fn the_title_bar_stays_on_a_monitor() {
    // 100 px of the window show at the top of the monitor, but its title bar is 500 px above the screen.
    let r = fit_window(rect(100.0, -500.0, 800.0, 600.0), &[mon()]);
    assert_eq!(r.min, pos2(100.0, 0.0));
    assert_eq!(r.size(), vec2(800.0, 600.0));
    // A title bar a little above a second monitor's top that is above the first's comes down onto the second.
    let upper = rect(0.0, -1080.0, 1920.0, 1080.0);
    let r = fit_window(rect(300.0, -1100.0, 800.0, 600.0), &[mon(), upper]);
    assert_eq!(r.min, pos2(300.0, -1080.0));
    // One already on the screen stays.
    let r = rect(100.0, 10.0, 800.0, 600.0);
    assert_eq!(fit_window(r, &[mon()]), r);
}

#[test]
fn the_preview_settings_panel_width_is_saved_too() {
    assert!(WIDTH_IDS.contains(&"preview-settings"));
    let (s, _) = load_text("[layout.widths]\npreview-settings = 300.0\n");
    assert_eq!(s.layout.widths.get("preview-settings"), Some(&300.0));
}

#[test]
fn the_3d_panels_requests_reach_the_app() {
    use sdc_gui::app::SdcApp;
    let d = tempfile::tempdir().unwrap();
    let model = support::opened(d.path());
    let key = model
        .item_for_game_path("Textures/Landscape/Rock01.dds")
        .expect("rock01 is in the open folder");
    assert!(model
        .item_for_game_path("textures/elsewhere/x.dds")
        .is_none());
    let mut app = SdcApp::new(model);
    let ctx = egui::Context::default();
    sdc_gui::ui::header::set_meshes_view(&ctx, true);
    app.meshes.v3.report = Some(render3d::LoadReport {
        missing_textures: vec![
            "textures/landscape/rock01.dds".into(),
            "textures/elsewhere/x.dds".into(),
        ],
        ..Default::default()
    });
    app.route_3d(&ctx);
    assert_eq!(app.meshes.v3.missing_items, vec![Some(key.clone()), None]);
    app.meshes.v3.show_in_textures = Some(key.clone());
    app.route_3d(&ctx);
    assert!(
        !sdc_gui::ui::header::meshes_view_active(&ctx),
        "back to Textures"
    );
    assert_eq!(app.model.selection, sdc_gui::model::Selection::Item(key));
    assert!(app.meshes.v3.show_in_textures.is_none());
}

#[test]
fn a_window_saved_at_egui_zoom_restores_at_the_same_size() {
    // Zoom 1.25: egui reports the rects in zoomed points, a 1400 x 900 window as 1120 x 720.
    let (inner, outer) = (
        rect(88.0, 72.0, 1120.0, 720.0),
        rect(80.0, 64.0, 1136.0, 760.0),
    );
    let (i, o) = (native_points(inner, 1.25), native_points(outer, 1.25));
    let l = LayoutSettings {
        window: Some(WindowSettings {
            x: o.min.x,
            y: o.min.y,
            w: i.width(),
            h: i.height(),
            ppp: 1.0,
            maximized: false,
        }),
        ..LayoutSettings::default()
    };
    let w = initial_window(&l, &[rect(0.0, 0.0, 4000.0, 3000.0)]);
    assert_eq!((w.size, w.pos), ([1400.0, 900.0], Some([100.0, 80.0])));
}

#[test]
fn the_details_footers_open_state_is_saved_and_restored() {
    use sdc_gui::ui::viewport3d::{details_open, set_details_open, MESHES_DETAILS, SET_DETAILS};
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("sdc/gui.toml");
    let model = sdc_gui::model::AppModel::new(
        GuiSettings::default(),
        Some(path.clone()),
        support::session(),
    );
    let mut app = sdc_gui::app::SdcApp::new(model);
    let ctx = egui::Context::default();
    let t0 = Instant::now();
    app.sync_layout_at(&ctx, t0);
    set_details_open(&ctx, SET_DETAILS, true);
    app.sync_layout_at(&ctx, t0 + Duration::from_secs(1));
    app.sync_layout_at(&ctx, t0 + Duration::from_secs(4));
    let saved = GuiSettings::load(&path).0.layout;
    assert_eq!(saved.details, vec![SET_DETAILS.to_string()]);
    // A new start opens it again, and only it.
    let ctx = egui::Context::default();
    sdc_gui::layout::restore_ctx(&ctx, &saved);
    assert!(details_open(&ctx, SET_DETAILS));
    assert!(!details_open(&ctx, MESHES_DETAILS));
}

/// GUI suggestions item 2: `bg` replaced the grey slider. An old `grey` still loads (as that grey), and a broken
/// `bg` falls back to it.
#[test]
fn an_old_grey_background_loads_as_that_grey_and_a_bad_bg_falls_back() {
    let (s, err) = load_text("[layout.look]\ngrey = 0.25\n");
    assert_eq!(err, None);
    assert_eq!(Look::from_settings(s.layout.look.as_ref()).bg, [0.25; 3]);
    let (s, _) = load_text("[layout.look]\ngrey = 0.5\nbg = [1.0, 0.5]\n");
    assert_eq!(
        Look::from_settings(s.layout.look.as_ref()).bg,
        [0.5; 3],
        "wrong arity: bg ignored"
    );
    let (s, _) = load_text("[layout.look]\nbg = \"red\"\n");
    assert_eq!(
        Look::from_settings(s.layout.look.as_ref()).bg,
        Look::default().bg
    );
    let (s, _) = load_text("[layout.look]\nbg = [2.0, 0.5, -1.0]\n");
    assert_eq!(
        s.layout.look.unwrap().bg,
        Some([1.0, 0.5, 0.0]),
        "clamped per channel"
    );
    // What the app writes back has bg and no grey.
    let text = toml::to_string(&LookSettings::of(&Look {
        bg: [0.1, 0.2, 0.3],
        ..Look::default()
    }))
    .unwrap();
    assert!(text.contains("bg") && !text.contains("grey"), "{text}");
}

/// Task 4 review: an untouched legacy look is saved back as it was loaded (`look_now`), so the load must already have
/// turned `grey` into `bg`; an unrelated layout change then writes `bg` (same colour) and no `grey`.
#[test]
fn a_legacy_grey_is_saved_back_as_bg_after_an_unrelated_change() {
    use sdc_gui::app::SdcApp;
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("gui.toml");
    std::fs::write(&path, "[layout.look]\ngrey = 0.25\n").unwrap();
    let (settings, err) = GuiSettings::load(&path);
    assert_eq!(err, None);
    let model = sdc_gui::model::AppModel::new(settings, Some(path.clone()), support::session());
    let mut app = SdcApp::new(model);
    let ctx = egui::Context::default();
    let t0 = Instant::now();
    app.sync_layout_at(&ctx, t0);
    app.model.filter.failed = true;
    app.sync_layout_at(&ctx, t0 + Duration::from_secs(3));
    app.sync_layout_at(&ctx, t0 + Duration::from_secs(6));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("failed"),
        "the unrelated change was written: {text}"
    );
    assert!(text.contains("bg") && !text.contains("grey"), "{text}");
    let (back, _) = GuiSettings::load(&path);
    assert_eq!(back.layout.look.unwrap().bg, Some([0.25; 3]));
}

// ---- Layout v2 Task 4.3 fix round 1: the Advanced drawer keeps the width the user chose

type AppHarness = egui_kittest::Harness<'static, (sdc_gui::model::AppModel, sdc_gui::ui::State)>;

/// A 1280×800 app with Advanced open at a saved `width`, restored the way a start restores gui.toml.
fn advanced_at(width: f32, d: &std::path::Path) -> AppHarness {
    let mut model = support::opened(d);
    model.settings.advanced_open = true;
    let h = egui_kittest::Harness::builder()
        .with_size(vec2(1280.0, 800.0))
        .build_ui_state(
            |ui, (m, s): &mut (sdc_gui::model::AppModel, sdc_gui::ui::State)| {
                sdc_gui::ui::draw_with(ui, m, s)
            },
            (model, sdc_gui::ui::State::default()),
        );
    sdc_gui::layout::restore_ctx(
        &h.ctx,
        &LayoutSettings {
            widths: [("advanced".to_string(), width)].into(),
            ..LayoutSettings::default()
        },
    );
    let mut h = h;
    h.run();
    h
}

/// What a save would write for the Advanced drawer now.
fn saved_advanced(h: &AppHarness) -> Option<f32> {
    let m = &h.state().0;
    sdc_gui::layout::capture(
        &h.ctx,
        &LayoutSettings::default(),
        &m.filter,
        &m.prefs,
        &m.sets_filter,
        &m.view,
        (&Default::default(), &Default::default()),
    )
    .widths
    .get("advanced")
    .copied()
}

fn drawn_advanced(h: &AppHarness) -> f32 {
    egui::containers::panel::PanelState::load(&h.ctx, egui::Id::new("advanced"))
        .unwrap()
        .outer_rect
        .width()
}

/// At 1280 the drawer's cap is 656 with nothing expanded: a saved 700 is drawn at the cap and still saved as 700.
#[test]
fn a_saved_advanced_width_over_the_cap_is_kept_in_gui_toml() {
    let d = tempfile::tempdir().unwrap();
    let mut h = advanced_at(700.0, d.path());
    h.run();
    assert!(drawn_advanced(&h) < 700.0, "drawn {}", drawn_advanced(&h));
    assert_eq!(saved_advanced(&h), Some(700.0));
}

/// An expanded preview narrows the drawer; collapsing it gives the user's 600 back, and 600 is saved throughout.
#[test]
fn the_advanced_width_comes_back_after_a_preview_and_is_saved_unchanged() {
    let d = tempfile::tempdir().unwrap();
    let mut h = advanced_at(600.0, d.path());
    assert_eq!(drawn_advanced(&h), 600.0);
    h.state_mut().0.expand("landscape/rock01_rmaos");
    assert!(h
        .state_mut()
        .0
        .wait_idle(std::time::Duration::from_secs(60)));
    h.run();
    assert!(drawn_advanced(&h) < 600.0, "drawn {}", drawn_advanced(&h));
    assert_eq!(saved_advanced(&h), Some(600.0));
    h.state_mut().0.collapse();
    h.run();
    assert_eq!(drawn_advanced(&h), 600.0);
    assert_eq!(saved_advanced(&h), Some(600.0));
}

// ---- Layout v2 final review I3: a clamped Presets window never overwrites the saved size

/// The Presets window open in a `size` app with `saved` in gui.toml: (its drawn rect, the area under the header,
/// what a start then a real change writes to gui.toml as `presets_window`).
fn presets_saved_after(
    size: egui::Vec2,
    saved: Option<[f32; 4]>,
) -> (Rect, Rect, Option<[f32; 4]>) {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("sdc/gui.toml");
    let settings = GuiSettings {
        layout: LayoutSettings {
            presets_window: saved,
            ..LayoutSettings::default()
        },
        ..GuiSettings::default()
    };
    let mut m = sdc_gui::model::AppModel::placeholder(settings.clone());
    m.open_presets();
    // A draft fills the window (with none it shrinks to one line).
    m.presets_model().select("cs-quality");
    let mut h = egui_kittest::Harness::builder()
        .with_size(size)
        .build_ui_state(
            |ui, m: &mut sdc_gui::model::AppModel| sdc_gui::ui::draw(ui, m),
            m,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    h.run();
    let drawn = h
        .ctx
        .memory(|m| m.area_rect(sdc_gui::ui::presets::window_id()))
        .expect("the Presets window is open");
    let below = sdc_gui::ui::below_header(&h.ctx).expect("below-header recorded");
    let mut app = sdc_gui::app::SdcApp::new(sdc_gui::model::AppModel::new(
        settings,
        Some(path.clone()),
        support::session(),
    ));
    let t0 = Instant::now();
    app.sync_layout_at(&h.ctx, t0);
    app.model.filter.failed = true;
    app.sync_layout_at(&h.ctx, t0 + Duration::from_secs(1));
    app.sync_layout_at(&h.ctx, t0 + Duration::from_secs(4));
    let written = GuiSettings::load(&path).0.layout;
    assert_eq!(
        written.filters,
        vec!["failed".to_string()],
        "gui.toml was written"
    );
    (drawn, below, written.presets_window)
}

#[test]
fn a_presets_window_clamped_under_the_header_keeps_its_saved_size() {
    let saved = [80.0, 120.0, 860.0, 680.0];
    let (drawn, below, written) = presets_saved_after(vec2(1100.0, 700.0), Some(saved));
    assert!(
        drawn.height() < 680.0 && drawn.bottom() <= below.bottom() + 0.5,
        "clamped at 1100x700: {drawn:?} in {below:?}"
    );
    let written = written.expect("the rect is saved");
    // The clamped height keeps the saved one; the width, not clamped, is what egui drew. (Pre-existing, out of this
    // fix: a saved W x H draws as W+3 x H+3, so the width written is 863.)
    assert_eq!(
        written[3], saved[3],
        "the saved height is kept: {written:?}"
    );
    assert!(drawn.width() < below.width() - 1.0);
    assert_eq!(written[2], drawn.width(), "{written:?}");
    // Back in a large window, the saved height comes back, not the clamped one.
    let (drawn, _, _) = presets_saved_after(vec2(1600.0, 1000.0), Some(written));
    assert!(drawn.height() >= 680.0 - 0.5, "{drawn:?}");
}

#[test]
fn a_fresh_profile_never_saves_a_clamped_presets_window() {
    // 480 px tall at least with none saved, so a 520 px app clamps it.
    let (drawn, below, written) = presets_saved_after(vec2(1100.0, 520.0), None);
    assert!(
        drawn.height() >= below.height() - 2.0,
        "clamped: {drawn:?} in {below:?}"
    );
    assert_eq!(written, None, "a clamped first rect is not saved");
}
