//! File pickers off the UI thread (Task 6, item 7; DECISIONS AG40-AG44). One service for every picker: the dialog
//! runs on a spawned thread (plain `rfd::FileDialog`; no executor is needed), only one is open at a time, and the
//! result lands in an inbox the owning model drains in its `poll`, tagged with its [`PickerPurpose`].
use eframe::egui;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// What a picker is for; the model that owns the purpose handles the result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerPurpose {
    /// Header and Ctrl+O: open a folder.
    OpenFolder,
    /// Header: the export folder setting.
    ExportFolder,
    /// Meshes page: an MO2 instance.
    ChooseMo2,
    /// Meshes page: a loose folder to scan.
    ScanFolder,
    /// Advanced: nvcompress.exe.
    NvttPath,
    /// Export drawer: where to write the report.
    SaveReport,
    /// 3D preview: the Community Shaders folder.
    ShaderFolder,
}

/// The hover text of every picker button while one is open.
pub const BUSY_HOVER: &str = "A file dialog is already open";

/// What to ask for.
#[derive(Debug, Clone)]
pub enum PickerKind {
    Folder,
    File {
        filter: (&'static str, &'static [&'static str]),
    },
    Save {
        file_name: &'static str,
        filters: &'static [(&'static str, &'static [&'static str])],
    },
}

#[derive(Debug, Clone)]
pub struct PickerSpec {
    pub title: Option<&'static str>,
    pub kind: PickerKind,
}

impl PickerSpec {
    pub fn folder() -> Self {
        PickerSpec {
            title: None,
            kind: PickerKind::Folder,
        }
    }
}

/// The seam tests replace: shows the dialog (blocking the calling thread) and returns the choice.
pub trait PickerBackend: Send + Sync {
    fn pick(&self, spec: &PickerSpec) -> Option<PathBuf>;
}

struct Native;

impl PickerBackend for Native {
    fn pick(&self, spec: &PickerSpec) -> Option<PathBuf> {
        let mut d = rfd::FileDialog::new();
        if let Some(t) = spec.title {
            d = d.set_title(t);
        }
        match &spec.kind {
            PickerKind::Folder => d.pick_folder(),
            PickerKind::File { filter } => d.add_filter(filter.0, filter.1).pick_file(),
            PickerKind::Save { file_name, filters } => {
                d = d.set_file_name(*file_name);
                for f in *filters {
                    d = d.add_filter(f.0, f.1);
                }
                d.save_file()
            }
        }
    }
}

/// A finished picker. `path` is `None` when the user cancelled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picked {
    pub purpose: PickerPurpose,
    /// The opened folder when the picker started (`None`: no folder, or the purpose is app-wide).
    pub scope: Option<PathBuf>,
    /// The requester (`Picker::owner`); only it takes the result.
    owner: u64,
    pub path: Option<PathBuf>,
}

struct State {
    backend: Arc<dyn PickerBackend>,
    open: Option<PickerPurpose>,
    inbox: Vec<Picked>,
}

/// A handle on the one picker state; clones share it.
#[derive(Clone)]
pub struct Picker(Arc<Mutex<State>>, u64);

static OWNERS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

thread_local! {
    // The UI thread has one; a test thread (each test is its own thread) gets its own.
    static SHARED: Picker = Picker::new();
}

impl Default for Picker {
    fn default() -> Self {
        Picker::shared()
    }
}

impl Picker {
    pub fn new() -> Picker {
        Picker(
            Arc::new(Mutex::new(State {
                backend: Arc::new(Native),
                open: None,
                inbox: vec![],
            })),
            0,
        )
    }

    /// The UI thread's picker: the app's models all hold a clone, so one dialog blocks every picker button.
    pub fn shared() -> Picker {
        SHARED.with(|p| {
            Picker(
                p.0.clone(),
                OWNERS.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            )
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Replace the dialog (tests).
    pub fn set_backend(&self, b: Arc<dyn PickerBackend>) {
        self.lock().backend = b;
    }

    /// Is a dialog open? Every picker button is disabled with [`BUSY_HOVER`] meanwhile.
    pub fn busy(&self) -> bool {
        self.lock().open.is_some()
    }

    /// Open the dialog on a thread; `false` (nothing started) when one is open already. `scope` is the opened
    /// folder for a result that must be dropped if the folder changed. A repaint is requested when it lands.
    pub fn request(
        &self,
        ctx: &egui::Context,
        purpose: PickerPurpose,
        scope: Option<PathBuf>,
        spec: PickerSpec,
    ) -> bool {
        let backend = {
            let mut s = self.lock();
            if s.open.is_some() {
                return false;
            }
            s.open = Some(purpose);
            s.backend.clone()
        };
        let (me, ctx, owner) = (self.clone(), ctx.clone(), self.1);
        let work = move || {
            // A panicking dialog must not leave the buttons disabled for good.
            let path =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| backend.pick(&spec)))
                    .ok()
                    .flatten();
            {
                let mut s = me.lock();
                s.open = None;
                s.inbox.push(Picked {
                    owner,
                    purpose,
                    scope,
                    path,
                });
            }
            ctx.request_repaint();
        };
        if std::thread::Builder::new()
            .name("picker".into())
            .spawn(work)
            .is_err()
        {
            self.lock().open = None;
            return false;
        }
        true
    }

    /// The finished pickers this handle asked for, oldest first. Each model and each 3D viewport has its own owner, so a
    /// result reaches only the one that opened the dialog.
    pub fn take(&self) -> Vec<Picked> {
        let me = self.1;
        let mut s = self.lock();
        let (taken, rest) = std::mem::take(&mut s.inbox)
            .into_iter()
            .partition(|p| p.owner == me);
        s.inbox = rest;
        taken
    }
}

/// A picker button: disabled with [`BUSY_HOVER`] while a dialog is open. True when clicked and free.
pub fn button(ui: &mut egui::Ui, picker: &Picker, b: egui::Button<'_>) -> bool {
    let r = ui.add_enabled(!picker.busy(), b);
    r.on_disabled_hover_text(BUSY_HOVER).clicked()
}

impl PickerPurpose {
    /// The purposes whose result belongs to the opened folder: it is dropped if the folder changed meanwhile.
    /// OpenFolder is not one: the folder picked is the user's latest choice whatever was open (AG129).
    /// The others (a scan source, nvcompress, the shader folder) mean the same whatever folder is open.
    fn folder_scoped(self) -> bool {
        matches!(
            self,
            PickerPurpose::ExportFolder | PickerPurpose::SaveReport
        )
    }
}

use super::AppModel;

impl AppModel {
    /// Open a picker for `purpose`, tagged with the opened folder when the purpose is folder-scoped. False when a
    /// dialog is already open.
    pub fn pick(&self, ctx: &egui::Context, purpose: PickerPurpose, spec: PickerSpec) -> bool {
        let scope = purpose.folder_scoped().then(|| self.root.clone()).flatten();
        self.picker.request(ctx, purpose, scope, spec)
    }

    /// Route one finished picker to its handler. A result for a folder that is no longer open is dropped; a cancel
    /// does nothing.
    pub fn picked(&mut self, d: Picked) {
        let Some(path) = d.path else { return };
        if d.purpose.folder_scoped() && d.scope != self.root {
            let note = "Picker result ignored: the folder changed while the dialog was open";
            self.status_line = note.into();
            self.messages
                .notify(super::messages::Severity::Warning, note);
            return;
        }
        match d.purpose {
            PickerPurpose::OpenFolder => {
                self.request_open(path);
            }
            PickerPurpose::ExportFolder if self.export.running.is_some() => {
                // A dialog opened before the export started (AG129).
                let note = "Picker result ignored: an export is running";
                self.status_line = note.into();
                self.messages
                    .notify(super::messages::Severity::Warning, note);
            }
            PickerPurpose::ExportFolder => {
                self.settings.export_folder = Some(path);
                self.save_settings();
                // Existing outputs decide which textures are up to date.
                self.replan();
            }
            PickerPurpose::NvttPath => self.set_nvtt_path(Some(path)),
            PickerPurpose::SaveReport => self.save_report(path),
            // The meshes and 3D models handle their own.
            PickerPurpose::ChooseMo2 | PickerPurpose::ScanFolder | PickerPurpose::ShaderFolder => {}
        }
    }
}

impl AppModel {
    /// AG130: the report text is built here, the file is written on a worker; the result goes to the status line
    /// and a toast (`report_saved`).
    pub fn save_report(&mut self, path: std::path::PathBuf) {
        let text = crate::ui::drawer::report_text(self, &path);
        self.report_saving = true;
        crate::worker::spawn_guarded(self.tx.clone(), "save-report", move |tx| {
            let result = std::fs::write(&path, text).map_err(|e| e.to_string());
            let _ = tx.send(crate::worker::WorkerMsg::ReportSaved { path, result });
        });
    }

    pub(super) fn report_saved(&mut self, path: &std::path::Path, result: Result<(), String>) {
        self.report_saving = false;
        let (sev, note) = match result {
            Ok(()) => (
                super::messages::Severity::Info,
                format!("Saved report: {}", path.display()),
            ),
            Err(e) => (
                super::messages::Severity::Error,
                format!("Could not write the report to {}: {e}", path.display()),
            ),
        };
        self.status_line = note.clone();
        self.messages.notify(sev, note);
    }
}
