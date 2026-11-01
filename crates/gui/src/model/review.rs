//! Review (layout v2 B2, C2, C5; rulings R1–R4): the pre-export checks as a bottom-panel tab, not a modal. It holds
//! only what the user answered; blockers, undecided textures and pitfalls are read live from the model.
use super::history::{record_key, records_at, sha256_of};
use super::{
    bottom::BottomTab, cm_json, cs_pbr_export, pbr_export, AppModel, Continue, ItemStatus,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
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
    /// Note FC2: the half-size answers by texture key, for this export only; a row with no entry is unanswered.
    pub half_size: BTreeMap<String, Option<HalfAnswer>>,
    /// Note FC2: why a Half answer cannot be saved (`sdc.toml` read-only); taken when Review opens and again on its
    /// Export. `None`: Half is answerable.
    pub half_unwritable: Option<String>,
    /// Note FC2: the *Half size* step Review's Export wrote, as (undo depth, `history.pushes`) after it, until the
    /// export starts; a refused export or Close undoes it.
    pub half_step: Option<(usize, u64)>,
}

/// Note FC2: write a texture at half size (`drop_top_mip = true`) or as it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HalfAnswer {
    Half,
    Full,
}

/// Note FC2: a texture the last export flagged (`CHK-LOW-DETAIL`) that the plan would still write at full size.
#[derive(Debug, Clone, PartialEq)]
pub struct HalfSizeRow {
    pub key: String,
    /// The texture's name, as the row shows it.
    pub name: String,
    /// The engine's sentence, for the hover (CL15: the code and "mip 0" are hover text).
    pub detail: String,
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
    /// Note FC5: the file's hash now, for an edited PGPatcher JSON only (what an answer is tied to).
    pub sha: Option<String>,
}

/// Keep or overwrite the edited PGPatcher JSON at `path`, for the file whose hash was `sha` when answered: a file
/// changed since gets the question again (C4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonAnswer {
    pub path: PathBuf,
    pub sha: Option<String>,
    pub overwrite: bool,
}

impl JsonAnswer {
    /// Note FC5: this answer is for `l` as it is on disk now; a file changed since the answer is asked again.
    pub fn answers(&self, l: &JsonLine) -> bool {
        self.path == l.path && self.sha == l.sha
    }
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

/// FC11 (Review Focus 4): a stale file whose stamp no longer matches its record, or that has none: no button.
pub const STALE_NOT_OURS: &str = "edited since Tessalith wrote it: left alone";

/// Note FC12 (CN45): the Blockers row of a zip at or over `limits` (entries, bytes; the ZIP format's are
/// `engine::zip::MAX_ENTRIES` and `MAX_BYTES`), `None` when it fits.
pub fn zip_block(e: &engine::zip::ZipEstimate, limits: (usize, u64)) -> Option<String> {
    (e.entries >= limits.0 || e.bytes >= limits.1).then(|| {
        format!(
            "Zip too large for the ZIP format ({} files, {}); untick \"Also write a zip\" or split the export",
            super::thousands(e.entries),
            super::out_size::human(e.bytes)
        )
    })
}

impl AppModel {
    /// Note FC12: "Also write a zip" is in effect (it applies only with an Also tick on).
    pub(crate) fn zip_on(&self) -> bool {
        self.also_on() && self.also_config().zip
    }

    /// Note FC12: the zip-size row's limits (entries, bytes); a test lowers them to reach the row cheaply.
    #[doc(hidden)]
    pub fn set_zip_limits(&mut self, entries: usize, bytes: u64) {
        self.zip_limits = (entries, bytes);
    }

    /// Note FC12: the zip-size Blockers row, when the PBR folder's zip would not fit.
    pub(crate) fn zip_row(&self) -> Option<String> {
        zip_block(&self.zip_estimate()?, self.zip_limits)
    }

    /// Note FC12 (CN45): the PBR folder's zip, when "Also write a zip" is on: every file `finish_also` puts in it
    /// (each planned output, which it zips by job key, plus the PGPatcher, CS PBR and complex-material JSON the export
    /// writes into the folder; `_sdc` is never zipped), outputs at their estimated DDS size and JSON at its text size.
    // shortcut: only the PBR folder's zip is counted (the Vanilla and CM zips hold at most about as many files, all
    // derived from the PBR sets) and an output whose source size the plan did not read counts 0 bytes; `write_zip`
    // still refuses what slips past, as a note. Count the other folders' plans if a mod gets past this row and fails
    // at the zip.
    pub fn zip_estimate(&self) -> Option<engine::zip::ZipEstimate> {
        if !self.zip_on() {
            return None;
        }
        let session = self.session.as_ref();
        // The PGPatcher JSON's text is built at the export's end, so it counts 0 bytes (a few KiB).
        let pgp = super::pbr_export::prepare(self).map(|_| 0);
        let cs = super::cs_pbr_export::prepare(self)
            .into_iter()
            .flat_map(|p| p.files);
        let cm = super::cm_json::prepare(self)
            .into_iter()
            .flat_map(|p| p.files);
        let json = pgp
            .into_iter()
            .chain(cs.chain(cm).map(|(_, text)| text.len() as u64));
        let sizes = self.items.iter().filter_map(|i| {
            let j = i.job.as_ref()?;
            if matches!(
                i.status,
                ItemStatus::NeedsDecision | ItemStatus::PackedInto(_) | ItemStatus::Skipped(_)
            ) {
                return None;
            }
            let dims = j
                .source
                .and_then(|st| session.and_then(|s| s.cache().dims(&j.input, st)));
            Some(dims.map_or(0, |d| {
                super::out_size::estimate_job(d, &j.profile, j.cube.as_ref()).file_bytes()
            }))
        });
        Some(engine::zip::estimate(sizes.chain(json)))
    }
}

impl AppModel {
    /// FC11: a `Stale outputs` row: the path under the export folder, and why it is listed.
    pub fn stale_text(&self, s: &engine::manifest::StaleOutput) -> String {
        let rel = self
            .sets_root
            .as_deref()
            .and_then(|r| s.path.strip_prefix(r).ok())
            .unwrap_or(&s.path);
        let why = match (s.ours, s.reason) {
            (false, _) => STALE_NOT_OURS,
            (true, engine::manifest::StaleWhy::SourceGone) => "its source is gone",
            (true, engine::manifest::StaleWhy::NotWritten) => "no longer written",
        };
        format!("{} · {why}", rel.display())
    }

    /// FC11 (fix round 1): the JSON files with a hash record that this project no longer writes (AG90's PGPatcher JSON
    /// under an earlier name or off the CS target, AD5's CS PBR JSON no block binds, AT3's complex-material JSON no
    /// set writes), as `stale` rows. The recorded SHA-256 is their baseline: ours when the file still hashes to it.
    /// Run when a plan settles and after an export's JSON step, never per frame (it hashes the files).
    pub(crate) fn refresh_stale_json(&mut self) {
        let old = std::mem::take(&mut self.stale_json);
        self.stale.retain(|s| !old.contains_key(&s.path));
        let Some(d) = self.sets_doc.as_ref() else {
            return;
        };
        let root = self.export_root();
        let rel = |root: &Path, p: &Path| {
            p.strip_prefix(root)
                .unwrap_or(p)
                .to_string_lossy()
                .into_owned()
        };
        // Note FC14 (final review I2): the PGPatcher JSON's path is derived from its name, whatever `prepare` gives: a
        // set undecided or blocked for now makes no entry, and the file PGPatcher reads must not get a Delete then.
        // As `PendingJson.root`: the PBR folder while a tick is on, else the export folder.
        let pgp_root = self.output_root();
        let pgp_made = match self.target {
            engine::config::Target::Cs => {
                let name = format!("{}.json", pbr_export::json_name(self));
                vec![rel(&pgp_root, &pgp_root.join("PBRNIFPatcher").join(name))]
            }
            _ => vec![],
        };
        let mut kinds = vec![(d.pgp_json_shas(), pgp_root, pgp_made)];
        // A refused CS file (a bad or shared EDID) keeps its record, as `cs_pbr_export::finish` does.
        kinds.push(match cs_pbr_export::prepare(self) {
            Some(p) => {
                let made = p.files.into_iter().map(|(r, _)| r).chain(p.refused);
                (d.cs_pbr_shas(), p.root, made.collect())
            }
            None => (d.cs_pbr_shas(), root.clone(), vec![]),
        });
        // A ticked set whose complex `_m` is undecided for now writes nothing this time: that kind is left out.
        match cm_json::prepare(self) {
            Some(p) if p.partial => {}
            Some(p) => {
                let made = p.files.into_iter().map(|(r, _)| r).collect();
                kinds.push((d.cm_json_shas(), p.root, made));
            }
            None => kinds.push((d.cm_json_shas(), root.clone(), vec![])),
        }
        for (shas, root, made) in kinds {
            let shas = records_at(&shas, &root);
            for path in super::history::orphans(&shas, &root, made.iter()) {
                let Some(sha) = shas.get(&record_key(&path)) else {
                    continue;
                };
                // `orphans` fixes the file name's case only; the folders keep the record's (folded) spelling.
                let path = engine::manifest::on_disk_case(path);
                if self.stale_json.contains_key(&path) {
                    continue;
                }
                self.stale.push(engine::manifest::StaleOutput {
                    ours: sha256_of(&path).as_deref() == Some(sha.as_str()),
                    reason: engine::manifest::StaleWhy::NotWritten,
                    path: path.clone(),
                });
                self.stale_json.insert(path, sha.clone());
            }
        }
        // Problems' stale row reads `stale`; rebuild only when there were or are JSON rows (K9 build counters).
        if !old.is_empty() || !self.stale_json.is_empty() {
            self.invalidate_views();
        }
    }

    /// FC11 (Review Focus 3, 4): the `Delete` of a `Stale outputs` row, on its click only. Deletes only a path the
    /// last plan listed as ours, never while an export runs, and only when the file still has the size and mtime its
    /// manifest record holds (checked again now); otherwise the row reads "edited since…" and the file stays.
    pub fn delete_stale_output(&mut self, path: &Path) -> Result<(), String> {
        if !self.stale.iter().any(|s| s.ours && s.path == path) {
            return Err(format!(
                "{} is not a stale output Tessalith wrote",
                path.display()
            ));
        }
        if self.export.running.is_some() {
            return Err(super::also::EXPORT_RUNNING.into());
        }
        let still_ours = match self.stale_json.get(path) {
            // A JSON row: its recorded SHA-256 is the baseline.
            Some(sha) => sha256_of(path).as_deref() == Some(sha.as_str()),
            None => self.manifest_stamp_matches(path),
        };
        if !still_ours {
            for s in self.stale.iter_mut().filter(|s| s.path == path) {
                s.ours = false;
            }
            self.invalidate_views();
            let e = format!("{}: {STALE_NOT_OURS}", path.display());
            self.status_line = e.clone();
            return Err(e);
        }
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{}: {e}", path.display())),
        }
        self.stale.retain(|s| s.path != path);
        self.stale_json.remove(path);
        self.invalidate_views();
        self.status_line = format!("Deleted {}", path.display());
        Ok(())
    }

    /// FC11: the file still has the size and mtime its manifest record holds (read again now).
    fn manifest_stamp_matches(&self, path: &Path) -> bool {
        self.sets_root.as_deref().is_some_and(|root| {
            let key = engine::manifest::output_key(root, path);
            engine::manifest::Manifest::at(root)
                .read()
                .ok()
                .and_then(|r| r.get(&key)?.output_stamp)
                .is_some_and(|s| Some(s) == engine::policy::SourceStamp::of(path))
        })
    }

    /// Open Review (its ticks are kept when it is already open) and show its tab.
    pub fn open_review(&mut self) {
        let unwritable = self.also_toml_unwritable();
        self.review
            .get_or_insert_with(Review::default)
            .toml_unwritable = unwritable;
        self.refresh_half_unwritable();
        self.show_tab(BottomTab::Review);
    }

    /// Note FC2 (finding r1 #2): whether a Half answer can be saved, taken when Review opens and on its Export (one
    /// stat, never per frame). Unsaveable, a Half answer already given is dropped, so the row asks again.
    pub(super) fn refresh_half_unwritable(&mut self) {
        let why = if self.half_size_rows().is_empty() {
            None
        } else {
            self.sdc_toml_writable().err()
        };
        if let Some(r) = self.review.as_mut() {
            if why.is_some() {
                r.half_size.retain(|_, a| *a != Some(HalfAnswer::Half));
            }
            r.half_unwritable = why;
        }
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
            let sha = (state == DiskState::Edited)
                .then(|| sha256_of(&p.path))
                .flatten();
            lines.push(JsonLine {
                kind: JsonKind::PgPatcher,
                path: p.path,
                state,
                sha,
            });
        }
        let doc = self.sets_doc.as_ref();
        let mut files = |kind, root: PathBuf, rels: Vec<String>, shas| {
            // As the finishers read them: a relative record is under the export folder.
            let shas = records_at(&shas, &root);
            for rel in rels {
                let path = root.join(rel);
                let state = disk_state(&path, shas.get(&record_key(&path)).map(String::as_str));
                lines.push(JsonLine {
                    kind,
                    path,
                    state,
                    sha: None,
                });
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
        // The next frame's lines hash the file again, so the answer matches the line it was given for.
        self.review_json_cache.take();
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
        // Note FC5 (GD29): the NIF answers go with Review; each foreign row asks again. Not while a run Review started
        // still owes the NIF export (a Reload mid-export): those answers are its own (rule 7.4).
        self.nif_clear_answers_request |= !self.nif_export_queued;
        self.nif_answer_request = None;
        self.review_json_cache.take();
        // Its queued export goes with it (Task 1.4's Close).
        self.continue_after_plan = None;
        if self.bottom.tab == BottomTab::Review {
            self.collapse_bottom();
        }
    }

    /// Review's Close (and Esc): a half size its Export wrote for an export that has not started is taken back
    /// (finding r1 #3), then Review closes.
    pub fn dismiss_review(&mut self) {
        self.undo_half_step();
        self.close_review();
    }

    /// Note FC2 (finding r1 #3): undo the *Half size* step Review's Export wrote, when the export it was for did not
    /// start and the step is still the top one (nothing undone or edited since).
    pub(super) fn undo_half_step(&mut self) {
        let Some((depth, pushes)) = self.review.as_mut().and_then(|r| r.half_step.take()) else {
            return;
        };
        if self.history.undo.len() == depth && self.history.pushes == pushes {
            self.undo();
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
            // Note GH8: with no project Review exports NIF edits only, to the gui.toml folder (never a `sdc-out` guess).
            // No NIF row at all is the fresh-start state, which keeps its old reason; a blocked row (no folder) says why.
            if self.nif_rows.is_empty() {
                return Err("Open a folder first".into());
            }
            if self.export_folder_chosen().is_none() {
                return Err(NO_EXPORT_FOLDER.into());
            }
            if self.export.running.is_some() {
                return Err("An export is running".into());
            }
            if !self.nif_rows_written() {
                return Err("Open a folder first".into());
            }
            if let Some(name) = self.unanswered_overwrites().first() {
                return Err(format!("Answer Keep or Overwrite for {name}"));
            }
            return Ok((0, 0));
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
        // Note FC2: last, as the questions only Review asks.
        let n = self.unanswered_half_size();
        if n > 0 {
            return Err(format!(
                "Answer half or full size for {n} texture{}",
                plural(n)
            ));
        }
        // Note FC5: then every overwrite question (supersedes CA7's "no choice made: asked after the export").
        if let Some(name) = self.unanswered_overwrites().first() {
            return Err(format!("Answer Keep or Overwrite for {name}"));
        }
        Ok((self.export_files(), self.undecided()))
    }

    /// Note FC5: the file names of the overwrite questions Review has no answer for: an edited PGPatcher JSON, then
    /// each NIF whose export path holds a file Tessalith did not write (a blocked NIF row is skipped, never asked).
    pub fn unanswered_overwrites(&self) -> Vec<String> {
        let answered = self.review.as_ref().and_then(|r| r.json_answer.as_ref());
        let name = |p: &Path| {
            p.file_name().map_or_else(
                || p.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            )
        };
        // shortcut: the header asks this every frame (export_is_clean), so `review_json` rereads the JSON files once a
        // second through its 1 s cache, even with no PGPatcher JSON edited (knowing that needs the same read); add a
        // cheap "any PBR set" check or an mtime key if a large CS PBR or CM set list makes that read show in a profile.
        let json = self.review_json().into_iter().filter(|l| {
            l.kind == JsonKind::PgPatcher
                && l.state == DiskState::Edited
                && !answered.is_some_and(|a| a.answers(l))
        });
        let nifs = self
            .nif_rows
            .iter()
            .filter(|r| r.blocked.is_none() && r.foreign.is_some() && r.answer.is_none());
        json.map(|l| name(&l.path))
            .chain(nifs.map(|r| name(Path::new(&r.path))))
            .collect()
    }

    /// Note FC5: the NIF edits Review lists, handed over by `app.rs` each frame.
    pub fn set_nif_review_rows(&mut self, rows: Vec<super::meshes::NifReviewRow>) {
        self.nif_rows = rows;
    }

    pub fn nif_review_rows(&self) -> &[super::meshes::NifReviewRow] {
        &self.nif_rows
    }

    /// Note FC14: a NIF edit Review's Export would write (a blocked one is skipped, so it keeps nothing waiting).
    pub(crate) fn nif_rows_written(&self) -> bool {
        self.nif_rows.iter().any(|r| r.blocked.is_none())
    }

    /// Note FC5: Review's Keep (false) or Overwrite (true) for the NIF at `path`, for the file with hash `sha256`;
    /// `app.rs` passes it to `MeshesModel::set_nif_answer`. No-op while Review is closed.
    pub fn answer_review_nif(&mut self, path: &str, overwrite: bool, sha256: [u8; 32]) {
        use super::meshes::export::GuardChoice;
        if self.review.is_none() {
            return;
        }
        let choice = if overwrite {
            GuardChoice::Overwrite
        } else {
            GuardChoice::KeepTheirs
        };
        self.nif_answer_request = Some((path.to_string(), choice, sha256));
    }

    /// Note FC5: true once, in the frame after the run Review's Export started has fully ended (with Also on, after
    /// its zips); `app.rs` then calls `MeshesModel::export_nif_edits` and passes the result to `nif_export_result`.
    pub fn take_nif_export_due(&mut self) -> bool {
        std::mem::take(&mut self.nif_export_due)
    }

    /// Note FC5: the NIF export's result, in Messages and in the export's notes (Review is closed by then; the notes
    /// are the Files tab's lines).
    pub fn nif_export_result(&mut self, result: Result<usize, String>) {
        let (sev, text) = match result {
            Ok(0) => return,
            Ok(n) => (
                super::messages::Severity::Info,
                format!("NIF edits written: {n} file{}", plural(n)),
            ),
            Err(why) => (
                super::messages::Severity::Warning,
                format!("NIF edits skipped: {why}"),
            ),
        };
        self.export.notes.push(text.clone());
        self.messages.notify(sev, text);
    }

    /// Note FC2: the half-size question's rows: each texture the last export flagged (`CHK-LOW-DETAIL`) that the plan
    /// still writes in full. One no longer eligible (resized, already halved, gone) drops out (Review Focus 2). Only
    /// while Low detail and its "Measure whether full size adds anything" switch are on. Cached like
    /// `export_files`: Review and the header ask every frame.
    pub fn half_size_rows(&self) -> Arc<Vec<HalfSizeRow>> {
        let c = self.settings.checks;
        if !(c.low_detail && c.drop_top_mip) {
            return Arc::default();
        }
        let key = self.view_key();
        let inner = super::bottom::problems_inner(self);
        {
            let c = self.views.borrow();
            if let Some((k, rows)) = c
                .half_size
                .as_ref()
                .filter(|_| c.key.as_ref() == Some(&key))
            {
                if *k == inner {
                    return rows.clone();
                }
            }
        }
        let flagged: Vec<(&str, &str)> = self
            .export
            .rows
            .iter()
            .filter_map(|r| {
                let w = r
                    .warnings
                    .iter()
                    .find(|w| w.code == engine::codes::Code::ChkLowDetail)?;
                Some((r.job_key.as_str(), w.message.as_str()))
            })
            .collect();
        let mut rows = vec![];
        if !flagged.is_empty() {
            let by_job: std::collections::HashMap<&str, &super::TextureItem> =
                self.items.iter().map(|i| (i.job_key.as_str(), i)).collect();
            for (job, detail) in flagged {
                let Some(i) = by_job.get(job) else { continue };
                // Finding r1 #1: a source changed since that export was not measured; its row drops out.
                let same_source = |j: &engine::policy::ResolvedJob| {
                    !i.changed_on_disk && self.export.sources.get(job) == Some(&j.source)
                };
                if i.job.as_ref().is_some_and(|j| {
                    same_source(j) && engine::checks::low_detail_profile_eligible(&j.profile)
                }) {
                    rows.push(HalfSizeRow {
                        key: i.key.clone(),
                        name: i.name.clone(),
                        detail: detail.to_string(),
                    });
                }
            }
            rows.sort_by(|a, b| a.key.cmp(&b.key));
        }
        let rows = Arc::new(rows);
        let mut c = self.views.borrow_mut();
        if c.key.as_ref() != Some(&key) {
            *c = super::ViewCache {
                key: Some(key),
                ..Default::default()
            };
        }
        c.half_size = Some((inner, rows.clone()));
        rows
    }

    /// Note FC2: the half-size rows Review has no answer for (all of them while Review is closed).
    pub fn unanswered_half_size(&self) -> usize {
        let answers = self.review.as_ref().map(|r| &r.half_size);
        self.half_size_rows()
            .iter()
            .filter(|r| !answers.is_some_and(|a| a.get(&r.key).is_some_and(Option::is_some)))
            .count()
    }

    /// Note FC2: answer one half-size row. No-op while Review is closed; Half is refused while it cannot be saved
    /// (`half_unwritable`), Full never is.
    pub fn answer_half_size(&mut self, key: &str, answer: HalfAnswer) {
        if let Some(r) = self.review.as_mut() {
            if answer == HalfAnswer::Half && r.half_unwritable.is_some() {
                return;
            }
            r.half_size.insert(key.to_string(), Some(answer));
        }
    }

    /// Note FC2: Review's Export writes each `Half` answer whose row is still asked as `drop_top_mip = true`, one
    /// undo step, on the settled plan with the sources checked (`Continue::Review`). A `Full` answer writes nothing.
    /// True when it wrote.
    pub(super) fn write_half_answers(&mut self) -> bool {
        let Some(r) = self.review.as_ref() else {
            return false;
        };
        if r.half_unwritable.is_some() || !self.toml_writable() {
            return false;
        }
        let keys: Vec<String> = self
            .half_size_rows()
            .iter()
            .filter(|row| r.half_size.get(&row.key) == Some(&Some(HalfAnswer::Half)))
            .map(|row| row.key.clone())
            .collect();
        if keys.is_empty() {
            return false;
        }
        let pushes = self.history.pushes;
        self.edit("Half size", move |p| {
            for k in &keys {
                p.set_file_value(k, "drop_top_mip", &serde_json::json!(true));
            }
        });
        if self.history.pushes == pushes {
            return false;
        }
        let step = (self.history.undo.len(), self.history.pushes);
        if let Some(r) = self.review.as_mut() {
            r.half_step = Some(step);
        }
        true
    }

    /// Note FB1: Review would write without skipping anything: a finished plan with files to write, the gate open and
    /// nothing undecided (A12 withheld "ready" from an empty or still-planning project too).
    pub fn export_is_clean(&self) -> bool {
        self.planning.is_none()
            && self.votes_pending.is_none()
            && self.export_files() > 0
            && self.review_gate().is_ok()
            && self.undecided() == 0
            // Note FC5: NIF edits are written by Review's Export only. Note FC14: a blocked one is never written.
            && !self.nif_rows_written()
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
        // Note GH8: no project, no texture run: the button writes the NIF edits alone.
        if self.root.is_none() {
            let n = self.nif_rows.iter().filter(|r| r.blocked.is_none()).count();
            return format!("Export {n} NIF edit{}", plural(n));
        }
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
        // Note GH8: Review's Export also works with no project (it writes the NIF edits alone).
        if self.review.is_none() {
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
