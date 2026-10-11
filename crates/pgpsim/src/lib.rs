//! PGPatcher 1.2.0 TruePBR patch, simulated in memory for the 3D preview (P1 spec §2.1).
//! Rules: `F:\ai\dds\nifparseref\plan\p1\01-pgpatcher-truepbr-rules.md` ("01"). Pure: no GPU, no egui, no IO.
//! It stops at "the NIF as PGPatcher leaves it", a `MaterialState`; CS's load-time remap belongs to render3d.

// `json` and `slots` are wired into `simulate` by Task 4; until then only their unit tests use them.
#[allow(dead_code)]
mod json;
mod matching;
mod prep;
#[allow(dead_code)]
mod slots;
mod text;

pub use matching::{matched_path, shape_matches};
pub use nif::material::MaterialState;
pub use prep::prepare;
