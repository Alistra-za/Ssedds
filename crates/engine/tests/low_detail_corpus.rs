//! v2 artist checks, Task 4 Step 6: check 3's detail ratio over the owner's 10k test set, measured on the chain
//! export builds (DECISIONS Note CI20). Owner data: `#[ignore]`, gated on `SDC_TEXTURES_10K`. Run with
//! `cargo test -p engine --release --test low_detail_corpus -- --ignored --nocapture`.
use engine::api::*;
use engine::checks::{detail_ratio, low_detail_eligible, LOW_DETAIL_RATIO};
use engine::config::Target;
use engine::events::*;
use rayon::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;

#[test]
#[ignore = "owner data: set SDC_TEXTURES_10K"]
fn low_detail_ratios_on_textures_10k() {
    let Some(root) = std::env::var_os("SDC_TEXTURES_10K").map(PathBuf::from) else {
        eprintln!("SDC_TEXTURES_10K unset: skipped");
        return;
    };
    assert!(root.is_dir(), "{} is not a folder", root.display());
    let out = std::env::temp_dir().join("sdc-low-detail-corpus-out");
    let plan = Engine::new()
        .unwrap()
        .plan(
            BatchRequest {
                recursive: true,
                ..BatchRequest::new(vec![root], out, Target::Cs)
            },
            &Reporter::new(Arc::new(NullSink), "corpus"),
            &CancelToken::default(),
        )
        .unwrap();
    let jobs: Vec<_> = plan
        .jobs
        .iter()
        .filter(|j| j.pack.is_none() && engine::checks::low_detail_profile_eligible(&j.profile))
        .collect();
    let mut rows: Vec<(f64, String, (u32, u32))> = jobs
        .par_iter()
        .filter_map(|job| {
            let p = &job.profile;
            let (img, info) = engine::load::load_image_as(&job.input, p.alpha.exr_straight).ok()?;
            let dims = (img.width, img.height);
            if !low_detail_eligible(p, dims) {
                return None;
            }
            let work = engine::ops::preprocess_with(img, p, engine::ops::SourceTransfer::of(&info));
            let chain = engine::mips::build_chain_with(&work, p, None);
            let r = detail_ratio(&chain)?;
            Some((r, job.input.display().to_string(), dims))
        })
        .collect();
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    eprintln!(
        "jobs {}, profile-eligible {}, measured {}",
        plan.jobs.len(),
        jobs.len(),
        rows.len()
    );
    let edges = [
        0.02, 0.05, 0.1, 0.15, 0.2, 0.25, 0.3, 0.4, 0.5, 0.75, 1.0, 2.0,
    ];
    let mut lo = 0.0;
    for e in edges {
        let n = rows.iter().filter(|r| r.0 >= lo && r.0 < e).count();
        eprintln!("[{lo:.2}, {e:.2}): {n}");
        lo = e;
    }
    eprintln!(
        "[{lo:.2}, inf): {}",
        rows.iter().filter(|r| r.0 >= lo).count()
    );
    for t in [0.05, 0.1, 0.15, 0.2, 0.25, 0.3] {
        eprintln!("below {t}: {}", rows.iter().filter(|r| r.0 < t).count());
    }
    eprintln!(
        "below LOW_DETAIL_RATIO ({LOW_DETAIL_RATIO}): {}",
        rows.iter().filter(|r| r.0 < LOW_DETAIL_RATIO).count()
    );
    for (r, path, (w, h)) in rows.iter().take(60) {
        eprintln!("{r:.4}\t{w}x{h}\t{path}");
    }
}
