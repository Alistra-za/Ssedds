//! The bottom panel's state (layout v2 A1, A9; R7, R8): which tab shows, and whether the panel is open.
use super::items::Decision;
use super::meshes::{NifLevel, NifTab};
use super::{checks, code_text, counts_toward_badge, pbr_export, sets, AppModel, ItemStatus};
use crate::pgp::settings::Feature;
use engine::codes::Code;
use nif::fields::Field;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
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

/// Red blocks the export; Decide and Warning are amber (A12). `Check` rows (artist checks, DECISIONS Note CL) and
/// `Info` rows are drawn weak and never change "ready" (`counts_as_ready_problem`, Note FC3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    Blocks,
    Decide,
    Warning,
    Check,
    /// Note FC3: a result with an action and no consequence for the export (a dropped fill, a cleared format).
    Info,
}

/// Whether a row takes "ready" away: every row but an artist check's (owner answer 1) and an Info row (Note FC3).
pub fn counts_as_ready_problem(p: &Problem) -> bool {
    !matches!(p.level, Level::Check | Level::Info)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
    /// Note FC3: a NIF, by its data-relative path.
    Nif(String),
}

/// Note FC3: a row's action, drawn as its button. Each arm is built by the row kind that needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fix {
    /// Note DU3: remove the set entry that matches no file.
    RemoveMissing(sets::Missing),
    /// GEN-STALE (CO70): open the Generate dialog for this generated item's map; its own Generate writes.
    Regenerate(String),
    /// CHAN-NORMAL-LENGTH (Note DP): set `channels.reconstruct_z` on this item.
    RebuildZ(String),
    /// DERIVE-* (CN67): open the Also export's conversion report.
    OpenReport(PathBuf),
    /// DERIVE-* with no report written: open the Messages tab, which says why.
    ShowMessages,
    /// IN-UNREADABLE (Y4): open the folder that holds the skipped entry.
    OpenFolder(PathBuf),
    /// CHK-GREEN-GUESS (CL5): answer this item's green convention as OpenGL.
    TreatAsOpenGl(String),
    /// P26: turn one of two exclusive features of a set (by id) off.
    TurnFeatureOff(String, Feature),
    /// P13: go to the set (by key) and focus the path field (a settings key) the blocker names.
    FixPath(String, String),
    /// R-fills-2, Z11: undo the step that caused the row; only built while that step is the top of the history.
    UndoLast,
    /// Note GD1: open the NIF on this tab and field.
    ShowNifTab(String, NifTab, Option<Field>),
    /// Note FC1 (AM14): open the header's Preset menu.
    ChoosePreset,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub level: Level,
    pub at: At,
    pub text: String,
    pub code: Option<engine::codes::Code>,
    /// Codes the text names in words (a failed texture's, a question's); only the row's hover prints them (Note FB8).
    pub also: Vec<engine::codes::Code>,
    /// Note FC3: the row's action button (the Decide rows' buttons and `Go to set` are drawn from the level and `at`).
    pub fix: Option<Fix>,
    /// Note FB9: a Decide row's suggested sub-type, which its ✓ button, Enter and Space accept.
    pub suggested: Option<engine::config::Subtype>,
    /// Note FC1: more for the row's hover (the combined row lists what it covers), one line each.
    pub detail: Option<String>,
}

impl Problem {
    /// The codes behind the row, for its hover and nowhere else: `CODE, CODE`.
    pub fn codes_text(&self) -> Option<String> {
        let all: Vec<_> = self
            .code
            .iter()
            .chain(&self.also)
            .map(|c| c.as_str())
            .collect();
        (!all.is_empty()).then(|| all.join(", "))
    }

    /// What the row shows (Note FB8): the code's action sentence, else the engine's text with any code taken out.
    pub fn shown(&self) -> std::borrow::Cow<'_, str> {
        code_text::shown_for(self.code, &self.text)
    }
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

    /// Note FC3: the NIF edit's problems, handed over by `app.rs` each frame. Only a new snapshot rebuilds Problems.
    pub fn set_nif_problems(&mut self, p: Arc<Vec<super::meshes::NifProblem>>) {
        if !Arc::ptr_eq(&self.nif_problems, &p) {
            self.nif_problems = p;
            self.invalidate_views();
        }
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
        // Note FC3: an `Undo` fix is built only while its step is the top one.
        m.history.pushes,
    )
}

/// The set keys and texture keys that carry a problem that takes "ready" away, and the row counts by that test.
#[derive(Debug, Default)]
pub struct Flagged {
    pub sets: HashSet<String>,
    pub textures: HashSet<String>,
    /// Rows passing `counts_as_ready_problem` (the Export dot and its hover, K6).
    pub ready_rows: usize,
    /// `Level::Check` rows (whether the Problems tab shows its `Checks` pill).
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
            f.check_rows += usize::from(p.level == Level::Check);
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
        also: vec![],
        fix: None,
        suggested: None,
        detail: None,
    };
    let mut out = vec![];
    // Note FC1 (AM14): the plan runs on the fallback so the window is not empty, but the export waits for a preset.
    if let Some(id) = m.missing_preset_id() {
        out.push(Problem {
            fix: Some(Fix::ChoosePreset),
            ..row(
                Level::Blocks,
                At::Project,
                format!("Preset {id} is not installed"),
                None,
            )
        });
    }
    for b in pbr_export::export_blocks(m) {
        let at = if b.set_key.is_empty() {
            At::Project
        } else {
            set_at(&b.set_key, &b.set_id)
        };
        out.push(Problem {
            fix: b.fix,
            ..row(Level::Blocks, at, b.text, None)
        });
    }
    // Note FC3 (GD1): the NIF edit's rows, each with its level's group. A NIF Info is a row too: it has an action.
    let nif_rows = |want: NifLevel, level: Level| {
        m.nif_problems
            .iter()
            .filter(move |n| n.level == want)
            .map(move |n| Problem {
                fix: Some(Fix::ShowNifTab(n.path.clone(), n.tab, n.field)),
                ..row(level, At::Nif(n.path.clone()), n.text.clone(), None)
            })
    };
    out.extend(nif_rows(NifLevel::Blocks, Level::Blocks));
    for i in &m.items {
        let (text, also) = match &i.status {
            ItemStatus::Failed(r) => (format!("Failed: {r}"), vec![]),
            ItemStatus::Error(cs0) => {
                let cs: Vec<_> = cs0.iter().map(|c| code_text::code_line(*c)).collect();
                (format!("Failed: {}", cs.join("; ")), cs0.clone())
            }
            _ => continue,
        };
        out.push(Problem {
            also,
            ..row(Level::Blocks, At::Texture(i.key.clone()), text, None)
        });
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
                Decision::CubeLayout => "cube layout".into(),
                // Note FB8: the sentence names the action; the code is on the hover.
                Decision::Acknowledge(c) => code_text::code_line(*c).into(),
            })
            .collect();
        let also = i
            .decisions
            .iter()
            .filter_map(|d| match d {
                Decision::Acknowledge(c) => Some(*c),
                _ => None,
            })
            .collect();
        let text = format!("Needs a decision: {}", parts.join("; "));
        let suggested = i.decisions.iter().find_map(|d| match d {
            Decision::Subtype { suggested } => *suggested,
            _ => None,
        });
        out.push(Problem {
            also,
            suggested,
            ..row(Level::Decide, At::Texture(i.key.clone()), text, None)
        });
    }
    for a in pbr_export::export_asks(m) {
        // FC7: a re-encode row is project-wide.
        let at = if a.set_id.is_empty() {
            At::Project
        } else {
            set_at(&a.set_key, &a.set_id)
        };
        out.push(row(Level::Warning, at, a.text, None));
    }
    for i in m
        .items
        .iter()
        .filter(|i| i.status != ItemStatus::NeedsDecision)
    {
        for w in i.warnings.iter().filter(|w| counts_toward_badge(w)) {
            let at = At::Texture(i.key.clone());
            // FC7 (CN3): a `_curves`/cavity map no recipe reads is Also export's input, not a mistake: an Info row
            // with no button (the generic PACK-UNUSED-INPUT sentence says to remove it).
            // shortcut: matched on the shared `ALSO_ONLY` text; give the warning a reason flag if a second row needs one.
            if w.code == Code::PackUnusedInput && w.message.contains(engine::pack::ALSO_ONLY) {
                let text = "Read only by Also export; not written on its own".to_string();
                out.push(row(Level::Info, at, text, None));
                continue;
            }
            // Note DP: a short normal's row rebuilds Z in place.
            let fix = (w.code == Code::ChanNormalLength).then(|| Fix::RebuildZ(i.key.clone()));
            out.push(Problem {
                fix,
                ..row(Level::Warning, at, w.message.clone(), Some(w.code))
            });
        }
        // Note FC3 (CO70): a stale generated map is a Warning row with Regenerate; GEN-REPLACED stays on hover.
        if let Some(reason) = m.generated_info(&i.key).and_then(|g| g.stale) {
            out.push(Problem {
                fix: Some(Fix::Regenerate(i.key.clone())),
                ..row(
                    Level::Warning,
                    At::Texture(i.key.clone()),
                    reason,
                    Some(Code::GenStale),
                )
            });
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
                fix: Some(Fix::RemoveMissing(miss.clone())),
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
    // Note FC3 (Y4): an entry of the folder the plan could not read, with its folder to open.
    for w in &m.unreadable {
        out.push(Problem {
            fix: unreadable_folder(&w.message).map(Fix::OpenFolder),
            ..row(Level::Warning, At::Project, w.message.clone(), Some(w.code))
        });
    }
    out.extend(nif_rows(NifLevel::Warning, Level::Warning));
    // Artist checks (DECISIONS Note CL): listed, never "ready"-changing. CHK-GREEN-GUESS, the one Info result, is a
    // row too, since it has an action (Note FC3, CL5).
    let check_row =
        |w: &engine::codes::Warning| counts_toward_badge(w) || w.code == Code::ChkGreenGuess;
    for i in m
        .items
        .iter()
        .filter(|i| i.status != ItemStatus::NeedsDecision)
    {
        for w in m.check_warnings(&i.key).iter().filter(|w| check_row(w)) {
            let at = At::Texture(i.key.clone());
            let fix = (w.code == Code::ChkGreenGuess).then(|| Fix::TreatAsOpenGl(i.key.clone()));
            out.push(Problem {
                fix,
                ..row(Level::Check, at, w.message.clone(), Some(w.code))
            });
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
        // Note FC3 (CN67): the DERIVE results the user acts on are Warning rows that open the report; the other
        // Info results stay on the row's hover (P11).
        let file_row = |w: &engine::codes::Warning| {
            counts_toward_badge(w)
                || matches!(w.code, Code::DeriveNoEnvmap | Code::DeriveNifSuggest)
        };
        // Note DP4: the live item row is the current truth and carries the fix, so a same-code copy of it from
        // the last export is dropped. Only when the job maps to exactly one item (None = several share a job_key).
        // Built once, and only when some export row has a listed warning (no per-row scan of all items).
        let mut owners: Option<HashMap<&str, Option<&super::items::TextureItem>>> = None;
        for r in &m.export.rows {
            if !r.warnings.iter().any(file_row) {
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
            for w in r.warnings.iter().filter(|w| file_row(w)) {
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
                let fix = match w.code {
                    // No report written: Messages says why (fix round 1: a row always carries its action).
                    Code::DeriveNoEnvmap | Code::DeriveNifSuggest => Some(
                        m.also_report
                            .clone()
                            .map_or(Fix::ShowMessages, Fix::OpenReport),
                    ),
                    // Note FC3 (DP7d): after Rebuild Z the file on disk is still short; the row keeps the button.
                    Code::ChanNormalLength => live.map(|i| Fix::RebuildZ(i.key.clone())),
                    _ => None,
                };
                let at = At::File(r.job_key.clone());
                out.push(Problem {
                    fix,
                    ..row(level, at, w.message.clone(), Some(w.code))
                });
            }
        }
    }
    // Note FC3 (R-fills-2, Z11): what an edit removed is an Info row, with Undo while that edit is the last step.
    let undo = |step: u64| (step == m.history.pushes && m.can_undo()).then_some(Fix::UndoLast);
    for (k, note) in &m.dropped_fills {
        out.push(Problem {
            fix: undo(m.dropped_fills_step),
            ..row(Level::Info, At::Texture(k.clone()), note.clone(), None)
        });
    }
    // One row per texture the cleanup touched (the note's hover list before FC3).
    for line in m.encoding_note.iter().flat_map(|(_, all)| all.lines()) {
        out.push(Problem {
            fix: undo(m.encoding_note_step),
            ..row(Level::Info, At::Project, line.to_string(), None)
        });
    }
    // Note FC1: what the plan decided on its own and the project now records, one row with every texture on hover.
    if let Some(a) = &m.auto_note {
        out.push(Problem {
            fix: undo(a.step),
            detail: Some(a.lines.join("\n")),
            ..row(Level::Info, At::Project, a.text.clone(), None)
        });
    }
    out.extend(nif_rows(NifLevel::Info, Level::Info));
    out
}

/// The folder that holds an IN-UNREADABLE entry: the engine writes `skipped: <path>: <error>` (engine `api.rs`,
/// `discover.rs`), with no `data` field.
/// shortcut: parses the engine's human message; switch to `w.data` once the engine puts the path there.
fn unreadable_folder(message: &str) -> Option<PathBuf> {
    let rest = message.strip_prefix("skipped: ")?;
    // A drive's `C:\` has no space after its colon; the error's separator does.
    let path = rest.split_once(": ").map_or(rest, |(p, _)| p);
    PathBuf::from(path).parent().map(PathBuf::from)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins the engine's IN-UNREADABLE message shape (`skipped: <path>: <error>`) that `unreadable_folder` parses.
    #[test]
    fn unreadable_folder_reads_the_engine_message() {
        let m = r"skipped: C:\mods\a\locked: Access is denied. (os error 5)";
        assert_eq!(unreadable_folder(m), Some(PathBuf::from(r"C:\mods\a")));
        assert_eq!(unreadable_folder("not the shape"), None);
    }
}
