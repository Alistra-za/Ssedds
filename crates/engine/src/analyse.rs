//! Analyse (spec §5.1): statistics used by suggestions, warnings and validation.
//!
//! One pass over the float image, no allocation beyond the result. Every
//! "code" comparison is made on the nearest 8-bit code ([`code8`]) so the
//! thresholds match what an 8-bit viewer of the same data would see.
use crate::checks::CheckStats;
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
    /// Alpha class. `Constant255` includes near-opaque alpha (see [`NEAR_OPAQUE_ALPHA`]).
    pub alpha: AlphaContent,
    /// The source carries alpha whose every texel is within 251..=255 but not all 255 (DECISIONS Note L): `alpha`
    /// is `Constant255` and the alpha is treated as BC encode noise.
    pub alpha_near_opaque: bool,
    pub alpha_hist: AlphaHistogram,
    /// R, G and B agree within 4/255 for every pixel.
    pub grayscale: bool,
    /// Share of pixels whose alpha code is not 255.
    pub non_white_share: f64,
    /// Predicted 1x1 alpha (box average of the base).
    pub mean_alpha: f64,
    /// Mean |length - 1| of RGB decoded as `*2 - 1`.
    pub normal_len_err: f64,
    /// Mean length of RGB decoded as `*2 - 1` (DECISIONS R1: below 0.9 a normal map's mip 0 is noticeably short).
    pub normal_len_mean: f64,
    /// More than half of the pixels have blue below 0.1.
    pub blue_mostly_black: bool,
    /// More than half of the pixels have red above 0.9.
    pub red_mostly_high: bool,
    /// Largest Rec. 709 luma over the image (not clamped, so HDR sources report above 1).
    pub peak_luminance: f32,
    /// The source embeds an ICC profile, which sdc ignores (DECISIONS S8). Set by the caller from `SourceMeta`.
    pub icc_profile: bool,
    /// DXGI format of a DDS input (DECISIONS S6, spec §5.1); `None` for images. Set by the caller from `SourceMeta`.
    pub dds_dxgi: Option<u32>,
    /// NaN and ±∞ samples per channel (R, G, B, A). They are left out of every statistic above; Preprocess replaces
    /// them (DECISIONS S9).
    pub nonfinite: [u64; 4],
    /// Complex grass facts (design §4, §5), for an image whose height is twice its width; `None` otherwise.
    pub grass: Option<crate::grass::GrassFacts>,
    /// `|L - 1|` of the bottom-left texel `(0, h - 1)`, the one CS's complex-grass detector reads, for every shape
    /// (PBR grass design R1); infinite for an empty image.
    pub corner_err: f32,
    /// Artist check stats (DECISIONS Note CL); `None` unless [`AnalyseHints::bands`] asked for them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checks: Option<CheckStats>,
}

/// Which artist check stats [`analyse_with`] computes besides the §5.1 statistics. All off by default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AnalyseHints {
    /// Band and luma counts; without them no [`CheckStats`] is made.
    pub bands: bool,
    /// The normal map's curl, as stored and flipped.
    pub curl: bool,
    /// The steep-step share of this channel.
    pub steps: Option<usize>,
}

/// DECISIONS Note L: source alpha whose minimum is at least 251/255 (no texel below 251) is opaque within BC
/// tolerance (BC7 re-exports leave 251..=254). The comparison is on the stored float, so an 8-bit source (exact
/// k/255) compares on its codes and a 16-bit or float source against 251/255 itself; [`NEAR_OPAQUE_EPS`] absorbs
/// only the float rounding of the division.
pub const NEAR_OPAQUE_ALPHA: f32 = CS_COMPLEX_GATE_CODE as f32 / 255.0;
const NEAR_OPAQUE_EPS: f32 = 1e-6;

/// Community Shaders' complex-material alpha gate, as an 8-bit code (spike S8, DECISIONS Note L): an `_m` whose
/// smallest-mip alpha is at or above it is a plain env mask, and source alpha whose minimum reaches it is opaque.
pub const CS_COMPLEX_GATE_CODE: u8 = 251;

/// Round a stored value to the nearest 8-bit code, clamping to `[0, 1]` first.
/// NaN maps to 0.
pub fn code8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Compute the §5.1 statistics for `img`. `info` says whether the source file
/// actually carried an alpha channel (the float image always has four).
pub fn analyse(img: &Rgba32F, info: &SourceInfo) -> Analysis {
    analyse_with(img, info, AnalyseHints::default())
}

/// [`analyse`], plus the artist check stats `hints` asks for.
pub fn analyse_with(img: &Rgba32F, info: &SourceInfo, hints: AnalyseHints) -> Analysis {
    let count = img.px.len();
    let n = count.max(1) as f64;
    let mut ch = [ChannelStats {
        min: f32::MAX,
        max: f32::MIN,
        mean: 0.0,
    }; 4];
    let mut hist = AlphaHistogram::default();
    let mut len_sum = 0.0f64;
    let mut nonfinite = [0u64; 4];
    let mut finite = [0u64; 4];
    let mut len_n = 0u64;
    let (mut gray, mut len_err, mut bb, mut rh, mut peak) = (true, 0.0f64, 0u64, 0u64, 0f32);
    let mut checks = hints.bands.then(|| CheckStats::new(info.channels == 2));
    for p in &img.px {
        for (c, (s, &v)) in ch.iter_mut().zip(p).enumerate() {
            if !v.is_finite() {
                nonfinite[c] += 1;
                continue;
            }
            s.min = s.min.min(v);
            s.max = s.max.max(v);
            s.mean += f64::from(v);
            finite[c] += 1;
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
        if let Some(s) = &mut checks {
            s.add([r as u8, g as u8, b as u8, a]);
        }
        bb += u64::from(p[2] < 0.1);
        rh += u64::from(p[0] > 0.9);
        if p[..3].iter().all(|v| v.is_finite()) {
            let v = [p[0] * 2.0 - 1.0, p[1] * 2.0 - 1.0, p[2] * 2.0 - 1.0];
            let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            len_err += f64::from((len - 1.0).abs());
            len_sum += f64::from(len);
            len_n += 1;
            peak = peak.max(0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]);
        }
    }
    for (c, s) in ch.iter_mut().enumerate() {
        if finite[c] == 0 {
            // No finite sample: report zeros rather than the fold identities.
            (s.min, s.max) = (0.0, 0.0);
        }
        s.mean /= finite[c].max(1) as f64;
    }
    if let Some(s) = &mut checks {
        s.curl = hints.curl.then(|| crate::checks::curl(img));
        s.steep = hints.steps.map(|c| crate::checks::steep_share(img, c));
    }
    let ln = len_n.max(1) as f64;
    let all_255 = hist.eq255 as usize == count;
    let near_opaque = info.has_alpha && ch[3].min >= NEAR_OPAQUE_ALPHA - NEAR_OPAQUE_EPS;
    let alpha = if !info.has_alpha {
        AlphaContent::None
    } else if all_255 || near_opaque {
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
        alpha_near_opaque: near_opaque && !all_255,
        alpha_hist: hist,
        grayscale: gray,
        non_white_share: (count as u64 - hist.eq255) as f64 / n,
        mean_alpha: ch[3].mean,
        normal_len_err: len_err / ln,
        normal_len_mean: len_sum / ln,
        blue_mostly_black: bb as f64 / n > 0.5,
        red_mostly_high: rh as f64 / n > 0.5,
        peak_luminance: peak,
        icc_profile: false,
        dds_dxgi: None,
        nonfinite,
        grass: crate::grass::facts(img),
        corner_err: img
            .px
            .get(img.height.saturating_sub(1) as usize * img.width as usize)
            .map_or(f32::INFINITY, |c| {
                crate::grass::unit_err([c[0], c[1], c[2]])
            }),
        checks,
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

    /// PBR grass design R1: `corner_err` reads texel `(0, h - 1)` of any shape, ignores alpha, and is infinite for an
    /// empty image.
    #[test]
    fn corner_err_reads_the_bottom_left_texel_of_any_shape() {
        const UNIT: [f32; 4] = [0.5, 0.5, 1.0, 1.0];
        const OFF: [f32; 4] = [0.5, 0.5, 0.5, 1.0];
        let corner = |w, h, at: (u32, u32), px: [f32; 4]| {
            let mut img = Rgba32F::new(w, h, OFF);
            *img.at_mut(at.0, at.1) = px;
            analyse(&img, &RGBA8).corner_err
        };
        for (w, h) in [(3, 5), (1, 4), (4, 1), (1, 1)] {
            assert!(corner(w, h, (0, h - 1), UNIT) < 1e-6, "{w}x{h}");
            assert!(
                (corner(w, h, (0, h - 1), [0.5, 0.5, 1.0, 0.0]) < 1e-6),
                "{w}x{h} alpha"
            );
            for at in [(w - 1, h - 1), (0, 0), (w - 1, 0)] {
                if at != (0, h - 1) {
                    assert!(corner(w, h, at, UNIT) > 0.9, "{w}x{h} {at:?}");
                }
            }
        }
        assert_eq!(
            analyse(&Rgba32F::new(0, 0, OFF), &RGBA8).corner_err,
            f32::INFINITY
        );
        assert_eq!(
            analyse(&Rgba32F::new(4, 0, OFF), &RGBA8).corner_err,
            f32::INFINITY
        );
    }

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
    fn normal_length_mean() {
        let flat = Rgba32F::new(4, 4, [0.5, 0.5, 1.0, 1.0]);
        assert!((analyse(&flat, &RGBA8).normal_len_mean - 1.0).abs() < 0.01);
        let short = Rgba32F::new(4, 4, [0.75, 0.5, 0.5, 1.0]); // decodes to (0.5, 0, 0)
        assert!((analyse(&short, &RGBA8).normal_len_mean - 0.5).abs() < 1e-6);
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
    fn nan_and_infinite_samples_are_counted_and_left_out_of_the_statistics() {
        let mut img = Rgba32F::new(4, 1, [0.5, 0.5, 1.0, 1.0]);
        img.px[0] = [f32::NAN, 0.5, 1.0, 1.0];
        img.px[1] = [0.5, f32::INFINITY, 1.0, f32::NAN];
        img.px[2] = [f32::NEG_INFINITY, 0.5, 1.0, 1.0];
        let a = analyse(&img, &RGBA8);
        assert_eq!(a.nonfinite, [2, 1, 0, 1]);
        assert_eq!((a.channels[0].min, a.channels[0].max), (0.5, 0.5));
        assert!(
            (a.channels[0].mean - 0.5).abs() < 1e-12,
            "mean over finite samples"
        );
        assert!(a.channels[1].mean.is_finite() && a.channels[1].max == 0.5);
        assert!((a.mean_alpha - 1.0).abs() < 1e-12);
        // Only the fully finite pixel (index 3) is measured: (0, 0, 1) has unit length.
        assert!(a.normal_len_err < 1e-6 && (a.normal_len_mean - 1.0).abs() < 1e-6);
        assert!(a.peak_luminance.is_finite());
    }

    #[test]
    fn an_all_nan_channel_reports_zeros_not_nan() {
        let img = Rgba32F::new(2, 2, [f32::NAN, 0.5, 0.5, 1.0]);
        let a = analyse(&img, &RGBA8);
        assert_eq!(a.nonfinite[0], 4);
        assert_eq!(
            (a.channels[0].min, a.channels[0].max, a.channels[0].mean),
            (0.0, 0.0, 0.0)
        );
        assert_eq!((a.normal_len_err, a.normal_len_mean), (0.0, 0.0));
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

    /// DECISIONS Note L: a minimum of 251/255 or more is opaque within BC tolerance; one code below is varying.
    #[test]
    fn near_opaque_alpha_is_constant_at_min_251_and_varying_at_250() {
        let alpha = |codes: &[u8]| {
            let mut img = Rgba32F::new(codes.len() as u32, 1, [0.5, 0.5, 0.5, 1.0]);
            for (p, &c) in img.px.iter_mut().zip(codes) {
                p[3] = f32::from(c) / 255.0;
            }
            analyse(&img, &RGBA8)
        };
        let a = alpha(&[251, 252, 253, 254, 255]);
        assert_eq!(a.alpha, AlphaContent::Constant255);
        assert!(a.alpha_near_opaque);
        assert_eq!(alpha(&[250, 255, 255]).alpha, AlphaContent::Varying);
        assert!(!alpha(&[250, 255, 255]).alpha_near_opaque);
        // Exactly 255 everywhere is opaque but not "near": nothing to report.
        let flat = alpha(&[255, 255]);
        assert_eq!(flat.alpha, AlphaContent::Constant255);
        assert!(!flat.alpha_near_opaque);
        // No alpha channel in the source: never near-opaque.
        let none = SourceInfo {
            channels: 3,
            has_alpha: false,
            ..RGBA8
        };
        let img = Rgba32F::new(2, 1, [0.5, 0.5, 0.5, 251.0 / 255.0]);
        assert!(!analyse(&img, &none).alpha_near_opaque);
    }

    #[test]
    fn band_counts_match_a_per_texel_count_and_are_absent_without_hints() {
        use crate::checks::BandCounts;
        let mut img = Rgba32F::new(4, 1, [0.0; 4]);
        for (p, c) in img.px.iter_mut().zip([0u8, 25, 230, 255]) {
            *p = [c as f32 / 255.0; 4];
        }
        let a = analyse_with(
            &img,
            &RGBA8,
            AnalyseHints {
                bands: true,
                ..Default::default()
            },
        );
        let s = a.checks.unwrap();
        assert_eq!(
            s.bands[0],
            BandCounts {
                eq0: 1,
                le4: 1,
                low: 2,
                high: 2,
                ge251: 1,
                eq255: 1
            }
        );
        assert_eq!(s.bands[3], s.bands[0]);
        assert_eq!(
            (s.luma_dark, s.luma_bright, s.luma_black, s.texels),
            (2, 1, 1, 4)
        );
        assert!(!s.two_channel);
        assert_eq!((s.curl, s.steep), (None, None));
        assert_eq!(analyse(&img, &RGBA8).checks, None);
        // Everything else is unchanged by the hints.
        let mut plain = a.clone();
        plain.checks = None;
        assert_eq!(plain, analyse(&img, &RGBA8));
        // A 0x0 image: texels 0, every count 0, no panic.
        let e = analyse_with(
            &Rgba32F::new(0, 0, [0.0; 4]),
            &RGBA8,
            AnalyseHints {
                bands: true,
                curl: true,
                steps: Some(0),
            },
        );
        let e = e.checks.unwrap();
        assert_eq!(e.texels, 0);
        assert_eq!(e.bands, [BandCounts::default(); 4]);
        // Two-channel sources (BC5 / RG8) are flagged; curl and steps run only when hinted.
        let rg = SourceInfo {
            channels: 2,
            has_alpha: false,
            ..RGBA8
        };
        let s = analyse_with(
            &img,
            &rg,
            AnalyseHints {
                bands: true,
                curl: true,
                steps: Some(0),
            },
        )
        .checks
        .unwrap();
        assert!(s.two_channel && s.curl.is_some() && s.steep.is_some());
    }

    /// 16-bit sources: the cutoff is 251/255 in float, so 250.9/255 is varying and 251/255 is not.
    #[test]
    fn near_opaque_cutoff_is_251_over_255_in_float() {
        let src = SourceInfo {
            bit_depth: 16,
            ..RGBA8
        };
        let mut img = Rgba32F::new(2, 1, [0.5, 0.5, 0.5, 1.0]);
        img.px[0][3] = 250.9 / 255.0;
        assert_eq!(analyse(&img, &src).alpha, AlphaContent::Varying);
        img.px[0][3] = 251.0 / 255.0;
        let a = analyse(&img, &src);
        assert_eq!(a.alpha, AlphaContent::Constant255);
        assert!(a.alpha_near_opaque);
    }
}
