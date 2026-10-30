//! Panic log: `tessalith.exe` has no console (`windows_subsystem = "windows"`), so a panic message would be lost.
use std::io::Write;
use std::path::{Path, PathBuf};

/// One log line per panic: thread, message, location, then the backtrace (all on one line, `\n` escaped).
pub fn format_panic(
    thread: &str,
    message: &str,
    location: &str,
    backtrace: &str,
    time: &str,
) -> String {
    let bt = backtrace.trim_end().replace('\n', "\\n");
    format!("{time} panic in thread '{thread}' at {location}: {message}\nbacktrace: {bt}\n")
}

/// `yyyymmdd-hhmmss` (UTC; std has no local time) for a count of seconds since the Unix epoch.
pub fn stamp(secs: u64) -> String {
    let (days, rem) = (secs / 86400, secs % 86400);
    // Civil date from days since 1970-01-01 (H. Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
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

fn now_stamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    stamp(secs)
}

/// `%LOCALAPPDATA%\sdc\logs`, where each run's `crash-<stamp>.log` goes.
fn log_dir() -> Option<PathBuf> {
    Some(
        Path::new(&std::env::var_os("LOCALAPPDATA")?)
            .join("sdc")
            .join("logs"),
    )
}

/// This run's crash log, set by [`install_in`].
static RUN_LOG: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
/// The unsaved documents as `(file, text)`, refreshed each frame; the panic hook writes them out (Note AR).
static RECOVERY: std::sync::Mutex<Vec<(PathBuf, String)>> = std::sync::Mutex::new(Vec::new());

pub fn run_log_path() -> Option<PathBuf> {
    RUN_LOG.get().cloned()
}

fn append(path: &Path, line: &str) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = f.write_all(line.as_bytes());
    }
}

/// Note AR: wgpu reports a lost device to no one (egui-wgpu then panics on the next frame with "Failed to create
/// staging buffer"), so the reason goes to the crash log and the timing log.
pub fn watch_device(rs: &eframe::egui_wgpu::RenderState, log: Option<PathBuf>) {
    let backend = format!("{:?}", rs.adapter.get_info().backend);
    rs.device.set_device_lost_callback(move |reason, message| {
        DEVICE_LOST.store(true, std::sync::atomic::Ordering::Release);
        let line = format!("{} wgpu device lost ({reason:?}, {backend}): {message}\n", now_stamp());
        if let Some(path) = &log {
            append(path, &line);
        }
        render3d::perf_log::emit(
            "device_lost",
            serde_json::json!({ "reason": format!("{reason:?}"), "backend": backend, "message": message }),
        );
    });
}

/// Set by the device-lost callback, which wgpu runs inside the present or submit of the frame that lost it.
static DEVICE_LOST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn device_lost() -> bool {
    DEVICE_LOST.load(std::sync::atomic::Ordering::Acquire)
}

/// Note FC4: a process started with `--after-device-loss` is one restart deep; see `should_relaunch`.
static AFTER_LOSS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static STARTED: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// Called once from `main` with the folder named by `--after-device-loss`, if any.
pub fn mark_started(after_loss: bool) {
    AFTER_LOSS.store(after_loss, std::sync::atomic::Ordering::Release);
    let _ = STARTED.set(std::time::Instant::now());
}

/// The folder after `--after-device-loss` in the process arguments (the program name is skipped by the caller).
pub fn after_device_loss_folder(
    args: impl IntoIterator<Item = std::ffi::OsString>,
) -> Option<PathBuf> {
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        if a == "--after-device-loss" {
            return it.next().map(PathBuf::from);
        }
    }
    None
}

/// Note FC4 (DN3): the arguments the restarted process gets, so it reopens the folder and FB5's restore runs.
pub fn relaunch_args(folder: Option<&Path>) -> Vec<std::ffi::OsString> {
    match folder {
        Some(f) => vec!["--after-device-loss".into(), f.as_os_str().to_owned()],
        None => Vec::new(),
    }
}

/// Loop guard: a restarted process that loses the device again within 60 s of its start does not restart again.
pub fn should_relaunch(started_with_flag: bool, uptime: std::time::Duration) -> bool {
    !started_with_flag || uptime >= std::time::Duration::from_secs(60)
}

/// Note FC4 (supersedes DN1's exit): called from the frame after a device loss, before egui-wgpu's paint would
/// panic. eframe cannot recreate the device, so the app writes the unsaved documents to their recovery files (the
/// caller has run `on_exit`), starts itself again on the same folder, and exits.
pub fn close_after_device_loss(folder: Option<&Path>) -> ! {
    let files = RECOVERY.lock().map(|r| r.clone()).unwrap_or_default();
    write_recovery(&files);
    let kept: Vec<String> = files
        .iter()
        .map(|(p, _)| recovery_path(p).display().to_string())
        .collect();
    if let Some(path) = RUN_LOG.get() {
        append(
            path,
            &format!(
                "{} closed after the device loss; recovery files: {kept:?}\n",
                now_stamp()
            ),
        );
    }
    let uptime = STARTED.get().map(|s| s.elapsed()).unwrap_or_default();
    if should_relaunch(
        AFTER_LOSS.load(std::sync::atomic::Ordering::Acquire),
        uptime,
    ) {
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title(crate::PRODUCT_NAME)
            .set_description(
                "The graphics device was lost. Tessalith restarts and brings your unsaved edits back.",
            )
            .set_buttons(rfd::MessageButtons::OkCustom("Restart".into()))
            .show();
        let spawned = std::env::current_exe().and_then(|exe| {
            std::process::Command::new(exe)
                .args(relaunch_args(folder))
                .spawn()
        });
        match spawned {
            Ok(_) => std::process::exit(0),
            Err(e) => {
                if let Some(path) = RUN_LOG.get() {
                    append(path, &format!("{} restart failed: {e}\n", now_stamp()));
                }
            }
        }
    }
    // A failed spawn, or a second loss within 60 s of a restart: FB5's text, then exit.
    let mut text = "The graphics device was lost (a driver reset, or a fault around alt-tab or screen capture), \
                    so Tessalith has to close."
        .to_string();
    if !kept.is_empty() {
        text += "\n\nUnsaved edits were kept and come back the next time you open the folder.";
    }
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title(crate::PRODUCT_NAME)
        .set_description(text)
        .show();
    std::process::exit(1)
}

pub fn set_recovery(files: Vec<(PathBuf, String)>) {
    if let Ok(mut r) = RECOVERY.lock() {
        *r = files;
    }
}

/// `sdc.toml` → `sdc.toml.recovery`, beside it.
pub fn recovery_path(file: &Path) -> PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(".recovery");
    PathBuf::from(name)
}

/// Writes each unsaved document to its recovery file; the original is left alone.
pub fn write_recovery(files: &[(PathBuf, String)]) {
    for (path, text) in files {
        let rec = recovery_path(path);
        // #47: an earlier crash's unrestored file is kept under a stamped name.
        // shortcut: the stamped copies are never cleaned up and get no banner line; add both if they pile up.
        if rec.exists() {
            let mut kept = rec.clone().into_os_string();
            kept.push(format!(".{}", now_stamp()));
            let _ = std::fs::rename(&rec, kept);
        }
        let _ = std::fs::write(rec, text);
    }
}

/// A saved document's recovery file is stale: remove it.
pub fn discard_recovery(file: &Path) {
    let _ = std::fs::remove_file(recovery_path(file));
}

/// What `file`'s recovery file holds (Note FB5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recovery {
    /// No file, or an older one than the document: leave it alone.
    None,
    /// Nothing to restore (empty, whitespace only, or the same text as the file or the document): delete it.
    Empty,
    /// Newer than the document (or the document is gone) and different: restore this text.
    Pending(String),
}

/// Line endings as `\n`, so a CRLF copy of the same text compares equal.
fn lf(s: &str) -> String {
    s.replace("\r\n", "\n")
}

/// Reads `file`'s recovery file. `doc_text` is the document as loaded, if any.
pub fn recovery_text(file: &Path, doc_text: Option<&str>) -> Recovery {
    recovery_text_held(file, doc_text, false)
}

/// [`recovery_text`]; `held` skips the age test: the document was read-only while the file waited, and fixing it by
/// hand made it newer than the file.
pub fn recovery_text_held(file: &Path, doc_text: Option<&str>, held: bool) -> Recovery {
    let rec = recovery_path(file);
    let Ok(text) = std::fs::read_to_string(&rec) else {
        return Recovery::None;
    };
    let on_disk = std::fs::read_to_string(file).ok();
    let norm = lf(&text);
    if norm.trim().is_empty()
        || [on_disk.as_deref(), doc_text]
            .into_iter()
            .flatten()
            .any(|t| lf(t) == norm)
    {
        return Recovery::Empty;
    }
    let rec_time = std::fs::metadata(&rec).and_then(|m| m.modified()).ok();
    let newer = held
        || std::fs::metadata(file)
            .and_then(|m| m.modified())
            .map_or(true, |t| rec_time.is_some_and(|r| r > t));
    if newer {
        Recovery::Pending(text)
    } else {
        Recovery::None
    }
}

/// The banner line for a read-only document that has crash edits waiting: they are not applied, and come back once
/// the file is fixed and reloaded (Note FB5). A document that can be edited gets no banner; its edits are restored.
pub fn recovery_note(file: &Path, doc_text: &str, held: bool) -> Option<String> {
    let Recovery::Pending(_) = recovery_text_held(file, Some(doc_text), held) else {
        return None;
    };
    let name = |p: &Path| {
        p.file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
    };
    Some(format!(
        "Unsaved edits from a crash are in {}; they come back when {} is fixed and reloaded.",
        name(&recovery_path(file)),
        name(file)
    ))
}

/// [`install_in`] the `%LOCALAPPDATA%\sdc\logs` folder.
pub fn install() {
    install_in(log_dir());
}

/// Install the hook: append the panic to this run's crash log in `dir`, write the unsaved documents to their
/// recovery files when the panic is on the main thread (a worker's panic is caught and shown, the app lives on), then
/// run the previous hook. Failures to write are ignored (a panic hook cannot report them anywhere).
pub fn install_in(dir: Option<PathBuf>) {
    let log = dir.map(|d| d.join(format!("crash-{}.log", now_stamp())));
    if let Some(path) = &log {
        let _ = RUN_LOG.set(path.clone());
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("(non-string payload)");
        let location = info
            .location()
            .map_or("(unknown)".to_string(), ToString::to_string);
        let thread = std::thread::current();
        let line = format_panic(
            thread.name().unwrap_or("(unnamed)"),
            message,
            &location,
            &std::backtrace::Backtrace::force_capture().to_string(),
            &now_stamp(),
        );
        if let Some(path) = &log {
            append(path, &line);
        }
        // try_lock: a panic while the snapshot is being replaced must not deadlock the hook.
        if thread.name() == Some("main") {
            if let Ok(files) = RECOVERY.try_lock() {
                write_recovery(&files);
            }
        }
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_names_thread_location_message_and_one_line_backtrace() {
        let l = format_panic("main", "boom", "a.rs:1:2", "0: f\n1: g\n", "T");
        assert_eq!(
            l,
            "T panic in thread 'main' at a.rs:1:2: boom\nbacktrace: 0: f\\n1: g\n"
        );
    }

    #[test]
    fn the_stamp_is_a_utc_date_and_time() {
        assert_eq!(stamp(0), "19700101-000000");
        assert_eq!(stamp(1_760_000_000), "20251009-085320");
        assert_eq!(stamp(951_782_400 + 3661), "20000229-010101");
    }

    fn age(p: &Path, secs: u64) {
        std::fs::File::options()
            .write(true)
            .open(p)
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs))
            .unwrap();
    }

    /// Note FB5: only a newer, non-empty, different recovery file is restored; an empty or equal one is deleted
    /// (Review Focus 2); an older one is left alone.
    #[test]
    fn recovery_text_sorts_files_into_pending_empty_and_none() {
        let d = tempfile::tempdir().unwrap();
        let doc = d.path().join("sdc.toml");
        let rec = recovery_path(&doc);
        assert_eq!(recovery_text(&doc, None), Recovery::None, "no file");
        std::fs::write(&doc, "a = 1\n").unwrap();
        age(&doc, 1_000);
        std::fs::write(&rec, "a = 2\n").unwrap();
        assert_eq!(
            recovery_text(&doc, Some("a = 1\n")),
            Recovery::Pending("a = 2\n".into())
        );
        std::fs::write(&rec, " \r\n\t").unwrap();
        assert_eq!(recovery_text(&doc, None), Recovery::Empty, "whitespace");
        std::fs::write(&rec, "a = 1\r\n").unwrap();
        assert_eq!(
            recovery_text(&doc, None),
            Recovery::Empty,
            "CRLF copy of the file"
        );
        std::fs::write(&rec, "b = 1\r\n").unwrap();
        assert_eq!(
            recovery_text(&doc, Some("b = 1\n")),
            Recovery::Empty,
            "equal to the document"
        );
        std::fs::write(&rec, "a = 2\n").unwrap();
        age(&rec, 10);
        assert_eq!(
            recovery_text(&doc, None),
            Recovery::None,
            "older than the file"
        );
        std::fs::remove_file(&doc).unwrap();
        assert_eq!(
            recovery_text(&doc, None),
            Recovery::Pending("a = 2\n".into()),
            "the document is gone"
        );
    }

    #[test]
    fn the_read_only_note_names_both_files() {
        let d = tempfile::tempdir().unwrap();
        let doc = d.path().join("sdc.toml");
        std::fs::write(recovery_path(&doc), "a = 2\n").unwrap();
        assert_eq!(
            recovery_note(&doc, "", false).as_deref(),
            Some("Unsaved edits from a crash are in sdc.toml.recovery; they come back when sdc.toml is fixed and reloaded.")
        );
    }

    /// Trace-debug 2026-10-16 #47: a second crash keeps the first crash's unrestored `.recovery` beside the new one.
    #[test]
    fn a_second_crash_keeps_the_first_unrestored_recovery() {
        let d = tempfile::tempdir().unwrap();
        let doc = d.path().join("sdc.toml");
        write_recovery(&[(doc.clone(), "A".into())]);
        write_recovery(&[(doc.clone(), "B".into())]);
        assert_eq!(std::fs::read_to_string(recovery_path(&doc)).unwrap(), "B");
        let kept: Vec<String> = std::fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p != &recovery_path(&doc))
            .map(|p| std::fs::read_to_string(p).unwrap())
            .collect();
        assert_eq!(kept, vec!["A".to_string()]);
    }

    /// Note FC4 (DN3): the restarted process reopens the folder that was open.
    #[test]
    fn relaunch_args_names_the_folder() {
        let args = relaunch_args(Some(Path::new("C:\\mods\\x")));
        assert_eq!(
            args,
            vec![
                std::ffi::OsString::from("--after-device-loss"),
                std::ffi::OsString::from("C:\\mods\\x")
            ]
        );
    }

    #[test]
    fn relaunch_args_is_empty_without_a_folder() {
        assert!(relaunch_args(None).is_empty());
    }

    /// Note FC4: a restarted process that loses the device again within 60 s does not restart again.
    #[test]
    fn no_second_relaunch_within_60s() {
        use std::time::Duration;
        assert!(should_relaunch(false, Duration::from_secs(1)));
        assert!(should_relaunch(false, Duration::from_secs(600)));
        assert!(!should_relaunch(true, Duration::from_secs(59)));
        assert!(should_relaunch(true, Duration::from_secs(60)));
    }
}
