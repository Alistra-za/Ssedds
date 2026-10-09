//! Decode one DDS level for validation and inspection (`bcdec_rs`; BC1 optionally through rgbcx at a decoder
//! model, spec §5.8).
//!
//! The output size is derived from `data.len()`: every function checks that `data` holds the whole level (with
//! checked arithmetic) before allocating, so a hostile `w`/`h` cannot force a large allocation.
use crate::dds::{DdsError, DdsPixelFormat};
use encoders_sys::{bc1_decode_block, Bc1Model};

#[derive(Debug, Clone, PartialEq)]
pub enum Decoded {
    Ldr(Vec<[u8; 4]>),
    Hdr(Vec<[f32; 3]>),
}

/// Validate `w`/`h` and that `data` holds a whole level; returns the level size in bytes.
fn level_len(fmt: DdsPixelFormat, w: u32, h: u32, data: &[u8]) -> Result<usize, DdsError> {
    if w == 0 || h == 0 {
        return Err(DdsError::Unsupported(format!("zero dimension ({w}x{h})")));
    }
    let need = fmt.checked_level_size(w, h).ok_or(DdsError::Truncated)?;
    if data.len() < need {
        return Err(DdsError::Truncated);
    }
    Ok(need)
}

/// Run `dec` over every `bs`-byte block and gather the `per`-element texels into a row-major `w` x `h` image.
/// `dec` fills a 4x4 scratch tile of `16 * per` elements.
fn blocks<T: Copy + Default>(
    w: u32,
    h: u32,
    data: &[u8],
    bs: usize,
    per: usize,
    mut dec: impl FnMut(&[u8], &mut [T]),
) -> Vec<T> {
    let (w, h) = (w as usize, h as usize);
    let bw = w.div_ceil(4);
    let mut img = vec![T::default(); w * h * per];
    let mut tmp = vec![T::default(); 16 * per];
    for (i, block) in data.chunks_exact(bs).take(bw * h.div_ceil(4)).enumerate() {
        dec(block, &mut tmp);
        let (bx, by) = (i % bw, i / bw);
        for y in 0..4.min(h - by * 4) {
            for x in 0..4.min(w - bx * 4) {
                let o = ((by * 4 + y) * w + bx * 4 + x) * per;
                img[o..o + per].copy_from_slice(&tmp[(y * 4 + x) * per..][..per]);
            }
        }
    }
    img
}

fn quads(v: Vec<u8>) -> Vec<[u8; 4]> {
    v.as_chunks::<4>().0.to_vec()
}

/// Decode a `w` x `h` level to RGBA8 (LDR) or RGB f32 (HDR).
///
/// BC1 with `Some(model)` decodes through rgbcx at that model; with `None` through `bcdec_rs`. BC4 gives
/// `[r, r, r, 255]` and BC5 `[r, g, 0, 255]` (unsigned), BC6H gives RGB floats, half and float formats drop
/// alpha. sRGB tags are not applied.
pub fn decode_level(
    fmt: DdsPixelFormat,
    w: u32,
    h: u32,
    data: &[u8],
    bc1_model: Option<Bc1Model>,
) -> Result<Decoded, DdsError> {
    use DdsPixelFormat::*;
    let need = level_len(fmt, w, h, data)?;
    let data = &data[..need];
    Ok(match fmt {
        Bc1 { .. } => Decoded::Ldr(quads(blocks(w, h, data, 8, 4, |b, o| match bc1_model {
            Some(m) => {
                let blk: &[u8; 8] = b.try_into().expect("8-byte block");
                for (i, p) in bc1_decode_block(blk, m).iter().enumerate() {
                    o[i * 4..i * 4 + 4].copy_from_slice(p);
                }
            }
            None => bcdec_rs::bc1(b, o, 16),
        }))),
        Bc2 { .. } => Decoded::Ldr(quads(blocks(w, h, data, 16, 4, |b, o| {
            bcdec_rs::bc2(b, o, 16)
        }))),
        Bc3 { .. } => Decoded::Ldr(quads(blocks(w, h, data, 16, 4, |b, o| {
            bcdec_rs::bc3(b, o, 16)
        }))),
        Bc7 { .. } => Decoded::Ldr(quads(blocks(w, h, data, 16, 4, |b, o| {
            bcdec_rs::bc7(b, o, 16)
        }))),
        Bc4 => {
            let r = blocks(w, h, data, 8, 1, |b, o| bcdec_rs::bc4(b, o, 4, false));
            Decoded::Ldr(r.iter().map(|&v| [v, v, v, 255]).collect())
        }
        Bc5 => {
            let rg = blocks(w, h, data, 16, 2, |b, o| bcdec_rs::bc5(b, o, 8, false));
            Decoded::Ldr(
                rg.as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| [c[0], c[1], 0, 255])
                    .collect(),
            )
        }
        Bc6hUf16 | Bc6hSf16 => {
            let signed = fmt == Bc6hSf16;
            let f = blocks(w, h, data, 16, 3, |b, o| {
                bcdec_rs::bc6h_float(b, o, 12, signed)
            });
            Decoded::Hdr(
                f.as_chunks::<3>()
                    .0
                    .iter()
                    .map(|c| [c[0], c[1], c[2]])
                    .collect(),
            )
        }
        Rgba8 { .. } => Decoded::Ldr(quads(data.to_vec())),
        Bgra8 | Bgrx8 => Decoded::Ldr(
            data.as_chunks::<4>()
                .0
                .iter()
                .map(|c| [c[2], c[1], c[0], if fmt == Bgrx8 { 255 } else { c[3] }])
                .collect(),
        ),
        R8 => Decoded::Ldr(data.iter().map(|&v| [v, v, v, 255]).collect()),
        Rg8 => Decoded::Ldr(
            data.as_chunks::<2>()
                .0
                .iter()
                .map(|c| [c[0], c[1], 0, 255])
                .collect(),
        ),
        Rgba16f => Decoded::Hdr(
            data.as_chunks::<8>()
                .0
                .iter()
                .map(|c| {
                    [0, 1, 2].map(|k| half::f16::from_le_bytes([c[k * 2], c[k * 2 + 1]]).to_f32())
                })
                .collect(),
        ),
        Rgba32f => Decoded::Hdr(
            data.as_chunks::<16>()
                .0
                .iter()
                .map(|c| {
                    [0, 1, 2].map(|k| {
                        f32::from_le_bytes([c[k * 4], c[k * 4 + 1], c[k * 4 + 2], c[k * 4 + 3]])
                    })
                })
                .collect(),
        ),
    })
}
