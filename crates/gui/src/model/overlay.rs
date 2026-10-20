//! The preview overlays (v2 artist checks, DECISIONS Note CI): the value-range overlay paints the texels a per-texel
//! CHK-RANGE rule rejects. Built on a worker from the preview's own source levels (no file read on the UI thread),
//! one job at a time, keyed by (item, mip, kind); a result for an older key is dropped by its generation.
use super::checks::{rules_for, RangeRule};
use super::heatmap::HeatImage;
use super::preview::SourceState;
use super::AppModel;
use crate::worker::{or_send, spawn_guarded, WorkerMsg};
use engine::analyse::code8;
use engine::codes::Code;
use engine::image::Rgba32F;
use engine::session::SourcePreview;
use std::sync::{Arc, Weak};

/// What an overlay shows. `Uv` is Task 8's UV coverage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OverlayKind {
    Range,
    Uv,
}

/// A texel a rule rejects (magenta, about 60 % opaque).
pub const RANGE_BAD: [u8; 4] = [255, 0, 255, 160];

/// `level` with every texel that breaks any of `rules` (on the 8-bit codes of its stored values) in `RANGE_BAD` and
/// the rest clear.
pub fn range_overlay(level: &Rgba32F, rules: &[RangeRule]) -> HeatImage {
    let mut rgba = Vec::with_capacity(level.px.len() * 4);
    for p in &level.px {
        let px = p.map(code8);
        let bad = rules.iter().any(|r| r.bad(px));
        rgba.extend_from_slice(&if bad { RANGE_BAD } else { [0; 4] });
    }
    HeatImage {
        width: level.width,
        height: level.height,
        rgba,
    }
}

/// The overlay job and the overlay on show.
#[derive(Default)]
pub struct OverlayState {
    /// What `image` was (or is being) built for: item, mip relative to the source's first level, kind.
    pub key: Option<(String, usize, OverlayKind)>,
    /// `None` while the job runs, or when it failed.
    pub image: Option<Arc<HeatImage>>,
    generation: u64,
    running: bool,
    /// The source the key's image is built from: a reloaded source (an edit, a re-plan) builds again.
    source: Weak<SourcePreview>,
}

/// The builder an overlay job runs (tests swap in a slow or panicking one).
pub type Build = fn(&Rgba32F, &[RangeRule]) -> HeatImage;

impl AppModel {
    /// The per-texel rules `item`'s CHK-RANGE warnings name (not `dark_metal` or `ao_dark`, which paint nothing).
    fn warned_rules(&self, item: &str) -> Vec<RangeRule> {
        let Some(st) = self
            .item(item)
            .and_then(|i| i.job.as_ref())
            .map(|j| j.profile.subtype)
        else {
            return vec![];
        };
        let warned = |r: &RangeRule| {
            self.check_warnings(item).iter().any(|w| {
                w.code == Code::ChkRange
                    && w.data.get("rule").and_then(|v| v.as_str()) == Some(r.id())
            })
        };
        rules_for(st).iter().copied().filter(warned).collect()
    }

    /// The Range toggle is offered: one of `item`'s CHK-RANGE warnings names a per-texel rule (owner, spec "Value-range
    /// overlay"). Cheap enough per frame: one map lookup and a few warnings.
    pub fn range_overlay_offered(&self, item: &str) -> bool {
        !self.warned_rules(item).is_empty()
    }

    /// The overlay of `kind` for `item`'s source mip `mip` (relative to `first_level`): starts a worker job on a new
    /// key and returns the image once it has landed. Only for the expanded item with its source ready (K23).
    pub fn overlay_for(
        &mut self,
        item: &str,
        mip: usize,
        kind: OverlayKind,
    ) -> Option<Arc<HeatImage>> {
        self.overlay_for_with(item, mip, kind, range_overlay)
    }

    /// `overlay_for` with the job's builder given (tests).
    #[doc(hidden)]
    pub fn overlay_for_with(
        &mut self,
        item: &str,
        mip: usize,
        kind: OverlayKind,
        build: Build,
    ) -> Option<Arc<HeatImage>> {
        // shortcut: only the range overlay exists; Task 8 adds the UV coverage job here.
        if kind != OverlayKind::Range || self.preview.item.as_deref() != Some(item) {
            return None;
        }
        let SourceState::Ready(src) = &self.preview.source else {
            return None;
        };
        let o = &self.overlay;
        let same = o
            .key
            .as_ref()
            .is_some_and(|(i, m, k)| i == item && *m == mip && *k == kind);
        if same && o.source.upgrade().is_some_and(|s| Arc::ptr_eq(&s, src)) {
            return o.image.clone();
        }
        src.levels.get(mip)?;
        let src = src.clone();
        let rules = self.warned_rules(item);
        let o = &mut self.overlay;
        o.generation += 1;
        let generation = o.generation;
        o.key = Some((item.to_string(), mip, kind));
        o.image = None;
        o.running = true;
        o.source = Arc::downgrade(&src);
        spawn_guarded(self.tx.clone(), "overlay", move |tx| {
            // A panic lands as this job's "no overlay", so v2_jobs_pending clears (K9) and the key is not retried.
            let image = or_send(
                tx,
                || build(&src.levels[mip], &rules),
                |_| WorkerMsg::Overlay {
                    generation,
                    image: None,
                },
            );
            let _ = tx.send(WorkerMsg::Overlay {
                generation,
                image: Some(Arc::new(image)),
            });
        });
        None
    }

    pub(super) fn overlay_landed(&mut self, generation: u64, image: Option<Arc<HeatImage>>) {
        let o = &mut self.overlay;
        if generation == o.generation && o.running {
            o.running = false;
            o.image = image;
        }
    }

    /// An overlay job is in flight.
    pub fn overlay_pending(&self) -> bool {
        self.overlay.running
    }
}
