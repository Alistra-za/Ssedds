//! M6 Task 4 (DECISIONS O2, S3): pbr_emissive is BC6H only for a float source peaking above 1.0.
use engine::api::*;
use engine::codes::Code;
use engine::config::*;
use engine::events::*;
use engine::overrides::parse_set;
use engine::policy::{AcceptSuggestions, JobStatus, ResolvedJob};
use image::{DynamicImage, ImageBuffer, Rgb};
use std::path::Path;
use std::sync::Arc;

fn plan_one(dir: &Path, name: &str, img: DynamicImage, sets: &[&str]) -> ResolvedJob {
    let p = dir.join("in/textures/pbr").join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    img.save(&p).unwrap();
    let mut req = BatchRequest::new(vec![dir.join("in")], dir.join("out"), Target::Cs);
    req.recursive = true;
    req.accept = AcceptSuggestions::High;
    req.overrides = sets.iter().map(|s| parse_set(s).unwrap()).collect();
    let plan = Engine::new()
        .unwrap()
        .plan(
            req,
            &Reporter::new(Arc::new(NullSink), "t"),
            &CancelToken::default(),
        )
        .unwrap();
    plan.jobs.into_iter().next().expect("one job")
}

fn float_g(peak: f32) -> DynamicImage {
    DynamicImage::ImageRgb32F(ImageBuffer::from_fn(16, 16, move |x, _| {
        Rgb([peak * x as f32 / 15.0, 0.2, 0.1])
    }))
}

fn eight_bit_g() -> DynamicImage {
    DynamicImage::ImageRgb8(ImageBuffer::from_fn(16, 16, |x, _| {
        Rgb([(x * 16) as u8, 40, 20])
    }))
}

fn reason(j: &ResolvedJob) -> String {
    j.warnings
        .iter()
        .find(|w| w.code == Code::FmtHdrSource)
        .expect("FMT-HDR-SOURCE")
        .message
        .clone()
}

fn shape(j: &ResolvedJob) -> (Format, ColorTag, Backend, String) {
    (
        j.profile.format,
        j.profile.color.tag,
        j.profile.encoder.backend,
        j.provenance["format"].to_string(),
    )
}

#[test]
fn an_8_bit_emissive_is_bc7_srgb_as_an_analysis_fact() {
    let d = tempfile::tempdir().unwrap();
    let j = plan_one(d.path(), "lamp_g.png", eight_bit_g(), &[]);
    assert_eq!(
        shape(&j),
        (
            Format::Bc7,
            ColorTag::Srgb,
            Backend::Bc7e,
            "analysis:hdr-source".into()
        )
    );
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert!(
        reason(&j).contains("8-bit source, no HDR → BC7 sRGB"),
        "{}",
        reason(&j)
    );
}

#[test]
fn a_float_emissive_above_one_is_bc6h() {
    let d = tempfile::tempdir().unwrap();
    let j = plan_one(d.path(), "lamp_g.tif", float_g(4.0), &[]);
    assert_eq!(
        (j.profile.format, j.profile.color.tag),
        (Format::Bc6hUf16, ColorTag::NotApplicable)
    );
    assert_eq!(j.profile.encoder.backend, Backend::IspcBc6h);
    assert!(
        reason(&j).contains("peak 4.00 > 1.0: HDR → BC6H"),
        "{}",
        reason(&j)
    );
}

#[test]
fn a_float_emissive_at_or_below_one_is_bc7_srgb() {
    for peak in [0.9f32, 1.0] {
        let d = tempfile::tempdir().unwrap();
        let j = plan_one(d.path(), "lamp_g.tif", float_g(peak), &[]);
        assert_eq!(
            (j.profile.format, j.profile.color.tag),
            (Format::Bc7, ColorTag::Srgb),
            "{peak}"
        );
        assert!(
            reason(&j).contains("no HDR → BC7 sRGB"),
            "{peak}: {}",
            reason(&j)
        );
    }
}

#[test]
fn an_explicit_format_wins_in_both_directions_and_the_tag_follows() {
    let d = tempfile::tempdir().unwrap();
    let j = plan_one(
        d.path(),
        "lamp_g.png",
        eight_bit_g(),
        &["file:**/lamp_g.png.format=bc6h_uf16"],
    );
    assert_eq!(
        shape(&j),
        (
            Format::Bc6hUf16,
            ColorTag::NotApplicable,
            Backend::IspcBc6h,
            "user".into()
        )
    );
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert!(
        reason(&j).contains("the HDR check is skipped"),
        "{}",
        reason(&j)
    );
    let d = tempfile::tempdir().unwrap();
    let j = plan_one(
        d.path(),
        "lamp_g.tif",
        float_g(4.0),
        &["file:**/lamp_g.tif.format=bc7"],
    );
    assert_eq!(
        (j.profile.format, j.profile.color.tag),
        (Format::Bc7, ColorTag::Srgb)
    );
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    // An explicit tag is kept, even when it disagrees (and CFG-INVALID then reports it).
    let d = tempfile::tempdir().unwrap();
    let j = plan_one(
        d.path(),
        "lamp_g.png",
        eight_bit_g(),
        &[
            "file:**/lamp_g.png.format=bc6h_uf16",
            "file:**/lamp_g.png.color.tag=srgb",
        ],
    );
    assert!(j.warnings.iter().any(|w| w.code == Code::CfgInvalid));
}

#[test]
fn hdr_sky_is_unchanged() {
    let d = tempfile::tempdir().unwrap();
    let j = plan_one(d.path(), "dome.tif", float_g(0.5), &["subtype=hdr_sky"]);
    assert_eq!(j.profile.format, Format::Bc6hUf16);
    assert!(j.warnings.iter().all(|w| w.code != Code::FmtHdrSource));
}
