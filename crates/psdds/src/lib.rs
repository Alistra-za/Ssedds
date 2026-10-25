//! Tessalith DDS: the Photoshop plugin's bridge to the engine, as a C ABI (`photoshop/psdds.h`). Every exported
//! function checks its pointers and runs inside `catch_unwind`, so no panic crosses the boundary.
pub mod settings;

pub use settings::{PsddsControls, PsddsSettings};

use engine::image::Rgba32F;
use std::ffi::{c_char, CStr};
use std::panic::{catch_unwind, AssertUnwindSafe};

/// An encoded file owned by Rust; release it with `psdds_free`.
#[repr(C)]
#[derive(Debug)]
pub struct PsddsBuf {
    pub data: *mut u8,
    pub len: usize,
}

fn panic_text(e: Box<dyn std::any::Any + Send>) -> String {
    let m = e
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| e.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panic".into());
    format!("internal error: {m}")
}

/// Copy `msg` into `err` as UTF-8, NUL-terminated, cut at a character boundary to fit `cap`.
///
/// # Safety
/// `err` is null or valid for `cap` bytes.
unsafe fn write_err(err: *mut c_char, cap: usize, msg: &str) {
    if err.is_null() || cap == 0 {
        return;
    }
    let mut n = msg.len().min(cap - 1);
    while !msg.is_char_boundary(n) {
        n -= 1;
    }
    std::ptr::copy_nonoverlapping(msg.as_ptr(), err.cast::<u8>(), n);
    *err.add(n) = 0;
}

/// The dialog's start values for a texture type (0 Colour, 1 Normal, 2 Linear mask; anything else: Colour).
#[no_mangle]
pub extern "C" fn psdds_defaults(texture_type: u32) -> PsddsSettings {
    catch_unwind(|| settings::defaults(texture_type)).unwrap_or(PsddsSettings {
        format: 4,
        texture_type: 0,
        srgb: 1,
        filter: 0,
        mips: 1,
        quality: settings::FALLBACK_QUALITY,
    })
}

/// Apply the greying rules: `effective` gets the values the dialog shows and the encoder uses, `controls` which
/// controls are editable. 0, or -1 for a null pointer or an out-of-range field other than quality (outputs
/// untouched); a format the type does not offer becomes the type's default, and a quality index outside the
/// format's list becomes the preset's.
///
/// # Safety
/// Each pointer is null or valid for its type.
#[no_mangle]
pub unsafe extern "C" fn psdds_resolve(
    input: *const PsddsSettings,
    has_alpha: u32,
    effective: *mut PsddsSettings,
    controls: *mut PsddsControls,
) -> i32 {
    if input.is_null() || effective.is_null() || controls.is_null() {
        return -1;
    }
    let s = *input;
    match catch_unwind(|| settings::resolve(&s, has_alpha != 0)) {
        Ok(Ok((e, c))) => {
            *effective = e;
            *controls = c;
            0
        }
        _ => -1,
    }
}

/// 1 when the texture type offers the format (the dialog lists only these), else 0 (also for unknown values).
#[no_mangle]
pub extern "C" fn psdds_format_allowed(texture_type: u32, format: u32) -> u32 {
    catch_unwind(|| u32::from(settings::format_allowed(texture_type, format))).unwrap_or(0)
}

/// Entries in a format's quality list; 0 for RGBA8 (quality greyed) and an unknown format.
#[no_mangle]
pub extern "C" fn psdds_quality_count(format: u32) -> u32 {
    catch_unwind(|| settings::quality_labels(format).len() as u32).unwrap_or(0)
}

/// A static UTF-8 label of a format's quality list, or NULL when out of range.
#[no_mangle]
pub extern "C" fn psdds_quality_label(format: u32, i: u32) -> *const c_char {
    catch_unwind(|| {
        settings::quality_labels(format)
            .get(i as usize)
            .map_or(std::ptr::null(), |l: &&CStr| l.as_ptr())
    })
    .unwrap_or(std::ptr::null())
}

/// The largest side Skyrim (D3D11) loads.
const MAX_SIDE: u32 = 16384;
const TOO_LARGE: &str =
    "This image is larger than 16384 pixels on a side, the most Skyrim supports.";

/// # Safety
/// As `psdds_encode`.
unsafe fn encode(
    rgba: *const f32,
    width: u32,
    height: u32,
    has_alpha: u32,
    s: *const PsddsSettings,
    out: *mut PsddsBuf,
) -> Result<(), String> {
    if rgba.is_null() || s.is_null() || out.is_null() {
        return Err("psdds_encode: null pointer argument".into());
    }
    if width == 0 || height == 0 {
        return Err(format!("psdds_encode: image is {width}x{height}"));
    }
    // DC12 (I2): refused before the caller's buffer is touched. The isize bound is `from_raw_parts`' precondition
    // (already implied by the side limit on 64-bit).
    let n = (width <= MAX_SIDE && height <= MAX_SIDE)
        .then(|| (width as usize).checked_mul(height as usize))
        .flatten()
        .filter(|n| n.checked_mul(16).is_some_and(|b| b <= isize::MAX as usize))
        .ok_or(TOO_LARGE)?;
    let profile = settings::profile_for(&*s, has_alpha != 0)?;
    let img = Rgba32F {
        width,
        height,
        // `[f32; 4]` has f32's alignment, so the caller's float array is read as whole pixels.
        px: std::slice::from_raw_parts(rgba.cast::<[f32; 4]>(), n).to_vec(),
    };
    let bytes = engine::memory::encode_image(img, &profile)?.into_boxed_slice();
    let len = bytes.len();
    *out = PsddsBuf {
        data: Box::into_raw(bytes).cast::<u8>(),
        len,
    };
    Ok(())
}

/// Encode `width * height` RGBA float pixels (row-major, [0, 1], already scaled by the caller) to a whole DDS file
/// in `out`. 0 on success; otherwise non-zero, `out` set empty when non-null, and the reason in `err`
/// (UTF-8, NUL-terminated, truncated to `err_cap`).
///
/// # Safety
/// `rgba` is null or valid for `width * height * 4` floats; `settings` and `out` are null or valid; `err` is null
/// or valid for `err_cap` bytes.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn psdds_encode(
    rgba: *const f32,
    width: u32,
    height: u32,
    has_alpha: u32,
    settings: *const PsddsSettings,
    out: *mut PsddsBuf,
    err: *mut c_char,
    err_cap: usize,
) -> i32 {
    if !out.is_null() {
        *out = PsddsBuf {
            data: std::ptr::null_mut(),
            len: 0,
        };
    }
    let r = catch_unwind(AssertUnwindSafe(|| {
        encode(rgba, width, height, has_alpha, settings, out)
    }))
    .unwrap_or_else(|e| Err(panic_text(e)));
    match r {
        Ok(()) => 0,
        Err(m) => {
            // A panic after `*out` was set is impossible (the write is last), so `out` stays empty here.
            write_err(err, err_cap, &m);
            1
        }
    }
}

const HDR_REFUSAL: &str = "This DDS holds 32-bit (HDR) data, which this plugin does not open yet.";

/// # Safety
/// As `psdds_decode`.
unsafe fn decode(
    dds: *const u8,
    len: usize,
    width: *mut u32,
    height: *mut u32,
    has_alpha: *mut u32,
    out: *mut PsddsBuf,
) -> Result<(), String> {
    use engine::dds::DdsPixelFormat::*;
    if dds.is_null() || width.is_null() || height.is_null() || has_alpha.is_null() || out.is_null()
    {
        return Err("psdds_decode: null pointer argument".into());
    }
    let bytes = std::slice::from_raw_parts(dds, len);
    // DC12 (I2) again: a header claiming a huge side is refused before `read_dds` copies the payload.
    let side = |i: usize| {
        bytes
            .get(i..i + 4)
            .map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    if bytes.starts_with(b"DDS ") && (side(12) > MAX_SIDE || side(16) > MAX_SIDE) {
        return Err(TOO_LARGE.into());
    }
    let f = engine::dds::read_dds(bytes).map_err(|e| e.to_string())?;
    if matches!(f.format, Bc6hUf16 | Bc6hSf16 | Rgba16f | Rgba32f) {
        return Err(HDR_REFUSAL.into());
    }
    if f.is_cubemap || f.caps2 & 0x200 != 0 || f.dx10.is_some_and(|d| d.misc & 0x4 != 0) {
        return Err("This DDS is a cube map, which this plugin does not open.".into());
    }
    if f.dx10.is_some_and(|d| d.array_size > 1) {
        return Err("This DDS is a texture array, which this plugin does not open.".into());
    }
    let (w, h, range) = f
        .levels
        .first()
        .ok_or("This DDS has no mip levels.")?
        .clone();
    let engine::decode::Decoded::Ldr(px) =
        engine::decode::decode_level(f.format, w, h, &f.data[range], None)
            .map_err(|e| e.to_string())?
    else {
        return Err(HDR_REFUSAL.into());
    };
    // A failed allocation aborts the host, so the output buffer is reserved fallibly.
    let mut rgba = Vec::new();
    rgba.try_reserve_exact(px.len() * 4)
        .map_err(|_| "Not enough memory to open this DDS.")?;
    let stores_alpha = matches!(
        f.format,
        Bc2 { .. } | Bc3 { .. } | Bc7 { .. } | Rgba8 { .. } | Bgra8 { .. }
    );
    let mut any_clear = false;
    for p in &px {
        // Formats without stored alpha are opaque; BC1's 1-bit alpha is real only where a texel uses it.
        let a = if stores_alpha || matches!(f.format, Bc1 { .. }) {
            p[3]
        } else {
            255
        };
        any_clear |= a < 255;
        rgba.extend_from_slice(&[p[0], p[1], p[2], a]);
    }
    drop(px);
    let rgba = rgba.into_boxed_slice();
    let len = rgba.len();
    *width = w;
    *height = h;
    *has_alpha = u32::from(stores_alpha || any_clear);
    *out = PsddsBuf {
        data: Box::into_raw(rgba).cast::<u8>(),
        len,
    };
    Ok(())
}

/// Decode the top mip of a DDS file to `*width * *height` RGBA8 pixels (row-major, `width * height * 4` bytes) in
/// `out`; release it once with `psdds_free`. `has_alpha` is 1 when the format stores alpha. 0 on success; otherwise
/// non-zero, `out` set empty when non-null, and the reason in `err` (UTF-8, NUL-terminated, truncated to `err_cap`).
/// HDR formats, cube maps and texture arrays are refused with a message; so is a side over 16384.
///
/// # Safety
/// `dds` is null or valid for `len` bytes; `width`, `height`, `has_alpha` and `out` are null or valid; `err` is
/// null or valid for `err_cap` bytes.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn psdds_decode(
    dds: *const u8,
    len: usize,
    width: *mut u32,
    height: *mut u32,
    has_alpha: *mut u32,
    out: *mut PsddsBuf,
    err: *mut c_char,
    err_cap: usize,
) -> i32 {
    if !out.is_null() {
        *out = PsddsBuf {
            data: std::ptr::null_mut(),
            len: 0,
        };
    }
    let r = catch_unwind(AssertUnwindSafe(|| {
        decode(dds, len, width, height, has_alpha, out)
    }))
    .unwrap_or_else(|e| Err(panic_text(e)));
    match r {
        Ok(()) => 0,
        Err(m) => {
            write_err(err, err_cap, &m);
            1
        }
    }
}
/// Release a buffer from `psdds_encode` or `psdds_decode`. An empty buffer (null `data`) is a no-op.
///
/// # Safety
/// `buf` came from `psdds_encode` or `psdds_decode` and is freed once.
#[no_mangle]
pub unsafe extern "C" fn psdds_free(buf: PsddsBuf) {
    if buf.data.is_null() {
        return;
    }
    let _ = catch_unwind(|| {
        drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
            buf.data, buf.len,
        )))
    });
}
