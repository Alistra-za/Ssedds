//! Check 3 in the GUI: half size is a Review question, answered before the export (DECISIONS Note FC2, which
//! supersedes the end-of-export box of CL24/CL27/CL43), and the remembered half size (`drop_top_mip`, Note CL).
mod support;
use eframe::egui::{self, Key};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use engine::codes::Code;
use image::{ImageBuffer, Rgb, RgbImage};
use sdc_gui::model::bottom::{problems, At, Level};
use sdc_gui::model::review::HalfAnswer;
use sdc_gui::model::{AppModel, Dialog};
use sdc_gui::settings::{ChecksSettings, GuiSettings};
use std::path::{Path, PathBuf};
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(120);

/// `n`² RGB white noise from a fixed LCG.
fn noise(n: u32, seed: u64) -> RgbImage {
    let mut s = seed;
    ImageBuffer::from_fn(n, n, |_, _| {
        let mut b = || {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (s >> 56) as u8
        };
        Rgb([b(), b(), b()])
    })
}

/// 512² noise upscaled 2x (nearest): mip 0 adds nothing over mip 1.
fn upscaled(seed: u64) -> RgbImage {
    let small = noise(512, seed);
    ImageBuffer::from_fn(1024, 1024, |x, y| *small.get_pixel(x / 2, y / 2))
}

/// `textures/` with `up.png`, `up2.png` (flagged) and `noise.png` (native 1024²), all diffuse, encoded ultrafast.
fn folder(dir: &Path) -> PathBuf {
    let r = dir.join("textures");
    std::fs::create_dir_all(&r).unwrap();
    noise(1024, 1).save(r.join("noise.png")).unwrap();
    upscaled(2).save(r.join("up.png")).unwrap();
    upscaled(3).save(r.join("up2.png")).unwrap();
    let mut toml = String::from("[defaults.encoder]\neffort = \"ultrafast\"\n");
    for n in ["noise.png", "up.png", "up2.png"] {
        toml += &format!("\n[files.\"{n}\"]\nsubtype = \"diffuse\"\n");
    }
    std::fs::write(r.join("sdc.toml"), toml).unwrap();
    r
}

/// The folder opened with "Measure whether full size adds anything" set to `measure`.
fn opened(dir: &Path, measure: bool) -> AppModel {
    let root = folder(dir);
    let settings = GuiSettings {
        export_folder: Some(dir.join("export")),
        checks: ChecksSettings {
            drop_top_mip: measure,
            ..ChecksSettings::default()
        },
        ..GuiSettings::default()
    };
    let mut m = AppModel::new(settings, None, support::session());
    m.open_folder(root);
    assert!(m.wait_idle(WAIT), "planning did not finish");
    support::choose_export(&mut m, dir.join("export"));
    m
}

/// The first export: nothing has been measured yet, so nothing is asked and everything is written at full size.
fn export(m: &mut AppModel) {
    m.export_clicked();
    assert!(m.review.is_none(), "the fixture needs no Review");
    assert!(m.wait_idle(WAIT));
}

/// Export through Review with these half-size answers.
fn answer_and_export(m: &mut AppModel, answers: &[(&str, HalfAnswer)]) {
    m.export_clicked();
    assert!(m.review.is_some(), "the half-size question opens Review");
    for (k, a) in answers {
        m.answer_half_size(k, *a);
    }
    m.export_from_review();
    assert!(m.wait_idle(WAIT));
}

fn job(m: &AppModel, item: &str) -> String {
    m.item(item).unwrap().job_key.clone()
}

/// The written DDS of `item`: (width, height).
fn dims(m: &AppModel, item: &str) -> (u32, u32) {
    let p = m.export_root().join(job(m, item));
    let b = std::fs::read(&p).unwrap_or_else(|e| panic!("{p:?}: {e}"));
    let u = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    (u(16), u(12))
}

fn toml(m: &AppModel) -> String {
    std::fs::read_to_string(m.root.clone().unwrap().join("sdc.toml")).unwrap()
}

fn drop_top_mip(m: &AppModel, item: &str) -> Option<serde_json::Value> {
    m.project.as_ref().unwrap().file_value(item, "drop_top_mip")
}

/// The texture keys of Review's half-size rows.
fn keys(m: &AppModel) -> Vec<String> {
    m.half_size_rows().iter().map(|r| r.key.clone()).collect()
}

fn logged(m: &AppModel, s: &str) -> usize {
    m.messages.entries().filter(|e| e.text.contains(s)).count()
}

/// The whole app, polled every frame as the real app does.
fn harness(model: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_ui_state(
            |ui, model: &mut AppModel| {
                model.poll();
                sdc_gui::ui::draw(ui, model)
            },
            model,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

#[test]
fn half_size_rows_come_from_the_last_export() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path(), true);
    assert!(keys(&m).is_empty(), "nothing measured before an export");
    export(&mut m);
    assert_eq!(
        dims(&m, "up.png"),
        (1024, 1024),
        "the first export writes full size"
    );
    assert_eq!(keys(&m), ["up.png", "up2.png"]);
    assert!(!m.export_is_clean());
    m.export_clicked();
    let r = m.review.as_ref().expect("Review opens on the question");
    assert!(
        r.half_size.values().all(Option::is_none),
        "unanswered by default"
    );
    assert_eq!(
        m.review_gate(),
        Err("Answer half or full size for 2 textures".to_string())
    );
    m.answer_half_size("up.png", HalfAnswer::Full);
    assert_eq!(
        m.review_gate(),
        Err("Answer half or full size for 1 texture".to_string())
    );
    let mut h = harness(m);
    h.run_steps(4);
    h.get_by_label("2 textures added nothing at full size last export");
    assert_eq!(h.query_all_by_label("Full size \u{25BE}").count(), 1);
    assert_eq!(
        h.query_all_by_label("Half or full size? \u{25BE}").count(),
        1
    );
}

/// No window after the export, over the whole run, and no second pass: the export runs once.
#[test]
fn no_dialog_after_export() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path(), true);
    export(&mut m);
    m.export_clicked();
    m.answer_half_size("up.png", HalfAnswer::Half);
    m.answer_half_size("up2.png", HalfAnswer::Full);
    let finished = logged(&m, "Export finished");
    m.export_from_review();
    let mut h = harness(m);
    let start = std::time::Instant::now();
    loop {
        h.run_steps(1);
        assert!(
            h.query_by_label("Export at half size?").is_none(),
            "the old box is gone"
        );
        let s = h.state();
        if s.export.running.is_none() && !s.export_queued() && s.export.summary.is_some() {
            break;
        }
        assert!(start.elapsed() < WAIT, "the export did not end");
        std::thread::sleep(Duration::from_millis(20));
    }
    h.run_steps(3);
    assert!(h.query_by_label("Export at half size?").is_none());
    let mut m = h.into_state();
    assert!(m.wait_idle(WAIT));
    assert_eq!(m.dialog, None);
    assert_eq!(logged(&m, "Export finished"), finished + 1, "one pass");
    assert_eq!(dims(&m, "up.png"), (512, 512));
}

#[test]
fn esc_and_close_write_nothing() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path(), true);
    export(&mut m);
    let before = toml(&m);
    m.export_clicked();
    m.answer_half_size("up.png", HalfAnswer::Half);
    m.answer_half_size("up2.png", HalfAnswer::Half);
    let mut h = harness(m);
    h.run_steps(3);
    h.key_press(Key::Escape);
    h.run_steps(3);
    assert!(h.state().export.running.is_none(), "Esc started nothing");
    h.get_by_label("Close").click();
    h.run_steps(3);
    let mut m = h.into_state();
    assert!(m.review.is_none());
    assert!(m.wait_idle(WAIT));
    assert_eq!(drop_top_mip(&m, "up.png"), None);
    assert_eq!(toml(&m), before);
    assert_eq!(dims(&m, "up.png"), (1024, 1024));
    assert_eq!(m.dialog, None);
}

#[test]
fn answer_half_writes_drop_top_mip_on_export() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path(), true);
    export(&mut m);
    answer_and_export(
        &mut m,
        &[("up.png", HalfAnswer::Half), ("up2.png", HalfAnswer::Full)],
    );
    let t = toml(&m);
    assert!(
        t.contains("[files.\"up.png\"]") && t.matches("drop_top_mip = true").count() == 1,
        "{t}"
    );
    assert_eq!(drop_top_mip(&m, "up.png"), Some(serde_json::json!(true)));
    assert_eq!(dims(&m, "up.png"), (512, 512));
    assert_eq!(dims(&m, "up2.png"), (1024, 1024));
    assert_eq!(dims(&m, "noise.png"), (1024, 1024));
    // CL20 "flagged again next time": the Full answer is for that export only.
    assert_eq!(keys(&m), ["up2.png"]);
    assert_eq!(m.dialog, None);
}

#[test]
fn answer_full_writes_nothing() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path(), true);
    export(&mut m);
    let before = toml(&m);
    answer_and_export(
        &mut m,
        &[("up.png", HalfAnswer::Full), ("up2.png", HalfAnswer::Full)],
    );
    assert!(m.export.summary.is_some(), "the export ran");
    assert_eq!(drop_top_mip(&m, "up.png"), None);
    assert_eq!(drop_top_mip(&m, "up2.png"), None);
    assert_eq!(toml(&m), before);
    assert_eq!(dims(&m, "up.png"), (1024, 1024));
}

/// Review Focus 2: answered half, then the plan no longer flags the texture: nothing is written for it.
#[test]
fn an_unflagged_texture_gets_no_drop_top_mip() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path(), true);
    export(&mut m);
    m.export_clicked();
    m.answer_half_size("up.png", HalfAnswer::Half);
    m.answer_half_size("up2.png", HalfAnswer::Full);
    m.set_field("up.png", "resize", serde_json::json!("half"));
    assert!(m.wait_idle(WAIT));
    assert_eq!(keys(&m), ["up2.png"], "a resized texture is not eligible");
    m.export_from_review();
    assert!(m.wait_idle(WAIT));
    assert!(m.export.summary.is_some(), "the export ran");
    assert_eq!(drop_top_mip(&m, "up.png"), None);
    assert!(!toml(&m).contains("drop_top_mip"));
}

/// Stop during an export cancels at the next file (CO50's Generate cancel), and nothing waits after it.
#[test]
fn stop_cancels_at_the_next_file() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path(), true);
    m.export_clicked();
    assert!(m.export.running.is_some());
    m.cancel_export();
    assert!(m.wait_idle(WAIT));
    assert!(!m.export_queued(), "no pass queued after a stop");
    assert!(
        !matches!(m.dialog, Some(Dialog::Error(_))),
        "{:?}",
        m.dialog
    );
    let written = ["noise.png", "up.png", "up2.png"]
        .iter()
        .filter(|i| m.export_root().join(job(&m, i)).exists())
        .count();
    assert!(written < 3, "the stop reached the files not yet started");
    let s = m.export.summary.as_ref().expect("a cancelled export ends");
    assert!(!s.cancelled.is_empty());
}

/// Note FC2 (supersedes CL26's dialog): Copy settings lists the half size as a tick like any other field.
#[test]
fn copy_settings_ticks_half_size() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path(), true);
    m.set_field("up.png", "drop_top_mip", serde_json::json!(true));
    m.set_field("up.png", "header", serde_json::json!("dx10"));
    assert!(m.wait_idle(WAIT));
    m.click_item("noise.png", false, false);
    m.click_item("up2.png", true, false);
    m.click_item("up.png", true, false);
    m.open_copy_settings();
    let fields = match &m.dialog {
        Some(Dialog::CopySettings { fields, .. }) => fields.clone(),
        d => panic!("{d:?}"),
    };
    assert!(
        fields.contains(&("drop_top_mip".to_string(), true)),
        "{fields:?}"
    );
    let mut h = harness(m);
    h.run_steps(3);
    h.get_by_label("Half size: true");
    h.get_by_label("Apply").click();
    h.run_steps(3);
    assert!(h.query_by_label("Copy the half size too?").is_none());
    let m = h.into_state();
    assert_eq!(m.dialog, None);
    assert_eq!(drop_top_mip(&m, "up2.png"), Some(serde_json::json!(true)));
    assert_eq!(drop_top_mip(&m, "noise.png"), Some(serde_json::json!(true)));
    assert_eq!(
        m.project.as_ref().unwrap().file_value("up2.png", "header"),
        Some(serde_json::json!("dx10"))
    );
    assert_eq!(m.undo_label(), Some("Copy settings from up.png"));
}

#[test]
fn a_saved_half_size_is_not_asked_again_and_undo_removes_it_in_one_step() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path(), true);
    export(&mut m);
    answer_and_export(
        &mut m,
        &[("up.png", HalfAnswer::Half), ("up2.png", HalfAnswer::Half)],
    );
    assert_eq!(dims(&m, "up2.png"), (512, 512));
    assert!(keys(&m).is_empty(), "nothing is eligible any more");
    m.undo();
    assert!(m.wait_idle(WAIT));
    assert_eq!(drop_top_mip(&m, "up.png"), None);
    assert_eq!(drop_top_mip(&m, "up2.png"), None);
}

#[test]
fn the_cli_command_reproduces_the_half_size_output() {
    use engine::api::*;
    use engine::events::{new_batch_id, Reporter, VecSink};
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path(), true);
    export(&mut m);
    answer_and_export(
        &mut m,
        &[("up.png", HalfAnswer::Half), ("up2.png", HalfAnswer::Full)],
    );
    let root = m.root.clone().unwrap();
    let mut req = BatchRequest::new(
        vec![root.clone()],
        d.path().join("cli"),
        engine::config::Target::Cs,
    );
    req.recursive = true;
    req.accept = engine::policy::AcceptSuggestions::High;
    req.project_config = Some(root.join("sdc.toml"));
    let sink = std::sync::Arc::new(VecSink::default());
    let rep = Reporter::new(sink, new_batch_id());
    let e = Engine::new().unwrap();
    let plan = e.plan(req, &rep, &CancelToken::default()).unwrap();
    let up = plan
        .jobs
        .iter()
        .find(|j| j.input.ends_with("up.png"))
        .unwrap();
    assert!(up.profile.drop_top_mip);
    e.run(plan, &rep, &CancelToken::default()).unwrap();
    for item in ["up.png", "up2.png"] {
        let k = job(&m, item);
        assert_eq!(
            std::fs::read(d.path().join("cli").join(&k)).unwrap(),
            std::fs::read(m.export_root().join(&k)).unwrap(),
            "{item}"
        );
    }
}

/// With the switch off, a flagged texture is written at full size and listed in Problems; Review asks nothing.
#[test]
fn with_the_switch_off_flagged_textures_export_full_and_are_listed() {
    use sdc_gui::model::bottom::{filtered, ProblemsFilter};
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path(), false);
    export(&mut m);
    assert_eq!(m.dialog, None);
    assert!(keys(&m).is_empty(), "no Review question without the switch");
    assert_eq!(dims(&m, "up.png"), (1024, 1024));
    let k = job(&m, "up.png");
    let row = m.export.rows.iter().find(|r| r.job_key == k).unwrap();
    assert!(row.warnings.iter().any(|w| w.code == Code::ChkLowDetail));
    // K2 / M9 (T2): the engine's CHK row is a check row, counted by the Checks filter and once per texture.
    let low = |m: &AppModel| {
        problems(m)
            .iter()
            .filter(|p| p.code == Some(Code::ChkLowDetail))
            .map(|p| (p.level, p.at.clone()))
            .collect::<Vec<_>>()
    };
    let rows = low(&m);
    assert!(
        rows.contains(&(Level::Check, At::File(k.clone()))),
        "{rows:?}"
    );
    assert_eq!(rows.len(), 2, "once per texture (up, up2): {rows:?}");
    assert!(rows.iter().all(|r| r.0 == Level::Check));
    assert!(filtered(&m, Some(&ProblemsFilter::Checks))
        .iter()
        .any(|p| p.code == Some(Code::ChkLowDetail)));
    // M3: the header Export dot reads `ready_rows`; turning the check off removes the 2 CHK rows and must leave
    // `ready_rows` unchanged (the fixture has other, real warnings, so the dot itself is not green here).
    let on = sdc_gui::model::bottom::flagged(&m);
    let summary_count = |m: &AppModel| -> usize {
        sdc_gui::model::checks::checks_summary(m)
            .and_then(|s| s.split_whitespace().nth(1)?.parse().ok())
            .unwrap_or(0)
    };
    let n_on = summary_count(&m);
    let mut off = m.settings.checks;
    off.low_detail = false;
    m.set_checks(off);
    m.poll();
    assert!(low(&m).is_empty());
    let f_off = sdc_gui::model::bottom::flagged(&m);
    assert_eq!(
        f_off.ready_rows, on.ready_rows,
        "a CHK row never counts as ready"
    );
    assert_eq!(f_off.check_rows + 2, on.check_rows);
    assert_eq!(summary_count(&m) + 2, n_on, "the summary counts each once");
}

#[test]
fn a_drop_top_mip_estimate_halves_the_size_and_writes_one_level_less() {
    use engine::config::{MipLevels, Profile};
    use sdc_gui::model::out_size::estimate;
    let d = tempfile::tempdir().unwrap();
    let m = opened(d.path(), true);
    let mut p: Profile = m.item("up.png").unwrap().job.clone().unwrap().profile;
    let full = estimate((1024, 1024), &p);
    p.drop_top_mip = true;
    let half = estimate((1024, 1024), &p);
    assert_eq!((half.width, half.height), (512, 512));
    assert_eq!(half.mips, full.mips - 1, "full stays full");
    assert_eq!(
        half.chain_bytes,
        estimate((512, 512), &{
            let mut q = p.clone();
            q.drop_top_mip = false;
            q
        })
        .chain_bytes
    );
    // M4: n levels with the top dropped writes n - 1.
    p.mips.levels = MipLevels::Count(5);
    assert_eq!(estimate((1024, 1024), &p).mips, 4);
}

/// Owner ruling O1: a saved half size has a row in the texture's Advanced settings; Clear removes the key (M9: it
/// never writes `false`).
#[test]
fn the_advanced_row_shows_a_saved_half_size_and_clear_removes_it() {
    use sdc_gui::model::fields::advanced_field_views;
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path(), true);
    export(&mut m);
    answer_and_export(
        &mut m,
        &[("up.png", HalfAnswer::Half), ("up2.png", HalfAnswer::Full)],
    );
    let j = m.item("up.png").unwrap().job.clone().unwrap();
    let v = advanced_field_views(m.target, &j)
        .into_iter()
        .find(|v| v.field == "drop_top_mip")
        .unwrap();
    assert_eq!(v.value, serde_json::json!(true));
    assert!(v.edited, "Clear (↺) shows for the texture's own value");
    m.settings.advanced_open = true;
    m.select_item("up.png");
    let mut h = harness(m);
    h.run();
    h.get_by_label("Half size");
    let mut m = h.into_state();
    m.clear_field("up.png", "drop_top_mip");
    assert!(m.wait_idle(WAIT));
    assert_eq!(m.undo_label(), Some("Reset drop_top_mip"));
    m.save().unwrap();
    assert!(!toml(&m).contains("drop_top_mip"), "Clear removes the key");
    // Unticking the row is a Clear too, never a `false`.
    m.set_field("up.png", "drop_top_mip", serde_json::json!(true));
    m.set_field("up.png", "drop_top_mip", serde_json::json!(false));
    assert_eq!(drop_top_mip(&m, "up.png"), None);
}

/// M6: one mip level has nothing to drop: the Advanced row is locked with the reason, and no write sets it.
#[test]
fn the_half_size_row_is_locked_without_mips() {
    use sdc_gui::model::fields::advanced_field_views;
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path(), true);
    m.set_field("up.png", "mips.levels", serde_json::json!("none"));
    assert!(m.wait_idle(WAIT));
    let j = m.item("up.png").unwrap().job.clone().unwrap();
    let v = advanced_field_views(m.target, &j)
        .into_iter()
        .find(|v| v.field == "drop_top_mip")
        .unwrap();
    assert_eq!(v.lock, Some("No mips to drop"));
    m.set_field("up.png", "drop_top_mip", serde_json::json!(true));
    assert_eq!(drop_top_mip(&m, "up.png"), None);
}
