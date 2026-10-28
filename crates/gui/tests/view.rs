use engine::image::Rgba32F;
use sdc_gui::model::view::*;

#[test]
fn zooming_keeps_the_texel_under_the_cursor() {
    let mut v = ViewState::default();
    v.fit([512.0, 512.0], (1024, 1024));
    let cursor = [0.25, 0.75];
    let before = v.uv_rect([512.0, 512.0], (1024, 1024));
    v.zoom_at(2.0, cursor);
    let after = v.uv_rect([512.0, 512.0], (1024, 1024));
    // The uv at the cursor's screen position is unchanged.
    let at = |(off, scale): ([f32; 2], [f32; 2]), sx: f32, sy: f32| {
        [off[0] + sx * scale[0], off[1] + sy * scale[1]]
    };
    let sx = (cursor[0] - before.0[0]) / before.1[0];
    let sy = (cursor[1] - before.0[1]) / before.1[1];
    let a = at(after, sx, sy);
    assert!(
        (a[0] - cursor[0]).abs() < 1e-4 && (a[1] - cursor[1]).abs() < 1e-4,
        "{a:?}"
    );
    assert!(
        after.1[0] < before.1[0],
        "zoomed in shows less of the texture"
    );
}

#[test]
fn the_mip_is_clamped_to_what_was_previewed() {
    let mut v = ViewState {
        mip: 0,
        ..Default::default()
    };
    v.clamp_mip(2, 7);
    assert_eq!(v.mip, 2);
    v.mip = 99;
    v.clamp_mip(2, 7);
    assert_eq!(v.mip, 6);
}

#[test]
fn the_heatmap_is_black_where_equal_and_bright_where_different() {
    let src = Rgba32F::new(2, 1, [0.5, 0.5, 0.5, 1.0]);
    let dec = vec![[0.5, 0.5, 0.5, 1.0], [1.0, 0.5, 0.5, 1.0]];
    let (w, h, px) = heatmap(&src, &dec, Channel::R, 0.1);
    assert_eq!((w, h), (2, 1));
    assert_eq!(&px[..3], &[0, 0, 0]);
    assert!(px[4] > 100 || px[5] > 100 || px[6] > 100);
}

#[test]
fn skipped_mips_are_named() {
    let dims = [(8192, 8192), (4096, 4096), (2048, 2048)];
    assert_eq!(
        skipped_note(&dims, 1).as_deref(),
        Some("mip 0 (8192×8192) not previewed: above the preview limit")
    );
    assert_eq!(skipped_note(&dims, 0), None);
    assert_eq!(
        skipped_note(&dims, 2).as_deref(),
        Some("mip 0 (8192×8192), mip 1 (4096×4096) not previewed: above the preview limit")
    );
}

#[test]
fn fit_shows_the_whole_texture_and_pan_moves_the_picture_with_the_pointer() {
    let mut v = ViewState::default();
    v.fit([400.0, 200.0], (100, 100));
    let (off, scale) = v.uv_rect([400.0, 200.0], (100, 100));
    // 200 px tall for 100 texels: zoom 2; the texture's height fills the view, its width is centred.
    assert!((v.zoom - 2.0).abs() < 1e-6);
    assert!((off[1]).abs() < 1e-6 && (scale[1] - 1.0).abs() < 1e-6);
    assert!((off[0] + scale[0] / 2.0 - 0.5).abs() < 1e-6);
    // Dragging 20 px right at zoom 2 over 100 texels moves the view 0.1 uv left.
    v.pan_pixels([20.0, 0.0], [400.0, 200.0], (100, 100));
    assert!((v.center[0] - 0.4).abs() < 1e-6, "{:?}", v.center);
}

#[test]
fn changing_the_mip_keeps_the_uv_region_in_view() {
    let mut v = ViewState::default();
    v.fit([512.0, 512.0], (1024, 1024));
    let before = v.uv_rect([512.0, 512.0], (1024, 1024));
    v.set_mip(1, (1024, 1024), (512, 512));
    assert_eq!(v.mip, 1);
    let after = v.uv_rect([512.0, 512.0], (512, 512));
    assert!((before.1[0] - after.1[0]).abs() < 1e-6);
    assert!((before.0[1] - after.0[1]).abs() < 1e-6);
}

fn preview_out(
    first_level: usize,
    normal_len: Vec<engine::session::NormalLenStats>,
) -> sdc_gui::model::preview::PreviewOut {
    use engine::session::{PreviewResult, SourcePreview};
    use engine::validate::alpha::AlphaStats;
    let alpha = |eq255, min| AlphaStats {
        pixels: 100,
        eq255,
        eq254: 0,
        below254: 100 - eq255,
        min,
    };
    sdc_gui::model::preview::PreviewOut {
        result: PreviewResult {
            source: SourcePreview {
                first_level,
                dims: vec![(8, 8), (4, 4), (2, 2)],
                levels: vec![],
            },
            format: engine::config::Format::Bc3,
            srgb: true,
            faces: 1,
            levels: vec![],
            validation: engine::validate::ValidationSummary {
                alpha: vec![alpha(100, 255), alpha(75, 40)],
                coverage: vec![0.5, 0.25],
                min_mip_psnr_db: Some(41.25),
                ..Default::default()
            },
            warnings: vec![],
            normal_len,
        },
        decoded: vec![],
    }
}

fn find<'a>(r: &'a Readouts, name: &str) -> Option<&'a str> {
    r.lines
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

#[test]
fn readouts_index_from_the_first_previewed_mip() {
    let out = preview_out(1, vec![]);
    // Absolute mip 2 is the second kept mip.
    let r = readouts(&out, 2);
    assert_eq!(find(&r, "Smallest-mip alpha"), Some("40"));
    assert_eq!(find(&r, "α ≠ 255"), Some("25.0 %"));
    assert_eq!(find(&r, "Coverage"), Some("25.0 %"));
    assert_eq!(find(&r, "Minimum mip PSNR"), Some("41.2 dB"));
    assert_eq!(find(&r, "Normal length error"), None);
}

#[test]
fn a_normal_length_that_could_not_be_measured_shows_a_dash() {
    let nan = engine::session::NormalLenStats {
        mean_err: f64::NAN,
        max_err: f64::NAN,
    };
    let r = readouts(&preview_out(0, vec![nan, nan]), 0);
    assert_eq!(find(&r, "Normal length error"), Some("—"));
}
