//! The view-model (M3a spec §3): all GUI state and logic, no egui types, testable without a window.
pub mod items;
pub mod preview;

use crate::project::{ProjectDoc, SaveError};
use crate::settings::GuiSettings;
use crate::worker::{spawn_plan, WorkerMsg};
use engine::api::{BatchRequest, CancelToken};
use engine::codes::Code;
use engine::config::{AlphaPolicy, GreenConvention, RawMap, Subtype, Target};
use engine::policy::AcceptSuggestions;
use engine::presets::ConfigFile;
use engine::session::Session;
use items::*;
use serde_json::json;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    None,
    Item(String),
    Set(String),
}

#[derive(Debug, Clone, Default)]
pub struct Filter {
    pub needs_decision: bool,
    pub warnings: bool,
    pub failed: bool,
    pub not_in_set: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dialog {
    ProceedAnyway(usize),
    ExternalChange,
    CloseUnsaved,
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Subtype(Subtype),
    Alpha(AlphaPolicy),
    Green(GreenConvention),
    Acknowledge(Code),
}

pub struct AppModel {
    pub settings: GuiSettings,
    pub settings_path: Option<PathBuf>,
    /// `%APPDATA%\sdc\config.toml`, set by `main`; `None` in tests so a developer's own config cannot leak in.
    pub user_config: Option<PathBuf>,
    pub session: Option<Arc<Session>>,
    pub root: Option<PathBuf>,
    pub project: Option<ProjectDoc>,
    pub target: Target,
    pub items: Vec<TextureItem>,
    pub sets: Vec<SetItem>,
    pub unmatched_keys: Vec<String>,
    pub selection: Selection,
    pub filter: Filter,
    pub search: String,
    pub status_line: String,
    pub banner: Option<String>,
    pub dialog: Option<Dialog>,
    pub expanded: Option<String>,
    generation: u64,
    planning: Option<CancelToken>,
    tx: Sender<WorkerMsg>,
    rx: Receiver<WorkerMsg>,
}

impl AppModel {
    pub fn new(
        settings: GuiSettings,
        settings_path: Option<PathBuf>,
        session: Arc<Session>,
    ) -> AppModel {
        let mut m = AppModel::placeholder(settings);
        m.settings_path = settings_path;
        m.session = Some(session);
        m
    }

    /// A model with no session: it draws, it never plans.
    pub fn placeholder(settings: GuiSettings) -> AppModel {
        let (tx, rx) = channel();
        AppModel {
            settings,
            settings_path: None,
            user_config: None,
            session: None,
            root: None,
            project: None,
            target: Target::Cs,
            items: vec![],
            sets: vec![],
            unmatched_keys: vec![],
            selection: Selection::None,
            filter: Filter::default(),
            search: String::new(),
            status_line: String::new(),
            banner: None,
            dialog: None,
            expanded: None,
            generation: 0,
            planning: None,
            tx,
            rx,
        }
    }

    pub fn open_folder(&mut self, root: PathBuf) {
        let project = ProjectDoc::open(&root);
        self.banner = project.error().map(|e| {
            format!(
                "{e} — sdc.toml is ignored and the GUI is read-only until it is fixed and reloaded"
            )
        });
        self.project = Some(project);
        self.settings.push_recent(root.clone());
        self.save_settings();
        self.root = Some(root);
        self.items.clear();
        self.sets.clear();
        self.unmatched_keys.clear();
        self.selection = Selection::None;
        self.expanded = None;
        self.replan();
    }

    pub fn save_settings(&self) {
        if let Some(p) = &self.settings_path {
            let _ = self.settings.save(p);
        }
    }

    /// `<export folder>` from the settings, else `<root>/sdc-out`.
    pub fn export_root(&self) -> PathBuf {
        self.settings
            .export_folder
            .clone()
            .unwrap_or_else(|| self.root.clone().unwrap_or_default().join("sdc-out"))
    }

    /// What the next plan uses: the folder, recursive, nothing accepted, the in-memory `sdc.toml` (DECISIONS N4).
    pub fn request(&self) -> Option<BatchRequest> {
        let root = self.root.clone()?;
        let project = self.project.as_ref()?;
        let mut req = BatchRequest::new(vec![root], self.export_root(), Target::Cs);
        req.target = None;
        req.recursive = true;
        req.accept = AcceptSuggestions::None;
        req.project_config_inline = Some(if project.read_only() {
            ConfigFile::default()
        } else {
            project.config().unwrap_or_default()
        });
        req.user_config = self.user_config.clone();
        req.jobs = self.settings.jobs;
        req.mem_budget = self.settings.mem_budget_mb * 1024 * 1024;
        Some(req)
    }

    /// Plan again on a worker; a newer plan supersedes (cancels) an older one.
    pub fn replan(&mut self) {
        let (Some(session), Some(req), Some(root)) =
            (self.session.clone(), self.request(), self.root.clone())
        else {
            return;
        };
        if let Some(c) = self.planning.take() {
            c.cancel();
        }
        self.generation += 1;
        let cancel = CancelToken::default();
        self.planning = Some(cancel.clone());
        self.status_line = "Planning…".into();
        spawn_plan(self.tx.clone(), session, req, cancel, self.generation, root);
    }

    pub fn busy(&self) -> bool {
        self.planning.is_some()
    }

    /// Drain worker messages. True when anything changed (the UI repaints).
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(msg) = self.rx.try_recv() {
            changed = true;
            self.handle(msg);
        }
        changed
    }

    fn handle(&mut self, msg: WorkerMsg) {
        match msg {
            WorkerMsg::PlanProgress { generation, total } if generation == self.generation => {
                self.status_line = format!("Analysing {total} files…");
            }
            WorkerMsg::PlanProgress { .. } => {}
            WorkerMsg::Planned { generation, result } if generation == self.generation => {
                self.planning = None;
                match result {
                    Ok(view) => self.apply_plan(view),
                    Err(e) => {
                        self.status_line = format!("Planning failed: {e}");
                        self.dialog = Some(Dialog::Error(e));
                    }
                }
            }
            WorkerMsg::Planned { .. } => {}
            WorkerMsg::Panicked { what, message } => {
                if what == "plan" {
                    self.planning = None;
                }
                self.dialog = Some(Dialog::Error(format!(
                    "internal error in {what}: {message}"
                )));
            }
            other => self.handle_more(other),
        }
    }

    /// Messages later tasks handle (export, preview).
    fn handle_more(&mut self, _msg: WorkerMsg) {}

    fn apply_plan(&mut self, view: PlanView) {
        self.target = view.target;
        self.items = view.items;
        self.sets = view.sets;
        self.unmatched_keys = view.unmatched;
        self.status_line = format!(
            "{} textures, {} sets, {} need a decision",
            self.items.len(),
            self.sets.len(),
            self.undecided()
        );
        // Keep the selection when its item or set still exists; else nothing.
        let gone = match &self.selection {
            Selection::Item(k) => self.item(k).is_none(),
            Selection::Set(k) => !self.sets.iter().any(|s| &s.key == k),
            Selection::None => false,
        };
        if gone {
            self.selection = Selection::None;
        }
        if let Some(k) = self.expanded.clone() {
            if self.item(&k).is_none() {
                self.expanded = None;
            }
        }
    }

    pub fn wait_idle(&mut self, timeout: Duration) -> bool {
        let end = Instant::now() + timeout;
        loop {
            self.poll();
            if !self.busy() {
                return true;
            }
            if Instant::now() > end {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn item(&self, key: &str) -> Option<&TextureItem> {
        self.items.iter().find(|i| i.key == key)
    }

    pub fn undecided(&self) -> usize {
        self.items
            .iter()
            .filter(|i| i.status == ItemStatus::NeedsDecision)
            .count()
    }

    pub fn visible_items(&self) -> Vec<usize> {
        let q = self.search.to_lowercase();
        let in_set = |i: &TextureItem| {
            i.set_key
                .as_ref()
                .is_some_and(|k| self.sets.iter().any(|s| &s.key == k && s.members.len() > 1))
        };
        self.items
            .iter()
            .enumerate()
            .filter(|(_, i)| q.is_empty() || i.key.to_lowercase().contains(&q))
            .filter(|(_, i)| !self.filter.needs_decision || i.status == ItemStatus::NeedsDecision)
            .filter(|(_, i)| !self.filter.warnings || !i.warnings.is_empty())
            .filter(|(_, i)| {
                !self.filter.failed
                    || matches!(i.status, ItemStatus::Failed(_) | ItemStatus::Error(_))
            })
            .filter(|(_, i)| !self.filter.not_in_set || !in_set(i))
            .map(|(n, _)| n)
            .collect()
    }

    pub fn select_item(&mut self, key: &str) {
        self.selection = Selection::Item(key.to_string());
    }

    pub fn select_set(&mut self, key: &str) {
        self.selection = Selection::Set(key.to_string());
    }

    pub fn expand(&mut self, key: &str) {
        self.select_item(key);
        self.expanded = Some(key.to_string());
    }

    pub fn collapse(&mut self) {
        self.expanded = None;
    }

    fn edit(&mut self, f: impl FnOnce(&mut ProjectDoc)) {
        let Some(p) = self.project.as_mut() else {
            return;
        };
        if p.read_only() {
            return;
        }
        f(p);
        self.replan();
    }

    pub fn set_field(&mut self, item_key: &str, field: &str, value: serde_json::Value) {
        let k = item_key.to_string();
        let f = field.to_string();
        self.edit(move |p| p.set_file_value(&k, &f, &value));
    }

    pub fn clear_field(&mut self, item_key: &str, field: &str) {
        let k = item_key.to_string();
        let f = field.to_string();
        self.edit(move |p| p.clear_file_value(&k, &f));
    }

    /// A confirmation writes the value explicitly, or adds the code to the file's `allow` (M3a spec §5.2).
    pub fn decide(&mut self, item_key: &str, answer: Answer) {
        let k = item_key.to_string();
        self.edit(move |p| match answer {
            Answer::Subtype(st) => p.set_file_value(&k, "subtype", &json!(st)),
            Answer::Alpha(a) => p.set_file_value(&k, "alpha.policy", &json!(a)),
            Answer::Green(g) => p.set_file_value(&k, "channels.green_convention", &json!(g)),
            Answer::Acknowledge(c) => {
                let mut codes = p.allow(&k);
                codes.insert(c);
                p.set_allow(&k, &codes);
            }
        });
    }

    pub fn set_target(&mut self, t: Target) {
        self.edit(move |p| p.set_target(t));
    }

    pub fn set_fill(&mut self, role: RawMap, value: Option<u8>) {
        self.edit(move |p| p.set_fill(role, value));
    }

    pub fn is_dirty(&self) -> bool {
        self.project.as_ref().is_some_and(ProjectDoc::is_dirty)
    }

    pub fn save(&mut self) -> Result<(), SaveError> {
        let Some(p) = self.project.as_mut() else {
            return Ok(());
        };
        let r = p.save(false);
        match &r {
            Err(SaveError::ChangedOnDisk) => self.dialog = Some(Dialog::ExternalChange),
            Err(e) => self.dialog = Some(Dialog::Error(e.to_string())),
            Ok(()) => {}
        }
        r
    }

    pub fn resolve_external_change(&mut self, overwrite: bool) {
        self.dialog = None;
        let Some(p) = self.project.as_mut() else {
            return;
        };
        if overwrite {
            if let Err(e) = p.save(true) {
                self.dialog = Some(Dialog::Error(e.to_string()));
            }
        } else {
            p.reload();
            self.banner = p.error().map(str::to_string);
            self.replan();
        }
    }

    pub fn request_close(&mut self) -> bool {
        if self.is_dirty() {
            self.dialog = Some(Dialog::CloseUnsaved);
            false
        } else {
            true
        }
    }
}
