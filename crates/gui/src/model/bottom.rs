//! The bottom panel's state (layout v2 A1, A9; R7, R8): which tab shows, and whether the panel is open.
use super::items::Decision;
use super::{counts_toward_badge, pbr_export, sets, AppModel, ItemStatus};
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

/// Red blocks the export; the other two are orange (A12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Blocks,
    Decide,
    Warning,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProblemsFilter {
    /// An engine set key.
    Set(String),
    Undecided,
}

impl AppModel {
    /// Open the Problems tab, narrowed to `f`.
    pub fn open_problems(&mut self, f: Option<ProblemsFilter>) {
        self.show_tab(BottomTab::Problems);
        self.bottom.filter = f;
    }
}

/// Every problem of the project: blocks, then decisions, then warnings. Cached (K9) under the views key plus
/// what the asks and the File rows read; the build holds no `RefCell` borrow.
pub fn problems(m: &AppModel) -> Arc<Vec<Problem>> {
    let key = m.view_key();
    let inner = (
        m.evidence_rev,
        m.export.revision,
        m.export.running.is_some(),
    );
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
    p
}

#[doc(hidden)]
pub fn problems_builds(m: &AppModel) -> u64 {
    m.problem_builds.get()
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
    // Rows only count once the export has ended: `revision` bumps on every engine event (R9).
    if m.export.running.is_none() {
        for r in &m.export.rows {
            for w in &r.warnings {
                let at = At::File(r.job_key.clone());
                out.push(row(Level::Warning, at, w.message.clone(), Some(w.code)));
            }
        }
    }
    out
}

/// The list narrowed by `f`: a set keeps its own rows and its members' rows; `Undecided` keeps the Decide rows.
pub fn filtered(m: &AppModel, f: Option<&ProblemsFilter>) -> Vec<Problem> {
    let all = problems(m);
    match f {
        None => (*all).clone(),
        Some(ProblemsFilter::Undecided) => all
            .iter()
            .filter(|p| p.level == Level::Decide)
            .cloned()
            .collect(),
        Some(ProblemsFilter::Set(k)) => {
            let members = sets::views(m)
                .iter()
                .find(|v| v.key == *k)
                .map(|v| v.members.clone())
                .unwrap_or_default();
            all.iter()
                .filter(|p| match &p.at {
                    At::Set { key, .. } => key == k,
                    At::Texture(t) => members.contains(t),
                    _ => false,
                })
                .cloned()
                .collect()
        }
    }
}
