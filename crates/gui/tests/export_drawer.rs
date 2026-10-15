//! Task 5 (items 6 and 14; DECISIONS AG34-AG38): Open export folder, Show in Explorer, the progress line, the
//! virtualised Files list with its filters, and the Warnings tab's messages.
mod support;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use engine::codes::{Code, Warning};
use engine::events::{EngineEvent, EventKind, SkipKind, Stage, EVENT_SCHEMA};
use engine::validate::ValidationSummary;
use sdc_gui::model::bulk::MenuAction;
use sdc_gui::model::export::*;
use sdc_gui::model::shell::{args, select_arg, ShellRequest};
use sdc_gui::model::AppModel;
use sdc_gui::settings::GuiSettings;
use sdc_gui::ui::drawer::{self, DrawerState};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use support::*;

// ---- Show in Explorer: the argument ----

#[test]
fn select_is_one_argument_with_backslashes() {
    assert_eq!(
        select_arg(Path::new("C:/Mods/rock01.dds")),
        OsString::from(r"/select,C:\Mods\rock01.dds")
    );
    // Spaces and commas stay inside the one argument (Explorer reads everything after the first comma).
    let p = Path::new(r"C:\My Mods\a,b\rock 01, copy.dds");
    assert_eq!(
        args(&ShellRequest::Reveal(p.into())),
        vec![OsString::from(r"/select,C:\My Mods\a,b\rock 01, copy.dds")]
    );
    // Unicode survives.
    assert_eq!(
        select_arg(Path::new("D:/Textures/fée ünï/ロック.dds")),
        OsString::from("/select,D:\\Textures\\fée ünï\\ロック.dds")
    );
    // Open takes the folder alone.
    assert_eq!(
        args(&ShellRequest::Open(PathBuf::from("C:/out dir/x"))),
        vec![OsString::from(r"C:\out dir\x")]
    );
}

#[test]
fn command_arg_keeps_the_select_prefix_and_path_together() {
    let mut c = std::process::Command::new("explorer.exe");
    c.args(args(&ShellRequest::Reveal(r"C:\a b\c,d.dds".into())));
    let got: Vec<_> = c.get_args().collect();
    assert_eq!(got, vec![OsString::from(r"/select,C:\a b\c,d.dds")]);
}

// ---- The progress line ----

fn at(t0: Instant, ms: u64) -> Instant {
    t0 + Duration::from_millis(ms)
}

#[test]
fn no_eta_before_three_files_or_two_seconds() {
    let t0 = Instant::now();
    let mut p = Pace::new(t0);
    p.finished(at(t0, 500));
    p.finished(at(t0, 900));
    // Two files, 1.9 s: nothing to base an ETA on yet.
    assert_eq!(p.eta(2, 10, at(t0, 1900)), None);
    assert!(!p.line(2, 10, at(t0, 1900)).contains("ETA"));
    // Two seconds have passed.
    assert!(p.eta(2, 10, at(t0, 2000)).is_some());
    // Or three files have finished, early on.
    let mut q = Pace::new(t0);
    for ms in [100, 200, 300] {
        q.finished(at(t0, ms));
    }
    assert!(q.eta(3, 10, at(t0, 400)).is_some());
}

#[test]
fn the_line_shows_done_elapsed_rate_and_eta() {
    let t0 = Instant::now();
    let mut p = Pace::new(t0);
    // Ten files in the first five seconds: 2 files/s; 10 of 30 done, 20 left, ETA 10 s.
    for i in 1..=10 {
        p.finished(at(t0, i * 500));
    }
    assert_eq!(
        p.line(10, 30, at(t0, 5000)),
        "10/30 · 0:05 · 2.0 files/s · ETA 0:10"
    );
}

#[test]
fn the_rate_is_a_rolling_ten_second_average() {
    let t0 = Instant::now();
    let mut p = Pace::new(t0);
    // 20 files in the first 2 s, then a pause, then 5 files between 20 s and 25 s.
    for i in 1..=20 {
        p.finished(at(t0, i * 100));
    }
    for i in 0..5 {
        p.finished(at(t0, 20_000 + i * 1000));
    }
    // At 25 s the window holds only the last five (those after 15 s): 0.5 files/s, not 25/25.
    let r = p.rate(at(t0, 25_000));
    assert!((r - 0.5).abs() < 1e-9, "{r}");
    // An idle window reads 0.
    assert_eq!(p.rate(at(t0, 60_000)), 0.0);
    assert_eq!(p.eta(25, 100, at(t0, 60_000)), None, "no rate, no ETA");
}

#[test]
fn elapsed_stops_when_the_export_ends_and_hours_format() {
    let t0 = Instant::now();
    let mut p = Pace::new(t0);
    p.stop(at(t0, 65_000));
    assert_eq!(p.elapsed(at(t0, 999_000)), Duration::from_secs(65));
    assert_eq!(fmt_duration(Duration::from_secs(65)), "1:05");
    assert_eq!(fmt_duration(Duration::from_millis(400)), "0:01");
    assert_eq!(fmt_duration(Duration::from_secs(3725)), "1:02:05");
}

#[test]
fn events_feed_the_pace_with_the_clock_they_are_given() {
    let t0 = Instant::now();
    let mut x = ExportModel::default();
    x.pace = Some(Pace::new(t0));
    x.on_event_at(
        ev(0, "a/b.dds", "a/b.png", EventKind::FileStarted),
        at(t0, 100),
    );
    x.on_event_at(ev(1, "a/b.dds", "a/b.png", failed("x")), at(t0, 1000));
    x.on_event_at(
        ev(2, "a/c.dds", "a/c.png", skipped(SkipKind::UpToDate)),
        at(t0, 1500),
    );
    let p = x.pace.as_ref().unwrap();
    // The up-to-date skip is not work: one converted file in the 2 s so far is 0.5 files/s.
    assert!((p.rate(at(t0, 2000)) - 0.5).abs() < 1e-9);
}

// ---- Events ----

fn ev(seq: u64, job: &str, path: &str, kind: EventKind) -> EngineEvent {
    EngineEvent {
        schema: EVENT_SCHEMA,
        seq,
        ts: "2026-10-14T00:00:00.000Z".into(),
        batch: "b".into(),
        set: None,
        path: Some(path.into()),
        job: Some(job.into()),
        kind,
    }
}

fn failed(e: &str) -> EventKind {
    EventKind::FileFailed {
        stage: Stage::Write,
        error: e.into(),
    }
}

fn skipped(kind: SkipKind) -> EventKind {
    EventKind::FileSkipped {
        kind,
        code: None,
        reason: "r".into(),
    }
}

fn discovered(x: &mut ExportModel, i: usize) {
    x.on_event(ev(
        i as u64,
        &format!("t/f{i:05}.dds"),
        &format!("t/f{i:05}.png"),
        EventKind::FileDiscovered {
            output: format!("out/t/f{i:05}.dds"),
        },
    ));
}

/// 10 000 rows: every 10th failed, every 4th has a warning (so every 20th is both).
fn big() -> ExportModel {
    let mut x = ExportModel::default();
    for i in 0..10_000 {
        discovered(&mut x, i);
        let (job, path) = (format!("t/f{i:05}.dds"), format!("t/f{i:05}.png"));
        if i % 4 == 0 {
            x.on_event(ev(
                0,
                &job,
                &path,
                EventKind::Warning(Warning::new(Code::DimNpot, format!("odd size {i}"))),
            ));
        }
        if i % 10 == 0 {
            x.on_event(ev(0, &job, &path, failed("boom")));
        }
    }
    x
}

#[test]
fn the_filters_combine_with_and() {
    let x = big();
    let n = |f: FileFilter| x.rows.iter().filter(|r| f.matches(r)).count();
    let failed_only = FileFilter {
        failed_only: true,
        ..Default::default()
    };
    let warn = FileFilter {
        has_warnings: true,
        ..Default::default()
    };
    let both = FileFilter {
        failed_only: true,
        has_warnings: true,
    };
    assert_eq!(n(FileFilter::default()), 10_000);
    assert_eq!(n(failed_only), 1000);
    assert_eq!(n(warn), 2500);
    assert_eq!(n(both), 500, "failed AND has a warning");
}

#[test]
fn the_filtered_list_is_computed_once_per_change() {
    let mut x = big();
    let mut idx = FilesIndex::default();
    let both = FileFilter {
        failed_only: true,
        has_warnings: true,
    };
    // 200 frames over 10k rows with nothing changing: one computation.
    for _ in 0..200 {
        assert_eq!(idx.len(&x, both), 500);
        assert_eq!(idx.row(&x, both, 0).unwrap().job_key, "t/f00000.dds");
    }
    assert_eq!(idx.computed, 1);
    // A change of filter recomputes once ...
    let warn = FileFilter {
        has_warnings: true,
        ..Default::default()
    };
    for _ in 0..50 {
        assert_eq!(idx.len(&x, warn), 2500);
    }
    assert_eq!(idx.computed, 2);
    // ... so does an event that changes a row ...
    x.on_event(ev(
        0,
        "t/f00001.dds",
        "t/f00001.png",
        EventKind::Warning(Warning::new(Code::DimNpot, "late")),
    ));
    for _ in 0..50 {
        assert_eq!(idx.len(&x, warn), 2501);
    }
    assert_eq!(idx.computed, 3);
    // ... but progress does not, and no filter needs no index at all.
    x.on_event(ev(
        0,
        "t/f00001.dds",
        "t/f00001.png",
        EventKind::Progress {
            stage: Stage::Write,
            done: 1,
            total: 2,
        },
    ));
    assert_eq!(idx.len(&x, warn), 2501);
    assert_eq!(idx.len(&x, FileFilter::default()), 10_000);
    assert_eq!(idx.computed, 3);
    // The warning list is cached by the revision as well.
    assert_eq!(idx.warnings(&x).len(), 2501);
    assert_eq!(idx.warnings(&x).len(), 2501);
    assert_eq!(idx.computed, 4);
}

// ---- The drawer ----

fn drawer_harness(m: AppModel) -> Harness<'static, (AppModel, DrawerState)> {
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(1100.0, 500.0))
        .build_ui_state(
            |ui, (m, st): &mut (AppModel, DrawerState)| drawer::draw(ui, m, st),
            (m, DrawerState::default()),
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

fn model_with(export: &Path, x: ExportModel) -> AppModel {
    let mut m = AppModel::placeholder(GuiSettings {
        export_folder: Some(export.to_path_buf()),
        ..GuiSettings::default()
    });
    m.export = x;
    m.export.open = true;
    m
}

#[test]
fn open_export_folder_is_disabled_until_the_folder_exists() {
    let d = tempfile::tempdir().unwrap();
    let out = d.path().join("export");
    let mut h = drawer_harness(model_with(&out, ExportModel::default()));
    h.run();
    assert!(h
        .get_by_label("Open export folder")
        .accesskit_node()
        .is_disabled());
    let why = h.state().0.export_folder_state().unwrap_err();
    assert!(why.contains("does not exist yet"), "{why}");
    h.get_by_label("Open export folder").click();
    h.run();
    assert!(
        h.state().0.shell.is_empty(),
        "a disabled button queues nothing"
    );
    // Once it exists the button opens it.
    h.state_mut().1.stats.clear();
    std::fs::create_dir_all(&out).unwrap();
    h.run();
    h.run();
    assert!(!h
        .get_by_label("Open export folder")
        .accesskit_node()
        .is_disabled());
    h.get_by_label("Open export folder").click();
    h.run();
    assert_eq!(h.state_mut().0.take_shell(), vec![ShellRequest::Open(out)]);
}

#[test]
fn the_files_list_draws_only_the_rows_on_screen() {
    let d = tempfile::tempdir().unwrap();
    let mut h = drawer_harness(model_with(d.path(), big()));
    h.run();
    h.run();
    let drawn = h.query_all_by_label_contains("t/f").count();
    assert!(drawn > 0 && drawn < 100, "{drawn} rows drawn of 10000");
}

#[test]
fn the_filter_toggles_narrow_the_files_list() {
    let d = tempfile::tempdir().unwrap();
    let mut h = drawer_harness(model_with(d.path(), big()));
    h.run();
    h.get_by_label("Failed only").click();
    h.run();
    h.run();
    // Rows 0, 10, 20 ... fail; the first screenful holds only those.
    assert!(h.query_by_label("t/f00001.dds").is_none());
    h.get_by_label("t/f00010.dds");
    h.get_by_label("Has warnings").click();
    h.run();
    h.run();
    // Failed AND warned: 0, 20, 40 ...
    assert!(h.query_by_label("t/f00010.dds").is_none(), "no warning");
    h.get_by_label("t/f00020.dds");
    assert_eq!(h.state().1.index.computed, 2, "once per filter change");
}

#[test]
fn warning_rows_show_the_message_with_code_and_severity() {
    let d = tempfile::tempdir().unwrap();
    let mut x = ExportModel::default();
    x.on_event(ev(
        0,
        "t/a.dds",
        "t/a.png",
        EventKind::FileDiscovered {
            output: "o/t/a.dds".into(),
        },
    ));
    x.on_event(ev(
        1,
        "t/a.dds",
        "t/a.png",
        EventKind::Warning(Warning::new(
            Code::DimNpot,
            "1000x750 is not a power of two; mips will be uneven",
        )),
    ));
    let mut h = drawer_harness(model_with(d.path(), x));
    h.run();
    h.get_by_label("Warnings").click();
    h.run();
    h.run();
    h.get_by_label(Code::DimNpot.as_str());
    h.get_by_label_contains("1000x750 is not a power of two; mips will be uneven");
    assert_eq!(
        warning_line(&h.state().0.export.rows[0].warnings[0]),
        format!(
            "{} ({}): 1000x750 is not a power of two; mips will be uneven",
            Code::DimNpot.as_str(),
            format!("{:?}", Code::DimNpot.severity()).to_lowercase()
        )
    );
}

#[test]
fn a_files_row_reveals_the_output_once_written_else_the_source() {
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("rock.png");
    let out = d.path().join("rock.dds");
    std::fs::write(&src, b"x").unwrap();
    let mut x = ExportModel::default();
    x.on_event(ev(
        0,
        "rock.dds",
        &src.to_string_lossy(),
        EventKind::FileDiscovered {
            output: out.to_string_lossy().replace('\\', "/"),
        },
    ));
    let m = model_with(d.path(), x);
    assert_eq!(m.row_reveal_target(&m.export.rows[0]), Some(src.clone()));
    std::fs::write(&out, b"y").unwrap();
    assert_eq!(m.row_reveal_target(&m.export.rows[0]), Some(out));
    std::fs::remove_file(&src).unwrap();
    // The output is still there.
    assert!(m.row_reveal_target(&m.export.rows[0]).is_some());
    // The row's button queues a Reveal.
    let mut h = drawer_harness(m);
    h.run();
    h.get_by_label("Show in Explorer").click();
    h.run();
    assert!(matches!(
        h.state_mut().0.take_shell().as_slice(),
        [ShellRequest::Reveal(_)]
    ));
}

#[test]
fn done_rows_remember_the_output_the_engine_wrote() {
    let mut x = ExportModel::default();
    x.on_event(ev(
        0,
        "a.dds",
        "a.png",
        EventKind::FileDone {
            output: "C:/out/a.dds".into(),
            validation: ValidationSummary {
                passed: true,
                ..Default::default()
            },
        },
    ));
    assert_eq!(
        x.outputs.get("a.dds").map(String::as_str),
        Some("C:/out/a.dds")
    );
}

// ---- The context menus ----

#[test]
fn the_tile_menu_has_show_in_explorer_for_a_single_texture() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.select_item("landscape/rock01.png");
    let menu = m.item_menu();
    let e = menu
        .iter()
        .find(|e| e.label == "Show in Explorer")
        .expect("the entry");
    assert_eq!(e.disabled, None);
    assert_eq!(
        e.action,
        MenuAction::ShowInExplorer("landscape/rock01.png".into())
    );
    assert_eq!(m.run_menu(e.action.clone()), None);
    let want = m.item("landscape/rock01.png").unwrap().input.clone();
    assert_eq!(m.take_shell(), vec![ShellRequest::Reveal(want.clone())]);
    // The file is gone: the entry is off, with the reason, and the command is refused.
    std::fs::remove_file(&want).unwrap();
    // The menu's answer is cached for a second (AG128); the clock did not move here.
    m.stats.borrow_mut().clear();
    let off = m
        .item_menu()
        .into_iter()
        .find(|e| e.label == "Show in Explorer")
        .unwrap();
    assert!(off.disabled.unwrap().contains("is not on disk"));
    assert!(m.run_menu(off.action).is_some());
    assert!(m.take_shell().is_empty());
    // Two or more selected: the bulk menu has no single file to show.
    m.select_item("landscape/leaf.png");
    m.click_item("landscape/rock01.png", true, false);
    assert!(m.item_menu().iter().all(|e| e.label != "Show in Explorer"));
}

#[test]
fn the_slot_menu_shows_the_member_that_fills_the_slot() {
    use sdc_gui::model::sets::SlotKind;
    let d = tempfile::tempdir().unwrap();
    let m = pbr_opened(d.path());
    let menu = m.slot_menu("landscape/rock01", SlotKind::Diffuse);
    let e = menu
        .iter()
        .find(|e| e.label == "Show in Explorer")
        .expect("the entry");
    assert!(matches!(&e.action, MenuAction::ShowInExplorer(k) if k == "landscape/rock01.png"));
}

// ---- Task 6 carry-over: By code, stat cache, pace without skips ----

fn warned(x: &mut ExportModel, key: &str, code: Code, msg: &str) {
    let png = key.replace(".dds", ".png");
    x.on_event(ev(
        0,
        key,
        &png,
        EventKind::FileDiscovered { output: key.into() },
    ));
    x.on_event(ev(
        1,
        key,
        &png,
        EventKind::Warning(Warning::new(code, msg)),
    ));
}

#[test]
fn by_code_lists_a_row_per_code_and_expands_to_its_files() {
    let d = tempfile::tempdir().unwrap();
    let mut x = ExportModel::default();
    warned(&mut x, "t/a.dds", Code::DimNpot, "first npot");
    warned(&mut x, "t/b.dds", Code::DimNpot, "second npot");
    warned(&mut x, "t/c.dds", Code::DimNpot, "third npot");
    let other = Code::PackConflict;
    warned(&mut x, "t/d.dds", other, "a conflict");
    let mut idx = FilesIndex::default();
    let g = idx.groups(&x).to_vec();
    assert_eq!(g.iter().map(|g| g.2 - g.1).sum::<usize>(), 4);
    assert_eq!(g.len(), 2);
    let mut h = drawer_harness(model_with(d.path(), x));
    h.run();
    h.get_by_label("Warnings").click();
    h.run();
    // The flat list is the default: the file link comes with its message.
    h.get_by_label("t/a.dds");
    h.get_by_label("By code").click();
    h.run();
    h.run();
    h.get_by_label(&format!("> {} (3)", Code::DimNpot.as_str()));
    h.get_by_label(&format!("> {} (1)", other.as_str()));
    assert!(h.query_by_label("t/a.dds").is_none());
    h.get_by_label(&format!("> {} (3)", Code::DimNpot.as_str()))
        .click();
    h.run();
    h.run();
    h.get_by_label(&format!("v {} (3)", Code::DimNpot.as_str()));
    for f in ["t/a.dds", "t/b.dds", "t/c.dds"] {
        h.get_by_label(f);
    }
    assert!(h.query_by_label("t/d.dds").is_none());
}

#[test]
fn stats_are_made_once_a_second() {
    let t0 = Instant::now();
    let d = tempfile::tempdir().unwrap();
    let mut c = sdc_gui::model::shell::StatCache::default();
    for i in 0..100 {
        assert!(c.is_dir(d.path(), at(t0, i)));
        assert!(!c.is_file(&d.path().join("x"), at(t0, i)));
    }
    assert_eq!(c.stats, 2, "100 frames, two paths: two stats");
    c.is_dir(d.path(), at(t0, 1500));
    assert_eq!(c.stats, 3, "re-checked after a second");
}

#[test]
fn skips_stay_out_of_the_rate_but_count_as_done() {
    let t0 = Instant::now();
    let mut x = ExportModel::default();
    x.pace = Some(Pace::new(t0));
    for i in 0..40 {
        let (job, png) = (format!("a/{i}.dds"), format!("a/{i}.png"));
        x.on_event_at(
            ev(i, &job, &png, skipped(SkipKind::UpToDate)),
            at(t0, 100 * i),
        );
    }
    let p = x.pace.as_ref().unwrap();
    assert_eq!(p.rate(at(t0, 4000)), 0.0);
    assert!(p.eta(40, 100, at(t0, 4000)).is_none(), "no ETA from skips");
}
