mod support;
use engine::codes::{Code, Warning};
use engine::events::{EngineEvent, EventKind, SkipKind, EVENT_SCHEMA};
use sdc_gui::model::export::*;
use sdc_gui::model::*;
use sdc_gui::settings::Bc1Choice;
use std::path::Path;
use std::time::Duration;
use support::*;

#[test]
fn undecided_textures_open_review_first_and_its_export_skips_them() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let n = m.undecided();
    assert!(n > 0);
    m.export_clicked();
    assert!(m.review.is_some() && m.dialog.is_none(), "{:?}", m.dialog);
    assert_eq!(m.undecided_keys().len(), n);
    assert!(m.export.running.is_none(), "nothing runs before the answer");
    m.export_from_review();
    assert!(m.wait_idle(Duration::from_secs(120)));
    let s = m.export.summary.clone().unwrap();
    assert!(
        s.converted >= 1,
        "the packed _rmaos is decided and converts"
    );
    let skipped: Vec<&Row> = m
        .export
        .rows
        .iter()
        .filter(|r| {
            matches!(
                r.state,
                RowState::Skipped(SkipKind::Unresolved | SkipKind::Blocked, _)
            )
        })
        .collect();
    assert_eq!(skipped.len(), n);
    assert!(d.path().join("export/landscape/rock01_rmaos.dds").exists());
    assert!(!d.path().join("export/landscape/leaf.dds").exists());
}

#[test]
fn a_decided_folder_exports_and_the_report_groups_warnings() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    decide_all(&mut m);
    m.export_clicked();
    assert_eq!(m.dialog, None);
    assert!(m.bottom.open && m.bottom.tab == sdc_gui::model::bottom::BottomTab::Files);
    assert!(m.wait_idle(Duration::from_secs(120)));
    let s = m.export.summary.clone().unwrap();
    assert_eq!(s.failed, 0);
    assert!(
        m.root.clone().unwrap().join("sdc.toml").exists(),
        "export saves first"
    );
    assert!(m.export.rows.iter().all(|r| !r.job_key.is_empty()));
    assert!(
        m.export.rows.iter().all(|r| r.state != RowState::Queued),
        "no row is left queued"
    );
    let codes = m.export.by_code();
    assert!(codes
        .get(&Code::DimNpot)
        .is_some_and(|keys| keys.contains(&"landscape/odd.dds".to_string())));
    let md = m.export.to_markdown();
    assert!(md.contains("DIM-NPOT") && md.contains("landscape/odd.dds"));
    assert_eq!(m.export.to_jsonl().lines().count(), m.export.events.len());
    m.select_job("landscape/odd.dds");
    assert_eq!(m.selection, Selection::Item("landscape/odd.png".into()));
}

#[test]
fn export_refuses_the_input_folder_or_an_ancestor() {
    let root = Path::new("C:/mods/x/textures");
    assert!(export_folder_problem(root, root).is_some());
    assert!(export_folder_problem(root, Path::new("C:/mods/x")).is_some());
    assert!(
        export_folder_problem(root, Path::new("C:/mods/X/Textures")).is_some(),
        "case-insensitive"
    );
    assert!(export_folder_problem(root, Path::new("C:/mods/x/textures/sdc-out")).is_none());
    assert!(export_folder_problem(root, Path::new("D:/out")).is_none());
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.settings.export_folder = m.root.clone();
    m.export_clicked();
    assert!(matches!(m.dialog, Some(Dialog::Error(ref e)) if e.contains("export folder")));
    assert!(m.export.running.is_none());
}

#[test]
fn export_stops_when_the_save_fails() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    decide_all(&mut m);
    // A directory where sdc.toml should be makes the save fail.
    let toml = m.root.clone().unwrap().join("sdc.toml");
    std::fs::create_dir(&toml).unwrap();
    m.export_clicked();
    assert!(matches!(
        m.dialog,
        Some(Dialog::Error(_)) | Some(Dialog::ExternalChange(_))
    ));
    assert!(m.export.running.is_none());
    assert!(m.export.summary.is_none());
    assert!(m.is_dirty());
}

#[test]
fn the_cli_command_reproduces_the_export() {
    let c = cli_command(
        Path::new("C:/mods/x/textures"),
        Path::new("D:/out dir"),
        Bc1Choice::Nvidia,
        0,
    );
    assert_eq!(
        c,
        r#"sdc convert "C:/mods/x/textures" --recursive -o "D:/out dir" --accept-suggestions high --bc1-decoder nvidia"#
    );
    let c = cli_command(Path::new("C:/a"), Path::new("C:/b"), Bc1Choice::Ideal, 6);
    assert!(c.ends_with("--bc1-decoder ideal --jobs 6"));
}

fn ev(seq: u64, job: &str, path: &str, kind: EventKind) -> EngineEvent {
    EngineEvent {
        schema: EVENT_SCHEMA,
        seq,
        ts: "2026-10-10T00:00:00.000Z".into(),
        batch: "b".into(),
        set: None,
        path: Some(path.into()),
        job: Some(job.into()),
        kind,
    }
}

#[test]
fn a_later_run_overrides_an_earlier_pack_conflict_skip_of_the_same_job() {
    let mut x = ExportModel::default();
    let skip = EventKind::FileSkipped {
        kind: SkipKind::PackConflict,
        code: Some(Code::PackConflict),
        reason: "raw replaces rock_rmaos.png".into(),
    };
    x.on_event(ev(0, "a/rock_rmaos.dds", "a/rock_rmaos.png", skip));
    assert!(matches!(
        x.rows[0].state,
        RowState::Skipped(SkipKind::PackConflict, _)
    ));
    x.on_event(ev(
        1,
        "a/rock_rmaos.dds",
        "a/rock_roughness.png",
        EventKind::FileStarted,
    ));
    assert_eq!(x.rows.len(), 1, "one row per job key");
    assert_eq!(x.rows[0].state, RowState::Running);
    x.on_event(ev(
        2,
        "a/rock_rmaos.dds",
        "a/rock_roughness.png",
        EventKind::FileFailed {
            stage: engine::events::Stage::Write,
            error: "disk full".into(),
        },
    ));
    assert_eq!(x.rows[0].state, RowState::Failed("disk full".into()));
    assert_eq!(x.events.len(), 3);
}

#[test]
fn a_skip_after_a_run_does_not_undo_it() {
    let mut x = ExportModel::default();
    x.on_event(ev(0, "a/b.dds", "a/b.png", EventKind::FileStarted));
    let skip = EventKind::FileSkipped {
        kind: SkipKind::PackConflict,
        code: Some(Code::PackConflict),
        reason: "late".into(),
    };
    x.on_event(ev(1, "a/b.dds", "a/b.png", skip));
    assert_eq!(x.rows[0].state, RowState::Running);
}

#[test]
fn packed_source_skips_make_no_row_and_cancelled_inputs_mark_their_rows() {
    let mut x = ExportModel::default();
    let packed = EventKind::FileSkipped {
        kind: SkipKind::PackedSource,
        code: None,
        reason: "packed into a/rock_rmaos.dds".into(),
    };
    x.on_event(ev(0, "a/rock_rmaos.dds", "a/rock_roughness.png", packed));
    assert!(x.rows.is_empty());
    x.on_event(ev(
        1,
        "a/b.dds",
        "C:/in/a/b.png",
        EventKind::FileDiscovered {
            output: "a/b.dds".into(),
        },
    ));
    x.on_event(ev(
        2,
        "a/b.dds",
        "C:/in/a/b.png",
        EventKind::Warning(Warning::new(Code::DimNpot, "npot")),
    ));
    assert_eq!(x.rows[0].warnings.len(), 1);
    let mut done = ev(
        3,
        "",
        "",
        EventKind::BatchDone {
            converted: 0,
            up_to_date: 0,
            skipped: 0,
            skipped_intentional: 0,
            failed: 0,
            validation_failed: 0,
            cancelled: vec!["C:/in/a/b.png".into()],
        },
    );
    done.job = None;
    done.path = None;
    x.on_event(done);
    assert_eq!(x.rows[0].state, RowState::Cancelled);
}

#[test]
fn the_report_is_written_as_json_lines_or_markdown_by_extension() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    decide_all(&mut m);
    m.export_clicked();
    assert!(m.wait_idle(Duration::from_secs(120)));
    let jsonl = d.path().join("r.jsonl");
    let md = d.path().join("r.MD");
    sdc_gui::ui::drawer::write_report(&m, &jsonl).unwrap();
    sdc_gui::ui::drawer::write_report(&m, &md).unwrap();
    assert_eq!(
        std::fs::read_to_string(&jsonl).unwrap(),
        m.export.to_jsonl()
    );
    assert_eq!(
        std::fs::read_to_string(&md).unwrap(),
        m.export.to_markdown()
    );
}

#[test]
fn the_copied_cli_command_uses_the_sessions_decoder_not_the_pending_setting() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    // The shared test session runs the ideal model; the setting changes for the next launch only.
    m.set_bc1_choice(Bc1Choice::Nvidia);
    assert!(m.restart_needed);
    assert_eq!(m.session_bc1(), Bc1Choice::Ideal);
    let c = cli_command(
        m.root.as_ref().unwrap(),
        &m.export_root(),
        m.session_bc1(),
        m.settings.jobs,
    );
    assert!(c.contains("--bc1-decoder ideal"), "{c}");
}

/// Audit left-out (DECISIONS Note Y7): rows are found by key; a packed-source drop keeps the index right.
#[test]
fn rows_stay_keyed_after_a_packed_source_drop() {
    let mut x = ExportModel::default();
    let found = |o: &str| EventKind::FileDiscovered { output: o.into() };
    x.on_event(ev(0, "a/r.dds", "a/r_roughness.png", found("a/r.dds")));
    x.on_event(ev(1, "a/b.dds", "a/b.png", found("a/b.dds")));
    x.on_event(ev(2, "a/c.dds", "a/c.png", found("a/c.dds")));
    let packed = EventKind::FileSkipped {
        kind: SkipKind::PackedSource,
        code: None,
        reason: "packed".into(),
    };
    x.on_event(ev(3, "", "a/r_roughness.png", packed));
    assert_eq!(x.rows.len(), 2);
    x.on_event(ev(4, "a/c.dds", "a/c.png", EventKind::FileStarted));
    let state = |k: &str| {
        x.rows
            .iter()
            .find(|r| r.job_key == k)
            .unwrap()
            .state
            .clone()
    };
    assert_eq!(state("a/c.dds"), RowState::Running);
    assert_eq!(state("a/b.dds"), RowState::Queued);
    assert_eq!(x.rows.len(), 2, "no duplicate row from a stale index");
}

/// Audit 08c-4: the export notes (JSON not written, kept files, copies) are in the Markdown report.
#[test]
fn the_markdown_report_lists_the_export_notes() {
    let mut x = ExportModel::default();
    x.notes = vec!["PGPatcher JSON not written: the export failed".into()];
    let md = x.to_markdown();
    assert!(
        md.contains("## Notes\n\n- PGPatcher JSON not written: the export failed\n"),
        "{md}"
    );
}

/// Audit 08f-1: an export queued behind a plan does not start while the Open-unsaved question is up; the user's
/// answer would otherwise open the other folder in the middle of the export and drop its JSON.
#[test]
fn a_queued_export_waits_out_an_open_dialog() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.set_json_value(
        "landscape/rock01",
        "specular_level",
        Some(serde_json::json!(0.05)),
    );
    m.replan();
    m.export_clicked();
    assert!(m.export_queued());
    let other = d.path().join("other");
    std::fs::create_dir_all(&other).unwrap();
    assert!(!m.request_open(other.clone()));
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(
        m.export.running.is_none() && m.export.rows.is_empty(),
        "the export started"
    );
    assert_eq!(m.dialog, Some(Dialog::OpenUnsaved(other)));
    assert!(m.status_line.contains("Export"), "{}", m.status_line);
}

/// Audit 08f-1: answering Open-unsaved while an export runs asks to stop the export first, as Open does.
#[test]
fn answering_open_unsaved_during_an_export_waits_for_it() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.set_json_value(
        "landscape/rock01",
        "specular_level",
        Some(serde_json::json!(0.05)),
    );
    export_through(&mut m);
    assert!(m.export.running.is_some(), "{:?}", m.dialog);
    let root = m.root.clone();
    let other = d.path().join("other");
    std::fs::create_dir_all(&other).unwrap();
    m.resolve_open_unsaved(other.clone(), false);
    assert_eq!(m.root, root, "not opened during the export");
    assert_eq!(
        m.dialog,
        Some(Dialog::ExportRunning(AfterExport::Open(other)))
    );
    m.cancel_export();
    assert!(m.wait_idle(Duration::from_secs(120)));
}

/// Re-check C3: an artist-check warning never opens Review and never reaches the blocks or asks.
#[test]
fn export_never_asks_about_check_warnings() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    decide_all(&mut m);
    m.poll();
    let c: usize = m
        .items
        .iter()
        .map(|i| m.check_warnings(&i.key).len())
        .sum::<usize>();
    assert!(
        c > 0,
        "the fixture raises a check result (flat alpha-less normal)"
    );
    for i in sdc_gui::model::pbr_export::export_blocks(&m)
        .iter()
        .chain(sdc_gui::model::pbr_export::export_asks(&m).iter())
    {
        assert!(!format!("{i:?}").contains("CHK-"), "{i:?}");
    }
    m.export_clicked();
    assert!(m.review.is_none(), "{:?}", m.review.is_some());
    assert!(m.export.running.is_some());
    assert!(m.wait_idle(Duration::from_secs(120)));
    assert!(m.export.summary.is_some());
}

// ---- the end-of-export checks summary: when it is logged (CL19) ----

const SUMMARY: &str = "Checks: 1 warning on 1 texture; see Problems";

/// A decided folder whose only check result is the fixture's plastic `_n`.
fn with_one_check() -> (tempfile::TempDir, AppModel) {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    decide_all(&mut m);
    m.poll();
    assert_eq!(
        sdc_gui::model::checks::checks_summary(&m).as_deref(),
        Some(SUMMARY)
    );
    (d, m)
}

fn summary_lines(m: &AppModel) -> usize {
    m.messages
        .entries()
        .filter(|e| e.text.starts_with("Checks:"))
        .count()
}

#[test]
fn a_completed_export_logs_the_summary_once() {
    let (_d, mut m) = with_one_check();
    m.end_export_with(Ok(Default::default()));
    assert_eq!(summary_lines(&m), 1);
}

#[test]
fn a_cancelled_export_logs_no_summary() {
    let (_d, mut m) = with_one_check();
    let s = engine::api::BatchSummary {
        cancelled: vec!["a.png".into()],
        ..Default::default()
    };
    m.end_export_with(Ok(s));
    assert_eq!(summary_lines(&m), 0);
}

#[test]
fn a_failed_export_logs_no_summary() {
    let (_d, mut m) = with_one_check();
    m.end_export_with(Err("disk full".into()));
    assert_eq!(summary_lines(&m), 0);
}

#[test]
fn an_export_that_ends_under_an_open_dialog_still_logs_the_summary_and_keeps_the_dialog() {
    let (_d, mut m) = with_one_check();
    m.dialog = Some(Dialog::ReloadUnsaved);
    m.end_export_with(Ok(Default::default()));
    assert_eq!(m.dialog, Some(Dialog::ReloadUnsaved));
    assert_eq!(summary_lines(&m), 1);
}
