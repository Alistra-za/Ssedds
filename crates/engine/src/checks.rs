//! Artist check stats (v2 artist checks, checks 1, 2, 4 and 5): value bands, luma, the normal map's curl and the
//! share of steep neighbour steps. They ride inside the cached [`crate::analyse::Analysis`] (DECISIONS Note CI).
use crate::analyse::{code8, AnalyseHints};
use crate::image::Rgba32F;
use schemars::JsonSchema;
use serde::Serialize;

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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, JsonSchema)]
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
    fn add(&mut self, c: u8) {
        self.eq0 += u64::from(c == 0);
        self.le4 += u64::from(c <= 4);
        self.low += u64::from(c <= BAND_LOW);
        self.high += u64::from(c >= BAND_HIGH);
        self.ge251 += u64::from(c >= 251);
        self.eq255 += u64::from(c == 255);
    }
}

/// Mean squared curl of a normal map's slope field, read as stored and with green flipped. A real height field's
/// slopes have no curl, so the smaller one names the green convention.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, JsonSchema)]
pub struct Curl {
    pub as_stored: f64,
    pub flipped: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, JsonSchema)]
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
    pub(crate) fn new(two_channel: bool) -> Self {
        CheckStats {
            texels: 0,
            two_channel,
            bands: [BandCounts::default(); 4],
            luma_dark: 0,
            luma_bright: 0,
            luma_black: 0,
            curl: None,
            steep: None,
        }
    }
    /// Count one texel's 8-bit codes (R, G, B, A).
    pub(crate) fn add(&mut self, c: [u8; 4]) {
        self.texels += 1;
        for (b, &v) in self.bands.iter_mut().zip(&c) {
            b.add(v);
        }
        let l = luma_code(c[0], c[1], c[2]);
        self.luma_dark += u64::from(l < LUMA_DARK);
        self.luma_bright += u64::from(l > LUMA_BRIGHT);
        self.luma_black += u64::from(l <= LUMA_BLACK);
    }
}

/// Rec. 709 luma of 8-bit codes, rounded to a code.
pub fn luma_code(r: u8, g: u8, b: u8) -> u8 {
    (0.2126 * f64::from(r) + 0.7152 * f64::from(g) + 0.0722 * f64::from(b)).round() as u8
}

/// What to measure for a texture whose canonical suffix is `suffix`; nothing when `checks` is off (the CLI).
// shortcut: the hints follow the suffix the file had when first analysed, and the file cache is keyed by path and
// stamp, so retyping a file does not re-measure it; key the cache by hints too if a retyped file must show its stats.
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

/// Image-space slopes `(p, q)` (y down) of an encoded normal: x and y decoded from R and G, z rebuilt (B is ignored,
/// so BC5's zero blue reads the same) and clamped to at least 0.1.
fn slopes(px: [f32; 4]) -> (f64, f64) {
    let (x, y) = (f64::from(px[0]) * 2.0 - 1.0, f64::from(px[1]) * 2.0 - 1.0);
    let z = (1.0 - x * x - y * y).max(0.0).sqrt().max(0.1);
    (-x / z, -y / z)
}

/// Mean squared curl `∂q/∂x − ∂p/∂y` (forward differences, wrapping), as stored and with green flipped. Texels whose
/// differences are not finite are skipped; 0 when none remain.
pub fn curl(img: &Rgba32F) -> Curl {
    let (w, h) = (img.width, img.height);
    let (mut a, mut f, mut n) = (0.0f64, 0.0f64, 0u64);
    for y in 0..h {
        for x in 0..w {
            let (p0, q0) = slopes(img.at(x, y));
            let (_, q1) = slopes(img.at((x + 1) % w, y));
            let (p1, _) = slopes(img.at(x, (y + 1) % h));
            let (qx, py) = (q1 - q0, p1 - p0);
            let (s, t) = ((qx - py).powi(2), (-qx - py).powi(2));
            if s.is_finite() && t.is_finite() {
                (a, f, n) = (a + s, f + t, n + 1);
            }
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
    let (w, h) = (img.width, img.height);
    let c = |x, y| i32::from(code8(img.at(x, y)[channel]));
    let steep = |a: i32, b: i32| u64::from((a - b).abs() >= i32::from(STEEP_CODES));
    let (mut hits, mut pairs) = (0u64, 0u64);
    for y in 0..h {
        for x in 0..w {
            let v = c(x, y);
            if x + 1 < w {
                hits += steep(v, c(x + 1, y));
                pairs += 1;
            }
            if y + 1 < h {
                hits += steep(v, c(x, y + 1));
                pairs += 1;
            }
        }
    }
    if pairs == 0 {
        0.0
    } else {
        hits as f64 / pairs as f64
    }
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
}
