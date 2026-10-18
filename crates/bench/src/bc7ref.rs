//! A near-exhaustive reference BC7 encoder, for measuring how much quality a fast encoder leaves on the
//! table (docs/research/bc7e-tuning.md). Measurement only: slow, scalar, never shipped.
//!
//! Every structural choice is tried: all 8 modes, every partition of modes 0/1/2/3/7 (16 or 64), all four
//! rotations of modes 4/5 and both index selectors of mode 4. Each subset is fitted with a PCA start,
//! iterated least squares with every p-bit combination, and a greedy ±1 local search over the quantised
//! endpoints and p-bits with the indices re-assigned at every step. Partitions are ranked by a short fit
//! and the top `top` per mode get the full fit, so the search is exhaustive over structure and locally
//! optimal over endpoints, not a proof of the global optimum (that needs 2^128 evaluations per block).
//!
//! The error is the sum of squared 8-bit channel errors with per-channel weights: 1 for RGB, 1 for alpha
//! on a block that has alpha, 0 for alpha on an opaque block (what RGB PSNR on an opaque image scores and
//! what bc7e's opaque path minimises).
#![allow(clippy::needless_range_loop)] // index-heavy by nature: subsets, texels and channels

use crate::bc7tables::{ANCHOR2, ANCHOR3_1, ANCHOR3_2, PARTITION2, PARTITION3};

const W2: [u32; 4] = [0, 21, 43, 64];
const W3: [u32; 8] = [0, 9, 18, 27, 37, 46, 55, 64];
const W4: [u32; 16] = [0, 4, 9, 13, 17, 21, 26, 30, 34, 38, 43, 47, 51, 55, 60, 64];

fn weights(ibits: u8) -> &'static [u32] {
    match ibits {
        2 => &W2,
        3 => &W3,
        _ => &W4,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PBits {
    None,
    PerEndpoint,
    Shared,
}

#[derive(Clone, Copy, Debug)]
struct Mode {
    subsets: u8,
    part_bits: u8,
    rotation: bool,
    idxsel: bool,
    cbits: u8,
    /// 0: alpha is not coded and decodes to 255.
    abits: u8,
    pbits: PBits,
    /// Colour index bits (alpha shares them unless `separate_alpha`).
    ibits: u8,
    /// Alpha index bits when alpha has its own indices (modes 4 and 5).
    aibits: u8,
}

const MODES: [Mode; 8] = [
    Mode {
        subsets: 3,
        part_bits: 4,
        rotation: false,
        idxsel: false,
        cbits: 4,
        abits: 0,
        pbits: PBits::PerEndpoint,
        ibits: 3,
        aibits: 0,
    },
    Mode {
        subsets: 2,
        part_bits: 6,
        rotation: false,
        idxsel: false,
        cbits: 6,
        abits: 0,
        pbits: PBits::Shared,
        ibits: 3,
        aibits: 0,
    },
    Mode {
        subsets: 3,
        part_bits: 6,
        rotation: false,
        idxsel: false,
        cbits: 5,
        abits: 0,
        pbits: PBits::None,
        ibits: 2,
        aibits: 0,
    },
    Mode {
        subsets: 2,
        part_bits: 6,
        rotation: false,
        idxsel: false,
        cbits: 7,
        abits: 0,
        pbits: PBits::PerEndpoint,
        ibits: 2,
        aibits: 0,
    },
    Mode {
        subsets: 1,
        part_bits: 0,
        rotation: true,
        idxsel: true,
        cbits: 5,
        abits: 6,
        pbits: PBits::None,
        ibits: 2,
        aibits: 3,
    },
    Mode {
        subsets: 1,
        part_bits: 0,
        rotation: true,
        idxsel: false,
        cbits: 7,
        abits: 8,
        pbits: PBits::None,
        ibits: 2,
        aibits: 2,
    },
    Mode {
        subsets: 1,
        part_bits: 0,
        rotation: false,
        idxsel: false,
        cbits: 7,
        abits: 7,
        pbits: PBits::PerEndpoint,
        ibits: 4,
        aibits: 0,
    },
    Mode {
        subsets: 2,
        part_bits: 6,
        rotation: false,
        idxsel: false,
        cbits: 5,
        abits: 5,
        pbits: PBits::PerEndpoint,
        ibits: 2,
        aibits: 0,
    },
];

fn separate_alpha(mode: usize) -> bool {
    mode == 4 || mode == 5
}

/// Expand a `bits`-bit code to 8 bits the way the BC7 decoder does.
fn expand(v: u32, bits: u32) -> u8 {
    ((v << (8 - bits)) | (v >> (2 * bits - 8))) as u8
}

fn lerp(a: u8, b: u8, w: u32) -> u8 {
    ((a as u32 * (64 - w) + b as u32 * w + 32) >> 6) as u8
}

/// One fitted colour cell: raw endpoint codes (without the p-bit), p-bits, and the pixel indices.
#[derive(Clone, Copy, Debug)]
struct Cell {
    lo: [u32; 4],
    hi: [u32; 4],
    p: [u32; 2],
    idx: [u8; 16],
    err: f64,
}

/// What a cell encodes: which channels, how many bits, which p-bits, how many indices.
#[derive(Clone, Copy)]
struct CellSpec {
    cbits: u8,
    /// 0: alpha not coded (decodes 255, counted in the error with its weight).
    abits: u8,
    pbits: PBits,
    ibits: u8,
    /// Alpha is fitted elsewhere (modes 4 and 5): the cell carries RGB only.
    rgb_only: bool,
    w: [f32; 4],
}

impl CellSpec {
    fn channels(&self) -> usize {
        if self.rgb_only || self.abits == 0 {
            3
        } else {
            4
        }
    }
    fn bits(&self, c: usize) -> u32 {
        let b = if c == 3 { self.abits } else { self.cbits } as u32;
        if self.pbits == PBits::None {
            b
        } else {
            b + 1
        }
    }
    /// Decoded 8-bit endpoint value of channel `c` from a raw code and its p-bit.
    fn decode(&self, c: usize, raw: u32, p: u32) -> u8 {
        if c == 3 && (self.abits == 0 || self.rgb_only) {
            return 255;
        }
        match self.pbits {
            PBits::None => expand(raw, self.bits(c)),
            _ => expand((raw << 1) | p, self.bits(c)),
        }
    }
    fn levels(&self, c: usize) -> u32 {
        1 << self.bits(c)
    }
}

/// The decoded palette of a cell.
fn palette(
    spec: &CellSpec,
    lo: &[u32; 4],
    hi: &[u32; 4],
    p: &[u32; 2],
    out: &mut [[u8; 4]; 16],
) -> usize {
    let n = 1usize << spec.ibits;
    let w = weights(spec.ibits);
    let (p0, p1) = match spec.pbits {
        PBits::Shared => (p[0], p[0]),
        _ => (p[0], p[1]),
    };
    let mut a = [255u8; 4];
    let mut b = [255u8; 4];
    for c in 0..spec.channels() {
        a[c] = spec.decode(c, lo[c], p0);
        b[c] = spec.decode(c, hi[c], p1);
    }
    for (i, slot) in out.iter_mut().enumerate().take(n) {
        for c in 0..4 {
            slot[c] = lerp(a[c], b[c], w[i]);
        }
    }
    n
}

/// Assign each pixel its nearest palette entry; returns the weighted squared error.
fn assign(
    spec: &CellSpec,
    pal: &[[u8; 4]; 16],
    n: usize,
    px: &[[f32; 4]],
    idx: &mut [u8; 16],
) -> f64 {
    let mut total = 0.0f64;
    for (i, c) in px.iter().enumerate() {
        let mut best = f32::MAX;
        let mut bi = 0u8;
        let nk = if spec.rgb_only { 3 } else { 4 };
        for (j, e) in pal.iter().enumerate().take(n) {
            let mut d = 0.0f32;
            for k in 0..nk {
                let t = e[k] as f32 - c[k];
                d += spec.w[k] * t * t;
            }
            if d < best {
                best = d;
                bi = j as u8;
            }
        }
        idx[i] = bi;
        total += best as f64;
    }
    total
}

fn evaluate(spec: &CellSpec, lo: &[u32; 4], hi: &[u32; 4], p: &[u32; 2], px: &[[f32; 4]]) -> Cell {
    let mut pal = [[0u8; 4]; 16];
    let n = palette(spec, lo, hi, p, &mut pal);
    let mut idx = [0u8; 16];
    let err = assign(spec, &pal, n, px, &mut idx);
    Cell {
        lo: *lo,
        hi: *hi,
        p: *p,
        idx,
        err,
    }
}

/// Nearest code of parity `parity` (or any parity when the mode has no p-bits) to a float target.
fn quantise(spec: &CellSpec, c: usize, t: f32, parity: Option<u32>) -> u32 {
    let levels = spec.levels(c);
    let q = (t.clamp(0.0, 255.0) / 255.0) * (levels - 1) as f32;
    match parity {
        None => (q + 0.5).floor() as u32,
        Some(par) => {
            let f = q.floor() as i64;
            let mut best = 0i64;
            let mut bd = f32::MAX;
            for cand in (f - 2)..=(f + 2) {
                if cand < 0 || cand >= levels as i64 || (cand as u32 & 1) != par {
                    continue;
                }
                let d = (cand as f32 - q).abs();
                if d < bd {
                    bd = d;
                    best = cand;
                }
            }
            (best >> 1) as u32
        }
    }
}

/// Quantise float endpoints under every p-bit combination and keep the best.
fn quantise_best(spec: &CellSpec, lo: &[f32; 4], hi: &[f32; 4], px: &[[f32; 4]]) -> Cell {
    let combos: &[[u32; 2]] = match spec.pbits {
        PBits::None => &[[0, 0]],
        PBits::Shared => &[[0, 0], [1, 1]],
        PBits::PerEndpoint => &[[0, 0], [0, 1], [1, 0], [1, 1]],
    };
    let mut best: Option<Cell> = None;
    for p in combos {
        let mut ql = [0u32; 4];
        let mut qh = [0u32; 4];
        for c in 0..spec.channels() {
            let (pl, ph) = match spec.pbits {
                PBits::None => (None, None),
                _ => (Some(p[0]), Some(p[1])),
            };
            ql[c] = quantise(spec, c, lo[c], pl);
            qh[c] = quantise(spec, c, hi[c], ph);
        }
        let cell = evaluate(spec, &ql, &qh, p, px);
        if best.is_none_or(|b| cell.err < b.err) {
            best = Some(cell);
        }
    }
    best.unwrap()
}

/// Least-squares endpoints for fixed indices; `None` when every pixel shares one index.
fn least_squares(spec: &CellSpec, idx: &[u8], px: &[[f32; 4]]) -> Option<([f32; 4], [f32; 4])> {
    let w = weights(spec.ibits);
    let (mut a00, mut a01, mut a11) = (0.0f32, 0.0f32, 0.0f32);
    let mut bl = [0.0f32; 4];
    let mut bh = [0.0f32; 4];
    for (i, c) in px.iter().enumerate() {
        let t = w[idx[i] as usize] as f32 / 64.0;
        let s = 1.0 - t;
        a00 += s * s;
        a01 += s * t;
        a11 += t * t;
        for k in 0..4 {
            bl[k] += s * c[k];
            bh[k] += t * c[k];
        }
    }
    let det = a00 * a11 - a01 * a01;
    if det.abs() < 1e-6 {
        return None;
    }
    let inv = 1.0 / det;
    let mut lo = [0.0f32; 4];
    let mut hi = [0.0f32; 4];
    for k in 0..4 {
        lo[k] = ((a11 * bl[k] - a01 * bh[k]) * inv).clamp(0.0, 255.0);
        hi[k] = ((a00 * bh[k] - a01 * bl[k]) * inv).clamp(0.0, 255.0);
    }
    Some((lo, hi))
}

/// PCA start: the mean plus the principal axis through the pixel cloud, clipped to the cloud.
fn pca_start(spec: &CellSpec, px: &[[f32; 4]]) -> ([f32; 4], [f32; 4]) {
    let n = px.len() as f32;
    let nc = spec.channels();
    let mut mean = [0.0f32; 4];
    for c in px {
        for k in 0..nc {
            mean[k] += c[k];
        }
    }
    for m in mean.iter_mut() {
        *m /= n;
    }
    let mut cov = [[0.0f32; 4]; 4];
    for c in px {
        for i in 0..nc {
            for j in 0..nc {
                cov[i][j] +=
                    (c[i] - mean[i]) * spec.w[i].sqrt() * (c[j] - mean[j]) * spec.w[j].sqrt();
            }
        }
    }
    let mut axis = [0.0f32; 4];
    for a in axis.iter_mut().take(nc) {
        *a = 1.0;
    }
    for _ in 0..12 {
        let mut next = [0.0f32; 4];
        for i in 0..nc {
            for j in 0..nc {
                next[i] += cov[i][j] * axis[j];
            }
        }
        let len = next.iter().map(|v| v * v).sum::<f32>().sqrt();
        if len < 1e-9 {
            break;
        }
        for i in 0..nc {
            axis[i] = next[i] / len;
        }
    }
    let (mut lo_d, mut hi_d) = (f32::MAX, f32::MIN);
    for c in px {
        let mut d = 0.0;
        for k in 0..nc {
            d += (c[k] - mean[k]) * axis[k];
        }
        lo_d = lo_d.min(d);
        hi_d = hi_d.max(d);
    }
    let mut lo = [255.0f32; 4];
    let mut hi = [255.0f32; 4];
    for k in 0..nc {
        lo[k] = (mean[k] + axis[k] * lo_d).clamp(0.0, 255.0);
        hi[k] = (mean[k] + axis[k] * hi_d).clamp(0.0, 255.0);
    }
    (lo, hi)
}

/// Fit one cell: PCA start, iterated least squares with full p-bit enumeration, then (when `full`) a
/// greedy ±1 local search over the quantised endpoints and p-bits.
fn fit_cell(spec: &CellSpec, px: &[[f32; 4]], full: bool) -> Cell {
    let (mut lo, mut hi) = pca_start(spec, px);
    let mut best = quantise_best(spec, &lo, &hi, px);
    let iters = if full { 8 } else { 2 };
    for _ in 0..iters {
        let Some((l, h)) = least_squares(spec, &best.idx, px) else {
            break;
        };
        lo = l;
        hi = h;
        let cell = quantise_best(spec, &lo, &hi, px);
        if cell.err + 1e-9 < best.err {
            best = cell;
        } else {
            break;
        }
    }
    if !full || best.err == 0.0 {
        return best;
    }
    // Greedy local search in the quantised space.
    let nc = spec.channels();
    for _round in 0..6 {
        let mut improved = false;
        for e in 0..2 {
            for c in 0..nc {
                for d in [-1i64, 1] {
                    let mut lo2 = best.lo;
                    let mut hi2 = best.hi;
                    let target = if e == 0 { &mut lo2[c] } else { &mut hi2[c] };
                    let raw_levels = match spec.pbits {
                        PBits::None => spec.levels(c),
                        _ => spec.levels(c) >> 1,
                    } as i64;
                    let v = *target as i64 + d;
                    if v < 0 || v >= raw_levels {
                        continue;
                    }
                    *target = v as u32;
                    let cell = evaluate(spec, &lo2, &hi2, &best.p, px);
                    if cell.err + 1e-9 < best.err {
                        best = cell;
                        improved = true;
                    }
                }
            }
        }
        if spec.pbits != PBits::None {
            let flips: &[[u32; 2]] = match spec.pbits {
                PBits::Shared => &[[1, 1]],
                _ => &[[1, 0], [0, 1], [1, 1]],
            };
            for f in flips {
                let p2 = [best.p[0] ^ f[0], best.p[1] ^ f[1]];
                let cell = evaluate(spec, &best.lo, &best.hi, &p2, px);
                if cell.err + 1e-9 < best.err {
                    best = cell;
                    improved = true;
                }
            }
        }
        if !improved {
            break;
        }
    }
    best
}

/// A fitted scalar (alpha) channel of modes 4 and 5: 8-bit codes, own indices.
#[derive(Clone, Copy, Debug)]
struct Scalar {
    lo: u32,
    hi: u32,
    idx: [u8; 16],
    err: f64,
}

fn scalar_palette(bits: u8, ibits: u8, lo: u32, hi: u32, out: &mut [u8; 16]) -> usize {
    let n = 1usize << ibits;
    let w = weights(ibits);
    let a = expand(lo, bits as u32);
    let b = expand(hi, bits as u32);
    for i in 0..n {
        out[i] = lerp(a, b, w[i]);
    }
    n
}

fn scalar_eval(bits: u8, ibits: u8, wgt: f32, lo: u32, hi: u32, v: &[f32; 16]) -> Scalar {
    let mut pal = [0u8; 16];
    let n = scalar_palette(bits, ibits, lo, hi, &mut pal);
    let mut idx = [0u8; 16];
    let mut err = 0.0f64;
    for (i, x) in v.iter().enumerate() {
        let mut best = f32::MAX;
        for (j, p) in pal.iter().enumerate().take(n) {
            let d = (*p as f32 - x) * (*p as f32 - x);
            if d < best {
                best = d;
                idx[i] = j as u8;
            }
        }
        err += (best * wgt) as f64;
    }
    Scalar { lo, hi, idx, err }
}

fn fit_scalar(bits: u8, ibits: u8, wgt: f32, v: &[f32; 16], full: bool) -> Scalar {
    let levels = (1u32 << bits) as f32;
    let q = |t: f32| ((t.clamp(0.0, 255.0) / 255.0) * (levels - 1.0) + 0.5).floor() as u32;
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for x in v {
        lo = lo.min(*x);
        hi = hi.max(*x);
    }
    let mut best = scalar_eval(bits, ibits, wgt, q(lo), q(hi), v);
    let w = weights(ibits);
    for _ in 0..(if full { 8 } else { 2 }) {
        let (mut a00, mut a01, mut a11, mut bl, mut bh) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
        for (i, x) in v.iter().enumerate() {
            let t = w[best.idx[i] as usize] as f32 / 64.0;
            let s = 1.0 - t;
            a00 += s * s;
            a01 += s * t;
            a11 += t * t;
            bl += s * x;
            bh += t * x;
        }
        let det = a00 * a11 - a01 * a01;
        if det.abs() < 1e-6 {
            break;
        }
        let l = (a11 * bl - a01 * bh) / det;
        let h = (a00 * bh - a01 * bl) / det;
        let cell = scalar_eval(bits, ibits, wgt, q(l), q(h), v);
        if cell.err + 1e-9 < best.err {
            best = cell;
        } else {
            break;
        }
    }
    if !full || best.err == 0.0 {
        return best;
    }
    let max = (1u32 << bits) - 1;
    for _ in 0..6 {
        let mut improved = false;
        for (e, d) in [(0, -1i64), (0, 1), (1, -1), (1, 1)] {
            let (mut lo2, mut hi2) = (best.lo as i64, best.hi as i64);
            if e == 0 {
                lo2 += d;
            } else {
                hi2 += d;
            }
            if lo2 < 0 || hi2 < 0 || lo2 > max as i64 || hi2 > max as i64 {
                continue;
            }
            let cell = scalar_eval(bits, ibits, wgt, lo2 as u32, hi2 as u32, v);
            if cell.err + 1e-9 < best.err {
                best = cell;
                improved = true;
            }
        }
        if !improved {
            break;
        }
    }
    best
}

/// The best encoding found for a block.
#[derive(Clone, Debug)]
struct Solution {
    mode: usize,
    partition: u32,
    rotation: u32,
    idxsel: u32,
    cells: [Cell; 3],
    alpha: Option<Scalar>,
    err: f64,
}

fn partition_of(mode: &Mode, partition: u32, texel: usize) -> usize {
    match mode.subsets {
        1 => 0,
        2 => PARTITION2[partition as usize * 16 + texel] as usize,
        _ => PARTITION3[partition as usize * 16 + texel] as usize,
    }
}

fn anchor(subsets: u8, partition: u32, subset: usize) -> usize {
    match (subsets, subset) {
        (_, 0) => 0,
        (2, _) => ANCHOR2[partition as usize] as usize,
        (_, 1) => ANCHOR3_1[partition as usize] as usize,
        _ => ANCHOR3_2[partition as usize] as usize,
    }
}

/// Fit every subset of one (mode, partition); `full` controls the fit depth.
fn fit_block(
    mode_i: usize,
    partition: u32,
    px: &[[f32; 4]; 16],
    w: [f32; 4],
    full: bool,
) -> Solution {
    let mode = &MODES[mode_i];
    let spec = CellSpec {
        cbits: mode.cbits,
        abits: mode.abits,
        pbits: mode.pbits,
        ibits: mode.ibits,
        rgb_only: separate_alpha(mode_i),
        w,
    };
    let empty = Cell {
        lo: [0; 4],
        hi: [0; 4],
        p: [0; 2],
        idx: [0; 16],
        err: 0.0,
    };
    let mut cells = [empty; 3];
    let mut err = 0.0;
    for s in 0..mode.subsets as usize {
        let sub: Vec<[f32; 4]> = (0..16)
            .filter(|&t| partition_of(mode, partition, t) == s)
            .map(|t| px[t])
            .collect();
        let cell = fit_cell(&spec, &sub, full);
        err += cell.err;
        cells[s] = cell;
    }
    Solution {
        mode: mode_i,
        partition,
        rotation: 0,
        idxsel: 0,
        cells,
        alpha: None,
        err,
    }
}

/// Write `val` as `bits` bits at `*pos` (little-endian bit order, as the format is read).
fn put(block: &mut [u8; 16], pos: &mut u32, val: u32, bits: u32) {
    for i in 0..bits {
        if (val >> i) & 1 != 0 {
            block[(*pos / 8) as usize] |= 1 << (*pos % 8);
        }
        *pos += 1;
    }
}

/// Pack a solution into 16 bytes, fixing anchor indices by swapping endpoints where needed.
fn pack(sol: &Solution, px_order: &[usize; 16]) -> [u8; 16] {
    let mode = &MODES[sol.mode];
    let mut cells = sol.cells;
    let mut alpha = sol.alpha;
    let spec_shared = mode.pbits == PBits::Shared;
    // Mode 4 with the index selector set swaps the index widths: colour gets the 3-bit set.
    let (cbits_i, abits_i) = if separate_alpha(sol.mode) && sol.idxsel == 1 {
        (mode.aibits, mode.ibits)
    } else {
        (mode.ibits, mode.aibits)
    };
    let n_idx = 1u8 << cbits_i;
    // Colour indices in texel order (the cells hold indices in subset order).
    let mut idx = [0u8; 16];
    let mut subset_of = [0usize; 16];
    for t in 0..16 {
        let s = partition_of(mode, sol.partition, t);
        subset_of[t] = s;
        let pos = (0..t)
            .filter(|&u| partition_of(mode, sol.partition, u) == s)
            .count();
        idx[t] = cells[s].idx[pos];
    }
    for s in 0..mode.subsets as usize {
        let a = anchor(mode.subsets, sol.partition, s);
        if idx[a] >= n_idx / 2 {
            // Swap the endpoints (and p-bits) and invert this subset's indices.
            let c = &mut cells[s];
            std::mem::swap(&mut c.lo, &mut c.hi);
            if !spec_shared {
                c.p.swap(0, 1);
            }
            for t in 0..16 {
                if subset_of[t] == s {
                    idx[t] = n_idx - 1 - idx[t];
                }
            }
        }
    }
    let mut aidx = [0u8; 16];
    if let Some(a) = alpha.as_mut() {
        let n = 1u8 << abits_i;
        aidx = a.idx;
        if aidx[0] >= n / 2 {
            std::mem::swap(&mut a.lo, &mut a.hi);
            for v in aidx.iter_mut() {
                *v = n - 1 - *v;
            }
        }
    }
    let _ = px_order;
    let mut b = [0u8; 16];
    let mut pos = 0u32;
    put(&mut b, &mut pos, 1 << sol.mode, sol.mode as u32 + 1);
    put(&mut b, &mut pos, sol.partition, mode.part_bits as u32);
    if mode.rotation {
        put(&mut b, &mut pos, sol.rotation, 2);
    }
    if mode.idxsel {
        put(&mut b, &mut pos, sol.idxsel, 1);
    }
    for c in 0..3 {
        for s in 0..mode.subsets as usize {
            put(&mut b, &mut pos, cells[s].lo[c], mode.cbits as u32);
            put(&mut b, &mut pos, cells[s].hi[c], mode.cbits as u32);
        }
    }
    if mode.abits > 0 {
        if let Some(a) = &alpha {
            put(&mut b, &mut pos, a.lo, mode.abits as u32);
            put(&mut b, &mut pos, a.hi, mode.abits as u32);
        } else {
            for s in 0..mode.subsets as usize {
                put(&mut b, &mut pos, cells[s].lo[3], mode.abits as u32);
                put(&mut b, &mut pos, cells[s].hi[3], mode.abits as u32);
            }
        }
    }
    match mode.pbits {
        PBits::None => {}
        PBits::Shared => {
            for s in 0..mode.subsets as usize {
                put(&mut b, &mut pos, cells[s].p[0], 1);
            }
        }
        PBits::PerEndpoint => {
            for s in 0..mode.subsets as usize {
                put(&mut b, &mut pos, cells[s].p[0], 1);
                put(&mut b, &mut pos, cells[s].p[1], 1);
            }
        }
    }
    // Index sets. Modes 4/5 write the 2-bit set first, then the 3-bit (mode 4) or second 2-bit (mode 5) set.
    let write_indices =
        |b: &mut [u8; 16], pos: &mut u32, set: &[u8; 16], bits: u32, anchors: &[usize]| {
            for t in 0..16 {
                let is_anchor = anchors.contains(&t);
                put(
                    b,
                    pos,
                    set[t] as u32,
                    if is_anchor { bits - 1 } else { bits },
                );
            }
        };
    let anchors: Vec<usize> = (0..mode.subsets as usize)
        .map(|s| anchor(mode.subsets, sol.partition, s))
        .collect();
    if separate_alpha(sol.mode) {
        // The set with `mode.ibits` bits is written first, whichever channel it indexes.
        let (first, first_bits, second, second_bits) = if sol.idxsel == 0 {
            (&idx, mode.ibits as u32, &aidx, mode.aibits as u32)
        } else {
            (&aidx, mode.ibits as u32, &idx, mode.aibits as u32)
        };
        write_indices(&mut b, &mut pos, first, first_bits, &[0]);
        write_indices(&mut b, &mut pos, second, second_bits, &[0]);
    } else {
        write_indices(&mut b, &mut pos, &idx, mode.ibits as u32, &anchors);
    }
    debug_assert_eq!(pos, 128, "mode {} packed {} bits", sol.mode, pos);
    b
}

/// Encode one 16-texel block (row-major RGBA8). `top` is how many partitions per mode get the full fit.
pub fn encode_block(src: &[[u8; 4]; 16], top: usize) -> ([u8; 16], f64) {
    let has_alpha = src.iter().any(|c| c[3] < 255);
    let w = [1.0f32, 1.0, 1.0, if has_alpha { 1.0 } else { 0.0 }];
    let px: [[f32; 4]; 16] = std::array::from_fn(|i| {
        [
            src[i][0] as f32,
            src[i][1] as f32,
            src[i][2] as f32,
            src[i][3] as f32,
        ]
    });
    let order: [usize; 16] = std::array::from_fn(|i| i);
    let mut best: Option<Solution> = None;
    let consider = |best: &mut Option<Solution>, s: Solution| {
        if best.as_ref().is_none_or(|b| s.err < b.err) {
            *best = Some(s);
        }
    };
    for (mode_i, mode) in MODES.iter().enumerate() {
        if separate_alpha(mode_i) {
            let idxsels = if mode.idxsel { 2 } else { 1 };
            for rot in 0..4u32 {
                // Rotation r swaps channel r-1 with alpha before encoding.
                let mut rp = px;
                let mut rw = w;
                if rot > 0 {
                    for c in rp.iter_mut() {
                        c.swap(rot as usize - 1, 3);
                    }
                    rw.swap(rot as usize - 1, 3);
                }
                for idxsel in 0..idxsels {
                    let (cib, aib) = if idxsel == 0 {
                        (mode.ibits, mode.aibits)
                    } else {
                        (mode.aibits, mode.ibits)
                    };
                    let spec = CellSpec {
                        cbits: mode.cbits,
                        abits: mode.abits,
                        pbits: PBits::None,
                        ibits: cib,
                        rgb_only: true,
                        w: rw,
                    };
                    let cell = fit_cell(&spec, &rp, true);
                    let av: [f32; 16] = std::array::from_fn(|i| rp[i][3]);
                    let a = fit_scalar(mode.abits, aib, rw[3], &av, true);
                    let empty = Cell {
                        lo: [0; 4],
                        hi: [0; 4],
                        p: [0; 2],
                        idx: [0; 16],
                        err: 0.0,
                    };
                    let sol = Solution {
                        mode: mode_i,
                        partition: 0,
                        rotation: rot,
                        idxsel,
                        cells: [cell, empty, empty],
                        alpha: Some(a),
                        err: cell.err + a.err,
                    };
                    consider(&mut best, sol);
                }
            }
            continue;
        }
        let parts = 1u32 << mode.part_bits;
        if parts == 1 {
            consider(&mut best, fit_block(mode_i, 0, &px, w, true));
            continue;
        }
        // Rank partitions by a short fit, then give the best `top` the full fit.
        let mut ranked: Vec<(f64, u32)> = (0..parts)
            .map(|p| (fit_block(mode_i, p, &px, w, false).err, p))
            .collect();
        ranked.sort_by(|a, b| a.0.total_cmp(&b.0));
        for &(_, p) in ranked.iter().take(top.max(1)) {
            consider(&mut best, fit_block(mode_i, p, &px, w, true));
        }
    }
    let best = best.unwrap();
    (pack(&best, &order), best.err)
}

/// The block as the search believes the decoder will reconstruct it (tests compare it with bcdec).
#[cfg(test)]
fn reconstruct(sol: &Solution, has_alpha: bool) -> [[u8; 4]; 16] {
    let mode = &MODES[sol.mode];
    let w = [1.0f32, 1.0, 1.0, if has_alpha { 1.0 } else { 0.0 }];
    let spec = CellSpec {
        cbits: mode.cbits,
        abits: mode.abits,
        pbits: mode.pbits,
        ibits: if separate_alpha(sol.mode) && sol.idxsel == 1 {
            mode.aibits
        } else {
            mode.ibits
        },
        rgb_only: separate_alpha(sol.mode),
        w,
    };
    let mut out = [[255u8; 4]; 16];
    let mut seen = [0usize; 3];
    for t in 0..16 {
        let s = partition_of(mode, sol.partition, t);
        let c = &sol.cells[s];
        let mut pal = [[0u8; 4]; 16];
        palette(&spec, &c.lo, &c.hi, &c.p, &mut pal);
        out[t] = pal[c.idx[seen[s]] as usize];
        seen[s] += 1;
    }
    if let Some(a) = &sol.alpha {
        let aib = if sol.idxsel == 0 {
            mode.aibits
        } else {
            mode.ibits
        };
        let mut pal = [0u8; 16];
        scalar_palette(mode.abits, aib, a.lo, a.hi, &mut pal);
        for t in 0..16 {
            out[t][3] = pal[a.idx[t] as usize];
        }
        if sol.rotation > 0 {
            for c in out.iter_mut() {
                c.swap(sol.rotation as usize - 1, 3);
            }
        }
    }
    out
}

#[cfg(test)]
pub(crate) fn encode_block_debug(
    src: &[[u8; 4]; 16],
    top: usize,
) -> ([u8; 16], f64, [[u8; 4]; 16], usize) {
    // Same search as `encode_block`, keeping the solution for inspection.
    let has_alpha = src.iter().any(|c| c[3] < 255);
    let (block, err) = encode_block(src, top);
    let _ = top;
    // Re-run the search to get the solution object (encode_block is deterministic).
    let sol = best_solution(src, top);
    (block, err, reconstruct(&sol, has_alpha), sol.mode)
}

#[cfg(test)]
fn best_solution(src: &[[u8; 4]; 16], top: usize) -> Solution {
    // Mirror of encode_block's search; kept test-only so the hot path stays a single function.
    let has_alpha = src.iter().any(|c| c[3] < 255);
    let w = [1.0f32, 1.0, 1.0, if has_alpha { 1.0 } else { 0.0 }];
    let px: [[f32; 4]; 16] = std::array::from_fn(|i| {
        [
            src[i][0] as f32,
            src[i][1] as f32,
            src[i][2] as f32,
            src[i][3] as f32,
        ]
    });
    let mut best: Option<Solution> = None;
    let consider = |best: &mut Option<Solution>, s: Solution| {
        if best.as_ref().is_none_or(|b| s.err < b.err) {
            *best = Some(s);
        }
    };
    for (mode_i, mode) in MODES.iter().enumerate() {
        if separate_alpha(mode_i) {
            let idxsels = if mode.idxsel { 2 } else { 1 };
            for rot in 0..4u32 {
                let mut rp = px;
                let mut rw = w;
                if rot > 0 {
                    for c in rp.iter_mut() {
                        c.swap(rot as usize - 1, 3);
                    }
                    rw.swap(rot as usize - 1, 3);
                }
                for idxsel in 0..idxsels {
                    let (cib, aib) = if idxsel == 0 {
                        (mode.ibits, mode.aibits)
                    } else {
                        (mode.aibits, mode.ibits)
                    };
                    let spec = CellSpec {
                        cbits: mode.cbits,
                        abits: mode.abits,
                        pbits: PBits::None,
                        ibits: cib,
                        rgb_only: true,
                        w: rw,
                    };
                    let cell = fit_cell(&spec, &rp, true);
                    let av: [f32; 16] = std::array::from_fn(|i| rp[i][3]);
                    let a = fit_scalar(mode.abits, aib, rw[3], &av, true);
                    let empty = Cell {
                        lo: [0; 4],
                        hi: [0; 4],
                        p: [0; 2],
                        idx: [0; 16],
                        err: 0.0,
                    };
                    consider(
                        &mut best,
                        Solution {
                            mode: mode_i,
                            partition: 0,
                            rotation: rot,
                            idxsel,
                            cells: [cell, empty, empty],
                            alpha: Some(a),
                            err: cell.err + a.err,
                        },
                    );
                }
            }
            continue;
        }
        let parts = 1u32 << mode.part_bits;
        if parts == 1 {
            consider(&mut best, fit_block(mode_i, 0, &px, w, true));
            continue;
        }
        let mut ranked: Vec<(f64, u32)> = (0..parts)
            .map(|p| (fit_block(mode_i, p, &px, w, false).err, p))
            .collect();
        ranked.sort_by(|a, b| a.0.total_cmp(&b.0));
        for &(_, p) in ranked.iter().take(top.max(1)) {
            consider(&mut best, fit_block(mode_i, p, &px, w, true));
        }
    }
    best.unwrap()
}

/// Encode a whole image (row-major RGBA8) in parallel. Returns the BC7 payload.
pub fn encode_image(px: &[[u8; 4]], width: u32, height: u32, top: usize) -> Vec<u8> {
    use rayon::prelude::*;
    let blocks = encoders_sys::gather_blocks(px, width, height);
    let mut out = vec![0u8; blocks.len()];
    out.par_chunks_mut(16)
        .zip(blocks.par_chunks(16))
        .for_each(|(o, b)| {
            let b: &[[u8; 4]; 16] = b.try_into().unwrap();
            o.copy_from_slice(&encode_block(b, top).0);
        });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(block: &[u8; 16]) -> [[u8; 4]; 16] {
        let mut o = [0u8; 64];
        bcdec_rs::bc7(block, &mut o, 16);
        std::array::from_fn(|i| [o[i * 4], o[i * 4 + 1], o[i * 4 + 2], o[i * 4 + 3]])
    }

    /// The packer and the error model agree with the real decoder: the error the search believed in is
    /// the error bcdec measures.
    #[test]
    fn packed_blocks_decode_to_the_error_the_search_computed() {
        let mut seed = 0x1234_5678_9abc_def0u64;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 32) as u32
        };
        for case in 0..400 {
            let base = [rnd() & 255, rnd() & 255, rnd() & 255, rnd() & 255];
            let spread = 1 + (rnd() % 120);
            let alpha_kind = case % 4;
            let src: [[u8; 4]; 16] = std::array::from_fn(|_| {
                let mut c = [0u8; 4];
                for k in 0..3 {
                    c[k] = ((base[k] as i64 + (rnd() % spread) as i64 - spread as i64 / 2)
                        .clamp(0, 255)) as u8;
                }
                c[3] = match alpha_kind {
                    0 => 255,
                    1 => (rnd() % 2 * 255) as u8,
                    2 => (rnd() & 255) as u8,
                    _ => ((base[3] as i64 + (rnd() % spread) as i64).clamp(0, 255)) as u8,
                };
                c
            });
            let (block, err, recon, mode) = encode_block_debug(&src, 4);
            let dec = decode(&block);
            if recon != dec {
                for t in 0..16 {
                    eprintln!(
                        "case {case} mode {mode} texel {t}: src {:?} recon {:?} bcdec {:?}",
                        src[t], recon[t], dec[t]
                    );
                }
                eprintln!("block {:02x?}", block);
            }
            let has_alpha = src.iter().any(|c| c[3] < 255);
            let mut measured = 0.0f64;
            for (s, d) in src.iter().zip(&dec) {
                for k in 0..3 {
                    let t = s[k] as f64 - d[k] as f64;
                    measured += t * t;
                }
                if has_alpha {
                    let t = s[3] as f64 - d[3] as f64;
                    measured += t * t;
                }
            }
            assert!(
                (measured - err).abs() < 0.5,
                "case {case}: search err {err} but decoded err {measured}; mode byte {:08b}",
                block[0]
            );
        }
    }
}
