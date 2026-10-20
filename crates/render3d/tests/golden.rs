//! Golden replay (v2 Phase D, D6b): every profile-A subject replayed deferred and compared per MRT and on the
//! composite's MainRW. First run: prints the numbers; thresholds come with the D9 ruling.
use render3d::golden::{self, Diff, Noise};
use render3d::{load_fixture, Renderer};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

const PROFILES: &[&str] = &["A", "A_RT", "A_RT2"];

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
    let (mut run, mut nonfinite) = (0usize, Vec::new());
    let mut rt_worst: Vec<Worst> = (0..8).map(|_| Worst::default()).collect();
    let (mut main_mask, mut main_clean) = (Worst::default(), Worst::default());

    for s in &subjects {
        let name = s.dir.file_name().unwrap().to_string_lossy().into_owned();
        let who = format!("{}/{name}", s.capture);
        let r = match load_fixture(&s.dir) {
            Ok(r) => r,
            Err(e) if e.contains("defines null") => {
                skipped.entry("defines null").or_default().push(who);
                continue;
            }
            Err(e) => {
                skipped
                    .entry("load error")
                    .or_default()
                    .push(format!("{who}: {e}"));
                continue;
            }
        };
        if r.blended {
            skipped.entry("blended").or_default().push(who);
            continue;
        }
        let Some(rc) = r.composite.as_ref() else {
            skipped.entry("no composite data").or_default().push(who);
            continue;
        };
        let out = match ren.render_replay(&r, true) {
            Ok(o) => o,
            Err(e) => {
                skipped
                    .entry("replay error")
                    .or_default()
                    .push(format!("{who}: {e}"));
                continue;
            }
        };
        run += 1;
        let n = (r.crop[2] * r.crop[3]) as usize;
        let mask = golden::read_mask(&s.dir.join("mask.bin"), n).unwrap();
        let masked = mask.iter().filter(|m| **m).count();
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
            rt_worst[i].add(&who, &d);
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
                for (m, w) in [(&mask, &mut main_mask), (&clean, &mut main_clean)] {
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
                    w.add(&who, &d);
                }
            }
            None => row.push_str(" | - | -"),
        }
        let _ = writeln!(row, " | {}", out.warnings.len());
        print!("{row}");
        table.push_str(&row);
    }

    let mut summary = format!(
        "\nsubjects {} (profiles {PROFILES:?}): run {run}, skipped {}\n",
        subjects.len(),
        skipped.values().map(Vec::len).sum::<usize>()
    );
    for (why, list) in &skipped {
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
    print!("{summary}");
    table.push_str(&summary);
    let out = std::env::temp_dir().join("golden-replay.txt");
    std::fs::write(&out, &table).unwrap();
    println!("table written to {}", out.display());

    assert!(nonfinite.is_empty(), "non-finite output: {nonfinite:?}");
    assert!(run >= 20, "only {run} subjects ran");
}
