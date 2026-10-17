//! Task 8 (items 1 and 15): the pixel readout, the lens, tiling, exposure and the heatmap's full scale, through the
//! model helpers (the preview itself needs a GPU).
mod support;
use engine::config::Format;
use engine::image::Rgba32F;
use engine::session::{PreviewResult, SourcePreview};
use sdc_gui::model::preview::{EncodedState, PreviewOut};
use sdc_gui::model::readout::*;
use sdc_gui::model::view::*;
use sdc_gui::model::AppModel;
use sdc_gui::settings::GuiSettings;
use std::sync::Arc;

fn img(w: u32, h: u32, px: Vec<[f32; 4]>) -> Rgba32F {
    Rgba32F {
        width: w,
        height: h,
        px,
    }
}

fn out(
    first: usize,
    dims: Vec<(u32, u32)>,
    src: Vec<Rgba32F>,
    dec: Vec<Vec<[f32; 4]>>,
    format: Format,
) -> PreviewOut {
    PreviewOut {
        result: PreviewResult {
            source: SourcePreview {
                first_level: first,
                dims,
                levels: src,
            },
            format,
            srgb: true,
            levels: vec![],
            validation: Default::default(),
            warnings: vec![],
            normal_len: vec![],
        },
        decoded: dec,
    }
}

fn opts() -> ReadoutOpts {
    ReadoutOpts {
        channel: Channel::Rgba,
        view_linear: false,
        hdr: false,
        normal: None,
    }
}

const SRC: [[f32; 4]; 2] = [[0.0, 0.0, 0.0, 1.0], [0.5, 0.25, 1.0, 1.0]];
const DEC: [[f32; 4]; 2] = [[0.0, 0.0, 0.0, 1.0], [0.5, 0.3, 0.9, 1.0]];

fn sides() -> (Side<'static>, Side<'static>) {
    (
        Side {
            width: 2,
            height: 1,
            px: &SRC,
        },
        Side {
            width: 2,
            height: 1,
            px: &DEC,
        },
    )
}

#[test]
fn the_readout_shows_source_and_encoded_as_bytes_and_floats_with_the_error() {
    let (s, e) = sides();
    let r = texel_readout(Some(s), Some(e), 0, (1, 0), opts());
    assert_eq!(r.texel, (1, 0));
    assert_eq!(r.source, Some([0.5, 0.25, 1.0, 1.0]));
    assert_eq!(r.encoded, Some([0.5, 0.3, 0.9, 1.0]));
    let err = r.error.unwrap();
    assert!((err[1] - 0.05).abs() < 1e-6 && (err[2] - 0.1).abs() < 1e-6 && err[3] == 0.0);
    // RGBA: the largest RGB difference, as the heatmap measures it.
    assert!((r.max_error.unwrap() - 0.1).abs() < 1e-6);
    let text = r.lines().join("\n");
    assert!(text.contains("128"), "0.5 is code 128: {text}");
    assert!(text.contains("0.5000"), "{text}");
    assert!(text.contains("255"), "{text}");
    assert!(text.contains("max 0.1000"), "{text}");
}

#[test]
fn the_readout_names_its_colour_space_from_the_view() {
    let (s, e) = sides();
    let srgb = texel_readout(Some(s), Some(e), 0, (1, 0), opts());
    let (s, e) = sides();
    let unorm = texel_readout(
        Some(s),
        Some(e),
        0,
        (1, 0),
        ReadoutOpts {
            view_linear: true,
            ..opts()
        },
    );
    // The numbers are the stored values either way; the label says how the view reads them.
    assert_eq!(srgb.source, unorm.source);
    assert_ne!(srgb.space, unorm.space);
    assert!(srgb.space.contains("sRGB"), "{}", srgb.space);
    assert!(unorm.space.contains("UNORM"), "{}", unorm.space);
    let (s, e) = sides();
    let hdr = texel_readout(
        Some(s),
        Some(e),
        0,
        (1, 0),
        ReadoutOpts {
            hdr: true,
            ..opts()
        },
    );
    assert!(hdr.space.contains("HDR"), "{}", hdr.space);
    assert!(!hdr.lines().join("\n").contains("128"), "no 0-255 for HDR");
}

#[test]
fn channel_isolation_narrows_the_readout_to_that_channel() {
    let (s, e) = sides();
    let r = texel_readout(
        Some(s),
        Some(e),
        0,
        (1, 0),
        ReadoutOpts {
            channel: Channel::G,
            ..opts()
        },
    );
    assert!((r.max_error.unwrap() - 0.05).abs() < 1e-6);
    let text = r.lines().join("\n");
    assert!(text.contains("G "), "{text}");
    assert!(!text.contains("B "), "only G shows: {text}");
}

#[test]
fn a_missing_side_shows_a_dash() {
    let (s, _) = sides();
    let r = texel_readout(Some(s), None, 0, (1, 0), opts());
    assert_eq!(r.encoded, None);
    assert_eq!(r.error, None);
    assert!(r
        .lines()
        .iter()
        .any(|l| l.contains("Encoded") && l.contains('—')));
    // A texel outside the side's image is missing too.
    let (s, e) = sides();
    let r = texel_readout(Some(s), Some(e), 0, (5, 0), opts());
    assert_eq!((r.source, r.encoded), (None, None));
}

#[test]
fn the_model_reads_the_texel_at_the_shown_mip_from_the_result() {
    let mut m = AppModel::new(GuiSettings::default(), None, support::session());
    // Mip 0 was above the preview limit: the result starts at mip 1 (2x2), then mip 2 (1x1).
    let src1 = img(2, 2, vec![[0.1; 4], [0.2; 4], [0.3; 4], [0.4; 4]]);
    let src2 = img(1, 1, vec![[0.9; 4]]);
    let dec1 = vec![[0.1; 4], [0.25; 4], [0.3; 4], [0.4; 4]];
    let dec2 = vec![[0.8; 4]];
    let o = out(
        1,
        vec![(4, 4), (2, 2), (1, 1)],
        vec![src1, src2],
        vec![dec1, dec2],
        Format::Bc7,
    );
    m.preview.encoded = EncodedState::Done(Arc::new(o));
    m.view.mip = 1;
    m.view.fit([200.0, 200.0], (2, 2));
    // The upper right quarter of the pane is texel (1, 0) of the 2x2 mip.
    let t = m
        .view
        .texel_at([200.0, 200.0], (2, 2), [0.75, 0.25])
        .unwrap();
    assert_eq!(t, (1, 0));
    let r = m.preview_texel(1, t);
    assert_eq!(r.mip, 1);
    assert_eq!(r.source, Some([0.2; 4]));
    assert_eq!(r.encoded, Some([0.25; 4]));
    let r = m.preview_texel(2, (0, 0));
    assert_eq!(r.source, Some([0.9; 4]));
    assert_eq!(r.encoded, Some([0.8; 4]));
    // Mip 0 was not previewed: nothing to read.
    let r = m.preview_texel(0, (0, 0));
    assert_eq!((r.source, r.encoded), (None, None));
}

#[test]
fn the_normal_readout_gives_the_vector_its_length_and_the_angle_error() {
    let a = 10f32.to_radians();
    // Source (0, 0, 1); a two-channel encode stores x = sin 10°, y = 0, and z is rebuilt.
    let src = [0.5, 0.5, 1.0, 1.0];
    let enc = [0.5 + a.sin() / 2.0, 0.5, 0.0, 1.0];
    let n = normal_readout(Some(src), Some(enc), true);
    let e = n.encoded.unwrap();
    assert!((e[0] - a.sin()).abs() < 1e-5 && e[1].abs() < 1e-5 && (e[2] - a.cos()).abs() < 1e-5);
    assert!((n.length.unwrap() - 1.0).abs() < 1e-5);
    assert!(
        (n.angle_deg.unwrap() - 10.0).abs() < 1e-3,
        "{:?}",
        n.angle_deg
    );
    // A three-channel encode keeps its own z: (0, 0, 0) stored as 0.5 has length 0.
    let n = normal_readout(Some(src), Some([0.5, 0.5, 0.5, 1.0]), false);
    assert!(n.length.unwrap() < 1e-6);
    // The texel readout carries it for normal sub-types only.
    let (s, e) = sides();
    let r = texel_readout(
        Some(s),
        Some(e),
        0,
        (1, 0),
        ReadoutOpts {
            normal: Some(false),
            ..opts()
        },
    );
    assert!(r.normal.is_some());
    assert!(r.lines().iter().any(|l| l.contains("Normal")));
    let (s, e) = sides();
    assert!(texel_readout(Some(s), Some(e), 0, (1, 0), opts())
        .normal
        .is_none());
}

#[test]
fn the_cursor_maps_to_a_texel_under_zoom_and_pan() {
    let mut v = ViewState::default();
    v.fit([100.0, 100.0], (10, 10));
    // Zoom 10: each texel is 10 px. The pane's centre is between texels 4 and 5; just right of it is texel 5.
    assert_eq!(
        v.texel_at([100.0, 100.0], (10, 10), [0.51, 0.51]),
        Some((5, 5))
    );
    v.zoom_at(2.0, [0.5, 0.5]);
    // Zoom 20: the pane shows texels 2.5..7.5; its left edge is in texel 2.
    assert_eq!(
        v.texel_at([100.0, 100.0], (10, 10), [0.0, 0.5]),
        Some((2, 5))
    );
    v.pan_pixels([40.0, 0.0], [100.0, 100.0], (10, 10));
    // Dragging 40 px right moves the picture two texels right.
    assert_eq!(
        v.texel_at([100.0, 100.0], (10, 10), [0.0, 0.5]),
        Some((0, 5))
    );
    // Off the picture: no texel.
    v.pan_pixels([200.0, 0.0], [100.0, 100.0], (10, 10));
    assert_eq!(v.texel_at([100.0, 100.0], (10, 10), [0.0, 0.5]), None);
}

#[test]
fn both_panes_read_the_same_texel() {
    let mut v = ViewState::default();
    v.fit([100.0, 80.0], (64, 64));
    v.zoom_at(3.0, [0.3, 0.6]);
    // Side by side: the left pane at x 0, the right at x 106. The cursor in the left pane maps to the same place in
    // the right one, and both read one texel.
    let (left, right) = ([0.0, 0.0], [106.0, 0.0]);
    let size = [100.0, 80.0];
    let cursor = [37.0, 51.0];
    let s = pane_fraction(cursor, left, size);
    // The same place in the right pane: 106 px further right.
    let twin = [143.0, 51.0];
    let s2 = pane_fraction(twin, right, size);
    assert_eq!(s, s2);
    assert_eq!(
        v.texel_at(size, (64, 64), s),
        v.texel_at(size, (64, 64), s2)
    );
    // Split: one rect for both sides, so one fraction and one texel by construction.
}

#[test]
fn exposure_zero_is_unchanged_and_one_stop_doubles() {
    assert_eq!(exposure_factor(0.0), 1.0);
    assert_eq!(exposure_factor(1.0), 2.0);
    assert_eq!(exposure_factor(-1.0), 0.5);
    assert_eq!(ViewState::default().exposure, 0.0);
    assert_eq!(EXPOSURE_RANGE, (-8.0, 8.0));
}

#[test]
fn the_lens_shows_an_eighth_of_its_width_around_the_texel_and_stays_on_the_picture() {
    let mut v = ViewState::default();
    v.fit([100.0, 100.0], (100, 100));
    // Zoom 1: the lens is 160 px at 8x, so it shows 20 texels: 0.2 uv, centred on the texel's centre.
    let (off, scale) = v.lens_uv([100.0, 100.0], (100, 100), [0.503, 0.503]);
    assert!((scale[0] - 0.2).abs() < 1e-6 && (scale[1] - 0.2).abs() < 1e-6);
    let c = [off[0] + scale[0] / 2.0, off[1] + scale[1] / 2.0];
    assert!(
        (c[0] - 0.505).abs() < 1e-6 && (c[1] - 0.505).abs() < 1e-6,
        "{c:?}"
    );
    // Past the right edge the centre clamps to the last texel.
    let (off, scale) = v.lens_uv([100.0, 100.0], (100, 100), [1.5, 0.0]);
    let c = [off[0] + scale[0] / 2.0, off[1] + scale[1] / 2.0];
    assert!(
        (c[0] - 0.995).abs() < 1e-6 && (c[1] - 0.005).abs() < 1e-6,
        "{c:?}"
    );
    // Zoomed in 4x the lens magnifies 8x more: 5 texels.
    v.zoom = 4.0;
    let (_, scale) = v.lens_uv([100.0, 100.0], (100, 100), [0.5, 0.5]);
    assert!((scale[0] - 0.05).abs() < 1e-6);
    assert_eq!((LENS_PX, LENS_ZOOM), (160.0, 8.0));
}

#[test]
fn tiling_repeats_the_picture_three_by_three() {
    assert_eq!(tile_uv([0.25, 0.5], false), Some([0.25, 0.5]));
    assert_eq!(tile_uv([1.25, 0.5], false), None);
    assert_eq!(tile_uv([1.25, -0.5], true), Some([0.25, 0.5]));
    assert_eq!(tile_uv([-1.0, 1.999], true).map(|u| u[0]), Some(0.0));
    assert_eq!(tile_uv([2.0, 0.5], true), None);
    assert_eq!(tile_uv([0.5, -1.01], true), None);
    // Fit shows all nine copies; the texel under the cursor is read through the wrap.
    let mut v = ViewState {
        tile: true,
        ..Default::default()
    };
    v.fit([300.0, 300.0], (10, 10));
    assert!((v.zoom - 10.0).abs() < 1e-6);
    // The top left copy's texel (1, 1) is the picture's texel (1, 1).
    assert_eq!(
        v.texel_at([300.0, 300.0], (10, 10), [0.05, 0.05]),
        Some((1, 1))
    );
    // The lens wraps too: past the edge it is not clamped.
    let (off, scale) = v.lens_uv([300.0, 300.0], (10, 10), [0.05, 0.05]);
    let c = [off[0] + scale[0] / 2.0, off[1] + scale[1] / 2.0];
    assert!((c[0] - 0.15).abs() < 1e-6, "{c:?}");
}

#[test]
fn full_scale_sets_where_the_heatmap_ramp_ends() {
    let src = img(1, 1, vec![[0.5, 0.5, 0.5, 1.0]]);
    let dec = vec![[0.55, 0.5, 0.5, 1.0]];
    let (_, _, tight) = heatmap(&src, &dec, Channel::Rgba, 0.05);
    let (_, _, loose) = heatmap(&src, &dec, Channel::Rgba, 0.5);
    assert_eq!(
        &tight[..3],
        &[255, 255, 255],
        "0.05 off at full scale 0.05 is white"
    );
    assert_ne!(tight, loose);
    assert_eq!(ramp(0.0), [0, 0, 0]);
    assert_eq!(ramp(1.0), [255, 255, 255]);
    assert_eq!(ViewState::default().full_scale, FULL_SCALE_DEFAULT);
    assert_eq!(FULL_SCALE_DEFAULT, 0.1);
    assert_eq!(FULL_SCALE_RANGE, (0.005, 0.5));
}

/// Trace-debug #16: a single-mip DDS above the limit is refused, not decoded at full size.
#[test]
fn dds_preview_refuses_when_no_mip_fits_the_limit() {
    use engine::dds::{dds_bytes, DdsDesc, ResolvedHeader};
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("big.dds");
    let desc = DdsDesc {
        width: 64,
        height: 64,
        format: Format::Rgba8,
        srgb: false,
        header: ResolvedHeader::Dx10,
    };
    std::fs::write(&path, dds_bytes(&desc, &[vec![0; 64 * 64 * 4]]).unwrap()).unwrap();
    let Err(err) = sdc_gui::model::preview::dds_preview(&path, 16, encoders_sys::Bc1Model::Ideal)
    else {
        panic!("refused");
    };
    assert!(err.contains("above the preview limit"), "{err}");
    // A chain that reaches the limit still previews from the first mip within it.
    assert!(sdc_gui::model::preview::dds_preview(&path, 64, encoders_sys::Bc1Model::Ideal).is_ok());
}
