//! Preprocess (spec §5.4): channel and colour operations before mips.
//!
//! Non-finite samples are sanitised first (see [`sanitize_non_finite`]). Steps then run in spec order: (1) alpha `remove`, (2) swizzle then grayscale, (3) vector decode,
//! (4) dilation, (5) sRGB decode (integer sources only, DECISIONS S1). Resize (step 6) is milestone M6; NPOT is blocked by `DIM-NPOT`
//! before this stage. Renormalising normals is a mips concern (Task 29), not done here.
use crate::config::*;
use crate::dilate::dilate_under_zero_alpha;
use crate::image::{Rgba32F, SourceInfo};

/// What the float values in a [`Work`] image mean; later stages key their filtering and quantising on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Domain {
    /// sRGB-encoded colour decoded to linear light (re-encoded at quantise).
    LinearLight,
    /// Stored values (linear data, grayscale, packed channels).
    Stored,
    /// Unit vectors in [-1, 1] (DirectX normal maps).
    Normal,
    /// Model-space normals: R = X, G = Z, B = Y; length-only renormalisation.
    Msn,
    /// `_flow`: signed RG vector, never normalised.
    SignedRg,
    /// Linear float for BC6H.
    Hdr,
}

/// Output of [`preprocess`]: the image in its working domain plus what later stages need to know.
#[derive(Debug, Clone, PartialEq)]
pub struct Work {
    pub img: Rgba32F,
    /// False when the alpha channel was dropped (`remove`); A is 1.0 in `img` and 255 is synthesised at quantise.
    pub alpha_present: bool,
    pub domain: Domain,
    /// `_rmaos`/`_rfaos`: red is roughness, filtered in squared space by mips.
    pub roughness_r_squared: bool,
    /// Source samples (any channel) that were NaN or infinite and were replaced before processing.
    /// Task 37 surfaces a non-zero count as a warning.
    pub nonfinite_samples: u64,
}

pub fn domain_for(p: &Profile) -> Domain {
    if p.format == Format::Bc6hUf16 {
        Domain::Hdr
    } else if p.subtype == Subtype::Msn {
        Domain::Msn
    } else if p.category == Category::Normal {
        Domain::Normal
    } else if p.subtype == Subtype::Flow {
        Domain::SignedRg
    } else if p.color.content == ColorContent::Srgb {
        Domain::LinearLight
    } else {
        Domain::Stored
    }
}

/// Exact piecewise sRGB EOTF (IEC 61966-2-1), not a 2.2 power.
pub fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// Exact piecewise sRGB OETF, the inverse of [`srgb_to_linear`].
pub fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// Apply a swizzle of up to four characters from `r g b a 0 1`; missing positions are 0.
/// The character set is checked at Resolve (`CFG-INVALID`), so anything else reads as 1.
fn swizzle(px: [f32; 4], s: &str) -> [f32; 4] {
    let mut out = [0.0; 4];
    for (o, c) in out.iter_mut().zip(s.chars()) {
        *o = match c {
            'r' => px[0],
            'g' => px[1],
            'b' => px[2],
            'a' => px[3],
            '0' => 0.0,
            _ => 1.0,
        };
    }
    out
}

/// Replace NaN and infinite samples so they cannot poison the mip accumulation; returns how many were
/// replaced. NaN and -inf become 0 (for vector channels that decodes to -1, the most negative
/// component), +inf becomes 65504 in the Hdr domain and 1 otherwise. Alpha: NaN and -inf become 0,
/// +inf becomes 1. A dropped alpha channel (`remove`) is not touched or counted.
fn sanitize_non_finite(img: &mut Rgba32F, hdr: bool, alpha_present: bool) -> u64 {
    let mut replaced = 0;
    for px in &mut img.px {
        for (c, v) in px.iter_mut().enumerate() {
            if v.is_finite() || (c == 3 && !alpha_present) {
                continue;
            }
            *v = if v.is_nan() || *v == f32::NEG_INFINITY {
                0.0
            } else if hdr && c < 3 {
                65504.0
            } else {
                1.0
            };
            replaced += 1;
        }
    }
    replaced
}

/// How a source's stored values relate to light (DECISIONS S1). An integer source holds stored values whose meaning
/// `color.content` gives (`srgb` → decoded in step 5). A float source is linear light already and is never decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceTransfer {
    Stored,
    Linear,
}

impl SourceTransfer {
    pub fn of(info: &SourceInfo) -> Self {
        if info.is_float {
            SourceTransfer::Linear
        } else {
            SourceTransfer::Stored
        }
    }
}

/// `preprocess_with` for an integer source (tests, bench and callers without a `SourceInfo`).
pub fn preprocess(img: Rgba32F, p: &Profile) -> Work {
    preprocess_with(img, p, SourceTransfer::Stored)
}

pub fn preprocess_with(mut img: Rgba32F, p: &Profile, transfer: SourceTransfer) -> Work {
    let domain = domain_for(p);
    let alpha_present = p.alpha.policy != AlphaPolicy::Remove;
    let nonfinite_samples = sanitize_non_finite(&mut img, domain == Domain::Hdr, alpha_present);
    let grayscale = p.category == Category::Grayscale;
    for px in &mut img.px {
        if !alpha_present {
            px[3] = 1.0; // (1) the channel is dropped; A = 255 is synthesised when the block input is built
        }
        if let Some(s) = &p.channels.swizzle {
            *px = swizzle(*px, s); // (2)
            if !alpha_present {
                px[3] = 1.0; // a swizzle such as `rgb0` must not bring back a dropped alpha
            }
        }
        if grayscale {
            let g = match p.channels.grayscale_source {
                GraySource::R => px[0],
                GraySource::Luma => 0.2126 * px[0] + 0.7152 * px[1] + 0.0722 * px[2],
            };
            px[..3].fill(g);
        }
        match domain {
            // (3) `flip_green` is already set by Resolve for `green_convention = opengl`
            Domain::Normal => {
                let x = px[0] * 2.0 - 1.0;
                let y = px[1] * 2.0 - 1.0;
                let z = px[2] * 2.0 - 1.0;
                let y = if p.channels.flip_green { -y } else { y };
                let z = if p.channels.reconstruct_z {
                    (1.0 - x * x - y * y).max(0.0).sqrt()
                } else {
                    z.max(0.0)
                };
                px[..3].copy_from_slice(&[x, y, z]);
            }
            Domain::Msn => {
                for c in &mut px[..3] {
                    *c = *c * 2.0 - 1.0;
                }
            }
            Domain::SignedRg => {
                for c in &mut px[..2] {
                    *c = *c * 2.0 - 1.0;
                }
            }
            _ => {}
        }
    }
    if alpha_present && p.alpha_weighted_mips() {
        dilate_under_zero_alpha(&mut img); // (4)
    }
    if p.color.content == ColorContent::Srgb
        && transfer == SourceTransfer::Stored
        && matches!(domain, Domain::LinearLight | Domain::Hdr)
    {
        for px in &mut img.px {
            for c in &mut px[..3] {
                *c = srgb_to_linear(*c); // (5) RGB only
            }
        }
    }
    if domain == Domain::Hdr {
        // BC6H UF16 has no negatives: clamp the base so mip 0 and the accumulated levels agree.
        for px in &mut img.px {
            for c in &mut px[..3] {
                *c = c.max(0.0);
            }
        }
    }
    Work {
        img,
        alpha_present,
        domain,
        roughness_r_squared: p.subtype.is_roughness_member(),
        nonfinite_samples,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets::PresetRegistry;

    fn profile(st: Subtype) -> Profile {
        PresetRegistry::builtin()
            .unwrap()
            .profile("cs-quality", Target::Cs, st)
            .unwrap()
    }

    #[test]
    fn a_float_source_is_never_srgb_decoded() {
        let p = profile(Subtype::Diffuse); // content srgb
        let img = Rgba32F::new(1, 1, [0.5, 0.5, 0.5, 1.0]);
        let stored = preprocess_with(img.clone(), &p, SourceTransfer::Stored);
        let linear = preprocess_with(img, &p, SourceTransfer::Linear);
        assert!(
            (stored.img.px[0][0] - srgb_to_linear(0.5)).abs() < 1e-6,
            "integer: decoded"
        );
        assert_eq!(linear.img.px[0][0], 0.5, "float: already linear");
        assert_eq!(linear.domain, Domain::LinearLight);
        let info = crate::image::SourceInfo {
            bit_depth: 32,
            channels: 3,
            has_alpha: false,
            is_float: true,
        };
        assert_eq!(SourceTransfer::of(&info), SourceTransfer::Linear);
    }

    #[test]
    fn srgb_curve_round_trips_every_8_bit_code() {
        for k in 0..=255u32 {
            let v = k as f32 / 255.0;
            let back = (linear_to_srgb(srgb_to_linear(v)) * 255.0 + 0.5).floor() as u32;
            assert_eq!(back, k);
        }
        assert!((srgb_to_linear(0.5) - 0.214_041).abs() < 1e-5);
    }

    #[test]
    fn remove_drops_alpha_and_srgb_content_goes_linear() {
        let img = Rgba32F::new(2, 2, [0.5, 0.5, 0.5, 0.3]);
        let w = preprocess(img, &profile(Subtype::Diffuse)); // diffuse default: remove, srgb
        assert!(!w.alpha_present);
        assert_eq!(w.domain, Domain::LinearLight);
        assert_eq!(w.img.px[0][3], 1.0);
        assert!((w.img.px[0][0] - 0.214_041).abs() < 1e-5);
    }

    #[test]
    fn normals_decode_flip_and_rebuild_z() {
        let mut p = profile(Subtype::Normal);
        p.channels.flip_green = true;
        p.channels.reconstruct_z = true;
        let img = Rgba32F::new(1, 1, [0.5, 1.0, 0.0, 0.8]);
        let w = preprocess(img, &p);
        assert_eq!(w.domain, Domain::Normal);
        let v = w.img.px[0];
        assert!((v[0]).abs() < 1e-6 && (v[1] + 1.0).abs() < 1e-6 && v[2].abs() < 1e-6);
        assert!(w.alpha_present && (v[3] - 0.8).abs() < 1e-6); // A = specular data, untouched
    }

    #[test]
    fn grayscale_luma_and_swizzle() {
        let mut p = profile(Subtype::Height);
        p.channels.grayscale_source = GraySource::Luma;
        let w = preprocess(Rgba32F::new(1, 1, [1.0, 0.0, 0.0, 1.0]), &p);
        assert!((w.img.px[0][0] - 0.2126).abs() < 1e-6 && w.img.px[0][1] == w.img.px[0][0]);
        let mut p = profile(Subtype::GenericData);
        p.channels.swizzle = Some("bgr1".into());
        let w = preprocess(Rgba32F::new(1, 1, [0.1, 0.2, 0.3, 0.4]), &p);
        assert_eq!(w.img.px[0], [0.3, 0.2, 0.1, 1.0]);
    }

    #[test]
    fn domains() {
        assert_eq!(domain_for(&profile(Subtype::Msn)), Domain::Msn);
        assert_eq!(domain_for(&profile(Subtype::Flow)), Domain::SignedRg);
        assert_eq!(domain_for(&profile(Subtype::PbrEmissive)), Domain::Hdr);
        assert_eq!(domain_for(&profile(Subtype::Rmaos)), Domain::Stored);
        assert!(
            preprocess(Rgba32F::new(1, 1, [0.5; 4]), &profile(Subtype::SkinRfaos))
                .roughness_r_squared
        );
    }

    #[test]
    fn msn_and_flow_decode_without_normal_handling() {
        // msn: all three channels *2-1; no flip, no Z rebuild, no clamp even if the profile asks.
        let mut p = profile(Subtype::Msn);
        p.channels.flip_green = true;
        p.channels.reconstruct_z = true;
        let w = preprocess(Rgba32F::new(1, 1, [0.25, 0.25, 0.0, 1.0]), &p);
        assert_eq!(w.domain, Domain::Msn);
        assert_eq!(&w.img.px[0][..3], &[-0.5, -0.5, -1.0]);
        // flow: only RG decoded; B and A are left alone.
        let mut p = profile(Subtype::Flow);
        p.alpha.policy = AlphaPolicy::Data;
        let w = preprocess(Rgba32F::new(1, 1, [0.25, 1.0, 0.7, 0.9]), &p);
        assert_eq!(w.domain, Domain::SignedRg);
        assert_eq!(w.img.px[0][..3], [-0.5, 1.0, 0.7]);
        assert_eq!(w.img.px[0][3], 0.9);
    }

    #[test]
    fn normal_without_rebuild_clamps_negative_z_and_leaves_length_alone() {
        let mut p = profile(Subtype::Normal);
        p.channels.flip_green = false;
        p.channels.reconstruct_z = false;
        let w = preprocess(Rgba32F::new(1, 1, [1.0, 1.0, 0.25, 1.0]), &p);
        // (1, 1, -0.5) is not unit length: no renormalisation here; z clamps to 0.
        assert_eq!(&w.img.px[0][..3], &[1.0, 1.0, 0.0]);
    }

    #[test]
    fn dilation_runs_only_for_kept_alpha_with_weighted_policy() {
        let src = || {
            let mut img = Rgba32F::new(2, 1, [0.8, 0.4, 0.2, 1.0]);
            img.px[1] = [0.0, 0.0, 0.0, 0.0]; // invisible texel with garbage colour
            img
        };
        let mut p = profile(Subtype::GenericData);
        p.color.content = ColorContent::Linear;
        p.alpha.policy = AlphaPolicy::Cutout;
        let w = preprocess(src(), &p);
        assert_eq!(&w.img.px[1][..3], &[0.8, 0.4, 0.2]);
        assert_eq!(w.img.px[1][3], 0.0);
        // `data` alpha is not weighted: the colour under A = 0 is data and stays.
        p.alpha.policy = AlphaPolicy::Data;
        assert_eq!(preprocess(src(), &p).img.px[1], [0.0; 4]);
        // `remove` drops alpha first, so nothing is invisible.
        p.alpha.policy = AlphaPolicy::Remove;
        assert_eq!(preprocess(src(), &p).img.px[1], [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn srgb_decode_touches_rgb_only() {
        let mut p = profile(Subtype::Diffuse);
        p.alpha.policy = AlphaPolicy::Blend;
        let w = preprocess(Rgba32F::new(1, 1, [0.5, 0.5, 0.5, 0.5]), &p);
        assert!(w.alpha_present);
        assert!((w.img.px[0][0] - 0.214_041).abs() < 1e-5);
        assert_eq!(w.img.px[0][3], 0.5);
    }

    #[test]
    fn z_rebuild_and_flip_change_the_vector() {
        let mut p = profile(Subtype::Normal);
        p.channels.flip_green = true;
        p.channels.reconstruct_z = true;
        // x = 0.3, y = 0.4 before the flip, stored z = -1: the rebuild replaces z.
        let w = preprocess(Rgba32F::new(1, 1, [0.65, 0.7, 0.0, 1.0]), &p);
        let v = w.img.px[0];
        assert!((v[0] - 0.3).abs() < 1e-6 && (v[1] + 0.4).abs() < 1e-6);
        assert!((v[2] - 0.75f32.sqrt()).abs() < 1e-6);
        p.channels.reconstruct_z = false;
        let w = preprocess(Rgba32F::new(1, 1, [0.65, 0.7, 0.0, 1.0]), &p);
        assert_eq!(w.img.px[0][2], 0.0);
    }

    #[test]
    fn swizzle_cannot_restore_a_removed_alpha() {
        let mut p = profile(Subtype::GenericData);
        p.alpha.policy = AlphaPolicy::Remove;
        p.channels.swizzle = Some("rgb0".into());
        let w = preprocess(Rgba32F::new(1, 1, [0.1, 0.2, 0.3, 0.4]), &p);
        assert!(!w.alpha_present);
        assert_eq!(w.img.px[0][3], 1.0);
    }

    #[test]
    fn hdr_decodes_srgb_content_and_leaves_linear_content() {
        let mut p = profile(Subtype::PbrEmissive);
        p.color.content = ColorContent::Srgb;
        let w = preprocess(Rgba32F::new(1, 1, [0.5, 0.5, 0.5, 1.0]), &p);
        assert_eq!(w.domain, Domain::Hdr);
        assert!((w.img.px[0][0] - 0.214_041).abs() < 1e-5);
        p.color.content = ColorContent::Linear;
        let w = preprocess(Rgba32F::new(1, 1, [2.5, 0.5, 0.5, 1.0]), &p);
        assert_eq!(&w.img.px[0][..3], &[2.5, 0.5, 0.5]);
    }

    #[test]
    fn non_finite_samples_are_replaced_and_counted() {
        let mut p = profile(Subtype::GenericData);
        p.alpha.policy = AlphaPolicy::Data;
        let mut img = Rgba32F::new(2, 1, [0.5; 4]);
        img.px[0] = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::NAN];
        let w = preprocess(img, &p);
        assert_eq!(w.nonfinite_samples, 4);
        assert_eq!(w.img.px[0], [0.0, 1.0, 0.0, 0.0]);
        assert_eq!(w.img.px[1], [0.5; 4]);
        // clean input counts zero
        assert_eq!(
            preprocess(Rgba32F::new(2, 2, [0.5; 4]), &p).nonfinite_samples,
            0
        );
        // a removed alpha channel is dropped, not counted
        p.alpha.policy = AlphaPolicy::Remove;
        let w = preprocess(Rgba32F::new(1, 1, [0.5, 0.5, 0.5, f32::NAN]), &p);
        assert_eq!((w.nonfinite_samples, w.img.px[0][3]), (0, 1.0));
    }

    #[test]
    fn hdr_clamps_negative_rgb_after_srgb_decode_but_not_alpha() {
        let mut p = profile(Subtype::PbrEmissive);
        p.color.content = ColorContent::Srgb;
        p.alpha.policy = AlphaPolicy::Data;
        let w = preprocess(Rgba32F::new(1, 1, [-0.5, 0.5, -2.0, -0.25]), &p);
        assert_eq!(w.domain, Domain::Hdr);
        assert_eq!(w.img.px[0][0], 0.0);
        assert!((w.img.px[0][1] - 0.214_041).abs() < 1e-5);
        assert_eq!(w.img.px[0][2], 0.0);
        assert_eq!(w.img.px[0][3], -0.25);
    }
}
