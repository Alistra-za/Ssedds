mod support;
use eframe::egui_wgpu::wgpu;
use eframe::egui_wgpu::{RenderState, RendererOptions};
use engine::config::Format;
use engine::image::Rgba32F;
use engine::session::{PreviewResult, SourcePreview};
use sdc_gui::model::preview::PreviewOut;
use sdc_gui::render::*;

#[test]
fn formats_map_to_unorm_block_formats() {
    assert_eq!(
        wgpu_format(Format::Bc7),
        Some(wgpu::TextureFormat::Bc7RgbaUnorm)
    );
    assert_eq!(
        wgpu_format(Format::Bc5),
        Some(wgpu::TextureFormat::Bc5RgUnorm)
    );
    assert_eq!(
        wgpu_format(Format::Bc4),
        Some(wgpu::TextureFormat::Bc4RUnorm)
    );
    assert_eq!(
        wgpu_format(Format::Bc6hUf16),
        Some(wgpu::TextureFormat::Bc6hRgbUfloat)
    );
}

#[test]
fn block_layout_rounds_up_to_whole_blocks() {
    assert_eq!(block_layout(Format::Bc7, 64, 32), (256, 8));
    assert_eq!(block_layout(Format::Bc1, 2, 2), (8, 1));
    assert_eq!(block_layout(Format::Bc4, 1, 1), (8, 1));
}

#[test]
fn bc_upload_needs_support_whole_blocks_and_limits() {
    assert!(can_upload_bc(true, 16384, (4096, 4096)));
    assert!(!can_upload_bc(false, 16384, (4096, 4096)));
    assert!(!can_upload_bc(true, 16384, (2, 2)));
    assert!(!can_upload_bc(true, 4096, (8192, 8192)));
}

#[test]
fn rgba16f_packs_little_endian_halves() {
    let b = to_rgba16f(&[[1.0, 0.5, 0.0, 1.0]]);
    assert_eq!(b, vec![0x00, 0x3c, 0x00, 0x38, 0x00, 0x00, 0x00, 0x3c]);
}

/// Compiles the shader on a real adapter when one exists; skips (passes) without one.
#[test]
fn the_shader_compiles_on_any_available_adapter() {
    let Some(rs) = render_state() else { return };
    let device = &rs.device;
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let _ = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("preview"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../src/render/preview.wgsl").into()),
    });
    assert!(block_on(scope.pop()).is_none());
}

/// A minimal executor: wgpu's native futures complete on the first poll after the backend call.
fn block_on<F: std::future::Future>(f: F) -> F::Output {
    use std::task::{Context, Poll, Waker};
    let mut f = std::pin::pin!(f);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::yield_now();
    }
}

// ---- Drawing on a real device. Each of these returns early when there is no adapter. ----

/// A headless render state built with the wgpu setup `main` hands to eframe, shared by every test: creating
/// all-backend instances on several threads at once deadlocks inside Vulkan + GL init, so a process gets one.
fn render_state() -> Option<&'static RenderState> {
    static RS: std::sync::OnceLock<Option<RenderState>> = std::sync::OnceLock::new();
    RS.get_or_init(|| {
        let instance = wgpu::Instance::default();
        block_on(RenderState::create(
            &wgpu_configuration(),
            &instance,
            None,
            RendererOptions::default(),
        ))
        .ok()
    })
    .as_ref()
}

const SIDE: u32 = 8;
/// The pixel in the middle of the 8x8 target.
const MIDDLE: usize = 4 * SIDE as usize + 4;

/// Draws `slot` into an 8x8 target and returns its pixels as RGBA.
fn draw(rs: &RenderState, r: &PreviewRenderer, slot: Slot, params: Params) -> Vec<[u8; 4]> {
    let (device, queue) = (&rs.device, &rs.queue);
    let size = wgpu::Extent3d {
        width: SIDE,
        height: SIDE,
        depth_or_array_layers: 1,
    };
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("test target"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: rs.target_format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("test readback"),
        size: 256 * SIDE as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    r.write_params(queue, slot, &params);
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let view = target.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("test pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        r.draw(&mut pass, slot, &params, SIDE as f32);
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: Some(SIDE),
            },
        },
        size,
    );
    queue.submit([encoder.finish()]);
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, |r| r.unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let bgra = rs.target_format == wgpu::TextureFormat::Bgra8Unorm;
    let data = readback.slice(..).get_mapped_range().unwrap();
    let mut out = Vec::new();
    for y in 0..SIDE as usize {
        for x in 0..SIDE as usize {
            let o = y * 256 + x * 4;
            let p = [data[o], data[o + 1], data[o + 2], data[o + 3]];
            out.push(if bgra { [p[2], p[1], p[0], p[3]] } else { p });
        }
    }
    out
}

fn params() -> Params {
    Params {
        uv_offset: [0.0, 0.0],
        uv_scale: [1.0, 1.0],
        mip: 0.0,
        channel: 0,
        view_linear: 0,
        tonemap: 0,
        exposure: 1.0,
        tile: 0,
        lens: 0,
    }
}

fn near(got: [u8; 4], want: [u8; 4]) {
    for i in 0..4 {
        assert!(
            (got[i] as i32 - want[i] as i32).abs() <= 3,
            "got {got:?}, want {want:?}"
        );
    }
}

/// An encoded preview of `w` x `h` made of one mip of `block` repeated; `decoded` is what the CPU decode gives.
fn encoded(format: Format, w: u32, h: u32, block: &[u8], decoded: [f32; 4]) -> PreviewOut {
    let blocks = (w.div_ceil(4) * h.div_ceil(4)) as usize;
    PreviewOut {
        result: PreviewResult {
            source: SourcePreview {
                first_level: 0,
                dims: vec![(w, h)],
                levels: vec![],
            },
            format,
            srgb: false,
            levels: vec![block.repeat(blocks)],
            validation: Default::default(),
            warnings: vec![],
            normal_len: vec![],
        },
        decoded: vec![vec![decoded; (w * h) as usize]],
    }
}

/// A BC4 block: red endpoints 128 and 0, every index 0, so every texel is 128.
const BC4_128: [u8; 8] = [128, 0, 0, 0, 0, 0, 0, 0];

#[test]
fn bc5_shows_a_reconstructed_blue_on_both_upload_paths() {
    let Some(rs) = render_state() else { return };
    let mut r = PreviewRenderer::new(rs);
    let block: Vec<u8> = [BC4_128, BC4_128].concat();
    let grey = 128.0 / 255.0;
    // 4x4 is a whole block: BC when the device supports it.
    let out = encoded(Format::Bc5, 4, 4, &block, [grey, grey, 0.0, 1.0]);
    let path = r.set_encoded(rs, &out);
    if r.supports_bc() {
        assert_eq!(path, UploadPath::Bc);
    } else {
        assert!(matches!(path, UploadPath::CpuDecoded(m) if m.contains("no BC texture support")));
    }
    // x = y = 128/255 * 2 - 1 is about 0, so the rebuilt z is about 1, shown as 255.
    let want = [128, 128, 255, 255];
    near(draw(rs, &r, Slot::Encoded, params())[MIDDLE], want);
    // 2x2 is not a multiple of 4: the CPU decode, drawn the same.
    let out = encoded(Format::Bc5, 2, 2, &block, [grey, grey, 0.0, 1.0]);
    let path = r.set_encoded(rs, &out);
    assert!(matches!(path, UploadPath::CpuDecoded(_)));
    if r.supports_bc() {
        assert_eq!(
            path,
            UploadPath::CpuDecoded("Mip size not a multiple of 4: showing a CPU decode")
        );
    }
    near(draw(rs, &r, Slot::Encoded, params())[MIDDLE], want);
}

/// Controller ruling (M6 Task 11): an RG8 normal rebuilds Z the way BC5 does.
#[test]
fn rg8_shows_a_reconstructed_blue_like_bc5() {
    let Some(rs) = render_state() else { return };
    let mut r = PreviewRenderer::new(rs);
    let grey = 128.0 / 255.0;
    // `encoded` repeats its block once per 4x4 block: 4 texels of (128, 128) each time. RG8 always takes the CPU decode.
    let out = encoded(Format::Rg8, 4, 4, &[128; 8], [grey, grey, 0.0, 1.0]);
    assert!(matches!(r.set_encoded(rs, &out), UploadPath::CpuDecoded(_)));
    near(
        draw(rs, &r, Slot::Encoded, params())[MIDDLE],
        [128, 128, 255, 255],
    );
}

#[test]
fn bc4_is_grey_and_channels_can_be_isolated() {
    let Some(rs) = render_state() else { return };
    let mut r = PreviewRenderer::new(rs);
    let grey = 128.0 / 255.0;
    let out = encoded(Format::Bc4, 4, 4, &BC4_128, [grey, grey, grey, 1.0]);
    r.set_encoded(rs, &out);
    near(
        draw(rs, &r, Slot::Encoded, params())[MIDDLE],
        [128, 128, 128, 255],
    );
    // BC4 has no alpha: it reads as opaque, which the alpha view shows as white.
    let alpha = Params {
        channel: 4,
        ..params()
    };
    near(
        draw(rs, &r, Slot::Encoded, alpha)[MIDDLE],
        [255, 255, 255, 255],
    );
}

#[test]
fn the_source_chain_draws_and_a_linear_view_re_encodes_to_srgb() {
    let Some(rs) = render_state() else { return };
    let mut r = PreviewRenderer::new(rs);
    let level = |w: u32, h: u32, v: f32| Rgba32F {
        width: w,
        height: h,
        px: vec![[v, v, v, 1.0]; (w * h) as usize],
    };
    let src = SourcePreview {
        first_level: 0,
        dims: vec![(2, 2), (1, 1)],
        levels: vec![level(2, 2, 0.5), level(1, 1, 0.25)],
    };
    assert!(r.set_source(rs, &src));
    near(
        draw(rs, &r, Slot::Source, params())[MIDDLE],
        [128, 128, 128, 255],
    );
    // 0.5 linear is 0.735 in sRGB.
    let linear = Params {
        view_linear: 1,
        ..params()
    };
    near(
        draw(rs, &r, Slot::Source, linear)[MIDDLE],
        [188, 188, 188, 255],
    );
    // Mip 1 holds 0.25.
    let mip1 = Params {
        mip: 1.0,
        ..params()
    };
    near(draw(rs, &r, Slot::Source, mip1)[MIDDLE], [64, 64, 64, 255]);
    // A cleared slot draws nothing.
    r.clear(Slot::Source);
    near(draw(rs, &r, Slot::Source, params())[MIDDLE], [0, 0, 0, 0]);
}

#[test]
fn the_heatmap_is_shown_as_is_and_the_alpha_mask_blends_over_a_checkerboard() {
    let Some(rs) = render_state() else { return };
    let mut r = PreviewRenderer::new(rs);
    assert!(r.set_heatmap(rs, 1, 1, &[255, 0, 0, 255]));
    near(
        draw(rs, &r, Slot::Heatmap, params())[MIDDLE],
        [255, 0, 0, 255],
    );
    // A fully transparent texel shows only the checkerboard: grey 0.35 or 0.65.
    assert!(r.set_heatmap(rs, 1, 1, &[255, 0, 0, 0]));
    let mask = Params {
        channel: 5,
        ..params()
    };
    let px = draw(rs, &r, Slot::Heatmap, mask)[MIDDLE];
    assert!(px[0] == px[1] && px[1] == px[2], "{px:?}");
    assert!(
        [89, 166].iter().any(|&v| (px[0] as i32 - v).abs() <= 3),
        "{px:?}"
    );
}

#[test]
fn the_wgpu_setup_asks_for_bc_and_the_adapters_limits() {
    let Some(rs) = render_state() else { return };
    let adapter = &rs.adapter;
    let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = wgpu_configuration().wgpu_setup else {
        panic!("a new device is requested");
    };
    let d = (setup.device_descriptor)(adapter);
    assert_eq!(
        d.required_features
            .contains(wgpu::Features::TEXTURE_COMPRESSION_BC),
        adapter
            .features()
            .contains(wgpu::Features::TEXTURE_COMPRESSION_BC)
    );
    assert_eq!(d.required_limits, adapter.limits());
}

#[test]
fn a_bc_chain_down_to_one_texel_uploads_every_mip() {
    let Some(rs) = render_state() else { return };
    let mut r = PreviewRenderer::new(rs);
    if !r.supports_bc() {
        return;
    }
    // 8x8 down to 1x1: the 2x2 and 1x1 mips are each written as a whole 4x4 block.
    let dims = vec![(8, 8), (4, 4), (2, 2), (1, 1)];
    let mut out = encoded(Format::Bc4, 8, 8, &BC4_128, [0.0; 4]);
    out.result.levels = dims
        .iter()
        .map(|&(w, h)| BC4_128.repeat((w as usize).div_ceil(4) * (h as usize).div_ceil(4)))
        .collect();
    out.result.source.dims = dims;
    assert_eq!(r.set_encoded(rs, &out), UploadPath::Bc);
    for mip in [0.0, 1.0, 2.0, 3.0] {
        let p = Params { mip, ..params() };
        near(draw(rs, &r, Slot::Encoded, p)[MIDDLE], [128, 128, 128, 255]);
    }
}

/// Item 15: exposure multiplies before the tone map. EV 0 (factor 1) is the old output; EV +1 (factor 2) shows 0.5 as
/// EV 0 shows 1.0.
#[test]
fn exposure_scales_hdr_values_before_the_tone_map() {
    let Some(rs) = render_state() else { return };
    let mut r = PreviewRenderer::new(rs);
    let level = |w: u32, h: u32, v: f32| Rgba32F {
        width: w,
        height: h,
        px: vec![[v, v, v, 1.0]; (w * h) as usize],
    };
    let src = SourcePreview {
        first_level: 0,
        dims: vec![(2, 2), (1, 1)],
        levels: vec![level(2, 2, 1.0), level(1, 1, 0.5)],
    };
    assert!(r.set_source(rs, &src));
    let hdr = |mip: f32, exposure: f32| Params {
        mip,
        tonemap: 1,
        exposure,
        ..params()
    };
    // 1.0 tone-maps to 0.5 linear, 0.735 in sRGB: 188, as before exposure existed.
    let one = draw(rs, &r, Slot::Source, hdr(0.0, 1.0))[MIDDLE];
    near(one, [188, 188, 188, 255]);
    // Mip 1 holds 0.5: one stop up it shows exactly as 1.0 does.
    near(draw(rs, &r, Slot::Source, hdr(1.0, 2.0))[MIDDLE], one);
    // And it is brighter than 0.5 at EV 0.
    assert!(draw(rs, &r, Slot::Source, hdr(1.0, 1.0))[MIDDLE][0] < one[0] - 10);
}

/// Item 1: the lens draws only the circle inscribed in its rect.
#[test]
fn the_lens_is_a_circle() {
    let Some(rs) = render_state() else { return };
    let mut r = PreviewRenderer::new(rs);
    assert!(r.set_heatmap(rs, 1, 1, &[255, 0, 0, 255]));
    let lens = Params {
        lens: 1,
        ..params()
    };
    let px = draw(rs, &r, Slot::Heatmap, lens);
    near(px[MIDDLE], [255, 0, 0, 255]);
    near(px[0], [0, 0, 0, 0]);
}

/// Item 15: tiling repeats the picture over uv −1..2.
#[test]
fn tiling_draws_the_neighbouring_copies() {
    let Some(rs) = render_state() else { return };
    let mut r = PreviewRenderer::new(rs);
    assert!(r.set_heatmap(rs, 1, 1, &[0, 255, 0, 255]));
    // The rect shows uv −1..2: the corner pixel is in the top left copy.
    let wide = Params {
        uv_offset: [-1.0, -1.0],
        uv_scale: [3.0, 3.0],
        ..params()
    };
    near(draw(rs, &r, Slot::Heatmap, wide)[0], [0, 0, 0, 0]);
    let tiled = Params { tile: 1, ..wide };
    let px = draw(rs, &r, Slot::Heatmap, tiled);
    near(px[0], [0, 255, 0, 255]);
    near(px[MIDDLE], [0, 255, 0, 255]);
}

/// DECISIONS Note AQ: an sRGB colour map's source pane and its encoded pane show the same stored values. Rock01 is a
/// PBR albedo (sRGB colour, filtered in linear light) encoded to lossless RGBA8, so the two sides differ only by
/// rounding.
#[test]
fn an_srgb_colour_source_shows_like_its_lossless_encode() {
    use sdc_gui::model::preview::{EncodedState, SourceState};
    use std::time::Duration;
    let d = tempfile::tempdir().unwrap();
    let mut m = support::pbr_opened(d.path());
    const ROCK: &str = "landscape/rock01.png";
    m.set_field(ROCK, "format", serde_json::json!("rgba8"));
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.expand(ROCK);
    m.encode_now();
    assert!(m.wait_idle(Duration::from_secs(120)));
    let SourceState::Ready(src) = &m.preview.source else {
        panic!("no source")
    };
    let EncodedState::Done(out) = &m.preview.encoded else {
        panic!("not encoded")
    };
    assert_eq!(out.result.format, Format::Rgba8);
    // The images the readout, the heatmap and the lens compare.
    let worst = |a: &[[f32; 4]], b: &[[f32; 4]]| {
        a.iter()
            .zip(b)
            .flat_map(|(s, e)| (0..3).map(move |c| (s[c] - e[c]).abs()))
            .fold(0f32, f32::max)
    };
    let (left, right) = m.compare_sides(0);
    let (left, right) = (left.unwrap(), right.unwrap());
    assert!(
        worst(left.px, right.px) <= 1.0 / 255.0,
        "{}",
        worst(left.px, right.px) * 255.0
    );
    assert!(worst(&src.levels[0].px, &out.decoded[0]) <= 1.0 / 255.0);
    let Some(rs) = render_state() else { return };
    let mut r = PreviewRenderer::new(rs);
    assert!(r.set_source(rs, src));
    r.set_encoded(rs, out);
    for view_linear in [0, 1] {
        let p = Params {
            view_linear,
            ..params()
        };
        let (s, e) = (
            draw(rs, &r, Slot::Source, p),
            draw(rs, &r, Slot::Encoded, p),
        );
        for (s, e) in s.iter().zip(&e) {
            for c in 0..3 {
                assert!(
                    (s[c] as i32 - e[c] as i32).abs() <= 2,
                    "view_linear {view_linear}: source {s:?}, encoded {e:?}"
                );
            }
        }
    }
}

/// Note AQ: a normal map's source side is stored values too (0..1, not the working −1..1 vectors), so it matches
/// the decoded encode of the flat (128, 128, 255) normal.
#[test]
fn a_normal_source_is_in_stored_values() {
    use sdc_gui::model::preview::EncodedState;
    use std::time::Duration;
    let d = tempfile::tempdir().unwrap();
    let mut m = support::pbr_opened(d.path());
    m.expand("landscape/rock01_normal.png");
    m.encode_now();
    assert!(m.wait_idle(Duration::from_secs(120)));
    assert!(matches!(m.preview.encoded, EncodedState::Done(_)));
    let (left, right) = m.compare_sides(0);
    for (s, e) in left.unwrap().px.iter().zip(right.unwrap().px) {
        for c in 0..2 {
            assert!(
                (s[c] - e[c]).abs() <= 2.0 / 255.0,
                "source {s:?}, encoded {e:?}"
            );
        }
    }
}
