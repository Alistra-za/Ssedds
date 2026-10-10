use eframe::egui_wgpu::wgpu;
use eframe::egui_wgpu::{RenderState, RendererOptions};
use engine::config::Format;
use engine::image::Rgba32F;
use engine::session::{PreviewResult, SourcePreview};
use sdc_gui::model::preview::PreviewOut;
use sdc_gui::render::*;

#[test]
fn formats_map_to_unorm_block_formats() {
    assert_eq!(wgpu_format(Format::Bc7), wgpu::TextureFormat::Bc7RgbaUnorm);
    assert_eq!(wgpu_format(Format::Bc5), wgpu::TextureFormat::Bc5RgUnorm);
    assert_eq!(wgpu_format(Format::Bc4), wgpu::TextureFormat::Bc4RUnorm);
    assert_eq!(
        wgpu_format(Format::Bc6hUf16),
        wgpu::TextureFormat::Bc6hRgbUfloat
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
    let instance = wgpu::Instance::default();
    let Ok(adapter) = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
    else {
        return;
    };
    let (device, _queue) =
        block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
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

/// A headless render state built with the wgpu setup `main` hands to eframe.
fn render_state() -> Option<RenderState> {
    let instance = wgpu::Instance::default();
    block_on(RenderState::create(
        &wgpu_configuration(),
        &instance,
        None,
        RendererOptions::default(),
    ))
    .ok()
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
    let mut r = PreviewRenderer::new(&rs);
    let block: Vec<u8> = [BC4_128, BC4_128].concat();
    let grey = 128.0 / 255.0;
    // 4x4 is a whole block: BC when the device supports it.
    let out = encoded(Format::Bc5, 4, 4, &block, [grey, grey, 0.0, 1.0]);
    let path = r.set_encoded(&rs, &out);
    if r.supports_bc() {
        assert_eq!(path, UploadPath::Bc);
    } else {
        assert!(matches!(path, UploadPath::CpuDecoded(m) if m.contains("no BC texture support")));
    }
    // x = y = 128/255 * 2 - 1 is about 0, so the rebuilt z is about 1, shown as 255.
    let want = [128, 128, 255, 255];
    near(draw(&rs, &r, Slot::Encoded, params())[MIDDLE], want);
    // 2x2 is not a multiple of 4: the CPU decode, drawn the same.
    let out = encoded(Format::Bc5, 2, 2, &block, [grey, grey, 0.0, 1.0]);
    let path = r.set_encoded(&rs, &out);
    assert!(matches!(path, UploadPath::CpuDecoded(_)));
    if r.supports_bc() {
        assert_eq!(
            path,
            UploadPath::CpuDecoded("Mip size not a multiple of 4: showing a CPU decode")
        );
    }
    near(draw(&rs, &r, Slot::Encoded, params())[MIDDLE], want);
}

#[test]
fn bc4_is_grey_and_channels_can_be_isolated() {
    let Some(rs) = render_state() else { return };
    let mut r = PreviewRenderer::new(&rs);
    let grey = 128.0 / 255.0;
    let out = encoded(Format::Bc4, 4, 4, &BC4_128, [grey, grey, grey, 1.0]);
    r.set_encoded(&rs, &out);
    near(
        draw(&rs, &r, Slot::Encoded, params())[MIDDLE],
        [128, 128, 128, 255],
    );
    // BC4 has no alpha: it reads as opaque, which the alpha view shows as white.
    let alpha = Params {
        channel: 4,
        ..params()
    };
    near(
        draw(&rs, &r, Slot::Encoded, alpha)[MIDDLE],
        [255, 255, 255, 255],
    );
}

#[test]
fn the_source_chain_draws_and_a_linear_view_re_encodes_to_srgb() {
    let Some(rs) = render_state() else { return };
    let mut r = PreviewRenderer::new(&rs);
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
    assert!(r.set_source(&rs, &src));
    near(
        draw(&rs, &r, Slot::Source, params())[MIDDLE],
        [128, 128, 128, 255],
    );
    // 0.5 linear is 0.735 in sRGB.
    let linear = Params {
        view_linear: 1,
        ..params()
    };
    near(
        draw(&rs, &r, Slot::Source, linear)[MIDDLE],
        [188, 188, 188, 255],
    );
    // Mip 1 holds 0.25.
    let mip1 = Params {
        mip: 1.0,
        ..params()
    };
    near(draw(&rs, &r, Slot::Source, mip1)[MIDDLE], [64, 64, 64, 255]);
    // A cleared slot draws nothing.
    r.clear(Slot::Source);
    near(draw(&rs, &r, Slot::Source, params())[MIDDLE], [0, 0, 0, 0]);
}

#[test]
fn the_heatmap_is_shown_as_is_and_the_alpha_mask_blends_over_a_checkerboard() {
    let Some(rs) = render_state() else { return };
    let mut r = PreviewRenderer::new(&rs);
    assert!(r.set_heatmap(&rs, 1, 1, &[255, 0, 0, 255]));
    near(
        draw(&rs, &r, Slot::Heatmap, params())[MIDDLE],
        [255, 0, 0, 255],
    );
    // A fully transparent texel shows only the checkerboard: grey 0.35 or 0.65.
    assert!(r.set_heatmap(&rs, 1, 1, &[255, 0, 0, 0]));
    let mask = Params {
        channel: 5,
        ..params()
    };
    let px = draw(&rs, &r, Slot::Heatmap, mask)[MIDDLE];
    assert!(px[0] == px[1] && px[1] == px[2], "{px:?}");
    assert!(
        [89, 166].iter().any(|&v| (px[0] as i32 - v).abs() <= 3),
        "{px:?}"
    );
}

#[test]
fn the_wgpu_setup_asks_for_bc_and_the_adapters_limits() {
    let instance = wgpu::Instance::default();
    let Ok(adapter) = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
    else {
        return;
    };
    let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = wgpu_configuration().wgpu_setup else {
        panic!("a new device is requested");
    };
    let d = (setup.device_descriptor)(&adapter);
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
    let mut r = PreviewRenderer::new(&rs);
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
    assert_eq!(r.set_encoded(&rs, &out), UploadPath::Bc);
    for mip in [0.0, 1.0, 2.0, 3.0] {
        let p = Params { mip, ..params() };
        near(
            draw(&rs, &r, Slot::Encoded, p)[MIDDLE],
            [128, 128, 128, 255],
        );
    }
}
