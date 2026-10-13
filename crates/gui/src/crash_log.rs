//! Panic log: `sdc-gui.exe` has no console (`windows_subsystem = "windows"`), so a panic message would be lost.
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

/// Install the hook: append the panic to this run's crash log, then run the previous hook. Failures to write are
/// ignored (a panic hook cannot report them anywhere).
pub fn install() {
    let started = now_stamp();
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
