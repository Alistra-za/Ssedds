//! The preview state machine (M3a spec §6.3): what the centre column shows for the expanded texture, when work is
//! requested, and which results are still wanted. All timing goes through [`AppModel::tick`], so tests drive time.
use super::cube_cross;
use super::items::{ItemKind, ItemStatus, TextureItem};
use super::AppModel;
use crate::worker::{spawn_dds, spawn_encode, spawn_source};
use encoders_sys::Bc1Model;
use engine::api::CancelToken;
use engine::config::Format;
use engine::dds::DdsPixelFormat;
use engine::decode::decode_level_rgba;
use engine::manifest::sha256_json;
use engine::policy::{ResolvedJob, SourceStamp};
use engine::session::{first_level_within, pack_key, PreviewResult, SourcePreview};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long an edit must rest before a debounced request starts.
pub const DEBOUNCE: Duration = Duration::from_millis(300);

const NOTHING: &str = "Nothing to preview";
const ERRORED: &str = "This texture has an error; fix its settings to preview it";
const SUGGESTED: &str = "Preview of the suggested settings";
const DDS_NO_SOURCE: &str = "DDS input: no source image";
const WAITS_FOR_EXPORT: &str = "Preview encoding waits for the export to finish";
/// What the viewports say when eframe found no usable GPU adapter (M3a spec §7).
pub const NO_GPU: &str = "No usable GPU: the preview is off; everything else works";

#[derive(Debug)]
pub struct PreviewOut {
    pub result: PreviewResult,
    /// CPU decode of every encoded mip (RGBA f32), for the heatmap and the no-BC-support fallback.
    pub decoded: Vec<Vec<[f32; 4]>>,
}

#[derive(Debug, Clone)]
pub enum SourceState {
    Loading,
    Ready(Arc<SourcePreview>),
    Failed(String),
}

#[derive(Debug, Clone)]
pub enum EncodedState {
    NotEncoded,
    Encoding {
        previous: Option<Arc<PreviewOut>>,
    },
    Done(Arc<PreviewOut>),
    /// The settings changed after this result was made.
    Stale(Arc<PreviewOut>),
    Failed(String),
}

impl EncodedState {
    /// The result on show: the finished one, the stale one, or the previous one while a new one is made.
    pub fn shown(&self) -> Option<&Arc<PreviewOut>> {
        match self {
            EncodedState::Done(r) | EncodedState::Stale(r) => Some(r),
            EncodedState::Encoding { previous } => previous.as_ref(),
            _ => None,
        }
    }
}

pub struct PreviewModel {
    /// The item key being previewed (the expanded one).
    pub item: Option<String>,
    pub source: SourceState,
    pub encoded: EncodedState,
    pub live: bool,
    /// What the preview says about itself, one line per reason (the item's status, the GPU limit, ...).
    pub notice: Option<String>,
    base_notice: Option<String>,
    gpu_note: Option<String>,
    /// Set by the first `tick` that sees something scheduled; the debounce counts from there.
    pending_since: Option<Instant>,
    source_pending: bool,
    encode_pending: bool,
    /// An Encode now pressed during an export: it runs once the export is done, Live encode or not.
    encode_after_export: bool,
    inflight_source: Option<(u64, CancelToken)>,
    inflight_encode: Option<(u64, CancelToken)>,
    next_token: u64,
    /// Everything that shapes the job's output; a change makes a finished result stale.
    profile_sig: Option<String>,
    /// The stamp of the DDS input the preview was made from; a plan that finds another one reads the file again.
    dds_stamp: Option<SourceStamp>,
    /// The error heatmap job and the heatmap on show (dropped with the texture).
    pub(crate) heat: crate::model::heatmap::HeatJob,
    /// The pinned encode (item 10): dropped with the preview (close, texture change, folder open), kept across
    /// settings changes.
    pub pin: Option<crate::model::pin::Pin>,
    /// "Compare: Source | Pinned": the layouts show the pin against the current encode.
    pub compare_pinned: bool,
    /// Pin ids go on across `clear`, so a heatmap result for a dropped pin can never match a new one.
    pub next_pin: u64,
}

impl PreviewModel {
    pub fn new(live: bool) -> PreviewModel {
        PreviewModel {
            item: None,
            source: SourceState::Loading,
            encoded: EncodedState::NotEncoded,
            live,
            notice: None,
            base_notice: None,
            gpu_note: None,
            pending_since: None,
            source_pending: false,
            encode_pending: false,
            encode_after_export: false,
            inflight_source: None,
            inflight_encode: None,
            next_token: 0,
            profile_sig: None,
            dds_stamp: None,
            heat: Default::default(),
            pin: None,
            compare_pinned: false,
            next_pin: 0,
        }
    }

    fn refresh_notice(&mut self) {
        let lines: Vec<&str> = [&self.base_notice, &self.gpu_note]
            .into_iter()
            .flatten()
            .map(String::as_str)
            .collect();
        self.notice = (!lines.is_empty()).then(|| {
            lines.join(
                "
",
            )
        });
    }

    /// True while a debounced request waits for its time (the UI keeps frames coming).
    pub fn scheduled(&self) -> bool {
        self.source_pending || self.encode_pending || self.encode_after_export
    }

    /// True while a worker is reading or encoding for the preview.
    pub fn working(&self) -> bool {
        self.inflight_source.is_some() || self.inflight_encode.is_some()
    }

    fn cancel_source(&mut self) {
        if let Some((_, c)) = self.inflight_source.take() {
            c.cancel();
        }
    }

    fn cancel_encode(&mut self) {
        if let Some((_, c)) = self.inflight_encode.take() {
            c.cancel();
        }
    }

    /// Back to nothing previewed. Cancels both jobs; their results, if they still arrive, are dropped by token.
    fn clear(&mut self) {
        self.cancel_source();
        self.cancel_encode();
        let next_token = self.next_token;
        let heat = self.heat.cleared();
        let next_pin = self.next_pin;
        *self = PreviewModel::new(self.live);
        self.next_token = next_token;
        self.heat = heat;
        self.next_pin = next_pin;
    }

    fn fresh_token(&mut self) -> (u64, CancelToken) {
        self.next_token += 1;
        (self.next_token, CancelToken::default())
    }

    /// The last finished result, kept on screen while a newer one is made.
    fn last_result(&self) -> Option<Arc<PreviewOut>> {
        self.encoded.shown().cloned()
    }
}

/// What the expanded item can show.
enum Previewable {
    Job(Box<ResolvedJob>),
    Dds,
    Blocked(String),
}

fn classify(item: &TextureItem) -> Previewable {
    if item.kind == ItemKind::Dds {
        return Previewable::Dds;
    }
    if matches!(item.status, ItemStatus::Error(_)) {
        // The engine's reason comes with it (a cube with a missing or odd-sized face says which, Note DT16).
        let ItemStatus::Error(codes) = &item.status else {
            unreachable!()
        };
        let why = item
            .warnings
            .iter()
            .filter(|w| codes.contains(&w.code))
            // Note FB8: the reason in words, no code.
            .map(|w| super::code_text::without_codes(&w.message));
        return Previewable::Blocked(
            std::iter::once(ERRORED.to_string())
                .chain(why)
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    match &item.job {
        Some(job)
            if !matches!(
                item.status,
                ItemStatus::Skipped(_) | ItemStatus::Failed(_) | ItemStatus::PackedInto(_)
            ) =>
        {
            Previewable::Job(Box::new(job.clone()))
        }
        _ => Previewable::Blocked(NOTHING.to_string()),
    }
}

fn status_notice(item: &TextureItem) -> Option<String> {
    (item.status == ItemStatus::NeedsDecision).then(|| SUGGESTED.to_string())
}

/// Profile plus, for a packed output, its recipe and sources (with their stamps), plus the job's bake, derive,
/// cube and fill inputs ([`extra_inputs_key`]).
fn signature(job: &ResolvedJob) -> String {
    let p = sha256_json(&job.profile).unwrap_or_default();
    let head = match &job.pack {
        Some(pk) => format!("{p}:{}", pack_key(pk).unwrap_or_default()),
        None => format!("{p}:{:?}:{:?}", job.source, job.active_rnm()),
    };
    format!("{head}:{}", extra_inputs_key(job))
}

/// What a job reads besides its profile, `source` and RNM layers: the bake's `_n`, the derive's and cube's files
/// and the fill's helper, each with its settings and stamp (Debug: plain structs, no maps, so equal jobs match).
/// Shared with the set preview's signature (Note FA1).
pub fn extra_inputs_key(job: &ResolvedJob) -> String {
    format!(
        "{:?}:{:?}:{:?}:{:?}",
        job.bake, job.derive, job.cube, job.fill
    )
}

/// CPU decode of `levels` (mips `first_level..` of the chain `dims`) as `fmt`; a mip that fails comes back black.
fn decode_chain(
    fmt: DdsPixelFormat,
    dims: &[(u32, u32)],
    first_level: usize,
    levels: &[Vec<u8>],
    bc1: Bc1Model,
) -> Vec<Vec<[f32; 4]>> {
    levels
        .iter()
        .enumerate()
        .map(|(i, data)| {
            let (w, h) = dims[first_level + i];
            let pixels = w as usize * h as usize;
            match decode_level_rgba(fmt, w, h, data, Some(bc1)) {
                Ok(mut v) => {
                    v.resize(pixels, [0.0, 0.0, 0.0, 1.0]);
                    v
                }
                Err(_) => vec![[0.0, 0.0, 0.0, 1.0]; pixels],
            }
        })
        .collect()
}

/// CPU decode of every encoded mip of `r` (a mip that fails to decode comes back black, never missing). A cube
/// (`faces == 6`, face-major levels) decodes to one horizontal cross per mip (Note DT16).
pub fn decode_levels(r: &PreviewResult, bc1: Bc1Model) -> Vec<Vec<[f32; 4]>> {
    let s = &r.source;
    let fmt = r.format.pixel_format(r.srgb);
    if r.faces != 6 {
        return decode_chain(fmt, &s.dims, s.first_level, &r.levels, bc1);
    }
    let per_face = r.levels.len() / 6;
    let faces: Vec<_> = r
        .levels
        .chunks(per_face.max(1))
        .map(|f| decode_chain(fmt, &s.dims, s.first_level, f, bc1))
        .collect();
    (0..per_face)
        .map(|i| {
            let size = s.dims[s.first_level + i].0;
            let refs = std::array::from_fn(|f| faces[f][i].as_slice());
            cube_cross::cross_image(refs, size).2
        })
        .collect()
}

/// A DDS input as stored: its mips within `max`, no source side (M3a spec §2.2).
pub fn dds_preview(path: &std::path::Path, max: u32, bc1: Bc1Model) -> Result<PreviewOut, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let f = engine::dds::read_dds(&bytes).map_err(|e| e.to_string())?;
    // DECISIONS S5: every uncompressed layout previews (N11 ends); BC2 and signed BC6H still cannot.
    let (format, srgb) = match f.format.as_format() {
        Some(v) => v,
        None if f.format.block_bytes().is_none() => {
            let float = matches!(f.format, DdsPixelFormat::Rgba32f);
            (
                if float {
                    Format::Rgba16f
                } else {
                    Format::Rgba8
                },
                false,
            )
        }
        None => {
            return Err(format!(
                "{:?} cannot be previewed (BC2 and signed BC6H are not supported)",
                f.format
            ));
        }
    };
    let dims: Vec<(u32, u32)> = f.levels.iter().map(|(w, h, _)| (*w, *h)).collect();
    let first_level = engine::session::first_level_within(&dims, max);
    // `first_level_within` falls back to the last mip; a short chain can end above the limit (trace-debug #16).
    if let Some(&(w, h)) = dims.get(first_level).filter(|(w, h)| *w.max(h) > max) {
        return Err(format!(
            "{w}x{h} (the smallest mip) is above the preview limit of {max}"
        ));
    }
    let levels: Vec<Vec<u8>> = f.levels[first_level..]
        .iter()
        .map(|(_, _, r)| f.data[r.clone()].to_vec())
        .collect();
    // Decoded as the file's own layout: for a format sdc never writes, `format` above is only the display kind.
    let decoded = decode_chain(f.format, &dims, first_level, &levels, bc1);
    let result = PreviewResult {
        source: SourcePreview {
            first_level,
            dims,
            levels: vec![],
        },
        format,
        srgb,
        faces: 1,
        levels,
        validation: Default::default(),
        warnings: vec![],
        normal_len: vec![],
    };
    Ok(PreviewOut { decoded, result })
}

/// A skipped DDS input's tile image (Note FI1): its largest mip of at most `THUMB_MAX`, else its smallest mip
/// box-downsampled, as stored (RGBA8).
/// shortcut: a mipless 4K DDS decodes its full level once on the worker; read a cached thumb from the DZ disk cache
/// if open time shows it.
/// shortcut: an HDR (BC6H, float) DDS is clamped, not tone-mapped; upgrade if HDR tiles look blown out.
pub fn dds_thumbnail(
    path: &std::path::Path,
    bc1: Bc1Model,
) -> Result<engine::session::Thumbnail, String> {
    use engine::session::THUMB_MAX;
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let f = engine::dds::read_dds(&bytes).map_err(|e| e.to_string())?;
    // The same refusals as `dds_preview`: BC2 and signed BC6H cannot be decoded.
    if f.format.as_format().is_none() && f.format.block_bytes().is_some() {
        return Err(format!(
            "{:?} cannot be previewed (BC2 and signed BC6H are not supported)",
            f.format
        ));
    }
    let dims: Vec<(u32, u32)> = f.levels.iter().map(|(w, h, _)| (*w, *h)).collect();
    let first = first_level_within(&dims, THUMB_MAX);
    let (w, h, range) = f.levels.get(first).ok_or("no mip levels")?.clone();
    let mut px =
        decode_level_rgba(f.format, w, h, &f.data[range], Some(bc1)).map_err(|e| e.to_string())?;
    px.resize(w as usize * h as usize, [0.0, 0.0, 0.0, 1.0]);
    let img = engine::image::Rgba32F {
        width: w,
        height: h,
        px,
    };
    // `make_thumbnail` box-downsamples a mip still over the limit and clamps as stored (`is_float: false`).
    let info = engine::image::SourceInfo {
        bit_depth: 8,
        channels: 4,
        has_alpha: true,
        is_float: false,
    };
    Ok(engine::session::make_thumbnail(&img, &info))
}

impl AppModel {
    /// The size limit the settings give: the setting, at most 8192 unless 16384 is allowed (M3a spec §6.5).
    fn settings_limit(&self) -> u32 {
        let m = self.settings.preview_max;
        if self.settings.allow_16k_preview {
            m
        } else {
            m.min(8192)
        }
    }

    /// The size limit in effect: the settings' limit, lowered to what the GPU can hold (M3a spec §6.5).
    pub fn preview_limit(&self) -> u32 {
        self.settings_limit().min(self.gpu_max_dim)
    }

    fn set_base_notice(&mut self, notice: Option<String>) {
        self.preview.base_notice = notice;
        self.preview.refresh_notice();
    }

    /// Say so when the GPU's limit, not the setting, left mips out of a result (`dims` is the whole chain).
    fn note_gpu_limit(&mut self, dims: &[(u32, u32)], first_level: usize) {
        let wanted = self.settings_limit();
        let lowered = self.gpu_max_dim < wanted && first_level > first_level_within(dims, wanted);
        self.preview.gpu_note = lowered.then(|| {
            format!(
                "This GPU shows at most {} px: previewing from the first mip that fits",
                self.gpu_max_dim
            )
        });
        self.preview.refresh_notice();
    }

    fn expanded_item(&self) -> Option<&TextureItem> {
        self.expanded.as_deref().and_then(|k| self.item(k))
    }

    /// `expand` calls this: a fresh preview of `key`.
    pub(super) fn preview_open(&mut self, key: &str) {
        self.preview.clear();
        self.preview.item = Some(key.to_string());
        self.view.exposure = 0.0;
        self.view.source_face = 0;
        let Some(item) = self.item(key) else {
            return;
        };
        if !self.gpu_available && !matches!(classify(item), Previewable::Blocked(_)) {
            self.preview.source = SourceState::Failed(NO_GPU.to_string());
            self.set_base_notice(Some(NO_GPU.to_string()));
            return;
        }
        match classify(item) {
            Previewable::Blocked(why) => {
                self.preview.source = SourceState::Failed(why.to_string());
                self.set_base_notice(Some(why.to_string()));
            }
            Previewable::Dds => {
                let path = item.input.clone();
                self.preview.dds_stamp = item.stamp;
                self.preview.source = SourceState::Ready(Arc::new(SourcePreview {
                    first_level: 0,
                    dims: vec![],
                    levels: vec![],
                }));
                self.set_base_notice(Some(DDS_NO_SOURCE.to_string()));
                self.start_dds(path);
            }
            Previewable::Job(job) => {
                let notice = status_notice(item);
                self.set_base_notice(notice);
                self.preview.profile_sig = Some(signature(&job));
                self.start_source();
                if self.preview.live {
                    self.preview.encode_pending = true;
                }
            }
        }
    }

    /// The source pane of a six-file cube shows face `face` (+X..-Z): read the source again for it.
    pub fn set_source_face(&mut self, face: usize) {
        let face = face.min(5);
        if self.view.source_face == face {
            return;
        }
        self.view.source_face = face;
        self.preview.cancel_source();
        self.preview.source_pending = true;
        self.preview.pending_since = None;
    }

    /// `collapse` calls this.
    pub(super) fn preview_close(&mut self) {
        self.preview.clear();
    }

    /// After a plan replaced the items: mark a changed preview stale, or re-open one whose kind changed.
    /// (A vanished item is handled by `apply_plan`.)
    pub(super) fn preview_after_plan(&mut self) {
        let Some(key) = self.expanded.clone() else {
            return;
        };
        let Some(item) = self.item(&key) else {
            return;
        };
        let notice = status_notice(item);
        match classify(item) {
            Previewable::Dds => {
                if item.stamp != self.preview.dds_stamp {
                    self.preview_open(&key);
                }
            }
            Previewable::Blocked(why) => {
                if self.preview.profile_sig.is_some() {
                    self.preview_open(&key);
                } else {
                    self.set_base_notice(Some(why.to_string()));
                }
            }
            Previewable::Job(job) => {
                let sig = signature(&job);
                match self.preview.profile_sig.clone() {
                    None => self.preview_open(&key),
                    Some(old) if old == sig => self.set_base_notice(notice),
                    Some(_) => {
                        self.set_base_notice(notice);
                        self.preview.profile_sig = Some(sig);
                        self.restart_job_preview();
                    }
                }
            }
        }
    }

    /// The result on show is out of date: keep it as stale, read the source again, and encode again when live.
    fn restart_job_preview(&mut self) {
        let last = self.preview.last_result();
        self.preview.cancel_encode();
        self.preview.cancel_source();
        self.preview.encoded = match last {
            Some(r) => EncodedState::Stale(r),
            None => EncodedState::NotEncoded,
        };
        self.preview.source_pending = true;
        self.preview.pending_since = None;
        if self.preview.live {
            self.preview.encode_pending = true;
        }
    }

    /// The size limit changed from `before`: what the expanded texture shows was made under the old one.
    fn preview_limit_changed(&mut self, before: u32) {
        if self.preview_limit() == before {
            return;
        }
        let Some(key) = self.expanded.clone() else {
            return;
        };
        match self.item(&key).map(classify) {
            Some(Previewable::Job(_)) => self.restart_job_preview(),
            Some(Previewable::Dds) => self.preview_open(&key),
            _ => {}
        }
    }

    /// The preview size limit (M3a spec §6.5); 16384 only with `allow_16k_preview`. Saved.
    pub fn set_preview_max(&mut self, size: u32) {
        if (size == 16384 && !self.settings.allow_16k_preview)
            || !crate::settings::PREVIEW_SIZES.contains(&size)
        {
            return;
        }
        let before = self.preview_limit();
        self.settings.preview_max = size;
        self.save_settings();
        self.preview_limit_changed(before);
    }

    /// Allow (or stop allowing) a 16384 px limit. Turning it off lowers a 16384 limit to 8192.
    pub fn set_allow_16k_preview(&mut self, on: bool) {
        let before = self.preview_limit();
        self.settings.allow_16k_preview = on;
        if !on && self.settings.preview_max > 8192 {
            self.settings.preview_max = 8192;
        }
        self.save_settings();
        self.preview_limit_changed(before);
    }

    /// Start whatever has waited out the debounce. The UI calls this every frame with `Instant::now()`.
    pub fn tick(&mut self, now: Instant) {
        self.expire_undo_note(now);
        let p = &mut self.preview;
        if !p.scheduled() {
            p.pending_since = None;
            return;
        }
        let since = *p.pending_since.get_or_insert(now);
        if now.saturating_duration_since(since) < DEBOUNCE {
            return;
        }
        if p.source_pending {
            p.source_pending = false;
            self.start_source();
        }
        let p = &mut self.preview;
        if (p.encode_pending && p.live || p.encode_after_export) && self.export.running.is_none() {
            p.encode_pending = false;
            p.encode_after_export = false;
            if self.preview.base_notice.as_deref() == Some(WAITS_FOR_EXPORT) {
                self.set_base_notice(None);
            }
            self.start_encode();
        }
        if !self.preview.scheduled() {
            self.preview.pending_since = None;
        }
    }

    /// Encode the expanded texture now, whether Live encode is on or not.
    pub fn encode_now(&mut self) {
        let Some(item) = self.expanded_item() else {
            return;
        };
        if !matches!(classify(item), Previewable::Job(_)) {
            return;
        }
        if !self.gpu_available {
            self.set_base_notice(Some(NO_GPU.to_string()));
            return;
        }
        if self.export.running.is_some() {
            // The notice says it waits, so it does: `tick` encodes once the export is done (trace #32).
            self.preview.encode_after_export = true;
            self.set_base_notice(Some(WAITS_FOR_EXPORT.to_string()));
            return;
        }
        self.preview.encode_pending = false;
        self.preview.encode_after_export = false;
        self.start_encode();
    }

    pub fn set_live(&mut self, on: bool) {
        self.settings.live_encode = on;
        self.save_settings();
        self.preview.live = on;
        if !on {
            self.preview.encode_pending = false;
            return;
        }
        let waiting = matches!(
            self.preview.encoded,
            EncodedState::Stale(_) | EncodedState::NotEncoded
        );
        let job = self
            .expanded_item()
            .is_some_and(|i| matches!(classify(i), Previewable::Job(_)));
        if waiting && job && self.preview.inflight_encode.is_none() {
            self.preview.encode_pending = true;
        }
    }

    fn start_source(&mut self) {
        if !self.gpu_available {
            return;
        }
        let Some(Previewable::Job(job)) = self.expanded_item().map(classify) else {
            return;
        };
        let Some(session) = self.session.clone() else {
            return;
        };
        let max = self.preview_limit();
        self.preview.cancel_source();
        let (token, cancel) = self.preview.fresh_token();
        self.preview.inflight_source = Some((token, cancel.clone()));
        let max = cube_cross::source_cap(&job, max);
        spawn_source(
            self.tx.clone(),
            session,
            *job,
            self.view.source_face,
            max,
            cancel,
            token,
        );
    }

    fn start_encode(&mut self) {
        if !self.gpu_available {
            return;
        }
        let Some(Previewable::Job(job)) = self.expanded_item().map(classify) else {
            return;
        };
        let Some(session) = self.session.clone() else {
            return;
        };
        let max = self.preview_limit();
        let previous = self.preview.last_result();
        self.preview.cancel_encode();
        let (token, cancel) = self.preview.fresh_token();
        self.preview.inflight_encode = Some((token, cancel.clone()));
        self.preview.encoded = EncodedState::Encoding { previous };
        spawn_encode(self.tx.clone(), session, *job, max, cancel, token);
    }

    fn start_dds(&mut self, path: std::path::PathBuf) {
        if !self.gpu_available {
            return;
        }
        let Some(session) = self.session.clone() else {
            return;
        };
        let max = self.preview_limit();
        let (token, cancel) = self.preview.fresh_token();
        self.preview.inflight_encode = Some((token, cancel));
        self.preview.encoded = EncodedState::Encoding { previous: None };
        spawn_dds(self.tx.clone(), path, max, session.bc1_model(), token);
    }

    pub(super) fn on_source(&mut self, token: u64, result: Result<Arc<SourcePreview>, String>) {
        if self.preview.inflight_source.as_ref().map(|t| t.0) != Some(token) {
            return;
        }
        self.preview.inflight_source = None;
        if let Ok(s) = &result {
            self.note_gpu_limit(&s.dims, s.first_level);
        }
        self.preview.source = match result {
            Ok(s) => SourceState::Ready(s),
            Err(e) => SourceState::Failed(e),
        };
    }

    pub(super) fn on_encoded(&mut self, token: u64, result: Result<Arc<PreviewOut>, String>) {
        if self.preview.inflight_encode.as_ref().map(|t| t.0) != Some(token) {
            return;
        }
        self.preview.inflight_encode = None;
        if let Ok(r) = &result {
            self.note_gpu_limit(&r.result.source.dims, r.result.source.first_level);
        }
        self.preview.encoded = match result {
            Ok(r) => EncodedState::Done(r),
            Err(e) => EncodedState::Failed(e),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::bare_model;
    use crate::model::{preview, Dialog};
    use crate::worker::WorkerMsg;

    /// Trace-debug #26: a panicking encode fails only the encode; the source read in flight still lands.
    #[test]
    fn an_encode_panic_keeps_the_source_read_in_flight() {
        let mut m = bare_model();
        m.preview.inflight_source = Some((7, CancelToken::default()));
        m.preview.inflight_encode = Some((8, CancelToken::default()));
        m.preview.encoded = preview::EncodedState::Encoding { previous: None };
        let (tx, rx) = std::sync::mpsc::channel();
        crate::worker::spawn_guarded(tx, "preview", |tx| {
            let _ = crate::worker::or_send(
                tx,
                || -> Result<Arc<preview::PreviewOut>, String> { panic!("boom") },
                |e| WorkerMsg::Encoded {
                    token: 8,
                    result: Err(e),
                },
            );
        });
        for msg in rx.iter() {
            m.handle(msg);
        }
        assert!(
            m.preview.inflight_source.is_some(),
            "the source read is still awaited"
        );
        assert!(m.preview.inflight_encode.is_none());
        assert!(
            matches!(&m.preview.encoded, preview::EncodedState::Failed(e) if e.contains("boom"))
        );
        assert!(matches!(m.dialog, Some(Dialog::Error(_))));
    }

    fn stamped(n: u64) -> Option<SourceStamp> {
        Some(SourceStamp {
            size: n,
            mtime_ns: n,
        })
    }

    /// A plain job carrying every extra input the change signature must cover: a fill, a bake, a derive, a cube.
    fn job_with_inputs() -> ResolvedJob {
        use engine::config::{CubeLayout, GraySource, RawMap, Subtype, Target};
        use engine::cube::{CubeJob, CubeSource};
        use engine::derive::{DeriveJob, DeriveKind, DeriveSource};
        use engine::helper::FillJob;
        use engine::pack::{ChannelSource, PackSource};
        let profile = engine::presets::PresetRegistry::builtin()
            .unwrap()
            .base_profile(Target::Cs, Subtype::Diffuse);
        let d = |value| ChannelSource::Default { value };
        ResolvedJob {
            input: "a.png".into(),
            output: "a.dds".into(),
            game_path: "a".into(),
            set_key: "x/a".into(),
            profile: profile.clone(),
            provenance: Default::default(),
            suggestions: vec![],
            warnings: vec![],
            status: engine::policy::JobStatus::Ready,
            suffix_alias: None,
            source: stamped(1),
            pack: None,
            bake: Some(engine::bake::BakeJob {
                normal: "a_n.png".into(),
                normal_profile: profile,
                shift: 1,
                stamp: stamped(2),
                rnm: None,
            }),
            shader: None,
            rnm: None,
            derive: Some(DeriveJob {
                kind: DeriveKind::LegacyN,
                landscape: false,
                albedo: None,
                normal: Some("a_n.png".into()),
                rough: d(0.5),
                metal: d(0.0),
                ao: d(1.0),
                height: None,
                curves: None,
                cavity: None,
                gray: GraySource::R,
                params: Default::default(),
                sources: vec![DeriveSource {
                    path: "a_n.png".into(),
                    role: "normal".into(),
                    stamp: stamped(3),
                }],
            }),
            cube: Some(CubeJob {
                layout: CubeLayout::Faces,
                sources: (0..6)
                    .map(|f| CubeSource {
                        path: format!("f{f}.png").into(),
                        stamp: stamped(10 + f as u64),
                        face: Some(f),
                    })
                    .collect(),
            }),
            fill: Some(FillJob {
                source: PackSource {
                    path: "h.png".into(),
                    role: RawMap::Roughness,
                    stamp: stamped(4),
                    read: None,
                },
                read: None,
                invert: false,
                assigned: true,
            }),
        }
    }

    /// Trace-debug G4: the change signature covers the fill, bake, derive and cube inputs, not only the profile and
    /// `source` (toggling Invert on a helper must re-encode); two identical jobs, or a clone, stay equal.
    #[test]
    fn signature_changes_with_fill_bake_derive_cube() {
        let job = job_with_inputs();
        let a = signature(&job);
        assert_eq!(a, signature(&job_with_inputs()), "an identical job");
        assert_eq!(a, signature(&job.clone()), "a re-plan with nothing changed");
        type Edit = fn(&mut ResolvedJob);
        let edits: [(&str, Edit); 6] = [
            ("fill invert", |j| j.fill.as_mut().unwrap().invert = true),
            ("fill read", |j| {
                j.fill.as_mut().unwrap().read = Some(engine::helper::HelperRead::G)
            }),
            ("fill source stamp", |j| {
                j.fill.as_mut().unwrap().source.stamp = stamped(99)
            }),
            ("bake stamp", |j| {
                j.bake.as_mut().unwrap().stamp = stamped(99)
            }),
            ("cube face 1 stamp", |j| {
                j.cube.as_mut().unwrap().sources[1].stamp = stamped(99)
            }),
            ("derive source stamp", |j| {
                j.derive.as_mut().unwrap().sources[0].stamp = stamped(99)
            }),
        ];
        for (what, edit) in edits {
            let mut j = job.clone();
            edit(&mut j);
            assert_ne!(a, signature(&j), "{what}");
        }
    }
}
