//! UV coverage (Task 7): which winning NIF shapes use a texture (scan DB), their UVs rasterised into a 1-bit mask,
//! and the share of used texels that sit close to another UV island (gutter). Everything here runs on a worker.
use super::viewport3d::{read_file, FileSource};
use modscan::store::{Store, TexKey};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub const MASK_SIZE: u32 = 512;
/// Mip-bleed distance in full-resolution texels (mip 3, the padding convention).
pub const GUTTER_TEXELS: u32 = 8;
pub const GUTTER_SHARE: f64 = 0.005;
pub const TILE_CAP: i32 = 16;

thread_local! {
    static UV_READS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Scan-DB reads `coverage_for` and `scan_has_rows` made on this thread (tests: the UI thread must make none).
pub fn uv_reads_on_this_thread() -> u32 {
    UV_READS.with(|c| c.get())
}

fn count_read() {
    UV_READS.with(|c| c.set(c.get() + 1));
}

/// One bit per texel, row-major, row 0 at v = 0.
#[derive(Debug, Clone, PartialEq)]
pub struct UvMask {
    pub size: u32,
    pub bits: Vec<u64>,
}

impl UvMask {
    pub fn new(size: u32) -> UvMask {
        let n = size as usize * size as usize;
        UvMask {
            size,
            bits: vec![0; n.div_ceil(64)],
        }
    }

    fn idx(&self, x: u32, y: u32) -> usize {
        y as usize * self.size as usize + x as usize
    }

    pub fn get(&self, x: u32, y: u32) -> bool {
        let i = self.idx(x, y);
        (self.bits[i / 64] >> (i % 64)) & 1 == 1
    }

    pub fn set(&mut self, x: u32, y: u32) {
        let i = self.idx(x, y);
        self.bits[i / 64] |= 1 << (i % 64);
    }

    fn used(&self) -> u64 {
        self.bits.iter().map(|w| w.count_ones() as u64).sum()
    }

    fn texels(&self) -> u64 {
        self.size as u64 * self.size as u64
    }

    /// Used texels / all texels; 0.0 for a 0-sized mask.
    pub fn used_share(&self) -> f64 {
        match self.texels() {
            0 => 0.0,
            n => self.used() as f64 / n as f64,
        }
    }

    pub fn is_full(&self) -> bool {
        self.used() == self.texels()
    }

    fn fill(&mut self) {
        let n = self.texels() as usize;
        self.bits.fill(u64::MAX);
        if !n.is_multiple_of(64) {
            *self.bits.last_mut().unwrap() = (1u64 << (n % 64)) - 1;
        }
    }
}

/// Marks the texels whose centre lies in a triangle. Each triangle is drawn once per integer tile offset its UV
/// box touches, shifted into [0, 1), so a strip repeating in u marks only its own v band. A triangle with an index
/// past `uvs` or a non-finite UV is skipped (K24).
pub fn rasterize(mask: &mut UvMask, uvs: &[[f32; 2]], tris: &[[u16; 3]]) {
    if mask.size == 0 {
        return;
    }
    for (i, t) in tris.iter().enumerate() {
        if i.is_multiple_of(256) && mask.is_full() {
            return;
        }
        let Some(p) = t
            .iter()
            .map(|&k| uvs.get(k as usize).map(|uv| [uv[0] as f64, uv[1] as f64]))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        if p.iter().flatten().any(|c| !c.is_finite()) {
            continue;
        }
        let lo = |a: usize| p.iter().map(|q| q[a]).fold(f64::INFINITY, f64::min).floor();
        let hi = |a: usize| {
            p.iter()
                .map(|q| q[a])
                .fold(f64::NEG_INFINITY, f64::max)
                .floor()
        };
        let (u0, v0, u1, v1) = (lo(0), lo(1), hi(0), hi(1));
        // shortcut: a huge tiling triangle is treated as full coverage; upgrade if trim sheets with more repeats
        // show up.
        if (u1 - u0 + 1.0) * (v1 - v0 + 1.0) > (TILE_CAP * TILE_CAP) as f64 {
            mask.fill();
            return;
        }
        for ty in v0 as i64..=v1 as i64 {
            for tx in u0 as i64..=u1 as i64 {
                let s = |q: [f64; 2]| [q[0] - tx as f64, q[1] - ty as f64];
                draw(mask, s(p[0]), s(p[1]), s(p[2]));
            }
        }
    }
}

/// One triangle in [0, 1)-space: texel-centre test with edge functions (inclusive, either winding).
fn draw(mask: &mut UvMask, a: [f64; 2], b: [f64; 2], c: [f64; 2]) {
    let n = mask.size as f64;
    let edge = |p: [f64; 2], q: [f64; 2], r: [f64; 2]| {
        (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])
    };
    let area = edge(a, b, c);
    if area == 0.0 {
        return;
    }
    let sign = area.signum();
    // Texels whose centre (x + 0.5) / n can fall inside the box, clipped to the mask.
    let range = |lo: f64, hi: f64| {
        let first = (lo * n - 0.5).ceil().max(0.0);
        let last = (hi * n - 0.5).floor().min(n - 1.0);
        (first as u32, last)
    };
    let (x0, x1) = range(a[0].min(b[0]).min(c[0]), a[0].max(b[0]).max(c[0]));
    let (y0, y1) = range(a[1].min(b[1]).min(c[1]), a[1].max(b[1]).max(c[1]));
    if x1 < 0.0 || y1 < 0.0 {
        return;
    }
    for y in y0..=y1 as u32 {
        let py = (y as f64 + 0.5) / n;
        for x in x0..=x1 as u32 {
            let q = [(x as f64 + 0.5) / n, py];
            if edge(a, b, q) * sign >= 0.0
                && edge(b, c, q) * sign >= 0.0
                && edge(c, a, q) * sign >= 0.0
            {
                mask.set(x, y);
            }
        }
    }
}

/// Used texels with another UV island (4-connected) at most `d = GUTTER_TEXELS * size / tex_width` empty mask
/// texels away (Chebyshev), divided by the used texels; 0.0 when none are used. `None` when `d < 2`.
pub fn gutter_share(mask: &UvMask, tex_width: u32) -> Option<f64> {
    let d = (GUTTER_TEXELS * mask.size).checked_div(tex_width)?;
    // shortcut: a 512 mask can't resolve the gap above 2048 wide; upgrade by measuring on a larger mask for 4K.
    if d < 2 {
        return None;
    }
    let n = mask.size as usize;
    let labels = label(mask);
    // A gap of d empty texels is a centre-to-centre distance of d + 1; past the mask it adds nothing.
    let r = (d as usize + 1).min(n);
    // Two-pass (rows, then columns) window min and max of the labels, 0 (unused) left out.
    let mut lo: Vec<u32> = labels
        .iter()
        .map(|&l| if l == 0 { u32::MAX } else { l })
        .collect();
    let mut hi = labels.clone();
    for y in 0..n {
        window(&mut lo[y * n..(y + 1) * n], 1, r, u32::MAX, u32::min);
        window(&mut hi[y * n..(y + 1) * n], 1, r, 0, u32::max);
    }
    for x in 0..n {
        window(&mut lo[x..], n, r, u32::MAX, u32::min);
        window(&mut hi[x..], n, r, 0, u32::max);
    }
    let (mut used, mut gutter) = (0u64, 0u64);
    for ((&l, a), b) in labels.iter().zip(&lo).zip(&hi) {
        if l != 0 {
            used += 1;
            // Its own label is in the window, so a different min or max is another island.
            gutter += (a != b) as u64;
        }
    }
    Some(if used == 0 {
        0.0
    } else {
        gutter as f64 / used as f64
    })
}

/// 4-connected components of the used texels, 1-based; 0 is unused.
fn label(mask: &UvMask) -> Vec<u32> {
    let n = mask.size as usize;
    let mut out = vec![0u32; n * n];
    let mut next = 0;
    let mut stack = vec![];
    for start in 0..n * n {
        if out[start] != 0 || !mask.get((start % n) as u32, (start / n) as u32) {
            continue;
        }
        next += 1;
        out[start] = next;
        stack.push(start);
        while let Some(i) = stack.pop() {
            let (x, y) = (i % n, i / n);
            let near = [
                (x > 0).then(|| i - 1),
                (x + 1 < n).then(|| i + 1),
                (y > 0).then(|| i - n),
                (y + 1 < n).then(|| i + n),
            ];
            for j in near.into_iter().flatten() {
                if out[j] == 0 && mask.get((j % n) as u32, (j / n) as u32) {
                    out[j] = next;
                    stack.push(j);
                }
            }
        }
    }
    out
}

/// In place: every `stride`-th element of `v` (a row or a column) becomes `op` over its neighbours within `r`.
/// van Herk / Gil-Werman, so O(len) whatever `r`.
fn window(v: &mut [u32], stride: usize, r: usize, neutral: u32, op: fn(u32, u32) -> u32) {
    let len = v.len().div_ceil(stride);
    let w = 2 * r + 1;
    let padded = len + 2 * r;
    let at = |i: usize| {
        if i < r || i >= r + len {
            neutral
        } else {
            v[(i - r) * stride]
        }
    };
    // g: op from each block's start up to i; h: op from i to its block's end.
    let mut g = vec![neutral; padded];
    let mut h = vec![neutral; padded];
    for i in 0..padded {
        g[i] = if i.is_multiple_of(w) {
            at(i)
        } else {
            op(g[i - 1], at(i))
        };
    }
    for i in (0..padded).rev() {
        h[i] = if i % w == w - 1 || i + 1 == padded {
            at(i)
        } else {
            op(h[i + 1], at(i))
        };
    }
    for i in 0..len {
        // Window [i, i + 2r] in padded indices.
        v[i * stride] = op(h[i], g[i + 2 * r]);
    }
}

/// A winning NIF copy to read and the blocks of its shapes that name the texture.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeRef {
    pub source: FileSource,
    pub nif: String,
    pub blocks: Vec<u32>,
}

/// The game-winning shapes that name `tex` in slot 0 (derived winners on their source copy), grouped per copy.
/// LE meshes are skipped (the SE geometry reader refuses them), and so is a copy gone from the scan.
pub fn winning_shapes(db: &Path, tex: &str) -> Result<Vec<ShapeRef>, String> {
    let store = Store::open_read_only(db).map_err(|e| e.to_string())?;
    let key = TexKey::Exact {
        slot: 0,
        path: tex.to_string(),
    };
    let rows = store
        .nifs_using_texture(&key, usize::MAX)
        .map_err(|e| e.to_string())?;
    let mut by_copy: BTreeMap<i64, (String, Vec<u32>)> = BTreeMap::new();
    for r in rows.into_iter().filter(|r| !r.le_mesh) {
        let e = by_copy
            .entry(r.load_copy_id)
            .or_insert((r.nif_path, vec![]));
        if !e.1.contains(&r.shape_block) {
            e.1.push(r.shape_block);
        }
    }
    let mut out = vec![];
    for (copy, (nif, blocks)) in by_copy {
        if let Some((p, kind, root, _)) = store.copy_location(copy).map_err(|e| e.to_string())? {
            out.push(ShapeRef {
                source: FileSource::from_store(kind, &root, &p),
                nif,
                blocks,
            });
        }
    }
    Ok(out)
}

/// False for a missing, old-schema or empty scan (no enabled origins).
pub fn scan_has_rows(db: &Path) -> bool {
    count_read();
    Store::open_read_only(db)
        .and_then(|s| s.mods())
        .is_ok_and(|m| !m.is_empty())
}

#[derive(Debug, Clone)]
pub struct Coverage {
    pub mask: Arc<UvMask>,
    pub used: f64,
    pub gutter: Option<f64>,
    /// NIFs read and parsed.
    pub nifs: usize,
    /// "<nif>: <error>" per NIF that failed to read or parse, and per shape that failed or was skipped.
    pub errors: Vec<String>,
}

/// The UV coverage of `tex` over every winning shape that names it. `Err` for a DB that cannot be opened or
/// queried, and when `cancel` is set (checked between NIFs), so a partial mask is never taken as the answer.
pub fn coverage_for(
    db: &Path,
    tex: &str,
    tex_width: u32,
    cancel: &AtomicBool,
) -> Result<Coverage, String> {
    count_read();
    let shapes = winning_shapes(db, tex)?;
    let mut mask = UvMask::new(MASK_SIZE);
    let (mut nifs, mut errors) = (0, vec![]);
    for s in &shapes {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        let bytes = match read_file(&s.source) {
            Ok(b) => b,
            Err(e) => {
                errors.push(format!("{}: {e}", s.nif));
                continue;
            }
        };
        let h = match nif::header::Header::parse(&bytes) {
            Ok(h) => h,
            Err(e) => {
                errors.push(format!("{}: {e}", s.nif));
                continue;
            }
        };
        nifs += 1;
        for &b in &s.blocks {
            match nif::geom::read_shape(&h, &bytes, b as usize) {
                Ok(m) => {
                    let bad = m
                        .triangles
                        .iter()
                        .filter(|t| t.iter().any(|&k| k as usize >= m.uvs.len()))
                        .count();
                    if bad > 0 {
                        errors.push(format!(
                            "{}: block {b}: {bad} triangles have no UVs, skipped",
                            s.nif
                        ));
                    }
                    rasterize(&mut mask, &m.uvs, &m.triangles);
                }
                Err(e) => errors.push(format!("{}: block {b}: {e}", s.nif)),
            }
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("cancelled".into());
    }
    Ok(Coverage {
        used: mask.used_share(),
        gutter: gutter_share(&mask, tex_width),
        mask: Arc::new(mask),
        nifs,
        errors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_is_the_min_over_the_neighbourhood() {
        let src = [5, 1, 7, 3, 9, 2, 8, 6, 4];
        for r in 0..12 {
            let mut v = src;
            window(&mut v, 1, r, u32::MAX, u32::min);
            for i in 0..src.len() {
                let want = src[i.saturating_sub(r)..=(i + r).min(src.len() - 1)]
                    .iter()
                    .copied()
                    .min()
                    .unwrap();
                assert_eq!(v[i], want, "r {r} i {i}");
            }
        }
        // A column: stride 3 over a 3x3 grid touches only column 0.
        let mut g = [9, 0, 0, 1, 0, 0, 5, 0, 0];
        window(&mut g, 3, 1, u32::MAX, u32::min);
        assert_eq!(g, [1, 0, 0, 1, 0, 0, 1, 0, 0]);
    }

    #[test]
    fn a_full_mask_of_odd_size_counts_every_texel_once() {
        let mut m = UvMask::new(7);
        m.fill();
        assert!(m.is_full());
        assert_eq!(m.used_share(), 1.0);
        assert!(!UvMask::new(7).is_full());
    }
}
