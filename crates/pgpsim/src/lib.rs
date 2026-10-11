//! PGPatcher 1.2.0 TruePBR patch, simulated in memory for the 3D preview (P1 spec §2.1).
//! Rules: `F:\ai\dds\nifparseref\plan\p1\01-pgpatcher-truepbr-rules.md` ("01"). Pure: no GPU, no egui, no IO.
//! It stops at "the NIF as PGPatcher leaves it", a `MaterialState`; CS's load-time remap belongs to render3d.
mod matching;
mod prep;
mod text;

pub use matching::shape_matches;
pub use nif::material::MaterialState;
pub use prep::prepare;
