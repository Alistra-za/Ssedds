//! Task 14 (item 22, DECISIONS AG118-AG120): the DDS size estimate, the totals and the cache by plan revision.
mod support;
use engine::config::{
    ColorTag, Format, HeaderForm, MipKeyword, MipLevels, Profile, ResizeMode, Subtype, Target,
};
use engine::presets::PresetRegistry;
use sdc_gui::model::out_size::{estimate, human};

fn profile(format: Format, tag: ColorTag, levels: MipLevels) -> Profile {
    let mut p = PresetRegistry::builtin()
        .unwrap()
        .base_profile(Target::Cs, Subtype::Diffuse);
    p.format = format;
    p.color.tag = tag;
    p.mips.levels = levels;
    p.header = HeaderForm::Auto;
    p.resize = ResizeMode::None;
    p
}

const FULL: MipLevels = MipLevels::Keyword(MipKeyword::Full);
const NONE: MipLevels = MipLevels::Keyword(MipKeyword::None);

#[test]
fn bc7_2048_with_a_full_chain_and_the_dx10_header() {
    let e = estimate((2048, 2048), &profile(Format::Bc7, ColorTag::Srgb, FULL));
    // 4^2 + ... + 4^11 for the block levels, plus the 2x2 and 1x1 levels at one block (16 bytes) each.
    assert_eq!((e.width, e.height, e.mips), (2048, 2048, 12));
    assert_eq!(e.chain_bytes, 5_592_432);
    assert_eq!(e.header_bytes, 148);
    assert_eq!(e.file_bytes(), 5_592_580);
}

#[test]
fn bc1_bc5_and_rgba8_at_a_few_sizes_with_and_without_mips() {
    // BC1 non-sRGB takes the legacy header (128 bytes); sRGB needs DX10 (148).
    let e = estimate((1024, 512), &profile(Format::Bc1, ColorTag::Unorm, FULL));
    assert_eq!((e.mips, e.chain_bytes, e.header_bytes), (11, 349_544, 128));
    let e = estimate((1024, 512), &profile(Format::Bc1, ColorTag::Srgb, NONE));
    assert_eq!(
        (e.mips, e.chain_bytes, e.header_bytes),
        (1, 256 * 128 * 8, 148)
    );
    let e = estimate((256, 256), &profile(Format::Bc5, ColorTag::Unorm, FULL));
    assert_eq!((e.mips, e.chain_bytes), (9, 87_408));
    let e = estimate((256, 256), &profile(Format::Bc5, ColorTag::Unorm, NONE));
    assert_eq!((e.mips, e.chain_bytes), (1, 65_536));
    let e = estimate((64, 32), &profile(Format::Rgba8, ColorTag::Unorm, FULL));
    assert_eq!((e.mips, e.chain_bytes, e.header_bytes), (7, 10_924, 128));
    let e = estimate((64, 32), &profile(Format::Rgba8, ColorTag::Unorm, NONE));
    assert_eq!((e.mips, e.chain_bytes), (1, 64 * 32 * 4));
}

#[test]
fn the_size_follows_a_resize() {
    let mut p = profile(Format::Bc7, ColorTag::Srgb, NONE);
    p.resize = ResizeMode::PotDown;
    let e = estimate((1000, 600), &p);
    assert_eq!((e.width, e.height, e.chain_bytes), (512, 512, 512 * 512));
    p.resize = ResizeMode::Half;
    let e = estimate((2048, 1024), &p);
    assert_eq!((e.width, e.height), (1024, 512));
}

#[test]
fn human_sizes() {
    assert_eq!(human(148), "148 B");
    assert_eq!(human(5_592_432), "5.3 MiB");
}

#[test]
fn totals_for_the_selection_and_the_folder_cached_by_plan_revision() {
    let d = tempfile::tempdir().unwrap();
    let mut m = support::opened(d.path());
    let key = "landscape/rock01.png";
    let one = m.size_of(key).expect("a 64 px png has an estimate");
    assert_eq!((one.width, one.height), (64, 64));
    let (sel, missing) = m.size_total(&[key.to_string(), key.to_string()]);
    assert_eq!((sel, missing), (one.file_bytes() * 2, 0));
    let (folder, _) = m.folder_size();
    assert!(
        folder >= one.file_bytes(),
        "the folder holds at least this texture"
    );
    let text = m.vram_text(&[key.to_string()]);
    assert!(text.starts_with("≈ VRAM: selection "), "{text}");
    assert!(text.contains("· folder "), "{text}");
    assert!(m.vram_text(&[]).starts_with("≈ VRAM: folder "));

    // Same revision: nothing is read or built again.
    let (reads, builds) = (m.sizes.reads, m.sizes.builds);
    assert_eq!(m.folder_size().0, folder);
    m.size_of(key);
    assert_eq!((m.sizes.reads, m.sizes.builds), (reads, builds));
    // A new revision rebuilds the sizes but not the header reads (same file, same stamp).
    m.invalidate_views();
    assert_eq!(m.folder_size().0, folder);
    assert_eq!(m.sizes.builds, builds + 1);
    assert_eq!(m.sizes.reads, reads, "the file headers are kept");
}

#[test]
fn the_settings_panel_and_the_status_line_show_the_estimate() {
    use egui_kittest::kittest::Queryable;
    let d = tempfile::tempdir().unwrap();
    let mut m = support::opened(d.path());
    m.select_item("landscape/rock01.png");
    let e = m.size_of("landscape/rock01.png").unwrap();
    let mut h = egui_kittest::Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 900.0))
        .build_ui_state(
            |ui, model: &mut sdc_gui::model::AppModel| sdc_gui::ui::draw(ui, model),
            m,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    let line = format!(
        "Output: {} · 64×64 · {} mips · {} ≈ VRAM",
        sdc_gui::model::fields::format_name(e.format),
        e.mips,
        human(e.chain_bytes)
    );
    h.get_by_label(&line);
    assert!(h
        .query_all_by_label_contains("≈ VRAM: selection ")
        .next()
        .is_some());
}

fn settle(m: &mut sdc_gui::model::AppModel) {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while m.dims_pending() {
        assert!(std::time::Instant::now() < end, "the header job landed");
        m.poll();
        std::thread::yield_now();
    }
}

#[test]
fn headers_are_read_on_a_worker_and_the_totals_follow() {
    use sdc_gui::model::out_size::header_reads_on_this_thread;
    let d = tempfile::tempdir().unwrap();
    let mut m = support::opened(d.path());
    // A session that analysed nothing and no size asked for yet: every header goes to the worker.
    m.session = Some(support::session());
    m.invalidate_views();
    let before = header_reads_on_this_thread();
    let (_, missing) = m.folder_size();
    assert!(missing > 0, "unsized until the worker lands");
    assert!(m.dims_pending());
    assert_eq!(
        header_reads_on_this_thread(),
        before,
        "no header read on the UI thread"
    );
    settle(&mut m);
    let (total, missing) = m.folder_size();
    assert_eq!(missing, 0);
    let one = m.size_of("landscape/rock01.png").unwrap();
    assert!(total >= one.file_bytes());
    assert!(m.sizes.reads >= 2);
    assert_eq!(header_reads_on_this_thread(), before);
}

fn panics(_: &std::path::Path) -> Option<(u32, u32)> {
    panic!("header reader blew up")
}

#[test]
fn a_panicked_header_job_is_retried_once_at_the_next_plan_then_left_unsized() {
    let d = tempfile::tempdir().unwrap();
    let mut m = support::opened(d.path());
    m.session = Some(support::session());
    m.sizes.reader = Some(panics);
    m.invalidate_views();
    let (_, missing) = m.folder_size();
    assert!(missing > 0);
    settle(&mut m);
    let reads = m.sizes.reads;
    assert!(reads > 0);
    // Not asked again until the next plan.
    m.folder_size();
    assert!(!m.dims_pending());
    assert_eq!(m.sizes.reads, reads);
    // The next plan asks again, once.
    m.invalidate_views();
    m.folder_size();
    settle(&mut m);
    assert_eq!(m.sizes.reads, 2 * reads, "retried once");
    assert!(m
        .messages
        .entries()
        .any(|e| e.text.contains("could not be read")));
    m.invalidate_views();
    let (_, still) = m.folder_size();
    assert!(!m.dims_pending());
    assert_eq!(m.sizes.reads, 2 * reads, "not a third time");
    assert_eq!(still, missing, "they stay unsized");
}
