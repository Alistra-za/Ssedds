//! One-shot encode for the Python bench runners (run_bench*.py): the engine levels the external CLIs lack, written
//! as a single-level DX10 DDS the runners' decoders read.
use crate::codec::{encode_bc6h_image, encode_bc7_image};
use crate::corpus::{load_f16, load_png_rgba8};
use anyhow::{bail, Result};
use encoders_sys::{Bc1Model, Bc6hLevel, Bc7Level, Bc7Params};
use engine::config::Format;
use engine::dds::{dds_bytes, DdsDesc, ResolvedHeader};
use std::path::Path;

/// `Bc7Level::Max` with the encoder settings every built-in preset resolves to for the bench's BC7 sets
/// (sdc-base.toml: perceptual off; exact_opaque only on envmask / complex_material).
pub const BC7_MAX: Bc7Params = Bc7Params {
    level: Bc7Level::Max,
    perceptual: false,
    exact_opaque: false,
};

/// `format` "bc7" (level "max", input an 8-bit PNG) or "bc6h" (level "veryslow_peak", input a bench `.f16`).
pub fn encode_one(format: &str, level: &str, input: &Path, output: &Path) -> Result<()> {
    encoders_sys::init(Bc1Model::Ideal)?;
    let (w, h, fmt, payload) = match (format, level) {
        ("bc7", "max") => {
            if !encoders_sys::BC7E_MAX_NATIVE {
                bail!("bc7e Max needs the patch-0002 objects (BC7E_MAX_NATIVE is false)");
            }
            let img = load_png_rgba8(input)?;
            (
                img.width,
                img.height,
                Format::Bc7,
                encode_bc7_image(&img, BC7_MAX),
            )
        }
        ("bc6h", "veryslow_peak") => {
            let img = load_f16(input)?;
            if img.width % 4 != 0 || img.height % 4 != 0 {
                bail!("bc6h input must be a multiple of 4 in both directions");
            }
            let enc = encode_bc6h_image(&img, Bc6hLevel::VeryslowPeak);
            (img.width, img.height, Format::Bc6hUf16, enc)
        }
        _ => bail!("unsupported --format {format} --level {level} (bc7 max, bc6h veryslow_peak)"),
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
