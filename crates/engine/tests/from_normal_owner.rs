//! Owner-data report for `from_normal::generate` (v2 Track B, Note CO). Read-only; `#[ignore]`.
use engine::from_normal::{generate, AoOpts, HeightOpts, Opts, Tiling, AO_MAX};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::time::Instant;

fn normals(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if out.len() >= 20 {
            return;
        }
        if p.is_dir() {
            normals(&p, out);
        } else {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if name.ends_with("_n.png") || name.ends_with("_n.dds") {
                out.push(p);
            }
        }
    }
}

#[test]
#[ignore] // SDC_PBR_RESOURCE (read-only owner folder)
fn owner_height_timing_and_fit() {
    let Ok(root) = std::env::var("SDC_PBR_RESOURCE") else {
        eprintln!("SDC_PBR_RESOURCE unset; skipped");
        return;
    };
    let mut files = Vec::new();
    normals(Path::new(&root), &mut files);
    // The PBR resource itself ships no textures (JSON and an ESP only); point the variable at a texture folder.
    if files.is_empty() {
        eprintln!("no *_n.png / *_n.dds under {root}; skipped");
        return;
    }
    let opts = Opts {
        size: 1024,
        tiling: Tiling::Auto,
        relief: 0.3,
        green: None,
        height: Some(HeightOpts::default()),
        ao: None,
        curvature: false,
    };
    let (mut ms, mut unsure, mut done) = (Vec::new(), 0, 0);
    for f in &files {
        let (img, _) = engine::load::load_image(f).expect("loads");
        let t0 = Instant::now();
        // Never above the normal's own size (Note CO).
        let opts = Opts {
            size: opts.size.min(img.width.max(img.height)),
            ..opts.clone()
        };
        let r = generate(&img, &opts, &AtomicU32::new(0), &AtomicBool::new(false));
        let el = t0.elapsed().as_secs_f64() * 1000.0;
        let Ok(g) = r else {
            println!(
                "{}: {}x{} skipped: {:?}",
                f.display(),
                img.width,
                img.height,
                r.err()
            );
            continue;
        };
        let height_finite = g
            .height
            .as_ref()
            .is_some_and(|h| h.px.iter().all(|v| v.is_finite()));
        let r = g.report;
        println!(
            "{}: {}x{} {:.0} ms fit {:.2}/{:.2} deg tilt {:.2} deg {} ratio {:.2}{} green {:?} sure {} margin {:.2}",
            f.display(),
            img.width,
            img.height,
            el,
            r.fit_mean_deg,
            r.fit_p95_deg,
            r.tilt_deg,
            if r.wrap { "wrap" } else { "mirror" },
            r.seam_ratio,
            if r.ambiguous { " (ambiguous)" } else { "" },
            r.green,
            r.green_sure,
            r.green_margin
        );
        // P22: a NaN path fails, not just prints.
        // seam_ratio is not asserted: it is legitimately infinite for a flat interior (`detect_tiling`).
        for v in [
            r.fit_mean_deg,
            r.fit_p95_deg,
            r.tilt_deg,
            r.green_margin,
            r.clamped_share,
        ] {
            assert!(v.is_finite(), "{}: non-finite value in {r:?}", f.display());
        }
        assert!(height_finite, "{}: a non-finite height texel", f.display());
        ms.push(el);
        unsure += usize::from(!r.green_sure);
        done += 1;
    }
    ms.sort_by(f64::total_cmp);
    println!(
        "{done} generated; median {:.0} ms; unsure green {unsure}/{done}",
        ms.get(ms.len() / 2).copied().unwrap_or(0.0)
    );
}

/// The set's `_rmaos` image next to a `*_n.*` normal (same base).
fn rmaos_of(normal: &Path) -> Option<PathBuf> {
    let name = normal.file_name()?.to_string_lossy().to_lowercase();
    let base = name[..name.rfind("_n.")?].to_string();
    std::fs::read_dir(normal.parent()?)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            let n = p
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            n.strip_prefix(base.as_str())
                .is_some_and(|rest| rest.starts_with("_rmaos.") && !rest.ends_with(".dds"))
        })
}

#[test]
#[ignore] // SDC_PBR_RESOURCE (read-only owner folder)
fn owner_generated_ao_against_baked_rmaos_b() {
    let Ok(root) = std::env::var("SDC_PBR_RESOURCE") else {
        eprintln!("SDC_PBR_RESOURCE unset; skipped");
        return;
    };
    let mut files = Vec::new();
    normals(Path::new(&root), &mut files);
    let pairs: Vec<(PathBuf, PathBuf)> = files
        .into_iter()
        .filter_map(|n| rmaos_of(&n).map(|r| (n, r)))
        .collect();
    if pairs.is_empty() {
        eprintln!("no *_n.* with a *_rmaos.* image under {root}; skipped");
        return;
    }
    let (mut ms, mut rs, mut done) = (Vec::new(), Vec::new(), 0);
    for (nf, rf) in &pairs {
        let (img, _) = engine::load::load_image(nf).expect("loads");
        let (rm, _) = engine::load::load_image(rf).expect("loads");
        let (lo, hi) = rm
            .px
            .iter()
            .fold((f32::MAX, f32::MIN), |(a, c), p| (a.min(p[2]), c.max(p[2])));
        if hi - lo < 2.0 / 255.0 {
            println!("{}: B has no content; skipped", rf.display());
            continue;
        }
        let opts = Opts {
            size: img.width.max(img.height).min(AO_MAX),
            tiling: Tiling::Auto,
            relief: 0.3,
            green: None,
            height: None,
            ao: Some(AoOpts::default()),
            curvature: false,
        };
        let t0 = Instant::now();
        let r = generate(&img, &opts, &AtomicU32::new(0), &AtomicBool::new(false));
        let el = t0.elapsed().as_secs_f64() * 1000.0;
        let Ok(g) = r else {
            println!("{}: skipped: {:?}", nf.display(), r.err());
            continue;
        };
        let ao = g.ao.expect("AO requested");
        // The baked B at the AO's size (area down or bilinear up).
        let b: Vec<f64> = engine::rnm::blend::resample(rm, (ao.width, ao.height))
            .px
            .iter()
            .map(|p| f64::from(p[2]))
            .collect();
        let a: Vec<f64> = ao.px.iter().map(|&v| f64::from(v)).collect();
        let n = a.len() as f64;
        let (ma, mb) = (a.iter().sum::<f64>() / n, b.iter().sum::<f64>() / n);
        let (mut sab, mut saa, mut sbb) = (0.0, 0.0, 0.0);
        for (x, y) in a.iter().zip(&b) {
            sab += (x - ma) * (y - mb);
            saa += (x - ma) * (x - ma);
            sbb += (y - mb) * (y - mb);
        }
        // A constant AO (a flat normal) has no correlation: reported as 0, not NaN.
        let pearson = if saa == 0.0 || sbb == 0.0 {
            0.0
        } else {
            sab / (saa * sbb).sqrt()
        };
        let rep = &g.report;
        let dark = rep.ao_dark_share.unwrap_or(f32::NAN);
        println!(
            "{}: {}x{} {:.0} ms r {:.3} bias {:+.3} mean {:.3} (B {:.3}) dark {:.4} {} green {:?}",
            nf.display(),
            ao.width,
            ao.height,
            el,
            pearson,
            ma - mb,
            ma,
            mb,
            dark,
            if rep.wrap { "wrap" } else { "mirror" },
            rep.green
        );
        // P22: every printed value is finite, so a NaN path fails.
        for v in [pearson, ma, mb, el, f64::from(dark)] {
            assert!(
                v.is_finite(),
                "{}: non-finite value in {rep:?}",
                nf.display()
            );
        }
        assert!(
            rep.ao_mean.is_some_and(f32::is_finite),
            "{}: {rep:?}",
            nf.display()
        );
        assert!(
            ao.px.iter().all(|v| v.is_finite()),
            "{}: a non-finite AO texel",
            nf.display()
        );
        ms.push(el);
        rs.push(pearson);
        done += 1;
    }
    ms.sort_by(f64::total_cmp);
    rs.sort_by(f64::total_cmp);
    let med = |v: &[f64]| v.get(v.len() / 2).copied().unwrap_or(0.0);
    println!(
        "{done} sets; median r {:.3}; median {:.0} ms",
        med(&rs),
        med(&ms)
    );
}
