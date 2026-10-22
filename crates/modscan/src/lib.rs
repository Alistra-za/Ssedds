pub mod archives;
pub mod mo2;
pub mod pgp;
pub mod plugin_store;
pub mod plugins;
pub mod qtini;
pub mod resolve;
pub mod scan;
pub mod store;
#[cfg(windows)]
pub mod win;

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("no MO2 instance: {0}")]
    NoInstance(String),
    #[error("MO2 settings: {0}")]
    Ini(String),
    #[error("scan cancelled")]
    Cancelled,
    #[error("PGPatcher is running")]
    PgpRunning,
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
    /// `Store::open_read_only` found another schema version; only a scan (read-write open) rebuilds it.
    #[error(
        "scan database version {found}, this build reads {want}: scan again in the Meshes view"
    )]
    SchemaVersion { found: i64, want: i64 },
    /// Another scan holds `scan.lock` next to this store.
    #[error("another scan is already running on this scan database")]
    Busy,
}

/// Reads a game text file: a UTF-8 BOM is dropped and each line decodes on its own, as UTF-8 when valid and as
/// Latin-1 otherwise, so one ANSI name does not garble a UTF-8 one (Note DA).
// shortcut: Latin-1, not Windows-1252, so bytes 0x80-0x9F become control characters; upgrade with a 1252 table.
pub(crate) fn read_text(p: &std::path::Path) -> std::io::Result<String> {
    let b = std::fs::read(p)?;
    let b = b.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&b);
    let mut out = String::with_capacity(b.len());
    for line in b.split_inclusive(|&c| c == b'\n') {
        match std::str::from_utf8(line) {
            Ok(s) => out.push_str(s),
            Err(_) => out.extend(line.iter().map(|&c| c as char)),
        }
    }
    Ok(out)
}
