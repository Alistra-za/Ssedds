//! The preview state machine (M3a spec §6.3): what the centre column shows for the expanded texture, when work is
//! requested, and which results are still wanted. All timing goes through [`AppModel::tick`], so tests drive time.
use super::items::{ItemKind, ItemStatus, TextureItem};
use super::AppModel;
use crate::worker::{spawn_dds, spawn_encode, spawn_source};
use encoders_sys::Bc1Model;
use engine::api::CancelToken;
use engine::config::Format;
use engine::dds::DdsPixelFormat;
use engine::decode::{decode_level, Decoded};
use engine::manifest::sha256_json;
use engine::policy::ResolvedJob;
use engine::session::{pack_key, PreviewResult, SourcePreview};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long an edit must rest before a debounced request starts.
pub const DEBOUNCE: Duration = Duration::from_millis(300);

const NOTHING: &str = "Nothing to preview";
const ERRORED: &str = "This texture has an error; fix its settings to preview it";
const SUGGESTED: &str = "Preview of the suggested settings";
const DDS_NO_SOURCE: &str = "DDS input: no source image";
const WAITS_FOR_EXPORT: &str = "Preview encoding waits for the export to finish";

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

pub struct PreviewModel {
    /// The item key being previewed (the expanded one).
    pub item: Option<String>,
    pub source: SourceState,
    pub encoded: EncodedState,
    pub live: bool,
    pub notice: Option<String>,
    /// Set by the first `tick` that sees something scheduled; the debounce counts from there.
    pending_since: Option<Instant>,
    source_pending: bool,
    encode_pending: bool,
    inflight_source: Option<(u64, CancelToken)>,
    inflight_encode: Option<(u64, CancelToken)>,
    next_token: u64,
    /// Everything that shapes the job's output; a change makes a finished result stale.
    profile_sig: Option<String>,
}

impl PreviewModel {
    pub fn new(live: bool) -> PreviewModel {
        PreviewModel {
            item: None,
            source: SourceState::Loading,
            encoded: EncodedState::NotEncoded,
            live,
            notice: None,
            pending_since: None,
            source_pending: false,
            encode_pending: false,
            inflight_source: None,
            inflight_encode: None,
            next_token: 0,
            profile_sig: None,
        }
    }

    /// True while a debounced request waits for its time (the UI keeps frames coming).
    pub fn scheduled(&self) -> bool {
        self.source_pending || self.encode_pending
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
        *self = PreviewModel::new(self.live);
        self.next_token = next_token;
    }

    fn fresh_token(&mut self) -> (u64, CancelToken) {
        self.next_token += 1;
        (self.next_token, CancelToken::default())
    }

    /// The last finished result, kept on screen while a newer one is made.
    fn last_result(&self) -> Option<Arc<PreviewOut>> {
        match &self.encoded {
            EncodedState::Done(r) | EncodedState::Stale(r) => Some(r.clone()),
            EncodedState::Encoding { previous } => previous.clone(),
            _ => None,
        }
    }
}

/// What the expanded item can show.
enum Previewable {
    Job(Box<ResolvedJob>),
    Dds,
    Blocked(&'static str),
}

fn classify(item: &TextureItem) -> Previewable {
    if item.kind == ItemKind::Dds {
        return Previewable::Dds;
    }
    if matches!(item.status, ItemStatus::Error(_)) {
        return Previewable::Blocked(ERRORED);
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
        _ => Previewable::Blocked(NOTHING),
    }
}

fn status_notice(item: &TextureItem) -> Option<String> {
    (item.status == ItemStatus::NeedsDecision).then(|| SUGGESTED.to_string())
}

/// Profile plus, for a packed output, its recipe and sources (with their stamps).
fn signature(job: &ResolvedJob) -> String {
    let p = sha256_json(&job.profile).unwrap_or_default();
    match &job.pack {
        Some(pk) => format!("{p}:{}", pack_key(pk)),
        None => p,
    }
}

/// `(format, srgb)` as the pixel format the decoder takes.
pub fn pixel_format(format: Format, srgb: bool) -> DdsPixelFormat {
    match format {
        Format::Bc1 => DdsPixelFormat::Bc1 { srgb },
        Format::Bc3 => DdsPixelFormat::Bc3 { srgb },
        Format::Bc4 => DdsPixelFormat::Bc4,
        Format::Bc5 => DdsPixelFormat::Bc5,
        Format::Bc6hUf16 => DdsPixelFormat::Bc6hUf16,
        Format::Bc7 => DdsPixelFormat::Bc7 { srgb },
    }
}

fn to_rgba(d: Decoded, pixels: usize) -> Vec<[f32; 4]> {
    let mut out: Vec<[f32; 4]> = match d {
        Decoded::Ldr(v) => v
            .iter()
            .map(|p| {
                [
                    p[0] as f32 / 255.0,
                    p[1] as f32 / 255.0,
                    p[2] as f32 / 255.0,
                    p[3] as f32 / 255.0,
                ]
            })
            .collect(),
        Decoded::Hdr(v) => v.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect(),
    };
    out.resize(pixels, [0.0, 0.0, 0.0, 1.0]);
    out
}

/// CPU decode of every encoded mip of `r` (a mip that fails to decode comes back black, never missing).
pub fn decode_levels(r: &PreviewResult, bc1: Bc1Model) -> Vec<Vec<[f32; 4]>> {
    let fmt = pixel_format(r.format, r.srgb);
    r.levels
        .iter()
        .enumerate()
        .map(|(i, data)| {
            let (w, h) = r.source.dims[r.source.first_level + i];
            let pixels = w as usize * h as usize;
            match decode_level(fmt, w, h, data, Some(bc1)) {
                Ok(d) => to_rgba(d, pixels),
                Err(_) => vec![[0.0, 0.0, 0.0, 1.0]; pixels],
            }
        })
        .collect()
}

/// A DDS input as stored: its mips within `max`, no source side (M3a spec §2.2).
pub fn dds_preview(path: &std::path::Path, max: u32, bc1: Bc1Model) -> Result<PreviewOut, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let f = engine::dds::read_dds(&bytes).map_err(|e| e.to_string())?;
    let Some((format, srgb)) = f.format.as_format() else {
        return Err(format!(
            "{:?} cannot be previewed (only the formats sdc writes)",
            f.format
        ));
    };
    let dims: Vec<(u32, u32)> = f.levels.iter().map(|(w, h, _)| (*w, *h)).collect();
    let first_level = engine::session::first_level_within(&dims, max);
    let levels: Vec<Vec<u8>> = f.levels[first_level..]
        .iter()
        .map(|(_, _, r)| f.data[r.clone()].to_vec())
        .collect();
    let result = PreviewResult {
        source: SourcePreview {
            first_level,
            dims,
            levels: vec![],
        },
        format,
        srgb,
        levels,
        validation: Default::default(),
        warnings: vec![],
        normal_len: vec![],
    };
    Ok(PreviewOut {
        decoded: decode_levels(&result, bc1),
        result,
    })
}

impl AppModel {
    /// The size limit in effect: the setting, at most 8192 unless 16384 is allowed (M3a spec §6.5).
    pub fn preview_limit(&self) -> u32 {
        let m = self.settings.preview_max;
        if self.settings.allow_16k_preview {
            m
        } else {
            m.min(8192)
        }
    }

    fn expanded_item(&self) -> Option<&TextureItem> {
        self.expanded.as_deref().and_then(|k| self.item(k))
    }

    /// `expand` calls this: a fresh preview of `key`.
    pub(super) fn preview_open(&mut self, key: &str) {
        self.preview.clear();
        self.preview.item = Some(key.to_string());
        let Some(item) = self.item(key) else {
            return;
        };
        match classify(item) {
            Previewable::Blocked(why) => {
                self.preview.source = SourceState::Failed(why.to_string());
                self.preview.notice = Some(why.to_string());
            }
            Previewable::Dds => {
                let path = item.input.clone();
                self.preview.source = SourceState::Ready(Arc::new(SourcePreview {
                    first_level: 0,
                    dims: vec![],
                    levels: vec![],
                }));
                self.preview.notice = Some(DDS_NO_SOURCE.to_string());
                self.start_dds(path);
            }
            Previewable::Job(job) => {
                self.preview.notice = status_notice(item);
                self.preview.profile_sig = Some(signature(&job));
                self.start_source();
                if self.preview.live {
                    self.preview.encode_pending = true;
                }
            }
        }
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
            Previewable::Dds => {}
            Previewable::Blocked(why) => {
                if self.preview.profile_sig.is_some() {
                    self.preview_open(&key);
                } else {
                    self.preview.notice = Some(why.to_string());
                }
            }
            Previewable::Job(job) => {
                let sig = signature(&job);
                match self.preview.profile_sig.clone() {
                    None => self.preview_open(&key),
                    Some(old) if old == sig => self.preview.notice = notice,
                    Some(_) => {
                        self.preview.notice = notice;
                        self.preview.profile_sig = Some(sig);
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
                }
            }
        }
    }

    /// Start whatever has waited out the debounce. The UI calls this every frame with `Instant::now()`.
    pub fn tick(&mut self, now: Instant) {
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
        if p.encode_pending && p.live && self.export.running.is_none() {
            p.encode_pending = false;
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
        if self.export.running.is_some() {
            self.preview.notice = Some(WAITS_FOR_EXPORT.to_string());
            return;
        }
        self.preview.encode_pending = false;
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
        spawn_source(self.tx.clone(), session, *job, max, cancel, token);
    }

    fn start_encode(&mut self) {
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
        self.preview.encoded = match result {
            Ok(r) => EncodedState::Done(r),
            Err(e) => EncodedState::Failed(e),
        };
    }

    /// A preview worker panicked: nothing it was making will arrive.
    pub(super) fn preview_panicked(&mut self, message: &str) {
        self.preview.inflight_source = None;
        self.preview.inflight_encode = None;
        if matches!(self.preview.source, SourceState::Loading) {
            self.preview.source = SourceState::Failed(message.to_string());
        }
        if matches!(self.preview.encoded, EncodedState::Encoding { .. }) {
            self.preview.encoded = EncodedState::Failed(message.to_string());
        }
    }
}
