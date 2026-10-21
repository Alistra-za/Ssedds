//! Track A Task 11 (DECISIONS Note CM): the GUI model of "Also export as" Vanilla / CM: the ticks, Review's gate,
//! the worker and the Files rows (C1 under layout v2: Review opens while a tick is on; its Export is the only way out).
mod support;
use engine::codes::{Code, Warning};
use engine::events::{EngineEvent, EventKind, EVENT_SCHEMA};
use engine::presets::{AlsoTarget, FolderNames};
use image::{ImageBuffer, Rgb, Rgba};
use sdc_gui::model::bottom::{problems, At};
use sdc_gui::model::export::{cli_command, RowState};
use sdc_gui::model::review::DiskState;
use sdc_gui::model::{AppModel, Dialog};
use std::path::{Path, PathBuf};
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(120);

fn png(path: &Path, px: [u8; 4]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    ImageBuffer::from_pixel(16, 16, Rgba(px))
        .save(path)
        .unwrap();
}

/// `<tmp>/mods/Mod`: a PBR set `textures/pbr/arm/steel`, a plain set `textures/clutter/cup` and a complex-material
/// candidate `textures/clutter/iron` (+ `_m`); exports go to `<tmp>/out`. Opened as the app opens a folder.
fn project(d: &Path) -> AppModel {
    let m = d.join("mods/Mod");
    let t = m.join("textures");
    png(&t.join("pbr/arm/steel.png"), [200, 200, 200, 255]);
    png(&t.join("pbr/arm/steel_n.png"), [128, 128, 255, 255]);
    png(&t.join("pbr/arm/steel_rmaos.png"), [128, 255, 200, 255]);
    let c = t.join("clutter");
    std::fs::create_dir_all(&c).unwrap();
    ImageBuffer::from_fn(16, 16, |x, y| {
        Rgb([(x * 16) as u8, (y * 16) as u8, ((x + y) * 8) as u8])
    })
    .save(c.join("cup.png"))
    .unwrap();
    png(&c.join("cup_n.png"), [128, 128, 255, 255]);
    support::opened_as_the_app(m, d.join("out"))
}

fn idle(m: &mut AppModel) {
    assert!(m.wait_idle(WAIT), "the model did not settle");
}

/// Export as a user would through Review: Export, acknowledge, Review's Export, wait.
fn export(m: &mut AppModel) {
    support::export_through(m);
    idle(m);
}

fn tick(m: &mut AppModel, t: AlsoTarget) {
    m.set_also_tick(t, true);
    idle(m);
    assert!(m.also_on(), "the tick took: {}", m.status_line);
}

fn sdc_toml(m: &AppModel) -> PathBuf {
    m.root.clone().unwrap().join("sdc.toml")
}

fn steel_id(m: &AppModel) -> String {
    sdc_gui::model::sets::views(m)
        .iter()
        .find(|v| v.key.ends_with("steel"))
        .expect("the steel set")
        .id
        .clone()
}

/// Every row ended (item 5: no row stays Queued or Running after an export ends).
fn all_rows_ended(m: &AppModel) {
    for r in &m.export.rows {
        assert!(
            !matches!(r.state, RowState::Queued | RowState::Running),
            "{} is {:?}",
            r.job_key,
            r.state
        );
    }
}

#[test]
fn a_tick_opens_review_and_reviews_export_is_the_only_way_out() {
    let d = tempfile::tempdir().unwrap();
    let mut m = project(d.path());
    tick(&mut m, AlsoTarget::Vanilla);
    m.export_clicked();
    assert!(m.review.is_some());
    assert_eq!(m.dialog, None);
    assert!(
        m.export.running.is_none(),
        "the header's Export never exports"
    );
    // A plan in flight: the click waits, and the landed plan still opens Review, not the export.
    m.close_review();
    m.replan();
    m.export_clicked();
    idle(&mut m);
    assert!(m.review.is_some() && m.export.summary.is_none());
    m.acknowledge_all_asks();
    m.export_from_review();
    idle(&mut m);
    assert_eq!(m.dialog, None, "{:?}", m.export.notes);
    let [p, v, c] = m.also_folders();
    assert!(p.join("textures/pbr/arm/steel.dds").is_file());
    assert!(v.join("textures/arm/steel.dds").is_file());
    assert!(!c.exists(), "Vanilla only: no CM folder");
    // The GUI names the folders as the engine does (also_file and the JSON root rely on it).
    assert_eq!(
        m.also_folders()[..2],
        m.also_folders_written().unwrap()[..2]
    );
    all_rows_ended(&m);
}

#[test]
fn nothing_ticked_exports_exactly_as_today() {
    let d = tempfile::tempdir().unwrap();
    let mut m = project(d.path());
    assert!(!m.also_on());
    assert_eq!(m.output_root(), m.export_root());
    assert_eq!(m.request().unwrap().skip_root, None);
    m.export_clicked();
    if m.review.is_some() {
        m.acknowledge_all_asks();
        m.export_from_review();
    }
    idle(&mut m);
    let out = d.path().join("out");
    assert!(out.join("textures/pbr/arm/steel.dds").is_file());
    let names: Vec<_> = std::fs::read_dir(&out)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        names.iter().all(|n| !n.starts_with("Mod ")),
        "no Also folder: {names:?}"
    );
    assert!(!std::fs::read_to_string(sdc_toml(&m))
        .unwrap_or_default()
        .contains("[export"));
}

#[test]
fn a_tick_is_one_undo_step_and_exports_three_folders() {
    let d = tempfile::tempdir().unwrap();
    let mut m = project(d.path());
    let before = m.project.as_ref().unwrap().text();
    tick(&mut m, AlsoTarget::Cm);
    assert_eq!(m.also_ticks(), (false, true));
    assert_eq!(m.undo_label(), Some("Also export as"));
    m.undo();
    idle(&mut m);
    assert!(!m.also_on());
    assert_eq!(m.project.as_ref().unwrap().text(), before);
    tick(&mut m, AlsoTarget::Cm);
    export(&mut m);
    let [p, v, c] = m.also_folders();
    assert!(p.join("textures/pbr/arm/steel.dds").is_file());
    assert!(
        v.join("textures/arm/steel.dds").is_file(),
        "CM writes Vanilla too"
    );
    assert!(v.join("textures/clutter/cup.dds").is_file());
    assert!(c.join("textures/arm/steel_m.dds").is_file());
    assert!(
        p.join("PBRNIFPatcher")
            .read_dir()
            .is_ok_and(|mut d| d.next().is_some()),
        "the PGPatcher JSON is in the PBR folder"
    );
    assert!(m
        .export
        .rows
        .iter()
        .any(|r| r.job_key.starts_with("Mod Vanilla/textures/")));
    assert!(m
        .export
        .notes
        .iter()
        .any(|n| n.contains("Mod conversion-report.md")));
    all_rows_ended(&m);
    let (done, total) = m.export.progress();
    assert_eq!(done, total);
}

#[test]
fn a_read_only_sdc_toml_disables_reviews_export_with_the_reason() {
    let d = tempfile::tempdir().unwrap();
    let mut m = project(d.path());
    tick(&mut m, AlsoTarget::Vanilla);
    m.save().unwrap();
    let path = sdc_toml(&m);
    let bytes = std::fs::read(&path).unwrap();
    let set_ro = |on: bool| {
        let mut p = std::fs::metadata(&path).unwrap().permissions();
        p.set_readonly(on);
        std::fs::set_permissions(&path, p).unwrap();
    };
    set_ro(true);
    m.export_clicked();
    let reason = "sdc.toml is read-only: Also export settings could not be saved";
    assert_eq!(m.review_gate(), Err(reason.to_string()));
    m.export_from_review();
    idle(&mut m);
    assert!(m.export.summary.is_none(), "nothing exported");
    assert!(m.status_line.contains(reason), "{}", m.status_line);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    // Taken once (Review open, Export click), not per frame: clearing the flag shows only at the next click.
    set_ro(false);
    assert_eq!(m.review_gate(), Err(reason.to_string()));
    m.export_from_review();
    idle(&mut m);
    assert!(m.export.summary.is_some());
}

#[test]
fn a_bad_location_is_the_reviews_reason() {
    let d = tempfile::tempdir().unwrap();
    let mut m = project(d.path());
    tick(&mut m, AlsoTarget::Vanilla);
    let root = m.root.clone().unwrap();
    m.set_also_location(&root);
    idle(&mut m);
    m.export_clicked();
    let why = m.review_gate().unwrap_err();
    assert!(why.contains("must not be the opened folder"), "{why}");
    // The MO2 layout (the location holds the opened folder) is fine.
    m.set_also_location(&d.path().join("mods"));
    idle(&mut m);
    m.export_from_review();
    idle(&mut m);
    assert!(m.export.summary.is_some(), "{}", m.status_line);
    assert!(d.path().join("mods/Mod Vanilla").is_dir());
}

#[test]
fn the_pbr_json_in_the_pbr_folder_is_ours_on_the_next_export() {
    let d = tempfile::tempdir().unwrap();
    let mut m = project(d.path());
    tick(&mut m, AlsoTarget::Vanilla);
    export(&mut m);
    let json = sdc_gui::model::pbr_export::prepare(&m).unwrap();
    assert!(
        json.path.starts_with(&m.also_folders()[0]),
        "{:?}",
        json.path
    );
    assert!(json.path.is_file());
    m.export_clicked();
    assert!(m.review_json().iter().all(|l| l.state != DiskState::Edited));
    assert!(!m.pgp_json_edited());
    m.acknowledge_all_asks();
    m.export_from_review();
    idle(&mut m);
    assert!(!matches!(m.dialog, Some(Dialog::OverwriteJson(_))));
}

#[test]
fn an_also_file_row_reveals_its_file_instead_of_selecting() {
    let d = tempfile::tempdir().unwrap();
    let mut m = project(d.path());
    tick(&mut m, AlsoTarget::Vanilla);
    export(&mut m);
    let key = "Mod Vanilla/textures/arm/steel.dds";
    assert_eq!(
        m.also_file(key),
        Some(d.path().join("out/Mod Vanilla/textures/arm/steel.dds"))
    );
    assert!(m.also_file(key).unwrap().is_file());
    assert_eq!(m.also_file("textures/pbr/arm/steel.dds"), None);
    // A warning on a Vanilla row is an ordinary File problem; a DERIVE-* Info never is a row.
    let ev = |w: Warning| EngineEvent {
        schema: EVENT_SCHEMA,
        seq: 0,
        ts: String::new(),
        batch: "b".into(),
        job: Some(key.into()),
        set: None,
        path: None,
        kind: EventKind::Warning(w),
    };
    m.export
        .on_event(ev(Warning::new(Code::OutUnknownExisting, "kept")));
    m.export
        .on_event(ev(Warning::new(Code::DeriveNoScan, "no scan")));
    m.invalidate_views();
    let rows = problems(&m);
    assert!(rows.iter().any(|p| p.at == At::File(key.into())));
    assert!(!rows.iter().any(|p| p.code == Some(Code::DeriveNoScan)));
}

#[test]
fn reviews_button_names_the_folders() {
    let d = tempfile::tempdir().unwrap();
    let mut m = project(d.path());
    let plain = m.review_button_text();
    assert!(!plain.contains("folders"), "{plain}");
    tick(&mut m, AlsoTarget::Vanilla);
    let n = m.export_files();
    assert!(m
        .review_button_text()
        .starts_with(&format!("Export {n} files to 2 folders")));
    tick(&mut m, AlsoTarget::Cm);
    assert!(m.review_button_text().contains(" to 3 folders"));
}

/// P8 (replaces "the progress never goes backwards"): one `drawer_epoch` for the three phases; every row done.
#[test]
fn one_epoch_for_the_whole_also_export() {
    let d = tempfile::tempdir().unwrap();
    let mut m = project(d.path());
    tick(&mut m, AlsoTarget::Cm);
    let e = m.drawer_epoch;
    export(&mut m);
    assert_eq!(m.drawer_epoch, e + 1);
    let (done, total) = m.export.progress();
    assert!(total > 0 && done == total);
}

#[test]
fn copy_cli_command_names_the_location_when_also_is_on() {
    let d = tempfile::tempdir().unwrap();
    let mut m = project(d.path());
    let loc = d.path().join("mods");
    m.set_also_location(&loc);
    tick(&mut m, AlsoTarget::Vanilla);
    let root = m.root.clone().unwrap();
    let slash = |p: &Path| p.to_string_lossy().replace('\\', "/");
    let c = cli_command(&root, &m.cli_output(), m.session_bc1(), 0);
    assert!(c.contains(&format!("-o \"{}\"", slash(&loc))), "{c}");
    assert!(!c.contains(&slash(&d.path().join("out"))), "{c}");
}

#[test]
fn renamed_folders_are_used_and_saved_in_export_names() {
    let d = tempfile::tempdir().unwrap();
    let mut m = project(d.path());
    tick(&mut m, AlsoTarget::Cm);
    m.set_also_settings(|c| {
        c.names = FolderNames {
            pbr: Some("A".into()),
            vanilla: Some("B".into()),
            cm: Some("C".into()),
        }
    });
    idle(&mut m);
    assert_eq!(m.undo_label(), Some("Also export settings"));
    export(&mut m);
    let out = d.path().join("out");
    for n in ["A", "B", "C"] {
        assert!(out.join(n).is_dir(), "{n}: {:?}", m.export.notes);
    }
    let cfg = engine::presets::ConfigFile::load(&sdc_toml(&m)).unwrap();
    assert_eq!(cfg.export.unwrap().names.vanilla.as_deref(), Some("B"));
}

#[test]
fn the_also_location_is_stored_per_project() {
    let d = tempfile::tempdir().unwrap();
    let mut m = project(d.path());
    let one = m.root.clone().unwrap();
    let l1 = d.path().join("l1");
    m.set_also_location(&l1);
    idle(&mut m);
    assert_eq!(m.also_location(), l1);
    m.save().unwrap();
    let two = d.path().join("two");
    png(&two.join("a.png"), [1, 2, 3, 255]);
    m.open_folder(two);
    idle(&mut m);
    assert_eq!(m.also_location(), m.export_root());
    m.open_folder(one.clone());
    idle(&mut m);
    assert_eq!(m.also_location(), l1);
    let text = std::fs::read_to_string(one.join("sdc-project.toml")).unwrap();
    assert!(text.contains("also_location"), "{text}");
}

#[test]
fn renaming_a_skipped_set_keeps_its_opt_out() {
    let d = tempfile::tempdir().unwrap();
    let mut m = project(d.path());
    let steel = steel_id(&m);
    m.set_skip_also(&steel, true);
    idle(&mut m);
    assert_eq!(m.also_config().skip, [steel.as_str()]);
    let iron = steel.replace("steel", "iron");
    m.rename_set(&steel, &iron).unwrap();
    idle(&mut m);
    assert_eq!(m.also_config().skip, [iron.as_str()]);
    m.undo();
    idle(&mut m);
    assert_eq!(m.also_config().skip, [steel], "one undo step");
}

/// P10 (TA24): Hold is Warn during an Also export, so no texture is held out of the folders and no half-size box
/// opens. The fixture has no PBR set, so there is no CM folder (TA30); the PBR and Vanilla folders hold everything.
#[test]
fn drop_top_mip_on_still_writes_every_texture_in_all_three_folders() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("Up/textures");
    std::fs::create_dir_all(&r).unwrap();
    let small = ImageBuffer::from_fn(512, 512, |x, y| {
        let v = ((x * 7919 + y * 104729) % 251) as u8;
        Rgb([v, v.wrapping_mul(3), v.wrapping_mul(7)])
    });
    ImageBuffer::from_fn(1024, 1024, |x, y| *small.get_pixel(x / 2, y / 2))
        .save(r.join("up.png"))
        .unwrap();
    std::fs::write(
        r.join("sdc.toml"),
        "[defaults.encoder]\neffort = \"ultrafast\"\n\n[files.\"up.png\"]\nsubtype = \"diffuse\"\n",
    )
    .unwrap();
    let settings = sdc_gui::settings::GuiSettings {
        export_folder: Some(d.path().join("out")),
        checks: sdc_gui::settings::ChecksSettings {
            drop_top_mip: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut m = AppModel::new(settings, None, support::session());
    m.open_folder(r);
    idle(&mut m);
    assert!(m.settings.checks.low_detail && m.settings.checks.drop_top_mip);
    tick(&mut m, AlsoTarget::Cm);
    export(&mut m);
    assert!(
        !matches!(m.dialog, Some(Dialog::HalfSize(_))),
        "{:?}",
        m.dialog
    );
    assert!(m.export.rows.iter().all(|r| !matches!(
        r.state,
        RowState::Skipped(engine::events::SkipKind::LowDetail, _)
    )));
    let [p, v, _] = m.also_folders();
    assert!(p.join("up.dds").is_file());
    assert!(v.join("up.dds").is_file());
}

#[test]
fn mesh_votes_are_asked_for_the_vanilla_key_of_a_pbr_colour_member() {
    let d = tempfile::tempdir().unwrap();
    let mut m = project(d.path());
    assert!(!m.mesh_vote_keys().contains("textures/arm/steel"));
    tick(&mut m, AlsoTarget::Vanilla);
    let keys = m.mesh_vote_keys();
    assert!(keys.contains("textures/arm/steel"), "{keys:?}");
    assert!(keys.contains("textures/pbr/arm/steel"), "{keys:?}");
}

/// P13: with a tick on, a complex `_m` set's CM JSON goes into the PBR folder (not "outside the export folder").
#[test]
fn a_cm_json_goes_into_the_pbr_folder_while_ticked() {
    let d = tempfile::tempdir().unwrap();
    let c = d.path().join("mods/Mod/textures/clutter");
    std::fs::create_dir_all(&c).unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Rgb([(x * 4) as u8, (y * 4) as u8, 90]))
        .save(c.join("iron.png"))
        .unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| {
        Rgba([(x * 4) as u8, (y * 4) as u8, 90, (40 + x * 2) as u8])
    })
    .save(c.join("iron_m.png"))
    .unwrap();
    let mut m = project(d.path());
    let iron = sdc_gui::model::sets::views(&m)
        .iter()
        .find(|v| v.key.ends_with("clutter/iron"))
        .expect("the iron set")
        .id
        .clone();
    m.set_cm_export(&iron, true);
    tick(&mut m, AlsoTarget::Vanilla);
    let ts = sdc_gui::model::cm_json::targets(&m);
    assert!(
        ts.iter().all(|t| t.rel.is_ok()),
        "{:?}",
        ts.iter().map(|t| &t.rel).collect::<Vec<_>>()
    );
    export(&mut m);
    assert!(
        m.also_folders()[0]
            .join("textures/clutter/iron_m.json")
            .is_file(),
        "{:?}",
        m.export.notes
    );
}

/// P15: an export folder inside the opened folder, ticked: the next plan finds no output there as a DDS input.
#[test]
fn a_location_inside_the_opened_folder_is_not_planned_as_inputs() {
    let d = tempfile::tempdir().unwrap();
    let mut m = project(d.path());
    let inside = m.root.clone().unwrap().join("out");
    m.set_also_location(&inside);
    tick(&mut m, AlsoTarget::Vanilla);
    assert_eq!(m.request().unwrap().skip_root, Some(inside.clone()));
    export(&mut m);
    assert!(inside.join("Mod Vanilla").is_dir());
    m.replan();
    idle(&mut m);
    assert!(
        m.items.iter().all(|i| !i.key.starts_with("out/")),
        "{:?}",
        m.items.iter().map(|i| &i.key).collect::<Vec<_>>()
    );
    export(&mut m);
    assert!(m.export.summary.as_ref().is_some_and(|s| s.failed == 0));
}

/// A tick is refused outside the cs mode (TA20), with the reason; nothing is edited.
#[test]
fn a_tick_is_refused_in_the_vanilla_mode() {
    let d = tempfile::tempdir().unwrap();
    let mut m = project(d.path());
    m.set_target(engine::config::Target::Vanilla);
    idle(&mut m);
    let before = m.project.as_ref().unwrap().text();
    m.set_also_tick(AlsoTarget::Vanilla, true);
    assert!(!m.also_on());
    assert_eq!(m.project.as_ref().unwrap().text(), before);
    assert_eq!(m.status_line, sdc_gui::model::also::NEEDS_CS);
}

/// Every `[export]` field set at once survives the write and the parse (prune, tables after values), as an unsaved
/// edit; only the non-default fields are written.
#[test]
fn every_export_field_round_trips_through_sdc_toml() {
    let d = tempfile::tempdir().unwrap();
    let mut m = project(d.path());
    tick(&mut m, AlsoTarget::Vanilla);
    let steel = steel_id(&m);
    m.set_also_settings(|c| {
        c.also.push(AlsoTarget::Cm);
        c.zip = true;
        c.skip = vec![steel.clone()];
        c.names.vanilla = Some("Plain".into());
        c.legacy.brightness = 0.8;
        c.legacy.keep_metal_brightness = true;
    });
    idle(&mut m);
    let p = m.project.as_ref().unwrap();
    assert!(p.is_dirty());
    let text = p.text();
    let parsed = engine::presets::ConfigFile::parse(&text, "sdc.toml")
        .unwrap()
        .export
        .unwrap();
    assert_eq!(parsed, m.also_config(), "{text}");
    assert_eq!(parsed.also, [AlsoTarget::Vanilla, AlsoTarget::Cm]);
    assert!(parsed.zip && parsed.legacy.brightness == 0.8 && parsed.legacy.keep_metal_brightness);
    assert_eq!(parsed.names.vanilla.as_deref(), Some("Plain"));
    assert_eq!(parsed.skip, [steel.as_str()]);
    assert!(
        !text.contains("ao_strength"),
        "defaults are not written: {text}"
    );
}
