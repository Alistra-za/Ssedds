//! Coverage-preserving alpha mips (spec §5.5).
//!
//! Rebuilds the idea of AlphaBloom's "Auto Coverage" (tkRiverGamer) and Ignacio Castaño's
//! coverage-preserving mipmap method; see `docs/CREDITS.md`. No code is copied.
//!
//! Level 0 defines the target: the fraction `p` of texels whose 8-bit alpha code passes the
//! alpha test. Each lower level is remapped so that its pass count is the achievable count
//! nearest `round(p · n)`. The counter ([`coverage_fraction`]) and the solver
//! ([`preserve_coverage`]) share [`passes`] and the engine's [`code8`] rounding, so they
//! cannot disagree.
//!
//! - **Boundary.** `T` is the smallest f32 whose code passes (found by search, not the code
//!   centre), so only the texels at or above the anchor pass.
//! - **Ties.** A monotone remap cannot split equal alphas, so the achievable counts are the
//!   group boundaries `#(alpha >= v)` of the distinct positive values `v`. The nearest one is
//!   taken, the larger on a tie (coverage loss is the visible failure). A binary base can
//!   therefore over-cover: one opaque texel per 2×2 gives four tied 0.25s at level 1, and the
//!   only achievable count for `k = 1` is 4.
//! - **Remap shape (deviation from Castaño).** Castaño scales alpha uniformly by `T/a`. When
//!   the anchor's group is 1.0 that halves opaque interiors, which visibly breaks
//!   `cutout_blend`. This module uses a monotone piecewise-linear remap keeping 0 → 0 and
//!   1 → 1: `[0, a] → [0, T]`, `[a, 1] → [T, 1]`. The pass set depends only on order and the
//!   boundary, so coverage is the same (controller ruling, owner check; spec §5.5 note).
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

/// The distinct positive alpha `a` whose pass count `#(w >= a)` is nearest `k` (`1 <= k <= n`),
/// with that count; on a tie the larger count wins. These counts are the only ones a monotone
/// remap can reach; zero alphas cannot be raised, so `a = 0` never qualifies. `w` is scratch.
fn nearest_group(w: &mut [f32], k: usize) -> Option<(f32, usize)> {
    let (_, &mut v, _) = w.select_nth_unstable_by(k - 1, |x, y| y.total_cmp(x));
    let (mut at_least, mut above, mut next_larger) = (0usize, 0usize, None::<f32>);
    for &x in w.iter() {
        if x >= v {
            at_least += 1;
        }
        if x > v {
            above += 1;
            next_larger = Some(next_larger.map_or(x, |m| m.min(x)));
        }
    }
    // `at_least >= k > above`: the smallest achievable count at or above `k`, and the largest
    // below it.
    let hi = (v > 0.0).then_some((v, at_least));
    let lo = next_larger.map(|u| (u, above));
    match (hi, lo) {
        (Some(h), Some(l)) => Some(if h.1 - k <= k - l.1 { h } else { l }),
        (h, l) => h.or(l),
    }
}

/// Rewrite the alpha of every level `>= 1` so its coverage matches level 0 (the reference,
/// left untouched). Per level the target is `k = round(p · n)`; the anchor `a` is the distinct
/// alpha whose count `#(alpha >= a)` is nearest `k` (ties to the larger count). Alpha is then
/// remapped piecewise-linearly, `[0, a] → [0, T]` and `[a, 1] → [T, 1]` with `T` the smallest
/// float whose code passes, so exactly the texels `>= a` pass. A level is left unchanged when
/// `k = 0`, when it has no positive alpha, when its pass set already matches, or when the
/// anchor is 1.0.
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
    let t_fail = t.next_down(); // the largest failing float
    let p = coverage_fraction(base.px.iter().map(|q| q[3]), threshold, compare);
    let pass_count = |l: &Rgba32F| {
        l.px.iter()
            .filter(|q| passes(code8(q[3]), threshold, compare))
            .count()
    };
    for level in rest {
        let n = level.px.len();
        let k = ((p * n as f64).round() as usize).min(n);
        if k == 0 {
            continue;
        }
        let mut w: Vec<f32> = level.px.iter().map(|q| unit_alpha(q[3])).collect();
        let Some((a, count)) = nearest_group(&mut w, k) else {
            continue;
        };
        // The current pass set is an upper set in alpha order, so an equal count is the same
        // set. An anchor of 1.0 cannot move to `T` while keeping 1 at 1 (controller ruling).
        if pass_count(level) == count || a >= 1.0 {
            continue;
        }
        let low = t / a; // finite: a >= f32::MIN_POSITIVE
        let high = (1.0 - t) / (1.0 - a);
        for q in &mut level.px {
            let w = unit_alpha(q[3]);
            q[3] = if w >= 1.0 {
                1.0
            } else if w >= a {
                // Every term is >= 0, so the result is >= T and passes; w = a gives T exactly.
                (t + (w - a) * high).min(1.0)
            } else {
                // Capped at the largest failing float, so f32 rounding cannot promote it.
                (w * low).min(t_fail)
            };
        }
        debug_assert_eq!(pass_count(level), count);
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
    fn binary_tie_takes_the_only_achievable_count() {
        // One opaque texel per 2x2 footprint: level 1 is four tied 0.25s and k = 1. A monotone
        // remap cannot split a tie, and count 0 is not considered for k >= 1, so the only
        // achievable count is 4 (documented limitation: three texels over).
        let mut base = Rgba32F::new(4, 4, [0.0; 4]);
        for (x, y) in [(0, 0), (2, 0), (0, 2), (2, 2)] {
            base.at_mut(x, y)[3] = 1.0;
        }
        let mut c = box_chain(base);
        assert!(alpha_of(&c[1]).all(|a| a == 0.25));
        preserve_coverage(&mut c, 127, AlphaCompare::Gt);
        assert_eq!(pass_count(&c[1], 127, AlphaCompare::Gt), 4);
        // The tied group lands exactly on the first passing float, not the code centre.
        let t = first_passing_float(127, AlphaCompare::Gt);
        assert!(alpha_of(&c[1]).all(|a| a == t), "{:?}", c[1].px);
    }

    #[test]
    fn tie_picks_the_nearer_achievable_group() {
        // Footprints with 3, 1, 1, 1 opaque texels: level 1 = [0.75, 0.25, 0.25, 0.25].
        // p = 6/16, k = round(1.5) = 2; achievable counts are 1 (>= 0.75) and 4 (>= 0.25).
        let mut base = Rgba32F::new(4, 4, [0.0; 4]);
        for (x, y) in [(0, 0), (1, 0), (0, 1), (2, 0), (0, 2), (2, 2)] {
            base.at_mut(x, y)[3] = 1.0;
        }
        let mut c = box_chain(base);
        let before = c[1].clone();
        assert_eq!(pass_count(&before, 127, AlphaCompare::Gt), 1);
        preserve_coverage(&mut c, 127, AlphaCompare::Gt);
        assert_eq!(pass_count(&c[1], 127, AlphaCompare::Gt), 1);
        assert_eq!(
            c[1], before,
            "the nearer group already passes: level unchanged"
        );
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
}
