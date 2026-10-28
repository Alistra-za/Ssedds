//! Radiance RGBE (`.hdr`) reader and the bundled studio HDRI (Poly Haven `studio_small_09`, 2K, CC0; see
//! `assets/hdri/README.md`). The workspace `image` crate has no `hdr` feature, so the format is read here:
//! header lines up to the blank line, the `-Y h +X w` resolution line, then new-style RLE scanlines (or flat
//! RGBE pixels).
//!
//! **Mapping (Z-up world, like the renderer):** the image is equirectangular, row 0 = +Z (straight up), the
//! last row = −Z; the column is the azimuth `atan2(y, x)` turned by [`YAW`], growing clockwise seen from above like a
//! 360° photo (Note DL). [`dir_to_uv`] and [`uv_to_dir`]
//! are the one definition: the background shader, the CPU ambient and the tests all go through it.
use std::f32::consts::{PI, TAU};
use std::sync::OnceLock;

/// The bundled HDRI file (6.3 MB).
static STUDIO_HDR: &[u8] = include_bytes!("../assets/hdri/studio_small_09_2k.hdr");

/// Azimuth (radians) added to the world azimuth before the column lookup. Chosen so the image's brightest
/// region (its key light) lies at the azimuth of the default directional light (`Scene::neutral`), upper front
/// left of the default camera. See `studio_key_light_faces_the_default_sun`.
/// Note DL: 0.774 after the clockwise flip (was −2.798), the same key-light azimuth.
pub const YAW: f32 = 0.774;

/// The HDRI is a photographic exposure (mean luminance 0.85, ambient means 0.65-1.3), several times the
/// neutral preview's calibrated ambient (0.12-0.30), and on top of the sun it would white out every material.
/// The ambient taken from it is scaled by this; the background is shown as is.
pub const AMBIENT_SCALE: f32 = 0.3;

/// World direction (unit, Z-up) -> image coordinates `(u, v)` in 0..1 (`v` = 0 at +Z), turned by [`YAW`].
pub fn dir_to_uv(d: [f32; 3]) -> (f32, f32) {
    engine::cube::latlong::dir_to_uv(d, YAW)
}

/// Inverse of [`dir_to_uv`].
pub fn uv_to_dir(u: f32, v: f32) -> [f32; 3] {
    engine::cube::latlong::uv_to_dir(u, v, YAW)
}

pub use engine::cube::rgbe::{decode_rgbe, Hdr};

/// One cell of the ≤ 64×32 solid-angle box-downsample: world direction of its centre, solid-angle weight, mean
/// radiance.
pub type Cell = ([f32; 3], f64, [f64; 3]);

const AXES: [[f32; 3]; 6] = [
    [1.0, 0.0, 0.0],
    [-1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, -1.0, 0.0],
    [0.0, 0.0, 1.0],
    [0.0, 0.0, -1.0],
];

/// The image box-downsampled to at most 64×32 cells, solid-angle weighted (so the poles do not dominate).
pub fn cells(img: &Hdr) -> Vec<Cell> {
    let (ow, oh) = (img.width.min(64), img.height.min(32));
    let mut out = Vec::with_capacity(ow * oh);
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
            out.push((
                uv_to_dir((ox as f32 + 0.5) / ow as f32, v),
                solid,
                c.map(|s| s / n),
            ));
        }
    }
    out
}

/// Ambient colours for the six DALC axes (+X −X +Y −Y +Z −Z, world space) of the environment turned by `yaw` about
/// +Z: for each axis the cosine-weighted mean radiance over the hemisphere around it. A turned environment seen from
/// axis `a` is the original seen from `a` turned by −yaw.
pub fn integrate(cells: &[Cell], yaw: f32) -> [[f32; 3]; 6] {
    let (s, c) = (-yaw).sin_cos();
    let axes = AXES.map(|a| [a[0] * c - a[1] * s, a[0] * s + a[1] * c, a[2]]);
    let mut sum = [[0.0f64; 3]; 6];
    let mut wsum = [0.0f64; 6];
    for (d, solid, mean) in cells {
        for (a, axis) in axes.iter().enumerate() {
            let cos = (d[0] * axis[0] + d[1] * axis[1] + d[2] * axis[2]).max(0.0) as f64;
            let w = cos * solid;
            wsum[a] += w;
            for k in 0..3 {
                sum[a][k] += w * mean[k];
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

/// [`integrate`] of the unturned image (the ambient the preview always had).
pub fn ambient6(img: &Hdr) -> [[f32; 3]; 6] {
    integrate(&cells(img), 0.0)
}

/// Side of the environment cube (v2 D5): the captured Dynamic Cubemaps cube is 256², 9 mips.
pub const ENV_SIZE: u32 = 256;

/// CS's GetSamplingVector, see [`engine::cube::cube_dir`] (moved to the engine, Note DS1).
pub use engine::cube::cube_dir;

/// Per texel of a `size` cube (face-major): the yaw-free part of [`dir_to_uv`] of its [`cube_dir`], the azimuth term
/// `atan2(y, x) + YAW` and `v`.
fn cube_angles(size: u32) -> Vec<(f32, f32)> {
    (0..6)
        .flat_map(|f| {
            (0..size * size).map(move |i| {
                let d = cube_dir(f, i % size, i / size, size);
                (d[1].atan2(d[0]) + YAW, d[2].clamp(-1.0, 1.0).acos() / PI)
            })
        })
        .collect()
}

/// Mip 0 of the env cube turned by `yaw` (as the backdrop and [`integrate`]): per texel, the equirect pixel holding
/// `dir_to_uv(Rz(-yaw)·cube_dir)`, alpha 1. Face-major, rows top first. A turn about +Z only shifts the azimuth, so
/// `u = (azimuth − yaw)` from a per-size angle table (built once for [`ENV_SIZE`]): a yaw drag costs no atan2/acos.
pub fn cube_faces(img: &Hdr, size: u32, yaw: f32) -> [Vec<[f32; 4]>; 6] {
    static ENV: OnceLock<Vec<(f32, f32)>> = OnceLock::new();
    let owned;
    let table: &[(f32, f32)] = if size == ENV_SIZE {
        ENV.get_or_init(|| cube_angles(ENV_SIZE))
    } else {
        owned = cube_angles(size);
        &owned
    };
    let (w, h) = (img.width, img.height);
    let n = (size * size) as usize;
    std::array::from_fn(|f| {
        table[f * n..(f + 1) * n]
            .iter()
            .map(|&(a, v)| {
                let u = (0.5 - (a - yaw) / TAU).rem_euclid(1.0);
                let px = ((u * w as f32) as usize).min(w - 1);
                let py = ((v * h as f32) as usize).min(h - 1);
                let i = (py * w + px) * 3;
                [img.rgb[i], img.rgb[i + 1], img.rgb[i + 2], 1.0]
            })
            .collect()
    })
}

/// 2×2 box mips down to 1×1, mip 0 included. Err unless `size` is a power of two and `face` is `size²` texels.
/// shortcut: a box chain, not CS's GGX-prefiltered specular irradiance; prefilter if the owner's visual check asks (R4).
pub fn mip_chain(face: &[[f32; 4]], size: u32) -> Result<Vec<Vec<[f32; 4]>>, String> {
    let n = size as usize;
    if !size.is_power_of_two() || n.checked_mul(n) != Some(face.len()) {
        return Err(format!(
            "mip chain: {} texels for a {size}² face (the size must be a power of two)",
            face.len()
        ));
    }
    let mut out = vec![face.to_vec()];
    let mut n = n;
    while n > 1 {
        let (prev, h) = (out.last().expect("mip 0"), n / 2);
        let next = (0..h * h)
            .map(|i| {
                let (x, y) = (i % h * 2, i / h * 2);
                let (a, b, c, d) = (
                    prev[y * n + x],
                    prev[y * n + x + 1],
                    prev[(y + 1) * n + x],
                    prev[(y + 1) * n + x + 1],
                );
                std::array::from_fn(|k| (a[k] + b[k] + c[k] + d[k]) * 0.25)
            })
            .collect();
        out.push(next);
        n = h;
    }
    Ok(out)
}

/// The bundled studio HDRI with its ambient, decoded on first use.
pub struct Studio {
    pub image: Hdr,
    /// = `ambient6_at(0.0)`.
    pub ambient6: [[f32; 3]; 6],
    cells: Vec<Cell>,
}

impl Studio {
    /// The ambient of the environment turned by `yaw` (radians about +Z), [`AMBIENT_SCALE`] applied. About 12k
    /// multiply-adds per call; `Scene::ambient` calls it at most twice a frame.
    /// shortcut: no memo; add a `(yaw bits, result)` memo if a profile shows it.
    pub fn ambient6_at(&self, yaw: f32) -> [[f32; 3]; 6] {
        integrate(&self.cells, yaw).map(|c| c.map(|v| v * AMBIENT_SCALE))
    }
}

/// Decodes the bundled HDRI once (about 0.1 s) and caches it for the process.
pub fn studio() -> &'static Studio {
    static STUDIO: OnceLock<Studio> = OnceLock::new();
    STUDIO.get_or_init(|| {
        let image = decode_rgbe(STUDIO_HDR).expect("the bundled HDRI decodes");
        let cells = cells(&image);
        let ambient6 = integrate(&cells, 0.0).map(|c| c.map(|v| v * AMBIENT_SCALE));
        Studio {
            image,
            ambient6,
            cells,
        }
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
    fn oversized_truncated_and_malformed_inputs_are_errors() {
        assert!(decode_rgbe(&[header(268_435_456, 1), vec![0; 16]].concat()).is_err());
        assert!(decode_rgbe(&[header(1, 268_435_456), vec![0; 16]].concat()).is_err());
        assert!(decode_rgbe(&[header(16384, 16384), vec![0; 16]].concat()).is_err());
        // Declared 4096x4096 flat, a few bytes of payload: rejected before allocating.
        assert!(decode_rgbe(&[header(4096, 4096), vec![1; 64]].concat()).is_err());
        // RLE header claiming 16384x4096 with 49152 bytes: too short for its runs, rejected before allocating.
        let mut b = header(16384, 4096);
        b.extend([2, 2, 0x40, 0]);
        b.resize(b.len() + 49_152 - 4, 0);
        assert!(decode_rgbe(&b).is_err());
        // RLE: truncated mid-scanline, and a zero-length run.
        let mut b = header(8, 1);
        b.extend([2, 2, 0, 8, 128 + 8, 1, 128 + 8, 1, 128 + 8, 1, 128 + 4, 1]);
        assert!(decode_rgbe(&b).is_err());
        let mut b = header(8, 1);
        b.extend([2, 2, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert!(decode_rgbe(&b).is_err());
        // A run longer than the scanline.
        let mut b = header(8, 1);
        b.extend([2, 2, 0, 8, 128 + 100, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert!(decode_rgbe(&b).is_err());
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
    fn moving_right_in_the_image_turns_clockwise() {
        // Note DL: a 360° photo runs clockwise seen from above (in game, cubemap-up-axis "Second check").
        let az = |u| {
            let d = uv_to_dir(u, 0.5);
            d[1].atan2(d[0])
        };
        let turn = (az(0.75) - az(0.5) + PI).rem_euclid(TAU) - PI;
        assert!((turn + PI / 2.0).abs() < 1e-4, "{turn}");
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

    #[test]
    fn yaw_zero_is_todays_ambient_and_a_half_turn_swaps_the_sides() {
        let s = studio();
        assert_eq!(s.ambient6_at(0.0), s.ambient6, "bit-equal at yaw 0");
        assert_eq!(integrate(&cells(&s.image), 0.0), ambient6(&s.image));
        let (a, b) = (s.ambient6_at(0.0), s.ambient6_at(PI));
        // +X −X +Y −Y +Z −Z: the half turn swaps ±X and ±Y and keeps ±Z.
        for (got, from) in [(0, 1), (1, 0), (2, 3), (3, 2), (4, 4), (5, 5)] {
            for k in 0..3 {
                assert!(
                    (b[got][k] - a[from][k]).abs() < 1e-4,
                    "axis {got}: {b:?} vs {a:?}"
                );
            }
        }
    }

    /// The sign test: the environment turns by +yaw about Z, as `Look::rotate_sun` turns the sun, so what lit +X at
    /// yaw 0 lights +Y at a quarter turn (and the backdrop shader samples `azimuth - yaw`, tests/background.rs).
    #[test]
    fn a_quarter_turn_moves_the_ambient_the_way_the_sun_turns() {
        let s = studio();
        let (a, b) = (s.ambient6_at(0.0), s.ambient6_at(PI / 2.0));
        for (got, from) in [(2, 0), (1, 2), (3, 1), (0, 3), (4, 4)] {
            for k in 0..3 {
                assert!((b[got][k] - a[from][k]).abs() < 1e-4, "axis {got}");
            }
        }
    }
}
