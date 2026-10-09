use crate::{ensure_init, ffi, Bc1Model};

/// Highest rgbcx encoder level (`rgbcx::MAX_LEVEL`).
pub const RGBCX_MAX_LEVEL: u32 = 18;
/// Search radius the bench used for `-3/-4/-5` (rdo_bc_params::m_bc345_search_rad = 5). Spec value `default`.
pub const BC45_RADIUS_DEFAULT: u32 = 5;
/// Spec value `max` (bench `-hr10`).
pub const BC45_RADIUS_MAX: u32 = 10;

fn bytes(px: &[[u8; 4]; 16]) -> [u8; 64] {
    let mut b = [0u8; 64];
    for (i, p) in px.iter().enumerate() {
        b[i * 4..i * 4 + 4].copy_from_slice(p);
    }
    b
}

/// Encode one BC1 block from 16 RGBA texels (alpha is ignored).
pub fn bc1_encode_block(
    px: &[[u8; 4]; 16],
    level: u32,
    allow_3color: bool,
    transparent_black: bool,
) -> [u8; 8] {
    assert!(level <= RGBCX_MAX_LEVEL, "rgbcx level {level} out of range");
    ensure_init();
    let src = bytes(px);
    let mut out = [0u8; 8];
    // SAFETY: `src` holds 64 bytes (16 RGBA texels) and `out` 8 bytes, the sizes
    // rgbcx::encode_bc1 reads and writes; `level` is range-checked above.
    unsafe {
        ffi::sdc_bc1_encode(
            out.as_mut_ptr(),
            src.as_ptr(),
            level,
            allow_3color as i32,
            transparent_black as i32,
        )
    };
    out
}

/// Encode one BC3 block (BC1 colour at `level` plus HQ BC4 alpha).
pub fn bc3_encode_block(px: &[[u8; 4]; 16], level: u32, search_radius: u32) -> [u8; 16] {
    assert!(level <= RGBCX_MAX_LEVEL, "rgbcx level {level} out of range");
    ensure_init();
    let src = bytes(px);
    let mut out = [0u8; 16];
    // SAFETY: `src` holds 64 bytes (16 RGBA texels) and `out` 16 bytes; `level` is range-checked above.
    unsafe { ffi::sdc_bc3_encode(out.as_mut_ptr(), src.as_ptr(), level, search_radius) };
    out
}

/// Encode one BC4 block from channel `channel` (0..=3) of the texels.
pub fn bc4_encode_block(px: &[[u8; 4]; 16], channel: usize, search_radius: u32) -> [u8; 8] {
    assert!(channel < 4, "channel {channel} out of range");
    ensure_init();
    let src = bytes(px);
    let mut out = [0u8; 8];
    // SAFETY: with stride 4 the encoder reads bytes `channel + 4 * i` for i in 0..16, at most
    // index 63 of `src`; `out` holds the 8 bytes written.
    unsafe {
        ffi::sdc_bc4_encode(
            out.as_mut_ptr(),
            src.as_ptr().add(channel),
            4,
            search_radius,
        )
    };
    out
}

/// Encode one BC5 block from the red (channel 0) and green (channel 1) texels.
pub fn bc5_encode_block(px: &[[u8; 4]; 16], search_radius: u32) -> [u8; 16] {
    ensure_init();
    let src = bytes(px);
    let mut out = [0u8; 16];
    // SAFETY: `src` holds 64 bytes (16 RGBA texels) and `out` 16 bytes (two BC4 blocks).
    unsafe { ffi::sdc_bc5_encode(out.as_mut_ptr(), src.as_ptr(), search_radius) };
    out
}

/// Decode one BC1 block with rgbcx under the given decoder model (validation and vendor previews).
pub fn bc1_decode_block(block: &[u8; 8], model: Bc1Model) -> [[u8; 4]; 16] {
    ensure_init();
    let mut px = [0u8; 64];
    // SAFETY: `block` is 8 bytes and `px` 64 bytes (16 RGBA texels); `model` comes from the closed
    // `Bc1Model` enum (0..=3), the values of `rgbcx::bc1_approx_mode`.
    unsafe { ffi::sdc_bc1_decode(block.as_ptr(), px.as_mut_ptr(), model as i32) };
    let mut out = [[0u8; 4]; 16];
    for (i, o) in out.iter_mut().enumerate() {
        o.copy_from_slice(&px[i * 4..i * 4 + 4]);
    }
    out
}
