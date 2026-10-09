//! Float mip chain (spec §5.5). Box filter only in v1 Phase 1; other kernels arrive in milestone M6.
//!
//! An *accumulation* chain is built by repeated box filtering of the unnormalised base; every output
//! level is a *finalised* copy (√ back from squared roughness, renormalised vectors, clamps). Mip 0 is
//! always the base as authored: it is never renormalised, square-rooted or clamped.
use crate::config::*;
use crate::image::Rgba32F;
use crate::ops::{Domain, Work};

/// Number of levels for a `w`×`h` base: full = ⌊log2 max(w, h)⌋ + 1, `none` = 1, a count is capped at full.
pub fn level_count(w: u32, h: u32, levels: &MipLevels) -> u32 {
    let full = 32 - w.max(h).max(1).leading_zeros();
    match levels {
        MipLevels::Keyword(MipKeyword::Full) => full,
        MipLevels::Keyword(MipKeyword::None) => 1,
        MipLevels::Count(n) => (*n).clamp(1, full),
    }
}

/// Source texel range `[start, end)` along one axis covered by destination texel `d`. For an even
/// source size this is exactly the 2×2 box pair; a size-1 axis maps to its single texel; an odd size
/// (NPOT is blocked upstream, so only a safety net) still covers every texel instead of dropping the edge.
fn footprint(d: u32, src: u32, dst: u32) -> std::ops::Range<u32> {
    let start = (u64::from(d) * u64::from(src) / u64::from(dst)) as u32;
    let end = ((u64::from(d + 1) * u64::from(src)).div_ceil(u64::from(dst)) as u32).max(start + 1);
    start..end.min(src)
}

/// One box step. With `weighted`, RGB is the alpha-weighted mean of the footprint (equal to the
/// base-footprint weighted mean when applied recursively) and falls back to the plain mean when the
/// footprint has no alpha; alpha itself is always the plain mean.
fn downsample(prev: &Rgba32F, weighted: bool) -> Rgba32F {
    let (w, h) = ((prev.width / 2).max(1), (prev.height / 2).max(1));
    let mut out = Rgba32F::new(w, h, [0.0; 4]);
    for y in 0..h {
        let ys = footprint(y, prev.height, h);
        for x in 0..w {
            let xs = footprint(x, prev.width, w);
            let n = (xs.len() * ys.len()) as f32;
            let mut sum = [0.0f32; 4];
            let mut sum_weighted = [0.0f32; 3];
            for cy in ys.clone() {
                for cx in xs.clone() {
                    let k = prev.at(cx, cy);
                    for c in 0..4 {
                        sum[c] += k[c];
                    }
                    for c in 0..3 {
                        sum_weighted[c] += k[c] * k[3];
                    }
                }
            }
            let use_weights = weighted && sum[3] > 0.0;
            let px = out.at_mut(x, y);
            for c in 0..3 {
                px[c] = if use_weights {
                    sum_weighted[c] / sum[3]
                } else {
                    sum[c] / n
                };
            }
            px[3] = sum[3] / n;
        }
    }
    out
}

/// Turn an accumulation level (>= 1) into its stored form: roughness √, then the domain's
/// vector renormalisation or clamp. `SignedRg` (`_flow`) is deliberately left alone.
fn finalize(acc: &Rgba32F, work: &Work) -> Rgba32F {
    let mut img = acc.clone();
    for px in &mut img.px {
        if work.roughness_r_squared {
            px[0] = px[0].max(0.0).sqrt();
        }
        match work.domain {
            Domain::Normal | Domain::Msn => {
                let len = (px[0] * px[0] + px[1] * px[1] + px[2] * px[2]).sqrt();
                if len > 1e-8 {
                    for c in &mut px[..3] {
                        *c /= len;
                    }
                } else if work.domain == Domain::Normal {
                    px[..3].copy_from_slice(&[0.0, 0.0, 1.0]);
                }
            }
            Domain::Hdr => {
                for c in &mut px[..3] {
                    *c = c.max(0.0);
                }
            }
            Domain::LinearLight | Domain::Stored => {
                for c in &mut px[..3] {
                    *c = c.clamp(0.0, 1.0);
                }
            }
            Domain::SignedRg => {}
        }
    }
    img
}

/// Finalised mip levels, level 0 first, in the Work's domain. Level 0 is the base exactly as given.
pub fn build_chain(work: &Work, p: &Profile) -> Vec<Rgba32F> {
    let n = level_count(work.img.width, work.img.height, &p.mips.levels);
    let weighted = work.alpha_present && p.alpha_weighted_mips();
    let mut acc = work.img.clone();
    if work.roughness_r_squared {
        for px in &mut acc.px {
            px[0] *= px[0];
        }
    }
    let mut out = Vec::with_capacity(n as usize);
    out.push(work.img.clone());
    for _ in 1..n {
        acc = downsample(&acc, weighted);
        out.push(finalize(&acc, work));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::preprocess;
    use crate::presets::PresetRegistry;

    fn profile(st: Subtype) -> Profile {
        PresetRegistry::builtin()
            .unwrap()
            .profile("cs-quality", Target::Cs, st)
            .unwrap()
    }
    fn work(img: Rgba32F, domain: Domain, alpha: bool) -> Work {
        Work {
            img,
            alpha_present: alpha,
            domain,
            roughness_r_squared: false,
        }
    }

    #[test]
    fn level_counts() {
        assert_eq!(
            level_count(256, 64, &MipLevels::Keyword(MipKeyword::Full)),
            9
        );
        assert_eq!(
            level_count(256, 64, &MipLevels::Keyword(MipKeyword::None)),
            1
        );
        assert_eq!(level_count(256, 64, &MipLevels::Count(3)), 3);
        assert_eq!(level_count(256, 64, &MipLevels::Count(40)), 9);
        assert_eq!(level_count(1, 1, &MipLevels::Keyword(MipKeyword::Full)), 1);
    }

    #[test]
    fn box_chain_of_checkerboard_and_non_square() {
        let mut img = Rgba32F::new(4, 2, [0.0, 0.0, 0.0, 1.0]);
        for y in 0..2 {
            for x in 0..4 {
                img.at_mut(x, y)[0] = ((x + y) % 2) as f32;
            }
        }
        let c = build_chain(
            &work(img, Domain::Stored, false),
            &profile(Subtype::GenericData),
        );
        assert_eq!(
            c.iter().map(|l| (l.width, l.height)).collect::<Vec<_>>(),
            [(4, 2), (2, 1), (1, 1)]
        );
        assert!(c[1].px.iter().all(|p| (p[0] - 0.5).abs() < 1e-6));
        assert!((c[2].px[0][0] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn one_pixel_high_chain_halves_width_only() {
        let mut img = Rgba32F::new(4, 1, [0.0, 0.0, 0.0, 1.0]);
        for (x, v) in [0.0f32, 1.0, 0.5, 0.25].into_iter().enumerate() {
            img.px[x][0] = v;
        }
        let c = build_chain(
            &work(img, Domain::Stored, false),
            &profile(Subtype::GenericData),
        );
        assert_eq!(
            c.iter().map(|l| (l.width, l.height)).collect::<Vec<_>>(),
            [(4, 1), (2, 1), (1, 1)]
        );
        assert!((c[1].px[0][0] - 0.5).abs() < 1e-6 && (c[1].px[1][0] - 0.375).abs() < 1e-6);
        assert!((c[2].px[0][0] - 0.4375).abs() < 1e-6);
    }

    #[test]
    fn odd_size_axis_still_covers_every_texel() {
        // Safety net only (NPOT is blocked upstream): a 3x1 base must not drop its last texel.
        let mut img = Rgba32F::new(3, 1, [0.0, 0.0, 0.0, 1.0]);
        img.px[2][0] = 0.9;
        let c = build_chain(
            &work(img, Domain::Stored, false),
            &profile(Subtype::GenericData),
        );
        assert_eq!(c.len(), 2);
        assert!((c[1].px[0][0] - 0.3).abs() < 1e-6);
    }

    #[test]
    fn alpha_weighted_rgb_ignores_invisible_colour() {
        let mut img = Rgba32F::new(2, 2, [0.0, 1.0, 0.0, 0.0]);
        img.px[0] = [1.0, 0.0, 0.0, 1.0];
        let mut p = profile(Subtype::Diffuse);
        p.alpha.policy = AlphaPolicy::Blend;
        let c = build_chain(&work(img, Domain::LinearLight, true), &p);
        assert_eq!(c[1].px[0], [1.0, 0.0, 0.0, 0.25]);
    }

    #[test]
    fn zero_alpha_footprint_falls_back_to_plain_average() {
        let mut img = Rgba32F::new(2, 1, [0.2, 0.4, 0.6, 0.0]);
        img.px[1] = [0.4, 0.0, 0.2, 0.0];
        let mut p = profile(Subtype::Diffuse);
        p.alpha.policy = AlphaPolicy::Cutout;
        let c = build_chain(&work(img, Domain::LinearLight, true), &p);
        let v = c[1].px[0];
        assert!(
            (v[0] - 0.3).abs() < 1e-6 && (v[1] - 0.2).abs() < 1e-6 && (v[2] - 0.4).abs() < 1e-6
        );
        assert_eq!(v[3], 0.0);
    }

    #[test]
    fn alpha_weighting_is_off_when_alpha_dropped_or_policy_is_data() {
        let mut img = Rgba32F::new(2, 1, [0.0, 0.0, 0.0, 1.0]);
        img.px[0] = [1.0, 0.0, 0.0, 1.0];
        img.px[1] = [0.0, 0.0, 0.0, 0.0];
        let mut p = profile(Subtype::Diffuse);
        p.alpha.policy = AlphaPolicy::Blend;
        // alpha channel was removed: plain average even though the policy would weight.
        let c = build_chain(&work(img.clone(), Domain::LinearLight, false), &p);
        assert!((c[1].px[0][0] - 0.5).abs() < 1e-6);
        // data alpha: plain average.
        p.alpha.policy = AlphaPolicy::Data;
        let c = build_chain(&work(img, Domain::LinearLight, true), &p);
        assert!((c[1].px[0][0] - 0.5).abs() < 1e-6 && (c[1].px[0][3] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn weighted_chain_matches_base_footprint_average_at_level_2() {
        // 4x1 with alphas 1, 0, 0.5, 0.5: the level-2 colour is sum(c*a)/sum(a) over all four texels.
        let mut img = Rgba32F::new(4, 1, [0.0, 0.0, 0.0, 0.0]);
        img.px[0] = [1.0, 0.0, 0.0, 1.0];
        img.px[1] = [0.0, 1.0, 0.0, 0.0];
        img.px[2] = [0.0, 0.0, 1.0, 0.5];
        img.px[3] = [0.0, 0.0, 1.0, 0.5];
        let mut p = profile(Subtype::Diffuse);
        p.alpha.policy = AlphaPolicy::Cutout;
        let c = build_chain(&work(img, Domain::LinearLight, true), &p);
        let v = c[2].px[0];
        assert!((v[0] - 0.5).abs() < 1e-6 && v[1].abs() < 1e-6 && (v[2] - 0.5).abs() < 1e-6);
        assert!((v[3] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn normals_renormalised_from_level_1_only() {
        let mut img = Rgba32F::new(2, 1, [0.0, 0.0, 1.0, 1.0]);
        img.px[0] = [1.0, 0.0, 0.0, 1.0];
        let c = build_chain(
            &work(img.clone(), Domain::Normal, true),
            &profile(Subtype::Normal),
        );
        assert_eq!(c[0].px, img.px);
        let v = c[1].px[0];
        assert!(
            (v[0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-5
                && (v[2] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-5
        );
        assert_eq!(v[3], 1.0); // specular alpha is a plain box, not part of the vector
    }

    #[test]
    fn normal_with_cancelling_vectors_falls_back_to_flat() {
        let mut img = Rgba32F::new(2, 1, [0.0, 0.0, 0.0, 1.0]);
        img.px[0] = [1.0, 0.0, 0.0, 1.0];
        img.px[1] = [-1.0, 0.0, 0.0, 1.0];
        let c = build_chain(&work(img, Domain::Normal, true), &profile(Subtype::Normal));
        assert_eq!(&c[1].px[0][..3], &[0.0, 0.0, 1.0]);
    }

    #[test]
    fn msn_is_length_only_without_z_clamp_or_flat_fallback() {
        // R = X, G = Z, B = Y. Negative "Z" must survive: no Normal-style z >= 0 clamp.
        let mut img = Rgba32F::new(2, 1, [0.0, 0.0, 0.0, 1.0]);
        img.px[0] = [0.0, -0.3, -0.4, 1.0];
        img.px[1] = [0.0, -0.3, -0.4, 1.0];
        let c = build_chain(&work(img, Domain::Msn, true), &profile(Subtype::Msn));
        let v = c[1].px[0];
        assert!(v[0].abs() < 1e-6 && (v[1] + 0.6).abs() < 1e-6 && (v[2] + 0.8).abs() < 1e-6);
        // exactly cancelling vectors stay zero: Msn has no flat-normal default.
        let mut img = Rgba32F::new(2, 1, [0.0, 0.0, 0.0, 1.0]);
        img.px[0] = [1.0, 0.0, 0.0, 1.0];
        img.px[1] = [-1.0, 0.0, 0.0, 1.0];
        let c = build_chain(&work(img, Domain::Msn, true), &profile(Subtype::Msn));
        assert_eq!(&c[1].px[0][..3], &[0.0, 0.0, 0.0]);
    }

    #[test]
    fn flow_is_never_normalised_even_though_presets_say_renormalize() {
        let p = profile(Subtype::Flow);
        assert_eq!(p.mips.normal, NormalMips::Renormalize);
        let mut img = Rgba32F::new(2, 1, [0.0, 0.0, 0.0, 1.0]);
        img.px[0] = [1.0, 0.0, 0.0, 1.0];
        img.px[1] = [0.0, 1.0, 0.0, 1.0];
        let c = build_chain(&work(img, Domain::SignedRg, true), &p);
        let v = c[1].px[0];
        let len = (v[0] * v[0] + v[1] * v[1]).sqrt();
        assert!(len < 0.75, "length {len}");
        assert!((v[0] - 0.5).abs() < 1e-6 && (v[1] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn flow_keeps_negative_components() {
        let mut img = Rgba32F::new(2, 1, [-0.5, -0.25, 0.0, 1.0]);
        img.px[1] = [-0.5, -0.75, 0.0, 1.0];
        let c = build_chain(&work(img, Domain::SignedRg, true), &profile(Subtype::Flow));
        assert!((c[1].px[0][0] + 0.5).abs() < 1e-6 && (c[1].px[0][1] + 0.5).abs() < 1e-6);
    }

    #[test]
    fn roughness_is_averaged_as_alpha_r_squared() {
        let mut img = Rgba32F::new(2, 1, [0.0, 0.0, 0.0, 1.0]);
        img.px[1][0] = 1.0;
        let w = preprocess(img, &profile(Subtype::Rmaos));
        let c = build_chain(&w, &profile(Subtype::Rmaos));
        assert!((c[1].px[0][0] - 0.5f32.sqrt()).abs() < 1e-6);
        assert_eq!(c[0].px[1][0], 1.0);
    }

    #[test]
    fn roughness_mip_0_is_untouched_and_other_channels_are_plain() {
        let mut img = Rgba32F::new(2, 1, [0.3, 0.2, 0.1, 1.0]);
        img.px[1] = [0.6, 0.4, 0.3, 1.0];
        let p = profile(Subtype::Rmaos);
        let w = preprocess(img, &p);
        let c = build_chain(&w, &p);
        assert_eq!(c[0].px, w.img.px);
        assert!((c[1].px[0][0] - ((0.09f32 + 0.36) / 2.0).sqrt()).abs() < 1e-6);
        assert!((c[1].px[0][1] - 0.3).abs() < 1e-6 && (c[1].px[0][2] - 0.2).abs() < 1e-6);
    }

    #[test]
    fn hdr_levels_clamp_negatives_but_keep_highlights() {
        let mut img = Rgba32F::new(2, 1, [-1.0, 4.0, 0.5, 1.0]);
        img.px[1] = [-3.0, 8.0, 0.5, 1.0];
        let c = build_chain(
            &work(img.clone(), Domain::Hdr, false),
            &profile(Subtype::PbrEmissive),
        );
        assert_eq!(c[0].px, img.px); // mip 0 passes through
        assert_eq!(&c[1].px[0][..3], &[0.0, 6.0, 0.5]);
    }

    #[test]
    fn linear_light_and_stored_levels_clamp_to_unit_range() {
        let mut img = Rgba32F::new(2, 1, [1.4, -0.2, 0.5, 1.0]);
        img.px[1] = [1.4, -0.2, 0.5, 1.0];
        for d in [Domain::LinearLight, Domain::Stored] {
            let c = build_chain(&work(img.clone(), d, false), &profile(Subtype::GenericData));
            assert_eq!(&c[1].px[0][..3], &[1.0, 0.0, 0.5]);
        }
    }

    #[test]
    fn none_keyword_gives_a_single_level() {
        let mut p = profile(Subtype::GenericData);
        p.mips.levels = MipLevels::Keyword(MipKeyword::None);
        let c = build_chain(
            &work(Rgba32F::new(8, 8, [0.5; 4]), Domain::Stored, false),
            &p,
        );
        assert_eq!(c.len(), 1);
    }
}
