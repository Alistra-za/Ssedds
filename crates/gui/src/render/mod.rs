//! The GPU preview (M3a spec §6.1): encoded mips are uploaded as real BC textures (or as a CPU decode when the
//! device cannot take them), the source chain as `Rgba16Float`, and one WGSL pass shows a texture with channel
//! isolation, an alpha mask, an sRGB / UNORM view and a BC6H tone map. The renderer lives in egui_wgpu's
//! callback resources; [`paint`] adds a paint callback that draws one [`Slot`] into a rect.
mod upload;

pub use upload::{block_layout, can_upload_bc, to_rgba16f, wgpu_format};

use crate::model::preview::PreviewOut;
use eframe::egui;
use eframe::egui_wgpu::{self, wgpu};
use engine::config::Format;
use engine::session::SourcePreview;
use std::sync::Arc;

const NO_BC: &str = "This GPU has no BC texture support: showing a CPU decode";
const NOT_MULTIPLE_OF_4: &str = "Mip size not a multiple of 4: showing a CPU decode";
const UNCOMPRESSED: &str = "uncompressed format: shown from the CPU decode";
const TOO_LARGE: &str = "This mip is larger than the GPU texture limit: no preview";
const NOT_DECODED: &str = "The decoded mips are missing: no preview";

/// Which texture a paint callback draws.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slot {
    Source,
    Encoded,
    Heatmap,
}

impl Slot {
    fn index(self) -> usize {
        self as usize
    }
}

/// What the shader does for one draw. `channel`: 0 RGBA, 1 R, 2 G, 3 B, 4 A, 5 alpha as a mask over a
/// checkerboard. `view_linear`: show the bytes as linear data (re-encoded to sRGB for display). `tonemap`:
/// Reinhard, for BC6H. `uv_offset`/`uv_scale` map the rect to the part of the texture shown; `mip` is the level
/// sampled.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    pub uv_offset: [f32; 2],
    pub uv_scale: [f32; 2],
    pub mip: f32,
    pub channel: u32,
    pub view_linear: u32,
    pub tonemap: u32,
}

/// The uniform buffer: the [`Params`] fields, then the slot's own `kind` (how its channels map to colour), padded
/// to the uniform block's 16-byte alignment.
const PARAMS_BYTES: usize = 48;

impl Params {
    fn to_bytes(self, kind: u32) -> [u8; PARAMS_BYTES] {
        let mut b = [0u8; PARAMS_BYTES];
        let floats = [
            self.uv_offset[0],
            self.uv_offset[1],
            self.uv_scale[0],
            self.uv_scale[1],
            self.mip,
        ];
        for (i, f) in floats.iter().enumerate() {
            b[i * 4..i * 4 + 4].copy_from_slice(&f.to_le_bytes());
        }
        let words = [self.channel, self.view_linear, self.tonemap, kind];
        for (i, w) in words.iter().enumerate() {
            b[20 + i * 4..24 + i * 4].copy_from_slice(&w.to_le_bytes());
        }
        b
    }
}

/// How an encoded mip reached the GPU.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UploadPath {
    Bc,
    /// The CPU decode was uploaded instead; the text says why (shown as the notice, spec §6.4).
    CpuDecoded(&'static str),
    /// Nothing could be uploaded (the mips exceed the device's texture limit, or are missing); the slot is empty.
    Skipped(&'static str),
}

/// How the shader maps a slot's channels to colour.
const KIND_PLAIN: u32 = 0;
const KIND_BC4: u32 = 1;
const KIND_BC5: u32 = 2;

fn kind_of(format: Format) -> u32 {
    match format {
        Format::Bc4 => KIND_BC4,
        Format::Bc5 => KIND_BC5,
        _ => KIND_PLAIN,
    }
}

struct SlotTexture {
    _texture: wgpu::Texture,
    params: wgpu::Buffer,
    bind_nearest: wgpu::BindGroup,
    bind_linear: wgpu::BindGroup,
    /// Size of mip 0.
    base: (u32, u32),
    kind: u32,
}

pub struct PreviewRenderer {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    nearest: wgpu::Sampler,
    linear: wgpu::Sampler,
    slots: [Option<SlotTexture>; 3],
    supports_bc: bool,
    max_dim: u32,
}

impl PreviewRenderer {
    pub fn new(rs: &egui_wgpu::RenderState) -> PreviewRenderer {
        let device = &rs.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sdc preview"),
            source: wgpu::ShaderSource::Wgsl(include_str!("preview.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sdc preview layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: std::num::NonZeroU64::new(PARAMS_BYTES as u64),
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sdc preview pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sdc preview pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: rs.target_format,
                    // egui's premultiplied blend: the transparent texels outside the image show what is behind.
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = |label, filter, mipmap_filter| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some(label),
                mag_filter: filter,
                min_filter: filter,
                mipmap_filter,
                ..Default::default()
            })
        };
        PreviewRenderer {
            pipeline,
            layout,
            nearest: sampler(
                "sdc preview nearest",
                wgpu::FilterMode::Nearest,
                wgpu::MipmapFilterMode::Nearest,
            ),
            linear: sampler(
                "sdc preview linear",
                wgpu::FilterMode::Linear,
                wgpu::MipmapFilterMode::Linear,
            ),
            slots: [None, None, None],
            supports_bc: device
                .features()
                .contains(wgpu::Features::TEXTURE_COMPRESSION_BC),
            max_dim: device.limits().max_texture_dimension_2d,
        }
    }

    /// Whether the device was created with `TEXTURE_COMPRESSION_BC`.
    pub fn supports_bc(&self) -> bool {
        self.supports_bc
    }

    /// The device's largest texture side.
    pub fn max_dim(&self) -> u32 {
        self.max_dim
    }

    /// Whether `slot` holds a texture to draw.
    pub fn has(&self, slot: Slot) -> bool {
        self.slots[slot.index()].is_some()
    }

    pub fn clear(&mut self, slot: Slot) {
        self.slots[slot.index()] = None;
    }

    fn store(
        &mut self,
        device: &wgpu::Device,
        slot: Slot,
        texture: wgpu::Texture,
        base: (u32, u32),
        kind: u32,
    ) {
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sdc preview params"),
            size: PARAMS_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind = |sampler: &wgpu::Sampler| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("sdc preview bind group"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: params.as_entire_binding(),
                    },
                ],
            })
        };
        let (bind_nearest, bind_linear) = (bind(&self.nearest), bind(&self.linear));
        self.slots[slot.index()] = Some(SlotTexture {
            _texture: texture,
            params,
            bind_nearest,
            bind_linear,
            base,
            kind,
        });
    }

    /// The source chain (mips from `src.first_level` on) as `Rgba16Float`. Returns false, with the slot empty,
    /// when there is nothing to show (a DDS input has no source) or the base mip exceeds the device's limit.
    pub fn set_source(&mut self, rs: &egui_wgpu::RenderState, src: &SourcePreview) -> bool {
        self.clear(Slot::Source);
        let levels: Vec<(u32, u32, &[[f32; 4]])> = src
            .levels
            .iter()
            .map(|l| (l.width, l.height, l.px.as_slice()))
            .collect();
        let Some(&(w, h, _)) = levels.first() else {
            return false;
        };
        if w.max(h) > self.max_dim {
            return false;
        }
        match upload::rgba16f_chain(&rs.device, &rs.queue, "preview source", &levels) {
            Some((texture, _)) => {
                self.store(&rs.device, Slot::Source, texture, (w, h), KIND_PLAIN);
                true
            }
            None => false,
        }
    }

    /// The encoded mips: as a BC texture when the device and the first mip's size allow, else the CPU decode.
    pub fn set_encoded(&mut self, rs: &egui_wgpu::RenderState, out: &PreviewOut) -> UploadPath {
        self.clear(Slot::Encoded);
        let r = &out.result;
        let first = r.source.first_level;
        let count = r.levels.len();
        let Some(dims) = r.source.dims.get(first..first + count) else {
            return UploadPath::Skipped(NOT_DECODED);
        };
        let Some(&base) = dims.first() else {
            return UploadPath::Skipped(NOT_DECODED);
        };
        if base.0.max(base.1) > self.max_dim {
            return UploadPath::Skipped(TOO_LARGE);
        }
        let kind = kind_of(r.format);
        let reason = if r.format.is_block() && can_upload_bc(self.supports_bc, self.max_dim, base) {
            if let Some((texture, _)) =
                upload::bc_chain(&rs.device, &rs.queue, r.format, dims, &r.levels)
            {
                self.store(&rs.device, Slot::Encoded, texture, base, kind);
                return UploadPath::Bc;
            }
            NOT_DECODED
        } else if !r.format.is_block() {
            UNCOMPRESSED
        } else if !self.supports_bc {
            NO_BC
        } else {
            NOT_MULTIPLE_OF_4
        };
        let levels: Vec<(u32, u32, &[[f32; 4]])> = dims
            .iter()
            .zip(&out.decoded)
            .map(|(&(w, h), px)| (w, h, px.as_slice()))
            .collect();
        match upload::rgba16f_chain(&rs.device, &rs.queue, "preview decoded", &levels) {
            Some((texture, _)) => {
                self.store(&rs.device, Slot::Encoded, texture, base, kind);
                UploadPath::CpuDecoded(reason)
            }
            None => UploadPath::Skipped(NOT_DECODED),
        }
    }

    /// The error heatmap (`rgba` is `w * h * 4` bytes), shown as is.
    pub fn set_heatmap(
        &mut self,
        rs: &egui_wgpu::RenderState,
        w: u32,
        h: u32,
        rgba: &[u8],
    ) -> bool {
        self.clear(Slot::Heatmap);
        if w.max(h) > self.max_dim {
            return false;
        }
        match upload::rgba8(&rs.device, &rs.queue, "preview heatmap", w, h, rgba) {
            Some(texture) => {
                self.store(&rs.device, Slot::Heatmap, texture, (w, h), KIND_PLAIN);
                true
            }
            None => false,
        }
    }

    /// Writes the slot's uniform block (before the render pass). A slot is drawn with one set of params per frame.
    pub fn write_params(&self, queue: &wgpu::Queue, slot: Slot, params: &Params) {
        if let Some(s) = &self.slots[slot.index()] {
            queue.write_buffer(&s.params, 0, &params.to_bytes(s.kind));
        }
    }

    /// Draws the slot into the pass's viewport. `viewport_px_width` (physical pixels) picks the sampler: nearest
    /// once a texel covers at least one pixel, linear while the image is shrunk.
    pub fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        slot: Slot,
        params: &Params,
        viewport_px_width: f32,
    ) {
        let Some(s) = &self.slots[slot.index()] else {
            return;
        };
        let nearest = pixels_per_texel(s.base.0, params, viewport_px_width) >= 1.0;
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(
            0,
            if nearest {
                &s.bind_nearest
            } else {
                &s.bind_linear
            },
            &[],
        );
        pass.draw(0..3, 0..1);
    }
}

/// Screen pixels per texel of the sampled mip: the viewport's width over the texels of that mip it shows.
fn pixels_per_texel(base_width: u32, params: &Params, viewport_px_width: f32) -> f32 {
    let mip_width = (base_width >> (params.mip.max(0.0) as u32).min(31)).max(1) as f32;
    viewport_px_width / (mip_width * params.uv_scale[0]).max(f32::MIN_POSITIVE)
}

struct PaintSlot {
    slot: Slot,
    params: Params,
}

impl egui_wgpu::CallbackTrait for PaintSlot {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(r) = resources.get::<PreviewRenderer>() {
            r.write_params(queue, self.slot, &self.params);
        }
        vec![]
    }

    fn paint(
        &self,
        info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        if let Some(r) = resources.get::<PreviewRenderer>() {
            r.draw(
                pass,
                self.slot,
                &self.params,
                info.viewport.width() * info.pixels_per_point,
            );
        }
    }
}

/// Draws `slot` into `rect`. Does nothing visible when the slot is empty or the app has no renderer.
pub fn paint(ui: &mut egui::Ui, rect: egui::Rect, slot: Slot, params: Params) {
    ui.painter().add(egui_wgpu::Callback::new_paint_callback(
        rect,
        PaintSlot { slot, params },
    ));
}

/// The wgpu setup `main` hands to eframe: egui's own defaults, plus `TEXTURE_COMPRESSION_BC` when the adapter has
/// it and the adapter's own limits (so a 16384-sided preview texture is allowed where the hardware allows it).
pub fn wgpu_configuration() -> egui_wgpu::WgpuConfiguration {
    let mut setup = egui_wgpu::WgpuSetupCreateNew::without_display_handle();
    let default_descriptor = Arc::clone(&setup.device_descriptor);
    setup.device_descriptor = Arc::new(move |adapter| {
        let mut descriptor = default_descriptor(adapter);
        if adapter
            .features()
            .contains(wgpu::Features::TEXTURE_COMPRESSION_BC)
        {
            descriptor.required_features |= wgpu::Features::TEXTURE_COMPRESSION_BC;
        }
        descriptor.required_limits = adapter.limits();
        descriptor
    });
    egui_wgpu::WgpuConfiguration {
        wgpu_setup: egui_wgpu::WgpuSetup::CreateNew(setup),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(mip: f32, scale: f32) -> Params {
        Params {
            uv_offset: [0.0; 2],
            uv_scale: [scale, scale],
            mip,
            channel: 0,
            view_linear: 0,
            tonemap: 0,
        }
    }

    #[test]
    fn params_bytes_are_little_endian_fields_then_kind() {
        let p = Params {
            uv_offset: [1.0, 2.0],
            uv_scale: [0.5, 0.25],
            mip: 3.0,
            channel: 4,
            view_linear: 1,
            tonemap: 1,
        };
        let b = p.to_bytes(2);
        assert_eq!(b.len(), 48);
        assert_eq!(&b[0..4], &1.0f32.to_le_bytes());
        assert_eq!(&b[12..16], &0.25f32.to_le_bytes());
        assert_eq!(&b[16..20], &3.0f32.to_le_bytes());
        assert_eq!(&b[20..24], &4u32.to_le_bytes());
        assert_eq!(&b[28..32], &1u32.to_le_bytes());
        assert_eq!(&b[32..36], &2u32.to_le_bytes());
        assert!(b[36..].iter().all(|&x| x == 0));
    }

    #[test]
    fn nearest_once_a_texel_covers_a_pixel() {
        // 1024 texels in 2048 pixels: zoomed in.
        assert!(pixels_per_texel(1024, &params(0.0, 1.0), 2048.0) >= 1.0);
        // 1024 texels in 512 pixels: shrunk.
        assert!(pixels_per_texel(1024, &params(0.0, 1.0), 512.0) < 1.0);
        // Mip 2 is 256 wide: 512 pixels cover each texel twice.
        assert!(pixels_per_texel(1024, &params(2.0, 1.0), 512.0) >= 1.0);
        // Zooming into a quarter of the image doubles the magnification.
        assert!(pixels_per_texel(1024, &params(0.0, 0.25), 512.0) >= 1.0);
    }
}
