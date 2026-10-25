//! Tessalith DDS: the Photoshop plugin's bridge to the engine, as a C ABI (`photoshop/psdds.h`). Every exported
//! function checks its pointers and runs inside `catch_unwind`, so no panic crosses the boundary.
pub mod helpers;
pub mod rmaos;
pub mod settings;

pub use rmaos::{
    psdds_rmaos_default_quality, psdds_rmaos_encode, PsddsRmaosJob, PsddsRmaosResult, PsddsSource,
};
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

/// Bumped on any change to `photoshop/psdds.h`'s declarations or documented behaviour (pinned by tests/abi.rs).
pub const ABI_VERSION: u32 = 2;

/// The `PSDDS_ABI_VERSION` this library was built with.
#[no_mangle]
pub extern "C" fn psdds_abi_version() -> u32 {
    ABI_VERSION
}

/// The workspace (engine) version as a static NUL-terminated UTF-8 string.
#[no_mangle]
pub extern "C" fn psdds_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr().cast()
}

/// The dialog's start values for a texture type (0 Colour, 1 Normal, 2 Linear mask; anything else: Colour).
#[no_mangle]
pub extern "C" fn psdds_defaults(texture_type: u32) -> PsddsSettings {
    catch_unwind(|| settings::defaults(texture_type)).unwrap_or(PsddsSettings {
        format: 4,
        texture_type: 0,
        srgb: 0,
        filter: 0,
        mips: 1,
        quality: settings::FALLBACK_QUALITY,
        discard_alpha: 0,
    })
}

/// The number of seed presets (DC35).
#[no_mangle]
pub extern "C" fn psdds_preset_count() -> u32 {
    catch_unwind(|| settings::seed_presets().len() as u32).unwrap_or(0)
}

/// A seed preset's static UTF-8 name, or NULL when out of range.
#[no_mangle]
pub extern "C" fn psdds_preset_name(i: u32) -> *const c_char {
    catch_unwind(|| {
        settings::seed_presets()
            .get(i as usize)
            .map_or(std::ptr::null(), |p| p.0.as_ptr())
    })
    .unwrap_or(std::ptr::null())
}

/// A seed preset's settings in `out`: 0, or -1 for a null pointer or an index out of range (`out` untouched).
///
/// # Safety
/// `out` is null or valid for a `PsddsSettings`.
#[no_mangle]
pub unsafe extern "C" fn psdds_preset_settings(i: u32, out: *mut PsddsSettings) -> i32 {
    if out.is_null() {
        return -1;
    }
    match catch_unwind(|| settings::seed_presets().get(i as usize).map(|p| p.1)) {
        Ok(Some(s)) => {
            *out = s;
            0
        }
        _ => -1,
    }
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

/// DC25 for every host: pin the module holding this code (the DLL, or the .8bi linking psdds.lib) on the first call
/// that can start the engine's rayon pool, so FreeLibrary never unmaps code under parked workers. Nothing runs in
/// DllMain; a failure is ignored (the module then stays unpinned, as before).
fn pin_self() {
    static PIN: std::sync::Once = std::sync::Once::new();
    PIN.call_once(|| {
        #[cfg(windows)]
        {
            extern "system" {
                fn GetModuleHandleExW(
                    flags: u32,
                    addr: *const u16,
                    out: *mut *mut std::ffi::c_void,
                ) -> i32;
            }
            const PIN_FLAG: u32 = 0x1;
            const FROM_ADDRESS: u32 = 0x4;
            let mut module = std::ptr::null_mut();
            // SAFETY: FROM_ADDRESS takes a code address; `module` is a valid out pointer.
            unsafe {
                GetModuleHandleExW(PIN_FLAG | FROM_ADDRESS, pin_self as *const u16, &mut module)
            };
        }
    });
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
    // DC12 (I2): refused before the caller's buffer is touched.
    let n = helpers::check_size(width, height)?;
    let profile = settings::profile_for(&*s, has_alpha != 0)?;
    let img = Rgba32F {
        width,
        height,
        // `[f32; 4]` has f32's alignment, so the caller's float array is read as whole pixels.
        px: std::slice::from_raw_parts(rgba.cast::<[f32; 4]>(), n)
            .iter()
            .map(|p| p.map(clamp01))
            .collect(),
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
        pin_self();
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

/// The one input rule for float hosts (M8): [0,1], NaN -> 0, +inf -> 1, -inf -> 0.
pub(crate) fn clamp01(v: f32) -> f32 {
    if v.is_nan() {
        0.0
    } else {
        v.clamp(0.0, 1.0)
    }
}

/// 0 when `width` x `height` can be encoded or opened; else 1 and the reason in `err` (UTF-8, NUL-terminated,
/// truncated to `err_cap`).
///
/// # Safety
/// `err` is null or valid for `err_cap` bytes.
#[no_mangle]
pub unsafe extern "C" fn psdds_check_size(
    width: u32,
    height: u32,
    err: *mut c_char,
    err_cap: usize,
) -> i32 {
    match catch_unwind(|| helpers::check_size(width, height)) {
        Ok(Ok(_)) => 0,
        Ok(Err(m)) => {
            write_err(err, err_cap, &m);
            1
        }
        Err(e) => {
            write_err(err, err_cap, &panic_text(e));
            1
        }
    }
}

/// 1 when the first `len` bytes of a file show a DDS `psdds_decode` can open (see `helpers::dds_openable`); a NULL
/// `head` gives 0.
///
/// # Safety
/// `head` is null or valid for `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn psdds_dds_openable(head: *const u8, len: usize) -> i32 {
    // A len over isize::MAX would abort in from_raw_parts's check, which catch_unwind cannot catch.
    if head.is_null() || len > isize::MAX as usize {
        return 0;
    }
    catch_unwind(|| i32::from(helpers::dds_openable(std::slice::from_raw_parts(head, len))))
        .unwrap_or(0)
}

/// Open's alpha rule (DC23) over `pixels` 4-byte pixels; a NULL buffer or an overflowing size gives 0.
///
/// # Safety
/// `px8` is null or valid for `pixels * 4` bytes.
#[no_mangle]
pub unsafe extern "C" fn psdds_opens_with_alpha(
    px8: *const u8,
    pixels: usize,
    has_alpha: u32,
) -> i32 {
    match pixels.checked_mul(4) {
        Some(n) if !px8.is_null() && n <= isize::MAX as usize => {
            let px = std::slice::from_raw_parts(px8, n);
            catch_unwind(|| i32::from(helpers::opens_with_alpha(px, has_alpha != 0))).unwrap_or(0)
        }
        _ => 0,
    }
}

/// Save's alpha rule (DC31) over `pixels` RGBA float pixels; a NULL buffer or an overflowing size gives 0.
///
/// # Safety
/// `rgba` is null or valid for `pixels * 4` floats and 4-byte aligned.
#[no_mangle]
pub unsafe extern "C" fn psdds_alpha_used(rgba: *const f32, pixels: usize) -> i32 {
    match pixels.checked_mul(16) {
        Some(n) if !rgba.is_null() && n <= isize::MAX as usize => {
            let px = std::slice::from_raw_parts(rgba.cast::<[f32; 4]>(), pixels);
            catch_unwind(|| i32::from(helpers::alpha_used(px))).unwrap_or(0)
        }
        _ => 0,
    }
}

/// The bound a host applies before reading a DDS file (`psdds_decode` does not check it).
#[no_mangle]
pub extern "C" fn psdds_max_file_bytes() -> u64 {
    helpers::MAX_DDS_FILE_BYTES
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
    // A len over isize::MAX would abort in from_raw_parts's check, which catch_unwind cannot catch.
    if len > isize::MAX as usize {
        return Err("psdds_decode: len is larger than isize::MAX".into());
    }
    let bytes = std::slice::from_raw_parts(dds, len);
    // DC12 (I2) again: a header claiming a huge side is refused before `read_dds` copies the payload.
    let side = |i: usize| {
        bytes
            .get(i..i + 4)
            .map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    // A missing or zero side is left to `read_dds`, which names the real fault.
    if bytes.starts_with(b"DDS ") && side(12) != 0 && side(16) != 0 {
        helpers::check_size(side(16), side(12))?;
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
        pin_self();
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
/// Release a buffer from `psdds_encode`, `psdds_decode` or `psdds_rmaos_encode`. An empty buffer (null `data`) is a
/// no-op.
///
/// # Safety
/// `buf` came from one of those and is freed once.
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
