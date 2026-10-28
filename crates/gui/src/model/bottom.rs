//! The bottom panel's state (layout v2 A1, A9; R7, R8): which tab shows, and whether the panel is open.
use super::items::Decision;
use super::{checks, counts_toward_badge, pbr_export, sets, AppModel, ItemStatus};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BottomTab {
    Problems,
    Review,
    Messages,
    #[default]
    Files,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct BottomPanel {
    pub open: bool,
    pub tab: BottomTab,
    pub filter: Option<ProblemsFilter>,
}

impl AppModel {
    /// Open the panel on `tab`.
    pub fn show_tab(&mut self, tab: BottomTab) {
        self.bottom = BottomPanel {
            open: true,
            tab,
            filter: None,
        };
        self.sync_messages_open();
    }

    /// Open on `tab`, or collapse when the panel already shows it.
    pub fn toggle_tab(&mut self, tab: BottomTab) {
        if self.bottom.open && self.bottom.tab == tab {
            self.bottom.open = false;
        } else {
            self.bottom = BottomPanel {
                open: true,
                tab,
                filter: None,
            };
        }
        self.sync_messages_open();
    }

    pub fn collapse_bottom(&mut self) {
        self.bottom.open = false;
        self.sync_messages_open();
    }

    /// R8: `Messages::is_open` means "the Messages tab is showing".
    fn sync_messages_open(&mut self) {
        let showing = self.bottom.open && self.bottom.tab == BottomTab::Messages;
        self.messages.set_open(showing);
    }
}

/// Red blocks the export; the others are orange in the list (A12). `Check` rows (artist checks, DECISIONS Note CL)
/// are listed but never change "ready" (`counts_as_ready_problem`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Blocks,
    Decide,
    Warning,
    Check,
}

/// Whether a row takes "ready" away: every row but an artist check's (owner answer 1).
pub fn counts_as_ready_problem(p: &Problem) -> bool {
    p.level != Level::Check
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum At {
    /// An item key.
    Texture(String),
    Set {
        key: String,
        id: String,
    },
    /// A job key.
    File(String),
    Project,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub level: Level,
    pub at: At,
    pub text: String,
    pub code: Option<engine::codes::Code>,
    /// Note DU3: the set entry that matches no file, which the row's fix button removes.
    pub missing: Option<sets::Missing>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProblemsFilter {
    /// An engine set key.
    Set(String),
    Undecided,
    /// Only the artist checks' rows.
    Checks,
    /// Only the artist checks' rows of one set (an engine set key): "Checks {n}" pressed while `Set` is on.
    SetChecks(String),
}

impl AppModel {
    /// Open the Problems tab, narrowed to `f`.
    pub fn open_problems(&mut self, f: Option<ProblemsFilter>) {
        self.show_tab(BottomTab::Problems);
        self.bottom.filter = f;
    }
}

/// Every problem of the project: blocks, then decisions, then warnings, then checks, then the last export's File rows
/// (warnings, or checks for an engine `CHK-*` code). Cached (K9) under the views key plus
/// what the asks and the File rows read; the build holds no `RefCell` borrow.
pub fn problems(m: &AppModel) -> Arc<Vec<Problem>> {
    m.problem_lookups.set(m.problem_lookups.get() + 1);
    let key = m.view_key();
    let inner = problems_inner(m);
    {
        let c = m.views.borrow();
        if c.key.as_ref() == Some(&key) {
            if let Some((k, p)) = &c.problems {
                if *k == inner {
                    return p.clone();
                }
            }
        }
    }
    let p = Arc::new(build_problems(m));
    m.problem_builds.set(m.problem_builds.get() + 1);
    let mut c = m.views.borrow_mut();
    if c.key.as_ref() != Some(&key) {
        *c = super::ViewCache {
            key: Some(key),
            ..Default::default()
        };
    }
    c.problems = Some((inner, p.clone()));
    c.flagged = None;
    p
}

/// What `problems` is keyed on inside the view-key cell; `export_files` shares it.
pub(super) fn problems_inner(m: &AppModel) -> super::ProblemsKey {
    (
        m.evidence_rev,
        m.export.revision,
        m.export.running.is_some(),
    )
}

/// The set keys and texture keys that carry a problem that takes "ready" away, and the row counts by that test.
#[derive(Debug, Default)]
pub struct Flagged {
    pub sets: HashSet<String>,
    pub textures: HashSet<String>,
    /// Rows passing `counts_as_ready_problem` (the Export dot and its hover, K6).
    pub ready_rows: usize,
    /// `Level::Check` rows (the Problems tab's "Checks {n}").
    pub check_rows: usize,
}

/// `Flagged`, built once per `problems` build (R-a: a set card asks every frame). Fetch it once per frame and ask
/// `set_ready_in` per card (Task 6).
pub fn flagged(m: &AppModel) -> Arc<Flagged> {
    let all = problems(m);
    if let Some(f) = &m.views.borrow().flagged {
        return f.clone();
    }
    let mut f = Flagged::default();
    for p in all.iter() {
        if !counts_as_ready_problem(p) {
            f.check_rows += 1;
            continue;
        }
        f.ready_rows += 1;
        match &p.at {
            At::Set { key, .. } => f.sets.insert(key.clone()),
            At::Texture(t) => f.textures.insert(t.clone()),
            _ => false,
        };
    }
    let f = Arc::new(f);
    m.views.borrow_mut().flagged = Some(f.clone());
    f
}

/// A set card shows a green dot: a real set whose own rows and members' rows are all absent from Problems.
pub fn set_ready_in(f: &Flagged, v: &sets::SetView) -> bool {
    !v.key.is_empty()
        && !f.sets.contains(&v.key)
        && !v.members.iter().any(|t| f.textures.contains(t))
}

/// `set_ready_in` with its own `flagged` fetch; a loop over sets fetches once and calls `set_ready_in`.
pub fn set_ready(m: &AppModel, v: &sets::SetView) -> bool {
    set_ready_in(&flagged(m), v)
}

/// A tile shows a green dot: ready, with nothing that counts toward its badge.
pub fn tile_ready(item: &super::items::TextureItem) -> bool {
    item.status == ItemStatus::Ready && !item.warnings.iter().any(counts_toward_badge)
}

#[doc(hidden)]
pub fn filtered_builds(m: &AppModel) -> u64 {
    m.filter_builds.get()
}

#[doc(hidden)]
pub fn problems_builds(m: &AppModel) -> u64 {
    m.problem_builds.get()
}

/// Every call to `problems()`, cached or not (A6).
#[doc(hidden)]
pub fn problem_lookups(m: &AppModel) -> u64 {
    m.problem_lookups.get()
}

/// `AppModel::export_files` counts that were not served from the cache.
#[doc(hidden)]
pub fn export_files_builds(m: &AppModel) -> u64 {
    m.export_files_builds.get()
}

fn build_problems(m: &AppModel) -> Vec<Problem> {
    let views = sets::views(m);
    let set_at = |key: &str, id: &str| At::Set {
        key: key.into(),
        id: id.into(),
    };
    let row = |level, at, text: String, code| Problem {
        level,
        at,
        text,
        code,
        missing: None,
    };
    let mut out = vec![];
    for b in pbr_export::export_blocks(m) {
        let at = if b.set_key.is_empty() {
            At::Project
        } else {
            set_at(&b.set_key, &b.set_id)
        };
        out.push(row(Level::Blocks, at, b.text, None));
    }
    for i in &m.items {
        let text = match &i.status {
            ItemStatus::Failed(r) => format!("Failed: {r}"),
            ItemStatus::Error(cs) => {
                let cs: Vec<_> = cs.iter().map(|c| c.as_str()).collect();
                format!("Failed: {}", cs.join(", "))
            }
            _ => continue,
        };
        out.push(row(Level::Blocks, At::Texture(i.key.clone()), text, None));
    }
    for i in m
        .items
        .iter()
        .filter(|i| i.status == ItemStatus::NeedsDecision)
    {
        let parts: Vec<String> = i
            .decisions
            .iter()
            .map(|d| match d {
                Decision::Subtype { .. } => "sub-type".into(),
                Decision::AlphaMeaning => "what its alpha means".into(),
                Decision::Acknowledge(c) => format!("acknowledge {}", c.as_str()),
            })
            .collect();
        let text = format!("Needs a decision: {}", parts.join("; "));
        out.push(row(Level::Decide, At::Texture(i.key.clone()), text, None));
    }
    for a in pbr_export::export_asks(m) {
        let at = set_at(&a.set_key, &a.set_id);
        out.push(row(Level::Warning, at, a.text, None));
    }
    for i in m
        .items
        .iter()
        .filter(|i| i.status != ItemStatus::NeedsDecision)
    {
        for w in i.warnings.iter().filter(|w| counts_toward_badge(w)) {
            let at = At::Texture(i.key.clone());
            out.push(row(Level::Warning, at, w.message.clone(), Some(w.code)));
        }
    }
    for v in views.iter() {
        for w in v.set_warnings.iter().filter(|w| counts_toward_badge(w)) {
            let at = set_at(&v.key, &v.id);
            out.push(row(Level::Warning, at, w.message.clone(), Some(w.code)));
        }
    }
    // Note DU3: a set entry that matches no file, with its removal as the fix.
    for v in views.iter() {
        for miss in &v.missing {
            let text = match miss {
                sets::Missing::NoFilesLeft => "No texture of this set is left in the folder".into(),
                _ => format!("No file matches {}", miss.text()),
            };
            out.push(Problem {
                missing: Some(miss.clone()),
                ..row(Level::Warning, set_at(&v.key, &v.id), text, None)
            });
        }
    }
    for (k, ws) in &m.rnm_notes {
        let at = views
            .iter()
            .find(|v| v.key == *k)
            .map_or(At::Project, |v| set_at(&v.key, &v.id));
        for w in ws.iter().filter(|w| counts_toward_badge(w)) {
            let (at, text) = (at.clone(), w.message.clone());
            out.push(row(Level::Warning, at, text, Some(w.code)));
        }
    }
    // Artist checks (DECISIONS Note CL): listed, never "ready"-changing; Info results stay on hover (owner answer 2).
    for i in m
        .items
        .iter()
        .filter(|i| i.status != ItemStatus::NeedsDecision)
    {
        for w in m
            .check_warnings(&i.key)
            .iter()
            .filter(|w| counts_toward_badge(w))
        {
            let at = At::Texture(i.key.clone());
            out.push(row(Level::Check, at, w.message.clone(), Some(w.code)));
        }
    }
    for v in views.iter() {
        for w in m
            .set_check_warnings(&v.key)
            .iter()
            .filter(|w| counts_toward_badge(w))
        {
            let at = set_at(&v.key, &v.id);
            out.push(row(Level::Check, at, w.message.clone(), Some(w.code)));
        }
    }
    // Rows only count once the export has ended: `revision` bumps on every engine event (R9).
    if m.export.running.is_none() {
        // K2: an engine CHK-* row is a check row and follows its switch. The switches are not in `ProblemsKey`:
        // a switch change reaches this build through `refresh_checks`' `invalidate_views()`.
        let on = &m.settings.checks;
        // Note DP4: the live item row is the current truth and carries the fix, so a same-code copy of it from
        // the last export is dropped. Only when the job maps to exactly one item (None = several share a job_key).
        // Built once, and only when some export row has a counted warning (no per-row scan of all items).
        let mut owners: Option<HashMap<&str, Option<&super::items::TextureItem>>> = None;
        for r in &m.export.rows {
            if !r.warnings.iter().any(counts_toward_badge) {
                continue;
            }
            let owners = owners.get_or_insert_with(|| {
                let mut map = HashMap::new();
                for i in &m.items {
                    map.entry(i.job_key.as_str())
                        .and_modify(|e| *e = None)
                        .or_insert(Some(i));
                }
                map
            });
            let live = owners.get(r.job_key.as_str()).copied().flatten();
            // P11 (owner answer 2): Info stays on the row's hover, as for item and set rows (`DERIVE-*` included).
            for w in r.warnings.iter().filter(|w| counts_toward_badge(w)) {
                if live.is_some_and(|i| {
                    i.status != ItemStatus::NeedsDecision
                        && i.warnings.iter().any(|x| x.code == w.code)
                }) {
                    continue;
                }
                let level = match checks::check_of(w.code) {
                    Some(c) if !checks::switch_on(on, c) => continue,
                    Some(_) => Level::Check,
                    None => Level::Warning,
                };
                let at = At::File(r.job_key.clone());
                out.push(row(level, at, w.message.clone(), Some(w.code)));
            }
        }
    }
    out
}

/// The list narrowed by `f`: a set keeps its own rows and its members' rows; `Undecided` keeps the Decide rows.
/// `None` is `problems(m)` itself, not a copy.
pub fn filtered(m: &AppModel, f: Option<&ProblemsFilter>) -> Arc<Vec<Problem>> {
    m.filter_builds.set(m.filter_builds.get() + 1);
    let all = problems(m);
    Arc::new(match f {
        None => return all,
        Some(ProblemsFilter::Undecided) => all
            .iter()
            .filter(|p| p.level == Level::Decide)
            .cloned()
            .collect(),
        Some(ProblemsFilter::Checks) => all
            .iter()
            .filter(|p| p.level == Level::Check)
            .cloned()
            .collect(),
        Some(ProblemsFilter::Set(k)) => {
            let members = set_members(m, k);
            all.iter()
                .filter(|p| in_set(p, k, &members))
                .cloned()
                .collect()
        }
        Some(ProblemsFilter::SetChecks(k)) => {
            let members = set_members(m, k);
            all.iter()
                .filter(|p| p.level == Level::Check && in_set(p, k, &members))
                .cloned()
                .collect()
        }
    })
}

fn set_members(m: &AppModel, key: &str) -> HashSet<String> {
    sets::views(m)
        .iter()
        .find(|v| v.key == key)
        .map(|v| v.members.iter().cloned().collect())
        .unwrap_or_default()
}

fn in_set(p: &Problem, key: &str, members: &HashSet<String>) -> bool {
    match &p.at {
        At::Set { key: k, .. } => k == key,
        At::Texture(t) => members.contains(t),
        _ => false,
    }
}

/// How many problems `filtered(m, Some(Set(key)))` lists, without cloning them.
pub fn set_problem_count(m: &AppModel, key: &str) -> usize {
    let members = set_members(m, key);
    problems(m)
        .iter()
        .filter(|p| in_set(p, key, &members))
        .count()
}
