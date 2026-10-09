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
