//! Encode an in-memory image with an explicit profile (the Photoshop plugin's path). The steps and their order are
//! those of `pipeline::prepare_then` → `encode_levels` → the `DdsDesc` write in `process_file`, so the bytes equal a
//! file run of the same pixels and profile (tests/memory.rs).
use crate::config::{Profile, Subtype};
use crate::coverage::preserve_coverage;
use crate::dds::{dds_bytes, resolve_header, DdsDesc};
use crate::encode::{encode_level, EncodeCtx};
use crate::image::Rgba32F;
use crate::mips::build_chain;
use crate::ops::{preprocess_with, SourceTransfer};
use crate::quantize::to_encoder_input;

/// The bytes of a whole DDS file. Refuses what the plugin does not use: complex grass, RNM, bake (never reached
/// from here), `drop_top_mip`, and a resize.
pub fn encode_image(img: Rgba32F, p: &Profile) -> Result<Vec<u8>, String> {
    if p.subtype == Subtype::ComplexGrass
        || p.rnm != Default::default()
        || p.drop_top_mip
        || !p.resize.is_none()
    {
        return Err(
            "encode_image: complex grass, RNM, drop_top_mip and resize are not supported".into(),
        );
    }
    let work = preprocess_with(img, p, SourceTransfer::Stored);
    let mut chain = build_chain(&work, p);
    if work.alpha_present && p.coverage_enabled() {
        preserve_coverage(&mut chain, p.alpha.threshold, p.alpha.compare);
    }
    let ctx = EncodeCtx::default();
    let levels = chain
        .iter()
        .map(|l| encode_level(&to_encoder_input(l, &work, p), l.width, l.height, p, &ctx))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "encode cancelled".to_string())?;
    let srgb = p.srgb_tag();
    let desc = DdsDesc {
        width: chain[0].width,
        height: chain[0].height,
        format: p.format,
        srgb,
        header: resolve_header(p.format, srgb, p.header),
    };
    dds_bytes(&desc, &levels).map_err(|e| e.to_string())
}
