//! The error heatmap off the UI thread (item 15, DECISIONS AG58): one job at a time on the worker pattern, keyed by
//! (result, mip, channel, full scale). The result `Arc` stands for the texture and its encode revision. The heatmap
//! on show stays until the one for the current key lands; a result for another key is dropped.
use crate::model::preview::PreviewOut;
use crate::model::view::{heatmap_of, Channel};
use crate::model::AppModel;
use crate::worker::{spawn_guarded, WorkerMsg};
use std::sync::Arc;

/// A heatmap ready to upload: `width * height * 4` RGBA bytes.
#[derive(Debug)]
pub struct HeatImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone)]
pub(crate) struct HeatKey {
    out: Arc<PreviewOut>,
    mip: usize,
    channel: Channel,
    full_scale: f32,
}

impl PartialEq for HeatKey {
    fn eq(&self, o: &HeatKey) -> bool {
        Arc::ptr_eq(&self.out, &o.out)
            && self.mip == o.mip
            && self.channel == o.channel
            && self.full_scale == o.full_scale
    }
}

#[derive(Default)]
pub(crate) struct HeatJob {
    next_id: u64,
    /// The job in flight: its id and key.
    running: Option<(u64, HeatKey)>,
    /// The heatmap on show and its key (`None` image: that key has no heatmap, e.g. a DDS input).
    shown: Option<(HeatKey, Option<Arc<HeatImage>>)>,
}

impl HeatJob {
    /// Nothing in flight or on show; the ids go on, so a late result of a dropped job can never match a new one.
    pub(crate) fn cleared(&self) -> HeatJob {
        HeatJob {
            next_id: self.next_id,
            ..HeatJob::default()
        }
    }
}

impl AppModel {
    fn heat_wanted(&self) -> Option<HeatKey> {
        if !self.view.heatmap {
            return None;
        }
        Some(HeatKey {
            out: self.preview.encoded.shown()?.clone(),
            mip: self.view.mip,
            channel: self.view.channel,
            full_scale: self.view.full_scale,
        })
    }

    /// Once a frame: starts the heatmap for the current view when it differs from the one on show and nothing is in
    /// flight. Returns at once; the image lands through `poll`.
    pub fn heatmap_frame(&mut self) {
        let Some(want) = self.heat_wanted() else {
            return;
        };
        let h = &mut self.preview.heat;
        if h.running.is_some() || h.shown.as_ref().is_some_and(|(k, _)| *k == want) {
            return;
        }
        h.next_id += 1;
        let id = h.next_id;
        h.running = Some((id, want.clone()));
        spawn_guarded(self.tx.clone(), "heatmap", move |tx| {
            let image = heatmap_of(&want.out, want.mip, want.channel, want.full_scale).map(
                |(width, height, rgba)| {
                    Arc::new(HeatImage {
                        width,
                        height,
                        rgba,
                    })
                },
            );
            let _ = tx.send(WorkerMsg::Heatmap { id, image });
        });
    }

    pub(super) fn heatmap_landed(&mut self, id: u64, image: Option<Arc<HeatImage>>) {
        let want = self.heat_wanted();
        let h = &mut self.preview.heat;
        let Some((_, key)) = h.running.take_if(|(rid, _)| *rid == id) else {
            return;
        };
        if want.as_ref() == Some(&key) {
            h.shown = Some((key, image));
        }
    }

    /// A panicked heatmap job (the error dialog says so): its key shows no heatmap, so it is not retried every frame.
    pub(super) fn heatmap_panicked(&mut self) {
        let h = &mut self.preview.heat;
        if let Some((_, key)) = h.running.take() {
            h.shown = Some((key, None));
        }
    }

    /// A heatmap job is in flight: the app keeps frames coming until it lands. Not part of `busy()`, which locks
    /// edits; the heatmap is view-only.
    pub fn heatmap_pending(&self) -> bool {
        self.preview.heat.running.is_some()
    }

    /// The heatmap on show.
    pub fn heatmap_image(&self) -> Option<Arc<HeatImage>> {
        self.preview
            .heat
            .shown
            .as_ref()
            .and_then(|(_, i)| i.clone())
    }
}
