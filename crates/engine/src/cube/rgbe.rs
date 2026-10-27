//! Radiance RGBE (`.hdr`) reader, moved from render3d (Note DN1). The workspace `image` crate has no `hdr`
//! feature, so the format is read here.

/// Largest side and pixel count the reader accepts (the bundled image is 2048x1024).
pub const MAX_SIDE: usize = 16384;
pub const MAX_PIXELS: usize = 64 << 20;

/// A decoded image: linear RGB, 3 `f32` per pixel, top row first.
#[derive(Debug, Clone)]
pub struct Hdr {
    pub width: usize,
    pub height: usize,
    pub rgb: Vec<f32>,
}

/// Decodes a Radiance RGBE file. Only the `-Y h +X w` orientation is accepted (every common export).
pub fn decode_rgbe(b: &[u8]) -> Result<Hdr, String> {
    if !b.starts_with(b"#?") {
        return Err("not a Radiance HDR file (no #? magic)".into());
    }
    // Header: text lines up to the first empty line.
    let mut pos = 0;
    let line = |pos: &mut usize| -> Option<&[u8]> {
        let rest = b.get(*pos..)?;
        let n = rest.iter().position(|&c| c == b'\n')?;
        *pos += n + 1;
        Some(&rest[..n])
    };
    let mut format_ok = true;
    loop {
        let l = line(&mut pos).ok_or("HDR header ends before the blank line")?;
        if l.is_empty() {
            break;
        }
        if let Some(f) = l.strip_prefix(b"FORMAT=") {
            format_ok = f == b"32-bit_rle_rgbe";
        }
    }
    if !format_ok {
        return Err("HDR format is not 32-bit_rle_rgbe".into());
    }
    let res = line(&mut pos).ok_or("HDR has no resolution line")?;
    let res = std::str::from_utf8(res).map_err(|_| "HDR resolution line is not text")?;
    let t: Vec<&str> = res.split_whitespace().collect();
    let (h, w) = match t[..] {
        ["-Y", h, "+X", w] => (
            h.parse::<usize>().map_err(|e| format!("HDR height: {e}"))?,
            w.parse::<usize>().map_err(|e| format!("HDR width: {e}"))?,
        ),
        _ => return Err(format!("unsupported HDR orientation {res:?}")),
    };
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE || w * h > MAX_PIXELS {
        return Err(format!("bad HDR size {w}x{h}"));
    }
    // Before any allocation: the payload must plausibly hold the image (flat: exactly w*h*4 bytes; RLE: at
    // least a 4-byte header per scanline plus one run byte-pair per channel).
    let payload = b.len() - pos;
    let rle_ok = (8..32768).contains(&w) && b[pos..].starts_with(&[2, 2]);
    let min = if rle_ok {
        h * (4 + 4 * w.div_ceil(127) * 2) // a run packet covers at most 127 pixels
    } else {
        w * h * 4
    };
    if payload < min {
        return Err(format!("HDR payload too short for {w}x{h}"));
    }
    let mut rgb = Vec::new();
    rgb.try_reserve_exact(w * h * 3)
        .map_err(|e| format!("HDR {w}x{h}: {e}"))?;
    let mut scan = vec![0u8; w * 4];
    for y in 0..h {
        let data = &b[pos..];
        let rle = (8..32768).contains(&w)
            && data.len() >= 4
            && data[..2] == [2, 2]
            && data[2] & 0x80 == 0;
        if rle {
            if (data[2] as usize) << 8 | data[3] as usize != w {
                return Err(format!("HDR scanline {y}: width mismatch"));
            }
            pos += 4;
            for c in 0..4 {
                let mut x = 0;
                while x < w {
                    let n = *b.get(pos).ok_or("HDR ends inside a scanline")? as usize;
                    pos += 1;
                    let (count, run) = if n > 128 { (n - 128, true) } else { (n, false) };
                    if count == 0 || x + count > w {
                        return Err(format!("HDR scanline {y}: bad run"));
                    }
                    if run {
                        let v = *b.get(pos).ok_or("HDR ends inside a scanline")?;
                        pos += 1;
                        for i in 0..count {
                            scan[(x + i) * 4 + c] = v;
                        }
                    } else {
                        let lit = b
                            .get(pos..pos + count)
                            .ok_or("HDR ends inside a scanline")?;
                        pos += count;
                        for (i, v) in lit.iter().enumerate() {
                            scan[(x + i) * 4 + c] = *v;
                        }
                    }
                    x += count;
                }
            }
        } else {
            let flat = b
                .get(pos..pos + w * 4)
                .ok_or("HDR ends inside a flat scanline")?;
            scan.copy_from_slice(flat);
            pos += w * 4;
        }
        for px in scan.as_chunks::<4>().0 {
            let e = px[3];
            let f = if e == 0 {
                0.0
            } else {
                (e as f32 - 136.0).exp2()
            };
            rgb.extend([px[0] as f32 * f, px[1] as f32 * f, px[2] as f32 * f]);
        }
    }
    Ok(Hdr {
        width: w,
        height: h,
        rgb,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(w: usize, h: usize) -> Vec<u8> {
        format!("#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n-Y {h} +X {w}\n").into_bytes()
    }

    #[test]
    fn flat_pixels_decode_mantissa_times_two_to_e_minus_136() {
        let mut b = header(2, 1);
        // (128, 64, 0) at e = 129 -> x 2^-7... e-136 = -7; second pixel e = 0 is black.
        b.extend([128, 64, 0, 129, 9, 9, 9, 0]);
        let h = decode_rgbe(&b).unwrap();
        assert_eq!((h.width, h.height), (2, 1));
        assert_eq!(h.rgb, [1.0, 0.5, 0.0, 0.0, 0.0, 0.0]);
    }
}
