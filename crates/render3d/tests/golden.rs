//! Golden replay (v2 Phase D, D6b): every profile-A subject replayed deferred and compared per MRT and on the
//! composite MainRW. Thresholds (D-P18): over == 0 on RT0-RT7 and on main over the clean sub-mask; the subject
//! classes must match tests/fixtures/golden-baseline.txt (D-P20).
use render3d::golden::{self, Diff, Noise};
use render3d::{load_fixture, Renderer};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

const PROFILES: &[&str] = &["A", "A_RT", "A_RT2"];
const TWO_SIDED: &str = "two-sided alpha-tested: no captured depth (capture not kept)";

fn bits(v: &[f32]) -> Vec<u32> {
    v.iter().map(|x| x.to_bits()).collect()
}

/// Masked pixels whose 4 channels are bit-identical in `a` and `b`.
fn same(a: &[f32], b: &[f32], m: &[bool]) -> usize {
    (0..m.len())
        .filter(|&p| m[p] && bits(&a[4 * p..4 * p + 4]) == bits(&b[4 * p..4 * p + 4]))
        .count()
}

fn pct(x: usize, of: usize) -> f32 {
    if of == 0 {
        0.0
    } else {
        100.0 * x as f32 / of as f32
    }
}

/// `over%/max (bit-identical%, evidence px)`.
fn cell(d: &Diff, ident: usize, evidence: usize) -> String {
    format!(
        "{:.2}%/{:.3e} ({:.1}%, {evidence})",
        100.0 * d.over,
        d.max_abs,
        pct(ident, d.pixels as usize)
    )
}

#[derive(Default)]
struct Worst {
    subjects_over: usize,
    worst: Option<(String, f32, f32)>,
}

impl Worst {
    fn add(&mut self, who: &str, d: &Diff) {
        if d.over > 0.0 {
            self.subjects_over += 1;
        }
        let o = 100.0 * d.over;
        if self
            .worst
            .as_ref()
            .is_none_or(|w| (o, d.max_abs) > (w.1, w.2))
        {
            self.worst = Some((who.to_string(), o, d.max_abs));
        }
    }
    fn line(&self, label: &str) -> String {
        match &self.worst {
            Some((s, o, m)) => format!(
                "{label}: {} subjects with over > 0; worst {s} over {o:.2}% max {m:.3e}",
                self.subjects_over
            ),
            None => format!("{label}: no subjects"),
        }
    }
}

#[test]
#[ignore]
fn replay_matches_capture_per_mrt() {
    let (Ok(root), Ok(shaders)) = (std::env::var("SDC_GOLDEN"), std::env::var("SDC_CS_SHADERS"))
    else {
        println!("skipped: set SDC_GOLDEN and SDC_CS_SHADERS");
        return;
    };
    let root = Path::new(&root);
    let cache = tempfile::tempdir().unwrap();
    let fp = render3d::locate::fingerprint(Path::new(&shaders), None).unwrap();
    let mut ren = Renderer::new(fp, cache.path().to_path_buf()).unwrap();
    let noise = Noise::load(
        &root.join("noise_Skyrim_AE_1.6.1170_frame207657_Skyrim_AE_1.6.1170_frame207658.json"),
    )
    .unwrap();
    let subjects = golden::subjects(root, PROFILES).unwrap();

    let mut table = String::from(
        "capture | subject | profile | masked px | RT0..RT7 over%/max (bit-identical%, evidence px) | main [mask] | main [clean] (px) | warnings\n",
    );
    let mut skipped: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    let (mut run, mut nonfinite, mut failures) = (0usize, Vec::new(), Vec::new());
    let (mut outcomes, mut no_clean) = (Vec::new(), Vec::new());
    let mut rt_worst: Vec<Worst> = (0..8).map(|_| Worst::default()).collect();
    let (mut main_mask, mut main_clean) = (Worst::default(), Worst::default());

    for s in &subjects {
        let name = s.dir.file_name().unwrap().to_string_lossy().into_owned();
        let who = format!("{}/{name}", s.capture);
        let mut skip = |code: &'static str, detail: String| {
            outcomes.push(format!("{who} {code}"));
            skipped.entry(code).or_default().push(detail);
        };
        // D-P17: an overlay mask means the capture never changed (the game rejected the draw by depth).
        let mrt: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(s.dir.join("mrt.json")).unwrap())
                .unwrap();
        if mrt["maskSource"] == "overlay" {
            skip("overlay-mask", who.clone());
            continue;
        }
        // No crop: the extractor wrote no mask at all (the draw rasterized nothing).
        if mrt["crop"].is_null() {
            skip("empty-mask", who.clone());
            continue;
        }
        let r = match load_fixture(&s.dir) {
            Ok(r) => r,
            Err(e) if e.contains("defines null") => {
                skip("defines-null", who.clone());
                continue;
            }
            Err(e) => {
                skip("load-error", format!("{who}: {e}"));
                continue;
            }
        };
        // D-P21: RUN or empty is decided by the real mask.bin population, not by metadata.
        let n = (r.crop[2] * r.crop[3]) as usize;
        let mask = golden::read_mask(&s.dir.join("mask.bin"), n).unwrap();
        let masked = mask.iter().filter(|m| **m).count();
        if masked == 0 {
            skip("empty-mask", who.clone());
            continue;
        }
        if r.blended {
            skip("blended", who.clone());
            continue;
        }
        let Some(rc) = r.composite.as_ref() else {
            skip("no-composite-data", who.clone());
            continue;
        };
        let out = match ren.render_replay(&r, true) {
            Ok(o) => o,
            Err(e) => {
                skip("replay-error", format!("{who}: {e}"));
                continue;
            }
        };
        // D6c: captured depth stands in for the game's prepass, so only subjects without it are skipped (D-P19).
        let two_sided = r.two_sided_alpha_tested() && r.depth.is_none();
        let mut clean_px = None;
        let mut fails = Vec::new();
        let mut row = format!("{} | {} | {} | {masked} |", s.capture, name, s.profile);
        for (i, rt) in out.mrt.iter().enumerate() {
            let file = format!("mrt{i}.f32");
            let after = golden::read_f32(&s.dir.join(&file), n * 4).unwrap();
            let (_, max) = noise
                .limit(&name, &file)
                .unwrap_or_else(|| panic!("{who}: no noise limit for {file}"));
            let d = golden::diff_tol(
                rt,
                &after,
                &mask,
                golden::format_tol(&r.formats[i]).unwrap(),
                max,
            );
            if rt.iter().any(|v| !v.is_finite()) {
                nonfinite.push(format!("{who} RT{i}"));
            }
            // An RT the draw left at its before-values proves little: count where the capture changed.
            let evidence = masked - same(&after, &r.before[i], &mask);
            let _ = write!(row, " {}", cell(&d, same(rt, &after, &mask), evidence));
            if d.over > 0.0 {
                fails.push(format!(
                    "{who} RT{i}: over {:.2}% max {:.3e}",
                    100.0 * d.over,
                    d.max_abs
                ));
            }
            if !two_sided {
                rt_worst[i].add(&who, &d);
            }
        }
        match out.main.as_ref() {
            Some(main) => {
                let after = golden::read_f32(&s.dir.join("mainrw_after.f32"), n * 4).unwrap();
                let (_, max) = noise
                    .limit(&name, "mainrw_after.f32")
                    .unwrap_or_else(|| panic!("{who}: no noise limit for mainrw_after.f32"));
                if main.iter().any(|v| !v.is_finite()) {
                    nonfinite.push(format!("{who} main"));
                }
                // D4: pixels overdrawn after the subject differ legitimately; the clean sub-mask drops them.
                let mrt0 = golden::read_f32(&s.dir.join("mrt0.f32"), n * 4).unwrap();
                let clean: Vec<bool> = (0..n)
                    .map(|p| {
                        mask[p]
                            && bits(&mrt0[4 * p..4 * p + 4]) == bits(&rc.before[4 * p..4 * p + 4])
                    })
                    .collect();
                for (label, m, w) in [
                    ("mask", &mask, &mut main_mask),
                    ("clean", &clean, &mut main_clean),
                ] {
                    let d = golden::diff_tol(
                        main,
                        &after,
                        m,
                        golden::format_tol("R16G16B16A16_FLOAT").unwrap(),
                        max,
                    );
                    let evidence = d.pixels as usize - same(&after, &rc.before, m);
                    let _ = write!(
                        row,
                        " | {} {}px",
                        cell(&d, same(main, &after, m), evidence),
                        d.pixels
                    );
                    if label == "clean" {
                        clean_px = Some(d.pixels);
                    }
                    if label == "clean" && d.over > 0.0 {
                        fails.push(format!(
                            "{who} main [clean]: over {:.2}% max {:.3e}",
                            100.0 * d.over,
                            d.max_abs
                        ));
                    }
                    if !two_sided {
                        w.add(&who, &d);
                    }
                }
            }
            None => {
                fails.push(format!("{who}: no composite output"));
                row.push_str(" | - | -");
            }
        }
        let depth = if r.depth.is_some() { " depth" } else { "" };
        let _ = writeln!(row, " | {}{depth}", out.warnings.len());
        print!("{row}");
        if two_sided {
            // Information only, so D6c can show the improvement.
            outcomes.push(format!("{who} two-sided-alpha-tested"));
            skipped
                .entry("two-sided-alpha-tested")
                .or_default()
                .push(row.trim_end().to_string());
        } else {
            // D-P21: an empty clean sub-mask checks nothing on the composite; its own class, MRTs still asserted.
            if clean_px == Some(0) {
                outcomes.push(format!("{who} no-clean-composite"));
                no_clean.push(who.clone());
            } else {
                outcomes.push(format!("{who} RUN"));
            }
            run += 1;
            failures.extend(fails);
            table.push_str(&row);
        }
    }

    let mut summary = format!(
        "\nsubjects {} (profiles {PROFILES:?}): run {run}, skipped {}\n",
        subjects.len(),
        skipped.values().map(Vec::len).sum::<usize>()
    );
    let _ = writeln!(
        summary,
        "run without a clean composite (no-clean-composite, MRTs asserted): {}",
        no_clean.len()
    );
    for w in &no_clean {
        let _ = writeln!(summary, "  {w}");
    }
    for (why, list) in &skipped {
        let why = if *why == "two-sided-alpha-tested" {
            TWO_SIDED
        } else {
            why
        };
        let _ = writeln!(summary, "skipped {why}: {}", list.len());
        for w in list {
            let _ = writeln!(summary, "  {w}");
        }
    }
    for (i, w) in rt_worst.iter().enumerate() {
        let _ = writeln!(summary, "{}", w.line(&format!("RT{i}")));
    }
    let _ = writeln!(summary, "{}", main_mask.line("main [mask]"));
    let _ = writeln!(summary, "{}", main_clean.line("main [clean]"));
    let _ = writeln!(summary, "non-finite: {nonfinite:?}");
    let _ = writeln!(summary, "failing (D-P18): {}", failures.len());
    for f in &failures {
        let _ = writeln!(summary, "  {f}");
    }
    print!("{summary}");
    table.push_str(&summary);
    let out = std::env::temp_dir().join("golden-replay.txt");
    std::fs::write(&out, &table).unwrap();
    println!("table written to {}", out.display());

    assert!(nonfinite.is_empty(), "non-finite output: {nonfinite:?}");
    // D-P18: zero over-limit pixels per RT and on the clean composite; main over the full mask is printed only.
    assert!(failures.is_empty(), "over-limit pixels: {failures:#?}");
    // D-P20: the subject list and each subject's class must match the tracked baseline exactly.
    outcomes.sort();
    let actual = std::env::temp_dir().join("golden-baseline.actual.txt");
    std::fs::write(&actual, outcomes.join("\n") + "\n").unwrap();
    let base_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/golden-baseline.txt");
    // Fail closed: a missing or empty baseline must never turn an empty run into a pass.
    let base = std::fs::read_to_string(&base_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", base_path.display()));
    let base: BTreeSet<&str> = base.lines().filter(|l| !l.trim().is_empty()).collect();
    assert!(!base.is_empty(), "{} is empty", base_path.display());
    assert!(
        !outcomes.is_empty(),
        "no golden subjects found under SDC_GOLDEN"
    );
    let now: BTreeSet<&str> = outcomes.iter().map(String::as_str).collect();
    let diff: Vec<String> = base
        .difference(&now)
        .map(|l| format!("- {l}"))
        .chain(now.difference(&base).map(|l| format!("+ {l}")))
        .collect();
    assert!(
        diff.is_empty(),
        "subjects differ from tests/fixtures/golden-baseline.txt (- baseline, + this run; this run's list is in {}):\n{}",
        actual.display(),
        diff.join("\n")
    );
}
