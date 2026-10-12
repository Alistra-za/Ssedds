//! The preview timing log: JSON Lines in `%LOCALAPPDATA%\sdc\logs\preview-timings-<UTC stamp>.jsonl`, one file
//! per app run, the 10 newest kept, 20 MiB cap per file. Events go through a bounded channel to one writer
//! thread, so neither the UI thread nor the render thread ever waits for the disk; when the channel is full the
//! event is dropped and counted (`run_end.dropped`). `SDC_PREVIEW_LOG=0` turns it off. The log is how the owner
//! reports timings (Task 10): the app measures, the owner sends the file.
use serde_json::{json, Map, Value};
use std::cell::RefCell;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const PREFIX: &str = "preview-timings-";
const EXT: &str = ".jsonl";
/// A UI frame gap at or above this, with work in flight, is a freeze worth a line.
pub const GAP_MS: f64 = 100.0;
/// Longer gaps are the machine sleeping or the window minimised, not a freeze.
const GAP_MAX_MS: f64 = 60_000.0;
const STATS_EVERY: Duration = Duration::from_secs(10);

pub struct Opts {
    pub max_files: usize,
    pub max_bytes: u64,
    pub queue: usize,
}

impl Default for Opts {
    fn default() -> Opts {
        Opts {
            max_files: 10,
            max_bytes: 20 * 1024 * 1024,
            queue: 8192,
        }
    }
}

enum Msg {
    Line(String),
    Stop,
}

pub struct PerfLog {
    start: Instant,
    path: PathBuf,
    tx: SyncSender<Msg>,
    dropped: Arc<AtomicU64>,
    writer: Option<JoinHandle<()>>,
}

fn r3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

fn object(event: &str, t_ms: f64, fields: Value) -> Value {
    let mut m = Map::new();
    m.insert("t_ms".into(), json!(r3(t_ms)));
    m.insert("event".into(), json!(event));
    if let Value::Object(f) = fields {
        m.extend(f);
    }
    Value::Object(m)
}

fn line_of(event: &str, start: Instant, fields: Value) -> String {
    let mut s = serde_json::to_string(&object(
        event,
        start.elapsed().as_secs_f64() * 1000.0,
        fields,
    ))
    .unwrap_or_default();
    s.push('\n');
    s
}

impl PerfLog {
    /// Creates `preview-timings-<stamp>.jsonl` in `dir` (a `_<n>` suffix when that name is taken) after deleting
    /// the oldest logs so that at most `max_files` remain, the new one included. Only names of this shape are
    /// ever deleted.
    pub fn open(dir: &Path, stamp: &str, o: &Opts) -> std::io::Result<PerfLog> {
        fs::create_dir_all(dir)?;
        let mut old: Vec<PathBuf> = fs::read_dir(dir)?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(PREFIX) && n.ends_with(EXT))
            })
            .collect();
        old.sort();
        let excess = old.len().saturating_sub(o.max_files.saturating_sub(1));
        for p in &old[..excess] {
            let _ = fs::remove_file(p);
        }
        let (path, file) = (0..1000)
            .find_map(|n| {
                let name = if n == 0 {
                    format!("{PREFIX}{stamp}{EXT}")
                } else {
                    format!("{PREFIX}{stamp}_{n}{EXT}")
                };
                let p = dir.join(name);
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&p)
                    .ok()
                    .map(|f| (p, f))
            })
            .ok_or_else(|| std::io::Error::other("no free log file name"))?;
        let (tx, rx) = sync_channel(o.queue);
        let dropped = Arc::new(AtomicU64::new(0));
        let start = Instant::now();
        let writer = std::thread::Builder::new().name("perf-log".into()).spawn({
            let (dropped, cap) = (dropped.clone(), o.max_bytes);
            move || write_loop(rx, BufWriter::new(file), cap, &dropped, start)
        })?;
        Ok(PerfLog {
            start,
            path,
            tx,
            dropped,
            writer: Some(writer),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Queues one event; never blocks. A full queue drops it and counts it.
    pub fn emit(&self, event: &str, fields: Value) {
        let line = line_of(event, self.start, fields);
        if let Err(TrySendError::Full(_)) = self.tx.try_send(Msg::Line(line)) {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl Drop for PerfLog {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Stop);
        if let Some(h) = self.writer.take() {
            let _ = h.join();
        }
    }
}

/// Writes batches and flushes when the queue is empty, so a killed app still leaves its lines on disk. Past the
/// cap: one `log_capped` line, then nothing (not even `run_end`).
fn write_loop(
    rx: Receiver<Msg>,
    mut out: BufWriter<File>,
    cap: u64,
    dropped: &AtomicU64,
    start: Instant,
) {
    let mut bytes = 0u64;
    let mut capped = false;
    let mut put = |out: &mut BufWriter<File>, line: &str| {
        if capped {
            return;
        }
        if bytes + line.len() as u64 > cap {
            capped = true;
            let l = line_of(
                "log_capped",
                start,
                json!({"bytes": bytes, "dropped": dropped.load(Ordering::Relaxed)}),
            );
            let _ = out.write_all(l.as_bytes());
        } else {
            bytes += line.len() as u64;
            let _ = out.write_all(line.as_bytes());
        }
    };
    while let Ok(first) = rx.recv() {
        for m in std::iter::once(first).chain(rx.try_iter()) {
            match m {
                Msg::Line(l) => put(&mut out, &l),
                Msg::Stop => {
                    let l = line_of(
                        "run_end",
                        start,
                        json!({"dropped": dropped.load(Ordering::Relaxed)}),
                    );
                    put(&mut out, &l);
                    let _ = out.flush();
                    return;
                }
            }
        }
        let _ = out.flush();
    }
    let _ = out.flush();
}

/// `switch` is the value of `SDC_PREVIEW_LOG`; `"0"` is off, anything else (or unset) is on.
pub fn open_if_enabled(dir: &Path, stamp: &str, switch: Option<&str>) -> Option<PerfLog> {
    if switch == Some("0") {
        return None;
    }
    PerfLog::open(dir, stamp, &Opts::default()).ok()
}

static GLOBAL: Mutex<Weak<PerfLog>> = Mutex::new(Weak::new());

thread_local! {
    static SINK: RefCell<Option<Vec<Value>>> = const { RefCell::new(None) };
}

/// Opens the process log in `%LOCALAPPDATA%\sdc\logs` unless `SDC_PREVIEW_LOG=0` (or no `%LOCALAPPDATA%`).
/// The caller keeps the returned `Arc` alive for the whole run: dropping the last one writes `run_end`.
pub fn start() -> Option<Arc<PerfLog>> {
    if std::env::var("SDC_PREVIEW_LOG").as_deref() == Ok("0") {
        return None;
    }
    let dir =
        std::env::var_os("LOCALAPPDATA").map(|a| PathBuf::from(a).join("sdc").join("logs"))?;
    start_in(&dir)
}

/// [`start`] with an explicit directory and no environment switch. A second call while a log is alive returns it.
pub fn start_in(dir: &Path) -> Option<Arc<PerfLog>> {
    let mut g = GLOBAL.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(l) = g.upgrade() {
        return Some(l);
    }
    let l = Arc::new(open_if_enabled(dir, &stamp(SystemTime::now()), None)?);
    *g = Arc::downgrade(&l);
    drop(g);
    l.emit("run_start", json!({"version": env!("CARGO_PKG_VERSION")}));
    Some(l)
}

/// The global emit: this thread's capture if one is active, else the live log, else nothing.
pub fn emit(event: &str, fields: Value) {
    let captured = SINK.with(|s| match s.borrow_mut().as_mut() {
        Some(v) => {
            v.push(object(event, 0.0, fields.clone()));
            true
        }
        None => false,
    });
    if captured {
        return;
    }
    let live = GLOBAL
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .upgrade();
    if let Some(l) = live {
        l.emit(event, fields);
    }
}

/// The live log file, for the panel.
pub fn path() -> Option<PathBuf> {
    let live = GLOBAL
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .upgrade();
    live.map(|l| l.path.clone())
}

/// Test seam: everything `emit` is given on this thread while `f` runs (events from other threads are not seen).
#[doc(hidden)]
pub fn capture<R>(f: impl FnOnce() -> R) -> (R, Vec<Value>) {
    SINK.with(|s| *s.borrow_mut() = Some(vec![]));
    let r = f();
    let events = SINK.with(|s| s.borrow_mut().take()).unwrap_or_default();
    (r, events)
}

/// UTC `YYYYMMDD-HHMMSS` (civil-from-days, no time crate).
pub fn stamp(t: SystemTime) -> String {
    let s = t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rem) = ((s / 86_400) as i64, s % 86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// The first 12 characters of a program key.
pub fn short(key: &str) -> String {
    key.chars().take(12).collect()
}

pub fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").to_string()
}

pub fn ms_since(t: Instant) -> f64 {
    r3(t.elapsed().as_secs_f64() * 1000.0)
}

/// The UI-thread freeze detector: call once per egui frame, after its replies are drained, with `busy` = "work is
/// still in flight" (the condition that asks egui for the next frame). Returns the gap
/// in ms when this frame came 100 ms or more (and under a minute) after the previous one AND the previous frame
/// had work in flight (so egui was being asked to repaint). Idle gaps are normal and never reported.
#[derive(Debug, Default)]
pub struct GapWatch {
    last: Option<Instant>,
    expect: bool,
}

impl GapWatch {
    pub fn tick(&mut self, now: Instant, busy: bool) -> Option<f64> {
        let gap = self
            .last
            .filter(|_| self.expect)
            .map(|t| now.saturating_duration_since(t).as_micros() as f64 / 1000.0)
            .filter(|g| (GAP_MS..GAP_MAX_MS).contains(g));
        self.last = Some(now);
        self.expect = busy;
        gap
    }

    /// Replaces this frame's `busy` with the owner's, taken after the owner's own work was polled.
    pub fn arm(&mut self, busy: bool) {
        self.expect = busy;
    }
}

/// Per-frame draw and total times, summarised once per `STATS_EVERY`.
#[derive(Debug, Default)]
pub struct FrameStats {
    since: Option<Instant>,
    draw: Vec<f64>,
    total: Vec<f64>,
}

fn pct(v: &mut [f64], q: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    r3(v[((v.len() - 1) as f64 * q).round() as usize])
}

impl FrameStats {
    pub fn push(&mut self, now: Instant, draw_ms: f64, total_ms: f64) -> Option<Value> {
        let since = *self.since.get_or_insert(now);
        self.draw.push(draw_ms);
        self.total.push(total_ms);
        if now.saturating_duration_since(since) < STATS_EVERY {
            return None;
        }
        let v = json!({
            "frames": self.draw.len(),
            "draw_ms_p50": pct(&mut self.draw, 0.5),
            "draw_ms_p95": pct(&mut self.draw, 0.95),
            "total_ms_p95": pct(&mut self.total, 0.95),
        });
        self.draw.clear();
        self.total.clear();
        self.since = None;
        Some(v)
    }
}
