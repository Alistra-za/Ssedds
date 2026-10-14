//! Export and its report (M3a spec §2.5, §2.6, §5.5).
use crate::settings::Bc1Choice;
use engine::api::{BatchSummary, CancelToken};
use engine::codes::{Code, Warning};
use engine::events::{EngineEvent, EventKind, SkipKind};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

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
    /// What the export did besides converting: the JSON, the vanilla/CPM copies, and what was left out.
    pub notes: Vec<String>,
    /// `job_key` to its index in `rows` (the drawer's per-event lookup; rebuilt when rows are removed).
    pub(crate) index: HashMap<String, usize>,
}

fn slash(p: &str) -> String {
    p.replace('\\', "/")
}

impl ExportModel {
    fn row(&mut self, ev: &EngineEvent) -> Option<&mut Row> {
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
        match &ev.kind {
            EventKind::FileSkipped {
                kind: SkipKind::PackedSource,
                ..
            } => {
                // A raw map consumed by a recipe has no job of its own: drop the row its discovery made.
                if let Some(p) = ev.path.as_deref() {
                    let p = slash(p);
                    self.rows
                        .retain(|r| !(r.state == RowState::Queued && slash(&r.path) == p));
                    self.reindex();
                }
            }
            EventKind::FileDiscovered { .. } => {
                self.row(&ev);
            }
            EventKind::FileStarted => self.run_row(&ev, RowState::Running),
            EventKind::FileDone { validation, .. } => {
                let passed = validation.passed;
                self.run_row(&ev, RowState::Done { passed });
            }
            EventKind::FileSkipped {
                kind: SkipKind::NormalLayer,
                code: None,
                ..
            } => {
                // A used layer shows only in the RNM section; one an RNM-* code explains stays visible.
                if let Some(p) = ev.path.as_deref() {
                    let p = slash(p);
                    self.rows
                        .retain(|r| !(r.state == RowState::Queued && slash(&r.path) == p));
                    self.reindex();
                }
            }
            EventKind::FileSkipped { kind, reason, .. } => {
                let state = match kind {
                    SkipKind::UpToDate => RowState::UpToDate,
                    k => RowState::Skipped(*k, reason.clone()),
                };
                if let Some(r) = self.row(&ev) {
                    if r.state.accepts_skip() {
                        r.state = state;
                    }
                }
            }
            EventKind::FileFailed { error, .. } => {
                let e = error.clone();
                self.run_row(&ev, RowState::Failed(e));
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
