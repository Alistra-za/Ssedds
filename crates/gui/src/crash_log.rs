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

/// `%LOCALAPPDATA%\sdc\logs\crash-<stamp>.log` for this run.
fn log_path(stamp: &str) -> Option<PathBuf> {
    let dir = Path::new(&std::env::var_os("LOCALAPPDATA")?)
        .join("sdc")
        .join("logs");
    Some(dir.join(format!("crash-{stamp}.log")))
}

/// This run's crash log, set by [`install`].
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
        let _ = std::fs::write(recovery_path(path), text);
    }
}

/// Install the hook: append the panic to this run's crash log, write any unsaved documents to their recovery files,
/// then run the previous hook. Failures to write are ignored (a panic hook cannot report them anywhere).
pub fn install() {
    let started = now_stamp();
    if let Some(path) = log_path(&started) {
        let _ = RUN_LOG.set(path);
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
        if let Some(path) = log_path(&started) {
            append(&path, &line);
        }
        // try_lock: a panic while the snapshot is being replaced must not deadlock the hook.
        if let Ok(files) = RECOVERY.try_lock() {
            write_recovery(&files);
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
}
