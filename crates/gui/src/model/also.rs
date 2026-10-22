//! Track A (DECISIONS Note CN): "Also export as" Vanilla and Complex Material in the GUI model (Task 11, C1 under
//! layout v2). The ticks and settings are `[export]` in `sdc.toml` (TA9), the location is `[project] also_location`
//! in `sdc-project.toml` (TA10). Review opens whenever a tick is on, and its Export is the only way out (CA2).
use super::{auto_flags, checks, messages, sets, AppModel};
use crate::project::ProjectDoc;
use engine::also::{AlsoSpec, AlsoSummary, Phase};
use engine::config::Target;
use engine::presets::{AlsoTarget, ExportConfig};
use engine::shader::Shader;
use std::path::{Path, PathBuf};

/// TA20, the engine's refusal text: a project in the vanilla mode has no PBR folder to derive from.
pub const NEEDS_CS: &str = "Also export needs the cs mode: the PBR folder is the cs export";
/// Review Focus 5 (P9): the gate's reason while `sdc.toml` has the read-only flag and a tick is on.
pub const TOML_READ_ONLY: &str = "sdc.toml is read-only: Also export settings could not be saved";
/// Every Also edit is refused while an export runs; the status line, and the hover of the disabled controls.
pub const EXPORT_RUNNING: &str = "Wait for the export to finish";

/// TA11: a set Vanilla and CM derive from, by its PBR-phase shader pick (as `engine::also::derive_sets` reads it).
pub fn is_pbr_set(v: &sets::SetView) -> bool {
    v.shader.as_ref().is_some_and(|p| {
        matches!(
            p.shader,
            Shader::Pbr | Shader::PbrLandscape | Shader::PbrGrass
        )
    })
}

fn phase_name(p: Phase) -> &'static str {
    match p {
        Phase::Pbr => "PBR",
        Phase::Vanilla => "Vanilla",
        Phase::Cm => "CM",
    }
}

impl AppModel {
    /// `[export]` of the project (the default without a usable `sdc.toml`).
    pub fn also_config(&self) -> ExportConfig {
        self.project
            .as_ref()
            .filter(|p| !p.read_only())
            .map(|p| p.export_config())
            .unwrap_or_default()
    }

    /// (Vanilla, CM) from `[export] also`.
    pub fn also_ticks(&self) -> (bool, bool) {
        let c = self.also_config();
        (
            c.also.contains(&AlsoTarget::Vanilla),
            c.also.contains(&AlsoTarget::Cm),
        )
    }

    /// Either tick is on.
    pub fn also_on(&self) -> bool {
        let (v, c) = self.also_ticks();
        v || c
    }

    /// One `[export]` edit, one undo step, planned again (the PBR folder is the plan's output root, TA10).
    fn edit_export(&mut self, label: &str, f: impl FnOnce(&mut ProjectDoc)) {
        // So the plan on screen and the NIF root match the running export (the finishers use roots captured at its
        // start). Undo and Redo are not guarded: harmless for the same reason (review M4).
        if self.export.running.is_some() {
            self.status_line = EXPORT_RUNNING.into();
            return;
        }
        self.edit(label, f);
    }

    fn edit_config(&mut self, label: &str, f: impl FnOnce(&mut ExportConfig)) {
        let mut c = self.also_config();
        f(&mut c);
        self.edit_export(label, move |p| p.set_export_config(&c));
    }

    /// Tick or untick "Also export as" Vanilla or CM. Ticking is refused (no edit) outside the cs mode, with TA20's
    /// reason; unticking always works, so a tick left from the cs mode can be cleared.
    pub fn set_also_tick(&mut self, t: AlsoTarget, on: bool) {
        if on && self.target != Target::Cs {
            self.status_line = NEEDS_CS.into();
            return;
        }
        self.edit_config("Also export as", |c| {
            let was = c.also.clone();
            c.also = [AlsoTarget::Vanilla, AlsoTarget::Cm]
                .into_iter()
                .filter(|x| if *x == t { on } else { was.contains(x) })
                .collect();
        });
    }

    /// The names, Keep metal brightness and its strength, the zip tick and the `[export.legacy]` recipe values.
    pub fn set_also_settings(&mut self, f: impl FnOnce(&mut ExportConfig)) {
        self.edit_config("Also export settings", f);
    }

    /// C2: leave a set out of Vanilla and CM (TA9 `[export] skip`), one undo step.
    pub fn set_skip_also(&mut self, set_id: &str, on: bool) {
        let id = set_id.to_string();
        self.edit_export("Skip vanilla/CM for this set", move |p| {
            p.set_skip_also(&id, on)
        });
    }

    /// C2: set `id` is in `[export] skip`.
    pub fn also_skipped(&self, id: &str) -> bool {
        self.also_config()
            .skip
            .iter()
            .any(|s| engine::sets::same_id(s, id))
    }

    /// TA10: `[project] also_location`, one undo step (with the `sdc-project.toml` text), planned again.
    pub fn set_also_location(&mut self, p: &Path) {
        if self.export.running.is_some() {
            self.status_line = EXPORT_RUNNING.into();
            return;
        }
        let p = p.to_path_buf();
        self.edit_docs("Also export location", None, true, move |_, s| {
            s.set_also_location(&p)
        });
    }

    /// TA10: where the three folders go: `[project] also_location`, else the export folder.
    pub fn also_location(&self) -> PathBuf {
        self.sets_doc
            .as_ref()
            .and_then(|d| d.also_location())
            .unwrap_or_else(|| self.export_root())
    }

    /// The project name the engine names the folders by: the opened folder's name (`also::project_name`).
    fn also_project(&self) -> Result<String, String> {
        let root = self.root.clone().unwrap_or_default();
        engine::also::project_name(&[root]).map_err(|e| e.to_string())
    }

    /// The PBR, Vanilla and CM folders: the location joined with `[export.names]` (default `<project> PBR`, …).
    pub fn also_folders(&self) -> [PathBuf; 3] {
        let loc = self.also_location();
        self.also_config()
            .folder_names(&self.also_project().unwrap_or_default())
            .map(|n| loc.join(n))
    }

    /// The PBR folder whenever a tick is on (not "while running": the finishers run after `running` is cleared).
    /// Review M1: only in the cs mode, so a tick left from it never sends the plan or the NIF edits into a PBR folder
    /// no export can fill (Review names TA20 instead).
    pub fn also_root(&self) -> Option<PathBuf> {
        (self.also_on() && self.target == Target::Cs).then(|| self.also_folders()[0].clone())
    }

    /// The folder the plan and the PBR outputs are under: the PBR folder while a tick is on, else the export folder.
    pub fn output_root(&self) -> PathBuf {
        self.also_root().unwrap_or_else(|| self.export_root())
    }

    /// Note CI29 / P16: where NIF edits go, handed to the Meshes model every frame by `app.rs`: `output_root()` when
    /// the Textures side has an export folder (a setting or an open project), else `None` (the CI3 fallback).
    pub fn nif_export_root(&self) -> Option<PathBuf> {
        (self.settings.export_folder.is_some() || self.root.is_some()).then(|| self.output_root())
    }

    /// What "Copy CLI command" passes as `-o` (P14): the location while a tick is on (the CLI reads `[export]`).
    pub fn cli_output(&self) -> PathBuf {
        if self.also_on() {
            self.also_location()
        } else {
            self.export_root()
        }
    }

    /// Re-check A5: the file a Problems `At::File` key names when it is a Vanilla or CM row of the last Also export
    /// (`"<folder name>/…"`); `None` for any other key, which selects its texture as before.
    pub fn also_file(&self, key: &str) -> Option<PathBuf> {
        let folders = self.also_written.as_ref()?;
        folders[1..].iter().find_map(|f| {
            let name = f.file_name()?.to_string_lossy();
            let rest = key
                .get(..name.len() + 1)
                .filter(|h| h.eq_ignore_ascii_case(&format!("{name}/")))
                .map(|_| &key[name.len() + 1..])?;
            Some(f.join(rest))
        })
    }

    /// The three folders the last Also export wrote (from its summary), for as long as this folder is open.
    pub fn also_folders_written(&self) -> Option<&[PathBuf; 3]> {
        self.also_written.as_ref()
    }

    /// Task 13 M3: a PBR grass set is not skipped, so the CM folder gets its complex-grass stack (Review's CM text).
    pub fn also_grass(&self) -> bool {
        let skip = self.also_config().skip;
        sets::views(self).iter().any(|v| {
            v.shader
                .as_ref()
                .is_some_and(|p| p.shader == Shader::PbrGrass)
                && !skip.iter().any(|s| engine::sets::same_id(s, &v.id))
        })
    }

    /// What `run_also` needs besides the request: the config, the location, TA13's `scanned` and TA27's sets.
    pub fn also_spec(&self) -> AlsoSpec {
        // P21: Track C's test for a 1.0-neutral `_p` (PBR multilayer with coat parallax).
        let inner_layer_sets = sets::views(self)
            .iter()
            .filter(|v| !v.key.is_empty())
            .filter(|v| {
                let shader = v.shader_choice.or(v.shader.as_ref().map(|p| p.shader));
                let resolved = auto_flags::resolved(v.pbr, &v.slots, &v.settings);
                checks::height_neutral(shader, &resolved) == checks::HeightNeutral::White
            })
            .map(|v| v.id.clone())
            .collect();
        AlsoSpec {
            cfg: self.also_config(),
            location: self.also_location(),
            scanned: !self.mesh_votes.is_empty(),
            inner_layer_sets,
        }
    }

    /// `sdc.toml`'s writability as Review's gate reads it, with a tick on: taken when Review opens and again before
    /// its Export runs (P9), never per frame (ui-rules 9.4).
    pub(super) fn also_toml_unwritable(&self) -> Option<String> {
        if !self.also_on() {
            return None;
        }
        self.sdc_toml_writable()
            .err()
            .map(|_| TOML_READ_ONLY.to_string())
    }

    /// Review's Also rows, in A9's order: the read-only `sdc.toml`, the cs mode (TA20), the location (TA19), the
    /// `[export]` values. The first `Err` is the button's reason.
    pub fn also_gate(&self) -> Result<(), String> {
        if let Some(why) = self.review.as_ref().and_then(|r| r.toml_unwritable.clone()) {
            return Err(why);
        }
        if self.target != Target::Cs {
            return Err(NEEDS_CS.into());
        }
        // The engine refuses an input with no folder name (a drive root) when it names the folders.
        let project = self.also_project()?;
        let root = self.root.clone().unwrap_or_default();
        engine::also::check_location(&[root], &self.also_location(), &self.also_folders())?;
        let cfg = self.also_config();
        cfg.check()?;
        // Review M3: a custom name equal to another folder's default, refused here rather than at run time.
        engine::also::check_names(&cfg, &project)
    }

    /// "to {k} folders": 3 with CM (it needs Vanilla, TA30), else 2.
    pub fn also_folder_count(&self) -> usize {
        if self.also_ticks().1 {
            3
        } else {
            2
        }
    }

    /// The worker's `AlsoFinished`: the summary becomes notes, never a dialog. A phase error is an error (CN56: a
    /// PBR-run failure is `Ok` from `run_also`), not a success.
    pub(super) fn also_finished(&mut self, s: AlsoSummary) {
        self.export.notes.extend(s.notes.iter().cloned());
        // CN99: why a PBR set got no Vanilla or CM files (left out, undecided, DDS, ...), as the report says.
        self.export
            .notes
            .extend(s.sets.iter().map(|n| format!("{}: {}", n.id, n.why)));
        if s.copied > 0 {
            self.export.notes.push(format!(
                "{} Vanilla file{} copied from the PBR folder",
                s.copied,
                if s.copied == 1 { "" } else { "s" }
            ));
        }
        if let Some(p) = s.cancelled_after {
            self.export.notes.push(format!(
                "Also export cancelled after the {} phase",
                phase_name(p)
            ));
        }
        if let Some((phase, msg, code)) = &s.error {
            let t = format!(
                "Also export: the {} phase failed (exit {code}): {msg}",
                phase_name(*phase)
            );
            // Review M2: a PBR-phase error is the export's `Err`, logged and shown once by `export_ended`.
            if *phase != Phase::Pbr {
                self.messages.notify(messages::Severity::Error, t.clone());
            }
            self.export.notes.push(t.clone());
            self.also_error = Some(t);
        }
        self.also_written = Some(s.folders);
    }

    /// Final review I2: the worker's `AlsoZipped`, after the JSON: the walk's, zips' and report's notes (only theirs),
    /// then the export ends. A late one, after a panic or a new export, is dropped.
    pub(super) fn also_zipped(&mut self, s: AlsoSummary) {
        let Some(failure) = self.also_tail.take() else {
            return;
        };
        self.export.running = None;
        self.export.notes.extend(s.notes.iter().cloned());
        for z in &s.zips {
            self.export
                .notes
                .push(format!("Zip written: {}", z.display()));
        }
        // The "Export finished" line was decided before the zips: a zip or report not written is said here.
        if s.zip_failed || s.report_failed {
            self.messages.notify(
                messages::Severity::Warning,
                "Also export: a zip, the sums file or the report was not written (see the notes)",
            );
        }
        if let Some(r) = &s.report {
            let t = format!("Conversion report: {}", r.display());
            self.messages.notify(messages::Severity::Info, t.clone());
            self.export.notes.push(t);
        }
        self.after_export(failure);
    }
}

/// Final review I2: the files of `files` (`/`-separated paths under `root`, with their text) that now hold exactly
/// that text: written by this export or found current. Their paths go to the Also export's zip step.
pub(super) fn current_files(root: &Path, files: &[(String, String)]) -> Vec<PathBuf> {
    files
        .iter()
        .map(|(rel, text)| (root.join(rel), text))
        .filter(|(p, text)| std::fs::read(p).is_ok_and(|b| b == text.as_bytes()))
        .map(|(p, _)| p)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::export::RowState;
    use super::super::tests::{bare_model, panicked};
    use engine::also::{AlsoSummary, Phase};
    use engine::events::{EngineEvent, EventKind, EVENT_SCHEMA};

    fn discovered(m: &mut super::AppModel, job: &str) {
        m.export.on_event(EngineEvent {
            schema: EVENT_SCHEMA,
            seq: 0,
            ts: String::new(),
            batch: "b".into(),
            job: Some(job.into()),
            set: None,
            path: Some(format!("in/{job}")),
            kind: EventKind::FileDiscovered {
                output: format!("out/{job}"),
            },
        });
    }

    /// Task 7 review L3: a refusal or plan error after the PBR plan emitted its rows (no `BatchDone`) leaves no row
    /// queued: each ends as failed with the reason.
    #[test]
    fn an_also_export_error_ends_every_queued_row() {
        let mut m = bare_model();
        m.also_pass = true;
        m.export.running = Some(Default::default());
        discovered(&mut m, "a.dds");
        discovered(&mut m, "textures Vanilla/a.dds");
        m.end_export_with(Err(super::NEEDS_CS.into()));
        assert!(m
            .export
            .rows
            .iter()
            .all(|r| r.state == RowState::Failed(format!("not run: {}", super::NEEDS_CS))));
    }

    /// A cancel ends the rows the engine never reached (a cancelled copy has no event) as cancelled.
    #[test]
    fn a_cancelled_also_export_ends_its_queued_rows_as_cancelled() {
        let mut m = bare_model();
        m.also_pass = true;
        let c = engine::api::CancelToken::default();
        c.cancel();
        m.export.running = Some(c);
        discovered(&mut m, "textures Vanilla/a.dds");
        m.end_export_with(Ok(Default::default()));
        assert_eq!(m.export.rows[0].state, RowState::Cancelled);
    }

    /// A summary with 2 converted in the PBR phase, 3 copies, and an error in `phase`.
    fn summary(phase: Option<Phase>) -> AlsoSummary {
        AlsoSummary {
            phases: vec![(
                Phase::Pbr,
                engine::api::BatchSummary {
                    converted: 2,
                    ..Default::default()
                },
            )],
            copied: 3,
            sets: vec![],
            folders: Default::default(),
            report: None,
            zips: vec![],
            notes: vec![],
            cancelled_after: None,
            error: phase.map(|p| (p, "plan broke".to_string(), 3)),
            zip_failed: false,
            report_failed: false,
        }
    }

    /// CN56 in the worker: a PBR-phase error is the export's `Err` (nothing written after it); a later phase's is
    /// `Ok` with the copies counted as written.
    #[test]
    fn the_worker_maps_a_pbr_phase_error_to_err() {
        use crate::worker::also_done;
        assert_eq!(
            also_done(&summary(Some(Phase::Pbr))),
            Err("plan broke".into())
        );
        for p in [None, Some(Phase::Vanilla), Some(Phase::Cm)] {
            assert_eq!(also_done(&summary(p)).map(|b| b.converted), Ok(5), "{p:?}");
        }
    }

    /// Review M2: a PBR-phase error is logged once ("Export failed", plus the dialog), as a plain export's.
    #[test]
    fn a_pbr_phase_error_is_logged_once() {
        let mut m = bare_model();
        m.also_pass = true;
        m.export.running = Some(Default::default());
        m.also_finished(summary(Some(Phase::Pbr)));
        m.end_export_with(Err("plan broke".into()));
        let n = m
            .messages
            .entries()
            .filter(|e| e.text.contains("plan broke"))
            .count();
        assert_eq!(n, 1);
        assert!(matches!(m.dialog, Some(super::super::Dialog::Error(_))));
    }

    /// A plain export keeps today's behaviour: no sweep.
    #[test]
    fn a_plain_export_error_keeps_its_rows_as_today() {
        let mut m = bare_model();
        m.export.running = Some(Default::default());
        discovered(&mut m, "a.dds");
        m.end_export_with(Err("x".into()));
        assert_eq!(m.export.rows[0].state, RowState::Queued);
    }

    /// CN56 carried: a later phase's error in `AlsoSummary::error` is an error, and the export's end is logged as
    /// one, not as a success; the PBR folder's finishers still run (the summary is `Ok`).
    #[test]
    fn a_later_phase_error_is_shown_as_an_error() {
        use super::super::messages::Severity;
        let mut m = bare_model();
        m.also_pass = true;
        m.export.running = Some(Default::default());
        discovered(&mut m, "textures Vanilla/a.dds");
        m.also_finished(summary(Some(Phase::Vanilla)));
        m.end_export_with(Ok(Default::default()));
        let last = m.messages.entries().next_back().unwrap();
        assert_eq!(last.severity, Severity::Error, "{}", last.text);
        assert!(m.messages.entries().any(|e| e
            .text
            .contains("the Vanilla phase failed (exit 3): plan broke")));
        assert!(matches!(&m.export.rows[0].state, RowState::Failed(e) if e.contains("plan broke")));
    }

    /// Re-review N2: only a file whose bytes equal this export's text goes to the zip step; a different or missing
    /// one does not.
    #[test]
    fn current_files_takes_only_files_that_hold_this_exports_text() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("sub")).unwrap();
        std::fs::write(d.path().join("sub/same.json"), "{}").unwrap();
        std::fs::write(d.path().join("other.json"), "{\"edited\":1}").unwrap();
        let files = [
            ("sub/same.json".to_string(), "{}".to_string()),
            ("other.json".to_string(), "{}".to_string()),
            ("missing.json".to_string(), "{}".to_string()),
        ];
        assert_eq!(
            super::current_files(d.path(), &files),
            [d.path().join("sub/same.json")]
        );
    }

    /// Final review I2: a panic while the zips run ends the export once; a late `AlsoZipped` is then dropped.
    #[test]
    fn a_panic_while_zipping_ends_the_export_and_a_late_zip_result_is_dropped() {
        let mut m = bare_model();
        m.also_pass = true;
        m.export.running = Some(Default::default());
        m.also_tail = Some(None);
        m.handle(panicked("export"));
        assert!(m.export.running.is_none() && m.also_tail.is_none());
        let n = m
            .messages
            .entries()
            .filter(|e| e.text.starts_with("Export failed"))
            .count();
        assert_eq!(n, 1);
        m.export.running = Some(Default::default());
        let mut s = summary(None);
        s.notes = vec!["late".into()];
        m.also_zipped(s);
        assert!(m.export.running.is_some(), "another export's token stays");
        assert!(!m.export.notes.contains(&"late".to_string()));
    }

    /// The worker tags its panic "export": an Also export's panic ends the export, so Export works again.
    #[test]
    fn an_also_export_panic_ends_the_export() {
        let mut m = bare_model();
        m.also_pass = true;
        m.export.running = Some(Default::default());
        discovered(&mut m, "textures Vanilla/a.dds");
        m.handle(panicked("export"));
        assert!(m.export.running.is_none());
        assert!(m.export.rows.iter().all(|r| r.state != RowState::Queued));
    }
}
