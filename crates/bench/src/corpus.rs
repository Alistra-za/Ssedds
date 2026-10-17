use anyhow::{anyhow, bail, Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The corpus root, or `None` when `SDC_BENCH_ROOT` is unset (a fresh clone).
pub fn corpus_root() -> Option<PathBuf> {
    std::env::var_os("SDC_BENCH_ROOT").map(PathBuf::from)
}

/// `SDC_BENCH_ROOT`, or a relative `bench` (nothing there on a fresh clone; the gated tests check `corpus_root` first).
pub fn bench_root() -> PathBuf {
    corpus_root().unwrap_or_else(|| PathBuf::from("bench"))
}

pub struct Rgba8 {
    pub width: u32,
    pub height: u32,
    pub px: Vec<[u8; 4]>,
}

/// Same as PIL `Image.open(p).convert("RGBA")` for the bench's 8-bit PNGs.
pub fn load_png_rgba8(path: &Path) -> Result<Rgba8> {
    let img = image::open(path)
        .with_context(|| path.display().to_string())?
        .to_rgba8();
    let (width, height) = img.dimensions();
    let px = img.pixels().map(|p| p.0).collect();
    Ok(Rgba8 { width, height, px })
}

pub struct HalfImage {
    pub width: u32,
    pub height: u32,
    pub px: Vec<[u16; 4]>,
}

/// Bench `.f16`: magic "F16\0" (0x00363146), u32 width, u32 height, then RGBA half floats.
pub fn load_f16(path: &Path) -> Result<HalfImage> {
    let b = std::fs::read(path).with_context(|| path.display().to_string())?;
    if b.len() < 12 {
        bail!("{}: not an F16 file", path.display());
    }
    let u = |i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
    if u(0) != 0x0036_3146 {
        bail!("{}: not an F16 file", path.display());
    }
    let (width, height) = (u(4), u(8));
    let n = width as usize * height as usize;
    if b.len() != 12 + n * 8 {
        bail!("{}: truncated", path.display());
    }
    let px = (0..n)
        .map(|i| {
            let o = 12 + i * 8;
            [0, 1, 2, 3].map(|c| u16::from_le_bytes([b[o + c * 2], b[o + c * 2 + 1]]))
        })
        .collect();
    Ok(HalfImage { width, height, px })
}

pub fn stem(p: &Path) -> String {
    p.file_stem().unwrap().to_string_lossy().into_owned()
}

pub fn set_files(root: &Path, dir: &str, set: &str, ext: &str) -> Result<Vec<PathBuf>> {
    let d = root.join(dir).join(set);
    let mut v: Vec<PathBuf> = std::fs::read_dir(&d)
        .with_context(|| d.display().to_string())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)))
        .collect();
    v.sort();
    Ok(v)
}

pub struct ResultRow {
    pub variant: String,
    pub set: String,
    pub image: String,
    pub values: BTreeMap<String, String>,
}

impl ResultRow {
    pub fn f(&self, key: &str) -> Option<f64> {
        self.values.get(key).and_then(|v| v.parse().ok())
    }
}

/// Reads results.csv / results2.csv (no quoted fields in successful rows; rows with errors are skipped).
pub fn read_results(path: &Path, variant: &str) -> Result<Vec<ResultRow>> {
    let text = std::fs::read_to_string(path).with_context(|| path.display().to_string())?;
    let mut lines = text.lines();
    let header: Vec<&str> = lines
        .next()
        .ok_or_else(|| anyhow!("empty csv"))?
        .split(',')
        .collect();
    let mut rows = vec![];
    for line in lines {
        let f: Vec<&str> = line.split(',').collect();
        if f.len() != header.len() || f[0] != variant || !f[header.len() - 1].is_empty() {
            continue;
        }
        let values: BTreeMap<String, String> = header
            .iter()
            .zip(&f)
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        rows.push(ResultRow {
            variant: f[0].into(),
            set: f[2].into(),
            image: f[3].into(),
            values,
        });
    }
    Ok(rows)
}

pub fn find<'a>(rows: &'a [ResultRow], set: &str, image: &str) -> Option<&'a ResultRow> {
    rows.iter().find(|r| r.set == set && r.image == image)
}
