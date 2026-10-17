//! The error heatmap off the UI thread (item 15, DECISIONS AG58): one job at a time on the worker pattern, keyed by
//! (result, mip, channel, full scale). The result `Arc` stands for the texture and its encode revision. The heatmap
//! on show stays until the one for the current key lands, but only for the same mip (a heatmap of another mip is
//! never stretched over this one: the plain image shows instead, DECISIONS AG62). A result that differs from the
//! current key only in its full scale is shown at once, so a slider drag never leaves the heatmap blank (AG63);
//! any other result for an older key is dropped.
use crate::model::preview::PreviewOut;
use crate::model::view::{heatmap_of, heatmap_vs_pin, Channel};
use crate::model::AppModel;
use crate::worker::{or_send, spawn_guarded, WorkerMsg};
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
    /// The pinned encode the current one is compared with (DECISIONS AG69): its id and images.
    pin: Option<u64>,
}

impl HeatKey {
    /// Same picture, only the full scale may differ.
    fn same_view(&self, o: &HeatKey) -> bool {
        Arc::ptr_eq(&self.out, &o.out)
            && self.mip == o.mip
            && self.channel == o.channel
            && self.pin_id() == o.pin_id()
    }

    fn pin_id(&self) -> Option<u64> {
        self.pin
    }
}

impl PartialEq for HeatKey {
    fn eq(&self, o: &HeatKey) -> bool {
        Arc::ptr_eq(&self.out, &o.out)
            && self.mip == o.mip
            && self.channel == o.channel
            && self.full_scale == o.full_scale
            && self.pin_id() == o.pin_id()
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
            pin: self.compared_pin().map(|p| p.id),
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
        // The pin's images go to the job only, never into the key: unpinning must free them (AG71).
        let pin_out = self.compared_pin().map(|p| p.out.clone());
        let h = &mut self.preview.heat;
        h.next_id += 1;
        let id = h.next_id;
        h.running = Some((id, want.clone()));
        spawn_guarded(self.tx.clone(), "heatmap", move |tx| {
            // A panic is this job's "no heatmap" (trace-debug #26), so its key is not retried every frame.
            let made = or_send(
                tx,
                || match &pin_out {
                    Some(pin) => {
                        heatmap_vs_pin(&want.out, pin, want.mip, want.channel, want.full_scale)
                    }
                    None => heatmap_of(&want.out, want.mip, want.channel, want.full_scale),
                },
                |_| WorkerMsg::Heatmap { id, image: None },
            );
            let image = made.map(|(width, height, rgba)| {
                Arc::new(HeatImage {
                    width,
                    height,
                    rgba,
                })
            });
            let _ = tx.send(WorkerMsg::Heatmap { id, image });
        });
    }

    pub(super) fn heatmap_landed(&mut self, id: u64, image: Option<Arc<HeatImage>>) {
        let want = self.heat_wanted();
        let h = &mut self.preview.heat;
        let Some((_, key)) = h.running.take_if(|(rid, _)| *rid == id) else {
            return;
        };
        if want.as_ref().is_some_and(|w| key.same_view(w)) {
            h.shown = Some((key, image));
        }
    }

    /// A heatmap job is in flight: the app keeps frames coming until it lands. Not part of `busy()`, which locks
    /// edits; the heatmap is view-only.
    pub fn heatmap_pending(&self) -> bool {
        self.preview.heat.running.is_some()
    }

    /// The heatmap on show: the one made for the mip on view (an older full scale is fine), else none.
    pub fn heatmap_image(&self) -> Option<Arc<HeatImage>> {
        let (key, image) = self.preview.heat.shown.as_ref()?;
        let want = self.heat_wanted()?;
        // Another mip, or the other comparison (source or pin), is not this picture; only the scale may lag.
        (key.mip == want.mip && key.pin_id() == want.pin_id())
            .then(|| image.clone())
            .flatten()
    }

    /// The heatmap is on but the one for the current key has not landed (the legend says "updating…").
    pub fn heatmap_updating(&self) -> bool {
        self.heat_wanted().is_some_and(|w| {
            self.preview
                .heat
                .shown
                .as_ref()
                .is_none_or(|(k, _)| *k != w)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::{bare_model, panicked};
    use crate::model::{preview, Dialog};

    /// Trace-debug #26: job A's panic lands after job B started; B keeps running and its result is shown.
    #[test]
    fn a_stale_heatmap_panic_does_not_blank_the_current_job() {
        let mut m = bare_model();
        let out = Arc::new(preview::PreviewOut {
            result: engine::session::PreviewResult {
                source: engine::session::SourcePreview {
                    first_level: 0,
                    dims: vec![(1, 1)],
                    levels: vec![],
                },
                format: engine::config::Format::Rgba8,
                srgb: false,
                levels: vec![],
                validation: Default::default(),
                warnings: vec![],
                normal_len: vec![],
            },
            decoded: vec![],
        });
        m.preview.encoded = preview::EncodedState::Done(out);
        m.view.heatmap = true;
        let key = m.heat_wanted().unwrap();
        m.preview.heat.running = Some((2, key));
        // Job 1 panicked: its own tagged result, then the dialog.
        m.handle(WorkerMsg::Heatmap { id: 1, image: None });
        m.handle(panicked("heatmap"));
        assert!(m.heatmap_pending(), "job 2 is still in flight");
        assert!(matches!(m.dialog, Some(Dialog::Error(_))));
    }
}
