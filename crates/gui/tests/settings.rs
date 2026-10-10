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
