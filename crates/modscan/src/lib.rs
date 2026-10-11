pub mod archives;
pub mod mo2;
pub mod pgp;
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
}
