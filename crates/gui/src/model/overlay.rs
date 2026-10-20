//! The preview overlays (v2 artist checks, DECISIONS Note CI): the value-range overlay paints the texels a per-texel
//! CHK-RANGE rule rejects. Built on a worker from the preview's own source levels (no file read on the UI thread),
//! as a ready `ColorImage`, so the UI only uploads it. One job at a time, keyed by (item, mip, kind) plus the rule
//! set and the source; a result for an older key is dropped by its generation.
use super::checks::{rules_for, RangeRule};
use super::heatmap::HeatImage;
use super::preview::SourceState;
use super::AppModel;
use crate::worker::{or_send, spawn_guarded, WorkerMsg};
use eframe::egui::ColorImage;
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
    /// What the job in flight, or else `image`, was built for: item, mip relative to the source's first level, kind.
    pub key: Option<(String, usize, OverlayKind)>,
    /// The overlay on show, ready to upload. While a rebuild of the same item and kind runs (another mip or rule
    /// set), the previous one stays (review M1); `None` when the job failed.
    pub image: Option<Arc<ColorImage>>,
    generation: u64,
    running: bool,
    /// The rules the key's image paints (review M2: a new warning on the same source builds again).
    rules: Vec<RangeRule>,
    /// The source the key's image is built from: a reloaded source (an edit, a re-plan) builds again.
    source: Weak<SourcePreview>,
}

impl OverlayState {
    /// Drop the overlay on show and the key (review M7). A job in flight still clears `running` when it lands.
    pub fn clear(&mut self) {
        self.key = None;
        self.image = None;
        self.rules.clear();
        self.source = Weak::new();
    }
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
    /// key and returns the image on show (the landed one, or the previous one while a rebuild of the same item
    /// runs). Only for the expanded item with its source ready (K23).
    pub fn overlay_for(
        &mut self,
        item: &str,
        mip: usize,
        kind: OverlayKind,
    ) -> Option<Arc<ColorImage>> {
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
    ) -> Option<Arc<ColorImage>> {
        // shortcut: only the range overlay exists; Task 8 adds the UV coverage job here.
        if kind != OverlayKind::Range || self.preview.item.as_deref() != Some(item) {
            return None;
        }
        let SourceState::Ready(src) = &self.preview.source else {
            return None;
        };
        let rules = self.warned_rules(item);
        let o = &self.overlay;
        let same = o
            .key
            .as_ref()
            .is_some_and(|(i, m, k)| i == item && *m == mip && *k == kind);
        if same && o.rules == rules && o.source.upgrade().is_some_and(|s| Arc::ptr_eq(&s, src)) {
            return o.image.clone();
        }
        src.levels.get(mip)?;
        let src = src.clone();
        let o = &mut self.overlay;
        // Another item's (or kind's) overlay is never shown over this one.
        if o.key
            .as_ref()
            .is_none_or(|(i, _, k)| i != item || *k != kind)
        {
            o.image = None;
        }
        o.generation += 1;
        let generation = o.generation;
        o.key = Some((item.to_string(), mip, kind));
        o.rules = rules.clone();
        o.running = true;
        o.source = Arc::downgrade(&src);
        spawn_guarded(self.tx.clone(), "overlay", move |tx| {
            // A panic lands as this job's "no overlay", so v2_jobs_pending clears (K9) and the key is not retried.
            let image = or_send(
                tx,
                || {
                    let h = build(&src.levels[mip], &rules);
                    // Review I1: the per-texel conversion runs here, not on the UI thread.
                    ColorImage::from_rgba_unmultiplied(
                        [h.width as usize, h.height as usize],
                        &h.rgba,
                    )
                },
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
        self.overlay.image.clone()
    }

    pub(super) fn overlay_landed(&mut self, generation: u64, image: Option<Arc<ColorImage>>) {
        let o = &mut self.overlay;
        if generation == o.generation && o.running {
            o.running = false;
            // Cleared while it ran (toggle off, preview closed): drop the result.
            if o.key.is_some() {
                o.image = image;
            }
        }
    }

    /// An overlay job is in flight.
    pub fn overlay_pending(&self) -> bool {
        self.overlay.running
    }
}

#[cfg(test)]
mod tests {
    use crate::model::checks::RangeRule;
    use crate::model::overlay::OverlayKind;
    use crate::model::AppModel;
    use crate::settings::GuiSettings;
    use engine::codes::{Code, Warning};
    use std::sync::Arc;
    use std::time::Duration;

    /// An `_rmaos` whose R is 0 everywhere (rough extreme) and G clean, opened and expanded.
    fn rmaos_model(dir: &std::path::Path) -> AppModel {
        let root = dir.join("textures/test");
        std::fs::create_dir_all(&root).unwrap();
        image::RgbImage::from_pixel(16, 16, image::Rgb([0, 10, 200]))
            .save(root.join("wall_rmaos.png"))
            .unwrap();
        let session = Arc::new(
            engine::session::Session::new(encoders_sys::Bc1Model::Ideal).expect("a session"),
        );
        let mut m = AppModel::new(GuiSettings::default(), None, session);
        m.open_folder(dir.join("textures"));
        assert!(m.wait_idle(Duration::from_secs(60)));
        m.expand("test/wall_rmaos.png");
        assert!(m.wait_idle(Duration::from_secs(60)));
        m
    }

    /// Review M2: a second per-texel warning on the same source builds the overlay again.
    #[test]
    fn a_new_range_warning_on_the_same_source_rebuilds_the_overlay() {
        let d = tempfile::tempdir().unwrap();
        let mut m = rmaos_model(d.path());
        let key = "test/wall_rmaos.png";
        assert_eq!(m.warned_rules(key), [RangeRule::RoughExtreme]);
        m.overlay_for(key, 0, OverlayKind::Range);
        assert!(m.wait_idle(Duration::from_secs(10)));
        let first = m.overlay_for(key, 0, OverlayKind::Range).expect("landed");
        assert!(!m.overlay_pending());
        m.check_state.push_item_warning(
            key,
            Warning::new(Code::ChkRange, "grey metal")
                .with_data(serde_json::json!({"rule": "metal_grey"})),
        );
        assert_eq!(
            m.warned_rules(key),
            [RangeRule::RoughExtreme, RangeRule::MetalGrey]
        );
        // The rebuild starts, and the first overlay stays on show until it lands (M1).
        let during = m.overlay_for(key, 0, OverlayKind::Range);
        assert!(m.overlay_pending(), "the new rule set did not rebuild");
        assert!(during.is_some_and(|d| Arc::ptr_eq(&d, &first)));
        assert!(m.wait_idle(Duration::from_secs(10)));
        assert!(!Arc::ptr_eq(
            &m.overlay_for(key, 0, OverlayKind::Range).unwrap(),
            &first
        ));
    }

    /// Review M1, M7: another mip keeps the overlay on show until its own lands; collapsing frees it.
    #[test]
    fn a_mip_change_keeps_the_overlay_until_the_new_one_lands_and_collapse_frees_it() {
        let d = tempfile::tempdir().unwrap();
        let mut m = rmaos_model(d.path());
        let key = "test/wall_rmaos.png";
        m.overlay_for(key, 0, OverlayKind::Range);
        assert!(m.wait_idle(Duration::from_secs(10)));
        let mip0 = m.overlay_for(key, 0, OverlayKind::Range).expect("mip 0");
        assert_eq!(mip0.size, [16, 16]);
        let during = m.overlay_for(key, 1, OverlayKind::Range);
        assert!(m.overlay_pending());
        assert!(
            during.is_some_and(|d| Arc::ptr_eq(&d, &mip0)),
            "the overlay blanked"
        );
        assert!(m.wait_idle(Duration::from_secs(10)));
        assert_eq!(
            m.overlay_for(key, 1, OverlayKind::Range).unwrap().size,
            [8, 8]
        );
        m.collapse();
        assert!(m.overlay.image.is_none() && m.overlay.key.is_none());
    }
}
