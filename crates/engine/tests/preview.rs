//! M3a Task 8: `Session::preview` mips equal the same mips of the DDS `run` writes, byte for byte.
use encoders_sys::Bc1Model;
use engine::api::*;
use engine::config::Target;
use engine::dds::read_dds;
use engine::events::*;
use engine::overrides::parse_set;
use engine::policy::{JobStatus, ResolvedJob};
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
