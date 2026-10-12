//! Task 9: the preview timing log. The writer is exercised on a temp dir with explicit stamps and options; the
//! gap watch and the frame statistics are pure (injected `Instant`s, no sleeps).
use render3d::perf_log::{self, FrameStats, GapWatch, Opts, PerfLog};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn lines(p: &Path) -> Vec<Value> {
    fs::read_to_string(p)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("not JSON: {l:?}: {e}")))
        .collect()
}

fn logs(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("preview-timings-"))
        .collect();
    v.sort();
    v
}

#[test]
fn every_line_is_a_json_object_with_t_ms_and_event_and_the_log_ends_with_run_end() {
    let d = tempfile::tempdir().unwrap();
    let log = PerfLog::open(d.path(), "20261011-100000", &Opts::default()).unwrap();
    let path = log.path().to_path_buf();
    assert!(path.ends_with("preview-timings-20261011-100000.jsonl"));
    log.emit(
        "compile",
        json!({"key": "abc", "ms": 1.5, "cache": "miss", "ok": true}),
    );
    log.emit(
        "edit",
        json!({"shape": 3, "kind": "Same", "ms_to_frame": 2.0}),
    );
    drop(log);
    let l = lines(&path);
    assert_eq!(l.len(), 3);
    for e in &l {
        assert!(e["t_ms"].is_f64() || e["t_ms"].is_u64(), "{e}");
        assert!(e["event"].is_string());
    }
    assert_eq!(
        (l[0]["event"].as_str(), l[0]["cache"].as_str()),
        (Some("compile"), Some("miss"))
    );
    assert_eq!(l[2]["event"], "run_end");
    assert_eq!(l[2]["dropped"], 0);
    assert!(l[0]["t_ms"].as_f64() <= l[2]["t_ms"].as_f64());
}

#[test]
fn only_the_ten_newest_logs_are_kept_and_other_files_are_left_alone() {
    let d = tempfile::tempdir().unwrap();
    for n in 1..=12 {
        fs::write(
            d.path()
                .join(format!("preview-timings-202610{n:02}-000000.jsonl")),
            "{}\n",
        )
        .unwrap();
    }
    fs::write(d.path().join("other.txt"), "x").unwrap();
    let log = PerfLog::open(d.path(), "20261020-000000", &Opts::default()).unwrap();
    drop(log);
    let kept = logs(d.path());
    assert_eq!(kept.len(), 10, "{kept:?}");
    assert!(!kept.contains(&"preview-timings-20261001-000000.jsonl".to_string()));
    assert!(!kept.contains(&"preview-timings-20261003-000000.jsonl".to_string()));
    assert!(kept.contains(&"preview-timings-20261004-000000.jsonl".to_string()));
    assert!(kept.contains(&"preview-timings-20261020-000000.jsonl".to_string()));
    assert!(d.path().join("other.txt").exists());
}

#[test]
fn two_runs_in_the_same_second_get_two_files() {
    let d = tempfile::tempdir().unwrap();
    let a = PerfLog::open(d.path(), "20261011-100000", &Opts::default()).unwrap();
    let b = PerfLog::open(d.path(), "20261011-100000", &Opts::default()).unwrap();
    assert_ne!(a.path(), b.path());
}

#[test]
fn a_full_file_gets_one_log_capped_line_and_nothing_after() {
    let d = tempfile::tempdir().unwrap();
    let o = Opts {
        max_bytes: 400,
        ..Opts::default()
    };
    let log = PerfLog::open(d.path(), "20261011-100000", &o).unwrap();
    let path = log.path().to_path_buf();
    for i in 0..50 {
        log.emit(
            "edit",
            json!({"shape": i, "kind": "Same", "ms_to_frame": 1.0}),
        );
    }
    drop(log);
    let l = lines(&path);
    let capped: Vec<_> = l.iter().filter(|e| e["event"] == "log_capped").collect();
    assert_eq!(capped.len(), 1, "{l:?}");
    assert_eq!(l.last().unwrap()["event"], "log_capped");
    assert!(l.len() < 12, "the cap stopped the writing: {}", l.len());
    assert!(fs::metadata(&path).unwrap().len() < 400 + 200);
}

#[test]
fn the_switch_turns_the_log_off_and_writes_nothing() {
    let d = tempfile::tempdir().unwrap();
    assert!(perf_log::open_if_enabled(d.path(), "20261011-100000", Some("0")).is_none());
    assert!(logs(d.path()).is_empty());
    assert!(perf_log::open_if_enabled(d.path(), "20261011-100001", Some("1")).is_some());
    assert!(perf_log::open_if_enabled(d.path(), "20261011-100002", None).is_some());
}

#[test]
fn the_stamp_is_utc_and_sorts_by_time() {
    let at = |s: u64| UNIX_EPOCH + Duration::from_secs(s);
    assert_eq!(perf_log::stamp(at(1_000_000_000)), "20010909-014640");
    assert_eq!(perf_log::stamp(at(1_700_000_000)), "20231114-221320");
    assert_eq!(perf_log::stamp(at(1_709_164_800)), "20240229-000000"); // leap day
    assert!(perf_log::stamp(SystemTime::now()).len() == 15);
}

#[test]
fn capture_collects_this_threads_events_and_the_global_emit_is_a_no_op_without_a_log() {
    perf_log::emit("nobody_listens", json!({}));
    let (r, ev) = perf_log::capture(|| {
        perf_log::emit("a", json!({"x": 1}));
        perf_log::emit("b", json!({}));
        7
    });
    assert_eq!(r, 7);
    assert_eq!(
        ev.iter()
            .map(|e| e["event"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert_eq!(ev[0]["x"], 1);
    let (_, none) = perf_log::capture(|| ());
    assert!(none.is_empty());
}

#[test]
fn helpers_shorten_keys_and_errors() {
    assert_eq!(perf_log::short("0123456789abcdef0123"), "0123456789ab");
    assert_eq!(perf_log::short("ab"), "ab");
    assert_eq!(perf_log::first_line("ps: X3000\nline 2"), "ps: X3000");
    assert_eq!(perf_log::first_line(""), "");
}

#[test]
fn a_gap_is_reported_only_when_the_previous_frame_saw_work_in_flight() {
    let t0 = Instant::now();
    let at = |ms: u64| t0 + Duration::from_millis(ms);
    let mut g = GapWatch::default();
    assert_eq!(
        g.tick(at(0), true),
        None,
        "the first tick has no previous one"
    );
    assert_eq!(
        g.tick(at(99), true),
        None,
        "under 100 ms is a frame, not a freeze"
    );
    assert_eq!(g.tick(at(199), true), Some(100.0), "exactly 100 ms counts");
    assert_eq!(
        g.tick(at(700), false),
        Some(501.0),
        "work was in flight at the previous tick"
    );
    assert_eq!(
        g.tick(at(5_000), false),
        None,
        "idle: egui simply did not run frames"
    );
    assert_eq!(g.tick(at(5_050), true), None);
    assert_eq!(
        g.tick(at(5_050 + 60_000), true),
        None,
        "a minute or more is sleep, not a freeze"
    );
    assert_eq!(g.tick(at(5_050 + 60_000 + 250), false), Some(250.0));
}

#[test]
fn frame_stats_report_once_per_ten_seconds_with_percentiles() {
    let t0 = Instant::now();
    let at = |ms: u64| t0 + Duration::from_millis(ms);
    let mut s = FrameStats::default();
    for i in 0..100u64 {
        // draw 1..=100 ms, total twice that
        assert_eq!(
            s.push(at(i * 50), (i + 1) as f64, 2.0 * (i + 1) as f64),
            None,
            "under 10 s"
        );
    }
    let v = s.push(at(10_000), 101.0, 202.0).expect("10 s passed");
    assert_eq!(v["frames"], 101);
    assert_eq!(v["draw_ms_p50"], 51.0);
    assert_eq!(v["draw_ms_p95"], 96.0);
    assert_eq!(v["total_ms_p95"], 192.0);
    // The window restarts at the next frame.
    assert_eq!(s.push(at(10_100), 1.0, 1.0), None);
    assert_eq!(s.push(at(19_000), 1.0, 1.0), None);
    assert!(s.push(at(20_100), 1.0, 1.0).is_some());
}
