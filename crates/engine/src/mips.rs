//! Float mip chain (spec §5.5). Box (the default) is recursive and unchanged; tent, Mitchell and Kaiser (S-mips-3)
//! build each level's RGB directly from mip 0 with [`crate::filter::resample_rgb`], the kernel widened by the
//! level's scale. How the two compose:
//! - **Box:** the code path is unchanged; nothing new runs.
//! - **Non-box:** the box loop still runs, so the accumulation, alpha and the roughness and coat bakes are exactly
//!   as for box. After `finalize` and before the bakes, RGB is overwritten by the direct resample and clamped to
//!   the domain again (linear light [0, 1], HDR ≥ 0). Alpha always stays the recursive box.
//! - **Coverage:** `preserve_coverage` reads only (box) alpha; a non-box filter is never allowed with `cutout` or
//!   `cutout_blend` (S-mips-2).
//! - **Box-only contents:** normal renormalisation, the `_cnr` self-bake, `LengthPyramid` and roughness r² are box
//!   only (S-mips-2); `effective_filter` returns box for them, and for alpha-weighted RGB, even if a blocked profile
//!   reached the chain.
//! - **Odd sizes** (an acknowledged `DIM-NPOT` without resize): the level size is the box chain's, and the resample
//!   takes any n -> m.
//!
//! An *accumulation* chain is built by repeated box filtering of the unnormalised base; every output
//! level is a *finalised* copy (√ back from squared roughness, renormalised vectors, clamps). Mip 0 is the base as
//! authored, except a normal map's vectors, which are renormalised unless `mips.normal_base = keep` (DECISIONS R1); a
//! vector chain accumulates from that written mip 0, so its lower levels and the variance bake see the stored vectors.
//! A `_cnr` with `mips.normal = renormalize_bake` (the default) widens its own coat roughness (alpha) from its own
//! footprint lengths (DECISIONS R2); `mips.normal` is inert on every other sub-type here: the `_n` -> `_rmaos` bake
//! arrives through [`build_chain_with`].
use crate::bake::{baked_roughness, variance_add, NormalVariance};
use crate::config::*;
use crate::image::Rgba32F;
use crate::ops::{Domain, Work};

/// Number of levels for a `w`×`h` base: full = `dds::full_chain_len`, the one chain-length rule the DDS header shares, `none` = 1, a count is capped at full.
pub fn level_count(w: u32, h: u32, levels: &MipLevels) -> u32 {
    let full = crate::dds::full_chain_len(w.max(1), h.max(1)) as u32;
    match levels {
        MipLevels::Keyword(MipKeyword::Full) => full,
        MipLevels::Keyword(MipKeyword::None) => 1,
        MipLevels::Count(n) => (*n).clamp(1, full),
    }
}

/// Source texels covered by destination texel `d` when every texel pairs up exactly: a size-1 axis
/// maps to its single texel, an even size to the 2-texel box.
fn pair_footprint(d: u32, src: u32) -> std::ops::Range<u32> {
    if src == 1 {
        0..1
    } else {
        2 * d..2 * d + 2
    }
}

/// One box step. Even and size-1 axes take the 2x2 pair path; any axis with an odd size > 1 uses
/// [`downsample_area`]. Power-of-two images only ever run the pair path.
fn downsample(prev: &Rgba32F, weighted: bool) -> Rgba32F {
    let odd = |n: u32| n > 1 && n % 2 == 1;
    if odd(prev.width) || odd(prev.height) {
        return downsample_area(prev, weighted);
    }
    let (w, h) = ((prev.width / 2).max(1), (prev.height / 2).max(1));
    let mut out = Rgba32F::new(w, h, [0.0; 4]);
    for y in 0..h {
        let ys = pair_footprint(y, prev.height);
        for x in 0..w {
            let xs = pair_footprint(x, prev.width);
            let n = (xs.len() * ys.len()) as f32;
            let mut sum = [0.0f32; 4];
            let mut sum_weighted = [0.0f32; 3];
            for cy in ys.clone() {
                for cx in xs.clone() {
                    let k = prev.at(cx, cy);
                    for c in 0..4 {
                        sum[c] += k[c];
                    }
                    if weighted {
                        for c in 0..3 {
                            sum_weighted[c] += k[c] * k[3];
                        }
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

/// One unweighted box step (the bake's length pyramid, `bake::LengthPyramid::build`).
pub(crate) fn box_down(prev: &Rgba32F) -> Rgba32F {
    downsample(prev, false)
}

/// Per-axis area weights: destination texel `i` of `m` covers the source interval
/// `[i*n/m, (i+1)*n/m)` of `n` texels; each source texel contributes its overlap length. Weights of
/// one destination texel sum to 1.
fn axis_weights(n: u32, m: u32) -> Vec<Vec<(u32, f64)>> {
    let (n64, m64) = (u64::from(n), u64::from(m));
    (0..m64)
        .map(|i| {
            // Work in units of 1/m texel: destination covers [i*n, (i+1)*n), source j covers [j*m, (j+1)*m).
            let (lo, hi) = (i * n64, (i + 1) * n64);
            let first = lo / m64;
            let last = hi.div_ceil(m64).min(n64);
            (first..last)
                .map(|j| {
                    let overlap = hi.min((j + 1) * m64) - lo.max(j * m64);
                    (j as u32, overlap as f64 / n64 as f64)
                })
                .collect()
        })
        .collect()
}

/// Area-exact box step for any size (odd sizes included).
fn downsample_area(prev: &Rgba32F, weighted: bool) -> Rgba32F {
    area_resample(
        prev,
        ((prev.width / 2).max(1), (prev.height / 2).max(1)),
        weighted,
    )
}

/// Area-exact box resample of `prev` to `w`×`h` (no upscaling), accumulated in f64. Weighted RGB uses
/// area x alpha as the weight.
pub(crate) fn area_resample(prev: &Rgba32F, (w, h): (u32, u32), weighted: bool) -> Rgba32F {
    let (wx, wy) = (axis_weights(prev.width, w), axis_weights(prev.height, h));
    let mut out = Rgba32F::new(w, h, [0.0; 4]);
    for (y, ys) in wy.iter().enumerate() {
        for (x, xs) in wx.iter().enumerate() {
            let mut sum = [0.0f64; 4];
            let mut sum_weighted = [0.0f64; 3];
            for &(cy, vy) in ys {
                for &(cx, vx) in xs {
                    let k = prev.at(cx, cy);
                    let area = vx * vy;
                    for c in 0..4 {
                        sum[c] += area * f64::from(k[c]);
                    }
                    if weighted {
                        for c in 0..3 {
                            sum_weighted[c] += area * f64::from(k[c]) * f64::from(k[3]);
                        }
                    }
                }
            }
            let mut px = sum.map(|v| v as f32);
            if weighted && sum[3] > 0.0 {
                for c in 0..3 {
                    px[c] = (sum_weighted[c] / sum[3]) as f32;
                }
            }
            *out.at_mut(x as u32, y as u32) = px;
        }
    }
    out
}

/// Rescale an RGB vector to unit length in place (spec §5.5). A zero `Normal` vector becomes flat +Z; a zero
/// `_msn` vector stays zero (model space has no flat default).
pub(crate) fn normalize_vector(px: &mut [f32; 4], domain: Domain) {
    let len = (px[0] * px[0] + px[1] * px[1] + px[2] * px[2]).sqrt();
    if len > 1e-8 {
        for c in &mut px[..3] {
            *c /= len;
        }
    } else if domain == Domain::Normal {
        px[..3].copy_from_slice(&[0.0, 0.0, 1.0]);
    }
}

/// Mip 0 as stored (spec §5.4, DECISIONS R1): a normal map's base vectors at unit length unless
/// `mips.normal_base = keep`; anything else as authored. A vector chain accumulates from this level, so the levels
/// below and the variance bake's footprint lengths average the vectors the file stores (controller correction to R1).
fn base_level(work: &Work, p: &Profile) -> Rgba32F {
    let mut out = work.img.clone();
    if p.mips.normal_base == NormalBase::Renormalize
        && matches!(work.domain, Domain::Normal | Domain::Msn)
    {
        for px in &mut out.px {
            normalize_vector(px, work.domain);
        }
    }
    out
}

/// Alpha-weighted RGB filtering (spec §5.5) is for colour. A vector chain (normal, `_msn`, `_flow`) or a roughness
/// chain is never weighted, even with an alpha-tested policy forced on it (Task 29 carry, DECISIONS R11):
/// weighting would bend the vectors, and the bake's footprint lengths, toward opaque texels.
pub(crate) fn weighted_rgb(work: &Work, p: &Profile) -> bool {
    work.alpha_present
        && p.alpha_weighted_mips()
        && !work.roughness_r_squared
        && !matches!(work.domain, Domain::Normal | Domain::Msn | Domain::SignedRg)
}

/// The filter the RGB levels use (S-mips-3): `p.mips.filter` when policy allows it ([`crate::filter::filter_allowed`])
/// and the RGB is not alpha-weighted, else box. The policy check is the gate; this is a backstop for a blocked job.
pub(crate) fn effective_filter(work: &Work, p: &Profile) -> MipFilter {
    if crate::filter::filter_allowed(p).is_ok() && !weighted_rgb(work, p) {
        p.mips.filter
    } else {
        MipFilter::Box
    }
}

/// The domain clamp of RGB for a level >= 1: HDR at 0 and above, linear light and stored data in [0, 1]; vectors and
/// `SignedRg` are untouched.
pub(crate) fn clamp_rgb(px: &mut [f32; 4], domain: Domain) {
    match domain {
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
        Domain::Normal | Domain::Msn | Domain::SignedRg => {}
    }
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
            Domain::Normal | Domain::Msn => normalize_vector(px, work.domain),
            d => clamp_rgb(px, d),
        }
    }
    img
}

/// Finalised mip levels, level 0 first, in the Work's domain.
///
/// Levels >= 1 are finalised and clamped (roughness √, vector renormalisation, domain clamps). Mip 0 is the base as
/// authored, except a normal map's vectors, renormalised unless `mips.normal_base = keep` (DECISIONS R1); the
/// consumer (Quantise, Task 31) clamps it for the target format.
pub fn build_chain(work: &Work, p: &Profile) -> Vec<Rgba32F> {
    build_chain_with(work, p, None)
}

/// [`build_chain`] with a roughness member's variance bake (spec §6.2): where `bake` adds variance, R is stored as
/// `(min(α² + add, 1))^¼`, α the footprint mean of r²; every other value is bit-identical to [`build_chain`].
/// A `complex_material` bakes its gloss (G) instead (Note CL40, [`bake_gloss`]); `bake` is ignored on anything else.
pub fn build_chain_with(work: &Work, p: &Profile, bake: Option<&NormalVariance>) -> Vec<Rgba32F> {
    let n = level_count(work.img.width, work.img.height, &p.mips.levels);
    let weighted = weighted_rgb(work, p);
    let rough = bake.filter(|_| work.roughness_r_squared);
    let gloss = bake.filter(|_| p.subtype == Subtype::ComplexMaterial);
    let coat = coat_self_bake(work, p);
    let filt = effective_filter(work, p);
    let mut out = Vec::with_capacity(n as usize);
    let mut base = base_level(work, p);
    if let Some(b) = rough {
        let src = &work.img;
        bake_roughness(&mut base, 0, b, |i| src.px[i][0] * src.px[i][0]);
    }
    if let Some(b) = gloss {
        bake_gloss(&mut base, 0, b);
    }
    out.push(base);
    // `acc` is the unnormalised accumulation level; the first step reads the base directly unless roughness needs
    // squaring, so the base is not cloned a second time.
    let mut acc: Option<Rgba32F> = None;
    for level in 1..n as usize {
        let next = match &acc {
            Some(prev) => downsample(prev, weighted),
            None if work.roughness_r_squared => {
                let mut squared = work.img.clone();
                for px in &mut squared.px {
                    px[0] *= px[0];
                }
                downsample(&squared, weighted)
            }
            // A vector chain accumulates from mip 0 as written (unit length unless `normal_base = keep`).
            None if matches!(work.domain, Domain::Normal | Domain::Msn) => {
                downsample(&out[0], weighted)
            }
            None => downsample(&work.img, weighted),
        };
        let mut fin = finalize(&next, work);
        if filt != MipFilter::Box {
            // S-mips-3: RGB straight from mip 0 at this level's size; alpha keeps the box value above.
            let size = (next.width, next.height);
            let direct = crate::filter::resample_rgb(&out[0], size, filt, p.mips.edge);
            for (px, d) in fin.px.iter_mut().zip(&direct.px) {
                px[..3].copy_from_slice(&d[..3]);
                clamp_rgb(px, work.domain);
            }
        }
        if let Some(b) = rough {
            bake_roughness(&mut fin, level, b, |i| next.px[i][0]);
        }
        if let Some(b) = gloss {
            bake_gloss(&mut fin, level, b);
        }
        if let Some(t) = coat {
            bake_coat(&mut fin, &next, t);
        }
        out.push(fin);
        acc = Some(next);
    }
    out
}

/// Spec §6.2 step 4 on R: a texel whose `_n` footprint adds variance gets `(min(α² + add, 1))^¼`, α = `alpha(i)` (the
/// footprint mean of r²); the rest keep the plain chain's value bit for bit.
fn bake_roughness(
    level: &mut Rgba32F,
    index: usize,
    b: &NormalVariance,
    alpha: impl Fn(usize) -> f32,
) {
    let w = level.width;
    for (i, px) in level.px.iter_mut().enumerate() {
        let add = b.add_at(index, i as u32 % w, i as u32 / w);
        if add > 0.0 {
            px[0] = baked_roughness(alpha(i), add);
        }
    }
}

/// The opt-in complex `_m` gloss bake (DECISIONS Note CL40): where the `_n` footprint adds variance, G is stored as
/// `1 − baked_roughness((1 − g)², add)`; the rest keep the plain chain's value bit for bit.
fn bake_gloss(level: &mut Rgba32F, index: usize, b: &NormalVariance) {
    let w = level.width;
    for (i, px) in level.px.iter_mut().enumerate() {
        let add = b.add_at(index, i as u32 % w, i as u32 / w);
        if add > 0.0 {
            // shortcut: g is the level's mean gloss, not the mean of α = (1 − g)²; average α if banding shows.
            let r = 1.0 - px[1];
            px[1] = 1.0 - baked_roughness(r * r, add);
        }
    }
}

/// `_cnr` self-bake (owner 2026-10-11, DECISIONS R2): the clamp `t` when a coat normal with
/// `mips.normal = renormalize_bake` keeps a data alpha (its coat roughness), else `None`.
fn coat_self_bake(work: &Work, p: &Profile) -> Option<f64> {
    (p.subtype == Subtype::CoatNormal
        && work.domain == Domain::Normal
        && p.mips.normal == NormalMips::RenormalizeBake
        && work.alpha_present
        && p.alpha.policy == AlphaPolicy::Data)
        .then_some(p.mips.bake_clamp)
}

/// The coat roughness of one level: where the level's own footprint length (`acc`, before renormalising) adds
/// variance, A = (min(r̄⁴ + add, 1))^¼ with r̄ the plain box mean the level already holds; elsewhere A is unchanged,
/// so a flat coat normal keeps its alpha bit for bit.
fn bake_coat(level: &mut Rgba32F, acc: &Rgba32F, clamp_t: f64) {
    for (px, a) in level.px.iter_mut().zip(&acc.px) {
        let len = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
        let add = variance_add(len, clamp_t);
        if add > 0.0 {
            px[3] = baked_roughness(px[3] * px[3], add);
        }
    }
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
            nonfinite_samples: 0,
        }
    }

    fn coat(img: Rgba32F) -> (Work, Profile) {
        let p = profile(Subtype::CoatNormal);
        (preprocess(img, &p), p)
    }

    #[test]
    fn cnr_profiles_self_bake_by_default() {
        assert_eq!(
            profile(Subtype::CoatNormal).mips.normal,
            NormalMips::RenormalizeBake
        );
        assert_eq!(
            profile(Subtype::PbrNormal).mips.normal,
            NormalMips::Renormalize
        );
    }

    #[test]
    fn a_flat_coat_normal_keeps_its_coat_roughness_bit_for_bit() {
        let mut img = Rgba32F::new(4, 4, [0.5, 0.5, 1.0, 0.0]);
        for (i, px) in img.px.iter_mut().enumerate() {
            px[3] = i as f32 / 15.0;
        }
        let (w, p) = coat(img);
        let mut off = p.clone();
        off.mips.normal = NormalMips::Renormalize;
        assert_eq!(build_chain(&w, &p), build_chain(&w, &off));
    }

    #[test]
    fn a_bumpy_coat_normal_widens_its_own_coat_roughness_by_the_formula() {
        // (0.6, 0, 0.8) and (-0.6, 0, 0.8): the level-1 footprint length is 0.8.
        let mut img = Rgba32F::new(2, 1, [0.8, 0.5, 0.9, 0.4]);
        img.px[1] = [0.2, 0.5, 0.9, 0.2];
        let (w, p) = coat(img);
        let c = build_chain(&w, &p);
        let r = (0.4f64 + 0.2) / 2.0;
        let l = 0.8f64;
        let add = (0.5 * (1.0 - l * l) / (3.0 * l - l * l * l)).min(0.0625);
        let want = (r.powi(4) + add).min(1.0).powf(0.25);
        let got = f64::from(c[1].px[0][3]);
        assert!((got - want).abs() < 1e-5, "{got} vs {want}");
        assert!(got > r + 0.01);
        assert_eq!(c[0].px[0][3], 0.4); // mip 0 keeps the source coat roughness
    }

    #[test]
    fn renormalize_turns_the_coat_self_bake_off() {
        let mut img = Rgba32F::new(2, 1, [0.8, 0.5, 0.9, 0.4]);
        img.px[1] = [0.2, 0.5, 0.9, 0.2];
        let (w, mut p) = coat(img);
        p.mips.normal = NormalMips::Renormalize;
        let c = build_chain(&w, &p);
        assert!((c[1].px[0][3] - 0.3).abs() < 1e-6, "{}", c[1].px[0][3]);
    }

    #[test]
    fn short_normals_at_mip_0_are_renormalised_by_default() {
        let mut img = Rgba32F::new(2, 1, [0.0, 0.0, 0.5, 0.7]);
        img.px[1] = [0.0, 0.0, 0.0, 0.7]; // a zero vector becomes flat +Z
        let c = build_chain(&work(img, Domain::Normal, true), &profile(Subtype::Normal));
        assert_eq!(c[0].px[0], [0.0, 0.0, 1.0, 0.7]);
        assert_eq!(c[0].px[1], [0.0, 0.0, 1.0, 0.7]); // the (specular) alpha is untouched
    }

    #[test]
    fn the_chain_accumulates_mip_0_as_written() {
        let mut img = Rgba32F::new(2, 1, [0.6, 0.0, 0.0, 1.0]);
        img.px[1] = [0.0, 0.0, 0.3, 1.0];
        let mut p = profile(Subtype::Normal);
        let renormalised = build_chain(&work(img.clone(), Domain::Normal, true), &p);
        p.mips.normal_base = NormalBase::Keep;
        let kept = build_chain(&work(img.clone(), Domain::Normal, true), &p);
        assert_eq!(kept[0].px, img.px);
        assert_ne!(renormalised[0].px, img.px);
        let h = std::f32::consts::FRAC_1_SQRT_2;
        // Default: level 1 averages the written unit vectors (1,0,0) and (0,0,1).
        let v = renormalised[1].px[0];
        assert!((v[0] - h).abs() < 1e-5 && (v[2] - h).abs() < 1e-5, "{v:?}");
        // keep: level 1 averages the authored vectors, (0.3, 0, 0.15) normalised (the pre-M4 chain).
        let len = (0.3f32 * 0.3 + 0.15 * 0.15).sqrt();
        let v = kept[1].px[0];
        assert!(
            (v[0] - 0.3 / len).abs() < 1e-5 && (v[2] - 0.15 / len).abs() < 1e-5,
            "{v:?}"
        );
    }

    #[test]
    fn msn_mip_0_is_renormalised_by_length_only() {
        let mut img = Rgba32F::new(2, 1, [0.0, -0.3, -0.4, 1.0]);
        img.px[1] = [0.0, 0.0, 0.0, 1.0];
        let c = build_chain(&work(img, Domain::Msn, true), &profile(Subtype::Msn));
        let v = c[0].px[0];
        assert!(v[0].abs() < 1e-6 && (v[1] + 0.6).abs() < 1e-6 && (v[2] + 0.8).abs() < 1e-6);
        assert_eq!(&c[0].px[1][..3], &[0.0, 0.0, 0.0]); // no flat default in model space
    }

    #[test]
    fn colour_and_data_mip_0_is_never_renormalised() {
        let img = Rgba32F::new(2, 1, [0.2, 0.2, 0.2, 1.0]);
        let c = build_chain(
            &work(img.clone(), Domain::Stored, false),
            &profile(Subtype::GenericData),
        );
        assert_eq!(c[0].px, img.px);
    }

    #[test]
    fn a_forced_cutout_does_not_alpha_weight_normals_or_roughness() {
        // An opaque +X and an invisible +Z: weighting would give +X, the vector chain averages both.
        let mut img = Rgba32F::new(2, 1, [0.0, 0.0, 1.0, 0.0]);
        img.px[0] = [1.0, 0.0, 0.0, 1.0];
        let mut p = profile(Subtype::Normal);
        p.alpha.policy = AlphaPolicy::Cutout;
        let c = build_chain(&work(img, Domain::Normal, true), &p);
        let v = c[1].px[0];
        assert!(
            (v[0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-5
                && (v[2] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-5,
            "{v:?}"
        );
        // Roughness: r = 1 opaque and r = 0 invisible average to sqrt(0.5) on alpha = r², not 1.
        let mut p = profile(Subtype::Rmaos);
        p.alpha.policy = AlphaPolicy::Cutout;
        let mut img = Rgba32F::new(2, 1, [0.0, 0.0, 0.0, 0.0]);
        img.px[0] = [1.0, 0.0, 0.0, 1.0];
        let w = Work {
            img,
            alpha_present: true,
            domain: Domain::Stored,
            roughness_r_squared: true,
            nonfinite_samples: 0,
        };
        let c = build_chain(&w, &p);
        assert!(
            (c[1].px[0][0] - 0.5f32.sqrt()).abs() < 1e-6,
            "{:?}",
            c[1].px[0]
        );
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

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-6
    }

    /// Red = 1 on one line (column `at` when `across_x`, else row `at`), 0 elsewhere; alpha 1.
    fn line_image(w: u32, h: u32, across_x: bool, at: u32) -> Rgba32F {
        let mut img = Rgba32F::new(w, h, [0.0, 0.0, 0.0, 1.0]);
        for y in 0..h {
            for x in 0..w {
                if (across_x && x == at) || (!across_x && y == at) {
                    img.at_mut(x, y)[0] = 1.0;
                }
            }
        }
        img
    }

    #[test]
    fn odd_width_centre_column_is_area_exact_at_every_level() {
        // 5x4 -> 2x2 -> 1x1. The centre column is split half and half between the two columns:
        // 0.5 / 2.5 = 0.2 everywhere (overlapping whole texels would give 1/3).
        let c = build_chain(
            &work(line_image(5, 4, true, 2), Domain::Stored, false),
            &profile(Subtype::GenericData),
        );
        assert_eq!(c.len(), 3);
        for l in &c[1..] {
            assert!(l.px.iter().all(|p| close(p[0], 0.2) && p[3] == 1.0));
        }
    }

    #[test]
    fn odd_height_centre_row_is_area_exact_at_every_level() {
        let c = build_chain(
            &work(line_image(4, 5, false, 2), Domain::Stored, false),
            &profile(Subtype::GenericData),
        );
        assert_eq!(c.len(), 3);
        for l in &c[1..] {
            assert!(l.px.iter().all(|p| close(p[0], 0.2) && p[3] == 1.0));
        }
    }

    #[test]
    fn odd_square_centre_texel_keeps_total_energy() {
        // 5x5 -> 2x2 -> 1x1: each child gets (0.5 * 0.5) / (2.5 * 2.5) = 0.04, and the mean of the
        // whole image is 1/25 = 0.04 at the last level.
        let mut img = Rgba32F::new(5, 5, [0.0, 0.0, 0.0, 1.0]);
        img.at_mut(2, 2)[0] = 1.0;
        let c = build_chain(
            &work(img, Domain::Stored, false),
            &profile(Subtype::GenericData),
        );
        assert_eq!(c.len(), 3);
        assert!(c[1].px.iter().all(|p| close(p[0], 0.04)));
        assert!(close(c[2].px[0][0], 0.04));
    }

    #[test]
    fn odd_width_plain_alpha_average_uses_area_weights() {
        // Alpha is data here (plain mean): a single opaque centre texel of 5 gives 0.2 per child.
        let mut img = Rgba32F::new(5, 1, [0.0; 4]);
        img.px[2][3] = 1.0;
        let mut p = profile(Subtype::GenericData);
        p.alpha.policy = AlphaPolicy::Data;
        let c = build_chain(&work(img, Domain::Stored, true), &p);
        assert!(close(c[1].px[0][3], 0.2) && close(c[1].px[1][3], 0.2));
        assert!(close(c[2].px[0][3], 0.2));
    }

    #[test]
    fn odd_width_alpha_weighted_rgb_uses_area_times_alpha() {
        let mut img = Rgba32F::new(5, 1, [0.0; 4]);
        for (i, (r, a)) in [
            (0.2f32, 1.0f32),
            (0.4, 0.5),
            (0.6, 1.0),
            (0.8, 0.25),
            (1.0, 0.0),
        ]
        .into_iter()
        .enumerate()
        {
            img.px[i] = [r, 0.0, 0.0, a];
        }
        let mut p = profile(Subtype::Diffuse);
        p.alpha.policy = AlphaPolicy::Cutout;
        let c = build_chain(&work(img, Domain::LinearLight, true), &p);
        // child 0 covers texels 0, 1 and half of 2; child 1 half of 2, then 3 and 4.
        let c0 = (0.2 * 1.0 + 0.4 * 0.5 + 0.6 * 0.5) / (1.0 + 0.5 + 0.5);
        let c1 = (0.6 * 0.5 + 0.8 * 0.25) / (0.5 + 0.25);
        assert!(close(c[1].px[0][0], c0) && close(c[1].px[1][0], c1));
        assert!(close(c[1].px[0][3], 2.0 / 2.5) && close(c[1].px[1][3], 0.75 / 2.5));
        // The last level equals the base-footprint weighted mean: sum(c a) / sum(a).
        let direct = (0.2 + 0.2 + 0.6 + 0.2) / 2.75;
        assert!(close(c[2].px[0][0], direct), "{}", c[2].px[0][0]);
        assert!(close(c[2].px[0][3], 2.75 / 5.0));
    }

    #[test]
    fn area_path_agrees_with_pair_path_on_even_sizes() {
        let mut seed = 7u32;
        let mut rnd = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f32 / 16_777_216.0
        };
        for (w, h) in [(8u32, 8u32), (16, 2), (1, 8), (6, 4)] {
            let mut img = Rgba32F::new(w, h, [0.0; 4]);
            for p in &mut img.px {
                *p = [rnd(), rnd(), rnd(), if rnd() < 0.3 { 0.0 } else { rnd() }];
            }
            for weighted in [false, true] {
                let a = downsample_area(&img, weighted);
                let b = downsample(&img, weighted);
                assert_eq!((a.width, a.height), (b.width, b.height));
                for (pa, pb) in a.px.iter().zip(&b.px) {
                    for c in 0..4 {
                        assert!((pa[c] - pb[c]).abs() < 1e-6, "{w}x{h} weighted={weighted}");
                    }
                }
            }
        }
    }

    #[test]
    fn weighted_chain_carries_child_alpha_as_weight() {
        // 4x1, alphas 1, 1, 0.25, 0 (the last colour is invisible and must not leak). Level 1 has
        // alphas 1 and 0.125, so the weights at level 2 are unequal: the result is
        // sum(c a) / sum(a) over the base, not the plain mean of the level-1 colours.
        let mut img = Rgba32F::new(4, 1, [0.0; 4]);
        img.px[0] = [1.0, 0.0, 0.0, 1.0];
        img.px[1] = [0.0, 1.0, 0.0, 1.0];
        img.px[2] = [0.0, 0.0, 1.0, 0.25];
        img.px[3] = [1.0, 1.0, 1.0, 0.0];
        let mut p = profile(Subtype::Diffuse);
        p.alpha.policy = AlphaPolicy::Cutout;
        let c = build_chain(&work(img, Domain::LinearLight, true), &p);
        let v = c[2].px[0];
        let (e0, e1, e2) = (1.0 / 2.25, 1.0 / 2.25, 0.25 / 2.25);
        assert!(
            close(v[0], e0) && close(v[1], e1) && close(v[2], e2),
            "{v:?}"
        );
        assert!(close(v[3], 2.25 / 4.0));
    }

    #[test]
    fn weighted_chain_matches_direct_footprint_mean_at_every_level() {
        let alphas = [1.0f32, 0.5, 0.0, 0.25, 0.75, 0.125, 1.0, 0.0];
        let reds = [0.9f32, 0.1, 0.7, 0.3, 0.5, 0.2, 0.8, 0.4];
        let mut img = Rgba32F::new(8, 1, [0.0; 4]);
        for i in 0..8 {
            img.px[i] = [reds[i], 0.0, 0.0, alphas[i]];
        }
        let mut p = profile(Subtype::Diffuse);
        p.alpha.policy = AlphaPolicy::Blend;
        let c = build_chain(&work(img, Domain::LinearLight, true), &p);
        assert_eq!(c.len(), 4);
        for (k, level) in c.iter().enumerate().skip(1) {
            let span = 1usize << k;
            for (i, px) in level.px.iter().enumerate() {
                let r = i * span..(i + 1) * span;
                let sa: f32 = alphas[r.clone()].iter().sum();
                let sca: f32 = r.clone().map(|j| reds[j] * alphas[j]).sum();
                assert!(close(px[3], sa / span as f32), "level {k} texel {i}");
                if sa > 0.0 {
                    assert!(close(px[0], sca / sa), "level {k} texel {i}");
                }
            }
        }
    }

    #[test]
    fn nan_source_sample_does_not_poison_the_chain() {
        let mut img = Rgba32F::new(4, 4, [1.0, 1.0, 1.0, 1.0]);
        img.px[0][0] = f32::NAN;
        let p = profile(Subtype::GenericData);
        let w = preprocess(img, &p);
        let c = build_chain(&w, &p);
        assert_eq!(c.len(), 3);
        for l in &c {
            assert!(l.px.iter().all(|p| p.iter().all(|v| v.is_finite())));
        }
        // NaN counts as 0: 15 of 16 base texels are white.
        assert!(close(c[2].px[0][0], 15.0 / 16.0), "{}", c[2].px[0][0]);
        assert_eq!(&c[2].px[0][1..3], &[1.0, 1.0]);
    }

    #[test]
    fn infinities_become_finite_before_accumulation() {
        // LDR: +inf -> 1, -inf -> 0.
        let mut img = Rgba32F::new(2, 1, [0.5, 0.5, 0.5, 1.0]);
        img.px[0][0] = f32::INFINITY;
        img.px[1][1] = f32::NEG_INFINITY;
        let mut p = profile(Subtype::GenericData);
        p.alpha.policy = AlphaPolicy::Data;
        let w = preprocess(img, &p);
        assert_eq!(&w.img.px[0][..3], &[1.0, 0.5, 0.5]);
        assert_eq!(&w.img.px[1][..3], &[0.5, 0.0, 0.5]);
        let c = build_chain(&w, &p);
        assert!(close(c[1].px[0][0], 0.75) && close(c[1].px[0][1], 0.25));
        // HDR: +inf -> 65504 (largest finite half), -inf -> 0.
        let mut p = profile(Subtype::PbrEmissive);
        p.color.content = ColorContent::Linear;
        let mut img = Rgba32F::new(2, 1, [1.0, 1.0, 1.0, 1.0]);
        img.px[0][0] = f32::INFINITY;
        img.px[1][1] = f32::NEG_INFINITY;
        let w = preprocess(img, &p);
        assert_eq!(w.img.px[0][0], 65504.0);
        assert_eq!(w.img.px[1][1], 0.0);
        let c = build_chain(&w, &p);
        assert!(close(c[1].px[0][0], (65504.0 + 1.0) / 2.0));
    }

    #[test]
    fn non_finite_vector_samples_decode_to_unit_extremes() {
        // Normal maps: -inf is the most negative component (-1 after decode), +inf is +1.
        let mut p = profile(Subtype::Normal);
        p.channels.reconstruct_z = false;
        p.channels.flip_green = false;
        let w = preprocess(
            Rgba32F::new(1, 1, [f32::NEG_INFINITY, f32::INFINITY, f32::NAN, 1.0]),
            &p,
        );
        assert_eq!(w.img.px[0][0], -1.0);
        assert_eq!(w.img.px[0][1], 1.0);
        assert_eq!(w.img.px[0][2], 0.0); // NaN -> 0 -> decoded -1 -> z clamps to 0
        assert!(w.img.px[0][3].is_finite());
        // Flow: only RG are vector channels.
        let mut p = profile(Subtype::Flow);
        p.alpha.policy = AlphaPolicy::Data;
        let w = preprocess(
            Rgba32F::new(1, 1, [f32::NEG_INFINITY, f32::INFINITY, 0.5, 1.0]),
            &p,
        );
        assert_eq!(&w.img.px[0][..2], &[-1.0, 1.0]);
    }

    #[test]
    fn non_finite_alpha_is_sanitised_when_alpha_is_kept() {
        let mut p = profile(Subtype::Diffuse);
        p.alpha.policy = AlphaPolicy::Blend;
        let mut img = Rgba32F::new(4, 1, [0.5, 0.5, 0.5, 0.5]);
        img.px[0][3] = f32::NAN;
        img.px[1][3] = f32::INFINITY;
        img.px[2][3] = f32::NEG_INFINITY;
        let w = preprocess(img, &p);
        assert!(w.alpha_present);
        let a: Vec<f32> = w.img.px.iter().map(|p| p[3]).collect();
        assert_eq!(a, [0.0, 1.0, 0.0, 0.5]);
        let c = build_chain(&w, &p);
        assert!(c
            .iter()
            .all(|l| l.px.iter().all(|p| p.iter().all(|v| v.is_finite()))));
    }

    #[test]
    fn hdr_negatives_are_clamped_before_accumulation() {
        // Without the base clamp mip 1 would be (-4 + 8) / 2 = 2 while mip 0 encodes as 0 and 8.
        let mut p = profile(Subtype::PbrEmissive);
        p.color.content = ColorContent::Linear;
        let mut img = Rgba32F::new(2, 1, [-4.0, 1.0, 1.0, 1.0]);
        img.px[1][0] = 8.0;
        let w = preprocess(img, &p);
        assert_eq!(w.domain, Domain::Hdr);
        let c = build_chain(&w, &p);
        assert_eq!((c[0].px[0][0], c[0].px[1][0]), (0.0, 8.0));
        assert!(close(c[1].px[0][0], 4.0));
    }

    /// A `w`×`h` image of LCG noise in [0, 1) on every channel.
    fn rnd_image(w: u32, h: u32, seed: u32) -> Rgba32F {
        let mut seed = seed;
        let mut rnd = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f32 / 16_777_216.0
        };
        let mut img = Rgba32F::new(w, h, [0.0; 4]);
        for p in &mut img.px {
            *p = [rnd(), rnd(), rnd(), rnd()];
        }
        img
    }

    #[test]
    fn box_is_the_default_and_unchanged() {
        assert_eq!(profile(Subtype::Diffuse).mips.filter, MipFilter::Box);
        // The pre-M6 chain is restated by the existing tests; this asserts the non-box branch is not taken.
        let w = work(rnd_image(8, 8, 1), Domain::LinearLight, false);
        let p = profile(Subtype::Diffuse);
        assert_eq!(effective_filter(&w, &p), MipFilter::Box);
    }

    #[test]
    fn non_box_levels_are_resampled_directly_from_mip_0() {
        for f in [MipFilter::Tent, MipFilter::Mitchell, MipFilter::Kaiser] {
            let mut p = profile(Subtype::Diffuse);
            p.alpha.policy = AlphaPolicy::Remove;
            p.mips.filter = f;
            let img = rnd_image(16, 16, 7);
            let c = build_chain(&work(img.clone(), Domain::LinearLight, false), &p);
            for (k, level) in c.iter().enumerate().skip(1) {
                let d = crate::filter::resample_rgb(
                    &img,
                    (level.width, level.height),
                    f,
                    MipEdge::Wrap,
                );
                for (a, b) in level.px.iter().zip(&d.px) {
                    for ch in 0..3 {
                        assert!(
                            (a[ch] - b[ch].clamp(0.0, 1.0)).abs() < 1e-6,
                            "{f:?} level {k}"
                        );
                    }
                }
            }
            // Not recursive: resampling level 1 again gives a different level 2.
            let rec =
                crate::filter::resample_rgb(&c[1], (c[2].width, c[2].height), f, MipEdge::Wrap);
            let diff = c[2]
                .px
                .iter()
                .zip(&rec.px)
                .map(|(a, b)| (a[0] - b[0]).abs())
                .fold(0.0f32, f32::max);
            assert!(diff > 1e-4, "{f:?}: {diff}");
        }
    }

    #[test]
    fn non_box_alpha_stays_the_box_alpha_bit_for_bit() {
        let img = rnd_image(16, 16, 3); // random alpha
        let mut bx = profile(Subtype::Diffuse);
        bx.alpha.policy = AlphaPolicy::Data;
        let mut k = bx.clone();
        k.mips.filter = MipFilter::Kaiser;
        let w = work(img, Domain::LinearLight, true);
        let (a, b) = (build_chain(&w, &bx), build_chain(&w, &k));
        for (la, lb) in a.iter().zip(&b) {
            assert!(la.px.iter().zip(&lb.px).all(|(x, y)| x[3] == y[3]));
        }
        assert_ne!(a[1].px, b[1].px); // RGB did change
    }

    #[test]
    fn kaiser_overshoot_is_clamped_in_linear_light() {
        // A one-texel white spike on black: Kaiser rings below 0 next to it at level 1.
        let mut img = Rgba32F::new(16, 16, [0.0, 0.0, 0.0, 1.0]);
        img.at_mut(8, 8)[..3].copy_from_slice(&[1.0, 1.0, 1.0]);
        let mut p = profile(Subtype::Diffuse);
        p.alpha.policy = AlphaPolicy::Remove;
        p.mips.filter = MipFilter::Kaiser;
        let c = build_chain(&work(img.clone(), Domain::LinearLight, false), &p);
        let raw = crate::filter::resample_rgb(&img, (8, 8), MipFilter::Kaiser, MipEdge::Wrap);
        assert!(raw.px.iter().any(|q| q[0] < 0.0), "the fixture must ring");
        assert!(c
            .iter()
            .skip(1)
            .all(|l| l.px.iter().all(|q| (0.0..=1.0).contains(&q[0]))));
    }

    #[test]
    fn hdr_tent_keeps_highlights_and_never_goes_negative() {
        // bc6h_uf16 and rgba16f are both HDR formats (F2); rgba16f keeps alpha, so it is removed here.
        for fmt in [Format::Bc6hUf16, Format::Rgba16f] {
            let mut img = Rgba32F::new(8, 8, [0.1, 0.1, 0.1, 1.0]);
            img.at_mut(3, 3)[0] = 100.0;
            let mut p = profile(Subtype::PbrEmissive);
            p.format = fmt;
            p.alpha.policy = AlphaPolicy::Remove;
            p.mips.filter = MipFilter::Tent;
            let w = work(img, Domain::Hdr, false);
            assert_eq!(effective_filter(&w, &p), MipFilter::Tent, "{fmt:?}");
            let c = build_chain(&w, &p);
            assert!(c[1].px.iter().any(|q| q[0] > 1.0), "{fmt:?}");
            assert!(
                c.iter().all(|l| l.px.iter().all(|q| q[0] >= 0.0)),
                "{fmt:?}"
            );
        }
    }

    #[test]
    fn box_only_contents_ignore_a_non_box_filter() {
        // A blocked profile (CFG-INVALID) never runs, but the chain itself must still be the box chain.
        for (st, dom) in [
            (Subtype::Normal, Domain::Normal),
            (Subtype::Msn, Domain::Msn),
            (Subtype::Flow, Domain::SignedRg),
            (Subtype::GenericData, Domain::Stored),
        ] {
            let img = rnd_image(8, 8, 5);
            let p = profile(st);
            let mut k = p.clone();
            k.mips.filter = MipFilter::Kaiser;
            let w = work(img, dom, true);
            assert_eq!(build_chain(&w, &p), build_chain(&w, &k), "{st:?}");
        }
        // Alpha-weighted colour.
        let mut p = profile(Subtype::Diffuse);
        p.alpha.policy = AlphaPolicy::Cutout;
        let mut k = p.clone();
        k.mips.filter = MipFilter::Kaiser;
        let w = work(rnd_image(8, 8, 9), Domain::LinearLight, true);
        assert_eq!(build_chain(&w, &p), build_chain(&w, &k));
    }

    #[test]
    fn baked_roughness_is_identical_under_any_filter_setting() {
        // A roughness member is box-only: build_chain_with + a NormalVariance gives equal chains for box and kaiser.
        // Fixture restated from `bake::tests` (`bumpy` normals, `ramp` roughness, written mip 0).
        use crate::bake::LengthPyramid;
        use crate::ops::preprocess;
        let mut normals = Rgba32F::new(16, 16, [0.0, 0.0, 0.0, 1.0]);
        for y in 0..16 {
            for x in 0..16 {
                let nx = 0.6 * (x as f32 * 1.3 + y as f32 * 0.7).sin();
                let ny = 0.6 * (x as f32 * 0.9 - y as f32 * 1.1).cos();
                let nz = (1.0 - nx * nx - ny * ny).sqrt();
                *normals.at_mut(x, y) = [nx * 0.5 + 0.5, ny * 0.5 + 0.5, nz * 0.5 + 0.5, 1.0];
            }
        }
        let nw = preprocess(normals, &profile(Subtype::PbrNormal));
        let mut written = nw.img;
        for px in &mut written.px {
            normalize_vector(px, nw.domain);
        }
        let mut rough = Rgba32F::new(16, 16, [0.0, 0.0, 0.8, 1.0]);
        for y in 0..16 {
            for x in 0..16 {
                rough.at_mut(x, y)[0] = 0.15 + 0.7 * ((x * 3 + y * 2) % 16) as f32 / 16.0;
            }
        }
        let pr = profile(Subtype::Rmaos);
        let rw = preprocess(rough, &pr);
        let nv = NormalVariance::new(LengthPyramid::build(&written), (16, 16), 0.25).unwrap();
        let mut pk = pr.clone();
        pk.mips.filter = MipFilter::Kaiser;
        let baked = build_chain_with(&rw, &pr, Some(&nv));
        assert_ne!(baked, build_chain(&rw, &pr)); // the bake is live
        assert_eq!(baked, build_chain_with(&rw, &pk, Some(&nv)));
    }

    #[test]
    fn thin_lines_are_exact_for_box_and_tent() {
        // B8 "Thin lines: box and tent are exact either way": a 1-texel line every 4 columns of a 16x16 image;
        // level 2 (4x4) is 0.25 everywhere for box and for tent (wrap).
        let mut img = Rgba32F::new(16, 16, [0.0, 0.0, 0.0, 1.0]);
        for y in 0..16 {
            for x in (0..16).step_by(4) {
                img.at_mut(x, y)[0] = 1.0;
            }
        }
        for f in [MipFilter::Box, MipFilter::Tent] {
            let mut p = profile(Subtype::Diffuse);
            p.alpha.policy = AlphaPolicy::Remove;
            p.mips.filter = f;
            let w = work(img.clone(), Domain::LinearLight, false);
            assert_eq!(effective_filter(&w, &p), f);
            let c = build_chain(&w, &p);
            assert_eq!((c[2].width, c[2].height), (4, 4));
            assert!(
                c[2].px.iter().all(|q| close(q[0], 0.25)),
                "{f:?}: {:?}",
                c[2].px
            );
        }
    }
}
