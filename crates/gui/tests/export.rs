mod support;
use engine::codes::{Code, Warning};
use engine::config::{AlphaPolicy, GreenConvention, Subtype};
use engine::events::{EngineEvent, EventKind, SkipKind, EVENT_SCHEMA};
use sdc_gui::model::export::*;
use sdc_gui::model::*;
use sdc_gui::settings::Bc1Choice;
use std::path::Path;
use std::time::Duration;
use support::*;

fn decide_all(m: &mut AppModel) {
    m.decide("landscape/rock01.png", Answer::Subtype(Subtype::Diffuse));
    m.decide("landscape/odd.png", Answer::Subtype(Subtype::Diffuse));
    m.decide("landscape/odd.png", Answer::Acknowledge(Code::DimNpot));
    m.decide("landscape/leaf.png", Answer::Subtype(Subtype::Diffuse));
    m.decide("landscape/leaf.png", Answer::Alpha(AlphaPolicy::Cutout));
    m.decide(
        "landscape/rock01_normal.png",
        Answer::Subtype(Subtype::Normal),
    );
    m.decide(
        "landscape/rock01_normal.png",
        Answer::Green(GreenConvention::Directx),
    );
    assert!(m.wait_idle(Duration::from_secs(60)));
}

#[test]
fn undecided_textures_ask_first_and_proceed_anyway_skips_them() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let n = m.undecided();
    assert!(n > 0);
    m.export_clicked();
    assert_eq!(m.dialog, Some(Dialog::ProceedAnyway(m.undecided_keys())));
    assert_eq!(m.undecided_keys().len(), n);
    assert!(m.export.running.is_none(), "nothing runs before the answer");
    m.proceed_anyway();
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
    assert!(m.export.open);
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
        r#"sdc convert "C:/mods/x/textures" --recursive -o "D:/out dir" --bc1-decoder nvidia"#
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
