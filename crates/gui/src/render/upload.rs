//! Texture formats, layouts and uploads for the preview renderer. The pure helpers are testable without a GPU.
use eframe::egui_wgpu::wgpu;
use engine::config::Format;

/// The block format a mip is uploaded as. Always UNORM: the shader decides how to show the bytes (spec §6.1).
pub fn wgpu_format(format: Format) -> Option<wgpu::TextureFormat> {
    Some(match format {
        Format::Bc1 => wgpu::TextureFormat::Bc1RgbaUnorm,
        Format::Bc3 => wgpu::TextureFormat::Bc3RgbaUnorm,
        Format::Bc4 => wgpu::TextureFormat::Bc4RUnorm,
        Format::Bc5 => wgpu::TextureFormat::Bc5RgUnorm,
        Format::Bc6hUf16 => wgpu::TextureFormat::Bc6hRgbUfloat,
        Format::Bc7 => wgpu::TextureFormat::Bc7RgbaUnorm,
        Format::Rgba8 | Format::R8 | Format::Rg8 | Format::Rgba16f => return None,
    })
}

/// Bytes per row of blocks and the number of block rows of a `w` x `h` mip.
pub fn block_layout(format: Format, w: u32, h: u32) -> (u32, u32) {
    (
        w.div_ceil(4) * format.block_bytes().unwrap_or(0) as u32,
        h.div_ceil(4),
    )
}

/// BC upload needs the device feature, a first mip whose sides are multiples of 4 (a smaller mip may be 2x2 or 1x1)
/// and sides within the device's texture limit.
pub fn can_upload_bc(supports_bc: bool, max_dim: u32, first: (u32, u32)) -> bool {
    supports_bc
        && first.0 > 0
        && first.1 > 0
        && first.0.is_multiple_of(4)
        && first.1.is_multiple_of(4)
        && first.0 <= max_dim
        && first.1 <= max_dim
}

/// RGBA `f16`, little-endian, for an `Rgba16Float` texture.
pub fn to_rgba16f(px: &[[f32; 4]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(px.len() * 8);
    for p in px {
        for &v in p {
            out.extend_from_slice(&half::f16::from_f32(v).to_le_bytes());
        }
    }
    out
}

/// Side lengths of mip `level` below a base of `base`.
fn mip_size(base: (u32, u32), level: u32) -> (u32, u32) {
    ((base.0 >> level).max(1), (base.1 >> level).max(1))
}

/// How many leading entries of `dims` form a standard mip chain below `dims[0]`. A texture's mips are fixed by its
/// base size, so a chain that departs from that is cut there rather than failing validation.
fn chain_len(dims: &[(u32, u32)]) -> u32 {
    let Some(&base) = dims.first() else { return 0 };
    let full = 32 - base.0.max(base.1).leading_zeros();
    dims.iter()
        .take(full as usize)
        .enumerate()
        .take_while(|(i, d)| **d == mip_size(base, *i as u32))
        .count() as u32
}

fn texture_desc(
    label: &'static str,
    format: wgpu::TextureFormat,
    size: (u32, u32),
    mips: u32,
) -> wgpu::TextureDescriptor<'static> {
    wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: mips,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    }
}

/// One mip's bytes: `rows` rows of `bytes_per_row`, copied over `extent` texels.
struct MipWrite<'a> {
    level: u32,
    data: &'a [u8],
    bytes_per_row: u32,
    rows: u32,
    extent: (u32, u32),
}

fn write_mip(queue: &wgpu::Queue, texture: &wgpu::Texture, m: MipWrite) {
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: m.level,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        m.data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(m.bytes_per_row),
            rows_per_image: Some(m.rows),
        },
        wgpu::Extent3d {
            width: m.extent.0,
            height: m.extent.1,
            depth_or_array_layers: 1,
        },
    );
}

/// An `Rgba16Float` texture holding `levels` (each `(w, h, pixels)`), the chain starting at `levels[0]`. Returns
/// the texture and the number of mips uploaded, or `None` when there is nothing valid to upload.
pub(super) fn rgba16f_chain(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &'static str,
    levels: &[(u32, u32, &[[f32; 4]])],
) -> Option<(wgpu::Texture, u32)> {
    let dims: Vec<(u32, u32)> = levels
        .iter()
        .take_while(|(w, h, px)| px.len() == *w as usize * *h as usize)
        .map(|&(w, h, _)| (w, h))
        .collect();
    let mips = chain_len(&dims);
    if mips == 0 {
        return None;
    }
    let texture = device.create_texture(&texture_desc(
        label,
        wgpu::TextureFormat::Rgba16Float,
        dims[0],
        mips,
    ));
    for (i, &(w, h, px)) in levels.iter().take(mips as usize).enumerate() {
        let bytes = to_rgba16f(px);
        let m = MipWrite {
            level: i as u32,
            data: &bytes,
            bytes_per_row: w * 8,
            rows: h,
            extent: (w, h),
        };
        write_mip(queue, &texture, m);
    }
    Some((texture, mips))
}

/// A BC texture from the encoded mips (`levels[i]` is `dims[i]`). The base must satisfy [`can_upload_bc`]. A copy
/// into a block-compressed texture covers whole blocks, so a 2x2 or 1x1 mip is written as its 4x4 block.
pub(super) fn bc_chain(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    format: Format,
    dims: &[(u32, u32)],
    levels: &[Vec<u8>],
) -> Option<(wgpu::Texture, u32)> {
    let usable: Vec<(u32, u32)> = dims
        .iter()
        .zip(levels)
        .take_while(|(&(w, h), data)| {
            let (row, rows) = block_layout(format, w, h);
            data.len() >= row as usize * rows as usize
        })
        .map(|(&d, _)| d)
        .collect();
    let mips = chain_len(&usable);
    if mips == 0 {
        return None;
    }
    let texture = device.create_texture(&texture_desc(
        "preview encoded",
        wgpu_format(format)?,
        usable[0],
        mips,
    ));
    for (i, &(w, h)) in usable.iter().take(mips as usize).enumerate() {
        let (row, rows) = block_layout(format, w, h);
        let m = MipWrite {
            level: i as u32,
            data: &levels[i][..row as usize * rows as usize],
            bytes_per_row: row,
            rows,
            extent: (w.div_ceil(4) * 4, h.div_ceil(4) * 4),
        };
        write_mip(queue, &texture, m);
    }
    Some((texture, mips))
}

/// A single-mip `Rgba8Unorm` texture.
pub(super) fn rgba8(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &'static str,
    w: u32,
    h: u32,
    rgba: &[u8],
) -> Option<wgpu::Texture> {
    if w == 0 || h == 0 || rgba.len() != w as usize * h as usize * 4 {
        return None;
    }
    let texture = device.create_texture(&texture_desc(
        label,
        wgpu::TextureFormat::Rgba8Unorm,
        (w, h),
        1,
    ));
    let m = MipWrite {
        level: 0,
        data: rgba,
        bytes_per_row: w * 4,
        rows: h,
        extent: (w, h),
    };
    write_mip(queue, &texture, m);
    Some(texture)
}
