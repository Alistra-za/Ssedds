//! Coverage-preserving alpha mips (spec §5.5).
//!
//! Rebuilds the idea of AlphaBloom's "Auto Coverage" (tkRiverGamer) and Ignacio Castaño's
//! coverage-preserving mipmap method; see `docs/CREDITS.md`. No code is copied.
//!
//! Level 0 defines the target: the fraction `p` of texels whose 8-bit alpha code passes the
//! alpha test. Each lower level is scaled so that its `round(p · n)` largest alphas pass.
//! The counter ([`coverage_fraction`]) and the solver ([`preserve_coverage`]) share
//! [`passes`] and the engine's [`code8`] rounding, so they cannot disagree.
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

/// Rescale the alpha of every level `>= 1` so its coverage matches level 0 (the reference,
/// left untouched). A level is left unchanged when its target count rounds to zero or it has
/// no positive alpha to scale.
pub fn preserve_coverage(chain: &mut [Rgba32F], threshold: u8, compare: AlphaCompare) {
    let Some((base, rest)) = chain.split_first_mut() else {
        return;
    };
    let p = coverage_fraction(base.px.iter().map(|q| q[3]), threshold, compare);
    // Smallest alpha whose rounded code passes.
    let tau = match compare {
        AlphaCompare::Gt => (f32::from(threshold) + 1.0) / 255.0,
        AlphaCompare::Ge => f32::from(threshold) / 255.0,
    }
    .min(1.0);
    for level in rest {
        let n = level.px.len();
        let k = (p * n as f64).round() as usize;
        if k == 0 {
            continue;
        }
        let mut sorted: Vec<f32> = level.px.iter().map(|q| q[3]).collect();
        sorted.sort_by(|a, b| b.total_cmp(a));
        let mut a = sorted[k.min(n) - 1];
        if a <= 0.0 {
            match sorted.iter().rev().find(|&&v| v > 0.0) {
                Some(&v) => a = v,
                None => continue,
            }
        }
        let scale = tau / a;
        for q in &mut level.px {
            q[3] = (q[3] * scale).clamp(0.0, 1.0);
        }
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

    /// 64x64 sparse "foliage": alpha = u^4 for a deterministic uniform u (continuous values, so the
    /// quantile is not degenerate; a binary base would make whole mip levels tie). Box-filtered down.
    fn foliage_chain() -> Vec<Rgba32F> {
        let mut base = Rgba32F::new(64, 64, [0.0; 4]);
        let mut s: u32 = 12345;
        for p in &mut base.px {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let u = (s >> 8) as f32 / (1u32 << 24) as f32;
            p[3] = u * u * u * u;
        }
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

    #[test]
    fn preservation_keeps_coverage_within_2_points_where_resolvable() {
        let mut c = foliage_chain();
        let base = coverage_fraction(alpha_of(&c[0]), 127, AlphaCompare::Gt);
        preserve_coverage(&mut c, 127, AlphaCompare::Gt);
        let mut checked = 0;
        for l in &c[1..] {
            let cov = coverage_fraction(alpha_of(l), 127, AlphaCompare::Gt);
            if l.px.len() >= 64 {
                checked += 1;
                assert!(
                    (cov - base).abs() <= 0.02,
                    "{}x{}: {cov} vs base {base}",
                    l.width,
                    l.height
                );
            }
        }
        assert!(checked >= 3, "only {checked} levels were checked");
    }

    #[test]
    fn preservation_works_for_ge_and_other_thresholds() {
        for (threshold, compare) in [
            (127, AlphaCompare::Ge),
            (64, AlphaCompare::Gt),
            (200, AlphaCompare::Ge),
        ] {
            let mut c = foliage_chain();
            let base = coverage_fraction(alpha_of(&c[0]), threshold, compare);
            preserve_coverage(&mut c, threshold, compare);
            let cov = coverage_fraction(alpha_of(&c[1]), threshold, compare);
            assert!(
                (cov - base).abs() <= 0.02,
                "{threshold} {compare:?}: {cov} vs {base}"
            );
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
