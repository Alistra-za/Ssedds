//! v2 artist checks, Task 2 (DECISIONS Note CI): the GUI turns the engine's check stats into `CHK-*` warnings,
//! lists the Warning-level ones as `Level::Check` Problems rows that never change "ready", and shows them on the set
//! card's "ⓘ n" badge.
mod support;
use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use engine::analyse::Analysis;
use engine::api::*;
use engine::checks::Curl;
use engine::codes::{Code, Severity, Warning};
use engine::config::{Subtype, Target};
use engine::events::*;
use engine::policy::ResolvedJob;
use engine::session::{AnalysisCache, Thumbnail};
use engine::shader::Shader;
use image::{ImageBuffer, Rgb, Rgba};
use sdc_gui::model::bottom::{self, At, Level, ProblemsFilter};
use sdc_gui::model::checks::*;
use sdc_gui::model::sets::{SlotFill, SlotKind};
use sdc_gui::model::AppModel;
use sdc_gui::pgp::settings::Settings;
use sdc_gui::settings::{ChecksSettings, GuiSettings};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

// ---- pure checks ------------------------------------------------------------------------------------------------

/// Plan `img` alone as `name` (cs, check stats on) and return its job and analysis.
fn planned(dir: &Path, name: &str, save: impl FnOnce(&Path)) -> (ResolvedJob, Analysis) {
    let input = dir.join("in");
    std::fs::create_dir_all(&input).unwrap();
    save(&input.join(name));
    let mut c = AnalysisCache::default();
    c.checks = true;
    let p = Engine::new()
        .unwrap()
        .plan_cached(
            BatchRequest::new(vec![input], dir.join("out"), Target::Cs),
            &Reporter::new(Arc::new(NullSink), "t"),
            &CancelToken::default(),
            &c,
        )
        .unwrap();
    let job = p
        .jobs
        .iter()
        .find(|j| j.input.file_name().is_some_and(|f| f == name))
        .unwrap_or_else(|| panic!("no job for {name}"))
        .clone();
    let a = c.analysis(&job).expect("an analysis");
    assert!(a.checks.is_some(), "check stats");
    (job, a)
}

/// A `w`×`h` RGB image from a per-texel colour.
fn rgb(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 3]) -> impl FnOnce(&Path) {
    move |p| {
        ImageBuffer::from_fn(w, h, |x, y| Rgb(f(x, y)))
            .save(p)
            .unwrap()
    }
}

fn rgba(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> impl FnOnce(&Path) {
    move |p| {
        ImageBuffer::from_fn(w, h, |x, y| Rgba(f(x, y)))
            .save(p)
            .unwrap()
    }
}

fn as_subtype(mut j: ResolvedJob, st: Subtype) -> ResolvedJob {
    j.profile.subtype = st;
    j.profile.category = st.category();
    j
}

fn with(code: Code, ws: &[Warning]) -> Vec<&Warning> {
    ws.iter().filter(|w| w.code == code).collect()
}

fn all_on() -> ChecksSettings {
    ChecksSettings::default()
}

#[test]
/// RANGE_SHARE (0.50 after the 10k tuning, CI8): 51 of 100 texels at luma 20 warn, 49 do not.
fn albedo_out_of_range_luma_raises_chk_range_above_range_share() {
    let at = (RANGE_SHARE * 100.0).round() as u32;
    let check = |dark: u32| {
        let d = tempfile::tempdir().unwrap();
        let (j, a) = planned(
            d.path(),
            "wall.png",
            rgb(10, 10, move |x, y| {
                if y * 10 + x < dark {
                    [20, 20, 20]
                } else {
                    [128, 128, 128]
                }
            }),
        );
        let j = as_subtype(j, Subtype::PbrAlbedo);
        texture_checks(&j, &a, HeightNeutral::Half, &all_on())
    };
    let ws = check(at + 1);
    let r = with(Code::ChkRange, &ws);
    assert_eq!(r.len(), 1, "{ws:?}");
    assert_eq!(r[0].data, json!({"rule": "albedo_luma"}));
    assert_eq!(r[0].severity, Severity::Warning);
    assert!(with(Code::ChkRange, &check(at - 1)).is_empty());
    // The switch hides it.
    let d = tempfile::tempdir().unwrap();
    let (j, a) = planned(d.path(), "wall.png", rgb(10, 10, |_, _| [20, 20, 20]));
    let j = as_subtype(j, Subtype::PbrAlbedo);
    let off = ChecksSettings {
        range: false,
        ..all_on()
    };
    assert!(texture_checks(&j, &a, HeightNeutral::Half, &off).is_empty());
}

#[test]
fn range_rule_bad_agrees_with_the_band_counts() {
    let mut s = 12345u32;
    let mut next = move || {
        s = s.wrapping_mul(1_103_515_245).wrapping_add(12345);
        (s >> 16) as u8
    };
    let px: Vec<[u8; 3]> = (0..64 * 64)
        .map(|i| {
            // Every fourth texel at an edge code, so each rule sees both outcomes.
            if i % 8 == 0 {
                [0, 255, 3]
            } else if i % 8 == 4 {
                [0, 0, 0]
            } else {
                [next(), next(), next()]
            }
        })
        .collect();
    let d = tempfile::tempdir().unwrap();
    let p2 = px.clone();
    let (_, a) = planned(
        d.path(),
        "noise.png",
        rgb(64, 64, move |x, y| p2[(y * 64 + x) as usize]),
    );
    let st = a.checks.unwrap();
    for rule in [
        RangeRule::AlbedoLuma,
        RangeRule::MetalGrey,
        RangeRule::RoughExtreme,
        RangeRule::EmissiveBackground,
    ] {
        let bad = px
            .iter()
            .filter(|c| rule.bad([c[0], c[1], c[2], 255]))
            .count() as u64;
        assert_eq!(bad, rule.count(&st), "{}", rule.id());
        assert!(bad > 0 && bad < st.texels, "{}: {bad}", rule.id());
    }
    assert_eq!(rules_for(Subtype::PbrAlbedo), &[RangeRule::AlbedoLuma]);
    assert_eq!(
        rules_for(Subtype::Rmaos),
        &[RangeRule::RoughExtreme, RangeRule::MetalGrey]
    );
    assert_eq!(rules_for(Subtype::Glow), &[RangeRule::EmissiveBackground]);
    assert!(rules_for(Subtype::Normal).is_empty());
}

#[test]
fn ao_dark_is_a_mean_check_without_an_overlay_rule() {
    let d = tempfile::tempdir().unwrap();
    let (j, a) = planned(d.path(), "wall_rmaos.png", rgb(8, 8, |_, _| [128, 0, 51]));
    let j = as_subtype(j, Subtype::Rmaos);
    let ws = texture_checks(&j, &a, HeightNeutral::Half, &all_on());
    let r = with(Code::ChkRange, &ws);
    assert_eq!(r.len(), 1, "{ws:?}");
    assert_eq!(r[0].data, json!({"rule": "ao_dark"}));
    assert!(rules_for(Subtype::Rmaos)
        .iter()
        .all(|r| r.id() != "ao_dark"));
}

#[test]
fn a_vanilla_n_with_no_alpha_or_white_alpha_raises_plastic_softly() {
    let plastic = |st: Subtype, alpha: Option<u8>| {
        let d = tempfile::tempdir().unwrap();
        let (j, a) = match alpha {
            None => planned(d.path(), "wall_n.png", rgb(8, 8, |_, _| [128, 128, 255])),
            Some(v) => planned(
                d.path(),
                "wall_n.png",
                rgba(8, 8, move |_, _| [128, 128, 255, v]),
            ),
        };
        let j = as_subtype(j, st);
        texture_checks(&j, &a, HeightNeutral::Half, &all_on())
            .into_iter()
            .filter(|w| w.code == Code::ChkPlastic)
            .collect::<Vec<_>>()
    };
    for ws in [
        plastic(Subtype::Normal, None),
        plastic(Subtype::Normal, Some(255)),
    ] {
        assert_eq!(ws.len(), 1, "{ws:?}");
        assert!(
            ws[0].message.contains("may look plastic"),
            "{}",
            ws[0].message
        );
        assert_eq!(ws[0].severity, Severity::Warning);
    }
    assert!(plastic(Subtype::Normal, Some(90)).is_empty());
    assert!(plastic(Subtype::PbrNormal, None).is_empty());
}

#[test]
fn a_two_channel_normal_raises_no_z_sign() {
    let d = tempfile::tempdir().unwrap();
    let (j, mut a) = planned(d.path(), "wall_n.png", rgb(8, 8, |_, _| [128, 128, 0]));
    let j = as_subtype(j, Subtype::Normal);
    a.channels[2].mean = 0.0;
    let z = |a: &Analysis| {
        with(
            Code::ChkZSign,
            &texture_checks(&j, a, HeightNeutral::Half, &all_on()),
        )
        .len()
    };
    a.checks.as_mut().unwrap().two_channel = true;
    assert_eq!(z(&a), 0);
    a.checks.as_mut().unwrap().two_channel = false;
    assert_eq!(z(&a), 1);
    // An MSN is object space: no Z sign.
    let msn = as_subtype(j.clone(), Subtype::Msn);
    assert!(with(
        Code::ChkZSign,
        &texture_checks(&msn, &a, HeightNeutral::Half, &all_on())
    )
    .is_empty());
}

#[test]
fn green_guess_is_info_and_only_when_the_file_does_not_flip() {
    let d = tempfile::tempdir().unwrap();
    let (j, mut a) = planned(d.path(), "wall_n.png", rgb(8, 8, |_, _| [128, 128, 255]));
    let j = as_subtype(j, Subtype::PbrNormal);
    let guess = |j: &ResolvedJob, a: &Analysis| {
        texture_checks(j, a, HeightNeutral::Half, &all_on())
            .into_iter()
            .filter(|w| w.code == Code::ChkGreenGuess)
            .collect::<Vec<_>>()
    };
    a.checks.as_mut().unwrap().curl = Some(Curl {
        as_stored: 1.0,
        flipped: 0.1,
    });
    let g = guess(&j, &a);
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].severity, Severity::Info);
    let mut flips = j.clone();
    flips.profile.channels.flip_green = true;
    assert!(guess(&flips, &a).is_empty());
    a.checks.as_mut().unwrap().curl = Some(Curl {
        as_stored: 5e-5,
        flipped: 0.0,
    });
    assert!(guess(&j, &a).is_empty());
}

fn parallax(ws: &[Warning]) -> Vec<String> {
    ws.iter()
        .filter(|w| w.code == Code::ChkParallax)
        .map(|w| w.message.clone())
        .collect()
}

#[test]
fn parallax_judges_against_0_5_except_pbr_inner_layer_and_skips_vanilla_multilayer() {
    // White-heavy: R 180..=250 down the rows.
    let d = tempfile::tempdir().unwrap();
    let (j, a) = planned(
        d.path(),
        "wall_p.png",
        rgb(64, 64, |_, y| {
            let v = 180 + y as u8 + (y / 9) as u8;
            [v, v, v]
        }),
    );
    let j = as_subtype(j, Subtype::Height);
    let at = |n| parallax(&texture_checks(&j, &a, n, &all_on()));
    let half = at(HeightNeutral::Half);
    assert!(half.iter().any(|m| m.contains("mostly white")), "{half:?}");
    assert!(
        at(HeightNeutral::White).is_empty(),
        "{:?}",
        at(HeightNeutral::White)
    );
    assert!(at(HeightNeutral::NotHeight).is_empty());

    // 25 % of the rows at code 0, the rest a mid ramp: clipped at the maximum recess for both neutrals.
    let d = tempfile::tempdir().unwrap();
    let (j, a) = planned(
        d.path(),
        "wall_p.png",
        rgb(64, 64, |_, y| {
            let v = if y < 16 { 0 } else { 60 + 2 * y as u8 };
            [v, v, v]
        }),
    );
    let j = as_subtype(j, Subtype::Height);
    for n in [HeightNeutral::Half, HeightNeutral::White] {
        let ms = parallax(&texture_checks(&j, &a, n, &all_on()));
        assert!(ms.iter().any(|m| m.contains("clipped")), "{n:?}: {ms:?}");
    }

    // An R8-style _p decodes with A = 255: A is never read for a height.
    let d = tempfile::tempdir().unwrap();
    let (j, a) = planned(
        d.path(),
        "wall_p.png",
        rgba(64, 64, |_, y| {
            let v = 60 + 2 * y as u8;
            [v, v, v, 255]
        }),
    );
    let j = as_subtype(j, Subtype::Height);
    let ms = parallax(&texture_checks(&j, &a, HeightNeutral::Half, &all_on()));
    assert!(ms.iter().all(|m| !m.contains("clipped")), "{ms:?}");
}

#[test]
fn cpm_alpha_at_or_below_4_counts_as_parallax_off() {
    let cpm = |code: u8, parallax_on: Option<bool>| {
        let d = tempfile::tempdir().unwrap();
        let (j, a) = planned(
            d.path(),
            "wall_m.png",
            rgba(64, 64, move |_, y| {
                [128, 128, 128, if y < 10 { code } else { 128 }]
            }),
        );
        let mut j = as_subtype(j, Subtype::ComplexMaterial);
        j.profile.alpha.parallax = parallax_on;
        parallax(&texture_checks(&j, &a, HeightNeutral::Half, &all_on()))
    };
    let ms = cpm(3, Some(true));
    assert!(
        ms.iter()
            .any(|m| m.contains("4/255") && m.contains("251/255")),
        "{ms:?}"
    );
    let ms = cpm(5, Some(true));
    assert!(ms.iter().all(|m| !m.contains("4/255")), "{ms:?}");
    assert!(cpm(3, None).is_empty());
    // Review I3 (CI1): an _m with no alpha has nothing to judge in A, even with parallax on.
    let d = tempfile::tempdir().unwrap();
    let (j, a) = planned(d.path(), "wall_m.png", rgb(64, 64, |_, _| [128, 128, 128]));
    let mut j = as_subtype(j, Subtype::ComplexMaterial);
    j.profile.alpha.parallax = Some(true);
    assert!(parallax(&texture_checks(&j, &a, HeightNeutral::Half, &all_on())).is_empty());
}

#[test]
fn height_neutral_reads_shader_and_resolved_pgp_flags() {
    let s = |kv: &[(&str, bool)]| -> Settings {
        kv.iter().map(|(k, v)| (k.to_string(), json!(v))).collect()
    };
    let ml_coat = s(&[("multilayer", true), ("coat_parallax", true)]);
    assert_eq!(
        height_neutral(Some(Shader::Pbr), &ml_coat),
        HeightNeutral::White
    );
    assert_eq!(
        height_neutral(Some(Shader::Pbr), &s(&[("coat_parallax", true)])),
        HeightNeutral::Half
    );
    assert_eq!(
        height_neutral(Some(Shader::Pbr), &Settings::new()),
        HeightNeutral::Half
    );
    assert_eq!(
        height_neutral(Some(Shader::Multilayer), &Settings::new()),
        HeightNeutral::NotHeight
    );
    for sh in [Shader::Parallax, Shader::Landscape] {
        assert_eq!(height_neutral(Some(sh), &ml_coat), HeightNeutral::Half);
    }
    // X-S1: a _cnr turns multilayer on by itself.
    let mut slots = BTreeMap::new();
    slots.insert(
        SlotKind::Coat,
        SlotFill::Own {
            item: "wall_cnr.png".into(),
        },
    );
    let r = sdc_gui::model::auto_flags::resolved(true, &slots, &s(&[("coat_parallax", true)]));
    assert_eq!(height_neutral(Some(Shader::Pbr), &r), HeightNeutral::White);
}

#[test]
fn landscape_sets_whose_scale_differs_by_more_than_2x_are_flagged() {
    let v = |xs: &[(&str, f64)]| -> Vec<(String, f64)> {
        xs.iter().map(|(k, s)| (k.to_string(), *s)).collect()
    };
    assert_eq!(
        scale_outliers(&v(&[("a", 1.0), ("b", 1.0), ("c", 2.5)])),
        vec!["c".to_string()]
    );
    assert!(scale_outliers(&v(&[("a", 1.0), ("b", 2.0)])).is_empty());
    assert_eq!(
        scale_outliers(&v(&[("a", 1.0), ("b", 3.0)])),
        vec!["a".to_string(), "b".to_string()]
    );
    assert!(scale_outliers(&[]).is_empty());
    assert!(scale_outliers(&v(&[("a", 1.0)])).is_empty());
    assert!(scale_outliers(&v(&[("a", 0.0), ("b", 1.0)])).is_empty());
    assert!(scale_outliers(&v(&[("a", f64::NAN), ("b", 1.0), ("c", 9.0)])).len() == 2);
}

fn thumb(n: usize, f: impl Fn(usize) -> [u8; 4]) -> Thumbnail {
    Thumbnail {
        width: n as u32,
        height: 1,
        rgba: (0..n).flat_map(f).collect(),
    }
}

#[test]
fn dark_metal_share_reads_the_two_thumbnails() {
    let albedo = thumb(100, |_| [40, 40, 40, 255]);
    assert_eq!(
        dark_metal_share(&albedo, &thumb(50, |_| [0, 255, 0, 255])),
        None
    );
    let one = thumb(100, |i| [0, if i == 0 { 255 } else { 0 }, 0, 255]);
    assert_eq!(dark_metal_share(&albedo, &one), None);
    let sixty = thumb(100, |i| [0, if i < 60 { 255 } else { 0 }, 0, 255]);
    let s = dark_metal_share(&albedo, &sixty).unwrap();
    assert!((s - 0.6).abs() < 1e-9, "{s}");
    let empty = thumb(0, |_| [0; 4]);
    assert_eq!(dark_metal_share(&empty, &empty), None);
}

// ---- the model ----------------------------------------------------------------------------------------------------

/// A bumpy tangent-space normal stored with OpenGL green (Y up) while its name says DirectX.
fn gl_bumpy(path: &Path) {
    ImageBuffer::from_fn(64, 64, |x, y| {
        let (u, v) = (x as f32 / 64.0, y as f32 / 64.0);
        let h = |u: f32, v: f32| 0.05 * (u * 12.0).sin() * (v * 7.0).cos();
        let e = 1e-3;
        let (dx, dy) = (
            (h(u + e, v) - h(u - e, v)) / (2.0 * e),
            (h(u, v + e) - h(u, v - e)) / (2.0 * e),
        );
        let (nx, ny) = (-dx * 0.1, dy * 0.1);
        let nz = (1.0 - nx * nx - ny * ny).sqrt();
        let enc = |c: f32| ((c * 0.5 + 0.5) * 255.0).round() as u8;
        Rgb([enc(nx), enc(ny), enc(nz)])
    })
    .save(path)
    .unwrap();
}

/// A PBR set under `textures/test`: `wall.png` at luma `albedo`, a clean `wall_rmaos.png` and a `wall_n.png`, flat,
/// or with `flipped` a bumpy one stored with the other green. Opened as the app opens a folder, shader Pbr, target cs.
fn pbr_wall(dir: &Path, albedo: u8, flipped: bool) -> AppModel {
    pbr_sets(dir, &[("wall", albedo)], flipped)
}

/// `pbr_wall` with one PBR set per `(stem, albedo luma)`.
fn pbr_sets(dir: &Path, sets: &[(&str, u8)], flipped: bool) -> AppModel {
    let r = dir.join("textures/test");
    std::fs::create_dir_all(&r).unwrap();
    for &(stem, albedo) in sets {
        ImageBuffer::from_fn(64, 64, |_, _| Rgb([albedo, albedo, albedo]))
            .save(r.join(format!("{stem}.png")))
            .unwrap();
        ImageBuffer::from_fn(64, 64, |_, _| Rgb([128u8, 10, 200]))
            .save(r.join(format!("{stem}_rmaos.png")))
            .unwrap();
        let n = r.join(format!("{stem}_n.png"));
        if flipped {
            gl_bumpy(&n);
        } else {
            ImageBuffer::from_fn(64, 64, |_, _| Rgb([128u8, 128, 255]))
                .save(n)
                .unwrap();
        }
    }
    let mut m = support::opened_as_the_app(dir.join("textures"), dir.join("export"));
    for &(stem, _) in sets {
        let id = sdc_gui::model::sets::views(&m)
            .iter()
            .find(|v| v.key.ends_with(stem))
            .expect("the set")
            .id
            .clone();
        m.set_shader(&id, Some(Shader::Pbr));
        assert!(m.wait_idle(Duration::from_secs(60)));
        let albedo = m.item(&format!("test/{stem}.png")).expect("albedo item");
        assert_eq!(
            albedo.job.as_ref().map(|j| j.profile.subtype),
            Some(Subtype::PbrAlbedo)
        );
    }
    m
}

fn set_key(m: &AppModel, stem: &str) -> String {
    sdc_gui::model::sets::views(m)
        .iter()
        .find(|v| v.key.ends_with(stem))
        .unwrap()
        .key
        .clone()
}

fn check_rows(m: &AppModel) -> Vec<bottom::Problem> {
    bottom::problems(m)
        .iter()
        .filter(|p| p.level == Level::Check)
        .cloned()
        .collect()
}

#[test]
fn turning_a_check_off_hides_its_warnings_without_a_replan() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_wall(d.path(), 20, false);
    let rows = check_rows(&m);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].code, Some(Code::ChkRange));
    assert_eq!(rows[0].at, At::Texture("test/wall.png".into()));
    let (gen, rev) = (m.generation(), m.items_rev());
    bottom::problems(&m);
    let builds = bottom::problems_builds(&m);
    m.set_checks(ChecksSettings {
        range: false,
        ..ChecksSettings::default()
    });
    m.poll();
    assert!(check_rows(&m).is_empty());
    assert!(m.check_warnings("test/wall.png").is_empty());
    assert!(m.items_rev() > rev, "invalidate_views ran");
    assert_eq!(bottom::problems_builds(&m), builds + 1);
    assert_eq!(m.generation(), gen, "no re-plan");

    // K1: a plan that lands is checked in the poll that lands it.
    m.set_checks(ChecksSettings::default());
    m.poll();
    assert_eq!(check_rows(&m).len(), 1);
    ImageBuffer::from_fn(32, 32, |_, _| Rgb([128u8, 128, 128]))
        .save(d.path().join("textures/test/wall.png"))
        .unwrap();
    m.replan();
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(check_rows(&m).is_empty(), "{:?}", check_rows(&m));
}

/// Review M6: opening another folder drops the old folder's check rows at once, before its plan lands.
#[test]
fn opening_another_folder_drops_the_old_check_rows() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_wall(d.path(), 20, false);
    assert_eq!(check_rows(&m).len(), 1);
    let other = tempfile::tempdir().unwrap();
    let r = other.path().join("textures/plain");
    std::fs::create_dir_all(&r).unwrap();
    ImageBuffer::from_fn(8, 8, |_, _| Rgb([128u8, 128, 128]))
        .save(r.join("rock.png"))
        .unwrap();
    m.open_folder(other.path().join("textures"));
    m.poll();
    assert!(m.check_warnings("test/wall.png").is_empty());
    assert!(check_rows(&m).is_empty(), "{:?}", check_rows(&m));
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(check_rows(&m).is_empty());
}

/// Review M6: an undone edit brings its check rows back with the plan it restores.
#[test]
fn undo_restores_the_check_rows_an_edit_removed() {
    use sdc_gui::model::Answer;
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_wall(d.path(), 20, false);
    assert_eq!(check_rows(&m).len(), 1);
    // As a plain diffuse the value-range rule does not apply.
    m.decide("test/wall.png", Answer::Subtype(Subtype::Diffuse));
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(check_rows(&m).is_empty(), "{:?}", check_rows(&m));
    m.undo();
    assert!(m.wait_idle(Duration::from_secs(60)));
    let rows = check_rows(&m);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].at, At::Texture("test/wall.png".into()));
}

/// Review M2: under a Set filter "Checks {n}" counts that set's check rows and keeps the set.
#[test]
fn the_checks_button_under_a_set_filter_counts_and_keeps_the_set() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_sets(d.path(), &[("wall", 20), ("rock", 20)], false);
    assert_eq!(check_rows(&m).len(), 2);
    let wall = set_key(&m, "wall");
    m.open_problems(Some(ProblemsFilter::Set(wall.clone())));
    let mut h = harness(m, 1600.0);
    h.run();
    h.get_by_label("Checks 1").click();
    h.run();
    assert_eq!(
        h.state().bottom.filter,
        Some(ProblemsFilter::SetChecks(wall.clone()))
    );
    let rows = bottom::filtered(h.state(), h.state().bottom.filter.as_ref());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].at, At::Texture("test/wall.png".into()));
    assert!(h.query_by_label_contains("Checks 1").is_none());
    h.get_by_label_contains("checks only ×").click();
    h.run();
    assert_eq!(h.state().bottom.filter, Some(ProblemsFilter::Set(wall)));
    // Without a Set filter the count is the project's.
    h.state_mut().open_problems(None);
    h.run();
    h.get_by_label("Checks 2");
}

#[test]
fn soft_check_rows_are_listed_but_leave_ready_alone() {
    let d = tempfile::tempdir().unwrap();
    let m = pbr_wall(d.path(), 20, false);
    assert!(sdc_gui::model::pbr_export::export_blocks(&m).is_empty());
    assert!(sdc_gui::model::pbr_export::export_asks(&m).is_empty());
    let all = bottom::problems(&m);
    assert_eq!(all.len(), 1, "{all:?}");
    assert_eq!(all[0].level, Level::Check);
    assert!(!bottom::counts_as_ready_problem(&all[0]));
    let views = sdc_gui::model::sets::views(&m);
    let v = views.iter().find(|v| v.key.ends_with("wall")).unwrap();
    assert!(bottom::set_ready(&m, v), "green set dot");
    let wall = m.item("test/wall.png").unwrap();
    assert!(bottom::tile_ready(wall));
    assert!(wall
        .warnings
        .iter()
        .all(|w| !w.code.as_str().starts_with("CHK-")));
    assert!(v
        .set_warnings
        .iter()
        .all(|w| !w.code.as_str().starts_with("CHK-")));
    assert_eq!(m.set_counts().attention, 0);
    assert!(sdc_gui::ui::header::export_dot(&m));
}

#[test]
fn info_check_results_are_never_problems_rows() {
    let d = tempfile::tempdir().unwrap();
    let m = pbr_wall(d.path(), 128, true);
    let n = m.item("test/wall_n.png").expect("the normal");
    assert_eq!(n.status, sdc_gui::model::items::ItemStatus::Ready);
    let ws = m.check_warnings("test/wall_n.png");
    assert!(
        ws.iter()
            .any(|w| w.code == Code::ChkGreenGuess && w.severity == Severity::Info),
        "{ws:?}"
    );
    let all = bottom::problems(&m);
    assert!(all.is_empty(), "{all:?}");
}

// ---- the card badge -----------------------------------------------------------------------------------------------

fn harness(model: AppModel, width: f32) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(egui::vec2(width, 1000.0))
        .with_step_dt(0.02)
        .build_ui_state(
            |ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model),
            model,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

#[test]
fn the_card_shows_check_results_on_an_info_badge_outside_the_warning_count() {
    let d = tempfile::tempdir().unwrap();
    let m = pbr_wall(d.path(), 20, true);
    assert!(m
        .check_warnings("test/wall_n.png")
        .iter()
        .any(|w| w.code == Code::ChkGreenGuess));
    let key = sdc_gui::model::sets::views(&m)
        .iter()
        .find(|v| v.key.ends_with("wall"))
        .unwrap()
        .key
        .clone();
    let mut h = harness(m, 1600.0);
    h.run();
    let badge = h.get_by_role_and_label(egui::accesskit::Role::Button, "Check results: 1");
    let badge_rect = badge.rect();
    // The visible text is "ⓘ 1", painted in the weak colour, not amber.
    let weak = h.ctx.global_style().visuals.weak_text_color();
    let painted = h.output().shapes.iter().any(|c| match &c.shape {
        egui::Shape::Text(t) => {
            t.galley.job.text == "ⓘ 1"
                && badge_rect.contains_rect(t.visual_bounding_rect())
                && t.galley.job.sections.iter().all(|s| s.format.color == weak)
        }
        _ => false,
    });
    assert!(painted, "\"ⓘ 1\" in the weak colour on the badge");
    assert!(
        h.query_all_by_label_contains("⚠").next().is_none(),
        "no ⚠ button"
    );
    let sets = egui::containers::panel::PanelState::load(&h.ctx, egui::Id::new("sets"))
        .expect("the Sets column")
        .outer_rect;
    assert!(
        h.query_all_by_label("ready")
            .any(|n| sets.contains_rect(n.rect())),
        "the card's dot says ready"
    );
    assert!(
        h.query_all_by_label("warning")
            .all(|n| !sets.contains_rect(n.rect())),
        "no amber dot on the card"
    );
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Check results: 1")
        .click();
    h.run();
    assert_eq!(h.state().bottom.filter, Some(ProblemsFilter::Set(key)));
    assert!(h.state().bottom.open);
    assert_eq!(h.state().bottom.tab, bottom::BottomTab::Problems);

    // On a card narrower than WIDE_CARD the badge wraps with the pill and stays inside the card.
    let d = tempfile::tempdir().unwrap();
    let m = pbr_wall(d.path(), 20, true);
    let mut h = harness(m, 1100.0);
    // The Sets column at its minimum: its card is under WIDE_CARD (CD9).
    h.ctx
        .data_mut(|d| d.insert_persisted(egui::Id::new("sets").with("chosen"), 0.0f32));
    h.run();
    let b = h
        .get_by_role_and_label(egui::accesskit::Role::Button, "Check results: 1")
        .rect();
    let sets = egui::containers::panel::PanelState::load(&h.ctx, egui::Id::new("sets"))
        .expect("the Sets column")
        .outer_rect;
    assert!(sets.width() < 200.0, "{sets:?}");
    // Review M7: inside the card's own frame, the smallest painted rect around the set's name and the chips below.
    let name = h
        .query_all_by_label("test/wall")
        .map(|n| n.rect())
        .filter(|r| sets.contains_rect(*r))
        .min_by(|a, b| a.min.x.total_cmp(&b.min.x))
        .expect("the card's name");
    let card = h
        .output()
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Rect(r) => Some(r.rect),
            _ => None,
        })
        .filter(|r| r.contains_rect(name) && r.height() > name.height() + 40.0)
        .min_by(|a, b| a.area().total_cmp(&b.area()))
        .expect("the card's frame");
    assert!(
        card.contains_rect(b),
        "badge {b:?} inside the card {card:?}"
    );
    assert!(b.min.y > name.max.y, "the badge wrapped below the name row");
}

/// Review I1: an undecided member's results are left out of the badge, as Problems leaves out its rows.
#[test]
fn an_undecided_members_check_results_are_not_on_the_badge() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_wall(d.path(), 20, false);
    let i = m
        .items
        .iter()
        .position(|i| i.key == "test/wall.png")
        .unwrap();
    m.items[i].status = sdc_gui::model::items::ItemStatus::NeedsDecision;
    m.invalidate_views();
    assert!(!m.check_warnings("test/wall.png").is_empty());
    assert!(check_rows(&m).is_empty());
    let mut h = harness(m, 1600.0);
    h.run();
    assert!(h.query_by_label_contains("Check results").is_none());
}

/// Review M3: a badge with only Info results is named for them, not "0".
#[test]
fn an_info_only_badge_is_named_info_only() {
    let d = tempfile::tempdir().unwrap();
    let m = pbr_wall(d.path(), 128, true);
    let mut h = harness(m, 1600.0);
    h.run();
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Check results: info only");
    assert!(h.query_by_label("Check results: 0").is_none());
}

#[test]
fn checks_settings_default_on_and_load_from_an_old_gui_toml() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("gui.toml");
    std::fs::write(&p, "preview_max = 2048\nreduce_motion = true\n").unwrap();
    let (s, err) = GuiSettings::load(&p);
    assert!(err.is_none(), "{err:?}");
    let c = s.checks;
    assert!(c.range && c.plastic && c.low_detail && c.green && c.parallax && c.uv);
    assert!(!c.drop_top_mip);
    let mut s = s;
    s.checks.plastic = false;
    s.save(&p).unwrap();
    let (back, _) = GuiSettings::load(&p);
    assert_eq!(back.checks, s.checks);
    assert!(!back.checks.plastic);
}

// ---- Task 3: the end-of-export summary and the green suggestion ---------------------------------------------------

#[test]
fn checks_summary_text_has_plurals_and_counts_sets_apart() {
    assert_eq!(
        summary_text(3, 1, 1),
        "Checks: 3 warnings on 1 texture and 1 set; see Problems"
    );
    assert_eq!(
        summary_text(1, 1, 0),
        "Checks: 1 warning on 1 texture; see Problems"
    );
    assert_eq!(
        summary_text(2, 0, 2),
        "Checks: 2 warnings on 2 sets; see Problems"
    );
}

#[test]
fn checks_summary_counts_warning_level_check_results_only() {
    let d = tempfile::tempdir().unwrap();
    let m = pbr_sets(d.path(), &[("a", 20), ("b", 20)], false);
    assert_eq!(
        checks_summary(&m).as_deref(),
        Some("Checks: 2 warnings on 2 textures; see Problems")
    );
    let d = tempfile::tempdir().unwrap();
    let m = pbr_wall(d.path(), 128, true); // only the Info CHK-GREEN-GUESS
    assert_eq!(checks_summary(&m), None);
}

#[test]
fn check_warnings_are_not_copied_into_the_export_rows() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_wall(d.path(), 20, false);
    m.export_clicked();
    assert!(m.wait_idle(Duration::from_secs(120)));
    assert!(m.export.rows.iter().all(|r| r
        .warnings
        .iter()
        .all(|w| !w.code.as_str().starts_with("CHK-"))));
    let rows = bottom::problems(&m);
    let chk: Vec<_> = rows
        .iter()
        .filter(|p| p.code == Some(Code::ChkRange))
        .collect();
    assert_eq!(chk.len(), 1, "{rows:?}");
    assert!(matches!(chk[0].at, At::Texture(_)));
    let want = "Checks: 1 warning on 1 texture; see Problems";
    let n = m.messages.entries().filter(|e| e.text == want).count();
    assert_eq!(n, 1, "the summary is logged once");
}

#[test]
fn a_check_row_click_selects_its_texture() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_wall(d.path(), 20, false);
    m.open_problems(None);
    let mut h = harness(m, 1400.0);
    h.run();
    h.get_by_label("test/wall.png").click();
    h.run();
    assert_eq!(
        h.state().selected_items(),
        vec!["test/wall.png".to_string()]
    );
}

#[test]
fn an_n_that_looks_opengl_offers_treat_as_opengl() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_wall(d.path(), 128, true);
    m.select_item("test/wall_n.png");
    let mut h = harness(m, 1400.0);
    h.run();
    h.get_by_label_contains("CHK-GREEN-GUESS");
    h.get_by_label("Treat as OpenGL (flip green)").click();
    h.run();
    assert!(h.state_mut().wait_idle(Duration::from_secs(60)));
    let j = h
        .state()
        .item("test/wall_n.png")
        .unwrap()
        .job
        .clone()
        .unwrap();
    assert_eq!(
        j.profile.channels.green_convention,
        Some(engine::config::GreenConvention::Opengl)
    );
}

#[test]
fn an_unstated_normal_that_looks_opengl_gets_the_hint_on_its_own_line() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("textures/test");
    std::fs::create_dir_all(&r).unwrap();
    gl_bumpy(&r.join("wall_normal.png"));
    let mut m = support::opened_as_the_app(d.path().join("textures"), d.path().join("export"));
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.select_item("test/wall_normal.png");
    let mut h = harness(m, 1400.0);
    h.run();
    h.get_by_label_contains("Looks OpenGL (curl test).");
    assert_eq!(h.query_all_by_label_contains("CHK-GREEN-GUESS").count(), 0);
}
