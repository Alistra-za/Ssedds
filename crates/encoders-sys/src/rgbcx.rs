use crate::{ensure_init, ffi, Bc1Model};

/// Highest rgbcx encoder level (`rgbcx::MAX_LEVEL`).
pub const RGBCX_MAX_LEVEL: u32 = 18;
/// Search radius the bench used for `-3/-4/-5` (rdo_bc_params::m_bc345_search_rad = 5). Spec value `default`.
pub const BC45_RADIUS_DEFAULT: u32 = 5;
/// Spec value `max` (bench `-hr10`).
pub const BC45_RADIUS_MAX: u32 = 10;
/// Spec value `fast` (M6 Track D, DECISIONS Note T11): the smallest radius within 0.1 dB of `default` on the bench
/// corpus (docs/research/encoder-benchmark.md, "BC4/BC5 search radius").
pub const BC45_RADIUS_FAST: u32 = 4;

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

/// `rgbcx::BC4_USE_ALL_MODES` (8-value and 6-value modes).
pub const BC4_USE_ALL_MODES: u32 = 3;
/// `rgbcx::BC4_SDC_SEARCH` (sdc patch): the extra endpoint search after the mode loop (`sdc_bench::bc4ref::search`).
/// `bc3/4/5_encode_block` set it. It requires [`BC4_USE_ALL_MODES`]: the search assumes both modes are allowed and the
/// C++ does not check ([`bc4_encode_block_flags`] debug-asserts it).
pub const BC4_SDC_SEARCH: u32 = 4;
/// `rgbcx::BC4_SDC_GUARD128` (sdc patch): with [`BC4_SDC_SEARCH`], keep the mode-loop block when the search would flip
/// more texels across 128. `bc3_encode_block` sets it for alpha.
pub const BC4_SDC_GUARD128: u32 = 8;

/// Like [`bc4_encode_block`] but `flags` reaches `encode_bc4_hq` verbatim (the shim adds no bit of its own).
pub fn bc4_encode_block_flags(
    px: &[[u8; 4]; 16],
    channel: usize,
    search_radius: u32,
    flags: u32,
) -> [u8; 8] {
    assert!(channel < 4, "channel {channel} out of range");
    debug_assert!(flags & BC4_SDC_SEARCH == 0 || flags & BC4_USE_ALL_MODES == BC4_USE_ALL_MODES);
    ensure_init();
    let src = bytes(px);
    let mut out = [0u8; 8];
    // SAFETY: as in `bc4_encode_block`: stride 4 reads at most index 63 of `src`; `out` holds the 8 bytes written.
    unsafe {
        ffi::sdc_bc4_encode_flags(
            out.as_mut_ptr(),
            src.as_ptr().add(channel),
            4,
            search_radius,
            flags,
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

/// The 8-entry BC4 palette rgbcx's endpoint search assumes for endpoints `(e0, e1)` (`bc4_block::get_block_values`).
pub fn bc4_block_values(e0: u8, e1: u8) -> [u8; 8] {
    let mut out = [0u8; 8];
    // SAFETY: `out` is 8 bytes, the palette size `get_block_values` writes; the function is pure (no global state).
    unsafe { ffi::sdc_bc4_block_values(e0 as u32, e1 as u32, out.as_mut_ptr()) };
    out
}
