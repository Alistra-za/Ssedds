use sdc_gui::settings::*;

#[test]
fn defaults_follow_the_spec() {
    let s = GuiSettings::default();
    assert_eq!(s.preview_max, 4096);
    assert!(!s.live_encode, "preview encoding is off by default");
    assert!(!s.allow_16k_preview);
    assert_eq!(s.bc1_decoder, Bc1Choice::Ideal);
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
