//! Golden tests (spec §11.1): each case generates its fixtures, runs the full `Engine::plan` + `Engine::run` and
//! asserts the format, `_SRGB` tag, header form, mip count, alpha-policy outcome and warning codes. Nothing is
//! committed but this file and no byte hashes are asserted (those belong to the `pinned-isa` CI job).
use encoders_sys::Bc1Model;
use engine::api::*;
use engine::codes::{Code, Severity, Warning};
use engine::config::Target;
use engine::dds::{read_dds, DdsFile, DdsPixelFormat, HeaderKind};
use engine::decode::{decode_level, Decoded};
use engine::events::*;
use engine::image::Rgba32F;
use engine::manifest::Manifest;
use engine::overrides::parse_set;
use engine::policy::AcceptSuggestions;
use engine::session::Session;
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
            EventKind::FilePlanned { job } if is_of(e, suffix) => {
                Some(serde_json::to_value(&**job).unwrap())
            }
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

/// A unit-length normal map whose direction changes from texel to texel, so every footprint is short.
fn bumpy_normal(w: u32) -> DynamicImage {
    DynamicImage::ImageRgb8(ImageBuffer::from_fn(w, w, |x, y| {
        let nx = 0.6 * (x as f32 * 1.3 + y as f32 * 0.7).sin();
        let ny = 0.6 * (x as f32 * 0.9 - y as f32 * 1.1).cos();
        let nz = (1.0 - nx * nx - ny * ny).sqrt();
        let e = |v: f32| ((v * 0.5 + 0.5) * 255.0).round() as u8;
        Rgb([e(nx), e(ny), e(nz)])
    }))
}

/// A roughness member: R a ramp, G 0, B 200 (AO), A specular 64 + y.
fn rough_member(w: u32) -> DynamicImage {
    rgba(w, |x, y| {
        [(30 + (x * 3 + y * 2) % 190) as u8, 0, 200, (64 + y) as u8]
    })
}

/// A normal texel as mip 0 writes it with the cs defaults: decoded as spec §5.4 does (no flip, no Z rebuild,
/// z >= 0), then renormalised (`mips.normal_base = renormalize`, DECISIONS R1). The bake averages these.
fn decoded_normal(p: &image::Rgb<u8>) -> [f64; 3] {
    let d = |c: u8| f64::from(c) / 255.0 * 2.0 - 1.0;
    let v = [d(p[0]), d(p[1]), d(p[2]).max(0.0)];
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len > 1e-8 {
        v.map(|c| c / len)
    } else {
        [0.0, 0.0, 1.0]
    }
}

/// Spec §6.2 step 2 in f64 (test code only): `min(2v, t²)` for the mean of `normals`, 0 at or above the noise floor.
fn reference_add(normals: impl Iterator<Item = [f64; 3]>) -> f64 {
    let t = 0.25f64;
    let (mut s, mut k) = ([0.0f64; 3], 0.0f64);
    for n in normals {
        for c in 0..3 {
            s[c] += n[c];
        }
        k += 1.0;
    }
    let l = ((s[0] / k).powi(2) + (s[1] / k).powi(2) + (s[2] / k).powi(2)).sqrt();
    // The 8-bit noise floor (owner 2026-10-11, DECISIONS Note R M4-final-2), restated here, not imported.
    if l >= 0.999 {
        0.0
    } else if l <= 0.0 {
        t * t
    } else {
        (0.5 * (1.0 - l * l) / (3.0 * l - l * l * l)).min(t * t)
    }
}

/// Spec §6.2 in f64 (test code only): the stored R of member texel (x, y) at mip `level`, the `_n` `shift` levels
/// larger (negative: smaller). `shift` far below zero gives the unbaked value.
fn reference_roughness(
    n: &image::RgbImage,
    r: &image::RgbaImage,
    level: u32,
    shift: i32,
    x: u32,
    y: u32,
) -> f64 {
    let f = 1u32 << level;
    let mut alpha = 0.0;
    for j in 0..f {
        for i in 0..f {
            let v = f64::from(r.get_pixel(x * f + i, y * f + j)[0]) / 255.0;
            alpha += v * v;
        }
    }
    alpha /= f64::from(f * f);
    let m = level as i32 + shift;
    let add = if m < 1 {
        0.0
    } else {
        let g = 1u32 << m;
        reference_add((0..g * g).map(|k| decoded_normal(n.get_pixel(x * g + k % g, y * g + k / g))))
    };
    if add == 0.0 {
        alpha.sqrt()
    } else {
        (alpha * alpha + add).min(1.0).powf(0.25)
    }
}

/// The float chain the encoder receives for the planned job whose input ends with `name`, and that job.
fn planned_chain(
    session: &Session,
    req: BatchRequest,
    name: &str,
) -> (engine::policy::ResolvedJob, Vec<Rgba32F>) {
    let rep = Reporter::new(Arc::new(NullSink), "golden");
    let plan = Engine::new()
        .unwrap()
        .plan(req, &rep, &CancelToken::default())
        .unwrap();
    let job = plan
        .jobs
        .iter()
        .find(|j| j.input.ends_with(name))
        .unwrap_or_else(|| panic!("no job for {name}"))
        .clone();
    let chain = session
        .preview_source(&job, 16384, &CancelToken::default())
        .unwrap();
    assert_eq!(chain.first_level, 0);
    (job, chain.levels)
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
    // Spike S6 (owner, 2026-10-09): turning the flag off on a BC7 `_m` is blocking; nothing is written.
    let o = convert(d.path(), Target::Cs, &off, &[]);
    assert_eq!(
        (
            o.summary.converted,
            o.summary.skipped,
            o.summary.exit_code()
        ),
        (0, 2, 1),
        "{:?}",
        warnings(&o)
    );
    for src in ["iron_m.png", "rock_m.png"] {
        assert_eq!(skip_code(&o, src), Some(Code::AlphaExactOffM));
    }
    let o = convert(d.path(), Target::Cs, &off, &[Code::AlphaExactOffM]);
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
    assert_profile(&o, "skin_rfaos.png", "skin_rfaos", "data", false, "bc7");
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

/// Spec §11.1 2:1 sets: a 128 px `_n` with a 64 px roughness member. 2:1 is a power-of-two ratio, so SET-BAKE-RATIO
/// (§10) does not fire and both sets bake (flat normals, so the roughness is unchanged).
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
    for set in ["textures/pbr/iron", "textures/pbr/skin/skin"] {
        assert!(
            o.events.iter().any(
                |e| matches!(&e.kind, EventKind::SetResolved { bake: true, .. })
                    && e.set.as_deref().is_some_and(|s| s.ends_with(set))
            ),
            "{set} bakes"
        );
    }
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
    assert_profile(&o, "skin_rfaos.png", "skin_rfaos", "data", false, "bc7");
}

#[test]
fn an_8_bit_emissive_is_bc7_srgb_not_bc6h() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/pbr/lamp_g.png", rgb(64));
    let o = convert(d.path(), Target::Cs, &[], &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    assert_eq!(warnings(&o), vec![Code::FmtHdrSource]);
    assert_profile(&o, "lamp_g.png", "pbr_emissive", "remove", false, "bc7");
    assert_eq!(planned(&o, "lamp_g.png")["profile"]["color"]["tag"], "srgb");
    assert_dds(
        &dds(d.path(), "textures/pbr/lamp_g.dds"),
        DdsPixelFormat::Bc7 { srgb: true },
        HeaderKind::Dx10,
        7,
    );
}

/// A float HDR emissive (linear radiance up to 4.0). The loader reads float32 TIFF, OpenEXR and Radiance HDR.
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
        vec![Code::FmtHdrSource],
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

/// Mean decoded value of channel `c` over mip 0.
fn mean_code(f: &DdsFile, c: usize) -> f64 {
    let l = &levels(f)[0];
    l.iter().map(|p| p[c] as f64).sum::<f64>() / l.len() as f64
}

/// DECISIONS S1: a float source is linear light. To BC7 under cs it converts without --allow; values above 1 are
/// clamped (LOSS-CLAMP-HDR, a warning) and every value is sRGB-encoded: a constant linear 0.25 blue decodes near
/// code 137 (sRGB 0.537), not 64 (the stored value).
#[test]
fn float_source_to_bc7_is_linear_light_encoded_as_srgb() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/lamp.tif", hdr_emissive());
    let o = convert(d.path(), Target::Cs, &["subtype=diffuse"], &[]);
    assert_eq!(
        (o.summary.converted, o.summary.exit_code()),
        (1, 0),
        "{:?}",
        warnings(&o)
    );
    assert!(!warnings(&o).contains(&Code::InFloatLdr));
    assert_eq!(
        warning_of(&o, "lamp.tif", Code::LossClampHdr).severity,
        Severity::Warning
    );
    assert_profile(&o, "lamp.tif", "diffuse", "remove", false, "bc7");
    let b = mean_code(&dds(d.path(), "textures/a/lamp.dds"), 2);
    assert!((b - 137.0).abs() <= 3.0, "blue {b}");
}

/// A float source inside [0, 1] has nothing to clamp: no LOSS-CLAMP-HDR, and linear 0.5 decodes near 188.
#[test]
fn in_range_float_source_has_no_clamp_warning() {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "textures/a/plate.tif",
        DynamicImage::ImageRgb32F(ImageBuffer::from_fn(64, 64, |x, y| {
            Rgb([x as f32 / 63.0, y as f32 / 63.0, 0.5])
        })),
    );
    let o = convert(d.path(), Target::Cs, &["subtype=diffuse"], &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    assert!(!warnings(&o).contains(&Code::LossClampHdr));
    assert!(!warnings(&o).contains(&Code::InFloatLdr));
    let b = mean_code(&dds(d.path(), "textures/a/plate.dds"), 2);
    assert!((b - 188.0).abs() <= 3.0, "blue {b}");
}

/// Review Focus 3: under vanilla (no BC6H) a float diffuse is still sRGB-encoded, into a UNORM file (content srgb,
/// tag unorm), not written as stored values.
#[test]
fn a_float_source_under_vanilla_is_srgb_encoded_into_unorm() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/lamp.tif", hdr_emissive());
    let o = convert(d.path(), Target::Vanilla, &["subtype=diffuse"], &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    let f = dds(d.path(), "textures/a/lamp.dds");
    assert_eq!(f.format, DdsPixelFormat::Bc7 { srgb: false });
    assert!((mean_code(&f, 2) - 137.0).abs() <= 3.0);
}

/// Review Focus 3: a float data map (content linear) is written as stored values.
#[test]
fn a_float_data_map_is_written_as_stored_values() {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "textures/a/mask.tif",
        DynamicImage::ImageRgb32F(ImageBuffer::from_fn(64, 64, |_, _| Rgb([0.25, 0.5, 0.75]))),
    );
    let o = convert(d.path(), Target::Cs, &["subtype=generic_data"], &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    let f = dds(d.path(), "textures/a/mask.dds");
    for (c, want) in [(0, 64.0), (1, 128.0), (2, 191.0)] {
        assert!(
            (mean_code(&f, c) - want).abs() <= 2.0,
            "channel {c}: {}",
            mean_code(&f, c)
        );
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
/// ALPHA-NEAR-OPAQUE info and no LOSS-ALPHA-DATA; one texel at 250 is genuinely varying alpha, dropped with ALPHA-PBR-NORMAL-DROPPED (P25).
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
    assert_eq!(
        warnings(&o),
        vec![Code::AlphaNearOpaque, Code::SetMissingCompanion]
    );
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
    // Owner 2026-10-11 (DECISIONS P25): a PBR normal's genuinely varying alpha is dropped with an info, no block.
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    assert_eq!(
        warnings(&o),
        vec![Code::AlphaPbrNormalDropped, Code::SetMissingCompanion]
    );
    assert!(!warnings(&o).contains(&Code::AlphaNearOpaque));
}
/// Roadmap M4 exit: `_n` + `_rmaos` and `_n` + `_rfaos` at 1:1 and 2:1 (and a half-size `_n`). Every texel of every
/// level of the roughness chain the encoder receives is within 1/255 of the f64 reference, and the bake widens it.
#[test]
fn variance_bake_matches_the_f64_reference() {
    let session = Session::new(Bc1Model::Ideal).unwrap();
    for (nw, mw, member, shift) in [
        (64u32, 64u32, "rmaos", 0i32),
        (128, 64, "rmaos", 1),
        (32, 64, "rmaos", -1),
        // File suffixes: `_rfaos` resolves to the sub-type `skin_rfaos` after the packing-fills plan.
        (64, 64, "rfaos", 0),
        (128, 64, "rfaos", 1),
    ] {
        let d = tempfile::tempdir().unwrap();
        put(d.path(), "textures/pbr/iron_n.png", bumpy_normal(nw));
        let name = format!("iron_{member}.png");
        put(d.path(), &format!("textures/pbr/{name}"), rough_member(mw));
        let (job, levels) = planned_chain(&session, request(d.path(), Target::Cs, &[], &[]), &name);
        assert_eq!(
            job.bake.as_ref().map(|b| b.shift),
            Some(shift),
            "{nw}:{mw} {member}: {:?}",
            job.warnings
        );
        let (n, r) = (bumpy_normal(nw).to_rgb8(), rough_member(mw).to_rgba8());
        let mut widened = 0;
        for (level, l) in levels.iter().enumerate() {
            for y in 0..l.height {
                for x in 0..l.width {
                    let want = reference_roughness(&n, &r, level as u32, shift, x, y);
                    let got = f64::from(l.at(x, y)[0]);
                    assert!(
                        (got - want).abs() <= 1.0 / 255.0,
                        "{nw}:{mw} {member} mip {level} ({x},{y}): {got} vs {want}"
                    );
                    let plain = reference_roughness(&n, &r, level as u32, -100, x, y);
                    if want > plain + 1.0 / 255.0 {
                        widened += 1;
                    }
                }
            }
        }
        assert!(widened > 0, "{nw}:{mw} {member}: nothing widened");
        // And it converts: both outputs written, the set reports the bake.
        let o = convert(d.path(), Target::Cs, &[], &[]);
        assert_eq!(
            (o.summary.converted, o.summary.exit_code()),
            (2, 0),
            "{:?}",
            warnings(&o)
        );
        assert!(o
            .events
            .iter()
            .any(|e| matches!(&e.kind, EventKind::SetResolved { bake: true, .. })));
        assert_dds(
            &dds(d.path(), &format!("textures/pbr/iron_{member}.dds")),
            DdsPixelFormat::Bc7 { srgb: false },
            HeaderKind::Dx10,
            7,
        );
    }
}

/// Renormalise-only paths are unchanged: a flat `_n` adds no variance, so the roughness chain is bit-identical to the
/// same job without its bake.
#[test]
fn a_flat_normal_leaves_the_roughness_chain_bit_identical() {
    let session = Session::new(Bc1Model::Ideal).unwrap();
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/pbr/iron_n.png", flat_normal(64));
    put(d.path(), "textures/pbr/iron_rmaos.png", rough_member(64));
    let (job, baked) = planned_chain(
        &session,
        request(d.path(), Target::Cs, &[], &[]),
        "iron_rmaos.png",
    );
    assert!(job.bake.is_some());
    let mut plain = job.clone();
    plain.bake = None;
    let unbaked = session
        .preview_source(&plain, 16384, &CancelToken::default())
        .unwrap()
        .levels;
    assert_eq!(baked, unbaked);
}

/// Spec §6.3: a non-PBR `_n` never bakes; `renormalize_bake` set on it changes nothing.
#[test]
fn renormalize_bake_on_a_non_pbr_normal_is_inert() {
    let session = Session::new(Bc1Model::Ideal).unwrap();
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/wall_n.png", bumpy_normal(64));
    let (_, plain) = planned_chain(
        &session,
        request(d.path(), Target::Cs, &[], &[]),
        "wall_n.png",
    );
    let (job, set) = planned_chain(
        &session,
        request(
            d.path(),
            Target::Cs,
            &["file:**/wall_n.png.mips.normal=renormalize_bake"],
            &[],
        ),
        "wall_n.png",
    );
    assert_eq!(job.profile.subtype.id(), "normal");
    assert_eq!(plain, set);
}

/// Owner 2026-10-11 (DECISIONS R2), roadmap exit: a `_cnr` widens its own coat roughness (alpha) by the formula, 1:1;
/// a flat `_cnr` keeps its alpha bit for bit, as with the self-bake off.
#[test]
fn cnr_self_bake_matches_the_reference_and_a_flat_coat_keeps_its_alpha() {
    let session = Session::new(Bc1Model::Ideal).unwrap();
    let coat_alpha = |x: u32, y: u32| (40 + (x * 3 + y) % 200) as u8;
    // Bumpy.
    let d = tempfile::tempdir().unwrap();
    let bumpy = bumpy_normal(64).to_rgb8();
    put(
        d.path(),
        "textures/pbr/silk_cnr.png",
        rgba(64, |x, y| {
            let p = bumpy.get_pixel(x, y);
            [p[0], p[1], p[2], coat_alpha(x, y)]
        }),
    );
    let (job, levels) = planned_chain(
        &session,
        request(d.path(), Target::Cs, &[], &[]),
        "silk_cnr.png",
    );
    assert_eq!(job.profile.subtype.id(), "coat_normal");
    let mut widened = 0;
    for (level, l) in levels.iter().enumerate() {
        let f = 1u32 << level;
        for y in 0..l.height {
            for x in 0..l.width {
                let mut r = 0.0;
                for j in 0..f {
                    for i in 0..f {
                        r += f64::from(coat_alpha(x * f + i, y * f + j)) / 255.0;
                    }
                }
                r /= f64::from(f * f);
                let add = if level == 0 {
                    0.0
                } else {
                    reference_add(
                        (0..f * f)
                            .map(|k| decoded_normal(bumpy.get_pixel(x * f + k % f, y * f + k / f))),
                    )
                };
                let want = if add == 0.0 {
                    r
                } else {
                    (r.powi(4) + add).min(1.0).powf(0.25)
                };
                let got = f64::from(l.at(x, y)[3]);
                assert!(
                    (got - want).abs() <= 1.0 / 255.0,
                    "mip {level} ({x},{y}): {got} vs {want}"
                );
                if want > r + 1.0 / 255.0 {
                    widened += 1;
                }
            }
        }
    }
    assert!(widened > 0);
    // Flat: the same alpha as with the self-bake off.
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "textures/pbr/silk_cnr.png",
        rgba(64, |x, y| [128, 128, 255, coat_alpha(x, y)]),
    );
    let (_, on) = planned_chain(
        &session,
        request(d.path(), Target::Cs, &[], &[]),
        "silk_cnr.png",
    );
    let (_, off) = planned_chain(
        &session,
        request(
            d.path(),
            Target::Cs,
            &["file:**/silk_cnr.png.mips.normal=renormalize"],
            &[],
        ),
        "silk_cnr.png",
    );
    for (a, b) in on.iter().zip(&off) {
        assert!(
            a.px.iter().zip(&b.px).all(|(p, q)| p[3] == q[3]),
            "{}x{}",
            a.width,
            a.height
        );
    }
}

/// A visually flat `_n` with 8-bit noise of up to ±2 codes on X and Y (deterministic), the case that lifted
/// near-mirror roughness to about 0.04–0.05 before the noise floor (final review Minor 9).
fn noisy_flat_normal(w: u32) -> image::RgbImage {
    let n = |x: u32, y: u32, salt: u32| {
        let h = (x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663) ^ salt)
            .wrapping_mul(2_654_435_761);
        ((h >> 13) % 5) as i32 - 2
    };
    ImageBuffer::from_fn(w, w, |x, y| {
        Rgb([
            (128 + n(x, y, 0x9e37)) as u8,
            (128 + n(x, y, 0x7f4a)) as u8,
            255,
        ])
    })
}

/// Columns alternating between X codes `128 + k` and `128 − k`: every footprint of mip ≥ 1 has the same length.
fn tilted_columns(w: u32, k: u8) -> image::RgbImage {
    ImageBuffer::from_fn(w, w, |x, _| {
        Rgb([if x % 2 == 0 { 128 + k } else { 128 - k }, 128, 255])
    })
}

/// The f64 footprint length (spec §6.2 step 1) of mip `level`'s texel (x, y) of `n`, test code only.
fn reference_length(n: &image::RgbImage, level: u32, x: u32, y: u32) -> f64 {
    let f = 1u32 << level;
    let mut s = [0.0f64; 3];
    for k in 0..f * f {
        let v = decoded_normal(n.get_pixel(x * f + k % f, y * f + k / f));
        for c in 0..3 {
            s[c] += v[c] / f64::from(f * f);
        }
    }
    (s[0] * s[0] + s[1] * s[1] + s[2] * s[2]).sqrt()
}

/// Owner 2026-10-11 (DECISIONS Note R M4-final-2): a flat-but-noisy `_n` adds no variance at any mip, so a mirror
/// roughness (R = 0) stays 0 and a near-mirror `_cnr` coat keeps its alpha, as with the bake off.
#[test]
fn a_flat_but_noisy_normal_keeps_mirror_roughness_at_every_mip() {
    let session = Session::new(Bc1Model::Ideal).unwrap();
    let noisy = noisy_flat_normal(64);
    // Measurement: every footprint is above the floor, and without it this fixture is the reported lift.
    let mut min_l = 1.0f64;
    for level in 1..=6u32 {
        let w = 64 >> level;
        for y in 0..w {
            for x in 0..w {
                min_l = min_l.min(reference_length(&noisy, level, x, y));
            }
        }
    }
    assert!(min_l >= 0.999, "min footprint length {min_l}");
    let old_lift = (0.5 * (1.0 - min_l * min_l) / (3.0 * min_l - min_l.powi(3))).powf(0.25);
    assert!(
        old_lift > 0.03,
        "the fixture reproduces the lift: {old_lift}"
    );

    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "textures/pbr/iron_n.png",
        DynamicImage::ImageRgb8(noisy.clone()),
    );
    put(
        d.path(),
        "textures/pbr/iron_rmaos.png",
        rgba(64, |_, _| [0, 0, 200, 255]),
    );
    let (job, levels) = planned_chain(
        &session,
        request(d.path(), Target::Cs, &[], &[]),
        "iron_rmaos.png",
    );
    assert!(job.bake.is_some(), "{:?}", job.warnings);
    for (level, l) in levels.iter().enumerate() {
        assert!(
            l.px.iter().all(|p| p[0] == 0.0),
            "mip {level}: roughness lifted to {:?}",
            l.px.iter().map(|p| p[0]).fold(0.0f32, f32::max)
        );
    }

    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "textures/pbr/silk_cnr.png",
        rgba(64, |x, y| {
            let p = noisy.get_pixel(x, y);
            [p[0], p[1], p[2], 2]
        }),
    );
    let chain = |sets: &[&str]| {
        planned_chain(
            &session,
            request(d.path(), Target::Cs, sets, &[]),
            "silk_cnr.png",
        )
        .1
    };
    let (on, off) = (
        chain(&[]),
        chain(&["file:**/silk_cnr.png.mips.normal=renormalize"]),
    );
    for (a, b) in on.iter().zip(&off) {
        assert!(
            a.px.iter().zip(&b.px).all(|(p, q)| p[3] == q[3]),
            "{}x{}: the coat roughness lifted",
            a.width,
            a.height
        );
    }
}

/// Just above the floor (footprint length 0.99889, X ±6 codes) the bake widens mirror roughness, matching the f64
/// reference; at ±5 codes (0.99923) it is below the floor and adds nothing.
#[test]
fn a_tilt_just_above_the_noise_floor_still_widens() {
    let session = Session::new(Bc1Model::Ideal).unwrap();
    let mirror = rgba(64, |_, _| [0, 0, 200, 255]);
    for (k, widens) in [(6u8, true), (5, false)] {
        let n = tilted_columns(64, k);
        let l1 = reference_length(&n, 1, 0, 0);
        assert_eq!(l1 < 0.999, widens, "k = {k}: L = {l1}");
        let d = tempfile::tempdir().unwrap();
        put(
            d.path(),
            "textures/pbr/iron_n.png",
            DynamicImage::ImageRgb8(n.clone()),
        );
        put(d.path(), "textures/pbr/iron_rmaos.png", mirror.clone());
        let (_, levels) = planned_chain(
            &session,
            request(d.path(), Target::Cs, &[], &[]),
            "iron_rmaos.png",
        );
        let r = mirror.to_rgba8();
        for (level, l) in levels.iter().enumerate().skip(1) {
            let want = reference_roughness(&n, &r, level as u32, 0, 0, 0);
            let got = f64::from(l.at(0, 0)[0]);
            assert!(
                (got - want).abs() <= 1.0 / 255.0,
                "k = {k} mip {level}: {got} vs {want}"
            );
            assert_eq!(got > 0.1, widens, "k = {k} mip {level}: {got}");
        }
    }
}

/// M6 (DECISIONS S10): an OpenEXR and a Radiance HDR emissive in a PBR folder are float HDR sources → BC6H.
#[test]
fn exr_and_radiance_hdr_float_emissives_are_bc6h() {
    for ext in ["exr", "hdr"] {
        let d = tempfile::tempdir().unwrap();
        put(
            d.path(),
            &format!("textures/pbr/lamp_g.{ext}"),
            hdr_emissive(),
        );
        let o = convert(d.path(), Target::Cs, &[], &[]);
        assert_eq!(
            (o.summary.converted, o.summary.exit_code()),
            (1, 0),
            "{ext}: {:?}",
            warnings(&o)
        );
        assert_eq!(warnings(&o), vec![Code::FmtHdrSource], "{ext}");
        assert_profile(
            &o,
            &format!("lamp_g.{ext}"),
            "pbr_emissive",
            "remove",
            false,
            "bc6h_uf16",
        );
        assert_dds(
            &dds(d.path(), "textures/pbr/lamp_g.dds"),
            DdsPixelFormat::Bc6hUf16,
            HeaderKind::Dx10,
            7,
        );
    }
}

/// M6: a BMP diffuse converts like a PNG (lossless 8-bit source).
#[test]
fn bmp_diffuse_is_bc7_unorm() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/rock.bmp", rgb(64));
    let o = convert(
        d.path(),
        Target::Vanilla,
        &["file:**/rock.bmp.subtype=diffuse"],
        &[],
    );
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    assert_eq!(warnings(&o), vec![]);
    assert_dds(
        &dds(d.path(), "textures/a/rock.dds"),
        DdsPixelFormat::Bc7 { srgb: false },
        HeaderKind::Dx10,
        7,
    );
}

/// M6: a JPEG diffuse converts; its mip 0 matches the decoded JPEG within BC7 error.
#[test]
fn jpeg_diffuse_is_bc7_unorm_close_to_the_decoded_jpeg() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/rock.jpg", rgb(64));
    let o = convert(
        d.path(),
        Target::Vanilla,
        &["file:**/rock.jpg.subtype=diffuse"],
        &[],
    );
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    let f = dds(d.path(), "textures/a/rock.dds");
    assert_dds(&f, DdsPixelFormat::Bc7 { srgb: false }, HeaderKind::Dx10, 7);
    let src = image::open(d.path().join("in/textures/a/rock.jpg"))
        .unwrap()
        .to_rgb8();
    for (i, p) in levels(&f)[0].iter().enumerate() {
        let s = src.get_pixel(i as u32 % 64, i as u32 / 64).0;
        for c in 0..3 {
            assert!(
                (p[c] as i32 - s[c] as i32).abs() <= 8,
                "texel {i} c{c}: {} vs {}",
                p[c],
                s[c]
            );
        }
    }
}

/// DECISIONS S8: an embedded ICC profile is an info, never a block; the output matches the same PNG without it.
#[test]
fn an_icc_profile_is_reported_as_info_and_ignored() {
    use image::ImageEncoder;
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("in/textures/a/rock.png");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    let px = rgb(64).to_rgb8().into_raw();
    let mut f = std::fs::File::create(&p).unwrap();
    let mut enc = image::codecs::png::PngEncoder::new(&mut f);
    let mut icc = vec![0u8; 132];
    icc[..4].copy_from_slice(&132u32.to_be_bytes());
    icc[36..40].copy_from_slice(b"acsp");
    enc.set_icc_profile(icc).unwrap();
    enc.write_image(&px, 64, 64, image::ExtendedColorType::Rgb8)
        .unwrap();
    drop(f);
    let o = convert(
        d.path(),
        Target::Vanilla,
        &["file:**/rock.png.subtype=diffuse"],
        &[],
    );
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    assert_eq!(
        warning_of(&o, "rock.png", Code::InIccIgnored).severity,
        Severity::Info
    );
    let plain = tempfile::tempdir().unwrap();
    put(plain.path(), "textures/a/rock.png", rgb(64));
    let o2 = convert(
        plain.path(),
        Target::Vanilla,
        &["file:**/rock.png.subtype=diffuse"],
        &[],
    );
    assert!(!warnings(&o2).contains(&Code::InIccIgnored));
    let out = |p: &Path| std::fs::read(p.join("out/textures/a/rock.dds")).unwrap();
    assert_eq!(
        out(d.path()),
        out(plain.path()),
        "the profile changes no output byte"
    );
}

/// Review Focus 4: a broken file of each new type fails alone with IN-DECODE; the batch goes on.
#[test]
fn broken_new_format_files_fail_with_in_decode_and_the_batch_continues() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/good.png", rgb(64));
    for (n, bytes) in [
        ("bad_exr.exr", &b"\x76\x2f\x31\x01garbage"[..]),
        ("bad_hdr.hdr", &b"#?RADIANCE\n"[..]),
        ("bad_jpg.jpg", &b""[..]),
        ("bad_bmp.bmp", &b"BM"[..]),
    ] {
        std::fs::write(d.path().join("in/textures/a").join(n), bytes).unwrap();
    }
    let o = convert(d.path(), Target::Vanilla, &["subtype=diffuse"], &[]);
    assert_eq!(
        (o.summary.converted, o.summary.failed),
        (1, 4),
        "{:?}",
        warnings(&o)
    );
    assert_eq!(o.summary.exit_code(), 1);
    assert!(
        warnings(&o)
            .iter()
            .filter(|c| **c == Code::InDecode)
            .count()
            == 4
    );
}

/// DECISIONS S4: `rgba8` UNORM is a legacy B8G8R8A8 file, lossless (mip 0 equals the source), full chain, 99 dB.
#[test]
fn rgba8_output_is_legacy_b8g8r8a8_and_lossless() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/rock.png", rgb(64));
    let o = convert(
        d.path(),
        Target::Vanilla,
        &["file:**/rock.png.subtype=diffuse", "format=rgba8"],
        &[],
    );
    assert_eq!(
        (o.summary.converted, o.summary.exit_code()),
        (1, 0),
        "{:?}",
        warnings(&o)
    );
    let f = dds(d.path(), "textures/a/rock.dds");
    assert_dds(
        &f,
        DdsPixelFormat::Bgra8 { srgb: false },
        HeaderKind::Legacy,
        7,
    );
    let src = rgb(64).to_rgb8();
    for (i, p) in levels(&f)[0].iter().enumerate() {
        let s = src.get_pixel(i as u32 % 64, i as u32 / 64).0;
        assert_eq!(&p[..3], &s[..], "texel {i}");
    }
    assert_eq!(validation_of(&o, "rock.png").min_mip_psnr_db, Some(99.0));
    assert_eq!(
        planned(&o, "rock.png")["profile"]["encoder"]["backend"],
        "uncompressed"
    );
}

/// A cs PBR albedo as rgba8 is DX10 B8G8R8A8_UNORM_SRGB (91).
#[test]
fn rgba8_srgb_pbr_albedo_is_dx10_91() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/pbr/iron.png", rgb(64));
    let o = convert(
        d.path(),
        Target::Cs,
        &["subtype=pbr_albedo", "format=rgba8"],
        &[],
    );
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    let f = dds(d.path(), "textures/pbr/iron.dds");
    assert_dds(
        &f,
        DdsPixelFormat::Bgra8 { srgb: true },
        HeaderKind::Dx10,
        7,
    );
    assert_eq!(f.dx10.unwrap().dxgi, 91);
}

/// `r8` and `rg8` are DX10 UNORM and keep R (and G) exactly.
#[test]
fn r8_and_rg8_outputs_are_dx10_and_exact() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/hill.png", gray_ramp(64));
    put(d.path(), "textures/a/vec.png", rgb(64));
    let o = convert(
        d.path(),
        Target::Cs,
        &[
            "file:**/hill.png.subtype=height",
            "file:**/hill.png.format=r8",
            "file:**/vec.png.subtype=generic_two_channel",
            "file:**/vec.png.format=rg8",
        ],
        &[],
    );
    assert_eq!(o.summary.converted, 2, "{:?}", warnings(&o));
    let h = dds(d.path(), "textures/a/hill.dds");
    assert_dds(&h, DdsPixelFormat::R8, HeaderKind::Dx10, 7);
    assert_eq!(h.dx10.unwrap().dxgi, 61);
    assert!(levels(&h)[0]
        .iter()
        .enumerate()
        .all(|(i, p)| p[0] == ((i as u32 % 64) * 4) as u8));
    let v = dds(d.path(), "textures/a/vec.dds");
    assert_dds(&v, DdsPixelFormat::Rg8, HeaderKind::Dx10, 7);
    let src = rgb(64).to_rgb8();
    for (i, p) in levels(&v)[0].iter().enumerate() {
        let s = src.get_pixel(i as u32 % 64, i as u32 / 64).0;
        assert_eq!((p[0], p[1]), (s[0], s[1]), "texel {i}");
    }
}

/// `rgba16f` keeps float values (half precision), tag n/a, DXGI 10; it is cs-only (open question 2).
#[test]
fn rgba16f_output_keeps_float_values_and_is_cs_only() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/pbr/lamp_g.tif", hdr_emissive());
    let o = convert(
        d.path(),
        Target::Cs,
        &["file:**/lamp_g.tif.format=rgba16f"],
        &[],
    );
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    assert_eq!(planned(&o, "lamp_g.tif")["profile"]["color"]["tag"], "n/a");
    let f = dds(d.path(), "textures/pbr/lamp_g.dds");
    assert_dds(&f, DdsPixelFormat::Rgba16f, HeaderKind::Dx10, 7);
    let (w, h, r) = f.levels[0].clone();
    let Decoded::Hdr(px) = decode_level(f.format, w, h, &f.data[r], None).unwrap() else {
        panic!()
    };
    let src = hdr_emissive().to_rgb32f();
    for (p, s) in px.iter().zip(src.pixels()) {
        for (q, t) in p.iter().zip(s.0) {
            assert!((q - t).abs() <= 2e-3 * t.max(1.0), "{q} vs {t}");
        }
    }
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/lamp.tif", hdr_emissive());
    let o = convert(
        d.path(),
        Target::Vanilla,
        &["subtype=glow", "format=rgba16f", "color.tag=n/a"],
        &[],
    );
    assert_eq!(o.summary.converted, 0);
    let w = warning_of(&o, "lamp.tif", Code::CfgUnavailable);
    assert_eq!(w.severity, Severity::Error);
}

/// Convert `png` (64 px rgb) with `sets` into a fresh tempdir and return the written DDS bytes.
fn converted_bytes(target: Target, sets: &[&str]) -> Vec<u8> {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/rock.png", rgb(64));
    let o = convert(d.path(), target, sets, &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    std::fs::read(d.path().join("out/textures/a/rock.dds")).unwrap()
}

/// DECISIONS S6: a compressed DDS opted in with `reencode` converts, with LOSS-REENCODE (warning).
#[test]
fn an_opted_in_bc7_dds_is_reencoded_with_loss_reencode() {
    let bc7 = converted_bytes(Target::Vanilla, &["subtype=diffuse"]);
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("in/textures/old")).unwrap();
    std::fs::write(d.path().join("in/textures/old/rock.dds"), &bc7).unwrap();
    let o = convert(
        d.path(),
        Target::Vanilla,
        &[
            "file:**/rock.dds.reencode=true",
            "file:**/rock.dds.subtype=diffuse",
        ],
        &[],
    );
    assert_eq!(
        (o.summary.converted, o.summary.exit_code()),
        (1, 0),
        "{:?}",
        warnings(&o)
    );
    let w = warning_of(&o, "rock.dds", Code::LossReencode);
    assert!(
        w.severity == Severity::Warning && w.message.contains("BC7"),
        "{}",
        w.message
    );
    assert_dds(
        &dds(d.path(), "textures/old/rock.dds"),
        DdsPixelFormat::Bc7 { srgb: false },
        HeaderKind::Dx10,
        7,
    );
}

/// O1 exit test "a re-encode round trip": an uncompressed rgba8 DDS re-encodes to the same BC7 bytes as its PNG
/// source, and to the same rgba8 mip 0; no LOSS-REENCODE (nothing was compressed).
#[test]
fn an_uncompressed_dds_round_trips_through_reencode() {
    let rgba8 = converted_bytes(Target::Vanilla, &["subtype=diffuse", "format=rgba8"]);
    let direct_bc7 = converted_bytes(Target::Vanilla, &["subtype=diffuse"]);
    for (fmt, want) in [("bc7", direct_bc7.clone()), ("rgba8", rgba8.clone())] {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("in/textures/a")).unwrap();
        std::fs::write(d.path().join("in/textures/a/rock.dds"), &rgba8).unwrap();
        let o = convert(
            d.path(),
            Target::Vanilla,
            &["reencode=true", "subtype=diffuse", &format!("format={fmt}")],
            &[],
        );
        assert_eq!(o.summary.converted, 1, "{fmt}: {:?}", warnings(&o));
        assert!(!warnings(&o).contains(&Code::LossReencode), "{fmt}");
        let got = std::fs::read(d.path().join("out/textures/a/rock.dds")).unwrap();
        assert_eq!(got, want, "{fmt}: byte-identical to the direct conversion");
    }
}

/// Review Focus 1: output root == input folder would overwrite the source: the file fails, the source is untouched.
#[test]
fn reencode_into_its_own_folder_fails_and_leaves_the_source_alone() {
    let rgba8 = converted_bytes(Target::Vanilla, &["subtype=diffuse", "format=rgba8"]);
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("mod");
    std::fs::create_dir_all(root.join("textures")).unwrap();
    let src = root.join("textures/rock.dds");
    std::fs::write(&src, &rgba8).unwrap();
    // An explicit file input: a folder walk skips everything under the output root, so only an explicit file can
    // map onto itself (`discover::make` gives `output == path`).
    let mut req = BatchRequest::new(vec![src.clone()], root.join("textures"), Target::Vanilla);
    req.overrides = ["reencode=true", "subtype=diffuse"]
        .iter()
        .map(|s| parse_set(s).unwrap())
        .collect();
    req.on_existing = OnExisting::Overwrite;
    let o = run(req);
    assert_eq!(
        (o.summary.converted, o.summary.failed, o.summary.exit_code()),
        (0, 1, 1)
    );
    assert_eq!(std::fs::read(&src).unwrap(), rgba8);
}

/// Review Focus 1: an opted-in DDS and a same-stem image map to one output: both are IN-DUPLICATE.
#[test]
fn an_opted_in_dds_and_a_same_stem_image_are_duplicates() {
    let rgba8 = converted_bytes(Target::Vanilla, &["subtype=diffuse", "format=rgba8"]);
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/rock.png", rgb(64));
    std::fs::write(d.path().join("in/textures/a/rock.dds"), &rgba8).unwrap();
    let o = convert(
        d.path(),
        Target::Vanilla,
        &["reencode=true", "subtype=diffuse"],
        &[],
    );
    assert_eq!(skip_code(&o, "rock.png"), Some(Code::InDuplicate));
    assert_eq!(skip_code(&o, "rock.dds"), Some(Code::InDuplicate));
    // Without the opt-in the DDS takes no part, as before.
    let o = convert(d.path(), Target::Vanilla, &["subtype=diffuse"], &[]);
    assert_eq!(skip_code(&o, "rock.dds"), Some(Code::InDdsSkipped));
    assert_eq!(o.summary.converted, 1);
}

/// DECISIONS S6 "a re-encoded DDS member does not bake": an opted-in DDS `_n` next to an image `_rmaos` is re-encoded,
/// the roughness is written without the variance bake, and SET-COMPANION-DDS says why.
#[test]
fn an_opted_in_dds_normal_does_not_bake() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/pbr/iron_n.png", bumpy_normal(64));
    assert_eq!(convert(d.path(), Target::Cs, &[], &[]).summary.converted, 1);
    let n_dds = std::fs::read(d.path().join("out/textures/pbr/iron_n.dds")).unwrap();
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("in/textures/pbr")).unwrap();
    std::fs::write(d.path().join("in/textures/pbr/iron_n.dds"), &n_dds).unwrap();
    put(d.path(), "textures/pbr/iron_rmaos.png", rough_member(64));
    let o = convert(d.path(), Target::Cs, &["reencode=true"], &[]);
    assert_eq!(
        (o.summary.converted, o.summary.exit_code()),
        (2, 0),
        "{:?}",
        warnings(&o)
    );
    assert!(!o
        .events
        .iter()
        .any(|e| matches!(&e.kind, EventKind::SetResolved { bake: true, .. })));
    assert!(planned(&o, "iron_rmaos.png").get("bake").is_none());
    let w = warning_of(&o, "iron_rmaos.png", Code::SetCompanionDds);
    assert!(
        w.message.contains("DDS members do not bake"),
        "{}",
        w.message
    );
    assert!(d.path().join("out/textures/pbr/iron_rmaos.dds").exists());
}

/// Review Focus 1: an input reached through a junction still maps onto itself: the guard compares canonical paths.
#[cfg(windows)]
#[test]
fn reencode_through_a_junction_into_its_own_folder_fails() {
    let rgba8 = converted_bytes(Target::Vanilla, &["subtype=diffuse", "format=rgba8"]);
    let d = tempfile::tempdir().unwrap();
    let real = d.path().join("real");
    std::fs::create_dir_all(&real).unwrap();
    std::fs::write(real.join("rock.dds"), &rgba8).unwrap();
    let link = d.path().join("link");
    let mk = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(&real)
        .output()
        .unwrap();
    assert!(
        mk.status.success(),
        "{}",
        String::from_utf8_lossy(&mk.stderr)
    );
    let mut req = BatchRequest::new(vec![link.join("rock.dds")], real.clone(), Target::Vanilla);
    req.overrides = ["reencode=true", "subtype=diffuse"]
        .iter()
        .map(|s| parse_set(s).unwrap())
        .collect();
    req.on_existing = OnExisting::Overwrite;
    let o = run(req);
    assert_eq!((o.summary.converted, o.summary.failed), (0, 1));
    assert_eq!(std::fs::read(real.join("rock.dds")).unwrap(), rgba8);
}

/// S-mips-6/7 (M6 exit): a 48x40 diffuse under `resize = pot-down` is written at 32x32 (6 mips) with no
/// acknowledgement; DIM-RESIZED reports the size it was written at.
#[test]
fn npot_diffuse_with_pot_down_converts_without_acknowledgement() {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "d/odd.png",
        DynamicImage::ImageRgb8(image::RgbImage::from_fn(48, 40, |x, y| {
            Rgb([(x * 5) as u8, (y * 6) as u8, 90])
        })),
    );
    let o = convert(
        d.path(),
        Target::Cs,
        &["resize=pot-down", "file:**/odd.png.subtype=diffuse"],
        &[],
    );
    assert_eq!(
        (
            o.summary.converted,
            o.summary.skipped,
            o.summary.exit_code()
        ),
        (1, 0, 0),
        "{:?}",
        warnings(&o)
    );
    assert_eq!(
        warning_of(&o, "odd.png", Code::DimResized).data["to"],
        serde_json::json!([32, 32])
    );
    let f = dds(d.path(), "d/odd.dds");
    assert_eq!((f.width, f.height, f.mip_count), (32, 32, 6));
}

/// S-mips-3 (M6 exit): Kaiser changes the RGB of mips >= 1 of an sRGB colour map and leaves mip 0 and the
/// box-filtered data alpha alone. Unscoped sets on a one-file batch; ruling M2: `alpha.policy = data` in both runs,
/// else the varying alpha leaves the file unresolved.
#[test]
fn kaiser_diffuse_differs_from_box_and_its_alpha_does_not() {
    let d = tempfile::tempdir().unwrap();
    // 3 px checker: high frequency below Nyquist, where box and Kaiser disagree.
    put(
        d.path(),
        "textures/a/rock.png",
        rgba(64, |x, y| {
            let c = if (x / 3 + y / 3) % 2 == 0 { 230 } else { 20 };
            [c, 255 - c, 90, (y * 4) as u8]
        }),
    );
    // A file-scoped sub-type, not `subtype:diffuse` (which may silently miss): a suffixless file is otherwise
    // unresolved (SUG-LOW-CONFIDENCE).
    const DIFFUSE: &str = "file:**/rock.png.subtype=diffuse";
    // Uncompressed: under BC7 a block holding this checker and an alpha ramp trades alpha for RGB, differently per
    // filter (measured: box 16 / Kaiser 10 at mip 1 texel 0 vs 2 in the source), which would hide the filter's alpha.
    const LOSSLESS: &str = "format=rgba8";
    let out = |sets: &[&str]| {
        let o = convert(d.path(), Target::Cs, sets, &[]);
        assert_eq!(
            (o.summary.converted, o.summary.exit_code()),
            (1, 0),
            "{:?}",
            warnings(&o)
        );
        let p = planned(&o, "rock.png")["profile"].clone();
        assert_eq!(
            (p["category"].as_str(), p["color"]["content"].as_str()),
            (Some("color"), Some("srgb")),
            "{p}"
        );
        levels(&dds(d.path(), "textures/a/rock.dds"))
    };
    let kaiser = out(&["mips.filter=kaiser", "alpha.policy=data", DIFFUSE, LOSSLESS]);
    let boxed = out(&["alpha.policy=data", DIFFUSE, LOSSLESS]);
    assert_eq!((kaiser.len(), boxed.len()), (7, 7));
    assert_eq!(kaiser[0], boxed[0], "mip 0 is not filtered");
    // Every level from 32x32 to 2x2. The 1x1 tail is left out: a wrapped Kaiser over the whole image is
    // (to 8 bits) its mean, as is box.
    for (i, (k, b)) in kaiser.iter().zip(&boxed).enumerate().take(6).skip(1) {
        assert!(
            k.iter().zip(b).any(|(k, b)| k[..3] != b[..3]),
            "mip {i}: Kaiser RGB equals box"
        );
    }
    for (i, (k, b)) in kaiser.iter().zip(&boxed).enumerate() {
        for (j, (k, b)) in k.iter().zip(b).enumerate() {
            assert!(
                (k[3] as i32 - b[3] as i32).abs() <= 2,
                "mip {i} texel {j}: alpha {} vs box {}",
                k[3],
                b[3]
            );
        }
    }
}

/// Spec §5.5 (M6 exit): Kaiser is for sRGB colour only; an unscoped set blocks the `_n` with CFG-INVALID and the
/// diffuse of the same set still converts.
#[test]
fn kaiser_on_a_normal_map_is_cfg_invalid() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/wall.png", rgb(64));
    put(d.path(), "textures/a/wall_n.png", bumpy_normal(64));
    let o = convert(
        d.path(),
        Target::Cs,
        &["mips.filter=kaiser", "file:**/wall.png.subtype=diffuse"],
        &[],
    );
    assert_eq!(
        (o.summary.converted, o.summary.failed),
        (1, 1),
        "{:?}",
        warnings(&o)
    );
    assert!(
        warning_of(&o, "wall_n.png", Code::CfgInvalid)
            .message
            .starts_with("mips.filter = kaiser: "),
        "{:?}",
        warning_of(&o, "wall_n.png", Code::CfgInvalid)
    );
    assert!(!d.path().join("out/textures/a/wall_n.dds").exists());
    assert!(d.path().join("out/textures/a/wall.dds").exists());
}

/// M6 exit: `resize` and `mips.edge` serialise only off their defaults, so no built-in profile's hash moves.
#[test]
fn default_profiles_hash_as_before() {
    let d = tempfile::tempdir().unwrap();
    let ids: Vec<String> = engine::config::Subtype::ALL
        .iter()
        .map(|s| s.id())
        .collect();
    let sets: Vec<String> = ids
        .iter()
        .map(|id| {
            put(d.path(), &format!("textures/h/{id}.png"), rgb(64));
            format!("file:**/h/{id}.png.subtype={id}")
        })
        .collect();
    let sets: Vec<&str> = sets.iter().map(String::as_str).collect();
    // Plan only: `file_planned` carries the resolved profile, and nothing needs encoding.
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "golden");
    Engine::new()
        .unwrap()
        .plan(
            request(d.path(), Target::Cs, &sets, &[]),
            &rep,
            &CancelToken::default(),
        )
        .unwrap();
    let o = Out {
        summary: BatchSummary::default(),
        events: sink.take(),
    };
    for id in &ids {
        let p = &planned(&o, &format!("h/{id}.png"))["profile"];
        assert_eq!(p["subtype"].as_str(), Some(id.as_str()));
        assert!(p.get("resize").is_none(), "{id}: {p}");
        assert!(p["mips"].get("edge").is_none(), "{id}: {p}");
    }
}
