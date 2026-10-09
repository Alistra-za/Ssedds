//! Validation (spec §5.8): re-read the written DDS, decode every mip, check it against the profile.
//!
//! `alpha` holds the decoded-alpha statistics used by spike S5 and by Validate.
pub mod alpha;

use crate::codes::{Code, Severity, Warning};
use crate::config::*;
use crate::coverage::coverage_fraction;
use crate::dds::{check_header, read_dds};
use crate::decode::{decode_level, Decoded};
use crate::quantize::EncoderInput;
use alpha::{alpha_stats, opaque_block_violations, AlphaStats};
use encoders_sys::Bc1Model;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Levels with fewer texels than this are not coverage-checked: a 1x1 level is 0 % or 100 %, and at 8x8 one
/// texel flipped by a one-code BC7 alpha error is already 1.6 points (the solver puts the boundary texel exactly
/// on the lowest passing code). At 16x16 one flip is 0.4 points.
const COVERAGE_MIN_TEXELS: u32 = 256;

/// Slack on the coverage tolerance, in points: a drift of exactly the tolerance is within it, but in f64
/// `(0.52 - 0.5) * 100.0` is 2.0000000000000018.
const COVERAGE_EPS: f64 = 1e-9;

pub struct ValidationInput<'a> {
    pub bytes: &'a [u8],
    pub profile: &'a Profile,
    /// What went into the encoder, largest level first (enables the per-mip error and exact-opaque checks).
    pub encoder_input: Option<&'a [EncoderInput]>,
    /// Coverage of mip 0 under the profile's threshold and comparison (enables the coverage check).
    /// Callers pass `None` when the alpha policy is `remove`: a removed channel decodes to all-255, which
    /// would otherwise be compared against the source's coverage.
    pub base_coverage: Option<f64>,
    /// Peak luminance of the HDR source (enables the BC6H peak check).
    pub source_peak: Option<f32>,
    /// rgbcx decoder model used to decode BC1.
    pub bc1_model: Bc1Model,
    pub expected_levels: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ValidationSummary {
    /// No `Error`-severity warning.
    pub passed: bool,
    /// The header's mip count.
    pub levels: u32,
    pub min_mip_psnr_db: Option<f64>,
    /// Decoded-alpha statistics per level, for formats that carry alpha (BC1, BC3, BC7).
    pub alpha: Vec<AlphaStats>,
    /// Decoded coverage per level; filled only when the coverage check ran.
    pub coverage: Vec<f64>,
    pub bc6h_peak_ratio: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ValidationReport {
    pub summary: ValidationSummary,
    pub warnings: Vec<Warning>,
}

/// PSNR in dB over the channels `ch` of two equally long images (99 dB when they are identical).
fn psnr(a: &[[u8; 4]], b: &[[u8; 4]], ch: &[usize]) -> f64 {
    let mut s = 0.0f64;
    for (x, y) in a.iter().zip(b) {
        for &c in ch {
            let e = x[c] as f64 - y[c] as f64;
            s += e * e;
        }
    }
    let mse = s / (a.len() * ch.len()).max(1) as f64;
    if mse <= 1e-12 {
        99.0
    } else {
        10.0 * (65025.0 / mse).log10()
    }
}

fn finish(mut summary: ValidationSummary, warnings: Vec<Warning>) -> ValidationReport {
    summary.passed = !warnings.iter().any(|w| w.severity == Severity::Error);
    ValidationReport { summary, warnings }
}

/// Re-read `inp.bytes`, decode every level and run the spec §5.8 checks for `inp.profile`.
///
/// Never panics on malformed bytes: header and decode failures become `VAL-HEADER` errors. `passed` is true
/// when no warning has `Error` severity.
///
/// Gating on the stored alpha (`alpha.policy != remove`):
/// - `VAL-M-GRAYSCALE` reads RGB only, so it runs for every complex `_m`, with or without stored alpha.
/// - `VAL-M-COMPLEX-MARGIN` and `VAL-M-HEIGHT-CLIP` measure the stored alpha and are skipped under `remove`
///   (they would only see the synthesised 255). A complex `_m` whose alpha is removed is Resolve's finding
///   (`LOSS-ALPHA-DATA` for a varying source, a violation for a flat one), not a Validate one.
/// - `VAL-PGPATCHER-50` likewise needs stored alpha.
/// - The remove guarantee (decoded alpha >= 254 for BC7 without `exact_opaque`, else 255) is checked for
///   BC1, BC3 and BC7 only; BC1 and BC3 store 255 exactly.
///
/// `encoder_input` is a contract with the caller: its level count and per-level texel counts must match the
/// file. A mismatch is a caller bug; it is reported as `VAL-HEADER` and trips a `debug_assert!`.
pub fn validate(inp: &ValidationInput) -> ValidationReport {
    let p = inp.profile;
    let v = &p.validation;
    let mut s = ValidationSummary::default();
    let mut w: Vec<Warning> = vec![];
    let file = match read_dds(inp.bytes) {
        Ok(f) => f,
        Err(e) => {
            w.push(Warning::new(Code::ValHeader, e.to_string()));
            return finish(s, w);
        }
    };
    for msg in check_header(&file) {
        w.push(Warning::new(Code::ValHeader, msg));
    }
    s.levels = file.mip_count;
    if file.format.as_format() != Some((p.format, p.srgb_tag())) {
        w.push(Warning::new(
            Code::ValHeader,
            format!(
                "file format {:?} != expected {:?} (srgb {})",
                file.format,
                p.format,
                p.srgb_tag()
            ),
        ));
        // Every later check reads the decoded data as the profile's format.
        return finish(s, w);
    }
    if inp.expected_levels.is_some_and(|n| n != file.mip_count) {
        w.push(Warning::new(
            Code::ValHeader,
            format!(
                "mip count {} != expected {:?}",
                file.mip_count, inp.expected_levels
            ),
        ));
    }
    let model = matches!(p.format, Format::Bc1).then_some(inp.bc1_model);
    let mut decoded = vec![];
    for (lw, lh, range) in &file.levels {
        match decode_level(file.format, *lw, *lh, &file.data[range.clone()], model) {
            Ok(d) => decoded.push((*lw, *lh, d)),
            Err(e) => {
                w.push(Warning::new(Code::ValHeader, format!("decode failed: {e}")));
                return finish(s, w);
            }
        }
    }
    let alpha_present = p.alpha.policy != AlphaPolicy::Remove;

    // Per-mip error and exact-opaque blocks.
    let mut opaque_causes: Vec<String> = vec![];
    if let Some(enc) = inp.encoder_input {
        if enc.len() != decoded.len() {
            let msg = format!(
                "caller supplied encoder_input does not match the file: {} levels, the file {}",
                enc.len(),
                decoded.len()
            );
            debug_assert!(false, "{msg}");
            w.push(Warning::new(Code::ValHeader, msg));
        }
        let ch: Vec<usize> = match p.format {
            Format::Bc4 => vec![0],
            Format::Bc5 => vec![0, 1],
            Format::Bc1 => vec![0, 1, 2],
            _ if alpha_present => vec![0, 1, 2, 3],
            _ => vec![0, 1, 2],
        };
        for (l, ((lw, lh, d), e)) in decoded.iter().zip(enc).enumerate() {
            let (Decoded::Ldr(d), EncoderInput::Ldr(e)) = (d, e) else {
                continue;
            };
            if d.len() != e.len() {
                let msg = format!(
                    "caller supplied encoder_input does not match the file: mip {l} has {} texels, the decoded level {}",
                    e.len(),
                    d.len()
                );
                debug_assert!(false, "{msg}");
                w.push(Warning::new(Code::ValHeader, msg));
                continue;
            }
            let q = psnr(e, d, &ch);
            s.min_mip_psnr_db = Some(s.min_mip_psnr_db.map_or(q, |m| m.min(q)));
            if q < v.min_mip_psnr_db {
                w.push(
                    Warning::new(
                        Code::ValMipError,
                        format!("mip {l}: {q:.2} dB < {:.1} dB", v.min_mip_psnr_db),
                    )
                    .with_data(json!({"level": l, "psnr": q})),
                );
            }
            if p.format == Format::Bc7 && p.alpha.exact_opaque {
                let bad = opaque_block_violations(e, d, *lw, *lh);
                if bad > 0 {
                    opaque_causes.push(format!(
                        "mip {l}: {bad} texels of all-255 blocks decode below 255"
                    ));
                }
            }
        }
    }

    if p.format.has_alpha() {
        let ldr: Vec<(u32, u32, &Vec<[u8; 4]>)> = decoded
            .iter()
            .filter_map(|(a, b, d)| match d {
                Decoded::Ldr(px) => Some((*a, *b, px)),
                Decoded::Hdr(_) => None,
            })
            .collect();
        s.alpha = ldr.iter().map(|(_, _, px)| alpha_stats(px)).collect();

        let guarantee = if p.format == Format::Bc7 && !p.alpha.exact_opaque {
            254
        } else {
            255
        };
        let mut causes: Vec<String> = vec![];
        if p.alpha.policy == AlphaPolicy::Remove {
            for (l, st) in s.alpha.iter().enumerate() {
                if st.min < guarantee {
                    causes.push(format!(
                        "mip {l}: decoded alpha {} below the guarantee {guarantee}",
                        st.min
                    ));
                }
            }
        }
        causes.extend(opaque_causes);
        // One error per file naming every cause: under `remove` with `exact_opaque` the opaque-block texels
        // are a subset of the decoded-alpha ones, so separate errors would only duplicate the finding.
        if !causes.is_empty() {
            w.push(
                Warning::new(Code::ValAlphaGuarantee, causes.join("; "))
                    .with_data(json!({"causes": causes})),
            );
        }
        if p.alpha.policy == AlphaPolicy::Remove
            && guarantee == 254
            && s.alpha.iter().any(|a| a.eq254 > 0)
        {
            let (first, last) = (s.alpha[0], *s.alpha.last().expect("alpha has a level"));
            w.push(
                Warning::new(
                    Code::Alpha254,
                    format!(
                        "decoded alpha 254 on {:.1}% of mip 0 and {:.1}% of the smallest mip",
                        first.share_254() * 100.0,
                        last.share_254() * 100.0
                    ),
                )
                .with_data(
                    json!({"mip0_share": first.share_254(), "smallest_share": last.share_254()}),
                ),
            );
        }

        if p.coverage_enabled() {
            if let Some(base) = inp.base_coverage {
                for (l, (lw, lh, px)) in ldr.iter().enumerate() {
                    let c = coverage_fraction(
                        px.iter().map(|q| q[3] as f32 / 255.0),
                        p.alpha.threshold,
                        p.alpha.compare,
                    );
                    s.coverage.push(c);
                    if u64::from(*lw) * u64::from(*lh) >= u64::from(COVERAGE_MIN_TEXELS)
                        && (c - base).abs() * 100.0 > v.coverage_tolerance_pts + COVERAGE_EPS
                    {
                        w.push(
                            Warning::new(
                                Code::ValCoverage,
                                format!(
                                    "mip {l}: coverage {:.1}% vs base {:.1}%",
                                    c * 100.0,
                                    base * 100.0
                                ),
                            )
                            .with_data(json!({"level": l, "coverage": c, "base": base})),
                        );
                    }
                }
            }
        }

        if p.subtype == Subtype::ComplexMaterial {
            if let Some((_, _, px)) = ldr.last().filter(|_| alpha_present) {
                let a = px.iter().map(|q| q[3]).max().unwrap_or(0);
                if a >= v.complex_m_warn_alpha {
                    w.push(Warning::new(
                        Code::ValMComplexMargin,
                        format!(
                            "smallest-mip alpha {a}/255 is within the 4-code margin of CS's 251/255 complex gate"
                        ),
                    ));
                }
            }
            if let Some((_, _, px)) = ldr.first() {
                let n = px.len().max(1) as f64;
                // Same definition as Analyse (`analyse.rs`): the whole R/G/B spread within 4 codes.
                let gray = px
                    .iter()
                    .filter(|q| q[0].max(q[1]).max(q[2]) - q[0].min(q[1]).min(q[2]) <= 4)
                    .count() as f64
                    / n;
                let mean = px
                    .iter()
                    .map(|q| (q[0] as f64 + q[1] as f64 + q[2] as f64) / 3.0)
                    .sum::<f64>()
                    / n;
                if gray > 0.95 && mean > 25.0 {
                    w.push(Warning::new(
                        Code::ValMGrayscale,
                        "complex _m with R≈G≈B: CS rejects it",
                    ));
                }
                let clip = px.iter().filter(|q| q[3] <= 4 || q[3] >= 251).count() as f64 / n;
                if alpha_present && clip > 0.25 {
                    w.push(Warning::new(
                        Code::ValMHeightClip,
                        format!("{:.0}% of height alpha is ≤4 or ≥251", clip * 100.0),
                    ));
                }
            }
        }

        if p.subtype == Subtype::Envmask && alpha_present {
            if let Some(a0) = s.alpha.first() {
                if a0.share_not_255() >= v.pgpatcher_non_white_share {
                    w.push(Warning::new(
                        Code::ValPgpatcher50,
                        format!(
                            "{:.0}% of alpha is not 255: PGPatcher treats this _m as complex",
                            a0.share_not_255() * 100.0
                        ),
                    ));
                }
            }
        }
    }

    if p.format == Format::Bc6hUf16 {
        if let (Some(peak), Some((_, _, Decoded::Hdr(px)))) = (inp.source_peak, decoded.first()) {
            let dec_peak = px
                .iter()
                .map(|c| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2])
                .fold(0.0f32, f32::max);
            if peak > 0.0 {
                let r = (dec_peak / peak) as f64;
                s.bc6h_peak_ratio = Some(r);
                if r < v.bc6h_min_peak_ratio {
                    w.push(Warning::new(
                        Code::ValBc6hPeak,
                        format!("BC6H keeps {:.0}% of the source peak", r * 100.0),
                    ));
                }
            }
        }
    }
    finish(s, w)
}
