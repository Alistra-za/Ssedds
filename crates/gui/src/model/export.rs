//! Export and its report (M3a spec §2.5, §2.6, §5.5).
use crate::settings::Bc1Choice;
use engine::api::{BatchSummary, CancelToken};
use engine::codes::{Code, Warning};
use engine::events::{EngineEvent, EventKind, SkipKind};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub enum RowState {
    Queued,
    Running,
    Done { passed: bool },
    UpToDate,
    Skipped(SkipKind, String),
    Failed(String),
    Cancelled,
}

impl RowState {
    /// The drawer's one-word state; `Skipped` for an undecided texture says why.
    pub fn label(&self) -> String {
        match self {
            RowState::Queued => "queued".into(),
            RowState::Running => "running".into(),
            RowState::Done { passed: true } => "converted".into(),
            RowState::Done { passed: false } => "validation failed".into(),
            RowState::UpToDate => "up to date".into(),
            RowState::Skipped(SkipKind::Unresolved | SkipKind::Blocked, _) => {
                "skipped: needs decision".into()
            }
            RowState::Skipped(_, reason) => format!("skipped: {reason}"),
            RowState::Failed(e) => format!("failed: {e}"),
            RowState::Cancelled => "cancelled".into(),
        }
    }

    /// A skip is provisional: a later run of the same job key (a `PACK-CONFLICT` skip of a pre-packed file shares
    /// the key of the recipe's output) replaces it, but nothing replaces a skip that arrives after a run started.
    fn accepts_skip(&self) -> bool {
        matches!(self, RowState::Queued | RowState::Skipped(..))
    }

    /// The "Failed only" filter: a failed file or a validation failure.
    pub fn is_failed(&self) -> bool {
        matches!(self, RowState::Failed(_) | RowState::Done { passed: false })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub job_key: String,
    pub path: String,
    pub state: RowState,
    pub warnings: Vec<Warning>,
}

#[derive(Default)]
pub struct ExportModel {
    /// The drawer.
    pub open: bool,
    pub running: Option<CancelToken>,
    /// In first-seen order, one per job key.
    pub rows: Vec<Row>,
    /// The whole stream, for JSON Lines export.
    pub events: Vec<EngineEvent>,
    pub summary: Option<BatchSummary>,
    pub error: Option<String>,
    /// What the export did besides converting: the JSON, the vanilla copies, and what was left out.
    pub notes: Vec<String>,
    /// Job key to the file the engine plans to write (or wrote): an absolute path with forward slashes.
    pub outputs: HashMap<String, String>,
    /// Progress timing of the running export (set when it starts).
    pub pace: Option<Pace>,
    /// Changes whenever a row, its state or its warnings change; the Files list caches by it.
    pub revision: u64,
    /// `job_key` to its index in `rows` (the drawer's per-event lookup; rebuilt when rows are removed).
    pub(crate) index: HashMap<String, usize>,
    /// Rows were removed since `index` was built: rebuild it at the next lookup (audit item 6).
    pub(crate) index_stale: bool,
}

fn slash(p: &str) -> String {
    p.replace('\\', "/")
}

/// `a == b` with `\` and `/` equal, without allocating.
fn same_path(a: &str, b: &str) -> bool {
    let sep = |c: u8| c == b'/' || c == b'\\';
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .all(|(x, y)| x == y || (sep(x) && sep(y)))
}

impl ExportModel {
    /// Drop the queued rows of `path` (a raw map packed into a recipe, a used RNM layer).
    fn drop_queued(&mut self, path: &str) {
        let before = self.rows.len();
        self.rows
            .retain(|r| !(r.state == RowState::Queued && same_path(&r.path, path)));
        self.index_stale |= self.rows.len() != before;
    }

    fn row(&mut self, ev: &EngineEvent) -> Option<&mut Row> {
        if self.index_stale {
            self.reindex();
            self.index_stale = false;
        }
        let key = ev.job.clone()?;
        let i = match self.index.get(&key) {
            Some(&i) => i,
            None => {
                self.index.insert(key.clone(), self.rows.len());
                self.rows.push(Row {
                    job_key: key,
                    path: ev.path.clone().unwrap_or_default(),
                    state: RowState::Queued,
                    warnings: vec![],
                });
                self.rows.len() - 1
            }
        };
        Some(&mut self.rows[i])
    }

    fn reindex(&mut self) {
        self.index = self
            .rows
            .iter()
            .enumerate()
            .map(|(i, r)| (r.job_key.clone(), i))
            .collect();
    }

    /// A row for a job that runs: its path is the input the engine reports for the run, which for a packed output
    /// replaces the pre-packed file's path of an earlier `PACK-CONFLICT` skip.
    fn run_row(&mut self, ev: &EngineEvent, state: RowState) {
        let path = ev.path.clone();
        if let Some(r) = self.row(ev) {
            r.state = state;
            if let Some(p) = path {
                r.path = p;
            }
        }
    }

    pub fn on_event(&mut self, ev: EngineEvent) {
        self.on_event_at(ev, Instant::now());
    }

    /// [`Self::on_event`] with the clock given, so the pace can be tested.
    pub fn on_event_at(&mut self, ev: EngineEvent, now: Instant) {
        // Progress and the like change no row; every other event may, and the Files list re-filters on a new revision.
        if !matches!(
            ev.kind,
            EventKind::Progress { .. } | EventKind::BatchStarted { .. }
        ) {
            static REV: AtomicU64 = AtomicU64::new(1);
            self.revision = REV.fetch_add(1, Ordering::Relaxed);
        }
        match &ev.kind {
            EventKind::FileSkipped {
                kind: SkipKind::PackedSource,
                ..
            } => {
                // A raw map consumed by a recipe has no job of its own: drop the row its discovery made.
                if let Some(p) = ev.path.as_deref() {
                    self.drop_queued(p);
                }
            }
            EventKind::FileDiscovered { output } => {
                self.row(&ev);
                self.note_output(&ev, output);
            }
            EventKind::FileStarted => self.run_row(&ev, RowState::Running),
            EventKind::FileDone { validation, output } => {
                let (passed, output) = (validation.passed, output.clone());
                self.run_row(&ev, RowState::Done { passed });
                self.note_output(&ev, &output);
                self.finished_at(now);
            }
            EventKind::FileSkipped {
                kind: SkipKind::NormalLayer,
                code: None,
                ..
            } => {
                // A used layer shows only in the RNM section; one an RNM-* code explains stays visible.
                if let Some(p) = ev.path.as_deref() {
                    self.drop_queued(p);
                }
            }
            EventKind::FileSkipped { kind, reason, .. } => {
                let state = match kind {
                    SkipKind::UpToDate => RowState::UpToDate,
                    k => RowState::Skipped(*k, reason.clone()),
                };
                // A skip is not work: it counts in done/total but not in the files/s rate or the ETA.
                if let Some(r) = self.row(&ev) {
                    if r.state.accepts_skip() {
                        r.state = state;
                    }
                }
            }
            EventKind::FileFailed { error, .. } => {
                let e = error.clone();
                self.run_row(&ev, RowState::Failed(e));
                self.finished_at(now);
            }
            EventKind::Warning(w) => {
                let w = w.clone();
                if let Some(r) = self.row(&ev) {
                    if !r.warnings.contains(&w) {
                        r.warnings.push(w);
                    }
                }
            }
            EventKind::BatchDone { cancelled, .. } => {
                if let Some(p) = self.pace.as_mut() {
                    p.stop(now);
                }
                let cancelled: HashSet<String> = cancelled.iter().map(|c| slash(c)).collect();
                for r in self.rows.iter_mut().filter(|r| {
                    matches!(r.state, RowState::Queued | RowState::Running)
                        && cancelled.contains(&slash(&r.path))
                }) {
                    r.state = RowState::Cancelled;
                }
            }
            _ => {}
        }
        self.events.push(ev);
    }

    fn note_output(&mut self, ev: &EngineEvent, output: &str) {
        if let Some(k) = &ev.job {
            self.outputs.insert(k.clone(), output.to_string());
        }
    }

    fn finished_at(&mut self, now: Instant) {
        if let Some(p) = self.pace.as_mut() {
            p.finished(now);
        }
    }

    /// Warnings grouped by code: code → job keys (M3a spec §2.5).
    pub fn by_code(&self) -> BTreeMap<Code, Vec<String>> {
        let mut m: BTreeMap<Code, Vec<String>> = BTreeMap::new();
        for r in &self.rows {
            for w in &r.warnings {
                let keys = m.entry(w.code).or_default();
                if !keys.contains(&r.job_key) {
                    keys.push(r.job_key.clone());
                }
            }
        }
        m
    }

    pub fn to_jsonl(&self) -> String {
        self.events
            .iter()
            .filter_map(|e| serde_json::to_string(e).ok())
            .map(|l| l + "\n")
            .collect()
    }

    pub fn to_markdown(&self) -> String {
        let mut s = String::from("# Tessalith export report\n\n");
        if let Some(b) = &self.summary {
            s += &format!(
                "Converted {}, up to date {}, skipped {}, failed {}, validation failed {}.\n\n",
                b.converted, b.up_to_date, b.skipped, b.failed, b.validation_failed
            );
        }
        s += "## Warnings by code\n\n";
        for (code, keys) in self.by_code() {
            s += &format!(
                "- **{}** ({}): {}\n",
                code.as_str(),
                keys.len(),
                keys.join(", ")
            );
        }
        s += "\n## Files\n\n| Output | State |\n|---|---|\n";
        for r in &self.rows {
            s += &format!("| {} | {} |\n", r.job_key, r.state.label());
        }
        s
    }
}

fn norm(p: &Path) -> String {
    std::path::absolute(p)
        .unwrap_or_else(|_| p.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_lowercase()
}

/// Review Focus 2: discovery skips everything under the output root, so the export folder must not be the opened
/// folder or contain it. A subfolder of the opened folder is fine.
pub fn export_folder_problem(root: &Path, export: &Path) -> Option<String> {
    let (r, e) = (norm(root), norm(export));
    (r == e || r.starts_with(&format!("{e}/"))).then(|| {
        "The export folder must not be the opened folder or contain it: Tessalith would not find the sources next time. \
         Choose another folder."
            .to_string()
    })
}

/// The `sdc convert` command that reproduces the export from the saved `sdc.toml` (M3a spec §2.5).
pub fn cli_command(root: &Path, export: &Path, bc1: Bc1Choice, jobs: usize) -> String {
    let q = |p: &Path| format!("\"{}\"", p.to_string_lossy().replace('\\', "/"));
    let mut c = format!(
        "sdc convert {} --recursive -o {} --bc1-decoder {}",
        q(root),
        q(export),
        bc1.cli()
    );
    if jobs > 0 {
        c += &format!(" --jobs {jobs}");
    }
    c
}

// ---- Task 5: progress line, Files filters and caches (DECISIONS AG35-AG38) ----

/// The files/s average looks back this far.
pub const RATE_WINDOW: Duration = Duration::from_secs(10);
/// No ETA before this many files have finished ...
pub const ETA_MIN_FILES: usize = 3;
/// ... or this much time has passed.
pub const ETA_MIN_TIME: Duration = Duration::from_secs(2);

/// The timing of one export: when it started and when each file finished. Every method takes the time as an argument
/// (the model passes `Instant::now()`), so the rules are tested with a fake clock.
#[derive(Debug, Clone)]
pub struct Pace {
    start: Instant,
    end: Option<Instant>,
    /// Finish times of the last `RATE_WINDOW`, oldest first.
    recent: VecDeque<Instant>,
}

impl Pace {
    pub fn new(start: Instant) -> Self {
        Pace {
            start,
            end: None,
            recent: VecDeque::new(),
        }
    }

    pub fn finished(&mut self, now: Instant) {
        self.recent.push_back(now);
        while self
            .recent
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) > RATE_WINDOW)
        {
            self.recent.pop_front();
        }
    }

    /// The export ended: the elapsed time stops.
    pub fn stop(&mut self, now: Instant) {
        self.end.get_or_insert(now);
    }

    pub fn elapsed(&self, now: Instant) -> Duration {
        self.end
            .unwrap_or(now)
            .saturating_duration_since(self.start)
    }

    /// Files finished per second over the last `RATE_WINDOW` (or since the start, while that is shorter).
    pub fn rate(&self, now: Instant) -> f64 {
        let span = self.elapsed(now).min(RATE_WINDOW).as_secs_f64();
        let n = self
            .recent
            .iter()
            .filter(|t| now.saturating_duration_since(**t) <= RATE_WINDOW)
            .count();
        if span <= 0.0 {
            0.0
        } else {
            n as f64 / span
        }
    }

    /// `None` until `ETA_MIN_FILES` have finished or `ETA_MIN_TIME` has passed, and while nothing finished lately.
    pub fn eta(&self, done: usize, total: usize, now: Instant) -> Option<Duration> {
        if done < ETA_MIN_FILES && self.elapsed(now) < ETA_MIN_TIME {
            return None;
        }
        let rate = self.rate(now);
        (rate > 0.0).then(|| Duration::from_secs_f64(total.saturating_sub(done) as f64 / rate))
    }

    /// `done/total · elapsed · files/s · ETA`; the ETA is left out while it is `None`.
    pub fn line(&self, done: usize, total: usize, now: Instant) -> String {
        let mut s = format!(
            "{done}/{total} · {} · {:.1} files/s",
            fmt_duration(self.elapsed(now)),
            self.rate(now)
        );
        if let Some(eta) = self.eta(done, total, now) {
            s += &format!(" · ETA {}", fmt_duration(eta));
        }
        s
    }
}

/// `m:ss`, or `h:mm:ss` from an hour on; a fraction of a second rounds up (an ETA never reads 0:00 early).
pub fn fmt_duration(d: Duration) -> String {
    let s = d.as_secs_f64().ceil() as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// The drawer's two Files filters; both on means both must hold.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileFilter {
    pub failed_only: bool,
    pub has_warnings: bool,
}

impl FileFilter {
    pub fn any(self) -> bool {
        self.failed_only || self.has_warnings
    }

    pub fn matches(self, r: &Row) -> bool {
        (!self.failed_only || r.state.is_failed()) && (!self.has_warnings || !r.warnings.is_empty())
    }
}

/// The revision, every (row, warning) pair by code, and the per-code ranges over it.
type Warns = (u64, Vec<(usize, usize)>, Vec<(Code, usize, usize)>);

/// The row indices the Files tab shows and the warnings the Warnings tab lists, cached by what they were computed
/// from: the filter and the export's `revision`. A frame that changed neither only reads the cache.
#[derive(Default)]
pub struct FilesIndex {
    files: Option<(FileFilter, u64, Vec<usize>)>,
    warns: Option<Warns>,
    /// How many times the filter / the warning list was computed (tests).
    pub computed: u32,
}

impl FilesIndex {
    /// How many rows pass `f`; recomputes the index only when `f` or the export changed.
    pub fn len(&mut self, m: &ExportModel, f: FileFilter) -> usize {
        if !f.any() {
            return m.rows.len();
        }
        let stale = !matches!(&self.files, Some((cf, rev, _)) if *cf == f && *rev == m.revision);
        if stale {
            self.computed += 1;
            let idx = (0..m.rows.len())
                .filter(|&i| f.matches(&m.rows[i]))
                .collect();
            self.files = Some((f, m.revision, idx));
        }
        self.files.as_ref().map_or(0, |(_, _, v)| v.len())
    }

    /// The `i`th row that passes `f`; call [`Self::len`] first in the frame.
    pub fn row<'a>(&self, m: &'a ExportModel, f: FileFilter, i: usize) -> Option<&'a Row> {
        if !f.any() {
            return m.rows.get(i);
        }
        let (_, _, idx) = self.files.as_ref()?;
        m.rows.get(*idx.get(i)?)
    }

    /// Every warning as (row, warning) indices, by code and then by row; cached by the revision.
    pub fn warnings(&mut self, m: &ExportModel) -> &[(usize, usize)] {
        self.refresh_warnings(m);
        self.warns.as_ref().map_or(&[], |(_, v, _)| v)
    }

    /// One `(code, start, end)` per code over [`Self::warnings`]: `end - start` warnings carry the code.
    pub fn groups(&mut self, m: &ExportModel) -> &[(Code, usize, usize)] {
        self.refresh_warnings(m);
        self.warns.as_ref().map_or(&[], |(_, _, g)| g)
    }

    fn refresh_warnings(&mut self, m: &ExportModel) {
        if matches!(&self.warns, Some((rev, ..)) if *rev == m.revision) {
            return;
        }
        self.computed += 1;
        let mut v: Vec<(usize, usize)> = m
            .rows
            .iter()
            .enumerate()
            .flat_map(|(r, row)| (0..row.warnings.len()).map(move |w| (r, w)))
            .collect();
        v.sort_by_key(|&(r, w)| m.rows[r].warnings[w].code);
        let mut groups: Vec<(Code, usize, usize)> = vec![];
        for (i, &(r, w)) in v.iter().enumerate() {
            let code = m.rows[r].warnings[w].code;
            match groups.last_mut() {
                Some(g) if g.0 == code => g.2 = i + 1,
                _ => groups.push((code, i, i + 1)),
            }
        }
        self.warns = Some((m.revision, v, groups));
    }
}

/// A warning in the Warnings tab: `CODE (severity): message`.
pub fn warning_line(w: &Warning) -> String {
    format!(
        "{} ({}): {}",
        w.code.as_str(),
        format!("{:?}", w.severity).to_lowercase(),
        w.message
    )
}
