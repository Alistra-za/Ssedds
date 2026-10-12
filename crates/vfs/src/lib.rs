//! Virtual file sources for the NIF scan: loose folders and SSE BSA v105 archives. FaceGen is skipped here.
pub mod bsa;
pub mod loose;

#[derive(Debug, thiserror::Error)]
pub enum VfsError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("not a BSA archive")]
    NotBsa,
    #[error("BSA version {0} is not Skyrim SE (105)")]
    UnsupportedBsaVersion(u32),
    #[error("corrupt archive: {0}")]
    Corrupt(String),
}
