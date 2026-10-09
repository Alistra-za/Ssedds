//! Safe wrappers over the vendored BC encoders (bc7e, rgbcx, ISPC TC).
mod ffi;

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

#[allow(dead_code)] // first caller arrives with the safe encoder wrappers
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
