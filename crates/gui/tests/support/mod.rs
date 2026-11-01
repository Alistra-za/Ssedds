#![allow(dead_code)]
//! A folder like a small mod: a diffuse with varying alpha (needs an alpha decision), a normal with no stated green
//! convention, a 48 px NPOT diffuse (blocked by DIM-NPOT), a plain diffuse, and roughness + AO that pack `_rmaos`.
use engine::codes::Code;
use engine::config::{AlphaPolicy, GreenConvention, Subtype};
use image::{ImageBuffer, Luma, Rgb, Rgba};
use sdc_gui::model::{Answer, AppModel};
use sdc_gui::settings::GuiSettings;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub fn mod_folder(dir: &Path) -> PathBuf {
    let r = dir.join("textures/landscape");
    std::fs::create_dir_all(&r).unwrap();
    let grad = |x: u32, y: u32| [(x * 4) as u8, (y * 4) as u8, 90u8];
    ImageBuffer::from_fn(64, 64, |x, y| Rgb(grad(x, y)))
        .save(r.join("rock01.png"))
        .unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| {
        let g = grad(x, y);
        Rgba([g[0], g[1], g[2], if x < 32 { 255 } else { 0 }])
    })
    .save(r.join("leaf.png"))
    .unwrap();
    ImageBuffer::from_fn(64, 64, |_, _| Rgb([128u8, 128, 255]))
        .save(r.join("rock01_normal.png"))
        .unwrap();
    ImageBuffer::from_fn(48, 48, |x, y| Rgb(grad(x, y)))
        .save(r.join("odd.png"))
        .unwrap();
    for (n, v) in [("rock01_roughness.png", 120u8), ("rock01_ao.png", 200)] {
        ImageBuffer::from_fn(64, 64, |x, _| Luma([v.wrapping_add(x as u8)]))
            .save(r.join(n))
            .unwrap();
    }
    dir.join("textures")
}

/// A session of its own per call: a shared one would drop another test's cache entries on every plan (audit 8). The
/// BC1 model is process-global (spec §4.6) and the same for all of them.
pub fn session() -> Arc<engine::session::Session> {
    Arc::new(engine::session::Session::new(encoders_sys::Bc1Model::Ideal).unwrap())
}

pub fn opened(dir: &Path) -> AppModel {
    let mut m = opened_unchosen(dir);
    choose_export(&mut m, dir.join("export"));
    m
}

/// `opened` before the export folder is chosen: `gui.toml` names one, the project has none (Note FB1), and nothing
/// is written to the project folder.
pub fn opened_unchosen(dir: &Path) -> AppModel {
    let root = mod_folder(dir);
    let settings = GuiSettings {
        export_folder: Some(dir.join("export")),
        ..GuiSettings::default()
    };
    let mut m = AppModel::new(settings, None, session());
    // M3-era fixture: High members stay undecided, as before M6 Track E (DECISIONS Note U3).
    m.accept = engine::policy::AcceptSuggestions::None;
    m.open_folder(root);
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
    m
}

/// Note FB1: a project's export folder is the one chosen for it (the `gui.toml` value is no longer inherited), so a
/// fixture that exports picks its folder as the user does and waits for the re-plan.
pub fn choose_export(m: &mut AppModel, folder: PathBuf) {
    m.set_export_folder(folder);
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
}

/// Task 5: `opened` with `m.sets` replaced by `n` clones of the first set, each under its own key, members unchanged.
/// The synthetic sets last only until the next plan: any re-plan (a reorder does one) replaces them with the fixture's
/// own sets. After an edit, check `sets_doc.order()`, not `views`.
pub fn many_sets(dir: &Path, n: usize) -> AppModel {
    let mut m = opened(dir);
    let first = m.sets[0].clone();
    m.sets = (0..n)
        .map(|i| {
            let mut s = first.clone();
            s.key = format!("{}{i:04}", first.key);
            s
        })
        .collect();
    m.invalidate_views();
    // A10: a doc-created set or a dedupe must not change N.
    assert_eq!(sdc_gui::model::sets::views(&m).len(), n);
    m
}

/// The M3a fixture with rock01 decided as a TruePBR set: `pbr_albedo` diffuse, `pbr_normal` normal (DirectX green).
/// leaf.png and odd.png stay undecided, as in M3a.
pub fn pbr_opened(dir: &Path) -> AppModel {
    use engine::config::{GreenConvention, Subtype};
    use sdc_gui::model::Answer;
    let mut m = opened(dir);
    m.decide("landscape/rock01.png", Answer::Subtype(Subtype::PbrAlbedo));
    m.decide(
        "landscape/rock01_normal.png",
        Answer::Subtype(Subtype::PbrNormal),
    );
    m.decide(
        "landscape/rock01_normal.png",
        Answer::Green(GreenConvention::Directx),
    );
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
    m
}

/// `pbr_opened` with rock01 picked as plain `pbr`: the fixture lives under `textures/landscape/`, so it is auto PBR
/// Landscape (Note BM); tests of plain-PBR cards, slots and the sphere/mesh preview use this.
pub fn pbr_opened_plain(dir: &Path) -> AppModel {
    let mut m = pbr_opened(dir);
    m.set_shader("landscape/rock01", Some(engine::shader::Shader::Pbr));
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
    m
}

/// A model built as the app builds it (no test overrides), opened on `root` with exports under `export`.
pub fn opened_as_the_app(root: PathBuf, export: PathBuf) -> AppModel {
    let settings = GuiSettings {
        export_folder: Some(export.clone()),
        ..GuiSettings::default()
    };
    let mut m = AppModel::new(settings, None, session());
    m.open_folder(root);
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
    choose_export(&mut m, export);
    m
}

/// Note AZ: give the set whose key ends with `key_end` the complex-material shader (under vanilla it then packs the
/// complex `_m`, the old cpm target) and wait for the re-plan.
pub fn complex_material(m: &mut AppModel, key_end: &str) {
    let views = sdc_gui::model::sets::views(m);
    let id = views
        .iter()
        .find(|v| v.key.ends_with(key_end))
        .unwrap_or_else(|| panic!("no set {key_end}"))
        .id
        .clone();
    m.set_shader(&id, Some(engine::shader::Shader::ComplexMaterial));
    assert!(m.wait_idle(Duration::from_secs(60)));
}

pub fn decide_all(m: &mut AppModel) {
    m.decide("landscape/rock01.png", Answer::Subtype(Subtype::Diffuse));
    m.decide("landscape/odd.png", Answer::Subtype(Subtype::Diffuse));
    m.decide("landscape/odd.png", Answer::Acknowledge(Code::DimNpot));
    m.decide("landscape/leaf.png", Answer::Subtype(Subtype::Diffuse));
    m.decide("landscape/leaf.png", Answer::Alpha(AlphaPolicy::Cutout));
    m.decide(
        "landscape/rock01_normal.png",
        Answer::Subtype(Subtype::Normal),
    );
    m.decide(
        "landscape/rock01_normal.png",
        Answer::Green(GreenConvention::Directx),
    );
    assert!(m.wait_idle(Duration::from_secs(60)));
}

/// `pbr_opened`, planned and idle (was pbr_export.rs::ready).
pub fn pbr_ready(dir: &Path) -> AppModel {
    let mut m = pbr_opened(dir);
    assert!(m.wait_idle(Duration::from_secs(60)));
    m
}

/// Note FB1: the header's Export button reads `Export {n} files` or `Review {n} files`; its label as drawn now (the
/// Review panel's own `Export {n} files` button is below the header, so the header's rect picks the right one).
pub fn header_export_label<S>(h: &egui_kittest::Harness<'_, S>) -> String {
    use eframe::egui::{accesskit::Role, containers::panel::PanelState, Id};
    use egui_kittest::kittest::{NodeT, Queryable};
    let header = PanelState::load(&h.ctx, Id::new("header")).expect("the header was drawn");
    h.query_all_by_role(Role::Button)
        .filter(|n| header.outer_rect.contains_rect(n.rect()))
        .filter_map(|n| n.accesskit_node().label())
        .find(|l| {
            *l == "Export"
                || ((l.starts_with("Export ") || l.starts_with("Review "))
                    && (l.ends_with(" file") || l.ends_with(" files")))
        })
        .expect("the header Export button")
}

/// Export as a user would: Export, then, if Review opened, acknowledge its pitfalls and press its Export.
pub fn export_through(m: &mut AppModel) {
    m.export_clicked();
    if m.review.is_some() {
        m.acknowledge_all_asks();
        m.export_from_review();
    }
}

/// The PGPatcher JSON an export of `m` writes (was pbr_export.rs::json_path).
pub fn pgp_json_path(m: &AppModel) -> PathBuf {
    m.export_root()
        .join("PBRNIFPatcher")
        .join(format!("{}.json", sdc_gui::model::pbr_export::json_name(m)))
}

/// Layout v2 K5: a slot row's label, the leftmost match (the Matching header repeats "Diffuse" and "Normal _n").
pub fn slot_rect<S>(h: &egui_kittest::Harness<'_, S>, label: &str) -> eframe::egui::Rect {
    use egui_kittest::kittest::Queryable;
    h.query_all_by_label(label)
        .map(|n| n.rect())
        .min_by(|a, b| a.min.x.total_cmp(&b.min.x))
        .unwrap_or_else(|| panic!("no slot row {label:?}"))
}

/// Layout v2 K11: whether `r` is inside the Sets column. Its cards' chips are named by their letter, and "RMAOS" is
/// also the set editor's slot row.
pub fn in_sets_column<S>(h: &egui_kittest::Harness<'_, S>, r: eframe::egui::Rect) -> bool {
    use eframe::egui::{containers::panel::PanelState, Id};
    PanelState::load(&h.ctx, Id::new("sets")).is_some_and(|p| p.outer_rect.contains_rect(r))
}

/// The one node labelled `label` outside the Sets column (K11).
pub fn outside_sets<'a, S>(
    h: &'a egui_kittest::Harness<'_, S>,
    label: &'a str,
) -> egui_kittest::Node<'a> {
    use egui_kittest::kittest::Queryable;
    let mut n: Vec<_> = h
        .query_all_by_label(label)
        .filter(|n| !in_sets_column(h, n.rect()))
        .collect();
    assert_eq!(n.len(), 1, "one {label:?} outside the Sets column");
    n.pop().unwrap()
}

/// Task 6: a PBR set under `textures/test` (`wall.png` at grey `albedo`, `wall_rmaos.png` filled with `rmaos`),
/// opened as the app opens a folder, shader Pbr. Luma 20 gives the albedo a per-texel CHK-RANGE; luma 100 with a
/// metal `_rmaos` (G 255) gives it only `dark_metal`.
pub fn pbr_wall(dir: &Path, albedo: u8, rmaos: [u8; 3]) -> AppModel {
    let r = dir.join("textures/test");
    std::fs::create_dir_all(&r).unwrap();
    ImageBuffer::from_fn(64, 64, |_, _| Rgb([albedo, albedo, albedo]))
        .save(r.join("wall.png"))
        .unwrap();
    ImageBuffer::from_fn(64, 64, |_, _| Rgb(rmaos))
        .save(r.join("wall_rmaos.png"))
        .unwrap();
    let mut m = opened_as_the_app(dir.join("textures"), dir.join("export"));
    let id = sdc_gui::model::sets::views(&m)
        .iter()
        .find(|v| v.key.ends_with("wall"))
        .expect("the set")
        .id
        .clone();
    m.set_shader(&id, Some(engine::shader::Shader::Pbr));
    assert!(m.wait_idle(Duration::from_secs(60)));
    m
}

/// Track B Task 6: the engine's `solve::tests::torus` relief (sines of periods W..W/8 plus a wrapped Gaussian) as
/// unit tangent-space normals, slopes `4·(h[x+1] − h[x−1])`, wrapped.
pub fn relief_normals(n: u32) -> Vec<[f64; 3]> {
    use std::f64::consts::PI;
    let (w, nf) = (n as usize, n as f64);
    let gauss = |x: f64, y: f64, c: f64, s: f64| {
        (-((x - c).powi(2) + (y - c).powi(2)) / (2.0 * s * s)).exp()
    };
    let hf: Vec<f64> = (0..w * w)
        .map(|i| {
            let (x, y) = ((i % w) as f64, (i / w) as f64);
            let mut v = 0.0;
            for d in [1.0, 2.0, 4.0, 8.0] {
                v += (2.0 * PI * d * x / nf).sin() + (2.0 * PI * d * y / nf + d).cos();
            }
            for ox in [-nf, 0.0, nf] {
                for oy in [-nf, 0.0, nf] {
                    v += 4.0 * gauss(x + ox, y + oy, nf / 3.0, 8.0);
                }
            }
            0.25 * v
        })
        .collect();
    slopes_to_normals(&hf, w, 4.0)
}

/// Unit normals of the wrapped `w`×`w` height field `hf`, slopes `k·(h[x+1] − h[x−1])`.
pub fn slopes_to_normals(hf: &[f64], w: usize, k: f64) -> Vec<[f64; 3]> {
    let n = w as isize;
    let at = |x: isize, y: isize| hf[(y.rem_euclid(n) * n + x.rem_euclid(n)) as usize];
    (0..w * w)
        .map(|i| {
            let (x, y) = ((i % w) as isize, (i / w) as isize);
            let p = k * (at(x + 1, y) - at(x - 1, y));
            let q = k * (at(x, y + 1) - at(x, y - 1));
            let l = (p * p + q * q + 1.0).sqrt();
            [-p / l, -q / l, 1.0 / l]
        })
        .collect()
}

/// A 64 px BC1 DDS at `path` (parent folders made), written by the engine's writer, never copied.
pub fn bc1_dds(path: &Path) {
    use engine::dds::{dds_bytes, DdsDesc, ResolvedHeader};
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let desc = DdsDesc {
        width: 64,
        height: 64,
        format: engine::config::Format::Bc1,
        srgb: false,
        header: ResolvedHeader::Legacy,
    };
    let data = vec![[0x00, 0xF8, 0x00, 0xF8, 0, 0, 0, 0].repeat(256)];
    std::fs::write(path, dds_bytes(&desc, &data).unwrap()).unwrap();
}

/// Note F?2: a released mod's all-DDS PBR sets: `textures/pbr/<s>/<s>.dds`, `<s>_n.dds` and `<s>_rmaos.dds` for each
/// of `sets` under `dir`; returns the `textures` folder.
pub fn dds_pbr_sets(dir: &Path, sets: &[&str]) -> PathBuf {
    for s in sets {
        for suffix in ["", "_n", "_rmaos"] {
            bc1_dds(&dir.join(format!("textures/pbr/{s}/{s}{suffix}.dds")));
        }
    }
    dir.join("textures")
}

/// A unit normal stored as 8-bit RGB, `(n + 1) / 2`.
pub fn normal_rgb(n: [f64; 3]) -> [u8; 3] {
    n.map(|c| ((c.clamp(-1.0, 1.0) * 0.5 + 0.5) * 255.0).round() as u8)
}

/// `relief_normals(size)` as an 8-bit PNG at `dir/name` (pass `rock01_normal.png` to replace the flat fixture).
pub fn relief_normal(dir: &Path, name: &str, size: u32) -> PathBuf {
    let n = relief_normals(size);
    let path = dir.join(name);
    ImageBuffer::from_fn(size, size, |x, y| {
        Rgb(normal_rgb(n[(y * size + x) as usize]))
    })
    .save(&path)
    .unwrap();
    path
}
