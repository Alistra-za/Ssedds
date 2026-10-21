//! Track A, Task 13: the PBR → vanilla recipe defaults against same-author mods that ship both a PBR and a vanilla
//! variant (DECISIONS Note CN). Owner data: `#[ignore]`, gated on `SDC_TRACK_A_ARCHIVES`, which names a folder laid
//! out as `<root>/<author>/vanilla/…` and `<root>/<author>/pbr/…`, each holding that variant's extracted archive(s).
//! Run with `cargo test -p engine --release --test derive_calibration -- --ignored --nocapture`.
//!
//! A pair is a PBR `X.dds` with an `X_rmaos.dds` sibling under `textures/pbr/`, and the vanilla `X.dds` (and `X_n.dds`)
//! at the same path under `textures/`, in the same option folder (or the only one with that path). Pairs whose
//! sizes differ are skipped, as a derived set's are (TA22). The derived maps use `LegacyParams::default()` through the
//! shipped per-texel recipe (`recipe::diffuse`, `recipe::spec_alpha`) on values as stored. Nothing is written.
//!
//! It asserts (05 R2.3, O5): the median over authors of the per-author median diffuse R² (shipped against derived)
//! ≥ 0.8; the median over authors of the fitted `_n` k in `a = k·g·AO` within [0.45, 0.83]; and, per author with
//! landscape, the mean derived landscape `_n` alpha within ×0.7–1.4 of the shipped mean (every landscape `_n`,
//! opaque ones included). The brightness k (owner answer 1: 0.9) is printed as a fit, never asserted.
use engine::derive::recipe::{diffuse, spec_alpha, DiffuseFlavour, LegacyParams};
use engine::load::load_image;
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Lower-case `/` path components.
fn parts(p: &Path) -> Vec<String> {
    p.components()
        .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
        .collect()
}

/// (option prefix, path below `textures/` or `textures/pbr/`, is under `pbr/`) of a `.dds` file.
fn key(rel: &Path) -> Option<(String, String, bool)> {
    let p = parts(rel);
    let t = p.iter().position(|c| c == "textures")?;
    let pbr = p.get(t + 1).is_some_and(|c| c == "pbr");
    let tail = p[t + 1 + usize::from(pbr)..].join("/");
    Some((p[..t].join("/"), tail, pbr))
}

fn dds_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            dds_files(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("dds")) {
            out.push(p);
        }
    }
}

struct Pair {
    albedo: PathBuf,
    rmaos: PathBuf,
    diffuse: PathBuf,
    normal: Option<PathBuf>,
    landscape: bool,
}

fn pairs(author: &Path) -> Vec<Pair> {
    let index = |side: &str| {
        let root = author.join(side);
        let mut files = vec![];
        dds_files(&root, &mut files);
        files
            .into_iter()
            .filter_map(|f| {
                let k = key(f.strip_prefix(&root).ok()?)?;
                Some(((k.0, k.1), (k.2, f)))
            })
            .collect::<HashMap<_, _>>()
    };
    let (pbr, van) = (index("pbr"), index("vanilla"));
    let mut by_tail: HashMap<&str, Vec<&PathBuf>> = HashMap::new();
    for ((_, tail), (_, f)) in &van {
        by_tail.entry(tail).or_default().push(f);
    }
    let vanilla = |opt: &str, tail: &str| {
        van.get(&(opt.to_string(), tail.to_string()))
            .map(|(_, f)| f.clone())
            .or_else(|| match by_tail.get(tail).map(Vec::as_slice) {
                Some([only]) => Some((*only).clone()),
                _ => None,
            })
    };
    let mut out = vec![];
    for ((opt, tail), (is_pbr, albedo)) in &pbr {
        let Some(stem) = tail.strip_suffix(".dds") else {
            continue;
        };
        if !is_pbr || stem.ends_with("_rmaos") || stem.ends_with("_n") || stem.ends_with("_p") {
            continue;
        }
        let Some((_, rmaos)) = pbr.get(&(opt.clone(), format!("{stem}_rmaos.dds"))) else {
            continue;
        };
        let Some(diffuse) = vanilla(opt, tail) else {
            continue;
        };
        out.push(Pair {
            albedo: albedo.clone(),
            rmaos: rmaos.clone(),
            diffuse,
            normal: vanilla(opt, &format!("{stem}_n.dds")),
            landscape: tail.starts_with("landscape/"),
        });
    }
    out.sort_by(|a, b| a.albedo.cmp(&b.albedo));
    out
}

/// One pair's measurements; `None` fields when its `_n` is missing or a size differs.
#[derive(Default)]
struct Row {
    /// R² of the shipped diffuse RGB against the derived one.
    r2: f64,
    /// Free fit of `d = k·a·lerp(1, AO, s)` on dielectric texels, s per the defaults.
    k: f64,
    /// `_n` alpha: free k in `a = k·g·AO` (dielectric texels) and its correlation.
    n_k: Option<(f64, f64)>,
    /// Sums of the derived and shipped `_n` alpha, and the texel count.
    n_mean: Option<(f64, f64, f64)>,
}

fn corr(x: &[f64], y: &[f64]) -> f64 {
    let n = x.len() as f64;
    let (mx, my) = (x.iter().sum::<f64>() / n, y.iter().sum::<f64>() / n);
    let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
    for (a, b) in x.iter().zip(y) {
        sxy += (a - mx) * (b - my);
        sxx += (a - mx).powi(2);
        syy += (b - my).powi(2);
    }
    sxy / (sxx * syy).sqrt()
}

fn measure(pair: &Pair, p: &LegacyParams) -> Result<Row, String> {
    let load = |f: &Path| load_image(f).map(|(i, _)| i).map_err(|e| e.to_string());
    let (alb, rm, van) = (
        load(&pair.albedo)?,
        load(&pair.rmaos)?,
        load(&pair.diffuse)?,
    );
    let size = |i: &engine::image::Rgba32F| (i.width, i.height);
    if size(&alb) != size(&rm) || size(&alb) != size(&van) {
        return Err(format!(
            "sizes differ: {:?} {:?} {:?}",
            size(&alb),
            size(&rm),
            size(&van)
        ));
    }
    let flavour = if pair.landscape {
        DiffuseFlavour::Landscape
    } else {
        DiffuseFlavour::Vanilla
    };
    let s = if pair.landscape {
        p.ao_strength_landscape
    } else {
        p.ao_strength
    } as f32;
    let (mut ss_res, mut sum, mut sum2, mut n) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let (mut kxy, mut kxx) = (0.0f64, 0.0f64);
    for i in 0..alb.px.len() {
        let (a, m, v) = (alb.px[i], rm.px[i], van.px[i]);
        let (metal, ao) = (if pair.landscape { 0.0 } else { m[1] }, m[2]);
        let d = diffuse([a[0], a[1], a[2]], ao, metal, None, None, p, flavour);
        for c in 0..3 {
            let (vc, dc) = (f64::from(v[c]), f64::from(d[c]));
            ss_res += (vc - dc).powi(2);
            sum += vc;
            sum2 += vc * vc;
            n += 1.0;
            if metal < 0.1 {
                let b = f64::from(a[c] * (1.0 + (ao - 1.0) * s));
                kxy += vc * b;
                kxx += b * b;
            }
        }
    }
    let ss_tot = sum2 - sum * sum / n;
    let mut row = Row {
        r2: 1.0 - ss_res / ss_tot,
        k: kxy / kxx,
        ..Row::default()
    };
    if let Some(np) = &pair.normal {
        let nm = load(np)?;
        if size(&nm) != size(&alb) {
            return Err(format!("_n size {:?} vs {:?}", size(&nm), size(&alb)));
        }
        let (mut x, mut y) = (vec![], vec![]);
        let (mut der, mut ship) = (0.0, 0.0);
        for i in 0..alb.px.len() {
            let m = rm.px[i];
            let metal = if pair.landscape { 0.0 } else { m[1] };
            let shipped = f64::from(nm.px[i][3]);
            der += f64::from(spec_alpha(m[0], m[2], metal, p, pair.landscape));
            ship += shipped;
            if metal < 0.1 {
                x.push(f64::from((1.0 - m[0]) * m[2]));
                y.push(shipped);
            }
        }
        let xy: f64 = x.iter().zip(&y).map(|(a, b)| a * b).sum();
        let xx: f64 = x.iter().map(|a| a * a).sum();
        row.n_k = (xx > 0.0).then(|| (xy / xx, corr(&x, &y)));
        row.n_mean = Some((der, ship, alb.px.len() as f64));
    }
    Ok(row)
}

fn median(mut v: Vec<f64>) -> f64 {
    v.retain(|x| x.is_finite());
    assert!(!v.is_empty(), "median of nothing");
    v.sort_by(f64::total_cmp);
    let m = v.len() / 2;
    if v.len() % 2 == 1 {
        v[m]
    } else {
        (v[m - 1] + v[m]) / 2.0
    }
}

#[test]
#[ignore = "owner data: set SDC_TRACK_A_ARCHIVES"]
fn recipe_defaults_against_same_author_archives() {
    let Some(root) = std::env::var_os("SDC_TRACK_A_ARCHIVES").map(PathBuf::from) else {
        eprintln!("SDC_TRACK_A_ARCHIVES unset: skipped");
        return;
    };
    assert!(root.is_dir(), "{} is not a folder", root.display());
    let p = LegacyParams::default();
    let mut authors: Vec<PathBuf> = std::fs::read_dir(&root)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|a| a.join("pbr").is_dir() && a.join("vanilla").is_dir())
        .collect();
    authors.sort();
    assert!(!authors.is_empty(), "no <author>/{{pbr,vanilla}} folders");
    let (mut r2s, mut nks) = (vec![], vec![]);
    let mut land_ratios = vec![];
    for author in &authors {
        let name = author.file_name().unwrap().to_string_lossy().to_string();
        let ps = pairs(author);
        let rows: Vec<(bool, Result<Row, String>)> = ps
            .par_iter()
            .map(|pair| (pair.landscape, measure(pair, &p)))
            .collect();
        let skipped = rows.iter().filter(|(_, r)| r.is_err()).count();
        for (pair, (_, r)) in ps.iter().zip(&rows) {
            if let Err(e) = r {
                println!("  skipped {}: {e}", pair.albedo.display());
            }
        }
        let ok: Vec<(bool, Row)> = rows
            .into_iter()
            .filter_map(|(l, r)| r.ok().map(|r| (l, r)))
            .collect();
        println!(
            "{name}: {} pairs ({} landscape), {skipped} skipped (size or read)",
            ps.len(),
            ps.iter().filter(|x| x.landscape).count()
        );
        if ok.is_empty() {
            continue;
        }
        let r2 = median(ok.iter().map(|(_, r)| r.r2).collect());
        let k = median(ok.iter().map(|(_, r)| r.k).collect());
        println!(
            "  diffuse: median R² {r2:.3} (k {} s {}/{}); fitted dielectric k median {k:.3} (default {})",
            p.brightness, p.ao_strength, p.ao_strength_landscape, p.brightness
        );
        for (label, land) in [("objects", false), ("landscape", true)] {
            let sub: Vec<&Row> = ok
                .iter()
                .filter(|(l, _)| *l == land)
                .map(|(_, r)| r)
                .collect();
            if !sub.is_empty() {
                println!(
                    "    {label}: n {} R² {:.3} k {:.3}",
                    sub.len(),
                    median(sub.iter().map(|r| r.r2).collect()),
                    median(sub.iter().map(|r| r.k).collect())
                );
            }
        }
        r2s.push(r2);
        let fits: Vec<(f64, f64)> = ok.iter().filter_map(|(_, r)| r.n_k).collect();
        if !fits.is_empty() {
            let nk = median(fits.iter().map(|f| f.0).collect());
            println!(
                "  _n alpha: n {} fitted k (a = k·g·AO) median {nk:.3}, corr median {:.3}",
                fits.len(),
                median(fits.iter().map(|f| f.1).collect())
            );
            nks.push(nk);
        }
        let (mut der, mut ship, mut n, mut opaque) = (0.0, 0.0, 0.0, 0);
        for (_, r) in ok.iter().filter(|(l, _)| *l) {
            if let Some((d, s, c)) = r.n_mean {
                (der, ship, n) = (der + d, ship + s, n + c);
                opaque += usize::from(s / c > 0.99);
            }
        }
        if n > 0.0 {
            let ratio = der / ship;
            println!(
                "  landscape _n alpha mean: derived {:.4} (landscape_spec {}) vs shipped {:.4}, ratio {ratio:.3} ({opaque} shipped _n opaque)",
                der / n,
                p.landscape_spec,
                ship / n
            );
            land_ratios.push((name.clone(), ratio));
        }
    }
    // Task 13 review M2: a data-layout mistake (no measurable pair) says so instead of "median of nothing".
    if r2s.is_empty() {
        println!("no author has a measurable diffuse pair (see the skips above): nothing to check");
        return;
    }
    let r2 = median(r2s);
    println!("over authors: median diffuse R² {r2:.3} (≥ 0.8)");
    assert!(r2 >= 0.8, "median diffuse R² {r2:.3} < 0.8");
    if nks.is_empty() {
        println!("no author has an _n pair with an _rmaos: the _n k check is skipped");
    } else {
        let nk = median(nks);
        println!("over authors: median _n k {nk:.3} (0.45–0.83)");
        assert!(
            (0.45..=0.83).contains(&nk),
            "median _n k {nk:.3} outside [0.45, 0.83]"
        );
    }
    for (name, ratio) in land_ratios {
        assert!(
            (0.7..=1.4).contains(&ratio),
            "{name}: landscape _n alpha ratio {ratio:.3} outside ×0.7–1.4"
        );
    }
}
