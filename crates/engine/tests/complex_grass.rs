//! Complex grass sub-type (design `F:\ai\dds\nifparseref\plan\esp\2026-10-12-complex-grass-design.md`).
use engine::analyse::code8;
use engine::api::*;
use engine::codes::Code;
use engine::config::*;
use engine::events::*;
use engine::image::Rgba32F;
use engine::policy::{
    availability, offered, AcceptSuggestions, Availability, JobStatus, Need, ResolvedJob,
};
use image::{DynamicImage, ImageBuffer, Rgba};
use std::sync::Arc;
use tempfile::TempDir;

const TAG: &str = "file:**/g.png.subtype=complex_grass";
const CUTOUT: &str = "file:**/g.png.alpha.policy=cutout";
const GRASS_CODES: [Code; 6] = [
    Code::GrassShape,
    Code::GrassTopOpaque,
    Code::GrassNormals,
    Code::GrassDetectSource,
    Code::GrassVanilla,
    Code::ValGrassDetect,
];

/// A `w` × 2`w` complex-grass source: top = colour with a ~30 % cutout alpha (all texels 0 or 255), bottom = unit
/// normals tilted along x with specular 12 in alpha. Its bottom-left texel is a unit vector.
fn grass(w: u32) -> Rgba32F {
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

fn png(img: &Rgba32F) -> DynamicImage {
    DynamicImage::ImageRgba8(ImageBuffer::from_fn(img.width, img.height, |x, y| {
        Rgba(img.at(x, y).map(code8))
    }))
}

/// Writes `img` as `in/textures/plants/<name>`.
fn put(d: &TempDir, name: &str, img: &Rgba32F) {
    let p = d.path().join("in/textures/plants").join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    png(img).save(p).unwrap();
}

fn req(d: &TempDir, target: Target, sets: &[&str]) -> BatchRequest {
    let mut r = BatchRequest::new(vec![d.path().join("in")], d.path().join("out"), target);
    r.recursive = true;
    r.accept = AcceptSuggestions::High;
    r.on_existing = OnExisting::Overwrite;
    r.overrides = sets
        .iter()
        .map(|s| engine::overrides::parse_set(s).unwrap())
        .collect();
    r
}

fn plan_req(r: BatchRequest) -> Plan {
    let rep = Reporter::new(Arc::new(VecSink::default()), "t");
    Engine::new()
        .unwrap()
        .plan(r, &rep, &CancelToken::default())
        .unwrap()
}

fn plan(d: &TempDir, target: Target, sets: &[&str]) -> Plan {
    plan_req(req(d, target, sets))
}

fn job<'a>(p: &'a Plan, name: &str) -> &'a ResolvedJob {
    p.jobs
        .iter()
        .find(|j| j.input.file_name().is_some_and(|n| n == name))
        .unwrap_or_else(|| panic!("no job for {name}"))
}

fn codes(j: &ResolvedJob) -> Vec<Code> {
    j.warnings.iter().map(|w| w.code).collect()
}

fn grass_codes(j: &ResolvedJob) -> Vec<Code> {
    codes(j)
        .into_iter()
        .filter(|c| GRASS_CODES.contains(c))
        .collect()
}

/// A clean source tagged by hand with its alpha meaning given: Ready, no grass code.
#[test]
fn a_clean_tagged_source_is_ready_with_no_grass_code() {
    let d = tempfile::tempdir().unwrap();
    put(&d, "g.png", &grass(32));
    let p = plan(&d, Target::Cs, &[TAG, CUTOUT]);
    let j = job(&p, "g.png");
    assert_eq!(j.profile.subtype, Subtype::ComplexGrass);
    assert_eq!(
        (j.profile.format, j.profile.srgb_tag()),
        (Format::Bc7, false)
    );
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert!(grass_codes(j).is_empty(), "{:?}", j.warnings);
}

/// §4 first bullet (Review Focus 1): not exactly 1:2 or not a power of two is an error.
#[test]
fn shape_errors_are_grass_shape() {
    for (w, h) in [(32, 32), (64, 32), (48, 96)] {
        let d = tempfile::tempdir().unwrap();
        put(&d, "g.png", &Rgba32F::new(w, h, [0.5, 0.5, 1.0, 1.0]));
        let p = plan(&d, Target::Cs, &[TAG, CUTOUT]);
        let j = job(&p, "g.png");
        assert!(
            codes(j).contains(&Code::GrassShape),
            "{w}x{h}: {:?}",
            j.warnings
        );
        assert!(
            matches!(&j.status, JobStatus::Error { codes } if codes.contains(&Code::GrassShape)),
            "{w}x{h}: {:?}",
            j.status
        );
    }
}

/// §4 second bullet: the top half has no transparency.
#[test]
fn an_opaque_top_half_warns() {
    let d = tempfile::tempdir().unwrap();
    let mut img = grass(32);
    for px in &mut img.px[..32 * 32] {
        px[3] = 1.0;
    }
    put(&d, "g.png", &img);
    let p = plan(&d, Target::Cs, &[TAG, CUTOUT]);
    assert_eq!(grass_codes(job(&p, "g.png")), [Code::GrassTopOpaque]);
}

/// §4 third bullet (Review Focus 5): fewer than 99 % unit normals in the bottom half.
#[test]
fn bottom_normals_share_threshold() {
    for (bad, want) in [(5usize, vec![]), (21, vec![Code::GrassNormals])] {
        // 32 × 32 bottom half = 1024 texels: 5 is 0.5 %, 21 is 2.05 %. Zero vectors, never the corner texel.
        let d = tempfile::tempdir().unwrap();
        let mut img = grass(32);
        for i in 0..bad {
            img.px[32 * 32 + i] = [0.5, 0.5, 0.5, 12.0 / 255.0];
        }
        put(&d, "g.png", &img);
        let p = plan(&d, Target::Cs, &[TAG, CUTOUT]);
        assert_eq!(grass_codes(job(&p, "g.png")), want, "{bad} bad texels");
    }
}

/// §4 last bullet: the source's bottom-left texel already fails CS's 0.03 test.
#[test]
fn a_failing_source_corner_warns() {
    let d = tempfile::tempdir().unwrap();
    let mut img = grass(32);
    *img.at_mut(0, 63) = [0.5, 0.5, 0.75, 12.0 / 255.0]; // decodes to (0, 0, 0.5): |L-1| = 0.5
    put(&d, "g.png", &img);
    let p = plan(&d, Target::Cs, &[TAG, CUTOUT]);
    let j = job(&p, "g.png");
    assert_eq!(grass_codes(j), [Code::GrassDetectSource]);
    let w = j
        .warnings
        .iter()
        .find(|w| w.code == Code::GrassDetectSource)
        .unwrap();
    assert!(w.message.contains("renormalised"), "{}", w.message);
}

/// R1 (Review Focus 4): under vanilla the tag stays, with a warning, and the file is not blocked.
#[test]
fn vanilla_tag_warns_and_stays_complex_grass() {
    let d = tempfile::tempdir().unwrap();
    put(&d, "g.png", &grass(32));
    let p = plan(&d, Target::Vanilla, &[TAG, CUTOUT]);
    let j = job(&p, "g.png");
    assert_eq!(j.profile.subtype, Subtype::ComplexGrass);
    assert_eq!(grass_codes(j), [Code::GrassVanilla]);
    assert!(!codes(j).contains(&Code::CfgUnavailable));
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert_eq!(
        availability(Target::Vanilla, Subtype::ComplexGrass),
        Availability::Available
    );
    assert!(!offered(Target::Vanilla, Subtype::ComplexGrass));
    assert!(
        offered(Target::Cs, Subtype::ComplexGrass) && offered(Target::Cpm, Subtype::ComplexGrass)
    );
    assert!(offered(Target::Vanilla, Subtype::Diffuse));
}

/// R4, R7: BC7 UNORM only, and no resize.
#[test]
fn another_format_an_srgb_tag_or_a_resize_is_cfg_invalid() {
    for extra in [
        "file:**/g.png.format=bc3",
        "file:**/g.png.color.tag=srgb",
        "file:**/g.png.resize=half",
    ] {
        let d = tempfile::tempdir().unwrap();
        put(&d, "g.png", &grass(512)); // 512 x 1024: `half` applies (larger side > 512)
        let p = plan(&d, Target::Cs, &[TAG, CUTOUT, extra]);
        let j = job(&p, "g.png");
        assert!(
            j.warnings
                .iter()
                .any(|w| w.code == Code::CfgInvalid && w.message.contains("complex_grass")),
            "{extra}: {:?}",
            j.warnings
        );
    }
}

/// R5: the alpha question is asked as for a diffuse, and its proposal reads the top half (the whole image is only
/// ~50 % near-binary because of the specular half, which would propose blend).
#[test]
fn the_alpha_proposal_reads_the_top_half() {
    let d = tempfile::tempdir().unwrap();
    put(&d, "g.png", &grass(32));
    let p = plan(&d, Target::Cs, &[TAG]);
    match &job(&p, "g.png").status {
        JobStatus::Unresolved { reasons, needs } => {
            assert!(needs.contains(&Need::AlphaMeaning), "{needs:?}");
            assert!(reasons.iter().any(|r| r.contains("Cutout")), "{reasons:?}");
        }
        s => panic!("expected the alpha question, got {s:?}"),
    }
}

/// Review Focus 3: a tall texture that is not tagged complex grass gets no grass code.
#[test]
fn a_tall_diffuse_raises_no_grass_code() {
    let d = tempfile::tempdir().unwrap();
    put(&d, "tall.png", &grass(32));
    let p = plan(&d, Target::Cs, &["file:**/tall.png.alpha.policy=cutout"]);
    let j = job(&p, "tall.png");
    assert_ne!(j.profile.subtype, Subtype::ComplexGrass);
    assert!(grass_codes(j).is_empty(), "{:?}", j.warnings);
}

fn run(d: &TempDir, target: Target, sets: &[&str]) -> (Vec<EngineEvent>, BatchSummary) {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "t");
    let e = Engine::new().unwrap();
    let p = e
        .plan(req(d, target, sets), &rep, &CancelToken::default())
        .unwrap();
    let s = e.run(p, &rep, &CancelToken::default()).unwrap();
    (sink.take(), s)
}

fn out(d: &TempDir) -> engine::dds::DdsFile {
    let b = std::fs::read(d.path().join("out/textures/plants/g.dds")).unwrap();
    engine::dds::read_dds(&b).unwrap()
}

/// §3.5, R6: one BC7 UNORM file, 64 x 128 down to 1 x 2 (7 levels).
#[test]
fn a_tagged_grass_texture_is_one_bc7_unorm_file_down_to_one_by_two() {
    let d = tempfile::tempdir().unwrap();
    put(&d, "g.png", &grass(64));
    let (_, s) = run(&d, Target::Cs, &[TAG, CUTOUT]);
    assert_eq!(s.converted, 1);
    let f = out(&d);
    assert_eq!(f.format.as_format(), Some((Format::Bc7, false)));
    assert_eq!((f.width, f.height, f.mip_count), (64, 128, 7));
}
