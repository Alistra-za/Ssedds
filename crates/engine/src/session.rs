//! A GUI session over the engine (M3a spec §4.4, §4.5): an `Engine`, the encoders initialised once with the
//! session's BC1 model, and an `AnalysisCache` so re-planning after an edit or a target change reloads only what
//! changed.
use crate::analyse::{code8, Analysis};
use crate::api::{lock, BatchRequest, BatchSummary, CancelToken, Engine, EngineError, Plan};
use crate::codes::Warning;
use crate::config::{Category, Format};
use crate::dds::{dds_bytes, read_dds, resolve_header, DdsDesc};
use crate::decode::{decode_level, Decoded};
use crate::events::{NullSink, Reporter, Stage};
use crate::image::{Rgba32F, SourceInfo};
use crate::manifest::sha256_json;
use crate::ops::{linear_to_srgb, Domain};
use crate::pack::PackJob;
use crate::pipeline::{encode_levels, prepare, FileError, FileTracker, Keep};
use crate::policy::{ResolvedJob, SourceStamp};
use crate::quantize::stored_rgb;
use crate::validate::{validate, ValidationInput, ValidationSummary};
use encoders_sys::Bc1Model;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Largest side of a thumbnail (DECISIONS AV2: 128, a 64 KiB RGBA8 tile per file; grid tiles are 64-192 pt).
pub const THUMB_MAX: u32 = 128;

/// A display-ready RGBA8 thumbnail (straight alpha).
#[derive(Debug, Clone, PartialEq)]
pub struct Thumbnail {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
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
    thumb: Option<Arc<Thumbnail>>,
}

/// A packed image's cached analysis, with what `finish_recipe` needs besides it.
#[derive(Clone)]
pub(crate) struct PackEntry {
    pub analysis: Analysis,
    pub info: SourceInfo,
    pub non_gray: Vec<PathBuf>,
    pub thumb: Option<Arc<Thumbnail>>,
}

/// Each source's analysis (keyed by path, valid while its size and mtime match) and each packed image's analysis
/// (keyed by `pack_key`, which includes every source's stamp).
#[derive(Default)]
pub struct AnalysisCache {
    /// Whether to build a thumbnail beside each analysis. Off by default, so the throwaway cache `Engine::plan`
    /// uses (the CLI) never pays for a downscale; `Session::new` turns it on.
    pub thumbnails: bool,
    /// Whether each analysis carries the artist check stats (DECISIONS Note CI). Off by default like `thumbnails`,
    /// so the CLI computes none; `Session::new` turns it on.
    pub checks: bool,
    files: Mutex<HashMap<PathBuf, FileEntry>>,
    packs: Mutex<HashMap<String, PackEntry>>,
    loads: AtomicU64,
}

/// The key of a packed image: everything that shapes it, including each source's stamp. `None` when the job cannot
/// be serialised.
pub fn pack_key(pk: &PackJob) -> Option<String> {
    sha256_json(pk).ok()
}

/// The key the pass-2 pack cache uses, or `None` when the image must not be cached: the key is unavailable, or a
/// source has no stamp (it could not be read when the job was planned), so nothing says when it changed.
pub fn pack_cache_key(pk: &PackJob) -> Option<String> {
    pk.sources
        .iter()
        .all(|s| s.stamp.is_some())
        .then(|| pack_key(pk))
        .flatten()
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
    pub(crate) fn make_thumb(&self, img: &Rgba32F, info: &SourceInfo) -> Option<Arc<Thumbnail>> {
        self.thumbnails.then(|| Arc::new(make_thumbnail(img, info)))
    }
    /// The pixel size of the source analysed at `path` while its size and mtime still match `stamp`; no file is read.
    pub fn dims(&self, path: &Path, stamp: SourceStamp) -> Option<(u32, u32)> {
        lock(&self.files)
            .get(path)
            .filter(|e| e.stamp == stamp)
            .map(|e| (e.analysis.width, e.analysis.height))
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
        thumb: Option<Arc<Thumbnail>>,
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
    /// Audit 8 (Note Y7): keep only what this plan can use. Files outside its inputs go; packs go only after an
    /// uncancelled plan (the caller checks), which saw everything it needs.
    pub(crate) fn retain_plan(&self, plan: &Plan) {
        let roots = &plan.request.inputs;
        lock(&self.files).retain(|p, _| roots.iter().any(|r| p.starts_with(r)));
        let keep: std::collections::HashSet<String> = plan
            .jobs
            .iter()
            .filter_map(|j| j.pack.as_ref())
            .filter_map(pack_cache_key)
            .collect();
        lock(&self.packs).retain(|k, _| keep.contains(k));
    }
    /// The source file's thumbnail, if the cache holds one.
    pub fn thumbnail_of(&self, path: &Path) -> Option<Arc<Thumbnail>> {
        lock(&self.files).get(path).and_then(|e| e.thumb.clone())
    }
    /// The job's thumbnail: its packed image's for a packed job, else its source's.
    pub fn thumbnail(&self, job: &ResolvedJob) -> Option<Arc<Thumbnail>> {
        match &job.pack {
            Some(pk) => {
                let key = pack_cache_key(pk)?;
                lock(&self.packs).get(&key).and_then(|e| e.thumb.clone())
            }
            None => self.thumbnail_of(&job.input),
        }
    }
    /// The job's analysis: its packed image's for a packed job, else its source's. Like [`Self::thumbnail`], it
    /// ignores stamps.
    pub fn analysis(&self, job: &ResolvedJob) -> Option<Analysis> {
        match &job.pack {
            Some(pk) => {
                let key = pack_cache_key(pk)?;
                lock(&self.packs).get(&key).map(|e| e.analysis.clone())
            }
            None => lock(&self.files)
                .get(&job.input)
                .map(|e| e.analysis.clone()),
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
                checks: true,
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
        let plan = self.engine.plan_cached(req, rep, cancel, &self.cache)?;
        // A cancelled plan saw only part of its folder (and may be superseded): it must not evict a live one.
        if plan.cancelled.is_empty() {
            self.cache.retain_plan(&plan);
        }
        Ok(plan)
    }
    pub fn run(
        &self,
        plan: Plan,
        rep: &Reporter,
        cancel: &CancelToken,
    ) -> Result<BatchSummary, EngineError> {
        self.engine.run(plan, rep, cancel)
    }
    pub fn thumbnail(&self, job: &ResolvedJob) -> Option<Arc<Thumbnail>> {
        self.cache.thumbnail(job)
    }

    /// The float chain the encoder works on (M3a spec §6.2), in its working domain (linear light for sRGB colour,
    /// −1..1 for vectors), from the first mip within `max_size`. Nothing is encoded or written.
    /// Test/golden use; the GUI uses [`Self::preview_source_stored`] (DECISIONS AV5).
    #[doc(hidden)]
    pub fn preview_source(
        &self,
        job: &ResolvedJob,
        max_size: u32,
        cancel: &CancelToken,
    ) -> Result<SourcePreview, FileError> {
        let prep = prepare(
            job,
            cancel,
            &FileTracker::default(),
            Keep::SourceOnly { max_size },
        )?;
        Ok(SourcePreview {
            first_level: prep.first_level,
            dims: prep.dims,
            levels: prep.chain,
        })
    }

    /// The source side the preview shows (DECISIONS Note AQ): [`Self::preview_source`] in stored values, mapped as
    /// the encoder input is, so it compares with the decoded encode.
    pub fn preview_source_stored(
        &self,
        job: &ResolvedJob,
        max_size: u32,
        cancel: &CancelToken,
    ) -> Result<SourcePreview, FileError> {
        let prep = prepare(
            job,
            cancel,
            &FileTracker::default(),
            Keep::SourceOnly { max_size },
        )?;
        Ok(stored_source(
            prep.first_level,
            prep.dims,
            prep.chain,
            prep.domain,
        ))
    }

    /// Source and encoded sides (M3a spec §4.5): every mip within `max_size`, byte-identical to the same mips of
    /// the DDS `run` writes, with the validation readouts of those mips (mip indices in the readouts count from
    /// the first mip kept; the checks that look at "mip 0" alpha or the BC6H peak read the first mip kept, with the
    /// peak measured on that same mip, so they describe the preview, not the export's mip 0). Nothing is written.
    pub fn preview(
        &self,
        job: &ResolvedJob,
        max_size: u32,
        cancel: &CancelToken,
    ) -> Result<PreviewResult, FileError> {
        let rep = Reporter::new(Arc::new(NullSink), "preview");
        // DECISIONS Note NV9: NVTT is 40-60x slower, so a texture on NVTT previews with the format's built-in
        // encoder; the export still uses NVTT. The caller tells from `job.profile.encoder.backend` and says so.
        let built_in;
        let p = if job.profile.encoder.backend == crate::config::Backend::Nvtt {
            let mut b = job.profile.clone();
            b.encoder.backend = crate::policy::default_backend(b.format);
            built_in = b;
            &built_in
        } else {
            &job.profile
        };
        let prep = prepare(
            job,
            cancel,
            &FileTracker::default(),
            Keep::Preview { max_size },
        )?;
        let levels = encode_levels(&prep, p, cancel, &rep, "", None, None, None)?;
        let srgb = p.srgb_tag();
        let (width, height) = prep.dims[prep.first_level];
        let desc = DdsDesc {
            width,
            height,
            format: p.format,
            srgb,
            header: resolve_header(p.format, srgb, p.header),
        };
        let bytes = dds_bytes(&desc, &levels)
            .map_err(|e| FileError::Failed(Stage::Write, e.to_string()))?;
        let report = validate(&ValidationInput {
            bytes: &bytes,
            profile: p,
            encoder_input: Some(&prep.inputs),
            base_coverage: prep.base_coverage,
            source_peak: prep.source_peak,
            bc1_model: self.bc1_model,
            expected_levels: Some(levels.len() as u32),
        });
        let normal_len = if p.subtype.category() == Category::Normal {
            normal_lengths(&bytes, levels.len(), self.bc1_model)
        } else {
            vec![]
        };
        Ok(PreviewResult {
            source: stored_source(prep.first_level, prep.dims, prep.chain, prep.domain),
            format: p.format,
            srgb,
            levels,
            validation: report.summary,
            warnings: report.warnings,
            normal_len,
        })
    }
}

/// The source side in stored values, mapped as the encoder input is (Note AQ).
fn stored_source(
    first_level: usize,
    dims: Vec<(u32, u32)>,
    mut levels: Vec<Rgba32F>,
    domain: Domain,
) -> SourcePreview {
    for p in levels.iter_mut().flat_map(|l| l.px.iter_mut()) {
        let rgb = stored_rgb(p, domain);
        p[..3].copy_from_slice(&rgb);
    }
    SourcePreview {
        first_level,
        dims,
        levels,
    }
}

/// The first mip whose larger side is at most `max_size`; the last mip when none is (M3a spec §6.5).
pub fn first_level_within(dims: &[(u32, u32)], max_size: u32) -> usize {
    dims.iter()
        .position(|&(w, h)| w.max(h) <= max_size)
        .unwrap_or(dims.len().saturating_sub(1))
}

/// The source side of a preview: `dims` of every mip, `levels` from `first_level` on (the chain after Preprocess,
/// Mipmaps and coverage). `Session::preview` and `preview_source_stored` hold stored values (Note AQ);
/// `preview_source` the working domain.
#[derive(Debug, Clone)]
pub struct SourcePreview {
    pub first_level: usize,
    pub dims: Vec<(u32, u32)>,
    pub levels: Vec<Rgba32F>,
}

/// How far a decoded normal map's vectors are from unit length, for one mip. Both fields are NaN for a mip that
/// could not be decoded, so `PreviewResult::normal_len` always has one entry per encoded level.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct NormalLenStats {
    pub mean_err: f64,
    pub max_err: f64,
}

/// `levels[i]` is mip `source.first_level + i`'s block data.
#[derive(Debug, Clone)]
pub struct PreviewResult {
    pub source: SourcePreview,
    pub format: Format,
    pub srgb: bool,
    pub levels: Vec<Vec<u8>>,
    pub validation: ValidationSummary,
    pub warnings: Vec<Warning>,
    pub normal_len: Vec<NormalLenStats>,
}

/// Per mip: decode with the engine's decoder and measure |length - 1| of RGB read as `2v - 1` (for BC5, whose blue
/// is reconstructed, the excess `max(0, x^2 + y^2 - 1)`).
fn normal_lengths(dds_bytes: &[u8], levels: usize, bc1_model: Bc1Model) -> Vec<NormalLenStats> {
    let unmeasured = NormalLenStats {
        mean_err: f64::NAN,
        max_err: f64::NAN,
    };
    let Ok(f) = read_dds(dds_bytes) else {
        return vec![unmeasured; levels];
    };
    let bc5 = matches!(f.format.as_format(), Some((Format::Bc5, _)));
    f.levels
        .iter()
        .map(|(w, h, r)| decode_level(f.format, *w, *h, &f.data[r.clone()], Some(bc1_model)))
        .map(|d| {
            let Ok(Decoded::Ldr(px)) = d else {
                return unmeasured;
            };
            let (mut sum, mut max) = (0f64, 0f64);
            for q in &px {
                let v = |i: usize| q[i] as f64 / 255.0 * 2.0 - 1.0;
                let e = if bc5 {
                    (v(0) * v(0) + v(1) * v(1) - 1.0).max(0.0)
                } else {
                    ((v(0) * v(0) + v(1) * v(1) + v(2) * v(2)).sqrt() - 1.0).abs()
                };
                sum += e;
                max = max.max(e);
            }
            NormalLenStats {
                mean_err: sum / px.len().max(1) as f64,
                max_err: max,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn session_is_send_and_sync() {
        fn check<T: Send + Sync>() {}
        check::<super::Session>();
    }

    /// Note AQ: the stored source of MSN, flow and complex grass is the encoder input before rounding. Grass builds
    /// its own work (`Stored`); the source takes the domain from the work, not from the profile.
    #[test]
    fn the_stored_source_is_the_encoder_input() {
        use super::*;
        use crate::api::{Engine, OnExisting};
        use crate::config::{Subtype, Target};
        use crate::policy::AcceptSuggestions;
        use crate::quantize::{unorm8, EncoderInput};
        use image::{ImageBuffer, Rgb, Rgba};
        let d = tempfile::tempdir().unwrap();
        let put = |rel: &str| {
            let p = d.path().join("in/textures").join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            p
        };
        let tilted = |x: u32, _| Rgb([if x.is_multiple_of(2) { 204u8 } else { 51 }, 128, 230]);
        for rel in ["a/w_msn.png", "hair/h_flow.png"] {
            ImageBuffer::from_fn(64, 64, tilted).save(put(rel)).unwrap();
        }
        // Top half colour with a cutout alpha, bottom half normals with specular in alpha.
        ImageBuffer::from_fn(32, 64, |x, y| {
            if y < 32 {
                Rgba([64, 128, 38, if (x * 7 + y * 13) % 10 < 3 { 255 } else { 0 }])
            } else {
                Rgba([160 + (x % 8) as u8, 128, 230, 12])
            }
        })
        .save(put("plants/g.png"))
        .unwrap();
        let mut r = BatchRequest::new(vec![d.path().join("in")], d.path().join("out"), Target::Cs);
        r.recursive = true;
        r.accept = AcceptSuggestions::High;
        r.on_existing = OnExisting::Overwrite;
        r.overrides = [
            "file:**/g.png.subtype=complex_grass",
            "file:**/g.png.alpha.policy=cutout",
        ]
        .iter()
        .map(|s| crate::overrides::parse_set(s).unwrap())
        .collect();
        let rep = Reporter::new(Arc::new(NullSink), "t");
        let plan = Engine::new()
            .unwrap()
            .plan(r, &rep, &CancelToken::default())
            .unwrap();
        for (name, subtype) in [
            ("w_msn.png", Subtype::Msn),
            ("h_flow.png", Subtype::Flow),
            ("g.png", Subtype::ComplexGrass),
        ] {
            let job = plan.jobs.iter().find(|j| j.input.ends_with(name)).unwrap();
            assert_eq!(job.profile.subtype, subtype);
            let prep = prepare(
                job,
                &CancelToken::default(),
                &FileTracker::default(),
                Keep::Preview { max_size: 16384 },
            )
            .unwrap();
            if subtype == Subtype::ComplexGrass {
                assert_eq!(prep.domain, Domain::Stored);
            }
            let src = stored_source(prep.first_level, prep.dims, prep.chain, prep.domain);
            for (level, input) in src.levels.iter().zip(&prep.inputs) {
                let EncoderInput::Ldr(want) = input else {
                    panic!("{name}: not LDR")
                };
                for (p, w) in level.px.iter().zip(want) {
                    assert_eq!(
                        p[..3].iter().map(|&v| unorm8(v)).collect::<Vec<_>>(),
                        w[..3],
                        "{name}"
                    );
                }
            }
        }
    }
}
