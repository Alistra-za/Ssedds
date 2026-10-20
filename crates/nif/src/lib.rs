//! Skyrim LE/SE NIF reader. Layer 0 (`scan`) reads only the header, shape heads and the small property blocks,
//! seeking by the header's block-size array (plan 01 Q1). Input is always `&[u8]`; loose files, BSA entries and
//! fixtures share one path. Layer 1 (`geom`) decodes SSE BSTriShape geometry for the 3D preview.
pub mod ctrl;
pub mod cursor;
pub mod fields;
pub mod geom;
pub mod graph;
pub mod header;
pub mod material;
pub mod net;
pub mod path;
pub mod props;
pub mod scan;
pub mod shape_head;
pub mod text;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum NifError {
    #[error("file ends early: needed {need} bytes at offset {at}")]
    Truncated { at: usize, need: usize },
    #[error("not a Skyrim LE/SE NIF (version {version:#x}, user {user}, BS version {bsver})")]
    NotSkyrim { version: u32, user: u32, bsver: u32 },
    #[error("implausible string count {0}")]
    TooManyStrings(u32),
    #[error("big-endian NIF")]
    BadEndian,
    #[error("no Gamebryo header line")]
    BadHeaderLine,
    #[error("block sizes do not add up to the file length ({expected} vs {actual})")]
    FileLength { expected: usize, actual: usize },
    #[error("block {block} ({kind}) declared {declared} bytes but decodes as {consumed}")]
    BlockSize {
        block: u32,
        kind: String,
        declared: u32,
        consumed: u32,
    },
    #[error("block {block}: {msg}")]
    Geometry { block: u32, msg: String },
}
