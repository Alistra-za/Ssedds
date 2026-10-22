use sdc_gui::settings::*;

#[test]
fn defaults_follow_the_spec() {
    let s = GuiSettings::default();
    assert_eq!(s.preview_max, 4096);
    assert!(!s.live_encode, "preview encoding is off by default");
    assert!(!s.allow_16k_preview);
    assert_eq!(s.bc1_decoder, Bc1Choice::Ideal);
    assert!(!s.reduce_motion, "motion is on by default");
}

#[test]
fn reduce_motion_round_trips_in_gui_toml() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("sdc/gui.toml");
    let s = GuiSettings {
        reduce_motion: true,
        ..Default::default()
    };
    s.save(&p).unwrap();
    assert_eq!(GuiSettings::load(&p).0, s);
    assert!(std::fs::read_to_string(&p)
        .unwrap()
        .contains("reduce_motion = true"));
}

#[test]
fn settings_round_trip_and_bad_files_fall_back_to_defaults() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("sdc/gui.toml");
    let (s, err) = GuiSettings::load(&p);
    assert_eq!((s, err), (GuiSettings::default(), None));
    let mut s = GuiSettings {
        live_encode: true,
        preview_max: 2048,
        ..GuiSettings::default()
    };
    s.push_recent("C:/a".into());
    s.push_recent("C:/b".into());
    s.push_recent("C:/a".into());
    assert_eq!(
        s.recent,
        vec![std::path::PathBuf::from("C:/a"), "C:/b".into()]
    );
    s.save(&p).unwrap();
    assert_eq!(GuiSettings::load(&p).0, s);
    std::fs::write(&p, "preview_max = \"big\"").unwrap();
    let (s, err) = GuiSettings::load(&p);
    assert_eq!(s, GuiSettings::default());
    assert!(err.is_some());
}

#[test]
fn recent_keeps_ten() {
    let mut s = GuiSettings::default();
    for i in 0..12 {
        s.push_recent(format!("C:/f{i}").into());
    }
    assert_eq!(s.recent.len(), 10);
    assert_eq!(s.recent[0], std::path::PathBuf::from("C:/f11"));
}

#[test]
fn the_bc1_choice_maps_back_from_the_sessions_model() {
    for c in Bc1Choice::ALL {
        assert_eq!(Bc1Choice::from_model(c.model()), c);
    }
}

#[test]
fn a_preview_size_that_is_not_offered_loads_as_4096() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("gui.toml");
    std::fs::write(&p, "preview_max = 1000\nlive_encode = true\n").unwrap();
    let (s, err) = GuiSettings::load(&p);
    assert_eq!(err, None);
    assert_eq!(s.preview_max, 4096);
    assert!(s.live_encode, "the rest of the file is kept");
    std::fs::write(&p, "preview_max = 8192\n").unwrap();
    assert_eq!(GuiSettings::load(&p).0.preview_max, 8192);
}

#[test]
fn saving_is_atomic_and_keeps_an_unreadable_file_as_gui_toml_bad() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("sdc/gui.toml");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, "preview_max = \"big\"\n# my hand-written notes\n").unwrap();
    let (s, err) = GuiSettings::load(&p);
    assert!(err.is_some());
    s.save(&p).unwrap();
    let bad = d.path().join("sdc/gui.toml.bad");
    assert_eq!(
        std::fs::read_to_string(&bad).unwrap(),
        "preview_max = \"big\"\n# my hand-written notes\n"
    );
    assert_eq!(GuiSettings::load(&p), (s.clone(), None));
    // Once: the next save finds a readable file and leaves the kept copy alone.
    let s2 = GuiSettings {
        live_encode: true,
        ..s
    };
    s2.save(&p).unwrap();
    assert_eq!(GuiSettings::load(&p).0, s2);
    assert!(std::fs::read_to_string(&bad)
        .unwrap()
        .contains("hand-written"));
    let leftovers: Vec<_> = std::fs::read_dir(p.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(leftovers.len(), 2, "no temporary file stays: {leftovers:?}");
}

mod support;

/// Audit 2026-10-15 item 15: a field edited by hand while the app runs survives the app's next save; a field the
/// app changed since its last write wins.
#[test]
fn a_hand_edit_made_while_running_survives_the_next_save() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("sdc/gui.toml");
    GuiSettings::default().save(&p).unwrap();
    let (s, _) = GuiSettings::load(&p);
    let mut m = sdc_gui::model::AppModel::new(s, Some(p.clone()), support::session());
    let mut hand = GuiSettings::load(&p).0;
    hand.jobs = 12;
    hand.mem_budget_mb = 4096;
    hand.save(&p).unwrap();
    m.settings.live_encode = true;
    m.settings.mem_budget_mb = 512;
    m.save_settings();
    let back = GuiSettings::load(&p).0;
    assert_eq!(back.jobs, 12, "the hand edit is kept");
    assert!(back.live_encode, "the app's change is written");
    assert_eq!(back.mem_budget_mb, 512, "both changed: the app wins");
    assert_eq!(m.settings, back, "the app now holds what it wrote");
}

/// Audit 2026-10-15 item 9: a failed gui.toml write is a warning toast, once per distinct error.
#[test]
fn a_failed_settings_save_warns_once() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("file"), "x").unwrap();
    let mut m = sdc_gui::model::AppModel::placeholder(GuiSettings::default());
    m.settings_path = Some(d.path().join("file/gui.toml"));
    m.save_settings();
    m.save_settings();
    let n = m
        .messages
        .entries()
        .filter(|e| e.text.starts_with("gui.toml"))
        .count();
    assert_eq!(n, 1);
}

fn running(p: &std::path::Path, s: GuiSettings) -> sdc_gui::model::AppModel {
    s.save(p).unwrap();
    let (s, _) = GuiSettings::load(p);
    sdc_gui::model::AppModel::new(s, Some(p.to_path_buf()), support::session())
}

/// Item 15: a key removed by hand while the app runs stays removed (an optional field becomes unset).
#[test]
fn a_key_removed_by_hand_while_running_stays_removed() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("sdc/gui.toml");
    let mut m = running(
        &p,
        GuiSettings {
            export_folder: Some(d.path().join("out")),
            ..GuiSettings::default()
        },
    );
    let text = std::fs::read_to_string(&p).unwrap();
    let edited: String = text
        .lines()
        .filter(|l| !l.starts_with("export_folder"))
        .map(|l| format!("{l}\n"))
        .collect();
    assert_ne!(edited, text);
    std::fs::write(&p, edited).unwrap();
    m.settings.live_encode = true;
    m.save_settings();
    let back = GuiSettings::load(&p).0;
    assert_eq!(back.export_folder, None);
    assert!(back.live_encode);
    assert_eq!(m.settings.export_folder, None);
}

/// Item 15 and review minor 6: a gui.toml broken by hand while the app runs is kept as `.bad` (with a warning) and
/// the app's settings are written.
#[test]
fn a_file_broken_while_running_is_kept_and_warned_about() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("sdc/gui.toml");
    let mut m = running(&p, GuiSettings::default());
    std::fs::write(&p, "jobs = [broken").unwrap();
    m.settings.jobs = 3;
    m.save_settings();
    assert_eq!(GuiSettings::load(&p), (m.settings.clone(), None));
    assert_eq!(GuiSettings::load(&p).0.jobs, 3);
    assert_eq!(
        std::fs::read_to_string(p.with_extension("toml.bad")).unwrap(),
        "jobs = [broken"
    );
    assert!(m
        .messages
        .entries()
        .any(|e| e.text.starts_with("gui.toml: it could not be read")));
}

/// Review minor 2: Recent merges per folder: the hand edit's removal and addition, and the app's addition.
#[test]
fn recent_folders_merge_per_folder() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("sdc/gui.toml");
    let f = |n: &str| std::path::PathBuf::from(format!("C:/m/{n}"));
    let mut m = running(
        &p,
        GuiSettings {
            recent: vec![f("a"), f("b")],
            ..GuiSettings::default()
        },
    );
    let mut hand = GuiSettings::load(&p).0;
    hand.recent = vec![f("a"), f("c")];
    hand.save(&p).unwrap();
    m.settings.push_recent(f("d"));
    m.save_settings();
    assert_eq!(GuiSettings::load(&p).0.recent, [f("d"), f("a"), f("c")]);
}

/// Trace-debug 2026-10-16 #45: a gui.toml that `load` could not read (not UTF-8) is kept as `.bad` by the first
/// save, as one that does not parse is.
#[test]
fn saving_keeps_a_non_utf8_gui_toml_as_bad() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("sdc/gui.toml");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    let bytes = b"jobs = 12\n# caf\xe9\n".to_vec();
    std::fs::write(&p, &bytes).unwrap();
    let (s, err) = GuiSettings::load(&p);
    assert!(err.is_some());
    let kept = s.save(&p).unwrap();
    assert_eq!(kept, Some(p.with_extension("toml.bad")));
    assert_eq!(std::fs::read(p.with_extension("toml.bad")).unwrap(), bytes);
}

/// #45: a gui.toml locked at launch (load failed, defaults used) is merged by the first save, not replaced.
#[cfg(windows)]
#[test]
fn a_gui_toml_locked_at_launch_is_merged_by_the_first_save() {
    use std::os::windows::fs::OpenOptionsExt;
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("sdc/gui.toml");
    GuiSettings {
        jobs: 12,
        ..GuiSettings::default()
    }
    .save(&p)
    .unwrap();
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&p)
        .unwrap();
    let (s, err) = GuiSettings::load(&p);
    assert!(err.is_some(), "the lock makes load fail");
    let mut m = sdc_gui::model::AppModel::new(s, Some(p.clone()), support::session());
    drop(lock);
    m.settings.live_encode = true;
    m.save_settings();
    let back = GuiSettings::load(&p).0;
    assert_eq!(back.jobs, 12, "the file's value is kept");
    assert!(back.live_encode, "the app's change is written");
}

/// Review M3 (#45, Note BR): when the broken gui.toml cannot be moved aside (held open by another program), the
/// earlier `gui.toml.bad` is not deleted; the save fails and both files stay.
#[cfg(windows)]
#[test]
fn a_locked_broken_gui_toml_keeps_the_earlier_bad_copy() {
    use std::os::windows::fs::OpenOptionsExt;
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("gui.toml");
    let bad = p.with_extension("toml.bad");
    std::fs::write(&p, "preview_max = [").unwrap();
    std::fs::write(&bad, "earlier hand edit").unwrap();
    // FILE_SHARE_READ only: readable, but it cannot be renamed while this handle is open.
    let hold = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&p)
        .unwrap();
    assert!(GuiSettings::default().save(&p).is_err());
    drop(hold);
    assert_eq!(std::fs::read_to_string(&bad).unwrap(), "earlier hand edit");
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "preview_max = [");
    // Unlocked, the next save replaces the earlier copy.
    assert_eq!(GuiSettings::default().save(&p).unwrap(), Some(bad.clone()));
    assert_eq!(std::fs::read_to_string(&bad).unwrap(), "preview_max = [");
}

/// Trace #31: a folder name that is not valid Unicode (a lone surrogate) cannot be written to TOML. It is left out of
/// the file; the rest of the settings still save.
#[cfg(windows)]
#[test]
fn a_non_unicode_path_does_not_stop_the_settings_saving() {
    use std::os::windows::ffi::OsStringExt;
    let bad = std::path::PathBuf::from(std::ffi::OsString::from_wide(&[0x43, 0x3a, 0x5c, 0xD800]));
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("sdc/gui.toml");
    let mut s = GuiSettings {
        live_encode: true,
        nvtt_path: Some(bad.clone()),
        ..Default::default()
    };
    s.meshes.folder = Some(bad.clone());
    s.push_recent("C:/a".into());
    s.push_recent(bad);
    s.save(&p).unwrap();
    let (back, err) = GuiSettings::load(&p);
    assert_eq!(err, None);
    assert!(back.live_encode);
    assert_eq!(back.recent, [std::path::PathBuf::from("C:/a")]);
    assert!(back.nvtt_path.is_none() && back.meshes.folder.is_none());
}
