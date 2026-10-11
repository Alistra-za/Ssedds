//! Coverage-preserving alpha mips (spec §5.5).
//!
//! Rebuilds the idea of AlphaBloom's "Auto Coverage" (tkRiverGamer) and Ignacio Castaño's
//! coverage-preserving mipmap method; see `docs/CREDITS.md`. No code is copied.
//!
//! Level 0 defines the target: the fraction `p` of texels whose 8-bit alpha code passes the
//! alpha test. Each lower level is remapped so that exactly `round(p · n)` of its texels pass.
//! The counter ([`coverage_fraction`]) and the solver
//! ([`preserve_coverage`]) share [`passes`] and the engine's [`code8`] rounding, so they
//! cannot disagree.
//!
//! - **Boundary.** `T` is the smallest f32 whose code passes (found by search, not the code
//!   centre), so only the texels at or above the anchor pass.
//! - **Ties, dithered (owner 2026-10-11, DECISIONS R13).** On thin strands every texel touching
//!   a strand has the same alpha, so whole tied groups cannot reach the target (mipcheck round
//!   2, B1: up to 32x over). Each level instead passes exactly `k = round(p · n)` texels: those
//!   above the k-th largest alpha `v`, plus as many texels tied at `v` as are still needed,
//!   chosen in 8x8 Bayer order of their position. That is deterministic, with no RNG, and spreads
//!   the passing texels along the strand, so foliage and grass keep their coverage at the low
//!   mips players mostly see. Zero alpha is never raised.
//! - **Remap shape (deviation from Castaño).** Castaño scales alpha uniformly by `T/a`, which
//!   halves opaque interiors and breaks `cutout_blend`. Here passing texels map monotonically
//!   onto `[T_pass, 1]` (1.0 stays 1.0) and failing ones onto `[0, T_fail]`, each side keeping
//!   the order of its alphas; only the tied boundary group is split (controller ruling, owner
//!   check; spec §5.5 note).
//! - **`k = 0`.** A level whose target rounds to zero is left unchanged (spec). Its "within
//!   one texel" result relies on the level being a box-filtered descendant of level 0: a box
//!   average passes only if its footprint holds a passing base texel, so ordinary content
//!   stays near `p · n`. It is not a hard bound: failing alpha just under the threshold around
//!   sparse passing texels can lift several footprints at once.
//! - **Task 37 (wiring).** For `cutout_blend` this rewrites the only alpha channel in place:
//!   the blended edge is the remapped alpha, not the box average.
use crate::analyse::code8;
use crate::config::AlphaCompare;
use crate::image::Rgba32F;

/// Does the 8-bit alpha `code` pass an alpha test with `threshold` under `compare`?
pub fn passes(code: u8, threshold: u8, compare: AlphaCompare) -> bool {
    match compare {
        AlphaCompare::Gt => code > threshold,
        AlphaCompare::Ge => code >= threshold,
    }
}

/// Fraction of `alpha` values (in `[0, 1]`) whose rounded 8-bit code passes the test.
/// An empty iterator yields 0.
pub fn coverage_fraction(
    alpha: impl Iterator<Item = f32>,
    threshold: u8,
    compare: AlphaCompare,
) -> f64 {
    let (mut n, mut pass) = (0u64, 0u64);
    for a in alpha {
        n += 1;
        pass += u64::from(passes(code8(a), threshold, compare));
    }
    if n == 0 {
        0.0
    } else {
        pass as f64 / n as f64
    }
}

/// The smallest `x` in `[0, 1]` whose 8-bit code passes, or `None` when no code passes
/// (`gt 255`). `passes(code8(x))` is monotone in `x`, and non-negative f32 bit patterns order
/// like their values, so a binary search over the bits finds the exact boundary.
fn first_passing(threshold: u8, compare: AlphaCompare) -> Option<f32> {
    let ok = |bits: u32| passes(code8(f32::from_bits(bits)), threshold, compare);
    let (mut lo, mut hi) = (0u32, 1.0f32.to_bits());
    if !ok(hi) {
        return None;
    }
    if ok(lo) {
        return Some(0.0);
    }
    // Invariant: `lo` fails, `hi` passes.
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if ok(mid) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    Some(f32::from_bits(hi))
}

/// Alpha as the solver sees it: clamped to `[0, 1]`, with NaN, negatives and values below
/// `f32::MIN_POSITIVE` treated as 0 (they all have code 0, and dividing by them is unsafe).
fn unit_alpha(x: f32) -> f32 {
    if x >= f32::MIN_POSITIVE {
        x.min(1.0)
    } else {
        0.0
    }
}

/// Rank of texel (x, y) in the 8x8 ordered-dither (Bayer) matrix, 0..64: the order in which texels tied at the
/// coverage boundary are let through (owner 2026-10-11, DECISIONS R13). The bit-reversed interleave of `x ^ y` and
/// `y`; deterministic, no RNG.
fn bayer8(x: u32, y: u32) -> u32 {
    let (a, b) = ((x ^ y) & 7, y & 7);
    ((a & 1) << 5)
        | ((b & 1) << 4)
        | ((a & 2) << 2)
        | ((b & 2) << 1)
        | ((a & 4) >> 1)
        | ((b & 4) >> 2)
}

/// Which texels pass so that exactly `k` do: every texel above the k-th largest alpha `v`, plus as many texels tied
/// at `v` as are still needed, in hierarchical Bayer order of their position (ties by index). Zero alpha is never raised, so with
/// fewer than `k` positive texels every positive one passes; `None` when none is positive. `w` is the level's alpha
/// as [`unit_alpha`] reads it, row-major with `width` texels per row.
fn exact_pass_set(w: &[f32], width: u32, k: usize) -> Option<Vec<bool>> {
    let positive = w.iter().filter(|&&a| a > 0.0).count();
    let k = k.min(positive);
    if k == 0 {
        return None;
    }
    let mut scratch = w.to_vec();
    let (_, &mut v, _) = scratch.select_nth_unstable_by(k - 1, |x, y| y.total_cmp(x));
    let mut pass: Vec<bool> = w.iter().map(|&a| a > v).collect();
    let above = pass.iter().filter(|&&p| p).count();
    let mut tied: Vec<usize> = (0..w.len()).filter(|&i| w[i] == v).collect();
    // Hierarchical Bayer: the 8x8 rank first, then the rank of the 8x8 block, then of the 64x64 block, so a partial
    // rank class is spread over the whole level (on a grid-aligned tied set the fine rank alone has only 8 values
    // and the index tie-break would fill it top rows first).
    tied.sort_by_cached_key(|&i| {
        let (x, y) = (i as u32 % width, i as u32 / width);
        (
            bayer8(x, y),
            bayer8(x >> 3, y >> 3),
            bayer8(x >> 6, y >> 6),
            i,
        )
    });
    for &i in tied.iter().take(k - above) {
        pass[i] = true;
    }
    Some(pass)
}

/// Rewrite alpha so exactly the texels in `pass` pass. Passing texels map monotonically onto `[t_pass, 1]` (the
/// smallest goes to `t_pass`, 1.0 stays 1.0); failing ones onto `[0, t_fail]` (the largest goes to `t_fail`). Each
/// side keeps the order of its alphas.
fn remap(level: &mut Rgba32F, w: &[f32], pass: &[bool], t_pass: f32, t_fail: f32) {
    let lo_pass = w
        .iter()
        .zip(pass)
        .filter(|(_, p)| **p)
        .map(|(a, _)| *a)
        .fold(1.0f32, f32::min);
    let hi_fail = w
        .iter()
        .zip(pass)
        .filter(|(_, p)| !**p)
        .map(|(a, _)| *a)
        .fold(0.0f32, f32::max);
    for (q, (&a, &p)) in level.px.iter_mut().zip(w.iter().zip(pass)) {
        q[3] = if p {
            if a >= 1.0 {
                1.0
            } else {
                // a >= lo_pass and a < 1, so lo_pass < 1; the result is >= t_pass, and a = lo_pass gives t_pass.
                (t_pass + (a - lo_pass) * ((1.0 - t_pass) / (1.0 - lo_pass))).min(1.0)
            }
        } else if a <= 0.0 {
            0.0 // zero alpha stays zero (invisible texels are never lifted toward the threshold)
        } else if a >= hi_fail {
            t_fail // the largest failing alpha (an unchosen tied texel included) lands exactly on t_fail
        } else {
            // 0 < a < hi_fail here; capped so f32 rounding cannot lift it past t_fail.
            (a * (t_fail / hi_fail)).min(t_fail)
        };
    }
}

/// Rewrite the alpha of every level `>= 1` so it passes exactly `k = round(p · n)` texels, `p` the pass fraction of
/// level 0 (the reference, left untouched): the texels above the k-th largest alpha, plus texels tied with it in
/// Bayer order of position (owner 2026-10-11, DECISIONS R13). Passing texels are remapped onto `[T_pass, 1]` and
/// failing ones onto `[0, T_fail]`. A level is left unchanged when `k = 0`, when it already passes `k`, or when it has
/// no positive alpha; zero alpha is never raised, so a level with fewer than `k` positive texels passes all of them.
pub fn preserve_coverage(chain: &mut [Rgba32F], threshold: u8, compare: AlphaCompare) {
    let Some((base, rest)) = chain.split_first_mut() else {
        return;
    };
    // No passing code (`gt 255`): p = 0. Every code passes (`ge 0`): every level is full.
    let Some(t) = first_passing(threshold, compare) else {
        return;
    };
    if t == 0.0 {
        return;
    }
    // Where remapped texels land: passing ones at or above `t_pass`, failing ones at or below `t_fail` (Task 2 moves
    // both clear of the threshold).
    let (t_pass, t_fail) = (t, t.next_down());
    let p = coverage_fraction(base.px.iter().map(|q| q[3]), threshold, compare);
    let pass_count = |l: &Rgba32F| {
        l.px.iter()
            .filter(|q| passes(code8(q[3]), threshold, compare))
            .count()
    };
    for level in rest {
        let n = level.px.len();
        let k = ((p * n as f64).round() as usize).min(n);
        // "No texel passes" is the target: the box alpha is left alone (spec §5.5). So is a level already on target.
        if k == 0 || pass_count(level) == k {
            continue;
        }
        let w: Vec<f32> = level.px.iter().map(|q| unit_alpha(q[3])).collect();
        // `None`: no positive alpha to let through, so the dither cannot apply and the box alpha stays.
        let Some(pass) = exact_pass_set(&w, level.width, k) else {
            continue;
        };
        remap(level, &w, &pass, t_pass, t_fail);
        debug_assert_eq!(pass_count(level), pass.iter().filter(|&&b| b).count());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AlphaCompare;
    use crate::image::Rgba32F;

    fn alpha_of(img: &Rgba32F) -> impl Iterator<Item = f32> + '_ {
        img.px.iter().map(|p| p[3])
    }

    /// 64x64 sparse "foliage": alpha = u^4 for a deterministic uniform u (continuous values, so
    /// every count is achievable). Box-filtered down.
    fn foliage_chain() -> Vec<Rgba32F> {
        let mut base = Rgba32F::new(64, 64, [0.0; 4]);
        let mut s: u32 = 12345;
        for p in &mut base.px {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let u = (s >> 8) as f32 / (1u32 << 24) as f32;
            p[3] = u * u * u * u;
        }
        box_chain(base)
    }

    /// Box-filter `base` down to 1x1 (power-of-two sizes), as `mips.rs` does for alpha.
    fn box_chain(base: Rgba32F) -> Vec<Rgba32F> {
        let mut chain = vec![base];
        while chain.last().unwrap().width > 1 {
            let p = chain.last().unwrap();
            let mut n = Rgba32F::new(p.width / 2, p.height / 2, [0.0; 4]);
            for y in 0..n.height {
                for x in 0..n.width {
                    let a = (p.at(2 * x, 2 * y)[3]
                        + p.at(2 * x + 1, 2 * y)[3]
                        + p.at(2 * x, 2 * y + 1)[3]
                        + p.at(2 * x + 1, 2 * y + 1)[3])
                        / 4.0;
                    n.at_mut(x, y)[3] = a;
                }
            }
            chain.push(n);
        }
        chain
    }

    #[test]
    fn comparisons() {
        assert!(passes(128, 127, AlphaCompare::Gt) && !passes(127, 127, AlphaCompare::Gt));
        assert!(passes(127, 127, AlphaCompare::Ge));
    }

    #[test]
    fn without_preservation_sparse_foliage_vanishes() {
        let c = foliage_chain();
        let base = coverage_fraction(alpha_of(&c[0]), 127, AlphaCompare::Gt);
        assert!((base - 0.159).abs() < 0.02, "base coverage {base}"); // P(u^4 > 0.5) = 1 - 0.5^0.25
        assert!(coverage_fraction(alpha_of(&c[3]), 127, AlphaCompare::Gt) < 0.05);
    }

    /// Test-local walk: the smallest f32 in [0, 1] whose 8-bit code passes, stepping one ulp
    /// at a time from the ideal transition `(c - 0.5) / 255` of the first passing code `c`.
    fn first_passing_float(threshold: u8, compare: AlphaCompare) -> f32 {
        let ok = |x: f32| passes(code8(x), threshold, compare);
        let c = (0..=255u8)
            .find(|&c| passes(c, threshold, compare))
            .unwrap();
        let mut x = ((f32::from(c) - 0.5) / 255.0).max(0.0);
        while x > 0.0 && ok(x) {
            x = x.next_down();
        }
        while !ok(x) {
            x = x.next_up();
        }
        x
    }

    fn pass_count(img: &Rgba32F, threshold: u8, compare: AlphaCompare) -> usize {
        alpha_of(img)
            .filter(|&a| passes(code8(a), threshold, compare))
            .count()
    }

    /// Every level >= 1 passes within one texel of `p * n`, `p` the level-0 pass fraction.
    fn assert_within_one_texel(chain: &[Rgba32F], threshold: u8, compare: AlphaCompare) {
        let p = coverage_fraction(alpha_of(&chain[0]), threshold, compare);
        for l in &chain[1..] {
            let want = p * l.px.len() as f64;
            let got = pass_count(l, threshold, compare) as f64;
            assert!(
                (got - want).abs() <= 1.0,
                "{threshold} {compare:?} {}x{}: {got} passing vs p*n = {want}",
                l.width,
                l.height
            );
        }
    }

    #[test]
    fn foliage_every_level_within_one_texel() {
        let mut c = foliage_chain();
        preserve_coverage(&mut c, 127, AlphaCompare::Gt);
        assert_within_one_texel(&c, 127, AlphaCompare::Gt);
    }

    #[test]
    fn foliage_low_and_other_thresholds_within_one_texel() {
        for (threshold, compare) in [
            (5, AlphaCompare::Gt),
            (5, AlphaCompare::Ge),
            (20, AlphaCompare::Gt),
            (64, AlphaCompare::Gt),
            (127, AlphaCompare::Ge),
            (200, AlphaCompare::Ge),
        ] {
            let mut c = foliage_chain();
            preserve_coverage(&mut c, threshold, compare);
            assert_within_one_texel(&c, threshold, compare);
        }
    }

    #[test]
    fn near_boundary_values_below_the_quantile_do_not_pass() {
        // Codex counterexample: a 4x4 base with p = 4/16 whose box mip has four distinct
        // alphas just around 0.5. Only the largest may pass at level 1.
        let mut base = Rgba32F::new(4, 4, [0.0; 4]);
        let blocks = [
            [1.0, 1.0, 0.0, 0.0],
            [1.0, 0.999, 0.0, 0.0],
            [0.499; 4],
            [0.4985; 4],
        ];
        for (b, vals) in (0u32..).zip(&blocks) {
            let (bx, by) = (2 * (b % 2), 2 * (b / 2));
            for (i, &v) in (0u32..).zip(vals) {
                base.at_mut(bx + i % 2, by + i / 2)[3] = v;
            }
        }
        let mut c = box_chain(base);
        let mut lvl1: Vec<f32> = alpha_of(&c[1]).collect();
        lvl1.sort_by(f32::total_cmp);
        lvl1.dedup();
        assert_eq!(lvl1.len(), 4, "level 1 alphas must be distinct: {lvl1:?}");
        assert_eq!(
            coverage_fraction(alpha_of(&c[0]), 127, AlphaCompare::Gt),
            0.25
        );
        preserve_coverage(&mut c, 127, AlphaCompare::Gt);
        assert_eq!(pass_count(&c[1], 127, AlphaCompare::Gt), 1);
    }

    #[test]
    fn bayer8_is_the_standard_8x8_ordered_dither() {
        let mut seen = [false; 64];
        for y in 0..8 {
            for x in 0..8 {
                seen[bayer8(x, y) as usize] = true;
                assert_eq!(bayer8(x, y), bayer8(x + 8, y + 16), "the matrix tiles");
            }
        }
        assert!(seen.iter().all(|&s| s), "a permutation of 0..64");
        let row0: Vec<u32> = (0..8).map(|x| bayer8(x, 0)).collect();
        assert_eq!(row0, [0, 32, 8, 40, 2, 34, 10, 42]);
        assert_eq!(bayer8(0, 1), 48);
    }

    #[test]
    fn binary_tie_lets_exactly_k_through_in_bayer_order() {
        // One opaque texel per 2x2 footprint: level 1 is four tied 0.25s and k = 1. The old solver passed all four;
        // now exactly one passes, the texel with the lowest Bayer rank, (0, 0).
        let mut base = Rgba32F::new(4, 4, [0.0; 4]);
        for (x, y) in [(0, 0), (2, 0), (0, 2), (2, 2)] {
            base.at_mut(x, y)[3] = 1.0;
        }
        let mut c = box_chain(base);
        assert!(alpha_of(&c[1]).all(|a| a == 0.25));
        preserve_coverage(&mut c, 127, AlphaCompare::Gt);
        assert_eq!(pass_count(&c[1], 127, AlphaCompare::Gt), 1);
        assert!(passes(code8(c[1].px[0][3]), 127, AlphaCompare::Gt));
    }

    #[test]
    fn mipcheck_b1_three_texel_repro_gets_exactly_k() {
        // RESULTS-2 B1, smallest reproduction: alpha 255 at (0,0), (0,2), (2,0). Level 1 holds three tied 0.25s and
        // k = round(3/16 * 4) = 1; the old solver passed all three.
        let mut base = Rgba32F::new(4, 4, [0.0; 4]);
        for (x, y) in [(0, 0), (0, 2), (2, 0)] {
            base.at_mut(x, y)[3] = 1.0;
        }
        let mut c = box_chain(base);
        preserve_coverage(&mut c, 128, AlphaCompare::Ge);
        assert_eq!(pass_count(&c[1], 128, AlphaCompare::Ge), 1);
    }

    #[test]
    fn ties_are_split_in_bayer_order_to_reach_k_exactly() {
        // Footprints with 3, 1, 1, 1 opaque texels: level 1 = [0.75, 0.25, 0.25, 0.25], k = round(1.5) = 2. The 0.75
        // passes, and one of the three tied 0.25s: (1, 1) has the lowest Bayer rank of (1,0), (0,1), (1,1).
        let mut base = Rgba32F::new(4, 4, [0.0; 4]);
        for (x, y) in [(0, 0), (1, 0), (0, 1), (2, 0), (0, 2), (2, 2)] {
            base.at_mut(x, y)[3] = 1.0;
        }
        let mut c = box_chain(base);
        preserve_coverage(&mut c, 127, AlphaCompare::Gt);
        assert_eq!(pass_count(&c[1], 127, AlphaCompare::Gt), 2);
        let pass: Vec<bool> = alpha_of(&c[1])
            .map(|a| passes(code8(a), 127, AlphaCompare::Gt))
            .collect();
        assert_eq!(pass, [true, false, false, true]);
    }

    /// 64x64 with a one-texel vertical strand every 16 columns (p = 1/16): every level's alphas are tied.
    fn strands() -> Vec<Rgba32F> {
        let mut base = Rgba32F::new(64, 64, [0.0; 4]);
        for y in 0..64 {
            for x in (0..64).step_by(16) {
                base.at_mut(x, y)[3] = 1.0;
            }
        }
        box_chain(base)
    }

    #[test]
    fn thin_tied_strands_keep_coverage_within_one_texel_down_to_4x4() {
        let mut c = strands();
        preserve_coverage(&mut c, 127, AlphaCompare::Gt);
        let p = coverage_fraction(alpha_of(&c[0]), 127, AlphaCompare::Gt);
        for l in c.iter().skip(1).filter(|l| l.px.len() >= 16) {
            let k = p * l.px.len() as f64;
            let got = pass_count(l, 127, AlphaCompare::Gt) as f64;
            assert!(
                (got - k).abs() <= 1.0,
                "{}x{}: {got} passing for k = {k}",
                l.width,
                l.height
            );
        }
    }

    #[test]
    fn the_dither_is_deterministic() {
        let (mut a, mut b) = (strands(), strands());
        preserve_coverage(&mut a, 127, AlphaCompare::Gt);
        preserve_coverage(&mut b, 127, AlphaCompare::Gt);
        assert_eq!(a, b);
        let (mut a, mut b) = (foliage_chain(), foliage_chain());
        preserve_coverage(&mut a, 64, AlphaCompare::Ge);
        preserve_coverage(&mut b, 64, AlphaCompare::Ge);
        assert_eq!(a, b);
    }

    #[test]
    fn foliage_every_level_reaches_k_exactly() {
        let mut c = foliage_chain();
        preserve_coverage(&mut c, 127, AlphaCompare::Gt);
        let p = coverage_fraction(alpha_of(&c[0]), 127, AlphaCompare::Gt);
        for l in &c[1..] {
            let k = ((p * l.px.len() as f64).round() as usize).min(l.px.len());
            if k > 0 {
                assert_eq!(
                    pass_count(l, 127, AlphaCompare::Gt),
                    k,
                    "{}x{}",
                    l.width,
                    l.height
                );
            }
        }
    }

    #[test]
    fn opaque_interior_stays_opaque() {
        // 8x8 base, Gt 127: an opaque 4x4 block, the rest at 0.4, so p = 16/64. Level 1's top
        // group is exactly 1.0 and already has the target count: it must stay 1.0.
        let mut base = Rgba32F::new(8, 8, [0.0, 0.0, 0.0, 0.4]);
        for y in 0..4 {
            for x in 0..4 {
                base.at_mut(x, y)[3] = 1.0;
            }
        }
        let mut c = box_chain(base);
        preserve_coverage(&mut c, 127, AlphaCompare::Gt);
        assert_eq!(alpha_of(&c[1]).filter(|&a| a == 1.0).count(), 4);
        assert_eq!(pass_count(&c[1], 127, AlphaCompare::Gt), 4);

        // Same block plus two 3-of-4 footprints, Gt 200: k = round(22/4) = 6 selects the 0.75
        // group, which is remapped up while the opaque interior keeps exactly 1.0.
        let mut base = Rgba32F::new(8, 8, [0.0; 4]);
        for y in 0..4 {
            for x in 0..4 {
                base.at_mut(x, y)[3] = 1.0;
            }
        }
        for (x, y) in [(4, 0), (5, 0), (4, 1), (4, 4), (5, 4), (4, 5)] {
            base.at_mut(x, y)[3] = 1.0;
        }
        let mut c = box_chain(base);
        preserve_coverage(&mut c, 200, AlphaCompare::Gt);
        assert_eq!(alpha_of(&c[1]).filter(|&a| a == 1.0).count(), 4);
        assert_eq!(pass_count(&c[1], 200, AlphaCompare::Gt), 6);
    }

    #[test]
    fn opaque_anchor_with_lower_passing_group_demotes_the_lower_group() {
        // Level 1 (4x4): 3 x 1.0, 5 x 0.9, 8 x 0.2. Base p = 3/16, so k = 3 and the nearest
        // achievable count is the 1.0 group. The 0.9s already pass, so leaving the level alone
        // would over-cover (8). They must drop to the largest failing float, 1.0 stays 1.0.
        for compare in [AlphaCompare::Gt, AlphaCompare::Ge] {
            let mut base = Rgba32F::new(4, 4, [0.0; 4]);
            for x in 0..3 {
                base.at_mut(x, 0)[3] = 1.0;
            }
            let mut lvl1 = Rgba32F::new(4, 4, [0.0; 4]);
            for (i, p) in lvl1.px.iter_mut().enumerate() {
                p[3] = match i {
                    0..=2 => 1.0,
                    3..=7 => 0.9,
                    _ => 0.2,
                };
            }
            assert_eq!(pass_count(&lvl1, 127, compare), 8);
            let mut c = vec![base, lvl1];
            preserve_coverage(&mut c, 127, compare);
            let t_fail = first_passing_float(127, compare).next_down();
            assert_eq!(pass_count(&c[1], 127, compare), 3, "{compare:?}");
            for (i, p) in c[1].px.iter().enumerate() {
                match i {
                    0..=2 => assert_eq!(p[3], 1.0),
                    3..=7 => assert_eq!(p[3], t_fail, "{compare:?}"),
                    _ => assert!(p[3] > 0.0 && p[3] < t_fail, "order kept: {}", p[3]),
                }
            }
        }
    }

    #[test]
    fn boundary_texel_lands_on_the_first_passing_float() {
        for (compare, first_code) in [(AlphaCompare::Gt, 128u8), (AlphaCompare::Ge, 127u8)] {
            let t = first_passing_float(127, compare);
            assert!(!passes(
                code8(f32::from_bits(t.to_bits() - 1)),
                127,
                compare
            ));
            // p = 4/16, so k = 1 at level 1. The runner-up would sit within half a code of the
            // anchor after a uniform scale to the code centre; it must stay below `t`.
            let mut base = Rgba32F::new(4, 4, [0.0; 4]);
            for x in 0..4 {
                base.at_mut(x, 0)[3] = 1.0;
            }
            let mut lvl1 = Rgba32F::new(2, 2, [0.0; 4]);
            lvl1.px[0][3] = 0.3;
            lvl1.px[1][3] = 0.2997;
            lvl1.px[2][3] = 0.1;
            let mut c = vec![base, lvl1];
            preserve_coverage(&mut c, 127, compare);
            assert_eq!(c[1].px[0][3], t, "{compare:?}");
            assert_eq!(code8(c[1].px[0][3]), first_code);
            assert!(!passes(code8(c[1].px[1][3]), 127, compare), "{compare:?}");
            assert_eq!(pass_count(&c[1], 127, compare), 1);
        }
    }

    #[test]
    fn base_level_and_non_alpha_channels_are_untouched() {
        let mut c = foliage_chain();
        for l in &mut c {
            for p in &mut l.px {
                p[0] = 0.25;
            }
        }
        let before = c.clone();
        preserve_coverage(&mut c, 127, AlphaCompare::Gt);
        assert_eq!(c[0], before[0]);
        for l in &c[1..] {
            assert!(l.px.iter().all(|p| p[0] == 0.25));
        }
    }

    #[test]
    fn zero_target_levels_are_left_alone() {
        let mut base = Rgba32F::new(4, 4, [0.0; 4]);
        base.px[0][3] = 1.0; // 1/16 coverage
        let mut lvl1 = Rgba32F::new(2, 2, [0.0; 4]);
        lvl1.px[0][3] = 0.25; // k = round(4/16) = 0 -> unchanged
        let mut c = vec![base, lvl1.clone()];
        preserve_coverage(&mut c, 127, AlphaCompare::Gt);
        assert_eq!(c[1], lvl1);
    }

    #[test]
    fn degenerate_inputs_do_not_panic_or_change() {
        // Empty chain and empty counter.
        preserve_coverage(&mut [], 127, AlphaCompare::Gt);
        assert_eq!(
            coverage_fraction(std::iter::empty(), 127, AlphaCompare::Gt),
            0.0
        );
        // All-zero base: p = 0, so lower levels are untouched.
        let mut c = vec![
            Rgba32F::new(4, 4, [0.0; 4]),
            Rgba32F::new(2, 2, [0.0, 0.0, 0.0, 0.3]),
        ];
        let before = c.clone();
        preserve_coverage(&mut c, 127, AlphaCompare::Gt);
        assert_eq!(c, before);
        // Opaque base but an all-zero lower level: nothing positive to scale.
        let mut c = vec![
            Rgba32F::new(4, 4, [0.0, 0.0, 0.0, 1.0]),
            Rgba32F::new(2, 2, [0.0; 4]),
        ];
        let before = c.clone();
        preserve_coverage(&mut c, 127, AlphaCompare::Gt);
        assert_eq!(c, before);
    }

    #[test]
    fn mostly_empty_level_falls_back_to_smallest_positive_alpha() {
        // Full coverage wanted (k = 4) but only one positive texel: it becomes the scaling anchor.
        let base = Rgba32F::new(4, 4, [0.0, 0.0, 0.0, 1.0]);
        let mut lvl1 = Rgba32F::new(2, 2, [0.0; 4]);
        lvl1.px[0][3] = 0.01;
        let mut c = vec![base, lvl1];
        preserve_coverage(&mut c, 127, AlphaCompare::Gt);
        assert!(passes(code8(c[1].px[0][3]), 127, AlphaCompare::Gt));
        assert_eq!(c[1].px[1][3], 0.0);
    }

    #[test]
    fn a_grid_aligned_tied_set_is_spread_over_the_whole_level() {
        // 256x256, tied alpha only in columns x % 8 == 0 (the fine Bayer rank has just 8 values there), k leaves the last
        // rank class partly filled. The 32-row bands must get about the same share, not top rows first.
        let (w, h) = (256u32, 256u32);
        let a: Vec<f32> = (0..w * h)
            .map(|i| if (i % w) % 8 == 0 { 0.5 } else { 0.0 })
            .collect();
        let k = 5000;
        let pass = exact_pass_set(&a, w, k).unwrap();
        assert_eq!(pass.iter().filter(|&&p| p).count(), k);
        let bands: Vec<usize> = (0..8)
            .map(|b| {
                let rows = (b * 32 * w) as usize..((b + 1) * 32 * w) as usize;
                pass[rows].iter().filter(|&&p| p).count()
            })
            .collect();
        let mean = k as f64 / 8.0;
        for n in &bands {
            assert!(
                (*n as f64 - mean).abs() <= 0.1 * mean,
                "bands {bands:?} vs mean {mean}"
            );
        }
    }

    #[test]
    fn exact_pass_set_edge_cases() {
        // k = n with every texel positive: all pass.
        let all = exact_pass_set(&[0.5; 16], 4, 16).unwrap();
        assert!(all.iter().all(|&p| p));
        // Fewer positive texels than k: every positive passes, zero is never raised.
        let mut w = [0.0f32; 16];
        w[3] = 0.2;
        w[9] = 0.7;
        let pass = exact_pass_set(&w, 4, 5).unwrap();
        assert_eq!(pass.iter().filter(|&&p| p).count(), 2);
        assert!(pass[3] && pass[9]);
        // No positive texel: nothing to do.
        assert!(exact_pass_set(&[0.0; 4], 2, 2).is_none());
        // A fully tied 2x2 with k = 2 takes ranks 0 and 16: (0, 0) and (1, 1).
        assert_eq!(
            exact_pass_set(&[0.5; 4], 2, 2).unwrap(),
            [true, false, false, true]
        );
        // A fully tied 1x1.
        assert_eq!(exact_pass_set(&[0.5], 1, 1).unwrap(), [true]);
    }
}
