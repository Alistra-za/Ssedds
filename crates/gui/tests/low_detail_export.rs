//! v2 artist checks, check 3 in the GUI (Task 5): Hold mode, the half-size box, and the remembered half size
//! (DECISIONS Note CL24-).
mod support;
use eframe::egui::{self, Key};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use engine::codes::{Code, Warning};
use engine::events::{EngineEvent, EventKind, SkipKind, EVENT_SCHEMA};
use image::{ImageBuffer, Rgb, RgbImage};
use sdc_gui::model::bottom::{problems, At, Level};
use sdc_gui::model::export::{ExportModel, RowState};
use sdc_gui::model::{AppModel, Dialog};
use sdc_gui::settings::{ChecksSettings, GuiSettings};
use std::path::{Path, PathBuf};
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(120);

/// `n`² RGB white noise from a fixed LCG.
fn noise(n: u32, seed: u64) -> RgbImage {
    let mut s = seed;
    ImageBuffer::from_fn(n, n, |_, _| {
        let mut b = || {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (s >> 56) as u8
        };
        Rgb([b(), b(), b()])
    })
}

/// 512² noise upscaled 2x (nearest): mip 0 adds nothing over mip 1.
fn upscaled(seed: u64) -> RgbImage {
    let small = noise(512, seed);
    ImageBuffer::from_fn(1024, 1024, |x, y| *small.get_pixel(x / 2, y / 2))
}

/// `textures/` with `up.png`, `up2.png` (flagged) and `noise.png` (native 1024²), all diffuse, encoded ultrafast.
fn folder(dir: &Path) -> PathBuf {
    let r = dir.join("textures");
    std::fs::create_dir_all(&r).unwrap();
    noise(1024, 1).save(r.join("noise.png")).unwrap();
    upscaled(2).save(r.join("up.png")).unwrap();
    upscaled(3).save(r.join("up2.png")).unwrap();
    let mut toml = String::from("[defaults.encoder]\neffort = \"ultrafast\"\n");
    for n in ["noise.png", "up.png", "up2.png"] {
        toml += &format!("\n[files.\"{n}\"]\nsubtype = \"diffuse\"\n");
    }
    std::fs::write(r.join("sdc.toml"), toml).unwrap();
    r
}

/// The folder opened with "Drop mip 0 when it adds nothing" set to `hold`.
fn opened(dir: &Path, hold: bool) -> AppModel {
    let root = folder(dir);
    let settings = GuiSettings {
        export_folder: Some(dir.join("export")),
        checks: ChecksSettings {
            drop_top_mip: hold,
            ..ChecksSettings::default()
        },
        ..GuiSettings::default()
    };
    let mut m = AppModel::new(settings, None, support::session());
    m.open_folder(root);
    assert!(m.wait_idle(WAIT), "planning did not finish");
    m
}

fn export(m: &mut AppModel) {
    m.export_clicked();
    assert!(m.review.is_none(), "the fixture needs no Review");
    assert!(m.wait_idle(WAIT));
}

fn job(m: &AppModel, item: &str) -> String {
    m.item(item).unwrap().job_key.clone()
}

/// The written DDS of `item`: (width, height).
fn dims(m: &AppModel, item: &str) -> (u32, u32) {
    let p = m.export_root().join(job(m, item));
    let b = std::fs::read(&p).unwrap_or_else(|e| panic!("{p:?}: {e}"));
    let u = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    (u(16), u(12))
}

fn toml(m: &AppModel) -> String {
    std::fs::read_to_string(m.root.clone().unwrap().join("sdc.toml")).unwrap()
}

fn half_rows(m: &AppModel) -> Vec<sdc_gui::model::HalfRow> {
    match &m.dialog {
        Some(Dialog::HalfSize(rows)) => rows.clone(),
        d => panic!("no half-size box: {d:?}"),
    }
}

fn logged(m: &AppModel, s: &str) -> usize {
    m.messages.entries().filter(|e| e.text.contains(s)).count()
}

fn ev(seq: u64, job: &str, path: &str, kind: EventKind) -> EngineEvent {
    EngineEvent {
        schema: EVENT_SCHEMA,
        seq,
        ts: "2026-10-20T00:00:00.000Z".into(),
        batch: "b".into(),
        set: None,
        path: Some(path.into()),
        job: Some(job.into()),
        kind,
    }
}

fn low_detail_warning(w: u32, h: u32) -> Warning {
    Warning::new(Code::ChkLowDetail, "mip 0 adds little detail")
        .with_data(serde_json::json!({ "width": w, "height": h, "ratio": 0.1 }))
}

/// A held row for `job`, as the engine reports it: started, the warning, then the skip.
fn held_events(x: &mut ExportModel, job: &str, path: &str) {
    x.on_event(ev(1, job, path, EventKind::FileStarted));
    x.on_event(ev(
        2,
        job,
        path,
        EventKind::Warning(low_detail_warning(1024, 1024)),
    ));
    x.on_event(ev(
        3,
        job,
        path,
        EventKind::FileSkipped {
            kind: SkipKind::LowDetail,
            code: Some(Code::ChkLowDetail),
            reason: "held (low detail)".into(),
        },
    ));
}

fn harness(model: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_ui_state(
            |ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model),
            model,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

#[test]
fn a_low_detail_skip_replaces_a_running_row() {
    let mut x = ExportModel::default();
    held_events(&mut x, "a.dds", "a.png");
    assert!(matches!(
        x.rows[0].state,
        RowState::Skipped(SkipKind::LowDetail, _)
    ));
    assert_eq!(x.held(), vec![("a.dds".to_string(), (1024, 1024))]);
    x.on_event(ev(4, "b.dds", "b.png", EventKind::FileStarted));
    x.on_event(ev(
        5,
        "b.dds",
        "b.png",
        EventKind::FileSkipped {
            kind: SkipKind::UpToDate,
            code: None,
            reason: "up to date".into(),
        },
    ));
    assert_eq!(x.rows[1].state, RowState::UpToDate);
    assert_eq!(x.held().len(), 1);
}

#[test]
fn hold_then_export_selected_writes_ticked_at_half_and_the_rest_at_full() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path(), true);
    export(&mut m);
    let rows = half_rows(&m);
    assert_eq!(rows.len(), 2, "{rows:?}");
    for r in &rows {
        assert!(r.tick);
        assert_eq!((r.full.width, r.half.width), (1024, 512));
        assert!(r.full.file_bytes() > r.half.file_bytes());
    }
    let n_rows = m.export.rows.len();
    assert!(!m.export_root().join(job(&m, "up.png")).exists());
    m.export_half_size(&["up.png".to_string()]);
    assert!(m.wait_idle(WAIT));
    assert_eq!(m.dialog, None);
    let t = toml(&m);
    assert!(
        t.contains("[files.\"up.png\"]") && t.matches("drop_top_mip = true").count() == 1,
        "{t}"
    );
    assert_eq!(dims(&m, "up.png"), (512, 512));
    assert_eq!(dims(&m, "up2.png"), (1024, 1024));
    assert_eq!(dims(&m, "noise.png"), (1024, 1024));
    assert_eq!(m.export.rows.len(), n_rows, "rows kept (fresh = false)");
    let noise = m
        .export
        .rows
        .iter()
        .find(|r| r.job_key == job(&m, "noise.png"))
        .unwrap();
    assert!(
        matches!(noise.state, RowState::Done { .. } | RowState::UpToDate),
        "{:?}",
        noise.state
    );
    // K14: two passes, two reports; the second is labelled.
    assert_eq!(logged(&m, "Held textures: Export finished"), 1);
}

#[test]
fn closing_the_box_exports_all_held_at_full_size() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path(), true);
    export(&mut m);
    half_rows(&m);
    let mut h = harness(m);
    h.run();
    h.run();
    h.key_press(Key::Escape);
    h.run();
    let mut m = h.into_state();
    assert!(m.wait_idle(WAIT));
    assert_eq!(m.dialog, None);
    assert_eq!(dims(&m, "up.png"), (1024, 1024));
    assert_eq!(dims(&m, "up2.png"), (1024, 1024));
    assert!(!toml(&m).contains("drop_top_mip"));
}

#[test]
fn the_half_size_box_has_no_enter_default_and_ignores_a_click_outside() {
    use sdc_gui::ui::dialogs::{half_size_buttons, Role};
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path(), true);
    export(&mut m);
    half_rows(&m);
    let mut h = harness(m);
    // K16: past the invisible sizing pass and the `was_open` frame.
    h.run();
    h.run();
    h.key_press(Key::Enter);
    h.run();
    assert!(matches!(h.state().dialog, Some(Dialog::HalfSize(_))));
    assert!(
        h.state().export.running.is_none(),
        "Enter started no export"
    );
    for pressed in [true, false] {
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: egui::pos2(10.0, 790.0),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    h.run();
    assert!(matches!(h.state().dialog, Some(Dialog::HalfSize(_))));
    assert!(h.state().export.running.is_none());
    h.get_by_label("Export all at full size");
    h.get_by_label("Export selected at half size");
    h.get_by_role_and_label(egui::accesskit::Role::CheckBox, "up.png");
    h.get_by_role_and_label(egui::accesskit::Role::CheckBox, "up2.png");
    assert!(half_size_buttons(true, true)
        .iter()
        .all(|b| b.1 == Role::Plain));
}

/// CE19 / ui-rules 3: with 40 rows the list scrolls, and the footer stays on screen at 1100, 1280 and 1600 px.
#[test]
fn the_half_size_footer_stays_on_screen_with_many_rows() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path(), true);
    export(&mut m);
    let mut rows = half_rows(&m);
    let long = format!("{}/up.png", "a_very_long_folder_name".repeat(6));
    rows = (0..40)
        .map(|i| sdc_gui::model::HalfRow {
            item: if i == 0 {
                long.clone()
            } else {
                format!("t{i}.png")
            },
            ..rows[0].clone()
        })
        .collect();
    m.dialog = Some(Dialog::HalfSize(rows));
    let mut h = harness(m);
    for w in [1100.0, 1280.0, 1600.0] {
        h.set_size(egui::vec2(w, 800.0));
        h.run();
        h.run();
        for b in ["Export all at full size", "Export selected at half size"] {
            let r = h.get_by_label(b).rect();
            assert!(
                r.max.y <= 800.0 && r.max.x <= w && r.min.x >= 0.0,
                "{b} at {r:?} is off a {w}x800 window"
            );
        }
        // M4: the shortened name's tick box is named by the full path. M5: the list is capped at 60 % of the
        // window: from the first row to the footer is the list plus a separator.
        let first = h
            .get_by_role_and_label(egui::accesskit::Role::CheckBox, &long)
            .rect();
        let footer = h.get_by_label("Export all at full size").rect();
        let cap = 800.0 * sdc_gui::ui::dialogs::HALF_SIZE_LIST_SHARE;
        assert!(
            footer.min.y - first.min.y <= cap + 30.0,
            "list from {} to {} is over the {cap} px cap",
            first.min.y,
            footer.min.y
        );
    }
}

#[test]
fn a_saved_half_size_is_not_asked_again_and_undo_removes_it_in_one_step() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path(), true);
    export(&mut m);
    m.export_half_size(&["up.png".to_string(), "up2.png".to_string()]);
    assert!(m.wait_idle(WAIT));
    assert_eq!(dims(&m, "up2.png"), (512, 512));
    assert_eq!(m.undo_label(), Some("Half size"));
    export(&mut m);
    assert_eq!(m.dialog, None, "nothing is eligible any more");
    m.undo();
    assert!(m.wait_idle(WAIT));
    let p = m.project.as_ref().unwrap();
    assert_eq!(p.file_value("up.png", "drop_top_mip"), None);
    assert_eq!(p.file_value("up2.png", "drop_top_mip"), None);
}

#[test]
fn an_unticked_texture_is_flagged_again_next_time() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path(), true);
    export(&mut m);
    m.export_full_size();
    assert!(m.wait_idle(WAIT));
    assert_eq!(dims(&m, "up.png"), (1024, 1024));
    export(&mut m);
    let items: Vec<String> = half_rows(&m).into_iter().map(|r| r.item).collect();
    assert_eq!(items, ["up.png", "up2.png"]);
}

#[test]
fn the_cli_command_reproduces_the_half_size_output() {
    use engine::api::*;
    use engine::events::{new_batch_id, Reporter, VecSink};
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path(), true);
    export(&mut m);
    m.export_half_size(&["up.png".to_string()]);
    assert!(m.wait_idle(WAIT));
    let root = m.root.clone().unwrap();
    let mut req = BatchRequest::new(
        vec![root.clone()],
        d.path().join("cli"),
        engine::config::Target::Cs,
    );
    req.recursive = true;
    req.accept = engine::policy::AcceptSuggestions::High;
    req.project_config = Some(root.join("sdc.toml"));
    let sink = std::sync::Arc::new(VecSink::default());
    let rep = Reporter::new(sink, new_batch_id());
    let e = Engine::new().unwrap();
    let plan = e.plan(req, &rep, &CancelToken::default()).unwrap();
    let up = plan
        .jobs
        .iter()
        .find(|j| j.input.ends_with("up.png"))
        .unwrap();
    assert!(up.profile.drop_top_mip);
    e.run(plan, &rep, &CancelToken::default()).unwrap();
    let k = job(&m, "up.png");
    assert_eq!(
        std::fs::read(d.path().join("cli").join(&k)).unwrap(),
        std::fs::read(m.export_root().join(&k)).unwrap()
    );
}

#[test]
fn half_size_box_with_a_read_only_toml_exports_full_size() {
    use sdc_gui::ui::dialogs::half_size_buttons;
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path(), true);
    let path = m.root.clone().unwrap().join("sdc.toml");
    let mut perm = std::fs::metadata(&path).unwrap().permissions();
    perm.set_readonly(true);
    std::fs::set_permissions(&path, perm.clone()).unwrap();
    export(&mut m);
    half_rows(&m);
    let why = m.sdc_toml_writable().unwrap_err();
    assert!(why.contains("read-only"), "{why}");
    assert_eq!(m.half_size_unwritable.as_deref(), Some(why.as_str()));
    assert!(
        !half_size_buttons(false, true)[1].2,
        "Export selected is off"
    );
    let mut h = harness(m);
    h.run();
    h.run();
    h.get_by_label_contains(&why);
    h.key_press(Key::Escape);
    h.run();
    let mut m = h.into_state();
    assert!(m.wait_idle(WAIT));
    assert!(
        !matches!(m.dialog, Some(Dialog::Error(_))),
        "{:?}",
        m.dialog
    );
    assert_eq!(dims(&m, "up.png"), (1024, 1024));
    #[allow(clippy::permissions_set_readonly_false)]
    perm.set_readonly(false);
    std::fs::set_permissions(&path, perm).unwrap();
}

/// The fixture opened in Hold mode with a made-up first pass: `up.png` held, then `summary`.
fn ended_with(dir: &Path, summary: engine::api::BatchSummary) -> AppModel {
    let mut m = opened(dir, true);
    for item in ["up.png", "up2.png"] {
        let (k, p) = (job(&m, item), m.item(item).unwrap().input.clone());
        held_events(&mut m.export, &k, &p.to_string_lossy());
    }
    m.end_export_with(Ok(summary));
    m
}

#[test]
fn a_cancelled_export_offers_no_half_size_box() {
    let d = tempfile::tempdir().unwrap();
    let m = ended_with(
        d.path(),
        engine::api::BatchSummary {
            cancelled: vec!["noise.png".into()],
            ..Default::default()
        },
    );
    assert!(!matches!(m.dialog, Some(Dialog::HalfSize(_))));
    assert!(matches!(
        m.export.rows[0].state,
        RowState::Skipped(SkipKind::LowDetail, _)
    ));
    assert!(
        m.status_line
            .contains("textures held for half size were not exported"),
        "{}",
        m.status_line
    );
}

#[test]
fn a_partly_failed_export_still_offers_the_half_size_box() {
    let d = tempfile::tempdir().unwrap();
    let m = ended_with(
        d.path(),
        engine::api::BatchSummary {
            failed: 1,
            ..Default::default()
        },
    );
    assert_eq!(m.export.summary.as_ref().unwrap().failed, 1);
    assert_eq!(half_rows(&m).len(), 2);
}

#[test]
fn an_open_dialog_at_export_end_exports_held_at_full_size_with_a_note() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path(), true);
    m.export_clicked();
    assert!(m.export.running.is_some());
    m.dialog = Some(Dialog::Error("x".into()));
    assert!(m.wait_idle(WAIT));
    assert_eq!(m.dialog, Some(Dialog::Error("x".into())));
    assert_eq!(dims(&m, "up.png"), (1024, 1024));
    assert_eq!(dims(&m, "up2.png"), (1024, 1024));
    assert_eq!(
        logged(&m, "exported at full size (another dialog was open)"),
        1
    );
    assert!(!toml(&m).contains("drop_top_mip"));
}

#[test]
fn a_queued_half_size_pass_is_not_dropped_by_a_dialog() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path(), true);
    export(&mut m);
    m.export_half_size(&["up.png".to_string()]);
    assert!(m.export_queued(), "the pass waits for the re-plan");
    m.dialog = Some(Dialog::Error("x".into()));
    assert!(m.wait_idle(WAIT));
    assert_eq!(dims(&m, "up.png"), (512, 512));
    assert_eq!(m.dialog, Some(Dialog::Error("x".into())));
}

#[test]
fn the_second_pass_keeps_the_progress_epoch() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path(), true);
    export(&mut m);
    let epoch = m.drawer_epoch;
    m.export_full_size();
    assert!(m.export.running.is_some());
    // K14: the clock runs again (the first pass's `BatchDone` stopped it).
    let now = std::time::Instant::now();
    let pace = m.export.pace.as_ref().unwrap();
    assert!(pace.elapsed(now + Duration::from_secs(1)) > pace.elapsed(now));
    assert!(m.wait_idle(WAIT));
    assert_eq!(m.drawer_epoch, epoch);
}

#[test]
fn with_drop_top_mip_off_flagged_textures_export_full_and_are_listed() {
    use sdc_gui::model::bottom::{filtered, ProblemsFilter};
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path(), false);
    export(&mut m);
    assert_eq!(m.dialog, None);
    assert_eq!(dims(&m, "up.png"), (1024, 1024));
    let k = job(&m, "up.png");
    let row = m.export.rows.iter().find(|r| r.job_key == k).unwrap();
    assert!(row.warnings.iter().any(|w| w.code == Code::ChkLowDetail));
    // K2 / M9 (T2): the engine's CHK row is a check row, counted by the Checks filter and once per texture.
    let low = |m: &AppModel| {
        problems(m)
            .iter()
            .filter(|p| p.code == Some(Code::ChkLowDetail))
            .map(|p| (p.level, p.at.clone()))
            .collect::<Vec<_>>()
    };
    let rows = low(&m);
    assert!(
        rows.contains(&(Level::Check, At::File(k.clone()))),
        "{rows:?}"
    );
    assert_eq!(rows.len(), 2, "once per texture (up, up2): {rows:?}");
    assert!(rows.iter().all(|r| r.0 == Level::Check));
    assert!(filtered(&m, Some(&ProblemsFilter::Checks))
        .iter()
        .any(|p| p.code == Some(Code::ChkLowDetail)));
    // M3: the header Export dot reads `ready_rows`; turning the switch off removes the 2 CHK rows and must leave
    // `ready_rows` unchanged (the fixture has other, real warnings, so the dot itself is not green here).
    let on = sdc_gui::model::bottom::flagged(&m);
    let summary_count = |m: &AppModel| -> usize {
        sdc_gui::model::checks::checks_summary(m)
            .and_then(|s| s.split_whitespace().nth(1)?.parse().ok())
            .unwrap_or(0)
    };
    let n_on = summary_count(&m);
    let mut off = m.settings.checks;
    off.low_detail = false;
    m.set_checks(off);
    m.poll();
    assert!(low(&m).is_empty());
    let f_off = sdc_gui::model::bottom::flagged(&m);
    assert_eq!(
        f_off.ready_rows, on.ready_rows,
        "a CHK row never counts as ready"
    );
    assert_eq!(f_off.check_rows + 2, on.check_rows);
    assert_eq!(summary_count(&m) + 2, n_on, "the summary counts each once");
}

/// A PBR set (`landscape/rock01`, its albedo upscaled 2x, so held) with a PGPatcher JSON, in Hold mode.
fn pbr_held(dir: &Path) -> AppModel {
    use sdc_gui::model::Answer;
    let r = dir.join("textures/landscape");
    std::fs::create_dir_all(&r).unwrap();
    upscaled(5).save(r.join("rock01.png")).unwrap();
    ImageBuffer::from_pixel(64, 64, Rgb([128u8, 128, 255]))
        .save(r.join("rock01_normal.png"))
        .unwrap();
    std::fs::write(
        dir.join("textures/sdc.toml"),
        "[defaults.encoder]\neffort = \"ultrafast\"\n",
    )
    .unwrap();
    let settings = GuiSettings {
        export_folder: Some(dir.join("export")),
        checks: ChecksSettings {
            drop_top_mip: true,
            ..ChecksSettings::default()
        },
        ..GuiSettings::default()
    };
    let mut m = AppModel::new(settings, None, support::session());
    m.accept = engine::policy::AcceptSuggestions::None;
    m.open_folder(dir.join("textures"));
    assert!(m.wait_idle(WAIT));
    m.decide(
        "landscape/rock01.png",
        Answer::Subtype(engine::config::Subtype::PbrAlbedo),
    );
    m.decide(
        "landscape/rock01_normal.png",
        Answer::Subtype(engine::config::Subtype::PbrNormal),
    );
    m.decide(
        "landscape/rock01_normal.png",
        Answer::Green(engine::config::GreenConvention::Directx),
    );
    assert!(m.wait_idle(WAIT));
    m
}

/// Export, through Review when it opens (its asks acknowledged).
fn go(m: &mut AppModel) {
    m.export_clicked();
    if m.review.is_some() {
        m.acknowledge_all_asks();
        m.export_from_review();
    }
    assert!(m.wait_idle(WAIT));
}

fn json_path(m: &AppModel) -> PathBuf {
    m.export_root()
        .join("PBRNIFPatcher")
        .join(format!("{}.json", sdc_gui::model::pbr_export::json_name(m)))
}

#[test]
fn the_json_finishers_run_once_after_the_final_pass() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_held(d.path());
    go(&mut m);
    let json = json_path(&m);
    half_rows(&m);
    assert!(!json.exists(), "no JSON while the box is open");
    assert_eq!(logged(&m, "Checks:"), 0, "no summary while the box is open");
    m.export_full_size();
    assert!(m.wait_idle(WAIT));
    assert!(json.exists(), "{:?}", m.export.notes);
    assert_eq!(logged(&m, "Checks:"), 1);
    // C7: Review's Overwrite answer survives the fresh = false pass and is applied once, after it.
    std::fs::write(&json, "{ \"edited\": true }\n").unwrap();
    m.set_json_value(
        "landscape/rock01",
        "specular_level",
        Some(serde_json::json!(0.06)),
    );
    assert!(m.wait_idle(WAIT));
    m.export_clicked();
    assert!(
        m.review.is_some(),
        "the edited JSON is asked about in Review"
    );
    m.acknowledge_all_asks();
    m.answer_review_json(true);
    m.export_from_review();
    assert!(m.wait_idle(WAIT));
    half_rows(&m);
    assert!(std::fs::read_to_string(&json).unwrap().contains("edited"));
    m.export_full_size();
    assert!(m.wait_idle(WAIT));
    assert_eq!(m.dialog, None, "{:?}", m.dialog);
    assert!(
        !std::fs::read_to_string(&json).unwrap().contains("edited"),
        "{:?}",
        m.export.notes
    );
}

#[test]
fn a_drop_top_mip_estimate_halves_the_size_and_writes_one_level_less() {
    use engine::config::{MipLevels, Profile};
    use sdc_gui::model::out_size::estimate;
    let d = tempfile::tempdir().unwrap();
    let m = opened(d.path(), true);
    let mut p: Profile = m.item("up.png").unwrap().job.clone().unwrap().profile;
    let full = estimate((1024, 1024), &p);
    p.drop_top_mip = true;
    let half = estimate((1024, 1024), &p);
    assert_eq!((half.width, half.height), (512, 512));
    assert_eq!(half.mips, full.mips - 1, "full stays full");
    assert_eq!(
        half.chain_bytes,
        estimate((512, 512), &{
            let mut q = p.clone();
            q.drop_top_mip = false;
            q
        })
        .chain_bytes
    );
    // M4: n levels with the top dropped writes n - 1.
    p.mips.levels = MipLevels::Count(5);
    assert_eq!(estimate((1024, 1024), &p).mips, 4);
}

/// Owner ruling O1: a saved half size has a row in the texture's Advanced settings; Clear removes the key (M9: it
/// never writes `false`).
#[test]
fn the_advanced_row_shows_a_saved_half_size_and_clear_removes_it() {
    use sdc_gui::model::fields::advanced_field_views;
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path(), true);
    export(&mut m);
    m.export_half_size(&["up.png".to_string()]);
    assert!(m.wait_idle(WAIT));
    let j = m.item("up.png").unwrap().job.clone().unwrap();
    let v = advanced_field_views(m.target, &j)
        .into_iter()
        .find(|v| v.field == "drop_top_mip")
        .unwrap();
    assert_eq!(v.value, serde_json::json!(true));
    assert!(v.edited, "Clear (↺) shows for the texture's own value");
    m.settings.advanced_open = true;
    m.select_item("up.png");
    let mut h = harness(m);
    h.run();
    h.get_by_label("Half size (drop mip 0)");
    let mut m = h.into_state();
    m.clear_field("up.png", "drop_top_mip");
    assert!(m.wait_idle(WAIT));
    assert_eq!(m.undo_label(), Some("Reset drop_top_mip"));
    m.save().unwrap();
    assert!(!toml(&m).contains("drop_top_mip"), "Clear removes the key");
    // Unticking the row is a Clear too, never a `false`.
    m.set_field("up.png", "drop_top_mip", serde_json::json!(true));
    m.set_field("up.png", "drop_top_mip", serde_json::json!(false));
    assert_eq!(
        m.project
            .as_ref()
            .unwrap()
            .file_value("up.png", "drop_top_mip"),
        None
    );
}

/// M1: an Export click while the half-size pass waits for its plan is not queued silently.
#[test]
fn an_export_click_while_the_half_size_pass_waits_gets_a_note() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path(), true);
    export(&mut m);
    m.export_half_size(&["up.png".to_string()]);
    assert!(m.export_queued());
    m.export_clicked();
    assert_eq!(m.status_line, "Export waits for the half-size pass");
    assert_eq!(logged(&m, "Export waits for the half-size pass"), 1);
    assert!(m.wait_idle(WAIT));
    assert_eq!(dims(&m, "up.png"), (512, 512));
    assert_eq!(m.dialog, None, "the click started no second export");
}

/// M1: a Review ask that appears while the half-size pass waits stops it: Review opens, nothing more is written
/// past the warning, and the first pass's JSON is still written.
#[test]
fn a_new_review_ask_stops_the_half_size_pass() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_held(d.path());
    go(&mut m);
    half_rows(&m);
    m.export_half_size(&["landscape/rock01.png".to_string()]);
    m.set_json_value("landscape/rock01", "match1", Some(serde_json::json!("")));
    assert!(m.wait_idle(WAIT));
    assert!(m.review.is_some(), "Review opens on the new ask");
    assert!(
        m.status_line
            .contains("not exported: a new warning needs a look in Review"),
        "{}",
        m.status_line
    );
    // The JSON finisher ran for the first pass: its only set was held, so it says why nothing was written.
    assert!(
        m.export
            .notes
            .iter()
            .any(|n| n.contains("not in JSON: landscape/rock01 (skipped: held")),
        "{:?}",
        m.export.notes
    );
    assert!(!m
        .export_root()
        .join(job(&m, "landscape/rock01.png"))
        .exists());
}

/// M5, Review Focus 1: the save fails after "Export selected" (the read-only flag set after the box opened): the
/// step is undone and everything is written at full size; no half size without a saved field.
#[test]
fn a_failed_half_size_save_is_undone_and_exports_full_size() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path(), true);
    export(&mut m);
    half_rows(&m);
    let path = m.root.clone().unwrap().join("sdc.toml");
    let mut perm = std::fs::metadata(&path).unwrap().permissions();
    perm.set_readonly(true);
    std::fs::set_permissions(&path, perm.clone()).unwrap();
    m.export_half_size(&["up.png".to_string()]);
    assert!(matches!(m.dialog, Some(Dialog::Error(_))), "{:?}", m.dialog);
    assert_eq!(
        m.project
            .as_ref()
            .unwrap()
            .file_value("up.png", "drop_top_mip"),
        None,
        "the step was undone"
    );
    assert!(m.wait_idle(WAIT));
    assert_eq!(dims(&m, "up.png"), (1024, 1024));
    assert!(!toml(&m).contains("drop_top_mip"));
    #[allow(clippy::permissions_set_readonly_false)]
    perm.set_readonly(false);
    std::fs::set_permissions(&path, perm).unwrap();
}

/// M6: one mip level has nothing to drop: the Advanced row is locked with the reason, and no write sets it.
#[test]
fn the_half_size_row_is_locked_without_mips() {
    use sdc_gui::model::fields::advanced_field_views;
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path(), true);
    m.set_field("up.png", "mips.levels", serde_json::json!("none"));
    assert!(m.wait_idle(WAIT));
    let j = m.item("up.png").unwrap().job.clone().unwrap();
    let v = advanced_field_views(m.target, &j)
        .into_iter()
        .find(|v| v.field == "drop_top_mip")
        .unwrap();
    assert_eq!(v.lock, Some("No mips to drop"));
    m.set_field("up.png", "drop_top_mip", serde_json::json!(true));
    assert_eq!(
        m.project
            .as_ref()
            .unwrap()
            .file_value("up.png", "drop_top_mip"),
        None
    );
}

/// M7: a texture the held pass wrote at half size loses its first-pass CHK-LOW-DETAIL row; the unticked one keeps it.
#[test]
fn a_texture_written_at_half_size_drops_its_low_detail_row() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path(), true);
    export(&mut m);
    m.export_half_size(&["up.png".to_string()]);
    assert!(m.wait_idle(WAIT));
    let low = |m: &AppModel, item: &str| {
        let k = job(m, item);
        let row = m.export.rows.iter().find(|r| r.job_key == k).unwrap();
        let in_row = row.warnings.iter().any(|w| w.code == Code::ChkLowDetail);
        let in_problems = problems(m)
            .iter()
            .any(|p| p.code == Some(Code::ChkLowDetail) && p.at == At::File(k.clone()));
        (in_row, in_problems)
    };
    assert_eq!(low(&m, "up.png"), (false, false));
    assert_eq!(low(&m, "up2.png"), (true, true));
}

/// Owner ruling 2026-10-20: Copy settings asks about a saved half size; its Enter default copies everything else.
#[test]
fn copy_settings_asks_about_a_saved_half_size() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path(), true);
    m.set_field("up.png", "drop_top_mip", serde_json::json!(true));
    m.set_field("up.png", "header", serde_json::json!("dx10"));
    assert!(m.wait_idle(WAIT));
    let select = |m: &mut AppModel| {
        m.click_item("noise.png", false, false);
        m.click_item("up2.png", true, false);
        m.click_item("up.png", true, false);
        m.open_copy_settings();
    };
    select(&mut m);
    let fields = match &m.dialog {
        Some(Dialog::CopySettings { fields, .. }) => fields.clone(),
        d => panic!("{d:?}"),
    };
    assert!(
        fields.iter().all(|(f, _)| f != "drop_top_mip"),
        "the question is the one place it is decided: {fields:?}"
    );
    let ticked: Vec<String> = fields.into_iter().map(|f| f.0).collect();
    m.copy_settings_chosen("up.png", &["noise.png".into(), "up2.png".into()], &ticked);
    assert!(matches!(m.dialog, Some(Dialog::CopyHalfSize { .. })));
    // Enter = "Copy without half size".
    let mut h = harness(m);
    h.run();
    h.run();
    h.key_press(Key::Enter);
    h.run();
    let mut m = h.into_state();
    assert_eq!(m.dialog, None);
    let p = m.project.as_ref().unwrap();
    assert_eq!(
        p.file_value("up2.png", "header"),
        Some(serde_json::json!("dx10"))
    );
    assert_eq!(p.file_value("up2.png", "drop_top_mip"), None);
    assert!(m.wait_idle(WAIT));
    select(&mut m);
    m.copy_settings_chosen("up.png", &["noise.png".into(), "up2.png".into()], &ticked);
    m.resolve_copy_half_size(true);
    let p = m.project.as_ref().unwrap();
    assert_eq!(
        p.file_value("noise.png", "drop_top_mip"),
        Some(serde_json::json!(true))
    );
    assert_eq!(m.undo_label(), Some("Copy settings from up.png"));
}
