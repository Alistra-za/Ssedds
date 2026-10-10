//! M3a Task 8: `Session::preview` mips equal the same mips of the DDS `run` writes, byte for byte.
use encoders_sys::Bc1Model;
use engine::api::*;
use engine::config::Target;
use engine::dds::read_dds;
use engine::events::*;
use engine::overrides::parse_set;
use engine::policy::{AcceptSuggestions, JobStatus, ResolvedJob};
use engine::session::*;
use image::{ImageBuffer, Luma, Rgb, Rgba};
use std::path::Path;
use std::sync::Arc;

/// Sources and the overrides that decide each one. They cover BC7, BC1, BC3, BC5 or BC7 normals, BC4, a cutout with
/// coverage on and a complex `_m` with exact opaque alpha on (the paths that look at more than one block or mip).
/// Every job must be Ready; if one is not, change its overrides (not the assertions) and say so in the report.
fn fixture(dir: &Path) -> Vec<&'static str> {
    let r = dir.join("in");
    std::fs::create_dir_all(&r).unwrap();
    let grad = |x: u32, y: u32| [(x * 4) as u8, (y * 4) as u8, ((x + y) * 2) as u8];
    ImageBuffer::from_fn(64, 64, |x, y| Rgb(grad(x, y)))
        .save(r.join("c7.png"))
        .unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Rgb(grad(x, y)))
        .save(r.join("c1.png"))
        .unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| {
        let g = grad(x, y);
        Rgba([g[0], g[1], g[2], (x * 4) as u8])
    })
    .save(r.join("c3.png"))
    .unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| {
        let g = grad(x, y);
        Rgba([
            g[0],
            g[1],
            g[2],
            if (x / 8 + y / 8) % 2 == 0 { 255 } else { 0 },
        ])
    })
    .save(r.join("cut.png"))
    .unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| {
        Rgb([128 + (x % 16) as u8, 128 - (y % 16) as u8, 250])
    })
    .save(r.join("rock_n.png"))
    .unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| {
        let g = grad(x, y);
        Rgba([g[0], g[1], g[2], if x < 32 { 255 } else { (y * 4) as u8 }])
    })
    .save(r.join("armor_m.png"))
    .unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Luma([((x + y) * 2) as u8]))
        .save(r.join("h_p.png"))
        .unwrap();
    vec![
        "file:c7.png.subtype=diffuse",
        "file:c1.png.subtype=diffuse",
        "file:c1.png.format=bc1",
        "file:c3.png.subtype=diffuse",
        "file:c3.png.alpha.policy=blend",
        "file:c3.png.format=bc3",
        "file:cut.png.subtype=diffuse",
        "file:cut.png.alpha.policy=cutout",
        "file:rock_n.png.subtype=normal",
        "file:rock_n.png.channels.green_convention=directx",
        "file:armor_m.png.subtype=complex_material",
        "file:h_p.png.subtype=height",
    ]
}

fn request(dir: &Path, sets: &[&str]) -> BatchRequest {
    let mut r = BatchRequest::new(vec![dir.join("in")], dir.join("out"), Target::Cs);
    r.overrides = sets.iter().map(|s| parse_set(s).unwrap()).collect();
    r.on_existing = OnExisting::Overwrite;
    r
}

fn check(max_size: u32, expect_first: usize) {
    let d = tempfile::tempdir().unwrap();
    let sets = fixture(d.path());
    let s = Session::new(Bc1Model::Ideal).unwrap();
    let rep = Reporter::new(Arc::new(NullSink), "t");
    let plan = s
        .plan(request(d.path(), &sets), &rep, &CancelToken::default())
        .unwrap();
    let jobs: Vec<ResolvedJob> = plan.jobs.clone();
    assert_eq!(jobs.len(), 7);
    for j in &jobs {
        assert_eq!(
            j.status,
            JobStatus::Ready,
            "{} is {:?}",
            j.input.display(),
            j.status
        );
    }
    s.run(plan, &rep, &CancelToken::default()).unwrap();
    for j in &jobs {
        let p = s.preview(j, max_size, &CancelToken::default()).unwrap();
        assert_eq!(p.source.first_level, expect_first, "{}", j.input.display());
        let dds = read_dds(&std::fs::read(&j.output).unwrap()).unwrap();
        assert_eq!(dds.levels.len(), p.source.dims.len());
        assert_eq!(p.levels.len(), dds.levels.len() - expect_first);
        for (i, enc) in p.levels.iter().enumerate() {
            let range = dds.levels[expect_first + i].2.clone();
            assert!(
                enc[..] == dds.data[range],
                "{} mip {}",
                j.input.display(),
                expect_first + i
            );
        }
        assert_eq!(p.source.levels.len(), p.levels.len());
        assert_eq!(p.source.levels[0].width, dds.levels[expect_first].0);
    }
}

#[test]
fn preview_mips_equal_the_exported_mips() {
    check(4096, 0);
}

#[test]
fn a_size_limit_skips_the_large_mips_and_keeps_the_rest_identical() {
    // 64 and 32 are above 16: the preview starts at mip 2 (16x16).
    check(16, 2);
}

#[test]
fn first_level_within_picks_the_first_mip_that_fits() {
    let dims = [(8192, 4096), (4096, 2048), (2048, 1024), (1, 1)];
    assert_eq!(first_level_within(&dims, 4096), 1);
    assert_eq!(first_level_within(&dims, 16384), 0);
    assert_eq!(first_level_within(&dims, 0), 3);
}

/// A float emissive with one very bright texel: its mip 0 peak is far above the peak of any smaller mip.
fn bright_spot_job(dir: &Path) -> (Session, ResolvedJob) {
    let p = dir.join("in/textures/pbr/lamp_g.tif");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    image::DynamicImage::ImageRgb32F(ImageBuffer::from_fn(64, 64, |x, y| {
        let v = if (x, y) == (31, 31) { 100.0 } else { 0.5 };
        Rgb([v, v, v])
    }))
    .save(p)
    .unwrap();
    let s = Session::new(Bc1Model::Ideal).unwrap();
    let rep = Reporter::new(Arc::new(NullSink), "t");
    let mut req = request(dir, &[]);
    req.recursive = true;
    req.accept = AcceptSuggestions::High;
    let plan = s.plan(req, &rep, &CancelToken::default()).unwrap();
    assert_eq!(plan.jobs.len(), 1, "{:?} {:?}", plan.failed, plan.skipped);
    let job = plan.jobs[0].clone();
    assert_eq!(job.status, JobStatus::Ready, "{:?}", job.status);
    (s, job)
}

#[test]
fn a_bc6h_preview_from_a_smaller_mip_measures_the_peak_of_that_mip() {
    let d = tempfile::tempdir().unwrap();
    let (s, job) = bright_spot_job(d.path());
    let p = s.preview(&job, 16, &CancelToken::default()).unwrap();
    assert_eq!(p.format, engine::config::Format::Bc6hUf16);
    assert_eq!(p.source.first_level, 2);
    // The peak the check compares with is the first kept mip's, not mip 0's (100).
    let ratio = p.validation.bc6h_peak_ratio.unwrap();
    assert!((0.8..1.25).contains(&ratio), "peak ratio {ratio}");
    assert!(
        p.warnings
            .iter()
            .all(|w| w.code != engine::codes::Code::ValBc6hPeak),
        "{:?}",
        p.warnings
    );
}

#[test]
fn normal_len_has_one_entry_per_encoded_level() {
    let d = tempfile::tempdir().unwrap();
    let sets = fixture(d.path());
    let s = Session::new(Bc1Model::Ideal).unwrap();
    let rep = Reporter::new(Arc::new(NullSink), "t");
    let plan = s
        .plan(request(d.path(), &sets), &rep, &CancelToken::default())
        .unwrap();
    let job = plan
        .jobs
        .iter()
        .find(|j| j.input.ends_with("rock_n.png"))
        .unwrap();
    for (max, first) in [(4096, 0), (16, 2)] {
        let p = s.preview(job, max, &CancelToken::default()).unwrap();
        assert_eq!(p.source.first_level, first);
        assert!(!p.levels.is_empty());
        assert_eq!(p.normal_len.len(), p.levels.len());
    }
}
