//! Track A (DECISIONS Note CM): "Also export as" Vanilla and Complex Material in the GUI model (Task 11, C1 under
//! layout v2). The ticks and settings are `[export]` in `sdc.toml` (TA9), the location is `[project] also_location`
//! in `sdc-project.toml` (TA10). Review opens whenever a tick is on, and its Export is the only way out (CA2).
use super::{auto_flags, checks, messages, sets, AppModel};
use crate::project::ProjectDoc;
use engine::also::{AlsoSpec, AlsoSummary, Phase};
use engine::config::Target;
use engine::presets::{AlsoTarget, ExportConfig};
use std::path::{Path, PathBuf};

/// TA20, the engine's refusal text: a project in the vanilla mode has no PBR folder to derive from.
pub const NEEDS_CS: &str = "Also export needs the cs mode: the PBR folder is the cs export";
/// Review Focus 5 (P9): the gate's reason while `sdc.toml` has the read-only flag and a tick is on.
pub const TOML_READ_ONLY: &str = "sdc.toml is read-only: Also export settings could not be saved";

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
        // A switch mid-export would move the JSON records under the finishers (re-check A4).
        if self.export.running.is_some() {
            self.status_line = "Wait for the export to finish".into();
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

    /// TA10: `[project] also_location`, one undo step (with the `sdc-project.toml` text), planned again.
    pub fn set_also_location(&mut self, p: &Path) {
        if self.export.running.is_some() {
            self.status_line = "Wait for the export to finish".into();
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
    fn also_project(&self) -> String {
        self.root
            .as_ref()
            .map(|r| std::path::absolute(r).unwrap_or_else(|_| r.clone()))
            .and_then(|r| r.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default()
    }

    /// The PBR, Vanilla and CM folders: the location joined with `[export.names]` (default `<project> PBR`, …).
    pub fn also_folders(&self) -> [PathBuf; 3] {
        let loc = self.also_location();
        self.also_config()
            .folder_names(&self.also_project())
            .map(|n| loc.join(n))
    }

    /// The PBR folder whenever a tick is on (not "while running": the finishers run after `running` is cleared).
    pub fn also_root(&self) -> Option<PathBuf> {
        self.also_on().then(|| self.also_folders()[0].clone())
    }

    /// The folder the plan and the PBR outputs are under: the PBR folder while a tick is on, else the export folder.
    pub fn output_root(&self) -> PathBuf {
        self.also_root().unwrap_or_else(|| self.export_root())
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
    pub(super) fn also_gate(&self) -> Result<(), String> {
        if let Some(why) = self.review.as_ref().and_then(|r| r.toml_unwritable.clone()) {
            return Err(why);
        }
        if self.target != Target::Cs {
            return Err(NEEDS_CS.into());
        }
        let root = self.root.clone().unwrap_or_default();
        engine::also::check_location(&[root], &self.also_location(), &self.also_folders())?;
        self.also_config().check()
    }

    /// "to {k} folders": 3 with CM (it needs Vanilla, TA30), else 2.
    pub(super) fn also_folder_count(&self) -> usize {
        if self.also_ticks().1 {
            3
        } else {
            2
        }
    }

    /// The worker's `AlsoFinished`: the summary becomes notes, never a dialog. A phase error is an error (CM56: a
    /// PBR-run failure is `Ok` from `run_also`), not a success.
    pub(super) fn also_finished(&mut self, s: AlsoSummary) {
        self.export.notes.extend(s.notes.iter().cloned());
        if s.copied > 0 {
            self.export.notes.push(format!(
                "{} Vanilla file{} copied from the PBR folder",
                s.copied,
                if s.copied == 1 { "" } else { "s" }
            ));
        }
        for z in &s.zips {
            self.export
                .notes
                .push(format!("Zip written: {}", z.display()));
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
            self.messages.notify(messages::Severity::Error, t.clone());
            self.export.notes.push(t.clone());
            self.also_error = Some(t);
        }
        if let Some(r) = &s.report {
            let t = format!("Conversion report: {}", r.display());
            self.messages.notify(messages::Severity::Info, t.clone());
            self.export.notes.push(t);
        }
        self.also_written = Some(s.folders);
    }
}

#[cfg(test)]
mod tests {
    use super::super::export::RowState;
    use super::super::tests::{bare_model, panicked};
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

    /// A plain export keeps today's behaviour: no sweep.
    #[test]
    fn a_plain_export_error_keeps_its_rows_as_today() {
        let mut m = bare_model();
        m.export.running = Some(Default::default());
        discovered(&mut m, "a.dds");
        m.end_export_with(Err("x".into()));
        assert_eq!(m.export.rows[0].state, RowState::Queued);
    }

    /// CM56 carried: a later phase's error in `AlsoSummary::error` is an error, and the export's end is logged as
    /// one, not as a success; the PBR folder's finishers still run (the summary is `Ok`).
    #[test]
    fn a_later_phase_error_is_shown_as_an_error() {
        use super::super::messages::Severity;
        let mut m = bare_model();
        m.also_pass = true;
        m.export.running = Some(Default::default());
        discovered(&mut m, "textures Vanilla/a.dds");
        m.also_finished(engine::also::AlsoSummary {
            phases: vec![],
            copied: 0,
            sets: vec![],
            folders: Default::default(),
            report: None,
            zips: vec![],
            notes: vec![],
            cancelled_after: None,
            error: Some((engine::also::Phase::Vanilla, "plan broke".into(), 3)),
            zip_failed: false,
            report_failed: false,
        });
        m.end_export_with(Ok(Default::default()));
        let last = m.messages.entries().next_back().unwrap();
        assert_eq!(last.severity, Severity::Error, "{}", last.text);
        assert!(m.messages.entries().any(|e| e
            .text
            .contains("the Vanilla phase failed (exit 3): plan broke")));
        assert!(matches!(&m.export.rows[0].state, RowState::Failed(e) if e.contains("plan broke")));
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
