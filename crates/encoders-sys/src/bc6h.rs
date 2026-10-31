use crate::{ensure_init, ffi};

/// ISPC TC BC6H effort levels. The discriminants index `g_bc6` in the shim, which has no range check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bc6hLevel {
    Veryfast = 0,
    Fast = 1,
    Basic = 2,
    Slow = 3,
    Veryslow = 4,
    // Veryslow plus the peak-preserving mode (kernel.ispc sdc patch 0001): a block whose brightest texel would decode
    // below 0.9 of its source luminance gets a candidate that keeps it when one exists. Not mapped to any effort.
    VeryslowPeak = 5,
}

/// BC6H UF16 compression of a strip of `rows` rows (a multiple of 4) of a `width`-wide RGBA16F image.
/// Pixels are row-major raw IEEE half bits; output blocks are in raster order.
pub fn bc6h_compress_rows(
    rgba_f16: &[[u16; 4]],
    width: u32,
    rows: u32,
    out: &mut [u8],
    level: Bc6hLevel,
) {
    assert!(
        width > 0 && width.is_multiple_of(4) && rows.is_multiple_of(4),
        "width and rows must be a multiple of 4"
    );
    let (w, r) = (width as usize, rows as usize);
    assert_eq!(
        rgba_f16.len(),
        w * r,
        "pixel count does not match width*rows"
    );
    assert_eq!(
        out.len(),
        (w / 4) * (r / 4) * 16,
        "out must hold 16 bytes per block"
    );
    if rows == 0 {
        return;
    }
    let stride = i32::try_from(w * 8).expect("row stride exceeds i32");
    let rows_i = i32::try_from(rows).expect("rows exceed i32");
    let width_i = i32::try_from(width).expect("width exceeds i32");
    ensure_init();
    // SAFETY: `rgba_f16` holds `width * rows` RGBA16F texels (8 bytes each) and `stride` is the
    // row pitch in bytes, so the shim reads only within the slice; `out` holds `(width/4)*(rows/4)`
    // 16-byte blocks, the number ISPC TC writes; `level` comes from the closed `Bc6hLevel` enum
    // (0..=5), which the shim indexes without a range check.
    unsafe {
        ffi::sdc_bc6h_compress(
            rgba_f16.as_ptr() as *const u16,
            width_i,
            rows_i,
            stride,
            out.as_mut_ptr(),
            level as i32,
        )
    };
}
