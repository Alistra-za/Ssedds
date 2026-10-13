//! Edge cases (M3a spec §7): a source changed on disk, a worker panic, and no usable GPU.
mod support;
use engine::api::CancelToken;
use engine::config::Subtype;
use image::{ImageBuffer, Rgb};
use sdc_gui::model::{AfterExport, Answer, AppModel, Dialog, DocKind};
use sdc_gui::settings::GuiSettings;
use sdc_gui::worker::*;
use std::time::Duration;
use support::*;

#[test]
fn a_source_changed_on_disk_is_flagged_and_reloads_alone() {
    let d = tempfile::tempdir().unwrap();
    // Its own session: the shared one counts the loads of tests running in parallel.
    let session =
        std::sync::Arc::new(engine::session::Session::new(encoders_sys::Bc1Model::Ideal).unwrap());
    let settings = sdc_gui::settings::GuiSettings {
        export_folder: Some(d.path().join("export")),
        ..Default::default()
    };
    let mut m = sdc_gui::model::AppModel::new(settings, None, session);
    m.accept = engine::policy::AcceptSuggestions::None;
    m.open_folder(mod_folder(d.path()));
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.check_sources(), 0);
    let loads = m.session.as_ref().unwrap().cache().loads();
    let p = m.root.clone().unwrap().join("landscape/rock01.png");
    ImageBuffer::from_fn(32, 32, |_, _| Rgb([9u8, 9, 9]))
        .save(&p)
        .unwrap();
    assert_eq!(m.check_sources(), 1);
    assert!(m.item("landscape/rock01.png").unwrap().changed_on_disk);
    m.reload_changed();
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(!m.item("landscape/rock01.png").unwrap().changed_on_disk);
    assert_eq!(m.session.as_ref().unwrap().cache().loads(), loads + 1);
}

#[test]
fn a_worker_panic_is_reported_and_the_model_keeps_working() {
    let (tx, rx) = std::sync::mpsc::channel();
    spawn_guarded(tx, "test", |_| panic!("boom"));
    match rx.recv_timeout(Duration::from_secs(10)).unwrap() {
        WorkerMsg::Panicked { what, message } => {
            assert_eq!(what, "test");
            assert!(message.contains("boom"));
        }
        _ => panic!("expected a panic message"),
    }
}

#[test]
fn without_a_gpu_the_preview_says_so() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.gpu_available = false;
    m.expand("landscape/rock01_rmaos");
    m.encode_now();
    assert_eq!(
        m.preview.notice.as_deref(),
        Some("No usable GPU: the preview is off; everything else works")
    );
}

#[test]
fn a_packed_output_is_flagged_when_one_of_its_sources_changes() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let p = m.root.clone().unwrap().join("landscape/rock01_ao.png");
    ImageBuffer::from_fn(64, 64, |_, _| image::Luma([7u8]))
        .save(&p)
        .unwrap();
    // The raw map and the packed output it feeds.
    assert_eq!(m.check_sources(), 2);
    assert!(m.item("landscape/rock01_ao.png").unwrap().changed_on_disk);
    assert!(m.item("landscape/rock01_rmaos").unwrap().changed_on_disk);
    assert!(!m.item("landscape/rock01.png").unwrap().changed_on_disk);
}

#[test]
fn export_after_a_change_plans_again_and_then_exports() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.decide("landscape/rock01.png", Answer::Subtype(Subtype::Diffuse));
    assert!(m.wait_idle(Duration::from_secs(60)));
    let p = m.root.clone().unwrap().join("landscape/rock01.png");
    ImageBuffer::from_fn(32, 32, |_, _| Rgb([9u8, 9, 9]))
        .save(&p)
        .unwrap();
    m.export_clicked();
    // The change was seen, the plan is being remade, and the export waits for it.
    assert!(m.export.running.is_none());
    assert!(m.wait_idle(Duration::from_secs(120)));
    assert!(!m.item("landscape/rock01.png").unwrap().changed_on_disk);
    // Other textures are undecided, so the export asks first; proceeding exports the fresh plan.
    assert_eq!(m.dialog, Some(Dialog::ProceedAnyway(m.undecided_keys())));
    m.proceed_anyway();
    assert!(m.wait_idle(Duration::from_secs(120)));
    assert_eq!(m.export.summary.as_ref().unwrap().failed, 0);
    let out = std::fs::read(d.path().join("export/landscape/rock01.dds")).unwrap();
    let dds = engine::dds::read_dds(&out).unwrap();
    assert_eq!((dds.levels[0].0, dds.levels[0].1), (32, 32));
}

#[test]
fn a_dds_input_changed_on_disk_is_flagged_and_its_preview_read_again() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.decide("landscape/rock01.png", Answer::Subtype(Subtype::Diffuse));
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.export_clicked();
    m.proceed_anyway();
    assert!(m.wait_idle(Duration::from_secs(120)));
    let root = m.root.clone().unwrap();
    let dds = d.path().join("export/landscape/rock01.dds");
    std::fs::copy(&dds, root.join("landscape/stored.dds")).unwrap();
    m.open_folder(root.clone());
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.expand("landscape/stored.dds");
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.check_sources(), 0);
    let first = m.preview.encoded.shown().cloned().unwrap();
    // A different DDS (a 32 px one) replaces the file.
    ImageBuffer::from_fn(32, 32, |_, _| Rgb([9u8, 9, 9]))
        .save(root.join("landscape/small.png"))
        .unwrap();
    m.open_folder(root.clone());
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.decide("landscape/small.png", Answer::Subtype(Subtype::Diffuse));
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.export_clicked();
    m.proceed_anyway();
    assert!(m.wait_idle(Duration::from_secs(120)));
    m.expand("landscape/stored.dds");
    assert!(m.wait_idle(Duration::from_secs(60)));
    std::fs::copy(
        d.path().join("export/landscape/small.dds"),
        root.join("landscape/stored.dds"),
    )
    .unwrap();
    assert_eq!(m.check_sources(), 1);
    assert!(m.item("landscape/stored.dds").unwrap().changed_on_disk);
    m.reload_changed();
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(!m.item("landscape/stored.dds").unwrap().changed_on_disk);
    let second = m.preview.encoded.shown().cloned().unwrap();
    assert_ne!(first.result.levels[0].len(), second.result.levels[0].len());
}

#[test]
fn opening_another_folder_cancels_an_export_waiting_for_a_plan() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let p = m.root.clone().unwrap().join("landscape/rock01.png");
    ImageBuffer::from_fn(32, 32, |_, _| Rgb([9u8, 9, 9]))
        .save(&p)
        .unwrap();
    m.export_clicked();
    // The export waits for the new plan; the user opens another folder before it arrives.
    let other = tempfile::tempdir().unwrap();
    m.open_folder(mod_folder(other.path()));
    assert!(m.wait_idle(Duration::from_secs(120)));
    assert!(m.export.running.is_none());
    assert!(m.export.summary.is_none());
    assert_eq!(m.dialog, None);
}

#[test]
fn export_during_a_plan_waits_for_it_and_then_asks_about_undecided_textures() {
    let d = tempfile::tempdir().unwrap();
    let root = mod_folder(d.path());
    let settings = GuiSettings {
        export_folder: Some(d.path().join("export")),
        ..GuiSettings::default()
    };
    let mut m = AppModel::new(settings, None, session());
    m.accept = engine::policy::AcceptSuggestions::None;
    m.open_folder(root);
    // The plan has not landed: the item list is still empty, so a count of it would say "nothing to decide".
    m.export_clicked();
    assert!(m.export.running.is_none(), "nothing starts before the plan");
    assert_eq!(m.dialog, None);
    assert!(m.wait_idle(Duration::from_secs(120)));
    assert!(m.export.running.is_none());
    assert_eq!(m.dialog, Some(Dialog::ProceedAnyway(m.undecided_keys())));
    assert!(m.undecided() > 0);
}

/// Open a mod folder whose `sdc.toml` is `text`.
fn opened_with_toml(d: &std::path::Path, text: &str) -> AppModel {
    let root = mod_folder(d);
    std::fs::write(root.join("sdc.toml"), text).unwrap();
    let settings = GuiSettings {
        export_folder: Some(d.join("export")),
        ..GuiSettings::default()
    };
    let mut m = AppModel::new(settings, None, session());
    m.accept = engine::policy::AcceptSuggestions::None;
    m.open_folder(root);
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
    m
}

/// Spec §7.4: a config the engine rejects is ignored (the folder still plans), the banner says why, the GUI is
/// read-only and the file is untouched; fixing the file and reloading brings the edits back.
fn assert_rejected_then_fixed(bad: &str, expect_in_banner: &str) {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened_with_toml(d.path(), bad);
    let toml = m.root.clone().unwrap().join("sdc.toml");
    assert_eq!(m.dialog, None, "no modal loop");
    assert!(
        !m.items.is_empty(),
        "the folder is planned without sdc.toml"
    );
    let banner = m.banner.clone().expect("a banner");
    assert!(
        banner.contains(expect_in_banner) && banner.contains("sdc.toml"),
        "{banner}"
    );
    assert!(m.project.as_ref().unwrap().read_only());
    m.set_field(
        "landscape/rock01.png",
        "alpha.threshold",
        serde_json::json!(90),
    );
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(!m.is_dirty(), "edits are refused");
    assert_eq!(std::fs::read_to_string(&toml).unwrap(), bad);

    std::fs::write(&toml, "# fixed\n").unwrap();
    m.resolve_external_change(DocKind::SdcToml, false);
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.banner, None);
    assert_eq!(m.dialog, None);
    assert!(!m.project.as_ref().unwrap().read_only());
    m.set_field(
        "landscape/rock01.png",
        "alpha.threshold",
        serde_json::json!(90),
    );
    assert!(m.is_dirty(), "editable again");
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.dialog, None);
}

#[test]
fn an_unknown_field_in_a_files_entry_makes_the_project_read_only() {
    assert_rejected_then_fixed(
        "[files.\"landscape/rock01.png\"]\nbogus_field = 1\n",
        "bogus_field",
    );
}

#[test]
fn an_unknown_allow_code_in_a_files_entry_makes_the_project_read_only() {
    assert_rejected_then_fixed(
        "[files.\"landscape/rock01.png\"]\nallow = [\"NO-SUCH-CODE\"]\n",
        "NO-SUCH-CODE",
    );
}

#[test]
fn an_out_of_range_default_makes_the_project_read_only() {
    assert_rejected_then_fixed("[defaults]\nalpha.threshold = 1000\n", "1000");
}

#[test]
fn an_unknown_sub_type_in_a_files_entry_makes_the_project_read_only() {
    assert_rejected_then_fixed(
        "[files.\"landscape/rock01.png\"]\nsubtype = \"nonsense\"\n",
        "nonsense",
    );
}

/// A model whose export has been started but not polled, so it is still running.
fn exporting(d: &std::path::Path) -> AppModel {
    let mut m = opened(d);
    m.export_clicked();
    m.proceed_anyway();
    assert!(m.export.running.is_some());
    m
}

#[test]
fn closing_during_an_export_asks_and_closes_after_it_was_cancelled() {
    let d = tempfile::tempdir().unwrap();
    let mut m = exporting(d.path());
    assert!(!m.request_close());
    assert_eq!(m.dialog, Some(Dialog::ExportRunning(AfterExport::Close)));
    // Keep running: nothing is cancelled.
    m.resolve_export_running(AfterExport::Close, false);
    assert_eq!(m.dialog, None);
    assert!(!m.export.running.as_ref().unwrap().is_cancelled());
    assert!(!m.take_close_ready());
    // Cancel and close: the token is set, and the window may close once the export has ended.
    assert!(!m.request_close());
    m.resolve_export_running(AfterExport::Close, true);
    assert!(m.export.running.as_ref().unwrap().is_cancelled());
    assert!(!m.take_close_ready(), "not before the export has ended");
    assert!(m.wait_idle(Duration::from_secs(120)));
    assert!(m.take_close_ready());
    assert!(!m.take_close_ready(), "once");
    assert!(m.request_close());
}

#[test]
fn opening_during_an_export_asks_and_opens_after_it_was_cancelled() {
    let d = tempfile::tempdir().unwrap();
    let mut m = exporting(d.path());
    let other_dir = tempfile::tempdir().unwrap();
    let other = mod_folder(other_dir.path());
    assert!(!m.request_open(other.clone()));
    assert_eq!(
        m.dialog,
        Some(Dialog::ExportRunning(AfterExport::Open(other.clone())))
    );
    m.resolve_export_running(AfterExport::Open(other.clone()), true);
    assert!(m.export.running.as_ref().unwrap().is_cancelled());
    assert_ne!(
        m.root,
        Some(other.clone()),
        "not before the export has ended"
    );
    assert!(m.wait_idle(Duration::from_secs(120)));
    assert_eq!(m.root, Some(other));
    assert_eq!(m.dialog, None);
    // The drawer belongs to the folder it ran on.
    assert!(m.export.rows.is_empty() && m.export.summary.is_none() && m.export.events.is_empty());
}

#[test]
fn an_export_that_ends_by_itself_drops_the_question_and_keeps_the_window() {
    let d = tempfile::tempdir().unwrap();
    let mut m = exporting(d.path());
    assert!(!m.request_close());
    assert!(m.wait_idle(Duration::from_secs(120)));
    assert_eq!(m.dialog, None);
    assert!(!m.take_close_ready(), "an unanswered question is not a yes");
    assert!(m.export.summary.is_some(), "the report is there to read");
}

#[test]
fn a_worker_panic_in_the_export_still_lets_the_pending_close_happen() {
    let mut m = AppModel::placeholder(GuiSettings::default());
    m.export.running = Some(CancelToken::default());
    assert!(!m.request_close());
    m.resolve_export_running(AfterExport::Close, true);
    assert!(m.export.running.as_ref().unwrap().is_cancelled());
    // The export worker dies instead of reporting.
    m.sender()
        .send(WorkerMsg::Panicked {
            what: "export",
            message: "boom".into(),
        })
        .unwrap();
    m.poll();
    assert!(m.export.running.is_none());
    assert!(m.take_close_ready());
    assert!(matches!(m.dialog, Some(Dialog::Error(_))));
}

#[test]
fn a_dropped_folder_opens_and_a_dropped_file_opens_its_parent() {
    let d = tempfile::tempdir().unwrap();
    let root = mod_folder(d.path());
    let file = root.join("landscape/rock01.png");
    assert_eq!(AppModel::drop_folder(&root), Some(root.clone()));
    assert_eq!(AppModel::drop_folder(&file), Some(root.join("landscape")));
    assert_eq!(AppModel::drop_folder(&root.join("nope.png")), None);

    let mut m = AppModel::new(GuiSettings::default(), None, session());
    m.accept = engine::policy::AcceptSuggestions::None;
    // A path that names nothing is skipped; the first usable one opens.
    assert!(m.dropped(&[root.join("nope.png"), file.clone()]));
    assert_eq!(m.root, Some(root.join("landscape")));
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(!m.dropped(&[root.join("nope.png")]));
    assert_eq!(m.root, Some(root.join("landscape")));
}

#[test]
fn a_drop_during_an_export_asks_first() {
    let d = tempfile::tempdir().unwrap();
    let mut m = exporting(d.path());
    let od = tempfile::tempdir().unwrap();
    let other = mod_folder(od.path());
    assert!(!m.dropped(std::slice::from_ref(&other)));
    assert_eq!(
        m.dialog,
        Some(Dialog::ExportRunning(AfterExport::Open(other)))
    );
}

#[test]
fn a_drop_with_unsaved_edits_asks_first() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_target(engine::config::Target::Vanilla);
    assert!(m.is_dirty());
    let od = tempfile::tempdir().unwrap();
    let other = mod_folder(od.path());
    assert!(!m.dropped(std::slice::from_ref(&other)));
    assert_eq!(m.dialog, Some(Dialog::OpenUnsaved(other)));
}

#[test]
fn a_user_config_the_engine_rejects_is_not_blamed_on_sdc_toml() {
    let d = tempfile::tempdir().unwrap();
    let root = mod_folder(d.path());
    let user = d.path().join("config.toml");
    std::fs::write(&user, "[defaults]\nalpha.threshold = 1000\n").unwrap();
    std::fs::write(root.join("sdc.toml"), "# fine\n").unwrap();
    let mut m = AppModel::new(GuiSettings::default(), None, session());
    m.accept = engine::policy::AcceptSuggestions::None;
    m.user_config = Some(user);
    m.open_folder(root);
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(matches!(m.dialog, Some(Dialog::Error(ref e)) if e.contains("1000")));
    assert_eq!(m.banner, None, "sdc.toml is not the culprit");
    assert!(!m.project.as_ref().unwrap().read_only());
}

/// Pretend an export is running: a token the model treats as the running export (no engine work starts).
fn fake_running(m: &mut sdc_gui::model::AppModel) {
    m.export.running = Some(engine::api::CancelToken::default());
}

fn finish_export(m: &mut sdc_gui::model::AppModel) {
    m.sender()
        .send(WorkerMsg::ExportDone(Ok(
            engine::api::BatchSummary::default(),
        )))
        .unwrap();
    m.poll();
}

#[test]
fn keep_running_after_a_cancelled_close_clears_the_pending_close() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    fake_running(&mut m);
    assert!(!m.request_close());
    assert_eq!(m.dialog, Some(Dialog::ExportRunning(AfterExport::Close)));
    m.resolve_export_running(AfterExport::Close, true); // cancel and close
                                                        // The export is still winding down; now the user asks to open another folder and keeps the export running.
    let other = d.path().join("other");
    std::fs::create_dir_all(&other).unwrap();
    assert!(!m.request_open(other.clone()));
    m.resolve_export_running(AfterExport::Open(other), false); // keep running
    finish_export(&mut m);
    assert!(!m.take_close_ready(), "the stale close must not fire");
    assert_ne!(m.root.as_deref(), Some(d.path().join("other").as_path()));
}

#[test]
fn a_second_answered_request_replaces_the_first() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    fake_running(&mut m);
    m.request_close();
    m.resolve_export_running(AfterExport::Close, true);
    let other = d.path().join("other2");
    std::fs::create_dir_all(&other).unwrap();
    m.request_open(other.clone());
    m.resolve_export_running(AfterExport::Open(other.clone()), true);
    finish_export(&mut m);
    // Exactly the latest action runs: the folder opens, the window does not close.
    assert!(!m.take_close_ready());
    assert_eq!(m.root.as_deref(), Some(other.as_path()));
    // A later export end triggers nothing.
    fake_running(&mut m);
    finish_export(&mut m);
    assert!(!m.take_close_ready());
    assert_eq!(m.root.as_deref(), Some(other.as_path()));
}

#[test]
fn opening_a_folder_clears_a_pending_action() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    fake_running(&mut m);
    m.request_close();
    m.resolve_export_running(AfterExport::Close, true);
    // The export ends without a message (e.g. the worker's channel closed), and the user opens a folder directly.
    m.export.running = None;
    m.open_folder(mod_folder(&d.path().join("again")));
    fake_running(&mut m);
    finish_export(&mut m);
    assert!(!m.take_close_ready());
}
