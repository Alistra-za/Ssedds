//! Documentation screenshots (docs/DOCUMENTATION.md §5). The unit tests run in CI; `render_doc_shots` is the GPU render.

#[path = "doc_shots/compare.rs"]
mod compare;
#[path = "doc_shots/manifest.rs"]
mod manifest;
#[path = "doc_shots/scenarios.rs"]
mod scenarios;
mod support;

use compare::{compare, overlay};
use image::{Rgba, RgbaImage};
use std::path::Path;

fn grey(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_pixel(w, h, Rgba([128, 128, 128, 255]))
}

#[test]
fn identical_images_are_unchanged() {
    let c = compare(&grey(4, 4), &grey(4, 4));
    assert_eq!(c.changed_px, 0);
    assert!(!c.changed());
}

#[test]
fn a_step_of_8_is_noise_and_9_is_a_change() {
    let old = grey(4, 4);
    let mut new = grey(4, 4);
    new.put_pixel(1, 1, Rgba([128, 136, 128, 255]));
    assert_eq!(compare(&old, &new).changed_px, 0);
    new.put_pixel(1, 1, Rgba([128, 137, 128, 255]));
    assert_eq!(compare(&old, &new).changed_px, 1);
}

#[test]
fn a_thousandth_is_noise_and_more_counts() {
    let old = grey(1000, 1);
    let mut new = grey(1000, 1);
    new.put_pixel(0, 0, Rgba([0, 0, 0, 255]));
    assert!(!compare(&old, &new).changed());
    new.put_pixel(1, 0, Rgba([0, 0, 0, 255]));
    assert!(compare(&old, &new).changed());
}

#[test]
fn overlay_dims_old_and_marks_changes_magenta() {
    let old = RgbaImage::from_pixel(2, 1, Rgba([200, 100, 50, 255]));
    let mut new = old.clone();
    new.put_pixel(1, 0, Rgba([0, 0, 0, 255]));
    let o = overlay(&old, &new);
    assert_eq!(o.get_pixel(0, 0), &Rgba([70, 35, 17, 255]));
    assert_eq!(o.get_pixel(1, 0), &Rgba([255, 0, 255, 255]));
}

#[test]
fn a_size_change_is_a_change() {
    let old = grey(4, 4);
    let new = grey(4, 5);
    let c = compare(&old, &new);
    assert!(c.size_changed);
    assert!(c.changed());
    assert_eq!(overlay(&old, &new), new);
}

#[test]
fn manifest_scenarios_and_pages_agree() {
    let repo = manifest::repo_root();
    let m = manifest::load(&repo.join("docs/img/gui/shots.toml"));
    let mut errors = vec![];
    for s in &m.shot {
        if !scenarios::SCENARIOS.iter().any(|(n, _)| *n == s.scenario) {
            errors.push(format!("shot {}: no scenario {}", s.id, s.scenario));
        }
        let rel = format!("img/gui/{}/{}.png", s.page, s.id);
        for page in &s.used_by {
            match std::fs::read_to_string(repo.join("docs/wiki").join(page)) {
                Ok(text) if text.contains(&rel) => {}
                Ok(_) => errors.push(format!("{page} does not show {rel}")),
                Err(_) => errors.push(format!("shot {}: no page docs/wiki/{page}", s.id)),
            }
        }
        if !s.png(&repo).exists() {
            errors.push(format!("shot {}: {} was never accepted", s.id, rel));
        }
    }
    for (name, _) in scenarios::SCENARIOS {
        let n = m.shot.iter().filter(|s| s.scenario == *name).count();
        if n != 1 {
            errors.push(format!("scenario {name} has {n} shots"));
        }
    }
    let pngs: Vec<_> = m.shot.iter().map(|s| s.png(&repo)).collect();
    for e in walkdir::WalkDir::new(repo.join("docs/img/gui"))
        .into_iter()
        .flatten()
    {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "png") && !pngs.iter().any(|q| same_file(q, p)) {
            errors.push(format!("orphan shot {}", p.display()));
        }
    }
    for x in &m.exempt {
        if !repo
            .join("docs/wiki")
            .join(format!("{}.md", x.page))
            .exists()
        {
            errors.push(format!("exempt {}: no page {}", x.what, x.page));
        }
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

/// Renders every manifest shot (or those in `DOC_SHOTS_ONLY`) into `DOC_SHOTS_OUT`, compares each with the committed
/// PNG and writes `<page>/<id>.png`, `<page>/<id>.diff.png` (changed shots), `summary.tsv` and `summary.md`. Needs a
/// GPU or WARP; run it through `tools\doc-shots.ps1`.
#[test]
#[ignore]
fn render_doc_shots() {
    let out = std::path::PathBuf::from(std::env::var_os("DOC_SHOTS_OUT").expect("DOC_SHOTS_OUT"));
    let only: Option<Vec<String>> = std::env::var("DOC_SHOTS_ONLY").ok().map(|v| {
        v.split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    });
    let tmp = tempfile::tempdir().unwrap();
    redirect_app_data(tmp.path());
    let repo = manifest::repo_root();
    let m = manifest::load(&repo.join("docs/img/gui/shots.toml"));

    let mut tsv = String::from("id\tpage\tstatus\tpercent\tused_by\n");
    let mut rows = vec![];
    let mut failed = vec![];
    for (i, s) in m.shot.iter().enumerate() {
        if only.as_ref().is_some_and(|o| !o.contains(&s.id)) {
            continue;
        }
        let used_by = s.used_by.join(",");
        let Some((_, scenario)) = scenarios::SCENARIOS.iter().find(|(n, _)| *n == s.scenario)
        else {
            failed.push(format!("{}: no scenario {}", s.id, s.scenario));
            tsv.push_str(&format!("{}\t{}\tfailed\t\t{used_by}\n", s.id, s.page));
            continue;
        };
        let dir = tmp.path().join(format!("shot-{i}"));
        std::fs::create_dir_all(&dir).unwrap();
        let frame = match std::panic::catch_unwind(|| scenario(&dir)) {
            Ok(f) => f,
            Err(p) => {
                let msg = p
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                failed.push(format!("{}: {msg}", s.id));
                tsv.push_str(&format!("{}\t{}\tfailed\t\t{used_by}\n", s.id, s.page));
                rows.push(format!("| {} | {} | failed | | {used_by} |", s.id, s.page));
                continue;
            }
        };
        let img = crop(frame.img, frame.crop);
        let page_dir = out.join(&s.page);
        std::fs::create_dir_all(&page_dir).unwrap();
        img.save(page_dir.join(format!("{}.png", s.id))).unwrap();
        let committed = s.png(&repo);
        let (status, percent) = if committed.exists() {
            let old = image::open(&committed).unwrap().to_rgba8();
            let c = compare(&old, &img);
            if c.changed() {
                overlay(&old, &img)
                    .save(page_dir.join(format!("{}.diff.png", s.id)))
                    .unwrap();
            }
            let status = if c.size_changed {
                "size"
            } else if c.changed() {
                "changed"
            } else {
                "unchanged"
            };
            (status, format!("{:.3}", c.percent()))
        } else {
            ("new", String::new())
        };
        tsv.push_str(&format!(
            "{}\t{}\t{status}\t{percent}\t{used_by}\n",
            s.id, s.page
        ));
        rows.push(format!(
            "| {} | {} | {status} | {percent} | {used_by} |",
            s.id, s.page
        ));
    }

    let adapter = scenarios::adapter_name();
    let fonts = font_hashes();
    let mut warn = vec![];
    if !adapter.is_empty() && adapter != m.render.adapter {
        warn.push(format!(
            "adapter differs: shots.toml has {:?}",
            m.render.adapter
        ));
    }
    for (f, h) in &fonts {
        if m.render.fonts.get(f) != Some(h) {
            warn.push(format!("font {f} differs from shots.toml"));
        }
    }
    let mut md = if warn.is_empty() {
        String::from("Render settings match shots.toml.\n\n")
    } else {
        format!(
            "**Warning: not comparable with the committed shots:** {}.\n\n",
            warn.join("; ")
        )
    };
    md.push_str(&format!("adapter = {adapter:?}\n\n[render.fonts]\n"));
    for (f, h) in &fonts {
        md.push_str(&format!("\"{f}\" = \"{h}\"\n"));
    }
    md.push_str("\n| Shot | Page | Status | Changed % | Used by |\n|---|---|---|---:|---|\n");
    for r in &rows {
        md.push_str(r);
        md.push('\n');
    }
    md.push_str("\n## Exempt\n\n");
    for x in &m.exempt {
        md.push_str(&format!("- {} — {} — {}\n", x.what, x.page, x.reason));
    }
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("summary.tsv"), tsv).unwrap();
    std::fs::write(out.join("summary.md"), md).unwrap();
    assert!(failed.is_empty(), "shots failed:\n{}", failed.join("\n"));
}

/// Crops to `rect` plus 16 px, clamped to the image.
fn crop(img: RgbaImage, rect: Option<eframe::egui::Rect>) -> RgbaImage {
    let Some(r) = rect else { return img };
    let (w, h) = (img.width() as f32, img.height() as f32);
    let x0 = (r.min.x - 16.0).clamp(0.0, w) as u32;
    let y0 = (r.min.y - 16.0).clamp(0.0, h) as u32;
    let x1 = (r.max.x + 16.0).clamp(0.0, w).ceil() as u32;
    let y1 = (r.max.y + 16.0).clamp(0.0, h).ceil() as u32;
    image::imageops::crop_imm(&img, x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0)).to_image()
}

/// sha256 of each font file the app loads (`fonts.rs`) that `%WINDIR%\Fonts` has.
fn font_hashes() -> std::collections::BTreeMap<String, String> {
    use sha2::{Digest, Sha256};
    let dir = std::path::PathBuf::from(
        std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into()),
    )
    .join("Fonts");
    let files = [
        "segoeui.ttf",
        "seguisym.ttf",
        "seguiemj.ttf",
        "msyh.ttc",
        "YuGothM.ttc",
        "malgun.ttf",
        "consola.ttf",
    ];
    files
        .iter()
        .filter_map(|f| {
            std::fs::read(dir.join(f))
                .ok()
                .map(|b| (f.to_string(), hex::encode(Sha256::digest(&b))))
        })
        .collect()
}

/// Points `%APPDATA%` and `%LOCALAPPDATA%` at `tmp` and proves the scan store followed.
fn redirect_app_data(tmp: &Path) {
    let a = tmp.join("appdata");
    std::fs::create_dir_all(&a).unwrap();
    std::env::set_var("LOCALAPPDATA", &a);
    std::env::set_var("APPDATA", &a);
    let store = sdc_gui::model::meshes::default_store_path().unwrap();
    assert!(store.starts_with(&a), "scan.db not redirected: {store:?}");
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

#[test]
fn the_manifest_parses() {
    let m = manifest::load(&manifest::repo_root().join("docs/img/gui/shots.toml"));
    assert_eq!(m.render.size, [1600, 900]);
    assert_eq!(m.render.theme, "dark");
}
