//! M2.5 Task 4: the packed float image, built from raw sources one at a time (spec §6.4 "Sources", "Precision").
use engine::config::*;
use engine::pack::assemble::*;
use engine::pack::*;
use image::{ImageBuffer, Luma, Rgb};
use std::path::{Path, PathBuf};

fn gray8(dir: &Path, name: &str, w: u32, f: impl Fn(u32, u32) -> u8) -> PathBuf {
    let p = dir.join(name);
    ImageBuffer::from_fn(w, w, |x, y| Luma([f(x, y)]))
        .save(&p)
        .unwrap();
    p
}

fn job(kind: RecipeKind, channels: [ChannelSource; 4], sources: &[(&PathBuf, RawMap)]) -> PackJob {
    PackJob {
        kind,
        channels,
        gray: GraySource::R,
        sources: sources
            .iter()
            .map(|(p, role)| PackSource {
                read: None,
                path: (*p).clone(),
                role: *role,
                stamp: None,
            })
            .collect(),
    }
}

fn src(p: &Path, role: RawMap, invert: bool) -> ChannelSource {
    ChannelSource::Source {
        path: p.to_path_buf(),
        role,
        invert,
        channel: None,
    }
}

/// The loader's u8/u16 → f32 conversion is a division; allow for its last-bit rounding.
fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 1e-6, "{a} vs {b}");
}

#[test]
fn channels_come_from_sources_and_constants() {
    let d = tempfile::tempdir().unwrap();
    let rough = gray8(d.path(), "iron_rough.png", 8, |x, _| x as u8 * 30);
    let ao = gray8(d.path(), "iron_ao.png", 8, |_, y| 255 - y as u8 * 20);
    let j = job(
        RecipeKind::Rmaos,
        [
            src(&rough, RawMap::Roughness, false),
            ChannelSource::Fill {
                role: RawMap::Metallic,
                code: 51,
            },
            src(&ao, RawMap::Ao, false),
            ChannelSource::Default { value: 1.0 },
        ],
        &[(&rough, RawMap::Roughness), (&ao, RawMap::Ao)],
    );
    let a = assemble(&j, GraySource::R).unwrap();
    assert_eq!((a.image.width, a.image.height), (8, 8));
    for y in 0..8 {
        for x in 0..8 {
            let p = a.image.at(x, y);
            // Exact before encode: the stored source value, no quantisation.
            close(p[0], f32::from(x as u8 * 30) / 255.0);
            close(p[1], 51.0 / 255.0);
            close(p[2], f32::from(255 - y as u8 * 20) / 255.0);
            close(p[3], 1.0);
        }
    }
    assert_eq!(a.info.bit_depth, 8);
    assert!(a.info.has_alpha && !a.info.is_float);
    assert!(a.non_gray.is_empty());
}

#[test]
fn inverted_mirrored_opaque_and_shared_sources() {
    let d = tempfile::tempdir().unwrap();
    let gloss = gray8(d.path(), "iron_gloss.png", 4, |x, _| x as u8 * 60);
    let metal = gray8(d.path(), "iron_metal.png", 4, |_, y| y as u8 * 60);
    // complex _m: metallic feeds R (env mask) and B; glossiness is G.
    let j = job(
        RecipeKind::CpmM,
        [
            src(&metal, RawMap::Metallic, false),
            src(&gloss, RawMap::Glossiness, false),
            src(&metal, RawMap::Metallic, false),
            ChannelSource::Default { value: 0.5 },
        ],
        &[(&metal, RawMap::Metallic), (&gloss, RawMap::Glossiness)],
    );
    let a = assemble(&j, GraySource::R).unwrap();
    let p = a.image.at(2, 3);
    close(p[0], 180.0 / 255.0);
    close(p[1], 120.0 / 255.0);
    close(p[2], 180.0 / 255.0);
    close(p[3], 0.5);
    // cs `_rmaos` R from glossiness, inverted; `_p` mirrors R and is opaque.
    let j = job(
        RecipeKind::Parallax,
        [
            src(&gloss, RawMap::Glossiness, true),
            ChannelSource::MirrorR,
            ChannelSource::MirrorR,
            ChannelSource::Opaque,
        ],
        &[(&gloss, RawMap::Glossiness)],
    );
    let a = assemble(&j, GraySource::R).unwrap();
    let p = a.image.at(1, 0);
    close(p[0], 1.0 - 60.0 / 255.0);
    assert_eq!((p[1], p[2], p[3]), (p[0], p[0], 1.0));
    assert!(!a.info.has_alpha);
}

/// Spec §6.4 "Precision": a 16-bit source keeps its precision until the encoder.
#[test]
fn sixteen_bit_sources_keep_their_precision() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("iron_height.png");
    ImageBuffer::from_fn(4, 4, |x, _| Luma([1000u16 + x as u16]))
        .save(&p)
        .unwrap();
    let j = job(
        RecipeKind::Parallax,
        [
            src(&p, RawMap::Height, false),
            ChannelSource::MirrorR,
            ChannelSource::MirrorR,
            ChannelSource::Opaque,
        ],
        &[(&p, RawMap::Height)],
    );
    let a = assemble(&j, GraySource::R).unwrap();
    // 1001/65535 lies between two 8-bit codes: the value is not quantised to 8 bits.
    close(a.image.at(1, 0)[0], 1001.0 / 65535.0);
    assert!((a.image.at(1, 0)[0] * 255.0).fract().abs() > 0.01);
    assert_eq!(a.info.bit_depth, 16);
}

#[test]
fn non_grey_sources_are_named_and_luma_is_honoured() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("iron_ao.png");
    ImageBuffer::from_fn(4, 4, |_, _| Rgb([200u8, 100, 0]))
        .save(&p)
        .unwrap();
    let mk = || {
        job(
            RecipeKind::Rmaos,
            [
                ChannelSource::Default { value: 0.5 },
                ChannelSource::Default { value: 0.0 },
                src(&p, RawMap::Ao, false),
                ChannelSource::Default { value: 1.0 },
            ],
            &[(&p, RawMap::Ao)],
        )
    };
    let a = assemble(&mk(), GraySource::R).unwrap();
    assert_eq!(a.non_gray, std::slice::from_ref(&p));
    close(a.image.at(0, 0)[2], 200.0 / 255.0);
    let a = assemble(&mk(), GraySource::Luma).unwrap();
    let luma = 0.2126 * (200.0 / 255.0) + 0.7152 * (100.0 / 255.0);
    assert!((a.image.at(0, 0)[2] - luma).abs() < 1e-6);
}

#[test]
fn sources_of_different_sizes_are_refused_with_every_size() {
    let d = tempfile::tempdir().unwrap();
    let a = gray8(d.path(), "iron_rough.png", 64, |_, _| 10);
    let b = gray8(d.path(), "iron_ao.png", 32, |_, _| 10);
    let err = check_sizes(&[a.clone(), b.clone()]).unwrap_err();
    match &err {
        PackError::SizeMismatch(s) => {
            assert_eq!(s, &vec![(a.clone(), 64, 64), (b.clone(), 32, 32)])
        }
        e => panic!("{e}"),
    }
    let text = err.to_string();
    assert!(
        text.contains("iron_rough.png 64x64") && text.contains("iron_ao.png 32x32"),
        "{text}"
    );
    assert_eq!(check_sizes(std::slice::from_ref(&a)).unwrap(), (64, 64));
    let j = job(
        RecipeKind::Rmaos,
        [
            src(&a, RawMap::Roughness, false),
            ChannelSource::Default { value: 0.0 },
            src(&b, RawMap::Ao, false),
            ChannelSource::Default { value: 1.0 },
        ],
        &[(&a, RawMap::Roughness), (&b, RawMap::Ao)],
    );
    assert!(matches!(
        assemble(&j, GraySource::R),
        Err(PackError::SizeMismatch(_))
    ));
}

#[test]
fn an_unreadable_source_is_a_load_error() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("iron_ao.png");
    std::fs::write(&p, b"not a png").unwrap();
    let j = job(
        RecipeKind::Rmaos,
        [
            ChannelSource::Default { value: 0.5 },
            ChannelSource::Default { value: 0.0 },
            src(&p, RawMap::Ao, false),
            ChannelSource::Default { value: 1.0 },
        ],
        &[(&p, RawMap::Ao)],
    );
    assert!(matches!(
        assemble(&j, GraySource::R),
        Err(PackError::Load(_))
    ));
}

/// Residual fix (re-review minor 1): the shared size is read by the decoder the file's content picks and is
/// MAX_DIM-checked before the packed canvas is allocated. A 16385-wide PNG is refused by `check_sizes`, with its
/// own extension and under a lying `.bmp` one, and `assemble` refuses it the same way.
#[test]
fn an_oversized_source_is_refused_before_the_canvas_is_allocated() {
    let d = tempfile::tempdir().unwrap();
    let png = d.path().join("wide_rough.png");
    ImageBuffer::from_fn(16385, 1, |_, _| Luma([0u8]))
        .save(&png)
        .unwrap();
    let too_large = |r: Result<(u32, u32), PackError>| {
        matches!(
            r,
            Err(PackError::Load(engine::load::LoadError::TooLarge {
                dim: 16385,
                ..
            }))
        )
    };
    assert!(too_large(check_sizes(std::slice::from_ref(&png))));
    let lying = d.path().join("wide_rough.bmp");
    std::fs::rename(&png, &lying).unwrap();
    assert!(too_large(check_sizes(std::slice::from_ref(&lying))));
    let j = job(
        RecipeKind::Rmaos,
        [
            src(&lying, RawMap::Roughness, false),
            ChannelSource::Default { value: 0.0 },
            ChannelSource::Default { value: 1.0 },
            ChannelSource::Default { value: 1.0 },
        ],
        &[(&lying, RawMap::Roughness)],
    );
    assert!(too_large(
        assemble(&j, GraySource::R).map(|a| (a.image.width, a.image.height))
    ));
}

/// Track A Task 1 (DECISIONS Note CN): an `_rmaos` is read by channel, alpha included, and is colour by design
/// (never `CHAN-NOT-GRAY`).
#[test]
fn an_rmaos_source_is_read_by_channel_including_alpha() {
    use image::Rgba;
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("x_rmaos.png");
    let px = [[10u8, 20, 30, 40], [50, 60, 70, 80]];
    ImageBuffer::from_fn(2, 1, |x, _| Rgba(px[x as usize]))
        .save(&p)
        .unwrap();
    let ch = |c: u8| ChannelSource::Source {
        path: p.clone(),
        role: RawMap::Rmaos,
        invert: false,
        channel: Some(c),
    };
    let sources = [PackSource {
        read: None,
        path: p.clone(),
        role: RawMap::Rmaos,
        stamp: None,
    }];
    let a = assemble_channels(
        &[ch(0), ch(2), ch(3), ChannelSource::Opaque],
        &sources,
        GraySource::R,
    )
    .unwrap();
    for (got, want) in a.image.px[0].iter().zip([10.0, 30.0, 40.0, 255.0]) {
        close(*got, want / 255.0);
    }
    assert!(a.non_gray.is_empty());
}
