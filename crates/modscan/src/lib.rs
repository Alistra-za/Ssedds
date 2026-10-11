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
    #[error("PGPatcher is running")]
    PgpRunning,
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
}
