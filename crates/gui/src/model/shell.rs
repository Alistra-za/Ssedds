//! Explorer requests (Task 5, items 6 and 14; DECISIONS AG34): open a folder, or open a folder with a file selected.
//! The model queues a [`ShellRequest`]; the app spawns it ([`spawn`]), so tests see the queue and never start
//! Explorer. The arguments are built by pure functions with no shell involved.
use super::export::Row;
use super::AppModel;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellRequest {
    /// `explorer.exe <folder>`.
    Open(PathBuf),
    /// `explorer.exe /select,<file>`.
    Reveal(PathBuf),
}

/// The path with the separators Explorer expects (`/select,` fails with forward slashes).
fn windows_path(p: &Path) -> PathBuf {
    p.components().collect()
}

/// The ONE argument that selects `file`: `/select,` and the path together. A path with spaces or commas stays inside
/// it, so `Command::arg` quotes the whole argument and Explorer reads everything after the first comma as the path.
pub fn select_arg(file: &Path) -> OsString {
    let mut a = OsString::from("/select,");
    a.push(windows_path(file));
    a
}

/// The arguments `explorer.exe` gets for `req`.
pub fn args(req: &ShellRequest) -> Vec<OsString> {
    match req {
        ShellRequest::Open(dir) => vec![windows_path(dir).into_os_string()],
        ShellRequest::Reveal(file) => vec![select_arg(file)],
    }
}

/// Start Explorer for `req`, detached and never waited on (Explorer exits with 1 even when it worked). A failure to
/// start is ignored: there is nothing the user could do with it.
pub fn spawn(req: &ShellRequest) {
    let mut c = Command::new("explorer.exe");
    c.args(args(req))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS
        c.creation_flags(0x0000_0008);
    }
    let _ = c.spawn();
}

impl AppModel {
    /// The requests queued since the last call; the app starts Explorer for each.
    pub fn take_shell(&mut self) -> Vec<ShellRequest> {
        std::mem::take(&mut self.shell)
    }

    /// The export folder, or why "Open export folder" is off: Explorer would show some other folder instead.
    pub fn export_folder_state(&self) -> Result<PathBuf, String> {
        let p = self.export_root();
        if p.is_dir() {
            Ok(p)
        } else {
            Err(format!(
                "The export folder does not exist yet: {}. Export creates it.",
                p.display()
            ))
        }
    }

    /// Open the export folder in Explorer; the reason when it cannot.
    pub fn open_export_folder(&mut self) -> Result<(), String> {
        self.export_folder_state()
            .map(|p| self.shell.push(ShellRequest::Open(p)))
    }

    /// Select `file` in Explorer; the reason when it is not on disk.
    pub fn reveal(&mut self, file: &Path) -> Result<(), String> {
        if !file.is_file() {
            return Err(format!("{} is not on disk", file.display()));
        }
        self.shell.push(ShellRequest::Reveal(file.to_path_buf()));
        Ok(())
    }

    /// Why "Show in Explorer" is off for the source of texture `key`.
    pub fn reveal_problem(&self, key: &str) -> Option<String> {
        match self.item(key) {
            None => Some("no such texture".into()),
            Some(i) if !i.input.is_file() => Some(format!("{} is not on disk", i.input.display())),
            Some(_) => None,
        }
    }

    /// Select the source file of texture `key` in Explorer.
    pub fn reveal_item(&mut self, key: &str) -> Result<(), String> {
        let input = self
            .item(key)
            .map(|i| i.input.clone())
            .ok_or_else(|| "no such texture".to_string())?;
        self.reveal(&input)
    }

    /// What a Files row reveals: the written output once it exists, otherwise the source.
    pub fn row_reveal_target(&self, r: &Row) -> Option<PathBuf> {
        let output = self.export.outputs.get(&r.job_key).map(PathBuf::from);
        output
            .filter(|p| p.is_file())
            .or_else(|| Some(PathBuf::from(&r.path)).filter(|p| p.is_file()))
    }
}
