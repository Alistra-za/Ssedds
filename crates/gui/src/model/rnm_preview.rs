//! Item 21 (DECISIONS AG115-AG117): a 128 px thumbnail of a base normal with its RNM layers blended in, made off the
//! UI thread by the engine's own blend (`engine::rnm::blend::apply`) on a downscaled base. One job at a time (the
//! heatmap's pattern, `model::heatmap`): a result whose key is no longer the wanted one is dropped, and the one on
//! show stays until the new one lands.
use crate::model::AppModel;
use crate::worker::{or_send, spawn_guarded, WorkerMsg};
use engine::policy::ResolvedJob;
use std::sync::Arc;

/// The longer side of the thumbnail.
pub const THUMB_MAX: u32 = 128;

/// An RGBA8 picture, `width * height * 4` bytes.
#[derive(Debug)]
pub struct RnmThumb {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Default)]
pub struct RnmPreview {
    next_id: u64,
    /// The job in flight: its id and key.
    running: Option<(u64, String)>,
    /// The item whose section was drawn last; its current settings are what a landing result must match.
    item: Option<String>,
    /// On show: (item key, the key it was made for, the picture; `None` = the blend failed).
    shown: Option<(String, String, Option<Arc<RnmThumb>>)>,
    /// The key of the item last asked about, by plan revision.
    key_cache: Option<(u64, String, Option<String>)>,
    /// Jobs started so far (tests).
    pub started: u32,
}

/// A unit normal as a colour: `(n + 1) / 2` per channel.
pub fn normal_rgba(n: [f32; 3]) -> [u8; 4] {
    let c = |v: f32| ((v.clamp(-1.0, 1.0) + 1.0) * 0.5 * 255.0).round() as u8;
    [c(n[0]), c(n[1]), c(n[2]), 255]
}

fn fit(w: u32, h: u32) -> (u32, u32) {
    let big = w.max(h);
    if big <= THUMB_MAX {
        return (w.max(1), h.max(1));
    }
    let s = THUMB_MAX as f32 / big as f32;
    (
        ((w as f32 * s).round() as u32).max(1),
        ((h as f32 * s).round() as u32).max(1),
    )
}

/// The blended normal of `job` at thumbnail size. Preprocessed like the encode does (decode, flip green), then
/// `blend::apply` with the profile's strengths, order, flips and masks.
pub fn render(job: &ResolvedJob) -> Result<RnmThumb, String> {
    let rnm = job.active_rnm().ok_or("RNM is off")?;
    let (img, _) = engine::load::load_image(&job.input).map_err(|e| e.to_string())?;
    let (w, h) = fit(img.width, img.height);
    let img = engine::rnm::blend::resample(img, (w, h));
    let mut work = engine::ops::preprocess(img, &job.profile);
    engine::rnm::blend::apply(&mut work.img, rnm, &job.profile)?;
    let rgba = work
        .img
        .px
        .iter()
        .flat_map(|p| normal_rgba([p[0], p[1], p[2]]))
        .collect();
    Ok(RnmThumb {
        width: w,
        height: h,
        rgba,
    })
}

/// Everything that changes the picture: the base file and stamp, the layers (files, stamps), and the profile's
/// RNM settings (strength, flips, masks, order) and green flip.
fn key_of(item_key: &str, job: &ResolvedJob) -> Option<String> {
    job.active_rnm()?;
    let body = serde_json::to_string(&(
        &job.input,
        &job.source,
        &job.rnm,
        &job.profile.rnm,
        &job.profile.channels,
    ))
    .ok()?;
    Some(format!("{item_key}|{body}"))
}

impl AppModel {
    /// Once a frame while the RNM section of `item_key` is drawn: starts the job for the current settings when it
    /// differs from the one on show and nothing is in flight. Returns at once; the picture lands through `poll`.
    pub fn rnm_preview_frame(&mut self, item_key: &str) {
        let job = self.item(item_key).and_then(|i| i.job.clone());
        // The key serialises the job; it only changes with a plan, so it is rebuilt per plan revision.
        let rev = self.items_rev();
        let cached = self
            .rnm_preview
            .key_cache
            .take()
            .filter(|(r, it, _)| *r == rev && it == item_key);
        let want = match cached {
            Some((_, _, k)) => k,
            None => job.as_ref().and_then(|j| key_of(item_key, j)),
        };
        self.rnm_preview.key_cache = Some((rev, item_key.to_string(), want.clone()));
        self.rnm_preview.item = Some(item_key.to_string());
        let (Some(job), Some(want)) = (job, want) else {
            return;
        };
        let p = &mut self.rnm_preview;
        if p.running.is_some() || p.shown.as_ref().is_some_and(|(_, k, _)| *k == want) {
            return;
        }
        p.next_id += 1;
        p.started += 1;
        let id = p.next_id;
        p.running = Some((id, want));
        spawn_guarded(self.tx.clone(), "rnm-preview", move |tx| {
            // A panic is this job's "no picture" (trace-debug #26), so its key is not retried every frame.
            let image = or_send(
                tx,
                || render(&job).ok().map(Arc::new),
                |_| WorkerMsg::RnmPreview { id, image: None },
            );
            let _ = tx.send(WorkerMsg::RnmPreview { id, image });
        });
    }

    pub(super) fn rnm_preview_landed(&mut self, id: u64, image: Option<Arc<RnmThumb>>) {
        let now = self.rnm_preview.item.clone().and_then(|it| {
            let job = self.item(&it)?.job.as_ref()?;
            key_of(&it, job)
        });
        let p = &mut self.rnm_preview;
        let Some((_, key)) = p.running.take_if(|(rid, _)| *rid == id) else {
            return;
        };
        // A result for settings that changed meanwhile is dropped; the next frame starts the current ones.
        if now.as_deref() == Some(key.as_str()) {
            let item = key.split('|').next().unwrap_or_default().to_string();
            p.shown = Some((item, key, image));
        }
    }

    /// A job is in flight: the app keeps frames coming until it lands.
    pub fn rnm_preview_pending(&self) -> bool {
        self.rnm_preview.running.is_some()
    }

    /// The picture on show for `item_key` (made for its settings of a moment ago while a newer one is in flight).
    pub fn rnm_preview_image(&self, item_key: &str) -> Option<Arc<RnmThumb>> {
        let (item, _, image) = self.rnm_preview.shown.as_ref()?;
        (item == item_key).then(|| image.clone()).flatten()
    }

    /// The blend failed for the settings on show (the section says so).
    pub fn rnm_preview_failed(&self, item_key: &str) -> bool {
        self.rnm_preview
            .shown
            .as_ref()
            .is_some_and(|(item, _, image)| item == item_key && image.is_none())
    }
}
