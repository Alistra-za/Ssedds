//! Preprocess (spec §5.4): channel and colour operations before mips.
//!
//! Steps run in spec order: (1) alpha `remove`, (2) swizzle then grayscale, (3) vector decode,
//! (4) dilation, (5) sRGB decode. Resize (step 6) is milestone M6; NPOT is blocked by `DIM-NPOT`
//! before this stage. Renormalising normals is a mips concern (Task 29), not done here.
use crate::config::*;
use crate::dilate::dilate_under_zero_alpha;
use crate::image::Rgba32F;

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

pub fn preprocess(mut img: Rgba32F, p: &Profile) -> Work {
    let domain = domain_for(p);
    let alpha_present = p.alpha.policy != AlphaPolicy::Remove;
    let grayscale = p.category == Category::Grayscale;
    for px in &mut img.px {
        if !alpha_present {
            px[3] = 1.0; // (1) the channel is dropped; A = 255 is synthesised when the block input is built
        }
        if let Some(s) = &p.channels.swizzle {
            *px = swizzle(*px, s); // (2)
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
    if p.color.content == ColorContent::Srgb && matches!(domain, Domain::LinearLight | Domain::Hdr)
    {
        for px in &mut img.px {
            for c in &mut px[..3] {
                *c = srgb_to_linear(*c); // (5) RGB only
            }
        }
    }
    Work {
        img,
        alpha_present,
        domain,
        roughness_r_squared: p.subtype.is_roughness_member(),
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
            preprocess(Rgba32F::new(1, 1, [0.5; 4]), &profile(Subtype::Rfaos)).roughness_r_squared
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
}
