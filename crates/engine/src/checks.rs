//! Artist check stats (v2 artist checks, checks 1, 2, 4 and 5): value bands, luma, the normal map's curl and the
//! share of steep neighbour steps. They ride inside the cached [`crate::analyse::Analysis`] (DECISIONS Note CL).
//! Check 3 (fake resolution) is measured on the export chain instead: [`detail_ratio`], [`low_detail_eligible`].
use crate::analyse::{code8, AnalyseHints};
use crate::config::{Category, MipKeyword, MipLevels, Profile, ResizeMode, Subtype};
use crate::from_normal::texel_slope;
use crate::image::Rgba32F;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Codes at or below this are "low" (about 0.1).
pub const BAND_LOW: u8 = 25;
/// Codes at or above this are "high" (about 0.9).
pub const BAND_HIGH: u8 = 230;
/// Luma codes below this are dark.
pub const LUMA_DARK: u8 = 30;
/// Luma codes above this are bright.
pub const LUMA_BRIGHT: u8 = 240;
/// Luma codes at or below this are black.
pub const LUMA_BLACK: u8 = 8;
/// A neighbour step of this many codes or more is steep.
pub const STEEP_CODES: u8 = 32;

/// Texel counts of one channel by 8-bit code. `le4` / `ge251` bound the complex-material parallax window (parallax
/// only strictly between 4/255 and 251/255).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BandCounts {
    pub eq0: u64,
    pub le4: u64,
    /// Codes `<= BAND_LOW`.
    pub low: u64,
    /// Codes `>= BAND_HIGH`.
    pub high: u64,
    pub ge251: u64,
    pub eq255: u64,
}

impl BandCounts {
    /// Count `n` texels of code `c`.
    fn add(&mut self, c: u8, n: u64) {
        self.eq0 += n * u64::from(c == 0);
        self.le4 += n * u64::from(c <= 4);
        self.low += n * u64::from(c <= BAND_LOW);
        self.high += n * u64::from(c >= BAND_HIGH);
        self.ge251 += n * u64::from(c >= 251);
        self.eq255 += n * u64::from(c == 255);
    }
}

/// Per-texel code histograms for [`CheckStats`]: R, G, B, A and luma. Five increments per texel instead of 27
/// counters (which spilled out of registers on every texel, DECISIONS Note CL42); folded once by
/// [`CheckStats::from_hist`].
pub(crate) type CodeHist = [[u64; 256]; 5];

/// Mean squared curl of a normal map's slope field, read as stored and with green flipped. A real height field's
/// slopes have no curl, so the smaller one names the green convention.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Curl {
    pub as_stored: f64,
    pub flipped: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CheckStats {
    pub texels: u64,
    /// The source had two channels (BC5 / RG8 DDS; also a grey+alpha PNG).
    pub two_channel: bool,
    /// R, G, B, A.
    pub bands: [BandCounts; 4],
    /// Luma codes `< LUMA_DARK`.
    pub luma_dark: u64,
    /// Luma codes `> LUMA_BRIGHT`.
    pub luma_bright: u64,
    /// Luma codes `<= LUMA_BLACK`.
    pub luma_black: u64,
    pub curl: Option<Curl>,
    /// Share of neighbour pairs whose step is at least [`STEEP_CODES`], in the hinted channel.
    pub steep: Option<f64>,
}

impl CheckStats {
    /// Count one texel's 8-bit codes (R, G, B, A) into `h`.
    #[inline]
    pub(crate) fn hist_add(h: &mut CodeHist, c: [u8; 4]) {
        for (b, &v) in h.iter_mut().zip(&c) {
            b[usize::from(v)] += 1;
        }
        h[4][usize::from(luma_code(c[0], c[1], c[2]))] += 1;
    }
    /// The stats of the texels counted into `h` (curl and steep unset).
    pub(crate) fn from_hist(two_channel: bool, h: &CodeHist) -> Self {
        let mut s = CheckStats {
            texels: h[0].iter().sum(),
            two_channel,
            bands: [BandCounts::default(); 4],
            luma_dark: 0,
            luma_bright: 0,
            luma_black: 0,
            curl: None,
            steep: None,
        };
        for c in 0..=255u8 {
            for (b, hc) in s.bands.iter_mut().zip(h) {
                b.add(c, hc[usize::from(c)]);
            }
            let n = h[4][usize::from(c)];
            s.luma_dark += n * u64::from(c < LUMA_DARK);
            s.luma_bright += n * u64::from(c > LUMA_BRIGHT);
            s.luma_black += n * u64::from(c <= LUMA_BLACK);
        }
        s
    }
}

/// Rec. 709 luma of 8-bit codes, rounded to a code.
pub fn luma_code(r: u8, g: u8, b: u8) -> u8 {
    let v = 0.2126 * f64::from(r) + 0.7152 * f64::from(g) + 0.0722 * f64::from(b);
    // Same as `v.round()` for v >= 0 (the fraction is exact), without the libm call it is on baseline x86-64; it ran
    // on every texel and was most of the check stats' load-time cost (DECISIONS Note CL42).
    let t = v as u32;
    (t + u32::from(v - f64::from(t) >= 0.5)) as u8
}

/// What to measure for a texture whose canonical suffix is `suffix`; nothing when `checks` is off (the CLI).
// The file cache is keyed by path, stamp and these hints (Note DZ), so a different suffix measures again.
pub fn hints_for(suffix: Option<&str>, checks: bool) -> AnalyseHints {
    if !checks {
        return AnalyseHints::default();
    }
    AnalyseHints {
        bands: true,
        curl: suffix == Some("n"),
        steps: match suffix {
            Some("p") => Some(0),
            Some("m") => Some(3),
            _ => None,
        },
    }
}

/// `hints_for` for a sub-type a `[files]` entry or `--set` gave a file (Note FC13, CL1/CL2): the suffix that
/// sub-type would carry. A sub-type with no checked suffix measures the bands only.
pub fn hints_for_subtype(subtype: Subtype, checks: bool) -> AnalyseHints {
    let suffix = match subtype {
        Subtype::Normal | Subtype::LandscapeNormal | Subtype::PbrNormal => Some("n"),
        Subtype::Height => Some("p"),
        Subtype::ComplexMaterial | Subtype::Envmask => Some("m"),
        _ => None,
    };
    hints_for(suffix, checks)
}

/// Mean squared curl `∂q/∂x − ∂p/∂y` (central differences over interior texels only, so a map that does not tile
/// gets no seam term), as stored and with green flipped. Texels whose differences are not finite are skipped; 0 when
/// none remain (also for images under 3x3).
pub fn curl(img: &Rgba32F) -> Curl {
    let (w, h) = (img.width as usize, img.height as usize);
    let (mut a, mut f, mut n) = (0.0f64, 0.0f64, 0u64);
    if w >= 3 && h >= 3 {
        let row = |y: usize| -> Vec<(f64, f64)> {
            img.px[y * w..(y + 1) * w]
                .iter()
                .map(|&p| texel_slope(p))
                .collect()
        };
        // Rolling rows y - 1, y, y + 1: each texel's slopes are decoded once.
        let (mut up, mut mid) = (row(0), row(1));
        for y in 1..h - 1 {
            let down = row(y + 1);
            for x in 1..w - 1 {
                let qx = mid[x + 1].1 - mid[x - 1].1;
                let py = down[x].0 - up[x].0;
                let (s, t) = ((qx - py).powi(2), (-qx - py).powi(2));
                if s.is_finite() && t.is_finite() {
                    (a, f, n) = (a + s, f + t, n + 1);
                }
            }
            (up, mid) = (mid, down);
        }
    }
    let n = n.max(1) as f64;
    Curl {
        as_stored: a / n,
        flipped: f / n,
    }
}

/// Share of horizontal and vertical neighbour pairs in `channel` whose 8-bit codes differ by at least
/// [`STEEP_CODES`]; 0 when there are no pairs.
pub fn steep_share(img: &Rgba32F, channel: usize) -> f64 {
    let (w, h) = (img.width as usize, img.height as usize);
    let steep = |a: u8, b: u8| u64::from(a.abs_diff(b) >= STEEP_CODES);
    // Each row's codes are taken once and kept for the next row's vertical pairs.
    let codes = |y: usize, out: &mut Vec<u8>| {
        out.clear();
        out.extend(img.px[y * w..(y + 1) * w].iter().map(|p| code8(p[channel])));
    };
    let (mut hits, mut pairs) = (0u64, 0u64);
    let (mut cur, mut next) = (Vec::with_capacity(w), Vec::with_capacity(w));
    if h > 0 {
        codes(0, &mut cur);
    }
    for y in 0..h {
        hits += cur.windows(2).map(|p| steep(p[0], p[1])).sum::<u64>();
        pairs += w.saturating_sub(1) as u64;
        if y + 1 < h {
            codes(y + 1, &mut next);
            hits += cur
                .iter()
                .zip(&next)
                .map(|(&a, &b)| steep(a, b))
                .sum::<u64>();
            pairs += w as u64;
            std::mem::swap(&mut cur, &mut next);
        }
    }
    if pairs == 0 {
        0.0
    } else {
        hits as f64 / pairs as f64
    }
}

/// Check 3 (fake resolution): a detail ratio below this flags the texture (DECISIONS Note CL20).
pub const LOW_DETAIL_RATIO: f64 = 0.25;

/// Whether a detail ratio flags the texture: strictly below [`LOW_DETAIL_RATIO`].
pub fn is_low_detail(ratio: f64) -> bool {
    ratio < LOW_DETAIL_RATIO
}

/// Mean squared difference over finite RGB samples between `hi` and `lo` sampled nearest at `hi`'s size.
fn up_error(hi: &Rgba32F, lo: &Rgba32F) -> f64 {
    let (mut sum, mut n) = (0.0f64, 0u64);
    for y in 0..hi.height {
        // Clamped: an odd level's half rounds down, so its last row and column map to the last texel.
        let ly = (y * lo.height / hi.height).min(lo.height - 1);
        for x in 0..hi.width {
            let lx = (x * lo.width / hi.width).min(lo.width - 1);
            let (a, b) = (hi.at(x, y), lo.at(lx, ly));
            for c in 0..3 {
                let d = f64::from(a[c] - b[c]);
                if d.is_finite() {
                    sum += d * d;
                    n += 1;
                }
            }
        }
    }
    if n == 0 {
        0.0
    } else {
        sum / n as f64
    }
}

/// Check 3: the detail mip 0 adds over mip 1 against what mip 1 adds over mip 2 (E01 / E12). A native texture
/// scores near or above 1, a 2x upscale near 0. `None` under 3 levels or when mip 1 adds nothing (E12 < 1e-6).
pub fn detail_ratio(chain: &[Rgba32F]) -> Option<f64> {
    let [m0, m1, m2, ..] = chain else {
        return None;
    };
    let e12 = up_error(m1, m2);
    (e12 >= 1e-6).then(|| up_error(m0, m1) / e12)
}

/// Whether check 3 measures a texture of `dims` under `p`: colour or normal (never complex grass), no resize or
/// dropped top level already, at least 3 mips, and above the half floor (the T4 `HALF_FLOOR`), so a texture that
/// could not be halved is never flagged.
pub fn low_detail_eligible(p: &Profile, dims: (u32, u32)) -> bool {
    low_detail_profile_eligible(p)
        && crate::resize::target_dims(dims, ResizeMode::Half) != dims
        && crate::mips::level_count(dims.0, dims.1, &p.mips.levels) >= 3
}

/// The half of [`low_detail_eligible`] known before Load: everything but the size.
pub fn low_detail_profile_eligible(p: &Profile) -> bool {
    p.resize == ResizeMode::None
        && !p.drop_top_mip
        && matches!(p.category, Category::Color | Category::Normal)
        && !matches!(p.subtype, Subtype::ComplexGrass | Subtype::Cubemap)
        && !matches!(
            p.mips.levels,
            MipLevels::Keyword(MipKeyword::None) | MipLevels::Count(0..=2)
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyse::AnalyseHints;
    use crate::image::Rgba32F;

    /// Height in RGB: Gaussian blobs at diagonal offsets, so the mixed derivative is not zero.
    fn blobs(n: u32) -> Rgba32F {
        let mut img = Rgba32F::new(n, n, [0.0, 0.0, 0.0, 1.0]);
        let s = n as f32 / 12.0;
        let centres = [(0.3, 0.35), (0.65, 0.6), (0.4, 0.7)];
        for y in 0..n {
            for x in 0..n {
                let h: f32 = centres
                    .iter()
                    .map(|&(cx, cy)| {
                        let (dx, dy) = (x as f32 - cx * n as f32, y as f32 - cy * n as f32);
                        (-(dx * dx + dy * dy) / (2.0 * s * s)).exp()
                    })
                    .sum();
                *img.at_mut(x, y) = [h, h, h, 1.0];
            }
        }
        img
    }

    fn encode01(n: &Rgba32F) -> Rgba32F {
        let mut out = n.clone();
        for p in &mut out.px {
            for v in &mut p[..3] {
                *v = *v * 0.5 + 0.5;
            }
        }
        out
    }

    fn flip_g(img: &Rgba32F) -> Rgba32F {
        let mut out = img.clone();
        for p in &mut out.px {
            p[1] = 1.0 - p[1];
        }
        out
    }

    fn zero_b(img: &Rgba32F) -> Rgba32F {
        let mut out = img.clone();
        for p in &mut out.px {
            p[2] = 0.0;
        }
        out
    }

    /// A one-row image whose R codes are `codes`.
    fn row(codes: &[u8]) -> Rgba32F {
        let mut img = Rgba32F::new(codes.len() as u32, 1, [0.0, 0.0, 0.0, 1.0]);
        for (p, &c) in img.px.iter_mut().zip(codes) {
            p[0] = f32::from(c) / 255.0;
        }
        img
    }

    #[test]
    fn a_dx_bump_map_has_low_curl_as_stored_and_high_flipped() {
        // Gaussian blobs at diagonal offsets: h_xy != 0, so the wrong sign shows.
        let h = blobs(128);
        let n = crate::rnm::blend::bump_normals(&h, 4.0); // DirectX vectors
        let enc = encode01(&n);
        let c = curl(&enc);
        assert!(c.as_stored < 0.1 * c.flipped, "{c:?}");
        let gl = flip_g(&enc);
        let c2 = curl(&gl);
        assert!(c2.flipped < 0.1 * c2.as_stored, "{c2:?}");
    }

    #[test]
    fn a_bc5_style_rg_normal_gives_the_same_curl_as_its_rgb_form() {
        // The same encoded normal with B zeroed (as decode.rs:96-104 gives BC5): z is rebuilt, so the curl matches.
        let enc = encode01(&crate::rnm::blend::bump_normals(&blobs(128), 4.0));
        let rg = zero_b(&enc);
        let (a, b) = (curl(&enc), curl(&rg));
        assert!(
            (a.as_stored - b.as_stored).abs() <= 1e-3 * a.flipped && b.flipped.is_finite(),
            "{a:?} {b:?}"
        );
    }

    /// Review I1: a map that does not tile must not get a seam term. Saddle `h = (x/w)(y/h)`: the far edges jump.
    #[test]
    fn a_non_tiling_saddle_has_low_curl_as_stored() {
        let mut h = Rgba32F::new(64, 64, [0.0; 4]);
        for y in 0..64 {
            for x in 0..64 {
                let v = (x as f32 / 64.0) * (y as f32 / 64.0);
                *h.at_mut(x, y) = [v, v, v, 1.0];
            }
        }
        let c = curl(&encode01(&crate::rnm::blend::bump_normals(&h, 4.0)));
        assert!(c.as_stored < 0.1 * c.flipped, "{c:?}");
    }

    /// Review I2: pixel-scale detail (white-noise height) still names the convention. Central differences commute
    /// with `bump_normals`' central stencil, so only float rounding is left as stored: the bound is 1e-6 of flipped
    /// (forward differences gave about 0.6).
    #[test]
    fn a_white_noise_bump_map_has_near_zero_curl_as_stored() {
        let mut h = Rgba32F::new(128, 128, [0.0; 4]);
        let mut s = 0x2545_f491_u32;
        for p in &mut h.px {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let v = (s >> 8) as f32 / (1u32 << 24) as f32;
            *p = [v, v, v, 1.0];
        }
        let c = curl(&encode01(&crate::rnm::blend::bump_normals(&h, 1.0)));
        assert!(c.flipped > 0.0 && c.as_stored < 1e-6 * c.flipped, "{c:?}");
        let c2 = curl(&flip_g(&encode01(&crate::rnm::blend::bump_normals(
            &h, 1.0,
        ))));
        assert!(c2.flipped < 1e-6 * c2.as_stored, "{c2:?}");
    }

    #[test]
    fn a_flat_normal_has_zero_curl_both_ways() {
        let c = curl(&Rgba32F::new(16, 16, [0.5, 0.5, 1.0, 1.0]));
        assert_eq!((c.as_stored, c.flipped), (0.0, 0.0));
    }

    #[test]
    fn curl_skips_non_finite_texels_and_is_zero_when_none_remain() {
        let mut img = Rgba32F::new(4, 4, [0.5, 0.5, 1.0, 1.0]);
        img.px[5] = [f32::NAN, 0.5, 1.0, 1.0];
        let c = curl(&img);
        assert_eq!((c.as_stored, c.flipped), (0.0, 0.0));
        let c = curl(&Rgba32F::new(2, 2, [f32::NAN; 4]));
        assert_eq!((c.as_stored, c.flipped), (0.0, 0.0));
        let c = curl(&Rgba32F::new(0, 0, [0.0; 4]));
        assert_eq!((c.as_stored, c.flipped), (0.0, 0.0));
    }

    #[test]
    fn steep_share_counts_neighbour_steps_of_32_codes_or_more() {
        // 4x1 R codes 0, 31, 63, 63: steps 31 (no), 32 (yes), 0 (no) => 1 of 3 horizontal pairs.
        let img = row(&[0, 31, 63, 63]);
        assert!((steep_share(&img, 0) - 1.0 / 3.0).abs() < 1e-12);
        assert_eq!(steep_share(&Rgba32F::new(1, 1, [0.0; 4]), 0), 0.0); // no pairs
                                                                        // 2x2: vertical pairs count too, and the channel is the one asked for.
        let mut sq = Rgba32F::new(2, 2, [0.0; 4]);
        sq.px[2][3] = 1.0; // (0, 1) alpha
        assert!((steep_share(&sq, 3) - 2.0 / 4.0).abs() < 1e-12);
        assert_eq!(steep_share(&sq, 0), 0.0);
    }

    #[test]
    fn luma_code_uses_rec_709_weights_rounded() {
        assert_eq!(luma_code(255, 255, 255), 255);
        assert_eq!(luma_code(0, 0, 0), 0);
        assert_eq!(luma_code(255, 0, 0), 54); // 0.2126 * 255 = 54.2
        assert_eq!(luma_code(0, 255, 0), 182); // 182.4
        assert_eq!(luma_code(0, 0, 255), 18); // 18.4

        // Exactly `round()` of the Rec. 709 sum for every code triple (CL42 replaced the libm call).
        for r in 0..=255u8 {
            for g in 0..=255u8 {
                for b in 0..=255u8 {
                    let v = 0.2126 * f64::from(r) + 0.7152 * f64::from(g) + 0.0722 * f64::from(b);
                    assert_eq!(luma_code(r, g, b), v.round() as u8, "{r} {g} {b}");
                }
            }
        }
    }

    #[test]
    fn hints_follow_the_canonical_suffix_and_the_cache_flag() {
        assert!(hints_for(Some("n"), true).curl && hints_for(Some("n"), true).bands);
        assert_eq!(hints_for(Some("p"), true).steps, Some(0));
        assert_eq!(hints_for(Some("m"), true).steps, Some(3));
        assert!(!hints_for(Some("msn"), true).curl);
        assert_eq!(hints_for(Some("n"), false), AnalyseHints::default());
        // Bands are on for every suffix, and for none.
        assert!(hints_for(Some("rmaos"), true).bands && hints_for(None, true).bands);
    }

    /// `n`² white noise from a fixed LCG.
    fn noise(n: u32, seed: u64) -> Rgba32F {
        let mut s = seed;
        let mut next = || {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (s >> 40) as f32 / (1u64 << 24) as f32
        };
        let mut img = Rgba32F::new(n, n, [0.0, 0.0, 0.0, 1.0]);
        for p in &mut img.px {
            *p = [next(), next(), next(), 1.0];
        }
        img
    }

    /// 2x upscale, nearest or bilinear (texel centres).
    fn up2(img: &Rgba32F, bilinear: bool) -> Rgba32F {
        let (w, h) = (img.width, img.height);
        let mut out = Rgba32F::new(w * 2, h * 2, [0.0; 4]);
        for y in 0..h * 2 {
            for x in 0..w * 2 {
                *out.at_mut(x, y) = if bilinear {
                    let fx = ((x as f32 + 0.5) / 2.0 - 0.5).clamp(0.0, (w - 1) as f32);
                    let fy = ((y as f32 + 0.5) / 2.0 - 0.5).clamp(0.0, (h - 1) as f32);
                    let (x0, y0) = (fx.floor() as u32, fy.floor() as u32);
                    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
                    let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
                    let (a, b) = (img.at(x0, y0), img.at(x1, y0));
                    let (c, d) = (img.at(x0, y1), img.at(x1, y1));
                    std::array::from_fn(|i| {
                        (a[i] * (1.0 - tx) + b[i] * tx) * (1.0 - ty)
                            + (c[i] * (1.0 - tx) + d[i] * tx) * ty
                    })
                } else {
                    img.at(x / 2, y / 2)
                };
            }
        }
        out
    }

    /// A box chain of `levels` levels.
    fn chain(img: Rgba32F, levels: usize) -> Vec<Rgba32F> {
        let mut c = vec![img];
        while c.len() < levels {
            let l = c.last().unwrap();
            let to = ((l.width / 2).max(1), (l.height / 2).max(1));
            c.push(crate::mips::area_resample(l, to, false));
        }
        c
    }

    #[test]
    fn native_noise_has_a_high_ratio_and_a_2x_upscale_a_low_one() {
        let r = detail_ratio(&chain(noise(256, 1), 3)).unwrap();
        assert!(r > 0.5, "{r}");
        let small = noise(128, 2);
        let r = detail_ratio(&chain(up2(&small, false), 3)).unwrap();
        assert!(r < 0.01, "{r}");
        // Bilinear: white noise scores about 0.19, noise softened with a 2x2 box about 0.24 (CL20).
        let r = detail_ratio(&chain(up2(&small, true), 3)).unwrap();
        assert!(r < LOW_DETAIL_RATIO, "{r}");
        let mut soft = small.clone();
        for y in 0..127 {
            for x in 0..127 {
                let (a, b, c, d) = (
                    small.at(x, y),
                    small.at(x + 1, y),
                    small.at(x, y + 1),
                    small.at(x + 1, y + 1),
                );
                *soft.at_mut(x, y) = std::array::from_fn(|i| (a[i] + b[i] + c[i] + d[i]) / 4.0);
            }
        }
        let r = detail_ratio(&chain(up2(&soft, true), 3)).unwrap();
        assert!(r < LOW_DETAIL_RATIO, "{r}");
        // NPOT with odd mips (1001x9 → 500x4 → 250x2) stays in bounds.
        let mut odd = Rgba32F::new(1001, 9, [0.0, 0.0, 0.0, 1.0]);
        odd.px = noise(1001, 3).px[..1001 * 9].to_vec();
        assert!(detail_ratio(&chain(odd, 3)).is_some());
    }

    /// Above the threshold: a bilinear upscale with a little native noise on top, a soft texture with some detail of
    /// its own. Pins that the threshold is not raised past it.
    #[test]
    fn a_soft_texture_with_some_native_detail_is_not_flagged() {
        let up = up2(&noise(128, 2), true);
        let fine = noise(256, 7);
        let mut mix = up.clone();
        for (m, f) in mix.px.iter_mut().zip(&fine.px) {
            for c in 0..3 {
                m[c] = 0.85 * m[c] + 0.15 * f[c];
            }
        }
        let r = detail_ratio(&chain(mix, 3)).unwrap();
        assert!((LOW_DETAIL_RATIO..0.6).contains(&r), "{r}");
    }

    /// The comparison is strict: exactly the threshold is not low detail.
    #[test]
    fn low_detail_is_strictly_below_the_threshold() {
        assert!(!is_low_detail(LOW_DETAIL_RATIO));
        assert!(is_low_detail(LOW_DETAIL_RATIO - 1e-9));
        assert!(!is_low_detail(LOW_DETAIL_RATIO + 1e-9));
    }

    #[test]
    fn a_flat_image_has_no_ratio() {
        let flat = Rgba32F::new(256, 256, [0.3, 0.4, 0.5, 1.0]);
        assert_eq!(detail_ratio(&chain(flat, 3)), None);
        assert_eq!(detail_ratio(&chain(noise(256, 4), 2)), None);
    }

    #[test]
    fn low_detail_is_not_measured_at_the_half_floor() {
        use crate::config::{MipKeyword, MipLevels, ResizeMode, Subtype, Target};
        let reg = crate::presets::PresetRegistry::builtin().unwrap();
        let prof = |st| reg.profile("cs-quality", Target::Cs, st).unwrap();
        let d = prof(Subtype::Diffuse);
        assert!(!low_detail_eligible(&d, (512, 512)));
        assert!(!low_detail_eligible(&d, (512, 256)));
        assert!(low_detail_eligible(&d, (1024, 256)));
        assert!(low_detail_eligible(&d, (512, 1024)));
        assert!(low_detail_eligible(&d, (1000, 600)));
        assert!(low_detail_eligible(&prof(Subtype::Normal), (1024, 1024)));
        let mut half = d.clone();
        half.resize = ResizeMode::Half;
        assert!(!low_detail_eligible(&half, (1024, 1024)));
        let mut dropped = d.clone();
        dropped.drop_top_mip = true;
        assert!(!low_detail_eligible(&dropped, (1024, 1024)));
        let mut none = d.clone();
        none.mips.levels = MipLevels::Keyword(MipKeyword::None);
        assert!(!low_detail_eligible(&none, (1024, 1024)));
        let mut two = d.clone();
        two.mips.levels = MipLevels::Count(2);
        assert!(!low_detail_eligible(&two, (1024, 1024)));
        assert!(!low_detail_eligible(
            &prof(Subtype::ComplexGrass),
            (1024, 2048)
        ));
        assert!(!low_detail_eligible(&prof(Subtype::Rmaos), (1024, 1024)));
    }
}
