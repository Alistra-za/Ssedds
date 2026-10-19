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
        export_folder: Some(export),
        ..GuiSettings::default()
    };
    let mut m = AppModel::new(settings, None, session());
    m.open_folder(root);
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
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
