//! Pin an encode and compare it against the next one (item 10, DECISIONS AG64-AG71). The pin holds the finished
//! encode (its `Arc`, so the decoded images are shared, not copied or re-encoded) and a summary of the settings that
//! made it. One pin at most; it lives in `PreviewModel`, so closing the preview, a texture change and a folder open
//! drop it, and a settings change does not.
use crate::model::preview::{EncodedState, PreviewOut};
use crate::model::readout::Side;
use crate::model::AppModel;
use engine::config::Profile;
use std::sync::Arc;

/// What the pinned side says when the pinned encode lacks the mip on view.
pub const NOT_IN_PIN: &str = "— not in the pinned encode";

pub struct Pin {
    /// Never reused within a session: the heatmap job is keyed on it.
    pub id: u64,
    pub out: Arc<PreviewOut>,
    /// Format, tag, encoder and effort, alpha policy and mips options of the settings that made `out`.
    pub summary: String,
}

/// The settings that shape an encode, in one line.
fn word<T: serde::Serialize>(v: &T) -> String {
    match serde_json::to_value(v) {
        Ok(serde_json::Value::String(s)) => s.replace('_', " "),
        Ok(v) => v.to_string(),
        Err(_) => String::new(),
    }
}

pub fn summary_of(p: &Profile) -> String {
    let encoder = word(&p.encoder.backend);
    format!(
        "{}, {} {} effort, alpha {}, mips {} {}",
        crate::model::fields::format_label(p.format, p.color.tag),
        crate::model::texture_view::encoder_label(&encoder),
        word(&p.encoder.effort),
        word(&p.alpha.policy),
        word(&p.mips.levels),
        word(&p.mips.filter)
    )
}

/// "PSNR 45.3 dB vs source", or a dash when the encode has none.
fn psnr_text(out: &PreviewOut) -> String {
    match out.result.validation.min_mip_psnr_db {
        Some(p) => format!("PSNR {p:.1} dB vs source"),
        None => "PSNR — vs source".into(),
    }
}

/// The decoded image of absolute mip `mip` of `out`.
pub(crate) fn decoded_side(out: &PreviewOut, mip: usize) -> Option<Side<'_>> {
    let (width, height) = out.result.source.dims.get(mip).copied()?;
    let px = out
        .decoded
        .get(mip.checked_sub(out.result.source.first_level)?)?;
    Some(Side { width, height, px })
}

impl AppModel {
    fn expanded_profile(&self) -> Option<&Profile> {
        let item = self.item(self.expanded.as_deref()?)?;
        item.job.as_ref().map(|j| &j.profile)
    }

    /// A finished encode of a texture with a job: its settings match the result, so the pin's summary is true.
    pub fn can_pin(&self) -> bool {
        matches!(self.preview.encoded, EncodedState::Done(_)) && self.expanded_profile().is_some()
    }

    /// Pin the encode on show; a pin already held is replaced (its images are released). Turns the comparison on.
    pub fn pin_this(&mut self) {
        if !self.can_pin() {
            return;
        }
        let EncodedState::Done(out) = &self.preview.encoded else {
            return;
        };
        let out = out.clone();
        let summary = summary_of(self.expanded_profile().expect("can_pin checked it"));
        self.preview.heat = self.preview.heat.cleared();
        self.preview.next_pin += 1;
        self.preview.pin = Some(Pin {
            id: self.preview.next_pin,
            out,
            summary,
        });
        self.preview.compare_pinned = true;
    }

    pub fn unpin(&mut self) {
        self.preview.pin = None;
        self.preview.heat = self.preview.heat.cleared();
        self.preview.compare_pinned = false;
    }

    /// "Compare: Source | Pinned". Pinned needs a pin.
    pub fn set_compare_pinned(&mut self, on: bool) {
        self.preview.compare_pinned = on && self.preview.pin.is_some();
    }

    /// The side-by-side and split layouts show Pinned against Current.
    pub fn comparing_pin(&self) -> bool {
        self.preview.compare_pinned && self.preview.pin.is_some()
    }

    /// The pin while comparing.
    pub fn compared_pin(&self) -> Option<&Pin> {
        self.preview.pin.as_ref().filter(|_| self.comparing_pin())
    }

    /// `NOT_IN_PIN` when comparing and the pinned encode has no mip `mip`.
    pub fn pin_missing_text(&self, mip: usize) -> Option<&'static str> {
        let pin = self.compared_pin()?;
        decoded_side(&pin.out, mip).is_none().then_some(NOT_IN_PIN)
    }

    /// The captions of the left (pinned) and right (current) side while comparing: the settings summary and the
    /// PSNR against the source.
    pub fn pin_captions(&self) -> Option<(String, String)> {
        let pin = self.compared_pin()?;
        let cur = self.preview.encoded.shown()?;
        // Only a finished result was made by the settings now on show.
        let current = match (&self.preview.encoded, self.expanded_profile()) {
            (EncodedState::Done(_), Some(p)) => summary_of(p),
            _ => "previous result, settings changed".into(),
        };
        Some((
            format!("Pinned: {} · {}", pin.summary, psnr_text(&pin.out)),
            format!("Current: {current} · {}", psnr_text(cur)),
        ))
    }

    /// The legend's label for the heatmap on show.
    pub fn heatmap_label(&self) -> &'static str {
        if self.comparing_pin() {
            "Heatmap |Δ| Current vs Pinned"
        } else {
            "Heatmap |Δ| Source vs Encoded"
        }
    }
}
