//! Radiance RGBE (`.hdr`) reader and the bundled studio HDRI (Poly Haven `studio_small_09`, 2K, CC0; see
//! `assets/hdri/README.md`). The workspace `image` crate has no `hdr` feature, so the format is read here:
//! header lines up to the blank line, the `-Y h +X w` resolution line, then new-style RLE scanlines (or flat
//! RGBE pixels).
//!
//! **Mapping (Z-up world, like the renderer):** the image is equirectangular, row 0 = +Z (straight up), the
//! last row = −Z; the column is the azimuth `atan2(y, x)` turned by [`YAW`]. [`dir_to_uv`] and [`uv_to_dir`]
//! are the one definition: the background shader, the CPU ambient and the tests all go through it.
use std::f32::consts::{PI, TAU};
use std::sync::OnceLock;

/// The bundled HDRI file (6.3 MB).
static STUDIO_HDR: &[u8] = include_bytes!("../assets/hdri/studio_small_09_2k.hdr");

/// Azimuth (radians) added to the world azimuth before the column lookup. Chosen so the image's brightest
/// region (its key light) lies at the azimuth of the default directional light (`Scene::neutral`), upper front
/// left of the default camera. See `studio_key_light_faces_the_default_sun`.
pub const YAW: f32 = -2.798;

/// The HDRI is a photographic exposure (mean luminance 0.85, ambient means 0.65-1.3), several times the
/// neutral preview's calibrated ambient (0.12-0.30), and on top of the sun it would white out every material.
/// The ambient taken from it is scaled by this; the background is shown as is.
pub const AMBIENT_SCALE: f32 = 0.3;

/// A decoded image: linear RGB, 3 `f32` per pixel, top row first.
#[derive(Debug, Clone)]
pub struct Hdr {
    pub width: usize,
    pub height: usize,
    pub rgb: Vec<f32>,
}

/// World direction (unit, Z-up) → image coordinates `(u, v)` in 0..1 (`v` = 0 at +Z).
pub fn dir_to_uv(d: [f32; 3]) -> (f32, f32) {
    let u = ((d[1].atan2(d[0]) + YAW) / TAU + 0.5).rem_euclid(1.0);
    let v = d[2].clamp(-1.0, 1.0).acos() / PI;
    (u, v)
}

/// Inverse of [`dir_to_uv`].
pub fn uv_to_dir(u: f32, v: f32) -> [f32; 3] {
    let phi = (u - 0.5) * TAU - YAW;
    let theta = v * PI;
    [
        theta.sin() * phi.cos(),
        theta.sin() * phi.sin(),
        theta.cos(),
    ]
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
    if w == 0 || h == 0 || w.checked_mul(h).is_none_or(|n| n > 1 << 28) {
        return Err(format!("bad HDR size {w}x{h}"));
    }
    let mut rgb = Vec::with_capacity(w * h * 3);
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

/// Ambient colours for the six DALC axes (+X −X +Y −Y +Z −Z, world space) from an equirectangular image: for each
/// axis the cosine-weighted mean radiance over the hemisphere around it. The image is box-downsampled to at most
/// 64×32 first (solid-angle weighted, so the poles do not dominate).
pub fn ambient6(img: &Hdr) -> [[f32; 3]; 6] {
    let (ow, oh) = (img.width.min(64), img.height.min(32));
    let axes: [[f32; 3]; 6] = [
        [1.0, 0.0, 0.0],
        [-1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, -1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, -1.0],
    ];
    let mut sum = [[0.0f64; 3]; 6];
    let mut wsum = [0.0f64; 6];
    for oy in 0..oh {
        let (y0, y1) = (oy * img.height / oh, (oy + 1) * img.height / oh);
        let v = (oy as f32 + 0.5) / oh as f32;
        let solid = (v * PI).sin() as f64;
        for ox in 0..ow {
            let (x0, x1) = (ox * img.width / ow, (ox + 1) * img.width / ow);
            let mut c = [0.0f64; 3];
            for y in y0..y1 {
                for x in x0..x1 {
                    for (k, s) in c.iter_mut().enumerate() {
                        *s += img.rgb[(y * img.width + x) * 3 + k] as f64;
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)).max(1) as f64;
            let d = uv_to_dir((ox as f32 + 0.5) / ow as f32, v);
            for (a, axis) in axes.iter().enumerate() {
                let cos = (d[0] * axis[0] + d[1] * axis[1] + d[2] * axis[2]).max(0.0) as f64;
                let w = cos * solid;
                wsum[a] += w;
                for k in 0..3 {
                    sum[a][k] += w * c[k] / n;
                }
            }
        }
    }
    let mut out = [[0.0f32; 3]; 6];
    for a in 0..6 {
        for k in 0..3 {
            out[a][k] = (sum[a][k] / wsum[a].max(1e-12)) as f32;
        }
    }
    out
}

/// The bundled studio HDRI with its ambient, decoded on first use.
pub struct Studio {
    pub image: Hdr,
    pub ambient6: [[f32; 3]; 6],
}

/// Decodes the bundled HDRI once (about 0.1 s) and caches it for the process.
pub fn studio() -> &'static Studio {
    static STUDIO: OnceLock<Studio> = OnceLock::new();
    STUDIO.get_or_init(|| {
        let image = decode_rgbe(STUDIO_HDR).expect("the bundled HDRI decodes");
        let ambient6 = ambient6(&image).map(|c| c.map(|v| v * AMBIENT_SCALE));
        Studio { image, ambient6 }
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

    #[test]
    fn rle_scanline_decodes_runs_and_literals() {
        let w = 8;
        let mut b = header(w, 1);
        b.extend([2, 2, 0, w as u8]);
        // R: a run of 8 x 128; G: 3 literals then a run of 5; B: all 0 (run); E: run of 8 x 136 (scale 1).
        b.extend([128 + 8, 128]);
        b.extend([3, 1, 2, 3, 128 + 5, 7]);
        b.extend([128 + 8, 0]);
        b.extend([128 + 8, 136]);
        let h = decode_rgbe(&b).unwrap();
        assert_eq!(h.rgb.len(), 24);
        assert!(h.rgb.chunks(3).all(|p| p[0] == 128.0 && p[2] == 0.0));
        let g: Vec<f32> = h.rgb.chunks(3).map(|p| p[1]).collect();
        assert_eq!(g, [1.0, 2.0, 3.0, 7.0, 7.0, 7.0, 7.0, 7.0]);
    }

    #[test]
    fn bad_input_is_an_error() {
        assert!(decode_rgbe(b"nope").is_err());
        let mut b = header(2, 1);
        b.extend([1, 2, 3]); // truncated
        assert!(decode_rgbe(&b).is_err());
        let b = b"#?RADIANCE\n\n+Y 1 +X 1\n".to_vec();
        assert!(decode_rgbe(&b).is_err());
    }

    #[test]
    fn uv_and_dir_round_trip() {
        for d in [[0.6, 0.0, 0.8], [-0.3, 0.5, -0.812_403_8], [0.0, -1.0, 0.0]] {
            let (u, v) = dir_to_uv(d);
            let e = uv_to_dir(u, v);
            for k in 0..3 {
                assert!((d[k] - e[k]).abs() < 1e-4, "{d:?} {e:?}");
            }
        }
    }

    #[test]
    fn bright_top_gives_bright_up_ambient() {
        // 8x4: the top two rows are 1.0, the bottom two black.
        let mut rgb = vec![0.0; 8 * 4 * 3];
        rgb[..8 * 2 * 3].fill(1.0);
        let a = ambient6(&Hdr {
            width: 8,
            height: 4,
            rgb,
        });
        assert!(a[4][0] > 0.6 && a[5][0] < 0.4, "{a:?}");
        // Sideways axes see the top half: in between.
        assert!(a[0][0] > a[5][0] && a[0][0] < a[4][0]);
    }

    #[test]
    fn uniform_image_gives_uniform_ambient() {
        let a = ambient6(&Hdr {
            width: 4,
            height: 2,
            rgb: vec![0.5; 4 * 2 * 3],
        });
        for c in a {
            assert!((c[1] - 0.5).abs() < 1e-5);
        }
    }

    #[test]
    fn bundled_asset_decodes_2k_finite_non_negative() {
        let s = studio();
        assert_eq!((s.image.width, s.image.height), (2048, 1024));
        assert!(s.image.rgb.iter().all(|v| v.is_finite() && *v >= 0.0));
        assert!(s
            .ambient6
            .iter()
            .flatten()
            .all(|v| v.is_finite() && *v > 0.0));
    }

    #[test]
    fn studio_key_light_faces_the_default_sun() {
        let img = &studio().image;
        let lum = |i: usize| img.rgb[i * 3] + img.rgb[i * 3 + 1] + img.rgb[i * 3 + 2];
        let max = (0..img.width * img.height).map(lum).fold(0.0, f32::max);
        let mut sum = [0.0f32; 3];
        for i in 0..img.width * img.height {
            if lum(i) > 0.3 * max {
                let (x, y) = (i % img.width, i / img.width);
                let d = uv_to_dir(
                    (x as f32 + 0.5) / img.width as f32,
                    (y as f32 + 0.5) / img.height as f32,
                );
                for k in 0..3 {
                    sum[k] += d[k] * lum(i);
                }
            }
        }
        let sun = crate::scene::Scene::neutral().dir_light.direction;
        let (a, b) = (sum[1].atan2(sum[0]), sun[1].atan2(sun[0]));
        let diff = (a - b + PI).rem_euclid(TAU) - PI;
        assert!(diff.abs() < 0.35, "key azimuth {a} vs sun {b}");
        assert!(sum[2] > 0.0, "key light is above the horizon");
    }
}
