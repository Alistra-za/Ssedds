//! UV coverage (Task 7): which winning NIF shapes use a texture (scan DB), their UVs rasterised into a 1-bit mask,
//! and the share of used texels that sit close to another UV island (gutter). Everything here runs on a worker.
use super::viewport3d::{FileReader, FileSource};
use modscan::store::{ShapeView, Store, TexKey};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub const MASK_SIZE: u32 = 512;
/// Mip-bleed distance in full-resolution texels (mip 3, the padding convention).
pub const GUTTER_TEXELS: u32 = 8;
pub const GUTTER_SHARE: f64 = 0.005;
pub const TILE_CAP: i32 = 16;
/// `coverage_for`'s `Err` when `cancel` was set: never a texture's answer (K18).
pub const CANCELLED: &str = "cancelled";

thread_local! {
    static UV_READS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Scan-DB opens by `winning_shapes` (so also `coverage_for`) and `scan_has_rows` on this thread (tests: the UI
/// thread must make none).
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
        debug_assert!(
            x < self.size && y < self.size,
            "texel ({x}, {y}) outside {}",
            self.size
        );
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

    fn texels(&self) -> u64 {
        self.size as u64 * self.size as u64
    }

    /// Used texels / all texels; 0.0 for a 0-sized mask.
    pub fn used_share(&self) -> f64 {
        match self.texels() {
            0 => 0.0,
            n => self.bits.iter().map(|w| w.count_ones() as u64).sum::<u64>() as f64 / n as f64,
        }
    }

    /// Every texel used (a 0-sized mask counts as full).
    pub fn is_full(&self) -> bool {
        self.bits.iter().map(|w| w.count_ones() as u64).sum::<u64>() == self.texels()
    }
}

/// Where the rasteriser writes: `t` is the triangle's index in its shape.
trait Sink {
    fn size(&self) -> u32;
    fn plot(&mut self, t: usize, x: u32, y: u32);
    /// Nothing more can change: stop drawing.
    fn done(&self) -> bool;
    /// A triangle covered every texel.
    fn filled(&mut self) {}
}

impl Sink for UvMask {
    fn size(&self) -> u32 {
        self.size
    }
    fn plot(&mut self, _: usize, x: u32, y: u32) {
        self.set(x, y);
    }
    fn done(&self) -> bool {
        self.is_full()
    }
}

/// A triangle's UVs, or `None` for an index past `uvs` or a non-finite UV (K24: skipped).
fn tri_uv(uvs: &[[f32; 2]], t: &[u16; 3]) -> Option<[[f64; 2]; 3]> {
    let mut p = [[0.0; 2]; 3];
    for (q, &k) in p.iter_mut().zip(t) {
        let uv = uvs.get(k as usize)?;
        if !uv[0].is_finite() || !uv[1].is_finite() {
            return None;
        }
        *q = [uv[0] as f64, uv[1] as f64];
    }
    Some(p)
}

/// Marks the texels whose centre lies in a triangle. Each triangle is drawn once per integer tile offset its UV
/// box touches, shifted into [0, 1), so a strip repeating in u marks only its own v band. A triangle with an index
/// past `uvs` or a non-finite UV is skipped (K24). It returns early once the mask is full.
pub fn rasterize(mask: &mut UvMask, uvs: &[[f32; 2]], tris: &[[u16; 3]]) {
    raster(mask, uvs, tris);
}

// shortcut: texel-centre coverage, so an island thinner than one mask texel (4 px at 2K) can mark nothing and drops
// out of the overlay and the gutter; upgrade with a conservative pass if hair-card strips need it.
fn raster(sink: &mut impl Sink, uvs: &[[f32; 2]], tris: &[[u16; 3]]) {
    let n = sink.size();
    if n == 0 {
        return;
    }
    // Texels tested since fullness was last checked: a check (a 4096-word popcount) costs no more than the drawing.
    let mut work = 0u64;
    for (t, tri) in tris.iter().enumerate() {
        if work >= 4096 {
            work = 0;
            if sink.done() {
                return;
            }
        }
        let Some(p) = tri_uv(uvs, tri) else {
            continue;
        };
        let min = |a: usize| p.iter().map(|q| q[a]).fold(f64::INFINITY, f64::min);
        let max = |a: usize| p.iter().map(|q| q[a]).fold(f64::NEG_INFINITY, f64::max);
        let (umin, umax, vmin, vmax) = (min(0), max(0), min(1), max(1));
        let (u0, u1, v0, v1) = (umin.floor(), umax.floor(), vmin.floor(), vmax.floor());
        let (wide, tall) = (
            u1 - u0 + 1.0 > TILE_CAP as f64,
            v1 - v0 + 1.0 > TILE_CAP as f64,
        );
        if wide || tall {
            // shortcut: a triangle repeating more than TILE_CAP times along an axis is treated as covering its whole
            // UV box, wrapped (the full axis where it repeats past the cap; so a long trim strip marks its own band
            // and only a triangle past the cap on both axes fills the mask); upgrade if trim sheets need the shape.
            let xs = axis(n, umin, umax, wide);
            let ys = axis(n, vmin, vmax, tall);
            for &y in &ys {
                for &x in &xs {
                    sink.plot(t, x, y);
                }
            }
            if wide && tall {
                sink.filled();
                return;
            }
            work += (xs.len() * ys.len()) as u64;
            continue;
        }
        for ty in v0 as i64..=v1 as i64 {
            for tx in u0 as i64..=u1 as i64 {
                let s = |q: [f64; 2]| [q[0] - tx as f64, q[1] - ty as f64];
                work += draw(sink, t, s(p[0]), s(p[1]), s(p[2]));
            }
        }
    }
}

/// The texels of one axis whose centre lies in [lo, hi] wrapped into [0, 1) (every texel with `all`).
fn axis(n: u32, lo: f64, hi: f64, all: bool) -> Vec<u32> {
    if all || hi - lo >= 1.0 {
        return (0..n).collect();
    }
    let mut on = vec![false; n as usize];
    for k in [lo.floor(), hi.floor()] {
        let (first, last) = texel_range(n, lo - k, hi - k);
        for x in first..=last {
            on[x as usize] = true;
        }
    }
    (0..n).filter(|&x| on[x as usize]).collect()
}

/// Texels whose centre (x + 0.5) / n lies in [lo, hi], clipped to the mask; empty when first > last.
fn texel_range(n: u32, lo: f64, hi: f64) -> (u32, u32) {
    let nf = n as f64;
    let first = (lo * nf - 0.5).ceil().max(0.0);
    let last = (hi * nf - 0.5).floor().min(nf - 1.0);
    if last < first {
        (1, 0)
    } else {
        (first as u32, last as u32)
    }
}

/// One triangle in [0, 1)-space: texel-centre test with edge functions (inclusive, either winding). Returns the
/// texels tested.
fn draw(sink: &mut impl Sink, t: usize, a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> u64 {
    let n = sink.size();
    let nf = n as f64;
    let edge = |p: [f64; 2], q: [f64; 2], r: [f64; 2]| {
        (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])
    };
    let area = edge(a, b, c);
    if area == 0.0 {
        return 0;
    }
    let sign = area.signum();
    let (x0, x1) = texel_range(n, a[0].min(b[0]).min(c[0]), a[0].max(b[0]).max(c[0]));
    let (y0, y1) = texel_range(n, a[1].min(b[1]).min(c[1]), a[1].max(b[1]).max(c[1]));
    if x1 < x0 || y1 < y0 {
        return 0;
    }
    for y in y0..=y1 {
        let py = (y as f64 + 0.5) / nf;
        for x in x0..=x1 {
            let q = [(x as f64 + 0.5) / nf, py];
            if edge(a, b, q) * sign >= 0.0
                && edge(b, c, q) * sign >= 0.0
                && edge(c, a, q) * sign >= 0.0
            {
                sink.plot(t, x, y);
            }
        }
    }
    (x1 - x0 + 1) as u64 * (y1 - y0 + 1) as u64
}

/// The used texels of every shape added, and which UV island each belongs to.
///
/// An island is a set of triangles of one shape connected through shared UV vertices, welded by UV value (a hard
/// edge or a split normal duplicates a vertex without moving its UV). Islands that cover the same texel (the same
/// layout in two NIFs, a mirrored half) are one island: they share those texels, so they cannot bleed into each
/// other. Islands of different shapes are otherwise apart, even where their edges meet exactly.
#[derive(Debug, Clone)]
pub struct UvIslands {
    pub mask: UvMask,
    /// Per texel: 0 unused, else a union-find node + 1.
    ids: Vec<u32>,
    parent: Vec<u32>,
    /// A triangle past the cap on both axes covered every texel: one island, nothing more can change.
    filled: bool,
}

fn find(parent: &mut [u32], mut a: u32) -> u32 {
    while parent[a as usize] != a {
        parent[a as usize] = parent[parent[a as usize] as usize];
        a = parent[a as usize];
    }
    a
}

fn union(parent: &mut [u32], a: u32, b: u32) {
    let (a, b) = (find(parent, a), find(parent, b));
    if a != b {
        parent[a.max(b) as usize] = a.min(b);
    }
}

impl UvIslands {
    pub fn new(size: u32) -> UvIslands {
        UvIslands {
            mask: UvMask::new(size),
            ids: vec![0; size as usize * size as usize],
            parent: vec![],
            filled: false,
        }
    }

    /// Rasterises one shape's triangles (as `rasterize`) and labels their islands.
    pub fn add(&mut self, uvs: &[[f32; 2]], tris: &[[u16; 3]]) {
        if self.filled {
            return;
        }
        let mut weld: HashMap<(u64, u64), u32> = HashMap::new();
        let mut node = vec![0u32; tris.len()];
        for (t, tri) in tris.iter().enumerate() {
            let Some(p) = tri_uv(uvs, tri) else {
                continue;
            };
            let mut ns = [0u32; 3];
            for (n, q) in ns.iter_mut().zip(p) {
                // + 0.0 folds -0.0 into 0.0.
                let key = ((q[0] + 0.0).to_bits(), (q[1] + 0.0).to_bits());
                *n = *weld.entry(key).or_insert_with(|| {
                    self.parent.push(self.parent.len() as u32);
                    self.parent.len() as u32 - 1
                });
            }
            union(&mut self.parent, ns[0], ns[1]);
            union(&mut self.parent, ns[0], ns[2]);
            node[t] = ns[0] + 1;
        }
        let mut sink = ShapeSink {
            isl: self,
            node: &node,
        };
        raster(&mut sink, uvs, tris);
    }
}

struct ShapeSink<'a> {
    isl: &'a mut UvIslands,
    node: &'a [u32],
}

impl Sink for ShapeSink<'_> {
    fn size(&self) -> u32 {
        self.isl.mask.size
    }
    fn plot(&mut self, t: usize, x: u32, y: u32) {
        let id = self.node[t];
        let i = self.isl.mask.idx(x, y);
        match self.isl.ids[i] {
            0 => {
                self.isl.ids[i] = id;
                self.isl.mask.set(x, y);
            }
            cur if cur != id => union(&mut self.isl.parent, cur - 1, id - 1),
            _ => {}
        }
    }
    fn done(&self) -> bool {
        // Only a full fill ends early: on a mask full of real coverage, overlaps can still merge islands.
        self.isl.filled
    }
    fn filled(&mut self) {
        self.isl.filled = true;
    }
}

/// Used texels with another UV island at most `d − 1` empty mask texels away on each axis (Chebyshev centre
/// distance ≤ d), divided by the used texels; 0.0 when none are used. `d = ceil(GUTTER_TEXELS × size / side)` per
/// axis, u from the width and v from the height, so an island counts when it sits closer than 8 full-res texels.
/// `None` when a side is 0 or the radius is under 2 (4096 and wider): a 512 mask can't resolve the gap there.
pub fn gutter_share(isl: &UvIslands, tex_size: [u32; 2]) -> Option<f64> {
    let n = isl.mask.size as usize;
    let radius = |side: u32| (side > 0).then(|| (GUTTER_TEXELS * isl.mask.size).div_ceil(side));
    let (dx, dy) = (radius(tex_size[0])?, radius(tex_size[1])?);
    // shortcut: a 512 mask can't resolve the gap at 4096 and wider; upgrade by measuring on a larger mask for 4K.
    if dx < 2 || dy < 2 {
        return None;
    }
    // shortcut: islands across the 0/1 edge are never neighbours, as with mip filters that don't wrap; upgrade if
    // tiling textures show bleed across the edge.
    let mut parent = isl.parent.clone();
    let labels: Vec<u32> = isl
        .ids
        .iter()
        .map(|&i| {
            if i == 0 {
                0
            } else {
                find(&mut parent, i - 1) + 1
            }
        })
        .collect();
    let (rx, ry) = ((dx as usize).min(n), (dy as usize).min(n));
    // Two-pass (rows, then columns) window min and max of the labels, 0 (unused) left out.
    let mut lo: Vec<u32> = labels
        .iter()
        .map(|&l| if l == 0 { u32::MAX } else { l })
        .collect();
    let mut hi = labels.clone();
    for y in 0..n {
        window(&mut lo[y * n..(y + 1) * n], 1, rx, u32::MAX, u32::min);
        window(&mut hi[y * n..(y + 1) * n], 1, rx, 0, u32::max);
    }
    for x in 0..n {
        window(&mut lo[x..], n, ry, u32::MAX, u32::min);
        window(&mut hi[x..], n, ry, 0, u32::max);
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

/// In place: every `stride`-th element of `v` (a row or a column) becomes `op` over its neighbours within `r`.
/// van Herk / Gil-Werman, so linear in the elements whatever `r`.
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

/// The game-winning shapes whose slot 0 resolves to `tex` (canonical path, as "Used by meshes" matches it), derived
/// winners on their source copy, grouped per copy. LE meshes are skipped (the SE geometry reader refuses them), and
/// so is a copy gone from the scan.
pub fn winning_shapes(db: &Path, tex: &str) -> Result<Vec<ShapeRef>, String> {
    count_read();
    let want = nif::path::canon_texture(tex.as_bytes()).norm;
    // The query matches the slot's raw string (PGPatcher's form), so it is asked for a fragment every alias of the
    // canonical path contains (`Data\Textures\a\wall.dds`, `a\wall.dds`, `textures\pbr\…`), and each shape is
    // re-checked on its canonical slot path.
    // shortcut: `Contains` cannot use the path index (a scan of `path`); upgrade with a typed canonical-path query in
    // modscan (NIF-team request) if Check all is slow on large scans.
    let rel = want.strip_prefix("textures/").unwrap_or(&want);
    let rel = rel.strip_prefix("pbr/").unwrap_or(rel);
    let frag = match rel.rfind('.') {
        Some(i) if !rel[i..].contains('/') => &rel[..i],
        _ => rel,
    };
    let store = Store::open_read_only(db).map_err(|e| e.to_string())?;
    let key = TexKey::Contains {
        slot: 0,
        frag: frag.to_string(),
    };
    let rows = store
        .nifs_using_texture(&key, usize::MAX)
        .map_err(|e| e.to_string())?;
    let mut shapes: HashMap<i64, Vec<ShapeView>> = HashMap::new();
    let mut by_copy: BTreeMap<i64, (String, Vec<u32>)> = BTreeMap::new();
    for r in rows.into_iter().filter(|r| !r.le_mesh) {
        let views = match shapes.entry(r.load_copy_id) {
            std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(store.shapes_of(r.load_copy_id).map_err(|e| e.to_string())?)
            }
        };
        let names_it = views
            .iter()
            .find(|v| v.block == r.shape_block)
            .and_then(|v| v.textures.first().cloned().flatten())
            .is_some_and(|p| p == want);
        if !names_it {
            continue;
        }
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
    /// "<nif>: <error>" per NIF that failed to read or parse, and per shape that failed or had triangles skipped.
    pub errors: Vec<String>,
}

/// The UV coverage of `tex` (`tex_size` = [width, height]) over every winning shape that names it. `Err` for a DB
/// that cannot be opened or queried, and [`CANCELLED`] when `cancel` is set (checked before the query, between NIFs
/// and between shapes), so a partial mask is never taken as the answer.
pub fn coverage_for(
    db: &Path,
    tex: &str,
    tex_size: [u32; 2],
    cancel: &AtomicBool,
) -> Result<Coverage, String> {
    let stop = || cancel.load(Ordering::Relaxed);
    if stop() {
        return Err(CANCELLED.into());
    }
    let shapes = winning_shapes(db, tex)?;
    let reader = FileReader::default(); // each archive is opened and indexed once per call
    let mut isl = UvIslands::new(MASK_SIZE);
    let (mut nifs, mut errors) = (0, vec![]);
    for s in &shapes {
        if stop() {
            return Err(CANCELLED.into());
        }
        let bytes = match reader.read(&s.source) {
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
            if stop() {
                return Err(CANCELLED.into());
            }
            match nif::geom::read_shape(&h, &bytes, b as usize) {
                Ok(m) => {
                    let bad = m
                        .triangles
                        .iter()
                        .filter(|t| tri_uv(&m.uvs, t).is_none())
                        .count();
                    if bad > 0 {
                        errors.push(format!(
                            "{}: block {b}: {bad} triangles with no or bad UVs skipped",
                            s.nif
                        ));
                    }
                    isl.add(&m.uvs, &m.triangles);
                }
                Err(e) => errors.push(format!("{}: block {b}: {e}", s.nif)),
            }
        }
    }
    if stop() {
        return Err(CANCELLED.into());
    }
    Ok(Coverage {
        used: isl.mask.used_share(),
        gutter: gutter_share(&isl, tex_size),
        mask: Arc::new(isl.mask),
        nifs,
        errors,
    })
}

// ---- Task 8: the session cache, the reads, the scan-has-rows probe and "Check all UV coverage" ----

/// How a read gets a texture's coverage: `coverage_for`, or a slow or panicking stand-in in tests.
pub type ReadFn = fn(&Path, &str, [u32; 2], &AtomicBool) -> Result<Coverage, String>;

/// A cache key: the scan path and the written size the gutter was measured at (review I1).
pub(super) type Key = (String, [u32; 2]);

/// A cached answer and its overlay image (`None` for a failed read, review I2).
#[derive(Clone)]
struct Entry {
    coverage: Arc<Coverage>,
    image: Option<Arc<eframe::egui::ColorImage>>,
}

/// A running "Check all UV coverage".
struct AllRun {
    run: u64,
    cancel: Arc<AtomicBool>,
    done: usize,
    total: usize,
}

/// How often landed Check-all answers re-run `refresh_checks` (review M8).
const REFRESH_EVERY: std::time::Duration = std::time::Duration::from_millis(250);

/// UV coverage in the GUI (DECISIONS Note CI): the session cache keyed by scan path and written size (no disk
/// cache, C4), the reads in flight, the Check-all run and the scan-has-rows answer. Every job here is counted in
/// `AppModel::v2_jobs_pending`.
pub struct UvState {
    cache: HashMap<Key, Entry>,
    /// Bumped by landed answers (at most every `REFRESH_EVERY` during Check all) and by a clear: part of
    /// `refresh_checks`' key.
    revision: u64,
    /// Answers landed since the last bump.
    dirty: bool,
    bumped: Option<std::time::Instant>,
    /// Bumped by a clear and by a new folder: an answer read before it is dropped.
    generation: u64,
    /// Keys with a single read in flight, and the generation it was started in.
    pending: HashMap<Key, u64>,
    /// Cancels the single reads of this generation.
    reads_cancel: Arc<AtomicBool>,
    all: Option<AllRun>,
    runs: u64,
    /// The probe's answer and the scan-DB stamp it read.
    has_rows: Option<(super::cs_pbr::Stamp, bool)>,
    /// The stamp of the probe in flight.
    probing: Option<super::cs_pbr::Stamp>,
    /// The stamp the cache was filled under: a new one (an external scan, a running scan's checkpoint) clears it.
    stamp: super::cs_pbr::Stamp,
    /// Some item of the open folder has a colour sub-type (set in `refresh_checks`, read per frame by Advanced).
    pub(super) any_colour: bool,
    #[doc(hidden)]
    pub read: ReadFn,
}

impl Default for UvState {
    fn default() -> UvState {
        UvState {
            cache: HashMap::new(),
            revision: 0,
            dirty: false,
            bumped: None,
            generation: 0,
            pending: HashMap::new(),
            reads_cancel: Arc::default(),
            all: None,
            runs: 0,
            has_rows: None,
            probing: None,
            stamp: None,
            any_colour: false,
            read: coverage_for,
        }
    }
}

impl UvState {
    /// A single read is in flight.
    pub fn read_pending(&self) -> bool {
        !self.pending.is_empty()
    }
    /// "Check all UV coverage" is running.
    pub fn all_running(&self) -> bool {
        self.all.is_some()
    }
    /// Stop was clicked, and the reads in flight are still finishing.
    pub fn all_stopping(&self) -> bool {
        self.all
            .as_ref()
            .is_some_and(|a| a.cancel.load(Ordering::Relaxed))
    }
    /// The scan-has-rows probe is in flight.
    pub fn probe_pending(&self) -> bool {
        self.probing.is_some()
    }
    /// (done, total) of the running Check all.
    pub fn all_progress(&self) -> Option<(usize, usize)> {
        self.all.as_ref().map(|a| (a.done, a.total))
    }
    /// The running Check all's number (its progress bar's own epoch, K10).
    pub fn all_run(&self) -> Option<u64> {
        self.all.as_ref().map(|a| a.run)
    }
    /// The generation an answer must carry to land (tests inject answers).
    pub fn generation(&self) -> u64 {
        self.generation
    }
    /// Part of `refresh_checks`' key (tests: how often the checks re-run).
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub(super) fn get(&self, tex: &str, size: [u32; 2]) -> Option<&Arc<Coverage>> {
        self.cache
            .get(&(tex.to_string(), size))
            .map(|e| &e.coverage)
    }

    fn bump(&mut self) {
        self.revision += 1;
        self.dirty = false;
        self.bumped = Some(std::time::Instant::now());
    }

    /// Drop the answers in flight and cancel their reads and the Check all; the cache stays (a new folder, review
    /// M7: the key does not depend on the folder).
    pub(super) fn drop_in_flight(&mut self) {
        self.pending.clear();
        self.generation += 1;
        self.reads_cancel.store(true, Ordering::Relaxed);
        self.reads_cancel = Arc::default();
        if let Some(a) = &self.all {
            a.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Forget every answer too (a finished scan, a new scan stamp).
    pub(super) fn clear(&mut self) {
        self.drop_in_flight();
        self.cache.clear();
        self.bump();
    }
}

/// A read that found nothing because it failed (no NIF read, an error line): it paints no overlay (review I2).
pub fn failed(c: &Coverage) -> bool {
    c.nifs == 0 && !c.errors.is_empty()
}

/// `read`'s answer as the message's coverage and overlay: an `Err` becomes a coverage with that error line (K18), a
/// panic too (K9); `None` when `cancel` is set, so a cancelled read is never an answer (the flag decides, not the
/// message, review M4).
fn read_one(
    read: ReadFn,
    db: &Path,
    tex: &str,
    size: [u32; 2],
    cancel: &AtomicBool,
) -> Option<(Arc<Coverage>, Option<Arc<eframe::egui::ColorImage>>)> {
    let got =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read(db, tex, size, cancel)))
            .unwrap_or_else(|p| {
                Err(format!(
                    "internal error: {}",
                    crate::worker::panic_text(&*p)
                ))
            });
    if cancel.load(Ordering::Relaxed) {
        return None;
    }
    let c = got.unwrap_or_else(|why| Coverage {
        mask: Arc::new(UvMask::new(MASK_SIZE)),
        used: 0.0,
        gutter: None,
        nifs: 0,
        errors: vec![why],
    });
    let image = (!failed(&c)).then(|| super::overlay::uv_overlay_image(&c.mask));
    Some((Arc::new(c), image))
}

/// One "Check all UV coverage" run's work: scan paths and written sizes.
pub struct AllJob {
    pub run: u64,
    pub generation: u64,
    pub db: std::path::PathBuf,
    pub work: Vec<(String, [u32; 2])>,
    /// Worker threads (at least 1).
    pub jobs: usize,
    pub read: ReadFn,
}

/// Reads every texture of `job` on `job.jobs` scoped threads: one `UvCoverage` and one `UvProgress` per texture
/// read, then one final `UvProgress { finished: true }`. `cancel` is checked before each texture, so a cancelled
/// run ends after the reads in flight (which return early) with `done < total`.
pub fn check_all_run(
    tx: &std::sync::mpsc::Sender<crate::worker::WorkerMsg>,
    job: &AllJob,
    cancel: &AtomicBool,
) {
    use crate::worker::WorkerMsg;
    use std::sync::atomic::AtomicUsize;
    let (next, done) = (AtomicUsize::new(0), AtomicUsize::new(0));
    let total = job.work.len();
    std::thread::scope(|s| {
        for _ in 0..job.jobs.clamp(1, total.max(1)) {
            s.spawn(|| {
                while !cancel.load(Ordering::Relaxed) {
                    let Some((tex, size)) = job.work.get(next.fetch_add(1, Ordering::Relaxed))
                    else {
                        break;
                    };
                    let Some((coverage, image)) = read_one(job.read, &job.db, tex, *size, cancel)
                    else {
                        break;
                    };
                    let _ = tx.send(WorkerMsg::UvCoverage {
                        generation: job.generation,
                        tex: tex.clone(),
                        size: *size,
                        coverage,
                        image,
                    });
                    let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                    let _ = tx.send(WorkerMsg::UvProgress {
                        run: job.run,
                        done: d,
                        total,
                        finished: false,
                    });
                }
            });
        }
    });
    let _ = tx.send(WorkerMsg::UvProgress {
        run: job.run,
        done: done.load(Ordering::Relaxed),
        total,
        finished: true,
    });
}

/// The overlay toggle's hover: how much of the texture the meshes use, the gutter, and any read errors.
pub fn summary(c: &Coverage) -> String {
    let mut s = if failed(c) {
        "UV coverage could not be read:".to_string()
    } else {
        format!(
            "{:.1} % of the texture is used by {} mesh{}",
            c.used * 100.0,
            c.nifs,
            if c.nifs == 1 { "" } else { "es" }
        )
    };
    match c.gutter {
        Some(g) => {
            s += &format!(
                "; {:.1} % of used texels sit closer than 8 texels to another UV island",
                g * 100.0
            )
        }
        None if !failed(c) => s +=
            "; gutter not measured at 4096 px or wider on either axis, or until the size is known",
        None => {}
    }
    for e in c.errors.iter().take(5) {
        s += &format!("\n{e}");
    }
    if c.errors.len() > 5 {
        s += &format!("\n… and {} more", c.errors.len() - 5);
    }
    s
}

/// The answers of the sub-types that have UV coverage: colour textures on meshes. `LandscapeDiffuse` is left out
/// (terrain uses LTEX/TXST records, not NIF shapes).
// shortcut: normals share their set's UVs but are not offered; offer them through their set's colour if asked.
pub(super) fn colour(st: engine::config::Subtype) -> bool {
    use engine::config::Subtype::*;
    matches!(st, Diffuse | PbrAlbedo | SkinAlbedo)
}

// Review M6: one text for a running and a stopped scan; the model cannot tell them apart without an app.rs line.
const SCAN_UNSETTLED: &str =
    "A Meshes scan is running or was stopped: wait for it to finish, or scan again";
const SCAN_FIRST: &str = "Scan your MO2 instance in the Meshes view first";
const COLOUR_ONLY: &str = "Only colour textures on meshes have UV coverage";
const SWITCH_OFF: &str = "Turn on UV coverage check first";

impl super::AppModel {
    /// The scan path the meshes name `item`'s output by, as "Used by meshes" asks for it (`<mesh_key>.dds`).
    pub fn uv_tex(&self, item: &str) -> Option<String> {
        let j = self.item(item)?.job.as_ref()?;
        let stem = j.output.file_stem()?.to_string_lossy().into_owned();
        Some(format!(
            "{}.dds",
            engine::shader::mesh_key(&j.game_path, &stem)
        ))
    }

    /// The written texture's [width, height] (the engine's size rules, so a half size counts); `None` until the
    /// analysis is in, and then nothing is read (review I1).
    fn uv_size(&self, item: &str) -> Option<[u32; 2]> {
        let i = self.item(item)?;
        let (j, a) = (i.job.as_ref()?, i.analysis.as_ref()?);
        let e = super::out_size::estimate((a.width, a.height), &j.profile);
        Some([e.width, e.height])
    }

    /// `item`'s cache key: its scan path and written size; `None` until the analysis is in.
    pub fn uv_key(&self, item: &str) -> Option<(String, [u32; 2])> {
        Some((self.uv_tex(item)?, self.uv_size(item)?))
    }

    /// Why no scan answer can be read now, whatever the texture: a Meshes scan is running or was stopped (BY6,
    /// re-check C12), or there is no scan with rows (no DB, no file, an empty or old-schema DB, or no answer yet).
    fn uv_scan_reason(&self) -> Result<(), &'static str> {
        if self.scan_store_unsettled {
            return Err(SCAN_UNSETTLED);
        }
        let rows = self
            .uv
            .has_rows
            .is_some_and(|(s, r)| r && s == self.scan_stamp);
        if self.scan_db.is_none() || self.scan_stamp.is_none() || !rows {
            return Err(SCAN_FIRST);
        }
        Ok(())
    }

    /// Whether `item` can show UV coverage, and why not. Cached state only: never the file system.
    pub fn uv_offered(&self, item: &str) -> Result<(), &'static str> {
        self.uv_scan_reason()?;
        let st = self
            .item(item)
            .and_then(|i| i.job.as_ref())
            .map(|j| j.profile.subtype);
        if !st.is_some_and(colour) {
            return Err(COLOUR_ONLY);
        }
        Ok(())
    }

    /// Why "Check all UV coverage" is off: the scan reason, the switch off (review M4), or no colour texture in the
    /// folder (O(1) per frame).
    pub fn uv_check_all_offered(&self) -> Result<(), &'static str> {
        self.uv_scan_reason()?;
        if !self.settings.checks.uv {
            return Err(SWITCH_OFF);
        }
        if !self.uv.any_colour {
            return Err(COLOUR_ONLY);
        }
        Ok(())
    }

    /// `item`'s UV coverage from the session cache. A miss starts one worker read per key (none while one is in
    /// flight, none until the analysis gives the written size); `None` until it lands, and always `None` when `item`
    /// is not offered.
    pub fn uv_coverage(&mut self, item: &str) -> Option<Arc<Coverage>> {
        self.uv_offered(item).ok()?;
        let key = self.uv_key(item)?;
        if let Some(e) = self.uv.cache.get(&key) {
            return Some(e.coverage.clone());
        }
        if self.uv.pending.contains_key(&key) {
            return None;
        }
        let db = self.scan_db.clone()?;
        let (generation, read) = (self.uv.generation, self.uv.read);
        let cancel = self.uv.reads_cancel.clone();
        self.uv.pending.insert(key.clone(), generation);
        let (tex, size) = key;
        crate::worker::spawn_guarded(self.tx.clone(), "uv-read", move |tx| {
            // `read_one` turns a panic into this tex's error answer, so the read is not pending forever (K9). Only a
            // clear or a new folder cancels a single read, and either already dropped it from `pending`.
            if let Some((coverage, image)) = read_one(read, &db, &tex, size, &cancel) {
                let _ = tx.send(crate::worker::WorkerMsg::UvCoverage {
                    generation,
                    tex,
                    size,
                    coverage,
                    image,
                });
            }
        });
        None
    }

    /// `item`'s cached answer when its read failed (the toggle is then disabled with it as the hover, review I2).
    /// Starts nothing.
    pub fn uv_failed(&self, item: &str) -> Option<Arc<Coverage>> {
        self.uv_offered(item).ok()?;
        let e = self.uv.cache.get(&self.uv_key(item)?)?;
        failed(&e.coverage).then(|| e.coverage.clone())
    }

    /// `item`'s overlay image from the cache (built on the worker with the coverage, CI29); `None` for a failed
    /// read. Starts nothing.
    pub fn uv_overlay(&self, item: &str) -> Option<Arc<eframe::egui::ColorImage>> {
        self.uv_offered(item).ok()?;
        self.uv.cache.get(&self.uv_key(item)?)?.image.clone()
    }

    /// Read every offered colour texture of the open folder that is not cached yet (C4) and whose size is known, on
    /// `settings.jobs` threads (0 = every core) inside one worker. Nothing to read: one `(0, 0)` and the run ends.
    pub fn check_all_uv(&mut self) {
        if self.uv.all.is_some() {
            return;
        }
        let mut seen = std::collections::HashSet::new();
        let work: Vec<Key> = self
            .items
            .iter()
            .filter(|i| self.uv_offered(&i.key).is_ok())
            .filter_map(|i| self.uv_key(&i.key))
            .filter(|k| !self.uv.cache.contains_key(k) && seen.insert(k.clone()))
            .collect();
        self.uv.runs += 1;
        let cancel = Arc::new(AtomicBool::new(false));
        self.uv.all = Some(AllRun {
            run: self.uv.runs,
            cancel: cancel.clone(),
            done: 0,
            total: work.len(),
        });
        let jobs = match self.settings.jobs {
            0 => std::thread::available_parallelism().map_or(4, |n| n.get()),
            n => n,
        };
        let job = AllJob {
            run: self.uv.runs,
            generation: self.uv.generation,
            db: self.scan_db.clone().unwrap_or_default(),
            work,
            jobs,
            read: self.uv.read,
        };
        crate::worker::spawn_guarded(self.tx.clone(), "uv-all", move |tx| {
            let (run, total) = (job.run, job.work.len());
            // A panic outside the reads still ends the run (K9).
            crate::worker::or_send(
                tx,
                || check_all_run(tx, &job, &cancel),
                |_| crate::worker::WorkerMsg::UvProgress {
                    run,
                    done: 0,
                    total,
                    finished: true,
                },
            );
        });
    }

    /// Stop Check all: the reads in flight return early, and the run ends when they have.
    pub fn cancel_check_all_uv(&mut self) {
        if let Some(a) = &self.uv.all {
            a.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Start the scan-has-rows probe when the scan DB's stamp has changed since the last one, unless a Meshes scan
    /// is unsettled (A12, C12). A new stamp also clears the cache (K18: errors are cached until then) and cancels a
    /// Check all (review M5). Landed Check-all answers re-run the checks at most every `REFRESH_EVERY` (M8).
    pub(super) fn uv_poll(&mut self) {
        if self.uv.stamp != self.scan_stamp {
            self.uv.stamp = self.scan_stamp;
            if !self.uv.cache.is_empty() || self.uv.read_pending() || self.uv.all_running() {
                self.uv.clear();
            }
        }
        if self.uv.dirty && self.uv.bumped.is_none_or(|t| t.elapsed() >= REFRESH_EVERY) {
            self.uv.bump();
        }
        let answered = self.uv.has_rows.is_some_and(|(s, _)| s == self.scan_stamp);
        if self.scan_store_unsettled
            || self.scan_stamp.is_none()
            || answered
            || self.uv.probing == Some(self.scan_stamp)
        {
            return;
        }
        let Some(db) = self.scan_db.clone() else {
            return;
        };
        let stamp = self.scan_stamp;
        self.uv.probing = Some(stamp);
        crate::worker::spawn_guarded(self.tx.clone(), "uv-probe", move |tx| {
            let rows = crate::worker::or_send(
                tx,
                || scan_has_rows(&db),
                |_| crate::worker::WorkerMsg::ScanHasRows { stamp, rows: false },
            );
            let _ = tx.send(crate::worker::WorkerMsg::ScanHasRows { stamp, rows });
        });
    }

    pub(super) fn uv_rows_landed(&mut self, stamp: super::cs_pbr::Stamp, rows: bool) {
        if self.uv.probing == Some(stamp) {
            self.uv.probing = None;
            self.uv.has_rows = Some((stamp, rows));
        }
    }

    pub(super) fn uv_landed(
        &mut self,
        generation: u64,
        key: Key,
        coverage: Arc<Coverage>,
        image: Option<Arc<eframe::egui::ColorImage>>,
    ) {
        let single = self.uv.pending.get(&key) == Some(&generation);
        if single {
            self.uv.pending.remove(&key);
        }
        if generation != self.uv.generation {
            return;
        }
        self.uv.cache.insert(key, Entry { coverage, image });
        // M8: a single read (or an injected answer) re-runs the checks now; Check all at most every 250 ms.
        self.uv.dirty = true;
        if single || !self.uv.all_running() {
            self.uv.bump();
        }
    }

    pub(super) fn uv_progress(&mut self, run: u64, done: usize, total: usize, finished: bool) {
        let Some(a) = self.uv.all.as_mut().filter(|a| a.run == run) else {
            return;
        };
        if finished {
            self.uv.all = None;
            if self.uv.dirty {
                self.uv.bump();
            }
        } else {
            (a.done, a.total) = (done.max(a.done), total);
        }
    }
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
    fn the_axis_wraps_into_the_mask() {
        assert_eq!(axis(8, 0.9, 1.2, false), vec![0, 1, 7]);
        assert_eq!(axis(8, 0.0, 0.25, false), vec![0, 1]);
        assert_eq!(axis(8, 0.1, 0.12, false), Vec::<u32>::new());
        assert_eq!(axis(4, 3.0, 3.1, true), vec![0, 1, 2, 3]);
    }

    #[test]
    #[should_panic(expected = "outside")]
    #[cfg(debug_assertions)]
    fn a_texel_past_the_mask_is_caught_in_debug() {
        UvMask::new(4).get(4, 0);
    }
}
