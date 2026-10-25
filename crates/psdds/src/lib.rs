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
        quality: 0,
    })
}

/// Apply the greying rules: `effective` gets the values the dialog shows and the encoder uses, `controls` which
/// controls are editable. 0, or -1 for a null pointer or an out-of-range field (outputs untouched).
///
/// # Safety
/// Each pointer is null or valid for its type.
#[no_mangle]
pub unsafe extern "C" fn psdds_resolve(
    input: *const PsddsSettings,
    effective: *mut PsddsSettings,
    controls: *mut PsddsControls,
) -> i32 {
    if input.is_null() || effective.is_null() || controls.is_null() {
        return -1;
    }
    let s = *input;
    match catch_unwind(|| settings::resolve(&s)) {
        Ok(Ok((e, c))) => {
            *effective = e;
            *controls = c;
            0
        }
        _ => -1,
    }
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
    let n = (width as usize)
        .checked_mul(height as usize)
        .filter(|n| n.checked_mul(4 * 4).is_some())
        .ok_or_else(|| format!("psdds_encode: image {width}x{height} is too large"))?;
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

/// Release a buffer from `psdds_encode`. An empty buffer (null `data`) is a no-op.
///
/// # Safety
/// `buf` came from `psdds_encode` and is freed once.
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
