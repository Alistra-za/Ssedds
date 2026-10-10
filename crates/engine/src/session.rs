//! A GUI session over the engine (M3a spec §4.4, §4.5): an `Engine`, the encoders initialised once with the
//! session's BC1 model, and an `AnalysisCache` so re-planning after an edit or a target change reloads only what
//! changed.
use crate::analyse::{code8, Analysis};
use crate::api::{BatchRequest, BatchSummary, CancelToken, Engine, EngineError, Plan};
use crate::events::Reporter;
use crate::image::{Rgba32F, SourceInfo};
use crate::manifest::sha256_json;
use crate::pack::PackJob;
use crate::policy::{ResolvedJob, SourceStamp};
use encoders_sys::Bc1Model;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Largest side of a thumbnail.
pub const THUMB_MAX: u32 = 256;

/// A display-ready RGBA8 thumbnail (straight alpha).
#[derive(Debug, Clone, PartialEq)]
pub struct Thumbnail {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// Box-downscale `img` so its larger side is at most `THUMB_MAX`. A float source holds linear radiance, so its
/// RGB is tone-mapped (`x / (1 + x)`) and sRGB-encoded; any other source is shown as stored, clamped to [0, 1].
pub fn make_thumbnail(img: &Rgba32F, info: &SourceInfo) -> Thumbnail {
    let step = img.width.max(img.height).div_ceil(THUMB_MAX).max(1);
    let (w, h) = (img.width.div_ceil(step), img.height.div_ceil(step));
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for ty in 0..h {
        for tx in 0..w {
            let mut acc = [0f32; 4];
            let mut n = 0f32;
            for y in ty * step..((ty + 1) * step).min(img.height) {
                for x in tx * step..((tx + 1) * step).min(img.width) {
                    let p = img.px[(y * img.width + x) as usize];
                    for c in 0..4 {
                        acc[c] += if p[c].is_finite() { p[c] } else { 0.0 };
                    }
                    n += 1.0;
                }
            }
            for (c, a) in acc.iter().enumerate() {
                let v = a / n;
                let v = if info.is_float && c < 3 {
                    linear_to_srgb(v.max(0.0) / (1.0 + v.max(0.0)))
                } else {
                    v
                };
                rgba.push(code8(v));
            }
        }
    }
    Thumbnail {
        width: w,
        height: h,
        rgba,
    }
}

#[derive(Clone)]
struct FileEntry {
    stamp: SourceStamp,
    analysis: Analysis,
    info: SourceInfo,
    thumb: Option<Thumbnail>,
}

/// A packed image's cached analysis, with what `finish_recipe` needs besides it.
#[derive(Clone)]
pub(crate) struct PackEntry {
    pub analysis: Analysis,
    pub info: SourceInfo,
    pub non_gray: Vec<PathBuf>,
    pub thumb: Option<Thumbnail>,
}

/// Each source's analysis (keyed by path, valid while its size and mtime match) and each packed image's analysis
/// (keyed by `pack_key`, which includes every source's stamp).
#[derive(Default)]
pub struct AnalysisCache {
    /// Whether to build a thumbnail beside each analysis. Off by default, so the throwaway cache `Engine::plan`
    /// uses (the CLI) never pays for a downscale; `Session::new` turns it on.
    pub thumbnails: bool,
    files: Mutex<HashMap<PathBuf, FileEntry>>,
    packs: Mutex<HashMap<String, PackEntry>>,
    loads: AtomicU64,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The cache key of a packed image: everything that shapes it, including each source's stamp.
pub fn pack_key(pk: &PackJob) -> String {
    sha256_json(pk).unwrap_or_default()
}

impl AnalysisCache {
    /// Images decoded so far (a packed image counts each of its sources).
    pub fn loads(&self) -> u64 {
        self.loads.load(Ordering::Relaxed)
    }
    pub(crate) fn count_loads(&self, n: u64) {
        self.loads.fetch_add(n, Ordering::Relaxed);
    }
    /// The thumbnail of `img` when this cache makes them.
    pub(crate) fn make_thumb(&self, img: &Rgba32F, info: &SourceInfo) -> Option<Thumbnail> {
        self.thumbnails.then(|| make_thumbnail(img, info))
    }
    pub(crate) fn file(&self, path: &Path, stamp: SourceStamp) -> Option<(Analysis, SourceInfo)> {
        lock(&self.files)
            .get(path)
            .filter(|e| e.stamp == stamp)
            .map(|e| (e.analysis.clone(), e.info))
    }
    pub(crate) fn put_file(
        &self,
        path: &Path,
        stamp: SourceStamp,
        analysis: Analysis,
        info: SourceInfo,
        thumb: Option<Thumbnail>,
    ) {
        lock(&self.files).insert(
            path.to_path_buf(),
            FileEntry {
                stamp,
                analysis,
                info,
                thumb,
            },
        );
    }
    pub(crate) fn pack(&self, key: &str) -> Option<PackEntry> {
        lock(&self.packs).get(key).cloned()
    }
    pub(crate) fn put_pack(&self, key: String, e: PackEntry) {
        lock(&self.packs).insert(key, e);
    }
    /// The source file's thumbnail, if the cache holds one.
    pub fn thumbnail_of(&self, path: &Path) -> Option<Thumbnail> {
        lock(&self.files).get(path).and_then(|e| e.thumb.clone())
    }
    /// The job's thumbnail: its packed image's for a packed job, else its source's.
    pub fn thumbnail(&self, job: &ResolvedJob) -> Option<Thumbnail> {
        match &job.pack {
            Some(pk) => lock(&self.packs)
                .get(&pack_key(pk))
                .and_then(|e| e.thumb.clone()),
            None => self.thumbnail_of(&job.input),
        }
    }
}

/// An engine for one GUI window: encoders initialised once (the BC1 model is process-global, spec §4.6).
pub struct Session {
    engine: Engine,
    cache: AnalysisCache,
    bc1_model: Bc1Model,
}

impl Session {
    pub fn new(bc1_model: Bc1Model) -> Result<Session, EngineError> {
        encoders_sys::init(bc1_model).map_err(|e| EngineError::Fatal(e.to_string()))?;
        Ok(Session {
            engine: Engine::new()?,
            cache: AnalysisCache {
                thumbnails: true,
                ..Default::default()
            },
            bc1_model,
        })
    }
    pub fn bc1_model(&self) -> Bc1Model {
        self.bc1_model
    }
    pub fn engine(&self) -> &Engine {
        &self.engine
    }
    pub fn cache(&self) -> &AnalysisCache {
        &self.cache
    }
    /// `Engine::plan` through the cache; the request's BC1 model is the session's.
    pub fn plan(
        &self,
        mut req: BatchRequest,
        rep: &Reporter,
        cancel: &CancelToken,
    ) -> Result<Plan, EngineError> {
        req.bc1_model = self.bc1_model;
        self.engine.plan_cached(req, rep, cancel, &self.cache)
    }
    pub fn run(
        &self,
        plan: Plan,
        rep: &Reporter,
        cancel: &CancelToken,
    ) -> Result<BatchSummary, EngineError> {
        self.engine.run(plan, rep, cancel)
    }
    pub fn thumbnail(&self, job: &ResolvedJob) -> Option<Thumbnail> {
        self.cache.thumbnail(job)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn session_is_send_and_sync() {
        fn check<T: Send + Sync>() {}
        check::<super::Session>();
    }
}
