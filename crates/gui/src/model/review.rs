//! Review (layout v2 B2, C2, C5; rulings R1–R4): the pre-export checks as a bottom-panel tab, not a modal. It holds
//! only what the user answered; blockers, undecided textures and pitfalls are read live from the model.
use super::history::{record_key, records_at, sha256_of};
use super::{
    bottom::BottomTab, cm_json, cs_pbr_export, pbr_export, AppModel, Continue, ItemStatus,
};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Note FB1: the gate's reason, and the Blockers row's text, while no export folder is chosen.
pub const NO_EXPORT_FOLDER: &str = "No export folder chosen";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Review {
    /// R4: `export_asks` texts acknowledged for this export.
    pub acknowledged: BTreeSet<String>,
    /// C4, R6: the answer, given in Review, for the edited PGPatcher JSON.
    pub json_answer: Option<JsonAnswer>,
    /// Note CN (Review Focus 5, P9): with a tick on, why `sdc.toml` cannot be saved; taken when Review opens and
    /// again on its Export, never per frame. `None`: writable (or no tick).
    pub toml_unwritable: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonKind {
    PgPatcher,
    CsPbr,
    ComplexMaterial,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskState {
    New,
    Ours,
    Edited,
    Unreadable,
}

/// One JSON file the export would write, and what is on disk there now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonLine {
    pub kind: JsonKind,
    pub path: PathBuf,
    pub state: DiskState,
}

/// Keep or overwrite the edited PGPatcher JSON at `path`, for the file whose hash was `sha` when answered: a file
/// changed since gets the question again (C4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonAnswer {
    pub path: PathBuf,
    pub sha: Option<String>,
    pub overwrite: bool,
}

/// What is at `path` against the hash recorded for Tessalith's last write there.
pub fn disk_state(path: &Path, recorded: Option<&str>) -> DiskState {
    use sha2::{Digest, Sha256};
    match std::fs::read(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => DiskState::New,
        Err(_) => DiskState::Unreadable,
        Ok(b) if recorded == Some(hex::encode(Sha256::digest(&b)).as_str()) => DiskState::Ours,
        Ok(_) => DiskState::Edited,
    }
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
        let unwritable = self.also_toml_unwritable();
        self.review
            .get_or_insert_with(Review::default)
            .toml_unwritable = unwritable;
        self.show_tab(BottomTab::Review);
    }

    /// C4: the JSON files the export would write and their state on disk. Cached (K3) per view key for 1 s, since
    /// Review draws it every frame and it reads the files (AG128).
    pub fn review_json(&self) -> Vec<JsonLine> {
        let key = self.view_key();
        if let Some((k, at, lines)) = &*self.review_json_cache.borrow() {
            if *k == key && at.elapsed() < Duration::from_secs(1) {
                return lines.clone();
            }
        }
        let mut lines = vec![];
        // K13: vanilla (and a project without a PBR set) writes no PGPatcher JSON.
        if let Some(p) = pbr_export::prepare(self).filter(|p| !p.entries.is_empty()) {
            let state = disk_state(&p.path, self.pgp_record(&p.root, &p.path).as_deref());
            lines.push(JsonLine {
                kind: JsonKind::PgPatcher,
                path: p.path,
                state,
            });
        }
        let doc = self.sets_doc.as_ref();
        let mut files = |kind, root: PathBuf, rels: Vec<String>, shas| {
            // As the finishers read them: a relative record is under the export folder.
            let shas = records_at(&shas, &root);
            for rel in rels {
                let path = root.join(rel);
                let state = disk_state(&path, shas.get(&record_key(&path)).map(String::as_str));
                lines.push(JsonLine { kind, path, state });
            }
        };
        if let (Some(p), Some(d)) = (cs_pbr_export::prepare(self), doc) {
            let rels = p.files.into_iter().map(|(r, _)| r).collect();
            files(JsonKind::CsPbr, p.root, rels, d.cs_pbr_shas());
        }
        if let (Some(p), Some(d)) = (cm_json::prepare(self), doc) {
            let rels = p.files.into_iter().map(|(r, _)| r).collect();
            files(JsonKind::ComplexMaterial, p.root, rels, d.cm_json_shas());
        }
        *self.review_json_cache.borrow_mut() = Some((key, Instant::now(), lines.clone()));
        lines
    }

    /// C4: Keep (false) or Overwrite (true) the edited PGPatcher JSON, for the file as it is now. No-op while Review
    /// is closed.
    pub fn answer_review_json(&mut self, overwrite: bool) {
        if self.review.is_none() {
            return;
        }
        let Some(p) = pbr_export::prepare(self).filter(|p| !p.entries.is_empty()) else {
            return;
        };
        let sha = sha256_of(&p.path);
        if let Some(r) = self.review.as_mut() {
            r.json_answer = Some(JsonAnswer {
                path: p.path,
                sha,
                overwrite,
            });
        }
    }

    /// R6: the PGPatcher JSON this export would write exists and is not Tessalith's (uncached: one read per click).
    pub fn pgp_json_edited(&self) -> bool {
        pbr_export::prepare(self)
            .filter(|p| !p.entries.is_empty())
            .is_some_and(|p| {
                disk_state(&p.path, self.pgp_record(&p.root, &p.path).as_deref())
                    == DiskState::Edited
            })
    }

    /// Close Review and its ticks; the panel collapses when it showed Review (R7: the tab is listed only while open).
    pub fn close_review(&mut self) {
        self.review = None;
        self.review_json_cache.take();
        // Its queued export goes with it (Task 1.4's Close).
        self.continue_after_plan = None;
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
    /// Cached like `bottom::problems` (Review and the header read it every frame): `items` is only replaced
    /// together with `invalidate_views`, which changes the view key.
    pub fn export_files(&self) -> usize {
        let key = self.view_key();
        let inner = super::bottom::problems_inner(self);
        {
            let c = self.views.borrow();
            if c.key.as_ref() == Some(&key) {
                if let Some((k, n)) = c.export_files {
                    if k == inner {
                        return n;
                    }
                }
            }
        }
        let n = self.count_export_files();
        self.export_files_builds
            .set(self.export_files_builds.get() + 1);
        let mut c = self.views.borrow_mut();
        if c.key.as_ref() != Some(&key) {
            *c = super::ViewCache {
                key: Some(key),
                ..Default::default()
            };
        }
        c.export_files = Some((inner, n));
        n
    }

    fn count_export_files(&self) -> usize {
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
        // Note FB1: never write to a folder nobody chose (the `sdc-out` fallback is only a name).
        if self.export_folder_chosen().is_none() {
            return Err(NO_EXPORT_FOLDER.into());
        }
        if self.export.running.is_some() {
            return Err("An export is running".into());
        }
        // Note FC1 (AM14): the plan ran on the fallback; exporting it would not be the project's preset (the CLI
        // exits 2 on the same file).
        if let Some(id) = self.missing_preset_id() {
            return Err(format!("Preset {id} is not installed"));
        }
        let n = pbr_export::export_blocks(self).len();
        if n > 0 {
            return Err(format!("Fix {n} blocker{} first", plural(n)));
        }
        // Note CN: the Also rows, after the blocks and before the acknowledgements (A9); nothing writes (7.1, 7.2).
        if self.also_on() {
            self.also_gate()?;
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

    /// Note FB1: Review would write without skipping anything: a finished plan with files to write, the gate open and
    /// nothing undecided (A12 withheld "ready" from an empty or still-planning project too).
    pub fn export_is_clean(&self) -> bool {
        self.planning.is_none()
            && self.votes_pending.is_none()
            && self.export_files() > 0
            && self.review_gate().is_ok()
            && self.undecided() == 0
    }

    /// Note FB1: the header's Export button: `Export {n} files` once the project is clean, else `Review {n} files`.
    pub fn header_export_text(&self) -> String {
        self.export_text(self.export_is_clean())
    }

    /// [`Self::header_export_text`] for a `clean` the caller already has (the header asks once per frame).
    pub fn export_text(&self, clean: bool) -> String {
        // Final review M5: with no folder open there is no count to give ("Review 0 files" greeted a new user).
        if self.root.is_none() {
            return "Export".into();
        }
        let n = self.export_files();
        let verb = if clean { "Export" } else { "Review" };
        format!("{verb} {n} file{}", plural(n))
    }

    /// "Export 20 files (skip 2 undecided)".
    pub fn review_button_text(&self) -> String {
        let files = self.export_files();
        let skipped = self.undecided();
        let mut t = format!("Export {files} file{}", plural(files));
        // Re-check A7 (TA30): this project's planned files, written to the PBR folder and copied or derived on.
        if self.also_on() {
            t.push_str(&format!(" to {} folders", self.also_folder_count()));
        }
        if skipped > 0 {
            t.push_str(&format!(" (skip {skipped} undecided)"));
        }
        t
    }

    /// Review's Export (R3): `export_clicked`'s front half (a plan in flight → wait; changed sources → plan again and
    /// wait), then `Continue::Review`: the guards and the gate again before the export. Never on a stale plan.
    pub fn export_from_review(&mut self) {
        if self.root.is_none() || self.review.is_none() {
            return;
        }
        self.status_note = None;
        self.continue_review();
    }

    /// The front half of Review's Export, also run when a scan-finished read it waited for lands.
    pub(super) fn continue_review(&mut self) {
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
