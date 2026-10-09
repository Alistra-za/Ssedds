//! Golden tests (spec §11.1): each case generates its fixtures, runs the full `Engine::plan` + `Engine::run` and
//! asserts the format, `_SRGB` tag, header form, mip count, alpha-policy outcome and warning codes. Nothing is
//! committed but this file and no byte hashes are asserted (those belong to the `pinned-isa` CI job).
use engine::api::*;
use engine::codes::{Code, Severity, Warning};
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
    let plan = e.plan(req, &rep, &CancelToken::default()).unwrap();
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

fn is_of(e: &EngineEvent, suffix: &str) -> bool {
    e.path.as_deref().is_some_and(|p| p.ends_with(suffix))
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

/// The `code` warning of the input file whose path ends with `suffix`.
fn warning_of(o: &Out, suffix: &str, code: Code) -> Warning {
    o.events
        .iter()
        .find_map(|e| match &e.kind {
            EventKind::Warning(w) if w.code == code && is_of(e, suffix) => Some(w.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no {code:?} for {suffix}: {:?}", warnings(o)))
}

/// The code of the `file_skipped` event of the input file whose path ends with `suffix`.
fn skip_code(o: &Out, suffix: &str) -> Option<Code> {
    o.events
        .iter()
        .find_map(|e| match &e.kind {
            EventKind::FileSkipped { code, .. } if is_of(e, suffix) => Some(*code),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no file_skipped for {suffix}"))
}

/// The resolved job (`file_planned`) of the input file whose path ends with `suffix`.
fn planned(o: &Out, suffix: &str) -> serde_json::Value {
    o.events
        .iter()
        .find_map(|e| match &e.kind {
            EventKind::FilePlanned { job } if is_of(e, suffix) => Some(job.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no file_planned for {suffix}"))
}

/// Sub-type, alpha policy, `alpha.exact_opaque` and format of a planned job, in one assertion.
fn assert_profile(
    o: &Out,
    suffix: &str,
    subtype: &str,
    policy: &str,
    exact_opaque: bool,
    format: &str,
) {
    let job = planned(o, suffix);
    let p = &job["profile"];
    assert_eq!(
        (
            p["subtype"].as_str(),
            p["alpha"]["policy"].as_str(),
            p["alpha"]["exact_opaque"].as_bool(),
            p["format"].as_str()
        ),
        (
            Some(subtype),
            Some(policy),
            Some(exact_opaque),
            Some(format)
        ),
        "planned profile of {suffix}"
    );
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
            EventKind::FileDone { validation, .. } if is_of(e, suffix) => Some(validation.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no file_done for {suffix}"))
}

/// Every decoded alpha of mip 0 is within `tol` of the source alpha `a(x, y)` (a 64 px source).
fn assert_alpha_kept(f: &DdsFile, a: impl Fn(u32, u32) -> u8, tol: i32) {
    let l0 = &levels(f)[0];
    assert_eq!(l0.len(), 64 * 64);
    for (i, p) in l0.iter().enumerate() {
        let (x, y) = (i as u32 % 64, i as u32 / 64);
        let want = a(x, y) as i32;
        assert!(
            (p[3] as i32 - want).abs() <= tol,
            "alpha at ({x}, {y}): {} vs source {want}",
            p[3]
        );
    }
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

/// A flat tangent-space normal (+Z), unit length.
fn flat_normal(w: u32) -> DynamicImage {
    DynamicImage::ImageRgb8(ImageBuffer::from_fn(w, w, |_, _| Rgb([128, 128, 255])))
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
    assert_eq!(warnings(&o), vec![], "a clean fixture raises nothing");
    // No alpha resolves to remove; exact_opaque is off for ordinary colour (spec §3.2/§3.3).
    assert_profile(&o, "rock.png", "diffuse", "remove", false, "bc7");
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
    assert_eq!(warnings(&o), vec![], "a clean fixture raises nothing");
    assert_profile(&o, "iron.png", "pbr_albedo", "remove", false, "bc7");
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
    // Spec §4.2/§10: CFG-UNAVAILABLE is blocking and the root cause; the inherited `_SRGB` tag is not reported
    // (no TAG-SRGB-VANILLA cascade), so the file is blocked (skipped), not failed, and nothing is written.
    assert_eq!(
        (o.summary.converted, o.summary.skipped, o.summary.failed),
        (0, 1, 0),
        "{:?}",
        warnings(&o)
    );
    assert_eq!(o.summary.exit_code(), 1);
    assert_eq!(warnings(&o), vec![Code::CfgUnavailable]);
    assert_eq!(skip_code(&o, "iron.png"), Some(Code::CfgUnavailable));
    assert!(!d.path().join("out/textures/pbr/iron.dds").exists());
}

/// Spec §4.2: every `—` cell under cpm and vanilla is CFG-UNAVAILABLE (blocking): nothing is written and the batch
/// exits 1. The unavailable profile raises nothing else (fix batch 39 item 2).
#[test]
fn cpm_and_vanilla_block_unavailable_subtypes() {
    for target in [Target::Cpm, Target::Vanilla] {
        let d = tempfile::tempdir().unwrap();
        put(d.path(), "textures/pbr/iron.png", rgb(64));
        put(
            d.path(),
            "textures/arm/steel_rmaos.png",
            rgba(64, |x, _| [(x * 4) as u8, 0, 200, 255]),
        );
        put(d.path(), "textures/hair/h_flow.png", rgb(64));
        put(d.path(), "textures/cloth/silk_f.png", rgb(64));
        let o = convert(
            d.path(),
            target,
            &["file:**/iron.png.subtype=pbr_albedo"],
            &[],
        );
        assert_eq!(
            (o.summary.converted, o.summary.skipped, o.summary.failed),
            (0, 4, 0),
            "{target:?}: {:?}",
            warnings(&o)
        );
        assert_eq!(o.summary.exit_code(), 1, "{target:?}");
        assert_eq!(warnings(&o), vec![Code::CfgUnavailable; 4], "{target:?}");
        for (src, out, subtype) in [
            ("iron.png", "textures/pbr/iron.dds", "pbr_albedo"),
            ("steel_rmaos.png", "textures/arm/steel_rmaos.dds", "rmaos"),
            ("h_flow.png", "textures/hair/h_flow.dds", "flow"),
            ("silk_f.png", "textures/cloth/silk_f.dds", "pbr_fuzz"),
        ] {
            assert_eq!(planned(&o, src)["profile"]["subtype"], subtype);
            assert_eq!(
                skip_code(&o, src),
                Some(Code::CfgUnavailable),
                "{target:?} {src}"
            );
            assert!(!d.path().join("out").join(out).exists(), "{target:?} {out}");
        }
    }
}

/// Spec §4.2/§10 [D#3]: BC6H is cs-only. Under vanilla an unavailable `pbr_emissive` is blocked by CFG-UNAVAILABLE
/// alone; BC6H forced onto an available sub-type is an error that `--allow CFG-UNAVAILABLE` cannot clear.
#[test]
fn vanilla_never_writes_bc6h() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/pbr/lamp_g.png", rgb(64));
    let o = convert(
        d.path(),
        Target::Vanilla,
        &["file:**/lamp_g.png.subtype=pbr_emissive"],
        &[],
    );
    assert_eq!(
        (o.summary.converted, o.summary.skipped, o.summary.failed),
        (0, 1, 0),
        "{:?}",
        warnings(&o)
    );
    assert_eq!(o.summary.exit_code(), 1);
    assert_eq!(warnings(&o), vec![Code::CfgUnavailable]);
    assert_eq!(skip_code(&o, "lamp_g.png"), Some(Code::CfgUnavailable));
    assert!(!d.path().join("out/textures/pbr/lamp_g.dds").exists());

    let sets = [
        "file:**/lamp_g.png.format=bc6h_uf16",
        "file:**/lamp_g.png.color.tag=n/a",
    ];
    for allow in [&[][..], &[Code::CfgUnavailable][..]] {
        let d = tempfile::tempdir().unwrap();
        put(d.path(), "textures/a/lamp_g.png", rgb(64));
        let o = convert(d.path(), Target::Vanilla, &sets, allow);
        assert_eq!(
            (o.summary.converted, o.summary.skipped, o.summary.failed),
            (0, 0, 1),
            "allow {allow:?}: {:?}",
            warnings(&o)
        );
        assert_eq!(o.summary.exit_code(), 1);
        assert_eq!(planned(&o, "lamp_g.png")["profile"]["subtype"], "glow");
        let w = warning_of(&o, "lamp_g.png", Code::CfgUnavailable);
        assert_eq!(w.severity, Severity::Error, "{}", w.message);
        assert!(o.events.iter().any(|e| is_of(e, "lamp_g.png")
            && matches!(
                &e.kind,
                EventKind::FileFailed {
                    stage: Stage::Resolve,
                    ..
                }
            )));
        assert!(!d.path().join("out/textures/a/lamp_g.dds").exists());
    }
}

/// Spec §4.2: with `--allow CFG-UNAVAILABLE` the offered fallback is resolved and written, with its own checks; the
/// acknowledged CFG-UNAVAILABLE stays as a warning. complex_material → envmask reports LOSS-ALPHA-DATA (§4.2 cell).
#[test]
fn vanilla_fallbacks_are_written_with_allow_cfg_unavailable() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/pbr/iron.png", rgb(64));
    put(
        d.path(),
        "textures/a/iron_n.png",
        rgba(64, |x, _| [128, 128, 255, (x * 4) as u8]),
    );
    put(d.path(), "textures/b/lamp_g.png", rgb(64));
    put(
        d.path(),
        "textures/c/iron_m.png",
        rgba(64, |x, _| [90, 90, 90, (x * 4) as u8]),
    );
    let sets = [
        "file:**/iron.png.subtype=pbr_albedo",
        "file:**/iron_n.png.subtype=pbr_normal",
        "file:**/lamp_g.png.subtype=pbr_emissive",
        "file:**/iron_m.png.subtype=complex_material",
    ];
    // Without --allow every one of them is blocked and nothing is written.
    let o = convert(d.path(), Target::Vanilla, &sets, &[]);
    assert_eq!(
        (
            o.summary.converted,
            o.summary.skipped,
            o.summary.exit_code()
        ),
        (0, 4, 1),
        "{:?}",
        warnings(&o)
    );
    assert_eq!(warnings(&o), vec![Code::CfgUnavailable; 4]);
    assert!(!d.path().join("out/textures").exists());

    let allow = [Code::CfgUnavailable, Code::LossAlphaData];
    let o = convert(d.path(), Target::Vanilla, &sets, &allow);
    assert_eq!(
        (o.summary.converted, o.summary.exit_code()),
        (4, 0),
        "{:?}",
        warnings(&o)
    );
    for (src, from, to) in [
        ("iron.png", "pbr_albedo", "diffuse"),
        ("iron_n.png", "pbr_normal", "normal"),
        ("lamp_g.png", "pbr_emissive", "glow"),
        ("iron_m.png", "complex_material", "envmask"),
    ] {
        let w = warning_of(&o, src, Code::CfgUnavailable);
        assert_eq!(w.severity, Severity::Warning, "{src}: {}", w.message);
        assert!(
            w.message.contains(from) && w.message.contains(to),
            "{src}: {}",
            w.message
        );
        assert_eq!(
            planned(&o, src)["provenance"]["subtype"],
            "fallback",
            "{src}"
        );
    }
    assert_eq!(
        warning_of(&o, "iron_m.png", Code::LossAlphaData).severity,
        Severity::Warning
    );
    assert_profile(&o, "iron.png", "diffuse", "remove", false, "bc7");
    assert_profile(&o, "iron_n.png", "normal", "data", false, "bc7");
    assert_profile(&o, "lamp_g.png", "glow", "remove", false, "bc7");
    assert_profile(&o, "iron_m.png", "envmask", "remove", true, "bc4");
    for rel in [
        "textures/pbr/iron.dds",
        "textures/a/iron_n.dds",
        "textures/b/lamp_g.dds",
    ] {
        assert_dds(
            &dds(d.path(), rel),
            DdsPixelFormat::Bc7 { srgb: false },
            HeaderKind::Dx10,
            7,
        );
    }
    assert_dds(
        &dds(d.path(), "textures/c/iron_m.dds"),
        DdsPixelFormat::Bc4,
        HeaderKind::Dx10,
        7,
    );
    // The normal fallback keeps the specular alpha that pbr_normal would have removed.
    assert_alpha_kept(
        &dds(d.path(), "textures/a/iron_n.dds"),
        |x, _| (x * 4) as u8,
        8,
    );
}

/// Spec §4.2/§3.4: cpm writes complex `_m` as BC7 and never writes `_SRGB`, not even when asked.
#[test]
fn cpm_complex_m_is_bc7_and_nothing_is_srgb() {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "textures/a/iron_m.png",
        rgba(64, |x, y| [80, 160, 40, (100 + (x + y) % 64) as u8]),
    );
    put(d.path(), "textures/rock/rock.png", rgb(64));
    put(d.path(), "textures/b/lamp_g.png", rgb(64));
    put(d.path(), "textures/c/wall_n.png", flat_normal(64));
    put(
        d.path(),
        "textures/landscape/dirt01.png",
        rgba(64, |x, y| [100, 80, 60, ((x + y) * 2) as u8]),
    );
    let o = convert(
        d.path(),
        Target::Cpm,
        &["file:**/rock.png.subtype=diffuse"],
        &[],
    );
    assert_eq!(
        (o.summary.converted, o.summary.exit_code()),
        (5, 0),
        "{:?}",
        warnings(&o)
    );
    assert_profile(&o, "iron_m.png", "complex_material", "data", true, "bc7");
    assert_profile(&o, "lamp_g.png", "glow", "remove", false, "bc7");
    assert_profile(&o, "dirt01.png", "landscape_diffuse", "data", false, "bc7");
    for rel in [
        "textures/a/iron_m.dds",
        "textures/rock/rock.dds",
        "textures/b/lamp_g.dds",
        "textures/c/wall_n.dds",
        "textures/landscape/dirt01.dds",
    ] {
        assert_dds(
            &dds(d.path(), rel),
            DdsPixelFormat::Bc7 { srgb: false },
            HeaderKind::Dx10,
            7,
        );
    }
    assert!(!warnings(&o).iter().any(|c| c.as_str().starts_with("TAG-")));
    // An explicit `_SRGB` tag under cpm fails the file (TAG-SRGB-VANILLA is an error, not allowable).
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/rock/rock.png", rgb(64));
    let o = convert(
        d.path(),
        Target::Cpm,
        &[
            "file:**/rock.png.subtype=diffuse",
            "file:**/rock.png.color.tag=srgb",
        ],
        &[Code::TagSrgbVanilla],
    );
    assert_eq!(
        (o.summary.converted, o.summary.failed, o.summary.exit_code()),
        (0, 1, 1),
        "{:?}",
        warnings(&o)
    );
    assert_eq!(
        warning_of(&o, "rock.png", Code::TagSrgbVanilla).severity,
        Severity::Error
    );
    assert!(!d.path().join("out/textures/rock/rock.dds").exists());
}

/// Spec §4.2/§4.3 item 3: under vanilla `_g` is `glow` (BC7 UNORM, even in a `pbr/` path) and `_m` is always
/// `envmask`; a varying alpha on it is LOSS-ALPHA-DATA (blocking) until acknowledged, then BC4.
#[test]
fn vanilla_g_is_glow_and_m_with_alpha_is_envmask_with_loss() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/pbr/lamp_g.png", rgb(64));
    let o = convert(d.path(), Target::Vanilla, &[], &[]);
    assert_eq!(
        (o.summary.converted, o.summary.exit_code()),
        (1, 0),
        "{:?}",
        warnings(&o)
    );
    assert_eq!(warnings(&o), vec![], "a clean fixture raises nothing");
    assert_profile(&o, "lamp_g.png", "glow", "remove", false, "bc7");
    assert_dds(
        &dds(d.path(), "textures/pbr/lamp_g.dds"),
        DdsPixelFormat::Bc7 { srgb: false },
        HeaderKind::Dx10,
        7,
    );

    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "textures/a/iron_m.png",
        rgba(64, |x, _| [90, 90, 90, (x * 4) as u8]),
    );
    let o = convert(d.path(), Target::Vanilla, &[], &[]);
    assert_eq!(
        (
            o.summary.converted,
            o.summary.skipped,
            o.summary.exit_code()
        ),
        (0, 1, 1),
        "{:?}",
        warnings(&o)
    );
    assert_profile(&o, "iron_m.png", "envmask", "remove", true, "bc4");
    assert_eq!(warnings(&o), vec![Code::LossAlphaData]);
    assert_eq!(skip_code(&o, "iron_m.png"), Some(Code::LossAlphaData));
    assert!(!d.path().join("out/textures/a/iron_m.dds").exists());
    let o = convert(d.path(), Target::Vanilla, &[], &[Code::LossAlphaData]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    assert_dds(
        &dds(d.path(), "textures/a/iron_m.dds"),
        DdsPixelFormat::Bc4,
        HeaderKind::Dx10,
        7,
    );
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
    assert_profile(&o, "iron_n.png", "normal", "data", false, "bc7");
    let f = dds(d.path(), "textures/a/iron_n.dds");
    assert_dds(&f, DdsPixelFormat::Bc7 { srgb: false }, HeaderKind::Dx10, 7);
    // The specular alpha survives texel by texel (0..=252 across the image).
    assert_alpha_kept(&f, |x, _| (x * 4) as u8, 8);
}

#[test]
fn landscape_diffuse_alpha_is_data() {
    // CS/CPM store height in the alpha, vanilla a snow mask (spec §4.2): data either way.
    for target in [Target::Cs, Target::Vanilla] {
        let d = tempfile::tempdir().unwrap();
        put(
            d.path(),
            "textures/landscape/dirt01.png",
            rgba(64, |x, y| [100, 80, 60, ((x + y) * 2) as u8]),
        );
        let o = convert(d.path(), target, &[], &[]);
        assert_eq!(o.summary.converted, 1, "{target:?}: {:?}", warnings(&o));
        assert_profile(&o, "dirt01.png", "landscape_diffuse", "data", false, "bc7");
        let f = dds(d.path(), "textures/landscape/dirt01.dds");
        assert_dds(&f, DdsPixelFormat::Bc7 { srgb: false }, HeaderKind::Dx10, 7);
        assert_alpha_kept(&f, |x, y| ((x + y) * 2) as u8, 8);
    }
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
    assert_profile(&o, "iron_n.png", "pbr_normal", "remove", false, "bc7");
    assert_eq!(planned(&o, "wall_n.png")["profile"]["subtype"], "normal");
    assert_eq!(
        planned(&o, "wall_n.png")["profile"]["alpha"]["exact_opaque"],
        false
    );
    assert_eq!(planned(&o, "h_flow.png")["profile"]["subtype"], "flow");
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
    assert_profile(&o, "iron_m.png", "complex_material", "data", true, "bc7");
    assert_profile(&o, "rock_m.png", "envmask", "remove", true, "bc4");
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
    assert_profile(&o, "iron_m.png", "complex_material", "data", true, "bc7");
    assert_profile(&o, "rock_m.png", "envmask", "remove", true, "bc7");
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
    // The guard bites on both paths (alpha kept as data, and alpha removed then synthesised): with the flag off
    // the same image decodes alpha 254 somewhere.
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/iron_m.png", dark_ramp(64, 255));
    put(d.path(), "textures/b/rock_m.png", dark_ramp(64, 255));
    let mut off = sets.to_vec();
    off.push("file:**/iron_m.png.alpha.exact_opaque=false");
    off.push("file:**/rock_m.png.alpha.exact_opaque=false");
    let o = convert(d.path(), Target::Cs, &off, &[]);
    assert_eq!(o.summary.converted, 2, "{:?}", warnings(&o));
    for (src, rel) in [
        ("iron_m.png", "textures/a/iron_m.dds"),
        ("rock_m.png", "textures/b/rock_m.dds"),
    ] {
        assert_eq!(planned(&o, src)["profile"]["alpha"]["exact_opaque"], false);
        warning_of(&o, src, Code::AlphaExactOffM);
        let lv = levels(&dds(d.path(), rel));
        assert!(
            lv.iter().flatten().any(|p| p[3] != 255),
            "{rel}: exact_opaque off should not be exact"
        );
    }
}

/// Alpha with ~16 % of its mass above the 127 threshold (1 - (127/255)^(1/4)) and ~30 % above 60, in
/// fractal-like noise so box-filtered mips drift.
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

/// Share of texels whose alpha is above `t` (comparison `gt`, the default).
fn coverage(px: &[[u8; 4]], t: u8) -> f64 {
    px.iter().filter(|p| p[3] > t).count() as f64 / px.len() as f64
}

fn alpha_share(alpha: &[u8], t: u8) -> f64 {
    alpha.iter().filter(|&&a| a > t).count() as f64 / alpha.len() as f64
}

fn put_leaf(dir: &Path, alpha: &[u8]) {
    put(
        dir,
        "textures/plants/leaf.png",
        rgba(64, |x, y| [40, 160, 40, alpha[(y * 64 + x) as usize]]),
    );
}

/// Coverage held on the levels with at least 256 texels (64, 32 and 16 px), both as reported and decoded.
fn assert_coverage_kept(o: &Out, dir: &Path, base: f64, t: u8) {
    let leaf = validation_of(o, "leaf.png");
    assert_eq!(leaf.coverage.len(), 7, "{:?}", leaf.coverage);
    for (i, c) in leaf.coverage.iter().take(3).enumerate() {
        assert!(
            (c - base).abs() * 100.0 <= 2.0,
            "mip {i}: coverage {c} vs source {base} at threshold {t}"
        );
    }
    let decoded = levels(&dds(dir, "textures/plants/leaf.dds"));
    for (i, l) in decoded.iter().take(3).enumerate() {
        assert!(
            (coverage(l, t) - base).abs() * 100.0 <= 2.5,
            "decoded mip {i} coverage {} vs source {base} at threshold {t}",
            coverage(l, t)
        );
    }
}

#[test]
fn cutout_keeps_coverage_and_blend_has_none() {
    let d = tempfile::tempdir().unwrap();
    let alpha = sparse_alpha();
    let base = alpha_share(&alpha, 127);
    put_leaf(d.path(), &alpha);
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
    assert_profile(&o, "leaf.png", "diffuse", "cutout", false, "bc7");
    assert_profile(&o, "blood.png", "diffuse", "blend", false, "bc7");
    for rel in ["textures/plants/leaf.dds", "textures/decals/blood.dds"] {
        assert_dds(
            &dds(d.path(), rel),
            DdsPixelFormat::Bc7 { srgb: false },
            HeaderKind::Dx10,
            7,
        );
    }
    // Cutout: the coverage is measured on every level, and held within the default 2-point tolerance on the
    // levels with at least 256 texels (smaller ones are too coarse to check).
    assert_coverage_kept(&o, d.path(), base, 127);
    // The guard bites: with coverage preservation switched off the box-filtered mips drift past the tolerance.
    let d2 = tempfile::tempdir().unwrap();
    put_leaf(d2.path(), &alpha);
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
        .map(|l| (coverage(l, 127) - base).abs() * 100.0)
        .fold(0.0f64, f64::max);
    assert!(drift > 2.0, "coverage off should drift, got {drift} points");
    // Blend: the alpha is kept but coverage preservation is off by default, so it is not measured.
    assert!(validation_of(&o, "blood.png").coverage.is_empty());
}

/// DECISIONS Note E: coverage preservation is on by default for cutout_blend too.
#[test]
fn cutout_blend_keeps_coverage_by_default() {
    let d = tempfile::tempdir().unwrap();
    let alpha = sparse_alpha();
    put_leaf(d.path(), &alpha);
    let o = convert(
        d.path(),
        Target::Cs,
        &[
            "file:**/leaf.png.subtype=diffuse",
            "file:**/leaf.png.alpha.policy=cutout_blend",
        ],
        &[],
    );
    assert_eq!(
        (o.summary.converted, o.summary.validation_failed),
        (1, 0),
        "{:?}",
        warnings(&o)
    );
    assert!(!warnings(&o).contains(&Code::ValCoverage));
    assert_profile(&o, "leaf.png", "diffuse", "cutout_blend", false, "bc7");
    assert_dds(
        &dds(d.path(), "textures/plants/leaf.dds"),
        DdsPixelFormat::Bc7 { srgb: false },
        HeaderKind::Dx10,
        7,
    );
    assert_coverage_kept(&o, d.path(), alpha_share(&alpha, 127), 127);
}

/// Spec §3.2/§3.3 [Requirement]: the alpha-test threshold is set per texture, and coverage is preserved at it.
#[test]
fn per_texture_threshold_preserves_coverage_at_that_threshold() {
    let d = tempfile::tempdir().unwrap();
    let alpha = sparse_alpha();
    let (base60, base127) = (alpha_share(&alpha, 60), alpha_share(&alpha, 127));
    assert!(
        (base60 - base127) * 100.0 > 10.0,
        "the fixture must tell 60 from 127: {base60} vs {base127}"
    );
    put_leaf(d.path(), &alpha);
    let o = convert(
        d.path(),
        Target::Cs,
        &[
            "file:**/leaf.png.subtype=diffuse",
            "file:**/leaf.png.alpha.policy=cutout",
            "file:**/leaf.png.alpha.threshold=60",
        ],
        &[],
    );
    assert_eq!(
        (o.summary.converted, o.summary.validation_failed),
        (1, 0),
        "{:?}",
        warnings(&o)
    );
    assert!(!warnings(&o).contains(&Code::ValCoverage));
    assert_eq!(planned(&o, "leaf.png")["profile"]["alpha"]["threshold"], 60);
    assert_coverage_kept(&o, d.path(), base60, 60);
}

/// Spec §4.1/§4.2 (DECISIONS Note B): `_rfaos` is a sibling of `_rmaos`: BC7 UNORM, linear, alpha = specular data.
#[test]
fn rfaos_is_bc7_unorm_with_data_alpha() {
    let spec = |_: u32, y: u32| (64 + y * 2) as u8;
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "textures/pbr/skin_rfaos.png",
        rgba(64, |x, y| [(x * 2) as u8, 128, 200, spec(x, y)]),
    );
    put(d.path(), "textures/pbr/skin_n.png", flat_normal(64));
    let o = convert(d.path(), Target::Cs, &[], &[]);
    assert_eq!(
        (o.summary.converted, o.summary.exit_code()),
        (2, 0),
        "{:?}",
        warnings(&o)
    );
    assert_profile(&o, "skin_rfaos.png", "rfaos", "data", false, "bc7");
    assert_eq!(
        planned(&o, "skin_rfaos.png")["profile"]["color"]["tag"],
        "unorm"
    );
    assert_profile(&o, "skin_n.png", "pbr_normal", "remove", false, "bc7");
    let f = dds(d.path(), "textures/pbr/skin_rfaos.dds");
    assert_dds(&f, DdsPixelFormat::Bc7 { srgb: false }, HeaderKind::Dx10, 7);
    assert_alpha_kept(&f, spec, 8);
    assert_dds(
        &dds(d.path(), "textures/pbr/skin_n.dds"),
        DdsPixelFormat::Bc7 { srgb: false },
        HeaderKind::Dx10,
        7,
    );
}

/// Spec §11.1 2:1 sets: a 128 px `_n` with a 64 px roughness member. 2:1 is a power-of-two ratio, so
/// SET-BAKE-RATIO (§10: ratio not a power of two) must not fire; the MVP renormalises (bake in M4).
#[test]
fn two_to_one_sets_keep_their_own_chains() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/pbr/iron_n.png", flat_normal(128));
    put(
        d.path(),
        "textures/pbr/iron_rmaos.png",
        rgba(64, |x, y| [(x * 2) as u8, 0, 200, (64 + y * 2) as u8]),
    );
    put(d.path(), "textures/pbr/skin/skin_n.png", flat_normal(128));
    put(
        d.path(),
        "textures/pbr/skin/skin_rfaos.png",
        rgba(64, |x, y| [(x * 2) as u8, 128, 200, (64 + y * 2) as u8]),
    );
    let o = convert(d.path(), Target::Cs, &[], &[]);
    assert_eq!(
        (o.summary.converted, o.summary.exit_code()),
        (4, 0),
        "{:?}",
        warnings(&o)
    );
    assert!(
        !warnings(&o).contains(&Code::SetBakeRatio),
        "{:?}",
        warnings(&o)
    );
    for (n, member, st) in [
        (
            "textures/pbr/iron_n.dds",
            "textures/pbr/iron_rmaos.dds",
            "rmaos",
        ),
        (
            "textures/pbr/skin/skin_n.dds",
            "textures/pbr/skin/skin_rfaos.dds",
            "rfaos",
        ),
    ] {
        let f = dds(d.path(), n);
        assert_eq!((f.width, f.height), (128, 128));
        assert_dds(&f, DdsPixelFormat::Bc7 { srgb: false }, HeaderKind::Dx10, 8);
        let m = dds(d.path(), member);
        assert_eq!((m.width, m.height), (64, 64), "{st}");
        assert_dds(&m, DdsPixelFormat::Bc7 { srgb: false }, HeaderKind::Dx10, 7);
    }
    assert_profile(&o, "iron_n.png", "pbr_normal", "remove", false, "bc7");
    assert_profile(&o, "iron_rmaos.png", "rmaos", "data", false, "bc7");
    assert_profile(&o, "skin_rfaos.png", "rfaos", "data", false, "bc7");
}

#[test]
fn bc6h_emissive_in_a_pbr_set() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/pbr/lamp_g.png", rgb(64));
    let o = convert(d.path(), Target::Cs, &[], &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    assert_eq!(warnings(&o), vec![], "a clean fixture raises nothing");
    assert_profile(
        &o,
        "lamp_g.png",
        "pbr_emissive",
        "remove",
        false,
        "bc6h_uf16",
    );
    assert_eq!(planned(&o, "lamp_g.png")["profile"]["color"]["tag"], "n/a");
    assert_dds(
        &dds(d.path(), "textures/pbr/lamp_g.dds"),
        DdsPixelFormat::Bc6hUf16,
        HeaderKind::Dx10,
        7,
    );
}

/// A float HDR emissive (linear radiance up to 4.0). The loader reads float32 TIFF (EXR is not a supported input:
/// `image` is built with png/tga/tiff only).
fn hdr_emissive() -> DynamicImage {
    DynamicImage::ImageRgb32F(ImageBuffer::from_fn(64, 64, |x, y| {
        Rgb([4.0 * x as f32 / 63.0, 2.0 * y as f32 / 63.0, 0.25])
    }))
}

/// Spec §4.1/§4.2/§10: a float `_g` in a PBR set is BC6H UF16 (tag n/a); BC6H is not an 8-bit-endpoint format and
/// keeps values above 1, so neither LOSS-BITDEPTH nor LOSS-CLAMP-HDR applies; the peak holds (no VAL-BC6H-PEAK).
#[test]
fn hdr_float_emissive_is_bc6h() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/pbr/lamp_g.tif", hdr_emissive());
    let o = convert(d.path(), Target::Cs, &[], &[]);
    assert_eq!(
        (o.summary.converted, o.summary.exit_code()),
        (1, 0),
        "{:?}",
        warnings(&o)
    );
    assert_eq!(
        warnings(&o),
        vec![],
        "no LOSS-*, VAL-BC6H-PEAK or other warning"
    );
    assert_profile(
        &o,
        "lamp_g.tif",
        "pbr_emissive",
        "remove",
        false,
        "bc6h_uf16",
    );
    assert_eq!(planned(&o, "lamp_g.tif")["profile"]["color"]["tag"], "n/a");
    assert_dds(
        &dds(d.path(), "textures/pbr/lamp_g.dds"),
        DdsPixelFormat::Bc6hUf16,
        HeaderKind::Dx10,
        7,
    );
    assert!(validation_of(&o, "lamp_g.tif").bc6h_peak_ratio.unwrap() >= 0.8);
}

/// Spec §4.1 `pbr_emissive` content: "sRGB (8/16-bit) or linear (float)"; §4.2: only an 8/16-bit `_g` is
/// decoded from sRGB. A float source is linear radiance and must reach BC6H unchanged (up to BC6H error).
#[test]
fn hdr_float_emissive_content_is_linear() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/pbr/lamp_g.tif", hdr_emissive());
    let o = convert(d.path(), Target::Cs, &[], &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    assert_eq!(
        planned(&o, "lamp_g.tif")["profile"]["color"]["content"],
        "linear"
    );
    let f = dds(d.path(), "textures/pbr/lamp_g.dds");
    let (w, h, r) = f.levels[0].clone();
    let Decoded::Hdr(px) = decode_level(f.format, w, h, &f.data[r], None).unwrap() else {
        panic!("BC6H decodes to float");
    };
    // BC6H error scales with the block's range, not the texel (measured max 3.2 % of the 4.0 peak); the sRGB
    // bug this guards against turns 4.0 into 23.7 and 0.25 into 0.051.
    const PEAK: f32 = 4.0;
    let src = hdr_emissive().to_rgb32f();
    let top = px.iter().flatten().cloned().fold(0.0_f32, f32::max);
    assert!((top - PEAK).abs() <= 0.05 * PEAK, "decoded peak {top}");
    for (i, (p, s)) in px.iter().zip(src.pixels()).enumerate() {
        for (c, (got, want)) in p.iter().zip(s.0).enumerate() {
            assert!(
                (got - want).abs() <= 0.04 * PEAK,
                "texel {i} channel {c}: {got} vs source {want}"
            );
        }
    }
}

/// Spec §3.4/§4.2: `_SRGB` only for PBR colour under cs (albedo, `_g` when not BC6H, `_f`, `_s`); non-PBR colour,
/// normals and landscape are UNORM.
#[test]
fn srgb_matrix_under_cs() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/pbr/silk_f.png", rgb(64));
    put(d.path(), "textures/pbr/silk_s.png", rgb(64));
    put(d.path(), "textures/pbr/silk_n.png", flat_normal(64));
    put(d.path(), "textures/pbr/lamp_g.png", rgb(64));
    put(d.path(), "textures/rock/rock_d.png", rgb(64));
    put(d.path(), "textures/a/wall_n.png", flat_normal(64));
    put(d.path(), "textures/landscape/dirt02.png", rgb(64));
    let o = convert(
        d.path(),
        Target::Cs,
        &[
            "file:**/lamp_g.png.format=bc7",
            "file:**/lamp_g.png.color.tag=srgb",
        ],
        &[],
    );
    assert_eq!(
        (o.summary.converted, o.summary.exit_code()),
        (7, 0),
        "{:?}",
        warnings(&o)
    );
    assert!(
        !warnings(&o).iter().any(|c| c.as_str().starts_with("TAG-")),
        "{:?}",
        warnings(&o)
    );
    for (src, subtype) in [
        ("silk_f.png", "pbr_fuzz"),
        ("silk_s.png", "pbr_subsurface"),
        ("silk_n.png", "pbr_normal"),
        ("lamp_g.png", "pbr_emissive"),
        ("rock_d.png", "diffuse"),
        ("wall_n.png", "normal"),
        ("dirt02.png", "landscape_diffuse"),
    ] {
        assert_eq!(planned(&o, src)["profile"]["subtype"], subtype, "{src}");
        assert_eq!(
            planned(&o, src)["profile"]["alpha"]["exact_opaque"],
            false,
            "{src}"
        );
    }
    for (rel, srgb) in [
        ("textures/pbr/silk_f.dds", true),
        ("textures/pbr/silk_s.dds", true),
        ("textures/pbr/lamp_g.dds", true),
        ("textures/pbr/silk_n.dds", false),
        ("textures/rock/rock_d.dds", false),
        ("textures/a/wall_n.dds", false),
        ("textures/landscape/dirt02.dds", false),
    ] {
        assert_dds(
            &dds(d.path(), rel),
            DdsPixelFormat::Bc7 { srgb },
            HeaderKind::Dx10,
            7,
        );
    }
}

/// Spec §4.2: `_SRGB` under a `lod/` path raises TAG-SRGB-LOD (blocking) in every target.
#[test]
fn srgb_under_lod_blocks_until_allowed() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/pbr/lod/iron_d.png", rgb(64));
    let o = convert(d.path(), Target::Cs, &[], &[]);
    assert_eq!(
        (
            o.summary.converted,
            o.summary.skipped,
            o.summary.exit_code()
        ),
        (0, 1, 1),
        "{:?}",
        warnings(&o)
    );
    assert_eq!(
        planned(&o, "iron_d.png")["profile"]["subtype"],
        "pbr_albedo"
    );
    assert_eq!(warnings(&o), vec![Code::TagSrgbLod]);
    assert_eq!(skip_code(&o, "iron_d.png"), Some(Code::TagSrgbLod));
    assert!(!d.path().join("out/textures/pbr/lod/iron_d.dds").exists());
    let o = convert(d.path(), Target::Cs, &[], &[Code::TagSrgbLod]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    assert_dds(
        &dds(d.path(), "textures/pbr/lod/iron_d.dds"),
        DdsPixelFormat::Bc7 { srgb: true },
        HeaderKind::Dx10,
        7,
    );
}

/// Spec §3.2/§4.2: `interface/` and `sky/` have no mips; `ui_sky` is BC7 UNORM.
#[test]
fn sky_and_interface_have_no_mips() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/sky/grad.png", rgb(64));
    // `_d` declares the colour role (high confidence), so the suggester's ui_sky is what runs.
    put(d.path(), "textures/interface/icon_d.png", rgb(64));
    let o = convert(
        d.path(),
        Target::Cs,
        &["file:**/grad.png.subtype=diffuse"],
        &[],
    );
    assert_eq!(
        (o.summary.converted, o.summary.exit_code()),
        (2, 0),
        "{:?}",
        warnings(&o)
    );
    assert_eq!(
        planned(&o, "icon_d.png")["provenance"]["subtype"],
        "suggestion"
    );
    assert_profile(&o, "icon_d.png", "ui_sky", "remove", false, "bc7");
    for rel in ["textures/sky/grad.dds", "textures/interface/icon_d.dds"] {
        assert_dds(
            &dds(d.path(), rel),
            DdsPixelFormat::Bc7 { srgb: false },
            HeaderKind::Dx10,
            1,
        );
    }
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
    assert_profile(&o, "wall.png", "diffuse", "remove", false, "bc1");
    let f = dds(d.path(), "textures/a/wall.dds");
    assert_dds(
        &f,
        DdsPixelFormat::Bc1 { srgb: false },
        HeaderKind::Legacy,
        7,
    );
    assert_eq!(&f.fourcc, b"DXT1");
    // DECISIONS Note F: BC1 is opaque only and never emits transparent-black texels.
    let lv = levels(&f);
    assert_eq!(lv.len(), 7);
    for (i, l) in lv.iter().enumerate() {
        assert!(
            l.iter().all(|p| p[3] == 255),
            "BC1 mip {i}: alpha below 255"
        );
    }
    let p = dds(d.path(), "textures/a/wall_p.dds");
    assert_dds(&p, DdsPixelFormat::Bc4, HeaderKind::Legacy, 7);
    assert_eq!(&p.fourcc, b"BC4U");
}

/// Spec §3.2 `header`: legacy is the default for BC1/BC3 UNORM, so BC3 is written with the `DXT5` FourCC.
#[test]
fn bc3_is_legacy_dxt5() {
    let d = tempfile::tempdir().unwrap();
    let a = |x: u32, _: u32| (x * 4) as u8;
    put(
        d.path(),
        "textures/decals/blood.png",
        rgba(64, |x, y| [150, 0, 0, a(x, y)]),
    );
    let o = convert(
        d.path(),
        Target::Cs,
        &[
            "file:**/blood.png.subtype=diffuse",
            "file:**/blood.png.alpha.policy=blend",
            "file:**/blood.png.format=bc3",
            "file:**/blood.png.encoder.backend=rgbcx",
        ],
        &[],
    );
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    assert_profile(&o, "blood.png", "diffuse", "blend", false, "bc3");
    let f = dds(d.path(), "textures/decals/blood.dds");
    assert_dds(
        &f,
        DdsPixelFormat::Bc3 { srgb: false },
        HeaderKind::Legacy,
        7,
    );
    assert_eq!(&f.fourcc, b"DXT5");
    assert_alpha_kept(&f, a, 8);
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
    // exact_opaque is off for data maps (DECISIONS: "Only _m / env masks").
    assert_profile(&o, "iron_rmaos.png", "rmaos", "remove", false, "bc7");
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
    assert_eq!(skip_code(&o, "leaf.png"), Some(Code::SugLowConfidence));
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
    assert_profile(&o, "leaf.png", "diffuse", "blend", false, "bc7");
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
    assert_eq!(skip_code(&o, "stray.dds"), Some(Code::InDdsSkipped));
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
    assert_profile(
        &o,
        "iron_rmaosultrahigh.png",
        "rmaos",
        "remove",
        false,
        "bc7",
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

/// DECISIONS Note L, end to end: a PBR `_n` re-exported with alpha 251..=255 converts with the alpha removed, the
/// ALPHA-NEAR-OPAQUE info and no LOSS-ALPHA-DATA; one texel at 250 blocks the file as varying alpha.
#[test]
fn near_opaque_normal_alpha_is_removed_with_an_info() {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "textures/pbr/iron_n.png",
        rgba(64, |x, _| [128, 128, 255, 251 + (x % 5) as u8]),
    );
    let o = convert(
        d.path(),
        Target::Cs,
        &["file:**/iron_n.png.subtype=pbr_normal"],
        &[],
    );
    assert_eq!(
        (o.summary.converted, o.summary.exit_code()),
        (1, 0),
        "{:?}",
        warnings(&o)
    );
    assert_profile(&o, "iron_n.png", "pbr_normal", "remove", false, "bc7");
    let w = warning_of(&o, "iron_n.png", Code::AlphaNearOpaque);
    assert_eq!(w.severity, Severity::Info);
    assert!(w.message.contains("min 251"), "{}", w.message);
    assert_eq!(warnings(&o), vec![Code::AlphaNearOpaque]);
    assert_dds(
        &dds(d.path(), "textures/pbr/iron_n.dds"),
        DdsPixelFormat::Bc7 { srgb: false },
        HeaderKind::Dx10,
        7,
    );
    // The alpha was dropped: every decoded texel is opaque.
    assert!(levels(&dds(d.path(), "textures/pbr/iron_n.dds"))[0]
        .iter()
        .all(|p| p[3] == 255));

    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "textures/pbr/iron_n.png",
        rgba(64, |x, y| {
            [128, 128, 255, if (x, y) == (0, 0) { 250 } else { 255 }]
        }),
    );
    let o = convert(
        d.path(),
        Target::Cs,
        &["file:**/iron_n.png.subtype=pbr_normal"],
        &[],
    );
    assert_eq!(o.summary.converted, 0, "{:?}", warnings(&o));
    assert_eq!(skip_code(&o, "iron_n.png"), Some(Code::LossAlphaData));
    assert!(!warnings(&o).contains(&Code::AlphaNearOpaque));
}
