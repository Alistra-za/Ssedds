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

#[test]
fn a_psd_source_has_a_size() {
    // Minimal raw 8-bit RGB PSD, 3x2: header, three empty sections, compression 0, planar pixels.
    let (w, h) = (3u32, 2u32);
    let mut b = b"8BPS".to_vec();
    b.extend(1u16.to_be_bytes());
    b.extend([0u8; 6]);
    b.extend(3u16.to_be_bytes());
    b.extend(h.to_be_bytes());
    b.extend(w.to_be_bytes());
    b.extend(8u16.to_be_bytes());
    b.extend(3u16.to_be_bytes());
    for _ in 0..3 {
        b.extend(0u32.to_be_bytes());
    }
    b.extend(0u16.to_be_bytes());
    b.extend(vec![128u8; (w * h * 3) as usize]);
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("a.psd");
    std::fs::write(&p, b).unwrap();
    assert_eq!(sdc_gui::model::out_size::source_dims(&p), Some((w, h)));
}

/// Final review M6: the four places that size a dropped top mip agree: the encode (`pipeline.rs`), the DIM checks
/// and the clear on a single-level chain (`policy.rs`), and this estimate. Odd sizes, 1×N and the smallest chains
/// are planned with `resize = half` (so DIM-RESIZED reports the size it checks); the sizes the DIM checks let through
/// are also exported and their headers read.
#[test]
fn the_dropped_top_mip_size_agrees_in_all_four_places() {
    use engine::api::*;
    use engine::codes::Code;
    use engine::events::*;
    use engine::resize::target_dims;
    use std::sync::Arc;
    // (source size, mips.levels, exported)
    let cases: &[((u32, u32), &str, bool)] = &[
        ((2050, 6), "\"full\"", false),
        ((2054, 2), "\"full\"", false),
        ((2, 2049), "\"full\"", false),
        ((1027, 1), "\"full\"", false),
        ((1030, 514), "2", false),
        ((1030, 514), "\"none\"", false),
        ((1026, 2), "2", false),
        ((2048, 8), "\"full\"", false),
        ((2048, 16), "\"full\"", true),
        ((1024, 1024), "2", true),
        ((1024, 1024), "\"none\"", true),
    ];
    let d = tempfile::tempdir().unwrap();
    let i = d.path().join("in");
    std::fs::create_dir_all(&i).unwrap();
    let mut toml = String::from("[defaults.encoder]\neffort = \"ultrafast\"\n");
    for (n, ((w, h), levels, _)) in cases.iter().enumerate() {
        image::RgbImage::from_pixel(*w, *h, image::Rgb([120, 90, 60]))
            .save(i.join(format!("s{n}.png")))
            .unwrap();
        toml += &format!(
            "\n[files.\"s{n}.png\"]\nsubtype = \"diffuse\"\nresize = \"half\"\ndrop_top_mip = true\nmips.levels = {levels}\n"
        );
    }
    std::fs::write(i.join("sdc.toml"), toml).unwrap();
    let mut r = BatchRequest::new(vec![i], d.path().join("out"), Target::Cs);
    r.accept = engine::policy::AcceptSuggestions::High;
    let rep = Reporter::new(Arc::new(NullSink), "t");
    let e = Engine::new().unwrap();
    let plan = e.plan(r, &rep, &CancelToken::default()).unwrap();
    let mut written = vec![];
    for (n, ((w, h), levels, exported)) in cases.iter().enumerate() {
        let name = format!("s{n}.png");
        let j = plan
            .jobs
            .iter()
            .find(|j| j.input.file_name().is_some_and(|f| f == name.as_str()))
            .unwrap_or_else(|| panic!("no job for {name}"));
        let p = &j.profile;
        let (rw, rh) = target_dims((*w, *h), p.resize);
        let chain = engine::mips::level_count(rw, rh, &p.mips.levels);
        let want = if chain > 1 {
            ((rw / 2).max(1), (rh / 2).max(1))
        } else {
            (rw, rh)
        };
        let case = format!("{w}x{h} levels {levels}");
        assert_eq!(p.drop_top_mip, chain > 1, "the clear: {case}");
        let est = estimate((*w, *h), p);
        assert_eq!((est.width, est.height), want, "the estimate: {case}");
        assert_eq!(est.mips, chain - u32::from(chain > 1), "{case}");
        let to = j
            .warnings
            .iter()
            .find(|w| w.code == Code::DimResized)
            .unwrap_or_else(|| panic!("no DIM-RESIZED: {case}"));
        assert_eq!(
            to.data["to"],
            serde_json::json!([want.0, want.1]),
            "the DIM checks: {case}"
        );
        if *exported {
            written.push((j.output.clone(), want, est.mips as usize, case));
        }
    }
    e.run(plan, &rep, &CancelToken::default()).unwrap();
    for (out, want, mips, case) in written {
        let b = std::fs::read(&out).unwrap_or_else(|_| panic!("not written: {case}"));
        let f = engine::dds::read_dds(&b).unwrap();
        assert_eq!((f.width, f.height), want, "the encode: {case}");
        assert_eq!(f.levels.len(), mips, "{case}");
    }
}
