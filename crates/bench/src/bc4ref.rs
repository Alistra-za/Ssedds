//! Reference BC4 block search (docs/research/bc4-family.md): a Rust port of `rgbcx::encode_bc4_hq` plus the measured
//! additions (6-value search on the inner range, least-squares refits, trimmed inner ranges, a local search).
//! Everything starts from the shipped result and replaces it only on a strictly lower SSE, so ties keep its bytes.
//! The vendored C++ is patched to equal [`search`] byte for byte, so the tie rules here are the contract: the first
//! minimum per texel, a strict `<` to replace the best, and candidates tried in the order written.
// Index loops mirror the C++ they port.
#![allow(clippy::needless_range_loop)]

/// The bcdec / patched-rgbcx BC4 palette (`get_block_values`); checked against the FFI for all 65536 pairs.
pub fn pal(e0: u8, e1: u8) -> [u8; 8] {
    let (l, h) = (e0 as u32, e1 as u32);
    let w = |a: u32, b: u32| ((a * l + b * h + 32768) >> 16) as u8;
    if l > h {
        [
            e0,
            e1,
            w(56173, 9363),
            w(46812, 18724),
            w(37450, 28086),
            w(28086, 37450),
            w(18724, 46812),
            w(9363, 56173),
        ]
    } else {
        [
            e0,
            e1,
            w(52429, 13107),
            w(39321, 26215),
            w(26215, 39321),
            w(13107, 52429),
            0,
            255,
        ]
    }
}

#[derive(Clone, Copy)]
pub struct Best {
    pub err: u32,
    pub e0: u8,
    pub e1: u8,
    pub sels: [u8; 16],
}

impl Default for Best {
    fn default() -> Self {
        Self::new()
    }
}

impl Best {
    pub fn new() -> Self {
        Best {
            err: u32::MAX,
            e0: 0,
            e1: 0,
            sels: [0; 16],
        }
    }
    pub fn bytes(&self) -> [u8; 8] {
        let mut bits = 0u64;
        for (i, &s) in self.sels.iter().enumerate() {
            bits |= (s as u64) << (3 * i);
        }
        let mut o = [0u8; 8];
        o[0] = self.e0;
        o[1] = self.e1;
        o[2..].copy_from_slice(&bits.to_le_bytes()[..6]);
        o
    }
    /// Try one endpoint pair exactly as rgbcx does (first minimum per texel, early out, strict improvement).
    pub fn try_pair(&mut self, v: &[u8; 16], e0: u8, e1: u8) {
        let p = pal(e0, e1);
        let mut err = 0u32;
        let mut sels = [0u8; 16];
        for i in 0..16 {
            let (mut be, mut bj) = (u32::MAX, 0u8);
            for (j, &pj) in p.iter().enumerate() {
                let d = pj as i32 - v[i] as i32;
                let e = (d * d) as u32;
                if e < be {
                    be = e;
                    bj = j as u8;
                    if e == 0 {
                        break;
                    }
                }
            }
            err += be;
            if err >= self.err {
                return;
            }
            sels[i] = bj;
        }
        *self = Best { err, e0, e1, sels };
    }
}

pub fn minmax(v: &[u8; 16]) -> (u8, u8) {
    (*v.iter().min().unwrap(), *v.iter().max().unwrap())
}

pub fn clampi(x: i32) -> u8 {
    x.clamp(0, 255) as u8
}

/// Port of `rgbcx::encode_bc4_hq(.., rad, BC4_USE_ALL_MODES)`.
pub fn v0(v: &[u8; 16], rad: i32) -> Best {
    let (mn, mx) = minmax(v);
    if mn == mx {
        return Best {
            err: 0,
            e0: mn,
            e1: mn,
            sels: [0; 16],
        };
    }
    let mut b = Best::new();
    for mode in 0..2 {
        for lo in -rad..=rad {
            for hi in -rad..=rad {
                let (mut e0, mut e1) = (clampi(mx as i32 + hi), clampi(mn as i32 + lo));
                if e0 == e1 {
                    continue;
                }
                if (mode == 0 && e0 <= e1) || (mode == 1 && e0 > e1) {
                    std::mem::swap(&mut e0, &mut e1);
                }
                b.try_pair(v, e0, e1);
            }
        }
    }
    b
}

/// The 6-value-mode search rgbcx cannot reach: endpoints around the inner range, so 0 and 255 come from the palette.
pub fn inner6(v: &[u8; 16], b: &mut Best, rad: i32) {
    if !v.iter().any(|&x| x == 0 || x == 255) {
        return;
    }
    let inner: Vec<u8> = v.iter().copied().filter(|&x| x != 0 && x != 255).collect();
    let (Some(&imn), Some(&imx)) = (inner.iter().min(), inner.iter().max()) else {
        return;
    };
    for lo in -rad..=rad {
        for hi in -rad..=rad {
            let (e0, e1) = (clampi(imn as i32 + lo), clampi(imx as i32 + hi));
            if e0 <= e1 {
                b.try_pair(v, e0, e1);
            }
        }
    }
}

/// Fraction of e1 in texel value for selector `s` (None = fixed 0/255 in the 6-value mode).
fn frac(mode8: bool, s: u8) -> Option<f64> {
    match (mode8, s) {
        (_, 0) => Some(0.0),
        (_, 1) => Some(1.0),
        (true, s) => Some((s - 1) as f64 / 7.0),
        (false, 6 | 7) => None,
        (false, s) => Some((s - 1) as f64 / 5.0),
    }
}

/// Least-squares refit of the endpoints for the current selectors, then the +-1 neighbourhood in the same mode.
pub fn ls_refine(v: &[u8; 16], b: &mut Best) {
    for _ in 0..3 {
        let mode8 = b.e0 > b.e1;
        let (mut aa, mut ab, mut bb, mut av, mut bv) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for i in 0..16 {
            if let Some(t) = frac(mode8, b.sels[i]) {
                let (wa, wb, x) = (1.0 - t, t, v[i] as f64);
                aa += wa * wa;
                ab += wa * wb;
                bb += wb * wb;
                av += wa * x;
                bv += wb * x;
            }
        }
        let det = aa * bb - ab * ab;
        if det.abs() < 1e-9 {
            return;
        }
        let a = (av * bb - bv * ab) / det;
        let c = (bv * aa - av * ab) / det;
        let before = b.err;
        for da in -1..=1 {
            for dc in -1..=1 {
                let (e0, e1) = (clampi(a.round() as i32 + da), clampi(c.round() as i32 + dc));
                if (e0 > e1) == mode8 {
                    b.try_pair(v, e0, e1);
                }
            }
        }
        if b.err >= before {
            return;
        }
    }
}

/// Refit the best of each mode, not only the overall best: `ls_refine` on `b`, then a fresh search of rgbcx's own
/// candidates in the other mode, `ls_refine` on that, keeping the strictly better.
pub fn refit_both(v: &[u8; 16], b: &mut Best, rad: i32) {
    ls_refine(v, b);
    let (mn, mx) = minmax(v);
    if mn != mx {
        let mut o = Best::new();
        let mode8 = b.e0 > b.e1;
        for lo in -rad..=rad {
            for hi in -rad..=rad {
                let (mut e0, mut e1) = (clampi(mx as i32 + hi), clampi(mn as i32 + lo));
                if e0 == e1 {
                    continue;
                }
                if (e0 > e1) == mode8 {
                    std::mem::swap(&mut e0, &mut e1);
                }
                o.try_pair(v, e0, e1);
            }
        }
        ls_refine(v, &mut o);
        if o.err < b.err {
            *b = o;
        }
    }
}

/// 6-value mode with near-extreme texels left to the palette's 0/255: inner range = values in (t, 255 - t).
pub fn trimmed6(v: &[u8; 16], b: &mut Best) {
    for t in [2u8, 6, 12, 24, 48] {
        if !v.iter().any(|&x| x <= t || x >= 255 - t) {
            continue;
        }
        let inner: Vec<u8> = v
            .iter()
            .copied()
            .filter(|&x| x > t && x < 255 - t)
            .collect();
        let (Some(&imn), Some(&imx)) = (inner.iter().min(), inner.iter().max()) else {
            continue;
        };
        let mut c = *b;
        for lo in -2..=2 {
            for hi in -2..=2 {
                let (e0, e1) = (clampi(imn as i32 + lo), clampi(imx as i32 + hi));
                if e0 <= e1 {
                    c.try_pair(v, e0, e1);
                }
            }
        }
        if c.e0 <= c.e1 {
            ls_refine(v, &mut c);
        }
        if c.err < b.err {
            *b = c;
        }
    }
}

/// Exhaustive +-3 around the current best, same mode.
pub fn local(v: &[u8; 16], b: &mut Best) {
    let (c0, c1, m8) = (b.e0 as i32, b.e1 as i32, b.e0 > b.e1);
    for d0 in -3..=3 {
        for d1 in -3..=3 {
            let (e0, e1) = (clampi(c0 + d0), clampi(c1 + d1));
            if (e0 > e1) == m8 {
                b.try_pair(v, e0, e1);
            }
        }
    }
}

/// Texels whose alpha-test result at 128 differs from the source.
pub fn flips(v: &[u8; 16], b: &Best) -> u32 {
    let p = pal(b.e0, b.e1);
    (0..16)
        .filter(|&i| (v[i] >= 128) != (p[b.sels[i] as usize] >= 128))
        .count() as u32
}

/// The shipped search: `rgbcx::encode_bc4_hq` with all modes, as bytes.
pub fn shipped(v: &[u8; 16], radius: u32) -> [u8; 8] {
    v0(v, radius as i32).bytes()
}

/// The measured search (probe variant v4; v5 with `guard128`, which falls back to the shipped block when it would
/// flip more texels across the alpha-test value 128). Never worse than [`shipped`] in SSE.
pub fn search(v: &[u8; 16], radius: u32, guard128: bool) -> [u8; 8] {
    let rad = radius as i32;
    let base = v0(v, rad);
    let mut b = base;
    inner6(v, &mut b, rad);
    refit_both(v, &mut b, rad);
    trimmed6(v, &mut b);
    local(v, &mut b);
    if guard128 && flips(v, &b) > flips(v, &base) {
        b = base;
    }
    b.bytes()
}
