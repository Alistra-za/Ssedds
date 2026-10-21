//! Owner-data report for `from_normal::generate` (v2 Track B, Note CO). Read-only; `#[ignore]`.
use engine::from_normal::{generate, HeightOpts, Opts, Tiling};
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
