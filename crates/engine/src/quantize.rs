//! Float level -> encoder input (u8 RGBA for BC1/3/4/5/7, half RGBA for BC6H).
//!
//! Quantise is the clamp point for mip 0: `mips::build_chain` returns level 0 as authored (normals renormalised), so
//! every path here clamps to the range its target format can hold before converting.
use crate::analyse::code8;
use crate::config::*;
use crate::image::Rgba32F;
use crate::ops::{linear_to_srgb, Domain, Work};
use half::f16;

/// Pixels handed to an encoder: 8-bit RGBA for the LDR formats, raw IEEE half bits for BC6H.
#[derive(Debug, Clone, PartialEq)]
pub enum EncoderInput {
    Ldr(Vec<[u8; 4]>),
    Hdr(Vec<[u16; 4]>),
}

/// A working-domain pixel's RGB as the file stores it (0..1, before rounding to 8 bits): sRGB-encoded for linear
/// light, `v * 0.5 + 0.5` for vectors, as is for stored values and HDR.
pub fn stored_rgb(q: &[f32; 4], domain: Domain) -> [f32; 3] {
    let enc = |v: f32| linear_to_srgb(v.clamp(0.0, 1.0));
    let signed = |v: f32| v * 0.5 + 0.5;
    match domain {
        Domain::LinearLight => [enc(q[0]), enc(q[1]), enc(q[2])],
        Domain::Normal | Domain::Msn => [signed(q[0]), signed(q[1]), signed(q[2])],
        Domain::SignedRg => [signed(q[0]), signed(q[1]), 0.0],
        Domain::Stored | Domain::Hdr => [q[0], q[1], q[2]],
    }
}

/// Largest finite half-float value.
const HALF_MAX: f32 = 65504.0;

/// Nearest 8-bit code of `v` clamped to `[0, 1]`; NaN maps to 0.
///
/// This is `floor(clamp(v) * 255 + 0.5)` in exact arithmetic, and it *is* [`code8`]: coverage counting
/// uses `code8`, so Quantise must produce bit-identical codes (the two-step f32 `floor(x + 0.5)` can
/// differ from a single rounding for values a hair below a half-code boundary).
pub fn unorm8(v: f32) -> u8 {
    code8(v)
}

/// `v` clamped to the half-float range `[0, 65504]`; -0.0 and NaN give 0. Quantise and the BC6H peak check share it.
pub(crate) fn clamp_half(v: f32) -> f32 {
    // Not `clamp`: it would keep -0.0 (sign bit set) and NaN.
    if v > 0.0 {
        v.min(HALF_MAX)
    } else {
        0.0
    }
}

/// IEEE half bits of `v` clamped to `[0, 65504]` (round to nearest even; NaN maps to 0).
fn half_bits(v: f32) -> u16 {
    f16::from_f32(clamp_half(v)).to_bits()
}

/// Quantise one finalised level (see `mips::build_chain`) to what the encoder for `p` consumes.
///
/// `_flow` (`Domain::SignedRg`): R and G are signed vector components stored as `v * 0.5 + 0.5`; B is
/// written as 0, because preprocess never decodes B and the BC5 encoder reads only R and G.
/// Alpha is `unorm8(a)` when the channel is present, else 255: this is the only place a removed
/// alpha is synthesised.
pub fn to_encoder_input(level: &Rgba32F, work: &Work, p: &Profile) -> EncoderInput {
    if p.format.is_hdr() {
        let one = f16::ONE.to_bits();
        // DECISIONS S4: rgba16f stores alpha as a half when it is kept; BC6H has no alpha.
        let keep_alpha = p.format == Format::Rgba16f && work.alpha_present;
        return EncoderInput::Hdr(
            level
                .px
                .iter()
                .map(|q| {
                    [
                        half_bits(q[0]),
                        half_bits(q[1]),
                        half_bits(q[2]),
                        if keep_alpha { half_bits(q[3]) } else { one },
                    ]
                })
                .collect(),
        );
    }
    EncoderInput::Ldr(
        level
            .px
            .iter()
            .map(|q| {
                let rgb = stored_rgb(q, work.domain);
                let a = if work.alpha_present {
                    unorm8(q[3])
                } else {
                    255
                };
                [unorm8(rgb[0]), unorm8(rgb[1]), unorm8(rgb[2]), a]
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::preprocess;
    use crate::presets::PresetRegistry;

    #[test]
    fn clamp_half_matches_the_documented_range() {
        assert_eq!(clamp_half(-0.0).to_bits(), 0);
        assert_eq!(clamp_half(f32::NAN), 0.0);
        assert_eq!(clamp_half(1e9), 65504.0);
        assert_eq!(clamp_half(1.5), 1.5);
    }

    fn profile(st: Subtype) -> Profile {
        PresetRegistry::builtin()
            .unwrap()
            .profile("cs-quality", Target::Cs, st)
            .unwrap()
    }

    fn work(px: [f32; 4], domain: Domain, alpha_present: bool) -> Work {
        Work {
            img: Rgba32F::new(1, 1, px),
            alpha_present,
            domain,
            roughness_r_squared: false,
            nonfinite_samples: 0,
        }
    }

    fn ldr(w: &Work, st: Subtype) -> [u8; 4] {
        let EncoderInput::Ldr(q) = to_encoder_input(&w.img, w, &profile(st)) else {
            panic!()
        };
        q[0]
    }

    fn hdr(px: [f32; 4]) -> [u16; 4] {
        let p = profile(Subtype::PbrEmissive);
        let w = work(px, Domain::Hdr, false);
        let EncoderInput::Hdr(h) = to_encoder_input(&w.img, &w, &p) else {
            panic!()
        };
        h[0]
    }

    #[test]
    fn eight_bit_sources_survive_load_preprocess_quantise_exactly() {
        for st in [
            Subtype::Diffuse,
            Subtype::PbrAlbedo,
            Subtype::GenericData,
            Subtype::Normal,
            Subtype::Msn,
        ] {
            let mut img = Rgba32F::new(256, 1, [0.0; 4]);
            for (k, px) in img.px.iter_mut().enumerate() {
                let v = k as f32 / 255.0;
                *px = [v, v, v, v];
            }
            let p = profile(st);
            let w = preprocess(img, &p);
            let EncoderInput::Ldr(q) = to_encoder_input(&w.img, &w, &p) else {
                panic!()
            };
            for (k, c) in q.iter().enumerate() {
                assert_eq!(c[0] as usize, k, "{st:?} code {k}");
                assert_eq!(c[1] as usize, k, "{st:?} code {k} (G)");
                if st != Subtype::Normal {
                    // Normal blue is Z, floored at 0 by preprocess (so codes below 128 stay 128).
                    assert_eq!(c[2] as usize, k, "{st:?} code {k} (B)");
                }
            }
        }
    }

    #[test]
    fn signed_rg_round_trips_and_blue_is_zero() {
        let p = profile(Subtype::Flow);
        let mut img = Rgba32F::new(256, 1, [0.0; 4]);
        for (k, px) in img.px.iter_mut().enumerate() {
            let v = k as f32 / 255.0;
            *px = [v, 1.0 - v, 0.7, 1.0];
        }
        let w = preprocess(img, &p);
        assert_eq!(w.domain, Domain::SignedRg);
        let EncoderInput::Ldr(q) = to_encoder_input(&w.img, &w, &p) else {
            panic!()
        };
        for (k, c) in q.iter().enumerate() {
            assert_eq!(c[0] as usize, k);
            assert_eq!(c[1] as usize, 255 - k);
            assert_eq!(c[2], 0);
        }
    }

    #[test]
    fn removed_alpha_is_255_and_hdr_is_half() {
        let p = profile(Subtype::Diffuse); // remove
        let w = preprocess(Rgba32F::new(1, 1, [0.5, 0.5, 0.5, 0.2]), &p);
        assert!(!w.alpha_present);
        assert_eq!(ldr(&w, Subtype::Diffuse)[3], 255);
        assert_eq!(hdr([2.0, -1.0, 0.5, 1.0]), [0x4000, 0x0000, 0x3800, 0x3C00]);
    }

    #[test]
    fn present_alpha_is_quantised() {
        let w = work([0.0, 0.0, 0.0, 0.5], Domain::Stored, true);
        assert_eq!(ldr(&w, Subtype::GenericData)[3], 128);
        let w = work([0.0, 0.0, 0.0, 2.0], Domain::Stored, true);
        assert_eq!(ldr(&w, Subtype::GenericData)[3], 255);
        let w = work([0.0, 0.0, 0.0, -2.0], Domain::Stored, true);
        assert_eq!(ldr(&w, Subtype::GenericData)[3], 0);
    }

    #[test]
    fn mip0_out_of_range_is_clamped_per_path() {
        // LinearLight: a float source above 1 must saturate, not wrap or go NaN through powf.
        let w = work([1.5, -0.5, 1.0, 1.0], Domain::LinearLight, true);
        assert_eq!(&ldr(&w, Subtype::PbrAlbedo)[..3], &[255, 0, 255]);
        // Stored and vector domains clamp after their mapping.
        let w = work([3.0, -3.0, 0.0, 1.0], Domain::Stored, true);
        assert_eq!(&ldr(&w, Subtype::GenericData)[..3], &[255, 0, 0]);
        let w = work([3.0, -3.0, 0.0, 1.0], Domain::Normal, true);
        assert_eq!(&ldr(&w, Subtype::Normal)[..3], &[255, 0, 128]);
        let w = work([3.0, -3.0, 0.0, 1.0], Domain::SignedRg, true);
        assert_eq!(&ldr(&w, Subtype::Flow)[..3], &[255, 0, 0]);
    }

    #[test]
    fn hdr_clamps_to_half_range() {
        assert_eq!(hdr([-5.0, -0.0, 0.0, 1.0])[..3], [0, 0, 0]);
        assert_eq!(
            hdr([65504.0, 70000.0, 1e30, 1.0])[..3],
            [0x7BFF, 0x7BFF, 0x7BFF]
        );
        assert_eq!(hdr([f32::NAN, 0.0, 0.0, 0.0])[0], 0);
        assert_eq!(hdr([0.0; 4])[3], 0x3C00, "alpha is always 1.0");
    }

    #[test]
    fn half_conversion_exact_values() {
        assert_eq!(half_bits(0.0), 0x0000);
        assert_eq!(half_bits(1.0), 0x3C00);
        assert_eq!(half_bits(0.5), 0x3800);
        assert_eq!(half_bits(65504.0), 0x7BFF);
        assert_eq!(half_bits(2f32.powi(-14)), 0x0400, "smallest normal");
        assert_eq!(half_bits(2f32.powi(-24)), 0x0001, "smallest subnormal");
        assert_eq!(half_bits(3.0 * 2f32.powi(-24)), 0x0003, "subnormal");
        // Round to nearest even: 1 + 2^-11 is a tie between 0x3C00 and 0x3C01 -> even (0x3C00);
        // 1 + 3*2^-11 is a tie between 0x3C01 and 0x3C02 -> even (0x3C02).
        assert_eq!(half_bits(1.0 + 2f32.powi(-11)), 0x3C00);
        assert_eq!(half_bits(1.0 + 3.0 * 2f32.powi(-11)), 0x3C02);
        assert_eq!(half_bits(1.0 + 2f32.powi(-11) + 2f32.powi(-20)), 0x3C01);
    }

    #[test]
    fn unorm8_matches_code8_and_code_boundaries() {
        let mut vals = vec![
            -1.0,
            -0.0,
            0.0,
            1.0,
            2.0,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ];
        for k in 1..=255u32 {
            let c = k as f32 / 255.0;
            let lo = (k as f32 - 0.5) / 255.0;
            let hi = (k as f32 + 0.5) / 255.0;
            for v in [c, lo, hi] {
                vals.extend([
                    v,
                    f32::from_bits(v.to_bits() + 1),
                    f32::from_bits(v.to_bits() - 1),
                ]);
            }
        }
        for i in 0..100_000u32 {
            vals.push(i as f32 / 99_999.0);
        }
        for v in vals {
            assert_eq!(unorm8(v), code8(v), "v = {v:e}");
        }
        for k in 0..=255u32 {
            assert_eq!(unorm8(k as f32 / 255.0) as u32, k);
        }
        assert_eq!(unorm8(f32::NAN), 0);
        // Nearest code, just below / above a half-code boundary.
        assert_eq!(unorm8(f32::from_bits((10.5f32 / 255.0).to_bits() - 2)), 10);
        assert_eq!(unorm8(f32::from_bits((10.5f32 / 255.0).to_bits() + 2)), 11);
    }

    #[test]
    fn rgba16f_keeps_alpha_as_half_and_bc6h_forces_one() {
        let mut p = profile(Subtype::PbrEmissive);
        p.format = Format::Rgba16f;
        let w = work([2.0, 0.5, 0.25, 0.5], Domain::Hdr, true);
        let EncoderInput::Hdr(h) = to_encoder_input(&w.img, &w, &p) else {
            panic!()
        };
        assert_eq!(h[0][3], f16::from_f32(0.5).to_bits());
        let w = work([2.0, 0.5, 0.25, 0.5], Domain::Hdr, false);
        let EncoderInput::Hdr(h) = to_encoder_input(&w.img, &w, &p) else {
            panic!()
        };
        assert_eq!(h[0][3], f16::ONE.to_bits());
        assert_eq!(
            hdr([2.0, 0.5, 0.25, 0.5])[3],
            f16::ONE.to_bits(),
            "BC6H unchanged"
        );
    }
}
