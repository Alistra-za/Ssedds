use crate::corpus::{HalfImage, Rgba8};
use encoders_sys::*;
use rayon::prelude::*;

/// bc7e over block rows, 64 blocks per call (as rdo_bc_encoder.cpp does).
pub fn encode_bc7_image(img: &Rgba8, p: Bc7Params) -> Vec<u8> {
    let blocks = gather_blocks(&img.px, img.width, img.height);
    let row = img.width.div_ceil(4) as usize * 16; // texels per block row == bytes per block row for BC7
    let mut out = vec![0u8; blocks.len()];
    out.par_chunks_mut(row)
        .zip(blocks.par_chunks(row))
        .for_each(|(o, b)| {
            for (oc, bc) in o.chunks_mut(64 * 16).zip(b.chunks(64 * 16)) {
                bc7e_compress_blocks(bc, oc, p);
            }
        });
    out
}

#[derive(Debug, Clone, Copy)]
pub enum RgbcxKind {
    Bc1 { transparent_black: bool },
    Bc3,
    Bc4,
    Bc5,
}

/// rgbcx exactly as bc7enc_rdo's tool configures it (level 18, 3-colour on, radius 5, all BC4 modes).
pub fn encode_rgbcx_image(img: &Rgba8, kind: RgbcxKind) -> Vec<u8> {
    let blocks = gather_blocks(&img.px, img.width, img.height);
    let bsize = match kind {
        RgbcxKind::Bc1 { .. } | RgbcxKind::Bc4 => 8,
        RgbcxKind::Bc3 | RgbcxKind::Bc5 => 16,
    };
    let mut out = vec![0u8; blocks.len() / 16 * bsize];
    out.par_chunks_mut(bsize)
        .zip(blocks.par_chunks(16))
        .for_each(|(o, b)| {
            let b: &[[u8; 4]; 16] = b.try_into().unwrap();
            match kind {
                RgbcxKind::Bc1 { transparent_black } => o.copy_from_slice(&bc1_encode_block(
                    b,
                    RGBCX_MAX_LEVEL,
                    true,
                    transparent_black,
                )),
                RgbcxKind::Bc3 => {
                    o.copy_from_slice(&bc3_encode_block(b, RGBCX_MAX_LEVEL, BC45_RADIUS_DEFAULT))
                }
                RgbcxKind::Bc4 => o.copy_from_slice(&bc4_encode_block(b, 0, BC45_RADIUS_DEFAULT)),
                RgbcxKind::Bc5 => o.copy_from_slice(&bc5_encode_block(b, BC45_RADIUS_DEFAULT)),
            }
        });
    out
}

pub fn encode_bc6h_image(img: &HalfImage) -> Vec<u8> {
    let w = img.width;
    let strip = (w * 4) as usize;
    let mut out = vec![0u8; (w / 4 * (img.height / 4) * 16) as usize];
    out.par_chunks_mut((w / 4 * 16) as usize)
        .zip(img.px.par_chunks(strip))
        .for_each(|(o, rows)| {
            bc6h_compress_rows(rows, w, 4, o, Bc6hLevel::Veryslow);
        });
    out
}

fn scatter<T: Copy + Default, const N: usize>(
    w: u32,
    h: u32,
    data: &[u8],
    bsize: usize,
    dec: impl Fn(&[u8]) -> [T; N],
) -> Vec<T> {
    let (bw, bh) = (w.div_ceil(4) as usize, h.div_ceil(4) as usize);
    let per = N / 16;
    let mut img = vec![T::default(); (w * h) as usize * per];
    for by in 0..bh {
        for bx in 0..bw {
            let px = dec(&data[(by * bw + bx) * bsize..][..bsize]);
            for y in 0..4 {
                for x in 0..4 {
                    let (gx, gy) = (bx * 4 + x, by * 4 + y);
                    if gx < w as usize && gy < h as usize {
                        let o = (gy * w as usize + gx) * per;
                        img[o..o + per].copy_from_slice(&px[(y * 4 + x) * per..][..per]);
                    }
                }
            }
        }
    }
    img
}

pub fn decode_bc7_image(data: &[u8], w: u32, h: u32) -> Vec<[u8; 4]> {
    let flat = scatter::<u8, 64>(w, h, data, 16, |b| {
        let mut o = [0u8; 64];
        bcdec_rs::bc7(b, &mut o, 16);
        o
    });
    flat.chunks(4).map(|c| [c[0], c[1], c[2], c[3]]).collect()
}

pub fn decode_bc6h_image(data: &[u8], w: u32, h: u32) -> Vec<[f32; 3]> {
    let flat = scatter::<f32, 48>(w, h, data, 16, |b| {
        let mut o = [0f32; 48];
        bcdec_rs::bc6h_float(b, &mut o, 12, false);
        o
    });
    flat.chunks(3).map(|c| [c[0], c[1], c[2]]).collect()
}

pub fn half_to_rgb_f32(img: &HalfImage) -> Vec<[f32; 3]> {
    img.px
        .iter()
        .map(|p| [0, 1, 2].map(|c| half::f16::from_bits(p[c]).to_f32()))
        .collect()
}
