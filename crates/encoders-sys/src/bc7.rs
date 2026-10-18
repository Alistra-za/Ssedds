use crate::{ensure_init, ffi};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bc7Level {
    Ultrafast = 0,
    Veryfast = 1,
    Fast = 2,
    Basic = 3,
    Slow = 4,
    Veryslow = 5,
    Slowest = 6,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bc7Params {
    pub level: Bc7Level,
    pub perceptual: bool,
    /// Force mode-6 p-bits to 1 on all-255 blocks (needs the patched bc7e objects, Task 14).
    pub exact_opaque: bool,
}

/// True once the vendored bc7e objects carry patch 0001 (exact opaque alpha).
pub const BC7E_EXACT_OPAQUE_NATIVE: bool = cfg!(sdc_bc7e_patched);

/// Compress `pixels.len() / 16` blocks. `pixels` holds 16 texels per block (see `gather_blocks`).
pub fn bc7e_compress_blocks(pixels: &[[u8; 4]], out: &mut [u8], p: Bc7Params) {
    assert_eq!(pixels.len() % 16, 0, "pixels must hold whole 4x4 blocks");
    let n = pixels.len() / 16;
    assert_eq!(out.len(), n * 16, "out must hold 16 bytes per block");
    assert!(
        !p.exact_opaque || BC7E_EXACT_OPAQUE_NATIVE,
        "exact_opaque needs the patched bc7e objects; use bc7_fixup::force_opaque_mode6 instead"
    );
    if n == 0 {
        return;
    }
    ensure_init();
    // Copy into u32/u64 buffers so the ISPC code sees naturally aligned data.
    let src: Vec<u32> = pixels.iter().map(|c| u32::from_le_bytes(*c)).collect();
    let mut dst = vec![0u64; n * 2];
    // SAFETY: `src` holds 16 * n texels and `dst` 2 * n u64 (16 bytes per block); `level` comes
    // from the closed `Bc7Level` enum (0..=6), which the shim indexes without a range check.
    unsafe {
        ffi::sdc_bc7e_compress(
            n as u32,
            dst.as_mut_ptr(),
            src.as_ptr(),
            p.level as i32,
            p.perceptual as i32,
            p.exact_opaque as i32,
        )
    };
    for (i, w) in dst.iter().enumerate() {
        out[i * 8..i * 8 + 8].copy_from_slice(&w.to_le_bytes());
    }
}

/// Per-field overrides of a level's bc7e parameters. Measurement only (the sdc-bench BC7 tuning study,
/// docs/research/bc7e-tuning.md); the engine never uses it. `None` keeps the level's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Bc7Tune {
    /// Partitions given a full encode after the cheap estimate (modes 0, 1/3, 2 and 7 together).
    pub partitions_to_try: Option<u32>,
    /// bc7e "uber" refinement level, 0..=4.
    pub uber_level: Option<u32>,
    /// Least-squares refinement passes after the first fit.
    pub refinement_passes: Option<u32>,
    /// Try every p-bit combination instead of rounding each endpoint to its nearest one.
    pub pbit_search: Option<bool>,
    /// Linear per-channel error weights (used with `perceptual = false`).
    pub weights: Option<[u32; 4]>,
    /// Partitions the cheap estimate ranks for modes 1, 3 and 7 (max 64).
    pub max_partitions_2subset: Option<u32>,
    /// Partitions the cheap estimate ranks for mode 0 (max 16) and mode 2 (max 64).
    pub max_partitions_3subset: Option<(u32, u32)>,
}

/// `bc7e_compress_blocks` with `tune` applied over the level's parameters.
pub fn bc7e_compress_blocks_tuned(pixels: &[[u8; 4]], out: &mut [u8], p: Bc7Params, tune: Bc7Tune) {
    assert_eq!(pixels.len() % 16, 0, "pixels must hold whole 4x4 blocks");
    let n = pixels.len() / 16;
    assert_eq!(out.len(), n * 16, "out must hold 16 bytes per block");
    assert!(
        !p.exact_opaque || BC7E_EXACT_OPAQUE_NATIVE,
        "exact_opaque needs the patched bc7e objects"
    );
    if n == 0 {
        return;
    }
    ensure_init();
    let o = |v: Option<u32>| v.map_or(-1, |x| x as i32);
    let w = tune.weights;
    let m3 = tune.max_partitions_3subset;
    let t: [i32; 12] = [
        o(tune.partitions_to_try),
        o(tune.uber_level),
        o(tune.refinement_passes),
        tune.pbit_search.map_or(-1, |b| b as i32),
        o(w.map(|w| w[0])),
        o(w.map(|w| w[1])),
        o(w.map(|w| w[2])),
        o(w.map(|w| w[3])),
        o(tune.max_partitions_2subset),
        o(m3.map(|m| m.0)),
        o(m3.map(|m| m.1)),
        -1,
    ];
    let src: Vec<u32> = pixels.iter().map(|c| u32::from_le_bytes(*c)).collect();
    let mut dst = vec![0u64; n * 2];
    // SAFETY: as in `bc7e_compress_blocks`; `t` is 12 ints, which is what the shim reads.
    unsafe {
        ffi::sdc_bc7e_compress_tuned(
            n as u32,
            dst.as_mut_ptr(),
            src.as_ptr(),
            p.level as i32,
            p.perceptual as i32,
            p.exact_opaque as i32,
            t.as_ptr(),
        )
    };
    for (i, w) in dst.iter().enumerate() {
        out[i * 8..i * 8 + 8].copy_from_slice(&w.to_le_bytes());
    }
}
