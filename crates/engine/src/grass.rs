//! Complex grass (design `2026-10-12-complex-grass-design.md`): one image, height = 2 × width. The top half is a
//! diffuse with alpha transparency, the bottom half a normal map with specular in alpha. Community Shaders'
//! `RunGrass.hlsl` reads both halves from one view and treats a texture as complex when the decoded bottom-left texel
//! of mip 0 is a unit vector (`|length(rgb * 2 - 1) - 1| < ComplexGrassThreshold`); ENB is assumed to do the same
//! (owner ruling 2026-10-12).
use crate::analyse::code8;
use crate::config::*;
use crate::coverage::preserve_coverage;
use crate::image::Rgba32F;
use crate::mips::build_chain;
use crate::ops::{preprocess_with, Domain, SourceTransfer, Work};
use crate::quantize::{to_encoder_input, EncoderInput};
use schemars::JsonSchema;
use serde::Serialize;

/// The owner's `ComplexGrassThreshold` (CS `SettingsUser.json`, "Grass Lighting"), design §2.
pub const CS_GRASS_THRESHOLD: f32 = 0.03;
/// What the encoded detection texel keeps below the threshold, so the file passes any threshold >= 0.02 (§3.6).
pub const GRASS_GUARD_MARGIN: f32 = 0.01;
/// `|L - 1|` below this is a unit-length normal (design §4, §5).
pub const GRASS_UNIT_TOL: f32 = 0.1;
/// Share of bottom-half texels that must be unit-length normals (design §4, §5).
pub const GRASS_UNIT_SHARE: f64 = 0.99;

/// What `analyse` records of an image whose height is twice its width (design §4, §5).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, JsonSchema)]
pub struct GrassFacts {
    /// Every top-half alpha code is 251 or more (DECISIONS Note L: 251..=254 is BC encode noise).
    pub top_opaque: bool,
    /// Share of bottom-half texels that decode to `|L - 1| < 0.1`.
    pub bottom_unit_share: f64,
    /// `|L - 1|` of the bottom-left texel `(0, h - 1)`, the one CS tests.
    pub corner_err: f32,
}

/// `|length(rgb * 2 - 1) - 1|` of stored values in [0, 1]; a non-finite input is infinitely far from unit length.
pub fn unit_err(rgb: [f32; 3]) -> f32 {
    let v = rgb.map(|c| c * 2.0 - 1.0);
    let e = ((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt() - 1.0).abs();
    if e.is_finite() {
        e
    } else {
        f32::INFINITY
    }
}

/// The key `BatchRequest::gras_diffuses` holds for a texture path: lower case, `/`, without the extension, so an
/// input `textures/plants/g.png` meets the scan's `textures/plants/g.dds`.
pub fn path_key(path: &str) -> String {
    let p = path.replace('\\', "/").to_ascii_lowercase();
    match p.rsplit_once('.') {
        Some((stem, ext)) if !ext.contains('/') => stem.to_string(),
        _ => p,
    }
}

/// Design §5 conditions 2-4 (condition 1, the GRAS record, is the caller's): 1:2, the bottom-left texel passes CS's
/// 0.03 test, >= 99 % unit-length normals in the bottom half.
pub fn looks_like_grass(a: &crate::analyse::Analysis) -> bool {
    a.grass.is_some_and(|g| {
        g.corner_err < CS_GRASS_THRESHOLD && g.bottom_unit_share >= GRASS_UNIT_SHARE
    })
}

/// The facts of `img` when its height is twice its width; `None` for every other shape.
pub fn facts(img: &Rgba32F) -> Option<GrassFacts> {
    let (w, h) = (img.width as usize, img.height as usize);
    if w == 0 || h != 2 * w {
        return None;
    }
    let (top, bottom) = img.px.split_at(w * w);
    let n = (w * w) as f64;
    let share = |half: &[[f32; 4]], keep: &dyn Fn(&[f32; 4]) -> bool| {
        half.iter().filter(|p| keep(p)).count() as f64 / n
    };
    let c = img.px[(h - 1) * w];
    Some(GrassFacts {
        top_opaque: top.iter().all(|p| code8(p[3]) >= 251),
        bottom_unit_share: share(bottom, &|p| unit_err([p[0], p[1], p[2]]) < GRASS_UNIT_TOL),
        corner_err: unit_err([c[0], c[1], c[2]]),
    })
}

/// Preprocess + Mipmaps of a complex-grass job (design §3.1-3.4): each half is built on its own by the existing
/// path (top: diffuse with coverage preservation; bottom: normal map, renormalised per level, alpha as data), so no
/// filter crosses the seam, and the halves are stacked per level as stored values (codes / 255). The chain ends at
/// 1 x 2 (ruling R6). The returned `Work` quantises the stacked levels as they are (`Stored`).
pub(crate) fn build(
    img: Rgba32F,
    p: &Profile,
    transfer: SourceTransfer,
) -> Result<(Work, Vec<Rgba32F>), String> {
    let (w, h) = (img.width, img.height);
    if !(w.is_power_of_two() && h == 2 * w) {
        return Err(format!(
            "{w}x{h}: complex_grass needs a power-of-two width and height = 2 x width (GRASS-SHAPE)"
        ));
    }
    let (top, bottom) = split(img);
    let (tp, bp) = (half_profile(p, false), half_profile(p, true));
    let tw = preprocess_with(top, &tp, transfer);
    let bw = preprocess_with(bottom, &bp, transfer);
    let mut tc = build_chain(&tw, &tp);
    if tw.alpha_present && tp.coverage_enabled() {
        preserve_coverage(&mut tc, tp.alpha.threshold, tp.alpha.compare);
    }
    let bc = build_chain(&bw, &bp);
    // Both half chains and the stacked chain are live together; `api::job_bytes` admits the extra chain (Note BB).
    let chain = tc
        .iter()
        .zip(&bc)
        .map(|(t, b)| {
            stack(
                &to_encoder_input(t, &tw, &tp),
                &to_encoder_input(b, &bw, &bp),
                t.width,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let work = Work {
        img: Rgba32F::new(0, 0, [0.0; 4]),
        alpha_present: p.alpha.policy != AlphaPolicy::Remove,
        domain: Domain::Stored,
        roughness_r_squared: false,
        nonfinite_samples: tw.nonfinite_samples + bw.nonfinite_samples,
    };
    Ok((work, chain))
}

/// The top and bottom halves of a `w` x `2w` image.
fn split(img: Rgba32F) -> (Rgba32F, Rgba32F) {
    let (w, h) = (img.width, img.height / 2);
    let mut px = img.px;
    let bottom = px.split_off((w * h) as usize);
    (
        Rgba32F {
            width: w,
            height: h,
            px,
        },
        Rgba32F {
            width: w,
            height: h,
            px: bottom,
        },
    )
}

/// One stacked level, top rows first, as stored floats (8-bit code / 255, so `to_encoder_input` gives the codes back).
fn stack(top: &EncoderInput, bottom: &EncoderInput, w: u32) -> Result<Rgba32F, String> {
    let (EncoderInput::Ldr(t), EncoderInput::Ldr(b)) = (top, bottom) else {
        return Err("complex_grass is written as BC7: an HDR format cannot hold it".into());
    };
    let px: Vec<[f32; 4]> = t
        .iter()
        .chain(b)
        .map(|c| c.map(|v| f32::from(v) / 255.0))
        .collect();
    Ok(Rgba32F {
        width: w,
        height: (px.len() / w as usize) as u32,
        px,
    })
}

/// The profile each half is built with (design §3.2-3.3, ruling R5): the top half is a diffuse (sRGB colour, the job's
/// alpha policy, coverage and filter); the bottom half a normal map whose alpha is data, unless the job removes
/// alpha. Never resized (ruling R7).
fn half_profile(p: &Profile, bottom: bool) -> Profile {
    let mut h = p.clone();
    h.resize = ResizeMode::None;
    if bottom {
        h.subtype = Subtype::Normal;
        h.category = Category::Normal;
        h.color.content = ColorContent::Linear;
        if h.alpha.policy != AlphaPolicy::Remove {
            h.alpha.policy = AlphaPolicy::Data;
        }
    } else {
        h.subtype = Subtype::Diffuse;
        h.category = Category::Color;
        h.color.content = ColorContent::Srgb;
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets::PresetRegistry;

    fn profile() -> Profile {
        let mut p = PresetRegistry::builtin()
            .unwrap()
            .profile("cs-quality", Target::Cs, Subtype::ComplexGrass)
            .unwrap();
        p.alpha.policy = AlphaPolicy::Cutout;
        p
    }

    /// Top = colour with ~30 % cutout dots, bottom = unit normals tilted along x with specular 12.
    fn source(w: u32) -> Rgba32F {
        let mut img = Rgba32F::new(w, 2 * w, [0.0; 4]);
        for y in 0..2 * w {
            for x in 0..w {
                *img.at_mut(x, y) = if y < w {
                    let a = if (x * 7 + y * 13) % 10 < 3 { 1.0 } else { 0.0 };
                    [0.25, 0.5, 0.15, a]
                } else {
                    let t = (x as f32 / w as f32 - 0.5) * 0.8;
                    [t.sin() * 0.5 + 0.5, 0.5, t.cos() * 0.5 + 0.5, 12.0 / 255.0]
                };
            }
        }
        img
    }

    fn codes(l: &Rgba32F) -> Vec<[u8; 4]> {
        l.px.iter().map(|q| q.map(code8)).collect()
    }

    /// R6: one level per half level, ending at 1 x 2.
    #[test]
    fn the_chain_stops_at_one_by_two() {
        let (_, chain) = build(source(8), &profile(), SourceTransfer::Stored).unwrap();
        let dims: Vec<_> = chain.iter().map(|l| (l.width, l.height)).collect();
        assert_eq!(dims, [(8, 16), (4, 8), (2, 4), (1, 2)]);
    }

    /// §3.4: constant halves stay exactly constant at every level; a filter tap across the seam would mix them.
    #[test]
    fn no_texel_crosses_the_seam() {
        let w = 16;
        let mut src = Rgba32F::new(w, 2 * w, [0.5, 0.5, 1.0, 12.0 / 255.0]);
        for px in &mut src.px[..(w * w) as usize] {
            *px = [1.0, 1.0, 1.0, 1.0];
        }
        let (_, chain) = build(src, &profile(), SourceTransfer::Stored).unwrap();
        for (l, level) in chain.iter().enumerate() {
            let n = (level.width * level.width) as usize;
            let c = codes(level);
            assert!(
                c[..n].iter().all(|q| *q == [255, 255, 255, 255]),
                "top, level {l}: {:?}",
                &c[..n]
            );
            assert!(
                c[n..].iter().all(|q| *q == [128, 128, 255, 12]),
                "bottom, level {l}: {:?}",
                &c[n..]
            );
        }
    }

    /// §3.3: the bottom half is renormalised per level and its alpha is data (box-averaged, never coverage-adjusted).
    #[test]
    fn bottom_normals_stay_unit_and_alpha_is_data() {
        let (_, chain) = build(source(16), &profile(), SourceTransfer::Stored).unwrap();
        for (l, level) in chain.iter().enumerate() {
            let n = (level.width * level.width) as usize;
            for q in &level.px[n..] {
                assert!(unit_err([q[0], q[1], q[2]]) < 0.01, "level {l}: {q:?}");
                assert_eq!(code8(q[3]), 12, "level {l}");
            }
        }
    }

    /// Each half is exactly what the existing path gives that half on its own: diffuse (with coverage preservation,
    /// §3.2) on top, normal map below. This is also the coverage test: a direct coverage-tolerance assertion on a
    /// binary dot source cannot pass, because `preserve_coverage` can only reach the cumulative counts of a level's
    /// few distinct alpha values.
    #[test]
    fn each_half_is_the_existing_path() {
        let p = profile();
        let src = source(16);
        let (_, chain) = build(src.clone(), &p, SourceTransfer::Stored).unwrap();
        let (top, bottom) = split(src);
        let (tp, bp) = (half_profile(&p, false), half_profile(&p, true));
        let tw = preprocess_with(top, &tp, SourceTransfer::Stored);
        let mut tc = build_chain(&tw, &tp);
        preserve_coverage(&mut tc, tp.alpha.threshold, tp.alpha.compare);
        let bw = preprocess_with(bottom, &bp, SourceTransfer::Stored);
        let bc = build_chain(&bw, &bp);
        let ldr = |e: EncoderInput| match e {
            EncoderInput::Ldr(v) => v,
            EncoderInput::Hdr(_) => panic!("LDR expected"),
        };
        for (l, level) in chain.iter().enumerate() {
            let n = (level.width * level.width) as usize;
            let got = codes(level);
            assert_eq!(
                got[..n],
                ldr(to_encoder_input(&tc[l], &tw, &tp))[..],
                "top, level {l}"
            );
            assert_eq!(
                got[n..],
                ldr(to_encoder_input(&bc[l], &bw, &bp))[..],
                "bottom, level {l}"
            );
        }
    }

    /// Review Focus 1: a job with the wrong shape (e.g. previewed while it is `GRASS-SHAPE`) is an error, not a panic.
    #[test]
    fn a_wrong_shape_is_an_error_not_a_panic() {
        for (w, h) in [(16, 16), (32, 16), (12, 24)] {
            let e = build(
                Rgba32F::new(w, h, [0.5; 4]),
                &profile(),
                SourceTransfer::Stored,
            )
            .unwrap_err();
            assert!(e.contains("complex_grass"), "{e}");
        }
    }

    /// R5: `remove` makes both halves opaque.
    #[test]
    fn remove_makes_both_halves_opaque() {
        let mut p = profile();
        p.alpha.policy = AlphaPolicy::Remove;
        let (work, chain) = build(source(8), &p, SourceTransfer::Stored).unwrap();
        assert!(!work.alpha_present);
        assert!(chain[0].px.iter().all(|q| q[3] == 1.0));
    }

    #[test]
    fn facts_only_for_one_by_two() {
        assert!(facts(&Rgba32F::new(4, 4, [0.5; 4])).is_none());
        assert!(facts(&Rgba32F::new(8, 4, [0.5; 4])).is_none());
        let mut img = Rgba32F::new(4, 8, [0.5, 0.5, 1.0, 12.0 / 255.0]);
        for px in &mut img.px[..16] {
            *px = [0.3, 0.6, 0.2, 1.0];
        }
        img.px[1][3] = 0.0;
        img.px[2][3] = 0.5;
        let f = facts(&img).unwrap();
        assert!(!f.top_opaque);
        assert_eq!(f.bottom_unit_share, 1.0);
        assert!(f.corner_err < 0.01, "{}", f.corner_err);
    }

    /// The owner's example corner (127, 126, 255) has length 1.0001 (design §2).
    #[test]
    fn owner_corner_is_unit() {
        let e = unit_err([127.0 / 255.0, 126.0 / 255.0, 1.0]);
        assert!(e < 0.005, "{e}");
        assert_eq!(unit_err([0.5, 0.5, f32::NAN]), f32::INFINITY);
    }

    #[test]
    fn path_key_normalises_and_drops_the_extension() {
        assert_eq!(path_key("textures/plants/g.png"), "textures/plants/g");
        assert_eq!(path_key("textures\\Plants\\G.dds"), "textures/plants/g");
        assert_eq!(path_key("textures/a.b/c"), "textures/a.b/c");
    }
}
