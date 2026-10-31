//! v2 artist checks, Task 6 (DECISIONS Note CL): the value-range overlay. Built on a worker from the preview's source
//! levels, offered only for a texture with a per-texel CHK-RANGE warning, and polled through `v2_jobs_pending()`.
mod support;
use eframe::egui;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use engine::codes::Code;
use engine::image::Rgba32F;
use sdc_gui::model::checks::RangeRule;
use sdc_gui::model::heatmap::HeatImage;
use sdc_gui::model::overlay::{range_overlay, OverlayKind};
use sdc_gui::model::preview::SourceState;
use sdc_gui::model::AppModel;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const DARK: u8 = 20;
const PLAIN: u8 = 128;
const CLEAN_RMAOS: [u8; 3] = [128, 10, 200];
const METAL_RMAOS: [u8; 3] = [128, 255, 200];

fn level(codes: &[[u8; 3]]) -> Rgba32F {
    let mut l = Rgba32F::new(codes.len() as u32, 1, [0.0; 4]);
    for (p, c) in l.px.iter_mut().zip(codes) {
        *p = [
            f32::from(c[0]) / 255.0,
            f32::from(c[1]) / 255.0,
            f32::from(c[2]) / 255.0,
            1.0,
        ];
    }
    l
}

#[test]
fn range_overlay_paints_exactly_the_texels_a_rule_rejects() {
    // Grey luma 20, 100, 245, 128: the albedo rule rejects below 30 and above 240.
    let l = level(&[[20; 3], [100; 3], [245; 3], [128; 3]]);
    let img = range_overlay(&l, &[RangeRule::AlbedoLuma]);
    assert_eq!((img.width, img.height), (4, 1));
    let alpha: Vec<u8> = img.rgba.as_chunks::<4>().0.iter().map(|p| p[3]).collect();
    assert_eq!(alpha, [160, 0, 160, 0]);
    assert_eq!(&img.rgba[..4], &[255, 0, 255, 160]);
    assert_eq!(&img.rgba[4..8], &[0, 0, 0, 0]);
    // Any rule: rough extremes (R 0 or 255) or grey metal (G 26..=229).
    let rm = level(&[[0, 10, 0], [128, 128, 0], [128, 10, 0], [255, 255, 0]]);
    let img = range_overlay(&rm, &[RangeRule::RoughExtreme, RangeRule::MetalGrey]);
    let alpha: Vec<u8> = img.rgba.as_chunks::<4>().0.iter().map(|p| p[3]).collect();
    assert_eq!(alpha, [160, 160, 0, 160]);
    // Empty level, no rules: nothing painted, no panic.
    let empty = range_overlay(&Rgba32F::new(0, 0, [0.0; 4]), &[RangeRule::AlbedoLuma]);
    assert!(empty.rgba.is_empty());
    assert!(range_overlay(&l, &[]).rgba.iter().all(|&b| b == 0));
}

fn harness(model: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(egui::vec2(1600.0, 1000.0))
        .build_ui_state(
            |ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model),
            model,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

/// The Range toggle for `test/wall.png` expanded in a PBR set with that albedo and `_rmaos`.
fn range_toggle_shown(albedo: u8, rmaos: [u8; 3]) -> (bool, Vec<String>) {
    let d = tempfile::tempdir().unwrap();
    let mut m = support::pbr_wall(d.path(), albedo, rmaos);
    let rules: Vec<String> = m
        .check_warnings("test/wall.png")
        .iter()
        .filter(|w| w.code == Code::ChkRange)
        .map(|w| w.data["rule"].as_str().unwrap_or_default().to_string())
        .collect();
    m.expand("test/wall.png");
    assert!(m.wait_idle(Duration::from_secs(60)));
    let offered = m.range_overlay_offered("test/wall.png");
    let mut h = harness(m);
    h.run();
    // Note FC13 (CL28): the toggle is drawn for every texture; it is enabled only when a range rule applies.
    let node = h
        .query_by_label("Range overlay")
        .expect("the Range toggle is drawn for every texture");
    let shown = !node.accesskit_node().is_disabled();
    assert_eq!(shown, offered, "the toggle follows range_overlay_offered");
    (shown, rules)
}

#[test]
fn the_range_button_is_drawn_for_every_texture_and_enabled_only_with_a_range_warning() {
    let (shown, rules) = range_toggle_shown(DARK, CLEAN_RMAOS);
    assert_eq!(rules, ["albedo_luma"]);
    assert!(shown, "a dark albedo offers the overlay");
    let (shown, rules) = range_toggle_shown(PLAIN, CLEAN_RMAOS);
    assert!(rules.is_empty());
    assert!(!shown, "a plain albedo's Range button is greyed");
    let (shown, rules) = range_toggle_shown(100, METAL_RMAOS);
    assert_eq!(rules, ["dark_metal"]);
    assert!(!shown, "dark metal is not a per-texel rule");
}

/// Poll until the expanded texture's source is ready, without waiting for v2 jobs.
fn wait_source(m: &mut AppModel) {
    let end = Instant::now() + Duration::from_secs(60);
    while !matches!(m.preview.source, SourceState::Ready(_)) {
        assert!(Instant::now() < end, "the source never loaded");
        m.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
}

static BUILT_OFF_UI: AtomicBool = AtomicBool::new(false);

fn slow(l: &Rgba32F, rules: &[RangeRule]) -> HeatImage {
    std::thread::sleep(Duration::from_millis(1000));
    range_overlay(l, rules)
}

fn noting_the_thread(l: &Rgba32F, rules: &[RangeRule]) -> HeatImage {
    let name = std::thread::current().name().map(str::to_string);
    if name.as_deref() == Some("tessalith-overlay") {
        BUILT_OFF_UI.store(true, Ordering::SeqCst);
    }
    range_overlay(l, rules)
}

fn panics(_: &Rgba32F, _: &[RangeRule]) -> HeatImage {
    panic!("overlay test panic")
}

#[test]
fn the_overlay_is_built_off_the_ui_thread_and_a_stale_result_is_dropped() {
    let d = tempfile::tempdir().unwrap();
    let mut m = support::pbr_wall(d.path(), DARK, CLEAN_RMAOS);
    let (a, b) = ("test/wall.png", "test/wall_rmaos.png");
    // No source for the item on show: nothing starts.
    assert!(m.overlay_for(a, 0, OverlayKind::Range).is_none());
    assert!(!m.overlay_pending());
    m.expand(a);
    assert!(m.wait_idle(Duration::from_secs(60)));
    // a's job is slow; b's starts before it lands, so a's result arrives last and must be dropped.
    assert!(m.overlay_for_with(a, 0, OverlayKind::Range, slow).is_none());
    assert!(m.overlay_pending());
    m.expand(b);
    wait_source(&mut m);
    assert!(m.overlay_pending(), "a is still in flight");
    assert!(m
        .overlay_for_with(b, 0, OverlayKind::Range, noting_the_thread)
        .is_none());
    assert!(m.wait_idle(Duration::from_secs(10)));
    std::thread::sleep(Duration::from_millis(1200));
    assert!(m.wait_idle(Duration::from_secs(10)));
    let img = m
        .overlay_for(b, 0, OverlayKind::Range)
        .expect("b's overlay");
    // b (a clean _rmaos) paints nothing; a's (all dark albedo) would paint every texel.
    assert_eq!(img.size, [64, 64]);
    assert!(
        img.pixels.iter().all(|p| p.a() == 0),
        "a's stale image is on show"
    );
    assert!(
        BUILT_OFF_UI.load(Ordering::SeqCst),
        "built on the overlay worker"
    );
    assert_eq!(m.overlay.key.as_ref().map(|k| k.0.as_str()), Some(b));
}

#[test]
fn v2_jobs_keep_the_app_polling_without_greying_advanced() {
    let d = tempfile::tempdir().unwrap();
    let mut m = support::pbr_wall(d.path(), DARK, CLEAN_RMAOS);
    m.expand("test/wall.png");
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(!m.busy() && !m.v2_jobs_pending());
    assert!(m
        .overlay_for_with("test/wall.png", 0, OverlayKind::Range, slow)
        .is_none());
    // In flight: the app keeps polling, and the Advanced fields stay enabled (`busy()` is false).
    assert!(m.v2_jobs_pending());
    assert!(!m.busy());
    assert!(m.wait_idle(Duration::from_secs(10)));
    assert!(!m.v2_jobs_pending() && !m.busy());
    let img = m
        .overlay_for("test/wall.png", 0, OverlayKind::Range)
        .expect("landed");
    assert!(img.pixels.iter().all(|p| *p == bad()));
    // The same key starts nothing new.
    assert!(!m.overlay_pending());
    // The UV kind is built by `uv_overlay` from the cached mask (CL44), never from the source here.
    assert!(m.overlay_for("test/wall.png", 0, OverlayKind::Uv).is_none());
    assert!(!m.v2_jobs_pending());
}

/// K9: a panicking overlay job never leaves `v2_jobs_pending()` set (the app would poll at 50 ms forever).
#[test]
fn a_panicking_overlay_job_clears_v2_jobs_pending() {
    let d = tempfile::tempdir().unwrap();
    let mut m = support::pbr_wall(d.path(), DARK, CLEAN_RMAOS);
    m.expand("test/wall.png");
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(m
        .overlay_for_with("test/wall.png", 0, OverlayKind::Range, panics)
        .is_none());
    assert!(
        m.wait_idle(Duration::from_secs(10)),
        "wait_idle spun to its timeout"
    );
    assert!(!m.v2_jobs_pending());
    // No image, and the same key is not retried every frame.
    assert!(m
        .overlay_for("test/wall.png", 0, OverlayKind::Range)
        .is_none());
    assert!(!m.overlay_pending());
}

/// A1, K8: `uv_rect` is (offset, scale); the overlay covers the picture's screen rect only, all nine copies with Tile.
#[test]
fn the_overlay_covers_the_picture_and_nothing_past_it() {
    use sdc_gui::model::view::ViewState;
    use sdc_gui::ui::preview::picture_rects;
    let pane = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(200.0, 100.0));
    let mut v = ViewState::default(); // zoom 1, centred
    let (pic, uv) = picture_rects(&v, pane, [200.0, 100.0], (100, 50));
    assert_eq!(
        pic,
        egui::Rect::from_min_max(egui::pos2(60.0, 45.0), egui::pos2(160.0, 95.0))
    );
    assert_eq!(
        uv,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0))
    );
    v.tile = true;
    let (pic, uv) = picture_rects(&v, pane, [200.0, 100.0], (100, 50));
    assert_eq!(
        pic,
        egui::Rect::from_min_max(egui::pos2(-40.0, -5.0), egui::pos2(260.0, 145.0))
    );
    assert_eq!(
        uv,
        egui::Rect::from_min_max(egui::pos2(-1.0, -1.0), egui::pos2(2.0, 2.0))
    );
    // Panned and zoomed: the texel at the pane's centre is the view's centre uv.
    v.tile = false;
    v.zoom = 2.0;
    v.center = [0.25, 0.5];
    let (pic, _) = picture_rects(&v, pane, [200.0, 100.0], (100, 50));
    let u = (pane.center().x - pic.min.x) / pic.width();
    assert!((u - 0.25).abs() < 1e-6, "{u}");
    assert!((pic.width() - 200.0).abs() < 1e-4);
}

/// `RANGE_BAD` as the worker's premultiplied `ColorImage` stores it.
fn bad() -> egui::Color32 {
    let [r, g, b, a] = sdc_gui::model::overlay::RANGE_BAD;
    egui::Color32::from_rgba_unmultiplied(r, g, b, a)
}

type GpuHarness = Harness<'static, (AppModel, sdc_gui::ui::State)>;

/// A harness whose GpuStatus says the source is on the GPU, so the source pane (and the overlay over it) is drawn.
fn gpu_harness(model: AppModel) -> GpuHarness {
    let mut s = sdc_gui::ui::State::default();
    s.gpu.renderer = true;
    s.gpu.source = true;
    let h = Harness::builder()
        .with_size(egui::vec2(1600.0, 1000.0))
        .build_ui_state(
            |ui, (m, s): &mut (AppModel, sdc_gui::ui::State)| sdc_gui::ui::draw_with(ui, m, s),
            (model, s),
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

/// The allocated "overlay" texture: its id and options.
fn overlay_texture(h: &GpuHarness) -> Option<(egui::TextureId, egui::TextureOptions)> {
    let manager = h.ctx.tex_manager();
    let tm = manager.read();
    let found = tm
        .allocated()
        .find(|(_, m)| m.name == "overlay")
        .map(|(id, m)| (*id, m.options));
    found
}

/// Steps until the overlay has landed and a frame has uploaded it; returns that texture's id.
fn step_until_uploaded(h: &mut GpuHarness) -> egui::TextureId {
    for _ in 0..50 {
        h.step();
        assert!(h.state_mut().0.wait_idle(Duration::from_secs(10)));
        if h.state().0.overlay.image.is_none() {
            continue;
        }
        h.step();
        if let Some((id, _)) = overlay_texture(h) {
            return id;
        }
    }
    panic!("the overlay was never uploaded");
}

/// A dark albedo expanded with the Range overlay on.
fn dark_with_range(dir: &std::path::Path) -> AppModel {
    let mut m = support::pbr_wall(dir, DARK, CLEAN_RMAOS);
    m.expand("test/wall.png");
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.view.overlay = Some(OverlayKind::Range);
    m
}

/// Review I1, M4, M7: the worker hands the UI a ready `ColorImage` (the model holds `Arc<ColorImage>`, which the
/// frame uploads as is); the texture is the image's size with Nearest both ways; turning the toggle off frees the
/// image and the texture. (kittest's renderer consumes `textures_delta` inside `step`, so the uploaded `Arc` itself
/// cannot be compared here.)
#[test]
fn the_ui_uploads_the_workers_image_and_frees_it_when_off() {
    let d = tempfile::tempdir().unwrap();
    let mut h = gpu_harness(dark_with_range(d.path()));
    let id = step_until_uploaded(&mut h);
    let img = h.state().0.overlay.image.clone().unwrap();
    assert_eq!(img.size, [64, 64]);
    assert!(img.pixels.iter().all(|p| *p == bad()));
    let (_, options) = overlay_texture(&h).unwrap();
    assert_eq!(options.minification, egui::TextureFilter::Nearest);
    assert_eq!(options.magnification, egui::TextureFilter::Nearest);
    assert_eq!(options.wrap_mode, egui::TextureWrapMode::Repeat);
    assert_eq!(h.ctx.tex_manager().read().meta(id).unwrap().size, [64, 64]);
    h.state_mut().0.view.overlay = None;
    h.step();
    h.step();
    assert!(
        h.state().0.overlay.image.is_none(),
        "the image is kept while off"
    );
    assert!(
        overlay_texture(&h).is_none(),
        "the texture is kept while off"
    );
}

/// Review T1 (K8): in Split the overlay's viewport is the whole viewport (`left`), and it is clipped to the
/// source side of the divider (`visible`), not drawn over the encoded side.
#[test]
fn in_split_the_overlay_is_laid_out_on_the_viewport_and_clipped_to_the_source_side() {
    use eframe::egui::containers::panel::PanelState;
    let d = tempfile::tempdir().unwrap();
    let mut m = dark_with_range(d.path());
    m.view.layout = sdc_gui::model::view::Layout::Split;
    m.view.split = 0.3;
    let mut h = gpu_harness(m);
    let id = step_until_uploaded(&mut h);
    let (clip, mesh) = h
        .output()
        .shapes
        .iter()
        .find_map(|c| match &c.shape {
            egui::Shape::Mesh(mesh) if mesh.texture_id == id => Some((c.clip_rect, mesh.clone())),
            _ => None,
        })
        .expect("the overlay is painted");
    let bar = PanelState::load(&h.ctx, egui::Id::new("preview-toolbar"))
        .expect("the toolbar")
        .outer_rect;
    let pic = mesh.calc_bounds();
    // Fit centres the picture in the viewport, which spans the toolbar's width.
    assert!(
        (pic.center().x - bar.center().x).abs() < 2.0,
        "picture {pic:?} not centred in the viewport {bar:?}"
    );
    assert!((clip.min.x - bar.min.x).abs() < 2.0, "clip {clip:?}");
    let divider = bar.min.x + 0.3 * bar.width();
    assert!(
        (clip.max.x - divider).abs() < 2.0,
        "clip {clip:?} vs divider {divider}"
    );
}

/// Note FC13 (CL28): a texture with no per-texel range warning has a greyed Range toggle that says why on hover.
#[test]
fn a_greyed_range_toggle_says_why() {
    let d = tempfile::tempdir().unwrap();
    let mut m = support::pbr_wall(d.path(), PLAIN, CLEAN_RMAOS);
    m.expand("test/wall.png");
    assert!(m.wait_idle(Duration::from_secs(60)));
    let mut h = harness(m);
    h.ctx.global_style_mut(|s| {
        s.interaction.tooltip_delay = 0.0;
        s.interaction.show_tooltips_only_when_still = false;
    });
    h.run();
    h.get_by_label("Range overlay").hover();
    h.run();
    h.run();
    h.get_by_label_contains("no value-range warning");
}

/// Review M3: while a pin is compared the source pane is the pin, so the toggle is disabled and says why.
#[test]
fn the_range_toggle_is_disabled_while_comparing_a_pin() {
    use egui_kittest::kittest::NodeT;
    let d = tempfile::tempdir().unwrap();
    let mut m = support::pbr_wall(d.path(), DARK, CLEAN_RMAOS);
    m.expand("test/wall.png");
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.encode_now();
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.pin_this();
    m.set_compare_pinned(true);
    assert!(m.comparing_pin());
    let mut h = harness(m);
    h.run();
    let node = h.get_by_label("Range overlay");
    assert!(
        node.accesskit_node().is_disabled(),
        "enabled while comparing"
    );
    node.hover();
    h.run();
    assert!(h
        .query_by_label_contains("Hidden while comparing")
        .is_some());
    h.state_mut().set_compare_pinned(false);
    h.run();
    assert!(!h
        .get_by_label("Range overlay")
        .accesskit_node()
        .is_disabled());
}
