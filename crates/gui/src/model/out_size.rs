//! Item 22 (DECISIONS AG118-AG120): the size of the DDS a job writes, from the engine's own size rules
//! (`resize::target_dims`, `mips::level_count`, `DdsPixelFormat::level_size`, `resolve_header`). A compressed
//! size is also its video memory, so the totals are labelled "≈ VRAM".
use crate::model::AppModel;
use engine::config::{Format, Profile};
use engine::dds::{resolve_header, ResolvedHeader};
use std::collections::HashMap;

/// Magic, the 124-byte header, and the 20-byte DX10 block when the header has one.
const HEADER_LEGACY: u64 = 128;
const HEADER_DX10: u64 = 148;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SizeEst {
    pub format: Format,
    pub width: u32,
    pub height: u32,
    pub mips: u32,
    /// All mip levels, no header.
    pub chain_bytes: u64,
    pub header_bytes: u64,
}

impl SizeEst {
    /// The file size.
    pub fn file_bytes(&self) -> u64 {
        self.chain_bytes + self.header_bytes
    }
}

/// The DDS of a `src`-sized image under `p`.
pub fn estimate(src: (u32, u32), p: &Profile) -> SizeEst {
    let (w, h) = engine::resize::target_dims(src, p.resize);
    let (w, h) = (w.max(1), h.max(1));
    let mips = engine::mips::level_count(w, h, &p.mips.levels);
    let pf = p.format.pixel_format(p.srgb_tag());
    let chain_bytes = (0..mips)
        .map(|i| pf.level_size((w >> i).max(1), (h >> i).max(1)) as u64)
        .sum();
    let header_bytes = match resolve_header(p.format, p.srgb_tag(), p.header) {
        ResolvedHeader::Dx10 => HEADER_DX10,
        _ => HEADER_LEGACY,
    };
    SizeEst {
        format: p.format,
        width: w,
        height: h,
        mips,
        chain_bytes,
        header_bytes,
    }
}

/// "BC7".
pub fn format_name(f: Format) -> String {
    serde_json::to_value(f)
        .ok()
        .and_then(|v| v.as_str().map(str::to_uppercase))
        .unwrap_or_else(|| format!("{f:?}"))
}

/// "5.3 MiB", "148 B".
pub fn human(bytes: u64) -> String {
    const U: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", U[i])
    }
}

/// The source size of an image, from its header only. `None` for a format the reader does not know (PSD) or an
/// unreadable file: that texture then shows no estimate.
/// shortcut: PSD sources have no size here; read the PSD header when they matter.
pub fn source_dims(path: &std::path::Path) -> Option<(u32, u32)> {
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("dds"))
    {
        use std::io::Read;
        let mut b = [0u8; 20];
        std::fs::File::open(path).ok()?.read_exact(&mut b).ok()?;
        let at = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        return Some((at(16), at(12)));
    }
    image::image_dimensions(path).ok()
}

/// A source file and the (size, mtime) stamp the plan read it at.
type DimKey = (std::path::PathBuf, Option<(u64, u64)>);

/// Per-item sizes by plan revision (`AppModel::items_rev`): the source dimensions are read once per file and
/// kept; the sizes are rebuilt when the revision moves.
#[derive(Default)]
pub struct SizeCache {
    rev: Option<u64>,
    /// By file and the stamp the plan read it at: a changed file is read again.
    dims: HashMap<DimKey, Option<(u32, u32)>>,
    folder: Option<(u64, (u64, usize))>,
    sizes: HashMap<String, Option<SizeEst>>,
    /// Source-header reads so far (tests).
    pub reads: u32,
    /// Rebuilds so far (tests).
    pub builds: u32,
}

impl SizeCache {
    /// The estimate of item `key` for plan revision `rev`.
    pub fn item(&mut self, m: &AppModel, key: &str) -> Option<SizeEst> {
        self.sync(m.items_rev());
        if let Some(e) = self.sizes.get(key) {
            return *e;
        }
        let e = self.build(m, key);
        self.sizes.insert(key.to_string(), e);
        e
    }

    fn sync(&mut self, rev: u64) {
        if self.rev != Some(rev) {
            self.rev = Some(rev);
            self.sizes.clear();
            self.folder = None;
            self.builds += 1;
        }
    }

    fn build(&mut self, m: &AppModel, key: &str) -> Option<SizeEst> {
        let item = m.item(key)?;
        let job = item.job.as_ref()?;
        let src = &job.input;
        let stamp = job.source.map(|s| (s.size, s.mtime_ns));
        let reads = &mut self.reads;
        let dims = *self.dims.entry((src.clone(), stamp)).or_insert_with(|| {
            *reads += 1;
            source_dims(src)
        });
        Some(estimate(dims?, &job.profile))
    }

    /// Sum over `keys` (items without an estimate count as 0) and how many had none.
    pub fn total<'a>(
        &mut self,
        m: &AppModel,
        keys: impl IntoIterator<Item = &'a str>,
    ) -> (u64, usize) {
        let (mut sum, mut missing) = (0, 0);
        for k in keys {
            match self.item(m, k) {
                Some(e) => sum += e.file_bytes(),
                None => missing += 1,
            }
        }
        (sum, missing)
    }
}

impl AppModel {
    /// The estimate of item `key`, cached by plan revision.
    pub fn size_of(&mut self, key: &str) -> Option<SizeEst> {
        let mut c = std::mem::take(&mut self.sizes);
        let e = c.item(self, key);
        self.sizes = c;
        e
    }

    /// Total file bytes over `keys` and how many had no estimate (a source the header reader cannot size).
    pub fn size_total(&mut self, keys: &[String]) -> (u64, usize) {
        let mut c = std::mem::take(&mut self.sizes);
        let t = c.total(self, keys.iter().map(String::as_str));
        self.sizes = c;
        t
    }

    /// The folder's total: every texture that is written, or will be once decided (a job that is Ready or
    /// NeedsDecision). Cached by plan revision.
    pub fn folder_size(&mut self) -> (u64, usize) {
        let rev = self.items_rev();
        if let Some((r, t)) = self.sizes.folder {
            if r == rev {
                return t;
            }
        }
        let keys: Vec<String> = self
            .items
            .iter()
            .filter(|i| {
                i.job.is_some()
                    && matches!(
                        i.status,
                        crate::model::items::ItemStatus::Ready
                            | crate::model::items::ItemStatus::NeedsDecision
                    )
            })
            .map(|i| i.key.clone())
            .collect();
        let t = self.size_total(&keys);
        self.sizes.folder = Some((rev, t));
        t
    }
}

fn with_missing(bytes: u64, missing: usize) -> String {
    match missing {
        0 => human(bytes),
        n => format!("{} (+{n} unsized)", human(bytes)),
    }
}

impl AppModel {
    /// "≈ VRAM: selection 4.0 MiB · folder 120.0 MiB" (the selection part only when `selection` has items): the
    /// header's and the bulk panel's line (DECISIONS AG119).
    pub fn vram_text(&mut self, selection: &[String]) -> String {
        let (f, fm) = self.folder_size();
        let folder = format!("folder {}", with_missing(f, fm));
        if selection.is_empty() {
            return format!("≈ VRAM: {folder}");
        }
        let (s, sm) = self.size_total(selection);
        format!("≈ VRAM: selection {} · {folder}", with_missing(s, sm))
    }
}
