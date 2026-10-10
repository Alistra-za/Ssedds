//! The view-model (M3a spec §3): all GUI state and logic, no egui types, testable without a window.
pub mod export;
pub mod fields;
pub mod inspector;
pub mod items;
pub mod preview;
pub mod view;

use crate::project::{ProjectDoc, SaveError};
use crate::settings::GuiSettings;
use crate::worker::{spawn_export, spawn_plan, PlanFailure, WorkerMsg};
use engine::api::{BatchRequest, CancelToken};
use engine::codes::Code;
use engine::config::{AlphaPolicy, GreenConvention, RawMap, Subtype, Target};
use engine::policy::{AcceptSuggestions, SourceStamp};
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
    /// Opening this folder would drop unsaved edits to the current `sdc.toml`.
    OpenUnsaved(PathBuf),
    /// An export is running; closing or opening would have to cancel it first (M3a spec §2.6).
    ExportRunning(AfterExport),
    Error(String),
}

/// What was asked while an export ran: it happens when the export has ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AfterExport {
    Close,
    Open(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Subtype(Subtype),
    Alpha(AlphaPolicy),
    Green(GreenConvention),
    Acknowledge(Code),
}

fn read_only_banner(error: &str) -> String {
    format!("{error} — sdc.toml is ignored and the GUI is read-only until it is fixed and reloaded")
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
    /// The preset id the last plan resolved (empty before the first plan).
    pub batch_preset: String,
    pub selection: Selection,
    pub filter: Filter,
    pub search: String,
    pub status_line: String,
    pub banner: Option<String>,
    pub dialog: Option<Dialog>,
    pub expanded: Option<String>,
    pub export: export::ExportModel,
    pub preview: preview::PreviewModel,
    pub view: view::ViewState,
    /// The largest texture side the GPU takes; set from the renderer at start-up (M3a spec §6.5).
    pub gpu_max_dim: u32,
    /// False when eframe found no usable GPU adapter: the preview is off, everything else works (M3a spec §7).
    pub gpu_available: bool,
    /// The BC1 decoder choice differs from the session's model, which is fixed at launch (M3a spec §4.6).
    pub restart_needed: bool,
    /// Export was clicked while sources had changed: it starts when the fresh plan arrives.
    export_after_plan: bool,
    /// The user chose to cancel the running export and then close or open: done when the export has ended.
    after_export: Option<AfterExport>,
    /// The export that was in the way of a close has ended; the window may close now (see `take_close_ready`).
    close_ready: bool,
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
        let preview = preview::PreviewModel::new(settings.live_encode);
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
            batch_preset: String::new(),
            selection: Selection::None,
            filter: Filter::default(),
            search: String::new(),
            status_line: String::new(),
            banner: None,
            dialog: None,
            expanded: None,
            export: export::ExportModel::default(),
            preview,
            view: view::ViewState::default(),
            gpu_max_dim: 16384,
            gpu_available: true,
            export_after_plan: false,
            after_export: None,
            close_ready: false,
            restart_needed: false,
            generation: 0,
            planning: None,
            tx,
            rx,
        }
    }

    pub fn open_folder(&mut self, root: PathBuf) {
        // An export waiting for a plan of the previous folder no longer applies.
        self.export_after_plan = false;
        let same_folder = self.root.as_ref() == Some(&root);
        let project = ProjectDoc::open(&root);
        self.banner = project.error().map(read_only_banner);
        self.project = Some(project);
        self.settings.push_recent(root.clone());
        self.save_settings();
        self.root = Some(root);
        self.items.clear();
        self.sets.clear();
        self.unmatched_keys.clear();
        self.selection = Selection::None;
        self.expanded = None;
        self.preview_close();
        // The drawer shows the folder it ran on; another folder starts with an empty one.
        if !same_folder && self.export.running.is_none() {
            self.export = export::ExportModel::default();
        }
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

    /// A sender into the model's own channel (tests inject worker messages with it).
    pub fn sender(&self) -> Sender<WorkerMsg> {
        self.tx.clone()
    }

    pub fn busy(&self) -> bool {
        self.planning.is_some() || self.export.running.is_some() || self.preview.working()
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
                    Err(f) => self.plan_failed(f),
                }
            }
            WorkerMsg::Planned { .. } => {}
            WorkerMsg::Panicked { what, message } => {
                match what {
                    "plan" => {
                        self.planning = None;
                        self.export_after_plan = false;
                    }
                    "export" => {
                        self.export.running = None;
                        self.run_after_export();
                    }
                    "preview" => self.preview_panicked(&message),
                    _ => {}
                }
                self.dialog = Some(Dialog::Error(format!(
                    "internal error in {what}: {message}"
                )));
            }
            other => self.handle_more(other),
        }
    }

    /// A plan failed. When the engine rejected the project config that parsed fine (an unknown field or code, an
    /// out-of-range value), ignore `sdc.toml` as for a parse error (spec §7.4): read-only, banner, and plan the
    /// folder again without it. Any other failure (or a config error with `sdc.toml` already ignored) is a dialog.
    fn plan_failed(&mut self, f: PlanFailure) {
        self.export_after_plan = false;
        // An error naming the user config is not the project's fault; ignoring `sdc.toml` would not help.
        let from_user_config = self
            .user_config
            .as_ref()
            .is_some_and(|p| f.message.contains(&p.display().to_string()));
        if f.config && !from_user_config {
            if let Some(p) = self.project.as_mut().filter(|p| !p.read_only()) {
                p.set_error(&f.message);
                self.banner = p.error().map(read_only_banner);
                self.replan();
                return;
            }
        }
        self.status_line = format!("Planning failed: {}", f.message);
        self.dialog = Some(Dialog::Error(f.message));
    }

    /// Messages the export and the preview handle.
    fn handle_more(&mut self, msg: WorkerMsg) {
        match msg {
            WorkerMsg::Event(e) => self.export.on_event(e),
            WorkerMsg::ExportDone(result) => {
                self.export.running = None;
                let mut failure = None;
                match result {
                    Ok(summary) => self.export.summary = Some(summary),
                    Err(e) => {
                        self.export.error = Some(e.clone());
                        failure = Some(e);
                    }
                }
                // A user who cancelled the export to leave does not need its error as a dialog.
                if !self.run_after_export() {
                    if let Some(e) = failure {
                        self.dialog = Some(Dialog::Error(e));
                    }
                }
            }
            WorkerMsg::Source { token, result } => self.on_source(token, result),
            WorkerMsg::Encoded { token, result } => self.on_encoded(token, result),
            _ => {}
        }
    }

    /// Export the folder; first the checks that need an answer (M3a spec §2.5). Sources that changed since the plan
    /// are flagged and the plan is made again first, so the export starts from fresh plans (M3a spec §7).
    pub fn export_clicked(&mut self) {
        if self.root.is_none() {
            return;
        }
        // A plan is in flight (just opened, or an edit): the items on screen are the previous plan's, so the checks
        // wait for the plan that lands (spec §2.6: the button is never silently disabled).
        if self.planning.is_some() {
            self.export_after_plan = true;
            return;
        }
        if self.check_sources() > 0 && self.session.is_some() {
            self.export_after_plan = true;
            self.replan();
            return;
        }
        self.export_checks();
    }

    /// The export folder and undecided-texture checks, then the export itself.
    fn export_checks(&mut self) {
        let Some(root) = self.root.clone() else {
            return;
        };
        if let Some(problem) = export::export_folder_problem(&root, &self.export_root()) {
            self.dialog = Some(Dialog::Error(problem));
            return;
        }
        let n = self.undecided();
        if n > 0 {
            self.dialog = Some(Dialog::ProceedAnyway(n));
            return;
        }
        self.start_export();
    }

    /// Mark every item whose source file differs from the stamp the plan took (or, for a DDS input, from the stamp
    /// taken when it was listed). A packed output is checked against each of its sources, and a raw source feeding
    /// one is flagged with it. Returns the number of items flagged.
    pub fn check_sources(&mut self) -> usize {
        let mut changed_paths: std::collections::HashSet<PathBuf> = Default::default();
        for item in &self.items {
            let Some(job) = &item.job else {
                continue;
            };
            let stamps: Vec<(&std::path::Path, Option<SourceStamp>)> = match &job.pack {
                Some(pk) => pk
                    .sources
                    .iter()
                    .map(|s| (s.path.as_path(), s.stamp))
                    .collect(),
                None => vec![(job.input.as_path(), job.source)],
            };
            for (path, planned) in stamps {
                if planned.is_some_and(|p| SourceStamp::of(path) != Some(p)) {
                    changed_paths.insert(path.to_path_buf());
                }
            }
        }
        let mut n = 0;
        for item in &mut self.items {
            let dds_changed = item
                .stamp
                .is_some_and(|s| SourceStamp::of(&item.input) != Some(s));
            item.changed_on_disk = dds_changed
                || changed_paths.contains(&item.input)
                || item.job.as_ref().is_some_and(|j| {
                    j.pack.as_ref().is_some_and(|pk| {
                        pk.sources.iter().any(|s| changed_paths.contains(&s.path))
                    })
                });
            n += usize::from(item.changed_on_disk);
        }
        n
    }

    /// Plan again after sources changed on disk; the session cache reloads only the changed files.
    pub fn reload_changed(&mut self) {
        self.replan();
    }

    /// DECISIONS N3: undecided textures are skipped, never decided here.
    pub fn proceed_anyway(&mut self) {
        self.dialog = None;
        self.start_export();
    }

    /// Save first (the CLI command and the next session read `sdc.toml`), then run the export on a worker.
    fn start_export(&mut self) {
        if self.export.running.is_some() || self.save().is_err() {
            return;
        }
        let (Some(session), Some(mut req)) = (self.session.clone(), self.request()) else {
            return;
        };
        req.on_existing = engine::api::OnExisting::IfChanged;
        self.export = export::ExportModel {
            open: true,
            ..Default::default()
        };
        let cancel = CancelToken::default();
        self.export.running = Some(cancel.clone());
        spawn_export(self.tx.clone(), session, req, cancel);
    }

    /// The export ended: do what was asked while it ran (close, or open another folder). True when there was
    /// something to do. A `Dialog::ExportRunning` still on screen is answered by the export ending on its own.
    fn run_after_export(&mut self) -> bool {
        let action = match self.dialog.clone() {
            Some(Dialog::ExportRunning(a)) => {
                self.dialog = None;
                Some(a)
            }
            _ => self.after_export.take(),
        };
        match action {
            None => false,
            Some(AfterExport::Close) => {
                self.close_ready = true;
                true
            }
            Some(AfterExport::Open(root)) => {
                self.request_open(root);
                true
            }
        }
    }

    /// True once, when the window may close because the export that was in its way has ended. The window then asks
    /// `request_close` again (unsaved edits still ask).
    pub fn take_close_ready(&mut self) -> bool {
        std::mem::take(&mut self.close_ready)
    }

    /// The answer to `Dialog::ExportRunning`: cancel the export and do `action` when it has ended, or keep it running.
    pub fn resolve_export_running(&mut self, action: AfterExport, cancel_and_continue: bool) {
        self.dialog = None;
        if !cancel_and_continue {
            return;
        }
        self.after_export = Some(action);
        if self.export.running.is_some() {
            self.cancel_export();
        } else {
            // It ended while the dialog was up.
            self.run_after_export();
        }
    }

    pub fn cancel_export(&mut self) {
        if let Some(c) = &self.export.running {
            c.cancel();
        }
    }

    /// Select the texture whose output is `job_key`; the centre keeps showing whatever it shows.
    pub fn select_job(&mut self, job_key: &str) {
        if let Some(k) = self
            .items
            .iter()
            .find(|i| i.job_key == job_key)
            .map(|i| i.key.clone())
        {
            self.select_item(&k);
        }
    }

    fn apply_plan(&mut self, view: PlanView) {
        // Look the expanded item's set up before the items are replaced: if the item vanishes, its set is the
        // fallback selection (Review Focus 1).
        let expanded_set = self
            .expanded
            .as_deref()
            .and_then(|k| self.item(k))
            .and_then(|i| i.set_key.clone());
        let selected_expanded = self.expanded.is_some()
            && self.selection == Selection::Item(self.expanded.clone().unwrap_or_default());
        self.target = view.target;
        self.items = view.items;
        self.sets = view.sets;
        self.unmatched_keys = view.unmatched;
        self.batch_preset = view.preset_id;
        // The count of textures that need a decision is shown (amber) in the Textures column header.
        self.status_line = format!("{} textures, {} sets", self.items.len(), self.sets.len());
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
                self.preview_close();
                if selected_expanded {
                    if let Some(set) =
                        expanded_set.filter(|s| self.sets.iter().any(|x| &x.key == s))
                    {
                        self.selection = Selection::Set(set);
                    }
                }
            } else {
                self.preview_after_plan();
            }
        }
        if std::mem::take(&mut self.export_after_plan) {
            self.export_checks();
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
        self.view.mip = 0;
        self.preview_open(key);
    }

    pub fn collapse(&mut self) {
        self.expanded = None;
        self.preview_close();
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

    /// Remove an unmatched `[files]` entry (one that names no file under the current target) and plan again.
    pub fn remove_unmatched(&mut self, key: &str) {
        let k = key.to_string();
        self.edit(move |p| p.remove_file_entry(&k));
    }

    /// The BC1 decoder for the next launch. The session's model is fixed, so a difference needs a restart.
    pub fn set_bc1_choice(&mut self, c: crate::settings::Bc1Choice) {
        self.settings.bc1_decoder = c;
        self.save_settings();
        self.restart_needed = self
            .session
            .as_ref()
            .is_some_and(|s| s.bc1_model() != c.model());
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

    /// Open `root`, or ask first when unsaved edits would be lost. True when the folder was opened.
    pub fn request_open(&mut self, root: PathBuf) -> bool {
        if self.export.running.is_some() {
            self.dialog = Some(Dialog::ExportRunning(AfterExport::Open(root)));
            false
        } else if self.is_dirty() {
            self.dialog = Some(Dialog::OpenUnsaved(root));
            false
        } else {
            self.open_folder(root);
            true
        }
    }

    /// The answer to `Dialog::OpenUnsaved`: save (opening only if that succeeded) or drop the edits, then open.
    pub fn resolve_open_unsaved(&mut self, root: PathBuf, save: bool) {
        self.dialog = None;
        if save {
            if self.save().is_err() {
                return;
            }
        } else if let Some(p) = self.project.as_mut() {
            p.reload();
        }
        self.open_folder(root);
    }

    pub fn request_close(&mut self) -> bool {
        if self.export.running.is_some() {
            self.dialog = Some(Dialog::ExportRunning(AfterExport::Close));
            false
        } else if self.is_dirty() {
            self.dialog = Some(Dialog::CloseUnsaved);
            false
        } else {
            true
        }
    }
}
