//! One-shot encode for the Python bench runners (run_bench*.py): engine paths the external CLIs lack, written as a
//! single-level DX10 DDS the runners' decoders read.
use crate::codec::encode_bc6h_image;
use crate::corpus::{load_f16, load_png_rgba8};
use anyhow::{anyhow, bail, Result};
use encoders_sys::{Bc1Model, Bc6hLevel};
use engine::config::{Effort, Format, Profile, SearchRadius, Subtype, Target};
use engine::dds::{dds_bytes, DdsDesc, ResolvedHeader};
use engine::encode::{encode_level, EncodeCtx};
use engine::presets::{PresetRegistry, DEFAULT_PRESET};
use engine::quantize::EncoderInput;
use std::path::Path;

/// The default preset's profile the engine encodes `format` with at `level` ("max" for BC7, "default" for BC3/4/5).
/// Every bench set's sub-type resolves to the same encoder fields (sdc-base.toml: perceptual off, no bc7_weights,
/// rgbcx level 18, BC4/BC5 radius "default"; exact_opaque, the zero-alpha fix and the grass guard only on
/// envmask / complex_material / complex_grass), so one representative sub-type per format stands for them.
/// BC3 has no built-in sub-type: it is a diffuse profile with the format overridden, as a user override would be.
pub fn engine_profile(format: &str, level: &str) -> Result<Profile> {
    let (st, fmt) = match (format, level) {
        ("bc7", "max") => (Subtype::Diffuse, Format::Bc7),
        ("bc3", "default" | "max") => (Subtype::Diffuse, Format::Bc3),
        ("bc4", "default" | "max") => (Subtype::Height, Format::Bc4),
        ("bc5", "default" | "max") => (Subtype::Flow, Format::Bc5),
        _ => bail!(
            "unsupported --format {format} --level {level} (bc7 max, bc3|bc4|bc5 default|max, bc6h veryslow_peak)"
        ),
    };
    let mut p = PresetRegistry::builtin()?.profile(DEFAULT_PRESET, Target::Cs, st)?;
    p.format = fmt;
    if fmt == Format::Bc7 {
        p.encoder.effort = Effort::Max;
    } else if level == "max" {
        p.encoder.bc45_search_radius = SearchRadius::Max;
    }
    Ok(p)
}

/// `format` "bc7" (level "max"), "bc3" / "bc4" / "bc5" (level "default", or "max" = search radius `max`), input an 8-bit PNG fed to the engine's
/// `encode_level` unchanged; or "bc6h" (level "veryslow_peak", input a bench `.f16`).
pub fn encode_one(format: &str, level: &str, input: &Path, output: &Path) -> Result<()> {
    // The engine's default BC1 model (engine::api); only BC3 colour reads it.
    encoders_sys::init(Bc1Model::Ideal)?;
    let (w, h, fmt, payload) = if (format, level) == ("bc6h", "veryslow_peak") {
        let img = load_f16(input)?;
        if img.width % 4 != 0 || img.height % 4 != 0 {
            bail!("bc6h input must be a multiple of 4 in both directions");
        }
        let enc = encode_bc6h_image(&img, Bc6hLevel::VeryslowPeak);
        (img.width, img.height, Format::Bc6hUf16, enc)
    } else {
        let p = engine_profile(format, level)?;
        if p.format == Format::Bc7 && !encoders_sys::BC7E_MAX_NATIVE {
            bail!("bc7e Max needs the patch-0002 objects (BC7E_MAX_NATIVE is false)");
        }
        let img = load_png_rgba8(input)?;
        let enc = encode_level(
            &EncoderInput::Ldr(img.px),
            img.width,
            img.height,
            &p,
            &EncodeCtx::default(),
        )
        .map_err(|_| anyhow!("encode cancelled"))?;
        (img.width, img.height, p.format, enc)
    };
    let desc = DdsDesc {
        width: w,
        height: h,
        format: fmt,
        srgb: false,
        header: ResolvedHeader::Dx10,
    };
    std::fs::write(output, dds_bytes(&desc, &[payload])?)?;
    Ok(())
}
