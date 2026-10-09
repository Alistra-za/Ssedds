//! Safe wrappers over the vendored BC encoders (bc7e, rgbcx, ISPC TC).
mod bc7;
mod ffi;
mod rgbcx;

pub use bc7::{bc7e_compress_blocks, Bc7Level, Bc7Params, BC7E_EXACT_OPAQUE_NATIVE};
pub use rgbcx::*;

use std::sync::OnceLock;

/// rgbcx BC1 decoder model; values equal `rgbcx::bc1_approx_mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum Bc1Model {
    Ideal = 0,
    Nvidia = 1,
    Amd = 2,
    IdealRound4 = 3,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InitError {
    #[error("encoders already initialised with BC1 model {active:?}; {requested:?} needs a new process (rgbcx::init is global)")]
    ModelMismatch {
        active: Bc1Model,
        requested: Bc1Model,
    },
}

static ACTIVE: OnceLock<Bc1Model> = OnceLock::new();

/// Initialise rgbcx and bc7e once per process. Later calls succeed only with the same model.
pub fn init(model: Bc1Model) -> Result<(), InitError> {
    let active = *ACTIVE.get_or_init(|| {
        unsafe { ffi::sdc_init(model as i32) };
        model
    });
    if active == model {
        Ok(())
    } else {
        Err(InitError::ModelMismatch {
            active,
            requested: model,
        })
    }
}

pub fn active_bc1_model() -> Option<Bc1Model> {
    ACTIVE.get().copied()
}

pub(crate) fn ensure_init() {
    let _ = ACTIVE.get_or_init(|| {
        unsafe { ffi::sdc_init(Bc1Model::Ideal as i32) };
        Bc1Model::Ideal
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Versions {
    pub bc7enc_rdo: &'static str,
    pub ispctc: &'static str,
    pub ispc: &'static str,
    pub bc7e_patch: Option<&'static str>,
}

pub fn versions() -> Versions {
    // The compiled shim reports whether the vendored bc7e header carries the exact-opaque patch.
    let patched = unsafe { ffi::sdc_bc7e_patched() } != 0;
    debug_assert_eq!(patched, cfg!(sdc_bc7e_patched));
    Versions {
        bc7enc_rdo: "b943862",
        ispctc: "79ddbc9",
        ispc: "1.31.0",
        bc7e_patch: if patched {
            Some("0001-exact-opaque-alpha")
        } else {
            None
        },
    }
}

/// Re-arrange a row-major RGBA8 image into 4x4 blocks (row-major blocks, row-major texels inside a block).
/// Texels past the right or bottom edge replicate the nearest edge texel.
pub fn gather_blocks(px: &[[u8; 4]], width: u32, height: u32) -> Vec<[u8; 4]> {
    let (w, h) = (width as usize, height as usize);
    assert!(w > 0 && h > 0, "empty image");
    assert_eq!(px.len(), w * h, "pixel count does not match width*height");
    let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
    let mut out = Vec::with_capacity(bw * bh * 16);
    for by in 0..bh {
        for bx in 0..bw {
            for y in 0..4 {
                let sy = (by * 4 + y).min(h - 1);
                for x in 0..4 {
                    let sx = (bx * 4 + x).min(w - 1);
                    out.push(px[sy * w + sx]);
                }
            }
        }
    }
    out
}
