//! v2 artist checks, Task 2 Step 6: how often each check fires on the owner's 10k test set (thresholds are tuned
//! against warning spam). Owner data: `#[ignore]`, gated on `SDC_TEXTURES_10K`. Run with
//! `cargo test -p sdc-gui --release --test checks_corpus -- --ignored --nocapture`.
use engine::api::*;
use engine::config::{Category, Subtype, Target};
use engine::events::*;
use engine::session::AnalysisCache;
use sdc_gui::model::checks::{texture_checks, HeightNeutral, RangeRule};
use sdc_gui::settings::ChecksSettings;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

/// The value at each of the quantiles in `qs` (nearest rank); empty for an empty list.
fn quantiles(mut v: Vec<f64>, qs: &[f64]) -> Vec<(f64, f64)> {
    v.retain(|x| x.is_finite());
    v.sort_by(f64::total_cmp);
    if v.is_empty() {
        return vec![];
    }
    qs.iter()
        .map(|&q| (q, v[((v.len() - 1) as f64 * q).round() as usize]))
        .collect()
}

#[test]
#[ignore = "owner data: set SDC_TEXTURES_10K"]
fn checks_counts_on_textures_10k() {
    let Some(root) = std::env::var_os("SDC_TEXTURES_10K").map(PathBuf::from) else {
        eprintln!("SDC_TEXTURES_10K unset: skipped");
        return;
    };
    let mut c = AnalysisCache::default();
    c.checks = true;
    let out = std::env::temp_dir().join("sdc-checks-corpus-out");
    let plan = Engine::new()
        .unwrap()
        .plan_cached(
            BatchRequest::new(vec![root], out, Target::Cs),
            &Reporter::new(Arc::new(NullSink), "corpus"),
            &CancelToken::default(),
            &c,
        )
        .unwrap();
    let on = ChecksSettings::default();
    let mut codes: BTreeMap<&str, usize> = BTreeMap::new();
    let (mut normals, mut vanilla_n) = (0usize, 0usize);
    let (mut curl_ratio, mut steep, mut albedo) = (vec![], vec![], vec![]);
    for job in &plan.jobs {
        let Some(a) = c.analysis(job) else { continue };
        let Some(s) = &a.checks else { continue };
        let st = job.profile.subtype;
        if st.category() == Category::Normal {
            normals += 1;
            if let Some(k) = s.curl.filter(|k| k.as_stored > 0.0) {
                curl_ratio.push(k.flipped / k.as_stored);
            }
        }
        vanilla_n += usize::from(st == Subtype::Normal);
        if let Some(x) = s.steep {
            steep.push(x);
        }
        if s.texels > 0 {
            albedo.push(RangeRule::AlbedoLuma.count(s) as f64 / s.texels as f64);
        }
        for w in texture_checks(job, &a, HeightNeutral::Half, &on) {
            *codes.entry(w.code.as_str()).or_default() += 1;
        }
    }
    println!(
        "jobs {}, normals {normals}, vanilla _n {vanilla_n}",
        plan.jobs.len()
    );
    for (k, n) in &codes {
        println!("{k}: {n}");
    }
    let qs = [0.01, 0.05, 0.25, 0.5, 0.75, 0.95, 0.99];
    println!("curl flipped/as_stored: {:?}", quantiles(curl_ratio, &qs));
    println!("steep share: {:?}", quantiles(steep, &qs));
    println!(
        "albedo-luma out-of-range share (all jobs): {:?}",
        quantiles(albedo, &qs)
    );
}
