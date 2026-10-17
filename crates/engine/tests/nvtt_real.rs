//! M5 Task 9 (DECISIONS Note NV): the NVTT backend against the real NVTT 3.2.5 at the default install path and the
//! bench corpus (`SDC_BENCH_ROOT`, required). Owner's machine only:
//! `cargo test --release -p engine --test nvtt_real -- --ignored --nocapture --test-threads 1`.
use encoders_sys::Bc1Model;
use engine::api::*;
use engine::dds::read_dds;
use engine::decode::{decode_level, Decoded};
use engine::events::*;
use engine::overrides::parse_set;
use engine::policy::AcceptSuggestions;
use engine::session::Session;
use engine::validate::psnr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;

fn bench_root() -> PathBuf {
    std::env::var_os("SDC_BENCH_ROOT")
        .map(PathBuf::from)
        .expect("set SDC_BENCH_ROOT to the bench corpus")
}

/// `column` of the bench results row `variant,<format>,<set>,<image>,...`.
fn bench_value(csv: &str, variant: &str, set: &str, image: &str, column: &str) -> f64 {
    let text = std::fs::read_to_string(bench_root().join("results").join(csv)).unwrap();
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().unwrap().split(',').collect();
    let col = header.iter().position(|h| *h == column).unwrap();
    let row: Vec<&str> = lines
        .map(|l| l.split(',').collect::<Vec<_>>())
        .find(|f| f[0] == variant && f[2] == set && f[3] == image)
        .unwrap_or_else(|| panic!("no {variant} {set}/{image} row in {csv}"));
    row[col].parse().unwrap()
}

/// Runs one batch over `dir/in` (recursive, Overwrite) and returns the plan's single job output and the run time.
fn run(dir: &Path, sets: &[&str]) -> (PathBuf, f64) {
    let mut req = BatchRequest::new(
        vec![dir.join("in")],
        dir.join("out"),
        engine::config::Target::Cs,
    );
    req.overrides = sets.iter().map(|s| parse_set(s).unwrap()).collect();
    req.on_existing = OnExisting::Overwrite;
    req.recursive = true;
    req.accept = AcceptSuggestions::High;
    // Explicit: under `cargo test` the target folder, which holds the fake nvcompress, is on PATH.
    req.nvtt = Some(engine::nvtt::NVTT_DEFAULT.into());
    let s = Session::new(Bc1Model::Ideal).unwrap();
    let rep = Reporter::new(Arc::new(NullSink), "t");
    let plan = s.plan(req, &rep, &CancelToken::default()).unwrap();
    if sets.contains(&"encoder.backend=nvtt") {
        real_nvtt(&plan);
    }
    assert_eq!(plan.jobs.len(), 1, "{:?} {:?}", plan.failed, plan.skipped);
    let job = plan.jobs[0].clone();
    assert_eq!(
        job.status,
        engine::policy::JobStatus::Ready,
        "{:?}",
        job.warnings
    );
    let t = Instant::now();
    let sum = s.run(plan, &rep, &CancelToken::default()).unwrap();
    let secs = t.elapsed().as_secs_f64();
    assert_eq!((sum.converted, sum.failed), (1, 0), "{sum:?}");
    (job.output, secs)
}

/// The batch probed the real NVTT 3.2.5, not the fake.
fn real_nvtt(plan: &Plan) {
    match &plan.nvtt {
        Some(engine::nvtt::NvttProbe::Found(t)) => {
            assert_eq!(t.path, Path::new(engine::nvtt::NVTT_DEFAULT));
            assert!(t.tested(), "{t:?}");
        }
        p => panic!("NVTT not probed or not found: {p:?}"),
    }
}

/// Every level of `dds` decoded to 8-bit RGBA, mip 0 first.
fn levels_ldr(dds: &Path) -> Vec<(u32, u32, Vec<[u8; 4]>)> {
    let f = read_dds(&std::fs::read(dds).unwrap()).unwrap();
    f.levels
        .iter()
        .map(
            |(w, h, r)| match decode_level(f.format, *w, *h, &f.data[r.clone()], None).unwrap() {
                Decoded::Ldr(px) => (*w, *h, px),
                d => panic!("not LDR: {d:?}"),
            },
        )
        .collect()
}

fn source_rgba8(p: &Path) -> Vec<[u8; 4]> {
    image::open(p)
        .unwrap()
        .to_rgba8()
        .pixels()
        .map(|p| p.0)
        .collect()
}

fn copy_in(dir: &Path, src: &Path, name: &str) -> PathBuf {
    let to = dir.join("in").join(name);
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    std::fs::copy(src, &to).unwrap();
    to
}

/// 2x2 box filter on 8-bit texels, rounding half up; odd edges clamp (Spike S7's `box_down_rgba8`).
fn box_down(px: &[[u8; 4]], w: u32, h: u32) -> (Vec<[u8; 4]>, u32, u32) {
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let mut out = Vec::with_capacity((nw * nh) as usize);
    for y in 0..nh {
        for x in 0..nw {
            let mut acc = [0u32; 4];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let p = px[((y * 2 + dy).min(h - 1) * w + (x * 2 + dx).min(w - 1)) as usize];
                for c in 0..4 {
                    acc[c] += p[c] as u32;
                }
            }
            out.push(acc.map(|a| ((a + 2) / 4) as u8));
        }
    }
    (out, nw, nh)
}

/// (a) mip 0 of `rgb_real/Bricks104` through the engine on NVTT equals the bench's direct `nvtt_production` encode,
/// every level decodes alpha 255; (c) the engine run takes at most 10% longer than direct per-level nvcompress
/// processes (`-nocuda`, as the backend runs them) over the same chain shape.
#[test]
#[ignore = "real NVTT 3.2.5 and the bench corpus; about a minute"]
fn bc7_parity_and_overhead() {
    let d = tempfile::tempdir().unwrap();
    let src = bench_root().join("images/rgb_real/Bricks104.png");
    copy_in(d.path(), &src, "textures/a/bricks.png");
    let (out, engine_secs) = run(
        d.path(),
        &[
            "subtype=diffuse",
            "alpha.policy=remove",
            "encoder.backend=nvtt",
        ],
    );
    let levels = levels_ldr(&out);
    let p = psnr(&source_rgba8(&src), &levels[0].2, &[0, 1, 2]);
    let e = bench_value(
        "results.csv",
        "nvtt_production",
        "rgb_real",
        "Bricks104",
        "rgb_psnr",
    );
    println!(
        "(a) mip 0 RGB PSNR {p:.6} dB, direct nvtt_production {e:.6} dB (delta {:+.6})",
        p - e
    );
    assert!((p - e).abs() <= 0.0001, "{p} vs {e}");
    for (i, (w, h, px)) in levels.iter().enumerate() {
        assert!(
            px.iter().all(|t| t[3] == 255),
            "mip {i} ({w}x{h}) alpha not 255"
        );
    }

    let nv = Path::new(engine::nvtt::NVTT_DEFAULT);
    let (mut px, mut w, mut h) = (source_rgba8(&src), levels[0].0, levels[0].1);
    let mut direct = 0.0;
    for l in 0.. {
        let png = d.path().join(format!("direct{l}.png"));
        let flat: Vec<u8> = px.iter().flatten().copied().collect();
        image::save_buffer(&png, &flat, w, h, image::ColorType::Rgba8).unwrap();
        let t = Instant::now();
        let st = Command::new(nv)
            .args([
                "-silent",
                "-nomips",
                "-production",
                "-bc7",
                "-noalpha",
                "-nocuda",
            ])
            .arg(&png)
            .arg(d.path().join(format!("direct{l}.dds")))
            .status()
            .unwrap();
        direct += t.elapsed().as_secs_f64();
        assert!(st.success());
        if (w, h) == (1, 1) {
            break;
        }
        (px, w, h) = box_down(&px, w, h);
    }
    println!(
        "(c) engine run {engine_secs:.2} s, direct per-level nvcompress {direct:.2} s (ratio {:.3})",
        engine_secs / direct
    );
    assert!(
        engine_secs <= direct * 1.10,
        "{engine_secs} > 1.10 x {direct}"
    );
}

/// (b) one `hdr_real` image on BC6H through the engine on NVTT passes validation with peak ratio >= 0.95.
#[test]
#[ignore = "real NVTT 3.2.5 and the bench corpus"]
fn bc6h_keeps_the_peak() {
    let d = tempfile::tempdir().unwrap();
    let f16 = std::fs::read(bench_root().join("images2/hdr_real/studio_small_08.f16")).unwrap();
    let u = |i: usize| u32::from_le_bytes(f16[i..i + 4].try_into().unwrap());
    let (w, h) = (u(4), u(8));
    let rgb: Vec<f32> = f16[12..]
        .chunks(8)
        .flat_map(|t| {
            (0..3).map(move |c| half::f16::from_le_bytes([t[c * 2], t[c * 2 + 1]]).to_f32())
        })
        .collect();
    let p = d.path().join("in/textures/pbr/studio_g.tif");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    image::DynamicImage::ImageRgb32F(image::ImageBuffer::from_raw(w, h, rgb).unwrap())
        .save(&p)
        .unwrap();
    let mut req = BatchRequest::new(
        vec![d.path().join("in")],
        d.path().join("out"),
        engine::config::Target::Cs,
    );
    req.overrides = vec![parse_set("encoder.backend=nvtt").unwrap()];
    req.recursive = true;
    req.accept = AcceptSuggestions::High;
    // Explicit: under `cargo test` the target folder, which holds the fake nvcompress, is on PATH.
    req.nvtt = Some(engine::nvtt::NVTT_DEFAULT.into());
    let s = Session::new(Bc1Model::Ideal).unwrap();
    let events = Arc::new(VecSink::default());
    let rep = Reporter::new(events.clone(), "t");
    let plan = s.plan(req, &rep, &CancelToken::default()).unwrap();
    real_nvtt(&plan);
    assert_eq!(
        plan.jobs[0].profile.format,
        engine::config::Format::Bc6hUf16
    );
    assert_eq!(
        plan.jobs[0].profile.encoder.backend,
        engine::config::Backend::Nvtt
    );
    let sum = s.run(plan, &rep, &CancelToken::default()).unwrap();
    assert_eq!(
        (sum.converted, sum.failed, sum.validation_failed),
        (1, 0, 0),
        "{sum:?}"
    );
    let done = events
        .take()
        .into_iter()
        .find_map(|e| match e.kind {
            EventKind::FileDone { validation, .. } => Some(validation),
            _ => None,
        })
        .unwrap();
    let ratio = done.bc6h_peak_ratio.unwrap();
    println!(
        "(b) studio_small_08 BC6H on NVTT: peak ratio {ratio:.4}, passed {}",
        done.passed
    );
    assert!(done.passed && ratio >= 0.95, "{done:?}");
}

/// (d) DECISIONS Note NV4: a texture that keeps alpha is encoded with `-alpha`, so NVTT spends bits on it. Mip-0 alpha
/// PSNR on NVTT must be within 3 dB of the built-in bc7e's on the same `alpha_real` image.
#[test]
#[ignore = "real NVTT 3.2.5 and the bench corpus; about a minute"]
fn kept_alpha_is_encoded() {
    let src = bench_root().join("images/alpha_real/Fingerprints009.png");
    let a_psnr = |backend: &str| {
        let d = tempfile::tempdir().unwrap();
        copy_in(d.path(), &src, "textures/a/prints.png");
        let (out, _) = run(
            d.path(),
            &[
                "subtype=diffuse",
                "alpha.policy=blend",
                &format!("encoder.backend={backend}"),
            ],
        );
        psnr(&source_rgba8(&src), &levels_ldr(&out)[0].2, &[3])
    };
    let (nvtt, bc7e) = (a_psnr("nvtt"), a_psnr("bc7e"));
    let bench = bench_value(
        "results.csv",
        "nvtt_production",
        "alpha_real",
        "Fingerprints009",
        "a_psnr",
    );
    println!("(d) Fingerprints009 alpha PSNR: NVTT {nvtt:.4} dB, bc7e {bc7e:.4} dB, bench nvtt_production {bench:.4} dB");
    assert!(nvtt >= bc7e - 3.0, "NVTT alpha {nvtt} vs bc7e {bc7e}");
}
