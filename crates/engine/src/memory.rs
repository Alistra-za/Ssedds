//! Encode in-memory images with explicit profiles (the psdds hosts' path): one image ([`encode_image`]), or a TruePBR
//! `_rmaos` + `_n` pair with the variance bake ([`encode_rmaos_set`]). The steps and their order are those of
//! `pipeline::prepare_then` → `encode_levels` → the `DdsDesc` write in `process_file`, so the bytes equal a file run
//! of the same pixels and profiles (tests/memory.rs, tests/memory_rmaos.rs).
use crate::bake::plan::{decide_bake, mark_normal_baking, BakeDecision, NO_NORMAL};
use crate::bake::{lengths_from, NormalVariance};
use crate::config::{PackRecipe, Profile, RawMap, Subtype, Target};
use crate::coverage::preserve_coverage;
use crate::dds::{dds_bytes, resolve_header, DdsDesc};
use crate::encode::{encode_level, EncodeCtx};
use crate::image::{Rgba32F, SourceInfo};
use crate::mips::build_chain_with;
use crate::ops::{preprocess_with, SourceTransfer};
use crate::pack::assemble::{assemble_channels_with, check_sizes_with, PackError};
use crate::pack::{recipes_for_with, resolve_channels, PackSource, RawInput, Recipe};
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

const ROLES: [RawMap; 4] = [
    RawMap::Roughness,
    RawMap::Metallic,
    RawMap::Ao,
    RawMap::Specular,
];

/// Each supplied source is a well-formed image and channel, and all share one size (the packer's own size rule,
/// `check_sizes_with`, and its text). Returns that size; (0, 0) with no source.
fn check_sources(src: &[Option<Channel>; 4]) -> Result<(u32, u32), String> {
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
    let paths: Vec<PathBuf> = ROLES
        .iter()
        .zip(src)
        .filter(|(_, c)| c.is_some())
        .map(|(r, _)| PathBuf::from(r.id()))
        .collect();
    check_sizes_with(&paths, |q| {
        let i = ROLES.iter().position(|r| Path::new(&r.id()) == q);
        let c = i.and_then(|i| src[i].as_ref()).expect("a supplied role");
        Ok((c.width, c.height))
    })
    .map_err(|e| e.to_string())
}

/// The sources checked ([`check_sources`]) and the cs `_rmaos` recipe they build, or why none builds.
fn rmaos_recipe(src: &[Option<Channel>; 4]) -> Result<Recipe, String> {
    check_sources(src)?;
    let raw: Vec<RawInput> = src
        .iter()
        .zip(ROLES)
        .filter(|(c, _)| c.is_some())
        .map(|(_, role)| RawInput {
            path: PathBuf::from(role.id()),
            role,
        })
        .collect();
    let (recipes, unused) = recipes_for_with(Target::Cs, &raw, PackRecipe::Rmaos, false);
    recipes.into_iter().next().ok_or_else(|| {
        unused
            .into_iter()
            .next()
            .map_or_else(|| "nothing to pack".into(), |u| u.reason)
    })
}

/// The cs `_rmaos` image of in-memory sources (role order roughness, metallic, AO, specular), by the engine's own
/// rule: the recipe, `resolve_channels` (fills and neutral defaults) and `assemble_channels_with` over synthetic
/// paths named by role id. Values pass through as the file path's packer reads them (no clamp: the host clamps).
/// No recipe: the first unused-input reason.
/// Returns the packed image and the indices (0..4) of the channels that took a neutral default.
pub fn pack_rmaos(
    src: [Option<Channel>; 4],
    fill: [Option<u8>; 4],
    p: &Profile,
) -> Result<(Rgba32F, Vec<usize>), String> {
    let recipe = &rmaos_recipe(&src)?;
    let mut packing = p.packing.clone();
    for (f, role) in fill.into_iter().zip(ROLES) {
        match role {
            RawMap::Roughness => packing.fill.roughness = f.or(packing.fill.roughness),
            RawMap::Metallic => packing.fill.metallic = f.or(packing.fill.metallic),
            RawMap::Ao => packing.fill.ao = f.or(packing.fill.ao),
            _ => packing.fill.specular = f.or(packing.fill.specular),
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
        p.channels.grayscale_source,
        |q| {
            let c = find(q);
            Ok((c.width, c.height))
        },
        |q| {
            let c = find(q);
            let (n, k) = (c.channels as usize, c.channel as usize);
            let mut img = Rgba32F::new(c.width, c.height, [0.0, 0.0, 0.0, 1.0]);
            for (o, i) in img.px.iter_mut().zip(c.px.chunks_exact(n)) {
                *o = [i[k], i[k], i[k], 1.0];
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
/// resize. No bake: [`encode_image_with`] with `None`.
pub fn encode_image(img: Rgba32F, p: &Profile) -> Result<Vec<u8>, String> {
    encode_image_with(img, p, None)
}

/// [`encode_image`] with a roughness member's variance bake (`build_chain_with(&work, p, bake)`, as the file run).
pub fn encode_image_with(
    img: Rgba32F,
    p: &Profile,
    bake: Option<&NormalVariance>,
) -> Result<Vec<u8>, String> {
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
    let mut chain = build_chain_with(&work, p, bake);
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

/// A TruePBR pair as `encode_rmaos_set` writes it.
#[derive(Debug)]
pub struct RmaosSet {
    pub rmaos: Vec<u8>,
    /// The `_n`, when one was supplied.
    pub normal: Option<Vec<u8>>,
    /// Indices (0..4) of the `_rmaos` channels that took a neutral default (`PACK-DEFAULTED`).
    pub defaulted: Vec<usize>,
    /// True only when the `_n`'s footprint variance was baked into the roughness mips.
    pub baked: bool,
    /// Why it was not baked; `None` when it was.
    pub bake_note: Option<String>,
}

/// A TruePBR pair as `Engine::plan` + `run` writes it (target cs): the `_rmaos` packed from `src`/`fill` by
/// [`pack_rmaos`] and encoded with `rmaos_p`, and, with a `normal` (image, its unbaked `pbr_normal` profile), the `_n`
/// and its variance baked into the roughness mips by the planner's rule ([`decide_bake`]). Not baked, with a note:
/// no normal, an sRGB `_rmaos` (R10) or sizes that cannot bake (`SET-BAKE-RATIO`'s text). A one-level chain bakes
/// mip 0 as the batch does (it changes only when the `_n` is larger). Sources are checked and the recipe found before
/// any encode. Values are not clamped here (the host does). Order keeps the peak near two normals + the packed image
/// + one source.
pub fn encode_rmaos_set(
    src: [Option<Channel>; 4],
    fill: [Option<u8>; 4],
    rmaos_p: &Profile,
    normal: Option<(Rgba32F, Profile)>,
) -> Result<RmaosSet, String> {
    // A set that cannot pack fails before any encode work.
    rmaos_recipe(&src)?;
    let mdims = check_sources(&src)?;
    let (mut bake, mut note, mut normal_dds) = (None, Some(NO_NORMAL.to_string()), None);
    if let Some((img, mut normal_p)) = normal {
        let ndims = (img.width, img.height);
        note = match decide_bake(false, ndims, rmaos_p, mdims) {
            BakeDecision::Never => {
                Some("variance bake off: an sRGB roughness member never bakes".into())
            }
            BakeDecision::Off(w) => Some(w.message),
            BakeDecision::Bake { .. } => {
                mark_normal_baking(&mut normal_p);
                // shortcut: the normal is cloned once for the lengths, a 2x normal peak (8 GiB at 16384²); the
                // file path re-decodes instead. Upgrade by letting `encode_image` hand back its preprocessed work,
                // if a host hits memory limits.
                let lengths = lengths_from(img.clone(), SourceTransfer::Stored, &normal_p, None)?;
                bake = Some(NormalVariance::new(
                    lengths,
                    mdims,
                    rmaos_p.mips.bake_clamp,
                )?);
                None
            }
        };
        normal_dds = Some(encode_image(img, &normal_p)?);
    }
    let (packed, defaulted) = pack_rmaos(src, fill, rmaos_p)?;
    Ok(RmaosSet {
        rmaos: encode_image_with(packed, rmaos_p, bake.as_ref())?,
        normal: normal_dds,
        defaulted,
        baked: bake.is_some(),
        bake_note: note,
    })
}
