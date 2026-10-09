//! Golden tests (spec §11.1): each case generates its fixtures, runs the full `Engine::plan` + `Engine::run` and
//! asserts the format, `_SRGB` tag, header form, mip count, alpha-policy outcome and warning codes. Nothing is
//! committed but this file and no byte hashes are asserted (those belong to the `pinned-isa` CI job).
use engine::api::*;
use engine::codes::Code;
use engine::config::Target;
use engine::dds::{read_dds, DdsFile, DdsPixelFormat, HeaderKind};
use engine::decode::{decode_level, Decoded};
use engine::events::*;
use engine::manifest::Manifest;
use engine::overrides::parse_set;
use engine::policy::AcceptSuggestions;
use engine::validate::ValidationSummary;
use image::{DynamicImage, ImageBuffer, Rgb, Rgba};
use std::path::Path;
use std::sync::Arc;

struct Out {
    summary: BatchSummary,
    events: Vec<EngineEvent>,
}

fn request(dir: &Path, target: Target, sets: &[&str], allow: &[Code]) -> BatchRequest {
    let mut req = BatchRequest::new(vec![dir.join("in")], dir.join("out"), target);
    req.recursive = true;
    req.accept = AcceptSuggestions::High;
    req.overrides = sets.iter().map(|s| parse_set(s).unwrap()).collect();
    req.allow = allow.iter().copied().collect();
    req.on_existing = OnExisting::Overwrite;
    req
}

fn run(req: BatchRequest) -> Out {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "golden");
    let e = Engine::new().unwrap();
    let plan = e.plan(req, &rep).unwrap();
    let summary = e.run(plan, &rep, &CancelToken::default()).unwrap();
    Out {
        summary,
        events: sink.take(),
    }
}

fn convert(dir: &Path, target: Target, sets: &[&str], allow: &[Code]) -> Out {
    run(request(dir, target, sets, allow))
}

fn put(dir: &Path, rel: &str, img: DynamicImage) {
    let p = dir.join("in").join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    img.save(p).unwrap();
}

fn warnings(o: &Out) -> Vec<Code> {
    o.events
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::Warning(w) => Some(w.code),
            _ => None,
        })
        .collect()
}

fn dds(dir: &Path, rel: &str) -> DdsFile {
    read_dds(&std::fs::read(dir.join("out").join(rel)).unwrap()).unwrap()
}

/// The decoded RGBA8 texels of every mip level.
fn levels(f: &DdsFile) -> Vec<Vec<[u8; 4]>> {
    f.levels
        .iter()
        .map(
            |(w, h, r)| match decode_level(f.format, *w, *h, &f.data[r.clone()], None).unwrap() {
                Decoded::Ldr(px) => px,
                Decoded::Hdr(_) => panic!("expected an LDR format"),
            },
        )
        .collect()
}

/// Format (incl. the `_SRGB` tag), header form and mip count of an output, in one assertion.
fn assert_dds(f: &DdsFile, format: DdsPixelFormat, header: HeaderKind, mips: u32) {
    assert_eq!(
        (f.format, f.header_kind, f.mip_count),
        (format, header, mips),
        "format / header / mips of {}x{}",
        f.width,
        f.height
    );
}

/// The `file_done` validation summary of the input file whose path ends with `suffix`.
fn validation_of(o: &Out, suffix: &str) -> ValidationSummary {
    o.events
        .iter()
        .find_map(|e| match &e.kind {
            EventKind::FileDone { validation, .. }
                if e.path.as_deref().is_some_and(|p| p.ends_with(suffix)) =>
            {
                Some(validation.clone())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no file_done for {suffix}"))
}

fn rgb(w: u32) -> DynamicImage {
    DynamicImage::ImageRgb8(ImageBuffer::from_fn(w, w, |x, y| {
        Rgb([(x * 4) as u8, (y * 4) as u8, 90])
    }))
}

fn rgba(w: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> DynamicImage {
    DynamicImage::ImageRgba8(ImageBuffer::from_fn(w, w, |x, y| Rgba(f(x, y))))
}

fn gray_ramp(w: u32) -> DynamicImage {
    DynamicImage::ImageRgb8(ImageBuffer::from_fn(w, w, |x, _| Rgb([(x * 4) as u8; 3])))
}

/// A dark opaque ramp tiled in 4x4 blocks: bc7e picks a p-bit-0 endpoint for it, so without `exact_opaque` the
/// decoded alpha is 254 (the image `tests/encode.rs` uses).
fn dark_ramp(w: u32, alpha: u8) -> DynamicImage {
    rgba(w, |x, y| {
        let t = ((y % 4) * 4 + x % 4) as u8;
        [t, t / 2, 100 + t, alpha]
    })
}

#[test]
fn vanilla_diffuse_bc7_unorm_no_srgb_full_chain() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/rock/rock.png", rgb(64));
    let o = convert(
        d.path(),
        Target::Vanilla,
        &["file:**/rock.png.subtype=diffuse"],
        &[],
    );
    assert_eq!(
        (o.summary.converted, o.summary.exit_code()),
        (1, 0),
        "{:?}",
        warnings(&o)
    );
    // 64 -> 1 is 7 levels; vanilla never carries the _SRGB tag.
    assert_dds(
        &dds(d.path(), "textures/rock/rock.dds"),
        DdsPixelFormat::Bc7 { srgb: false },
        HeaderKind::Dx10,
        7,
    );
}

#[test]
fn cs_pbr_albedo_is_srgb_and_vanilla_refuses_it() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/pbr/iron.png", rgb(64));
    let o = convert(d.path(), Target::Cs, &["subtype=pbr_albedo"], &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    assert_dds(
        &dds(d.path(), "textures/pbr/iron.dds"),
        DdsPixelFormat::Bc7 { srgb: true },
        HeaderKind::Dx10,
        7,
    );

    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/pbr/iron.png", rgb(64));
    // file-scoped: an unscoped `subtype=pbr_albedo` under vanilla is a batch-wide usage error (exit 2, Task 37)
    let o = convert(
        d.path(),
        Target::Vanilla,
        &["file:**/iron.png.subtype=pbr_albedo"],
        &[],
    );
    assert_eq!(
        (o.summary.converted, o.summary.skipped, o.summary.failed),
        (0, 0, 1),
        "{:?}",
        warnings(&o)
    );
    assert_eq!(o.summary.exit_code(), 1);
    assert!(
        warnings(&o).contains(&Code::CfgUnavailable)
            && warnings(&o).contains(&Code::TagSrgbVanilla)
    );
    // TAG-SRGB-VANILLA is error severity: the file fails at Resolve and nothing is written.
    assert!(o.events.iter().any(|e| matches!(
        &e.kind,
        EventKind::FileFailed {
            stage: Stage::Resolve,
            ..
        }
    )));
    assert!(!d.path().join("out/textures/pbr/iron.dds").exists());
}

#[test]
fn vanilla_normal_keeps_specular_alpha() {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "textures/a/iron_n.png",
        rgba(64, |x, _| [128, 128, 255, (x * 4) as u8]),
    );
    let o = convert(d.path(), Target::Vanilla, &[], &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    let f = dds(d.path(), "textures/a/iron_n.dds");
    assert_dds(&f, DdsPixelFormat::Bc7 { srgb: false }, HeaderKind::Dx10, 7);
    assert!(
        levels(&f)[0].iter().any(|p| p[3] < 200),
        "the specular alpha must survive"
    );
}

#[test]
fn landscape_diffuse_alpha_is_data() {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "textures/landscape/dirt01.png",
        rgba(64, |x, y| [100, 80, 60, ((x + y) * 2) as u8]),
    );
    let o = convert(d.path(), Target::Cs, &[], &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    let f = dds(d.path(), "textures/landscape/dirt01.dds");
    assert_dds(&f, DdsPixelFormat::Bc7 { srgb: false }, HeaderKind::Dx10, 7);
    assert!(levels(&f)[0].iter().any(|p| p[3] < 200) && levels(&f)[0].iter().any(|p| p[3] > 100));
}

#[test]
fn normal_maps_are_bc7_under_cs_and_bc5_only_for_flow() {
    let d = tempfile::tempdir().unwrap();
    // A plain `_n` (non-PBR set) and a PBR-set `_n` (alpha-less), both normal maps under CS.
    put(d.path(), "textures/a/wall_n.png", rgb(64));
    put(d.path(), "textures/pbr/iron_n.png", rgb(64));
    put(
        d.path(),
        "textures/pbr/iron_rmaos.png",
        rgba(64, |x, _| [(x * 4) as u8, 0, 200, 255]),
    );
    put(d.path(), "textures/hair/h_flow.png", rgb(64));
    let o = convert(d.path(), Target::Cs, &[], &[]);
    assert_eq!(
        (o.summary.converted, o.summary.exit_code()),
        (4, 0),
        "{:?}",
        warnings(&o)
    );
    for rel in ["textures/a/wall_n.dds", "textures/pbr/iron_n.dds"] {
        assert_dds(
            &dds(d.path(), rel),
            DdsPixelFormat::Bc7 { srgb: false },
            HeaderKind::Dx10,
            7,
        );
    }
    assert_dds(
        &dds(d.path(), "textures/hair/h_flow.dds"),
        DdsPixelFormat::Bc5,
        HeaderKind::Dx10,
        7,
    );
}

#[test]
fn bc5_on_a_normal_map_blocks_until_allowed() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/iron_n.png", rgb(64));
    let sets = [
        "subtype:normal.format=bc5",
        "subtype:normal.encoder.backend=rgbcx",
    ];
    let o = convert(d.path(), Target::Cs, &sets, &[]);
    assert_eq!((o.summary.converted, o.summary.skipped), (0, 1));
    assert_eq!(
        o.summary.exit_code(),
        1,
        "a blocked file is not an intentional skip"
    );
    assert!(warnings(&o).contains(&Code::FmtBc5Normal));
    assert!(!d.path().join("out/textures/a/iron_n.dds").exists());

    let o = convert(d.path(), Target::Cs, &sets, &[Code::FmtBc5Normal]);
    assert_eq!(o.summary.converted, 1);
    assert_dds(
        &dds(d.path(), "textures/a/iron_n.dds"),
        DdsPixelFormat::Bc5,
        HeaderKind::Dx10,
        7,
    );
}

#[test]
fn complex_m_near_251_warns_and_plain_m_is_bc4() {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "textures/a/iron_m.png",
        rgba(64, |_, _| [80, 160, 40, 250]),
    );
    put(d.path(), "textures/b/rock_m.png", gray_ramp(64));
    let o = convert(d.path(), Target::Cs, &[], &[]);
    assert_eq!(o.summary.converted, 2, "{:?}", warnings(&o));
    assert!(warnings(&o).contains(&Code::ValMComplexMargin));
    assert_dds(
        &dds(d.path(), "textures/a/iron_m.dds"),
        DdsPixelFormat::Bc7 { srgb: false },
        HeaderKind::Dx10,
        7,
    );
    assert_dds(
        &dds(d.path(), "textures/b/rock_m.dds"),
        DdsPixelFormat::Bc4,
        HeaderKind::Dx10,
        7,
    );
}

#[test]
fn exact_opaque_m_decodes_alpha_255_at_every_mip() {
    // Complex `_m` (alpha is data, exact_opaque on by default) and an envmask forced to BC7 (alpha removed, the
    // channel synthesised as 255; exact_opaque on by default): bc7e alone would decode this image's alpha as 254.
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/iron_m.png", dark_ramp(64, 255));
    put(d.path(), "textures/b/rock_m.png", dark_ramp(64, 255));
    let sets = [
        "file:**/iron_m.png.subtype=complex_material",
        "file:**/rock_m.png.subtype=envmask",
        "subtype:envmask.format=bc7",
        "subtype:envmask.encoder.backend=bc7e",
    ];
    let o = convert(d.path(), Target::Cs, &sets, &[]);
    assert_eq!(o.summary.converted, 2, "{:?}", warnings(&o));
    assert_eq!(o.summary.validation_failed, 0, "{:?}", warnings(&o));
    assert!(!warnings(&o).contains(&Code::ValAlphaGuarantee));
    for rel in ["textures/a/iron_m.dds", "textures/b/rock_m.dds"] {
        let f = dds(d.path(), rel);
        assert_dds(&f, DdsPixelFormat::Bc7 { srgb: false }, HeaderKind::Dx10, 7);
        let lv = levels(&f);
        assert_eq!(lv.len(), 7);
        for (i, l) in lv.iter().enumerate() {
            assert!(
                l.iter().all(|p| p[3] == 255),
                "{rel} mip {i}: decoded alpha below 255"
            );
        }
    }
    // The guard bites: with the flag off the same image decodes alpha 254 somewhere.
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/iron_m.png", dark_ramp(64, 255));
    let off = [sets[0], "file:**/iron_m.png.alpha.exact_opaque=false"];
    let o = convert(d.path(), Target::Cs, &off, &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    let lv = levels(&dds(d.path(), "textures/a/iron_m.dds"));
    assert!(
        lv.iter().flatten().any(|p| p[3] != 255),
        "exact_opaque off should not be exact"
    );
}

/// Alpha with ~8 % of its mass above the 127 threshold, in fractal-like noise so box-filtered mips drift.
fn sparse_alpha() -> Vec<u8> {
    let mut s: u32 = 7;
    (0..64 * 64)
        .map(|_| {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let u = (s >> 8) as f32 / (1u32 << 24) as f32;
            (u.powi(4) * 255.0) as u8
        })
        .collect()
}

fn coverage(px: &[[u8; 4]]) -> f64 {
    px.iter().filter(|p| p[3] > 127).count() as f64 / px.len() as f64
}

#[test]
fn cutout_keeps_coverage_and_blend_has_none() {
    let d = tempfile::tempdir().unwrap();
    let alpha = sparse_alpha();
    let base = alpha.iter().filter(|&&a| a > 127).count() as f64 / alpha.len() as f64;
    put(
        d.path(),
        "textures/plants/leaf.png",
        rgba(64, |x, y| [40, 160, 40, alpha[(y * 64 + x) as usize]]),
    );
    put(
        d.path(),
        "textures/decals/blood.png",
        rgba(64, |x, y| {
            [
                150,
                0,
                0,
                (255 - ((x as i32 - 32).pow(2) + (y as i32 - 32).pow(2)).min(255)) as u8,
            ]
        }),
    );
    let o = convert(
        d.path(),
        Target::Cs,
        &[
            "file:**/leaf.png.subtype=diffuse",
            "file:**/leaf.png.alpha.policy=cutout",
            "file:**/blood.png.subtype=diffuse",
            "file:**/blood.png.alpha.policy=blend",
        ],
        &[],
    );
    assert_eq!(
        (o.summary.converted, o.summary.validation_failed),
        (2, 0),
        "{:?}",
        warnings(&o)
    );
    assert!(!warnings(&o).contains(&Code::ValCoverage));
    for rel in ["textures/plants/leaf.dds", "textures/decals/blood.dds"] {
        assert_dds(
            &dds(d.path(), rel),
            DdsPixelFormat::Bc7 { srgb: false },
            HeaderKind::Dx10,
            7,
        );
    }
    // Cutout: the coverage is measured on every level, and held within the default 2-point tolerance on the
    // levels with at least 256 texels (64, 32 and 16 px; smaller ones are too coarse to check).
    let leaf = validation_of(&o, "leaf.png");
    assert_eq!(leaf.coverage.len(), 7, "{:?}", leaf.coverage);
    for (i, c) in leaf.coverage.iter().take(3).enumerate() {
        assert!(
            (c - base).abs() * 100.0 <= 2.0,
            "mip {i}: coverage {c} vs source {base}"
        );
    }
    let decoded = levels(&dds(d.path(), "textures/plants/leaf.dds"));
    assert!(
        (coverage(&decoded[2]) - base).abs() * 100.0 <= 2.5,
        "decoded 16x16 coverage drifted"
    );
    // The guard bites: with coverage preservation switched off the box-filtered mips drift past the tolerance.
    let d2 = tempfile::tempdir().unwrap();
    put(
        d2.path(),
        "textures/plants/leaf.png",
        rgba(64, |x, y| [40, 160, 40, alpha[(y * 64 + x) as usize]]),
    );
    let off = convert(
        d2.path(),
        Target::Cs,
        &[
            "file:**/leaf.png.subtype=diffuse",
            "file:**/leaf.png.alpha.policy=cutout",
            "file:**/leaf.png.mips.coverage=off",
        ],
        &[],
    );
    assert_eq!(off.summary.converted, 1, "{:?}", warnings(&off));
    let drift = levels(&dds(d2.path(), "textures/plants/leaf.dds"))
        .iter()
        .take(3)
        .map(|l| (coverage(l) - base).abs() * 100.0)
        .fold(0.0f64, f64::max);
    assert!(drift > 2.0, "coverage off should drift, got {drift} points");
    // Blend: the alpha is kept but coverage preservation is off by default, so it is not measured.
    assert!(validation_of(&o, "blood.png").coverage.is_empty());
}

#[test]
fn bc6h_emissive_in_a_pbr_set() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/pbr/lamp_g.png", rgb(64));
    let o = convert(d.path(), Target::Cs, &[], &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    assert_dds(
        &dds(d.path(), "textures/pbr/lamp_g.dds"),
        DdsPixelFormat::Bc6hUf16,
        HeaderKind::Dx10,
        7,
    );
}

#[test]
fn sky_has_no_mips() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/sky/grad.png", rgb(64));
    let o = convert(
        d.path(),
        Target::Cs,
        &["file:**/grad.png.subtype=diffuse"],
        &[],
    );
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    assert_eq!(dds(d.path(), "textures/sky/grad.dds").mip_count, 1);
}

#[test]
fn bc1_compact_is_legacy_dxt1_and_bc4_legacy_header_is_bc4u() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/wall.png", rgb(64));
    put(d.path(), "textures/a/wall_p.png", gray_ramp(64));
    let o = convert(
        d.path(),
        Target::Vanilla,
        &[
            "file:**/wall.png.subtype=diffuse",
            "subtype:diffuse.format=bc1",
            "subtype:diffuse.encoder.backend=rgbcx",
            "subtype:height.header=legacy",
        ],
        &[],
    );
    assert_eq!(o.summary.converted, 2, "{:?}", warnings(&o));
    let f = dds(d.path(), "textures/a/wall.dds");
    assert_dds(
        &f,
        DdsPixelFormat::Bc1 { srgb: false },
        HeaderKind::Legacy,
        7,
    );
    assert_eq!(&f.fourcc, b"DXT1");
    let p = dds(d.path(), "textures/a/wall_p.dds");
    assert_dds(&p, DdsPixelFormat::Bc4, HeaderKind::Legacy, 7);
    assert_eq!(&p.fourcc, b"BC4U");
}

#[test]
fn rmaos_with_constant_alpha_removes_it_and_16_bit_reports_bitdepth() {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "textures/pbr/iron_rmaos.png",
        rgba(64, |x, _| [(x * 4) as u8, 0, 200, 255]),
    );
    put(
        d.path(),
        "textures/pbr/iron_n.png",
        DynamicImage::ImageRgb16(ImageBuffer::from_fn(64, 64, |_, _| {
            Rgb([32768u16, 32768, 65535])
        })),
    );
    let o = convert(d.path(), Target::Cs, &[], &[]);
    assert_eq!(o.summary.converted, 2, "{:?}", warnings(&o));
    assert!(warnings(&o).contains(&Code::LossBitdepth));
    assert!(o
        .events
        .iter()
        .any(|e| matches!(&e.kind, EventKind::FilePlanned { job }
        if job["profile"]["subtype"] == "rmaos" && job["profile"]["alpha"]["policy"] == "remove")));
    assert_dds(
        &dds(d.path(), "textures/pbr/iron_rmaos.dds"),
        DdsPixelFormat::Bc7 { srgb: false },
        HeaderKind::Dx10,
        7,
    );
    assert!(levels(&dds(d.path(), "textures/pbr/iron_rmaos.dds"))
        .iter()
        .flatten()
        .all(|p| p[3] >= 254));
}

#[test]
fn diffuse_with_varying_alpha_is_skipped_never_guessed() {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "textures/plants/leaf.png",
        rgba(64, |x, _| [40, 160, 40, (x * 4) as u8]),
    );
    put(d.path(), "textures/rock/rock.png", rgb(64));
    let o = convert(
        d.path(),
        Target::Cs,
        &[
            "file:**/leaf.png.subtype=diffuse",
            "file:**/rock.png.subtype=diffuse",
        ],
        &[],
    );
    assert_eq!(
        (
            o.summary.converted,
            o.summary.skipped,
            o.summary.skipped_intentional
        ),
        (1, 1, 0)
    );
    assert_eq!(o.summary.exit_code(), 1);
    assert!(o.events.iter().any(
        |e| e.path.as_deref().is_some_and(|p| p.ends_with("leaf.png"))
            && matches!(
                &e.kind,
                EventKind::FileSkipped {
                    code: Some(Code::SugLowConfidence),
                    ..
                }
            )
    ));
    assert!(!d.path().join("out/textures/plants/leaf.dds").exists());
    assert!(d.path().join("out/textures/rock/rock.dds").exists());
    // A mere high-confidence acceptance does not decide the alpha either; only an explicit policy does.
    let o = convert(
        d.path(),
        Target::Cs,
        &[
            "file:**/leaf.png.subtype=diffuse",
            "file:**/leaf.png.alpha.policy=blend",
            "file:**/rock.png.subtype=diffuse",
        ],
        &[],
    );
    assert_eq!(
        (o.summary.skipped, o.summary.exit_code()),
        (0, 0),
        "{:?}",
        warnings(&o)
    );
    assert!(d.path().join("out/textures/plants/leaf.dds").exists());
}

#[test]
fn a_stray_dds_input_is_skipped_on_purpose_and_exits_zero() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/rock/rock.png", rgb(64));
    std::fs::create_dir_all(d.path().join("in/textures/old")).unwrap();
    std::fs::write(
        d.path().join("in/textures/old/stray.dds"),
        b"DDS not a real texture",
    )
    .unwrap();
    let o = convert(
        d.path(),
        Target::Vanilla,
        &["file:**/rock.png.subtype=diffuse"],
        &[],
    );
    // Note I: IN-DDS-SKIPPED is reported and counted, but does not make the batch partial.
    assert_eq!(
        (
            o.summary.converted,
            o.summary.skipped,
            o.summary.skipped_intentional,
            o.summary.failed
        ),
        (1, 1, 1, 0)
    );
    assert_eq!(o.summary.exit_code(), 0);
    assert!(o.events.iter().any(
        |e| e.path.as_deref().is_some_and(|p| p.ends_with("stray.dds"))
            && matches!(
                &e.kind,
                EventKind::FileSkipped {
                    code: Some(Code::InDdsSkipped),
                    ..
                }
            )
    ));
    assert!(!d.path().join("out/textures/old/stray.dds").exists());
    let done = o.events.iter().find_map(|e| match &e.kind {
        EventKind::BatchDone {
            skipped,
            skipped_intentional,
            ..
        } => Some((*skipped, *skipped_intentional)),
        _ => None,
    });
    assert_eq!(done, Some((1, 1)));
}

#[test]
fn suffix_alias_writes_the_canonical_name_with_the_alias_preset_provenance() {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "textures/armor/iron_rmaosultrahigh.png",
        rgba(64, |x, _| [(x * 4) as u8, 0, 200, 255]),
    );
    let ultra = d.path().join("ultra.toml");
    std::fs::write(
        &ultra,
        "schema_version = 1\nid = \"ultra\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n\n\
         [suffix_aliases]\nrmaosultrahigh = \"rmaos\"\n\n[subtypes.rmaos]\nmips = { levels = 1 }\n",
    )
    .unwrap();
    let mut req = request(d.path(), Target::Cs, &[], &[]);
    req.preset_files = vec![ultra];
    let o = run(req);
    assert_eq!(
        (o.summary.converted, o.summary.exit_code()),
        (1, 0),
        "{:?}",
        warnings(&o)
    );
    let f = dds(d.path(), "textures/armor/iron_rmaos.dds");
    assert_dds(&f, DdsPixelFormat::Bc7 { srgb: false }, HeaderKind::Dx10, 1);
    assert!(!d
        .path()
        .join("out/textures/armor/iron_rmaosultrahigh.dds")
        .exists());
    let m = Manifest::at(&d.path().join("out")).load().unwrap();
    assert_eq!(m.len(), 1, "{:?}", m.keys());
    let e = &m["textures/armor/iron_rmaos.dds"];
    assert_eq!(e.preset.as_ref().unwrap().id, "ultra");
    assert_eq!(e.raw["provenance"]["mips.levels"], "preset:ultra");
}
