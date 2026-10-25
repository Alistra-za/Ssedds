//! Encode an in-memory image with an explicit profile (the Photoshop plugin's path). The steps and their order are
//! those of `pipeline::prepare_then` → `encode_levels` → the `DdsDesc` write in `process_file`, so the bytes equal a
//! file run of the same pixels and profile (tests/memory.rs).
use crate::config::{GraySource, PackRecipe, Profile, RawMap, Subtype, Target};
use crate::coverage::preserve_coverage;
use crate::dds::{dds_bytes, resolve_header, DdsDesc};
use crate::encode::{encode_level, EncodeCtx};
use crate::image::{Rgba32F, SourceInfo};
use crate::mips::build_chain;
use crate::ops::{preprocess_with, SourceTransfer};
use crate::pack::assemble::{assemble_channels_with, PackError};
use crate::pack::{recipes_for_with, resolve_channels, PackSource, RawInput};
use crate::quantize::to_encoder_input;
use std::path::{Path, PathBuf};

/// One channel of an interleaved float image: value i = `px[i * channels + channel]`; `px.len() == w * h * channels`.
pub struct Channel<'a> {
    pub px: &'a [f32],
    pub width: u32,
    pub height: u32,
    pub channels: u32,
    pub channel: u32,
}

/// The cs `_rmaos` image of in-memory sources (role order roughness, metallic, AO, specular), by the engine's own
/// rule: the recipe, `resolve_channels` (fills and neutral defaults) and `assemble_channels_with` over synthetic
/// paths named by role id. Values are clamped to 0..=1 (NaN to 0). No recipe: the first unused-input reason.
/// Returns the packed image and the indices (0..4) of the channels that took a neutral default.
pub fn pack_rmaos(
    src: [Option<Channel>; 4],
    fill: [Option<u8>; 4],
    p: &Profile,
) -> Result<(Rgba32F, Vec<usize>), String> {
    const ROLES: [RawMap; 4] = [
        RawMap::Roughness,
        RawMap::Metallic,
        RawMap::Ao,
        RawMap::Specular,
    ];
    for (c, role) in src.iter().zip(ROLES) {
        let Some(c) = c else { continue };
        let ok = c.width > 0
            && c.height > 0
            && c.channel < c.channels
            && u64::from(c.width) * u64::from(c.height) * u64::from(c.channels)
                == c.px.len() as u64;
        if !ok {
            return Err(format!("{}: invalid source image or channel", role.id()));
        }
    }
    let path = |role: RawMap| PathBuf::from(role.id());
    let raw: Vec<RawInput> = src
        .iter()
        .zip(ROLES)
        .filter(|(c, _)| c.is_some())
        .map(|(_, role)| RawInput {
            path: path(role),
            role,
        })
        .collect();
    let (recipes, unused) = recipes_for_with(Target::Cs, &raw, PackRecipe::Rmaos, false);
    let Some(recipe) = recipes.first() else {
        return Err(unused
            .into_iter()
            .next()
            .map_or_else(|| "nothing to pack".into(), |u| u.reason));
    };
    let mut packing = p.packing.clone();
    for (f, role) in fill.into_iter().zip(ROLES) {
        match role {
            RawMap::Roughness => packing.fill.roughness = f,
            RawMap::Metallic => packing.fill.metallic = f,
            RawMap::Ao => packing.fill.ao = f,
            _ => packing.fill.specular = f,
        }
    }
    let (channels, defaulted) = resolve_channels(recipe, &packing);
    let sources: Vec<PackSource> = recipe
        .sources()
        .into_iter()
        .map(|(path, role)| PackSource {
            path,
            role,
            stamp: None,
        })
        .collect();
    let find = |q: &Path| {
        src.iter()
            .zip(ROLES)
            .find(|(_, role)| Path::new(&role.id()) == q)
            .and_then(|(c, _)| c.as_ref())
            .expect("every recipe source is one of the supplied channels")
    };
    let a = assemble_channels_with(
        &channels,
        &sources,
        GraySource::R,
        |q| {
            let c = find(q);
            Ok((c.width, c.height))
        },
        |q| {
            let c = find(q);
            let (n, k) = (c.channels as usize, c.channel as usize);
            let mut img = Rgba32F::new(c.width, c.height, [0.0, 0.0, 0.0, 1.0]);
            for (o, i) in img.px.iter_mut().zip(c.px.chunks_exact(n)) {
                let v = if i[k].is_nan() {
                    0.0
                } else {
                    i[k].clamp(0.0, 1.0)
                };
                *o = [v, v, v, 1.0];
            }
            // shortcut: 8-bit `Stored` is what `encode_image` always uses; the float precision is kept regardless.
            // Upgrade if encode_image ever takes a transfer.
            let info = SourceInfo {
                bit_depth: 8,
                channels: 1,
                has_alpha: false,
                is_float: false,
            };
            Ok::<_, PackError>((img, info))
        },
    )
    .map_err(|e| e.to_string())?;
    Ok((a.image, defaulted.into_iter().map(|(i, _)| i).collect()))
}

/// The bytes of a whole DDS file. Refuses what the plugin does not use: complex grass, RNM, `drop_top_mip` and a
/// resize. It never bakes: no bake step runs here.
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
