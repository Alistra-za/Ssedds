//! Analyse (spec §5.1): statistics used by suggestions, warnings and validation.
//!
//! One pass over the float image, no allocation beyond the result. Every
//! "code" comparison is made on the nearest 8-bit code ([`code8`]) so the
//! thresholds match what an 8-bit viewer of the same data would see.
use crate::image::{Rgba32F, SourceInfo};
use crate::presets::AlphaContent;
use schemars::JsonSchema;
use serde::Serialize;

/// Minimum, maximum and mean of one channel, in the source's own value domain.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, JsonSchema)]
pub struct ChannelStats {
    pub min: f32,
    pub max: f32,
    pub mean: f64,
}

/// Pixel counts by 8-bit alpha code. `le5` and `ge250` are cumulative bands
/// (they include the 0 and 255/254 codes respectively).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, JsonSchema)]
pub struct AlphaHistogram {
    pub eq255: u64,
    pub eq254: u64,
    pub eq0: u64,
    pub le5: u64,
    pub ge250: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Analysis {
    pub width: u32,
    pub height: u32,
    pub is_pot: bool,
    /// R, G, B, A.
    pub channels: [ChannelStats; 4],
    pub alpha: AlphaContent,
    pub alpha_hist: AlphaHistogram,
    /// R, G and B agree within 4/255 for every pixel.
    pub grayscale: bool,
    /// Share of pixels whose alpha code is not 255.
    pub non_white_share: f64,
    /// Predicted 1x1 alpha (box average of the base).
    pub mean_alpha: f64,
    /// Mean |length - 1| of RGB decoded as `*2 - 1`.
    pub normal_len_err: f64,
    /// More than half of the pixels have blue below 0.1.
    pub blue_mostly_black: bool,
    /// More than half of the pixels have red above 0.9.
    pub red_mostly_high: bool,
    /// Largest Rec. 709 luma over the image (not clamped, so HDR sources report above 1).
    pub peak_luminance: f32,
}

/// Round a stored value to the nearest 8-bit code, clamping to `[0, 1]` first.
/// NaN maps to 0.
pub fn code8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Compute the §5.1 statistics for `img`. `info` says whether the source file
/// actually carried an alpha channel (the float image always has four).
pub fn analyse(img: &Rgba32F, info: &SourceInfo) -> Analysis {
    let count = img.px.len();
    let n = count.max(1) as f64;
    let mut ch = [ChannelStats {
        min: f32::MAX,
        max: f32::MIN,
        mean: 0.0,
    }; 4];
    let mut hist = AlphaHistogram::default();
    let (mut gray, mut len_err, mut bb, mut rh, mut peak) = (true, 0.0f64, 0u64, 0u64, 0f32);
    for p in &img.px {
        for (s, &v) in ch.iter_mut().zip(p) {
            s.min = s.min.min(v);
            s.max = s.max.max(v);
            s.mean += f64::from(v);
        }
        let a = code8(p[3]);
        match a {
            255 => hist.eq255 += 1,
            254 => hist.eq254 += 1,
            0 => hist.eq0 += 1,
            _ => {}
        }
        hist.le5 += u64::from(a <= 5);
        hist.ge250 += u64::from(a >= 250);
        let (r, g, b) = (
            i32::from(code8(p[0])),
            i32::from(code8(p[1])),
            i32::from(code8(p[2])),
        );
        gray &= r.max(g).max(b) - r.min(g).min(b) <= 4;
        let v = [p[0] * 2.0 - 1.0, p[1] * 2.0 - 1.0, p[2] * 2.0 - 1.0];
        len_err += f64::from(((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt() - 1.0).abs());
        bb += u64::from(p[2] < 0.1);
        rh += u64::from(p[0] > 0.9);
        peak = peak.max(0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]);
    }
    for c in &mut ch {
        if count == 0 {
            // No pixels: report zeros rather than the fold identities.
            (c.min, c.max) = (0.0, 0.0);
        }
        c.mean /= n;
    }
    let alpha = if !info.has_alpha {
        AlphaContent::None
    } else if hist.eq255 as usize == count {
        AlphaContent::Constant255
    } else {
        AlphaContent::Varying
    };
    Analysis {
        width: img.width,
        height: img.height,
        is_pot: img.width.is_power_of_two() && img.height.is_power_of_two(),
        channels: ch,
        alpha,
        alpha_hist: hist,
        grayscale: gray,
        non_white_share: (count as u64 - hist.eq255) as f64 / n,
        mean_alpha: ch[3].mean,
        normal_len_err: len_err / n,
        blue_mostly_black: bb as f64 / n > 0.5,
        red_mostly_high: rh as f64 / n > 0.5,
        peak_luminance: peak,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::{Rgba32F, SourceInfo};

    const RGBA8: SourceInfo = SourceInfo {
        bit_depth: 8,
        channels: 4,
        has_alpha: true,
        is_float: false,
    };

    #[test]
    fn alpha_classification() {
        let opaque = Rgba32F::new(4, 4, [0.5, 0.5, 0.5, 1.0]);
        assert_eq!(analyse(&opaque, &RGBA8).alpha, AlphaContent::Constant255);
        let no_alpha = SourceInfo {
            channels: 3,
            has_alpha: false,
            ..RGBA8
        };
        assert_eq!(analyse(&opaque, &no_alpha).alpha, AlphaContent::None);
        let mut cut = opaque.clone();
        cut.px[0][3] = 0.0;
        let a = analyse(&cut, &RGBA8);
        assert_eq!(a.alpha, AlphaContent::Varying);
        assert_eq!(
            (a.alpha_hist.eq0, a.alpha_hist.eq255, a.alpha_hist.le5),
            (1, 15, 1)
        );
        assert!((a.non_white_share - 1.0 / 16.0).abs() < 1e-12);
        assert!(a.is_pot && a.grayscale);
    }

    #[test]
    fn grayscale_tolerance_is_4_codes() {
        let mut img = Rgba32F::new(2, 1, [0.5, 0.5, 0.5, 1.0]);
        img.px[1] = [0.5, 0.5 + 4.0 / 255.0, 0.5, 1.0];
        assert!(analyse(&img, &RGBA8).grayscale);
        img.px[1] = [0.5, 0.5 + 6.0 / 255.0, 0.5, 1.0];
        assert!(!analyse(&img, &RGBA8).grayscale);
    }

    /// Pairwise tolerance is not transitive: the whole R/G/B spread must be within 4 codes.
    #[test]
    fn grayscale_spread_covers_red_to_blue() {
        let px = |c: [f32; 3]| Rgba32F::new(2, 2, [c[0] / 255.0, c[1] / 255.0, c[2] / 255.0, 1.0]);
        assert!(!analyse(&px([100.0, 104.0, 108.0]), &RGBA8).grayscale);
        assert!(analyse(&px([100.0, 102.0, 104.0]), &RGBA8).grayscale);
    }

    #[test]
    fn normal_length_error_and_roughness_hints() {
        let flat = Rgba32F::new(4, 4, [0.5, 0.5, 1.0, 1.0]); // (0, 0, 1) encoded
        assert!(analyse(&flat, &RGBA8).normal_len_err < 0.01);
        let rough = Rgba32F::new(4, 4, [0.95, 0.0, 0.02, 1.0]);
        let a = analyse(&rough, &RGBA8);
        assert!(a.blue_mostly_black && a.red_mostly_high);
    }

    #[test]
    fn code8_rounds_and_clamps() {
        assert_eq!(code8(0.0), 0);
        assert_eq!(code8(1.0), 255);
        assert_eq!(code8(-3.0), 0);
        assert_eq!(code8(7.5), 255);
        assert_eq!(code8(f32::NAN), 0);
        assert_eq!(code8(127.4 / 255.0), 127);
        assert_eq!(code8(127.6 / 255.0), 128);
    }

    #[test]
    fn channel_stats_alpha_bands_and_peak() {
        let mut img = Rgba32F::new(3, 1, [0.0, 0.0, 0.0, 1.0]);
        img.px[0] = [0.2, 0.4, 0.6, 254.0 / 255.0];
        img.px[1] = [1.0, 1.0, 1.0, 250.0 / 255.0];
        img.px[2] = [0.0, 0.0, 0.0, 5.0 / 255.0];
        let a = analyse(&img, &RGBA8);
        assert_eq!((a.channels[0].min, a.channels[0].max), (0.0, 1.0));
        assert!((a.channels[1].mean - 1.4 / 3.0).abs() < 1e-6);
        assert_eq!(
            a.alpha_hist,
            AlphaHistogram {
                eq255: 0,
                eq254: 1,
                eq0: 0,
                le5: 1,
                ge250: 2
            }
        );
        assert!((a.mean_alpha - a.channels[3].mean).abs() < 1e-12);
        assert!((a.peak_luminance - 1.0).abs() < 1e-6);
        assert!(!a.is_pot);
    }

    #[test]
    fn hdr_peak_is_not_clamped_and_empty_image_is_safe() {
        let hdr = Rgba32F::new(2, 2, [4.0, 4.0, 4.0, 1.0]);
        assert!(analyse(&hdr, &RGBA8).peak_luminance > 3.9);
        let empty = Rgba32F::new(0, 0, [0.0; 4]);
        let a = analyse(&empty, &RGBA8);
        assert_eq!(a.channels[0].min, 0.0);
        assert!(!a.is_pot && a.normal_len_err == 0.0 && a.non_white_share == 0.0);
    }
}
