//! Task 38 gates: the engine's own stage functions (load -> preprocess -> mips -> quantise -> encode -> DDS writer ->
//! DDS reader -> decode) against the bench, and the exact-opaque guarantee on full mip chains (spec §11.2, §12 S5).
use crate::codec::{decode_bc7_image, encode_bc7_image};
use crate::corpus::*;
use crate::gates::{GateReport, BC7_SETS, U5};
use crate::{ddsraw, metrics};
use anyhow::{anyhow, bail, ensure, Result};
use encoders_sys::{Bc1Model, Bc7Params};
use engine::config::*;
use engine::dds::{dds_bytes, full_chain_len, read_dds, resolve_header, DdsDesc};
use engine::decode::{decode_level, Decoded};
use engine::encode::{encode_level, EncodeCtx};
use engine::load::load_image;
use engine::mips::build_chain;
use engine::ops::preprocess;
use engine::presets::PresetRegistry;
use engine::quantize::{to_encoder_input, EncoderInput};
use engine::validate::alpha::alpha_stats;
use std::path::Path;

/// Images per set (the bench corpus; a gate that sees fewer has been run on a shrunken corpus).
const PARITY_IMAGES: [(&str, usize); 10] = [
    ("rgb_real", 7),
    ("rgb_grad", 7),
    ("alpha_real", 4),
    ("alpha_grad", 3),
    ("cutout_real", 5),
    ("cutout_grad", 3),
    ("packed_real", 4),
    ("packed_grad", 2),
    ("stray_real", 2),
    ("stray_grad", 2),
];

fn profile(st: Subtype) -> Profile {
    PresetRegistry::builtin()
        .unwrap()
        .profile("cs-quality", Target::Cs, st)
        .unwrap()
}

/// One quantised level: what the encoder is handed.
struct Level {
    width: u32,
    height: u32,
    px: Vec<[u8; 4]>,
}

/// load -> preprocess -> build_chain -> quantise, exactly the pipeline's stages for a profile without coverage
/// preservation (none of the profiles used here keep an alpha channel with a cutout policy).
fn quantised_chain(src: &Path, p: &Profile) -> Result<Vec<Level>> {
    let (img, _) = load_image(src)?;
    let work = preprocess(img, p);
    build_chain(&work, p)
        .iter()
        .map(|l| match to_encoder_input(l, &work, p) {
            EncoderInput::Ldr(px) => Ok(Level {
                width: l.width,
                height: l.height,
                px,
            }),
            EncoderInput::Hdr(_) => bail!("unexpected HDR encoder input"),
        })
        .collect()
}

fn encode(level: &Level, p: &Profile) -> Result<Vec<u8>> {
    encode_level(
        &EncoderInput::Ldr(level.px.clone()),
        level.width,
        level.height,
        p,
        &EncodeCtx::default(),
    )
    .map_err(|_| anyhow!("encode cancelled"))
}

/// Encode every level, write a whole DDS, read it back and decode each level from the file (BC1 through `model`).
fn encode_decode_chain(
    chain: &[Level],
    p: &Profile,
    model: Option<Bc1Model>,
) -> Result<Vec<Vec<[u8; 4]>>> {
    let levels = chain
        .iter()
        .map(|l| encode(l, p))
        .collect::<Result<Vec<_>>>()?;
    let srgb = p.srgb_tag();
    let desc = DdsDesc {
        width: chain[0].width,
        height: chain[0].height,
        format: p.format,
        srgb,
        header: resolve_header(p.format, srgb, p.header),
    };
    let file = read_dds(&dds_bytes(&desc, &levels)?)?;
    ensure!(
        file.levels.len() == chain.len() && chain.len() == full_chain_len(desc.width, desc.height),
        "DDS holds {} levels, chain has {} (full chain is {})",
        file.levels.len(),
        chain.len(),
        full_chain_len(desc.width, desc.height)
    );
    ensure!(
        file.levels
            .iter()
            .zip(chain)
            .all(|((w, h, _), l)| (*w, *h) == (l.width, l.height)),
        "DDS level sizes differ from the chain"
    );
    let last = file.levels.last().unwrap();
    ensure!((last.0, last.1) == (1, 1), "chain does not end at 1x1");
    file.levels
        .iter()
        .map(
            |(w, h, r)| match decode_level(file.format, *w, *h, &file.data[r.clone()], model)? {
                Decoded::Ldr(px) => Ok(px),
                Decoded::Hdr(_) => bail!("unexpected HDR decode"),
            },
        )
        .collect()
}

/// Engine path vs the bench at mip 0, on every BC7 bench image.
///
/// Per image: load -> preprocess -> quantise returns the PNG's 8-bit RGBA exactly (alpha kept under `data`;
/// `remove` on the opaque sets gives A = 255), and the encoded mip 0 equals the bench's bc7e u5 payload byte for byte.
pub fn engine_parity(root: &Path) -> Result<GateReport> {
    encoders_sys::init(Bc1Model::Ideal)?;
    let mut r = GateReport::new(
        "engine path (load->preprocess->quantise->encode) == bench bc7e u5, mip 0, lossless 8-bit input",
    );
    let mut base = profile(Subtype::GenericData); // linear content, bc7e veryslow, perceptual off, flag off
    base.mips.levels = MipLevels::Keyword(MipKeyword::None);
    ensure!(
        base.format == Format::Bc7
            && base.encoder.effort == Effort::Veryslow
            && !base.encoder.perceptual
            && !base.alpha.exact_opaque,
        "generic_data must resolve to plain bc7e u5"
    );
    let mut remove = base.clone();
    remove.alpha.policy = AlphaPolicy::Remove;
    ensure!(
        BC7_SETS
            .iter()
            .all(|s| PARITY_IMAGES.iter().any(|(n, _)| n == s)),
        "PARITY_IMAGES must cover BC7_SETS"
    );
    let (mut lossless, mut identical, mut total) = (0, 0, 0);
    for (set, expected) in PARITY_IMAGES {
        let files = set_files(root, "images", set, "png")?;
        r.check(
            files.len() == expected,
            format!("{set}: image count {} == expected {expected}", files.len()),
        );
        let (mut set_lossless, mut set_identical) = (0, 0);
        // `data` keeps alpha; the opaque sets are also run through `remove`, the policy the real jobs use for them.
        let mut policies = vec![("data", &base)];
        if set.starts_with("rgb_") {
            policies.push(("remove", &remove));
        }
        for src in &files {
            let name = format!("{set}/{}", stem(src));
            let png = load_png_rgba8(src)?;
            let bench = std::fs::read(
                root.join("out/bc7e_u5")
                    .join(set)
                    .join(format!("{}.dds", stem(src))),
            )?;
            for (label, p) in &policies {
                let chain = quantised_chain(src, p)?;
                let level = &chain[0];
                ensure!(chain.len() == 1, "mips none must give one level");
                let want: Vec<[u8; 4]> = if *label == "remove" {
                    png.px.iter().map(|q| [q[0], q[1], q[2], 255]).collect()
                } else {
                    png.px.clone()
                };
                let exact =
                    (level.width, level.height) == (png.width, png.height) && level.px == want;
                let same = ddsraw::payload(&bench)? == encode(level, p)?.as_slice();
                total += 1;
                lossless += exact as usize;
                identical += same as usize;
                set_lossless += exact as usize;
                set_identical += same as usize;
                r.check(
                    exact,
                    format!("{name} [{label}]: quantised mip 0 == source 8-bit RGBA"),
                );
                r.check(
                    same,
                    format!("{name} [{label}]: mip 0 bytes == bench bc7e_u5"),
                );
            }
        }
        r.info(format!(
            "{set}: lossless {set_lossless}/{n}, byte-identical to bench {set_identical}/{n} ({} checks)",
            files.len() * policies.len(),
            n = files.len() * policies.len()
        ));
    }
    r.info(format!(
        "total: lossless {lossless}/{total}, byte-identical to bench {identical}/{total}"
    ));
    Ok(r)
}

const FULL_CHAIN_SETS: [(&str, usize); 4] = [
    ("rgb_real", 7),
    ("rgb_grad", 7),
    ("stray_real", 2),
    ("stray_grad", 2),
];

fn min_alpha(d: &[[u8; 4]]) -> u8 {
    alpha_stats(d).min
}

fn all_255(d: &[[u8; 4]]) -> bool {
    alpha_stats(d).eq255 as usize == d.len()
}

/// Decoded alpha after `remove`, at EVERY mip down to 1x1: flag on == 255, BC1 == 255 (both decoders), flag off >= 254.
/// The 254 share of the flag-off chain is recorded per level. Colour cost (mean RGB PSNR delta, on - off, mip 0) is recorded, and the flag-on RGB is checked against the codec-wrapper path.
pub fn full_chain_exact_opaque(root: &Path) -> Result<GateReport> {
    ensure!(
        encoders_sys::BC7E_EXACT_OPAQUE_NATIVE,
        "the full-chain gate compares against the native flag and needs the patched bc7e objects"
    );
    encoders_sys::init(Bc1Model::Ideal)?;
    let mut r =
        GateReport::new("full-chain decoded alpha after remove (S5 / §11.2), every mip to 1x1");
    let mut off = profile(Subtype::Diffuse); // sRGB content: linear-light box mips, full chain, bc7e veryslow
    off.alpha.policy = AlphaPolicy::Remove;
    off.alpha.exact_opaque = false;
    ensure!(
        off.format == Format::Bc7
            && off.mips.levels == MipLevels::Keyword(MipKeyword::Full)
            && !off.encoder.perceptual,
        "diffuse must resolve to a full-chain bc7e profile"
    );
    let mut on = off.clone();
    on.alpha.exact_opaque = true;
    let mut bc1 = off.clone();
    bc1.format = Format::Bc1;
    bc1.encoder.backend = Backend::Rgbcx;
    for (set, expected) in FULL_CHAIN_SETS {
        let files = set_files(root, "images", set, "png")?;
        r.check(
            files.len() == expected,
            format!("{set}: image count {} == expected {expected}", files.len()),
        );
        let (mut d_rgb, mut one_by_one_254) = (vec![], 0usize);
        for src in &files {
            let name = format!("{set}/{}", stem(src));
            // The quantised levels do not depend on the flag or the format: encode the same chain three ways.
            let chain = quantised_chain(src, &off)?;
            let d_off = encode_decode_chain(&chain, &off, None)?;
            let d_on = encode_decode_chain(&chain, &on, None)?;
            let d_bc1 = encode_decode_chain(&chain, &bc1, Some(Bc1Model::Ideal))?;
            let d_bc1_bcdec = encode_decode_chain(&chain, &bc1, None)?;
            let dims = |i: usize| format!("{}x{}", chain[i].width, chain[i].height);
            let shares: Vec<String> = d_off
                .iter()
                .enumerate()
                .map(|(i, d)| format!("{}:{:.2}%", dims(i), alpha_stats(d).share_254() * 100.0))
                .collect();
            r.info(format!(
                "{name} flag off, 254 share per level: {}",
                shares.join(" ")
            ));
            let share_1x1 = alpha_stats(d_off.last().unwrap()).share_254();
            one_by_one_254 += (share_1x1 > 0.0) as usize;
            r.check(
                d_off.iter().all(|d| min_alpha(d) >= 254),
                format!(
                    "{name}: flag off >= 254 at all {} levels (1x1 254 share {:.0}%)",
                    d_off.len(),
                    share_1x1 * 100.0
                ),
            );
            r.check(
                d_on.iter().all(|d| all_255(d)),
                format!(
                    "{name}: flag on == 255 at all {} levels incl. 1x1",
                    d_on.len()
                ),
            );
            r.check(
                d_bc1.iter().all(|d| all_255(d)) && d_bc1_bcdec.iter().all(|d| all_255(d)),
                format!(
                    "{name}: BC1 == 255 at all {} levels (Ideal and bcdec decoders)",
                    d_bc1.len()
                ),
            );
            d_rgb.push(
                metrics::rgb_psnr(&chain[0].px, &d_on[0])
                    - metrics::rgb_psnr(&chain[0].px, &d_off[0]),
            );
            // The same mip 0 straight through the codec wrappers (the S5 gates' path), flag on.
            let png = load_png_rgba8(src)?;
            let opaque = Rgba8 {
                px: png.px.iter().map(|q| [q[0], q[1], q[2], 255]).collect(),
                ..png
            };
            r.check(
                chain[0].px == opaque.px,
                format!("{name}: quantised sRGB mip 0 == source 8-bit RGB (A = 255)"),
            );
            let direct = decode_bc7_image(
                &encode_bc7_image(
                    &opaque,
                    Bc7Params {
                        exact_opaque: true,
                        ..U5
                    },
                ),
                opaque.width,
                opaque.height,
            );
            r.check(
                direct.iter().zip(&d_on[0]).all(|(a, b)| a[..3] == b[..3]),
                format!("{name}: flag-on mip 0 RGB == direct codec path (the rotated-block fix leaves RGB untouched)"),
            );
        }
        let m = metrics::mean(&d_rgb);
        let worst = d_rgb.iter().cloned().fold(f64::INFINITY, f64::min);
        r.info(format!(
            "{set}: flag off has alpha 254 at 1x1 in {one_by_one_254}/{} images",
            files.len()
        ));
        // Recorded, not gated: the 0.05 dB RGB-cost limit fails by design (spike S5, owner ruling 2026-10-09:
        // exact_opaque is on by default only for envmask / complex_material), so the S5 gates are #[ignore]d on it.
        r.info(format!(
            "{set}: mean dRGB (flag on - off) at mip 0 {m:+.4} dB, worst image {worst:+.4} dB (S5 limit -0.05 dB not met, see DECISIONS)"
        ));
    }
    Ok(r)
}
