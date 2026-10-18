//! Review (layout v2 B2, C2, C5; rulings R1–R4): the pre-export checks as a bottom-panel tab, not a modal. It holds
//! only what the user answered; blockers, undecided textures and pitfalls are read live from the model.
use super::{bottom::BottomTab, pbr_export, AppModel, Continue, ItemStatus};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Review {
    /// R4: `export_asks` texts acknowledged for this export.
    pub acknowledged: BTreeSet<String>,
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

impl AppModel {
    /// Open Review (its ticks are kept when it is already open) and show its tab.
    pub fn open_review(&mut self) {
        self.review.get_or_insert_with(Review::default);
        self.show_tab(BottomTab::Review);
    }

    /// Close Review and its ticks; the panel collapses when it showed Review (R7: the tab is listed only while open).
    pub fn close_review(&mut self) {
        self.review = None;
        if self.bottom.tab == BottomTab::Review {
            self.collapse_bottom();
        }
    }

    /// R4: tick or untick one pitfall, keyed by its exact text. No-op while Review is closed.
    pub fn acknowledge_ask(&mut self, text: &str, on: bool) {
        let Some(r) = self.review.as_mut() else {
            return;
        };
        if on {
            r.acknowledged.insert(text.to_string());
        } else {
            r.acknowledged.remove(text);
        }
    }

    /// R4: Acknowledge all ticks every current pitfall. No-op while Review is closed.
    pub fn acknowledge_all_asks(&mut self) {
        let asks = pbr_export::export_asks(self);
        if let Some(r) = self.review.as_mut() {
            r.acknowledged.extend(asks.into_iter().map(|a| a.text));
        }
    }

    /// The files an export writes: items with a job that is neither undecided, packed into another nor skipped.
    pub fn export_files(&self) -> usize {
        self.items
            .iter()
            .filter(|i| {
                i.job.is_some()
                    && !matches!(
                        i.status,
                        ItemStatus::NeedsDecision
                            | ItemStatus::PackedInto(_)
                            | ItemStatus::Skipped(_)
                    )
            })
            .count()
    }

    /// Export can start: a folder is open, no export runs, and `sdc.toml` is not ignored (M3a spec §7.4: an export
    /// would not be the project's settings).
    pub fn can_export(&self) -> bool {
        let read_only = self.project.as_ref().is_some_and(|p| p.read_only());
        self.root.is_some() && self.export.running.is_none() && !read_only
    }

    /// K11: why Problems' and Review's fix buttons are off, or `None` when they work.
    pub fn fix_off(&self) -> Option<String> {
        self.toml_read_only_reason().or_else(|| {
            (self.planning.is_some() || self.votes_pending.is_some())
                .then(|| "Wait for the plan to finish".to_string())
        })
    }

    /// Ok((files, skipped_undecided)) or Err(the reason the button shows), in A9's order.
    pub fn review_gate(&self) -> Result<(usize, usize), String> {
        if self.root.is_none() {
            return Err("Open a folder first".into());
        }
        // `toml_read_only_reason` is `Some` exactly when `project.read_only()` (both read the load error).
        if let Some(why) = self.toml_read_only_reason() {
            return Err(why);
        }
        if self.export.running.is_some() {
            return Err("An export is running".into());
        }
        let n = pbr_export::export_blocks(self).len();
        if n > 0 {
            return Err(format!("Fix {n} blocker{} first", plural(n)));
        }
        let acked = self.review.as_ref().map(|r| &r.acknowledged);
        let k = pbr_export::export_asks(self)
            .iter()
            .filter(|a| !acked.is_some_and(|s| s.contains(&a.text)))
            .count();
        if k > 0 {
            return Err(format!("Acknowledge {k} warning{} first", plural(k)));
        }
        Ok((self.export_files(), self.undecided()))
    }

    /// "Export 20 files (skip 2 undecided)".
    pub fn review_button_text(&self) -> String {
        let files = self.export_files();
        let skipped = self.undecided();
        let mut t = format!("Export {files} file{}", plural(files));
        if skipped > 0 {
            t.push_str(&format!(" (skip {skipped} undecided)"));
        }
        t
    }

    /// Review's Export (R3): `export_clicked`'s front half (a plan in flight → wait; changed sources → plan again and
    /// wait), then `Continue::Review`: the guards and the gate again before the export. Never on a stale plan.
    pub fn export_from_review(&mut self) {
        if self.root.is_none() {
            return;
        }
        self.status_note = None;
        if self.planning.is_some() || self.votes_pending.is_some() {
            self.continue_after_plan = Some(Continue::Review);
            return;
        }
        if self.check_sources() > 0 && self.session.is_some() {
            self.continue_after_plan = Some(Continue::Review);
            self.replan();
            return;
        }
        self.run_continue(Continue::Review);
    }
}
