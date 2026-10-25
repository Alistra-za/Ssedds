//! `psdds_rmaos_encode`: the C adapter for a TruePBR `_rmaos` + `_n` pair. Every packing, fill, default, size and bake
//! rule is the engine's (`engine::memory::encode_rmaos_set`); this file only checks the C inputs, clamps them (M8) and
//! builds the two profiles.
use crate::settings::{self, PsddsSettings};
use crate::{clamp01, helpers, panic_text, pin_self, write_err, PsddsBuf};
use engine::config::{MipKeyword, MipLevels, Profile, ResizeMode, Subtype};
use engine::image::Rgba32F;
use engine::memory::{encode_rmaos_set, Channel};
use std::ffi::c_char;
use std::panic::{catch_unwind, AssertUnwindSafe};

/// One channel of an interleaved float image: pixel i's value is `px[i * channels + channel]`. `px` NULL = no source.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PsddsSource {
    pub px: *const f32,
    pub width: u32,
    pub height: u32,
    pub channels: u32,
    pub channel: u32,
}

/// The inputs of one pair (see `photoshop/psdds.h`). Role order: roughness, metallic, AO, specular.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PsddsRmaosJob {
    pub source: [PsddsSource; 4],
    /// -1 unset (neutral default), 0..=255 the fill for that role.
    pub fill: [i32; 4],
    /// BC7 effort index; out of range = the profile's.
    pub quality: u32,
    /// 0 top level only, 1 the rmaos profile's chain.
    pub mips: u32,
    /// RGBA floats of the `_n`, or NULL.
    pub normal: *const f32,
    pub normal_width: u32,
    pub normal_height: u32,
    pub normal_settings: PsddsSettings,
}

/// The pair; release both buffers with `psdds_free`.
#[repr(C)]
#[derive(Debug)]
pub struct PsddsRmaosResult {
    pub rmaos: PsddsBuf,
    pub normal: PsddsBuf,
    /// Bit `1 << c` for each channel R, G, B, A that took a neutral default.
    pub defaulted: u32,
    /// 1 when the `_n`'s variance was baked into the roughness mips.
    pub baked: u32,
}

const ROLES: [&str; 4] = ["roughness", "metallic", "ao", "specular"];

/// The `_rmaos` profile: `cs-quality`'s, with the BC7 effort of list index `quality` (out of range: the profile's) and
/// mips 0 = top level only.
pub fn rmaos_profile(quality: u32, mips: u32) -> Result<Profile, String> {
    if mips > 1 {
        return Err(format!("mips {mips} out of range 0..2"));
    }
    let mut p = settings::base_of(Subtype::Rmaos)?.clone();
    if let Some(e) = settings::EFFORTS.get(quality as usize) {
        p.encoder.effort = e.0;
    }
    if mips == 0 {
        p.mips.levels = MipLevels::Keyword(MipKeyword::None);
    }
    p.resize = ResizeMode::None;
    p.drop_top_mip = false;
    Ok(p)
}

/// The `_n` profile: the dialog's Normal settings over the `pbr_normal` base (R12), resolved as `psdds_resolve` does.
pub fn normal_profile(ns: &PsddsSettings) -> Result<Profile, String> {
    if ns.texture_type != 1 {
        return Err("normal_settings must use texture type Normal".into());
    }
    // No image alpha in a `_n`: `build` removes it, so discard_alpha (checked by resolve) changes nothing (R19).
    settings::profile_for_base(ns, false, Subtype::PbrNormal)
}

/// # Safety
/// As `psdds_rmaos_encode`.
unsafe fn encode(
    job: *const PsddsRmaosJob,
    result: *mut PsddsRmaosResult,
    note: *mut c_char,
    note_cap: usize,
) -> Result<(), String> {
    if job.is_null() || result.is_null() {
        return Err("psdds_rmaos_encode: null pointer argument".into());
    }
    let j = *job;
    // Sizes and channels are checked before any source is read (M3).
    let mut checked = [None; 4];
    for (k, s) in j.source.iter().enumerate() {
        if s.px.is_null() {
            continue;
        }
        let n = helpers::check_size(s.width, s.height)?;
        if !(1..=4).contains(&s.channels) || s.channel >= s.channels {
            return Err(format!(
                "{} channel {} is not in a {}-channel image",
                ROLES[k], s.channel, s.channels
            ));
        }
        checked[k] = Some((*s, n * s.channels as usize));
    }
    let normal_n = if j.normal.is_null() {
        None
    } else {
        Some(helpers::check_size(j.normal_width, j.normal_height)?)
    };
    let mut fill = [None; 4];
    for (k, &v) in j.fill.iter().enumerate() {
        fill[k] = match v {
            -1 => None,
            0..=255 => Some(v as u8),
            _ => return Err(format!("fill for {} is {v}, not -1 or 0..255", ROLES[k])),
        };
    }
    let rp = rmaos_profile(j.quality, j.mips)?;
    let np = normal_profile_if(&j, normal_n)?;
    // M8: every value clamped in the copy, as psdds_encode's input. Roles sharing one image share one copy.
    let key = |s: &PsddsSource| (s.px, s.width, s.height, s.channels);
    let mut images: Vec<(PsddsSource, Vec<f32>)> = vec![];
    let mut at = [None; 4];
    for (k, c) in checked.iter().enumerate() {
        let Some((s, len)) = c else { continue };
        at[k] = Some(match images.iter().position(|(o, _)| key(o) == key(s)) {
            Some(i) => i,
            None => {
                let v = std::slice::from_raw_parts(s.px, *len)
                    .iter()
                    .map(|&x| clamp01(x))
                    .collect();
                images.push((*s, v));
                images.len() - 1
            }
        });
    }
    let src: [Option<Channel>; 4] = std::array::from_fn(|k| {
        at[k].map(|i| Channel {
            px: &images[i].1,
            width: j.source[k].width,
            height: j.source[k].height,
            channels: j.source[k].channels,
            channel: j.source[k].channel,
        })
    });
    let normal = np.map(|(n, p)| {
        let px = std::slice::from_raw_parts(j.normal.cast::<[f32; 4]>(), n)
            .iter()
            .map(|q| q.map(clamp01))
            .collect();
        let img = Rgba32F {
            width: j.normal_width,
            height: j.normal_height,
            px,
        };
        (img, p)
    });
    let set = encode_rmaos_set(src, fill, &rp, normal)?;
    write_err(note, note_cap, set.bake_note.as_deref().unwrap_or(""));
    let boxed = |v: Vec<u8>| {
        let b = v.into_boxed_slice();
        let len = b.len();
        PsddsBuf {
            data: Box::into_raw(b).cast::<u8>(),
            len,
        }
    };
    // Written last, so an error or panic above leaves `result` empty.
    *result = PsddsRmaosResult {
        rmaos: boxed(set.rmaos),
        normal: set.normal.map_or(empty(), boxed),
        defaulted: set.defaulted.iter().map(|&c| 1u32 << c).sum(),
        baked: u32::from(set.baked),
    };
    Ok(())
}

/// The `_n` pixel count and profile when a normal is supplied.
fn normal_profile_if(
    j: &PsddsRmaosJob,
    n: Option<usize>,
) -> Result<Option<(usize, Profile)>, String> {
    n.map(|n| Ok((n, normal_profile(&j.normal_settings)?)))
        .transpose()
}

fn empty() -> PsddsBuf {
    PsddsBuf {
        data: std::ptr::null_mut(),
        len: 0,
    }
}

/// Encode a TruePBR pair (see `photoshop/psdds.h`). 0 on success; otherwise non-zero, both buffers empty and the
/// reason in `err`. `note` gets the engine's reason when the `_n` was not baked, empty otherwise.
///
/// # Safety
/// `job` and `result` are null or valid; each non-null source `px` holds `width * height * channels` floats and the
/// normal `normal_width * normal_height * 4`; `note` and `err` are null or valid for their caps.
#[no_mangle]
pub unsafe extern "C" fn psdds_rmaos_encode(
    job: *const PsddsRmaosJob,
    result: *mut PsddsRmaosResult,
    note: *mut c_char,
    note_cap: usize,
    err: *mut c_char,
    err_cap: usize,
) -> i32 {
    if !result.is_null() {
        *result = PsddsRmaosResult {
            rmaos: empty(),
            normal: empty(),
            defaulted: 0,
            baked: 0,
        };
    }
    write_err(note, note_cap, "");
    let r = catch_unwind(AssertUnwindSafe(|| {
        pin_self();
        encode(job, result, note, note_cap)
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

/// The `_rmaos` profile's BC7 effort as an index of `psdds_quality_label(PSDDS_BC7, i)`.
#[no_mangle]
pub extern "C" fn psdds_rmaos_default_quality() -> u32 {
    catch_unwind(|| {
        settings::base_of(Subtype::Rmaos).map_or(settings::FALLBACK_QUALITY, |p| {
            settings::preset_quality(4, p)
        })
    })
    .unwrap_or(settings::FALLBACK_QUALITY)
}
