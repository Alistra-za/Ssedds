//! The analysis disk cache's version guard (DECISIONS Note DZ): fails when what decoding, `analyse_with`, `make_thumbnail`
//! and pack assembly produce changes without `ANALYSIS_VERSION` being bumped. Same shape as encode_versions.rs: the pin is rewritten
//! when the version moved, and left alone (with a failure) when only the hash did.
use engine::analyse::Analysis;
use engine::analyse::{analyse_with, AnalyseHints};
use engine::analysis_store::ANALYSIS_VERSION;
use engine::config::HeaderForm;
use engine::config::{Format, GraySource, RawMap};
use engine::dds::{dds_bytes, dds_bytes_cube, level_size, resolve_header, write_atomic, DdsDesc};
use engine::image::{Rgba32F, SourceInfo};
use engine::load::load_image_meta;
use engine::pack::{assemble::assemble, ChannelSource, PackJob, PackSource, RecipeKind};
use engine::session::make_thumbnail;
use image::{ImageBuffer, Luma, Rgb, Rgba};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::Path;

fn img(w: u32, h: u32, f: impl Fn(u32, u32) -> [f32; 4]) -> Rgba32F {
    let mut i = Rgba32F::new(w, h, [0.0; 4]);
    for y in 0..h {
        for x in 0..w {
            *i.at_mut(x, y) = f(x, y);
        }
    }
    i
}

fn info(bit_depth: u8, has_alpha: bool, is_float: bool) -> SourceInfo {
    SourceInfo {
        bit_depth,
        channels: 4,
        has_alpha,
        is_float,
    }
}

/// What a planned load of PNGs made here (8-bit RGBA, 16-bit RGB, gray) and of one packed image (two gray members)
/// gives: the decoder and pack assembly are as much a part of a cached analysis as `analyse_with` is.
fn hash_loaded(h: &mut Sha256, hints: &[AnalyseHints]) {
    let d = tempfile::tempdir().unwrap();
    let p = |n: &str| d.path().join(n);
    ImageBuffer::from_fn(24, 20, |x, y| {
        Rgba([(x * 10) as u8, (y * 12) as u8, 77, (x * y) as u8])
    })
    .save(p("rgba8.png"))
    .unwrap();
    ImageBuffer::from_fn(24, 20, |x, y| {
        Rgb([(x * 2600) as u16, (y * 3000) as u16, 40000 + x as u16 * 7])
    })
    .save(p("rgb16.png"))
    .unwrap();
    ImageBuffer::from_fn(16, 16, |x, y| Luma([(x * 9 + y) as u8]))
        .save(p("gray.png"))
        .unwrap();
    ImageBuffer::from_fn(16, 16, |x, _| Luma([255 - x as u8 * 11]))
        .save(p("gray2.png"))
        .unwrap();
    for name in ["rgba8.png", "rgb16.png", "gray.png"] {
        let (im, info, meta) = load_image_meta(&p(name)).unwrap();
        h.update(format!("{info:?}{meta:?}"));
        for &hi in hints {
            h.update(serde_json::to_vec(&analyse_with(&im, &info, hi)).unwrap());
        }
        let t = make_thumbnail(&im, &info);
        h.update(&t.rgba);
    }
    let src = |n: &str, role| PackSource {
        path: p(n),
        role,
        stamp: None,
        read: None,
    };
    let job = PackJob {
        kind: RecipeKind::Rmaos,
        channels: [
            ChannelSource::Source {
                path: p("gray.png"),
                role: RawMap::Roughness,
                invert: false,
                channel: None,
            },
            ChannelSource::Fill {
                role: RawMap::Metallic,
                code: 51,
            },
            ChannelSource::Source {
                path: p("gray2.png"),
                role: RawMap::Ao,
                invert: true,
                channel: None,
            },
            ChannelSource::Default { value: 1.0 },
        ],
        gray: GraySource::R,
        sources: vec![
            src("gray.png", RawMap::Roughness),
            src("gray2.png", RawMap::Ao),
        ],
        hand_placed: false,
    };
    // Channel extraction, a non-gray member (read by luma) and a 16-bit member. Members of other sizes are refused by
    // `assemble`, so none is pinned.
    let mixed = PackJob {
        kind: RecipeKind::Rmaos,
        channels: [
            ChannelSource::Source {
                path: p("rgba8.png"),
                role: RawMap::Rmaos,
                invert: false,
                channel: Some(0),
            },
            ChannelSource::Source {
                path: p("rgba8.png"),
                role: RawMap::Rmaos,
                invert: true,
                channel: Some(3),
            },
            ChannelSource::Source {
                path: p("rgb16.png"),
                role: RawMap::Ao,
                invert: false,
                channel: None,
            },
            ChannelSource::Opaque,
        ],
        gray: GraySource::Luma,
        sources: vec![
            src("rgba8.png", RawMap::Rmaos),
            src("rgb16.png", RawMap::Ao),
        ],
        hand_placed: false,
    };
    for job in [&job, &mixed] {
        let asm = assemble(job, job.gray).unwrap();
        // File names only: the folder is a fresh temp dir each run.
        let non_gray: Vec<_> = asm
            .non_gray
            .iter()
            .map(|p| p.file_name().unwrap())
            .collect();
        h.update(format!("{:?}{non_gray:?}", asm.info));
        for &hi in hints {
            h.update(serde_json::to_vec(&analyse_with(&asm.image, &asm.info, hi)).unwrap());
        }
        h.update(&make_thumbnail(&asm.image, &asm.info).rgba);
    }
    hash_dds(h, hints, &p);
}

/// xorshift64 bytes: block data any BC decoder reads, so the DDS decode is pinned without an encoder (whose output
/// can differ by CPU).
fn noise(seed: u64, n: usize) -> Vec<u8> {
    let mut s = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..n)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s >> 24) as u8
        })
        .collect()
}

/// DDS sources written by the engine's own writer: 2D BC1, BC5 and BC7 (the last with a second mip, unread), and a
/// single BC1 cube (loaded as a 6:1 strip, DECISIONS DS7).
fn hash_dds(h: &mut Sha256, hints: &[AnalyseHints], p: &dyn Fn(&str) -> std::path::PathBuf) {
    let desc = |format, w, hh| DdsDesc {
        width: w,
        height: hh,
        format,
        srgb: false,
        header: resolve_header(format, false, HeaderForm::Auto),
    };
    let blocks = |f: Format, w: u32, hh: u32, seed| noise(seed, level_size(f, w, hh));
    let mut files = vec![];
    for (name, f, seed) in [
        ("bc1", Format::Bc1, 1u64),
        ("bc5", Format::Bc5, 2),
        ("bc7", Format::Bc7, 3),
    ] {
        let d = desc(f, 20, 12);
        let mut levels = vec![blocks(f, 20, 12, seed)];
        if f == Format::Bc7 {
            levels.push(blocks(f, 10, 6, seed + 9));
        }
        files.push((name, dds_bytes(&d, &levels).unwrap()));
    }
    let d = desc(Format::Bc1, 8, 8);
    let faces: Vec<_> = (0..6).map(|k| blocks(Format::Bc1, 8, 8, 20 + k)).collect();
    files.push(("cube", dds_bytes_cube(&d, &faces).unwrap()));
    for (name, bytes) in files {
        write_atomic(&p(&format!("{name}.dds")), &bytes).unwrap();
        let (im, info, meta) = load_image_meta(&p(&format!("{name}.dds"))).unwrap();
        h.update(format!("{name}{info:?}{meta:?}"));
        for &hi in hints {
            h.update(serde_json::to_vec(&analyse_with(&im, &info, hi)).unwrap());
        }
        h.update(&make_thumbnail(&im, &info).rgba);
    }
}

/// Sha256 of every fixture's analysis under every hint set used in practice, and of its thumbnail.
fn observed() -> String {
    let c = |v: u32, n: u32| v as f32 / n as f32;
    let fixtures = [
        // Colour with varying alpha.
        (
            img(64, 48, |x, y| [c(x, 63), c(y, 47), 0.35, c(x + y, 110)]),
            info(8, true, false),
        ),
        // A tilted normal map.
        (
            img(32, 32, |x, y| [0.5 + c(x, 128), 0.5 - c(y, 128), 0.9, 1.0]),
            info(8, false, false),
        ),
        // Float HDR.
        (
            img(40, 40, |x, y| [c(x, 10), c(y, 20), 0.25, 1.0]),
            info(32, false, true),
        ),
        // Complex grass shape: height twice the width.
        (
            img(16, 32, |x, y| {
                if y < 16 {
                    [0.25, 0.5, 0.15, if (x + y) % 3 == 0 { 1.0 } else { 0.0 }]
                } else {
                    [0.6, 0.5, 0.8, 0.05]
                }
            }),
            info(8, true, false),
        ),
        // Gray, near-opaque alpha.
        (
            img(24, 24, |x, _| [c(x, 23), c(x, 23), c(x, 23), 253.0 / 255.0]),
            info(8, true, false),
        ),
    ];
    let hint_sets = [
        AnalyseHints::default(),
        AnalyseHints {
            bands: true,
            ..Default::default()
        },
        AnalyseHints {
            bands: true,
            curl: true,
            steps: None,
        },
        AnalyseHints {
            bands: true,
            curl: false,
            steps: Some(0),
        },
        AnalyseHints {
            bands: true,
            curl: false,
            steps: Some(3),
        },
    ];
    let mut h = Sha256::new();
    for (im, info) in &fixtures {
        for hints in hint_sets {
            h.update(serde_json::to_vec(&analyse_with(im, info, hints)).unwrap());
        }
        let t = make_thumbnail(im, info);
        h.update(t.width.to_le_bytes());
        h.update(t.height.to_le_bytes());
        h.update(&t.rgba);
    }
    hash_loaded(&mut h, &hint_sets);
    // The shape of `Analysis`: a field added with a default is still a change to what an entry holds.
    h.update(serde_json::to_vec(&schemars::schema_for!(Analysis)).unwrap());
    hex::encode(h.finalize())
}

/// `Err` carries the failure. The pin is rewritten when it is missing or the version moved, never when only the
/// hash did.
fn check_pin(file: &Path, version: u32, sha: &str) -> Result<(), String> {
    let old: Option<Value> = std::fs::read_to_string(file)
        .ok()
        .map(|s| serde_json::from_str(&s).unwrap());
    let result = match &old {
        Some(p) if p["analysis_version"] == version && p["sha256"] != sha => Err(format!(
            "what decoding, analyse_with, make_thumbnail or pack assembly produce changed: bump ANALYSIS_VERSION in \
             crates/engine/src/analysis_store.rs and accept the new pin ({})",
            file.display()
        )),
        _ => Ok(()),
    };
    let pin = json!({ "analysis_version": version, "sha256": sha });
    // Compare the parsed value, not the bytes: the checkout is CRLF.
    if result.is_ok() && old.as_ref() != Some(&pin) {
        let text = serde_json::to_string_pretty(&pin).unwrap() + "\n";
        write_atomic(file, text.as_bytes()).unwrap();
    }
    result
}

#[test]
fn analysis_output_changes_only_with_a_version_bump() {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/analysis_version.json");
    if let Err(e) = check_pin(&file, ANALYSIS_VERSION, &observed()) {
        panic!("{e}");
    }
}

#[test]
fn check_pin_fails_an_unbumped_change_and_accepts_a_bumped_one() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("pin.json");
    assert!(check_pin(&f, 1, "aa").is_ok(), "a first run pins");
    assert!(check_pin(&f, 1, "aa").is_ok());
    assert!(check_pin(&f, 1, "bb").is_err(), "same version, new hash");
    assert!(check_pin(&f, 1, "aa").is_ok(), "the pin was left alone");
    assert!(check_pin(&f, 2, "bb").is_ok(), "bumped: accepted");
    assert!(check_pin(&f, 2, "bb").is_ok());
    assert!(check_pin(&f, 2, "aa").is_err());
}
